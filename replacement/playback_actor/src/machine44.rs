//! Machine 44. Its 19 actions (table indices 2..=20 of the action table at 0x01217c84) read the
//! stream block at 0x014ff894 and the request halfword at 0x014ea6e4, call retained services, set
//! the machine's state halfword and return the dispatcher's result. Where stock returns the r0 a
//! service left, the action returns that service's result.

/// The state halfword of machine 44 (0x014ea604).
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
    S10 = 10,
    S11 = 11,
    S12 = 12,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120bc20
    Dispatch3Events,
    /// 0x0120bfd8
    Fn120bfd8,
    /// 0x0120c1a8
    Fn120c1a8,
    /// 0x0121eb68
    Trace,
    /// 0x0121ff9c
    Fn121ff9c,
    /// 0x0121ffc0
    Fn121ffc0,
    /// 0x012202d0
    Fn12202d0,
    /// 0x012205e4
    Fn12205e4,
    /// 0x0123d414
    Fn123d414,
    /// 0x0123d454
    Fn123d454,
    /// 0x0123d4d0
    Fn123d4d0,
    /// 0x0123d54c
    Fn123d54c,
    /// 0x0123d584
    Fn123d584,
    /// 0x0123d87c
    Fn123d87c,
    /// 0x0123d89c
    Fn123d89c,
    /// 0x0123d8bc
    Fn123d8bc,
    /// 0x0123da10
    Fn123da10,
    /// 0x0123db1c
    Fn123db1c,
    /// 0x0123dc24
    Fn123dc24,
    /// 0x0123dd1c
    Fn123dd1c,
    /// 0x0123dd38
    Fn123dd38,
    /// 0x0123ddc0
    Fn123ddc0,
    /// 0x0123de08
    Fn123de08,
    /// 0x0123de38
    Fn123de38,
    /// 0x0123df40
    Fn123df40,
    /// 0x0123e084
    Fn123e084,
    /// 0x0123e098
    Fn123e098,
}

/// Retained services called with one word in r0; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x012293a8
    Fn12293a8,
    /// 0x0123d3d4
    Fn123d3d4,
    /// 0x0123d3f4
    Fn123d3f4,
}

/// Words of the stream block at 0x014ff894, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Word {
    /// A pointer to a word.
    Current = 0x00,
    W44 = 0x44,
    /// A pointer to a word.
    Target = 0x4c,
    W58 = 0x58,
    W5c = 0x5c,
}

/// Bytes of the stream block at 0x014ff894, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Byte {
    B64 = 0x64,
    B7c = 0x7c,
}

/// What machine 44 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32) -> i32;
    fn set_state(&mut self, state: State);
    fn word(&self, word: Word) -> u32;
    fn byte(&self, byte: Byte) -> u8;
    /// The word at `address`, a pointer read from the stream block.
    fn word_at(&self, address: u32) -> u32;
    fn set_word_at(&mut self, address: u32, value: u32);
    /// The request halfword at 0x014ea6e4.
    fn request_code(&self) -> u16;
}

const IDLE: i32 = 4;
const FAILED: i32 = 12;
const SET: u32 = 1;
const CURRENT_SECOND: u32 = 2;
const B7C_MASK: u8 = 2;
const REQUEST_1410: u16 = 0x1410;
const STATUS_4: i32 = 4;
const STATUS_9: i32 = 9;
const CODE_1: u32 = 1;
const CODE_9: u32 = 9;
const CODE_11: u32 = 11;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn fail<S: Services>(services: &mut S, code: u32) -> i32 {
    services.set_state(State::S0);
    services.call_with(CallWith::Fn123d3f4, code);
    FAILED
}

fn abandon<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    services.call_with(CallWith::Fn123d3f4, CODE_1);
    services.call(Call::Fn121ff9c);
    services.call(Call::Fn120bfd8);
    FAILED
}

fn busy<S: Services>(services: &mut S) -> bool {
    if services.word(Word::W44) == SET {
        services.set_state(State::S0);
        true
    } else {
        false
    }
}

fn settle<S: Services>(services: &mut S, status: i32) -> i32 {
    if status != 0 {
        set(services, State::S0, IDLE)
    } else {
        0
    }
}

fn resume<S: Services>(services: &mut S) -> i32 {
    let current = services.word(Word::Current);
    if services.word_at(current) == CURRENT_SECOND {
        services.set_state(State::S4);
        services.call(Call::Fn123ddc0);
        0
    } else {
        services.set_state(State::S2);
        services.call(Call::Fn123d89c);
        services.call(Call::Fn120bfd8)
    }
}

fn step<S: Services>(services: &mut S, state: State) -> i32 {
    if services.call(Call::Dispatch3Events) == 0 {
        set(services, state, 0)
    } else {
        fail(services, CODE_11)
    }
}

fn mark<S: Services>(services: &mut S) {
    services.call_with(CallWith::Fn123d3d4, SET);
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    if services.word(Word::W58) == SET || services.call(Call::Fn123d454) == 0 {
        return resume(services);
    }
    if services.call(Call::Fn123d4d0) == 0 {
        services.call_with(CallWith::Fn123d3f4, CODE_1);
        return FAILED;
    }
    services.set_state(State::S1);
    services.call(Call::Fn120c1a8)
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(_services: &mut S) -> i32 {
    IDLE
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    mark(services);
    services.call(Call::Fn12202d0);
    let status = services.call(Call::Fn120c1a8);
    settle(services, status)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    mark(services);
    services.call(Call::Fn121ffc0);
    let status = services.call(Call::Fn120bfd8);
    settle(services, status)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    mark(services);
    services.call(Call::Fn121ff9c);
    services.call(Call::Fn120bfd8);
    services.call(Call::Fn12202d0);
    let status = services.call(Call::Fn120c1a8);
    settle(services, status)
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    mark(services);
    0
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    mark(services);
    services.call(Call::Fn12205e4);
    let status = services.call(Call::Dispatch3Events);
    settle(services, status)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    if busy(services) {
        return IDLE;
    }
    if services.word(Word::W5c) == SET {
        return set(services, State::S0, IDLE);
    }
    services.call(Call::Fn123d8bc);
    step(services, State::S9)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    if busy(services) {
        return IDLE;
    }
    services.set_state(State::S8);
    services.call(Call::Fn123de38);
    0
}

fn select_b7c<S: Services>(services: &mut S) -> i32 {
    if services.byte(Byte::B7c) & B7C_MASK != 0 {
        services.call(Call::Fn123dc24);
        step(services, State::S12)
    } else {
        set(services, State::S0, IDLE)
    }
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    if busy(services) {
        return IDLE;
    }
    services.call(Call::Fn123dd1c);
    let current = services.word(Word::Current);
    services.call_with(CallWith::Fn12293a8, current);
    if services.call(Call::Fn123d414) != 0 {
        services.call(Call::Fn123dd38);
    }
    if services.byte(Byte::B64) != 0 {
        services.call(Call::Fn123db1c);
        return step(services, State::S11);
    }
    select_b7c(services)
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Dispatch3Events);
    if status == 0 {
        return 0;
    }
    if busy(services) {
        return IDLE;
    }
    if status == STATUS_4 {
        select_b7c(services)
    } else {
        fail(services, CODE_11)
    }
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Dispatch3Events);
    if status == 0 {
        return 0;
    }
    if busy(services) {
        return IDLE;
    }
    if status == STATUS_4 {
        set(services, State::S0, status)
    } else {
        fail(services, CODE_11)
    }
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120bfd8);
    if status == 0 {
        return 0;
    }
    if busy(services) {
        return IDLE;
    }
    match status {
        STATUS_4 => {
            if services.call(Call::Fn123de08) != 0 {
                services.set_state(State::S7);
                services.call(Call::Fn123df40);
            } else {
                services.set_state(State::S8);
                services.call(Call::Fn123de38);
            }
            0
        }
        STATUS_9 => {
            if services.call(Call::Fn123d584) != 0 {
                services.set_state(State::S3);
                let target = services.word(Word::Target);
                services.set_word_at(target, 0);
                services.call(Call::Fn120c1a8)
            } else {
                abandon(services)
            }
        }
        _ => fail(services, CODE_9),
    }
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    let result = action16_body(services);
    if cfg!(playback_control_m44_action16_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action16_body<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120c1a8);
    if status == 0 {
        return 0;
    }
    if busy(services) {
        return IDLE;
    }
    if status == STATUS_4 {
        services.call(Call::Fn123d54c);
        resume(services)
    } else {
        fail(services, CODE_1)
    }
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    if services.request_code() != REQUEST_1410 {
        return 0;
    }
    let status = services.call(Call::Fn120c1a8);
    if status == 0 {
        return 0;
    }
    if busy(services) {
        return IDLE;
    }
    if status == STATUS_4 {
        services.set_state(State::S2);
        services.call(Call::Fn123d87c);
        services.call(Call::Fn120bfd8)
    } else {
        abandon(services)
    }
}

/// `None` when 0x0120bc20 returned 4.
fn dispatched<S: Services>(services: &mut S) -> Option<i32> {
    let status = services.call(Call::Dispatch3Events);
    if status == 0 {
        Some(0)
    } else if busy(services) {
        Some(IDLE)
    } else if status == STATUS_4 {
        None
    } else {
        Some(fail(services, CODE_11))
    }
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = dispatched(services) {
        return result;
    }
    services.call(Call::Fn123e084);
    step(services, State::S6)
}

pub fn action19<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = dispatched(services) {
        return result;
    }
    services.set_state(State::S10);
    services.call(Call::Fn123da10);
    0
}

pub fn action20<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = dispatched(services) {
        return result;
    }
    services.set_state(State::S10);
    services.call(Call::Fn123e098);
    services.call(Call::Fn123da10);
    0
}
