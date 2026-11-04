use crate::app::tabs::{Tab, Tool};
use crate::app::widgets::text::faint;
use crate::icons;
use egui::{Button, Ui};

pub struct DevSection {
    pub title: &'static str,
    pub tools: &'static [(&'static str, Tab)],
}

pub const DEV_SECTIONS: [DevSection; 3] = [
    DevSection {
        title: "Code",
        tools: &[
            (icons::BUG, Tab::Disassembly),
            (icons::CPU, Tab::Registers),
            (icons::MEMORY, Tab::Tool(Tool::Memory)),
        ],
    },
    DevSection {
        title: "Video and sound",
        tools: &[
            (icons::GRID_FOUR, Tab::Tool(Tool::Video)),
            (icons::WAVEFORM, Tab::Apu),
        ],
    },
    DevSection {
        title: "App",
        tools: &[
            (icons::GAUGE, Tab::Performance),
            (icons::SPEAKER_HIGH, Tab::AudioDebug),
            (icons::GAME_CONTROLLER, Tab::ControllerDebug),
            (icons::WRENCH, Tab::DebugActions),
            (icons::TEST_TUBE, Tab::E2ETest),
        ],
    },
];

pub fn dev_menu(ui: &mut Ui) -> Option<Tab> {
    let mut picked = None;
    for (index, section) in DEV_SECTIONS.iter().enumerate() {
        if index > 0 {
            ui.separator();
        }
        ui.label(faint(section.title));
        for (icon, tab) in section.tools {
            let label = format!("{icon}  {}", tab.label());
            if ui.add(Button::new(label).frame(false)).clicked() {
                picked = Some(*tab);
            }
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    const NOT_DEV: [Tab; 14] = [
        Tab::GameBoy,
        Tab::GameInfo,
        Tab::Settings,
        Tab::Saves,
        Tab::Media,
        Tab::Stats,
        Tab::RomInfo,
        Tab::Friends,
        Tab::Homebrew,
        Tab::Info,
        Tab::Playing,
        Tab::RoomChat,
        Tab::TimeControl,
        Tab::ApuWaves,
    ];

    #[test]
    fn every_dev_tab_sits_in_one_section() {
        let tools = Tool::iter()
            .filter(|tool| *tool != Tool::Unknown)
            .map(Tab::Tool);
        let tabs = Tab::iter().filter(|tab| !matches!(tab, Tab::Tool(_)));
        for tab in tabs.chain(tools).filter(|tab| !NOT_DEV.contains(tab)) {
            let homes = DEV_SECTIONS
                .iter()
                .flat_map(|section| section.tools)
                .filter(|(_, listed)| *listed == tab)
                .count();
            assert_eq!(homes, 1, "{tab:?}");
        }
    }

    #[test]
    fn every_tool_has_its_own_icon() {
        let mut seen = std::collections::HashSet::new();
        for (icon, tab) in DEV_SECTIONS.iter().flat_map(|section| section.tools) {
            assert!(seen.insert(*icon), "{tab:?} shares its icon");
        }
    }
}
