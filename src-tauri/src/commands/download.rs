//! Saving an attachment's original from the server straight to a file.
//!
//! The window could fetch the original and hand its bytes to
//! [`super::paths::save_file`], but then a video of hundreds of megabytes is
//! held whole in the window, again as the bytes it sends, and again in the
//! request the app receives. Here the window sends only a Media Link for the
//! original, and the app copies the server's answer to the file a buffer at a
//! time, so memory stays flat whatever the file's size.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Url};

use super::paths::choose_save_path;

/// How long one download may take, from the request to its last byte. A Media
/// Link is open for an hour (`docs/architecture/http-api.md`, "Credentials and
/// reach"), so the download gets the same hour. The client's default of 30
/// seconds would cut off a large video from a server on another computer.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Show the Save dialog with `file_name` filled in, then download the
/// attachment's original that `url` names to the file the person chose,
/// replacing a file already there.
///
/// `url` is a Media Link's `url`: `…/v1/assets/{sha256}?media_link=…`, which
/// reads one original with no `Authorization` header. The bytes go from the
/// server to the disk and never through the window. The path comes from the
/// dialog, never from the window, and the window may name only an original
/// read by a Media Link, so a script in the window can make the app neither
/// overwrite a file of its choosing nor fetch any other address.
///
/// Returns `false` when the person closed the dialog without choosing a
/// place, and `true` once the file is written.
///
/// # Errors
///
/// Returns an error when `url` is not an original read by a Media Link, when
/// `file_name` is empty, when the server refuses the download or it breaks
/// off, or when the file cannot be written.
#[tauri::command]
pub async fn save_download(app: AppHandle, url: String, file_name: String) -> Result<bool, String> {
    let url = original_by_media_link(&url)?;
    if file_name.trim().is_empty() {
        return Err("The file to save came without its name".into());
    }
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
fn original_by_media_link(raw: &str) -> Result<Url, String> {
    let refused = || format!("{raw:?} is not a Media Link to an attachment's original");
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
/// The bytes go to a part file beside `path` first, which is renamed onto
/// `path` once the last byte is written. A download that fails part-way
/// removes its part file and leaves a file already at `path` as it was,
/// rather than a cut-off copy under the name the person chose.
fn download_to_file(url: &Url, path: &Path) -> Result<(), String> {
    let client = message_crate_http::build_client().map_err(|e| format!("{e:#}"))?;
    let mut response = client
        .get(url.as_str())
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .map_err(|e| format!("Could not reach the server to download the file: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(format!(
            "The server refused the download ({status}): {}",
            message_crate_http::error_sentence(&body)
        ));
    }

    let part = part_path(path);
    let written = File::create(&part)
        .and_then(|mut file| {
            std::io::copy(&mut response, &mut file)?;
            file.flush()?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&part, path));
    written.map_err(|error| {
        let _ = std::fs::remove_file(&part);
        format!("Could not save {}: {error}", path.display())
    })
}

/// The part file a download to `path` is written to: the same name with
/// `.part` added, in the same directory, so the rename that finishes it never
/// crosses a file system.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
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
            assert!(original_by_media_link(raw).is_err(), "{raw:?} was accepted");
        }
    }

    #[test]
    fn the_original_is_written_to_the_chosen_file_with_no_part_file_left() {
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

        download_to_file(&link(&server), &path).unwrap();

        original.assert();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!part_path(&path).exists());
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
        assert!(!part_path(&path).exists());
    }

    #[test]
    fn a_download_that_cannot_be_written_leaves_no_part_file() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path(format!("/v1/assets/{SHA}"));
            then.status(200).body("bytes");
        });
        let dir = tempfile::tempdir().unwrap();
        // A directory stands where the file should go, so the rename fails.
        let path = dir.path().join("Clip.mov");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("inside"), b"kept").unwrap();

        let error = download_to_file(&link(&server), &path).unwrap_err();

        assert!(error.starts_with("Could not save"), "{error}");
        assert!(!part_path(&path).exists());
        assert!(path.join("inside").exists());
    }
}
