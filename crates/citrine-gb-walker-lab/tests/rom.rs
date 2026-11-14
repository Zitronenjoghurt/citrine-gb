use citrine_gb_test_roms as test_roms;
use citrine_walker_lab::harness;
use citrine_walker_lab::walker::{Button, EEPROM_SIZE, Walker};

fn walker() -> Option<Walker> {
    let path = test_roms::env_path("walker", "CITRINE_WALKER_ROM")?;
    let rom = test_roms::read("walker", path)?;
    Some(Walker::new(rom, vec![0xFF; EEPROM_SIZE]))
}

fn steps(w: &mut Walker, at: u16) -> u32 {
    u32::from_be_bytes([0, 1, 2, 3].map(|i| w.read(at + i)))
}

#[test]
fn boots_to_a_drawn_connect_screen() {
    let Some(mut w) = walker() else { return };
    w.run_seconds(8.0).unwrap();
    let frame = w.frame();
    assert!(frame.iter().any(|&p| p != frame[0]));
    assert_eq!(w.watchdog_resets.len(), 0);
}

#[test]
fn identify_gets_the_walker_info() {
    let Some(mut w) = walker() else { return };
    w.run_seconds(8.0).unwrap();
    w.press(Button::Enter).unwrap();
    let id = harness::identify(&mut w).unwrap();
    assert_eq!(id.reply.cmd, 0x22);
    assert_eq!(id.reply.payload.len(), 0x68);
}

#[test]
fn walking_after_a_walk_start_raises_the_step_count() {
    let Some(mut w) = walker() else { return };
    w.run_seconds(8.0).unwrap();
    w.press(Button::Enter).unwrap();
    let cmds = [0x32, 0x38, 0x5A].map(|c| (c, 0x01, Vec::new()));
    let replies = harness::connect(&mut w, &cmds).unwrap();
    assert_eq!(
        replies.iter().map(|r| r.cmd).collect::<Vec<_>>(),
        [0x22, 0x34, 0x38, 0x5A]
    );
    w.run_seconds(4.0).unwrap();
    assert_eq!(steps(&mut w, 0xF780), 0);
    w.walk(60, 2.0);
    w.run_seconds(40.0).unwrap();
    let total = steps(&mut w, 0xF780);
    assert!((50..=70).contains(&total), "{total} steps");
    assert_eq!(steps(&mut w, 0xF79C), total);
}
