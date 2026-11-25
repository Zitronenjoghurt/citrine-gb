use super::referee::{Answer, Peer, Referee};
use super::summary::{Left, Summary};
use super::sync::LastStored;
use super::{Hold, Pokewalker, seed};
use crate::online::cloud::{WalkerRemote, WalkerReply, WalkerRequest};
use citrine_gb_net::PeerKey;
use citrine_gb_walker::Walker;
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"CWLN";
#[cfg(not(target_arch = "wasm32"))]
const MAX_OFFERS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Local,
    Cloud,
    Leave,
    Reclaim,
    Me,
    Lan(PeerKey),
    Away,
}

impl Route {
    pub fn uses_cloud(self) -> bool {
        matches!(self, Self::Cloud | Self::Leave | Self::Reclaim)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub chain: u64,
    pub remote: WalkerRemote,
    pub holder: PeerKey,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Announce {
    Walker {
        chain: u64,
        remote: WalkerRemote,
        holder: PeerKey,
    },
    Cloud,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    pub steps: u32,
    pub pokemon: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct LanState {
    leaving: Option<u64>,
    referee: Option<Referee>,
    #[serde(default)]
    away: Option<String>,
}

impl LanState {
    pub(super) fn to_bytes(&self) -> Vec<u8> {
        let meta = serde_json::to_vec(self).unwrap_or_default();
        let len = u32::try_from(meta.len()).unwrap_or(u32::MAX).to_le_bytes();
        let data = self.referee.as_ref().map_or(&[][..], Referee::data);
        [&MAGIC[..], &len, &meta, data].concat()
    }

    pub(super) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let rest = bytes.strip_prefix(&MAGIC[..])?;
        let (len, rest) = rest.split_first_chunk::<4>()?;
        let len = usize::try_from(u32::from_le_bytes(*len)).ok()?;
        let (meta, data) = rest.split_at_checked(len)?;
        let mut state: Self = serde_json::from_slice(meta).ok()?;
        if let Some(referee) = &mut state.referee {
            referee.set_data(data.to_vec());
        }
        Some(state)
    }
}

impl Pokewalker {
    pub fn set_peer(&mut self, peer: Option<Peer>) {
        self.peer = peer;
    }

    pub fn route(&self) -> Route {
        self.route
    }

    pub fn offers(&self) -> &[Offer] {
        &self.offers
    }

    pub fn holder_on_cloud(&self) -> bool {
        matches!(self.route, Route::Lan(holder) if self.on_cloud == Some(holder))
    }

    #[cfg(test)]
    pub(crate) fn lan_referee(&self) -> Option<&Referee> {
        self.lan.referee.as_ref()
    }

    pub(crate) fn follow_route(&mut self, cloud_on: bool) -> Route {
        self.load();
        self.cloud_on = cloud_on;
        if cloud_on && self.lan.leaving.is_some() && !self.sync.is_busy() {
            self.lan.leaving = None;
            self.save_lan();
        }
        if !cloud_on
            && self.route == Route::Cloud
            && self.peer.is_some()
            && *self.sync.hold() == Hold::Here
            && self.sync.listed().is_some()
        {
            self.lan.leaving = Some(seed());
            self.save_lan();
        }
        if cloud_on && self.lan.away.take().is_some() {
            self.save_lan();
        }
        if !cloud_on
            && self.route == Route::Cloud
            && self.lan.referee.is_none()
            && let Hold::Elsewhere(device) = self.sync.hold()
        {
            self.lan.away = Some(device.clone());
            self.save_lan();
        }
        if !cloud_on && self.lan.leaving.is_none() {
            self.cloud_listed = None;
        }
        if !cloud_on
            && self.lan.leaving.is_none()
            && self.lan.referee.is_none()
            && self.lan.away.is_none()
            && self.sync.last_stored().is_none()
            && self.peer.is_some()
        {
            let data = self.copy.clone();
            self.start_referee(seed(), 1, data);
        }
        let route = self.pick(cloud_on);
        if let Route::Lan(holder) = route
            && self.lan.referee.is_none()
            && let (Some(mark), Some(me)) = (self.mark(), self.peer.as_ref().map(|peer| peer.key))
            && let Some(Some(listed)) = self.cloud_listed.clone()
        {
            let remote = WalkerRemote {
                version: 0,
                ..listed
            };
            self.lan.referee = Some(Referee::follower(mark.chain, me, remote, holder));
            self.save_lan();
        }
        if route != self.route {
            self.switch(route);
        }
        if route == Route::Leave {
            self.sync.resend();
        }
        route
    }

    fn mark(&self) -> Option<Left> {
        let remote = self.cloud_listed.as_ref()?.as_ref()?;
        serde_json::from_str::<Summary>(&remote.name).ok()?.left
    }

    fn pick(&self, cloud_on: bool) -> Route {
        let referee = self.lan.referee.as_ref();
        if cloud_on && self.cloud_listed.is_none() && referee.is_some() {
            return self.route;
        }
        if cloud_on {
            let Some(mark) = self.mark() else {
                return if self.lan.leaving.is_some() {
                    Route::Leave
                } else {
                    Route::Cloud
                };
            };
            return match referee.filter(|referee| referee.chain() == mark.chain) {
                Some(referee) if referee.refereeing() => Route::Reclaim,
                Some(referee) => Route::Lan(referee.holder()),
                None => Route::Lan(mark.key),
            };
        }
        if self.lan.leaving.is_some() {
            return Route::Leave;
        }
        match referee {
            Some(referee) if referee.refereeing() => Route::Me,
            Some(referee) => Route::Lan(referee.holder()),
            None if self.lan.away.is_some() => Route::Away,
            None => Route::Local,
        }
    }

    fn switch(&mut self, route: Route) {
        let old = std::mem::replace(&mut self.route, route);
        if route == Route::Local {
            self.sync.off();
            return;
        }
        if route == Route::Away {
            self.sync.off();
            if let Some(device) = self.lan.away.clone() {
                self.sync.stay_elsewhere(device);
            }
            return;
        }
        if old == Route::Local {
            self.sync.on();
        } else {
            self.sync.rewire();
        }
        if route == Route::Cloud && self.lan.referee.take().is_some() {
            self.save_lan();
        }
        if route.uses_cloud()
            && let Some(listed) = self.cloud_listed.clone()
        {
            self.sync.reply(WalkerReply::Listed(listed), &self.copy);
        }
        if let (Route::Lan(_), Some(referee)) = (route, &self.lan.referee) {
            let listed = referee.remote().clone();
            self.sync
                .reply(WalkerReply::Listed(Some(listed)), &self.copy);
        }
        match route {
            Route::Leave => self.sync.resend(),
            Route::Reclaim => self.sync.overwrite(),
            _ => {}
        }
    }

    fn start_referee(&mut self, chain: u64, version: u64, data: Vec<u8>) {
        let Some(peer) = self.peer.clone() else {
            return;
        };
        let referee = Referee::start(
            chain,
            &peer,
            version,
            data,
            String::new(),
            crate::storage::now_unix(),
        );
        let last_stored = Some(LastStored {
            version,
            hash: referee.remote().hash,
        });
        self.sync.set_last_stored(last_stored);
        self.store.set_last_stored(last_stored);
        self.lan.referee = Some(referee);
        self.save_lan();
    }

    fn save_lan(&mut self) {
        let empty = self.lan == LanState::default();
        self.store
            .set_lan((!empty).then(|| self.lan.to_bytes()).as_deref());
    }

    pub(super) fn load_lan(&mut self) {
        self.lan = self
            .store
            .lan()
            .and_then(|bytes| LanState::from_bytes(&bytes))
            .unwrap_or_default();
    }

    pub(super) fn heard_cloud(&mut self, reply: &WalkerReply) -> bool {
        if let WalkerReply::Listed(listed) = reply {
            self.cloud_listed = Some(listed.clone());
        }
        self.route.uses_cloud()
    }

    pub(super) fn leaving_label(&self, summary: &mut Summary) {
        if self.route != Route::Leave {
            return;
        }
        if let (Some(chain), Some(peer)) = (self.lan.leaving, &self.peer) {
            summary.left = Some(Left {
                key: peer.key,
                chain,
            });
        }
    }

    pub(super) fn sent_cloud(&mut self, request: &WalkerRequest) {
        if self.route == Route::Leave
            && let WalkerRequest::Put { data, .. } = request
        {
            self.leave_data = Some(data.clone());
        }
    }

    pub(super) fn stored_cloud(&mut self, remote: &WalkerRemote) {
        if self.route != Route::Leave {
            return;
        }
        let (Some(chain), Some(data)) = (self.lan.leaving.take(), self.leave_data.take()) else {
            return;
        };
        self.start_referee(chain, remote.version, data);
    }

    pub(super) fn left_cloud_elsewhere(&mut self) {
        if self.route == Route::Leave && matches!(self.sync.hold(), Hold::Elsewhere(_)) {
            self.lan.leaving = None;
            self.leave_data = None;
            self.save_lan();
        }
    }

    pub(crate) fn settle_here(&mut self) -> Option<Walker> {
        if self.route != Route::Me {
            return None;
        }
        let peer = self.peer.clone()?;
        let mut adopted = None;
        for _ in 0..4 {
            let listed = self.lan.referee.as_ref()?.remote().clone();
            if let Some(walker) = self.reply_any(WalkerReply::Listed(Some(listed))) {
                adopted = Some(walker);
            }
            let Some(request) = self.next_request() else {
                break;
            };
            let now = crate::storage::now_unix();
            let answer = self.lan.referee.as_mut()?.answer(request, &peer, now);
            self.save_lan();
            let Answer::Reply(reply) = answer else {
                break;
            };
            if let Some(walker) = self.reply_any(reply) {
                adopted = Some(walker);
            }
        }
        adopted
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Pokewalker {
    pub(crate) fn answer_peer(&mut self, request: WalkerRequest, from: &Peer) -> Answer {
        self.load();
        match self.route {
            Route::Me | Route::Lan(_) => {}
            Route::Cloud => return Answer::UsesCloud,
            _ => return Answer::Reply(WalkerReply::Lost),
        }
        let Some(referee) = self.lan.referee.as_mut() else {
            return Answer::Reply(WalkerReply::Lost);
        };
        let answer = referee.answer(request, from, crate::storage::now_unix());
        self.save_lan();
        answer
    }

    pub(crate) fn save_before_answering(&mut self) -> Option<Walker> {
        self.load();
        if self.route != Route::Me || *self.sync.hold() != Hold::Here {
            return None;
        }
        if let super::Held::Home(walker) = &self.held
            && super::encode(walker, self.credited) != self.copy
        {
            self.persist_home();
        }
        self.settle_here()
    }

    pub(crate) fn lan_request(&mut self) -> Option<(PeerKey, WalkerRequest)> {
        let Route::Lan(holder) = self.route else {
            return None;
        };
        if self.on_cloud == Some(holder) {
            return None;
        }
        Some((holder, self.next_request()?))
    }

    pub(crate) fn lan_reply(
        &mut self,
        from: PeerKey,
        request: Option<&WalkerRequest>,
        answer: Answer,
    ) -> Option<Walker> {
        if self.route != Route::Lan(from) {
            return None;
        }
        match answer {
            Answer::Reply(reply) => {
                if let WalkerReply::Fetched { remote, .. } = &reply
                    && self.taking.take().is_some()
                {
                    let copy = self.live_copy().unwrap_or_else(|| self.copy.clone());
                    let by = remote.device.clone();
                    self.keep_copy(&copy, super::KeptWhy::Replaced { by });
                }
                if let (WalkerReply::Stored(remote), Some(WalkerRequest::Put { data, .. })) =
                    (&reply, request)
                    && let Some(referee) = self.lan.referee.as_mut()
                    && referee.took(remote.clone(), data.clone())
                {
                    self.save_lan();
                }
                self.reply_any(reply)
            }
            Answer::Follow { remote, holder } => {
                let chain = self.lan.referee.as_ref()?.chain();
                if let Some(WalkerRequest::Put { data, .. }) = request
                    && self.take_over(chain, remote.clone(), holder, data.clone())
                {
                    return None;
                }
                self.sync.reply(WalkerReply::Lost, &self.copy);
                self.lan_listing(from, chain, remote, holder)
            }
            Answer::UsesCloud => {
                self.sync.reply(WalkerReply::Lost, &self.copy);
                self.lan_holder_on_cloud(from);
                None
            }
        }
    }

    pub(crate) fn lan_holder_on_cloud(&mut self, from: PeerKey) {
        if self.route != Route::Lan(from) {
            return;
        }
        self.on_cloud = Some(from);
        if self.sync.step_aside() {
            let copy = self.live_copy().unwrap_or_else(|| self.copy.clone());
            let holder = self.holder_name();
            self.keep_copy(&copy, super::KeptWhy::ChangedHere { holder });
        }
    }

    fn take_over(
        &mut self,
        chain: u64,
        remote: WalkerRemote,
        holder: PeerKey,
        data: Vec<u8>,
    ) -> bool {
        let me = self.peer.as_ref().map(|peer| peer.key);
        if me != Some(holder) || !matches!(self.route, Route::Lan(_)) {
            return false;
        }
        let Some(referee) = self
            .lan
            .referee
            .as_mut()
            .filter(|referee| referee.chain() == chain)
        else {
            return false;
        };
        if !referee.took(remote.clone(), data) {
            return false;
        }
        self.save_lan();
        self.sync.took(remote);
        self.store.set_last_stored(self.sync.last_stored());
        true
    }

    pub(crate) fn lan_listing(
        &mut self,
        from: PeerKey,
        chain: u64,
        remote: WalkerRemote,
        holder: PeerKey,
    ) -> Option<Walker> {
        self.load();
        let me = self.peer.as_ref()?.key;
        if holder == me {
            let copy = self.copy.clone();
            self.take_over(chain, remote, holder, copy);
            return None;
        }
        self.on_cloud = None;
        let same = self
            .lan
            .referee
            .as_ref()
            .is_some_and(|referee| referee.chain() == chain);
        if !same {
            self.offer(Offer {
                chain,
                remote,
                holder,
            });
            return None;
        }
        if self.lan.referee.as_mut()?.follow(chain, remote, holder) {
            self.save_lan();
        }
        if from != holder || self.route != Route::Lan(holder) {
            return None;
        }
        let listed = self.lan.referee.as_ref()?.remote().clone();
        self.reply_any(WalkerReply::Listed(Some(listed)))
    }

    pub(crate) fn announce(&self) -> Option<Announce> {
        if self.route == Route::Cloud && *self.sync.hold() == Hold::Here {
            return Some(Announce::Cloud);
        }
        if self.route != Route::Me || !self.has_walked() {
            return None;
        }
        let referee = self.lan.referee.as_ref()?;
        let (remote, holder) = referee.listing();
        Some(Announce::Walker {
            chain: referee.chain(),
            remote,
            holder,
        })
    }

    pub fn replaced_by_move(&mut self) -> Option<Replaced> {
        self.load();
        if !matches!(self.route, Route::Me | Route::Local) || !self.has_walked() {
            return None;
        }
        let super::Held::Home(walker) = &self.held else {
            return None;
        };
        let mut walker = Walker::clone(walker);
        let pokemon = Summary::of(&mut walker, super::StepsFrom::Nothing)
            .pokemon
            .map(|(name, _)| name);
        Some(Replaced {
            steps: walker.health().total_steps(),
            pokemon,
        })
    }

    fn has_walked(&self) -> bool {
        self.walked
            || matches!(&self.held, super::Held::Home(walker) if walker.health().total_steps() > 0)
    }

    pub(crate) fn lan_holder(&self) -> Option<PeerKey> {
        match self.route {
            Route::Lan(holder) if self.on_cloud != Some(holder) => Some(holder),
            _ => None,
        }
    }

    pub(crate) fn lan_declined(&mut self, from: PeerKey) {
        if self.route != Route::Lan(from) {
            return;
        }
        let Some((before, hold)) = self.taking.take() else {
            return;
        };
        self.lan = before;
        self.save_lan();
        self.sync.cancel_take(hold);
        let route = self.pick(self.cloud_on);
        if route != self.route {
            self.switch(route);
        }
    }

    fn offer(&mut self, offer: Offer) {
        if !matches!(
            self.route,
            Route::Me | Route::Lan(_) | Route::Local | Route::Away
        ) {
            return;
        }
        let known = self
            .offers
            .iter_mut()
            .find(|known| known.holder == offer.holder && known.chain == offer.chain);
        match known {
            Some(known) if known.remote.version < offer.remote.version => *known = offer,
            Some(_) => {}
            None => {
                self.offers.push(offer);
                if self.offers.len() > MAX_OFFERS {
                    self.offers.remove(0);
                }
            }
        }
    }

    pub fn take_offer(&mut self, holder: PeerKey) -> bool {
        self.load();
        let Some(me) = self.peer.as_ref().map(|peer| peer.key) else {
            return false;
        };
        let Some(at) = self.offers.iter().position(|offer| offer.holder == holder) else {
            return false;
        };
        if !matches!(
            self.route,
            Route::Me | Route::Lan(_) | Route::Local | Route::Away
        ) {
            return false;
        }
        let offer = self.offers.remove(at);
        self.taking = Some((self.lan.clone(), self.sync.hold().clone()));
        self.lan.away = None;
        self.lan.referee = Some(Referee::follower(offer.chain, me, offer.remote, holder));
        self.save_lan();
        self.switch(Route::Lan(holder));
        self.sync.start_taking();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::tests::{Cloud, Shared, pokewalker};
    use crate::walker::{KeptCard, KeptWhy};
    use citrine_gb_walker::Button;

    struct Dev {
        name: &'static str,
        peer: Peer,
        memory: Shared,
        walker: Pokewalker,
        cloud_on: bool,
    }

    fn dev(n: u8, name: &'static str, cloud_on: bool) -> Dev {
        let memory = Shared::default();
        let peer = Peer {
            key: citrine_gb_net::RelayKey([n; 32]),
            name: name.to_owned(),
        };
        let mut walker = pokewalker(&memory);
        walker.set_peer(Some(peer.clone()));
        Dev {
            name,
            peer,
            memory,
            walker,
            cloud_on,
        }
    }

    fn walk(dev: &mut Dev, steps: u32) {
        dev.walker.walker().unwrap().steps(steps);
        dev.walker.press(Button::Middle);
    }

    fn steps_of(dev: &mut Dev) -> u32 {
        dev.walker.walker().unwrap().today_steps()
    }

    fn mark_of(cloud: &Cloud) -> Option<Left> {
        serde_json::from_str::<Summary>(&cloud.name).ok()?.left
    }

    fn at(devs: &[Dev], key: PeerKey) -> usize {
        devs.iter()
            .position(|dev| dev.peer.key == key)
            .expect("a device on the LAN")
    }

    fn pair(devs: &mut [Dev], i: usize, j: usize) -> (&mut Dev, &mut Dev) {
        assert_ne!(i, j);
        if i < j {
            let (a, b) = devs.split_at_mut(j);
            (&mut a[i], &mut b[0])
        } else {
            let (a, b) = devs.split_at_mut(i);
            (&mut b[0], &mut a[j])
        }
    }

    fn listen(devs: &mut [Dev], i: usize, j: usize) {
        let (me, other) = pair(devs, i, j);
        match other.walker.lan_referee() {
            Some(referee) => {
                let (remote, holder) = referee.listing();
                let chain = referee.chain();
                me.walker.lan_listing(other.peer.key, chain, remote, holder);
            }
            None => me.walker.lan_holder_on_cloud(other.peer.key),
        }
    }

    fn step(devs: &mut [Dev], i: usize, cloud: Option<&mut Cloud>) {
        let on = devs[i].cloud_on;
        let route = devs[i].walker.follow_route(on);
        if let Some(cloud) = cloud
            && (on || route.uses_cloud())
        {
            let name = devs[i].name;
            cloud.serve(name, &mut devs[i].walker);
            devs[i].walker.follow_route(on);
        }
        devs[i].walker.settle_here();
        for _ in 0..8 {
            let route = devs[i].walker.follow_route(on);
            let Route::Lan(holder) = route else {
                break;
            };
            let j = at(devs, holder);
            listen(devs, i, j);
            let Some((to, request)) = devs[i].walker.lan_request() else {
                if devs[i].walker.follow_route(on) == route {
                    break;
                }
                continue;
            };
            let j = at(devs, to);
            let (me, other) = pair(devs, i, j);
            let answer = other.walker.answer_peer(request.clone(), &me.peer);
            me.walker.lan_reply(to, Some(&request), answer);
        }
        devs[i].walker.follow_route(on);
        devs[i].walker.settle_here();
    }

    fn holder_on_cloud(cloud: &mut Cloud) -> Dev {
        let mut phone = dev(1, "Pixel 8", true);
        phone.walker.cloud_on();
        phone.walker.take();
        let mut devs = vec![phone];
        step(&mut devs, 0, Some(cloud));
        walk(&mut devs[0], 77);
        step(&mut devs, 0, Some(cloud));
        let phone = devs.pop().unwrap();
        assert_eq!(phone.walker.hold(), &Hold::Here);
        assert_eq!(phone.walker.route(), Route::Cloud);
        phone
    }

    fn left_for_the_lan(cloud: &mut Cloud) -> Vec<Dev> {
        let mut devs = vec![holder_on_cloud(cloud)];
        devs[0].cloud_on = false;
        step(&mut devs, 0, Some(cloud));
        assert_eq!(devs[0].walker.route(), Route::Me);
        devs
    }

    #[test]
    fn cloud_saves_off_marks_the_cloud_copy_before_the_lan_may_take_it() {
        let mut cloud = Cloud::default();
        let mut devs = vec![holder_on_cloud(&mut cloud)];
        let version = cloud.version;
        devs[0].cloud_on = false;
        assert_eq!(devs[0].walker.follow_route(false), Route::Leave);
        let tab = dev(2, "Galaxy Tab", false);
        assert_eq!(
            devs[0].walker.answer_peer(WalkerRequest::Fetch, &tab.peer),
            Answer::Reply(WalkerReply::Lost)
        );
        step(&mut devs, 0, Some(&mut cloud));
        assert_eq!(cloud.version, version + 1);
        let left = mark_of(&cloud).expect("a marked cloud copy");
        assert_eq!(left.key, devs[0].peer.key);
        assert_eq!(devs[0].walker.route(), Route::Me);
        assert_eq!(devs[0].walker.hold(), &Hold::Here);
        assert!(matches!(
            devs[0].walker.answer_peer(WalkerRequest::Fetch, &tab.peer),
            Answer::Reply(WalkerReply::Fetched { .. })
        ));
    }

    #[test]
    fn a_cloud_take_that_beats_the_mark_keeps_the_walker_on_the_cloud() {
        let mut cloud = Cloud::default();
        let mut devs = vec![
            holder_on_cloud(&mut cloud),
            dev(3, "Clara's MacBook Pro", true),
        ];
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.hold(), &Hold::Elsewhere("Pixel 8".into()));
        devs[0].cloud_on = false;
        assert_eq!(devs[0].walker.follow_route(false), Route::Leave);
        let mark = devs[0].walker.cloud_request().expect("the marked put");
        assert!(matches!(mark, WalkerRequest::Put { .. }));
        devs[1].walker.take();
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.hold(), &Hold::Here);
        devs[0]
            .walker
            .cloud_reply(WalkerReply::Refused { changed: true });
        step(&mut devs, 0, Some(&mut cloud));
        assert_eq!(cloud.device, "Clara's MacBook Pro");
        assert_eq!(mark_of(&cloud), None);
        assert!(devs[0].walker.lan_referee().is_none());
        assert_eq!(devs[0].walker.route(), Route::Local);
        let mac = devs[1].peer.clone();
        assert_eq!(
            devs[0].walker.answer_peer(WalkerRequest::Fetch, &mac),
            Answer::Reply(WalkerReply::Lost)
        );
    }

    #[test]
    fn offline_between_the_switch_and_the_mark_the_lan_waits() {
        let mut cloud = Cloud::default();
        let mut devs = vec![holder_on_cloud(&mut cloud)];
        let version = cloud.version;
        devs[0].cloud_on = false;
        assert_eq!(devs[0].walker.follow_route(false), Route::Leave);
        let tab = dev(2, "Galaxy Tab", false);
        let memory = devs[0].memory.clone();
        let peer = devs[0].peer.clone();
        devs[0].walker = pokewalker(&memory);
        devs[0].walker.set_peer(Some(peer));
        assert_eq!(devs[0].walker.follow_route(false), Route::Leave);
        assert_eq!(
            devs[0].walker.answer_peer(WalkerRequest::Fetch, &tab.peer),
            Answer::Reply(WalkerReply::Lost)
        );
        assert_eq!(cloud.version, version);
        step(&mut devs, 0, Some(&mut cloud));
        assert_eq!(cloud.version, version + 1);
        assert!(mark_of(&cloud).is_some());
        assert_eq!(devs[0].walker.route(), Route::Me);
        assert_eq!(steps_of(&mut devs[0]), 77);
    }

    #[test]
    fn a_lan_take_wins_over_the_stale_cloud_copy_on_the_way_back() {
        let mut cloud = Cloud::default();
        let mut devs = left_for_the_lan(&mut cloud);
        devs.push(dev(2, "Galaxy Tab", false));
        step(&mut devs, 1, None);
        assert_eq!(devs[1].walker.route(), Route::Me);
        listen(&mut devs, 1, 0);
        let phone = devs[0].peer.key;
        assert!(devs[1].walker.take_offer(phone));
        step(&mut devs, 1, None);
        assert_eq!(devs[1].walker.hold(), &Hold::Here);
        assert_eq!(steps_of(&mut devs[1]), 77);
        assert_eq!(devs[1].memory.borrow().kept.len(), 1);
        step(&mut devs, 0, None);
        assert_eq!(devs[0].walker.hold(), &Hold::Elsewhere("Galaxy Tab".into()));
        walk(&mut devs[1], 5);
        step(&mut devs, 1, None);
        let fetches = cloud.fetches;
        devs[1].cloud_on = true;
        step(&mut devs, 1, Some(&mut cloud));
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(cloud.fetches, fetches);
        assert_eq!(cloud.device, "Galaxy Tab");
        assert_eq!(mark_of(&cloud), None);
        assert_eq!(devs[1].walker.route(), Route::Cloud);
        assert_eq!(devs[1].walker.hold(), &Hold::Here);
        assert_eq!(steps_of(&mut devs[1]), 82);
        step(&mut devs, 0, None);
        assert!(devs[0].walker.holder_on_cloud());
    }

    #[test]
    fn a_cloud_device_follows_the_mark_to_the_lan_holder() {
        let mut cloud = Cloud::default();
        let mut devs = left_for_the_lan(&mut cloud);
        devs.push(dev(3, "Clara's MacBook Pro", true));
        step(&mut devs, 1, Some(&mut cloud));
        step(&mut devs, 1, Some(&mut cloud));
        let phone = devs[0].peer.key;
        assert_eq!(devs[1].walker.route(), Route::Lan(phone));
        assert_eq!(devs[1].walker.hold(), &Hold::Elsewhere("Pixel 8".into()));
        let (version, fetches) = (cloud.version, cloud.fetches);
        devs[1].walker.take();
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.hold(), &Hold::Here);
        assert_eq!(steps_of(&mut devs[1]), 77);
        assert_eq!(cloud.fetches, fetches);
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(cloud.version, version + 1);
        assert_eq!(cloud.device, "Clara's MacBook Pro");
        assert_eq!(mark_of(&cloud), None);
        assert_eq!(devs[1].walker.route(), Route::Cloud);
        step(&mut devs, 0, None);
        assert_eq!(
            devs[0].walker.hold(),
            &Hold::Elsewhere("Clara's MacBook Pro".into())
        );
    }

    #[test]
    fn the_lan_state_survives_a_restart() {
        let peer = Peer {
            key: citrine_gb_net::RelayKey([4; 32]),
            name: "Pixel 8".into(),
        };
        let state = LanState {
            leaving: Some(9),
            referee: Some(Referee::start(9, &peer, 3, vec![1, 2, 3], "x".into(), 5)),
            away: Some("Pixel 8".into()),
        };
        assert_eq!(LanState::from_bytes(&state.to_bytes()), Some(state));
        assert_eq!(LanState::from_bytes(b"CWLN"), None);
    }

    #[test]
    fn move_here_over_a_used_walker_says_what_it_replaces() {
        let mut devs = vec![dev(1, "Pixel 8", false), dev(2, "Galaxy Tab", false)];
        step(&mut devs, 0, None);
        step(&mut devs, 1, None);
        walk(&mut devs[0], 77);
        step(&mut devs, 0, None);
        assert_eq!(devs[1].walker.replaced_by_move(), None);
        walk(&mut devs[1], 12);
        step(&mut devs, 1, None);
        assert_eq!(
            devs[1].walker.replaced_by_move(),
            Some(Replaced {
                steps: 12,
                pokemon: None,
            })
        );
        devs[1].walker.walker().unwrap().steps(5);
        listen(&mut devs, 1, 0);
        let phone = devs[0].peer.key;
        assert!(devs[1].walker.take_offer(phone));
        step(&mut devs, 1, None);
        assert_eq!(steps_of(&mut devs[1]), 77);
        assert_eq!(devs[1].memory.borrow().kept.len(), 1);
        assert_eq!(
            devs[1].walker.kept_card(),
            Some(KeptCard {
                steps: 17,
                why: KeptWhy::Replaced {
                    by: "Pixel 8".into()
                },
                can_use: true,
            })
        );
        step(&mut devs, 0, None);
        assert_eq!(devs[0].walker.replaced_by_move(), None);
    }

    #[test]
    fn a_device_that_leaves_the_cloud_without_the_walker_stays_read_only() {
        let mut cloud = Cloud::default();
        let mut devs = vec![
            holder_on_cloud(&mut cloud),
            dev(3, "Clara's MacBook Pro", true),
        ];
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.hold(), &Hold::Elsewhere("Pixel 8".into()));
        devs[1].cloud_on = false;
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.route(), Route::Away);
        assert!(devs[1].walker.read_only());
        assert!(devs[1].walker.announce().is_none());
        let away = super::super::Place::Away {
            device: "Pixel 8".into(),
        };
        assert_eq!(devs[1].walker.place(), away);
        devs[1].walker.take();
        assert_eq!(devs[1].walker.hold(), &Hold::Elsewhere("Pixel 8".into()));
        let mut again = pokewalker(&devs[1].memory);
        again.set_peer(Some(devs[1].peer.clone()));
        assert_eq!(again.follow_route(false), Route::Away);
        assert!(again.read_only());
        assert_eq!(again.place(), away);
    }

    #[test]
    fn the_holder_hands_the_walker_to_a_device_that_left_the_cloud() {
        let mut cloud = Cloud::default();
        let mut devs = vec![
            holder_on_cloud(&mut cloud),
            dev(3, "Clara's MacBook Pro", true),
        ];
        step(&mut devs, 1, Some(&mut cloud));
        devs[1].cloud_on = false;
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.route(), Route::Away);
        devs[0].cloud_on = false;
        step(&mut devs, 0, Some(&mut cloud));
        assert_eq!(devs[0].walker.route(), Route::Me);
        listen(&mut devs, 1, 0);
        let phone = devs[0].peer.key;
        assert!(devs[1].walker.take_offer(phone));
        step(&mut devs, 1, None);
        assert_eq!(devs[1].walker.route(), Route::Me);
        assert_eq!(devs[1].walker.hold(), &Hold::Here);
        assert!(!devs[1].walker.read_only());
        assert_eq!(steps_of(&mut devs[1]), 77);
        step(&mut devs, 0, None);
        assert_eq!(
            devs[0].walker.hold(),
            &Hold::Elsewhere("Clara's MacBook Pro".into())
        );
    }

    #[test]
    fn cloud_saves_on_again_ends_the_read_only_copy() {
        let mut cloud = Cloud::default();
        let mut devs = vec![
            holder_on_cloud(&mut cloud),
            dev(3, "Clara's MacBook Pro", true),
        ];
        step(&mut devs, 1, Some(&mut cloud));
        devs[1].cloud_on = false;
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.route(), Route::Away);
        devs[1].cloud_on = true;
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.route(), Route::Cloud);
        assert_eq!(devs[1].walker.hold(), &Hold::Elsewhere("Pixel 8".into()));
        devs[1].walker.take();
        step(&mut devs, 1, Some(&mut cloud));
        assert_eq!(devs[1].walker.hold(), &Hold::Here);
        assert_eq!(steps_of(&mut devs[1]), 77);
    }

    enum Cut {
        PutLost,
        StoredLost,
        StoredThenRestart,
    }

    fn restart(dev: &mut Dev) {
        dev.walker = pokewalker(&dev.memory);
        dev.walker.set_peer(Some(dev.peer.clone()));
    }

    fn holders(devs: &mut [Dev]) -> Vec<&'static str> {
        devs.iter_mut()
            .filter_map(|dev| {
                (dev.walker.follow_route(dev.cloud_on) == Route::Me).then_some(dev.name)
            })
            .collect()
    }

    fn cut_move(cut: Cut) {
        let mut devs = vec![dev(1, "Pixel 8", false), dev(2, "Galaxy Tab", false)];
        step(&mut devs, 0, None);
        step(&mut devs, 1, None);
        walk(&mut devs[0], 77);
        step(&mut devs, 0, None);
        let walker = devs[0].walker.copy.clone();
        listen(&mut devs, 1, 0);
        let phone = devs[0].peer.key;
        assert!(devs[1].walker.take_offer(phone));
        assert_eq!(devs[1].walker.follow_route(false), Route::Lan(phone));
        listen(&mut devs, 1, 0);
        let (tab, holder) = pair(&mut devs, 1, 0);
        let (to, fetch) = tab.walker.lan_request().expect("a fetch");
        assert_eq!(fetch, WalkerRequest::Fetch);
        let fetched = holder.walker.answer_peer(fetch.clone(), &tab.peer);
        tab.walker.lan_reply(to, Some(&fetch), fetched);
        let (to, put) = tab.walker.lan_request().expect("a put");
        assert!(matches!(put, WalkerRequest::Put { .. }));
        let lost = Answer::Reply(WalkerReply::Lost);
        match cut {
            Cut::PutLost => {
                tab.walker.lan_reply(to, None, lost);
            }
            Cut::StoredLost => {
                let stored = holder.walker.answer_peer(put, &tab.peer);
                assert!(matches!(stored, Answer::Reply(WalkerReply::Stored(_))));
                tab.walker.lan_reply(to, None, lost);
            }
            Cut::StoredThenRestart => {
                let stored = holder.walker.answer_peer(put, &tab.peer);
                assert!(matches!(stored, Answer::Reply(WalkerReply::Stored(_))));
                restart(tab);
            }
        }
        assert!(holders(&mut devs).len() <= 1);
        for _ in 0..3 {
            step(&mut devs, 1, None);
            step(&mut devs, 0, None);
            assert!(holders(&mut devs).len() <= 1);
        }
        assert_eq!(holders(&mut devs), vec!["Galaxy Tab"]);
        assert_eq!(devs[1].walker.hold(), &Hold::Here);
        assert_eq!(devs[0].walker.hold(), &Hold::Elsewhere("Galaxy Tab".into()));
        assert!(devs[1].walker.copy == walker);
        assert_eq!(steps_of(&mut devs[1]), 77);
        assert_eq!(devs[1].memory.borrow().kept.len(), 1);
        restart(&mut devs[1]);
        assert_eq!(holders(&mut devs), vec!["Galaxy Tab"]);
    }

    #[test]
    fn a_move_cut_before_the_put_arrives_still_has_one_holder() {
        cut_move(Cut::PutLost);
    }

    #[test]
    fn a_move_whose_stored_reply_is_lost_still_has_one_holder() {
        cut_move(Cut::StoredLost);
    }

    #[test]
    fn a_taker_that_restarts_before_saving_the_move_still_holds_it() {
        cut_move(Cut::StoredThenRestart);
    }
}
