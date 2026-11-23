//! Machine 5. Its 9 actions (table indices 2..=10 of the action table at 0x012175bc) call
//! retained JPEG services, read and write the JPEG block at 0x014eae2c, set the machine's state
//! halfword and return the dispatcher's result.

/// The state halfword of machine 5 (0x014ea5b6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
}

/// Retained services called without arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x012200c4
    Fn12200c4,
    /// 0x012200ec
    Fn12200ec,
    /// 0x01220120
    Fn1220120,
    /// 0x01220238
    Fn1220238,
    /// 0x0122026c
    Fn122026c,
    /// 0x01220294
    Fn1220294,
}

/// Words of the JPEG block at 0x014eae2c, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Word {
    Pending = 0x28,
    Phase = 0x58,
}

/// Bytes of the JPEG block at 0x014eae2c, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Byte {
    Flag = 0x60,
}

/// What machine 5 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call);
    fn set_state(&mut self, state: State);
    fn word(&self, word: Word) -> u32;
    fn set_word(&mut self, word: Word, value: u32);
    fn byte(&self, byte: Byte) -> u8;
}

const IDLE: i32 = 4;
const SETTLED: i32 = 9;
const CANCELLED: i32 = 12;
const PHASE_PENDING: u32 = 1;
const PHASE_CANCELLED: u32 = 2;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn concluded<S: Services>(services: &mut S) -> Option<i32> {
    if services.word(Word::Phase) == PHASE_CANCELLED {
        return Some(set(services, State::S0, CANCELLED));
    }
    if services.byte(Byte::Flag) == 1 {
        return Some(set(services, State::S0, IDLE));
    }
    None
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S1);
    services.call(Call::Fn12200c4);
    services.call(Call::Fn1220294);
    0
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn12200c4);
    services.call(Call::Fn12200ec);
    0
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    services.call(Call::Fn122026c);
    IDLE
}

pub fn action06<S: Services>(_services: &mut S) -> i32 {
    IDLE
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1220238);
    0
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1220238);
    services.set_word(Word::Phase, PHASE_CANCELLED);
    0
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    let result = action09_body(services);
    if cfg!(playback_control_m5_action09_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action09_body<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = concluded(services) {
        return result;
    }
    services.set_state(State::S0);
    services.call(Call::Fn122026c);
    if services.word(Word::Pending) != 0 {
        services.set_word(Word::Phase, PHASE_PENDING);
        return CANCELLED;
    }
    IDLE
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = concluded(services) {
        return result;
    }
    services.set_state(State::S2);
    services.call(Call::Fn1220120);
    SETTLED
}
