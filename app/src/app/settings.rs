use crate::Citrine;
use citrine_gb::gb::ppu::types::theme::DmgTheme;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    ui_scale: f32,
    dmg_theme: DmgTheme,
    #[serde(skip, default = "web_time::Instant::now")]
    last_change: web_time::Instant,
    dirty: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            #[cfg(target_arch = "wasm32")]
            ui_scale: 3.0,
            #[cfg(not(target_arch = "wasm32"))]
            ui_scale: 1.5,
            dmg_theme: DmgTheme::default(),
            last_change: web_time::Instant::now(),
            dirty: true,
        }
    }
}

impl Settings {
    pub fn apply(&self, ctx: &egui::Context, app: &mut Citrine) {
        if !self.is_dirty() {
            return;
        }

        ctx.set_pixels_per_point(self.ui_scale);
        app.emulator.gb.ppu.dmg_theme = self.dmg_theme;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty && self.last_change.elapsed() > web_time::Duration::from_millis(500)
    }

    fn set_dirty(&mut self) {
        self.dirty = true;
        self.last_change = web_time::Instant::now();
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
}
