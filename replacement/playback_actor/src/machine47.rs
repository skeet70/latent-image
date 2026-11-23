//! Machine 47. Its 12 actions (table indices 2..=13 of the action table at 0x01217708) read the
//! word at 0x014ff874, call retained services, set the machine's state halfword and return the
//! dispatcher's result.

/// The state halfword of machine 47 (0x014ea60a).
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

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120fe64
    Fn120fe64,
    /// 0x0120ff70
    Fn120ff70,
    /// 0x012100d8
    Fn12100d8,
    /// 0x012136fc
    Fn12136fc,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0123a724
    Fn123a724,
    /// 0x0123b470
    Fn123b470,
    /// 0x0123c328
    Fn123c328,
    /// 0x0123c490
    Fn123c490,
    /// 0x0123c4c8
    Fn123c4c8,
    /// 0x0123cb4c
    Fn123cb4c,
    /// 0x0123cba8
    Fn123cba8,
    /// 0x0123cbe0
    Fn123cbe0,
    /// 0x0123cc18
    Fn123cc18,
    /// 0x0123ce14
    Fn123ce14,
    /// 0x0123cf48
    Fn123cf48,
    /// 0x0123d078
    Fn123d078,
    /// 0x0123d16c
    Fn123d16c,
    /// 0x0123d24c
    Fn123d24c,
    /// 0x0123d264
    Fn123d264,
    /// 0x0123d390
    Fn123d390,
    /// 0x0123e298
    Fn123e298,
}

/// Retained services called with one word in r0; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0123c2c8
    Fn123c2c8,
    /// 0x0123c2e8
    Fn123c2e8,
    /// 0x0123c308
    Fn123c308,
}

/// Words of the block at 0x014ff3c4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Block14ff3c4 {
    W4b0 = 0x4b0,
}

/// What machine 47 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32) -> i32;
    fn set_state(&mut self, state: State);
    fn block_14ff3c4(&self, word: Block14ff3c4) -> u32;
}

const IDLE: i32 = 4;
const FAILED: i32 = 12;
const PENDING: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Action02,
    Action09,
}

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn step<S: Services>(
    services: &mut S,
    start: Call,
    wait: Call,
    next: State,
    collect: Call,
    status: Option<u32>,
) -> i32 {
    services.call(start);
    if services.call(wait) == 0 {
        return set(services, next, 0);
    }
    fail(services, collect, status)
}

fn fail<S: Services>(services: &mut S, collect: Call, status: Option<u32>) -> i32 {
    services.set_state(State::S0);
    services.call(collect);
    if let Some(status) = status {
        services.call_with(CallWith::Fn123c308, status);
    }
    FAILED
}

fn report<S: Services>(services: &mut S, variant: Variant, code: u32) -> i32 {
    services.set_state(State::S0);
    if variant == Variant::Action09 {
        services.call_with(CallWith::Fn123c2e8, code);
    }
    FAILED
}

fn pipeline<S: Services>(services: &mut S, variant: Variant) -> i32 {
    let (third, sixth) = match variant {
        Variant::Action02 => (Call::Fn123cba8, Call::Fn123cbe0),
        Variant::Action09 => (Call::Fn123cbe0, Call::Fn123cba8),
    };
    match services.call(Call::Fn123cb4c) as u32 {
        0 | 5 => report(services, variant, 2),
        1 | 2 => step(services, Call::Fn123c490, Call::Fn120ff70, State::S1, Call::Fn123c4c8, None),
        3 => step(services, Call::Fn123ce14, Call::Fn120fe64, State::S2, third, Some(0)),
        4 | 7 => step(services, Call::Fn123cf48, Call::Fn120fe64, State::S5, Call::Fn123cba8, Some(1)),
        6 => step(services, Call::Fn123d16c, Call::Fn12136fc, State::S3, sixth, Some(0)),
        8 => step(services, Call::Fn123d078, Call::Fn12100d8, State::S4, Call::Fn123cc18, None),
        9 => report(services, variant, 12),
        _ => set(services, State::S0, FAILED),
    }
}

fn resolve<S: Services>(services: &mut S, result: i32, variant: Variant) -> i32 {
    if result == PENDING {
        pipeline(services, variant)
    } else {
        fail(services, Call::Fn123c4c8, None)
    }
}

fn settle<S: Services>(services: &mut S, wait: Call) -> i32 {
    if services.call(wait) == 0 {
        0
    } else {
        set(services, State::S0, IDLE)
    }
}

fn release<S: Services>(services: &mut S, wait: Call, collect: Call, status: Option<u32>) -> i32 {
    match services.call(wait) {
        0 => 0,
        PENDING => {
            let held = services.call(Call::Fn123d24c);
            services.set_state(State::S0);
            if held != 0 {
                services.call(Call::Fn123d264);
            }
            IDLE
        }
        _ => fail(services, collect, status),
    }
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn123c328);
    let result = services.call(Call::Fn120ff70);
    if result == 0 {
        return set(services, State::S1, 0);
    }
    resolve(services, result, Variant::Action02)
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(_services: &mut S) -> i32 {
    IDLE
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn123c2c8, 1);
    services.call(Call::Fn123a724);
    settle(services, Call::Fn120ff70)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn123d390);
    settle(services, Call::Fn120fe64)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn123e298);
    settle(services, Call::Fn12136fc)
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn123b470);
    settle(services, Call::Fn12100d8)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    let result = services.call(Call::Fn120ff70);
    if result == 0 {
        return 0;
    }
    if services.block_14ff3c4(Block14ff3c4::W4b0) == 1 {
        return set(services, State::S0, IDLE);
    }
    resolve(services, result, Variant::Action09)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    let result = release(services, Call::Fn120fe64, Call::Fn123cba8, Some(0));
    if cfg!(playback_control_m47_action10_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    match services.call(Call::Fn12136fc) {
        0 => 0,
        PENDING => set(services, State::S0, PENDING),
        _ => fail(services, Call::Fn123cbe0, Some(0)),
    }
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    release(services, Call::Fn12100d8, Call::Fn123cc18, None)
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    release(services, Call::Fn120fe64, Call::Fn123cba8, Some(1))
}
