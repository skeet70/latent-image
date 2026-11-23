//! Machine 61. Its 26 actions (table indices 2..=27 of the action table at 0x01215c2c) read the
//! request block at 0x014ea6e4 and the selection block at 0x014ea9bc, call retained services, set
//! the machine's state halfword and return the dispatcher's result.

/// The state halfword of machine 61 (0x014ea626).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
    S3 = 3,
    S4 = 4,
    S5 = 5,
    S6 = 6,
    S7 = 7,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120c1a8
    Fn120c1a8,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0121eda0
    Fn121eda0,
    /// 0x0121ef68
    Fn121ef68,
    /// 0x0121f0fc
    Fn121f0fc,
    /// 0x0121f118
    Fn121f118,
    /// 0x0121f1c4, sets the word at 0x014eaaec
    Fn121f1c4,
    /// 0x0121f1e8, sets the word at 0x014eaaf0
    Fn121f1e8,
    /// 0x0121f20c
    Fn121f20c,
    /// 0x0121f224
    Fn121f224,
    /// 0x0121f254
    Fn121f254,
    /// 0x0121f2a4
    Fn121f2a4,
    /// 0x0121f390
    Fn121f390,
    /// 0x0121f3ec
    Fn121f3ec,
    /// 0x0121f500
    Fn121f500,
    /// 0x0121f518
    Fn121f518,
    /// 0x0121f7ac
    Fn121f7ac,
    /// 0x0121f83c
    Fn121f83c,
    /// 0x0121f85c
    Fn121f85c,
    /// 0x012202d0
    Fn12202d0,
    /// 0x01243590
    Fn1243590,
    /// 0x01243644
    Fn1243644,
    /// 0x01243760
    Fn1243760,
    /// 0x012437bc
    Fn12437bc,
    /// 0x01243868
    Fn1243868,
    /// 0x01243890
    Fn1243890,
    /// 0x012438f0
    Fn12438f0,
    /// 0x01243a6c
    Fn1243a6c,
}

/// Retained services called with one word in r0; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x01229f84
    Fn1229f84,
    /// 0x0123265c
    Fn123265c,
    /// 0x01243198
    Fn1243198,
    /// 0x01243948
    Fn1243948,
    /// 0x01245c7c
    Fn1245c7c,
}

/// Words of the request block at 0x014ea6e4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Request {
    Selection = 0x100,
    Kind = 0x10c,
    Name = 0x178,
    R1c4 = 0x1c4,
    R1f8 = 0x1f8,
    R1fc = 0x1fc,
}

/// Words of the selection block at 0x014ea9bc, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Selection {
    Choice = 0x000,
    S07c = 0x07c,
    S128 = 0x128,
    S12c = 0x12c,
    Done = 0x130,
    Cancelled = 0x134,
}

/// What machine 61 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32) -> i32;
    fn set_state(&mut self, state: State);
    fn request(&self, word: Request) -> u32;
    /// The byte at `address`, a pointer read from the request block.
    fn byte_at(&self, address: u32) -> u8;
    fn selection(&self, word: Selection) -> u32;
    fn set_selection(&mut self, word: Selection, value: u32);
}

const DONE: i32 = 8;
const IDLE: i32 = 4;
const CANCELLED: i32 = 12;
const BACK: u32 = -3i32 as u32;
const FORWARD: u32 = 3;
const PREVIOUS: u32 = -1i32 as u32;
const UNSELECTED: u32 = 0x7fff_ffff;
const NAMED_KIND: u32 = 48;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn latch_direction<S: Services>(services: &mut S, selection: u32) -> bool {
    match selection {
        BACK | FORWARD => {
            services.set_selection(Selection::Choice, selection);
            true
        }
        _ => false,
    }
}

fn select<S: Services>(services: &mut S) {
    let selection = services.request(Request::Selection);
    services.call_with(CallWith::Fn1245c7c, selection);
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S3);
    let selection = services.request(Request::Selection);
    if !latch_direction(services, selection) {
        services.call_with(CallWith::Fn1245c7c, selection);
    }
    if services.request(Request::Kind) == NAMED_KIND {
        let name = services.request(Request::Name);
        if name == 0 || services.byte_at(name) == 0 {
            services.call(Call::Fn121f85c);
            return 0;
        }
    }
    services.call(Call::Fn121eda0);
    0
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S3);
    select(services);
    let selection = services.request(Request::Selection);
    if selection != UNSELECTED && selection != 0 {
        let choice = services.call_with(CallWith::Fn1229f84, selection);
        services.set_selection(Selection::Choice, choice as u32);
    }
    services.call(Call::Fn121eda0);
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S3);
    select(services);
    services.call(Call::Fn121eda0);
    0
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    if services.call(Call::Fn121f500) != 0 {
        services.set_state(State::S4);
        select(services);
        services.call(Call::Fn121f518);
        return 0;
    }
    services.set_selection(Selection::S128, 0);
    IDLE
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S4);
    let selection = services.request(Request::Selection);
    services.call_with(CallWith::Fn123265c, selection);
    services.call(Call::Fn121f7ac);
    0
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S3);
    services.call(Call::Fn121ef68);
    0
}

/// The selection step of actions 9, 11 and 12: the step kind, or the code a refused step
/// reports through 0x01243948.
fn step_kind<S: Services>(services: &mut S) -> Result<u32, u32> {
    match services.request(Request::Selection) {
        1 => {
            if services.call(Call::Fn1243890) == 0 {
                Ok(1)
            } else {
                Err(4)
            }
        }
        PREVIOUS => {
            if services.call(Call::Fn12438f0) == 0 {
                Ok(2)
            } else {
                Err(5)
            }
        }
        _ => Ok(0),
    }
}

fn refuse<S: Services>(services: &mut S, code: u32) -> i32 {
    services.set_state(State::S0);
    services.call_with(CallWith::Fn1243948, code);
    IDLE
}

fn begin_step<S: Services>(services: &mut S, state: State, kind: u32) {
    services.set_state(state);
    services.call(Call::Fn1243868);
    services.call_with(CallWith::Fn1243948, kind);
    services.call_with(CallWith::Fn1243198, 0);
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    match step_kind(services) {
        Ok(kind) => {
            begin_step(services, State::S5, kind);
            services.call(Call::Fn1243590);
            0
        }
        Err(code) => refuse(services, code),
    }
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S5);
    services.call_with(CallWith::Fn1243948, 0);
    services.call_with(CallWith::Fn1243198, 0);
    services.call(Call::Fn1243590);
    services.call(Call::Fn1243868);
    0
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    match step_kind(services) {
        Ok(kind) => {
            begin_step(services, State::S6, kind);
            services.call(Call::Fn1243644);
            0
        }
        Err(code) => refuse(services, code),
    }
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    match step_kind(services) {
        Ok(kind) => {
            begin_step(services, State::S6, kind);
            services.call(Call::Fn1243760);
            services.call(Call::Fn121f7ac);
            0
        }
        Err(code) => refuse(services, code),
    }
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S3);
    let selection = services.request(Request::Selection);
    latch_direction(services, selection);
    services.call(Call::Fn121f3ec);
    0
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    let state = match services.call(Call::Fn121f118) {
        3 => State::S2,
        1 => State::S1,
        _ => State::S0,
    };
    set(services, state, IDLE)
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    let state = if services.call(Call::Fn121f118) == 3 { State::S2 } else { State::S0 };
    set(services, state, IDLE)
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    set(services, State::S0, IDLE)
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    set(services, State::S2, IDLE)
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    set(services, State::S0, DONE)
}

pub fn action19<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121f1c4);
    0
}

pub fn action20<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121f1c4);
    services.call(Call::Fn12202d0);
    if services.call(Call::Fn120c1a8) != 0 {
        services.set_state(State::S0);
        services.set_selection(Selection::Done, 0);
        return DONE;
    }
    0
}

pub fn action21<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121f1e8);
    0
}

fn flags<S: Services>(services: &mut S, cancel: bool) -> Option<i32> {
    if services.selection(Selection::Done) != 0 {
        services.set_state(State::S0);
        services.set_selection(Selection::Done, 0);
        return Some(DONE);
    }
    if cancel && services.selection(Selection::Cancelled) != 0 {
        services.set_state(State::S0);
        services.set_selection(Selection::Cancelled, 0);
        return Some(IDLE);
    }
    None
}

fn cancel<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    services.call(Call::Fn121f224);
    services.call(Call::Fn121f20c);
    CANCELLED
}

fn confirm<S: Services>(services: &mut S) -> i32 {
    match services.request(Request::R1f8) {
        15 => {
            services.call(Call::Fn121f254);
            0
        }
        2 => {
            let value = services.selection(Selection::S07c);
            services.set_selection(Selection::S12c, value);
            services.call(Call::Fn121f83c);
            if services.call(Call::Fn121f0fc) == 0 {
                set(services, State::S0, IDLE)
            } else {
                0
            }
        }
        _ => set(services, State::S0, IDLE),
    }
}

pub fn action22<S: Services>(services: &mut S) -> i32 {
    let result = action22_body(services);
    if cfg!(playback_control_m61_action22_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action22_body<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = flags(services, true) {
        return result;
    }
    let pending = services.request(Request::R1fc);
    if pending != 0 {
        if services.request(Request::Kind) == NAMED_KIND && pending == 6 {
            services.call(Call::Fn121f85c);
            return 0;
        }
        return cancel(services);
    }
    confirm(services)
}

pub fn action23<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = flags(services, true) {
        return result;
    }
    if services.request(Request::R1fc) != 0 {
        return cancel(services);
    }
    confirm(services)
}

fn finish_selection<S: Services>(services: &mut S) -> i32 {
    if services.call(Call::Fn12437bc) == 0 {
        return refuse(services, 0);
    }
    match services.call(Call::Fn120c1a8) {
        0 => set(services, State::S7, 0),
        12 => refuse(services, 0),
        _ => {
            services.set_state(State::S0);
            let code = services.call(Call::Fn1243a6c);
            services.call_with(CallWith::Fn1243948, code as u32);
            IDLE
        }
    }
}

pub fn action24<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = flags(services, false) {
        return result;
    }
    if services.request(Request::R1fc) != 0 {
        return cancel(services);
    }
    services.call(Call::Fn121f0fc);
    finish_selection(services)
}

pub fn action25<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = flags(services, false) {
        return result;
    }
    if services.request(Request::R1fc) != 0 {
        return cancel(services);
    }
    services.call(Call::Fn121f83c);
    services.call(Call::Fn121f0fc);
    finish_selection(services)
}

pub fn action26<S: Services>(services: &mut S) -> i32 {
    match services.call(Call::Fn120c1a8) {
        0 => 0,
        4 => {
            if let Some(result) = flags(services, false) {
                return result;
            }
            services.set_state(State::S0);
            let code = services.call(Call::Fn1243a6c);
            services.call_with(CallWith::Fn1243948, code as u32);
            IDLE
        }
        _ => refuse(services, 0),
    }
}

pub fn action27<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = flags(services, true) {
        return result;
    }
    if services.request(Request::R1c4) == 0 && services.call(Call::Fn121f2a4) != 0 {
        services.call(Call::Fn121f390);
        return 0;
    }
    services.set_state(State::S0);
    services.call(Call::Fn121f20c);
    CANCELLED
}
