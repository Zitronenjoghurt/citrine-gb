use crate::nds::wifi::frame::{Frame, HEADER_BYTES, Kind, half};
use crate::rom::nds::ICON_BYTES;

pub const NINTENDO_OUI: [u8; 4] = [0x00, 0x09, 0xBF, 0x00];
pub const MULTIBOOT: u8 = 0x0B;
pub const FRAGMENTS: usize = 9;
pub const FRAGMENT_BYTES: usize = 0x62;
pub const ADVERT_BYTES: usize = 0x358;

const BEACON: u8 = 8;
const FIXED_FIELDS: usize = 12;
const BEACON_PERIOD: usize = 8;
const VENDOR_TAG: u8 = 0xDD;
const CHANNEL_TAG: u8 = 0x03;
const GAME_ID: usize = 0x0C;
const STREAM: usize = 0x10;
const BEACON_TYPE: usize = 0x13;
const CMD_BYTES: usize = 0x14;
const SESSION: usize = 0x1D;
const FRAGMENT: usize = 0x1F;
const CHECKSUM: usize = 0x20;
const SUMMED: usize = 0x22;
const LAST_FRAGMENT: usize = 0x23;
const DATA_SIZE: usize = 0x24;
const DATA: usize = 0x26;
const PAYLOAD_END: usize = DATA + FRAGMENT_BYTES;
const PALETTE_BYTES: usize = 0x20;
const TILES_BYTES: usize = 0x200;
const HOST_NAME_LENGTH: usize = 0x221;
const HOST_NAME: usize = 0x222;
const HOST_NAME_UNITS: usize = 10;
const MAX_PLAYERS: usize = 0x236;
const TITLE: usize = 0x238;
const TITLE_UNITS: usize = 48;
const DESCRIPTION: usize = 0x298;
const DESCRIPTION_UNITS: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Offer {
    pub host: [u8; 6],
    pub channel: u8,
    pub game_id: u32,
    pub stream: u16,
    pub session: u8,
    pub beacon_period: u16,
    pub cmd_bytes: u16,
    pub title: String,
    pub description: String,
    pub host_name: String,
    pub max_players: u8,
    icon: Vec<u8>,
}

impl Offer {
    pub fn icon(&self) -> Option<[u8; ICON_BYTES]> {
        self.icon.as_slice().try_into().ok()
    }

    pub fn same_stream(&self, other: &Offer) -> bool {
        self.host == other.host && self.game_id == other.game_id && self.stream == other.stream
    }

    fn from_advert(beacon: &Beacon, advert: &[u8]) -> Self {
        let mut icon = advert[PALETTE_BYTES..PALETTE_BYTES + TILES_BYTES].to_vec();
        icon.extend_from_slice(&advert[..PALETTE_BYTES]);
        let host_units = usize::from(advert[HOST_NAME_LENGTH]).min(HOST_NAME_UNITS);
        Self {
            host: beacon.host,
            channel: beacon.channel,
            game_id: beacon.game_id,
            stream: beacon.stream,
            session: beacon.session,
            beacon_period: beacon.period,
            cmd_bytes: beacon.cmd_bytes,
            title: ucs2(&advert[TITLE..], TITLE_UNITS),
            description: ucs2(&advert[DESCRIPTION..], DESCRIPTION_UNITS),
            host_name: ucs2(&advert[HOST_NAME..], host_units),
            max_players: advert[MAX_PLAYERS],
            icon,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Beacon {
    pub host: [u8; 6],
    pub channel: u8,
    pub period: u16,
    pub game_id: u32,
    pub stream: u16,
    pub cmd_bytes: u16,
    pub session: u8,
    pub fragment: u8,
    pub data: Vec<u8>,
}

impl Beacon {
    pub fn parse(frame: &Frame) -> Option<Self> {
        if frame.kind() != Kind::Management(BEACON) || !frame.fcs_ok() {
            return None;
        }
        let body = frame.bytes.get(HEADER_BYTES..frame.bytes.len() - 4)?;
        let mut channel = frame.channel;
        let mut vendor = None;
        let mut tags = body.get(FIXED_FIELDS..)?;
        while let [tag, length, rest @ ..] = tags {
            let length = usize::from(*length);
            let value = rest.get(..length)?;
            match *tag {
                CHANNEL_TAG if length == 1 => channel = value[0],
                VENDOR_TAG if value.starts_with(&NINTENDO_OUI) => vendor = Some(value),
                _ => {}
            }
            tags = &rest[length..];
        }
        let vendor = vendor?;
        if vendor.len() < PAYLOAD_END || vendor[BEACON_TYPE] != MULTIBOOT {
            return None;
        }
        if checksum(&vendor[SUMMED..PAYLOAD_END]) != half(vendor, CHECKSUM) {
            return None;
        }
        let fragment = vendor[FRAGMENT];
        let size = usize::from(half(vendor, DATA_SIZE)).min(FRAGMENT_BYTES);
        Some(Self {
            host: frame.address(2),
            channel,
            period: half(body, BEACON_PERIOD),
            game_id: u32::from_le_bytes(vendor[GAME_ID..GAME_ID + 4].try_into().ok()?),
            stream: half(vendor, STREAM),
            cmd_bytes: half(vendor, CMD_BYTES),
            session: vendor[SESSION],
            fragment,
            data: if usize::from(fragment) < FRAGMENTS
                && vendor[LAST_FRAGMENT] as usize == FRAGMENTS
            {
                vendor[DATA..DATA + size].to_vec()
            } else {
                Vec::new()
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Adverts {
    collecting: Vec<Collecting>,
    offers: Vec<Heard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Collecting {
    host: [u8; 6],
    game_id: u32,
    stream: u16,
    session: u8,
    fragments: Vec<Option<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Heard {
    offer: Offer,
    last: u64,
}

impl Adverts {
    pub fn new() -> Self {
        Self {
            collecting: Vec::new(),
            offers: Vec::new(),
        }
    }

    pub fn offers(&self) -> impl Iterator<Item = &Offer> {
        self.offers.iter().map(|heard| &heard.offer)
    }

    pub fn hear(&mut self, beacon: &Beacon, at: u64) {
        if let Some(heard) = self.offers.iter_mut().find(|heard| {
            heard.offer.host == beacon.host
                && heard.offer.game_id == beacon.game_id
                && heard.offer.stream == beacon.stream
                && heard.offer.session == beacon.session
        }) {
            heard.last = at;
            heard.offer.channel = beacon.channel;
            return;
        }
        if beacon.data.is_empty() {
            return;
        }
        let index = match self
            .collecting
            .iter()
            .position(|item| item.host == beacon.host)
        {
            Some(index) => index,
            None => {
                self.collecting.push(Collecting::for_beacon(beacon));
                self.collecting.len() - 1
            }
        };
        let item = &mut self.collecting[index];
        if (item.game_id, item.stream, item.session)
            != (beacon.game_id, beacon.stream, beacon.session)
        {
            *item = Collecting::for_beacon(beacon);
        }
        item.fragments[usize::from(beacon.fragment)] = Some(beacon.data.clone());
        if item.fragments.iter().all(Option::is_some) {
            let mut advert: Vec<u8> = item.fragments.iter().flatten().flatten().copied().collect();
            advert.resize(ADVERT_BYTES, 0);
            let offer = Offer::from_advert(beacon, &advert);
            self.collecting.remove(index);
            self.offers.retain(|heard| heard.offer.host != offer.host);
            self.offers.push(Heard { offer, last: at });
        }
    }

    pub fn forget_older_than(&mut self, at: u64) {
        self.offers.retain(|heard| heard.last >= at);
    }
}

impl Default for Adverts {
    fn default() -> Self {
        Self::new()
    }
}

impl Collecting {
    fn for_beacon(beacon: &Beacon) -> Self {
        Self {
            host: beacon.host,
            game_id: beacon.game_id,
            stream: beacon.stream,
            session: beacon.session,
            fragments: vec![None; FRAGMENTS],
        }
    }
}

pub fn checksum(summed: &[u8]) -> u16 {
    let sum: u32 = summed.chunks(2).map(|pair| u32::from(half(pair, 0))).sum();
    !((sum >> 16) + (sum & 0xFFFF)) as u16
}

fn ucs2(bytes: &[u8], units: usize) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .take(units)
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units).trim_end().to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::nds::wifi::frame::{self, FCS_BYTES, Rate};

    pub(crate) const HOST: [u8; 6] = [0x00, 0x09, 0xBF, 0x12, 0x34, 0x56];
    pub(crate) const GGID: u32 = 0x0040_00A0;
    pub(crate) const TGID: u16 = 0x1234;
    pub(crate) const BEACON_GAP_US: u64 = 200_000;
    const PAYLOAD: usize = 0x18;

    pub(crate) fn advert(title: &str, host_name: &str) -> Vec<u8> {
        let mut advert = vec![0; ADVERT_BYTES];
        advert[..PALETTE_BYTES].fill(0xAA);
        advert[PALETTE_BYTES..PALETTE_BYTES + TILES_BYTES].fill(0x55);
        let units: Vec<u16> = host_name.encode_utf16().collect();
        advert[HOST_NAME_LENGTH] = units.len() as u8;
        put_ucs2(&mut advert[HOST_NAME..], &units);
        advert[MAX_PLAYERS] = 8;
        put_ucs2(
            &mut advert[TITLE..],
            &title.encode_utf16().collect::<Vec<_>>(),
        );
        put_ucs2(
            &mut advert[DESCRIPTION..],
            &"Download a kart".encode_utf16().collect::<Vec<_>>(),
        );
        advert
    }

    fn put_ucs2(bytes: &mut [u8], units: &[u16]) {
        for (index, unit) in units.iter().enumerate() {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    pub(crate) fn beacon(
        advert: &[u8],
        fragment: u8,
        session: u8,
        cmd_bytes: u16,
        start: u64,
    ) -> Frame {
        let mut vendor = vec![0; PAYLOAD_END];
        vendor[..4].copy_from_slice(&NINTENDO_OUI);
        vendor[4] = 0x0A;
        vendor[0x08..0x0C].copy_from_slice(&0x0040_0001u32.to_le_bytes());
        vendor[GAME_ID..GAME_ID + 4].copy_from_slice(&GGID.to_le_bytes());
        vendor[STREAM..STREAM + 2].copy_from_slice(&TGID.to_le_bytes());
        vendor[0x12] = 0x70;
        vendor[BEACON_TYPE] = MULTIBOOT;
        vendor[CMD_BYTES..CMD_BYTES + 2].copy_from_slice(&cmd_bytes.to_le_bytes());
        vendor[0x16] = 0x08;
        vendor[PAYLOAD..PAYLOAD + 4].copy_from_slice(&GGID.to_le_bytes());
        vendor[SESSION] = session;
        vendor[0x1E] = 1;
        vendor[FRAGMENT] = fragment;
        vendor[SUMMED] = fragment;
        vendor[LAST_FRAGMENT] = FRAGMENTS as u8;
        let first = usize::from(fragment) * FRAGMENT_BYTES;
        if usize::from(fragment) < FRAGMENTS {
            let data = &advert[first..(first + FRAGMENT_BYTES).min(ADVERT_BYTES)];
            vendor[DATA_SIZE..DATA_SIZE + 2].copy_from_slice(&(data.len() as u16).to_le_bytes());
            vendor[DATA..DATA + data.len()].copy_from_slice(data);
        } else {
            vendor[0x1C] = 2;
        }
        let sum = checksum(&vendor[SUMMED..PAYLOAD_END]);
        vendor[CHECKSUM..CHECKSUM + 2].copy_from_slice(&sum.to_le_bytes());
        let mut bytes = vec![0x80, 0x00, 0, 0];
        bytes.extend_from_slice(&[0xFF; 6]);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&0x00C8u16.to_le_bytes());
        bytes.extend_from_slice(&0x0021u16.to_le_bytes());
        bytes.extend_from_slice(&[0x01, 0x02, 0x82, 0x84]);
        bytes.extend_from_slice(&[CHANNEL_TAG, 1, 7]);
        bytes.extend_from_slice(&[0x05, 0x05, 0x00, 0x02, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[VENDOR_TAG, PAYLOAD_END as u8]);
        bytes.extend_from_slice(&vendor);
        bytes.extend_from_slice(&[0; FCS_BYTES]);
        frame::seal(&mut bytes);
        Frame {
            start,
            channel: 7,
            rate: Rate::Mbit1,
            short_preamble: false,
            bytes,
        }
    }

    pub(crate) fn round(advert: &[u8], cmd_bytes: u16, start: u64) -> Vec<Frame> {
        (0..=FRAGMENTS as u8)
            .map(|fragment| {
                beacon(
                    advert,
                    fragment,
                    0,
                    cmd_bytes,
                    start + u64::from(fragment) * BEACON_GAP_US,
                )
            })
            .collect()
    }

    #[test]
    fn nine_fragments_make_one_offer_with_the_banner_and_host() {
        let advert = advert("Mario Kart DS", "Clara");
        let mut adverts = Adverts::new();
        for (fragment, frame) in round(&advert, 0x01FE, 0).iter().enumerate() {
            assert_eq!(adverts.offers().count(), usize::from(fragment == FRAGMENTS));
            adverts.hear(&Beacon::parse(frame).unwrap(), frame.end());
        }
        let offer = adverts.offers().next().unwrap();
        assert_eq!(offer.title, "Mario Kart DS");
        assert_eq!(offer.description, "Download a kart");
        assert_eq!(offer.host_name, "Clara");
        assert_eq!(offer.host, HOST);
        assert_eq!(
            (offer.channel, offer.game_id, offer.stream),
            (7, GGID, TGID)
        );
        assert_eq!(
            (offer.cmd_bytes, offer.beacon_period, offer.max_players),
            (0x01FE, 0xC8, 8)
        );
        let icon = offer.icon().unwrap();
        assert_eq!((icon[0], icon[TILES_BYTES - 1]), (0x55, 0x55));
        assert_eq!((icon[TILES_BYTES], icon[ICON_BYTES - 1]), (0xAA, 0xAA));
    }

    #[test]
    fn a_fragment_with_a_wrong_checksum_is_not_a_beacon() {
        let mut frame = beacon(&advert("Mario Kart DS", "Clara"), 3, 0, 0x01FE, 0);
        let checksum_at = frame.bytes.len() - FCS_BYTES - PAYLOAD_END + CHECKSUM;
        frame.bytes[checksum_at] ^= 1;
        frame::seal(&mut frame.bytes);
        assert!(Beacon::parse(&frame).is_none());
        let mut other = beacon(&advert("Mario Kart DS", "Clara"), 3, 0, 0x01FE, 0);
        let type_at = other.bytes.len() - FCS_BYTES - PAYLOAD_END + BEACON_TYPE;
        other.bytes[type_at] = 0x09;
        frame::seal(&mut other.bytes);
        assert!(Beacon::parse(&other).is_none());
    }

    #[test]
    fn a_new_session_starts_the_fragments_over() {
        let advert = advert("Mario Kart DS", "Clara");
        let mut adverts = Adverts::new();
        for fragment in 0..5 {
            let frame = beacon(&advert, fragment, 0, 0x01FE, 0);
            adverts.hear(&Beacon::parse(&frame).unwrap(), 0);
        }
        for fragment in 5..FRAGMENTS as u8 {
            let frame = beacon(&advert, fragment, 1, 0x01FE, 0);
            adverts.hear(&Beacon::parse(&frame).unwrap(), 0);
        }
        assert_eq!(adverts.offers().count(), 0);
        for fragment in 0..5 {
            let frame = beacon(&advert, fragment, 1, 0x01FE, 0);
            adverts.hear(&Beacon::parse(&frame).unwrap(), 0);
        }
        assert_eq!(adverts.offers().next().unwrap().session, 1);
    }

    #[test]
    fn an_offer_goes_when_its_host_falls_silent() {
        let advert = advert("Mario Kart DS", "Clara");
        let mut adverts = Adverts::new();
        for frame in round(&advert, 0x01FE, 0) {
            adverts.hear(&Beacon::parse(&frame).unwrap(), frame.end());
        }
        let last = beacon(&advert, 9, 0, 0x01FE, 5_000_000);
        adverts.hear(&Beacon::parse(&last).unwrap(), last.end());
        adverts.forget_older_than(last.end() - 1);
        assert_eq!(adverts.offers().count(), 1);
        adverts.forget_older_than(last.end() + 1);
        assert_eq!(adverts.offers().count(), 0);
    }
}
