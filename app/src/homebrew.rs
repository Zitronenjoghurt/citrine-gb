use crate::icons;

include!(concat!(env!("OUT_DIR"), "/homebrew.rs"));

impl LinkKind {
    pub fn icon(&self) -> &'static str {
        match self {
            LinkKind::Official => icons::BUILDING_OFFICE,
            LinkKind::Itch => icons::GAME_CONTROLLER,
            LinkKind::Github => icons::GITHUB_LOGO,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            LinkKind::Official => "Website",
            LinkKind::Itch => "itch.io",
            LinkKind::Github => "GitHub",
        }
    }
}

impl HomebrewGame {
    pub fn show_links(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for link in self.links {
                if ui
                    .button(format!("{} {}", link.kind.icon(), link.kind.label()))
                    .on_hover_text(link.url)
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(link.url));
                }
            }
        });
    }
}
