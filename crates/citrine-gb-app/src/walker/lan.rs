use super::route::Announce;
use super::{Answer, Peer, Pokewalker};
use crate::lan::{Event, Lan, Service};
use crate::online::cloud::{WalkerRemote, WalkerReply, WalkerRequest};
use citrine_gb_net::PeerKey;
use citrine_gb_walker::Walker;
use serde::{Deserialize, Serialize};

const ASK_SECS: f64 = 60.0;
const MAX_ASKS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Msg {
    Request(WalkerRequest),
    Reply(WalkerReply),
    Follow {
        remote: WalkerRemote,
        holder: PeerKey,
    },
    Listing {
        chain: u64,
        remote: WalkerRemote,
        holder: PeerKey,
    },
    UsesCloud,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanAsk {
    pub key: PeerKey,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nearby {
    pub key: PeerKey,
    pub name: String,
    pub uses_cloud: bool,
}

struct Visit {
    key: PeerKey,
    allowed: bool,
    asked: Option<(WalkerRequest, f64)>,
    sent: Option<WalkerRequest>,
    announced: Option<Announce>,
    uses_cloud: bool,
}

#[derive(Default)]
pub struct WalkerLan {
    visits: Vec<Visit>,
}

impl WalkerLan {
    pub fn nearby(&self, lan: &Lan) -> Vec<Nearby> {
        self.visits
            .iter()
            .filter_map(|visit| {
                Some(Nearby {
                    key: visit.key,
                    name: lan.name(visit.key)?.to_owned(),
                    uses_cloud: visit.uses_cloud,
                })
            })
            .collect()
    }

    pub fn asks(&self, lan: &Lan) -> Vec<LanAsk> {
        let mut asks: Vec<(f64, LanAsk)> = self
            .visits
            .iter()
            .filter(|visit| !visit.allowed)
            .filter_map(|visit| {
                let (_, at) = visit.asked.as_ref()?;
                Some((
                    *at,
                    LanAsk {
                        key: visit.key,
                        name: lan.name(visit.key)?.to_owned(),
                    },
                ))
            })
            .collect();
        asks.sort_by(|a, b| b.0.total_cmp(&a.0));
        asks.into_iter()
            .take(MAX_ASKS)
            .map(|(_, ask)| ask)
            .collect()
    }

    pub fn allow(&mut self, lan: &mut Lan, key: PeerKey) {
        let Some(visit) = self
            .visits
            .iter_mut()
            .find(|visit| visit.key == key && visit.asked.is_some())
        else {
            return;
        };
        visit.allowed = true;
        lan.remember(key);
    }

    pub fn decline(&mut self, lan: &mut Lan, key: PeerKey) {
        for visit in &mut self.visits {
            if visit.key == key && visit.asked.take().is_some() {
                send(lan, key, &Msg::Declined);
            }
        }
    }

    pub fn step(
        &mut self,
        lan: &mut Lan,
        events: &[Event],
        walker: &mut Pokewalker,
        now: f64,
    ) -> Vec<Walker> {
        let mut adopted = Vec::new();
        for event in events {
            match event {
                Event::Joined(key) => self.join(lan, *key),
                Event::Left(key) => self.leave(*key, walker),
                Event::Part {
                    from,
                    service: Service::Walker,
                    body,
                } => {
                    if let Ok(msg) = serde_json::from_slice::<Msg>(body) {
                        self.read(lan, *from, msg, walker, now, &mut adopted);
                    }
                }
                Event::Part { .. } => {}
            }
        }
        if self
            .visits
            .iter()
            .any(|visit| visit.allowed && visit.asked.is_some())
        {
            adopted.extend(walker.save_before_answering());
        }
        for visit in &mut self.visits {
            if visit.allowed
                && let Some((request, _)) = visit.asked.take()
            {
                answer(lan, visit.key, walker, request);
            }
            if !visit.allowed
                && visit
                    .asked
                    .as_ref()
                    .is_some_and(|(_, at)| now - at >= ASK_SECS)
            {
                visit.asked = None;
                send(lan, visit.key, &Msg::Declined);
            }
        }
        self.announce(lan, walker);
        self.request(lan, walker);
        adopted
    }

    fn join(&mut self, lan: &Lan, key: PeerKey) {
        if self.visits.iter().any(|visit| visit.key == key) {
            return;
        }
        self.visits.push(Visit {
            key,
            allowed: lan.trusted(key),
            asked: None,
            sent: None,
            announced: None,
            uses_cloud: false,
        });
    }

    fn leave(&mut self, key: PeerKey, walker: &mut Pokewalker) {
        let Some(at) = self.visits.iter().position(|visit| visit.key == key) else {
            return;
        };
        if self.visits.remove(at).sent.is_some() {
            walker.lan_reply(key, None, Answer::Reply(WalkerReply::Lost));
        }
    }

    fn read(
        &mut self,
        lan: &mut Lan,
        key: PeerKey,
        msg: Msg,
        walker: &mut Pokewalker,
        now: f64,
        adopted: &mut Vec<Walker>,
    ) {
        let Some(visit) = self.visits.iter_mut().find(|visit| visit.key == key) else {
            return;
        };
        match msg {
            Msg::Request(request) if visit.allowed => answer(lan, key, walker, request),
            Msg::Request(request) => visit.asked = Some((request, now)),
            Msg::Reply(reply) => {
                let Some(sent) = visit.sent.take() else {
                    return;
                };
                if !visit.allowed {
                    visit.allowed = true;
                    lan.remember(key);
                }
                adopted.extend(walker.lan_reply(key, Some(&sent), Answer::Reply(reply)));
            }
            Msg::Follow { remote, holder } => {
                let sent = visit.sent.take();
                adopted.extend(walker.lan_reply(
                    key,
                    sent.as_ref(),
                    Answer::Follow { remote, holder },
                ));
            }
            Msg::Listing {
                chain,
                remote,
                holder,
            } => {
                visit.uses_cloud = false;
                adopted.extend(walker.lan_listing(key, chain, remote, holder));
            }
            Msg::UsesCloud => {
                visit.uses_cloud = true;
                match visit.sent.take() {
                    Some(sent) => {
                        walker.lan_reply(key, Some(&sent), Answer::UsesCloud);
                    }
                    None => walker.lan_holder_on_cloud(key),
                }
            }
            Msg::Declined => {
                visit.sent = None;
                walker.lan_declined(key);
            }
        }
    }

    fn announce(&mut self, lan: &mut Lan, walker: &Pokewalker) {
        let now = walker.announce();
        for visit in &mut self.visits {
            if visit.announced == now {
                continue;
            }
            visit.announced.clone_from(&now);
            match &now {
                Some(Announce::Walker {
                    chain,
                    remote,
                    holder,
                }) => send(
                    lan,
                    visit.key,
                    &Msg::Listing {
                        chain: *chain,
                        remote: remote.clone(),
                        holder: *holder,
                    },
                ),
                Some(Announce::Cloud) => send(lan, visit.key, &Msg::UsesCloud),
                None => {}
            }
        }
    }

    fn request(&mut self, lan: &mut Lan, walker: &mut Pokewalker) {
        let Some(holder) = walker.lan_holder() else {
            return;
        };
        let Some(visit) = self
            .visits
            .iter_mut()
            .find(|visit| visit.key == holder && visit.sent.is_none())
        else {
            return;
        };
        let Some((_, request)) = walker.lan_request() else {
            return;
        };
        send(lan, holder, &Msg::Request(request.clone()));
        visit.sent = Some(request);
    }
}

fn answer(lan: &mut Lan, key: PeerKey, walker: &mut Pokewalker, request: WalkerRequest) {
    let Some(name) = lan.name(key) else {
        return;
    };
    let peer = Peer {
        key,
        name: name.to_owned(),
    };
    let msg = match walker.answer_peer(request, &peer) {
        Answer::Reply(reply) => Msg::Reply(reply),
        Answer::Follow { remote, holder } => Msg::Follow { remote, holder },
        Answer::UsesCloud => Msg::UsesCloud,
    };
    send(lan, key, &msg);
}

fn send(lan: &mut Lan, to: PeerKey, msg: &Msg) {
    if let Ok(body) = serde_json::to_vec(msg) {
        lan.send(to, Service::Walker, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan::tests::{connect, lan};
    use crate::walker::Route;
    use crate::walker::tests::{Shared, pokewalker};
    use citrine_gb_walker::Button;

    const PSK: [u8; 32] = [9; 32];

    struct Dev {
        lan: Lan,
        service: WalkerLan,
        walker: Pokewalker,
        _memory: Shared,
    }

    fn dev(name: &str, instance: u64) -> Dev {
        let memory = Shared::default();
        let lan = lan(name, instance);
        let mut walker = pokewalker(&memory);
        walker.set_peer(Some(Peer {
            key: lan.key(),
            name: name.to_owned(),
        }));
        walker.follow_route(false);
        Dev {
            lan,
            service: WalkerLan::default(),
            walker,
            _memory: memory,
        }
    }

    fn walk(dev: &mut Dev, steps: u32) {
        dev.walker.walker().unwrap().steps(steps);
        dev.walker.press(Button::Middle);
    }

    fn steps_of(dev: &mut Dev) -> u32 {
        dev.walker.walker().unwrap().today_steps()
    }

    fn run(devs: &mut [&mut Dev], now: f64) {
        for _ in 0..12 {
            for dev in devs.iter_mut() {
                dev.walker.follow_route(false);
                let events = dev.lan.step(now);
                dev.service
                    .step(&mut dev.lan, &events, &mut dev.walker, now);
                dev.walker.settle_here();
            }
        }
    }

    fn holder_and_taker() -> (Dev, Dev) {
        let mut mac = dev("Mac", 1);
        let mut pixel = dev("Pixel", 2);
        walk(&mut mac, 120);
        connect(&mut pixel.lan, &mut mac.lan, None);
        run(&mut [&mut mac, &mut pixel], 0.0);
        (mac, pixel)
    }

    fn allow(dev: &mut Dev, key: PeerKey) {
        dev.service.allow(&mut dev.lan, key);
    }

    #[test]
    fn a_nearby_walker_shows_as_an_offer_without_asking() {
        let (mac, pixel) = holder_and_taker();
        assert_eq!(pixel.walker.offers().len(), 1);
        assert_eq!(pixel.walker.offers()[0].holder, mac.lan.key());
        assert!(mac.service.asks(&mac.lan).is_empty());
        assert_eq!(
            pixel
                .service
                .nearby(&pixel.lan)
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["Mac"]
        );
    }

    #[test]
    fn an_allowed_take_moves_the_walker_and_both_remember() {
        let (mut mac, mut pixel) = holder_and_taker();
        assert!(pixel.walker.take_offer(mac.lan.key()));
        run(&mut [&mut mac, &mut pixel], 1.0);
        assert_eq!(steps_of(&mut pixel), 0);
        let asks = mac.service.asks(&mac.lan);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].name, "Pixel");
        allow(&mut mac, pixel.lan.key());
        run(&mut [&mut mac, &mut pixel], 2.0);
        assert_eq!(steps_of(&mut pixel), 120);
        assert_eq!(pixel.walker.route(), Route::Me);
        assert_eq!(mac.walker.route(), Route::Lan(pixel.lan.key()));
        assert!(mac.lan.known().iter().any(|p| p.key == pixel.lan.key()));
        assert!(pixel.lan.known().iter().any(|p| p.key == mac.lan.key()));
    }

    #[test]
    fn a_walker_that_never_walked_is_not_offered() {
        let (mut mac, mut pixel) = holder_and_taker();
        assert!(mac.walker.offers().is_empty());
        pixel.walker.walker().unwrap().steps(5);
        run(&mut [&mut mac, &mut pixel], 1.0);
        assert_eq!(mac.walker.offers().len(), 1);
    }

    #[test]
    fn a_take_carries_steps_the_holder_has_not_saved_yet() {
        let (mut mac, mut pixel) = holder_and_taker();
        mac.walker.walker().unwrap().steps(80);
        assert!(pixel.walker.take_offer(mac.lan.key()));
        run(&mut [&mut mac, &mut pixel], 1.0);
        allow(&mut mac, pixel.lan.key());
        run(&mut [&mut mac, &mut pixel], 2.0);
        assert_eq!(steps_of(&mut pixel), 200);
        assert_eq!(pixel.walker.route(), Route::Me);
        assert_eq!(mac.walker.route(), Route::Lan(pixel.lan.key()));
    }

    #[test]
    fn a_reply_nobody_asked_for_grants_nothing() {
        let (mut mac, mut pixel) = holder_and_taker();
        let key = mac.lan.key();
        send(&mut pixel.lan, key, &Msg::Reply(WalkerReply::Lost));
        send(&mut pixel.lan, key, &Msg::Request(WalkerRequest::Fetch));
        run(&mut [&mut mac, &mut pixel], 1.0);
        assert!(mac.lan.known().is_empty());
        assert_eq!(mac.service.asks(&mac.lan).len(), 1);
        assert_eq!(mac.walker.route(), Route::Me);
        assert_eq!(steps_of(&mut mac), 120);
    }

    #[test]
    fn a_declined_take_leaves_both_walkers_as_they_were() {
        let (mut mac, mut pixel) = holder_and_taker();
        walk(&mut pixel, 30);
        assert!(pixel.walker.take_offer(mac.lan.key()));
        run(&mut [&mut mac, &mut pixel], 1.0);
        mac.service.decline(&mut mac.lan, pixel.lan.key());
        run(&mut [&mut mac, &mut pixel], 2.0);
        assert_eq!(steps_of(&mut pixel), 30);
        assert_eq!(steps_of(&mut mac), 120);
        assert_eq!(pixel.walker.route(), Route::Me);
        assert_eq!(mac.walker.route(), Route::Me);
        assert!(mac.lan.known().is_empty());
        assert!(pixel.lan.known().is_empty());
    }

    #[test]
    fn an_unanswered_ask_ends_after_a_minute() {
        let (mut mac, mut pixel) = holder_and_taker();
        assert!(pixel.walker.take_offer(mac.lan.key()));
        run(&mut [&mut mac, &mut pixel], 1.0);
        run(&mut [&mut mac, &mut pixel], 60.5);
        assert_eq!(mac.service.asks(&mac.lan).len(), 1);
        run(&mut [&mut mac, &mut pixel], 61.0);
        assert!(mac.service.asks(&mac.lan).is_empty());
        assert_eq!(pixel.walker.route(), Route::Me);
        assert_eq!(steps_of(&mut pixel), 0);
    }

    #[test]
    fn the_same_account_takes_without_asking() {
        let mut mac = dev("Mac", 1);
        let mut pixel = dev("Pixel", 2);
        walk(&mut mac, 120);
        connect(&mut pixel.lan, &mut mac.lan, Some(PSK));
        run(&mut [&mut mac, &mut pixel], 0.0);
        assert!(pixel.walker.take_offer(mac.lan.key()));
        run(&mut [&mut mac, &mut pixel], 1.0);
        assert!(mac.service.asks(&mac.lan).is_empty());
        assert_eq!(steps_of(&mut pixel), 120);
        assert!(mac.lan.known().is_empty());
    }

    #[test]
    fn at_most_three_asks_show_newest_first_with_names_cut() {
        let mut mac = dev("Mac", 1);
        walk(&mut mac, 120);
        let long = "A very long device name that goes on and on and on";
        let mut takers: Vec<Dev> = (0..4)
            .map(|n| dev(&format!("{n} {long}"), 10 + n))
            .collect();
        for (n, taker) in takers.iter_mut().enumerate() {
            connect(&mut taker.lan, &mut mac.lan, None);
            run(&mut [&mut mac, &mut *taker], 0.0);
            assert!(taker.walker.take_offer(mac.lan.key()));
            run(&mut [&mut mac, &mut *taker], 1.0 + n as f64);
        }
        let asks = mac.service.asks(&mac.lan);
        assert_eq!(asks.len(), 3);
        assert!(asks[0].name.starts_with('3'));
        assert!(asks[2].name.starts_with('1'));
        assert!(asks.iter().all(|ask| ask.name.chars().count() == 40));
    }

    #[test]
    fn a_lost_link_drops_the_ask_and_the_taker_waits_for_the_holder() {
        let (mut mac, mut pixel) = holder_and_taker();
        assert!(pixel.walker.take_offer(mac.lan.key()));
        run(&mut [&mut mac, &mut pixel], 1.0);
        mac.lan.forget(pixel.lan.key());
        run(&mut [&mut mac, &mut pixel], 2.0);
        assert!(mac.service.asks(&mac.lan).is_empty());
        assert!(pixel.service.nearby(&pixel.lan).is_empty());
        assert_eq!(pixel.walker.route(), Route::Lan(mac.lan.key()));
        assert_eq!(steps_of(&mut pixel), 0);
    }
}
