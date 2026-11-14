use super::Online;
use citrine_gb_net::{ClientMsg, GTS_PROTOCOL, GtsDeposit, GtsListing, ServerMsg};

pub const GTS_GENERATION: u8 = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GtsQuery {
    pub species: Option<u16>,
    pub page: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GtsPage {
    pub query: GtsQuery,
    pub total: u32,
    pub listings: Vec<GtsListing>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GtsSimNote {
    Searched,
    TradedWith(String),
}

#[derive(Default)]
pub(super) struct GtsCache {
    mine: Option<Vec<GtsDeposit>>,
    mine_stale: bool,
    asked_mine: bool,
    page: Option<GtsPage>,
    asked_page: Option<GtsQuery>,
    off: bool,
    traded: Vec<GtsDeposit>,
    simulated: bool,
    told_simulated: bool,
    sim_notes: Vec<GtsSimNote>,
}

impl Online {
    pub(super) fn gts_reset(&mut self) {
        self.gts = GtsCache::default();
    }

    pub(super) fn gts_disconnected(&mut self) {
        self.gts.asked_mine = false;
        self.gts.asked_page = None;
        self.gts.mine_stale = true;
    }

    pub(super) fn gts_refused(&mut self) {
        self.gts.off = true;
        self.gts.asked_mine = false;
        self.gts.asked_page = None;
    }

    pub(super) fn gts_receive(&mut self, msg: ServerMsg) -> Option<ServerMsg> {
        match msg {
            ServerMsg::GtsMine(mine) => {
                self.gts.mine = Some(mine);
                self.gts.mine_stale = false;
                self.gts.asked_mine = false;
                None
            }
            ServerMsg::GtsListings {
                generation,
                species,
                page,
                total,
                listings,
                simulated,
            } => {
                self.gts.simulated = simulated;
                if generation == GTS_GENERATION {
                    let query = GtsQuery { species, page };
                    if self.gts.asked_page == Some(query) {
                        self.gts.asked_page = None;
                    }
                    self.gts.page = Some(GtsPage {
                        query,
                        total,
                        listings,
                    });
                }
                None
            }
            ServerMsg::GtsTraded(deposit) => {
                let mine = self.gts.mine.get_or_insert_with(Vec::new);
                match mine.iter_mut().find(|d| same_deposit(d, &deposit)) {
                    Some(old) => *old = deposit.clone(),
                    None => mine.push(deposit.clone()),
                }
                self.gts.traded.push(deposit);
                None
            }
            ServerMsg::GtsSimulated => {
                self.gts.simulated = true;
                if !self.gts.told_simulated {
                    self.gts.told_simulated = true;
                    self.gts.sim_notes.push(GtsSimNote::Searched);
                }
                None
            }
            ServerMsg::GtsTradedWithSim { trainer, .. } => {
                self.gts.told_simulated = true;
                self.gts.sim_notes.push(GtsSimNote::TradedWith(trainer));
                None
            }
            ServerMsg::Gts { .. } => {
                self.gts.mine_stale = true;
                Some(msg)
            }
            other => Some(other),
        }
    }

    pub fn gts_open(&self) -> bool {
        self.is_connected()
            && self.account().is_some()
            && self
                .info()
                .is_some_and(|info| info.protocol >= GTS_PROTOCOL)
            && !self.gts.off
    }

    pub fn gts_off(&self) -> bool {
        self.gts.off || self.info().is_some_and(|info| info.protocol < GTS_PROTOCOL)
    }

    pub fn gts_mine(&self) -> Option<&[GtsDeposit]> {
        self.gts.mine.as_deref()
    }

    pub fn gts_page(&self) -> Option<&GtsPage> {
        self.gts.page.as_ref()
    }

    pub fn gts_loading(&self) -> bool {
        self.gts.asked_mine || self.gts.asked_page.is_some()
    }

    pub fn gts_want_mine(&mut self) {
        if self.gts.asked_mine || (self.gts.mine.is_some() && !self.gts.mine_stale) {
            return;
        }
        if self.gts_open() && self.send(ClientMsg::GtsMine) {
            self.gts.asked_mine = true;
        }
    }

    pub fn gts_browse(&mut self, query: GtsQuery) {
        let shown = self.gts.page.as_ref().map(|page| page.query);
        if shown == Some(query) || self.gts.asked_page == Some(query) {
            return;
        }
        self.gts_ask_page(query);
    }

    pub fn gts_reload(&mut self, query: GtsQuery) {
        self.gts.mine_stale = true;
        self.gts_want_mine();
        if self.gts.asked_page != Some(query) {
            self.gts_ask_page(query);
        }
    }

    pub fn take_gts_traded(&mut self) -> Vec<GtsDeposit> {
        std::mem::take(&mut self.gts.traded)
    }

    pub fn take_gts_sim_notes(&mut self) -> Vec<GtsSimNote> {
        std::mem::take(&mut self.gts.sim_notes)
    }

    pub fn gts_simulated(&self) -> bool {
        self.gts.simulated
    }

    fn gts_ask_page(&mut self, query: GtsQuery) {
        if self.gts_open()
            && self.send(ClientMsg::GtsBrowse {
                generation: GTS_GENERATION,
                species: query.species,
                page: query.page,
            })
        {
            self.gts.asked_page = Some(query);
        }
    }
}

fn same_deposit(a: &GtsDeposit, b: &GtsDeposit) -> bool {
    a.generation == b.generation
        && a.listing.deposited == b.listing.deposited
        && a.listing.offered == b.listing.offered
}

#[cfg(test)]
mod tests {
    use super::super::tests::{sent, signed_in};
    use super::*;
    use crate::online::RelayId;
    use citrine_gb_net::{ErrorCode, GtsPokemon, GtsState, GtsWanted};

    fn pokemon(species: u16, nickname: &str) -> GtsPokemon {
        GtsPokemon {
            species,
            level: 5,
            gender: 1,
            shiny: false,
            nickname: nickname.into(),
            language: 5,
        }
    }

    fn listing() -> GtsListing {
        GtsListing {
            offered: pokemon(155, "Feuri"),
            wanted: GtsWanted {
                species: 156,
                gender: 3,
                min_level: 0,
                max_level: 0,
            },
            trainer: "Clara".into(),
            game: 7,
            country: 0,
            deposited: 1_793_995_587,
            friend: None,
            simulated: false,
        }
    }

    fn waiting() -> GtsDeposit {
        GtsDeposit {
            generation: 4,
            listing: listing(),
            state: GtsState::Waiting,
        }
    }

    fn deliver(online: &mut Online, msg: ServerMsg) {
        online.receive(msg, web_time::Instant::now());
    }

    #[test]
    fn mine_is_asked_once_and_again_after_a_ds_call() {
        let (mut online, wire, _) = signed_in(&RelayId::random());
        sent(&wire);
        online.gts_want_mine();
        online.gts_want_mine();
        assert_eq!(sent(&wire), vec![ClientMsg::GtsMine]);
        deliver(&mut online, ServerMsg::GtsMine(vec![waiting()]));
        online.gts_want_mine();
        assert!(sent(&wire).is_empty());
        assert_eq!(online.gts_mine().map(<[_]>::len), Some(1));
        deliver(
            &mut online,
            ServerMsg::Gts {
                call: 1,
                reply: citrine_gb_net::GtsReply::Status(200),
            },
        );
        online.gts_want_mine();
        assert_eq!(sent(&wire), vec![ClientMsg::GtsMine]);
    }

    #[test]
    fn a_trade_replaces_the_waiting_deposit_and_is_told_once() {
        let (mut online, _wire, _) = signed_in(&RelayId::random());
        deliver(&mut online, ServerMsg::GtsMine(vec![waiting()]));
        let traded = GtsDeposit {
            state: GtsState::Traded {
                at: 1_793_995_670,
                received: pokemon(155, "Feuir"),
                trainer: "Ren".into(),
                simulated: false,
            },
            ..waiting()
        };
        deliver(&mut online, ServerMsg::GtsTraded(traded.clone()));
        assert_eq!(online.gts_mine(), Some(&[traded.clone()][..]));
        assert_eq!(online.take_gts_traded(), vec![traded]);
        assert!(online.take_gts_traded().is_empty());
    }

    #[test]
    fn a_page_is_asked_once_per_query() {
        let (mut online, wire, _) = signed_in(&RelayId::random());
        sent(&wire);
        let query = GtsQuery {
            species: Some(155),
            page: 0,
        };
        online.gts_browse(query);
        online.gts_browse(query);
        assert_eq!(
            sent(&wire),
            vec![ClientMsg::GtsBrowse {
                generation: 4,
                species: Some(155),
                page: 0
            }]
        );
        assert!(online.gts_loading());
        deliver(
            &mut online,
            ServerMsg::GtsListings {
                generation: 4,
                species: Some(155),
                page: 0,
                total: 1,
                listings: vec![listing()],
                simulated: false,
            },
        );
        assert!(!online.gts_loading());
        online.gts_browse(query);
        assert!(sent(&wire).is_empty());
        assert_eq!(online.gts_page().map(|p| p.total), Some(1));
    }

    #[test]
    fn simulated_trainers_are_told_once_per_session_and_each_trade() {
        let (mut online, _wire, _) = signed_in(&RelayId::random());
        assert!(!online.gts_simulated());
        deliver(&mut online, ServerMsg::GtsSimulated);
        deliver(&mut online, ServerMsg::GtsSimulated);
        assert!(online.gts_simulated());
        assert_eq!(online.take_gts_sim_notes(), vec![GtsSimNote::Searched]);
        deliver(
            &mut online,
            ServerMsg::GtsTradedWithSim {
                trainer: "Ren".into(),
                received: pokemon(155, "Feuri"),
            },
        );
        assert_eq!(
            online.take_gts_sim_notes(),
            vec![GtsSimNote::TradedWith("Ren".into())]
        );
        assert!(online.take_gts_sim_notes().is_empty());
        deliver(
            &mut online,
            ServerMsg::GtsListings {
                generation: 4,
                species: None,
                page: 0,
                total: 0,
                listings: Vec::new(),
                simulated: false,
            },
        );
        assert!(!online.gts_simulated());
    }

    #[test]
    fn a_relay_without_a_gts_is_asked_nothing_more() {
        let (mut online, wire, _) = signed_in(&RelayId::random());
        sent(&wire);
        online.gts_want_mine();
        deliver(&mut online, ServerMsg::Error(ErrorCode::GtsOff));
        assert!(online.gts_off());
        assert!(!online.gts_open());
        sent(&wire);
        online.gts_want_mine();
        online.gts_browse(GtsQuery::default());
        assert!(sent(&wire).is_empty());
    }
}
