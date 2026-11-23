//! Machine 45. Its 9 actions (table indices 2..=10 of the action table at 0x01217b48) read the
//! overlay-47 block at 0x014ff2f4 and a word through its pointer at offset 0x50, call retained
//! services, set the machine's state halfword and return the dispatcher's result.

/// The state halfword of machine 45 (0x014ea606).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
    S1 = 1,
    S2 = 2,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120c1a8
    Fn120c1a8,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x012202d0
    Fn12202d0,
    /// 0x0123a80c
    Fn123a80c,
    /// 0x0123aa04
    Fn123aa04,
    /// 0x0123aa84
    Fn123aa84,
    /// 0x0123ab00
    Fn123ab00,
    /// 0x0123b228
    Fn123b228,
}

/// Retained services called with one word in r0 whose result no action reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0123a7cc, stores its argument at 0x014ff2f8
    Fn123a7cc,
    /// 0x0123a7ec, stores its argument at 0x014ff31c
    Fn123a7ec,
    /// 0x0123ab84
    Fn123ab84,
}

/// Words of the overlay-47 block at 0x014ff2f4, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Overlay47 {
    W004 = 0x004,
    /// A pointer; actions read the signed word at its offset 0x434.
    Pointer = 0x050,
}

/// What machine 45 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_state(&mut self, state: State);
    fn overlay47(&self, word: Overlay47) -> u32;
    /// The word at `address`, through the overlay-47 pointer.
    fn word_at(&self, address: u32) -> u32;
}

const IDLE: i32 = 4;
const CANCELLED: i32 = 12;
const POINTEE_WORD: u32 = 0x434;

fn idle<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    IDLE
}

fn fail<S: Services>(services: &mut S, code: u32) -> i32 {
    services.set_state(State::S0);
    services.call_with(CallWith::Fn123a7ec, code);
    CANCELLED
}

fn settle<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn123b228) as u32;
    if status.wrapping_sub(1) <= 1 {
        idle(services)
    } else {
        fail(services, 2)
    }
}

fn begin<S: Services>(services: &mut S, ready: i32) -> i32 {
    if ready != 0 {
        services.set_state(State::S2);
        services.call(Call::Fn120c1a8)
    } else {
        fail(services, 1)
    }
}

/// The shared head of actions 9 and 10: `Err` holds the result of a path that ends there.
fn resume<S: Services>(services: &mut S, which: u32) -> Result<(), i32> {
    let status = services.call(Call::Fn120c1a8);
    if status == 0 {
        return Err(0);
    }
    if services.overlay47(Overlay47::W004) == 1 {
        return Err(idle(services));
    }
    if status != 4 {
        return Err(fail(services, 1));
    }
    services.call_with(CallWith::Fn123ab84, which);
    Ok(())
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    match services.call(Call::Fn123a80c) as u32 {
        0 => fail(services, 1),
        1 => {
            services.set_state(State::S1);
            services.call(Call::Fn120c1a8)
        }
        _ => settle(services),
    }
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    settle(services)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    if services.call(Call::Fn123aa04) != 0 {
        let ready = services.call(Call::Fn123aa84);
        begin(services, ready)
    } else {
        settle(services)
    }
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    let pointer = services.overlay47(Overlay47::Pointer);
    if (services.word_at(pointer.wrapping_add(POINTEE_WORD)) as i32) > 0 {
        let ready = services.call(Call::Fn123ab00);
        return begin(services, ready);
    }
    match services.call(Call::Fn123b228) {
        1 => idle(services),
        2 => {
            services.call(Call::Fn123aa04);
            let ready = services.call(Call::Fn123aa84);
            begin(services, ready)
        }
        _ => fail(services, 2),
    }
}

pub fn action07<S: Services>(_services: &mut S) -> i32 {
    IDLE
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::Fn123a7cc, 1);
    services.call(Call::Fn12202d0);
    if services.call(Call::Fn120c1a8) != 0 {
        idle(services)
    } else {
        0
    }
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    let result = match resume(services, 0) {
        Ok(()) => settle(services),
        Err(result) => result,
    };
    if cfg!(playback_control_m45_action09_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    if let Err(result) = resume(services, 1) {
        return result;
    }
    if services.call(Call::Fn123b228) == 1 {
        idle(services)
    } else {
        fail(services, 2)
    }
}
