//! The shared client trusts the operating system's certificate store, as the
//! desktop app's webview does (#1803).
//!
//! A test cannot add a root to the real system store, so it stands one up the
//! way `rustls-native-certs` reads it on every platform: `SSL_CERT_FILE` names
//! a PEM file, and that file then stands in for the system store. The file
//! holds a private CA, as mkcert or Caddy's internal CA would install one.
//! Each test serves HTTPS on a loopback port and fetches it with
//! [`message_crate_http::build_client`], the client every caller uses.
//!
//! This file is its own test binary, so the variable reaches no other test.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// The private CA the stand-in system store trusts.
struct TrustedCa {
    issuer: CertifiedIssuer<'static, KeyPair>,
}

/// Make the private CA, write it to the file `SSL_CERT_FILE` names, and set
/// the variable, once for the whole binary and before any client is built.
fn trusted_ca() -> &'static TrustedCa {
    static CA: OnceLock<TrustedCa> = OnceLock::new();
    CA.get_or_init(|| {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "Message Crate test private CA");
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let issuer = CertifiedIssuer::self_signed(params, KeyPair::generate().expect("CA key"))
            .expect("self-sign the CA");

        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("os-trust-private-ca.pem");
        std::fs::write(&path, issuer.pem()).expect("write the CA file");
        // SAFETY: this runs inside `OnceLock::get_or_init`, which every test
        // calls first, before it starts a thread or builds a client, so no other
        // thread of this binary reads the environment while it is written.
        unsafe { std::env::set_var("SSL_CERT_FILE", &path) };
        TrustedCa { issuer }
    })
}

/// Serve one HTTPS request on a loopback port with `chain` and `key`, and
/// return the URL to fetch.
fn serve_once(chain: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) -> String {
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("TLS versions")
    .with_no_client_auth()
    .with_single_cert(chain, key)
    .expect("server certificate");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local address").port();
    std::thread::spawn(move || {
        let Ok((tcp, _)) = listener.accept() else {
            return;
        };
        let conn = rustls::ServerConnection::new(Arc::new(config)).expect("TLS connection");
        let mut tls = rustls::StreamOwned::new(conn, tcp);
        // A refused handshake ends here with an error, which is the point of
        // the refusal test; the thread just stops.
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match tls.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => request.extend_from_slice(&buf[..n]),
            }
        }
        let _ =
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        tls.conn.send_close_notify();
        let _ = tls.flush();
    });
    format!("https://127.0.0.1:{port}/")
}

fn key_der(key: &KeyPair) -> PrivateKeyDer<'static> {
    PrivatePkcs8KeyDer::from(key.serialize_der()).into()
}

#[test]
fn a_server_signed_by_a_ca_the_system_trusts_is_accepted() {
    let ca = trusted_ca();
    let leaf_key = KeyPair::generate().expect("leaf key");
    let leaf = CertificateParams::new(vec!["127.0.0.1".to_string()])
        .expect("leaf params")
        .signed_by(&leaf_key, &ca.issuer)
        .expect("sign the leaf");
    let url = serve_once(vec![leaf.der().clone()], key_der(&leaf_key));

    let client = message_crate_http::build_client().expect("build the client");
    let body = client
        .get(&url)
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.text())
        .expect("a certificate from a CA the system trusts is accepted");
    assert_eq!(body, "ok");
}

#[test]
fn a_self_signed_certificate_nobody_trusts_is_refused() {
    trusted_ca();
    let key = KeyPair::generate().expect("key");
    let cert = CertificateParams::new(vec!["127.0.0.1".to_string()])
        .expect("params")
        .self_signed(&key)
        .expect("self-sign");
    let url = serve_once(vec![cert.der().clone()], key_der(&key));

    let client = message_crate_http::build_client().expect("build the client");
    let error = client
        .get(&url)
        .send()
        .expect_err("a certificate neither the system nor the bundled roots trust is refused");
    assert!(
        error.is_connect(),
        "expected a TLS connect error, got {error:?}"
    );
    let chain = format!("{error:?}");
    assert!(
        chain.contains("UnknownIssuer"),
        "expected the refusal to name UnknownIssuer, got {chain}"
    );
}
