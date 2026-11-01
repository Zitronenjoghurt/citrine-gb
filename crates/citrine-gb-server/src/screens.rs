use crate::App;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const CACHE: &str = "max-age=60";

#[derive(Default)]
pub struct Screens {
    latest: Mutex<HashMap<String, Bytes>>,
}

impl Screens {
    fn latest(&self) -> MutexGuard<'_, HashMap<String, Bytes>> {
        self.latest.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn put(&self, token: &str, png: Vec<u8>) {
        self.latest().insert(token.to_string(), Bytes::from(png));
    }

    pub fn get(&self, token: &str) -> Option<Bytes> {
        self.latest().get(token).cloned()
    }

    pub fn remove(&self, token: &str) {
        self.latest().remove(token);
    }
}

pub fn token() -> String {
    let mut bytes = [0; 16];
    getrandom::getrandom(&mut bytes).expect("no system random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn serve(State(app): State<Arc<App>>, Path(token): Path<String>) -> Response {
    let shown = app.settings.get().pictures;
    match app.screens.get(&token).filter(|_| shown) {
        Some(png) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, CACHE),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            png,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_picture_replaces_the_old_one() {
        let screens = Screens::default();
        let token = token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        screens.put(&token, vec![1, 2]);
        screens.put(&token, vec![3]);
        assert_eq!(screens.get(&token).as_deref(), Some(&[3u8][..]));
        screens.remove(&token);
        assert_eq!(screens.get(&token), None);
    }
}
