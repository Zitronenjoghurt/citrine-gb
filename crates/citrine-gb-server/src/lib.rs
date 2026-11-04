mod admin;
mod client_ip;
pub mod config;
mod connections;
mod db;
mod hub;
mod limit;
mod relay;
mod screens;
mod settings;
mod stats;
mod storage;
mod two_factor;

pub use config::{Config, ConfigError};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use citrine_gb_net::{Notice, RelayKey, RelaySecret};
use connections::Connections;
use db::Db;
use hub::Hub;
use limit::{Attempts, Rates};
use screens::Screens;
use settings::Settings;
use stats::Stats;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use storage::Storage;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;

const EMPTY_ACCOUNT_SECS: u64 = 30 * 86_400;
const KEEP_BACKUPS: usize = 14;

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn backup_name(now: u64) -> String {
    let (y, m, d) = civil((now / 86_400) as i64);
    format!("citrine-{y:04}-{m:02}-{d:02}.db")
}

pub struct App {
    config: Config,
    db: Arc<Db>,
    settings: Settings,
    hub: Hub,
    screens: Screens,
    stats: Stats,
    wrong_tries: Attempts,
    unknown_tries: Attempts,
    account_tries: Attempts<i64>,
    admin_attempts: Attempts,
    admin_hash: Option<[u8; 32]>,
    admin_sessions: Mutex<HashMap<[u8; 32], Instant>>,
    admin_setup: Mutex<Option<Vec<u8>>>,
    files_dir: PathBuf,
    storage: Arc<Storage>,
    connections: Arc<Connections>,
    rates: Rates,
    relay_secret: RelaySecret,
}

impl App {
    pub(crate) fn relay_key(&self) -> RelayKey {
        self.relay_secret.public()
    }
}

impl App {
    fn client_ip(&self, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
        client_ip::client_address(headers, peer, self.config.trusted_proxy_hops)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
}

pub struct Running {
    pub addr: SocketAddr,
    app: Arc<App>,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<std::io::Result<()>>,
    purge: JoinHandle<()>,
}

async fn delete_empty_accounts(app: &App, now: u64) {
    if !app.settings.get().delete_empty {
        return;
    }
    let ids = match app
        .db
        .empty_accounts(now.saturating_sub(EMPTY_ACCOUNT_SECS))
        .await
    {
        Ok(ids) => ids,
        Err(e) => return log::error!("database: {e}"),
    };
    let online = app.hub.online();
    let mut deleted = 0;
    for id in ids.into_iter().filter(|id| !online.contains_key(id)) {
        match app.db.delete_account(id).await {
            Ok(Some(_)) => deleted += 1,
            Ok(None) => {}
            Err(e) => return log::error!("database: {e}"),
        }
    }
    if deleted > 0 {
        log::info!("deleted {deleted} accounts that stayed empty for 30 days");
    }
}

async fn tidy(app: &App, now: u64) {
    delete_empty_accounts(app, now).await;
    let db = &app.db;
    let request_age = app.config.limits.friend_request_age;
    match db
        .purge_requests(now.saturating_sub(request_age.as_secs()))
        .await
    {
        Ok(0) => {}
        Ok(n) => log::info!("removed {n} friend requests nobody answered"),
        Err(e) => log::error!("database: {e}"),
    }
    match db.reclaim().await {
        Ok(0) => {}
        Ok(n) => log::info!("gave {n} bytes of the database back to the disk"),
        Err(e) => log::error!("database: {e}"),
    }
}

async fn write_backup(db: &Db, dir: &Path, now: u64) -> std::io::Result<Option<PathBuf>> {
    let path = dir.join(backup_name(now));
    if tokio::fs::try_exists(&path).await? {
        return Ok(None);
    }
    tokio::fs::create_dir_all(dir).await?;
    let part = path.with_extension("db.part");
    let _ = tokio::fs::remove_file(&part).await;
    db.back_up(&part).await.map_err(std::io::Error::other)?;
    tokio::fs::rename(&part, &path).await?;
    let mut names = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("citrine-") && name.ends_with(".db") {
            names.push(name);
        }
    }
    names.sort();
    for old in names.iter().rev().skip(KEEP_BACKUPS) {
        tokio::fs::remove_file(dir.join(old)).await?;
    }
    Ok(Some(path))
}

async fn back_up(db: &Db, dir: &Path, now: u64) {
    match write_backup(db, dir, now).await {
        Ok(Some(path)) => log::info!("copied the database to {}", path.display()),
        Ok(None) => {}
        Err(e) => log::error!("database backup: {e}"),
    }
}

impl Running {
    pub async fn tidy(&self, now: u64) {
        tidy(&self.app, now).await;
    }

    pub async fn require_invites(&self) -> sqlx::Result<()> {
        self.app.settings.set_flag(settings::INVITES, true).await
    }

    pub async fn create_invite(&self) -> sqlx::Result<citrine_gb_net::InviteCode> {
        Ok(self.app.db.create_invite("", now()).await?.code)
    }

    pub async fn stored_messages(&self) -> sqlx::Result<u64> {
        Ok(self.app.db.stored_messages().await?.0)
    }

    pub async fn back_up(&self, now: u64) {
        back_up(&self.app.db, &self.app.config.data_dir.join("backups"), now).await;
    }

    pub fn relay_key(&self) -> RelayKey {
        self.app.relay_key()
    }

    pub async fn stop(self) {
        self.app.hub.close_all(Notice::ShuttingDown);
        self.purge.abort();
        let _ = self.shutdown.send(());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.task).await;
    }
}

async fn not_found(axum::extract::State(app): axum::extract::State<Arc<App>>) -> Response {
    let s = app.settings.get();
    if s.relay || app.config.web {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    let admin = if app.admin_hash.is_some() {
        r#"<p>The admin panel is at <a href="/admin">/admin</a>.</p>"#
    } else {
        ""
    };
    (
        StatusCode::NOT_FOUND,
        Html(format!(
            r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light dark"><title>Citrine server</title></head><body style="font:16px/1.5 system-ui,sans-serif;max-width:36rem;margin:15vh auto;padding:0 1.5rem"><h1 style="font-size:1.3rem">This Citrine server has nothing switched on</h1><p>Set <code>CITRINE_WEB=true</code> to serve the web app, or <code>CITRINE_RELAY=true</code> to run the relay.</p>{admin}</body></html>"#
        )),
    )
        .into_response()
}

async fn web_missing(axum::extract::State(app): axum::extract::State<Arc<App>>) -> Response {
    let Some(dir) = app.config.missing_web_files() else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    (StatusCode::NOT_FOUND, Html(admin::web_missing_page(&dir))).into_response()
}

pub async fn start(config: Config) -> Result<Running, StartError> {
    let files_dir = config.data_dir.join("files");
    tokio::fs::create_dir_all(&files_dir).await?;
    let mut entries = tokio::fs::read_dir(&files_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.path().extension().is_some_and(|e| e == "part") {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
    let db = Arc::new(Db::open(&config.data_dir.join("citrine.db")).await?);
    if config.admin_2fa_reset {
        if db.clear_admin_totp().await? {
            log::warn!(
                "CITRINE_ADMIN_2FA_RESET turned two-factor sign-in for the admin panel off. Remove the variable, sign in and set it up again in Settings"
            );
        } else {
            log::warn!(
                "CITRINE_ADMIN_2FA_RESET is set but two-factor sign-in was already off. Remove the variable"
            );
        }
    }
    let relay_secret =
        RelaySecret::from_bytes(db.relay_secret(*RelaySecret::random().as_bytes()).await?);
    log::info!("relay key {}", relay_secret.public().fingerprint());
    let settings = Settings::load(&config, db.clone()).await?;
    let effective = settings.get();
    if effective.relay && !effective.password_set && !config.relay_open {
        return Err(ConfigError::OpenRelay.into());
    }
    if config.game_sharing && !effective.invites {
        log::warn!(
            "CITRINE_GAME_SHARING is on, but the relay passes games on only while new accounts need an invite code. Set CITRINE_INVITES=true, or turn on invite codes in the admin panel's Settings"
        );
    }
    let app = Arc::new(App {
        admin_hash: config.admin_password.as_deref().map(settings::hash),
        config: config.clone(),
        db: db.clone(),
        settings,
        hub: Hub::default(),
        screens: Screens::default(),
        stats: Stats::default(),
        wrong_tries: Attempts::new(
            config.limits.wrong_tries_per_minute,
            Duration::from_secs(60),
        ),
        unknown_tries: Attempts::new(
            config.limits.unknown_tries_per_minute,
            Duration::from_secs(60),
        ),
        account_tries: Attempts::new(
            config.limits.wrong_tries_per_minute,
            Duration::from_secs(60),
        ),
        admin_attempts: Attempts::new(5, Duration::from_secs(60)),
        admin_sessions: Mutex::new(HashMap::new()),
        admin_setup: Mutex::new(None),
        storage: Arc::new(Storage::new(config.data_dir.clone(), &config.limits)),
        connections: Arc::new(Connections::new(&config.limits)),
        rates: Rates::new(&config.limits),
        files_dir,
        relay_secret,
    });

    let mut router = Router::new()
        .route("/relay", get(relay::upgrade))
        .route("/f/{token}", get(screens::serve));
    router = if app.admin_hash.is_some() {
        router.nest("/admin", admin::router())
    } else {
        router.route("/admin/icon.png", get(admin::icon_png))
    };
    router = if config.web {
        router.fallback_service(
            ServeDir::new(&config.static_dir)
                .not_found_service(axum::handler::Handler::with_state(web_missing, app.clone())),
        )
    } else {
        router.fallback(not_found)
    };
    let router = router.with_state(app.clone());

    let listener = tokio::net::TcpListener::bind((config.bind, config.port)).await?;
    let addr = listener.local_addr()?;
    log::info!(
        "listening on {addr}: web {}, relay {}, admin panel {}",
        if config.web { "on" } else { "off" },
        if effective.relay { "on" } else { "off" },
        if app.admin_hash.is_some() {
            "on"
        } else {
            "off"
        },
    );

    if let Some(dir) = config.missing_web_files() {
        log::warn!(
            "CITRINE_WEB is true, but {} has no web app in it. Build it in the crates/citrine-gb-app folder of the Citrine repository with `{}`, or with Docker, run `docker compose up -d --build` in crates/citrine-gb-server/docker",
            dir.display(),
            config::web_build_command(&dir),
        );
    }

    let tidied = app.clone();
    let backups = config.data_dir.join("backups");
    let purge = tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(3600));
        loop {
            every.tick().await;
            tidy(&tidied, now()).await;
            back_up(&tidied.db, &backups, now()).await;
        }
    });

    let (shutdown, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
    });
    Ok(Running {
        addr,
        app,
        shutdown,
        task,
        purge,
    })
}
