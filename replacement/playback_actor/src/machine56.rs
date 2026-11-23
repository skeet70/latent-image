//! Machine 56. Its 17 actions (table indices 2..=18 of the action table at 0x01217350) read the
//! request block at 0x014ea6e4, call retained services, set the machine's state halfword and
//! return the dispatcher's result.

/// The state halfword of machine 56 (0x014ea61c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
    S3 = 3,
    S4 = 4,
    S5 = 5,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x01211250, dispatches machine 57
    Fn1211250,
    /// 0x0121132c, dispatches machine 58
    Fn121132c,
    /// 0x012135e8, dispatches machine 82
    Fn12135e8,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0122b3f0
    Fn122b3f0,
    /// 0x0122b440
    Fn122b440,
    /// 0x0122b468
    Fn122b468,
    /// 0x0122b800
    Fn122b800,
}

/// Words of the request block at 0x014ea6e4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Request {
    R1fc = 0x1fc,
}

/// What machine 56 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn set_state(&mut self, state: State);
    fn request(&self, word: Request) -> u32;
}

const DONE: i32 = 8;
const IDLE: i32 = 4;
const CANCELLED: i32 = 12;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn advance<S: Services>(services: &mut S, call: Call, state: State) -> i32 {
    match services.call(call) {
        0 => set(services, state, 0),
        IDLE => IDLE,
        _ => CANCELLED,
    }
}

fn arm<S: Services>(services: &mut S, call: Call) -> i32 {
    if services.call(call) != 0 {
        services.set_state(State::S4);
        services.call(Call::Fn122b3f0);
    }
    0
}

pub fn action02<S: Services>(_services: &mut S) -> i32 {
    DONE
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S5);
    services.call(Call::Fn1211250)
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S5);
    services.call(Call::Fn121132c)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn12135e8)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    set(services, State::S5, 0)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1211250)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn121132c)
}

pub fn action10<S: Services>(_services: &mut S) -> i32 {
    0
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    let result = advance(services, Call::Fn1211250, State::S1);
    if cfg!(playback_control_m56_action11_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    advance(services, Call::Fn121132c, State::S2)
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    advance(services, Call::Fn12135e8, State::S3)
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    arm(services, Call::Fn1211250)
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    arm(services, Call::Fn121132c)
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    arm(services, Call::Fn12135e8)
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    if services.request(Request::R1fc) == 0 {
        services.call(Call::Fn122b440);
    } else {
        services.call(Call::Fn122b468);
    }
    if services.call(Call::Fn122b800) == 0 {
        CANCELLED
    } else {
        IDLE
    }
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    set(services, State::S0, DONE)
}
