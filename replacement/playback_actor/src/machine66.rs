//! Machine 66. Its 7 actions (table indices 2..=8 of the action table at 0x01218f5c) read the
//! request block at 0x014ea6e4, call retained services, set the machine's state halfword and
//! return the dispatcher's result.

/// The state halfword of machine 66 (0x014ea630).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
    S3 = 3,
    S4 = 4,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x01211e78
    Fn1211e78,
    /// 0x01211fdc
    Fn1211fdc,
    /// 0x01212120
    Fn1212120,
    /// 0x01212310
    Fn1212310,
    /// 0x0121eb68, packet 0x91b
    Trace,
}

/// Words of the request block at 0x014ea6e4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Request {
    R034 = 0x034,
}

/// What machine 66 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn set_state(&mut self, state: State);
    fn request(&self, word: Request) -> u32;
}

const DONE: i32 = 8;

fn probe<S: Services>(services: &mut S, call: Call, state: State) -> i32 {
    let result = services.call(call);
    services.set_state(if result == 0 { state } else { State::S0 });
    result
}

pub fn action02<S: Services>(_services: &mut S) -> i32 {
    DONE
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    probe(services, Call::Fn1211fdc, State::S1)
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    probe(services, Call::Fn1211e78, State::S2)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    probe(services, Call::Fn1212120, State::S3)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    let result = probe(services, Call::Fn1212310, State::S4);
    if cfg!(playback_control_m66_action06_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    match services.request(Request::R034) {
        112 => probe(services, Call::Fn1211fdc, State::S1),
        151 => probe(services, Call::Fn1211e78, State::S2),
        126 => probe(services, Call::Fn1212120, State::S3),
        _ => probe(services, Call::Fn1212310, State::S4),
    }
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}
