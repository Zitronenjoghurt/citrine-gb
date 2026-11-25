#![no_main]

use std::collections::HashSet;

use citrine_gb_nds_files::{FileTree, FileType, Narc, NdsRom, NodeId, Query};
use libfuzzer_sys::fuzz_target;

const MAX_FILES: usize = 64;

fuzz_target!(|data: &[u8]| {
    if let Ok(narc) = Narc::parse(data) {
        let _ = narc.len();
        for file in narc.files().take(MAX_FILES).flatten() {
            let _ = FileType::detect(file);
        }
    }
    let Ok(rom) = NdsRom::parse(data) else {
        return;
    };
    let _ = rom.game_code();
    let _ = rom.arm9();
    for (path, id) in rom.named_files().take(MAX_FILES) {
        let _ = rom.file(path);
        let _ = rom.file_by_id(id);
    }
    let Ok(tree) = FileTree::read(&rom) else {
        return;
    };
    let plain = Query::default();
    let search = Query {
        text: String::from("a"),
        ..Query::default()
    };
    let _ = tree.rows(&plain, &HashSet::new());
    let _ = tree.rows(&search, &HashSet::new());
    let _ = tree.found(&search);
    let _ = tree.count(&search);
    let _ = tree.listing(0, &plain);
    let _ = tree.group_counts();
    let _ = tree.file_count();
    let last = tree.len().min(MAX_FILES) as NodeId;
    for id in 0..last {
        let _ = tree.path(id);
        let _ = tree.parent(id);
        let _ = tree.nearest(id, FileType::Nclr);
        if let Ok(bytes) = tree.bytes(&rom, id) {
            let _ = FileType::detect(bytes);
        }
    }
});
