use crate::eeprom::Eeprom;
use crate::rules::Source;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub shades: Vec<u8>,
}

impl Image {
    pub fn blank(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            shades: vec![0; width * height],
        }
    }

    pub fn byte_len(width: usize, height: usize) -> usize {
        width * height / 4
    }

    pub fn decode(bytes: &[u8], width: usize, height: usize) -> Self {
        let mut image = Self::blank(width, height);
        for stripe in 0..height / 8 {
            for x in 0..width {
                let at = (stripe * width + x) * 2;
                let (high, low) = match bytes.get(at..at + 2) {
                    Some(pair) => (pair[0], pair[1]),
                    None => (0, 0),
                };
                for bit in 0..8 {
                    let shade = ((high >> bit) & 1) << 1 | ((low >> bit) & 1);
                    image.shades[(stripe * 8 + bit) * width + x] = shade;
                }
            }
        }
        image
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::byte_len(self.width, self.height));
        for stripe in 0..self.height / 8 {
            for x in 0..self.width {
                let (mut high, mut low) = (0u8, 0u8);
                for bit in 0..8 {
                    let shade = self.shade(x, stripe * 8 + bit);
                    high |= ((shade >> 1) & 1) << bit;
                    low |= (shade & 1) << bit;
                }
                bytes.push(high);
                bytes.push(low);
            }
        }
        bytes
    }

    pub fn shade(&self, x: usize, y: usize) -> u8 {
        self.shades[y * self.width + x]
    }

    pub fn is_empty(&self) -> bool {
        self.shades.iter().all(|&shade| shade == self.shades[0])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Picture {
    Digits,
    Watts,
    Ball,
    Item,
    MenuArrowLeft,
    MenuArrowRight,
    MenuTitle(usize),
    MenuIcon(usize),
    Person,
    TrainerName,
    StepsFrame,
    DaysFrame,
    TotalDaysFrame,
    SoundFrame,
    ShadeFrame,
    Speaker(usize),
    PresentLarge,
    DowsingBushDark,
    DowsingBushLight,
    TextLeft,
    RadarBush,
    RadarBubble(usize),
    RadarClick,
    HpBlip,
    PokewalkerBig,
    IrArcs,
    Text(Text),
    RouteImage,
    RouteName,
    WalkingSmall(usize),
    WalkingName,
    RoutePokemonSmall(usize, usize),
    RoutePokemonName(usize),
    RouteItemName(usize),
    EventPokemonSmall(usize),
    EventPokemonName,
    EventItemName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Text {
    Connecting,
    NeedWatts,
    NoPokemonHeld,
    NothingHeld,
    Found,
    NothingFound,
    ItsNear,
    FarAway,
    FindPokemon,
    GotAway,
    Appeared,
    WasCaught,
    Fled,
    Attacked,
    Evaded,
    CriticalHit,
    ThrewBall,
    Lost,
    Joined,
    Switch,
}

pub(crate) struct Spec {
    pub address: usize,
    pub width: usize,
    pub height: usize,
    pub source: Source,
}

const fn spec(address: usize, width: usize, height: usize) -> Spec {
    Spec {
        address,
        width,
        height,
        source: Source::Pico,
    }
}

const DIGIT_BYTES: usize = 32;
const SPRITE_SMALL_BYTES: usize = 192;
const NAME_BYTES: usize = 320;
const ITEM_NAME_BYTES: usize = 384;
const TEXT_LINE_BYTES: usize = 384;

impl Picture {
    pub(crate) fn spec(self) -> Spec {
        match self {
            Picture::Digits => spec(0x0280, 8 * 13, 16),
            Picture::Watts => spec(0x0420, 16, 16),
            Picture::Ball => spec(0x0460, 8, 8),
            Picture::Item => spec(0x0488, 8, 8),
            Picture::MenuArrowLeft => spec(0x05B8, 8, 16),
            Picture::MenuArrowRight => spec(0x05D8, 8, 16),
            Picture::MenuTitle(app) => spec(0x0910 + app * NAME_BYTES, 80, 16),
            Picture::MenuIcon(app) => spec(0x1090 + app * 64, 16, 16),
            Picture::Person => spec(0x1210, 16, 16),
            Picture::TrainerName => spec(0x1250, 80, 16),
            Picture::StepsFrame => spec(0x13D0, 40, 16),
            Picture::DaysFrame => spec(0x14F0, 40, 16),
            Picture::TotalDaysFrame => spec(0x1590, 64, 16),
            Picture::SoundFrame => spec(0x1690, 40, 16),
            Picture::ShadeFrame => spec(0x1730, 40, 16),
            Picture::Speaker(level) => spec(0x17D0 + level * 96, 24, 16),
            Picture::PresentLarge => spec(0x1A90, 32, 24),
            Picture::DowsingBushDark => spec(0x1B50, 16, 16),
            Picture::DowsingBushLight => spec(0x1B90, 16, 16),
            Picture::TextLeft => spec(0x1BD0, 32, 16),
            Picture::RadarBush => spec(0x1CB0, 32, 24),
            Picture::RadarBubble(level) => spec(0x1D70 + level * 64, 16, 16),
            Picture::RadarClick => spec(0x1E30, 16, 16),
            Picture::HpBlip => spec(0x2030, 8, 8),
            Picture::PokewalkerBig => spec(0x2350, 32, 32),
            Picture::IrArcs => spec(0x2450, 8, 16),
            Picture::Text(text) => spec(text.address(), text.width(), 16),
            Picture::RouteImage => spec(0x8FBE, 32, 24),
            Picture::RouteName => spec(0x907E, 80, 16),
            Picture::WalkingSmall(frame) => spec(0x91BE + frame * SPRITE_SMALL_BYTES, 32, 24),
            Picture::WalkingName => spec(0x993E, 80, 16),
            Picture::RoutePokemonSmall(slot, frame) => {
                spec(0x9A7E + (slot * 2 + frame) * SPRITE_SMALL_BYTES, 32, 24)
            }
            Picture::RoutePokemonName(slot) => spec(0xA4FE + slot * NAME_BYTES, 80, 16),
            Picture::RouteItemName(slot) => spec(0xA8BE + slot * ITEM_NAME_BYTES, 96, 16),
            Picture::EventPokemonSmall(frame) => spec(0xBA80 + frame * SPRITE_SMALL_BYTES, 32, 24),
            Picture::EventPokemonName => spec(0xBC00, 80, 16),
            Picture::EventItemName => spec(0xBD48, 96, 16),
        }
    }

    pub fn source(self) -> Source {
        self.spec().source
    }

    pub(crate) fn read(self, eeprom: &Eeprom) -> Option<Image> {
        let spec = self.spec();
        let bytes = eeprom.read(spec.address, Image::byte_len(spec.width, spec.height));
        let image = Image::decode(&bytes, spec.width, spec.height);
        (!image.is_empty()).then_some(image)
    }

    pub(crate) fn digit(eeprom: &Eeprom, digit: usize) -> Option<Image> {
        let spec = Picture::Digits.spec();
        let bytes = eeprom.read(spec.address + digit * DIGIT_BYTES, DIGIT_BYTES);
        let image = Image::decode(&bytes, 8, 16);
        (!image.is_empty()).then_some(image)
    }
}

impl Text {
    fn width(self) -> usize {
        match self {
            Text::Switch => 80,
            _ => 96,
        }
    }

    fn address(self) -> usize {
        let line = |index: usize| 0x44B0 + index * TEXT_LINE_BYTES;
        match self {
            Text::Connecting => 0x2530,
            Text::NeedWatts => 0x4330,
            Text::NoPokemonHeld => line(0),
            Text::NothingHeld => line(1),
            Text::Found => 0x4930,
            Text::NothingFound => 0x4AB0,
            Text::ItsNear => 0x4C30,
            Text::FarAway => 0x4DB0,
            Text::FindPokemon => 0x4F30,
            Text::GotAway => 0x5230,
            Text::Appeared => 0x53B0,
            Text::WasCaught => 0x5530,
            Text::Fled => 0x56B0,
            Text::Attacked => 0x59B0,
            Text::Evaded => 0x5B30,
            Text::CriticalHit => 0x5CB0,
            Text::ThrewBall => 0x5FB0,
            Text::Lost => 0x6430,
            Text::Joined => 0x86B0,
            Text::Switch => 0x8B30,
        }
    }

    pub(crate) fn fallback(self) -> &'static str {
        match self {
            Text::Connecting => "CONNECTING...",
            Text::NeedWatts => "NEED MORE WATTS",
            Text::NoPokemonHeld => "NO POKEMON HELD",
            Text::NothingHeld => "NOTHING HELD",
            Text::Found => "FOUND!",
            Text::NothingFound => "NOTHING FOUND",
            Text::ItsNear => "IT'S NEAR!",
            Text::FarAway => "IT'S FAR AWAY",
            Text::FindPokemon => "FIND A POKEMON!",
            Text::GotAway => "IT GOT AWAY!",
            Text::Appeared => "IT APPEARED!",
            Text::WasCaught => "CAUGHT IT!",
            Text::Fled => "IT FLED!",
            Text::Attacked => "ATTACK!",
            Text::Evaded => "EVADED!",
            Text::CriticalHit => "CRITICAL HIT!",
            Text::ThrewBall => "THREW A BALL",
            Text::Lost => "LOST...",
            Text::Joined => "A POKEMON JOINED!",
            Text::Switch => "SWITCH?",
        }
    }
}

pub(crate) const MENU_TITLES: [&str; 6] = [
    "POKE RADAR",
    "DOWSING",
    "CONNECT",
    "TRAINER CARD",
    "INVENTORY",
    "SETTINGS",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(width: usize, height: usize) -> Image {
        let mut image = Image::blank(width, height);
        for y in 0..height {
            for x in 0..width {
                image.shades[y * width + x] = ((x + y * 3) % 4) as u8;
            }
        }
        image
    }

    #[test]
    fn images_round_trip_through_the_eeprom_format() {
        let image = checker(32, 24);
        let bytes = image.encode();
        assert_eq!(bytes.len(), Image::byte_len(32, 24));
        assert_eq!(Image::decode(&bytes, 32, 24), image);
    }

    #[test]
    fn the_first_byte_of_a_column_is_the_high_plane_and_bit_zero_the_top_row() {
        let image = Image::decode(&[0b0000_0001, 0b0000_0011], 1, 8);
        assert_eq!(image.shade(0, 0), 3);
        assert_eq!(image.shade(0, 1), 1);
        assert_eq!(image.shade(0, 2), 0);
    }

    #[test]
    fn a_blank_region_reads_as_missing() {
        let mut eeprom = Eeprom::blank();
        assert_eq!(Picture::RouteImage.read(&eeprom), None);
        let image = checker(32, 24);
        eeprom.write(Picture::RouteImage.spec().address, &image.encode());
        assert_eq!(Picture::RouteImage.read(&eeprom), Some(image));
    }

    #[test]
    fn the_image_blocks_follow_each_other_without_overlap() {
        let pictures = [
            Picture::RouteImage,
            Picture::RouteName,
            Picture::WalkingSmall(0),
            Picture::WalkingSmall(1),
        ];
        for pair in pictures.windows(2) {
            let (a, b) = (pair[0].spec(), pair[1].spec());
            assert_eq!(a.address + Image::byte_len(a.width, a.height), b.address);
        }
        let last_item = Picture::RouteItemName(9).spec();
        assert_eq!(last_item.address + ITEM_NAME_BYTES, 0xA8BE + 3840);
        let third = Picture::RoutePokemonSmall(2, 1).spec();
        assert_eq!(third.address + SPRITE_SMALL_BYTES, 0x9A7E + 1152);
    }

    #[test]
    fn every_picture_comes_from_the_documented_map() {
        assert_eq!(Picture::RouteImage.source(), Source::Pico);
        assert_eq!(Picture::Text(Text::Switch).source(), Source::Pico);
    }
}
