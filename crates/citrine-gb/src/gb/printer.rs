pub const PRINT_WIDTH: usize = 160;
const BUFFER_LIMIT: usize = 0x2000;
const BAND_BYTES: usize = PRINT_WIDTH / 8 * 16 * 2;
const BUSY_POLLS: u8 = 4;

const STATUS_CHECKSUM: u8 = 0x01;
const STATUS_PRINTING: u8 = 0x02;
const STATUS_FULL: u8 = 0x04;
const STATUS_UNPROCESSED: u8 = 0x08;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PrintedImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub margin_before: u8,
    pub margin_after: u8,
    pub exposure: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Stage {
    Magic1,
    Magic2,
    Command,
    Compression,
    LengthLow,
    LengthHigh,
    Data,
    ChecksumLow,
    ChecksumHigh,
    Ack,
    Status,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Printer {
    stage: Stage,
    command: u8,
    compressed: bool,
    length: u16,
    data: Vec<u8>,
    sum: u16,
    checksum: u16,
    status: u8,
    buffer: Vec<u8>,
    busy_polls: u8,
    after_status: Option<u8>,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    prints: Vec<PrintedImage>,
}

impl Default for Printer {
    fn default() -> Self {
        Self {
            stage: Stage::Magic1,
            command: 0,
            compressed: false,
            length: 0,
            data: Vec::new(),
            sum: 0,
            checksum: 0,
            status: 0,
            buffer: Vec::new(),
            busy_polls: 0,
            after_status: None,
            prints: Vec::new(),
        }
    }
}

impl Printer {
    pub fn take_prints(&mut self) -> Vec<PrintedImage> {
        std::mem::take(&mut self.prints)
    }

    pub fn exchange(&mut self, byte: u8) -> u8 {
        let reply = match self.stage {
            Stage::Ack => 0x81,
            Stage::Status => self.status,
            _ => 0x00,
        };
        self.receive(byte);
        reply
    }

    fn receive(&mut self, byte: u8) {
        self.stage = match self.stage {
            Stage::Magic1 if byte == 0x88 => Stage::Magic2,
            Stage::Magic1 => Stage::Magic1,
            Stage::Magic2 if byte == 0x33 => {
                self.sum = 0;
                self.data.clear();
                Stage::Command
            }
            Stage::Magic2 if byte == 0x88 => Stage::Magic2,
            Stage::Magic2 => Stage::Magic1,
            Stage::Command => {
                self.add(byte);
                self.command = byte;
                Stage::Compression
            }
            Stage::Compression => {
                self.add(byte);
                self.compressed = byte & 0x01 != 0;
                Stage::LengthLow
            }
            Stage::LengthLow => {
                self.add(byte);
                self.length = byte as u16;
                Stage::LengthHigh
            }
            Stage::LengthHigh => {
                self.add(byte);
                self.length |= (byte as u16) << 8;
                if self.length == 0 {
                    Stage::ChecksumLow
                } else {
                    Stage::Data
                }
            }
            Stage::Data => {
                self.add(byte);
                self.data.push(byte);
                if self.data.len() >= self.length as usize {
                    Stage::ChecksumLow
                } else {
                    Stage::Data
                }
            }
            Stage::ChecksumLow => {
                self.checksum = byte as u16;
                Stage::ChecksumHigh
            }
            Stage::ChecksumHigh => {
                self.checksum |= (byte as u16) << 8;
                self.run();
                Stage::Ack
            }
            Stage::Ack => Stage::Status,
            Stage::Status => {
                if let Some(status) = self.after_status.take() {
                    self.status = status;
                }
                Stage::Magic1
            }
        };
    }

    fn add(&mut self, byte: u8) {
        self.sum = self.sum.wrapping_add(byte as u16);
    }

    fn run(&mut self) {
        if self.checksum != self.sum {
            self.status |= STATUS_CHECKSUM;
            return;
        }
        self.status &= !STATUS_CHECKSUM;
        match self.command {
            0x01 => {
                self.buffer.clear();
                self.busy_polls = 0;
                self.after_status = None;
                self.status = 0;
            }
            0x02 => self.print(),
            0x04 => {
                let data = if self.compressed {
                    decompress(&self.data)
                } else {
                    std::mem::take(&mut self.data)
                };
                let room = BUFFER_LIMIT - self.buffer.len();
                self.buffer.extend_from_slice(&data[..data.len().min(room)]);
                if !self.buffer.is_empty() {
                    self.after_status = Some(self.status | STATUS_UNPROCESSED);
                }
            }
            0x08 => {
                self.buffer.clear();
                self.busy_polls = 0;
                self.after_status = None;
                self.status &= !(STATUS_PRINTING | STATUS_UNPROCESSED);
            }
            0x0F if self.busy_polls > 0 => {
                self.busy_polls -= 1;
                if self.busy_polls == 0 {
                    self.status &= !STATUS_PRINTING;
                }
            }
            _ => {}
        }
    }

    fn print(&mut self) {
        let [sheets, margins, palette, exposure] = match self.data.as_slice() {
            [a, b, c, d, ..] => [*a, *b, *c, *d],
            _ => return,
        };
        let bands = self.buffer.len() / BAND_BYTES;
        if sheets > 0 && bands > 0 {
            let height = bands * 16;
            let mut pixels = vec![0u8; PRINT_WIDTH * height];
            for (tile_index, tile) in self.buffer.as_chunks::<16>().0.iter().enumerate() {
                let tile_x = tile_index % (PRINT_WIDTH / 8);
                let tile_y = tile_index / (PRINT_WIDTH / 8);
                if tile_y * 8 >= height {
                    break;
                }
                for row in 0..8 {
                    let (low, high) = (tile[row * 2], tile[row * 2 + 1]);
                    for column in 0..8 {
                        let bit = 7 - column;
                        let colour = ((low >> bit) & 1) | (((high >> bit) & 1) << 1);
                        let shade = (palette >> (colour * 2)) & 0x03;
                        let x = tile_x * 8 + column;
                        let y = tile_y * 8 + row;
                        pixels[y * PRINT_WIDTH + x] = 255 - shade * 85;
                    }
                }
            }
            self.prints.push(PrintedImage {
                width: PRINT_WIDTH,
                height,
                pixels,
                margin_before: margins >> 4,
                margin_after: margins & 0x0F,
                exposure: exposure & 0x7F,
            });
        }
        self.buffer.clear();
        self.busy_polls = BUSY_POLLS;
        self.after_status =
            Some((self.status & !STATUS_UNPROCESSED) | STATUS_PRINTING | STATUS_FULL);
    }
}

pub fn decompress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let control = data[at];
        at += 1;
        if control & 0x80 != 0 {
            let Some(&byte) = data.get(at) else { break };
            at += 1;
            out.extend(std::iter::repeat_n(byte, (control & 0x7F) as usize + 2));
        } else {
            let end = (at + (control & 0x7F) as usize + 1).min(data.len());
            out.extend_from_slice(&data[at..end]);
            at = end;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(command: u8, compressed: bool, data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x88, 0x33, command, u8::from(compressed)];
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(data);
        let sum = bytes[2..]
            .iter()
            .fold(0u16, |sum, &byte| sum.wrapping_add(byte as u16));
        bytes.extend_from_slice(&sum.to_le_bytes());
        bytes.extend_from_slice(&[0x00, 0x00]);
        bytes
    }

    fn send(printer: &mut Printer, bytes: &[u8]) -> (u8, u8) {
        let replies: Vec<u8> = bytes.iter().map(|&byte| printer.exchange(byte)).collect();
        assert!(replies[..replies.len() - 2].iter().all(|&r| r == 0x00));
        (replies[replies.len() - 2], replies[replies.len() - 1])
    }

    #[test]
    fn detection_packet_answers_81_and_status() {
        let mut printer = Printer::default();
        let bytes = [0x88, 0x33, 0x0F, 0x00, 0x00, 0x00, 0x0F, 0x00, 0x00, 0x00];
        assert_eq!(send(&mut printer, &bytes), (0x81, 0x00));
    }

    #[test]
    fn bad_checksum_sets_bit_zero() {
        let mut printer = Printer::default();
        let mut bytes = packet(0x01, false, &[]);
        bytes[6] ^= 0xFF;
        send(&mut printer, &bytes);
        assert_eq!(send(&mut printer, &packet(0x0F, false, &[])).1 & 0x01, 0);
        send(&mut printer, &bytes);
        assert_eq!(printer.status & STATUS_CHECKSUM, STATUS_CHECKSUM);
    }

    #[test]
    fn the_documented_print_sequence() {
        let mut printer = Printer::default();
        assert_eq!(send(&mut printer, &packet(0x01, false, &[])), (0x81, 0x00));
        let band = [0xFFu8; BAND_BYTES];
        assert_eq!(
            send(&mut printer, &packet(0x04, false, &band)),
            (0x81, 0x00)
        );
        assert_eq!(send(&mut printer, &packet(0x0F, false, &[])), (0x81, 0x08));
        assert_eq!(send(&mut printer, &packet(0x04, false, &[])), (0x81, 0x08));
        let print = [0x01, 0x13, 0xE4, 0x40];
        assert_eq!(
            send(&mut printer, &packet(0x02, false, &print)),
            (0x81, 0x08)
        );
        let mut polls = Vec::new();
        for _ in 0..6 {
            polls.push(send(&mut printer, &packet(0x0F, false, &[])).1);
        }
        assert_eq!(polls[0], 0x06);
        assert_eq!(*polls.last().unwrap(), 0x04);
        let prints = printer.take_prints();
        assert_eq!(prints.len(), 1);
        assert_eq!((prints[0].width, prints[0].height), (160, 16));
        assert_eq!((prints[0].margin_before, prints[0].margin_after), (1, 3));
        assert!(prints[0].pixels.iter().all(|&p| p == 0));
        assert!(printer.take_prints().is_empty());
    }

    #[test]
    fn tiles_land_in_tilemap_order_through_the_palette() {
        let mut printer = Printer::default();
        let mut band = vec![0u8; BAND_BYTES];
        band[16] = 0x80;
        band[20 * 16 + 1] = 0x01;
        send(&mut printer, &packet(0x04, false, &band));
        send(&mut printer, &packet(0x02, false, &[1, 0, 0xE4, 0]));
        let print = &printer.take_prints()[0];
        assert_eq!(print.pixels[8], 255 - 85);
        assert_eq!(print.pixels[9], 255);
        assert_eq!(print.pixels[8 * 160 + 7], 255 - 170);
    }

    #[test]
    fn zero_sheets_feeds_paper_without_an_image() {
        let mut printer = Printer::default();
        send(&mut printer, &packet(0x04, false, &[0u8; BAND_BYTES]));
        send(&mut printer, &packet(0x02, false, &[0, 0, 0xE4, 0]));
        assert!(printer.take_prints().is_empty());
    }

    #[test]
    fn compressed_data_is_expanded() {
        assert_eq!(
            decompress(&[0x82, 0xAA, 0x01, 0x11, 0x22]),
            vec![0xAA, 0xAA, 0xAA, 0xAA, 0x11, 0x22]
        );
        let mut printer = Printer::default();
        let data = [
            0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
        ];
        let mut packed = Vec::new();
        for _ in 0..BAND_BYTES / 128 {
            packed.extend_from_slice(&[0xFF, 0x00]);
        }
        packed.extend_from_slice(&[0x0B]);
        packed.extend_from_slice(&data);
        send(&mut printer, &packet(0x04, true, &packed));
        assert_eq!(printer.buffer.len(), BAND_BYTES / 128 * 129 + 12);
    }

    #[test]
    fn init_clears_the_buffer() {
        let mut printer = Printer::default();
        send(&mut printer, &packet(0x04, false, &[1u8; 32]));
        send(&mut printer, &packet(0x01, false, &[]));
        assert!(printer.buffer.is_empty());
        assert_eq!(printer.status, 0);
    }
}
