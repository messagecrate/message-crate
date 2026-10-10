//! The `msg_box` values of an `<mms>` element: which box on the phone the
//! message was in.

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
