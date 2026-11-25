#[expect(
    dead_code,
    reason = "the ledger helpers are shared with the ledger runners"
)]
mod archives;

use archives::{codeload_url, download, sha256_hex};
use citrine_gb_threads::{Mode, Threads};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const NO_WAIT: [Duration; 4] = [Duration::ZERO; 4];

#[derive(Clone)]
enum Reply {
    Status(u16),
    Body(Vec<u8>),
    CutOff(Vec<u8>),
}

struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
}

fn serve(replies: Vec<Reply>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/suite.zip", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    Threads::new(Mode::Threads)
        .spawner()
        .thread("archive server", move || {
            for (index, stream) in listener.incoming().enumerate() {
                let mut stream = stream.unwrap();
                let mut request = Vec::new();
                let mut byte = [0u8];
                while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                    request.push(byte[0]);
                }
                counter.fetch_add(1, Ordering::SeqCst);
                let reply = replies[index.min(replies.len() - 1)].clone();
                let (head, body) = match reply {
                    Reply::Status(code) => (
                        format!(
                            "HTTP/1.1 {code} Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        ),
                        Vec::new(),
                    ),
                    Reply::Body(body) => (
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ),
                        body,
                    ),
                    Reply::CutOff(body) => (
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len() * 4
                        ),
                        body,
                    ),
                };
                stream.write_all(head.as_bytes()).ok();
                stream.write_all(&body).ok();
            }
        });
    Server { url, hits }
}

fn suite_bytes() -> Vec<u8> {
    (0..64 * 1024).map(|i| (i * 7 % 251) as u8).collect()
}

#[expect(
    clippy::disallowed_methods,
    reason = "the core crate has no storage folders, so its tests use the system temp folder"
)]
fn empty_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("archives-fetch-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    dir
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_download_retries_past_two_503s_and_caches_the_file() {
    let data = suite_bytes();
    let sha = sha256_hex(&data);
    let server = serve(vec![
        Reply::Status(503),
        Reply::Status(503),
        Reply::Body(data.clone()),
    ]);
    let dir = empty_dir("retry");

    let path = download(&dir, "suite", &sha, &[server.url], &NO_WAIT).unwrap();

    assert_eq!(server.hits.load(Ordering::SeqCst), 3);
    assert_eq!(path, dir.join(format!("{sha}.zip")));
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert_eq!(files_in(&dir), vec![format!("{sha}.zip")]);
}

#[test]
fn a_cut_off_download_never_sits_in_the_cache() {
    let data = suite_bytes();
    let sha = sha256_hex(&data);
    let server = serve(vec![Reply::CutOff(data)]);
    let dir = empty_dir("cut-off");

    let failure = download(&dir, "suite", &sha, &[server.url], &NO_WAIT).unwrap_err();

    assert_eq!(server.hits.load(Ordering::SeqCst), 5);
    assert!(failure.contains("curl exit 18"), "{failure}");
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[test]
fn a_wrong_sha_never_sits_in_the_cache() {
    let server = serve(vec![Reply::Body(suite_bytes())]);
    let dir = empty_dir("wrong-sha");
    let pinned = "0".repeat(64);

    let failure = download(&dir, "suite", &pinned, &[server.url], &NO_WAIT).unwrap_err();

    assert!(failure.contains("got SHA-256"), "{failure}");
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[test]
fn a_source_that_stays_down_fails_with_one_line_naming_the_url_and_sha() {
    let sha = sha256_hex(&suite_bytes());
    let server = serve(vec![Reply::Status(503)]);
    let dir = empty_dir("down");

    let failure = download(
        &dir,
        "suite",
        &sha,
        std::slice::from_ref(&server.url),
        &NO_WAIT,
    )
    .unwrap_err();

    assert_eq!(server.hits.load(Ordering::SeqCst), 5);
    assert_eq!(
        failure,
        format!(
            "suite: could not download SHA-256 {sha} from {} (HTTP 503, 5 tries)",
            server.url
        )
    );
}

#[test]
fn a_missing_file_is_not_retried() {
    let sha = sha256_hex(&suite_bytes());
    let server = serve(vec![Reply::Status(404)]);
    let dir = empty_dir("missing");

    let failure = download(&dir, "suite", &sha, &[server.url], &NO_WAIT).unwrap_err();

    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    assert!(failure.contains("HTTP 404"), "{failure}");
}

#[test]
fn a_download_falls_back_to_the_second_source() {
    let data = suite_bytes();
    let sha = sha256_hex(&data);
    let down = serve(vec![Reply::Status(503)]);
    let mirror = serve(vec![Reply::Body(data.clone())]);
    let dir = empty_dir("mirror");

    let path = download(&dir, "suite", &sha, &[down.url, mirror.url], &NO_WAIT).unwrap();

    assert_eq!(down.hits.load(Ordering::SeqCst), 5);
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert_eq!(files_in(&dir), vec![format!("{sha}.zip")]);
}

#[test]
fn a_github_archive_has_a_codeload_second_source() {
    assert_eq!(
        codeload_url(
            "https://github.com/SingleStepTests/ARM7TDMI/archive/e3097d88d428752736b949d94c094d5da50f0db6.zip"
        )
        .as_deref(),
        Some(
            "https://codeload.github.com/SingleStepTests/ARM7TDMI/zip/e3097d88d428752736b949d94c094d5da50f0db6"
        )
    );
    assert_eq!(
        codeload_url(
            "https://github.com/c-sp/game-boy-test-roms/releases/download/v7.0/game-boy-test-roms-v7.0.zip"
        ),
        None
    );
}
