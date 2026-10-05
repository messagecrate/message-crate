//! Saving an attachment's original from the server straight to a file.
//!
//! The window could fetch the original and hand its bytes to
//! [`super::paths::save_file`], but then a video of hundreds of megabytes is
//! held whole in the window, again as the bytes it sends, and again in the
//! request the app receives. Here the window sends only a Media Link for the
//! original, and the app copies the server's answer to the file a buffer at a
//! time, so memory stays flat whatever the file's size.

use std::io::Write;
use std::path::Path;

use tauri::{AppHandle, Url};

use super::paths::choose_save_path;

/// Show the Save dialog with `file_name` filled in, then download the
/// attachment's original that `url` names to the file the person chose,
/// replacing a file already there.
///
/// `url` is a Media Link's `url`: `…/v1/assets/{sha256}?media_link=…`, which
/// reads one original with no `Authorization` header. The bytes go from the
/// server to the disk and never through the window. The path comes from the
/// dialog, never from the window, so a script in the window cannot name a
/// file for the app to overwrite. The URL must have the shape of a Media Link
/// to an original, which keeps the command to that one use. Its host is not
/// checked, because the app does not know which server the window logged in
/// to: a script in the window can point the app at another address, and the
/// answer still goes only to the file the person chose.
///
/// Returns `false` when the person closed the dialog without choosing a
/// place, and `true` once the file is written.
///
/// # Errors
///
/// Returns an error when `url` is not an original read by a Media Link, when
/// `file_name` is empty (from [`choose_save_path`]), when the server refuses the download or it breaks
/// off, or when the file cannot be written.
#[tauri::command]
pub async fn save_download(app: AppHandle, url: String, file_name: String) -> Result<bool, String> {
    let url = original_by_media_link(&url)?;
    let Some(path) = choose_save_path(&app, &file_name).await? else {
        return Ok(false);
    };
    tauri::async_runtime::spawn_blocking(move || download_to_file(&url, &path))
        .await
        .map_err(|e| e.to_string())??;
    Ok(true)
}

/// `raw` as a URL, when it reads an attachment's original by a Media Link:
/// HTTP or HTTPS, a path that ends in `/v1/assets/{sha256}`, and a
/// `media_link` in the query. The path may start with a prefix, for a server
/// behind a proxy that serves it under one.
///
/// The refusal never repeats `raw`, because its query may carry a Media Link,
/// a credential, and the window shows the error.
fn original_by_media_link(raw: &str) -> Result<Url, String> {
    let refused = || "The download is not a Media Link to an attachment's original".to_string();
    let url = Url::parse(raw).map_err(|_| refused())?;
    let segments: Vec<&str> = url
        .path_segments()
        .map(Iterator::collect)
        .unwrap_or_default();
    let reads_an_original = matches!(
        segments.as_slice(),
        [.., "v1", "assets", sha256] if !sha256.is_empty()
    );
    let has_media_link = url.query_pairs().any(|(key, _)| key == "media_link");
    if matches!(url.scheme(), "http" | "https") && reads_an_original && has_media_link {
        Ok(url)
    } else {
        Err(refused())
    }
}

/// Download `url` to `path`, replacing a file already there.
///
/// The request carries no timeout of its own, so the client's applies
/// (`message_crate_http::build_client`, 30 seconds). The blocking client
/// gives that time to the answer's headers and again to each read of the
/// body, so a large file on a slow link finishes however long it takes,
/// and a server that stops sending fails the download after 30 seconds.
///
/// The bytes go to a temporary file of a unique name beside `path` first,
/// which is renamed onto `path` once the last byte is written. A unique name
/// leaves alone any file already in the directory, such as a browser's own
/// `.part` file, and two saves to one path at once never write one file. A
/// download that fails part-way removes its temporary file and leaves a file
/// already at `path` as it was, rather than a cut-off copy under the name the
/// person chose.
fn download_to_file(url: &Url, path: &Path) -> Result<(), String> {
    let client = message_crate_http::build_client().map_err(|e| format!("{e:#}"))?;
    // `without_url`: the URL's query is a Media Link, which the error must
    // not carry to the window.
    let mut response = client.get(url.as_str()).send().map_err(|e| {
        format!(
            "Could not reach the server to download the file: {}",
            e.without_url()
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!(
            "The server refused the download ({status}): {}",
            message_crate_http::error_sentence(&body)
        ));
    }

    let could_not_save =
        |error: std::io::Error| format!("Could not save {}: {error}", path.display());
    // The same directory as `path`, so the rename that finishes the download
    // never crosses a file system. A dropped temporary file removes itself.
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut part = tempfile::Builder::new()
        .prefix(".message-crate-download-")
        .suffix(".part")
        .tempfile_in(directory)
        .map_err(could_not_save)?;
    std::io::copy(&mut response, part.as_file_mut()).map_err(could_not_save)?;
    part.as_file_mut().flush().map_err(could_not_save)?;
    part.as_file().sync_all().map_err(could_not_save)?;
    part.persist(path).map_err(|e| could_not_save(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    const SHA: &str = "0123abcd";

    fn link(server: &MockServer) -> Url {
        Url::parse(&server.url(format!("/v1/assets/{SHA}?media_link=1.2.sig"))).unwrap()
    }

    #[test]
    fn a_media_link_to_an_original_is_accepted_with_or_without_a_prefix() {
        for raw in [
            "http://127.0.0.1:8080/v1/assets/abc?media_link=1.2.sig",
            "https://crate.example.com/messages/v1/assets/abc?media_link=1.2.sig",
        ] {
            assert_eq!(original_by_media_link(raw).unwrap().as_str(), raw);
        }
    }

    #[test]
    fn anything_but_an_original_read_by_a_media_link_is_refused() {
        for raw in [
            // No Media Link: the app would send no credential, and nothing
            // but an asset read by one is this command's to fetch.
            "http://127.0.0.1:8080/v1/assets/abc",
            // A Preview: a download is always the original.
            "http://127.0.0.1:8080/v1/assets/abc/preview?media_link=1.2.sig",
            "http://127.0.0.1:8080/v1/assets/?media_link=1.2.sig",
            "http://127.0.0.1:8080/v1/messages?media_link=1.2.sig",
            "file:///v1/assets/abc?media_link=1.2.sig",
            "/v1/assets/abc?media_link=1.2.sig",
            "",
        ] {
            let error = original_by_media_link(raw).expect_err(raw);
            // The refusal never repeats the credential the URL may carry.
            assert!(!error.contains("1.2.sig"), "{error}");
        }
    }

    /// The names in `dir`, sorted, so a test sees every file a download left.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_original_replaces_the_chosen_file_and_leaves_a_browsers_part_file_alone() {
        let server = MockServer::start();
        let bytes: Vec<u8> = (0..=255u8).cycle().take(3 * 1024 * 1024 + 7).collect();
        let original = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/v1/assets/{SHA}"))
                .query_param("media_link", "1.2.sig");
            then.status(200).body(&bytes);
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Clip.mov");
        std::fs::write(&path, b"an older file").unwrap();
        // A browser's unfinished download of a file with the same name.
        std::fs::write(dir.path().join("Clip.mov.part"), b"a browser's bytes").unwrap();

        download_to_file(&link(&server), &path).unwrap();

        original.assert();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::read(dir.path().join("Clip.mov.part")).unwrap(),
            b"a browser's bytes"
        );
        assert_eq!(names_in(dir.path()), ["Clip.mov", "Clip.mov.part"]);
    }

    #[test]
    fn a_refused_download_says_what_the_server_said_and_leaves_the_file_there_alone() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path(format!("/v1/assets/{SHA}"));
            then.status(401)
                .header("content-type", "application/problem+json")
                .body(r#"{"type":"about:blank","title":"Unauthorized","status":401,"detail":"The Media Link has ended."}"#);
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Clip.mov");
        std::fs::write(&path, b"an older file").unwrap();

        let error = download_to_file(&link(&server), &path).unwrap_err();

        assert!(error.contains("401 Unauthorized"), "{error}");
        assert!(error.contains("The Media Link has ended."), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), b"an older file");
        assert_eq!(names_in(dir.path()), ["Clip.mov"]);
    }

    #[test]
    fn a_download_that_cannot_be_written_leaves_no_temporary_file() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path(format!("/v1/assets/{SHA}"));
            then.status(200).body("bytes");
        });
        let dir = tempfile::tempdir().unwrap();
        // A directory with a file in it stands where the file should go, so
        // the rename fails.
        let path = dir.path().join("Clip.mov");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("inside"), b"kept").unwrap();

        let error = download_to_file(&link(&server), &path).unwrap_err();

        assert!(error.starts_with("Could not save"), "{error}");
        assert_eq!(names_in(dir.path()), ["Clip.mov"]);
        assert!(path.join("inside").exists());
    }

    #[test]
    fn an_unreachable_server_is_reported_without_the_media_link() {
        // A port nothing listens on: bound, read, and released.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = Url::parse(&format!(
            "http://127.0.0.1:{port}/v1/assets/{SHA}?media_link=1.2.sig"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();

        let error = download_to_file(&url, &dir.path().join("Clip.mov")).unwrap_err();

        assert!(error.starts_with("Could not reach the server"), "{error}");
        assert!(!error.contains("1.2.sig"), "{error}");
    }
}
