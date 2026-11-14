mod art;
mod font;

pub use art::{Image, Picture, Text};

use crate::Walker;
use crate::battle::{Action, BattleState};
use crate::dowsing::{DowsingState, Hint, Prize};
use crate::info::PokemonSummary;
use crate::radar::RadarState;
use crate::route::{self, Found};
use crate::screen::{App, HomeEvent, Keep, Message, Screen, Setting, Shelf};
use art::MENU_TITLES;

pub const LCD_WIDTH: usize = 96;
pub const LCD_HEIGHT: usize = 64;
const INK: u8 = 3;
const MID: u8 = 2;
const PALE: u8 = 1;
const MAX_SHELF: usize = 3;
const NICKNAME_BYTES: usize = 0x16;
const ROUTE_NAME_BYTES: usize = 0x2A;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lcd {
    shades: Vec<u8>,
}

impl Default for Lcd {
    fn default() -> Self {
        Self {
            shades: vec![0; LCD_WIDTH * LCD_HEIGHT],
        }
    }
}

impl Lcd {
    pub fn shade(&self, x: usize, y: usize) -> u8 {
        self.shades[y * LCD_WIDTH + x]
    }

    pub fn shades(&self) -> &[u8] {
        &self.shades
    }

    pub fn is_blank(&self) -> bool {
        self.shades.iter().all(|&shade| shade == 0)
    }

    fn put(&mut self, x: usize, y: usize, shade: u8) {
        if x < LCD_WIDTH && y < LCD_HEIGHT {
            self.shades[y * LCD_WIDTH + x] = shade;
        }
    }

    fn fill(&mut self, x: usize, y: usize, width: usize, height: usize, shade: u8) {
        for row in y..y + height {
            for col in x..x + width {
                self.put(col, row, shade);
            }
        }
    }

    fn line(&mut self, y: usize, shade: u8) {
        self.fill(0, y, LCD_WIDTH, 1, shade);
    }

    fn blit(&mut self, image: &Image, x: usize, y: usize) {
        for row in 0..image.height {
            for col in 0..image.width {
                let shade = image.shade(col, row);
                if shade != 0 {
                    self.put(x + col, y + row, shade);
                }
            }
        }
    }

    fn text(&mut self, text: &str, x: usize, y: usize) {
        font::draw(text, x, y, |px, py| self.put(px, py, INK));
    }

    fn text_centred(&mut self, text: &str, x: usize, width: usize, y: usize) {
        let room = width.saturating_sub(font::width(text)) / 2;
        self.text(text, x + room, y);
    }

    fn ball(&mut self, x: usize, y: usize, shade: u8) {
        for (row, bits) in [0x3C, 0x7E, 0xFF, 0x81, 0xFF, 0x7E, 0x3C, 0]
            .iter()
            .enumerate()
        {
            for col in 0..8 {
                if bits & (0x80 >> col) != 0 {
                    self.put(x + col, y + row, shade);
                }
            }
        }
    }

    fn bush(&mut self, x: usize, y: usize, width: usize, height: usize, shade: u8) {
        let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
        for row in 0..height {
            for col in 0..width {
                let dx = (col as f32 + 0.5 - cx) / cx;
                let dy = (row as f32 + 0.5 - cy) / cy;
                let wave = 0.12 * ((col as f32) * 1.3).sin();
                if dx * dx + dy * dy <= 1.0 - wave {
                    let edge = dx * dx + dy * dy > 0.6;
                    self.put(x + col, y + row, if edge { INK } else { shade });
                }
            }
        }
    }

    fn arrow_down(&mut self, x: usize, y: usize) {
        for row in 0..4 {
            for col in row..7 - row {
                self.put(x + col, y + row, INK);
            }
        }
    }

    fn arrow_side(&mut self, x: usize, y: usize, left: bool) {
        for col in 0..4 {
            for row in col..7 - col {
                let px = if left { x + col } else { x + 3 - col };
                self.put(px, y + row, INK);
            }
        }
    }
}

impl Walker {
    pub fn picture(&self, picture: Picture) -> Option<Image> {
        picture.read(&self.eeprom)
    }

    pub fn shelf_name(&self, shelf: Shelf) -> Option<Image> {
        let eeprom = &self.eeprom;
        match shelf {
            Shelf::Walking => self.picture(Picture::WalkingName),
            Shelf::Caught(slot) => {
                let species = route::caught_slot(eeprom, slot as usize).species;
                self.pokemon_name(species)
            }
            Shelf::EventPokemon => self.picture(Picture::EventPokemonName),
            Shelf::Item(slot) => self.item_name(route::item_slot(eeprom, slot as usize)),
            Shelf::EventItem => self.picture(Picture::EventItemName),
        }
    }

    pub fn walking_nickname(&self) -> Vec<u16> {
        units(&self.eeprom.read(route::NICKNAME, NICKNAME_BYTES))
    }

    pub fn route_name(&self) -> Vec<u16> {
        units(&self.eeprom.read(route::ROUTE_NAME, ROUTE_NAME_BYTES))
    }

    pub fn shelf_level(&self, shelf: Shelf) -> Option<u8> {
        match shelf {
            Shelf::Caught(slot) => Some(route::caught_slot(&self.eeprom, slot as usize).level),
            Shelf::Walking | Shelf::EventPokemon | Shelf::Item(_) | Shelf::EventItem => None,
        }
    }

    pub fn lcd(&self, phase: u32) -> Lcd {
        let mut lcd = Lcd::default();
        if self.asleep() {
            return lcd;
        }
        let frame = (phase / 2 % 2) as usize;
        match self.screen() {
            Screen::Home => self.draw_home(&mut lcd, frame),
            Screen::Menu { app, message } => self.draw_menu(&mut lcd, app, message),
            Screen::Radar(radar) => {
                for bush in 0..4u8 {
                    let (x, y) = radar_bush(bush);
                    self.paint(&mut lcd, Picture::RadarBush, x, y);
                    if radar.cursor() == bush && radar.state() == RadarState::Looking {
                        lcd.arrow_down(x + 13, y.saturating_sub(5));
                    }
                }
                if let Some(bush) = radar.shown() {
                    let (x, y) = radar_bush(bush);
                    let level = (radar.level() as usize).min(2);
                    let picture = match radar.state() {
                        RadarState::Hit => Picture::RadarClick,
                        _ => Picture::RadarBubble(level),
                    };
                    lcd.fill(x + 20, y, 16, 16, 0);
                    self.paint(&mut lcd, picture, x + 20, y);
                }
                let level = format!("{}/{}", radar.level(), radar.target());
                lcd.text_centred(&level, 32, 32, 29);
                let text = match radar.state() {
                    RadarState::Starting => Some(Text::FindPokemon),
                    RadarState::Failed => Some(Text::GotAway),
                    RadarState::Looking | RadarState::Hit => None,
                };
                if let Some(text) = text {
                    lcd.fill(0, 48, LCD_WIDTH, 16, 0);
                    self.paint(&mut lcd, Picture::Text(text), 0, 48);
                }
            }
            Screen::Battle(battle) => {
                self.draw_found(&mut lcd, battle.found(), frame, 60, 2);
                self.draw_walking(&mut lcd, frame, 4, 22);
                hp(&mut lcd, battle.their_hp(), 4, 4);
                hp(&mut lcd, battle.our_hp(), 60, 38);
                let text = match battle.state() {
                    BattleState::Appearing => Some(Text::Appeared),
                    BattleState::Choosing => None,
                    BattleState::Round { ours, theirs } => Some(match (ours, theirs) {
                        (_, Action::Special) => Text::CriticalHit,
                        (Action::Evade, _) => Text::Evaded,
                        _ => Text::Attacked,
                    }),
                    BattleState::Throw { .. } => Some(Text::ThrewBall),
                    BattleState::Fled => Some(Text::Fled),
                    BattleState::Lost => Some(Text::Lost),
                    BattleState::Caught => Some(Text::WasCaught),
                };
                match text {
                    Some(text) => self.paint(&mut lcd, Picture::Text(text), 0, 48),
                    None => {
                        lcd.text("ATTACK", 2, 54);
                        lcd.text_centred("CATCH", 32, 32, 54);
                        lcd.text("EVADE", 74, 54);
                    }
                }
            }
            Screen::Dowsing(dowsing) => {
                self.paint(&mut lcd, Picture::TextLeft, 0, 0);
                self.number_small(&mut lcd, u32::from(dowsing.tries_left()), 40, 5);
                let shaking = dowsing.state() == DowsingState::Shaking;
                for bush in 0..6u8 {
                    let x = usize::from(bush) * 16;
                    let picture = if dowsing.tried(bush) {
                        Picture::DowsingBushLight
                    } else {
                        Picture::DowsingBushDark
                    };
                    let shake = usize::from(shaking && dowsing.cursor() == bush && frame == 1);
                    self.paint(&mut lcd, picture, x + shake, 18);
                }
                if matches!(dowsing.state(), DowsingState::Choosing) {
                    lcd.arrow_down(usize::from(dowsing.cursor()) * 16 + 4, 36);
                }
                match dowsing.state() {
                    DowsingState::Choosing | DowsingState::Shaking => {}
                    DowsingState::Hint(Hint::Near) => {
                        self.paint(&mut lcd, Picture::Text(Text::ItsNear), 0, 48)
                    }
                    DowsingState::Hint(Hint::Far) => {
                        self.paint(&mut lcd, Picture::Text(Text::FarAway), 0, 48)
                    }
                    DowsingState::Found(prize) => {
                        self.paint(&mut lcd, Picture::Text(Text::Found), 0, 32);
                        self.draw_prize(&mut lcd, prize, 48);
                    }
                    DowsingState::Missed => {
                        self.paint(&mut lcd, Picture::Text(Text::NothingFound), 0, 48)
                    }
                }
            }
            Screen::Switch { new, cursor } => {
                self.paint(&mut lcd, Picture::Text(Text::Switch), 8, 0);
                let shelves = match new {
                    Keep::Pokemon(_) => self.caught_shelves(),
                    Keep::Item(_) => self.item_shelves(),
                };
                for (row, shelf) in shelves.iter().take(MAX_SHELF).enumerate() {
                    let y = 18 + row * 15;
                    if let Some(name) = self.shelf_name(*shelf) {
                        lcd.blit(&name, 10, y);
                    }
                    if usize::from(cursor) == row {
                        lcd.arrow_side(2, y + 4, false);
                    }
                }
            }
            Screen::Collected { event, page: _ } => {
                self.paint(&mut lcd, Picture::PresentLarge, 32, 4);
                match event {
                    HomeEvent::Nothing => {
                        self.paint(&mut lcd, Picture::Text(Text::NothingFound), 0, 40)
                    }
                    HomeEvent::Watts(watts) => self.draw_watts(&mut lcd, watts, 40),
                    HomeEvent::Item(item) => {
                        if let Some(name) = self.item_name(item) {
                            lcd.blit(&name, 0, 40);
                        }
                    }
                    HomeEvent::Joined => self.paint(&mut lcd, Picture::Text(Text::Joined), 0, 40),
                }
            }
            Screen::Connect { started } => {
                self.paint(&mut lcd, Picture::PokewalkerBig, 32, 4);
                if started {
                    for arc in 0..=frame {
                        self.paint(&mut lcd, Picture::IrArcs, 66 + arc * 8, 12);
                    }
                    self.paint(&mut lcd, Picture::Text(Text::Connecting), 0, 44);
                }
            }
            Screen::Card { page } => self.draw_card(&mut lcd, page),
            Screen::Inventory { shelf } => {
                match shelf {
                    Shelf::Walking => self.draw_walking(&mut lcd, frame, 32, 4),
                    Shelf::Caught(slot) => {
                        let species = route::caught_slot(&self.eeprom, slot as usize).species;
                        if let Some(sprite) = self.pokemon_sprite(species, frame) {
                            lcd.blit(&sprite, 32, 4);
                        }
                    }
                    Shelf::EventPokemon => {
                        self.paint(&mut lcd, Picture::EventPokemonSmall(frame), 32, 4)
                    }
                    Shelf::Item(_) | Shelf::EventItem => {
                        self.paint(&mut lcd, Picture::PresentLarge, 32, 4)
                    }
                }
                if let Some(name) = self.shelf_name(shelf) {
                    lcd.blit(&name, (LCD_WIDTH - name.width) / 2, 32);
                }
                if let Some(level) = self.shelf_level(shelf) {
                    lcd.text_centred(&format!("LV {level}"), 0, LCD_WIDTH, 52);
                }
            }
            Screen::Settings { setting, editing } => {
                let health = self.health();
                match setting {
                    Setting::Sound => {
                        self.paint(&mut lcd, Picture::SoundFrame, 28, 4);
                        let level = editing.unwrap_or(health.sound()).min(2) as usize;
                        self.paint(&mut lcd, Picture::Speaker(level), 36, 28);
                    }
                    Setting::Shade => {
                        self.paint(&mut lcd, Picture::ShadeFrame, 28, 4);
                        let shade = editing.unwrap_or(health.shade());
                        lcd.text_centred(&shade.to_string(), 0, LCD_WIDTH, 34);
                    }
                }
                if editing.is_some() {
                    lcd.arrow_side(20, 32, true);
                    lcd.arrow_side(72, 32, false);
                }
            }
        }
        lcd
    }

    fn paint(&self, lcd: &mut Lcd, picture: Picture, x: usize, y: usize) {
        if let Some(image) = self.picture(picture) {
            lcd.blit(&image, x, y);
            return;
        }
        let spec = picture.spec();
        match picture {
            Picture::Text(text) => lcd.text_centred(text.fallback(), x, spec.width, y + 5),
            Picture::MenuTitle(app) => lcd.text_centred(MENU_TITLES[app], x, spec.width, y + 5),
            Picture::TextLeft => lcd.text("LEFT", x, y + 5),
            Picture::MenuArrowLeft => lcd.arrow_side(x + 2, y + 4, true),
            Picture::MenuArrowRight => lcd.arrow_side(x + 2, y + 4, false),
            Picture::Ball => lcd.ball(x, y, MID),
            Picture::Item => {
                lcd.fill(x + 1, y + 2, 6, 5, MID);
                lcd.fill(x + 3, y + 1, 2, 1, INK);
            }
            Picture::RadarBush => lcd.bush(x, y, spec.width, spec.height, MID),
            Picture::DowsingBushDark => lcd.bush(x + 1, y + 1, 14, 14, MID),
            Picture::DowsingBushLight => lcd.bush(x + 1, y + 1, 14, 14, PALE),
            Picture::RadarBubble(_) | Picture::RadarClick => {
                lcd.fill(x + 6, y + 2, 3, 8, INK);
                lcd.fill(x + 6, y + 12, 3, 3, INK);
            }
            Picture::HpBlip => lcd.fill(x + 1, y + 2, 6, 4, INK),
            Picture::PresentLarge => {
                lcd.fill(x + 6, y + 8, 20, 14, MID);
                lcd.fill(x + 4, y + 5, 24, 4, INK);
                lcd.fill(x + 15, y + 5, 2, 17, PALE);
            }
            Picture::PokewalkerBig => {
                lcd.bush(x, y, spec.width, spec.height, PALE);
                lcd.fill(x + 9, y + 9, 14, 10, 0);
                lcd.fill(x + 9, y + 9, 14, 1, INK);
            }
            Picture::IrArcs => lcd.fill(x + 2, y + 4, 2, 8, INK),
            Picture::SoundFrame => lcd.text_centred("SOUND", x, spec.width, y + 5),
            Picture::ShadeFrame => lcd.text_centred("SHADE", x, spec.width, y + 5),
            Picture::Speaker(level) => {
                lcd.fill(x + 4, y + 5, 4, 6, INK);
                lcd.fill(x + 8, y + 3, 2, 10, INK);
                for bar in 0..level {
                    lcd.fill(x + 13 + bar * 4, y + 6 - bar * 2, 2, 4 + bar * 4, INK);
                }
            }
            _ => {}
        }
    }

    fn draw_home(&self, lcd: &mut Lcd, frame: usize) {
        let mut x = 2;
        for _ in self.caught_shelves() {
            self.paint(lcd, Picture::Ball, x, 2);
            x += 9;
        }
        for _ in self.item_shelves() {
            self.paint(lcd, Picture::Item, x, 2);
            x += 9;
        }
        let watts = format!("{}W", self.health().watts());
        lcd.text(&watts, LCD_WIDTH - 2 - font::width(&watts), 3);
        self.paint(lcd, Picture::RouteImage, 6, 20);
        self.draw_walking(lcd, frame, 58, 20);
        lcd.line(45, MID);
        lcd.line(46, INK);
        self.number(lcd, self.today_steps(), LCD_WIDTH - 2, 48);
    }

    fn draw_menu(&self, lcd: &mut Lcd, app: App, message: Option<Message>) {
        self.paint(lcd, Picture::MenuArrowLeft, 0, 2);
        self.paint(lcd, Picture::MenuTitle(app.index()), 8, 2);
        self.paint(lcd, Picture::MenuArrowRight, 88, 2);
        for other in App::ALL {
            let x = other.index() * 16;
            if other == app {
                lcd.fill(x, 22, 16, 18, PALE);
            }
            if self.picture(Picture::MenuIcon(other.index())).is_some() {
                self.paint(lcd, Picture::MenuIcon(other.index()), x, 23);
            } else {
                let initial = &MENU_TITLES[other.index()][..1];
                lcd.text_centred(initial, x, 16, 29);
            }
        }
        match message {
            Some(message) => {
                let text = match message {
                    Message::NothingHeld => Text::NothingHeld,
                    Message::NoPokemon => Text::NoPokemonHeld,
                    Message::NeedWatts => Text::NeedWatts,
                };
                self.paint(lcd, Picture::Text(text), 0, 48);
            }
            None => self.draw_watts(lcd, self.health().watts(), 48),
        }
    }

    fn draw_card(&self, lcd: &mut Lcd, page: u8) {
        let health = self.health();
        match page {
            0 => {
                self.paint(lcd, Picture::Person, 4, 4);
                if self.picture(Picture::TrainerName).is_some() {
                    self.paint(lcd, Picture::TrainerName, 16, 4);
                }
                self.paint(lcd, Picture::TotalDaysFrame, 0, 26);
                self.number(lcd, u32::from(health.total_days()), LCD_WIDTH - 2, 26);
            }
            1 => {
                self.paint(lcd, Picture::StepsFrame, 0, 4);
                lcd.text("TOTAL", 4, 9);
                self.number(lcd, health.total_steps(), LCD_WIDTH - 2, 26);
            }
            _ => {
                let day = usize::from(page - 2);
                let steps = route::history(&self.eeprom, self.rules.history_days)
                    .get(day)
                    .copied()
                    .unwrap_or(0);
                self.paint(lcd, Picture::DaysFrame, 0, 4);
                lcd.text(&format!("-{}", day + 1), 46, 9);
                self.number(lcd, steps, LCD_WIDTH - 2, 26);
            }
        }
    }

    fn draw_watts(&self, lcd: &mut Lcd, watts: u16, y: usize) {
        let right = LCD_WIDTH - 2;
        self.number(lcd, u32::from(watts), right - 18, y);
        if self.picture(Picture::Watts).is_some() {
            self.paint(lcd, Picture::Watts, right - 16, y);
        } else {
            lcd.text("W", right - 12, y + 6);
        }
    }

    fn draw_prize(&self, lcd: &mut Lcd, prize: Prize, y: usize) {
        match prize {
            Prize::Item(item) => {
                if let Some(name) = self.item_name(item) {
                    lcd.blit(&name, 0, y);
                }
            }
            Prize::EventItem(_) => self.paint(lcd, Picture::EventItemName, 0, y),
            Prize::Watts(watts) => self.draw_watts(lcd, watts, y),
        }
    }

    fn draw_walking(&self, lcd: &mut Lcd, frame: usize, x: usize, y: usize) {
        if self.walking_now() {
            self.paint(lcd, Picture::WalkingSmall(frame), x, y);
        }
    }

    fn draw_found(&self, lcd: &mut Lcd, found: Found, frame: usize, x: usize, y: usize) {
        let picture = match found {
            Found::Route(slot) => Picture::RoutePokemonSmall(usize::from(slot).min(2), frame),
            Found::GroupC => Picture::RoutePokemonSmall(2, frame),
            Found::Event => Picture::EventPokemonSmall(frame),
        };
        self.paint(lcd, picture, x, y);
    }

    fn number(&self, lcd: &mut Lcd, value: u32, right: usize, y: usize) {
        let text = value.to_string();
        let real = Picture::digit(&self.eeprom, 0).is_some();
        let mut x = right;
        for c in text.bytes().rev() {
            let digit = usize::from(c - b'0');
            x = x.saturating_sub(8);
            match Picture::digit(&self.eeprom, digit).filter(|_| real) {
                Some(image) => lcd.blit(&image, x, y),
                None => {
                    let mut big = Lcd::default();
                    big.text(&(c as char).to_string(), 0, 0);
                    for row in 0..font::HEIGHT {
                        for col in 0..3 {
                            if big.shade(col, row) != 0 {
                                lcd.fill(x + 1 + col * 2, y + 3 + row * 2, 2, 2, INK);
                            }
                        }
                    }
                }
            }
        }
    }

    fn number_small(&self, lcd: &mut Lcd, value: u32, x: usize, y: usize) {
        lcd.text(&value.to_string(), x, y);
    }

    fn walking_now(&self) -> bool {
        route::has_walking_pokemon(&self.eeprom)
    }

    fn caught_shelves(&self) -> Vec<Shelf> {
        self.shelves()
            .into_iter()
            .filter(|shelf| matches!(shelf, Shelf::Caught(_) | Shelf::EventPokemon))
            .collect()
    }

    fn item_shelves(&self) -> Vec<Shelf> {
        self.shelves()
            .into_iter()
            .filter(|shelf| matches!(shelf, Shelf::Item(_) | Shelf::EventItem))
            .collect()
    }

    fn pokemon_slot(&self, species: u16) -> Option<usize> {
        (0..3).find(|&slot| {
            let PokemonSummary { species: other, .. } = route::route_pokemon(&self.eeprom, slot);
            other == species && species != 0
        })
    }

    fn pokemon_name(&self, species: u16) -> Option<Image> {
        let slot = self.pokemon_slot(species)?;
        self.picture(Picture::RoutePokemonName(slot))
    }

    fn pokemon_sprite(&self, species: u16, frame: usize) -> Option<Image> {
        let slot = self.pokemon_slot(species)?;
        self.picture(Picture::RoutePokemonSmall(slot, frame))
    }

    fn item_name(&self, item: u16) -> Option<Image> {
        let slot = (0..route::ROUTE_ITEM_COUNT)
            .find(|&slot| item != 0 && route::route_item(&self.eeprom, slot) == item)?;
        self.picture(Picture::RouteItemName(slot))
    }
}

fn units(bytes: &[u8]) -> Vec<u16> {
    (0..bytes.len() / 2)
        .map(|at| u16::from_le_bytes([bytes[at * 2], bytes[at * 2 + 1]]))
        .take_while(|&unit| unit != 0xFFFF && unit != 0)
        .collect()
}

fn radar_bush(bush: u8) -> (usize, usize) {
    let x = if bush.is_multiple_of(2) { 4 } else { 60 };
    let y = if bush < 2 { 6 } else { 24 };
    (x, y)
}

fn hp(lcd: &mut Lcd, hp: u8, x: usize, y: usize) {
    for blip in 0..4u8 {
        let shade = if blip < hp { INK } else { PALE };
        lcd.fill(x + usize::from(blip) * 7, y, 5, 4, shade);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eeprom::{self, Block, Eeprom};
    use crate::info::FLAG_INIT;
    use crate::screen::Button;

    fn walker(watts: u16) -> Walker {
        let mut eeprom = Eeprom::blank();
        eeprom.write(eeprom::ROUTE, &[25, 0]);
        for slot in 0..3 {
            eeprom.write(0x8F52 + 16 * slot, &[0x10 + slot as u8, 0]);
            eeprom.write(0x8F88 + slot, &[100]);
        }
        let mut info = vec![0; 0x68];
        info[0x5B] = FLAG_INIT | 0x02;
        eeprom.write_reliable(Block::Info, &info);
        let mut walker = Walker::from_eeprom(eeprom, 7);
        walker.ram.walking = true;
        walker.ram.awake_seconds = 30;
        walker.add_watts(watts);
        walker
    }

    fn sprite(seed: usize) -> Image {
        let mut image = Image::blank(32, 24);
        for (at, shade) in image.shades.iter_mut().enumerate() {
            *shade = ((at + seed) % 3 + 1) as u8;
        }
        image
    }

    fn ink(lcd: &Lcd, x: usize, y: usize, width: usize, height: usize) -> usize {
        (y..y + height)
            .flat_map(|row| (x..x + width).map(move |col| (col, row)))
            .filter(|&(col, row)| lcd.shade(col, row) != 0)
            .count()
    }

    #[test]
    fn the_radar_bubble_hides_the_grass_behind_it() {
        let mut walker = walker(10);
        let mut grass = Image::blank(32, 24);
        grass.shades.fill(INK);
        let mut bubble = Image::blank(16, 16);
        bubble.shades[0] = INK;
        let pictures = [
            Picture::RadarBubble(0),
            Picture::RadarBubble(1),
            Picture::RadarBubble(2),
            Picture::RadarClick,
        ];
        for picture in pictures {
            walker
                .eeprom
                .write(picture.spec().address, &bubble.encode());
        }
        walker
            .eeprom
            .write(Picture::RadarBush.spec().address, &grass.encode());
        walker.press(Button::Right);
        walker.press(Button::Middle);
        let mut now = 0;
        let bush = loop {
            let Screen::Radar(radar) = walker.screen() else {
                panic!("not on the radar: {:?}", walker.screen());
            };
            if let Some(bush) = radar.shown() {
                break bush;
            }
            assert!(now < 600_000, "no bubble in ten minutes");
            now += 100;
            walker.tick(now);
        };
        let (x, y) = radar_bush(bush);
        let lcd = walker.lcd(0);
        assert_eq!(ink(&lcd, x + 20, y, 16, 16), 1);
        assert_eq!(ink(&lcd, x + 16, y, 4, 16), 64);
    }

    #[test]
    fn an_asleep_walker_shows_nothing() {
        let mut walker = walker(0);
        walker.ram.awake_seconds = 0;
        assert!(walker.lcd(0).is_blank());
    }

    #[test]
    fn home_draws_the_uploaded_route_and_pokemon_where_the_game_put_them() {
        let mut walker = walker(0);
        let route = sprite(0);
        let first = sprite(1);
        let second = sprite(2);
        walker
            .eeprom
            .write(Picture::RouteImage.spec().address, &route.encode());
        walker
            .eeprom
            .write(Picture::WalkingSmall(0).spec().address, &first.encode());
        walker
            .eeprom
            .write(Picture::WalkingSmall(1).spec().address, &second.encode());
        let lcd = walker.lcd(0);
        assert_eq!(lcd.shade(6, 20), route.shade(0, 0));
        assert_eq!(lcd.shade(58 + 5, 20 + 7), first.shade(5, 7));
        let later = walker.lcd(2);
        assert_eq!(later.shade(58 + 5, 20 + 7), second.shade(5, 7));
    }

    #[test]
    fn home_shows_todays_steps_at_the_bottom_right() {
        let mut walker = walker(0);
        walker.steps(12_345);
        let lcd = walker.lcd(0);
        assert!(ink(&lcd, 54, 48, 40, 16) > 0);
        assert_eq!(ink(&lcd, 0, 48, 50, 16), 0);
    }

    #[test]
    fn a_menu_message_without_its_image_falls_back_to_our_own_letters() {
        let mut walker = walker(0);
        walker.press(Button::Right);
        walker.press(Button::Middle);
        assert_eq!(
            walker.screen(),
            Screen::Menu {
                app: App::Radar,
                message: Some(Message::NeedWatts)
            }
        );
        let lcd = walker.lcd(0);
        assert!(ink(&lcd, 0, 48, LCD_WIDTH, 16) > 0);
    }

    #[test]
    fn an_uploaded_text_image_wins_over_the_fallback_letters() {
        let mut walker = walker(0);
        let mut line = Image::blank(96, 16);
        line.shades[0] = 3;
        line.shades[1] = 1;
        let address = Picture::Text(Text::NeedWatts).spec().address;
        walker.eeprom.write(address, &line.encode());
        walker.press(Button::Right);
        walker.press(Button::Middle);
        let lcd = walker.lcd(0);
        assert_eq!(ink(&lcd, 0, 48, LCD_WIDTH, 16), 2);
    }

    #[test]
    fn caught_pokemon_take_the_name_image_of_their_route_slot() {
        let mut walker = walker(0);
        let mut name = Image::blank(80, 16);
        name.shades[3] = 2;
        walker
            .eeprom
            .write(Picture::RoutePokemonName(1).spec().address, &name.encode());
        let pokemon = route::route_pokemon(&walker.eeprom, 1);
        route::put_caught(&mut walker.eeprom, 0, pokemon);
        assert_eq!(walker.shelf_name(Shelf::Caught(0)), Some(name));
        assert_eq!(walker.shelf_name(Shelf::Caught(1)), None);
    }

    #[test]
    fn names_stop_at_the_end_mark() {
        let mut walker = walker(0);
        walker.eeprom.write(
            route::NICKNAME,
            &[0x2B, 0x01, 0x2C, 0x01, 0xFF, 0xFF, 0x2D, 0x01],
        );
        assert_eq!(walker.walking_nickname(), vec![0x012B, 0x012C]);
        assert!(walker.route_name().is_empty());
    }

    #[test]
    fn every_screen_draws_inside_the_lcd() {
        let mut walker = walker(500);
        for button in [
            Button::Right,
            Button::Middle,
            Button::Middle,
            Button::Left,
            Button::Right,
            Button::Middle,
        ] {
            walker.press(button);
            for phase in 0..4 {
                let lcd = walker.lcd(phase);
                assert_eq!(lcd.shades().len(), LCD_WIDTH * LCD_HEIGHT);
            }
        }
    }
}
