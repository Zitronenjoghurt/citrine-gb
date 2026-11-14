use std::fmt::Write;
use std::str::FromStr;

use super::{BaseStats, Encounter, EncounterMethod, Evolution, Game, LevelMove, Place, Tables};
use crate::pkm::Language;
use crate::stats::Stats;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TsvError {
    #[error("{file} line {line} is broken")]
    Broken { file: &'static str, line: usize },
}

const BASE_STATS: &str = "base_stats.tsv";
const LEARNSETS: &str = "learnsets.tsv";
const EVOLUTIONS: &str = "evolutions.tsv";
const ENCOUNTERS: &str = "encounters.tsv";
const SPECIES_NAMES: &str = "species_names.tsv";
const PLACES: &str = "places.tsv";
const GROWTH: &str = "growth.tsv";
const MOVES: &str = "moves.tsv";

pub(super) fn write(tables: &Tables) -> Vec<(&'static str, String)> {
    let mut base = String::from(
        "species\thp\tatk\tdef\tspe\tspa\tspd\ttype1\ttype2\tcatch_rate\tbase_exp\tgender_ratio\tegg_cycles\tfriendship\tgrowth\tegg_group1\tegg_group2\tability1\tability2\tability3\tform_count\tform_index\tev_hp\tev_atk\tev_def\tev_spe\tev_spa\tev_spd\n",
    );
    for (species, b) in tables.base_stats.iter().enumerate() {
        let mut row = vec![species.to_string()];
        row.extend(b.stats.to_array().map(|s| s.to_string()));
        row.extend(b.types.map(|t| t.to_string()));
        row.push(b.catch_rate.to_string());
        row.push(b.base_exp.to_string());
        row.extend(
            [b.gender_ratio, b.egg_cycles, b.base_friendship, b.growth].map(|v| v.to_string()),
        );
        row.extend(b.egg_groups.map(|g| g.to_string()));
        row.extend(b.abilities.map(|a| a.to_string()));
        row.push(b.form_count.to_string());
        row.push(b.form_index.to_string());
        row.extend(b.ev_yield.to_array().map(|e| e.to_string()));
        let _ = writeln!(base, "{}", row.join("\t"));
    }
    let mut learnsets = String::from("species\tlevel\tmove\n");
    for (species, moves) in tables.learnsets.iter().enumerate() {
        for m in moves {
            let _ = writeln!(learnsets, "{species}\t{}\t{}", m.level, m.move_id);
        }
    }
    let mut evolutions = String::from("species\tmethod\tparam\tinto\n");
    for (species, evos) in tables.evolutions.iter().enumerate() {
        for e in evos {
            let _ = writeln!(
                evolutions,
                "{species}\t{}\t{}\t{}",
                e.method, e.param, e.into
            );
        }
    }
    let mut encounters =
        String::from("place\tseason\tmethod\tslot\tspecies\tform\tmin_level\tmax_level\n");
    for e in &tables.encounters {
        let season = e.season.map_or(String::from("-"), |s| s.to_string());
        let _ = writeln!(
            encounters,
            "{}\t{season}\t{:?}\t{}\t{}\t{}\t{}\t{}",
            e.place, e.method, e.slot, e.species, e.form, e.min_level, e.max_level
        );
    }
    let language = format!("{:?}", tables.language);
    let mut names = String::from("language\tspecies\tname\n");
    for (species, name) in tables.species_names.iter().enumerate() {
        let _ = writeln!(names, "{language}\t{species}\t{name}");
    }
    let mut places = String::from("header\tencounters\tlocation\tarea_icon\tbattle_background\n");
    for p in &tables.places {
        let file = p.encounters.map_or(String::from("-"), |f| f.to_string());
        let _ = writeln!(
            places,
            "{}\t{file}\t{}\t{}\t{}",
            p.header, p.location, p.area_icon, p.battle_background
        );
    }
    let mut growth = String::from("growth\tlevel\texp\n");
    for (curve, levels) in tables.growth.iter().enumerate() {
        for (level, exp) in levels.iter().enumerate() {
            let _ = writeln!(growth, "{curve}\t{level}\t{exp}");
        }
    }
    let mut moves = String::from("move\tpp\n");
    for (id, pp) in tables.pp.iter().enumerate() {
        let _ = writeln!(moves, "{id}\t{pp}");
    }
    vec![
        (BASE_STATS, base),
        (LEARNSETS, learnsets),
        (EVOLUTIONS, evolutions),
        (ENCOUNTERS, encounters),
        (SPECIES_NAMES, names),
        (PLACES, places),
        (GROWTH, growth),
        (MOVES, moves),
    ]
}

struct Rows<'a> {
    file: &'static str,
    lines: std::iter::Skip<std::iter::Enumerate<std::str::Lines<'a>>>,
}

struct Row<'a> {
    file: &'static str,
    line: usize,
    cells: std::str::Split<'a, char>,
}

impl<'a> Rows<'a> {
    fn new(file: &'static str, text: &'a str) -> Self {
        Self {
            file,
            lines: text.lines().enumerate().skip(1),
        }
    }
}

impl<'a> Iterator for Rows<'a> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Row<'a>> {
        let (index, line) = self.lines.next()?;
        Some(Row {
            file: self.file,
            line: index + 1,
            cells: line.split('\t'),
        })
    }
}

impl Row<'_> {
    fn broken(&self) -> TsvError {
        TsvError::Broken {
            file: self.file,
            line: self.line,
        }
    }

    fn text(&mut self) -> Result<&str, TsvError> {
        let broken = self.broken();
        self.cells.next().ok_or(broken)
    }

    fn get<T: FromStr>(&mut self) -> Result<T, TsvError> {
        let broken = self.broken();
        self.text()?.parse().map_err(|_| broken)
    }

    fn optional<T: FromStr>(&mut self) -> Result<Option<T>, TsvError> {
        let broken = self.broken();
        match self.text()? {
            "-" => Ok(None),
            text => text.parse().map(Some).map_err(|_| broken),
        }
    }

    fn array<T: FromStr + Copy + Default, const N: usize>(&mut self) -> Result<[T; N], TsvError> {
        let mut out = [T::default(); N];
        for cell in &mut out {
            *cell = self.get()?;
        }
        Ok(out)
    }
}

fn grow<T: Default>(list: &mut Vec<T>, index: usize) -> &mut T {
    if list.len() <= index {
        list.resize_with(index + 1, T::default);
    }
    &mut list[index]
}

fn method_named(name: &str) -> Option<EncounterMethod> {
    use EncounterMethod::*;
    if let Some(version) = name
        .strip_prefix("DualSlot(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return version.parse().ok().map(DualSlot);
    }
    [
        Grass,
        GrassMorning,
        GrassDay,
        GrassNight,
        Swarm,
        Radar,
        HoennSound,
        SinnohSound,
        Surf,
        RockSmash,
        OldRod,
        GoodRod,
        SuperRod,
        SurfSwarm,
        FishingSwarm,
        NightGoodRod,
        NightSuperRod,
        SafariGrass,
        SafariSurf,
        SafariOldRod,
        SafariGoodRod,
        SafariSuperRod,
        SafariGrassBlock,
        SafariSurfBlock,
        SafariOldRodBlock,
        SafariGoodRodBlock,
        SafariSuperRodBlock,
        BugContest,
        GreatMarsh,
        GreatMarshNational,
        TrophyGarden,
        DoubleGrass,
        ShakingGrass,
        SurfSpot,
        Fishing,
        FishingSpot,
    ]
    .into_iter()
    .find(|m| format!("{m:?}") == name)
}

fn language_named(name: &str) -> Option<Language> {
    (0..=u8::MAX)
        .map(Language::from_byte)
        .find(|l| format!("{l:?}") == name)
}

pub(super) fn read<'a>(
    game: Game,
    file: impl Fn(&str) -> Option<&'a str>,
) -> Result<Tables, TsvError> {
    let rows = |name: &'static str| Rows::new(name, file(name).unwrap_or(""));
    let mut tables = Tables {
        game,
        language: Language::Other(0),
        base_stats: Vec::new(),
        learnsets: Vec::new(),
        evolutions: Vec::new(),
        encounters: Vec::new(),
        species_names: Vec::new(),
        places: Vec::new(),
        growth: Vec::new(),
        pp: Vec::new(),
    };
    for mut r in rows(BASE_STATS) {
        let species: usize = r.get()?;
        let stats = Stats::from_array(r.array()?);
        let types = r.array()?;
        let catch_rate = r.get()?;
        let base_exp = r.get()?;
        let [gender_ratio, egg_cycles, base_friendship, growth] = r.array()?;
        *grow(&mut tables.base_stats, species) = BaseStats {
            stats,
            types,
            catch_rate,
            base_exp,
            gender_ratio,
            egg_cycles,
            base_friendship,
            growth,
            egg_groups: r.array()?,
            abilities: r.array()?,
            form_count: r.get()?,
            form_index: r.get()?,
            ev_yield: Stats::from_array(r.array()?),
        };
    }
    for mut r in rows(LEARNSETS) {
        let species: usize = r.get()?;
        let level = r.get()?;
        let move_id = r.get()?;
        grow(&mut tables.learnsets, species).push(LevelMove { level, move_id });
    }
    for mut r in rows(EVOLUTIONS) {
        let species: usize = r.get()?;
        let [method, param, into] = r.array()?;
        grow(&mut tables.evolutions, species).push(Evolution {
            method,
            param,
            into,
        });
    }
    let species = tables.base_stats.len();
    if tables.learnsets.len() < species {
        tables.learnsets.resize_with(species, Vec::new);
    }
    if tables.evolutions.len() < species {
        tables.evolutions.resize_with(species, Vec::new);
    }
    for mut r in rows(ENCOUNTERS) {
        let place = r.get()?;
        let season = r.optional()?;
        let broken = r.broken();
        let method = method_named(r.text()?).ok_or(broken)?;
        tables.encounters.push(Encounter {
            place,
            season,
            method,
            slot: r.get()?,
            species: r.get()?,
            form: r.get()?,
            min_level: r.get()?,
            max_level: r.get()?,
        });
    }
    for mut r in rows(SPECIES_NAMES) {
        let broken = r.broken();
        tables.language = language_named(r.text()?).ok_or(broken)?;
        let species: usize = r.get()?;
        *grow(&mut tables.species_names, species) = r.text()?.to_owned();
    }
    for mut r in rows(PLACES) {
        let header = r.get()?;
        let encounters = r.optional()?;
        tables.places.push(Place {
            header,
            encounters,
            location: r.get()?,
            area_icon: r.get()?,
            battle_background: r.get()?,
        });
    }
    for mut r in rows(GROWTH) {
        let curve: usize = r.get()?;
        let level: usize = r.get()?;
        *grow(grow(&mut tables.growth, curve), level) = r.get()?;
    }
    for mut r in rows(MOVES) {
        let id: usize = r.get()?;
        *grow(&mut tables.pp, id) = r.get()?;
    }
    Ok(tables)
}
