use strum_macros::EnumIter;

#[cfg(not(target_arch = "wasm32"))]
pub const RESOLUTION_HINT: &str = "Draws 3D sharper than the DS does. Games that use fog or outlines stay at the DS's resolution.";
#[cfg(not(target_arch = "wasm32"))]
pub const SMOOTH_EDGES_HINT: &str = "Smooths jagged 3D edges. More is smoother and slower.";

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum Resolution {
    #[default]
    Native,
    Double,
    Triple,
    Quadruple,
    Five,
    Six,
    Seven,
    Eight,
}

impl Resolution {
    pub fn scale(self) -> u32 {
        match self {
            Self::Native => 1,
            Self::Double => 2,
            Self::Triple => 3,
            Self::Quadruple => 4,
            Self::Five => 5,
            Self::Six => 6,
            Self::Seven => 7,
            Self::Eight => 8,
        }
    }

    pub fn upscaled(self) -> bool {
        self != Self::Native
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native => f.write_str("Native"),
            other => write!(f, "{}x", other.scale()),
        }
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum SmoothEdges {
    #[default]
    Off,
    On,
    More,
}

impl SmoothEdges {
    pub fn samples(self) -> u32 {
        match self {
            Self::Off => 1,
            Self::On => 4,
            Self::More => 8,
        }
    }
}

impl std::fmt::Display for SmoothEdges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => f.write_str("Off"),
            Self::On => f.write_str("On"),
            Self::More => f.write_str("More"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Ds3d {
    pub resolution: Resolution,
    pub smooth_edges: SmoothEdges,
}

impl Ds3d {
    pub fn upscaled(self) -> bool {
        self.resolution.upscaled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn resolutions_read_native_then_2x_to_8x() {
        let shown: Vec<String> = Resolution::iter().map(|r| r.to_string()).collect();
        assert_eq!(shown, ["Native", "2x", "3x", "4x", "5x", "6x", "7x", "8x"]);
        let edges: Vec<String> = SmoothEdges::iter().map(|e| e.to_string()).collect();
        assert_eq!(edges, ["Off", "On", "More"]);
    }

    #[test]
    fn the_3d_settings_survive_a_settings_file() {
        let chosen = Ds3d {
            resolution: Resolution::Quadruple,
            smooth_edges: SmoothEdges::More,
        };
        let text = serde_json::to_string(&chosen).unwrap();
        assert_eq!(serde_json::from_str::<Ds3d>(&text).unwrap(), chosen);
        assert_eq!(serde_json::from_str::<Ds3d>("{}").unwrap(), Ds3d::default());
        assert_eq!(Ds3d::default().resolution, Resolution::Native);
    }
}
