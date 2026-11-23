//! Machine 70. Its 21 actions (table indices 2..=22 of the action table at 0x01216a10) read the
//! request block at 0x014ea6e4 and the sequence block at 0x01513154, call retained services, set
//! the machine's state halfword and return the dispatcher's result. Where stock returns the word
//! at 0x0151315c, the action returns that word.

/// The state halfword of machine 70 (0x014ea638).
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
    /// 0x00ec03dc
    Fn0ec03dc,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0122c468
    Fn122c468,
    /// 0x01233dcc
    Fn1233dcc,
    /// 0x01233e0c
    Fn1233e0c,
    /// 0x01251de8
    Fn1251de8,
    /// 0x01251e40
    Fn1251e40,
    /// 0x01251e80
    Fn1251e80,
    /// 0x01251ea8
    Fn1251ea8,
    /// 0x01251fc8
    Fn1251fc8,
    /// 0x0125202c
    Fn125202c,
    /// 0x012520ec
    Fn12520ec,
    /// 0x01252328
    Fn1252328,
    /// 0x0125233c
    Fn125233c,
    /// 0x01252430
    Fn1252430,
    /// 0x01252464
    Fn1252464,
    /// 0x01252490
    Fn1252490,
    /// 0x012524a0
    Fn12524a0,
}

/// Retained services called with one word in r0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0123265c
    Fn123265c,
    /// 0x01251c80
    Fn1251c80,
    /// 0x01251cd0
    Fn1251cd0,
    /// 0x01251e10
    Fn1251e10,
    /// 0x01251f24
    Fn1251f24,
    /// 0x01252064
    Fn1252064,
    /// 0x012520b0
    Fn12520b0,
    /// 0x01252374
    Fn1252374,
}

/// Words of the request block at 0x014ea6e4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Request {
    R034 = 0x034,
    R1e8 = 0x1e8,
}

/// Words of the sequence block at 0x01513154, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Word {
    W00 = 0x00,
    W08 = 0x08,
    W10 = 0x10,
    W14 = 0x14,
    W18 = 0x18,
    W1c = 0x1c,
}

/// Bytes of the sequence block at 0x01513154, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Byte {
    B26 = 0x26,
    B27 = 0x27,
}

/// What machine 70 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_state(&mut self, state: State);
    fn request(&self, word: Request) -> u32;
    fn word(&self, word: Word) -> u32;
    fn set_word(&mut self, word: Word, value: u32);
    fn byte(&self, byte: Byte) -> u8;
    fn set_byte(&mut self, byte: Byte, value: u8);
}

const IDLE: i32 = 4;
const DONE: i32 = 8;
const LAST: i32 = 6;
const CANCELLED: i32 = 12;
const CANCEL_CODE: u32 = 13;
const SLOT_EVENT: u32 = 46;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn enter<S: Services>(services: &mut S, state: State, call: Call) -> i32 {
    services.set_state(state);
    services.call(call);
    0
}

fn enter_with<S: Services>(services: &mut S, state: State, call: CallWith, argument: u32) -> i32 {
    services.set_state(state);
    services.call_with(call, argument);
    0
}

fn request_value<S: Services>(services: &mut S) -> i32 {
    enter_with(services, State::S4, CallWith::Fn1251e10, 1)
}

fn request_value_after_e0c<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S4);
    services.call(Call::Fn1233e0c);
    services.call_with(CallWith::Fn1251e10, 1);
    0
}

fn stop<S: Services>(services: &mut S) {
    services.set_state(State::S0);
    services.call(Call::Fn12520ec);
    services.call(Call::Fn1252328);
}

fn cancel<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn12520b0, CANCEL_CODE);
    CANCELLED
}

fn stop_cancel<S: Services>(services: &mut S) -> i32 {
    stop(services);
    cancel(services)
}

/// The word at 0x0151315c when the word at 0x01513154 is set.
fn finished<S: Services>(services: &mut S) -> Option<i32> {
    if services.word(Word::W00) != 0 {
        Some(services.word(Word::W08) as i32)
    } else {
        None
    }
}

fn increment<S: Services>(services: &mut S, word: Word) {
    let value = services.word(word);
    services.set_word(word, value.wrapping_add(1));
}

pub fn action02<S: Services>(_services: &mut S) -> i32 {
    DONE
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    set(services, State::S0, DONE)
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn1252064, 1);
    0
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1252490);
    action04(services)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    enter(services, State::S1, Call::Fn1251e40)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    enter(services, State::S1, Call::Fn1251ea8)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    match services.call(Call::Fn125202c) {
        0 => enter_with(services, State::S7, CallWith::Fn1251f24, 0),
        1 => {
            enter_with(services, State::S2, CallWith::Fn1251c80, 1);
            services.call_with(CallWith::Fn1251cd0, 0x7fff_ffff);
            0
        }
        _ => {
            enter_with(services, State::S5, CallWith::Fn1251c80, 2);
            services.call(Call::Fn1233dcc);
            services.call_with(CallWith::Fn123265c, 0);
            services.call(Call::Fn1251de8);
            0
        }
    }
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    enter(services, State::S1, Call::Fn1251fc8)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    enter(services, State::S1, Call::Fn1251e80)
}

fn slot<S: Services>(services: &mut S, chosen: impl FnOnce(&mut S) -> bool) -> i32 {
    if services.call(Call::Fn0ec03dc) != 0 {
        return action11(services);
    }
    services.set_state(State::S9);
    if chosen(services) {
        services.call(Call::Fn125233c);
    } else {
        services.call_with(CallWith::Fn1252374, 1);
    }
    0
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    slot(services, |services| services.request(Request::R034) == SLOT_EVENT)
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    slot(services, |services| services.call(Call::Fn122c468) != 0)
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    if services.word(Word::W00) != 0 {
        return request_value(services);
    }
    enter_with(services, State::S3, CallWith::Fn1251f24, 1)
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    if services.word(Word::W00) != 0 {
        return request_value_after_e0c(services);
    }
    enter_with(services, State::S6, CallWith::Fn1251f24, 2)
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    stop(services);
    services.call(Call::Fn12524a0);
    if let Some(result) = finished(services) {
        return result;
    }
    if services.request(Request::R1e8) != 0 {
        return cancel(services);
    }
    IDLE
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    if services.word(Word::W00) != 0 {
        return request_value(services);
    }
    match services.request(Request::R1e8) {
        0 => {
            let count = services.word(Word::W14);
            if services.word(Word::W10) >= count.wrapping_sub(1) {
                return request_value(services);
            }
            enter_with(services, State::S2, CallWith::Fn1251cd0, 1)
        }
        2 => {
            services.set_state(State::S4);
            services.set_byte(Byte::B26, 1);
            services.call_with(CallWith::Fn1251e10, 1);
            0
        }
        3 => {
            let count = services.word(Word::W14);
            if services.word(Word::W10) >= count.wrapping_sub(1) {
                if services.call(Call::Fn1252430) != 0 {
                    stop(services);
                    return LAST;
                }
                return request_value(services);
            }
            services.set_state(State::S2);
            increment(services, Word::W1c);
            services.call_with(CallWith::Fn1251cd0, 1);
            0
        }
        _ => stop_cancel(services),
    }
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    stop(services);
    if let Some(result) = finished(services) {
        return result;
    }
    if services.request(Request::R1e8) != 0 {
        return cancel(services);
    }
    if services.call(Call::Fn1252464) == 0 { IDLE } else { 5 }
}

fn step<S: Services>(services: &mut S, position: u32, counted: bool) -> i32 {
    services.set_state(State::S5);
    if counted {
        increment(services, Word::W1c);
    }
    services.set_word(Word::W18, position.wrapping_add(1));
    services.call_with(CallWith::Fn123265c, 1);
    services.call(Call::Fn1251de8);
    0
}

pub fn action19<S: Services>(services: &mut S) -> i32 {
    if services.word(Word::W00) != 0 {
        return request_value_after_e0c(services);
    }
    match services.request(Request::R1e8) {
        0 => {
            let position = services.word(Word::W18);
            if position < services.word(Word::W14).wrapping_sub(1) {
                return step(services, position, false);
            }
            request_value_after_e0c(services)
        }
        2 => {
            services.set_state(State::S4);
            services.call(Call::Fn1233e0c);
            services.set_byte(Byte::B26, 1);
            services.call_with(CallWith::Fn1251e10, 1);
            0
        }
        3 => {
            let position = services.word(Word::W18);
            if position < services.word(Word::W14).wrapping_sub(1) {
                return step(services, position, true);
            }
            if services.call(Call::Fn1252430) != 0 {
                stop(services);
                services.call(Call::Fn1233e0c);
                return LAST;
            }
            request_value_after_e0c(services)
        }
        _ => stop_cancel(services),
    }
}

pub fn action20<S: Services>(services: &mut S) -> i32 {
    stop(services);
    if let Some(result) = finished(services) {
        return result;
    }
    match services.request(Request::R1e8) {
        0 => IDLE,
        2 => 5,
        3 => LAST,
        _ => cancel(services),
    }
}

pub fn action21<S: Services>(services: &mut S) -> i32 {
    stop(services);
    let result = if services.byte(Byte::B27) == 0 { IDLE } else { CANCELLED };
    if cfg!(playback_control_m70_action21_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action22<S: Services>(services: &mut S) -> i32 {
    stop(services);
    if services.request(Request::R1e8) != 0 {
        return cancel(services);
    }
    if services.word(Word::W00) == 1 { 18 } else { IDLE }
}
