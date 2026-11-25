use citrine_gb_gameinfo::covers::names::{Names, System};
use citrine_gb_gameinfo::covers::source;
use futures_util::{Stream, StreamExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const HOST: &str = "raw.githubusercontent.com";
const TIMEOUT: Duration = Duration::from_secs(20);
pub const FIND_TIME: Duration = Duration::from_secs(45);
const NAMES_FRESH: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const NAMES_MAX: usize = 64 << 20;
pub const ART_MAX: usize = 2 << 20;
const MOST_LINKS: u8 = 2;
const MOST_REDIRECTS: usize = 3;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FetchError {
    #[error("only {HOST} is asked for covers")]
    Host,
    #[error("the answer is over {0} bytes")]
    TooBig(usize),
    #[error("not found")]
    Missing,
    #[error("{0}")]
    Net(String),
}

pub struct Find<'a> {
    pub systems: &'a [System],
    pub sha1: [u8; 20],
    pub serial: Option<[u8; 4]>,
    pub file_name: &'a str,
}

pub struct Covers {
    http: reqwest::Client,
    dir: PathBuf,
}

pub fn allowed(url: &str) -> bool {
    reqwest::Url::parse(url)
        .is_ok_and(|url| url.scheme() == "https" && url.host_str() == Some(HOST))
}

pub async fn capped<S, B, E>(body: S, cap: usize) -> Result<Vec<u8>, FetchError>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let mut body = std::pin::pin!(body);
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| FetchError::Net(e.to_string()))?;
        if out.len() + chunk.as_ref().len() > cap {
            return Err(FetchError::TooBig(cap));
        }
        out.extend_from_slice(chunk.as_ref());
    }
    Ok(out)
}

impl Covers {
    pub fn new(dir: &Path) -> reqwest::Result<Self> {
        let redirects = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < MOST_REDIRECTS && allowed(attempt.url().as_str()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .redirect(redirects)
                .build()?,
            dir: dir.to_owned(),
        })
    }

    async fn get(&self, url: &str, cap: usize) -> Result<Vec<u8>, FetchError> {
        if !allowed(url) {
            return Err(FetchError::Host);
        }
        let answer = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| FetchError::Net(e.to_string()))?;
        if answer.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FetchError::Missing);
        }
        if !answer.status().is_success() {
            return Err(FetchError::Net(answer.status().to_string()));
        }
        if answer.content_length().is_some_and(|len| len > cap as u64) {
            return Err(FetchError::TooBig(cap));
        }
        let chunks = futures_util::stream::unfold(answer, |mut answer| async move {
            match answer.chunk().await {
                Ok(Some(chunk)) => Some((Ok(chunk), answer)),
                Ok(None) => None,
                Err(e) => Some((Err(e), answer)),
            }
        });
        capped(chunks, cap).await
    }

    async fn names(&self, system: System) -> Option<Names> {
        let path = self.dir.join(format!("{}.dat", system.short()));
        let fresh = tokio::fs::metadata(&path)
            .await
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|age| age < NAMES_FRESH);
        if !fresh {
            match self.get(&source::names_url(system), NAMES_MAX).await {
                Ok(dat) => {
                    let _ = tokio::fs::create_dir_all(&self.dir).await;
                    if let Err(e) = tokio::fs::write(&path, &dat).await {
                        log::warn!("couldn't keep the {} cover names: {e}", system.short());
                    }
                }
                Err(e) => log::warn!("couldn't fetch the {} cover names: {e}", system.short()),
            }
        }
        let dat = tokio::fs::read(&path).await.ok()?;
        tokio::task::spawn_blocking(move || Names::parse(&String::from_utf8_lossy(&dat)))
            .await
            .ok()
            .filter(|names| !names.is_empty())
    }

    pub async fn find(&self, game: Find<'_>) -> Option<Vec<u8>> {
        let mut lists = Vec::new();
        for &system in game.systems {
            if let Some(names) = self.names(system).await {
                lists.push((system, names));
            }
        }
        let (system, name) = lists
            .iter()
            .find_map(|(system, names)| Some((*system, names.find(&game.sha1, game.serial)?)))
            .or_else(|| {
                lists
                    .iter()
                    .find_map(|(system, names)| Some((*system, names.find_title(game.file_name)?)))
            })?;
        for file in source::art_files(name) {
            let mut url = source::art_url(system, &file);
            let mut links = 0;
            loop {
                let body = match self.get(&url, ART_MAX).await {
                    Ok(body) => body,
                    Err(FetchError::Missing) => break,
                    Err(e) => {
                        log::warn!("couldn't fetch a cover: {e}");
                        return None;
                    }
                };
                if body.starts_with(b"\x89PNG") {
                    return Some(body);
                }
                match source::link_target(&body) {
                    Some(target) if links < MOST_LINKS => {
                        url = source::art_url(system, target);
                        links += 1;
                    }
                    _ => break,
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(sizes: &[usize]) -> impl Stream<Item = Result<Vec<u8>, String>> {
        futures_util::stream::iter(sizes.iter().map(|&n| Ok(vec![7; n])).collect::<Vec<_>>())
    }

    #[test]
    fn only_the_libretro_host_is_asked() {
        assert!(allowed(&source::art_url(
            System::Gba,
            "Metroid Fusion (USA).png"
        )));
        assert!(allowed(&source::names_url(System::Nds)));
        assert!(!allowed(
            "http://raw.githubusercontent.com/libretro-thumbnails/x.png"
        ));
        assert!(!allowed(
            "https://raw.githubusercontent.com.evil.example/x.png"
        ));
        assert!(!allowed(
            "https://example.com/raw.githubusercontent.com/x.png"
        ));
        assert!(!allowed("https://user@evil.example/x.png"));
        assert!(!allowed("file:///etc/passwd"));
        assert!(!allowed("not a url"));
    }

    #[tokio::test]
    async fn a_url_off_the_host_is_never_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let covers = Covers::new(dir.path()).unwrap();
        assert_eq!(
            covers.get("https://127.0.0.1:9/cover.png", ART_MAX).await,
            Err(FetchError::Host)
        );
    }

    #[tokio::test]
    async fn an_answer_over_the_cap_is_dropped() {
        assert_eq!(capped(chunks(&[4, 4]), 8).await.unwrap().len(), 8);
        assert_eq!(
            capped(chunks(&[4, 4, 1]), 8).await,
            Err(FetchError::TooBig(8))
        );
        assert_eq!(
            capped(chunks(&[ART_MAX + 1]), ART_MAX).await,
            Err(FetchError::TooBig(ART_MAX))
        );
    }
}
