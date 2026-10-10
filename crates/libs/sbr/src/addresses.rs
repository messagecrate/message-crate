//! MMS addresses: each as a [`Handle`], the participants and peers of a
//! message, its sender, and which addresses are the owner's.

use phone::{Handle, OwnerHandleSet};
use std::collections::{BTreeMap, HashMap};

use crate::xml::{btree, get};

const INSERT_ADDRESS_TOKEN: &str = "insert-address-token";
pub(crate) const MMS_ADDR_FROM: &str = "137";

/// Raw `<addr>` element: the address, its type, and the full attribute map.
#[derive(Debug, Clone, Default)]
pub(crate) struct MmsAddr {
    /// The address as written, from the `address` attribute.
    pub(crate) address: String,
    /// The address's role from the `type` attribute, such as `137` for `FROM`.
    pub(crate) addr_type: String,
    /// All raw attributes.
    pub(crate) attrs: BTreeMap<String, String>,
}

/// An MMS address from an `<addr>` element's attributes.
pub(crate) fn addr(attrs: &HashMap<String, String>) -> MmsAddr {
    MmsAddr {
        address: get(attrs, "address").into(),
        addr_type: get(attrs, "type").into(),
        attrs: btree(attrs),
    }
}

/// An address as a [`Handle`], classified from the value as written. `None`
/// for a blank value and for the placeholder the phone writes where its own
/// number goes. The writer counts a group's participants with it, so that it
/// counts them as this reader does.
pub fn address_handle(raw: &str) -> Option<Handle> {
    if raw.trim().eq_ignore_ascii_case(INSERT_ADDRESS_TOKEN) {
        return None;
    }
    Handle::parse(raw)
}

/// Every address on the element: the `~`-joined `address` attribute, then
/// each `<addr>` child. Blank entries are dropped; owners are not.
pub(crate) fn mms_participants(attrs: &HashMap<String, String>, addrs: &[MmsAddr]) -> Vec<Handle> {
    get(attrs, "address")
        .split('~')
        .chain(addrs.iter().map(|a| a.address.as_str()))
        .filter_map(address_handle)
        .collect()
}

/// The sender of an incoming MMS: the `FROM` (`type="137"`) address when it
/// is an address other than the owner's; without one, the peer of a direct
/// conversation, since nobody else could have sent it; in a group, nobody.
///
/// A group message without a `FROM` is left without a sender rather than
/// credited to a guess. SMS Backup & Restore writes exactly one `FROM` on
/// every MMS (6,464 of 6,464 in the 2021 reference backup), and where the
/// sender sits in the `address` list is arbitrary: in that backup it is the
/// first entry on 1,192 of 5,367 received group MMS, so "the first peer"
/// would be wrong four times out of five.
pub(crate) fn mms_sender(
    addrs: &[MmsAddr],
    peers: &[Handle],
    owners: Option<&OwnerHandleSet>,
) -> Option<Handle> {
    addrs
        .iter()
        .find(|a| a.addr_type == MMS_ADDR_FROM)
        .and_then(|a| address_handle(&a.address))
        .filter(|a| !is_owner(owners, a))
        .or_else(|| match peers {
            [peer] => Some(peer.clone()),
            _ => None,
        })
}

/// The other parties: every participant that is not the owner, sorted by
/// key and de-duplicated so the same group always gets the same key.
pub(crate) fn mms_peers(participants: Vec<Handle>, owners: Option<&OwnerHandleSet>) -> Vec<Handle> {
    let mut peers: Vec<Handle> = participants
        .into_iter()
        .filter(|p| !is_owner(owners, p))
        .collect();
    peers.sort_by(|a, b| a.key().cmp(b.key()));
    peers.dedup_by(|a, b| a.key() == b.key());
    peers
}

/// Whether an address is one of the owner's; with no owner given, none is.
fn is_owner(owners: Option<&OwnerHandleSet>, address: &Handle) -> bool {
    owners.is_some_and(|o| o.is_owner(address))
}
