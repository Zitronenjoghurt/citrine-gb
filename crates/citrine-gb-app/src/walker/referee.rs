use crate::online::cloud::{WalkerRemote, WalkerReply, WalkerRequest};
use citrine_gb_net::{PeerKey, SavePayload};
use serde::{Deserialize, Serialize};

const DATA: &str = "walker";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub key: PeerKey,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Reply(WalkerReply),
    Follow {
        remote: WalkerRemote,
        holder: PeerKey,
    },
    UsesCloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Referee {
    chain: u64,
    me: PeerKey,
    holder: PeerKey,
    remote: WalkerRemote,
    #[serde(skip)]
    data: Vec<u8>,
}

pub fn hash(data: &[u8]) -> [u8; 32] {
    SavePayload {
        files: vec![(DATA.to_owned(), data.to_vec())],
    }
    .hash()
}

impl Referee {
    pub fn start(
        chain: u64,
        me: &Peer,
        version: u64,
        data: Vec<u8>,
        name: String,
        now: u64,
    ) -> Self {
        Self {
            chain,
            me: me.key,
            holder: me.key,
            remote: WalkerRemote {
                version,
                device: me.name.clone(),
                hash: hash(&data),
                saved_at: now,
                name,
            },
            data,
        }
    }

    pub fn follower(chain: u64, me: PeerKey, remote: WalkerRemote, holder: PeerKey) -> Self {
        Self {
            chain,
            me,
            holder,
            remote,
            data: Vec::new(),
        }
    }

    pub fn chain(&self) -> u64 {
        self.chain
    }

    pub fn holder(&self) -> PeerKey {
        self.holder
    }

    pub fn refereeing(&self) -> bool {
        self.holder == self.me
    }

    pub fn remote(&self) -> &WalkerRemote {
        &self.remote
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn listing(&self) -> (WalkerRemote, PeerKey) {
        (self.remote.clone(), self.holder)
    }

    pub fn answer(&mut self, request: WalkerRequest, from: &Peer, now: u64) -> Answer {
        if !self.refereeing() {
            return Answer::Follow {
                remote: self.remote.clone(),
                holder: self.holder,
            };
        }
        let reply = match request {
            WalkerRequest::Fetch => WalkerReply::Fetched {
                remote: self.remote.clone(),
                data: self.data.clone(),
            },
            WalkerRequest::Put { base, .. } if base != self.remote.version => {
                WalkerReply::Refused { changed: true }
            }
            WalkerRequest::Put { data, name, .. } => {
                self.remote = WalkerRemote {
                    version: self.remote.version + 1,
                    device: from.name.clone(),
                    hash: hash(&data),
                    saved_at: now,
                    name,
                };
                self.holder = from.key;
                self.data = if self.refereeing() { data } else { Vec::new() };
                WalkerReply::Stored(self.remote.clone())
            }
        };
        Answer::Reply(reply)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn follow(&mut self, chain: u64, remote: WalkerRemote, holder: PeerKey) -> bool {
        if chain != self.chain || remote.version <= self.remote.version || holder == self.me {
            return false;
        }
        self.remote = remote;
        self.holder = holder;
        self.data.clear();
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn took(&mut self, remote: WalkerRemote, data: Vec<u8>) -> bool {
        if remote.version <= self.remote.version || remote.hash != hash(&data) {
            return false;
        }
        self.remote = remote;
        self.holder = self.me;
        self.data = data;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::sync::{Effect, Hold, LastStored, Sync};

    const CHAIN: u64 = 7;
    const NOW: u64 = 1_700_000_000;

    struct Device {
        peer: Peer,
        sync: Sync,
        copy: Vec<u8>,
        kept: Vec<Vec<u8>>,
        referee: Referee,
    }

    fn peer(n: u8, name: &str) -> Peer {
        Peer {
            key: citrine_gb_net::RelayKey([n; 32]),
            name: name.to_owned(),
        }
    }

    fn holder(n: u8, name: &str, copy: &[u8]) -> Device {
        let peer = peer(n, name);
        let referee = Referee::start(CHAIN, &peer, 1, copy.to_vec(), String::new(), NOW);
        let mut sync = Sync::default();
        sync.on();
        sync.set_last_stored(Some(LastStored {
            version: 1,
            hash: hash(copy),
        }));
        Device {
            peer,
            sync,
            copy: copy.to_vec(),
            kept: Vec::new(),
            referee,
        }
    }

    fn newcomer(n: u8, name: &str, from: &Device) -> Device {
        let peer = peer(n, name);
        let (remote, key) = from.referee.listing();
        let referee = Referee::follower(CHAIN, peer.key, remote, key);
        let mut sync = Sync::default();
        sync.on();
        Device {
            peer,
            sync,
            copy: Vec::new(),
            kept: Vec::new(),
            referee,
        }
    }

    impl Device {
        fn edit(&mut self, copy: &[u8]) {
            assert!(self.sync.may_change(), "{} is read-only", self.peer.name);
            self.copy = copy.to_vec();
            self.sync.changed();
        }

        fn hear(&mut self, reply: WalkerReply) {
            match self.sync.reply(reply, &self.copy) {
                Some(Effect::Adopt(data)) => self.copy = data,
                Some(Effect::KeepAndAdopt { kept, adopt }) => {
                    self.kept.push(kept);
                    self.copy = adopt;
                }
                None => {}
            }
        }
    }

    fn at(devices: &[Device], key: PeerKey) -> usize {
        devices
            .iter()
            .position(|device| device.peer.key == key)
            .expect("a known device")
    }

    fn ask(devices: &mut [Device], from: usize, request: &WalkerRequest) -> WalkerReply {
        let asker = devices[from].peer.clone();
        let mut to = at(devices, devices[from].referee.holder());
        for _ in 0..3 {
            match devices[to].referee.answer(request.clone(), &asker, NOW) {
                Answer::Reply(reply) => {
                    if let (WalkerReply::Stored(remote), WalkerRequest::Put { data, .. }) =
                        (&reply, request)
                        && to != from
                    {
                        devices[from].referee.took(remote.clone(), data.clone());
                    }
                    return reply;
                }
                Answer::Follow { remote, holder } => {
                    let chain = devices[to].referee.chain();
                    devices[from].referee.follow(chain, remote, holder);
                    to = at(devices, holder);
                }
                Answer::UsesCloud => return WalkerReply::Lost,
            }
        }
        WalkerReply::Lost
    }

    fn listen(devices: &mut [Device], i: usize) {
        let mut to = at(devices, devices[i].referee.holder());
        for _ in 0..3 {
            let (remote, holder) = devices[to].referee.listing();
            let chain = devices[to].referee.chain();
            devices[i].referee.follow(chain, remote, holder);
            if holder == devices[to].peer.key {
                break;
            }
            to = at(devices, holder);
        }
        let listed = devices[i].referee.remote().clone();
        devices[i].hear(WalkerReply::Listed(Some(listed)));
    }

    fn sync(devices: &mut [Device], i: usize) {
        listen(devices, i);
        while let Some(request) = devices[i].sync.request(&devices[i].copy) {
            let reply = ask(devices, i, &request);
            devices[i].hear(reply);
            listen(devices, i);
        }
    }

    #[test]
    fn the_holders_own_changes_go_through_its_referee() {
        let mut devices = vec![holder(1, "Pixel 8", b"one")];
        sync(&mut devices, 0);
        assert_eq!(devices[0].sync.hold(), &Hold::Here);
        devices[0].edit(b"two");
        sync(&mut devices, 0);
        assert_eq!(devices[0].referee.remote().version, 2);
        assert_eq!(devices[0].referee.remote().hash, hash(b"two"));
        assert!(devices[0].referee.refereeing());
    }

    #[test]
    fn a_take_moves_the_walker_and_the_referee_with_it() {
        let mut devices = vec![holder(1, "Pixel 8", b"one")];
        sync(&mut devices, 0);
        let mac = newcomer(2, "Clara's MacBook Pro", &devices[0]);
        devices.push(mac);
        devices[1].sync.take();
        sync(&mut devices, 1);
        assert_eq!(devices[1].sync.hold(), &Hold::Here);
        assert_eq!(devices[1].copy, b"one");
        assert!(devices[1].referee.refereeing());
        assert!(!devices[0].referee.refereeing());
        sync(&mut devices, 0);
        assert_eq!(
            devices[0].sync.hold(),
            &Hold::Elsewhere("Clara's MacBook Pro".into())
        );
        assert!(!devices[0].sync.may_change());
        devices[1].edit(b"two");
        sync(&mut devices, 1);
        assert_eq!(devices[1].referee.remote().version, 3);
        devices[0].sync.take();
        sync(&mut devices, 0);
        assert_eq!(devices[0].sync.hold(), &Hold::Here);
        assert_eq!(devices[0].copy, b"two");
        assert!(devices[0].referee.refereeing());
        assert_eq!(devices[0].referee.remote().version, 4);
    }

    #[test]
    fn a_refused_take_leaves_both_copies_as_they_were() {
        let mut devices = vec![holder(1, "Pixel 8", b"one")];
        sync(&mut devices, 0);
        let mac = newcomer(2, "Clara's MacBook Pro", &devices[0]);
        devices.push(mac);
        devices[1].sync.take();
        listen(&mut devices, 1);
        let copy = devices[1].copy.clone();
        let fetch = devices[1].sync.request(&copy).expect("a fetch");
        let reply = ask(&mut devices, 1, &fetch);
        devices[1].hear(reply);
        devices[0].edit(b"two");
        sync(&mut devices, 0);
        let copy = devices[1].copy.clone();
        let put = devices[1].sync.request(&copy).expect("a put");
        let reply = ask(&mut devices, 1, &put);
        assert_eq!(reply, WalkerReply::Refused { changed: true });
        devices[1].hear(reply);
        assert_eq!(devices[0].copy, b"two");
        assert_eq!(devices[0].sync.hold(), &Hold::Here);
        assert!(devices[0].referee.refereeing());
        assert_eq!(devices[0].referee.remote().version, 2);
        assert_eq!(devices[1].copy, b"one");
        assert_eq!(devices[1].sync.hold(), &Hold::Taking);
        assert!(!devices[1].referee.refereeing());
    }

    #[test]
    fn a_device_that_knows_an_old_holder_is_sent_on_to_the_new_one() {
        let mut devices = vec![holder(1, "Pixel 8", b"one")];
        sync(&mut devices, 0);
        let mac = newcomer(2, "Clara's MacBook Pro", &devices[0]);
        let tab = newcomer(3, "Galaxy Tab", &devices[0]);
        devices.push(mac);
        devices.push(tab);
        devices[1].sync.take();
        sync(&mut devices, 1);
        assert_eq!(devices[2].referee.holder(), devices[0].peer.key);
        devices[2].sync.take();
        sync(&mut devices, 2);
        assert_eq!(devices[2].sync.hold(), &Hold::Here);
        assert!(devices[2].referee.refereeing());
        assert_eq!(devices[2].referee.remote().version, 3);
        sync(&mut devices, 0);
        sync(&mut devices, 1);
        assert_eq!(
            devices[0].sync.hold(),
            &Hold::Elsewhere("Galaxy Tab".into())
        );
        assert_eq!(
            devices[1].sync.hold(),
            &Hold::Elsewhere("Galaxy Tab".into())
        );
    }

    #[test]
    fn follow_takes_only_newer_listings_of_the_same_walker() {
        let me = peer(1, "Pixel 8");
        let other = peer(2, "Clara's MacBook Pro");
        let mut referee = Referee::start(CHAIN, &me, 4, b"one".to_vec(), String::new(), NOW);
        let mut newer = referee.remote().clone();
        newer.version = 5;
        assert!(!referee.follow(CHAIN + 1, newer.clone(), other.key));
        assert!(!referee.follow(CHAIN, newer.clone(), me.key));
        let mut older = newer.clone();
        older.version = 3;
        assert!(!referee.follow(CHAIN, older, other.key));
        assert!(referee.refereeing());
        assert!(referee.follow(CHAIN, newer, other.key));
        assert!(!referee.refereeing());
        assert_eq!(
            referee.answer(WalkerRequest::Fetch, &other, NOW),
            Answer::Follow {
                remote: referee.remote().clone(),
                holder: other.key,
            }
        );
    }

    #[test]
    fn took_needs_the_bytes_that_were_stored() {
        let me = peer(1, "Pixel 8");
        let other = peer(2, "Clara's MacBook Pro");
        let mut referee = Referee::follower(
            CHAIN,
            me.key,
            Referee::start(CHAIN, &other, 1, b"one".to_vec(), String::new(), NOW)
                .remote()
                .clone(),
            other.key,
        );
        let mut stored = referee.remote().clone();
        stored.version = 2;
        stored.hash = hash(b"two");
        assert!(!referee.took(stored.clone(), b"three".to_vec()));
        assert!(referee.took(stored, b"two".to_vec()));
        assert!(referee.refereeing());
    }
}
