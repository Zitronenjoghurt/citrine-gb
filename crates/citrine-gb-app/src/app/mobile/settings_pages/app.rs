use super::{Sheet, label, opens, pick, settled, slide, stepped};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::ui_state::settings::LayoutMode;
use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::touch::TouchSettings;
use crate::app::widgets::settings::pages::app::{
    COVERS_NOTE, DISCORD_DETAILS_HINT, DISCORD_PICTURES_OFF, DISCORD_SCREENSHOT_HINT,
    HIDE_POINTER_HINT, PROGRESS_NOTE, UPDATES_NOTE,
};
use egui::{Context, Ui};

const LAYOUT_NOTE: &str = "Automatic uses the touch layout on small screens.";

impl Citrine {
    pub(super) fn app_page(&mut self, ui: &mut Ui) {
        let narrow = LayoutMode::too_narrow_for_desktop(ui.ctx());
        self.tree_page(
            ui,
            SettingsTab::App,
            |_| None,
            |app, list, row| app.app_row(list, row, narrow),
            |_| {},
        );
    }

    fn app_row(&mut self, list: &mut kit::List<'_>, row: RowId, narrow: bool) -> Option<Sheet> {
        let pictures_off = self.online.pictures_off();
        let s = &mut self.ui.settings;
        let mut dirty = false;
        let open = match row {
            RowId::Theme => stepped(
                list,
                Row::new(label(row)).value(s.ui_theme.to_string()),
                &mut s.ui_theme,
                &mut dirty,
                Sheet::Theme,
            ),
            RowId::AppLayout => {
                let too_narrow = if s.layout == LayoutMode::Desktop && narrow {
                    "Too narrow here, so it uses touch"
                } else {
                    ""
                };
                stepped(
                    list,
                    Row::new(label(row))
                        .value(s.layout.to_string())
                        .detail(too_narrow),
                    &mut s.layout,
                    &mut dirty,
                    Sheet::Layout,
                )
            }
            RowId::UiScale => opens(
                list,
                Row::new(label(row)).value(sheet::percent(s.touch.ui_scale.into())),
                Sheet::UiSize,
            ),
            RowId::Covers => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.covers).info(COVERS_NOTE));
                None
            }
            RowId::AchievementProgress => {
                dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.achievements.in_library).info(PROGRESS_NOTE),
                );
                None
            }
            RowId::DiscordActivity => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.discord.enabled));
                None
            }
            RowId::DiscordGame if s.discord.enabled => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.discord.game));
                None
            }
            RowId::DiscordDetails if s.discord.enabled => {
                dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.discord.details).info(DISCORD_DETAILS_HINT),
                );
                None
            }
            RowId::DiscordTime if s.discord.enabled => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.discord.play_time));
                None
            }
            RowId::DiscordShot if s.discord.enabled => {
                let mut off = false;
                let shot = if pictures_off {
                    &mut off
                } else {
                    &mut s.discord.screenshot
                };
                dirty |= list.switch(
                    SwitchRow::new(label(row), shot)
                        .info(DISCORD_SCREENSHOT_HINT)
                        .detail(pictures_off.then_some(DISCORD_PICTURES_OFF))
                        .enabled(!pictures_off),
                );
                None
            }
            RowId::DiscordEvery if s.discord.enabled && s.discord.screenshot && !pictures_off => {
                opens(
                    list,
                    Row::new(label(row)).value(s.discord.every.to_string()),
                    Sheet::DiscordEvery,
                )
            }
            RowId::DiscordDsScreens
                if s.discord.enabled && s.discord.screenshot && !pictures_off =>
            {
                opens(
                    list,
                    Row::new(label(row)).value(s.discord.screens.to_string()),
                    Sheet::DiscordScreens,
                )
            }
            RowId::Fullscreen => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.fullscreen));
                None
            }
            RowId::HidePointer => {
                dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.hide_pointer).info(HIDE_POINTER_HINT),
                );
                None
            }
            RowId::Subfolders => opens(
                list,
                Row::new(label(row)).value(s.library_depth.to_string()),
                Sheet::Subfolders,
            ),
            RowId::CheckUpdates => {
                dirty |= list
                    .switch(SwitchRow::new(label(row), &mut s.check_updates).info(UPDATES_NOTE));
                None
            }
            _ => None,
        };
        s.dirty |= dirty;
        open
    }

    pub(super) fn app_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let s = &mut self.ui.settings;
        match open {
            Sheet::UiSize => {
                let shown = slide(
                    ctx,
                    label(RowId::UiScale),
                    None,
                    Amount {
                        value: &mut s.touch.ui_scale,
                        range: TouchSettings::MIN_UI_SCALE..=TouchSettings::MAX_UI_SCALE,
                        step: 0.05,
                        default: 1.0,
                        text: sheet::percent,
                    },
                );
                if settled(&shown.inner) {
                    s.dirty = true;
                }
                s.note_scale_drag(&shown.inner);
                shown.closed
            }
            Sheet::Layout => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::AppLayout),
                    Some(LAYOUT_NOTE),
                    &mut s.layout,
                );
                s.dirty |= changed;
                done
            }
            Sheet::DiscordEvery => {
                let (changed, done) =
                    pick(ctx, label(RowId::DiscordEvery), None, &mut s.discord.every);
                s.dirty |= changed;
                done
            }
            Sheet::DiscordScreens => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::DiscordDsScreens),
                    None,
                    &mut s.discord.screens,
                );
                s.dirty |= changed;
                done
            }
            Sheet::Subfolders => {
                let (changed, done) =
                    pick(ctx, label(RowId::Subfolders), None, &mut s.library_depth);
                s.dirty |= changed;
                done
            }
            _ => {
                let (changed, done) = pick(ctx, label(RowId::Theme), None, &mut s.ui_theme);
                s.dirty |= changed;
                done
            }
        }
    }
}
