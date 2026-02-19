use crate::disassembler::Disassembly;
use crate::error::{GbError, GbResult};

pub const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

#[derive(Debug)]
pub struct RomHeader {
    pub title: String,
    pub valid_nintendo_logo: bool,
    pub cgb_mode: RomCgbMode,
    pub sgb_support: bool,
    pub licensee: RomLicensee,
    pub cartridge_type: Option<RomCartridgeType>,
    pub rom_banks: usize,
    pub ram_banks: usize,
    pub overseas_only: bool,
    pub version_number: u8,
    pub provided_header_checksum: u8,
    pub actual_header_checksum: u8,
    pub provided_global_checksum: u16,
    pub actual_global_checksum: u16,
    pub entrypoint: Disassembly,
}

impl RomHeader {
    pub fn new(data: &[u8]) -> GbResult<Self> {
        Ok(Self {
            title: Self::parse_title(data)?,
            valid_nintendo_logo: Self::parse_valid_nintendo_logo(data)?,
            cgb_mode: Self::parse_cgb_mode(data)?,
            sgb_support: Self::parse_sgb_support(data)?,
            licensee: Self::parse_licensee(data)?,
            cartridge_type: Self::parse_cartridge_type(data)?,
            rom_banks: Self::parse_rom_banks(data)?,
            ram_banks: Self::parse_ram_banks(data)?,
            overseas_only: Self::parse_overseas_only(data)?,
            version_number: Self::parse_version_number(data)?,
            provided_header_checksum: Self::parse_header_checksum(data)?,
            actual_header_checksum: Self::calculate_header_checksum(data)?,
            provided_global_checksum: Self::parse_global_checksum(data)?,
            actual_global_checksum: Self::calculate_global_checksum(data)?,
            entrypoint: Self::parse_entrypoint(data)?,
        })
    }

    pub fn parse_title(data: &[u8]) -> GbResult<String> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        let bytes = &data[0x134..=0x143];
        let end = bytes.iter().position(|&b| b == 0x00).unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..end]).to_string())
    }

    pub fn parse_valid_nintendo_logo(data: &[u8]) -> GbResult<bool> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(data[0x104..=0x133] == NINTENDO_LOGO)
    }

    pub fn parse_cgb_mode(data: &[u8]) -> GbResult<RomCgbMode> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(match data[0x143] {
            0x80 => RomCgbMode::CgbAndGb,
            0xC0 => RomCgbMode::CgbOnly,
            _ => RomCgbMode::None,
        })
    }

    pub fn parse_sgb_support(data: &[u8]) -> GbResult<bool> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(data[0x146] == 0x03)
    }

    pub fn parse_licensee(data: &[u8]) -> GbResult<RomLicensee> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        let old_code = data[0x14B];
        if old_code == 0x33 {
            Ok(Self::parse_new_licensee(data))
        } else {
            Ok(Self::parse_old_licensee(old_code))
        }
    }

    fn parse_old_licensee(code: u8) -> RomLicensee {
        match code {
            0x00 => RomLicensee::None,
            0x01 => RomLicensee::Nintendo,
            0x08 => RomLicensee::Capcom,
            0x09 => RomLicensee::HotB,
            0x0A => RomLicensee::Jaleco,
            0x0B => RomLicensee::CoconutsJapan,
            0x0C => RomLicensee::EliteSystems,
            0x13 => RomLicensee::ElectronicArts,
            0x18 => RomLicensee::HudsonSoft,
            0x19 => RomLicensee::ItcEntertainment,
            0x1A => RomLicensee::Yanoman,
            0x1D => RomLicensee::JapanClary,
            0x1F => RomLicensee::VirginGames,
            0x24 => RomLicensee::PcmComplete,
            0x25 => RomLicensee::SanX,
            0x28 => RomLicensee::Kemco,
            0x29 => RomLicensee::SetaCorporation,
            0x30 => RomLicensee::Infogrames,
            0x31 => RomLicensee::Nintendo,
            0x32 => RomLicensee::Bandai,
            0x34 => RomLicensee::Konami,
            0x35 => RomLicensee::HectorSoft,
            0x38 => RomLicensee::Capcom,
            0x39 => RomLicensee::Banpresto,
            0x3C => RomLicensee::EntertainmentInteractive,
            0x3E => RomLicensee::Gremlin,
            0x41 => RomLicensee::UbiSoft,
            0x42 => RomLicensee::Atlus,
            0x44 => RomLicensee::MalibuInteractive,
            0x46 => RomLicensee::Angel,
            0x47 => RomLicensee::SpectrumHoloByte,
            0x49 => RomLicensee::Irem,
            0x4A => RomLicensee::VirginGames,
            0x4D => RomLicensee::MalibuInteractive,
            0x4F => RomLicensee::UsGold,
            0x50 => RomLicensee::Absolute,
            0x51 => RomLicensee::AcclaimEntertainment,
            0x52 => RomLicensee::Activision,
            0x53 => RomLicensee::SammyUsaCorporation,
            0x54 => RomLicensee::GameTek,
            0x55 => RomLicensee::ParkPlace,
            0x56 => RomLicensee::Ljn,
            0x57 => RomLicensee::Matchbox,
            0x59 => RomLicensee::MiltonBradleyCompany,
            0x5A => RomLicensee::Mindscape,
            0x5B => RomLicensee::Romstar,
            0x5C => RomLicensee::NaxatSoft,
            0x5D => RomLicensee::Tradewest,
            0x60 => RomLicensee::TitusInteractive,
            0x61 => RomLicensee::VirginGames,
            0x67 => RomLicensee::OceanSoftware,
            0x69 => RomLicensee::ElectronicArts,
            0x6E => RomLicensee::EliteSystems,
            0x6F => RomLicensee::ElectroBrain,
            0x70 => RomLicensee::Infogrames,
            0x71 => RomLicensee::InterplayEntertainment,
            0x72 => RomLicensee::Broderbund,
            0x73 => RomLicensee::SculpturedSoftware,
            0x75 => RomLicensee::TheSalesCurveLimited,
            0x78 => RomLicensee::Thq,
            0x79 => RomLicensee::Accolade,
            0x7A => RomLicensee::TriffixEntertainment,
            0x7C => RomLicensee::MicroProse,
            0x7F => RomLicensee::Kemco,
            0x80 => RomLicensee::MisawaEntertainment,
            0x83 => RomLicensee::LozcG,
            0x86 => RomLicensee::TokumaShoten,
            0x8B => RomLicensee::BulletProofSoftware,
            0x8C => RomLicensee::VicTokaiCorp,
            0x8E => RomLicensee::ApeInc,
            0x8F => RomLicensee::IMax,
            0x91 => RomLicensee::Chunsoft,
            0x92 => RomLicensee::VideoSystem,
            0x93 => RomLicensee::TsubarayaProductions,
            0x95 => RomLicensee::Varie,
            0x96 => RomLicensee::YonezawaSPal,
            0x97 => RomLicensee::Kemco,
            0x99 => RomLicensee::Arc,
            0x9A => RomLicensee::NihonBussan,
            0x9B => RomLicensee::Tecmo,
            0x9C => RomLicensee::Imagineer,
            0x9D => RomLicensee::Banpresto,
            0x9F => RomLicensee::Nova,
            0xA1 => RomLicensee::HoriElectric,
            0xA2 => RomLicensee::Bandai,
            0xA4 => RomLicensee::Konami,
            0xA6 => RomLicensee::Kawada,
            0xA7 => RomLicensee::Takara,
            0xA9 => RomLicensee::TechnosJapan,
            0xAA => RomLicensee::Broderbund,
            0xAC => RomLicensee::ToeiAnimation,
            0xAD => RomLicensee::Toho,
            0xAF => RomLicensee::Namco,
            0xB0 => RomLicensee::AcclaimEntertainment,
            0xB1 => RomLicensee::AsciiOrNexsoft,
            0xB2 => RomLicensee::Bandai,
            0xB4 => RomLicensee::SquareEnix,
            0xB6 => RomLicensee::HalLaboratory,
            0xB7 => RomLicensee::Snk,
            0xB9 => RomLicensee::PonyCanyon,
            0xBA => RomLicensee::CultureBrain,
            0xBB => RomLicensee::Sunsoft,
            0xBD => RomLicensee::SonyImagesoft,
            0xBF => RomLicensee::SammyCorporation,
            0xC0 => RomLicensee::Taito,
            0xC2 => RomLicensee::Kemco,
            0xC3 => RomLicensee::Square,
            0xC4 => RomLicensee::TokumaShoten,
            0xC5 => RomLicensee::DataEast,
            0xC6 => RomLicensee::TonkinHouse,
            0xC8 => RomLicensee::Koei,
            0xC9 => RomLicensee::Ufl,
            0xCA => RomLicensee::UltraGames,
            0xCB => RomLicensee::VapInc,
            0xCC => RomLicensee::UseCorporation,
            0xCD => RomLicensee::Meldac,
            0xCE => RomLicensee::PonyCanyon,
            0xCF => RomLicensee::Angel,
            0xD0 => RomLicensee::Taito,
            0xD1 => RomLicensee::Sofel,
            0xD2 => RomLicensee::Quest,
            0xD3 => RomLicensee::SigmaEnterprises,
            0xD4 => RomLicensee::AskKodansha,
            0xD6 => RomLicensee::NaxatSoft,
            0xD7 => RomLicensee::CopyaSystem,
            0xD9 => RomLicensee::Banpresto,
            0xDA => RomLicensee::Tomy,
            0xDB => RomLicensee::Ljn,
            0xDD => RomLicensee::NipponComputerSystems,
            0xDE => RomLicensee::HumanEntertainment,
            0xDF => RomLicensee::Altron,
            0xE0 => RomLicensee::Jaleco,
            0xE1 => RomLicensee::TowaChiki,
            0xE2 => RomLicensee::Yutaka,
            0xE3 => RomLicensee::Varie,
            0xE5 => RomLicensee::Epoch,
            0xE7 => RomLicensee::Athena,
            0xE8 => RomLicensee::AsmikAceEntertainment,
            0xE9 => RomLicensee::Natsume,
            0xEA => RomLicensee::KingRecords,
            0xEB => RomLicensee::Atlus,
            0xEC => RomLicensee::EpicSonyRecords,
            0xEE => RomLicensee::Igs,
            0xF0 => RomLicensee::AWave,
            0xF3 => RomLicensee::ExtremeEntertainment,
            0xFF => RomLicensee::Ljn,
            _ => RomLicensee::Unknown,
        }
    }

    fn parse_new_licensee(data: &[u8]) -> RomLicensee {
        let code = [data[0x144], data[0x145]];
        match &code {
            b"00" => RomLicensee::None,
            b"01" => RomLicensee::NintendoRd1,
            b"08" => RomLicensee::Capcom,
            b"13" => RomLicensee::ElectronicArts,
            b"18" => RomLicensee::HudsonSoft,
            b"19" => RomLicensee::BAi,
            b"20" => RomLicensee::Kss,
            b"22" => RomLicensee::PlanningOfficeWada,
            b"24" => RomLicensee::PcmComplete,
            b"25" => RomLicensee::SanX,
            b"28" => RomLicensee::Kemco,
            b"29" => RomLicensee::SetaCorporation,
            b"30" => RomLicensee::Viacom,
            b"31" => RomLicensee::Nintendo,
            b"32" => RomLicensee::Bandai,
            b"33" => RomLicensee::OceanAcclaim,
            b"34" => RomLicensee::Konami,
            b"35" => RomLicensee::HectorSoft,
            b"37" => RomLicensee::Taito,
            b"38" => RomLicensee::HudsonSoft,
            b"39" => RomLicensee::Banpresto,
            b"41" => RomLicensee::UbiSoft,
            b"42" => RomLicensee::Atlus,
            b"44" => RomLicensee::MalibuInteractive,
            b"46" => RomLicensee::Angel,
            b"47" => RomLicensee::BulletProofSoftware,
            b"49" => RomLicensee::Irem,
            b"50" => RomLicensee::Absolute,
            b"51" => RomLicensee::AcclaimEntertainment,
            b"52" => RomLicensee::Activision,
            b"53" => RomLicensee::SammyUsaCorporation,
            b"54" => RomLicensee::Konami,
            b"55" => RomLicensee::HiTechExpressions,
            b"56" => RomLicensee::Ljn,
            b"57" => RomLicensee::Matchbox,
            b"58" => RomLicensee::Mattel,
            b"59" => RomLicensee::MiltonBradleyCompany,
            b"60" => RomLicensee::TitusInteractive,
            b"61" => RomLicensee::VirginGames,
            b"64" => RomLicensee::LucasfilmGames,
            b"67" => RomLicensee::OceanSoftware,
            b"69" => RomLicensee::ElectronicArts,
            b"70" => RomLicensee::Infogrames,
            b"71" => RomLicensee::InterplayEntertainment,
            b"72" => RomLicensee::Broderbund,
            b"73" => RomLicensee::SculpturedSoftware,
            b"75" => RomLicensee::TheSalesCurveLimited,
            b"78" => RomLicensee::Thq,
            b"79" => RomLicensee::Accolade,
            b"80" => RomLicensee::MisawaEntertainment,
            b"83" => RomLicensee::LozcG,
            b"86" => RomLicensee::TokumaShoten,
            b"87" => RomLicensee::TsukudaOriginal,
            b"91" => RomLicensee::Chunsoft,
            b"92" => RomLicensee::VideoSystem,
            b"93" => RomLicensee::OceanAcclaim,
            b"95" => RomLicensee::Varie,
            b"96" => RomLicensee::YonezawaSPal,
            b"97" => RomLicensee::Kaneko,
            b"99" => RomLicensee::PackInVideo,
            b"9H" => RomLicensee::BottomUp,
            b"A4" => RomLicensee::KonamiYuGiOh,
            b"BL" => RomLicensee::Mto,
            b"DK" => RomLicensee::Kodansha,
            _ => RomLicensee::Unknown,
        }
    }

    pub fn parse_cartridge_type(data: &[u8]) -> GbResult<Option<RomCartridgeType>> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(match data[0x147] {
            0x00 => Some(RomCartridgeType::RomOnly),
            0x01 => Some(RomCartridgeType::Mbc1),
            0x02 => Some(RomCartridgeType::Mbc1Ram),
            0x03 => Some(RomCartridgeType::Mbc1RamBattery),
            0x05 => Some(RomCartridgeType::Mbc2),
            0x06 => Some(RomCartridgeType::Mbc2Battery),
            0x08 => Some(RomCartridgeType::RomRam),
            0x09 => Some(RomCartridgeType::RomRamBattery),
            0x0B => Some(RomCartridgeType::Mmm01),
            0x0C => Some(RomCartridgeType::Mmm01Ram),
            0x0D => Some(RomCartridgeType::Mmm01RamBattery),
            0x0F => Some(RomCartridgeType::Mbc3TimerBattery),
            0x10 => Some(RomCartridgeType::Mbc3TimerRamBattery),
            0x11 => Some(RomCartridgeType::Mbc3),
            0x12 => Some(RomCartridgeType::Mbc3Ram),
            0x13 => Some(RomCartridgeType::Mbc3RamBattery),
            0x19 => Some(RomCartridgeType::Mbc5),
            0x1A => Some(RomCartridgeType::Mbc5Ram),
            0x1B => Some(RomCartridgeType::Mbc5RamBattery),
            0x1C => Some(RomCartridgeType::Mbc5Rumble),
            0x1D => Some(RomCartridgeType::Mbc5RumbleRam),
            0x1E => Some(RomCartridgeType::Mbc5RumbleRamBattery),
            0x20 => Some(RomCartridgeType::Mbc6),
            0x22 => Some(RomCartridgeType::Mbc7SensorRumbleRamBattery),
            0xFC => Some(RomCartridgeType::PocketCamera),
            0xFD => Some(RomCartridgeType::BandaiTama5),
            0xFE => Some(RomCartridgeType::HuC3),
            0xFF => Some(RomCartridgeType::HuC1RamBattery),
            _ => None,
        })
    }

    pub fn parse_rom_banks(data: &[u8]) -> GbResult<usize> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(match data[0x148] {
            0x00 => 2,
            0x01 => 4,
            0x02 => 8,
            0x03 => 16,
            0x04 => 32,
            0x05 => 64,
            0x06 => 128,
            0x07 => 256,
            0x08 => 512,
            0x52 => 72,
            0x53 => 80,
            0x54 => 96,
            _ => 0,
        })
    }

    pub fn parse_ram_banks(data: &[u8]) -> GbResult<usize> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(match data[0x149] {
            0x02 => 1,
            0x03 => 4,
            0x04 => 16,
            0x05 => 8,
            _ => 0,
        })
    }

    pub fn parse_overseas_only(data: &[u8]) -> GbResult<bool> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(data[0x14A] == 0x01)
    }

    pub fn parse_version_number(data: &[u8]) -> GbResult<u8> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(data[0x14C])
    }

    pub fn calculate_header_checksum(data: &[u8]) -> GbResult<u8> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(data[0x134..=0x14C]
            .iter()
            .fold(0, |acc, &b| acc.wrapping_sub(b).wrapping_sub(1)))
    }

    pub fn parse_header_checksum(data: &[u8]) -> GbResult<u8> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(data[0x14D])
    }

    pub fn calculate_global_checksum(data: &[u8]) -> GbResult<u16> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(data
            .iter()
            .enumerate()
            .filter(|(addr, _)| *addr < 0x14E || *addr > 0x14F)
            .fold(0, |acc, (_, byte)| acc.wrapping_add(*byte as u16)))
    }

    pub fn parse_global_checksum(data: &[u8]) -> GbResult<u16> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        Ok(u16::from_be_bytes([data[0x14E], data[0x14F]]))
    }

    pub fn parse_entrypoint(data: &[u8]) -> GbResult<Disassembly> {
        if data.len() < 0x150 {
            return Err(GbError::RomTooSmall);
        }
        let mut entrypoint = Disassembly::new();
        entrypoint.decode_range(&data, 0x100, 0x104);
        Ok(entrypoint)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RomCgbMode {
    None = 0x00,
    CgbOnly = 0x80,
    CgbAndGb = 0xC0,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RomLicensee {
    Unknown,
    None,
    Nintendo,
    NintendoRd1,
    Capcom,
    HotB,
    Jaleco,
    CoconutsJapan,
    EliteSystems,
    ElectronicArts,
    HudsonSoft,
    ItcEntertainment,
    Yanoman,
    JapanClary,
    VirginGames,
    PcmComplete,
    SanX,
    Kemco,
    SetaCorporation,
    Infogrames,
    Bandai,
    Konami,
    HectorSoft,
    Banpresto,
    EntertainmentInteractive,
    Gremlin,
    UbiSoft,
    Atlus,
    MalibuInteractive,
    Angel,
    SpectrumHoloByte,
    Irem,
    UsGold,
    Absolute,
    AcclaimEntertainment,
    Activision,
    SammyUsaCorporation,
    GameTek,
    ParkPlace,
    Ljn,
    Matchbox,
    MiltonBradleyCompany,
    Mindscape,
    Romstar,
    NaxatSoft,
    Tradewest,
    TitusInteractive,
    OceanSoftware,
    ElectroBrain,
    InterplayEntertainment,
    Broderbund,
    SculpturedSoftware,
    TheSalesCurveLimited,
    Thq,
    Accolade,
    TriffixEntertainment,
    MicroProse,
    MisawaEntertainment,
    LozcG,
    TokumaShoten,
    BulletProofSoftware,
    VicTokaiCorp,
    ApeInc,
    IMax,
    Chunsoft,
    VideoSystem,
    TsubarayaProductions,
    Varie,
    YonezawaSPal,
    Arc,
    NihonBussan,
    Tecmo,
    Imagineer,
    Nova,
    HoriElectric,
    Kawada,
    Takara,
    TechnosJapan,
    ToeiAnimation,
    Toho,
    Namco,
    AsciiOrNexsoft,
    SquareEnix,
    HalLaboratory,
    Snk,
    PonyCanyon,
    CultureBrain,
    Sunsoft,
    SonyImagesoft,
    SammyCorporation,
    Taito,
    Square,
    DataEast,
    TonkinHouse,
    Koei,
    Ufl,
    UltraGames,
    VapInc,
    UseCorporation,
    Meldac,
    Sofel,
    Quest,
    SigmaEnterprises,
    AskKodansha,
    CopyaSystem,
    Tomy,
    NipponComputerSystems,
    HumanEntertainment,
    Altron,
    TowaChiki,
    Yutaka,
    Epoch,
    Athena,
    AsmikAceEntertainment,
    Natsume,
    KingRecords,
    EpicSonyRecords,
    Igs,
    AWave,
    ExtremeEntertainment,
    BAi,
    Kss,
    PlanningOfficeWada,
    Viacom,
    OceanAcclaim,
    HiTechExpressions,
    Mattel,
    LucasfilmGames,
    TsukudaOriginal,
    Kaneko,
    PackInVideo,
    BottomUp,
    KonamiYuGiOh,
    Mto,
    Kodansha,
}

impl std::fmt::Display for RomLicensee {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RomLicensee::Unknown => write!(f, "Unknown Licensee"),
            RomLicensee::None => write!(f, "None"),
            RomLicensee::Nintendo => write!(f, "Nintendo"),
            RomLicensee::NintendoRd1 => write!(f, "Nintendo Research & Development 1"),
            RomLicensee::Capcom => write!(f, "Capcom"),
            RomLicensee::HotB => write!(f, "HOT-B"),
            RomLicensee::Jaleco => write!(f, "Jaleco"),
            RomLicensee::CoconutsJapan => write!(f, "Coconuts Japan"),
            RomLicensee::EliteSystems => write!(f, "Elite Systems"),
            RomLicensee::ElectronicArts => write!(f, "EA (Electronic Arts)"),
            RomLicensee::HudsonSoft => write!(f, "Hudson Soft"),
            RomLicensee::ItcEntertainment => write!(f, "ITC Entertainment"),
            RomLicensee::Yanoman => write!(f, "Yanoman"),
            RomLicensee::JapanClary => write!(f, "Japan Clary"),
            RomLicensee::VirginGames => write!(f, "Virgin Games Ltd."),
            RomLicensee::PcmComplete => write!(f, "PCM Complete"),
            RomLicensee::SanX => write!(f, "San-X"),
            RomLicensee::Kemco => write!(f, "Kemco"),
            RomLicensee::SetaCorporation => write!(f, "SETA Corporation"),
            RomLicensee::Infogrames => write!(f, "Infogrames"),
            RomLicensee::Bandai => write!(f, "Bandai"),
            RomLicensee::Konami => write!(f, "Konami"),
            RomLicensee::HectorSoft => write!(f, "HectorSoft"),
            RomLicensee::Banpresto => write!(f, "Banpresto"),
            RomLicensee::EntertainmentInteractive => write!(f, "Entertainment Interactive"),
            RomLicensee::Gremlin => write!(f, "Gremlin"),
            RomLicensee::UbiSoft => write!(f, "Ubi Soft"),
            RomLicensee::Atlus => write!(f, "Atlus"),
            RomLicensee::MalibuInteractive => write!(f, "Malibu Interactive"),
            RomLicensee::Angel => write!(f, "Angel"),
            RomLicensee::SpectrumHoloByte => write!(f, "Spectrum HoloByte"),
            RomLicensee::Irem => write!(f, "Irem"),
            RomLicensee::UsGold => write!(f, "U.S. Gold"),
            RomLicensee::Absolute => write!(f, "Absolute"),
            RomLicensee::AcclaimEntertainment => write!(f, "Acclaim Entertainment"),
            RomLicensee::Activision => write!(f, "Activision"),
            RomLicensee::SammyUsaCorporation => write!(f, "Sammy USA Corporation"),
            RomLicensee::GameTek => write!(f, "GameTek"),
            RomLicensee::ParkPlace => write!(f, "Park Place"),
            RomLicensee::Ljn => write!(f, "LJN"),
            RomLicensee::Matchbox => write!(f, "Matchbox"),
            RomLicensee::MiltonBradleyCompany => write!(f, "Milton Bradley Company"),
            RomLicensee::Mindscape => write!(f, "Mindscape"),
            RomLicensee::Romstar => write!(f, "Romstar"),
            RomLicensee::NaxatSoft => write!(f, "Naxat Soft"),
            RomLicensee::Tradewest => write!(f, "Tradewest"),
            RomLicensee::TitusInteractive => write!(f, "Titus Interactive"),
            RomLicensee::OceanSoftware => write!(f, "Ocean Software"),
            RomLicensee::ElectroBrain => write!(f, "Electro Brain"),
            RomLicensee::InterplayEntertainment => write!(f, "Interplay Entertainment"),
            RomLicensee::Broderbund => write!(f, "Broderbund"),
            RomLicensee::SculpturedSoftware => write!(f, "Sculptured Software"),
            RomLicensee::TheSalesCurveLimited => write!(f, "The Sales Curve Limited"),
            RomLicensee::Thq => write!(f, "THQ"),
            RomLicensee::Accolade => write!(f, "Accolade"),
            RomLicensee::TriffixEntertainment => write!(f, "Triffix Entertainment"),
            RomLicensee::MicroProse => write!(f, "MicroProse"),
            RomLicensee::MisawaEntertainment => write!(f, "Misawa Entertainment"),
            RomLicensee::LozcG => write!(f, "LOZC G."),
            RomLicensee::TokumaShoten => write!(f, "Tokuma Shoten"),
            RomLicensee::BulletProofSoftware => write!(f, "Bullet-Proof Software"),
            RomLicensee::VicTokaiCorp => write!(f, "Vic Tokai Corp."),
            RomLicensee::ApeInc => write!(f, "Ape Inc."),
            RomLicensee::IMax => write!(f, "I'Max"),
            RomLicensee::Chunsoft => write!(f, "Chunsoft Co."),
            RomLicensee::VideoSystem => write!(f, "Video System"),
            RomLicensee::TsubarayaProductions => write!(f, "Tsubaraya Productions"),
            RomLicensee::Varie => write!(f, "Varie"),
            RomLicensee::YonezawaSPal => write!(f, "Yonezawa/S'Pal"),
            RomLicensee::Arc => write!(f, "Arc"),
            RomLicensee::NihonBussan => write!(f, "Nihon Bussan"),
            RomLicensee::Tecmo => write!(f, "Tecmo"),
            RomLicensee::Imagineer => write!(f, "Imagineer"),
            RomLicensee::Nova => write!(f, "Nova"),
            RomLicensee::HoriElectric => write!(f, "Hori Electric"),
            RomLicensee::Kawada => write!(f, "Kawada"),
            RomLicensee::Takara => write!(f, "Takara"),
            RomLicensee::TechnosJapan => write!(f, "Technos Japan"),
            RomLicensee::ToeiAnimation => write!(f, "Toei Animation"),
            RomLicensee::Toho => write!(f, "Toho"),
            RomLicensee::Namco => write!(f, "Namco"),
            RomLicensee::AsciiOrNexsoft => write!(f, "ASCII Corporation/Nexsoft"),
            RomLicensee::SquareEnix => write!(f, "Square Enix"),
            RomLicensee::HalLaboratory => write!(f, "HAL Laboratory"),
            RomLicensee::Snk => write!(f, "SNK"),
            RomLicensee::PonyCanyon => write!(f, "Pony Canyon"),
            RomLicensee::CultureBrain => write!(f, "Culture Brain"),
            RomLicensee::Sunsoft => write!(f, "Sunsoft"),
            RomLicensee::SonyImagesoft => write!(f, "Sony Imagesoft"),
            RomLicensee::SammyCorporation => write!(f, "Sammy Corporation"),
            RomLicensee::Taito => write!(f, "Taito"),
            RomLicensee::Square => write!(f, "Square"),
            RomLicensee::DataEast => write!(f, "Data East"),
            RomLicensee::TonkinHouse => write!(f, "Tonkin House"),
            RomLicensee::Koei => write!(f, "Koei"),
            RomLicensee::Ufl => write!(f, "UFL"),
            RomLicensee::UltraGames => write!(f, "Ultra Games"),
            RomLicensee::VapInc => write!(f, "VAP, Inc."),
            RomLicensee::UseCorporation => write!(f, "Use Corporation"),
            RomLicensee::Meldac => write!(f, "Meldac"),
            RomLicensee::Sofel => write!(f, "SOFEL"),
            RomLicensee::Quest => write!(f, "Quest"),
            RomLicensee::SigmaEnterprises => write!(f, "Sigma Enterprises"),
            RomLicensee::AskKodansha => write!(f, "ASK Kodansha Co."),
            RomLicensee::CopyaSystem => write!(f, "Copya System"),
            RomLicensee::Tomy => write!(f, "Tomy"),
            RomLicensee::NipponComputerSystems => write!(f, "Nippon Computer Systems"),
            RomLicensee::HumanEntertainment => write!(f, "Human Entertainment"),
            RomLicensee::Altron => write!(f, "Altron"),
            RomLicensee::TowaChiki => write!(f, "Towa Chiki"),
            RomLicensee::Yutaka => write!(f, "Yutaka"),
            RomLicensee::Epoch => write!(f, "Epoch"),
            RomLicensee::Athena => write!(f, "Athena"),
            RomLicensee::AsmikAceEntertainment => write!(f, "Asmik Ace Entertainment"),
            RomLicensee::Natsume => write!(f, "Natsume"),
            RomLicensee::KingRecords => write!(f, "King Records"),
            RomLicensee::EpicSonyRecords => write!(f, "Epic/Sony Records"),
            RomLicensee::Igs => write!(f, "IGS"),
            RomLicensee::AWave => write!(f, "A Wave"),
            RomLicensee::ExtremeEntertainment => write!(f, "Extreme Entertainment"),
            RomLicensee::BAi => write!(f, "B-AI"),
            RomLicensee::Kss => write!(f, "KSS"),
            RomLicensee::PlanningOfficeWada => write!(f, "Planning Office WADA"),
            RomLicensee::Viacom => write!(f, "Viacom"),
            RomLicensee::OceanAcclaim => write!(f, "Ocean Software/Acclaim Entertainment"),
            RomLicensee::HiTechExpressions => write!(f, "Hi Tech Expressions"),
            RomLicensee::Mattel => write!(f, "Mattel"),
            RomLicensee::LucasfilmGames => write!(f, "Lucasfilm Games"),
            RomLicensee::TsukudaOriginal => write!(f, "Tsukuda Original"),
            RomLicensee::Kaneko => write!(f, "Kaneko"),
            RomLicensee::PackInVideo => write!(f, "Pack-In-Video"),
            RomLicensee::BottomUp => write!(f, "Bottom Up"),
            RomLicensee::KonamiYuGiOh => write!(f, "Konami (Yu-Gi-Oh!)"),
            RomLicensee::Mto => write!(f, "MTO"),
            RomLicensee::Kodansha => write!(f, "Kodansha"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RomCartridgeType {
    RomOnly = 0x00,
    Mbc1 = 0x01,
    Mbc1Ram = 0x02,
    Mbc1RamBattery = 0x03,
    Mbc2 = 0x05,
    Mbc2Battery = 0x06,
    RomRam = 0x08,
    RomRamBattery = 0x09,
    Mmm01 = 0x0B,
    Mmm01Ram = 0x0C,
    Mmm01RamBattery = 0x0D,
    Mbc3TimerBattery = 0x0F,
    Mbc3TimerRamBattery = 0x10,
    Mbc3 = 0x11,
    Mbc3Ram = 0x12,
    Mbc3RamBattery = 0x13,
    Mbc5 = 0x19,
    Mbc5Ram = 0x1A,
    Mbc5RamBattery = 0x1B,
    Mbc5Rumble = 0x1C,
    Mbc5RumbleRam = 0x1D,
    Mbc5RumbleRamBattery = 0x1E,
    Mbc6 = 0x20,
    Mbc7SensorRumbleRamBattery = 0x22,
    PocketCamera = 0xFC,
    BandaiTama5 = 0xFD,
    HuC3 = 0xFE,
    HuC1RamBattery = 0xFF,
}

impl std::fmt::Display for RomCartridgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RomOnly => write!(f, "ROM Only"),
            Self::Mbc1 => write!(f, "MBC1"),
            Self::Mbc1Ram => write!(f, "MBC1 + RAM"),
            Self::Mbc1RamBattery => write!(f, "MBC1 + RAM + Battery"),
            Self::Mbc2 => write!(f, "MBC2"),
            Self::Mbc2Battery => write!(f, "MBC2 + Battery"),
            Self::RomRam => write!(f, "ROM + RAM"),
            Self::RomRamBattery => write!(f, "ROM + RAM + Battery"),
            Self::Mmm01 => write!(f, "MMM01"),
            Self::Mmm01Ram => write!(f, "MMM01 + RAM"),
            Self::Mmm01RamBattery => write!(f, "MMM01 + RAM + Battery"),
            Self::Mbc3TimerBattery => write!(f, "MBC3 + Timer + Battery"),
            Self::Mbc3TimerRamBattery => write!(f, "MBC3 + Timer + RAM + Battery"),
            Self::Mbc3 => write!(f, "MBC3"),
            Self::Mbc3Ram => write!(f, "MBC3 + RAM"),
            Self::Mbc3RamBattery => write!(f, "MBC3 + RAM + Battery"),
            Self::Mbc5 => write!(f, "MBC5"),
            Self::Mbc5Ram => write!(f, "MBC5 + RAM"),
            Self::Mbc5RamBattery => write!(f, "MBC5 + RAM + Battery"),
            Self::Mbc5Rumble => write!(f, "MBC5 + Rumble"),
            Self::Mbc5RumbleRam => write!(f, "MBC5 + Rumble + RAM"),
            Self::Mbc5RumbleRamBattery => write!(f, "MBC5 + Rumble + RAM + Battery"),
            Self::Mbc6 => write!(f, "MBC6"),
            Self::Mbc7SensorRumbleRamBattery => write!(f, "MBC7 + Sensor + Rumble + RAM + Battery"),
            Self::PocketCamera => write!(f, "Pocket Camera"),
            Self::BandaiTama5 => write!(f, "Bandai TAMA5"),
            Self::HuC3 => write!(f, "HuC3"),
            Self::HuC1RamBattery => write!(f, "HuC1 + RAM + Battery"),
        }
    }
}
