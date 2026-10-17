//! The HTTP client a test calls its own server with, shared with the tests
//! under `tests/` through `tests/common`.

/// An HTTP client for a test's calls to its own server. It loads no root
/// certificates: every call is plain HTTP to 127.0.0.1, and reading the
/// system's certificate store costs about 23 ms a client. The OpenAPI walks
/// in `openapi/` make about a thousand requests.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .tls_built_in_root_certs(false)
        .build()
        .expect("build the test HTTP client")
}
