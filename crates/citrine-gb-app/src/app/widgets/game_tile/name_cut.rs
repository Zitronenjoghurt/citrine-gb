use crate::app::tabs::shorten;

const SEPARATORS: [&str; 3] = ["-", "–", "—"];
const MIN_LETTERS: usize = 2;
const WHOLE_WORDS_SHARE: f32 = 0.6;
const SHORT_WORD: usize = 3;

pub fn cut(name: &str, neighbours: &[&str], room: f32, measure: impl Fn(&str) -> f32) -> String {
    let cutter = Cutter {
        neighbours,
        room,
        measure,
    };
    cutter.cut(name)
}

#[derive(Clone, Copy)]
struct Differs {
    at: usize,
    ends: bool,
}

struct Cutter<'a, M> {
    neighbours: &'a [&'a str],
    room: f32,
    measure: M,
}

impl<M: Fn(&str) -> f32> Cutter<'_, M> {
    fn fits(&self, text: &str) -> bool {
        (self.measure)(text) <= self.room
    }

    fn cut(&self, name: &str) -> String {
        if self.fits(name) {
            return name.to_owned();
        }
        let words = words(name);
        let lead = self.lead(&words);
        let next = next(&words, lead);
        let second = match self.start(name) {
            Some(start) => self
                .tail(&words, start, self.differs(name), self.room)
                .unwrap_or_else(|| self.forced_tail(&words, start)),
            None => self.end_cut("", &words[next..], None, self.room),
        };
        format!("{}\n{second}", self.first_line(&words[..lead]))
    }

    fn first_line(&self, words: &[&str]) -> String {
        let line = format!("{}…", words.join(" "));
        if self.fits(&line) {
            line
        } else {
            shorten(&line, self.room, &self.measure)
        }
    }

    fn separator_break(&self, words: &[&str]) -> Option<(usize, bool)> {
        (1..words.len().saturating_sub(1))
            .filter(|at| is_separator(words[*at]))
            .find_map(|at| {
                let start = at + 1;
                let after = words[start..].join(" ");
                let first = format!("{}…", words[..line_end(words, start)].join(" "));
                if self.fits(&after) && self.fits(&first) {
                    return Some((start, false));
                }
                self.fits(&format!("…{after}")).then_some((start, true))
            })
    }

    fn lead(&self, words: &[&str]) -> usize {
        if let Some((start, false)) = self.separator_break(words) {
            return line_end(words, start);
        }
        let fits = |count: usize| self.fits(&format!("{}…", words[..count].join(" ")));
        let mut lead = 1;
        while lead < words.len() && fits(lead + 1) {
            lead += 1;
        }
        lead = drop_separator(words, lead);
        let next = next(words, lead);
        let alone = next + 1 == words.len() && words[next].chars().count() <= SHORT_WORD;
        if alone && lead > 1 && self.fits(&words[lead - 1..].join(" ")) {
            lead = drop_separator(words, lead - 1);
        }
        lead
    }

    fn next(&self, words: &[&str]) -> usize {
        next(words, self.lead(words))
    }

    fn differs(&self, name: &str) -> Option<Differs> {
        let words = words(name);
        let next = self.next(&words);
        let rest = &words[next.min(words.len())..];
        if self.fits(name) || rest.is_empty() || self.fits(&rest.join(" ")) {
            return None;
        }
        let shared = self
            .neighbours
            .iter()
            .filter(|other| **other != name)
            .map(|other| shared_words(&words, other))
            .filter(|shared| *shared >= next)
            .max()?;
        Some(Differs {
            at: shared.min(words.len() - 1),
            ends: shared >= words.len(),
        })
    }

    fn own_start(&self, name: &str) -> Option<usize> {
        let differs = self.differs(name)?;
        let words = words(name);
        let from = self.next(&words);
        if differs.at < from {
            return None;
        }
        let shows = |start: &usize| {
            self.tail(&words, *start, Some(differs), self.room)
                .is_some()
        };
        let after_separator = (from..=differs.at)
            .rev()
            .find(|start| *start > 0 && is_separator(words[start - 1]))
            .filter(shows);
        after_separator
            .or_else(|| (from..=differs.at).find(|start| opens_tail(words[*start]) && shows(start)))
            .or_else(|| (differs.at..words.len()).find(|start| !is_separator(words[*start])))
    }

    fn start(&self, name: &str) -> Option<usize> {
        let words = words(name);
        let from = self.next(&words);
        if from >= words.len() {
            return None;
        }
        if let Some((start, true)) = self.separator_break(&words) {
            return Some(start);
        }
        let differs = self.differs(name);
        let group = self
            .neighbours
            .iter()
            .filter(|other| **other != name)
            .filter_map(|other| {
                let start = self.own_start(other)?;
                let theirs = self::words(other);
                let same_start = start >= from
                    && start < words.len()
                    && !is_separator(words[start])
                    && theirs.len() >= start
                    && theirs[..start] == words[..start];
                (same_start && self.tail(&words, start, differs, self.room).is_some())
                    .then_some(start)
            });
        self.own_start(name).into_iter().chain(group).min()
    }

    fn tail(
        &self,
        words: &[&str],
        start: usize,
        differs: Option<Differs>,
        room: f32,
    ) -> Option<String> {
        let rest = &words[start..];
        let before = if start > self.next(words) { "…" } else { "" };
        let whole = format!("{before}{}", rest.join(" "));
        if (self.measure)(&whole) <= room {
            return Some(whole);
        }
        if differs.is_some_and(|differs| differs.ends) {
            return None;
        }
        let keep = differs.map(|differs| differs.at - start);
        let end = self.end_cut(before, rest, keep, room);
        (end != before).then_some(end)
    }

    fn forced_tail(&self, words: &[&str], start: usize) -> String {
        let before = if start > self.next(words) { "…" } else { "" };
        self.end_cut(before, &words[start..], None, self.room)
    }

    fn end_cut(&self, before: &str, words: &[&str], keep: Option<usize>, room: f32) -> String {
        let fits = |text: &str| (self.measure)(text) <= room;
        let text = format!("{before}{}", words.join(" "));
        if fits(&text) {
            return text;
        }
        let whole = (1..words.len())
            .rev()
            .filter(|count| keep.is_none_or(|keep| *count > keep))
            .map(|count| format!("{before}{}…", words[..count].join(" ")))
            .find(|text| fits(text));
        let mid = shorten(&text, room, &self.measure);
        let mid_ok = mid_cut_keeps(&mid, before, words, keep);
        match (whole, mid_ok) {
            (Some(whole), true) if (self.measure)(&whole) < room * WHOLE_WORDS_SHARE => mid,
            (Some(whole), _) => whole,
            (None, true) => mid,
            (None, false) if keep.is_none() => mid,
            (None, false) => before.to_owned(),
        }
    }
}

fn mid_cut_keeps(cut: &str, before: &str, words: &[&str], keep: Option<usize>) -> bool {
    let shown = cut
        .strip_prefix(before)
        .unwrap_or(cut)
        .trim_end_matches('…');
    let shown: Vec<&str> = shown.split(' ').filter(|word| !word.is_empty()).collect();
    let Some(last) = shown.len().checked_sub(1) else {
        return false;
    };
    let whole_last = words.get(last).is_some_and(|word| *word == shown[last]);
    let letters = shown[last].chars().filter(|c| c.is_alphanumeric()).count();
    let last_ok = whole_last || letters >= MIN_LETTERS;
    let keeps = keep.is_none_or(|keep| last > keep || (last == keep && last_ok));
    last_ok && keeps
}

fn next(words: &[&str], lead: usize) -> usize {
    match words.get(lead) {
        Some(word) if SEPARATORS.contains(word) => lead + 1,
        _ => lead,
    }
}

fn line_end(words: &[&str], start: usize) -> usize {
    if SEPARATORS.contains(&words[start - 1]) {
        start - 1
    } else {
        start
    }
}

fn drop_separator(words: &[&str], lead: usize) -> usize {
    if lead > 1 && SEPARATORS.contains(&words[lead - 1]) {
        lead - 1
    } else {
        lead
    }
}

fn words(name: &str) -> Vec<&str> {
    name.split(' ').filter(|word| !word.is_empty()).collect()
}

fn is_separator(word: &str) -> bool {
    SEPARATORS.contains(&word) || word.ends_with(':')
}

fn opens_tail(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|first| first.is_uppercase() || first.is_numeric())
}

fn shared_words(words: &[&str], other: &str) -> usize {
    words
        .iter()
        .zip(other.split(' ').filter(|word| !word.is_empty()))
        .take_while(|(mine, theirs)| *mine == theirs)
        .count()
}

#[cfg(test)]
mod tests {
    use super::cut;

    fn measure(text: &str) -> f32 {
        text.chars().count() as f32
    }

    const ZELDA: [&str; 6] = [
        "The Legend of Zelda - Oracle of Ages",
        "The Legend of Zelda - Oracle of Seasons",
        "The Legend of Zelda - Link's Awakening",
        "The Legend of Zelda - Link's Awakening DX",
        "The Legend of Zelda - The Minish Cap",
        "The Legend of Zelda - A Link to the Past & Four Swords",
    ];

    fn two_lines(names: &[&str], room: f32) -> Vec<String> {
        names
            .iter()
            .map(|name| cut(name, names, room, measure))
            .collect()
    }

    #[test]
    fn a_name_that_fits_stays_whole() {
        assert_eq!(cut("Tetris", &ZELDA, 18.0, measure), "Tetris");
    }

    #[test]
    fn every_two_line_name_ends_line_one_in_an_ellipsis() {
        assert_eq!(
            two_lines(&ZELDA, 16.0)[0],
            "The Legend of…\n…Oracle of Ages"
        );
        assert_eq!(
            two_lines(&["Kirby & the Amazing Mirror"], 16.0)[0],
            "Kirby & the…\nAmazing Mirror"
        );
        assert_eq!(
            two_lines(&["Yu-Gi-Oh! 5D's World Championship 2011"], 16.0)[0],
            "Yu-Gi-Oh! 5D's…\nWorld Champions…"
        );
    }

    #[test]
    fn a_separator_at_the_break_is_dropped() {
        assert_eq!(
            cut("Pokémon - Yellow Version", &ZELDA, 16.0, measure),
            "Pokémon…\nYellow Version"
        );
        assert_eq!(
            two_lines(&ZELDA, 20.0),
            [
                "The Legend of Zelda…\nOracle of Ages",
                "The Legend of Zelda…\nOracle of Seasons",
                "The Legend of Zelda…\nLink's Awakening",
                "The Legend of Zelda…\nLink's Awakening DX",
                "The Legend of Zelda…\nThe Minish Cap",
                "The Legend of Zelda…\nA Link to the Past…",
            ]
        );
    }

    #[test]
    fn a_series_breaks_at_its_separator() {
        let names = [
            "Pokémon - Red Version",
            "Pokémon - Blue Version",
            "Pokémon - Ruby Version",
            "Pokémon - Sapphire Version",
        ];
        for room in [16.0, 20.0] {
            assert_eq!(
                two_lines(&names, room),
                [
                    "Pokémon…\nRed Version",
                    "Pokémon…\nBlue Version",
                    "Pokémon…\nRuby Version",
                    "Pokémon…\nSapphire Version",
                ]
            );
        }
        let names = [
            "Pokémon Mystery Dungeon: Red Rescue Team",
            "Pokémon Mystery Dungeon: Blue Rescue Team",
        ];
        assert_eq!(
            two_lines(&names, 16.0)[0],
            "Pokémon Mystery…\n…Red Rescue Team"
        );
    }

    #[test]
    fn a_cut_keeps_the_part_that_differs() {
        let names = [
            "Pokémon Mystery Dungeon: Red Rescue Team",
            "Pokémon Mystery Dungeon: Blue Rescue Team",
        ];
        assert_eq!(
            two_lines(&names, 20.0),
            [
                "Pokémon Mystery…\n…Red Rescue Team",
                "Pokémon Mystery…\n…Blue Rescue Team",
            ]
        );
    }

    #[test]
    fn a_short_last_word_never_stands_alone() {
        assert_eq!(
            two_lines(&["Kirby's Dream Land 2"], 19.0)[0],
            "Kirby's Dream…\nLand 2"
        );
    }

    #[test]
    fn a_cut_never_leaves_a_lone_letter() {
        let cut = two_lines(&["Pokémon Mystery Dungeon Red Rescue Team"], 20.0);
        assert_eq!(cut[0], "Pokémon Mystery…\nDungeon Red Rescue…");
    }

    #[test]
    fn colliding_names_draw_differently() {
        for room in [16.0, 20.0, 22.0] {
            let cut = two_lines(&ZELDA, room);
            for (at, shown) in cut.iter().enumerate() {
                assert!(
                    cut[at + 1..].iter().all(|other| other != shown),
                    "{shown:?} twice at {room}: {cut:?}"
                );
            }
        }
    }

    #[test]
    fn a_tail_opens_after_the_series_separator() {
        let cut = two_lines(&ZELDA, 16.0);
        assert_eq!(cut[1], "The Legend of…\n…Oracle of Seas…");
        assert_eq!(cut[4], "The Legend of…\n…The Minish Cap");
        assert_eq!(cut[5], "The Legend of…\n…A Link to the…");
    }

    #[test]
    fn a_name_that_ends_where_its_twin_goes_on_keeps_its_end() {
        let cut = two_lines(&ZELDA, 16.0);
        assert_eq!(cut[2], "The Legend of…\n…Awakening");
        assert_eq!(cut[3], "The Legend of…\n…Awakening DX");
    }

    #[test]
    fn a_tail_never_opens_on_a_separator() {
        let names = [
            "Pokémon Mystery Dungeon - Red Rescue Team",
            "Pokémon Mystery Dungeon - Blue Rescue Team",
        ];
        let cut = two_lines(&names, 14.0);
        assert_eq!(cut, ["Pokémon…\n…Red Rescue…", "Pokémon…\n…Blue Rescue…"]);
        assert!(cut.iter().all(|shown| !shown.contains("…-")), "{cut:?}");
    }
}
