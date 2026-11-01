use crate::dex::Dex;
use crate::events::{Dvs, Event, Genes, Trainer, TrainerId};
use crate::protocol::{Block, NO_DATA, PREAMBLE, Speech, Sync};
use crate::rng::Rng;
use crate::{OfferError, gen1_index};
use citrine_gb_gamedata::{Game, Generation, Language, encode_name};

pub(crate) const PARTY_LENGTH: usize = 6;
const GEN1_MON: usize = 44;
const GEN2_MON: usize = 48;
const PATCH_LIST: usize = 200;
const PATCH_PART: usize = 252;
const PATCH_START: usize = 10;
const PATCH_END: u8 = 0xFF;
const RN_PREAMBLE: usize = 7;
const RNS: usize = 10;
const PADDING: usize = 3;
const MAIL_PREAMBLE: u8 = 0x20;
const MAIL_PREAMBLE_LENGTH: usize = 5;
const MAIL_MESSAGE: usize = 33;
const EGG: u8 = 0xFD;
const BASE_HAPPINESS: u8 = 70;
const SHINY_ATTACK: [u8; 8] = [2, 3, 6, 7, 10, 11, 14, 15];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    pub species: u8,
    pub ot_id: u16,
    pub ot_name: Vec<u8>,
    pub trainer: &'static str,
    pub nickname: Vec<u8>,
    pub mon: Vec<u8>,
    pub dvs: Dvs,
}

pub(crate) struct Context {
    pub game: Game,
    pub speech: Speech,
    pub dex: Dex,
    pub pp: Vec<u8>,
}

impl Context {
    pub fn name_length(&self) -> usize {
        self.game.name_length()
    }

    fn data_generation(&self) -> Generation {
        match self.speech {
            Speech::Gen1 => Generation::One,
            Speech::Gen2 => Generation::Two,
        }
    }

    fn mail_metadata(&self) -> usize {
        match self.game.language {
            Language::Japanese => 9,
            _ => 14,
        }
    }

    fn mail_message(&self) -> usize {
        match self.game.language {
            Language::Korean => 65,
            _ => MAIL_MESSAGE,
        }
    }
}

pub(crate) fn offer(ctx: &Context, event: &Event, rng: &mut Rng) -> Result<Offered, OfferError> {
    let entry = ctx.dex.get(event.dex).ok_or(OfferError::Species)?;
    let species = match ctx.speech {
        Speech::Gen1 if ctx.game.generation() == Generation::Two => {
            gen1_index(event.dex).ok_or(OfferError::Species)?
        }
        _ => entry.index,
    };
    let base = entry.base;
    let trainers: Vec<&Trainer> = event.trainers_for(ctx.game.language).collect();
    if trainers.is_empty() {
        return Err(OfferError::Language);
    }
    let trainer = trainers[rng.below(trainers.len())];
    let length = ctx.name_length();
    let ot_name = encode_name(ctx.game, trainer.name, length).ok_or(OfferError::Name)?;
    let nickname = if event.egg {
        encode_name(ctx.game, egg_name(ctx.game.language), length).ok_or(OfferError::Name)?
    } else {
        encode_name(ctx.game, &entry.name, length).ok_or(OfferError::Name)?
    };
    let ot_id = match trainer.id {
        TrainerId::Fixed(id) => id,
        TrainerId::Random { low, high } => low + rng.below(usize::from(high - low) + 1) as u16,
    };
    let dvs = match event.genes {
        Genes::Fixed(dvs) => dvs,
        Genes::Random => Dvs::from_bytes(rng.byte(), rng.byte()),
        Genes::Shiny => Dvs {
            attack: SHINY_ATTACK[rng.below(SHINY_ATTACK.len())],
            defense: 10,
            speed: 10,
            special: 10,
        },
    };
    let level = event.level;
    let stats = base.stats(level, dvs, [0; 5]);
    let exp = base.growth_rate.exp_at(level);
    let mut moves = [0u8; 4];
    let mut pp = [0u8; 4];
    for (i, &id) in event.moves.iter().take(4).enumerate() {
        moves[i] = id;
        pp[i] = *ctx.pp.get(usize::from(id)).ok_or(OfferError::Moves)?;
        if pp[i] == 0 {
            return Err(OfferError::Moves);
        }
    }
    let [dv_high, dv_low] = dv_bytes(dvs);
    let be = |v: u16| v.to_be_bytes();
    let exp = exp.to_be_bytes();
    let mut mon = Vec::new();
    match ctx.data_generation() {
        Generation::One => {
            mon.push(species);
            mon.extend(be(stats.max_hp));
            mon.push(level);
            mon.push(0);
            mon.extend(base.types);
            mon.push(event.item.unwrap_or(base.catch_rate));
            mon.extend(moves);
            mon.extend(be(ot_id));
            mon.extend(&exp[1..]);
            mon.extend([0; 10]);
            mon.extend([dv_high, dv_low]);
            mon.extend(pp);
            mon.push(level);
            mon.extend(be(stats.max_hp));
            mon.extend(be(stats.attack));
            mon.extend(be(stats.defense));
            mon.extend(be(stats.speed));
            mon.extend(be(stats.special_attack));
            debug_assert_eq!(mon.len(), GEN1_MON);
        }
        Generation::Two => {
            mon.push(species);
            mon.push(event.item.unwrap_or(0));
            mon.extend(moves);
            mon.extend(be(ot_id));
            mon.extend(&exp[1..]);
            mon.extend([0; 10]);
            mon.extend([dv_high, dv_low]);
            mon.extend(pp);
            mon.push(if event.egg {
                base.egg_cycles.ok_or(OfferError::Species)?
            } else {
                BASE_HAPPINESS
            });
            mon.push(0);
            mon.extend([0, 0]);
            mon.push(level);
            mon.push(0);
            mon.push(0);
            mon.extend(be(if event.egg { 0 } else { stats.max_hp }));
            mon.extend(be(stats.max_hp));
            mon.extend(be(stats.attack));
            mon.extend(be(stats.defense));
            mon.extend(be(stats.speed));
            mon.extend(be(stats.special_attack));
            mon.extend(be(stats.special_defense));
            debug_assert_eq!(mon.len(), GEN2_MON);
        }
    }
    Ok(Offered {
        species: if event.egg { EGG } else { species },
        ot_id,
        ot_name,
        trainer: trainer.name,
        nickname,
        mon,
        dvs,
    })
}

fn dv_bytes(dvs: Dvs) -> [u8; 2] {
    [
        (dvs.attack << 4) | dvs.defense,
        (dvs.speed << 4) | dvs.special,
    ]
}

fn egg_name(language: Language) -> &'static str {
    match language {
        Language::Japanese => "タマゴ",
        Language::German => "EI",
        Language::French => "OEUF",
        Language::Italian => "UOVO",
        Language::Spanish => "HUEVO",
        Language::English | Language::Korean => "EGG",
    }
}

pub(crate) fn blocks(ctx: &Context, offered: &Offered, rng: &mut Rng) -> Vec<Block> {
    let length = ctx.name_length();
    let mut rn = vec![PREAMBLE; RN_PREAMBLE];
    rn.extend((0..RNS).map(|_| rng.byte() % PREAMBLE));

    let mon_size = offered.mon.len();
    let mut patched = Vec::new();
    if ctx.speech == Speech::Gen2 {
        patched.extend(offered.ot_id.to_be_bytes());
    }
    patched.extend(&offered.mon);
    patched.resize(patched.len() + mon_size * (PARTY_LENGTH - 1), 0);
    let patches = patch(&mut patched);

    let mut party = vec![PREAMBLE; 6];
    party.extend(&offered.ot_name);
    party.push(1);
    party.push(offered.species);
    party.push(0xFF);
    party.extend([0; PARTY_LENGTH - 1]);
    party.extend(&patched);
    let mut names = offered.ot_name.clone();
    names.resize(length * PARTY_LENGTH, 0);
    party.extend(&names);
    let mut names = offered.nickname.clone();
    names.resize(length * PARTY_LENGTH, 0);
    party.extend(&names);
    party.extend([0; PADDING]);

    let mut list = vec![PREAMBLE; 3];
    list.resize(PATCH_START, 0);
    list.extend(patches);
    list.resize(PATCH_LIST, 0);

    let mut blocks = vec![
        Block {
            data: rn,
            sync: Sync::Preamble,
        },
        Block {
            data: party,
            sync: Sync::Preamble,
        },
        Block {
            data: list,
            sync: Sync::Preamble,
        },
    ];
    if ctx.speech == Speech::Gen2 && ctx.game.generation() == Generation::Two {
        let meta = ctx.mail_metadata();
        let mut mail = vec![MAIL_PREAMBLE; MAIL_PREAMBLE_LENGTH];
        mail.resize(mail.len() + ctx.mail_message() * PARTY_LENGTH, 0);
        mail.resize(mail.len() + meta * PARTY_LENGTH, 0);
        mail.push(PATCH_END);
        mail.resize(mail.len() + (meta + 3) * PARTY_LENGTH, 0);
        blocks.push(Block {
            data: mail,
            sync: Sync::FirstByte,
        });
    }
    blocks
}

fn patch(data: &mut [u8]) -> Vec<u8> {
    let mut list = Vec::new();
    let (first, second) = data.split_at_mut(PATCH_PART.min(data.len()));
    for part in [first, second] {
        for (i, byte) in part.iter_mut().enumerate() {
            if *byte == NO_DATA {
                *byte = 0xFF;
                list.push(i as u8 + 1);
            }
        }
        list.push(PATCH_END);
    }
    list
}
