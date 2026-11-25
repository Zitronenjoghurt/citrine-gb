use crate::app::Citrine;
use crate::app::focus::ScreenPlace;
use crate::app::hud::{Hud, HudSettings, INSET, SIZES};
use crate::app::layout_sheet::Level;
use crate::app::ui_state::UiState;
use crate::app::ui_state::ds_screens::Placement;
use crate::app::ui_state::layout_picks::{Builtin, Pick, Place};
use crate::app::ui_state::screen_layouts::{LayoutConsole, LayoutView, Spot, Spots};
use crate::app::widgets::confirm;
use crate::app::widgets::layout_editor::{EditItem, LayoutEditor, PadEdit, Target};
use crate::emulator::Emulator;
use crate::icons;
use egui::{Rect, Ui};

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutEdit {
    pub console: LayoutConsole,
    pub view: LayoutView,
    pub place: Place,
    pub editor: LayoutEditor,
    pub undo: Option<Undo>,
    pub resume: bool,
    pub start: EditStart,
    pub saving: Saving,
    before: Before,
    placement: Placement,
    spots: Spots,
    pub naming: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditStart {
    pub new: bool,
    pub level: Option<Level>,
}

impl EditStart {
    pub const CURRENT: Self = Self {
        new: false,
        level: None,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saving {
    Into(u32),
    Builtin(Builtin),
    Create(Level),
}

impl Saving {
    pub fn label(self) -> &'static str {
        match self {
            Self::Into(_) | Self::Builtin(_) => SAVE,
            Self::Create(_) => CREATE,
        }
    }
}

pub const SAVE: &str = "Save";
pub const CREATE: &str = "Create";
pub const CANCEL: &str = "Cancel";
pub const NAME_QUESTION: &str = "Name this layout";

#[derive(Debug, Clone, PartialEq)]
struct Before {
    spots: Spots,
    hud: HudSettings,
    focus_place: ScreenPlace,
    opacity: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Undo {
    spots: Spots,
    huds: Vec<Hud>,
    place: Option<ScreenPlace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetButton {
    Reset,
    Undo,
    Nothing,
}

impl ResetButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::Reset | Self::Nothing => "Reset",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Undo => icons::ARROW_U_UP_LEFT,
            Self::Reset | Self::Nothing => icons::ARROW_COUNTER_CLOCKWISE,
        }
    }
}

impl Citrine {
    #[cfg(all(test, feature = "nds"))]
    pub(crate) fn start_layout_edit(&mut self) {
        self.start_layout_edit_with(EditStart::CURRENT);
    }

    pub(crate) fn start_layout_edit_with(&mut self, start: EditStart) {
        if !self.emulator.has_game() {
            return;
        }
        let resume =
            self.play.session().is_none() && self.emulator.running && self.emulator.try_pause();
        self.open_edit(self.new_edit(start, resume, self.layout_view()));
        self.ui.layout_sheet = None;
        if self.plays_phone_default(self.layout_view()) {
            self.mobile.hidden.hide();
        } else {
            self.mobile.hidden.show();
        }
        self.release_touch();
    }

    fn new_edit(&self, start: EditStart, resume: bool, view: LayoutView) -> LayoutEdit {
        let console = LayoutConsole::of(self.emulator.played_kind());
        let place = Some(self.layout_place())
            .filter(|place| place.view() == view)
            .unwrap_or(Place::of_view(view));
        let settings = &self.ui.settings;
        let resolved = match start.level {
            Some(level) => settings.layout_pick_from(console, place, level.games, level.every_mode),
            None => settings.layout_pick(console, place),
        };
        let level = start.level.unwrap_or_else(|| Level::of(&resolved));
        let saving = match resolved.pick {
            _ if start.new => Saving::Create(level),
            Pick::Saved(id) if settings.saved_layout(id).is_some() => Saving::Into(id),
            pick => Builtin::of(pick).map_or(Saving::Create(level), Saving::Builtin),
        };
        let working = settings.layouts.spots(console, view);
        let (placement, spots) = if start.new {
            let shown = settings.layout_pick(console, place).pick;
            (settings.pick_placement(shown, place), working.clone())
        } else {
            (
                settings.pick_placement(resolved.pick, place),
                settings.pick_spots(console, place, resolved.pick),
            )
        };
        LayoutEdit {
            console,
            view,
            place,
            editor: LayoutEditor::default(),
            undo: None,
            resume,
            start,
            saving,
            before: Before {
                spots: working,
                hud: settings.hud.clone(),
                focus_place: settings.focus_place,
                opacity: settings.touch.opacity,
            },
            placement,
            spots,
            naming: None,
        }
    }

    fn open_edit(&mut self, edit: LayoutEdit) {
        let layouts = &mut self.ui.settings.layouts;
        if layouts.spots(edit.console, edit.view) != edit.spots {
            layouts.restore(edit.console, edit.view, edit.spots.clone());
            self.ui.settings.dirty = true;
        }
        if edit.console == LayoutConsole::Ds {
            *self.emulator.ds_screens.placement_mut(edit.place.context()) = edit.placement;
        }
        self.ui.layout_edit = Some(edit);
    }

    pub(crate) fn layout_view(&self) -> LayoutView {
        if self.big_screen_on() {
            LayoutView::BigScreen
        } else {
            LayoutView::of(self.emulator.screen_context)
        }
    }

    pub(crate) fn cancel_layout_edit(&mut self) {
        cancel(&mut self.ui, &mut self.emulator);
    }

    pub(crate) fn done_layout_edit(&mut self) {
        done(&mut self.ui, &mut self.emulator);
    }

    pub(crate) fn naming_layout(&self) -> bool {
        self.ui
            .layout_edit
            .as_ref()
            .is_some_and(|edit| edit.naming.is_some())
    }

    pub(crate) fn layout_edit_keys(&mut self, ctx: &egui::Context) -> bool {
        if self.editing_layout() && !self.emulator.has_game() {
            self.cancel_layout_edit();
        }
        let escape = self.editing_layout()
            && !self.naming_layout()
            && !self.is_mobile(ctx)
            && ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape {
            self.cancel_layout_edit();
        }
        escape
    }

    pub(crate) fn layout_dialogs(&mut self, ctx: &egui::Context) {
        self.ui.layout_sheet_dialogs(ctx);
        let Some(edit) = &mut self.ui.layout_edit else {
            return;
        };
        let Some(name) = &mut edit.naming else {
            return;
        };
        match confirm::name(ctx, "name_layout", NAME_QUESTION, name, CREATE) {
            Some(true) => {
                let name = name.clone();
                create(&mut self.ui, &mut self.emulator, &name);
            }
            Some(false) => {
                edit.naming = None;
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            }
            None => {}
        }
    }

    pub(crate) fn editing_layout(&self) -> bool {
        self.ui.layout_edit.is_some()
    }

    pub(crate) fn follow_layout_view(&mut self, view: LayoutView) {
        let console = LayoutConsole::of(self.emulator.played_kind());
        let Some(edit) = &self.ui.layout_edit else {
            return;
        };
        if edit.view == view && edit.console == console {
            return;
        }
        let (start, resume) = (edit.start, edit.resume);
        self.ui.restore_before_edit();
        self.open_edit(self.new_edit(start, resume, view));
    }
}

fn finish(ui: &mut UiState, emulator: &mut Emulator) {
    if let Some(edit) = ui.layout_edit.take()
        && edit.resume
        && emulator.has_game()
    {
        emulator.running = true;
    }
}

pub(crate) fn cancel(ui: &mut UiState, emulator: &mut Emulator) {
    ui.restore_before_edit();
    finish(ui, emulator);
}

pub(crate) fn done(ui: &mut UiState, emulator: &mut Emulator) {
    let Some(edit) = &mut ui.layout_edit else {
        return;
    };
    match edit.saving {
        Saving::Into(id) => {
            let settings = &mut ui.settings;
            let spots = settings.layouts.spots(edit.console, edit.view);
            settings.update_saved_layout(id, edit.placement, spots);
            ui.layout_applied = None;
            finish(ui, emulator);
        }
        Saving::Builtin(builtin) => {
            let settings = &mut ui.settings;
            let spots = settings.layouts.spots(edit.console, edit.view);
            settings.keep_builtin_edit(edit.console, edit.place, builtin, edit.placement, spots);
            ui.layout_applied = None;
            finish(ui, emulator);
        }
        Saving::Create(_) => {
            edit.naming = Some(ui.settings.next_layout_name());
        }
    }
}

fn create(ui: &mut UiState, emulator: &mut Emulator, name: &str) {
    let Some(edit) = &ui.layout_edit else {
        return;
    };
    let Saving::Create(level) = edit.saving else {
        return;
    };
    let (console, view, place, placement) = (edit.console, edit.view, edit.place, edit.placement);
    let settings = &mut ui.settings;
    let spots = settings.layouts.spots(console, view);
    let id = settings.save_layout(name.trim(), console, place, placement, spots);
    settings.set_layout_pick(
        console,
        place,
        Pick::Saved(id),
        level.games,
        level.every_mode,
    );
    ui.layout_applied = None;
    finish(ui, emulator);
}

impl UiState {
    fn restore_before_edit(&mut self) {
        let Some(edit) = &self.layout_edit else {
            return;
        };
        let before = edit.before.clone();
        self.settings
            .layouts
            .restore(edit.console, edit.view, before.spots);
        self.settings.hud = before.hud;
        self.settings.focus_place = before.focus_place;
        self.settings.touch.opacity = before.opacity;
        self.settings.dirty = true;
    }

    pub(crate) fn layout_done_label(&self) -> &'static str {
        self.layout_edit
            .as_ref()
            .map_or(SAVE, |edit| edit.saving.label())
    }

    pub(crate) fn hud_items(&self) -> Vec<EditItem> {
        self.hud
            .placed
            .iter()
            .filter_map(|&(id, rect)| {
                let hud = self.settings.hud.get(id)?;
                Some(EditItem::hud(id, rect, hud.scale()))
            })
            .collect()
    }

    pub(crate) fn edit_layout(
        &mut self,
        ui: &mut Ui,
        room: Rect,
        hud_area: Rect,
        items: &[EditItem],
        pad: PadEdit,
    ) {
        let Some(edit) = &mut self.layout_edit else {
            return;
        };
        let mut shown = items.to_vec();
        let placed = edit
            .editor
            .show(ui, room, items)
            .or_else(|| edit.editor.pad(room, items, pad));
        if let Some((target, rect)) = placed {
            match target {
                Target::Layout(item) if item.is_screen() => {
                    for shown in items {
                        if let Target::Layout(screen) = shown.target
                            && screen.is_screen()
                        {
                            let at = if screen == item { rect } else { shown.rect };
                            self.settings.layouts.set(
                                edit.console,
                                edit.view,
                                screen,
                                Spot::of(at, room),
                            );
                        }
                    }
                }
                Target::Layout(item) => {
                    self.settings
                        .layouts
                        .set(edit.console, edit.view, item, Spot::of(rect, room));
                }
                Target::Hud(id) => {
                    let from = items.iter().find(|i| i.target == target).map(|i| i.rect);
                    if let (Some(from), Some(hud)) = (from, self.settings.hud.get_mut(id)) {
                        let grown = hud.scale() * rect.width() / from.width().max(1.0);
                        hud.size = grown.clamp(*SIZES.start(), *SIZES.end());
                        hud.place(hud_area, rect);
                    }
                }
            }
            edit.undo = None;
            self.settings.dirty = true;
            if let Some(moved) = shown.iter_mut().find(|i| i.target == target) {
                moved.rect = rect;
            }
        }
        edit.editor.paint(ui, room, &shown);
    }

    fn moved_huds(&self) -> Vec<Hud> {
        self.hud
            .placed
            .iter()
            .filter_map(|&(id, _)| self.settings.hud.get(id))
            .filter(|hud| {
                hud.offset != [INSET, INSET]
                    || hud.size != 1.0
                    || hud.corner != self.settings.hud.home_corner(hud.id)
            })
            .cloned()
            .collect()
    }

    fn moved_place(&self, view: LayoutView) -> Option<ScreenPlace> {
        let place = self.settings.focus_place;
        (view == LayoutView::BigScreen && place != ScreenPlace::default()).then_some(place)
    }

    pub(crate) fn layout_reset_button(&self) -> ResetButton {
        match &self.layout_edit {
            Some(edit) if edit.undo.is_some() => ResetButton::Undo,
            Some(edit)
                if self.settings.layouts.edited(edit.console, edit.view)
                    || !self.moved_huds().is_empty()
                    || self.moved_place(edit.view).is_some() =>
            {
                ResetButton::Reset
            }
            _ => ResetButton::Nothing,
        }
    }

    pub(crate) fn press_layout_reset(&mut self) {
        let huds = self.moved_huds();
        let place = self
            .layout_edit
            .as_ref()
            .and_then(|edit| self.moved_place(edit.view));
        let Some(edit) = &mut self.layout_edit else {
            return;
        };
        let layouts = &mut self.settings.layouts;
        match edit.undo.take() {
            Some(undo) => {
                layouts.restore(edit.console, edit.view, undo.spots);
                for hud in undo.huds {
                    if let Some(kept) = self.settings.hud.get_mut(hud.id) {
                        *kept = hud;
                    }
                }
                if let Some(place) = undo.place {
                    self.settings.focus_place = place;
                }
            }
            None => {
                let spots = layouts.reset(edit.console, edit.view);
                for hud in &huds {
                    let corner = self.settings.hud.home_corner(hud.id);
                    if let Some(moved) = self.settings.hud.get_mut(hud.id) {
                        moved.offset = [INSET, INSET];
                        moved.size = 1.0;
                        moved.corner = corner;
                    }
                }
                if place.is_some() {
                    self.settings.focus_place = ScreenPlace::default();
                }
                let undone = !spots.is_empty() || !huds.is_empty() || place.is_some();
                edit.undo = undone.then_some(Undo { spots, huds, place });
                edit.editor.selected = None;
            }
        }
        self.settings.dirty = true;
    }
}
