//! Machine 2. Its 14 actions (table indices 2..=15 of the action table at 0x0121739c) read the
//! request block at 0x014ea6e4, call retained services, set the machine's state halfword and
//! return the dispatcher's result.

/// The state halfword of machine 2 (0x014ea5b0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
    S3 = 3,
    S4 = 4,
    S6 = 6,
}

/// Retained services called without arguments whose r0 the actions do not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120c650
    PublishRequestedMode,
    /// 0x0121daf4
    Fn121daf4,
    /// 0x0121db3c
    Fn121db3c,
    /// 0x0121db7c
    Fn121db7c,
    /// 0x0121dbb4
    Fn121dbb4,
    /// 0x0121dc40
    Fn121dc40,
    /// 0x0121dcac
    Fn121dcac,
    /// 0x0121dd98
    Fn121dd98,
    /// 0x0121eb68, packet 0x91b
    Trace,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// 0x00ec026c
    Fn0ec026c,
    /// 0x00ec03dc
    Fn0ec03dc,
    /// 0x0121ddf0
    Fn121ddf0,
    /// 0x0121de10
    Fn121de10,
    /// 0x0121eabc
    Fn121eabc,
}

/// Retained services called with one word in r0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0121daa0
    Fn121daa0,
    /// 0x0121dc78
    Fn121dc78,
    /// 0x0121ddd0
    Fn121ddd0,
}

/// The mode-work state written through 0x0121ca80.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum WorkState {
    Fourth = 4,
    Fifth = 5,
}

/// Words of the request block at 0x014ea6e4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Request {
    R034 = 0x034,
    R1c4 = 0x1c4,
}

/// What machine 2 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call);
    fn status(&mut self, status: Status) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_work_state(&mut self, state: WorkState);
    fn set_state(&mut self, state: State);
    fn request(&self, word: Request) -> u32;
}

const DONE: i32 = 8;
const IDLE: i32 = 4;
const CANCELLED: i32 = 12;

fn reset<S: Services>(services: &mut S) {
    services.set_state(State::S0);
    services.call(Call::Fn121dcac);
}

fn cancel<S: Services>(services: &mut S, selector: u32) -> i32 {
    reset(services);
    services.call_with(CallWith::Fn121dc78, selector);
    CANCELLED
}

fn release<S: Services>(services: &mut S, selector: u32) -> i32 {
    if services.request(Request::R1c4) != 0 {
        return cancel(services, selector);
    }
    reset(services);
    if services.status(Status::Fn121ddf0) == 0 {
        return IDLE;
    }
    services.call_with(CallWith::Fn121ddd0, 0);
    18
}

fn resolve<S: Services>(services: &mut S, selector: u32) -> i32 {
    if services.request(Request::R1c4) != 0 {
        return cancel(services, selector);
    }
    reset(services);
    IDLE
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    let kind = services.request(Request::R034);
    services.set_state(State::S3);
    services.call(if kind == 50 { Call::Fn121daf4 } else { Call::Fn121db3c });
    0
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    match services.request(Request::R034) {
        57 => {
            services.set_state(State::S4);
            services.call_with(CallWith::Fn121daa0, 1);
        }
        64 => {
            services.set_state(State::S4);
            services.call_with(CallWith::Fn121daa0, 0);
        }
        50 => {
            services.set_state(State::S1);
            services.call(Call::Fn121db7c);
        }
        _ => {
            services.set_state(State::S6);
            services.call(Call::Fn121dd98);
        }
    }
    0
}

pub fn action05<S: Services>(_services: &mut S) -> i32 {
    DONE
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    DONE
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn121ddd0, 1);
    0
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    if services.status(Status::Fn0ec026c) == 0 {
        services.set_state(State::S2);
        let call = if services.request(Request::R034) == 46 {
            Call::Fn121dc40
        } else {
            Call::Fn121dbb4
        };
        services.call(call);
    } else {
        services.set_state(State::S1);
        services.call(Call::Fn121db7c);
    }
    0
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    release(services, 0)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    resolve(services, 3)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    let result = action11_body(services);
    if cfg!(playback_control_m2_action11_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action11_body<S: Services>(services: &mut S) -> i32 {
    reset(services);
    if services.status(Status::Fn0ec03dc) != 0 || services.status(Status::Fn121eabc) != 0 {
        return if services.status(Status::Fn121de10) == 0 { IDLE } else { CANCELLED };
    }
    let (state, result) = if services.status(Status::Fn121de10) != 0 {
        (WorkState::Fifth, 11)
    } else {
        (WorkState::Fourth, 10)
    };
    services.set_work_state(state);
    services.call(Call::PublishRequestedMode);
    result
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    resolve(services, 1)
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    release(services, 1)
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    resolve(services, 2)
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    IDLE
}
