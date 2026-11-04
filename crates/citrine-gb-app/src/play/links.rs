use super::{SLOW_DELAY, SLOW_ROUND_TRIP_MS};
use citrine_gb::netplay::{MAX_ROLLBACK_FRAMES, Sync};
use citrine_gb_net::{Console, OnlineId};

pub const MAX_INPUT_DELAY: u32 = 4;
const FRAME_US: u32 = 16_743;
const DRAW_US: u32 = 4_000;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn replay_us(console: Option<Console>) -> u32 {
    match console {
        Some(Console::Gb) | None => 1_750,
        Some(Console::Gbc) => 1_850,
        Some(Console::Gba) => 15_000,
    }
}

#[cfg(target_arch = "wasm32")]
fn replay_us(console: Option<Console>) -> u32 {
    match console {
        Some(Console::Gb) | None => 2_400,
        Some(Console::Gbc) => 2_650,
        Some(Console::Gba) => 19_500,
    }
}

#[cfg(target_os = "android")]
fn replay_us(console: Option<Console>) -> u32 {
    match console {
        Some(Console::Gb) | None => 4_400,
        Some(Console::Gbc) => 4_650,
        Some(Console::Gba) => 37_500,
    }
}

fn least_budget(console: Option<Console>) -> u32 {
    match console {
        Some(Console::Gb | Console::Gbc) | None => MAX_ROLLBACK_FRAMES,
        Some(Console::Gba) => 0,
    }
}

pub fn capacity(console: Option<Console>) -> usize {
    match console {
        Some(Console::Gb | Console::Gbc) | None => 2,
        Some(Console::Gba) => 4,
    }
}

pub fn budget(console: Option<Console>, units: usize) -> u32 {
    let units = units.clamp(2, citrine_gb_net::MAX_UNITS) as u32;
    ((FRAME_US - DRAW_US) / (units * replay_us(console)))
        .max(least_budget(console))
        .min(MAX_ROLLBACK_FRAMES)
}

pub fn sync_for(console: Option<Console>, units: usize) -> Sync {
    Sync::new(budget(console, units), 0)
}

pub fn delay_for(choice: Option<u32>, round_trip_ms: Option<u32>, budget: u32) -> u32 {
    if let Some(frames) = choice {
        return frames.min(MAX_INPUT_DELAY);
    }
    let Some(ms) = round_trip_ms else {
        return 0;
    };
    let late = (ms * 1_000 / 2).div_ceil(FRAME_US) + 1;
    let floor = if ms > SLOW_ROUND_TRIP_MS {
        u32::from(SLOW_DELAY)
    } else {
        0
    };
    late.saturating_sub(budget).max(floor).min(MAX_INPUT_DELAY)
}

pub fn group_delay(choice: Option<u32>, round_trips: &[Option<u32>], budget: u32) -> u32 {
    round_trips
        .iter()
        .map(|&ms| delay_for(choice, ms, budget))
        .max()
        .unwrap_or_else(|| delay_for(choice, None, budget))
}

pub fn fits(group: Option<Console>, asker: Option<Console>) -> bool {
    let gba = |c: Console| c == Console::Gba;
    match (group, asker) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(group), Some(asker)) => gba(group) == gba(asker),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkGroup {
    members: Vec<OnlineId>,
    console: Option<Console>,
    round: u32,
    watchers: Vec<OnlineId>,
}

impl LinkGroup {
    pub fn members(&self) -> &[OnlineId] {
        &self.members
    }

    pub fn console(&self) -> Option<Console> {
        self.console
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn watchers(&self) -> &[OnlineId] {
        &self.watchers
    }

    pub fn authority(&self) -> OnlineId {
        self.members[0]
    }

    pub fn holds(&self, who: OnlineId) -> bool {
        self.members.contains(&who)
    }

    pub fn is_full(&self) -> bool {
        self.members.len() >= capacity(self.console)
    }

    pub fn sync(&self) -> Sync {
        sync_for(self.console, self.members.len())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Links {
    groups: Vec<LinkGroup>,
    asking: Vec<OnlineId>,
}

impl Links {
    pub fn groups(&self) -> &[LinkGroup] {
        &self.groups
    }

    pub fn group_of(&self, who: OnlineId) -> Option<&LinkGroup> {
        self.groups.iter().find(|g| g.holds(who))
    }

    pub fn asking(&self) -> &[OnlineId] {
        &self.asking
    }

    pub fn set(&mut self, members: Vec<OnlineId>, console: Option<Console>) {
        let round = members
            .first()
            .and_then(|first| self.group_of(*first))
            .map_or(0, |g| g.round);
        self.set_group(members, console, round, Vec::new());
    }

    pub fn set_group(
        &mut self,
        members: Vec<OnlineId>,
        console: Option<Console>,
        round: u32,
        watchers: Vec<OnlineId>,
    ) {
        let kept_watchers = members
            .first()
            .and_then(|first| self.group_of(*first))
            .map(|g| g.watchers.clone())
            .unwrap_or_default();
        for group in &mut self.groups {
            group.members.retain(|w| !members.contains(w));
            group.watchers.retain(|w| !watchers.contains(w));
        }
        self.asking.retain(|w| !members.contains(w));
        if members.len() > 1 {
            let watchers = if watchers.is_empty() {
                kept_watchers
            } else {
                watchers
            };
            let watchers = watchers
                .into_iter()
                .filter(|w| !members.contains(w))
                .collect();
            self.groups.push(LinkGroup {
                members,
                console,
                round,
                watchers,
            });
        }
        self.groups.retain(|g| g.members.len() > 1);
    }

    pub fn set_watchers(&mut self, of: OnlineId, watchers: Vec<OnlineId>) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.holds(of)) {
            group.watchers = watchers;
        }
    }

    pub fn bump(&mut self, of: OnlineId, round: u32) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.holds(of)) {
            group.round = round;
        }
    }

    pub fn ask(&mut self, from: OnlineId) {
        if !self.asking.contains(&from) {
            self.asking.push(from);
        }
    }

    pub fn cancel(&mut self, from: OnlineId) {
        self.asking.retain(|w| *w != from);
    }

    pub fn prefer(&mut self, from: OnlineId) -> bool {
        let Some(at) = self.asking.iter().position(|w| *w == from) else {
            return false;
        };
        let first = self.asking.remove(at);
        self.asking.insert(0, first);
        true
    }

    pub fn clear_asking(&mut self) {
        self.asking.clear();
    }

    pub fn remove(&mut self, who: OnlineId) {
        self.asking.retain(|w| *w != who);
        for group in &mut self.groups {
            group.members.retain(|w| *w != who);
            group.watchers.retain(|w| *w != who);
        }
        self.groups.retain(|g| g.members.len() > 1);
    }

    pub fn next_for(
        &self,
        me: OnlineId,
        console: Option<Console>,
        asker: impl Fn(OnlineId) -> Option<Console>,
    ) -> Option<Vec<OnlineId>> {
        let mut members = match self.group_of(me) {
            Some(group) if group.authority() != me || group.is_full() => return None,
            Some(group) => group.members.clone(),
            None if capacity(console) > 1 => vec![me],
            None => return None,
        };
        let first = self.asking.iter().copied().find(|w| !members.contains(w));
        if members.len() == 1
            && let Some(first) = first
            && !fits(console, asker(first))
        {
            members.push(first);
            return Some(members);
        }
        let room = capacity(console).saturating_sub(members.len());
        let joining: Vec<OnlineId> = self
            .asking
            .iter()
            .copied()
            .filter(|w| !members.contains(w) && fits(console, asker(*w)))
            .take(room)
            .collect();
        if joining.is_empty() {
            return None;
        }
        members.extend(joining);
        Some(members)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> OnlineId {
        OnlineId::new(u64::from(n)).unwrap()
    }

    fn gb(_: OnlineId) -> Option<Console> {
        Some(Console::Gb)
    }

    #[test]
    fn two_asking_to_link_with_a_free_gba_player_join_one_group() {
        let (a, b, c) = (id(1), id(2), id(3));
        let mut links = Links::default();
        links.ask(b);
        links.ask(c);
        let gba = |_| Some(Console::Gba);
        assert_eq!(
            links.next_for(a, Some(Console::Gba), gba),
            Some(vec![a, b, c])
        );
        assert_eq!(links.next_for(a, Some(Console::Gb), gb), Some(vec![a, b]));
    }

    #[test]
    fn a_free_player_links_with_the_first_asker_whatever_their_console() {
        let (a, b, c) = (id(1), id(2), id(3));
        let mut links = Links::default();
        links.ask(b);
        links.ask(c);
        let gba = |_| Some(Console::Gba);
        assert_eq!(links.next_for(a, Some(Console::Gb), gba), Some(vec![a, b]));
        links.set(vec![a], Some(Console::Gb));
        assert_eq!(links.next_for(a, Some(Console::Gb), gba), Some(vec![a, b]));
    }

    #[test]
    fn a_request_to_join_a_link_waits_until_the_console_fits_and_there_is_room() {
        let (a, b, c, d, e) = (id(1), id(2), id(3), id(4), id(5));
        let mut links = Links::default();
        links.set(vec![a, b, c], Some(Console::Gba));
        links.ask(d);
        let on_gb = |w| Some(if w == d { Console::Gb } else { Console::Gba });
        assert_eq!(links.next_for(a, Some(Console::Gba), on_gb), None);
        assert_eq!(
            links.next_for(b, Some(Console::Gba), |_| Some(Console::Gba)),
            None
        );
        links.ask(e);
        assert_eq!(
            links.next_for(a, Some(Console::Gba), on_gb),
            Some(vec![a, b, c, e])
        );
        links.set(vec![a, b, c, e], Some(Console::Gba));
        assert_eq!(
            links.next_for(a, Some(Console::Gba), |_| Some(Console::Gba)),
            None
        );
        assert_eq!(links.asking(), [d]);
    }

    #[test]
    fn the_group_delay_follows_the_slowest_member() {
        let trips = [Some(20), Some(150), Some(40)];
        assert_eq!(group_delay(None, &trips, 0), delay_for(None, Some(150), 0));
        assert!(group_delay(None, &trips, 0) > delay_for(None, Some(40), 0));
        assert_eq!(group_delay(Some(1), &trips, 0), 1);
    }

    #[test]
    fn a_request_to_a_full_pair_waits_until_it_unlinks() {
        let (a, b, c) = (id(1), id(2), id(3));
        let mut links = Links::default();
        links.set(vec![a, b], Some(Console::Gb));
        links.ask(c);
        assert_eq!(links.next_for(a, Some(Console::Gb), gb), None);
        assert_eq!(links.next_for(b, Some(Console::Gb), gb), None);
        links.set(vec![a], Some(Console::Gb));
        assert!(links.groups().is_empty());
        assert_eq!(links.next_for(b, Some(Console::Gb), gb), Some(vec![b, c]));
        links.set(vec![b, c], Some(Console::Gb));
        assert!(links.asking().is_empty());
        assert_eq!(links.group_of(c).unwrap().authority(), b);
    }

    #[test]
    fn requests_are_answered_in_the_order_they_came() {
        let (a, b, c, d) = (id(1), id(2), id(3), id(4));
        let mut links = Links::default();
        links.set(vec![a, b], Some(Console::Gb));
        links.ask(c);
        links.ask(d);
        links.ask(c);
        assert_eq!(links.asking(), [c, d]);
        links.remove(b);
        assert_eq!(links.next_for(a, Some(Console::Gb), gb), Some(vec![a, c]));
        links.cancel(c);
        assert_eq!(links.next_for(a, Some(Console::Gb), gb), Some(vec![a, d]));
    }

    #[test]
    fn gba_groups_hold_four_and_game_boy_groups_two() {
        assert_eq!(capacity(Some(Console::Gba)), 4);
        assert_eq!(capacity(Some(Console::Gb)), 2);
        assert_eq!(capacity(Some(Console::Gbc)), 2);
    }

    #[test]
    fn every_console_gets_the_same_kind_of_sync_with_a_budget_that_shrinks_with_units() {
        for console in [Console::Gb, Console::Gbc, Console::Gba] {
            let two = sync_for(Some(console), 2);
            let four = sync_for(Some(console), 4);
            assert_eq!(two.delay, 0);
            assert!(four.budget <= two.budget);
            assert!(two.budget <= MAX_ROLLBACK_FRAMES);
        }
        assert!(budget(Some(Console::Gba), 2) <= budget(Some(Console::Gb), 2));
        assert_eq!(budget(Some(Console::Gba), 2), 0);
        assert_eq!(delay_for(None, Some(20), 0), 2);
    }

    #[test]
    fn a_chosen_delay_wins_and_auto_covers_what_the_budget_cannot() {
        assert_eq!(delay_for(Some(3), Some(400), 0), 3);
        assert_eq!(delay_for(Some(9), None, 0), MAX_INPUT_DELAY);
        assert_eq!(delay_for(None, None, 0), 0);
        assert_eq!(delay_for(None, Some(20), 4), 0);
        assert_eq!(delay_for(None, Some(100), 1), 3);
        assert_eq!(delay_for(None, Some(100), 0), 4);
        assert_eq!(delay_for(None, Some(130), 30), u32::from(SLOW_DELAY));
        assert_eq!(delay_for(None, Some(1_000), 2), MAX_INPUT_DELAY);
    }

    #[test]
    fn game_boy_rooms_keep_the_window_and_delay_they_had() {
        for console in [Console::Gb, Console::Gbc] {
            for units in [2, 4] {
                assert_eq!(
                    sync_for(Some(console), units),
                    Sync::new(MAX_ROLLBACK_FRAMES, 0)
                );
            }
        }
        let budget = budget(Some(Console::Gb), 2);
        assert_eq!(delay_for(None, Some(80), budget), 0);
        assert_eq!(delay_for(None, Some(150), budget), u32::from(SLOW_DELAY));
    }

    #[test]
    fn the_group_hands_its_size_to_the_sync() {
        let mut links = Links::default();
        links.set(vec![id(1), id(2), id(3), id(4)], Some(Console::Gba));
        let group = links.group_of(id(1)).unwrap();
        assert!(group.is_full());
        assert_eq!(group.sync(), sync_for(Some(Console::Gba), 4));
    }

    #[test]
    fn the_longest_member_takes_over_when_the_authority_leaves() {
        let (a, b, c) = (id(1), id(2), id(3));
        let mut links = Links::default();
        links.set(vec![a, b, c], None);
        assert_eq!(links.group_of(c).unwrap().authority(), a);
        links.remove(a);
        assert_eq!(links.group_of(c).unwrap().authority(), b);
        links.remove(b);
        assert!(links.group_of(c).is_none());
    }
}
