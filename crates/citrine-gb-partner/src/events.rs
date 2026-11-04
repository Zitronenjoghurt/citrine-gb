pub use citrine_gb_gamedata::Dvs;
use citrine_gb_gamedata::{Game, Generation, Language};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainerId {
    Fixed(u16),
    Random { low: u16, high: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Genes {
    Fixed(Dvs),
    Random,
    Shiny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trainer {
    pub languages: &'static [Language],
    pub name: &'static str,
    pub id: TrainerId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub key: &'static str,
    pub place: &'static str,
    pub year: u16,
    pub generation: Generation,
    pub dex: u16,
    pub level: u8,
    pub moves: &'static [u8],
    pub trainers: &'static [Trainer],
    pub genes: Genes,
    pub item: Option<u8>,
    pub egg: bool,
    pub source: &'static str,
}

impl Event {
    pub fn trainers_for(&self, language: Language) -> impl Iterator<Item = &Trainer> {
        self.trainers
            .iter()
            .filter(move |t| t.languages.contains(&language))
    }

    pub fn fits(&self, game: Game) -> bool {
        let generation_fits = match (self.generation, game.generation()) {
            (Generation::One, Generation::One) | (Generation::Two, Generation::Two) => true,
            (Generation::One, Generation::Two) => !self.egg,
            (Generation::Two, Generation::One) => false,
            (Generation::Three, _) | (_, Generation::Three) => false,
        };
        generation_fits && self.trainers_for(game.language).next().is_some()
    }
}

pub fn events_for(game: Game) -> impl Iterator<Item = &'static Event> {
    EVENTS.iter().filter(move |e| e.fits(game))
}

pub fn event(key: &str) -> Option<&'static Event> {
    EVENTS.iter().find(|e| e.key == key)
}

use Language::{
    English as EN, French as FR, German as DE, Italian as IT, Japanese as JP, Spanish as ES,
};

const ANY_ID: TrainerId = TrainerId::Random {
    low: 0,
    high: 65535,
};
const PCNY_ID: TrainerId = TrainerId::Random {
    low: 32,
    high: 1053,
};
const MEW_DVS: Genes = Genes::Fixed(Dvs {
    attack: 10,
    defense: 1,
    speed: 12,
    special: 5,
});
const MADRID_DVS: Genes = Genes::Fixed(Dvs {
    attack: 6,
    defense: 4,
    speed: 3,
    special: 8,
});
const ALL_15: Genes = Genes::Fixed(Dvs {
    attack: 15,
    defense: 15,
    speed: 15,
    special: 15,
});

const fn one(languages: &'static [Language], name: &'static str, id: TrainerId) -> Trainer {
    Trainer {
        languages,
        name,
        id,
    }
}

#[allow(clippy::too_many_arguments)]
const fn gen1(
    key: &'static str,
    place: &'static str,
    year: u16,
    dex: u16,
    level: u8,
    moves: &'static [u8],
    trainers: &'static [Trainer],
    genes: Genes,
    source: &'static str,
) -> Event {
    Event {
        key,
        place,
        year,
        generation: Generation::One,
        dex,
        level,
        moves,
        trainers,
        genes,
        item: None,
        egg: false,
        source,
    }
}

#[allow(clippy::too_many_arguments)]
const fn gen2(
    key: &'static str,
    place: &'static str,
    year: u16,
    dex: u16,
    level: u8,
    moves: &'static [u8],
    trainers: &'static [Trainer],
    genes: Genes,
    source: &'static str,
) -> Event {
    Event {
        key,
        place,
        year,
        generation: Generation::Two,
        dex,
        level,
        moves,
        trainers,
        genes,
        item: None,
        egg: false,
        source,
    }
}

const fn holding(event: Event, item: u8) -> Event {
    Event {
        item: Some(item),
        ..event
    }
}

const fn egg(
    key: &'static str,
    place: &'static str,
    year: u16,
    dex: u16,
    moves: &'static [u8],
    trainers: &'static [Trainer],
    source: &'static str,
) -> Event {
    Event {
        key,
        place,
        year,
        generation: Generation::Two,
        dex,
        level: 5,
        moves,
        trainers,
        genes: Genes::Random,
        item: None,
        egg: true,
        source,
    }
}

mod moves {
    pub const POUND: u8 = 1;
    pub const COMET_PUNCH: u8 = 4;
    pub const SCRATCH: u8 = 10;
    pub const SWORDS_DANCE: u8 = 14;
    pub const GUST: u8 = 16;
    pub const WING_ATTACK: u8 = 17;
    pub const FLY: u8 = 19;
    pub const DOUBLE_KICK: u8 = 24;
    pub const SAND_ATTACK: u8 = 28;
    pub const TACKLE: u8 = 33;
    pub const TAIL_WHIP: u8 = 39;
    pub const POISON_STING: u8 = 40;
    pub const LEER: u8 = 43;
    pub const BITE: u8 = 44;
    pub const GROWL: u8 = 45;
    pub const ROAR: u8 = 46;
    pub const SING: u8 = 47;
    pub const EMBER: u8 = 52;
    pub const FLAMETHROWER: u8 = 53;
    pub const MIST: u8 = 54;
    pub const WATER_GUN: u8 = 55;
    pub const SURF: u8 = 57;
    pub const ICE_BEAM: u8 = 58;
    pub const DRILL_PECK: u8 = 65;
    pub const SUBMISSION: u8 = 66;
    pub const COUNTER: u8 = 68;
    pub const LEECH_SEED: u8 = 73;
    pub const RAZOR_LEAF: u8 = 75;
    pub const POISON_POWDER: u8 = 77;
    pub const SLEEP_POWDER: u8 = 79;
    pub const PETAL_DANCE: u8 = 80;
    pub const THUNDER_SHOCK: u8 = 84;
    pub const THUNDER_WAVE: u8 = 86;
    pub const EARTHQUAKE: u8 = 89;
    pub const CONFUSION: u8 = 93;
    pub const MEDITATE: u8 = 96;
    pub const AGILITY: u8 = 97;
    pub const QUICK_ATTACK: u8 = 98;
    pub const RAGE: u8 = 99;
    pub const NIGHT_SHADE: u8 = 101;
    pub const RECOVER: u8 = 105;
    pub const HARDEN: u8 = 106;
    pub const WITHDRAW: u8 = 110;
    pub const DEFENSE_CURL: u8 = 111;
    pub const METRONOME: u8 = 118;
    pub const LICK: u8 = 122;
    pub const SWIFT: u8 = 129;
    pub const AMNESIA: u8 = 133;
    pub const SLASH: u8 = 163;
    pub const MIND_READER: u8 = 170;
    pub const FLAME_WHEEL: u8 = 172;
    pub const AEROBLAST: u8 = 177;
    pub const PROTECT: u8 = 182;
    pub const SCARY_FACE: u8 = 184;
    pub const BELLY_DRUM: u8 = 187;
    pub const DETECT: u8 = 197;
    pub const ENDURE: u8 = 203;
    pub const CHARM: u8 = 204;
    pub const HEAL_BELL: u8 = 215;
    pub const SAFEGUARD: u8 = 219;
    pub const SACRED_FIRE: u8 = 221;
    pub const BATON_PASS: u8 = 226;
    pub const ENCORE: u8 = 227;
    pub const RAPID_SPIN: u8 = 229;
    pub const SWEET_SCENT: u8 = 230;
    pub const CRUNCH: u8 = 242;
    pub const ANCIENT_POWER: u8 = 246;
}

use moves::*;

const GOLD_BERRY: u8 = 0xAE;
const MYSTERY_BERRY: u8 = 0x96;
const GORGEOUS_BOX: u8 = 168;

const PCNY: &[Trainer] = &[
    one(&[EN], "PCNYa", PCNY_ID),
    one(&[EN], "PCNYb", PCNY_ID),
    one(&[EN], "PCNYc", PCNY_ID),
    one(&[EN], "PCNYd", PCNY_ID),
];
const PCNY_EGG: &[Trainer] = &[one(&[EN], "PCNY", ANY_ID)];
const POKEMON_CENTER_EGG: &[Trainer] = &[one(&[JP], "ポケセン", ANY_ID)];
const STADIUM: &[Trainer] = &[
    one(&[JP], "スタジアム", TrainerId::Fixed(1999)),
    one(&[EN], "STADIUM", TrainerId::Fixed(2000)),
];
const STADIUM_2: &[Trainer] = &[
    one(&[JP], "スタジアム", TrainerId::Fixed(2000)),
    one(&[EN], "Stadium", TrainerId::Fixed(2000)),
    one(&[FR], "Stade", TrainerId::Fixed(2001)),
    one(&[DE], "Stadion", TrainerId::Fixed(2001)),
    one(&[IT], "Stadio", TrainerId::Fixed(2001)),
    one(&[ES], "Estadio", TrainerId::Fixed(2001)),
];

const POKEWIKI_JP_1: &str = "https://www.pokewiki.de/Events/1._Generation/Japan";
const POKEWIKI_NA_1: &str = "https://www.pokewiki.de/Events/1._Generation/Nordamerika";
const POKEWIKI_EU_1: &str = "https://www.pokewiki.de/Events/1._Generation/Europa";
const POKEBIP_EU_1: &str = "https://www.pokebip.com/page/jeuxvideo/evendex/event-europe-1g";
const WIKIDEX_1: &str =
    "https://www.wikidex.net/wiki/Lista_de_eventos_de_la_primera_generaci%C3%B3n";
const POKELOVE_ASH: &str =
    "https://www.pokelove.fr/2026/08/evenement-1g-mew-club-nintendo-amerique.html";
const SEREBII_MEW: &str = "https://www.serebii.net/events/dex/151.shtml";
const STADIUM_PRIZES: &str = "https://www.serebii.net/stadium/prizes.shtml";
const POKEBIP_JP_2: &str = "https://www.pokebip.com/page/jeuxvideo/evendex/event-japon-2g";
const POKEBIP_EU_2: &str = "https://www.pokebip.com/page/jeuxvideo/evendex/event-europe-2g";
const PCNY_DOCS: &str = "https://pokemonhistorian.com/pcny-project/pcny-documentation-gen2/";
const STADIUM_2_PRIZES: &str = "https://www.serebii.net/stadium2/prizes.shtml";

const MEW: u16 = 151;
const PIKACHU: u16 = 25;
const MEW_MOVES: &[u8] = &[POUND];

pub static EVENTS: &[Event] = &[
    gen1(
        "mew-corocoro-1996",
        "CoroCoro Comic, Japan",
        1996,
        MEW,
        5,
        MEW_MOVES,
        &[one(
            &[JP],
            "コロコロ",
            TrainerId::Random { low: 1, high: 20 },
        )],
        MEW_DVS,
        POKEWIKI_JP_1,
    ),
    gen1(
        "mew-space-world-1997",
        "Space World, Makuhari",
        1997,
        MEW,
        5,
        MEW_MOVES,
        &[
            one(&[JP], "マリオ", ANY_ID),
            one(&[JP], "クッパ", ANY_ID),
            one(&[JP], "ルイージ", ANY_ID),
            one(&[JP], "ピーチ", ANY_ID),
            one(&[JP], "ヨッシー", ANY_ID),
            one(&[JP], "ドンキー", ANY_ID),
        ],
        MEW_DVS,
        POKEWIKI_JP_1,
    ),
    gen1(
        "mew-hobby-fair-1997",
        "Hobby Fair, Japan",
        1997,
        MEW,
        5,
        MEW_MOVES,
        &[
            one(&[JP], "フクオカ", ANY_ID),
            one(&[JP], "トウキョー", ANY_ID),
            one(&[JP], "オーサカ", ANY_ID),
            one(&[JP], "サッポロ", ANY_ID),
            one(&[JP], "ナゴヤ", ANY_ID),
        ],
        MEW_DVS,
        POKEWIKI_JP_1,
    ),
    gen1(
        "mew-space-world-1999",
        "Space World, Makuhari",
        1999,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[JP], "マクハリ", ANY_ID)],
        MEW_DVS,
        POKEWIKI_JP_1,
    ),
    gen1(
        "mew-game-freak-2016",
        "Virtual Console release",
        2016,
        MEW,
        5,
        MEW_MOVES,
        &[
            one(&[JP], "ゲーフリ", TrainerId::Fixed(22796)),
            one(&[EN], "GF", TrainerId::Fixed(22796)),
        ],
        ALL_15,
        SEREBII_MEW,
    ),
    gen1(
        "mew-nintendo-power-1998",
        "Nintendo Power, United States",
        1998,
        MEW,
        5,
        MEW_MOVES,
        &[one(
            &[EN],
            "MARIO",
            TrainerId::Random {
                low: 51259,
                high: 51352,
            },
        )],
        MEW_DVS,
        WIKIDEX_1,
    ),
    gen1(
        "mew-training-tour-1999",
        "Pokémon League Training Tour, United States",
        1999,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[EN], "YOSHIRA", ANY_ID), one(&[EN], "YOSHIRB", ANY_ID)],
        MEW_DVS,
        POKEWIKI_NA_1,
    ),
    gen1(
        "mew-stadium-tour-2000",
        "Pokémon 2000 Stadium Tour, United States",
        2000,
        MEW,
        5,
        MEW_MOVES,
        &[
            one(&[EN], "LINKE", ANY_ID),
            one(&[EN], "LINKW", ANY_ID),
            one(&[EN], "LUIGE", ANY_ID),
            one(&[EN], "LUIGW", ANY_ID),
        ],
        MEW_DVS,
        POKEBIP_EU_1,
    ),
    gen1(
        "mew-club-nintendo-2002",
        "Club Nintendo, Mexico",
        2002,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[EN], "ASH", TrainerId::Fixed(45515))],
        Genes::Random,
        POKELOVE_ASH,
    ),
    gen1(
        "mew-poke-tour-1999",
        "Poké Tour, Australia",
        1999,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[EN], "AUS", ANY_ID), one(&[EN], "NAL", ANY_ID)],
        MEW_DVS,
        POKEBIP_EU_1,
    ),
    gen1(
        "mew-uk-tour-2000",
        "Nintendo Official Magazine Tour, United Kingdom",
        2000,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[EN], "UK", ANY_ID)],
        MEW_DVS,
        POKEBIP_EU_1,
    ),
    gen1(
        "mew-paris-2000",
        "Pokémon tour, Paris",
        2000,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[FR], "FRANCE", ANY_ID)],
        MEW_DVS,
        POKEBIP_EU_1,
    ),
    gen1(
        "mew-germany-2000",
        "Christmas tour, Germany",
        2000,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[DE], "GERMANY", ANY_ID)],
        Genes::Random,
        POKEWIKI_EU_1,
    ),
    gen1(
        "mew-madrid-2000",
        "Pokémon tour, Madrid",
        2000,
        MEW,
        5,
        MEW_MOVES,
        &[one(&[ES], "D-J", TrainerId::Fixed(60130))],
        MADRID_DVS,
        POKEBIP_EU_1,
    ),
    gen1(
        "pikachu-surf-corocoro-1997",
        "CoroCoro Comic, Japan",
        1997,
        PIKACHU,
        5,
        &[THUNDER_SHOCK, GROWL, SURF],
        &[one(&[JP], "コロコロ", ANY_ID)],
        Genes::Random,
        WIKIDEX_1,
    ),
    gen1(
        "pikachu-fly-corocoro-1997",
        "CoroCoro Comic, Japan",
        1997,
        PIKACHU,
        5,
        &[THUNDER_SHOCK, GROWL, FLY],
        &[one(&[JP], "コロコロ", ANY_ID)],
        Genes::Random,
        WIKIDEX_1,
    ),
    gen1(
        "pikachu-nintendo-64-1997",
        "Nintendo 64 launch, Japan",
        1997,
        PIKACHU,
        5,
        &[THUNDER_SHOCK, GROWL, SURF],
        &[one(&[JP], "ニンテン", ANY_ID)],
        Genes::Random,
        WIKIDEX_1,
    ),
    gen1(
        "pikachu-nintendo-power-1999",
        "Nintendo Power, United States",
        1999,
        PIKACHU,
        5,
        &[THUNDER_SHOCK, GROWL, SURF],
        &[one(&[EN], "NINTEN", ANY_ID)],
        Genes::Random,
        WIKIDEX_1,
    ),
    stadium("bulbasaur-stadium", 1, 5, &[TACKLE, GROWL]),
    stadium("charmander-stadium", 4, 5, &[SCRATCH, LEER]),
    stadium("squirtle-stadium", 7, 5, &[TACKLE, TAIL_WHIP]),
    stadium("hitmonlee-stadium", 106, 20, &[DOUBLE_KICK, MEDITATE]),
    stadium("hitmonchan-stadium", 107, 20, &[COMET_PUNCH, AGILITY]),
    stadium("eevee-stadium", 133, 25, &[TACKLE, SAND_ATTACK]),
    stadium("omanyte-stadium", 138, 20, &[WATER_GUN, WITHDRAW]),
    stadium("kabuto-stadium", 140, 20, &[SCRATCH, HARDEN]),
    stadium("psyduck-stadium", 54, 15, &[SCRATCH, AMNESIA]),
    gen2(
        "celebi-space-world-2000",
        "Space World, Makuhari",
        2000,
        CELEBI,
        5,
        CELEBI_MOVES,
        &[
            one(&[JP], "スペース", ANY_ID),
            one(&[JP], "ワールド", ANY_ID),
        ],
        Genes::Random,
        POKEBIP_JP_2,
    ),
    gen2(
        "celebi-hobby-fair-2001",
        "Hobby Fair, Tokyo",
        2001,
        CELEBI,
        5,
        CELEBI_MOVES,
        &[one(&[JP], "トウキョー", ANY_ID)],
        Genes::Random,
        POKEBIP_JP_2,
    ),
    gen2(
        "celebi-tour-2001",
        "Celebi Tour, Europe",
        2001,
        CELEBI,
        5,
        CELEBI_MOVES,
        &[
            one(&[EN, DE, FR, ES], "FRANCO", ANY_ID),
            one(&[EN, DE, FR, ES], "JENS", ANY_ID),
            one(&[EN, DE, FR, ES], "NICO", ANY_ID),
        ],
        Genes::Random,
        POKEBIP_EU_2,
    ),
    gen2(
        "celebi-fun-fest-2001",
        "Pokémon Fun Fest, United States",
        2001,
        CELEBI,
        5,
        CELEBI_MOVES,
        &[
            one(&[EN], "FLAGSA", ANY_ID),
            one(&[EN], "FLAGSB", ANY_ID),
            one(&[EN], "FLAGSC", ANY_ID),
            one(&[EN], "FLAGSD", ANY_ID),
            one(&[EN], "FLAGSE", ANY_ID),
        ],
        Genes::Random,
        PCNY_DOCS,
    ),
    gen2(
        "celebi-westfield-2001",
        "Westfield Tour, Australia",
        2001,
        CELEBI,
        5,
        CELEBI_MOVES,
        &[one(&[EN], "AUS", ANY_ID)],
        Genes::Random,
        POKEBIP_EU_2,
    ),
    gen2(
        "celebi-sweden-2002",
        "Celebi Tour, Sweden",
        2002,
        CELEBI,
        5,
        CELEBI_MOVES,
        &[one(&[EN], "SWEDEN", ANY_ID)],
        Genes::Random,
        POKEBIP_EU_2,
    ),
    gen2(
        "celebi-pcny",
        "Pokémon Center New York",
        2002,
        CELEBI,
        5,
        CELEBI_MOVES,
        PCNY,
        Genes::Random,
        PCNY_DOCS,
    ),
    pcny(
        "articuno-pcny",
        144,
        50,
        &[MIST, AGILITY, MIND_READER, ICE_BEAM],
    ),
    pcny(
        "zapdos-pcny",
        145,
        50,
        &[THUNDER_WAVE, AGILITY, DETECT, DRILL_PECK],
    ),
    pcny(
        "moltres-pcny",
        146,
        50,
        &[FLAME_WHEEL, AGILITY, ENDURE, FLAMETHROWER],
    ),
    pcny(
        "raikou-pcny",
        243,
        40,
        &[LEER, THUNDER_SHOCK, ROAR, QUICK_ATTACK],
    ),
    pcny("entei-pcny", 244, 40, &[LEER, EMBER, ROAR, FLAME_WHEEL]),
    pcny("suicune-pcny", 245, 40, &[LEER, WATER_GUN, ROAR, GUST]),
    pcny(
        "venusaur-pcny",
        3,
        40,
        &[POISON_POWDER, SLEEP_POWDER, RAZOR_LEAF, SWEET_SCENT],
    ),
    pcny(
        "charizard-pcny",
        6,
        40,
        &[RAGE, SCARY_FACE, FLAMETHROWER, WING_ATTACK],
    ),
    pcny(
        "blastoise-pcny",
        9,
        40,
        &[WATER_GUN, BITE, RAPID_SPIN, PROTECT],
    ),
    pcny(
        "lugia-pcny",
        249,
        40,
        &[AEROBLAST, SAFEGUARD, GUST, RECOVER],
    ),
    pcny(
        "ho-oh-pcny",
        250,
        40,
        &[SACRED_FIRE, SAFEGUARD, GUST, RECOVER],
    ),
    holding(
        gen2(
            "farfetchd-stadium-2",
            "Pokémon Stadium 2 prize",
            2000,
            83,
            5,
            &[BATON_PASS, AGILITY, SWORDS_DANCE, SLASH],
            STADIUM_2,
            Genes::Random,
            STADIUM_2_PRIZES,
        ),
        GOLD_BERRY,
    ),
    holding(
        gen2(
            "gligar-stadium-2",
            "Pokémon Stadium 2 prize",
            2000,
            207,
            5,
            &[EARTHQUAKE, POISON_STING, WING_ATTACK, COUNTER],
            STADIUM_2,
            Genes::Random,
            STADIUM_2_PRIZES,
        ),
        MYSTERY_BERRY,
    ),
    pcny_egg("psyduck-egg-pcny", 54, &[SCRATCH, TAIL_WHIP, PETAL_DANCE]),
    pcny_egg("chikorita-egg-pcny", 152, &[TACKLE, GROWL, PETAL_DANCE]),
    pcny_egg("pichu-egg-pcny", 172, &[THUNDER_SHOCK, CHARM, PETAL_DANCE]),
    pcny_egg("cleffa-egg-pcny", 173, &[POUND, CHARM, ENCORE, PETAL_DANCE]),
    pcny_egg(
        "igglybuff-egg-pcny",
        174,
        &[SING, CHARM, DEFENSE_CURL, PETAL_DANCE],
    ),
    pcny_egg("smoochum-egg-pcny", 238, &[POUND, LICK, PETAL_DANCE]),
    jp_egg("chikorita-egg-jp", 152, &[TACKLE, GROWL, PETAL_DANCE]),
    jp_egg("pichu-egg-jp", 172, &[THUNDER_SHOCK, CHARM, SING]),
    jp_egg("cleffa-egg-jp", 173, &[POUND, CHARM, ENCORE, SWIFT]),
    jp_egg("wooper-egg-jp", 194, &[WATER_GUN, TAIL_WHIP, BELLY_DRUM]),
    jp_egg("phanpy-egg-jp", 231, &[TACKLE, GROWL, ENCORE]),
    jp_egg("smoochum-egg-jp", 238, &[POUND, LICK, METRONOME]),
    jp_egg("bulbasaur-egg-jp", 1, &[TACKLE, GROWL, ANCIENT_POWER]),
    jp_egg("charmander-egg-jp", 4, &[SCRATCH, GROWL, CRUNCH]),
    jp_egg("totodile-egg-jp", 158, &[SCRATCH, LEER, SUBMISSION]),
    jp_egg("hoothoot-egg-jp", 163, &[TACKLE, LEER, NIGHT_SHADE]),
];

const CELEBI: u16 = 251;
const CELEBI_MOVES: &[u8] = &[LEECH_SEED, CONFUSION, HEAL_BELL, RECOVER];

const fn stadium(key: &'static str, dex: u16, level: u8, moves: &'static [u8]) -> Event {
    holding(
        gen1(
            key,
            "Pokémon Stadium prize",
            1999,
            dex,
            level,
            moves,
            STADIUM,
            Genes::Random,
            STADIUM_PRIZES,
        ),
        GORGEOUS_BOX,
    )
}

const fn pcny(key: &'static str, dex: u16, level: u8, moves: &'static [u8]) -> Event {
    gen2(
        key,
        "Pokémon Center New York",
        2002,
        dex,
        level,
        moves,
        PCNY,
        Genes::Shiny,
        PCNY_DOCS,
    )
}

const fn pcny_egg(key: &'static str, dex: u16, moves: &'static [u8]) -> Event {
    egg(
        key,
        "Pokémon Center New York, Spring into Spring",
        2002,
        dex,
        moves,
        PCNY_EGG,
        PCNY_DOCS,
    )
}

const fn jp_egg(key: &'static str, dex: u16, moves: &'static [u8]) -> Event {
    egg(
        key,
        "Pokémon Center Mystery Egg, Japan",
        2002,
        dex,
        moves,
        POKEMON_CENTER_EGG,
        POKEBIP_JP_2,
    )
}
