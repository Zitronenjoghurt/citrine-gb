#[cfg(not(target_arch = "wasm32"))]
mod install;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod release;
#[cfg(not(target_arch = "wasm32"))]
mod worker;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
pub use install::relaunch;
#[cfg(not(target_arch = "wasm32"))]
use release::Platform;
use release::Release;

use std::sync::{Arc, Mutex};

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub page: String,
    pub installable: bool,
    pub size: u64,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Available(Offer),
    Downloading {
        offer: Offer,
        done: u64,
        total: u64,
    },
    Ready(Offer),
    Failed {
        offer: Offer,
        reason: String,
        retry: bool,
    },
}

impl Phase {
    pub fn offer(&self) -> Option<&Offer> {
        match self {
            Self::Idle => None,
            Self::Available(offer) | Self::Ready(offer) => Some(offer),
            Self::Downloading { offer, .. } | Self::Failed { offer, .. } => Some(offer),
        }
    }

    pub fn waiting(&self) -> bool {
        self.offer().is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tap {
    Nothing,
    Open(String),
    Restart,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Default)]
struct Shared {
    phase: Phase,
    release: Option<Release>,
    checking: bool,
}

#[cfg_attr(target_arch = "wasm32", derive(Default))]
pub struct Updater {
    shared: Arc<Mutex<Shared>>,
    restart: bool,
    #[cfg(not(target_arch = "wasm32"))]
    backend: Option<worker::Backend>,
    #[cfg(not(target_arch = "wasm32"))]
    last_check: Option<std::time::Instant>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Updater {
    fn default() -> Self {
        Self {
            shared: Arc::default(),
            restart: false,
            #[cfg(not(target_arch = "wasm32"))]
            backend: Platform::current().map(worker::Backend::live),
            #[cfg(not(target_arch = "wasm32"))]
            last_check: None,
        }
    }
}

impl Updater {
    pub fn phase(&self) -> Phase {
        self.lock().phase.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn take_restart(&mut self) -> bool {
        std::mem::take(&mut self.restart)
    }

    pub fn tap(&mut self) -> Tap {
        let phase = self.phase();
        let Some(offer) = phase.offer().cloned() else {
            return Tap::Nothing;
        };
        match phase {
            Phase::Downloading { .. } | Phase::Idle => Tap::Nothing,
            Phase::Ready(_) if cfg!(target_os = "android") => {
                self.reopen_installer();
                Tap::Nothing
            }
            Phase::Ready(_) => {
                self.restart = true;
                Tap::Restart
            }
            Phase::Available(_) | Phase::Failed { retry: true, .. } if offer.installable => {
                self.download(offer);
                Tap::Nothing
            }
            Phase::Available(_) | Phase::Failed { .. } => Tap::Open(offer.page),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn tick(&mut self, _enabled: bool) {}

    #[cfg(target_arch = "wasm32")]
    fn download(&mut self, _offer: Offer) {}

    #[cfg(not(target_os = "android"))]
    fn reopen_installer(&self) {}

    #[cfg(target_os = "android")]
    fn reopen_installer(&self) {
        install::reopen();
    }
}
