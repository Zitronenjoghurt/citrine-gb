#[cfg(feature = "gba")]
use crate::gba::save::SaveType;
use crate::persistence::rtc_footer::RtcFooter;
#[cfg(feature = "gba")]
use crate::persistence::seiko_footer::SeikoFooter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatterySave {
    pub ram: Vec<u8>,
    pub clock: Option<RtcFooter>,
    #[cfg(feature = "gba")]
    pub seiko: Option<SeikoFooter>,
    #[cfg(feature = "gba")]
    pub chip: Option<SaveType>,
}

impl BatterySave {
    pub fn raw(ram: Vec<u8>) -> Self {
        Self {
            ram,
            clock: None,
            #[cfg(feature = "gba")]
            seiko: None,
            #[cfg(feature = "gba")]
            chip: None,
        }
    }

    pub fn to_file(&self) -> Vec<u8> {
        let mut data = self.ram.clone();
        if let Some(clock) = &self.clock {
            data.extend_from_slice(&clock.encode());
        }
        #[cfg(feature = "gba")]
        if let Some(seiko) = &self.seiko {
            data.extend_from_slice(&seiko.encode());
        }
        data
    }
}
