//! Machine 1. Its 14 actions (table indices 2..=15 of the action table at 0x01218d90) read the
//! image control byte at 0x015114f8 and the media output word at 0x015149a0, call retained
//! services, set the machine's state halfword and return the dispatcher's result; where stock
//! returns a service's r0, the action returns that result.

/// The state halfword of machine 1 (0x014ea5ae).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
    S3 = 3,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120b40c
    Fn120b40c,
    /// 0x0120cab0
    Fn120cab0,
    /// 0x01210250
    Fn1210250,
    /// 0x012142f8
    Fn12142f8,
    /// 0x0121eb68, packet 0x91b
    Trace,
}

/// Retained services called with one word in r0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0124c6fc, stores its argument's low byte at 0x015114f8 unless that byte is 1
    Fn124c6fc,
}

/// Bytes of the image control block at 0x015114f4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ImageControl {
    C004 = 0x004,
}

/// Words of the media output block at 0x015149a0, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MediaOutput {
    O000 = 0x000,
}

/// What machine 1 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_state(&mut self, state: State);
    fn image_control(&self, byte: ImageControl) -> u8;
    fn media_output(&self, word: MediaOutput) -> u32;
}

const SELECTED: i32 = 3;

fn enter<S: Services>(services: &mut S, state: State, call: Call) -> i32 {
    services.set_state(state);
    services.call(call)
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120cab0);
    if status != SELECTED {
        services.set_state(State::S1);
    }
    status
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120cab0);
    if status == SELECTED {
        services.set_state(State::S0);
    }
    status
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn124c6fc, 1);
    services.call(Call::Fn1210250)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    enter(services, State::S2, Call::Fn1210250)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1210250)
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    let state = if services.image_control(ImageControl::C004) == 1 { State::S0 } else { State::S2 };
    enter(services, state, Call::Fn1210250)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn12142f8)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    enter(services, State::S3, Call::Fn12142f8)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    let state = if services.media_output(MediaOutput::O000) == 1 { State::S0 } else { State::S3 };
    enter(services, state, Call::Fn12142f8)
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    let result = services.call(Call::Fn120b40c);
    if cfg!(playback_control_m1_action12_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1210250);
    services.call(Call::Fn120b40c)
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn12142f8);
    services.call(Call::Fn120b40c)
}

pub fn action15<S: Services>(_services: &mut S) -> i32 {
    0
}
