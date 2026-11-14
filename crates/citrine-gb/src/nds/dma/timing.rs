#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Unmapped,
    MainRam,
    Slow(u8),
    Plain(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    area: Area,
    half: [u32; 2],
    word: [u32; 2],
    steady: bool,
}

impl Timing {
    pub const UNMAPPED: Self = Self::new(Area::Unmapped, 32, 1, 1);

    pub const fn new(area: Area, bus_bits: u32, n: u32, s: u32) -> Self {
        let word = if bus_bits == 16 {
            [n + s, 2 * s]
        } else {
            [n, s]
        };
        Self {
            area,
            half: [n, s],
            word,
            steady: true,
        }
    }

    pub const fn set_by_registers(self) -> Self {
        Self {
            steady: false,
            ..self
        }
    }

    pub fn steady(self) -> bool {
        self.steady
    }

    fn n(self, wide: bool) -> u32 {
        if wide { self.word[0] } else { self.half[0] }
    }

    fn s(self, wide: bool) -> u32 {
        if wide { self.word[1] } else { self.half[1] }
    }

    fn slow(self) -> bool {
        matches!(self.area, Area::Slow(_))
    }
}

type Burst = [u8; 256];

const fn burst(runs: &[(u8, usize)]) -> Burst {
    let mut table = [0; 256];
    let mut at = 0;
    let mut run = 0;
    while run < runs.len() {
        let (value, count) = runs[run];
        let mut index = 0;
        while index < count {
            table[at] = value;
            at += 1;
            index += 1;
        }
        run += 1;
    }
    table
}

const READ16: [Burst; 3] = [
    burst(&[
        (7, 1),
        (3, 1),
        (2, 117),
        (7, 1),
        (3, 1),
        (2, 117),
        (7, 1),
        (3, 1),
    ]),
    burst(&[(8, 1), (6, 1), (5, 94)]),
    burst(&[
        (10, 1),
        (8, 1),
        (7, 32),
        (12, 1),
        (8, 1),
        (7, 32),
        (12, 1),
        (8, 1),
        (7, 32),
        (12, 1),
        (8, 1),
        (7, 32),
        (12, 1),
        (8, 1),
        (7, 32),
        (12, 1),
        (8, 1),
        (7, 32),
        (12, 1),
        (8, 1),
        (7, 32),
        (12, 1),
        (8, 1),
    ]),
];

const READ32: [Burst; 4] = [
    burst(&[(9, 1), (4, 1), (3, 77), (9, 1)]),
    burst(&[(9, 1), (3, 1), (2, 116)]),
    burst(&[
        (14, 1),
        (10, 1),
        (9, 25),
        (13, 1),
        (10, 1),
        (9, 25),
        (13, 1),
        (10, 1),
        (9, 25),
        (13, 1),
        (10, 1),
        (9, 25),
        (13, 1),
        (10, 1),
        (9, 25),
        (13, 1),
    ]),
    burst(&[
        (18, 1),
        (14, 1),
        (13, 17),
        (17, 1),
        (14, 1),
        (13, 17),
        (17, 1),
        (14, 1),
        (13, 17),
        (17, 1),
        (14, 1),
        (13, 17),
        (17, 1),
        (14, 1),
        (13, 17),
        (17, 1),
    ]),
];

const WRITE16: [Burst; 3] = [
    burst(&[(8, 1), (2, 119)]),
    burst(&[(10, 1), (5, 47)]),
    burst(&[(9, 1), (7, 34)]),
];

const WRITE32: [Burst; 4] = [
    burst(&[(9, 1), (4, 59)]),
    burst(&[(9, 1), (3, 79)]),
    burst(&[(15, 1), (10, 23)]),
    burst(&[(16, 1), (14, 17)]),
];

#[derive(Clone, Copy)]
pub struct Unit {
    pub source: Timing,
    pub dest: Timing,
    pub source_address: u32,
    pub source_increments: bool,
    pub dest_increments: bool,
    pub wide: bool,
}

#[derive(Clone, Copy)]
enum Rule {
    Flat(u32),
    Burst(&'static Burst),
    MainRamSource {
        wide: bool,
        quick_dest: bool,
        n: u32,
        s: u32,
    },
    Lead {
        lead: u32,
        n: u32,
        s: u32,
    },
}

impl Rule {
    fn of(unit: &Unit) -> Self {
        let wide = unit.wide;
        let (source, dest) = (unit.source, unit.dest);
        if source.area == Area::MainRam {
            if dest.area == Area::MainRam {
                return Self::Flat(if wide { 18 } else { 16 });
            }
            if unit.source_increments {
                return Self::Burst(read_table(wide, dest));
            }
            return Self::MainRamSource {
                wide,
                quick_dest: dest.n(true) == 2,
                n: dest.n(wide),
                s: dest.s(wide),
            };
        }
        if dest.area == Area::MainRam {
            if unit.dest_increments {
                return Self::Burst(write_table(wide, source));
            }
            return Self::Lead {
                lead: if wide { 8 } else { 7 },
                n: source.n(wide),
                s: source.s(wide),
            };
        }
        if source.area == dest.area && source.area != Area::Unmapped {
            let cost = source.n(wide) + dest.n(wide) + 1;
            return Self::Lead {
                lead: 0,
                n: cost,
                s: cost,
            };
        }
        Self::Lead {
            lead: 0,
            n: source.n(wide) + dest.n(wide),
            s: source.s(wide) + dest.s(wide),
        }
    }
}

#[derive(Default)]
pub struct Meter {
    table: Option<&'static Burst>,
    index: usize,
    started: bool,
}

impl Meter {
    pub fn run(&mut self, unit: &Unit, units: u32, source_step: u32) -> u32 {
        let rule = Rule::of(unit);
        let mut source_address = unit.source_address;
        let mut total = 0;
        for _ in 0..units {
            total += self.apply(rule, source_address);
            source_address = source_address.wrapping_add(source_step);
        }
        total
    }

    #[inline(always)]
    fn apply(&mut self, rule: Rule, source_address: u32) -> u32 {
        let first = !std::mem::replace(&mut self.started, true);
        match rule {
            Rule::Flat(cost) => cost,
            Rule::Burst(table) => self.next(first, table),
            Rule::MainRamSource {
                wide,
                quick_dest,
                n,
                s,
            } => {
                let at_edge = source_address & 0x1F == if wide { 0x1C } else { 0x1E };
                let lead = match (wide, at_edge) {
                    (true, true) if quick_dest => 7,
                    (true, true) | (false, false) => 8,
                    (true, false) => 9,
                    (false, true) => 7,
                };
                lead + if first { n } else { s }
            }
            Rule::Lead { lead, n, s } => lead + if first { n } else { s },
        }
    }

    fn next(&mut self, first: bool, pick: &'static Burst) -> u32 {
        let ended = self.table.is_none_or(|table| table[self.index] == 0);
        if first || ended {
            self.table = Some(pick);
            self.index = 0;
        }
        let table = self.table.unwrap_or(&READ16[0]);
        let cost = table[self.index];
        self.index += 1;
        u32::from(cost)
    }
}

fn read_table(wide: bool, dest: Timing) -> &'static Burst {
    match (wide, dest.slow()) {
        (false, true) if dest.s(false) == 4 => &READ16[1],
        (false, true) => &READ16[2],
        (false, false) => &READ16[0],
        (true, true) if dest.s(true) == 8 => &READ32[2],
        (true, true) => &READ32[3],
        (true, false) if dest.n(true) == 2 => &READ32[0],
        (true, false) => &READ32[1],
    }
}

fn write_table(wide: bool, source: Timing) -> &'static Burst {
    match (wide, source.slow()) {
        (false, true) if source.s(false) == 4 => &WRITE16[1],
        (false, true) => &WRITE16[2],
        (false, false) => &WRITE16[0],
        (true, true) if source.s(true) == 8 => &WRITE32[2],
        (true, true) => &WRITE32[3],
        (true, false) if source.n(true) == 2 => &WRITE32[0],
        (true, false) => &WRITE32[1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Timing = Timing::new(Area::MainRam, 16, 8, 1);
    const IO: Timing = Timing::new(Area::Plain(3), 32, 1, 1);
    const VRAM: Timing = Timing::new(Area::Plain(5), 16, 1, 1);
    const WRAM: Timing = Timing::new(Area::Plain(2), 32, 1, 1);
    const SLOT: Timing = Timing::new(Area::Slow(9), 16, 10, 6);

    fn unit(source: Timing, dest: Timing, wide: bool) -> Unit {
        Unit {
            source,
            dest,
            source_address: 0x0200_0000,
            source_increments: true,
            dest_increments: true,
            wide,
        }
    }

    fn costs(unit: &Unit, count: usize) -> Vec<u32> {
        let mut meter = Meter::default();
        (0..count).map(|_| meter.run(unit, 1, 0)).collect()
    }

    #[test]
    fn a_card_word_into_main_ram_starts_a_new_burst_each_time() {
        let mut card = unit(IO, MAIN, true);
        card.source_increments = false;
        assert_eq!(costs(&card, 1), [9]);
    }

    #[test]
    fn main_ram_to_vram_words_follow_the_read_burst() {
        let copy = costs(&unit(MAIN, VRAM, true), 82);
        assert_eq!(&copy[..4], [9, 4, 3, 3]);
        assert_eq!(copy[79], 9);
        assert_eq!(&copy[80..], [9, 4]);
    }

    #[test]
    fn main_ram_to_wram_words_follow_the_faster_read_burst() {
        let copy = costs(&unit(MAIN, WRAM, true), 1024);
        assert_eq!(&copy[..3], [9, 3, 2]);
        assert_eq!(copy[118..120], [9, 3]);
    }

    #[test]
    fn main_ram_to_main_ram_costs_a_flat_rate() {
        assert_eq!(costs(&unit(MAIN, MAIN, true), 3), [18, 18, 18]);
        assert_eq!(costs(&unit(MAIN, MAIN, false), 3), [16, 16, 16]);
    }

    #[test]
    fn copies_inside_one_area_pay_two_nonsequential_accesses() {
        assert_eq!(costs(&unit(WRAM, WRAM, true), 2), [3, 3]);
    }

    #[test]
    fn copies_between_other_areas_pay_sequential_after_the_first() {
        assert_eq!(costs(&unit(WRAM, SLOT, true), 3), [17, 13, 13]);
        assert_eq!(costs(&unit(WRAM, SLOT, false), 2), [11, 7]);
    }

    #[test]
    fn a_run_costs_what_its_units_cost_one_by_one() {
        let fast_slot = Timing::new(Area::Slow(9), 16, 6, 4);
        let slot_ram = Timing::new(Area::Plain(8), 8, 10, 10);
        let areas = [
            MAIN,
            IO,
            VRAM,
            WRAM,
            SLOT,
            fast_slot,
            slot_ram,
            Timing::UNMAPPED,
        ];
        for source in areas {
            for dest in areas {
                for wide in [false, true] {
                    let width: u32 = if wide { 4 } else { 2 };
                    for (source_step, dest_step) in [
                        (width, width),
                        (0, width),
                        (width, 0),
                        (width.wrapping_neg(), 0),
                    ] {
                        for start in [0x0200_0000, 0x0200_0018, 0x0200_001C, 0x0200_001E] {
                            for units in [1, 2, 47, 118, 119, 250] {
                                let unit = Unit {
                                    source,
                                    dest,
                                    source_address: start,
                                    source_increments: source_step.cast_signed() > 0,
                                    dest_increments: dest_step.cast_signed() > 0,
                                    wide,
                                };
                                let mut one_by_one = Meter::default();
                                let mut address = unit;
                                let mut expected = 0;
                                for _ in 0..units {
                                    expected += one_by_one.run(&address, 1, 0);
                                    address.source_address =
                                        address.source_address.wrapping_add(source_step);
                                }
                                let mut run = Meter::default();
                                assert_eq!(run.run(&unit, units, source_step), expected);
                                let after = costs_after(&mut run, &address, 130);
                                assert_eq!(after, costs_after(&mut one_by_one, &address, 130));
                            }
                        }
                    }
                }
            }
        }
    }

    fn costs_after(meter: &mut Meter, unit: &Unit, count: usize) -> Vec<u32> {
        (0..count).map(|_| meter.run(unit, 1, 0)).collect()
    }
}
