//! Machine 3. Its 9 actions (table indices 2..=10 of the action table at 0x01218e94) call
//! retained image services, read and write the image block at 0x014ead40, set the machine's
//! state halfword and return the dispatcher's result.

/// The state halfword of machine 3 (0x014ea5b2).
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
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0122062c
    Fn122062c,
    /// 0x01220c94
    Fn1220c94,
    /// 0x01220cb8
    Fn1220cb8,
    /// 0x01220cc8
    Fn1220cc8,
    /// 0x01220dd0
    Fn1220dd0,
    /// 0x01220e68
    Fn1220e68,
    /// 0x01220f4c
    Fn1220f4c,
    /// 0x01220fd0
    Fn1220fd0,
    /// 0x01220fe0
    Fn1220fe0,
    /// 0x01220ff0
    Fn1220ff0,
}

/// Retained services called with one word in r0; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x01220bdc
    Fn1220bdc,
    /// 0x01220f34
    Fn1220f34,
}

/// Words of the image block at 0x014ead40, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Word {
    Status = 0xdc,
    Remaining = 0xe4,
}

/// Bytes of the image block at 0x014ead40, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Byte {
    Released = 0xe0,
}

/// What machine 3 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32) -> i32;
    fn set_state(&mut self, state: State);
    fn word(&self, word: Word) -> u32;
    fn set_word(&mut self, word: Word, value: u32);
    fn byte(&self, byte: Byte) -> u8;
}

const IDLE: i32 = 4;
const FAILED: i32 = 12;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn fail<S: Services>(services: &mut S, status: i32) -> i32 {
    services.set_word(Word::Status, status as u32);
    FAILED
}

fn settle<S: Services>(services: &mut S, status: i32, state: State) -> i32 {
    if status == 0 {
        set(services, state, 0)
    } else {
        fail(services, status)
    }
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn122062c);
    let status = services.call_with(CallWith::Fn1220bdc, 0);
    settle(services, status, State::S1)
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    if services.call_with(CallWith::Fn1220f34, 0) != 0 {
        let status = services.call(Call::Fn1220f4c);
        return settle(services, status, State::S3);
    }
    if services.call(Call::Fn1220dd0) != 0 {
        services.set_state(State::S3);
        services.call(Call::Fn1220e68);
        return 0;
    }
    let status = services.call(Call::Fn1220cc8);
    settle(services, status, State::S2)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    set(services, State::S0, IDLE)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1220c94);
    0
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1220cb8);
    0
}

pub fn action08<S: Services>(_services: &mut S) -> i32 {
    0
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    let result = action09_body(services);
    if cfg!(playback_control_m3_action09_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action09_body<S: Services>(services: &mut S) -> i32 {
    if services.byte(Byte::Released) == 1 || services.word(Word::Remaining) == 0 {
        services.set_state(State::S0);
        services.call(Call::Fn1220fd0);
        return IDLE;
    }
    services.call(Call::Fn1220fd0);
    let status = services.call_with(CallWith::Fn1220bdc, 1);
    if status == 0 {
        return set(services, State::S1, 0);
    }
    services.set_state(State::S0);
    fail(services, status)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn1220ff0);
    services.set_state(State::S0);
    services.call(Call::Fn1220fe0);
    if status == 0 {
        return IDLE;
    }
    fail(services, status)
}
