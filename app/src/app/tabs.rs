use crate::app::events::AppEventQueue;
use crate::app::ui_state::UiState;
use crate::audio::Audio;
use crate::emulator::Emulator;
use crate::icons;
use crate::media::MediaStore;
use crate::pad_input::PadSource;
use crate::utils::file_channels::FileChannels;
use egui::{Ui, WidgetText};
use std::collections::{HashMap, HashSet};
use strum_macros::EnumIter;

mod apu;
mod apu_waves;
mod audio_debug;
mod controller_debug;
mod debug_actions;
mod disassembly;
mod e2e;
mod game_boy;
mod homebrew;
mod info;
pub mod media;
mod performance;
mod registers;
mod rom_info;
pub mod saves;
mod settings;
mod time_control;

pub const CONTENT_MARGIN: egui::Margin = egui::Margin::symmetric(10, 8);

pub struct TabViewer<'a> {
    pub audio: &'a mut Option<Audio>,
    pub commonmark: &'a mut egui_commonmark::CommonMarkCache,
    pub emulator: &'a mut Emulator,
    pub pads: &'a dyn PadSource,
    pub events: &'a mut AppEventQueue,
    pub files: &'a mut FileChannels,
    pub media: &'a mut MediaStore,
    pub ui: &'a mut UiState,
}

impl TabViewer<'_> {
    fn content(&mut self, ui: &mut Ui, tab: Tab) {
        match tab {
            Tab::GameBoy => game_boy::show(self, ui),
            Tab::Settings => settings::show(self, ui),
            Tab::Saves => saves::show(self, ui),
            Tab::Media => media::show(self, ui),
            Tab::TimeControl => time_control::show(self, ui),
            Tab::Registers => registers::show(self, ui),
            Tab::RomInfo => rom_info::show(self, ui),
            Tab::E2ETest => e2e::show(self, ui),
            Tab::Homebrew => homebrew::show(self, ui),
            Tab::Info => info::show(self, ui),
            Tab::AudioDebug => audio_debug::show(self, ui),
            Tab::ControllerDebug => controller_debug::show(self, ui),
            Tab::Apu => apu::show(self, ui),
            Tab::ApuWaves => apu_waves::show(self, ui),
            Tab::Disassembly => disassembly::show(self, ui),
            Tab::Performance => performance::show(self, ui),
            Tab::DebugActions => debug_actions::show(self, ui),
        }
    }
}

impl<'a> egui_dock::TabViewer for TabViewer<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        let recorder = &self.emulator.screen_recorder;
        if *tab == Tab::GameBoy && recorder.is_recording() {
            let time = crate::app::recording_time(recorder);
            return egui::RichText::new(format!("{}  {}  {time}", tab.title(), icons::RECORD))
                .color(egui::Color32::RED)
                .into();
        }
        match self.ui.tab_fit.titles.get(tab) {
            Some(short) => short.as_str().into(),
            None => tab.title().into(),
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        if self.ui.tab_fit.titles.contains_key(tab) {
            response.clone().on_hover_text(tab.title());
        }
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(tab.title())
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        if *tab == Tab::GameBoy {
            game_boy::show(self, ui);
            return;
        }
        egui::Frame::new()
            .inner_margin(CONTENT_MARGIN)
            .show(ui, |ui| self.content(ui, *tab));
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        tab.closable() && !self.ui.tab_fit.no_close.contains(tab)
    }

    fn allowed_in_windows(&self, tab: &mut Self::Tab) -> bool {
        tab.closable()
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        [
            !matches!(
                tab,
                Tab::Saves | Tab::Media | Tab::Info | Tab::ControllerDebug
            ),
            true,
        ]
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum Tab {
    Apu,
    ApuWaves,
    AudioDebug,
    ControllerDebug,
    DebugActions,
    Disassembly,
    E2ETest,
    GameBoy,
    Homebrew,
    Info,
    Media,
    Performance,
    Registers,
    RomInfo,
    Saves,
    Settings,
    TimeControl,
}

impl Tab {
    pub fn title(&self) -> &'static str {
        match self {
            Tab::GameBoy => "Game Boy",
            Tab::Settings => "Settings",
            Tab::Saves => "Saves",
            Tab::Media => "Media",
            Tab::TimeControl => "Time control",
            Tab::Registers => "Registers",
            Tab::RomInfo => "ROM info",
            Tab::E2ETest => "E2E tests",
            Tab::Homebrew => "Homebrew",
            Tab::Info => "About",
            Tab::AudioDebug => "Audio debug",
            Tab::ControllerDebug => "Controller debug",
            Tab::Apu => "APU",
            Tab::ApuWaves => "APU waves",
            Tab::Disassembly => "Disassembly",
            Tab::Performance => "Performance",
            Tab::DebugActions => "Debug actions",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Homebrew => "Homebrew games",
            Tab::Saves => "Saves & snapshots",
            Tab::Info => "About & controls",
            other => other.title(),
        }
    }

    pub fn scrolls_itself(&self) -> bool {
        matches!(
            self,
            Tab::GameBoy
                | Tab::Saves
                | Tab::Media
                | Tab::Settings
                | Tab::Homebrew
                | Tab::Disassembly
        )
    }

    pub fn closable(&self) -> bool {
        !matches!(self, Tab::GameBoy)
    }
}

const TAB_PADDING: f32 = 16.0;
const TAB_CLOSE: f32 = 24.0;

#[derive(Default)]
pub struct TabFit {
    pub titles: HashMap<Tab, String>,
    pub no_close: HashSet<Tab>,
}

pub fn fit_titles(ctx: &egui::Context, dock: &egui_dock::DockState<Tab>) -> TabFit {
    let font = egui::TextStyle::Button.resolve(&ctx.style());
    let measure = |text: &str| {
        ctx.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
        })
    };

    let mut fit = TabFit::default();
    for (_, leaf) in dock.iter_leaves() {
        if !leaf.rect.is_positive() || !leaf.rect.is_finite() {
            continue;
        }
        let available = leaf.rect.width() - 1.0;
        let text: Vec<f32> = leaf.tabs.iter().map(|tab| measure(tab.title())).collect();
        let all_close = |index: usize| leaf.tabs[index].closable();
        let active_close = |index: usize| all_close(index) && index == leaf.active.0;
        let overhead = |close: bool| TAB_PADDING + if close { TAB_CLOSE } else { 0.0 };
        let widths = |close: &dyn Fn(usize) -> bool| -> Vec<f32> {
            (0..text.len())
                .map(|i| text[i] + overhead(close(i)))
                .collect()
        };

        let full = widths(&all_close);
        if full.iter().sum::<f32>() <= available {
            continue;
        }
        let narrow = widths(&active_close);
        let budgets = title_budgets(available, &narrow);
        for (index, tab) in leaf.tabs.iter().enumerate() {
            if all_close(index) && !active_close(index) {
                fit.no_close.insert(*tab);
            }
            if let Some(budget) = budgets[index] {
                let room = budget - overhead(active_close(index));
                fit.titles.insert(*tab, shorten(tab.title(), room, measure));
            }
        }
    }
    fit
}

fn title_budgets(available: f32, widths: &[f32]) -> Vec<Option<f32>> {
    if widths.iter().sum::<f32>() <= available {
        return vec![None; widths.len()];
    }
    let mut order: Vec<usize> = (0..widths.len()).collect();
    order.sort_by(|a, b| widths[*a].total_cmp(&widths[*b]));

    let mut budgets = vec![None; widths.len()];
    let mut remaining = available;
    for (done, &index) in order.iter().enumerate() {
        let share = remaining / (widths.len() - done) as f32;
        if widths[index] <= share {
            remaining -= widths[index];
        } else {
            for &rest in &order[done..] {
                budgets[rest] = Some(share);
            }
            break;
        }
    }
    budgets
}

fn shorten(title: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(title) <= max {
        return title.to_owned();
    }
    let chars: Vec<char> = title.chars().collect();
    for keep in (1..chars.len()).rev() {
        let prefix: String = chars[..keep].iter().collect();
        let short = format!("{}…", prefix.trim_end());
        if measure(&short) <= max || keep == 1 {
            return short;
        }
    }
    format!("{}…", chars.first().copied().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_that_fit_keep_their_width() {
        assert_eq!(title_budgets(300.0, &[100.0, 80.0, 90.0]), vec![None; 3]);
    }

    #[test]
    fn only_the_long_titles_give_up_room() {
        let budgets = title_budgets(250.0, &[106.0, 83.0, 70.0]);
        assert_eq!(budgets[1], None, "a title that fits stays whole");
        assert_eq!(budgets[2], None, "the shortest title stays whole");
        assert_eq!(budgets[0], Some(250.0 - 83.0 - 70.0));
    }

    #[test]
    fn four_tabs_share_one_strip() {
        let widths = [106.0, 83.0, 70.0, 68.0];
        let budgets = title_budgets(245.0, &widths);
        let used: f32 = widths
            .iter()
            .zip(&budgets)
            .map(|(width, budget)| budget.unwrap_or(*width))
            .sum();
        assert!(used <= 245.0 + 0.01, "used {used}");
    }

    #[test]
    fn a_shortened_title_ends_in_an_ellipsis() {
        let measure = |text: &str| text.chars().count() as f32 * 10.0;
        assert_eq!(shorten("Settings", 80.0, measure), "Settings");
        assert_eq!(shorten("General Info", 80.0, measure), "General…");
        assert_eq!(shorten("Saves", 5.0, measure), "S…");
    }

    const PROPER_NOUNS: [&str; 7] = [
        "Game",
        "Boy",
        "Color",
        "Citrine",
        "SameBoy",
        "RetroArch",
        "Nintendo",
    ];

    fn sentence_case_slips(text: &str) -> Vec<String> {
        let mut slips = Vec::new();
        let mut sentence_start = true;
        for word in text.split_whitespace() {
            let letters: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
            let capital = letters.chars().next().is_some_and(char::is_uppercase);
            let acronym = letters.chars().all(|c| !c.is_lowercase());
            let plural_acronym = letters
                .strip_suffix('s')
                .is_some_and(|stem| stem.len() > 1 && stem.chars().all(|c| !c.is_lowercase()));
            if capital
                && !sentence_start
                && !acronym
                && !plural_acronym
                && !PROPER_NOUNS.contains(&letters.as_str())
            {
                slips.push(word.to_owned());
            }
            if !letters.is_empty() {
                sentence_start = word.ends_with(['.', '?', '!']);
            }
        }
        slips
    }

    fn hover_literals(source: &str) -> Vec<String> {
        source
            .split("on_hover_text(")
            .skip(1)
            .filter_map(|rest| {
                let rest = rest.trim_start().strip_prefix('"')?;
                Some(rest[..rest.find('"')?].to_owned())
            })
            .collect()
    }

    fn rust_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    #[test]
    fn sentence_case_check_allows_acronyms_and_names() {
        assert!(sentence_case_slips("ROM info").is_empty());
        assert!(sentence_case_slips("How Game Boy Color colours look. Like a GBC.").is_empty());
        assert!(sentence_case_slips("Game Boy ROMs").is_empty());
        assert_eq!(sentence_case_slips("Homebrew Games"), vec!["Games"]);
    }

    #[test]
    fn tab_names_are_in_sentence_case() {
        use strum::IntoEnumIterator;
        for tab in Tab::iter() {
            for name in [tab.title(), tab.label()] {
                assert!(sentence_case_slips(name).is_empty(), "{name}");
            }
        }
    }

    #[test]
    fn hover_texts_are_in_sentence_case() {
        let mut files = Vec::new();
        rust_files(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut slips = Vec::new();
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            for text in hover_literals(&source) {
                if !sentence_case_slips(&text).is_empty() {
                    slips.push(format!("{}: {text}", file.display()));
                }
            }
        }
        assert!(slips.is_empty(), "{slips:#?}");
    }
}
