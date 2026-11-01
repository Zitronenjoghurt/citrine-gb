use super::input::{CableEvent, Input};
use crate::gb::GameBoy;
use crate::gb::joypad::JoypadState;
use crate::gb::serial::LinkMessage;
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub(crate) struct Cable {
    pub(crate) plugged: bool,
    pub(crate) clocks: [u64; 2],
    pub(crate) in_flight: [VecDeque<(u64, LinkMessage)>; 2],
    pub(crate) messages: [u64; 2],
    pub(crate) transfers: [u64; 2],
    pub(crate) stalls: [u64; 2],
}

impl Cable {
    pub(crate) fn copy_from(&mut self, source: &Self) {
        let Self {
            plugged,
            clocks,
            in_flight,
            messages,
            transfers,
            stalls,
        } = self;
        *plugged = source.plugged;
        *clocks = source.clocks;
        for (queue, from) in in_flight.iter_mut().zip(&source.in_flight) {
            queue.clear();
            queue.extend(from.iter().copied());
        }
        *messages = source.messages;
        *transfers = source.transfers;
        *stalls = source.stalls;
    }

    fn deliver(&mut self, to: usize, gb: &mut GameBoy) {
        while let Some(&(due, message)) = self.in_flight[to].front() {
            if due > self.clocks[to] {
                break;
            }
            self.in_flight[to].pop_front();
            gb.receive_link_message(message);
        }
    }
}

fn set_buttons(gb: &mut GameBoy, buttons: JoypadState) {
    gb.release_button(buttons.complement());
    gb.press_button(buttons);
}

fn connect(consoles: &mut [&mut GameBoy; 2], cable: &mut Cable) {
    cable.clocks = [0; 2];
    cable.in_flight.iter_mut().for_each(VecDeque::clear);
    consoles.iter_mut().for_each(|gb| gb.plug_link());
}

pub(crate) fn disconnect(consoles: &mut [&mut GameBoy; 2], cable: &mut Cable) {
    cable.in_flight.iter_mut().for_each(VecDeque::clear);
    consoles.iter_mut().for_each(|gb| gb.unplug_link());
}

pub(crate) fn run_frame(
    consoles: &mut [GameBoy; 2],
    cable: &mut Cable,
    inputs: &[Input; 2],
    on: [bool; 2],
) {
    let [one, two] = consoles;
    run_pair([one, two], cable, inputs, on);
}

pub(crate) fn run_pair(
    mut consoles: [&mut GameBoy; 2],
    cable: &mut Cable,
    inputs: &[Input; 2],
    on: [bool; 2],
) {
    for ((gb, input), on) in consoles.iter_mut().zip(inputs).zip(on) {
        if on {
            set_buttons(gb, input.buttons);
        }
    }
    for input in inputs {
        match input.cable {
            Some(CableEvent::Plug) => cable.plugged = true,
            Some(CableEvent::Pull) => cable.plugged = false,
            None => {}
        }
    }
    let linked = cable.plugged && on == [true; 2];
    if linked != consoles[0].link_plugged() {
        if linked {
            connect(&mut consoles, cable);
        } else {
            disconnect(&mut consoles, cable);
        }
    }
    if !linked {
        for (gb, on) in consoles.iter_mut().zip(on) {
            if on {
                gb.run_frame();
            }
        }
        return;
    }

    for gb in consoles.iter_mut() {
        gb.ppu.frame_ready = false;
    }
    let mut done = [false; 2];
    loop {
        let i = match done {
            [true, true] => break,
            [false, true] => 0,
            [true, false] => 1,
            [false, false] => usize::from(cable.clocks[1] < cable.clocks[0]),
        };
        let gb = &mut *consoles[i];
        cable.deliver(i, gb);
        if done[1 - i] && gb.link_waiting() {
            cable.stalls[i] += 1;
            finish_frame(gb);
            done[i] = true;
            continue;
        }

        let before = gb.cycle_counter;
        gb.step();
        let spent = gb.cycle_counter.wrapping_sub(before) as u64;
        cable.clocks[i] += if gb.cgb.double_speed {
            spent
        } else {
            spent * 2
        };
        for message in gb.take_link_messages() {
            cable.messages[i] += 1;
            cable.transfers[i] += u64::from(matches!(message, LinkMessage::Transfer { .. }));
            cable.in_flight[1 - i].push_back((cable.clocks[i], message));
        }

        if gb.ppu.frame_ready || (!gb.ppu.lcdc.lcd_enabled && gb.cycle_counter >= gb.frame_cycles())
        {
            finish_frame(gb);
            done[i] = true;
        }
    }
}

pub(crate) fn run_solo(
    consoles: &mut [GameBoy; 2],
    cable: &mut Cable,
    who: usize,
    input: Input,
    on: bool,
) -> bool {
    let gb = &mut consoles[who];
    if !on {
        return true;
    }
    set_buttons(gb, input.buttons);
    if !gb.link_plugged() {
        gb.run_frame();
        return true;
    }

    gb.ppu.frame_ready = false;
    loop {
        if gb.link_waiting() {
            return false;
        }
        let before = gb.cycle_counter;
        gb.step();
        let spent = gb.cycle_counter.wrapping_sub(before) as u64;
        cable.clocks[who] += if gb.cgb.double_speed {
            spent
        } else {
            spent * 2
        };
        if !gb.take_link_messages().is_empty() {
            return false;
        }
        if gb.ppu.frame_ready || (!gb.ppu.lcdc.lcd_enabled && gb.cycle_counter >= gb.frame_cycles())
        {
            finish_frame(gb);
            return true;
        }
    }
}

fn finish_frame(gb: &mut GameBoy) {
    if gb.cycle_counter >= gb.frame_cycles() {
        gb.cycle_counter -= gb.frame_cycles();
    }
    gb.apu.flush_audio();
}
