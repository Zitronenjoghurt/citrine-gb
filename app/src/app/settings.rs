use crate::emulator::Emulator;
use citrine_gb::gb::ppu::types::theme::DmgTheme;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    ui_scale: f32,
    dmg_theme: DmgTheme,
    matrix: bool,
    ghosting: bool,
    matrix_edge_darkness: f32,
    matrix_corner_darkness: f32,
    ghosting_strength: f32,
    pub dirty: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            #[cfg(target_arch = "wasm32")]
            ui_scale: 3.0,
            #[cfg(not(target_arch = "wasm32"))]
            ui_scale: 1.5,
            dmg_theme: DmgTheme::default(),
            matrix: false,
            ghosting: false,
            matrix_edge_darkness: 0.15,
            matrix_corner_darkness: 0.25,
            ghosting_strength: 0.3,
            dirty: true,
        }
    }
}

impl Settings {
    pub fn apply(&mut self, ctx: &egui::Context, emulator: &mut Emulator) {
        if !self.dirty {
            return;
        }

        ctx.set_pixels_per_point(self.ui_scale);
        emulator.gb.ppu.dmg_theme = self.dmg_theme;
        emulator.enable_matrix = self.matrix;
        emulator.enable_ghosting = self.ghosting;
        emulator.matrix_edge_brightness = 1.0 - self.matrix_edge_darkness;
        emulator.matrix_corner_brightness = 1.0 - self.matrix_corner_darkness;
        emulator.ghosting_blend = 1.0 - self.ghosting_strength;

        self.dirty = false;
    }

    fn set_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    pub fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
        self.set_dirty();
    }

    pub fn dmg_theme(&self) -> DmgTheme {
        self.dmg_theme
    }

    pub fn set_dmg_theme(&mut self, theme: DmgTheme) {
        self.dmg_theme = theme;
        self.set_dirty();
    }

    pub fn ghosting(&self) -> bool {
        self.ghosting
    }

    pub fn set_ghosting(&mut self, ghosting: bool) {
        self.ghosting = ghosting;
        self.set_dirty();
    }

    pub fn matrix(&self) -> bool {
        self.matrix
    }

    pub fn set_matrix(&mut self, matrix: bool) {
        self.matrix = matrix;
        self.set_dirty();
    }

    pub fn matrix_edge_darkness(&self) -> f32 {
        self.matrix_edge_darkness
    }

    pub fn set_matrix_edge_darkness(&mut self, darkness: f32) {
        self.matrix_edge_darkness = darkness;
        self.set_dirty();
    }

    pub fn matrix_corner_darkness(&self) -> f32 {
        self.matrix_corner_darkness
    }

    pub fn set_matrix_corner_darkness(&mut self, darkness: f32) {
        self.matrix_corner_darkness = darkness;
        self.set_dirty();
    }

    pub fn ghosting_strength(&self) -> f32 {
        self.ghosting_strength
    }

    pub fn set_ghosting_strength(&mut self, strength: f32) {
        self.ghosting_strength = strength;
        self.set_dirty();
    }
}
