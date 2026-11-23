//! Machine 57. Its 7 actions (table indices 2..=8 of the action table at 0x01216bf4) read the
//! request block at 0x014ea6e4, call retained services, set the machine's state halfword and
//! return the dispatcher's result.

/// The state halfword of machine 57 (0x014ea61e).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0122b4d8
    Fn122b4d8,
    /// 0x0122b504
    Fn122b504,
}

/// Retained services called with one word in r0; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0122b48c, r0 compared with 1
    Fn122b48c,
}

/// Words of the request block at 0x014ea6e4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Request {
    R0f8 = 0x0f8,
    R1fc = 0x1fc,
}

/// What machine 57 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32) -> i32;
    fn set_state(&mut self, state: State);
    fn request(&self, word: Request) -> u32;
}

const DONE: i32 = 8;
const IDLE: i32 = 4;
const CANCELLED: i32 = 12;
const NAMED_KIND: u32 = 48;

fn reset<S: Services>(services: &mut S) {
    services.set_state(State::S0);
    services.call(Call::Fn122b504);
}

pub fn action02<S: Services>(_services: &mut S) -> i32 {
    DONE
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    reset(services);
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    reset(services);
    19
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    let named = services.request(Request::R0f8) == NAMED_KIND;
    services.set_state(State::S1);
    services.call_with(CallWith::Fn122b48c, u32::from(named));
    0
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action07<S: Services>(_services: &mut S) -> i32 {
    IDLE
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    let result = action08_body(services);
    if cfg!(playback_control_m57_action08_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action08_body<S: Services>(services: &mut S) -> i32 {
    let pending = services.request(Request::R1fc);
    services.set_state(State::S0);
    if pending == 0 {
        return IDLE;
    }
    services.call(Call::Fn122b4d8);
    CANCELLED
}
