//! One async mutex per key, created on demand and forgotten when the last
//! holder leaves.
//!
//! Two request paths need "at most one of these per account" or "per
//! (account, asset)": the importer, so same-account imports do not wipe each
//! other's staging rows, and multipart upload completion, so two clients
//! cannot race `store_verified` on one SHA-256. A plain map of `Arc<Mutex<()>>`
//! does the job but never shrinks: one entry per asset ever completed, for the
//! life of the process. [`KeyedLocks`] shrinks it again. The guard removes its
//! key on drop unless another task is already waiting on the same mutex.
//!
//! A task stops waiting when its `lock` future is dropped. An import
//! batch's future is dropped when its client goes. If the holder left first
//! and saw that task waiting, the key stays in the map with nobody holding
//! it. The next lock of that key takes the entry over and removes it as it
//! leaves. So at most one such entry stays per key.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};

use tokio::sync::{Mutex, OwnedMutexGuard};

type LockMap = HashMap<String, Arc<Mutex<()>>>;

/// Per-key async mutexes. Cloning shares the same map.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyedLocks {
    /// A `std` mutex: it is only ever held for one map operation, never
    /// across an `.await`, and `Drop` cannot await.
    map: Arc<StdMutex<LockMap>>,
    /// How many tasks were queued for each key's mutex, so a test can act
    /// once a task waits on a key rather than after a sleep.
    #[cfg(test)]
    waiting: Arc<tokio::sync::watch::Sender<HashMap<String, usize>>>,
    /// How many tasks that were queued for each key's mutex have since taken
    /// it, so a test can act once a waiting task holds a key rather than
    /// after a sleep.
    #[cfg(test)]
    taken_after_waiting: Arc<tokio::sync::watch::Sender<HashMap<String, usize>>>,
}

/// Holds the key's mutex until dropped.
#[derive(Debug)]
pub(crate) struct KeyedLockGuard {
    guard: OwnedMutexGuard<()>,
    key: String,
    locks: KeyedLocks,
}

impl KeyedLocks {
    /// Wait for and take the mutex for `key`.
    pub(crate) async fn lock(&self, key: String) -> KeyedLockGuard {
        let mutex = self.map_guard().entry(key.clone()).or_default().clone();
        // A free mutex is taken at once. A held one is waited for: tokio's
        // mutex hands it to its waiters in turn, and `try_lock_owned` fails
        // while any waits, so trying first never overtakes one.
        let guard = match Arc::clone(&mutex).try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => {
                let taking = mutex.lock_owned();
                #[cfg(test)]
                let taking = self.counted_wait(&key, taking);
                taking.await
            }
        };
        KeyedLockGuard {
            guard,
            key,
            locks: self.clone(),
        }
    }

    /// Take the map, poisoned or not. A panic while it is held cannot leave
    /// it half-changed: each holder does one `entry`, `remove` or `len`. So a
    /// poisoned map is used as it is, rather than failing every later import
    /// and upload completion until the server restarts, or aborting the
    /// process when a guard drops during an unwind.
    fn map_guard(&self) -> MutexGuard<'_, LockMap> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wait on `taking`, the mutex for `key`, and count the wait for
    /// [`Self::a_task_waits`] and [`Self::a_waiting_task_takes`].
    ///
    /// The task counts as waiting only once tokio's mutex has queued it,
    /// which happens in the first poll of `taking` that returns pending.
    /// `taking` runs outside tokio's per-task budget, so a pending poll
    /// always means the mutex queued the task, never that the budget ran
    /// out. A queued task is handed the mutex in turn, and gives it back
    /// unused if it stops waiting first. The count must not come before
    /// that poll: a test that frees the mutex on the count would otherwise
    /// let the task take the free mutex without ever queuing (#3106). A
    /// task whose first poll takes the mutex never waited, so it is not
    /// counted at all.
    #[cfg(test)]
    async fn counted_wait(
        &self,
        key: &str,
        taking: impl std::future::Future<Output = OwnedMutexGuard<()>>,
    ) -> OwnedMutexGuard<()> {
        let mut taking = std::pin::pin!(tokio::task::unconstrained(taking));
        let mut queued = false;
        let guard = std::future::poll_fn(|cx| {
            let poll = taking.as_mut().poll(cx);
            if poll.is_pending() && !queued {
                queued = true;
                self.waiting
                    .send_modify(|keys| *keys.entry(key.to_owned()).or_default() += 1);
            }
            poll
        })
        .await;
        if queued {
            self.taken_after_waiting
                .send_modify(|keys| *keys.entry(key.to_owned()).or_default() += 1);
        }
        guard
    }

    /// Wait until a task has been queued for the mutex for `key` and is
    /// waiting for it. Each task that waited is counted once, so the next
    /// call waits for another.
    #[cfg(test)]
    pub(crate) async fn a_task_waits(&self, key: &str) {
        take_one_count(&self.waiting, key).await;
    }

    /// Wait until a task that was queued for the mutex for `key` has taken
    /// it. Each such task is counted once, so the next call waits for
    /// another.
    #[cfg(test)]
    pub(crate) async fn a_waiting_task_takes(&self, key: &str) {
        take_one_count(&self.taken_after_waiting, key).await;
    }

    /// How many tasks that were queued for the mutex for `key` have taken
    /// it since [`Self::a_waiting_task_takes`] last counted one.
    #[cfg(test)]
    pub(crate) fn waiting_tasks_that_took(&self, key: &str) -> usize {
        self.taken_after_waiting
            .borrow()
            .get(key)
            .copied()
            .unwrap_or(0)
    }

    /// Number of keys currently known. Tests use it to show the map shrinks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map_guard().len()
    }
}

/// Wait until `counts` holds `key`, then count one off it.
#[cfg(test)]
async fn take_one_count(counts: &tokio::sync::watch::Sender<HashMap<String, usize>>, key: &str) {
    let mut seen = counts.subscribe();
    // The sender lives in the `KeyedLocks`, which outlives the wait, so the
    // wait cannot see it dropped.
    let _ = seen.wait_for(|keys| keys.contains_key(key)).await;
    counts.send_modify(|keys| {
        if let Some(count) = keys.get_mut(key) {
            *count -= 1;
            if *count == 0 {
                keys.remove(key);
            }
        }
    });
}

impl Drop for KeyedLockGuard {
    fn drop(&mut self) {
        let mut map = self.locks.map_guard();
        // Two holders is the map's entry plus this guard. A task waiting on
        // the same key cloned the `Arc` before calling `lock_owned`, so it
        // shows as a third and the entry stays for it.
        if Arc::strong_count(OwnedMutexGuard::mutex(&self.guard)) == 2 {
            map.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[tokio::test]
    async fn entry_is_removed_when_the_last_holder_leaves() {
        let locks = KeyedLocks::default();
        {
            let _guard = locks.lock("a:1".into()).await;
            assert_eq!(locks.len(), 1);
        }
        assert_eq!(locks.len(), 0);
    }

    #[tokio::test]
    async fn waiters_serialize_and_the_entry_survives_until_the_last_one() {
        let locks = KeyedLocks::default();
        let first = locks.lock("a:1".into()).await;
        let waiter = {
            let locks = locks.clone();
            crate::request_id::spawn_in_current_span(async move {
                let _guard = locks.lock("a:1".into()).await;
                locks.len()
            })
        };
        locks.a_task_waits("a:1").await;
        assert_eq!(locks.len(), 1);
        drop(first);
        // The waiter held the entry, so it was still there when it ran.
        assert_eq!(waiter.await.unwrap(), 1);
        assert_eq!(locks.len(), 0);
    }

    /// Poison the map's `std` mutex by panicking on another thread while
    /// holding it.
    fn poison(locks: &KeyedLocks) {
        let map = Arc::clone(&locks.map);
        let panicked = std::thread::spawn(move || {
            let _held = map.lock().unwrap();
            panic!("poisoning the keyed lock map on purpose");
        })
        .join();
        assert!(panicked.is_err());
        assert!(locks.map.is_poisoned());
    }

    #[tokio::test]
    async fn a_poisoned_map_still_locks_and_releases() {
        let locks = KeyedLocks::default();
        // Taken before the poisoning, so its drop runs against a poisoned map.
        let before = locks.lock("a:1".into()).await;
        poison(&locks);
        drop(before);
        assert_eq!(locks.len(), 0);
        {
            let _after = locks.lock("a:2".into()).await;
            assert_eq!(locks.len(), 1);
        }
        assert_eq!(locks.len(), 0);
    }

    #[tokio::test]
    async fn different_keys_do_not_block_each_other() {
        let locks = KeyedLocks::default();
        let _a = locks.lock("a".into()).await;
        let _b = locks
            .lock("b".into())
            .now_or_never()
            .expect("lock on a different key must not wait");
        assert_eq!(locks.len(), 2);
    }
}
