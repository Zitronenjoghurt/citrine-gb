use citrine_gb::netplay::LinkPeer;

pub(crate) const PREAMBLE: u8 = 0xFD;
const NO_DATA: u8 = 0xFE;
const PATCH_END: u8 = 0xFF;
const PATCH_PART: usize = 252;
const ROUNDS: usize = 10;
const PATIENCE: usize = 600;

pub(crate) struct Side<'a> {
    pub peer: &'a mut dyn LinkPeer,
}

impl Side<'_> {
    pub fn byte(&mut self, sent: u8) -> u8 {
        self.peer.exchange(sent)
    }

    pub fn handshake(&mut self) {
        assert_eq!(self.byte(0x01), 0x02);
        self.byte(0x00);
        self.byte(0x00);
    }

    pub fn nybble(&mut self, high: u8, value: u8) -> u8 {
        let sent = high | value;
        let mut got = (0..PATIENCE)
            .find_map(|_| {
                let reply = self.byte(sent);
                (reply & 0xF0 == high).then_some(reply & 0x0F)
            })
            .expect("the partner never answered the nybble");
        for _ in 0..ROUNDS {
            let reply = self.byte(sent);
            if reply & 0xF0 == high {
                got = reply & 0x0F;
            }
        }
        for _ in 0..ROUNDS {
            self.byte(0x00);
        }
        got
    }

    pub fn sync_bytes(&mut self, sent: u8) -> u8 {
        for _ in 0..PATIENCE {
            self.byte(sent);
            let first = self.byte(sent);
            let second = self.byte(sent);
            if let Some(found) = [first, second].into_iter().find(|b| b & 0xF0 == 0xD0) {
                return found;
            }
        }
        panic!("the partner never sent a sync byte")
    }

    pub fn exchange_bytes(&mut self, data: &[u8]) -> Vec<u8> {
        let mut received = Vec::with_capacity(data.len());
        let mut at = 0;
        let mut ignoring = true;
        let mut tries = 0;
        while received.len() < data.len() {
            let reply = self.byte(data[at]);
            if ignoring {
                tries += 1;
                assert!(tries < PATIENCE, "the partner never sent a preamble");
                ignoring = reply != PREAMBLE;
                continue;
            }
            received.push(reply);
            at += 1;
        }
        received
    }

    pub fn exchange_mail(&mut self, data: &[u8]) -> Vec<u8> {
        self.byte(data[0]);
        data.iter().map(|&b| self.byte(b)).collect()
    }
}

pub(crate) struct Party {
    pub name: Vec<u8>,
    pub count: u8,
    pub species: Vec<u8>,
    pub patched: Vec<u8>,
    pub ots: Vec<u8>,
    pub nicknames: Vec<u8>,
}

pub(crate) fn read_party(received: &[u8], patches: &[u8], name: usize, patched: usize) -> Party {
    let start = received
        .iter()
        .position(|&b| !matches!(b, 0x00 | PREAMBLE | NO_DATA))
        .expect("a name");
    let data: Vec<u8> = received[start..]
        .iter()
        .copied()
        .filter(|&b| b != NO_DATA)
        .collect();
    let (head, rest) = data.split_at(name);
    let count = rest[0];
    let species = rest[1..8].to_vec();
    let mut patched_data = rest[8..8 + patched].to_vec();
    let tail = &rest[8 + patched..];
    let mut base = 0;
    let mut parts = 2;
    for &entry in patches {
        if parts == 0 {
            break;
        }
        match entry {
            0x00 | PREAMBLE | NO_DATA => {}
            PATCH_END => {
                base = PATCH_PART;
                parts -= 1;
            }
            offset => patched_data[base + usize::from(offset) - 1] = NO_DATA,
        }
    }
    Party {
        name: head.to_vec(),
        count,
        species,
        patched: patched_data,
        ots: tail[..name * 6].to_vec(),
        nicknames: tail[name * 6..name * 12].to_vec(),
    }
}

pub(crate) fn game_block(len: usize, preamble: usize, fill: u8) -> Vec<u8> {
    let mut block = vec![PREAMBLE; preamble];
    block.resize(len, fill);
    block
}
