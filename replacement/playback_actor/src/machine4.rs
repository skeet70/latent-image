//! Machine 4. Its 18 actions (table indices 2..=19 of the action table at 0x0121a3c0) mark
//! storage completion bits through 0x0124a8d4, read and write the storage block at 0x01511338,
//! call retained services, set the machine's state halfword and return the dispatcher's result;
//! where stock returns the storage block's word at 0x01511348, the action returns that word.

/// The state halfword of machine 4 (0x014ea5b4).
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
    /// 0x0120e6e8
    Fn120e6e8,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x0123261c
    Fn123261c,
    /// 0x012390d4
    Fn12390d4,
    /// 0x01239178
    Fn1239178,
    /// 0x01249f84
    Fn1249f84,
    /// 0x01249fa0
    Fn1249fa0,
    /// 0x0124a010
    Fn124a010,
    /// 0x0124a0a8
    Fn124a0a8,
    /// 0x0124a0d4
    Fn124a0d4,
    /// 0x0124a3c0
    Fn124a3c0,
    /// 0x0124a42c
    Fn124a42c,
    /// 0x0124a480
    Fn124a480,
    /// 0x0124a4b0
    Fn124a4b0,
    /// 0x0124a518
    Fn124a518,
    /// 0x0124a54c
    Fn124a54c,
    /// 0x0124a628
    Fn124a628,
    /// 0x0124a650
    Fn124a650,
    /// 0x0124a66c
    Fn124a66c,
    /// 0x0124a688
    Fn124a688,
    /// 0x0124a698
    Fn124a698,
    /// 0x0124a8c0
    Fn124a8c0,
    /// 0x0124a8fc
    Fn124a8fc,
}

/// The completion bit 0x0124a8d4 ors into the byte at 0x01511338.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Completion {
    Bit0 = 1,
    Bit1 = 2,
    Bit2 = 4,
    Bit3 = 8,
}

/// Words of the storage block at 0x01511338, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Storage {
    Pending = 0x0c,
    Outcome = 0x10,
}

/// What machine 4 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    /// 0x0124a8d4 with the bit in r0.
    fn complete(&mut self, bit: Completion);
    fn set_state(&mut self, state: State);
    fn storage(&self, word: Storage) -> u32;
    fn set_storage(&mut self, word: Storage, value: u32);
    /// Stores 1 to the byte at 0x01511339.
    fn set_active(&mut self);
}

fn outcome<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    services.storage(Storage::Outcome) as i32
}

fn settle<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    services.call(Call::Fn12390d4);
    services.storage(Storage::Outcome) as i32
}

/// `checked` adds the 0x0124a518 test.
fn finish<S: Services>(services: &mut S, checked: bool) -> i32 {
    if services.call(Call::Fn124a0a8) == 0 {
        return 0;
    }
    let value = services.call(Call::Fn124a0d4) as u32;
    services.set_storage(Storage::Outcome, value);
    if checked && services.call(Call::Fn124a518) == 0 {
        return outcome(services);
    }
    if services.call(Call::Fn1239178) != 0 {
        return 0;
    }
    settle(services)
}

fn completed<S: Services>(services: &mut S, bit: Completion, checked: bool) -> i32 {
    services.complete(bit);
    finish(services, checked)
}

fn flush<S: Services>(services: &mut S, checked: bool) -> i32 {
    services.complete(Completion::Bit2);
    if services.storage(Storage::Pending) != 0 {
        services.call(Call::Fn124a4b0);
        services.call(Call::Fn120e6e8);
    }
    finish(services, checked)
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S1);
    services.call(Call::Fn1249f84);
    services.call(Call::Fn1249fa0);
    services.call(Call::Fn124a010);
    services.call(Call::Fn124a8fc);
    services.call(Call::Fn124a8c0);
    if services.call(Call::Fn124a54c) != 0 {
        services.call(Call::Fn124a650);
    } else {
        services.call(Call::Fn124a698);
    }
    0
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action04<S: Services>(_services: &mut S) -> i32 {
    8
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S2);
    services.set_active();
    0
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    let result = completed(services, Completion::Bit0, true);
    if cfg!(playback_control_m4_action06_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    completed(services, Completion::Bit0, false)
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    completed(services, Completion::Bit3, true)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    completed(services, Completion::Bit3, false)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn124a42c);
    services.call(Call::Fn123261c);
    if services.call(Call::Fn124a3c0) == 1 {
        services.call(Call::Fn124a480);
        return 0;
    }
    completed(services, Completion::Bit1, true)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    completed(services, Completion::Bit1, false)
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    completed(services, Completion::Bit1, true)
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn124a698);
    0
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    if services.call(Call::Fn124a628) != 0 {
        services.call(Call::Fn124a66c);
    } else {
        services.call(Call::Fn124a688);
    }
    0
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    flush(services, true)
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    flush(services, false)
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn120e6e8);
    0
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn120e6e8);
    if services.call(Call::Fn124a518) == 0 || services.call(Call::Fn124a0a8) == 0 {
        return 0;
    }
    settle(services)
}

pub fn action19<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn120e6e8);
    services.call(Call::Fn12390d4);
    if services.call(Call::Fn124a0a8) != 0 {
        return outcome(services);
    }
    0
}
