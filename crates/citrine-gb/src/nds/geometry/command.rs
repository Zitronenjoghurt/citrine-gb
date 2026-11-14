pub const NOP: u8 = 0x00;
pub const MTX_MODE: u8 = 0x10;
pub const MTX_PUSH: u8 = 0x11;
pub const MTX_POP: u8 = 0x12;
pub const MTX_STORE: u8 = 0x13;
pub const MTX_RESTORE: u8 = 0x14;
pub const MTX_IDENTITY: u8 = 0x15;
pub const MTX_LOAD_4X4: u8 = 0x16;
pub const MTX_LOAD_4X3: u8 = 0x17;
pub const MTX_MULT_4X4: u8 = 0x18;
pub const MTX_MULT_4X3: u8 = 0x19;
pub const MTX_MULT_3X3: u8 = 0x1A;
pub const MTX_SCALE: u8 = 0x1B;
pub const MTX_TRANS: u8 = 0x1C;
pub const COLOR: u8 = 0x20;
pub const NORMAL: u8 = 0x21;
pub const TEXCOORD: u8 = 0x22;
pub const VTX_16: u8 = 0x23;
pub const VTX_10: u8 = 0x24;
pub const VTX_XY: u8 = 0x25;
pub const VTX_XZ: u8 = 0x26;
pub const VTX_YZ: u8 = 0x27;
pub const VTX_DIFF: u8 = 0x28;
pub const POLYGON_ATTR: u8 = 0x29;
pub const TEXIMAGE_PARAM: u8 = 0x2A;
pub const PLTT_BASE: u8 = 0x2B;
pub const DIF_AMB: u8 = 0x30;
pub const SPE_EMI: u8 = 0x31;
pub const LIGHT_VECTOR: u8 = 0x32;
pub const LIGHT_COLOR: u8 = 0x33;
pub const SHININESS: u8 = 0x34;
pub const BEGIN_VTXS: u8 = 0x40;
pub const END_VTXS: u8 = 0x41;
pub const SWAP_BUFFERS: u8 = 0x50;
pub const VIEWPORT: u8 = 0x60;
pub const BOX_TEST: u8 = 0x70;
pub const POS_TEST: u8 = 0x71;
pub const VEC_TEST: u8 = 0x72;

const PARAMETERS: [u8; 256] = {
    let mut table = [0; 256];
    let mut command = 0;
    while command < table.len() {
        table[command] = listed_parameters(command as u8);
        command += 1;
    }
    table
};

pub fn parameters(command: u8) -> usize {
    usize::from(PARAMETERS[usize::from(command)])
}

const fn listed_parameters(command: u8) -> u8 {
    match command {
        MTX_MODE | MTX_POP | MTX_STORE | MTX_RESTORE => 1,
        MTX_LOAD_4X4 | MTX_MULT_4X4 => 16,
        MTX_LOAD_4X3 | MTX_MULT_4X3 => 12,
        MTX_MULT_3X3 => 9,
        MTX_SCALE | MTX_TRANS | BOX_TEST => 3,
        VTX_16 | POS_TEST => 2,
        COLOR | NORMAL | TEXCOORD | VTX_10..=PLTT_BASE | DIF_AMB..=LIGHT_COLOR => 1,
        SHININESS => 32,
        BEGIN_VTXS | SWAP_BUFFERS | VIEWPORT | VEC_TEST => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_counts_follow_gbatek() {
        let expected = [
            (MTX_PUSH, 0),
            (MTX_IDENTITY, 0),
            (END_VTXS, 0),
            (MTX_POP, 1),
            (MTX_LOAD_4X4, 16),
            (MTX_MULT_4X3, 12),
            (MTX_MULT_3X3, 9),
            (MTX_TRANS, 3),
            (VTX_16, 2),
            (VTX_DIFF, 1),
            (PLTT_BASE, 1),
            (LIGHT_COLOR, 1),
            (SHININESS, 32),
            (BEGIN_VTXS, 1),
            (SWAP_BUFFERS, 1),
            (VIEWPORT, 1),
            (BOX_TEST, 3),
            (POS_TEST, 2),
            (VEC_TEST, 1),
        ];
        for (command, count) in expected {
            assert_eq!(parameters(command), count, "command {command:02X}");
        }
    }

    #[test]
    fn invalid_commands_take_no_parameters() {
        for command in [0x00, 0x1D, 0x2C, 0x35, 0x42, 0x51, 0x61, 0x73, 0xFF] {
            assert_eq!(parameters(command), 0, "command {command:02X}");
        }
    }
}
