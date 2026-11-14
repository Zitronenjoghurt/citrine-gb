use citrine_gb_pkm::{Gen, Pokemon};
use citrine_gb_walker::{
    Block, CYCLES_PER_SECOND, Eeprom, End, Event, FLAG_HAS_POKEMON, FLAG_INIT, PokemonSummary,
    Repair, Walker, airtime,
};

const MASTER_ID: [u8; 4] = [0x3C, 0x5A, 0x96, 0xF0];
const POLL: u64 = CYCLES_PER_SECOND / 1000;
const ROUTE_STAGING: usize = 0xD700;
const ROUTE_SIZE: usize = 0x2900;
const TEAM_STAGING: usize = 0xD480;
const TEAM_SIZE: usize = 0x280;
const TID: u16 = 31_337;
const SID: u16 = 0x1234;
const LAST_SYNC: u32 = 0x1234_5678;

fn encode(cmd: u8, extra: u8, session: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut plain = vec![cmd, extra, 0, 0];
    plain.extend_from_slice(&session);
    plain.extend_from_slice(payload);
    let sum = checksum(&plain);
    plain[2..4].copy_from_slice(&sum.to_le_bytes());
    plain.into_iter().map(|b| b ^ 0xAA).collect()
}

fn checksum(plain: &[u8]) -> u16 {
    let mut sum: u32 = 2;
    for pair in plain.chunks(2) {
        sum += (u32::from(pair[0]) << 8) | u32::from(*pair.get(1).unwrap_or(&0));
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    sum as u16
}

#[derive(Debug, PartialEq)]
struct Reply {
    cmd: u8,
    extra: u8,
    session: [u8; 4],
    payload: Vec<u8>,
}

fn decode(wire: &[u8]) -> Reply {
    let mut plain: Vec<u8> = wire.iter().map(|b| b ^ 0xAA).collect();
    let stored = u16::from_le_bytes([plain[2], plain[3]]);
    plain[2] = 0;
    plain[3] = 0;
    assert_eq!(checksum(&plain), stored, "reply checksum");
    Reply {
        cmd: plain[0],
        extra: plain[1],
        session: [plain[4], plain[5], plain[6], plain[7]],
        payload: plain[8..].to_vec(),
    }
}

struct Game {
    walker: Walker,
    now: u64,
    session: [u8; 4],
    packed_writes: usize,
    raw_writes: usize,
}

impl Game {
    fn new(walker: Walker) -> Self {
        Self {
            walker,
            now: CYCLES_PER_SECOND,
            session: [0; 4],
            packed_writes: 0,
            raw_writes: 0,
        }
    }

    fn wait(&mut self, limit: u64) -> Option<Vec<u8>> {
        let until = self.now + limit;
        while self.now < until {
            self.now += POLL;
            if let Some(wire) = self.walker.take(self.now) {
                return Some(wire);
            }
        }
        None
    }

    fn handshake(&mut self) {
        self.walker.connect(self.now);
        let advert = self.wait(CYCLES_PER_SECOND).expect("advertisement");
        assert_eq!(advert, vec![0x56]);
        let assert_master = encode(0xFA, 0x01, MASTER_ID, &[]);
        self.walker.send(&assert_master, self.now);
        self.now += airtime(assert_master.len());
        let ack = decode(&self.wait(CYCLES_PER_SECOND / 10).expect("slave ack"));
        assert_eq!((ack.cmd, ack.extra, ack.payload.len()), (0xF8, 0x02, 0));
        self.session = std::array::from_fn(|i| ack.session[i] ^ MASTER_ID[i]);
    }

    fn request(&mut self, cmd: u8, extra: u8, payload: &[u8]) -> Option<Reply> {
        let wire = encode(cmd, extra, self.session, payload);
        self.walker.send(&wire, self.now);
        self.now += airtime(wire.len());
        let reply = self.wait(CYCLES_PER_SECOND / 10).map(|wire| decode(&wire));
        if let Some(reply) = &reply {
            let echoed = if reply.cmd == 0x04 { extra } else { 0x02 };
            assert_eq!((reply.extra, reply.session), (echoed, self.session));
        }
        reply
    }

    fn expect(&mut self, cmd: u8, extra: u8, payload: &[u8], answer: u8) -> Vec<u8> {
        let reply = self
            .request(cmd, extra, payload)
            .unwrap_or_else(|| panic!("no answer to {cmd:#04x}"));
        assert_eq!(reply.cmd, answer, "answer to {cmd:#04x}");
        reply.payload
    }

    fn write(&mut self, address: usize, data: &[u8]) {
        for (i, block) in data.chunks(128).enumerate() {
            let at = address + i * 128;
            let low = (at & 0x80) as u8;
            let extra = (at >> 8) as u8;
            match runs(block) {
                Some(stream) => {
                    let mut packed = vec![0x10, 0x80, 0x00, 0x00];
                    packed.extend(stream);
                    self.expect(low, extra, &packed, 0x04);
                    self.packed_writes += 1;
                }
                None => {
                    self.expect(low | 0x02, extra, block, 0x04);
                    self.raw_writes += 1;
                }
            }
        }
    }

    fn read(&mut self, address: usize, len: u8) -> Vec<u8> {
        let [high, low] = (address as u16).to_be_bytes();
        let data = self.expect(0x0C, 0x01, &[high, low, len], 0x0E);
        assert_eq!(data.len(), len as usize);
        data
    }
}

fn runs(block: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < block.len() {
        let flag_at = out.len();
        out.push(0);
        for bit in (0..8).rev() {
            if at >= block.len() {
                break;
            }
            let mut run = 0;
            while at > 0 && run < 18 && at + run < block.len() && block[at + run] == block[at - 1] {
                run += 1;
            }
            if run >= 3 {
                out[flag_at] |= 1 << bit;
                out.extend_from_slice(&[((run - 3) << 4) as u8, 0x00]);
                at += run;
            } else {
                out.push(block[at]);
                at += 1;
            }
        }
    }
    (out.len() <= 124).then_some(out)
}

fn pikachu() -> Pokemon {
    let mut pokemon = Pokemon::new(Gen::Four);
    pokemon.species = 25;
    pokemon.held_item = 0x00EC;
    pokemon.moves = [84, 39, 98, 0];
    pokemon.friendship = 120;
    pokemon.tid = TID;
    pokemon.sid = SID;
    pokemon.set_nickname("Sparky").unwrap();
    if let Some(party) = pokemon.party.as_mut() {
        party.level = 15;
    }
    pokemon
}

fn summary(pokemon: &Pokemon) -> PokemonSummary {
    PokemonSummary {
        species: pokemon.species,
        held_item: pokemon.held_item,
        moves: pokemon.moves,
        level: pokemon.party.as_ref().map_or(15, |party| party.level),
        flags: [0, 0],
    }
}

fn trainer_info() -> Vec<u8> {
    let mut info = vec![0; 0x68];
    info[0x00..0x04].copy_from_slice(&1u32.to_le_bytes());
    info[0x08..0x0A].copy_from_slice(&7u16.to_le_bytes());
    info[0x0C..0x0E].copy_from_slice(&TID.to_le_bytes());
    info[0x0E..0x10].copy_from_slice(&SID.to_le_bytes());
    for (i, byte) in info[0x10..0x38].iter_mut().enumerate() {
        *byte = 0x40 + i as u8;
    }
    for (i, unit) in [0x0136u16, 0x0145, 0x0138, 0x0149, 0xFFFF]
        .iter()
        .enumerate()
    {
        info[0x48 + 2 * i..0x4A + 2 * i].copy_from_slice(&unit.to_le_bytes());
    }
    info[0x5C] = 0x02;
    info[0x5E] = 0x01;
    info[0x60..0x64].copy_from_slice(&LAST_SYNC.to_be_bytes());
    info
}

fn route_upload(pokemon: &Pokemon) -> Vec<u8> {
    let mut route: Vec<u8> = (0..ROUTE_SIZE)
        .map(|i| {
            if i % 512 < 256 {
                (i / 24 * 7) as u8
            } else {
                (i * 13) as u8
            }
        })
        .collect();
    route[..16].copy_from_slice(&summary(pokemon).to_bytes());
    for (i, unit) in pokemon.nickname.iter().enumerate() {
        route[0x10 + 2 * i..0x12 + 2 * i].copy_from_slice(&unit.to_le_bytes());
    }
    route[0x26] = pokemon.friendship;
    route
}

fn team_upload() -> Vec<u8> {
    (0..TEAM_SIZE).map(|i| (i as u8) ^ 0x3C).collect()
}

fn send_pokemon(game: &mut Game, pokemon: &Pokemon) {
    game.handshake();
    let info = game.expect(0x20, 0x01, &[], 0x22);
    assert_eq!(info.len(), 0x68);
    assert_eq!(info[0x5B] & FLAG_HAS_POKEMON, 0);
    let ack = game
        .request(0x32, 0x01, &trainer_info())
        .expect("identity ack");
    assert_eq!((ack.cmd, ack.payload.len()), (0x34, 0));
    game.write(ROUTE_STAGING, &route_upload(pokemon));
    game.write(TEAM_STAGING, &team_upload());
    assert!(game.expect(0x38, 0x01, &[], 0x38).is_empty());
    assert!(game.expect(0x5A, 0x01, &[], 0x5A).is_empty());
}

fn assert_sound(eeprom: &Eeprom) {
    let mut copy = eeprom.clone();
    for block in [Block::Info, Block::Health, Block::CopyMarker] {
        assert_eq!(
            copy.repair(block),
            Repair::Intact,
            "{block:?} has two good copies"
        );
    }
}

#[test]
fn the_game_sends_a_pokemon_on_a_walk() {
    let pokemon = pikachu();
    let mut game = Game::new(Walker::new(7));
    send_pokemon(&mut game, &pokemon);
    assert!(game.packed_writes > 20 && game.raw_writes > 20);
    assert_eq!(
        game.packed_writes + game.raw_writes,
        (ROUTE_SIZE + TEAM_SIZE) / 128
    );
    let walker = &mut game.walker;

    assert_eq!(
        walker.events(),
        vec![
            Event::Connected,
            Event::WalkStarted,
            Event::Ended(End::Completed)
        ]
    );
    let info = walker.info();
    assert_eq!(
        info.flags() & (FLAG_INIT | FLAG_HAS_POKEMON),
        FLAG_INIT | FLAG_HAS_POKEMON
    );
    assert_eq!((info.tid(), info.sid()), (TID, SID));
    assert_eq!(
        info.trainer_name()[..5],
        [0x0136, 0x0145, 0x0138, 0x0149, 0xFFFF]
    );
    assert_eq!(info.identity(), &[0xFF; 0x28][..]);
    assert_eq!(walker.clock(), LAST_SYNC);
    assert_eq!(walker.walking_pokemon(), Some(summary(&pokemon)));

    let eeprom = walker.eeprom();
    assert_eq!(eeprom.read(0x8F00, ROUTE_SIZE), route_upload(&pokemon));
    assert_eq!(eeprom.read(0x8F26, 1), vec![120]);
    assert_eq!(eeprom.read(0xCC00, TEAM_SIZE), team_upload());
    assert_eq!(eeprom.read(0x016F, 1), vec![0x00]);
    assert_eq!(eeprom.read(0x00ED, 0x69), eeprom.read(0x01ED, 0x69));
    assert_sound(eeprom);
}

#[test]
fn the_walk_returns_the_pokemon_with_its_steps_and_watts() {
    let pokemon = pikachu();
    let mut game = Game::new(Walker::new(11));
    send_pokemon(&mut game, &pokemon);
    game.walker.steps(2_345);
    game.walker.events();
    game.now += 60 * CYCLES_PER_SECOND;

    game.handshake();
    let info = game.expect(0x20, 0x01, &[], 0x22);
    assert_eq!(info[0x64..0x68], 2_345u32.to_be_bytes());
    assert_ne!(info[0x5B] & FLAG_HAS_POKEMON, 0);
    assert_eq!(game.read(0xCE8A, 2), 117u16.to_be_bytes());
    assert_eq!(game.read(0x8F00, 16), summary(&pokemon).to_bytes());
    assert_eq!(game.read(0xCE8C, 0x30), vec![0; 0x30]);
    assert!(game.expect(0x4E, 0x01, &[], 0x50).is_empty());
    assert!(game.request(0xF4, 0x01, &[]).is_none());

    let walker = &mut game.walker;
    assert_eq!(
        walker.events(),
        vec![
            Event::Connected,
            Event::WalkEnded,
            Event::Ended(End::Completed)
        ]
    );
    assert_eq!(walker.walking_pokemon(), None);
    let info = walker.info();
    assert_eq!(info.flags() & (FLAG_INIT | FLAG_HAS_POKEMON), FLAG_INIT);
    assert_eq!(info.tid(), TID);
    let health = walker.health();
    assert_eq!((health.total_steps(), health.watts()), (2_345, 0));
    assert_eq!(walker.eeprom().read(0x8F00, 16), vec![0; 16]);
    assert_sound(walker.eeprom());
}

#[test]
fn a_corrupted_copy_is_repaired_before_the_game_reads_it() {
    let pokemon = pikachu();
    let mut game = Game::new(Walker::new(3));
    send_pokemon(&mut game, &pokemon);
    game.walker.steps(400);
    let mut image = game.walker.eeprom().bytes().to_vec();
    image[0x00ED + 0x0C] ^= 0xFF;
    image[0x0256 + 0x0E] ^= 0x01;
    let eeprom = Eeprom::from_bytes(&image).unwrap();
    let mut broken = eeprom.clone();
    assert!(!broken.repair_all().is_empty());

    let mut game = Game::new(Walker::from_eeprom(eeprom, 3));
    assert_sound(game.walker.eeprom());
    game.handshake();
    let info = game.expect(0x20, 0x01, &[], 0x22);
    assert_eq!(info[0x0C..0x0E], TID.to_le_bytes());
    assert_eq!(game.read(0xCE8A, 2), 0u16.to_be_bytes());
    let mut copy = game.walker.eeprom().clone();
    assert_eq!(
        copy.read_reliable(Block::Health),
        copy.read(0x0256, 0x18).into()
    );
}
