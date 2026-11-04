use super::input::{CableEvent, Input};
use crate::console::Console;
use crate::error::{GbError, GbResult};
use std::collections::VecDeque;

pub trait LinkPort {
    type Message: Copy + serde::Serialize + serde::de::DeserializeOwned;

    fn plug_link(&mut self);
    fn plug_link_as(&mut self, _unit: usize, _units: usize) {
        self.plug_link();
    }
    fn unplug_link(&mut self);
    fn link_plugged(&self) -> bool;
    fn link_waiting(&self) -> bool;
    fn take_link_messages(&mut self) -> Vec<Self::Message>;
    fn receive_link_message(&mut self, message: Self::Message);
    fn is_transfer(message: &Self::Message) -> bool;
    fn begin_linked_frame(&mut self) -> u64;
    fn step_linked(&mut self) -> u64;
    fn linked_frame_done(&self) -> bool;
    fn end_linked_frame(&mut self);
}

#[derive(Debug)]
pub(crate) struct Cable<M, const N: usize = 2> {
    pub(crate) plugged: bool,
    pub(crate) clocks: [u64; N],
    pub(crate) in_flight: [VecDeque<(u64, M)>; N],
    pub(crate) messages: [u64; N],
    pub(crate) transfers: [u64; N],
    pub(crate) stalls: [u64; N],
}

impl<M, const N: usize> Default for Cable<M, N> {
    fn default() -> Self {
        Self {
            plugged: false,
            clocks: [0; N],
            in_flight: std::array::from_fn(|_| VecDeque::new()),
            messages: [0; N],
            transfers: [0; N],
            stalls: [0; N],
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CableWire<M> {
    plugged: bool,
    clocks: Vec<u64>,
    in_flight: Vec<Vec<(u64, M)>>,
    messages: Vec<u64>,
    transfers: Vec<u64>,
    stalls: Vec<u64>,
}

impl<M: Copy, const N: usize> Cable<M, N> {
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

    pub(crate) fn encode(&self) -> GbResult<Vec<u8>>
    where
        M: serde::Serialize,
    {
        let wire = CableWire {
            plugged: self.plugged,
            clocks: self.clocks.to_vec(),
            in_flight: self
                .in_flight
                .iter()
                .map(|q| q.iter().copied().collect())
                .collect(),
            messages: self.messages.to_vec(),
            transfers: self.transfers.to_vec(),
            stalls: self.stalls.to_vec(),
        };
        Ok(rmp_serde::to_vec(&wire)?)
    }

    pub(crate) fn decode(&mut self, data: &[u8]) -> GbResult<()>
    where
        M: serde::de::DeserializeOwned,
    {
        let wire: CableWire<M> = rmp_serde::from_slice(data)?;
        let fits = [&wire.clocks, &wire.messages, &wire.transfers, &wire.stalls]
            .iter()
            .all(|list| list.len() == N)
            && wire.in_flight.len() == N;
        if !fits {
            return Err(GbError::IO(std::io::ErrorKind::InvalidData.into()));
        }
        self.plugged = wire.plugged;
        for i in 0..N {
            self.clocks[i] = wire.clocks[i];
            self.messages[i] = wire.messages[i];
            self.transfers[i] = wire.transfers[i];
            self.stalls[i] = wire.stalls[i];
            self.in_flight[i] = wire.in_flight[i].iter().copied().collect();
        }
        Ok(())
    }

    pub(crate) fn reseat(&mut self) {
        self.clocks = [0; N];
        self.in_flight.iter_mut().for_each(VecDeque::clear);
    }

    pub(crate) fn quiet(&self) -> bool {
        self.in_flight.iter().all(VecDeque::is_empty)
    }

    fn deliver<C: LinkPort<Message = M>>(&mut self, to: usize, console: &mut C) {
        while let Some(&(due, message)) = self.in_flight[to].front() {
            if due > self.clocks[to] {
                break;
            }
            self.in_flight[to].pop_front();
            console.receive_link_message(message);
        }
    }

    fn ends(&self, on: [bool; N]) -> [bool; N] {
        let ends = on.map(|on| on && self.plugged);
        if ends.iter().filter(|&&end| end).count() >= 2 {
            ends
        } else {
            [false; N]
        }
    }
}

fn connect<C: LinkPort, const N: usize>(
    consoles: &mut [&mut C; N],
    cable: &mut Cable<C::Message, N>,
    ends: [bool; N],
) {
    cable.reseat();
    for console in consoles.iter_mut().filter(|console| console.link_plugged()) {
        console.unplug_link();
    }
    let units = ends.iter().filter(|&&end| end).count();
    let attached = consoles.iter_mut().zip(ends).filter(|(_, end)| *end);
    for (unit, (console, _)) in attached.enumerate() {
        console.plug_link_as(unit, units);
    }
}

pub(crate) fn disconnect<C: LinkPort, const N: usize>(
    consoles: &mut [&mut C; N],
    cable: &mut Cable<C::Message, N>,
) {
    cable.in_flight.iter_mut().for_each(VecDeque::clear);
    consoles.iter_mut().for_each(|gb| gb.unplug_link());
}

pub(crate) fn run_frame<C: Console + LinkPort, const N: usize>(
    consoles: &mut [C; N],
    cable: &mut Cable<C::Message, N>,
    inputs: &[Input; N],
    on: [bool; N],
) {
    run_linked(consoles.each_mut(), cable, inputs, on);
}

pub(crate) fn run_pair<C: Console + LinkPort>(
    consoles: [&mut C; 2],
    cable: &mut Cable<C::Message>,
    inputs: &[Input; 2],
    on: [bool; 2],
) {
    run_linked(consoles, cable, inputs, on);
}

pub(crate) fn run_linked<C: Console + LinkPort, const N: usize>(
    mut consoles: [&mut C; N],
    cable: &mut Cable<C::Message, N>,
    inputs: &[Input; N],
    on: [bool; N],
) {
    for ((gb, input), on) in consoles.iter_mut().zip(inputs).zip(on) {
        if on {
            gb.set_keys(input.buttons);
        }
    }
    for input in inputs {
        match input.cable {
            Some(CableEvent::Plug) => cable.plugged = true,
            Some(CableEvent::Pull) => cable.plugged = false,
            None => {}
        }
    }
    let ends = cable.ends(on);
    let moved = consoles
        .iter()
        .zip(ends)
        .any(|(gb, end)| gb.link_plugged() != end);
    if moved {
        if ends.contains(&true) {
            connect(&mut consoles, cable, ends);
        } else {
            disconnect(&mut consoles, cable);
        }
    }
    for ((gb, on), end) in consoles.iter_mut().zip(on).zip(ends) {
        if on && !end {
            gb.run_frame();
        }
    }
    if !ends.contains(&true) {
        return;
    }

    for (i, gb) in consoles.iter_mut().enumerate().filter(|&(i, _)| ends[i]) {
        cable.clocks[i] += gb.begin_linked_frame();
    }
    let mut done = ends.map(|end| !end);
    while let Some(i) = (0..N)
        .filter(|&i| !done[i])
        .min_by_key(|&i| cable.clocks[i])
    {
        let gb = &mut *consoles[i];
        cable.deliver(i, gb);
        let alone = (0..N).all(|j| j == i || done[j]);
        if alone && gb.link_waiting() {
            cable.stalls[i] += 1;
            gb.end_linked_frame();
            done[i] = true;
            continue;
        }

        cable.clocks[i] += gb.step_linked();
        for message in gb.take_link_messages() {
            cable.messages[i] += 1;
            cable.transfers[i] += u64::from(C::is_transfer(&message));
            for to in (0..N).filter(|&to| to != i && ends[to]) {
                cable.in_flight[to].push_back((cable.clocks[i], message));
            }
        }

        if gb.linked_frame_done() {
            gb.end_linked_frame();
            done[i] = true;
        }
    }
}

pub(crate) fn run_solo<C: Console + LinkPort, const N: usize>(
    consoles: &mut [C; N],
    cable: &mut Cable<C::Message, N>,
    who: usize,
    input: Input,
    on: bool,
) -> bool {
    let gb = &mut consoles[who];
    if !on {
        return true;
    }
    gb.set_keys(input.buttons);
    if !gb.link_plugged() {
        gb.run_frame();
        return true;
    }

    cable.clocks[who] += gb.begin_linked_frame();
    loop {
        if gb.link_waiting() {
            return false;
        }
        cable.clocks[who] += gb.step_linked();
        if !gb.take_link_messages().is_empty() {
            return false;
        }
        if gb.linked_frame_done() {
            gb.end_linked_frame();
            return true;
        }
    }
}
