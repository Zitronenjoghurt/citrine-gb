use crate::online::cloud::{WalkerRemote, WalkerReply, WalkerRequest};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Hold {
    #[default]
    Local,
    Idle,
    Taking,
    Here,
    Elsewhere(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Adopt(Vec<u8>),
    KeepAndAdopt { kept: Vec<u8>, adopt: Vec<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastStored {
    pub version: u64,
    pub hash: [u8; 32],
}

impl LastStored {
    pub const SIZE: usize = 40;

    fn of(remote: &WalkerRemote) -> Self {
        Self {
            version: remote.version,
            hash: remote.hash,
        }
    }

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..8].copy_from_slice(&self.version.to_le_bytes());
        bytes[8..].copy_from_slice(&self.hash);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE {
            return None;
        }
        Some(Self {
            version: u64::from_le_bytes(bytes[..8].try_into().ok()?),
            hash: bytes[8..].try_into().ok()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Release {
    #[default]
    No,
    Wanted,
    Sent,
}

#[derive(Default)]
pub struct Sync {
    hold: Hold,
    last_stored: Option<LastStored>,
    listed: Option<Option<WalkerRemote>>,
    base: Option<u64>,
    changes: u64,
    synced: Option<u64>,
    sent: Option<u64>,
    stuck: Option<u64>,
    busy: bool,
    release: Release,
    again: bool,
    overwrite: bool,
}

impl Sync {
    pub fn hold(&self) -> &Hold {
        &self.hold
    }

    pub fn may_change(&self) -> bool {
        !matches!(self.hold, Hold::Elsewhere(_))
    }

    pub fn may_credit(&self) -> bool {
        matches!(self.hold, Hold::Local | Hold::Here)
    }

    pub fn on(&mut self) {
        if self.hold == Hold::Local {
            self.hold = Hold::Idle;
        }
    }

    pub fn off(&mut self) {
        *self = Self {
            changes: self.changes,
            last_stored: self.last_stored,
            ..Self::default()
        };
    }

    pub fn stay_elsewhere(&mut self, device: String) {
        self.hold = Hold::Elsewhere(device);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn took(&mut self, remote: WalkerRemote) {
        self.base = Some(remote.version);
        self.last_stored = Some(LastStored::of(&remote));
        self.synced = Some(self.changes);
        self.sent = None;
        self.stuck = None;
        self.busy = false;
        self.release = Release::No;
        self.again = false;
        self.overwrite = false;
        self.hold = Hold::Here;
        self.listed = Some(Some(remote));
    }

    pub fn last_stored(&self) -> Option<LastStored> {
        self.last_stored
    }

    pub fn set_last_stored(&mut self, last_stored: Option<LastStored>) {
        self.last_stored = last_stored;
    }

    pub fn changed(&mut self) {
        self.changes += 1;
    }

    pub fn take(&mut self) {
        if !matches!(self.hold, Hold::Local | Hold::Here) {
            self.hold = Hold::Taking;
        }
    }

    pub fn untake(&mut self) -> bool {
        if self.hold == Hold::Taking && !self.busy {
            self.hold = match self.listed() {
                Some(remote) => Hold::Elsewhere(remote.device.clone()),
                None => Hold::Idle,
            };
            self.release = Release::No;
            return true;
        }
        false
    }

    pub fn releasing(&self) -> bool {
        self.release != Release::No
    }

    pub fn release(&mut self) {
        if matches!(self.hold, Hold::Taking | Hold::Here) && self.release == Release::No {
            self.release = Release::Wanted;
        }
    }

    pub fn claim(&mut self) {
        if self.hold == Hold::Idle && self.listed == Some(None) {
            self.hold = Hold::Taking;
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_taking(&mut self) {
        if self.hold != Hold::Local {
            self.hold = Hold::Taking;
            self.release = Release::No;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn cancel_take(&mut self, hold: Hold) {
        if self.hold == Hold::Taking {
            self.hold = hold;
            self.busy = false;
            self.sent = None;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn step_aside(&mut self) -> bool {
        let Some(device) = self.listed().map(|remote| remote.device.clone()) else {
            return false;
        };
        if !matches!(self.hold, Hold::Here | Hold::Taking) {
            return false;
        }
        let unsent = self.unsent();
        self.hold = Hold::Elsewhere(device);
        self.release = Release::No;
        self.busy = false;
        unsent
    }

    pub fn resend(&mut self) {
        if self.hold == Hold::Here {
            self.again = true;
        }
    }

    pub fn overwrite(&mut self) {
        if self.hold != Hold::Local {
            self.hold = Hold::Here;
            self.release = Release::No;
            self.overwrite = true;
            self.again = true;
        }
    }

    pub fn rewire(&mut self) {
        self.resend_release();
        self.listed = None;
        self.base = None;
        self.busy = false;
        self.sent = None;
        self.stuck = None;
        self.again = false;
        self.overwrite = false;
    }

    pub fn listed(&self) -> Option<&WalkerRemote> {
        self.listed.as_ref()?.as_ref()
    }

    fn unsent(&self) -> bool {
        self.synced
            .map_or(self.changes > 0, |synced| synced != self.changes)
    }

    pub fn request(&mut self, copy: &[u8]) -> Option<WalkerRequest> {
        if self.busy || self.hold == Hold::Local {
            return None;
        }
        if self.overwrite
            && let Some(Some(remote)) = &self.listed
        {
            self.base = Some(remote.version);
        }
        let listed = self.listed.as_ref()?;
        if let Some(remote) = listed
            && Some(remote.version) != self.base
        {
            let fetch = match self.hold {
                Hold::Taking | Hold::Here => true,
                Hold::Idle => self.unsent(),
                Hold::Local | Hold::Elsewhere(_) => false,
            };
            if !fetch {
                self.hold = Hold::Elsewhere(remote.device.clone());
                return None;
            }
            self.busy = true;
            return Some(WalkerRequest::Fetch);
        }
        let releasing = self.release == Release::Wanted;
        let put = match self.hold {
            Hold::Taking => true,
            Hold::Here => self.unsent() || releasing || self.again,
            _ => false,
        };
        if !put || (!releasing && self.stuck == Some(self.changes)) {
            return None;
        }
        let base = listed.as_ref().map_or(0, |remote| remote.version);
        self.busy = true;
        self.sent = Some(self.changes);
        if releasing {
            self.release = Release::Sent;
        }
        Some(WalkerRequest::Put {
            base,
            data: copy.to_vec(),
            name: String::new(),
        })
    }

    pub fn reply(&mut self, reply: WalkerReply, copy: &[u8]) -> Option<Effect> {
        if self.hold == Hold::Local {
            return None;
        }
        match reply {
            WalkerReply::Listed(remote) => {
                if remote.is_none() {
                    self.base = None;
                    if matches!(self.hold, Hold::Elsewhere(_)) {
                        self.hold = Hold::Idle;
                    }
                }
                if let (Some(remote), Hold::Elsewhere(device)) = (&remote, &mut self.hold) {
                    device.clone_from(&remote.device);
                }
                if let Some(remote) = &remote
                    && self.base.is_none()
                    && self.last_stored == Some(LastStored::of(remote))
                {
                    self.base = Some(remote.version);
                    if self.hold == Hold::Idle {
                        self.hold = Hold::Here;
                    }
                }
                self.listed = Some(remote);
                None
            }
            WalkerReply::Fetched { remote, data } => {
                self.busy = false;
                let keep = self.unsent() && data != copy;
                self.settle(remote);
                if data == copy {
                    None
                } else if keep {
                    Some(Effect::KeepAndAdopt {
                        kept: copy.to_vec(),
                        adopt: data,
                    })
                } else {
                    Some(Effect::Adopt(data))
                }
            }
            WalkerReply::Unreadable(remote) => {
                self.busy = false;
                self.base = Some(remote.version);
                None
            }
            WalkerReply::Stored(remote) => {
                self.busy = false;
                self.base = Some(remote.version);
                self.last_stored = Some(LastStored::of(&remote));
                self.synced = self.sent.take();
                self.stuck = None;
                self.again = false;
                self.overwrite = false;
                if self.release == Release::Sent {
                    self.release = Release::No;
                    self.hold = Hold::Elsewhere(remote.device.clone());
                } else if self.hold == Hold::Taking {
                    self.hold = Hold::Here;
                }
                self.listed = Some(Some(remote));
                None
            }
            WalkerReply::Refused { changed } => {
                self.busy = false;
                let sent = self.sent.take();
                if changed {
                    self.resend_release();
                    self.base = None;
                } else {
                    self.release = Release::No;
                    self.stuck = sent;
                }
                None
            }
            WalkerReply::Lost => {
                self.busy = false;
                self.resend_release();
                self.sent = None;
                None
            }
        }
    }

    fn resend_release(&mut self) {
        if self.release == Release::Sent {
            self.release = Release::Wanted;
        }
    }

    fn settle(&mut self, remote: WalkerRemote) {
        self.base = Some(remote.version);
        self.synced = Some(self.changes);
        if self.hold != Hold::Taking {
            self.release = Release::No;
            self.hold = Hold::Elsewhere(remote.device);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn digest(data: &[u8]) -> [u8; 32] {
        use std::hash::{Hash, Hasher};
        let mut hash = [0; 32];
        for (n, part) in hash.chunks_mut(8).enumerate() {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (n, data).hash(&mut hasher);
            part.copy_from_slice(&hasher.finish().to_le_bytes());
        }
        hash
    }

    #[derive(Default)]
    struct Server {
        version: u64,
        device: String,
        data: Vec<u8>,
        fetches: usize,
    }

    impl Server {
        fn remote(&self) -> WalkerRemote {
            WalkerRemote {
                version: self.version,
                device: self.device.clone(),
                hash: digest(&self.data),
                saved_at: 0,
                name: String::new(),
            }
        }

        fn listed(&self) -> WalkerReply {
            WalkerReply::Listed((self.version > 0).then(|| self.remote()))
        }

        fn answer(&mut self, device: &str, request: WalkerRequest) -> WalkerReply {
            match request {
                WalkerRequest::Fetch => {
                    self.fetches += 1;
                    WalkerReply::Fetched {
                        remote: self.remote(),
                        data: self.data.clone(),
                    }
                }
                WalkerRequest::Put { base, .. } if base != self.version => {
                    WalkerReply::Refused { changed: true }
                }
                WalkerRequest::Put { data, .. } => {
                    self.version += 1;
                    self.device = device.to_string();
                    self.data = data;
                    WalkerReply::Stored(self.remote())
                }
            }
        }
    }

    struct Device {
        name: &'static str,
        sync: Sync,
        copy: Vec<u8>,
        kept: Vec<Vec<u8>>,
    }

    impl Device {
        fn new(name: &'static str, copy: &[u8]) -> Self {
            let mut sync = Sync::default();
            sync.on();
            if !copy.is_empty() {
                sync.changed();
            }
            Self {
                name,
                sync,
                copy: copy.to_vec(),
                kept: Vec::new(),
            }
        }

        fn edit(&mut self, copy: &[u8]) {
            assert!(self.sync.may_change(), "{} is read-only", self.name);
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

        fn sync(&mut self, server: &mut Server) {
            self.hear(server.listed());
            while let Some(request) = self.sync.request(&self.copy) {
                let reply = server.answer(self.name, request);
                self.hear(reply);
                self.hear(server.listed());
            }
        }

        fn send_only(&mut self, server: &mut Server) -> WalkerReply {
            let request = self.sync.request(&self.copy).expect("a request");
            server.answer(self.name, request)
        }
    }

    fn holding(server: &mut Server, name: &'static str, copy: &[u8]) -> Device {
        let mut device = Device::new(name, copy);
        device.sync.take();
        device.sync(server);
        assert_eq!(device.sync.hold(), &Hold::Here);
        device
    }

    #[test]
    fn the_first_take_uploads_the_local_walker() {
        let mut server = Server::default();
        let device = holding(&mut server, "PC", b"local");
        assert_eq!((server.version, server.data.as_slice()), (1, &b"local"[..]));
        assert_eq!(device.copy, b"local");
    }

    fn restarted(name: &'static str, copy: &[u8], last_stored: Option<LastStored>) -> Device {
        let mut device = Device::new(name, copy);
        device.sync.set_last_stored(last_stored);
        device
    }

    #[test]
    fn a_restarted_holder_keeps_holding_and_puts_its_newer_copy() {
        let mut server = Server::default();
        let pc = holding(&mut server, "PC", b"stored");
        let mut pc = restarted("PC", b"credited later", pc.sync.last_stored());
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Here);
        assert_eq!(server.data, b"credited later");
        assert!(pc.kept.is_empty());
    }

    #[test]
    fn a_restarted_holder_whose_walker_was_taken_since_keeps_its_copy_aside() {
        let mut server = Server::default();
        let pc = holding(&mut server, "PC", b"stored");
        let last_stored = pc.sync.last_stored();
        let mut phone = Device::new("Phone", b"");
        phone.sync(&mut server);
        phone.sync.take();
        phone.sync(&mut server);
        phone.edit(b"phone steps");
        phone.sync(&mut server);
        let mut pc = restarted("PC", b"credited later", last_stored);
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
        assert_eq!(pc.copy, b"phone steps");
        assert_eq!(pc.kept, [b"credited later".to_vec()]);
    }

    #[test]
    fn a_record_of_the_same_version_with_other_bytes_is_not_trusted() {
        let mut server = Server {
            version: 1,
            device: "Phone".into(),
            data: b"after a cloud delete".to_vec(),
            fetches: 0,
        };
        let last_stored = LastStored {
            version: 1,
            hash: digest(b"stored before the delete"),
        };
        let mut pc = restarted("PC", b"credited later", Some(last_stored));
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
        assert_eq!(server.version, 1);
        assert_eq!(pc.kept, [b"credited later".to_vec()]);
    }

    #[test]
    fn the_record_survives_turning_cloud_saves_off() {
        let mut server = Server::default();
        let mut pc = holding(&mut server, "PC", b"stored");
        let last_stored = pc.sync.last_stored();
        pc.sync.off();
        assert_eq!(pc.sync.last_stored(), last_stored);
        assert_eq!(
            LastStored::from_bytes(&last_stored.unwrap().to_bytes()),
            last_stored
        );
    }

    #[test]
    fn a_signed_in_device_that_never_takes_uploads_nothing() {
        let mut server = Server::default();
        let mut device = Device::new("PC", b"local");
        device.edit(b"pressed");
        device.sync(&mut server);
        assert_eq!(server.version, 0);
        assert_eq!(device.sync.hold(), &Hold::Idle);
    }

    #[test]
    fn the_holder_saves_and_a_taker_continues_from_its_last_save() {
        let mut server = Server::default();
        let mut pc = holding(&mut server, "PC", b"v1");
        pc.edit(b"pc walked");
        pc.sync(&mut server);
        let mut phone = Device::new("Phone", b"phone stale");
        phone.sync.take();
        phone.sync(&mut server);
        assert_eq!(phone.sync.hold(), &Hold::Here);
        assert_eq!(phone.copy, b"pc walked");
        assert_eq!(server.device, "Phone");
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
        assert_eq!(pc.copy, b"pc walked");
        assert!(pc.kept.is_empty());
        assert!(!pc.sync.may_change());
    }

    #[test]
    fn a_save_after_a_take_is_refused_and_kept_not_lost() {
        let mut server = Server::default();
        let mut pc = holding(&mut server, "PC", b"v1");
        let mut phone = Device::new("Phone", b"");
        phone.sync.take();
        phone.sync(&mut server);
        pc.edit(b"pc late");
        let refused = pc.send_only(&mut server);
        assert_eq!(refused, WalkerReply::Refused { changed: true });
        pc.hear(refused);
        pc.sync(&mut server);
        assert_eq!(server.data, b"v1");
        assert_eq!(server.device, "Phone");
        assert_eq!(pc.kept, vec![b"pc late".to_vec()]);
        assert_eq!(pc.copy, b"v1");
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
    }

    #[test]
    fn a_take_that_races_a_save_builds_on_the_save() {
        let mut server = Server::default();
        let mut pc = holding(&mut server, "PC", b"v1");
        let mut phone = Device::new("Phone", b"");
        phone.sync.take();
        phone.hear(server.listed());
        let fetched = phone.send_only(&mut server);
        phone.hear(fetched);
        assert_eq!(phone.copy, b"v1");
        pc.edit(b"pc saved first");
        pc.sync(&mut server);
        let refused = phone.send_only(&mut server);
        assert_eq!(refused, WalkerReply::Refused { changed: true });
        phone.hear(refused);
        phone.sync(&mut server);
        assert_eq!(phone.sync.hold(), &Hold::Here);
        assert_eq!(phone.copy, b"pc saved first");
        assert_eq!(server.data, b"pc saved first");
        assert_eq!(server.version, 3);
        pc.sync(&mut server);
        assert!(pc.kept.is_empty());
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
    }

    #[test]
    fn a_device_without_the_walker_fetches_it_only_to_take_it_back() {
        let mut server = Server::default();
        let mut pc = holding(&mut server, "PC", b"v1");
        let mut phone = holding(&mut server, "Phone", b"");
        pc.sync(&mut server);
        phone.edit(b"phone walked");
        phone.sync(&mut server);
        let fetches = server.fetches;
        pc.sync(&mut server);
        assert_eq!(server.fetches, fetches);
        assert_eq!(pc.copy, b"v1");
        assert_eq!(pc.sync.listed().map(|remote| remote.version), Some(3));
        pc.sync.take();
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Here);
        assert_eq!(pc.copy, b"phone walked");
        phone.sync(&mut server);
        assert_eq!(phone.sync.hold(), &Hold::Elsewhere("PC".into()));
        assert!(pc.kept.is_empty() && phone.kept.is_empty());
    }

    #[test]
    fn a_new_device_meeting_the_account_walker_shows_where_it_is_without_fetching() {
        let mut server = Server::default();
        holding(&mut server, "Phone", b"account walker");
        let mut pc = Device::new("PC", b"");
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
        assert_eq!(server.fetches, 0);
        assert!(!pc.sync.may_change());
    }

    #[test]
    fn a_device_with_its_own_walker_keeps_it_aside_when_another_holds_the_account_walker() {
        let mut server = Server::default();
        holding(&mut server, "Phone", b"account walker");
        let mut pc = Device::new("PC", b"pc local walker");
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
        assert_eq!(pc.kept, vec![b"pc local walker".to_vec()]);
        assert_eq!(server.data, b"account walker");
    }

    #[test]
    fn the_holder_name_follows_the_listing() {
        let mut server = Server::default();
        holding(&mut server, "Phone", b"v1");
        let mut pc = Device::new("PC", b"");
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Phone".into()));
        server.device = "Clara's Pixel".into();
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Elsewhere("Clara's Pixel".into()));
        server.version = 0;
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Idle);
    }

    #[test]
    fn a_device_claims_the_walker_only_when_the_account_has_none() {
        let mut server = Server::default();
        let mut pc = Device::new("PC", b"local");
        pc.sync.claim();
        assert_eq!(pc.sync.hold(), &Hold::Idle);
        pc.sync(&mut server);
        pc.sync.claim();
        pc.sync(&mut server);
        assert_eq!(pc.sync.hold(), &Hold::Here);
        assert_eq!(server.data, b"local");
        let mut phone = Device::new("Phone", b"");
        phone.sync(&mut server);
        phone.sync.claim();
        phone.sync(&mut server);
        assert_eq!(phone.sync.hold(), &Hold::Elsewhere("PC".into()));
        assert_eq!(server.device, "PC");
    }

    #[test]
    fn a_local_walker_meeting_another_cloud_walker_is_kept() {
        let mut server = Server::default();
        holding(&mut server, "Phone", b"account walker");
        let mut pc = Device::new("PC", b"pc local walker");
        pc.sync.take();
        pc.sync(&mut server);
        assert_eq!(pc.kept, vec![b"pc local walker".to_vec()]);
        assert_eq!(pc.copy, b"account walker");
        assert_eq!(pc.sync.hold(), &Hold::Here);
    }

    #[test]
    fn a_full_account_does_not_retry_until_the_walker_changes() {
        let mut pc = Device::new("PC", b"v1");
        pc.sync.take();
        pc.hear(WalkerReply::Listed(None));
        assert!(pc.sync.request(&pc.copy.clone()).is_some());
        pc.hear(WalkerReply::Refused { changed: false });
        assert_eq!(pc.sync.request(&pc.copy.clone()), None);
        pc.sync.changed();
        assert!(pc.sync.request(&pc.copy.clone()).is_some());
    }

    #[test]
    fn signed_out_the_walker_is_local_and_asks_nothing() {
        let mut sync = Sync::default();
        sync.take();
        sync.changed();
        assert_eq!(sync.request(b"x"), None);
        assert!(sync.may_change() && sync.may_credit());
        sync.on();
        sync.take();
        assert!(!sync.may_credit());
        sync.off();
        assert_eq!(sync.hold(), &Hold::Local);
    }
}
