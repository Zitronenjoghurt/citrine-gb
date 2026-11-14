use std::fmt::Display;
use std::net::Ipv4Addr;
use strum_macros::EnumIter;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum OnlineService {
    #[default]
    None,
    Citrine,
    Kaeru,
    Other,
}

impl OnlineService {
    pub fn preset_dns(self) -> Option<&'static str> {
        match self {
            Self::Kaeru => Some("178.62.43.212"),
            Self::None | Self::Citrine | Self::Other => None,
        }
    }
}

impl Display for OnlineService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "Off",
            Self::Citrine => "Citrine relay",
            Self::Kaeru => "Kaeru WFC",
            Self::Other => "Other",
        })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DsOnline {
    pub service: OnlineService,
    pub dns: Option<String>,
    pub direct: bool,
}

impl DsOnline {
    pub fn dns_text(&self) -> &str {
        self.dns
            .as_deref()
            .or(self.service.preset_dns())
            .unwrap_or_default()
    }

    pub fn dns(&self) -> Option<Ipv4Addr> {
        match self.service {
            OnlineService::None | OnlineService::Citrine => None,
            OnlineService::Kaeru | OnlineService::Other => self.dns_text().trim().parse().ok(),
        }
    }

    pub fn dns_needed(&self) -> bool {
        matches!(self.service, OnlineService::Kaeru | OnlineService::Other)
    }

    pub fn relay(&self) -> bool {
        self.service == OnlineService::Citrine
    }

    pub fn gts_here(&self) -> bool {
        !cfg!(ds_wifi) || self.relay()
    }

    pub fn set_service(&mut self, service: OnlineService) {
        if service != self.service {
            self.service = service;
            self.dns = None;
        }
    }

    pub fn set_dns(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            self.dns = None;
        } else if Some(text) == OnlineService::Kaeru.preset_dns() {
            self.reset_dns();
        } else {
            if self.service == OnlineService::Kaeru {
                self.service = OnlineService::Other;
            }
            self.dns = Some(text.to_owned());
        }
    }

    pub fn reset_dns(&mut self) {
        self.service = OnlineService::Kaeru;
        self.dns = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_install_talks_to_no_service() {
        assert_eq!(DsOnline::default().dns(), None);
    }

    #[test]
    fn kaeru_uses_its_preset_until_the_address_is_changed() {
        let mut online = DsOnline::default();
        online.set_service(OnlineService::Kaeru);
        assert_eq!(online.dns(), Some(Ipv4Addr::new(178, 62, 43, 212)));
        online.set_dns("20.104.25.104");
        assert_eq!(online.service, OnlineService::Other);
        assert_eq!(online.dns(), Some(Ipv4Addr::new(20, 104, 25, 104)));
        online.set_dns("178.62.43.212");
        assert_eq!(online.service, OnlineService::Kaeru);
        assert_eq!(online.dns, None);
    }

    #[test]
    fn reset_returns_an_edited_address_to_kaeru() {
        let mut online = DsOnline::default();
        online.set_service(OnlineService::Kaeru);
        online.set_dns("20.104.25.104");
        online.reset_dns();
        assert_eq!(online.service, OnlineService::Kaeru);
        assert_eq!(online.dns_text(), "178.62.43.212");
    }

    #[test]
    fn clearing_the_address_keeps_the_service() {
        let mut online = DsOnline::default();
        online.set_service(OnlineService::Other);
        online.set_dns("1.2.3.4");
        online.set_dns("  ");
        assert_eq!(online.service, OnlineService::Other);
        assert_eq!(online.dns(), None);
    }

    #[test]
    fn other_needs_a_valid_address() {
        let mut online = DsOnline::default();
        online.set_service(OnlineService::Other);
        assert_eq!(online.dns(), None);
        online.set_dns("not an address");
        assert_eq!(online.dns(), None);
        online.set_dns(" 1.2.3.4 ");
        assert_eq!(online.dns(), Some(Ipv4Addr::new(1, 2, 3, 4)));
    }

    #[test]
    fn the_citrine_relay_needs_no_address() {
        let mut online = DsOnline::default();
        online.set_service(OnlineService::Citrine);
        assert!(online.relay());
        assert_eq!(online.dns(), None);
        online.set_service(OnlineService::Kaeru);
        assert!(!online.relay());
    }

    #[test]
    fn switching_service_drops_the_typed_address() {
        let mut online = DsOnline::default();
        online.set_service(OnlineService::Other);
        online.set_dns("1.2.3.4");
        online.set_service(OnlineService::Kaeru);
        assert_eq!(online.dns_text(), "178.62.43.212");
    }
}
