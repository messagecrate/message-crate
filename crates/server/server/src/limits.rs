//! The body caps the server holds every request to. Fixed in the code, so the
//! routes, the layers and the problem types' pages all read one figure.

/// The body cap of the routes a stranger may call (`limited_auth_router` in
/// [`crate::server`]): 32 KiB, so password hashing cannot be fed a large body.
pub(crate) const MAX_AUTH_BODY_BYTES: usize = 32 * 1024;

/// The cap on a JSON body, the one `crate::extract::Json` reads: 32 MiB. It
/// is sized for the largest body the web app sends, the completion of an
/// Import Run (`POST /v1/imports/{id}/complete`), which carries every issue
/// of the run; at a few hundred bytes an issue, that is about a hundred
/// thousand issues. Without it a JSON body is held to Axum's own 2 MiB
/// default, a figure nobody chose.
pub(crate) const MAX_JSON_BODY_BYTES: usize = 32 * 1024 * 1024;

/// The body cap of every route but the attachment uploads: 512 MiB, the
/// attachment size limit a new Message Crate starts with. It is fixed in the
/// code because the owner's limit must never reach the login or the settings
/// change that would raise it again. Routes that read a body of their own
/// hold it to a smaller figure first.
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024 * 1024;
