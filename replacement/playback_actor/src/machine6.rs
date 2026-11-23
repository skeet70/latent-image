//! Machine 6. Its 10 actions (table indices 2..=11 of the action table at 0x012193c0) read the
//! request block at 0x014ea6e4 and the io block at 0x014eaca8, call retained services, set the
//! machine's state halfword and return the dispatcher's result.

/// The state halfword of machine 6 (0x014ea5b8).
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

/// Retained services called without arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x01220314
    Fn1220314,
    /// 0x01220340
    Fn1220340,
    /// 0x01220378
    Fn1220378,
    /// 0x012203a4
    Fn12203a4,
    /// 0x012203d0
    Fn12203d0,
    /// 0x01220480
    Fn1220480,
    /// 0x012204cc
    Fn12204cc,
    /// 0x01220560
    Fn1220560,
    /// 0x0121eb68, packet 0x91b
    Trace,
}

/// Retained services called with one word in r0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x012203f4, stores request +0x1fc at io +0x14 and its argument at io +0x10
    Fn12203f4,
}

/// Words of the request block at 0x014ea6e4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Request {
    R1f8 = 0x1f8,
    R1fc = 0x1fc,
}

/// Words of the io block at 0x014eaca8, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Io {
    Command = 0x0c,
    Pending = 0x10,
    I2c = 0x2c,
}

/// What machine 6 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call);
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_state(&mut self, state: State);
    fn request(&self, word: Request) -> u32;
    fn io(&self, word: Io) -> u32;
    fn set_io(&mut self, word: Io, value: u32);
    /// The signed doubleword at io +0x20.
    fn io_count(&self) -> i64;
}

const IDLE: i32 = 4;
const CANCELLED: i32 = 12;
const SUBMIT: u32 = 1;
const ABANDON: u32 = 2;
const CONFIRMED: u32 = 14;

fn pending<S: Services>(services: &S) -> bool {
    services.request(Request::R1fc) != 0
}

fn submit<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S1);
    services.set_io(Io::Command, 0);
    services.set_io(Io::Pending, 0);
    services.set_io(Io::I2c, 0);
    services.call(Call::Fn1220314);
    0
}

fn defer<S: Services>(services: &mut S, code: u32) -> i32 {
    services.set_state(State::S5);
    services.call_with(CallWith::Fn12203f4, code);
    services.call(Call::Fn12203d0);
    services.call(Call::Fn1220480);
    0
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    match services.io(Io::Command) {
        SUBMIT => submit(services),
        ABANDON => {
            services.set_io(Io::Command, 0);
            IDLE
        }
        _ => 0,
    }
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    if pending(services) {
        return defer(services, 1);
    }
    services.call(Call::Fn12204cc);
    if services.io_count() >= 1 {
        services.set_state(State::S2);
        services.call(Call::Fn1220340);
    } else {
        services.set_state(State::S3);
        services.call(Call::Fn1220378);
    }
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    if pending(services) {
        return defer(services, 2);
    }
    services.set_state(State::S3);
    services.call(Call::Fn1220378);
    0
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    if pending(services) {
        return defer(services, 3);
    }
    services.set_state(State::S4);
    services.call(Call::Fn1220560);
    services.call(Call::Fn12203a4);
    0
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    let result = action06_body(services);
    if cfg!(playback_control_m6_action06_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action06_body<S: Services>(services: &mut S) -> i32 {
    let pending = pending(services);
    services.set_state(State::S0);
    if !pending {
        services.set_io(Io::Command, 0);
        return IDLE;
    }
    services.call_with(CallWith::Fn12203f4, 4);
    services.set_io(Io::Command, 0);
    services.call(Call::Fn1220480);
    CANCELLED
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    if services.request(Request::R1f8) != CONFIRMED {
        return 0;
    }
    let pending = services.io(Io::Pending);
    services.set_state(State::S0);
    services.set_io(Io::Command, 0);
    if pending != 0 {
        CANCELLED
    } else {
        IDLE
    }
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    submit(services)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action10<S: Services>(_services: &mut S) -> i32 {
    IDLE
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S5);
    services.call(Call::Fn12203d0);
    0
}
