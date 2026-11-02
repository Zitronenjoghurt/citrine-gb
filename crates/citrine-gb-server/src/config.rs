mod limits;

pub use limits::Limits;

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{var} must be true or false, not '{value}'")]
    Switch { var: &'static str, value: String },
    #[error("{var} must be a number, not '{value}'")]
    Number { var: &'static str, value: String },
    #[error("CITRINE_BIND must be an IP address, not '{0}'")]
    Bind(String),
    #[error(
        "the relay is on without a password. Set CITRINE_RELAY_PASSWORD, or set CITRINE_RELAY_OPEN=true to run an open relay"
    )]
    OpenRelay,
}

#[derive(Clone)]
pub struct Config {
    pub web: bool,
    pub relay: bool,
    pub relay_password: Option<String>,
    pub relay_open: bool,
    pub admin_password: Option<String>,
    pub admin_2fa_reset: bool,
    pub static_dir: PathBuf,
    pub data_dir: PathBuf,
    pub storage_mb: Option<u64>,
    pub game_sharing: bool,
    pub invites: bool,
    pub bind: IpAddr,
    pub port: u16,
    pub public_url: Option<String>,
    pub limits: Limits,
    pub trusted_proxy_hops: usize,
    pub given: BTreeSet<&'static str>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("web", &self.web)
            .field("relay", &self.relay)
            .field(
                "relay_password",
                &self.relay_password.as_ref().map(|_| ".."),
            )
            .field("relay_open", &self.relay_open)
            .field(
                "admin_password",
                &self.admin_password.as_ref().map(|_| ".."),
            )
            .field("admin_2fa_reset", &self.admin_2fa_reset)
            .field("static_dir", &self.static_dir)
            .field("data_dir", &self.data_dir)
            .field("storage_mb", &self.storage_mb)
            .field("game_sharing", &self.game_sharing)
            .field("invites", &self.invites)
            .field("bind", &self.bind)
            .field("port", &self.port)
            .field("public_url", &self.public_url)
            .field("limits", &self.limits)
            .field("trusted_proxy_hops", &self.trusted_proxy_hops)
            .field("given", &self.given)
            .finish()
    }
}

const VARS: &[&str] = &[
    "CITRINE_WEB",
    "CITRINE_RELAY",
    "CITRINE_RELAY_PASSWORD",
    "CITRINE_RELAY_OPEN",
    "CITRINE_ADMIN_PASSWORD",
    "CITRINE_ADMIN_2FA_RESET",
    "CITRINE_STATIC_DIR",
    "CITRINE_DATA_DIR",
    "CITRINE_STORAGE_MB",
    "CITRINE_GAME_SHARING",
    "CITRINE_INVITES",
    "CITRINE_BIND",
    "CITRINE_PORT",
    "CITRINE_PUBLIC_URL",
    "CITRINE_TRUSTED_PROXY_HOPS",
];

fn switch(var: &'static str, value: Option<String>, default: bool) -> Result<bool, ConfigError> {
    let Some(value) = value else {
        return Ok(default);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        _ => Err(ConfigError::Switch { var, value }),
    }
}

fn number<T: std::str::FromStr>(
    var: &'static str,
    value: Option<String>,
    default: T,
) -> Result<T, ConfigError> {
    match value {
        None => Ok(default),
        Some(value) => value
            .trim()
            .parse()
            .map_err(|_| ConfigError::Number { var, value }),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        Self::from_vars(|name| std::env::var(name).ok())
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let var = |name: &str| var(name).filter(|v| !v.trim().is_empty());
        let given = VARS
            .iter()
            .chain(limits::VARS)
            .copied()
            .filter(|name| var(name).is_some())
            .collect();
        let bind = match var("CITRINE_BIND") {
            None => IpAddr::from([0, 0, 0, 0]),
            Some(text) => text.trim().parse().map_err(|_| ConfigError::Bind(text))?,
        };
        Ok(Self {
            web: switch("CITRINE_WEB", var("CITRINE_WEB"), true)?,
            relay: switch("CITRINE_RELAY", var("CITRINE_RELAY"), false)?,
            relay_password: var("CITRINE_RELAY_PASSWORD"),
            relay_open: switch("CITRINE_RELAY_OPEN", var("CITRINE_RELAY_OPEN"), false)?,
            admin_password: var("CITRINE_ADMIN_PASSWORD"),
            admin_2fa_reset: switch(
                "CITRINE_ADMIN_2FA_RESET",
                var("CITRINE_ADMIN_2FA_RESET"),
                false,
            )?,
            static_dir: var("CITRINE_STATIC_DIR")
                .unwrap_or_else(|| "./static".into())
                .into(),
            data_dir: var("CITRINE_DATA_DIR")
                .unwrap_or_else(|| "./data".into())
                .into(),
            storage_mb: var("CITRINE_STORAGE_MB")
                .map(|v| number("CITRINE_STORAGE_MB", Some(v), 0))
                .transpose()?,
            game_sharing: switch("CITRINE_GAME_SHARING", var("CITRINE_GAME_SHARING"), false)?,
            invites: switch("CITRINE_INVITES", var("CITRINE_INVITES"), false)?,
            bind,
            port: number("CITRINE_PORT", var("CITRINE_PORT"), 13707)?,
            public_url: var("CITRINE_PUBLIC_URL")
                .map(|u| u.trim().trim_end_matches('/').to_string()),
            limits: Limits::from_vars(var)?,
            trusted_proxy_hops: number(
                "CITRINE_TRUSTED_PROXY_HOPS",
                var("CITRINE_TRUSTED_PROXY_HOPS"),
                0,
            )?,
            given,
        })
    }

    pub fn missing_web_files(&self) -> Option<PathBuf> {
        (self.web && !self.static_dir.join("index.html").is_file()).then(|| {
            std::path::absolute(&self.static_dir).unwrap_or_else(|_| self.static_dir.clone())
        })
    }
}

pub fn web_build_command(dir: &std::path::Path) -> String {
    format!("trunk build --release --dist {}", dir.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_vars(|name| map.get(name).cloned())
    }

    #[test]
    fn defaults_match_the_design() {
        let c = config(&[]).unwrap();
        assert!(c.web);
        assert!(!c.relay);
        assert_eq!(c.relay_password, None);
        assert!(!c.relay_open);
        assert_eq!(c.admin_password, None);
        assert!(!c.admin_2fa_reset);
        assert_eq!(c.static_dir, PathBuf::from("./static"));
        assert_eq!(c.data_dir, PathBuf::from("./data"));
        assert_eq!(c.storage_mb, None);
        assert!(!c.game_sharing);
        assert!(!c.invites);
        assert_eq!(c.port, 13707);
        assert_eq!(c.bind, IpAddr::from([0, 0, 0, 0]));
        assert_eq!(c.public_url, None);
        assert_eq!(c.trusted_proxy_hops, 0);
    }

    #[test]
    fn reads_every_var() {
        let c = config(&[
            ("CITRINE_WEB", "false"),
            ("CITRINE_RELAY", "true"),
            ("CITRINE_RELAY_PASSWORD", "pw"),
            ("CITRINE_RELAY_OPEN", "true"),
            ("CITRINE_ADMIN_PASSWORD", "admin"),
            ("CITRINE_ADMIN_2FA_RESET", "true"),
            ("CITRINE_STATIC_DIR", "/srv/static"),
            ("CITRINE_DATA_DIR", "/srv/data"),
            ("CITRINE_STORAGE_MB", "0"),
            ("CITRINE_GAME_SHARING", "true"),
            ("CITRINE_INVITES", "true"),
            ("CITRINE_PORT", "8080"),
            ("CITRINE_BIND", "127.0.0.1"),
            ("CITRINE_PUBLIC_URL", "https://gb.example.com/"),
            ("CITRINE_TRUSTED_PROXY_HOPS", "1"),
        ])
        .unwrap();
        assert!(!c.web);
        assert!(c.relay);
        assert_eq!(c.relay_password.as_deref(), Some("pw"));
        assert!(c.relay_open);
        assert_eq!(c.admin_password.as_deref(), Some("admin"));
        assert!(c.admin_2fa_reset);
        assert_eq!(c.static_dir, PathBuf::from("/srv/static"));
        assert_eq!(c.data_dir, PathBuf::from("/srv/data"));
        assert_eq!(c.storage_mb, Some(0));
        assert!(c.game_sharing);
        assert!(c.invites);
        assert_eq!(c.port, 8080);
        assert_eq!(c.bind, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(c.public_url.as_deref(), Some("https://gb.example.com"));
        assert_eq!(c.trusted_proxy_hops, 1);
    }

    #[test]
    fn older_switch_words_still_read() {
        for (on, off) in [("on", "off"), ("yes", "no"), ("1", "0")] {
            let c = config(&[
                ("CITRINE_WEB", off),
                ("CITRINE_RELAY", on),
                ("CITRINE_RELAY_OPEN", on),
            ])
            .unwrap();
            assert!(!c.web && c.relay && c.relay_open, "{on} and {off}");
        }
    }

    #[test]
    fn empty_values_count_as_unset() {
        let c = config(&[("CITRINE_RELAY_PASSWORD", ""), ("CITRINE_PORT", " ")]).unwrap();
        assert_eq!(c.relay_password, None);
        assert_eq!(c.port, 13707);
    }

    #[test]
    fn bad_values_are_refused() {
        assert!(matches!(
            config(&[("CITRINE_RELAY", "maybe")]),
            Err(ConfigError::Switch {
                var: "CITRINE_RELAY",
                ..
            })
        ));
        assert!(matches!(
            config(&[("CITRINE_PORT", "http")]),
            Err(ConfigError::Number {
                var: "CITRINE_PORT",
                ..
            })
        ));
        assert!(matches!(
            config(&[("CITRINE_BIND", "localhost")]),
            Err(ConfigError::Bind(_))
        ));
    }

    #[test]
    fn debug_hides_passwords() {
        let c = config(&[
            ("CITRINE_RELAY_PASSWORD", "relay-secret"),
            ("CITRINE_ADMIN_PASSWORD", "admin-secret"),
        ])
        .unwrap();
        let text = format!("{c:?}");
        assert!(!text.contains("relay-secret"));
        assert!(!text.contains("admin-secret"));
    }
}
