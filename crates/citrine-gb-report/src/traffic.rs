use citrine_gb::traffic::{Entry, Ring};
use std::fmt::Write;

pub fn traffic_text(rings: &[Ring<'_>]) -> String {
    let mut lines: Vec<(i64, u64, &'static str, Entry)> = Vec::new();
    for ring in rings {
        let now = ring.traffic.frame();
        let age = |entry: &Entry| entry.frame as i64 - now as i64;
        let open = ring.traffic.open();
        for entry in ring.traffic.entries().chain(open.iter()) {
            lines.push((age(entry), entry.at, ring.name, *entry));
        }
    }
    lines.sort_by_key(|&(frame, at, _, _)| (frame, at));
    let mut text = String::from("frame  device  event\n");
    for (frame, _, name, entry) in lines {
        let _ = writeln!(text, "{frame:>6}  {name:<6}  {}", entry.describe());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::traffic::{Kind, Traffic};

    #[test]
    fn rings_merge_in_frame_and_clock_order_with_frames_counted_back() {
        let mut aux = Traffic::default();
        let mut ipc = Traffic::default();
        aux.push(Entry::new(Kind::AuxControl).at(5).sent(&[0x40, 0xA0]));
        ipc.push(Entry::new(Kind::IpcSend).at(3).word(0x1234));
        aux.next_frame();
        ipc.next_frame();
        ipc.push(Entry::new(Kind::IpcSync).tag(1).at(9).word(0x0100));
        aux.byte(Kind::AuxSpi, 0, 10, 0x05, 0x00);
        let text = traffic_text(&[
            Ring {
                name: "auxspi",
                traffic: &aux,
            },
            Ring {
                name: "ipc",
                traffic: &ipc,
            },
        ]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], "    -1  ipc     ipc arm9 sends 0x00001234");
        assert_eq!(lines[2], "    -1  auxspi  auxspicnt 0xa040");
        assert_eq!(lines[3], "     0  ipc     ipc arm7 sync 0x0100");
        assert_eq!(
            lines[4],
            "     0  auxspi  auxspi 05 -> 00 (1 bytes, ended by still selected)"
        );
    }
}
