use std::fmt::{Debug, Display};

pub const MOST_INPUTS: usize = 4;

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(
    try_from = "Vec<I>",
    into = "Vec<I>",
    bound(
        serialize = "I: serde::Serialize + Clone",
        deserialize = "I: serde::Deserialize<'de> + Ord + Copy"
    )
)]
pub struct Combo<I>(Vec<I>);

impl<I: Ord + Copy> Combo<I> {
    pub fn new(inputs: impl IntoIterator<Item = I>) -> Option<Self> {
        let mut inputs: Vec<I> = inputs.into_iter().collect();
        inputs.sort();
        inputs.dedup();
        (!inputs.is_empty() && inputs.len() <= MOST_INPUTS).then_some(Self(inputs))
    }

    pub fn single(input: I) -> Self {
        Self(vec![input])
    }

    pub fn inputs(&self) -> &[I] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_single(&self) -> bool {
        self.0.len() == 1
    }

    pub fn only(&self) -> Option<I> {
        match self.0.as_slice() {
            [input] => Some(*input),
            _ => None,
        }
    }

    pub fn contains(&self, input: &I) -> bool {
        self.0.contains(input)
    }

    pub fn held_in(&self, down: &[I]) -> bool {
        self.0.iter().all(|input| down.contains(input))
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.0.iter().any(|input| other.contains(input))
    }
}

impl<I: Display> Display for Combo<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self.0.iter().map(ToString::to_string).collect();
        f.write_str(&names.join(" + "))
    }
}

impl<I: Ord + Copy> TryFrom<Vec<I>> for Combo<I> {
    type Error = &'static str;

    fn try_from(inputs: Vec<I>) -> Result<Self, Self::Error> {
        Self::new(inputs).ok_or("a combination holds one to four inputs")
    }
}

impl<I> From<Combo<I>> for Vec<I> {
    fn from(combo: Combo<I>) -> Self {
        combo.0
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Self = Self {
        ctrl: false,
        shift: false,
        alt: false,
    };

    pub fn of(modifiers: egui::Modifiers) -> Self {
        Self {
            ctrl: modifiers.command || modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            ctrl: self.ctrl || other.ctrl,
            shift: self.shift || other.shift,
            alt: self.alt || other.alt,
        }
    }

    fn count(self) -> usize {
        usize::from(self.ctrl) + usize::from(self.shift) + usize::from(self.alt)
    }

    pub fn names(self) -> impl Iterator<Item = &'static str> {
        [
            (self.ctrl, "Ctrl"),
            (self.shift, "Shift"),
            (self.alt, "Alt"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bound<I, T> {
    pub combo: Combo<I>,
    pub mods: Option<Mods>,
    pub target: T,
}

impl<I: Ord + Copy, T> Bound<I, T> {
    pub fn any_mods(combo: Combo<I>, target: T) -> Self {
        Self {
            combo,
            mods: None,
            target,
        }
    }

    pub fn exact(combo: Combo<I>, mods: Mods, target: T) -> Self {
        Self {
            combo,
            mods: Some(mods),
            target,
        }
    }

    pub fn map<U>(self, target: impl FnOnce(T) -> U) -> Bound<I, U> {
        Bound {
            combo: self.combo,
            mods: self.mods,
            target: target(self.target),
        }
    }

    fn weight(&self) -> usize {
        self.combo.len() + self.mods.map_or(0, Mods::count)
    }

    fn completes(&self, input: &I, mods: Mods, down: &[I]) -> bool {
        self.combo.contains(input)
            && self.combo.held_in(down)
            && self.mods.is_none_or(|wanted| wanted == mods)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge<T> {
    Down(T),
    Up(T),
}

#[derive(Debug)]
pub struct Chords<I, T> {
    down: Vec<I>,
    active: Vec<(Combo<I>, T)>,
}

impl<I, T> Default for Chords<I, T> {
    fn default() -> Self {
        Self {
            down: Vec::new(),
            active: Vec::new(),
        }
    }
}

impl<I: Ord + Copy, T: Copy + PartialEq> Chords<I, T> {
    pub fn press(&mut self, input: I, mods: Mods, table: &[Bound<I, T>]) -> Vec<Edge<T>> {
        if self.down.contains(&input) {
            return Vec::new();
        }
        self.down.push(input);
        let completed: Vec<&Bound<I, T>> = table
            .iter()
            .filter(|bound| bound.completes(&input, mods, &self.down))
            .collect();
        let Some(heaviest) = completed.iter().map(|bound| bound.weight()).max() else {
            return Vec::new();
        };
        let before = self.targets();
        let winners: Vec<&Bound<I, T>> = completed
            .into_iter()
            .filter(|bound| bound.weight() == heaviest)
            .collect();
        self.active
            .retain(|(combo, _)| !winners.iter().any(|won| won.combo.overlaps(combo)));
        self.active.extend(
            winners
                .into_iter()
                .map(|won| (won.combo.clone(), won.target)),
        );
        self.edges(before)
    }

    pub fn release(&mut self, input: I) -> Vec<Edge<T>> {
        let before = self.targets();
        self.down.retain(|down| *down != input);
        self.active.retain(|(combo, _)| !combo.contains(&input));
        self.edges(before)
    }

    pub fn clear(&mut self) -> Vec<Edge<T>> {
        let before = self.targets();
        self.down.clear();
        self.active.clear();
        self.edges(before)
    }

    pub fn holds(&self, target: T) -> bool {
        self.active.iter().any(|(_, held)| *held == target)
    }

    #[cfg(test)]
    pub fn is_down(&self, input: I) -> bool {
        self.down.contains(&input)
    }

    pub fn targets(&self) -> Vec<T> {
        let mut targets: Vec<T> = Vec::new();
        for (_, target) in &self.active {
            if !targets.contains(target) {
                targets.push(*target);
            }
        }
        targets
    }

    fn edges(&self, before: Vec<T>) -> Vec<Edge<T>> {
        let after = self.targets();
        let ups = before
            .iter()
            .filter(|target| !after.contains(target))
            .map(|target| Edge::Up(*target));
        let downs = after
            .iter()
            .filter(|target| !before.contains(target))
            .map(|target| Edge::Down(*target));
        ups.chain(downs).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
    )]
    enum Pad {
        A,
        L,
        R,
        Select,
        Up,
        Up2,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Out {
        GameA,
        GameL,
        GameR,
        GameSelect,
        GameUp,
        Menu,
        Save,
        SaveNew,
    }

    use Edge::{Down, Up};

    fn combo(inputs: &[Pad]) -> Combo<Pad> {
        Combo::new(inputs.iter().copied()).expect("a combo")
    }

    fn game(input: Pad, out: Out) -> Bound<Pad, Out> {
        Bound::any_mods(Combo::single(input), out)
    }

    fn pad_table() -> Vec<Bound<Pad, Out>> {
        vec![
            game(Pad::A, Out::GameA),
            game(Pad::L, Out::GameL),
            game(Pad::R, Out::GameR),
            game(Pad::Select, Out::GameSelect),
            game(Pad::Up, Out::GameUp),
            game(Pad::Up2, Out::GameUp),
            Bound::any_mods(combo(&[Pad::L, Pad::R, Pad::Select]), Out::Menu),
        ]
    }

    const NONE: Mods = Mods::NONE;

    #[test]
    fn a_single_binding_follows_its_input() {
        let table = pad_table();
        let mut chords = Chords::default();
        assert_eq!(chords.press(Pad::A, NONE, &table), [Down(Out::GameA)]);
        assert!(chords.holds(Out::GameA));
        assert_eq!(chords.release(Pad::A), [Up(Out::GameA)]);
        assert!(chords.targets().is_empty());
    }

    #[test]
    fn a_combo_fires_once_when_its_last_input_goes_down() {
        let table = vec![Bound::any_mods(
            combo(&[Pad::L, Pad::R, Pad::Select]),
            Out::Menu,
        )];
        let mut chords = Chords::default();
        assert!(chords.press(Pad::L, NONE, &table).is_empty());
        assert!(chords.press(Pad::R, NONE, &table).is_empty());
        assert_eq!(chords.press(Pad::Select, NONE, &table), [Down(Out::Menu)]);
        assert!(chords.press(Pad::Select, NONE, &table).is_empty());
        assert_eq!(chords.release(Pad::R), [Up(Out::Menu)]);
        assert!(chords.release(Pad::L).is_empty());
        assert!(chords.release(Pad::Select).is_empty());
    }

    #[test]
    fn the_order_of_presses_does_not_matter() {
        let table = pad_table();
        let mut chords = Chords::default();
        chords.press(Pad::Select, NONE, &table);
        chords.press(Pad::L, NONE, &table);
        assert_eq!(
            chords.press(Pad::R, NONE, &table),
            [Up(Out::GameSelect), Up(Out::GameL), Down(Out::Menu)]
        );
    }

    #[test]
    fn a_combo_takes_its_inputs_from_the_single_bindings() {
        let table = pad_table();
        let mut chords = Chords::default();
        assert_eq!(chords.press(Pad::L, NONE, &table), [Down(Out::GameL)]);
        assert_eq!(chords.press(Pad::R, NONE, &table), [Down(Out::GameR)]);
        assert_eq!(
            chords.press(Pad::Select, NONE, &table),
            [Up(Out::GameL), Up(Out::GameR), Down(Out::Menu)],
            "the game never gets Select, and lets go of L and R"
        );
        assert_eq!(chords.release(Pad::Select), [Up(Out::Menu)]);
        assert!(
            chords.release(Pad::L).is_empty(),
            "L stays quiet until it goes up"
        );
        assert!(chords.release(Pad::R).is_empty());
        assert!(chords.targets().is_empty());
    }

    #[test]
    fn inputs_a_combo_used_wake_only_with_a_new_press() {
        let table = pad_table();
        let mut chords = Chords::default();
        for pad in [Pad::L, Pad::R, Pad::Select] {
            chords.press(pad, NONE, &table);
        }
        assert_eq!(chords.release(Pad::Select), [Up(Out::Menu)]);
        assert!(!chords.holds(Out::GameL), "no stuck L after the combo");
        assert_eq!(
            chords.press(Pad::Select, NONE, &table),
            [Down(Out::Menu)],
            "pressing the last input again fires the combo again"
        );
        chords.release(Pad::Select);
        chords.release(Pad::R);
        assert_eq!(chords.press(Pad::R, NONE, &table), [Down(Out::GameR)]);
    }

    #[test]
    fn a_held_game_button_outside_the_combo_stays_held() {
        let table = pad_table();
        let mut chords = Chords::default();
        chords.press(Pad::A, NONE, &table);
        for pad in [Pad::L, Pad::R, Pad::Select] {
            chords.press(pad, NONE, &table);
        }
        assert!(chords.holds(Out::GameA));
        for pad in [Pad::Select, Pad::R, Pad::L] {
            let edges = chords.release(pad);
            assert!(!edges.contains(&Up(Out::GameA)), "{pad:?}");
        }
        assert!(chords.holds(Out::GameA));
        assert_eq!(chords.release(Pad::A), [Up(Out::GameA)]);
        assert!(chords.targets().is_empty(), "nothing stuck");
    }

    #[test]
    fn the_longest_match_wins() {
        let table = vec![
            Bound::any_mods(combo(&[Pad::L, Pad::R]), Out::Save),
            Bound::any_mods(combo(&[Pad::L, Pad::R, Pad::Select]), Out::Menu),
        ];
        let mut chords = Chords::default();
        chords.press(Pad::Select, NONE, &table);
        chords.press(Pad::L, NONE, &table);
        assert_eq!(chords.press(Pad::R, NONE, &table), [Down(Out::Menu)]);
    }

    #[test]
    fn two_inputs_for_one_button_hold_it_until_both_let_go() {
        let table = pad_table();
        let mut chords = Chords::default();
        chords.press(Pad::Up, NONE, &table);
        assert!(chords.press(Pad::Up2, NONE, &table).is_empty());
        assert!(chords.release(Pad::Up).is_empty());
        assert_eq!(chords.release(Pad::Up2), [Up(Out::GameUp)]);
    }

    #[test]
    fn shortcuts_want_their_exact_modifiers_and_game_buttons_take_any() {
        let shift = Mods {
            shift: true,
            ..NONE
        };
        let ctrl_shift = Mods {
            ctrl: true,
            ..shift
        };
        let table = vec![
            game(Pad::A, Out::GameA),
            Bound::exact(combo(&[Pad::L]), shift, Out::Save),
            Bound::exact(combo(&[Pad::L]), ctrl_shift, Out::SaveNew),
        ];
        let mut chords = Chords::default();
        assert_eq!(chords.press(Pad::A, ctrl_shift, &table), [Down(Out::GameA)]);
        assert!(chords.press(Pad::L, NONE, &table).is_empty());
        chords.release(Pad::L);
        assert_eq!(chords.press(Pad::L, shift, &table), [Down(Out::Save)]);
        chords.release(Pad::L);
        assert_eq!(
            chords.press(Pad::L, ctrl_shift, &table),
            [Down(Out::SaveNew)],
            "more modifiers is the longer match"
        );
    }

    #[test]
    fn a_shortcut_with_a_modifier_beats_the_game_button_on_its_key() {
        let shift = Mods {
            shift: true,
            ..NONE
        };
        let table = vec![
            game(Pad::A, Out::GameA),
            Bound::exact(combo(&[Pad::A]), shift, Out::Save),
        ];
        let mut chords = Chords::default();
        assert_eq!(chords.press(Pad::A, shift, &table), [Down(Out::Save)]);
        chords.release(Pad::A);
        assert_eq!(chords.press(Pad::A, NONE, &table), [Down(Out::GameA)]);
    }

    #[test]
    fn clearing_lets_go_of_everything() {
        let table = pad_table();
        let mut chords = Chords::default();
        chords.press(Pad::A, NONE, &table);
        chords.press(Pad::Up, NONE, &table);
        let edges = chords.clear();
        assert!(edges.contains(&Up(Out::GameA)) && edges.contains(&Up(Out::GameUp)));
        assert!(!chords.is_down(Pad::A));
        assert_eq!(chords.press(Pad::A, NONE, &table), [Down(Out::GameA)]);
    }

    #[test]
    fn a_combo_keeps_one_to_four_inputs_in_one_order() {
        assert_eq!(
            combo(&[Pad::Select, Pad::L, Pad::L]).inputs(),
            [Pad::L, Pad::Select]
        );
        assert!(Combo::<Pad>::new([]).is_none());
        assert!(Combo::new([Pad::A, Pad::L, Pad::R, Pad::Select, Pad::Up]).is_none());
        let json = serde_json::to_string(&combo(&[Pad::R, Pad::L])).expect("saves");
        assert!(serde_json::from_str::<Combo<Pad>>("[]").is_err());
        assert_eq!(json.matches(',').count(), 1);
    }
}
