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
//! A task that stops waiting, because its `lock` future is dropped, as an
//! import batch's is when its client goes, can leave its key behind when
//! the holder left first and saw it waiting. The next lock of that key takes
//! the entry over and removes it as it leaves, so at most one such entry
//! stays per key.

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
    /// How many tasks found each key's mutex held and started to
    /// wait, so a test can act once a task waits on a key rather than after
    /// a sleep.
    #[cfg(test)]
    waiting: Arc<tokio::sync::watch::Sender<HashMap<String, usize>>>,
    /// How many tasks that found each key's mutex held have since taken
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
                #[cfg(test)]
                self.waiting
                    .send_modify(|keys| *keys.entry(key.clone()).or_default() += 1);
                let guard = mutex.lock_owned().await;
                #[cfg(test)]
                self.taken_after_waiting
                    .send_modify(|keys| *keys.entry(key.clone()).or_default() += 1);
                guard
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

    /// Wait until a task has found the mutex for `key` held and is waiting
    /// for it. Each task that waited is counted once, so the next call waits
    /// for another.
    #[cfg(test)]
    pub(crate) async fn a_task_waits(&self, key: &str) {
        take_one_count(&self.waiting, key).await;
    }

    /// Wait until a task that found the mutex for `key` held has taken it.
    /// Each such task is counted once, so the next call waits for another.
    #[cfg(test)]
    pub(crate) async fn a_waiting_task_takes(&self, key: &str) {
        take_one_count(&self.taken_after_waiting, key).await;
    }

    /// How many tasks that found the mutex for `key` held have taken it
    /// since [`Self::a_waiting_task_takes`] last counted one.
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
