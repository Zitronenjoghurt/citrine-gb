use citrine_gb::nds::ir::{IrChip, IrPort, IrVersion};
use citrine_gb::nds::save::{Backup, SaveType};
use citrine_gb_walker::{CYCLES_PER_SECOND, Event, Link, Walker};

struct Port<'a>(&'a mut Walker);

impl IrPort for Port<'_> {
    fn send(&mut self, packet: &[u8], at: u64) {
        self.0.send(packet, at);
    }

    fn take(&mut self, now: u64) -> Option<Vec<u8>> {
        self.0.take(now)
    }
}

fn send(chip: &mut IrChip, walker: &mut Walker, packet: &[u8], now: u64) {
    let mut port = Port(walker);
    for byte in [&[0x02][..], packet].concat() {
        chip.transfer(byte, &mut port, now);
    }
    chip.deselect(&mut port, now);
}

fn receive(chip: &mut IrChip, walker: &mut Walker, now: u64) -> Vec<u8> {
    let mut port = Port(walker);
    chip.transfer(0x01, &mut port, now);
    let len = chip.transfer(0, &mut port, now) as usize;
    let data = (0..len).map(|_| chip.transfer(0, &mut port, now)).collect();
    chip.deselect(&mut port, now);
    data
}

fn poll(chip: &mut IrChip, walker: &mut Walker, now: &mut u64) -> Vec<u8> {
    for _ in 0..1000 {
        *now += CYCLES_PER_SECOND / 1000;
        let data = receive(chip, walker, *now);
        if !data.is_empty() {
            return data;
        }
    }
    panic!("nothing came in");
}

#[test]
fn heartgold_s_ir_chip_reaches_the_walker_through_the_port() {
    let mut chip = IrChip::new(
        IrVersion::for_game_code(*b"IPKE").unwrap(),
        Backup::new(SaveType::Flash512K),
    );
    let mut walker = Walker::new(5);
    let mut now = CYCLES_PER_SECOND;
    walker.connect(now);
    assert_eq!(poll(&mut chip, &mut walker, &mut now), vec![0x56]);

    let assert_master = [0x50, 0xAB, 0xC0, 0x94, 0xBB, 0x88, 0x99, 0xEE];
    send(&mut chip, &mut walker, &assert_master, now);
    let ack = poll(&mut chip, &mut walker, &mut now);
    assert_eq!(ack.len(), 8);
    assert_eq!([ack[0] ^ 0xAA, ack[1] ^ 0xAA], [0xF8, 0x02]);
    assert_eq!(walker.link(), Link::Connected);
    assert_eq!(walker.events(), vec![Event::Connected]);
}
