#![no_main]

use citrine_gb_nds_files::{Depth, Ncgr, Nclr, Nscr, Tex0, models};
use libfuzzer_sys::fuzz_target;

const MAX_TEXTURES: usize = 4;

fuzz_target!(|data: &[u8]| {
    let palette = Nclr::parse(data).unwrap_or(Nclr {
        depth: Depth::Bpp4,
        colors: &[],
    });
    let _ = palette.rows();
    let _ = palette.rgba(1);
    if let Ok(tiles) = Ncgr::parse(data) {
        let _ = tiles.decode(&palette, 0, tiles.default_width());
        if let Ok(screen) = Nscr::parse(data) {
            let _ = screen.decode(&tiles, &palette);
        }
    }
    if let Ok(tex0) = Tex0::find(data).or_else(|_| Tex0::parse(data)) {
        for texture in 0..tex0.textures().len().min(MAX_TEXTURES) {
            let palette = tex0.palette_for(texture);
            let _ = tex0.decode(texture, palette);
            let _ = tex0.texture_param(texture);
        }
        let _ = tex0.vram();
    }
    let _ = models(data);
});
