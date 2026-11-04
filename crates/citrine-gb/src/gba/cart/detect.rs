use super::Devices;

const GAME_CODE: usize = 0xAC;
const NONE: Devices = Devices {
    rtc: false,
    solar: false,
    gyro: false,
    rumble: false,
    tilt: false,
};
const RTC: Devices = Devices { rtc: true, ..NONE };
const BOKTAI: Devices = Devices {
    rtc: true,
    solar: true,
    ..NONE
};
const TWISTED: Devices = Devices {
    gyro: true,
    rumble: true,
    ..NONE
};
const RUMBLE: Devices = Devices {
    rumble: true,
    ..NONE
};
const TILT: Devices = Devices { tilt: true, ..NONE };

const GAMES: [(&[u8; 3], Devices); 12] = [
    (b"AXV", RTC),
    (b"AXP", RTC),
    (b"BPE", RTC),
    (b"BR4", RTC),
    (b"BKA", RTC),
    (b"U3I", BOKTAI),
    (b"U32", BOKTAI),
    (b"U33", BOKTAI),
    (b"RZW", TWISTED),
    (b"V49", RUMBLE),
    (b"KYG", TILT),
    (b"KHP", TILT),
];

impl Devices {
    pub fn detect(rom: &[u8]) -> Self {
        rom.get(GAME_CODE..GAME_CODE + 3)
            .and_then(|code| GAMES.iter().find(|(game, _)| game.as_slice() == code))
            .map_or(NONE, |&(_, devices)| devices)
    }
}
