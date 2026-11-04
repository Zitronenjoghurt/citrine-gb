use crate::app::tabs::TabViewer;
use crate::app::widgets::registers::RegisterTable;
use citrine_gb::debug::Debug;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let mut groups = Vec::new();
    let machine = viewer.emulator.debug();
    machine.registers(&mut groups);
    let cpu_groups = machine.features().cpu_groups;
    groups.retain(|group| !cpu_groups.contains(&group.name));
    RegisterTable::new(&groups, &mut viewer.ui.registers).ui(ui);
}
