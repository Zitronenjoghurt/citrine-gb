use strum_macros::EnumIter;

#[cfg(all(not(target_arch = "wasm32"), feature = "nds"))]
pub const RESOLUTION_HINT: &str = "Draws 3D sharper than the DS does. Games that use fog or outlines stay at the DS's resolution.";
#[cfg(all(not(target_arch = "wasm32"), feature = "nds"))]
pub const SMOOTH_EDGES_HINT: &str = "Smooths jagged 3D edges. 8x is smoother and slower.";
#[cfg(all(not(target_arch = "wasm32"), feature = "nds"))]
pub const TEXTURE_FILTER_HINT: &str = "How the pixels on 3D characters, floors and walls look. Nearest keeps them square, as on the DS. Bilinear blends them. Bicubic blends them too and keeps edges crisper. xBR redraws them as lines and curves, and needs the most GPU time.";
#[cfg(all(not(target_arch = "wasm32"), feature = "nds"))]
pub const DISTANT_TEXTURES_HINT: &str = "Stops far floors and walls from shimmering as you move. Off leaves the shimmer. Trilinear stops it and blurs them a little. 2x, 4x and 16x stop it and keep them sharp; each step is sharper and costs more GPU time.";

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

    pub fn log_name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::More => "more",
        }
    }
}

impl std::fmt::Display for SmoothEdges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => f.write_str("Off"),
            Self::On => f.write_str("4x MSAA"),
            Self::More => f.write_str("8x MSAA"),
        }
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum TextureFilter {
    Nearest,
    Bilinear,
    #[default]
    Bicubic,
    Xbr,
}

impl TextureFilter {
    fn before_the_option() -> Self {
        Self::Nearest
    }
}

impl std::fmt::Display for TextureFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Nearest => "Nearest",
            Self::Bilinear => "Bilinear",
            Self::Bicubic => "Bicubic",
            Self::Xbr => "xBR",
        })
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum DistantTextures {
    Off,
    Trilinear,
    Anisotropic2x,
    #[default]
    Anisotropic4x,
    #[serde(alias = "Anisotropic8x")]
    Anisotropic16x,
}

impl DistantTextures {
    fn before_the_option() -> Self {
        Self::Off
    }
}

impl std::fmt::Display for DistantTextures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Off => "Off",
            Self::Trilinear => "Trilinear",
            Self::Anisotropic2x => "2x",
            Self::Anisotropic4x => "4x",
            Self::Anisotropic16x => "16x",
        })
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Ds3d {
    pub resolution: Resolution,
    pub smooth_edges: SmoothEdges,
    #[serde(default = "TextureFilter::before_the_option")]
    pub texture_filter: TextureFilter,
    #[serde(default = "DistantTextures::before_the_option")]
    pub distant_textures: DistantTextures,
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
        assert_eq!(edges, ["Off", "4x MSAA", "8x MSAA"]);
        let filters: Vec<String> = TextureFilter::iter().map(|t| t.to_string()).collect();
        assert_eq!(filters, ["Nearest", "Bilinear", "Bicubic", "xBR"]);
        let distant: Vec<String> = DistantTextures::iter().map(|d| d.to_string()).collect();
        assert_eq!(distant, ["Off", "Trilinear", "2x", "4x", "16x"]);
    }

    #[test]
    fn new_players_start_on_bicubic_and_4x() {
        let start = Ds3d::default();
        assert_eq!(start.texture_filter, TextureFilter::Bicubic);
        assert_eq!(start.distant_textures, DistantTextures::Anisotropic4x);
        assert_eq!(start.texture_filter.to_string(), "Bicubic");
        assert_eq!(start.distant_textures.to_string(), "4x");
    }

    #[test]
    fn the_3d_settings_survive_a_settings_file() {
        let chosen = Ds3d {
            resolution: Resolution::Quadruple,
            smooth_edges: SmoothEdges::More,
            texture_filter: TextureFilter::Xbr,
            distant_textures: DistantTextures::Trilinear,
        };
        let text = serde_json::to_string(&chosen).unwrap();
        assert_eq!(serde_json::from_str::<Ds3d>(&text).unwrap(), chosen);
        let start = serde_json::to_string(&Ds3d::default()).unwrap();
        assert_eq!(
            serde_json::from_str::<Ds3d>(&start).unwrap(),
            Ds3d::default()
        );
        assert_eq!(Ds3d::default().resolution, Resolution::Native);
        let eight = r#"{"distant_textures":"Anisotropic8x"}"#;
        let read = serde_json::from_str::<Ds3d>(eight).unwrap();
        assert_eq!(read.distant_textures, DistantTextures::Anisotropic16x);
        let soft = r#"{"distant_textures":"Trilinear"}"#;
        let read = serde_json::from_str::<Ds3d>(soft).unwrap();
        assert_eq!(read.distant_textures.to_string(), "Trilinear");
    }

    #[test]
    fn settings_from_before_the_texture_options_stay_nearest() {
        let older = r#"{"resolution":"Quadruple","smooth_edges":"On"}"#;
        let read = serde_json::from_str::<Ds3d>(older).unwrap();
        assert_eq!(read.resolution, Resolution::Quadruple);
        assert_eq!(read.smooth_edges, SmoothEdges::On);
        assert_eq!(read.texture_filter.to_string(), "Nearest");
        assert_eq!(read.distant_textures.to_string(), "Off");
        let one = r#"{"texture_filter":"Bilinear"}"#;
        let read = serde_json::from_str::<Ds3d>(one).unwrap();
        assert_eq!(read.texture_filter, TextureFilter::Bilinear);
        assert_eq!(read.distant_textures, DistantTextures::Off);
    }
}
