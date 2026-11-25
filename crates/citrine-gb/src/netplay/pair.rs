use super::input::{CableEvent, CablePace, Input};
use crate::console::Console;
use crate::error::{GbError, GbResult};
use citrine_gb_threads::{Split, Turnout};
use std::collections::VecDeque;

pub const WINDOWED_UNITS: usize = 3;

pub trait LinkPort {
    type Message: Clone + Send + serde::Serialize + serde::de::DeserializeOwned;
    const CABLE_WINDOWS: bool = false;
    const STAMPS_AT_STEP_START: bool = false;
    const AIR_ROUND: Option<u64> = None;

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
    fn aged(message: Self::Message, _late: u64) -> Self::Message {
        message
    }
    fn begin_linked_frame(&mut self) -> u64;
    fn step_linked(&mut self) -> u64;
    fn step_linked_within(&mut self, _room: u64) -> u64 {
        self.step_linked()
    }
    fn takes_room(&self) -> bool {
        true
    }
    fn link_horizon(&self) -> u64 {
        0
    }
    fn linked_frame_done(&self) -> bool;
    fn end_linked_frame(&mut self);
    fn holds_its_frame(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub(crate) struct Cable<M, const N: usize = 2> {
    pub(crate) plugged: bool,
    pub(crate) clocks: [u64; N],
    pub(crate) in_flight: [VecDeque<(u64, M)>; N],
    pub(crate) messages: [u64; N],
    pub(crate) transfers: [u64; N],
    pub(crate) stalls: [u64; N],
    pub(crate) window: u64,
    pub(crate) exact: [bool; N],
    pub(crate) follows: [bool; N],
}

impl<M, const N: usize> Default for Cable<M, N> {
    fn default() -> Self {
        Self {
            window: 0,
            plugged: false,
            clocks: [0; N],
            in_flight: std::array::from_fn(|_| VecDeque::new()),
            messages: [0; N],
            transfers: [0; N],
            stalls: [0; N],
            exact: [false; N],
            follows: [false; N],
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exact: Vec<bool>,
}

impl<M: Clone, const N: usize> Cable<M, N> {
    pub(crate) fn copy_from(&mut self, source: &Self) {
        let Self {
            plugged,
            clocks,
            in_flight,
            messages,
            transfers,
            stalls,
            window,
            exact,
            follows: _,
        } = self;
        *window = source.window;
        *exact = source.exact;
        *plugged = source.plugged;
        *clocks = source.clocks;
        for (queue, from) in in_flight.iter_mut().zip(&source.in_flight) {
            queue.clear();
            queue.extend(from.iter().cloned());
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
                .map(|q| q.iter().cloned().collect())
                .collect(),
            messages: self.messages.to_vec(),
            transfers: self.transfers.to_vec(),
            stalls: self.stalls.to_vec(),
            exact: if self.exact.contains(&true) {
                self.exact.to_vec()
            } else {
                Vec::new()
            },
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
            && wire.in_flight.len() == N
            && (wire.exact.is_empty() || wire.exact.len() == N);
        if !fits {
            return Err(GbError::IO(std::io::ErrorKind::InvalidData.into()));
        }
        self.plugged = wire.plugged;
        for i in 0..N {
            self.clocks[i] = wire.clocks[i];
            self.messages[i] = wire.messages[i];
            self.transfers[i] = wire.transfers[i];
            self.stalls[i] = wire.stalls[i];
            self.in_flight[i] = wire.in_flight[i].iter().cloned().collect();
            self.exact[i] = wire.exact.get(i).copied().unwrap_or(false);
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

    fn deliver<C: LinkPort<Message = M>>(&mut self, to: usize, console: &mut C) -> Option<u64> {
        deliver(&mut self.in_flight[to], self.clocks[to], console)
    }

    pub(crate) fn windowed(&self, ends: [bool; N]) -> bool {
        let units = ends.iter().filter(|&&end| end).count();
        let slowed = ends
            .iter()
            .zip(self.exact)
            .any(|(&end, exact)| end && exact);
        self.window > 0 && units >= WINDOWED_UNITS && !slowed
    }

    pub(crate) fn held_back_by(&self, on: [bool; N], unit: usize) -> Option<usize> {
        let ends = self.ends(on);
        let units = ends.iter().filter(|&&end| end).count();
        let held = ends[unit] && self.exact[unit] && self.window > 0 && units >= WINDOWED_UNITS;
        held.then_some(units)
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

fn deliver<C: LinkPort>(
    queue: &mut VecDeque<(u64, C::Message)>,
    clock: u64,
    console: &mut C,
) -> Option<u64> {
    while let Some(&(due, _)) = queue.front() {
        if due > clock {
            return Some(due);
        }
        if let Some((_, message)) = queue.pop_front() {
            console.receive_link_message(message);
        }
    }
    None
}

fn deliver_aged<C: LinkPort>(
    queue: &mut VecDeque<(u64, C::Message)>,
    clock: u64,
    window: u64,
    console: &mut C,
) {
    while queue.front().is_some_and(|&(due, _)| due <= clock) {
        let Some((due, message)) = queue.pop_front() else {
            break;
        };
        console.receive_link_message(C::aged(message, clock - due + window));
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

#[cfg(test)]
pub(crate) fn run_frame<C: Console + LinkPort + Send, const N: usize>(
    consoles: &mut [C; N],
    cable: &mut Cable<C::Message, N>,
    inputs: &[Input; N],
    on: [bool; N],
) -> Option<Turnout> {
    run_frame_on(consoles, cable, inputs, on, &mut Split::inline())
}

pub(crate) fn run_frame_on<C: Console + LinkPort + Send, const N: usize>(
    consoles: &mut [C; N],
    cable: &mut Cable<C::Message, N>,
    inputs: &[Input; N],
    on: [bool; N],
    split: &mut Split<'_>,
) -> Option<Turnout> {
    run_linked_on(consoles.each_mut(), cable, inputs, on, split)
}

pub(crate) fn run_pair<C: Console + LinkPort + Send>(
    consoles: [&mut C; 2],
    cable: &mut Cable<C::Message>,
    inputs: &[Input; 2],
    on: [bool; 2],
) {
    run_linked_on(consoles, cable, inputs, on, &mut Split::inline());
}

pub(crate) fn run_linked_on<C: Console + LinkPort + Send, const N: usize>(
    mut consoles: [&mut C; N],
    cable: &mut Cable<C::Message, N>,
    inputs: &[Input; N],
    on: [bool; N],
    split: &mut Split<'_>,
) -> Option<Turnout> {
    for ((gb, input), on) in consoles.iter_mut().zip(inputs).zip(on) {
        if on {
            gb.set_keys(input.buttons);
            gb.set_touch(input.touch);
        }
    }
    for (exact, input) in cable.exact.iter_mut().zip(inputs) {
        match input.pace {
            Some(CablePace::Exact) => *exact = true,
            Some(CablePace::Windows) => *exact = false,
            None => {}
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
    let windows = C::CABLE_WINDOWS && cable.windowed(ends);
    let rounds = C::AIR_ROUND.is_some()
        && !ends
            .iter()
            .zip(cable.follows)
            .any(|(&end, follows)| end && follows);
    let mut linked = [const { None }; N];
    let mut exact = (!windows && !rounds && ends.contains(&true)).then_some(&mut *cable);
    split.scope("linked console", |scope| {
        for (i, ((gb, on), end)) in consoles.into_iter().zip(on).zip(ends).enumerate() {
            if end {
                linked[i] = Some(gb);
            } else if on {
                scope.spawn(move || gb.run_frame());
            }
        }
        if let Some(cable) = exact.take() {
            let linked = std::mem::replace(&mut linked, [const { None }; N]);
            scope.spawn(move || step_linked(linked, cable));
        }
    });
    if rounds && ends.contains(&true) {
        step_rounds(linked, cable, split);
        return None;
    }
    windows.then(|| step_windows(linked, cable, split))
}

struct Lane<'a, C: LinkPort> {
    gb: &'a mut C,
    clock: &'a mut u64,
    queue: &'a mut VecDeque<(u64, C::Message)>,
    outbox: Vec<(u64, C::Message)>,
    messages: &'a mut u64,
    transfers: &'a mut u64,
    stalls: &'a mut u64,
    done: bool,
    end: u64,
    alone: bool,
}

impl<C: LinkPort> Lane<'_, C> {
    fn run_window(&mut self, window: u64) {
        while !self.done && *self.clock < self.end {
            deliver_aged(self.queue, *self.clock, window, self.gb);
            if self.alone && self.queue.is_empty() && self.gb.link_waiting() {
                *self.stalls += 1;
                self.gb.end_linked_frame();
                self.done = true;
                return;
            }
            *self.clock += self.gb.step_linked();
            for message in self.gb.take_link_messages() {
                *self.messages += 1;
                *self.transfers += u64::from(C::is_transfer(&message));
                self.outbox.push((*self.clock + window, message));
            }
            if self.gb.linked_frame_done() {
                self.gb.end_linked_frame();
                self.done = true;
            }
        }
    }
}

impl<C: LinkPort> Lane<'_, C> {
    fn run_round(&mut self) {
        while !self.done && *self.clock < self.end {
            deliver(self.queue, *self.clock, self.gb);
            *self.clock += self.gb.step_linked_within(self.end - *self.clock);
            for message in self.gb.take_link_messages() {
                *self.messages += 1;
                *self.transfers += u64::from(C::is_transfer(&message));
                self.outbox.push((*self.clock, message));
            }
            if self.gb.linked_frame_done() {
                self.gb.end_linked_frame();
                self.done = true;
            }
        }
    }
}

fn next_round<C: LinkPort>(lanes: &mut [&mut Lane<'_, C>]) -> bool {
    let running = lanes.iter().filter(|lane| !lane.done);
    let Some(start) = running.clone().map(|lane| *lane.clock).min() else {
        return false;
    };
    let end = round_end::<C>(start);
    for lane in lanes.iter_mut() {
        lane.end = end;
    }
    true
}

fn round_end<C: LinkPort>(clock: u64) -> u64 {
    C::AIR_ROUND.map_or(u64::MAX, |length| (clock / length + 1) * length)
}

fn next_window<C: LinkPort>(lanes: &mut [&mut Lane<'_, C>], window: u64) -> bool {
    let Some(start) = lanes
        .iter()
        .filter(|lane| !lane.done)
        .map(|lane| *lane.clock)
        .min()
    else {
        return false;
    };
    let alone = lanes.iter().filter(|lane| !lane.done).count() == 1;
    for lane in lanes.iter_mut() {
        lane.end = start + window;
        lane.alone = alone;
    }
    true
}

fn pass_messages<C: LinkPort>(lanes: &mut [&mut Lane<'_, C>]) {
    for from in 0..lanes.len() {
        let sent = std::mem::take(&mut lanes[from].outbox);
        if sent.is_empty() {
            continue;
        }
        for (to, lane) in lanes.iter_mut().enumerate() {
            if to != from {
                lane.queue.extend(sent.iter().cloned());
            }
        }
    }
    for lane in lanes.iter_mut() {
        lane.queue.make_contiguous().sort_by_key(|&(due, _)| due);
    }
}

fn step_windows<C: Console + LinkPort + Send, const N: usize>(
    mut consoles: [Option<&mut C>; N],
    cable: &mut Cable<C::Message, N>,
    split: &mut Split<'_>,
) -> Turnout {
    let window = cable.window;
    let mut lanes = lanes(&mut consoles, cable);
    let mut all: Vec<&mut Lane<'_, C>> = lanes.iter_mut().collect();
    if !next_window(&mut all, window) {
        return Turnout::default();
    }
    split.rounds(
        "linked console",
        &mut lanes,
        |lane| lane.run_window(window),
        |lanes| {
            pass_messages(lanes);
            next_window(lanes, window)
        },
    )
}

fn lanes<'a, C: LinkPort, const N: usize>(
    consoles: &'a mut [Option<&mut C>; N],
    cable: &'a mut Cable<C::Message, N>,
) -> Vec<Lane<'a, C>> {
    let Cable {
        clocks,
        in_flight,
        messages,
        transfers,
        stalls,
        ..
    } = cable;
    consoles
        .iter_mut()
        .zip(clocks.iter_mut())
        .zip(in_flight.iter_mut())
        .zip(messages.iter_mut().zip(transfers.iter_mut()))
        .zip(stalls.iter_mut())
        .filter_map(|((((gb, clock), queue), (messages, transfers)), stalls)| {
            let gb = gb.as_deref_mut()?;
            *clock += gb.begin_linked_frame();
            Some(Lane {
                gb,
                clock,
                queue,
                outbox: Vec::new(),
                messages,
                transfers,
                stalls,
                done: false,
                end: 0,
                alone: false,
            })
        })
        .collect()
}

fn step_rounds<C: Console + LinkPort + Send, const N: usize>(
    mut consoles: [Option<&mut C>; N],
    cable: &mut Cable<C::Message, N>,
    split: &mut Split<'_>,
) -> Turnout {
    let mut lanes = lanes(&mut consoles, cable);
    let mut all: Vec<&mut Lane<'_, C>> = lanes.iter_mut().collect();
    if !next_round(&mut all) {
        return Turnout::default();
    }
    split.rounds("linked console", &mut lanes, Lane::run_round, |lanes| {
        pass_messages(lanes);
        next_round(lanes)
    })
}

fn step_linked<C: Console + LinkPort, const N: usize>(
    mut consoles: [Option<&mut C>; N],
    cable: &mut Cable<C::Message, N>,
) {
    let ends = consoles.each_ref().map(Option::is_some);
    for (i, gb) in consoles.iter_mut().enumerate() {
        if let Some(gb) = gb {
            cable.clocks[i] += gb.begin_linked_frame();
        }
    }
    let leader = (0..N).find(|&i| ends[i] && !cable.follows[i]);
    let follows = cable.follows.map(|follows| follows && leader.is_some());
    let mut cut = None;
    let mut done = ends.map(|end| !end);
    while let Some(i) = next_linked(&done, &cable.clocks) {
        let Some(gb) = consoles[i].as_deref_mut() else {
            unreachable!("only linked consoles step")
        };
        let cut_here = follows[i] && !gb.holds_its_frame();
        if cut_here && cut.is_some_and(|cut| cable.clocks[i] >= cut) {
            done[i] = true;
            continue;
        }
        let due = cable.deliver(i, gb);
        let alone = if N == 2 {
            done[1 - i]
        } else {
            (0..N).all(|j| j == i || done[j])
        };
        if alone && gb.link_waiting() {
            cable.stalls[i] += 1;
            gb.end_linked_frame();
            done[i] = true;
            cut = cut.or((Some(i) == leader).then_some(cable.clocks[i]));
            continue;
        }

        let start = cable.clocks[i];
        let room = if gb.takes_room() {
            let ahead = if N == 2 {
                linked_ahead(&consoles, cable, 1 - i)
            } else {
                0
            };
            let bound = if N == 2 {
                let j = 1 - i;
                let turn = if done[j] {
                    None
                } else {
                    Some((cable.clocks[j] + u64::from(i < j)).max(cable.clocks[j] + ahead))
                };
                match (turn, due) {
                    (Some(turn), Some(due)) => Some(turn.min(due)),
                    (turn, due) => turn.or(due),
                }
            } else {
                (0..N)
                    .filter(|&j| j != i && !done[j])
                    .map(|j| (cable.clocks[j] + u64::from(i < j)).max(cable.clocks[j] + ahead))
                    .chain(due)
                    .min()
            };
            let bound = match (bound, cut.filter(|_| cut_here)) {
                (Some(bound), Some(cut)) => Some(bound.min(cut)),
                (bound, cut) => bound.or(cut),
            };
            bound.map_or(u64::MAX, |bound| bound - start)
        } else {
            u64::MAX
        };
        let Some(gb) = consoles[i].as_deref_mut() else {
            unreachable!("only linked consoles step")
        };
        cable.clocks[i] += gb.step_linked_within(room);
        let stamp = if C::STAMPS_AT_STEP_START {
            start
        } else {
            cable.clocks[i]
        };
        for message in gb.take_link_messages() {
            cable.messages[i] += 1;
            cable.transfers[i] += u64::from(C::is_transfer(&message));
            for to in (0..N).filter(|&to| to != i && ends[to]) {
                cable.in_flight[to].push_back((stamp, message.clone()));
            }
        }

        if gb.linked_frame_done() {
            gb.end_linked_frame();
            if !follows[i] {
                done[i] = true;
                cut = cut.or((Some(i) == leader).then_some(cable.clocks[i]));
            } else if cut.is_some_and(|cut| cable.clocks[i] >= cut) {
                done[i] = true;
            } else {
                cable.clocks[i] += gb.begin_linked_frame();
            }
        }
    }
}

fn next_linked<const N: usize>(done: &[bool; N], clocks: &[u64; N]) -> Option<usize> {
    if N == 2 {
        return match (done[0], done[1]) {
            (false, false) => Some(usize::from(clocks[1] < clocks[0])),
            (false, true) => Some(0),
            (true, false) => Some(1),
            (true, true) => None,
        };
    }
    (0..N).filter(|&i| !done[i]).min_by_key(|&i| clocks[i])
}

fn linked_ahead<C: LinkPort, const N: usize>(
    consoles: &[Option<&mut C>; N],
    cable: &Cable<C::Message, N>,
    j: usize,
) -> u64 {
    if C::STAMPS_AT_STEP_START || !cable.in_flight[j].is_empty() {
        return 0;
    }
    consoles[j].as_deref().map_or(0, C::link_horizon)
}

pub(crate) fn run_alone<C: Console + LinkPort>(
    gb: &mut C,
    clock: &mut u64,
    input: Input,
    on: bool,
) -> bool {
    if !on {
        return true;
    }
    gb.set_keys(input.buttons);
    gb.set_touch(input.touch);
    if !gb.link_plugged() {
        gb.run_frame();
        return true;
    }

    *clock += gb.begin_linked_frame();
    loop {
        if gb.link_waiting() {
            return false;
        }
        *clock += match C::AIR_ROUND {
            Some(_) => gb.step_linked_within(round_end::<C>(*clock) - *clock),
            None => gb.step_linked(),
        };
        if !gb.take_link_messages().is_empty() {
            return false;
        }
        if gb.linked_frame_done() {
            gb.end_linked_frame();
            return true;
        }
    }
}
