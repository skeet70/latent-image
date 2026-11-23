//! Machine 34. Its 13 actions (table indices 2..=14 of the action table at 0x012177ec) read no
//! guest block. Each of actions 2 to 10, 13 and 14 calls one retained service, sets the machine's
//! state halfword from the service's result and returns that result to the dispatcher.

/// The state halfword of machine 34 (0x014ea5f0).
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
    S8 = 8,
    S9 = 9,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120b9e0
    Fn120b9e0,
    /// 0x0120d22c
    Fn120d22c,
    /// 0x0120e42c
    Fn120e42c,
    /// 0x0120e830
    Fn120e830,
    /// 0x0120eb10
    Fn120eb10,
    /// 0x012108d0
    Fn12108d0,
    /// 0x012110f8
    Fn12110f8,
    /// 0x01211dd0
    Fn1211dd0,
    /// 0x01213c50
    Fn1213c50,
    /// 0x0121eb68, packet 0x91b
    Trace,
}

/// What machine 34 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn set_state(&mut self, state: State);
}

const IDLE: i32 = 4;
const DONE: i32 = 8;
const CANCELLED: i32 = 12;
const ABORTED: i32 = 255;

fn settle<S: Services>(
    services: &mut S,
    call: Call,
    settled: fn(i32) -> bool,
    pending: State,
) -> i32 {
    let result = services.call(call);
    services.set_state(if settled(result) { State::S0 } else { pending });
    result
}

fn ended(result: i32) -> bool {
    matches!(result, IDLE | DONE | CANCELLED)
}

fn ended_or_aborted(result: i32) -> bool {
    matches!(result, IDLE | DONE | CANCELLED | ABORTED)
}

fn ended_aborted_or_18(result: i32) -> bool {
    matches!(result, IDLE | DONE | CANCELLED | 18 | ABORTED)
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120d22c, ended_or_aborted, State::S1)
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn12110f8, ended_or_aborted, State::S2)
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120b9e0, ended_aborted_or_18, State::S3)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    settle(
        services,
        Call::Fn120e42c,
        |result| matches!(result, IDLE | DONE | CANCELLED | 15 | 16 | ABORTED),
        State::S4,
    )
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120e830, ended_or_aborted, State::S5)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn120eb10, ended, State::S6)
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn12108d0, ended, State::S7)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    settle(services, Call::Fn1213c50, ended, State::S8)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    let result = settle(services, Call::Fn1211dd0, ended, State::S9);
    if cfg!(playback_control_m34_action10_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action11<S: Services>(_services: &mut S) -> i32 {
    DONE
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    settle(
        services,
        Call::Fn120d22c,
        |result| matches!(result, IDLE | 5 | DONE | CANCELLED | ABORTED),
        State::S1,
    )
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    let result = services.call(Call::Fn120b9e0);
    services.set_state(match result {
        10 | 11 => State::S9,
        result if ended_aborted_or_18(result) => State::S0,
        _ => State::S3,
    });
    result
}
