use super::guard;
use crate::archive::{AddError, Added, Console};
use crate::{App, now, rand_id};
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use citrine_gb_net::ArchiveId;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

const STEPS: u64 = 200;

#[derive(Deserialize)]
pub(super) struct UploadQuery {
    name: String,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "step", rename_all = "lowercase")]
pub(super) enum Line {
    Compressing { done: u64, of: u64 },
    Cover,
    Added(Finished),
    There(Finished),
    Failed { why: Why },
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub(super) struct Finished {
    id: String,
    name: String,
    cover: bool,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Why {
    Off,
    NotGame,
    Empty,
    TooBig,
    TooMany,
    Full,
    Busy,
    Broken,
}

impl Why {
    pub(super) fn from_slug(slug: &str) -> Option<Self> {
        Some(match slug {
            "off" => Self::Off,
            "not-game" => Self::NotGame,
            "empty" => Self::Empty,
            "too-big" => Self::TooBig,
            "too-many" => Self::TooMany,
            "full" => Self::Full,
            "busy" => Self::Busy,
            "broken" => Self::Broken,
            _ => return None,
        })
    }

    fn of(error: &AddError) -> Self {
        match error {
            AddError::Off => Self::Off,
            AddError::Empty => Self::Empty,
            AddError::TooLarge(_) => Self::TooBig,
            AddError::TooMany(_) => Self::TooMany,
            AddError::Full => Self::Full,
            AddError::Io(_) | AddError::Db(_) => Self::Broken,
        }
    }

    pub(super) fn words(self, app: &App) -> String {
        match self {
            Self::Off => "the Game Archive is off".into(),
            Self::NotGame => "it isn't a Game Boy, Game Boy Advance or DS game".into(),
            Self::Empty => "the file is empty".into(),
            Self::TooBig => format!("it's over the {} MB limit", app.archive.file_bytes() >> 20),
            Self::TooMany => format!(
                "the Game Archive holds {} games, its limit",
                app.archive.max_games()
            ),
            Self::Full => "the Game Archive is full".into(),
            Self::Busy => "another upload was running".into(),
            Self::Broken => "the relay couldn't store it".into(),
        }
    }
}

fn line(line: &Line) -> Bytes {
    let mut text = serde_json::to_vec(line).unwrap_or_default();
    text.push(b'\n');
    Bytes::from(text)
}

fn failed(status: StatusCode, why: Why) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        line(&Line::Failed { why }),
    )
        .into_response()
}

pub(super) async fn upload(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<UploadQuery>,
    body: Body,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if !app.archive.on() {
        return failed(StatusCode::CONFLICT, Why::Off);
    }
    let name = Path::new(&query.name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    if Console::of(&name) == Console::Other {
        return failed(StatusCode::UNPROCESSABLE_ENTITY, Why::NotGame);
    }
    let limit = app.archive.file_bytes();
    let Some(slot) = app.archive.upload_slot() else {
        return failed(StatusCode::CONFLICT, Why::Busy);
    };
    let path = app
        .archive
        .dir()
        .join(format!("{}.upload", ArchiveId(rand_id())));
    let Ok(mut file) = tokio::fs::File::create(&path).await else {
        return failed(StatusCode::INTERNAL_SERVER_ERROR, Why::Broken);
    };
    let mut stream = body.into_data_stream();
    let mut got = 0u64;
    let refused = loop {
        let Some(chunk) = stream.next().await else {
            break None;
        };
        let Ok(chunk) = chunk else {
            break Some((StatusCode::BAD_REQUEST, Why::Broken));
        };
        got += chunk.len() as u64;
        if got > limit {
            break Some((StatusCode::PAYLOAD_TOO_LARGE, Why::TooBig));
        }
        if file.write_all(&chunk).await.is_err() {
            break Some((StatusCode::INTERNAL_SERVER_ERROR, Why::Broken));
        }
    };
    let flushed = file.flush().await;
    drop(file);
    if let Some((status, why)) = refused.or(flushed
        .err()
        .map(|_| (StatusCode::INTERNAL_SERVER_ERROR, Why::Broken)))
    {
        let _ = tokio::fs::remove_file(&path).await;
        return failed(status, why);
    }
    let (tx, rx) = mpsc::channel::<Bytes>(8);
    tokio::spawn(async move {
        let _slot = slot;
        let of = got;
        let step = (of / STEPS).max(1);
        let mut shown = 0;
        let progress_tx = tx.clone();
        let progress = move |done: u64| {
            if progress_tx.is_closed() {
                return false;
            }
            if done == of || done - shown >= step {
                shown = done;
                let _ = progress_tx.try_send(line(&Line::Compressing { done, of }));
            }
            true
        };
        let added = app
            .archive
            .add_file(&app.db, &app.storage, &path, &name, now(), progress)
            .await;
        let _ = tokio::fs::remove_file(&path).await;
        let last = match added {
            Ok(Added { id, new }) => {
                if new {
                    app.tell_archive_all().await;
                    if app.archive.covers_on() {
                        let _ = tx.send(line(&Line::Cover)).await;
                        app.archive.find_cover(&app.db, id, &name).await;
                    }
                }
                let done = Finished {
                    id: id.to_string(),
                    name: app.archive.name(&app.db, id).await.unwrap_or_default(),
                    cover: app.archive.has_cover(id),
                };
                if new {
                    Line::Added(done)
                } else {
                    Line::There(done)
                }
            }
            Err(e) => {
                if !tx.is_closed() {
                    log::warn!("couldn't add {name} to the Game Archive: {e}");
                }
                Line::Failed { why: Why::of(&e) }
            }
        };
        let _ = tx.send(line(&last)).await;
    });
    let lines = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|b| (Ok::<_, Infallible>(b), rx))
    });
    (
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from_stream(lines),
    )
        .into_response()
}
