pub const W_ID: usize = 0x000;
pub const W_MODE_RST: usize = 0x004;
pub const W_MODE_WEP: usize = 0x006;
pub const W_TXSTATCNT: usize = 0x008;
pub const W_X_00A: usize = 0x00A;
pub const W_IF: usize = 0x010;
pub const W_IE: usize = 0x012;
pub const W_MACADDR: usize = 0x018;
pub const W_BSSID: usize = 0x020;
pub const W_AID_LOW: usize = 0x028;
pub const W_AID_FULL: usize = 0x02A;
pub const W_TX_RETRYLIMIT: usize = 0x02C;
pub const W_INTERNAL_02E: usize = 0x02E;
pub const W_RXCNT: usize = 0x030;
pub const W_WEP_CNT: usize = 0x032;
pub const W_TRX_POWER: usize = 0x034;
pub const W_POWER_US: usize = 0x036;
pub const W_POWER_TX: usize = 0x038;
pub const W_POWERSTATE: usize = 0x03C;
pub const W_POWERFORCE: usize = 0x040;
pub const W_RANDOM: usize = 0x044;
pub const W_POWERDOWNCTRL: usize = 0x048;
pub const W_RXBUF_BEGIN: usize = 0x050;
pub const W_RXBUF_END: usize = 0x052;
pub const W_RXBUF_WRCSR: usize = 0x054;
pub const W_RXBUF_WR_ADDR: usize = 0x056;
pub const W_RXBUF_RD_ADDR: usize = 0x058;
pub const W_RXBUF_READCSR: usize = 0x05A;
pub const W_RXBUF_COUNT: usize = 0x05C;
pub const W_RXBUF_RD_DATA: usize = 0x060;
pub const W_RXBUF_GAP: usize = 0x062;
pub const W_RXBUF_GAPDISP: usize = 0x064;
pub const W_TXBUF_WR_ADDR: usize = 0x068;
pub const W_TXBUF_COUNT: usize = 0x06C;
pub const W_TXBUF_WR_DATA: usize = 0x070;
pub const W_TXBUF_GAP: usize = 0x074;
pub const W_TXBUF_GAPDISP: usize = 0x076;
pub const W_TXBUF_WR_ADDR_MIRROR: usize = 0x078;
pub const W_TXBUF_BEACON: usize = 0x080;
pub const W_TXBUF_TIM: usize = 0x084;
pub const W_LISTENCOUNT: usize = 0x088;
pub const W_BEACONINT: usize = 0x08C;
pub const W_LISTENINT: usize = 0x08E;
pub const W_TXBUF_CMD: usize = 0x090;
pub const W_TXBUF_REPLY1: usize = 0x094;
pub const W_TXBUF_REPLY2: usize = 0x098;
pub const W_TXBUF_LOC1: usize = 0x0A0;
pub const W_TXBUF_LOC2: usize = 0x0A4;
pub const W_TXBUF_LOC3: usize = 0x0A8;
pub const W_TXREQ_RESET: usize = 0x0AC;
pub const W_TXREQ_SET: usize = 0x0AE;
pub const W_TXREQ_READ: usize = 0x0B0;
pub const W_TXBUF_RESET: usize = 0x0B4;
pub const W_TXBUSY: usize = 0x0B6;
pub const W_TXSTAT: usize = 0x0B8;
pub const W_PREAMBLE: usize = 0x0BC;
pub const W_CMD_TOTALTIME: usize = 0x0C0;
pub const W_CMD_REPLYTIME: usize = 0x0C4;
pub const W_RXFILTER: usize = 0x0D0;
pub const W_CONFIG_0D4: usize = 0x0D4;
pub const W_RX_LEN_CROP: usize = 0x0DA;
pub const W_RXFILTER2: usize = 0x0E0;
pub const W_US_COUNTCNT: usize = 0x0E8;
pub const W_US_COMPARECNT: usize = 0x0EA;
pub const W_CONFIG_0EC: usize = 0x0EC;
pub const W_CMD_COUNTCNT: usize = 0x0EE;
pub const W_US_COMPARE0: usize = 0x0F0;
pub const W_US_COMPARE3: usize = 0x0F6;
pub const W_US_COUNT0: usize = 0x0F8;
pub const W_US_COUNT3: usize = 0x0FE;
pub const W_CONTENTFREE: usize = 0x10C;
pub const W_PRE_BEACON: usize = 0x110;
pub const W_CMD_COUNT: usize = 0x118;
pub const W_BEACON_COUNT: usize = 0x11C;
pub const W_POST_BEACON: usize = 0x134;
pub const W_BB_CNT: usize = 0x158;
pub const W_BB_WRITE: usize = 0x15A;
pub const W_BB_READ: usize = 0x15C;
pub const W_BB_BUSY: usize = 0x15E;
pub const W_BB_POWER: usize = 0x168;
pub const W_RF_DATA2_MIRROR: usize = 0x178;
pub const W_RF_DATA2: usize = 0x17C;
pub const W_RF_DATA1: usize = 0x17E;
pub const W_RF_BUSY: usize = 0x180;
pub const W_RF_CNT: usize = 0x184;
pub const W_TX_HDR_CNT: usize = 0x194;
pub const W_INTERNAL_198: usize = 0x198;
pub const W_RF_PINS: usize = 0x19C;
pub const W_X_1A2: usize = 0x1A2;
pub const W_X_1A4: usize = 0x1A4;
pub const W_RXSTAT_INC_IF: usize = 0x1A8;
pub const W_RXSTAT_OVF_IF: usize = 0x1AC;
pub const W_RXSTAT: usize = 0x1B0;
pub const W_RXSTAT_LAST: usize = 0x1BE;
pub const W_RX_COUNT: usize = 0x1C4;
pub const W_CMD_STAT: usize = 0x1D0;
pub const W_CMD_STAT_LAST: usize = 0x1DE;
pub const W_TX_SEQNO: usize = 0x210;
pub const W_RF_STATUS: usize = 0x214;
pub const W_IF_SET: usize = 0x21C;
pub const W_INTERNAL_224: usize = 0x224;
pub const W_INTERNAL_230: usize = 0x230;
pub const W_RXTX_ADDR: usize = 0x268;
pub const W_INTERNAL_278: usize = 0x278;
pub const W_INTERNAL_27C: usize = 0x27C;

pub const RESET: [(usize, u16); 61] = [
    (W_TX_RETRYLIMIT, 0x0707),
    (W_POWER_US, 0x0001),
    (W_POWER_TX, 0x0003),
    (W_POWERSTATE, 0x0200),
    (W_RXBUF_BEGIN, 0x4000),
    (W_RXBUF_END, 0x4800),
    (W_BEACONINT, 0x0064),
    (0x09C, 0x0050),
    (W_TXREQ_RESET, 0x0050),
    (W_TXREQ_SET, 0x0050),
    (W_TXREQ_READ, 0x0010),
    (W_PREAMBLE, 0x0001),
    (W_RXFILTER, 0x0401),
    (W_CONFIG_0D4, 0x0001),
    (0x0D8, 0x0004),
    (W_RX_LEN_CROP, 0x0602),
    (W_RXFILTER2, 0x0008),
    (W_CONFIG_0EC, 0x3F03),
    (W_CMD_COUNTCNT, 0x0001),
    (W_US_COMPARE0, 0xFC00),
    (0x0F2, 0xFFFF),
    (0x0F4, 0xFFFF),
    (W_US_COMPARE3, 0xFFFF),
    (0x120, 0x0048),
    (0x122, 0x4840),
    (0x126, 0x0080),
    (0x12A, 0x1000),
    (0x130, 0x0142),
    (0x132, 0x8064),
    (W_POST_BEACON, 0xFFFF),
    (0x142, 0x2443),
    (0x144, 0x0042),
    (0x146, 0x0016),
    (0x148, 0x0016),
    (0x14A, 0x0016),
    (0x14C, 0x162C),
    (0x150, 0x0204),
    (0x154, 0x0058),
    (W_BB_CNT, 0x00B5),
    (W_BB_READ, 0x00B5),
    (0x160, 0x0100),
    (W_BB_POWER, 0x800D),
    (0x16A, 0x0001),
    (W_RF_DATA2, 0x0800),
    (W_RF_DATA1, 0xC008),
    (W_RF_CNT, 0x0018),
    (W_RF_PINS, 0x0004),
    (W_X_1A2, 0x0001),
    (0x20C, 0x0050),
    (W_RF_STATUS, 0x0009),
    (W_INTERNAL_224, 0x0003),
    (W_INTERNAL_230, 0x0047),
    (0x234, 0x0EFF),
    (0x254, 0xFFFF),
    (0x260, 0x0FEF),
    (W_RXTX_ADDR, 0x0005),
    (0x274, 0x0001),
    (W_INTERNAL_278, 0x000F),
    (W_INTERNAL_27C, 0x000A),
    (0x290, 0xFFFF),
    (0x2A2, 0x7FFF),
];

pub const READ_ONLY: [usize; 18] = [
    W_ID,
    W_TRX_POWER,
    W_RANDOM,
    W_RXBUF_WRCSR,
    W_TXBUF_REPLY2,
    W_TXREQ_READ,
    W_TXBUSY,
    W_TXSTAT,
    W_BB_READ,
    W_BB_BUSY,
    W_RF_BUSY,
    W_RF_PINS,
    W_RXSTAT_INC_IF,
    W_RXSTAT_OVF_IF,
    W_RX_COUNT,
    W_TX_SEQNO,
    W_RF_STATUS,
    W_RXTX_ADDR,
];

pub const WRITE_MASKS: [(usize, u16); 37] = [
    (W_MODE_WEP, 0x007F),
    (W_AID_LOW, 0x000F),
    (W_AID_FULL, 0x07FF),
    (W_POWER_US, 0x0003),
    (W_POWER_TX, 0x0007),
    (W_POWERFORCE, 0x8001),
    (W_POWERDOWNCTRL, 0x0003),
    (W_RXBUF_WR_ADDR, 0x0FFF),
    (W_RXBUF_RD_ADDR, 0x1FFE),
    (W_RXBUF_READCSR, 0x0FFF),
    (W_RXBUF_COUNT, 0x0FFF),
    (W_RXBUF_GAP, 0x1FFE),
    (W_RXBUF_GAPDISP, 0x0FFF),
    (W_TXBUF_WR_ADDR, 0x1FFE),
    (W_TXBUF_COUNT, 0x0FFF),
    (W_TXBUF_GAP, 0x1FFE),
    (W_TXBUF_GAPDISP, 0x0FFF),
    (W_TXBUF_TIM, 0x00FF),
    (W_LISTENCOUNT, 0x00FF),
    (W_BEACONINT, 0x03FF),
    (W_LISTENINT, 0x00FF),
    (W_PREAMBLE, 0x0007),
    (W_US_COUNTCNT, 0x0001),
    (W_US_COMPARECNT, 0x0001),
    (W_CMD_COUNTCNT, 0x0001),
    (W_US_COMPARE0, 0xFC00),
    (0x120, 0x81FF),
    (0x130, 0x0FFF),
    (0x132, 0x8FFF),
    (0x144, 0x00FF),
    (0x146, 0x00FF),
    (0x148, 0x00FF),
    (0x14A, 0x00FF),
    (0x150, 0xFF3F),
    (0x154, 0x7A7F),
    (W_RF_CNT, 0x413F),
    (W_MODE_RST, 0x9FFF),
];

pub fn write_mask(offset: usize) -> u16 {
    WRITE_MASKS
        .iter()
        .find(|&&(at, _)| at == offset)
        .map_or(0xFFFF, |&(_, mask)| mask)
}
