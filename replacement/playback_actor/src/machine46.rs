//! Machine 46. Its 14 actions (table indices 2..=15 of the action table at 0x01216ea0) read the
//! block at 0x014ff36c and the word its +0x10 points at, clear the request block's message code
//! halfword at 0x014ea6e4, call retained services, set the machine's state halfword and return
//! the dispatcher's result. Where stock returns the r0 a service left, the action returns that
//! service's result.

/// The state halfword of machine 46 (0x014ea608).
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
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0121ffc0
    Fn121ffc0,
    /// 0x012202d0
    Fn12202d0,
    /// 0x012205e4
    Fn12205e4,
    /// 0x0123b4f4
    Fn123b4f4,
    /// 0x0123b5a8
    Fn123b5a8,
    /// 0x0123b688
    Fn123b688,
    /// 0x0123b76c
    Fn123b76c,
    /// 0x0123b960
    Fn123b960,
    /// 0x0123ba6c
    Fn123ba6c,
    /// 0x0123bb74
    Fn123bb74,
    /// 0x0123bf2c
    Fn123bf2c,
}

/// Retained services called with one word in r0; their results are not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0123b4b4
    Fn123b4b4,
    /// 0x0123b4d4
    Fn123b4d4,
}

/// Words of the block at 0x014ff36c, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Word {
    Pending = 0x04,
    /// The address of the word action 13 advances by `Step`.
    Counter = 0x10,
    Step = 0x14,
    W50 = 0x50,
}

/// Bytes of the block at 0x014ff36c, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Byte {
    B54 = 0x54,
    B56 = 0x56,
}

/// What machine 46 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_state(&mut self, state: State);
    /// Stores halfword 0 at 0x014ea6e4.
    fn clear_request_code(&mut self);
    fn word(&self, word: Word) -> u32;
    fn byte(&self, byte: Byte) -> u8;
    /// The word at `address`, a pointer read from `Word::Counter`.
    fn word_at(&self, address: u32) -> u32;
    fn set_word_at(&mut self, address: u32, value: u32);
}

const IDLE: i32 = 4;
const FAILED: i32 = 12;
const PENDING: u32 = 1;
const B56_BIT: u8 = 2;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn idle<S: Services>(services: &mut S) -> i32 {
    set(services, State::S0, IDLE)
}

fn fail<S: Services>(services: &mut S, code: u32) -> i32 {
    services.call_with(CallWith::Fn123b4b4, code);
    FAILED
}

fn abort<S: Services>(services: &mut S, code: u32) -> i32 {
    services.set_state(State::S0);
    fail(services, code)
}

fn release<S: Services>(services: &mut S) {
    services.call_with(CallWith::Fn123b4d4, 1);
}

fn settle<S: Services>(services: &mut S, call: Call) -> i32 {
    if services.call(call) != 0 {
        idle(services)
    } else {
        0
    }
}

fn pending<S: Services>(services: &mut S) -> bool {
    services.word(Word::Pending) == PENDING
}

fn submit<S: Services>(services: &mut S, call: Call, state: State) -> i32 {
    services.call(call);
    if services.call(Call::Dispatch3Events) == 0 {
        set(services, state, 0)
    } else {
        abort(services, 4)
    }
}

fn begin<S: Services>(services: &mut S, state: State, call: Call) -> i32 {
    services.set_state(state);
    services.call(call);
    0
}

/// The nonzero status of `call` with nothing pending, or the action's result otherwise.
fn status<S: Services>(services: &mut S, call: Call) -> Result<i32, i32> {
    let status = services.call(call);
    if status == 0 {
        Err(0)
    } else if pending(services) {
        Err(idle(services))
    } else {
        Ok(status)
    }
}

fn second<S: Services>(services: &mut S) -> i32 {
    if services.call(Call::Fn123bf2c) == 0 {
        return abort(services, 2);
    }
    if services.call(Call::Fn123b5a8) == 0 {
        return fail(services, 2);
    }
    services.set_state(State::S2);
    services.call(Call::Fn120bfd8)
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    if services.word(Word::Step) == 0 {
        return second(services);
    }
    if services.call(Call::Fn123b4f4) == 0 {
        return fail(services, 1);
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
    release(services);
    services.call(Call::Fn12202d0);
    settle(services, Call::Fn120c1a8)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    release(services);
    services.call(Call::Fn121ffc0);
    settle(services, Call::Fn120bfd8)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    release(services);
    0
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    release(services);
    services.call(Call::Fn12205e4);
    settle(services, Call::Dispatch3Events)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    let result = action09_body(services);
    if cfg!(playback_control_m46_action09_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action09_body<S: Services>(services: &mut S) -> i32 {
    if pending(services) || services.word(Word::W50) == PENDING {
        return idle(services);
    }
    services.clear_request_code();
    submit(services, Call::Fn123b76c, State::S4)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    if pending(services) {
        return idle(services);
    }
    if services.byte(Byte::B54) != 0 {
        services.clear_request_code();
        return submit(services, Call::Fn123ba6c, State::S6);
    }
    if services.byte(Byte::B56) & B56_BIT != 0 {
        services.clear_request_code();
        return submit(services, Call::Fn123bb74, State::S7);
    }
    idle(services)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    match status(services, Call::Dispatch3Events) {
        Ok(4) if services.byte(Byte::B56) & B56_BIT != 0 => {
            submit(services, Call::Fn123bb74, State::S7)
        }
        Ok(4) => idle(services),
        Ok(_) => abort(services, 4),
        Err(result) => result,
    }
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    match status(services, Call::Dispatch3Events) {
        Ok(4) => idle(services),
        Ok(_) => abort(services, 4),
        Err(result) => result,
    }
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    match status(services, Call::Fn120c1a8) {
        Ok(4) => {
            let counter = services.word(Word::Counter);
            let value = services.word_at(counter);
            let step = services.word(Word::Step);
            services.set_word_at(counter, value.wrapping_add(step));
            second(services)
        }
        Ok(_) => abort(services, 1),
        Err(result) => result,
    }
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    match status(services, Call::Fn120bfd8) {
        Ok(4) => begin(services, State::S3, Call::Fn123b688),
        Ok(_) => abort(services, 3),
        Err(result) => result,
    }
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    match status(services, Call::Dispatch3Events) {
        Ok(4) => begin(services, State::S5, Call::Fn123b960),
        Ok(_) => abort(services, 4),
        Err(result) => result,
    }
}
