#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Pico,
    Serebii,
    Rom,
    Chosen,
}

macro_rules! rules {
    ($($name:ident: $ty:ty = $value:expr, $source:ident;)*) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Rules {
            $(pub $name: $ty,)*
        }

        impl Rules {
            pub const ORIGINAL: Rules = Rules {
                $($name: $value,)*
            };

            pub const SOURCES: &'static [(&'static str, Source)] = &[
                $((stringify!($name), Source::$source),)*
            ];

            pub fn source(name: &str) -> Option<Source> {
                Self::SOURCES
                    .iter()
                    .find(|(rule, _)| *rule == name)
                    .map(|(_, source)| *source)
            }
        }

        impl Default for Rules {
            fn default() -> Self {
                Self::ORIGINAL
            }
        }
    };
}

rules! {
    steps_per_watt: u32 = 20, Rom;
    max_today_steps: u32 = 99_999, Rom;
    max_total_steps: u32 = 9_999_999, Rom;
    max_hour_steps: u16 = 9_999, Rom;
    max_watts: u16 = 9_999, Rom;
    max_days: u16 = 9_999, Rom;
    history_days: usize = 7, Rom;
    event_log_entries: u8 = 23, Rom;
    met_peers: usize = 10, Rom;
    draw_ms: u64 = 250, Rom;
    passes_per_draw: u32 = 5, Rom;
    catch_up_draws: u32 = 256, Chosen;
    awake_seconds: u8 = 90, Rom;
    wake_seconds: u8 = 60, Rom;
    home_event_seconds: u16 = 3_600, Rom;
    home_chance: u8 = 40, Rom;
    home_offer_passes: u32 = 48, Rom;
    home_item_happiness: u8 = 90, Rom;
    home_item_steps: u16 = 500, Rom;
    home_big_happiness: u8 = 80, Rom;
    home_big_steps: u16 = 250, Rom;
    home_mid_steps: u16 = 200, Rom;
    home_small_steps: u16 = 100, Rom;
    home_watts: [u16; 3] = [50, 20, 10], Rom;
    home_nothing_minutes: u16 = 60, Rom;
    home_nothing_steps: u16 = 50, Rom;
    home_joined_steps: u16 = 300, Rom;
    joined_happiness: u8 = 70, Rom;
    app_costs: [u16; 6] = [10, 3, 0, 0, 0, 0], Rom;
    radar_bushes: u8 = 4, Rom;
    radar_shown_passes: [u32; 4] = [64, 48, 32, 24], Rom;
    radar_first_hidden_passes: u32 = 5, Rom;
    radar_hidden_base: u32 = 16, Rom;
    radar_hidden_spread: [u32; 3] = [16, 32, 48], Rom;
    radar_hit_passes: u32 = 16, Rom;
    radar_start_draws: u32 = 5, Rom;
    radar_slot_hits: [u8; 3] = [3, 2, 1], Rom;
    radar_event_hits: u8 = 3, Rom;
    radar_fallback_hits: u8 = 1, Rom;
    battle_hp: u8 = 4, Rom;
    battle_action_odds: [[u8; 3]; 5] = [
        [45, 35, 20],
        [40, 30, 30],
        [50, 40, 10],
        [60, 30, 10],
        [20, 30, 50],
    ], Rom;
    battle_row_after: [usize; 3] = [1, 3, 2], Rom;
    battle_evade_row_after: [usize; 2] = [1, 4], Rom;
    battle_special_damage: u8 = 2, Rom;
    battle_counter_damage: u8 = 1, Rom;
    battle_intro_draws: u32 = 9, Rom;
    battle_round_draws: u32 = 8, Rom;
    catch_odds: [u8; 4] = [97, 79, 66, 56], Rom;
    catch_checks: u8 = 3, Rom;
    catch_check_draws: u32 = 8, Rom;
    lost_battle_watts: u16 = 10, Rom;
    max_caught: usize = 3, Rom;
    dowsing_bushes: u8 = 6, Rom;
    dowsing_tries: u8 = 2, Rom;
    dowsing_near: u8 = 1, Rom;
    dowsing_shake_draws: u32 = 4, Rom;
    dowsing_watts_per_try: u16 = 4, Rom;
    dowsing_watts_base: u16 = 2, Rom;
    max_items: usize = 3, Rom;
    sound_levels: u8 = 3, Rom;
    shade_levels: u8 = 10, Rom;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_names_its_source() {
        assert_eq!(Rules::source("steps_per_watt"), Some(Source::Rom));
        assert_eq!(Rules::source("catch_up_draws"), Some(Source::Chosen));
        assert_eq!(Rules::source("nothing"), None);
        assert_eq!(Rules::SOURCES.len(), 61);
    }

    #[test]
    fn the_odds_rows_each_add_up_to_a_hundred() {
        let rules = Rules::ORIGINAL;
        for row in rules.battle_action_odds {
            assert_eq!(row.iter().map(|p| *p as u32).sum::<u32>(), 100);
        }
    }
}
