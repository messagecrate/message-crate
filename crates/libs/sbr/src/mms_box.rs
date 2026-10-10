//! The `msg_box` values of an `<mms>` element that the reader treats apart
//! from a received message: the box of a sent message, and the boxes of a
//! message that was never sent, which the reader skips.

/// A message the owner sent.
pub(crate) const SENT: &str = "2";
/// A draft the owner had not sent.
pub(crate) const DRAFT: &str = "3";
/// A message waiting in the outbox.
pub(crate) const OUTBOX: &str = "4";
/// A message the phone failed to send.
pub(crate) const FAILED: &str = "5";
/// A message queued to be sent.
pub(crate) const QUEUED: &str = "6";
