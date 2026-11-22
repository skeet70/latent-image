//! Machine 30, the playback transfer machine. Its 34 actions (table indices 2..=35 of the action
//! table at 0x01219888) work on the transfer block at 0x014eaed8 through retained services, set
//! the machine's state halfword and return the dispatcher's result. Where stock returns the r0 a
//! service left, the action returns that service's result.

use crate::DevicePacket;

/// The state halfword of machine 30 (0x014ea5e8).
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
    S13 = 13,
    S14 = 14,
    S15 = 15,
    S16 = 16,
    S17 = 17,
    S18 = 18,
    S19 = 19,
}

/// Retained services called without arguments; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0120bc20
    Dispatch3Events,
    /// 0x0120ce60
    Fn120ce60,
    /// 0x0120ec28
    Fn120ec28,
    /// 0x012101bc
    Dispatch46Events,
    /// 0x012119a4
    Fn12119a4,
    /// 0x01212a48
    Fn1212a48,
    /// 0x0121c9f8
    Fn121c9f8,
    /// 0x0121eb68, packet 0x91b
    Trace,
    /// 0x012205e4
    Fn12205e4,
    /// 0x0122586c
    Fn122586c,
    /// 0x012258bc
    Fn12258bc,
    /// 0x01225928
    Fn1225928,
    /// 0x01225a98
    Fn1225a98,
    /// 0x01225b20
    Fn1225b20,
    /// 0x01225bfc
    Fn1225bfc,
    /// 0x01225d08
    Fn1225d08,
    /// 0x01225f64
    Fn1225f64,
    /// 0x01225fb0
    Fn1225fb0,
    /// 0x01225fdc
    SelectSourceKind22,
    /// 0x01226014
    Fn1226014,
    /// 0x01226230
    Fn1226230,
    /// 0x012263ec
    Fn12263ec,
    /// 0x012264fc
    Fn12264fc,
    /// 0x01226518
    Fn1226518,
    /// 0x012265cc
    Fn12265cc,
    /// 0x01226684
    Fn1226684,
    /// 0x0122669c
    Fn122669c,
    /// 0x01229038
    FillControlModeFrame,
    /// 0x01229068
    Fn1229068,
    /// 0x0122925c
    PrepareSelectedSourceEvent,
    /// 0x012292a4
    PrepareSelectedTransferEvent,
    /// 0x012294cc
    Fn12294cc,
    /// 0x0122962c
    Fn122962c,
    /// 0x0122968c
    Fn122968c,
    /// 0x01229800
    Fn1229800,
    /// 0x01229824
    Fn1229824,
    /// 0x012298a4
    Fn12298a4,
    /// 0x01236698
    Fn1236698,
    /// 0x012377d4
    Fn12377d4,
    /// 0x0123c224
    Fn123c224,
    /// 0x0123c268
    Fn123c268,
    /// 0x012431b8
    Fn12431b8,
    /// 0x01243868
    Fn1243868,
    /// 0x01243e48
    Fn1243e48,
    /// 0x01243fa0
    Fn1243fa0,
    /// 0x012441f0
    Fn12441f0,
    /// 0x01243178 with the step block 0x014eaeec
    Fn1243178Step,
    /// 0x01242fa4 with the step block 0x014eaeec
    Fn1242fa4Step,
    /// 0x012293a8 with the step block 0x014eaeec
    Fn12293a8Step,
}

/// Retained services called with one word in r0; each yields the r0 it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x01225984, stores its argument at 0x014eaed8
    SetPhase,
    /// 0x012259a4
    StartStep,
    /// 0x012262fc
    Fn12262fc,
    /// 0x012296dc
    Fn12296dc,
    /// 0x01243198
    Fn1243198,
    /// 0x012441d0
    Fn12441d0,
}

/// Words of the transfer block at 0x014eaed8, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Word {
    Phase = 0x000,
    W004 = 0x004,
    Step = 0x014,
    W09c = 0x09c,
    W0a4 = 0x0a4,
    Event = 0x6c0,
    W6d0 = 0x6d0,
    W6d4 = 0x6d4,
    W6f4 = 0x6f4,
}

/// Bytes of the transfer block at 0x014eaed8, by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Byte {
    B6bd = 0x6bd,
    B6be = 0x6be,
}

/// What machine 30 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call) -> i32;
    fn call_with(&mut self, call: CallWith, argument: u32) -> i32;
    fn set_state(&mut self, state: State);
    fn word(&self, word: Word) -> u32;
    fn set_word(&mut self, word: Word, value: u32);
    fn byte(&self, byte: Byte) -> u8;
    fn set_byte(&mut self, byte: Byte, value: u8);
    /// The word at 0x014ea694.
    fn mode_value(&self) -> u32;
    /// Through 0x72b680.
    fn notify_device(&mut self, packet: &DevicePacket);
    /// Writes the request halfword at 0x014ea6e4 and the event word at 0x014ea710.
    fn post_request(&mut self, code: u16, event: u32);
}

const PHASE_START: u32 = 1;
const PHASE_SECOND: u32 = 2;
const STEP_PRIMARY: u32 = 8;
const STEP_SECONDARY: u32 = 10;
const PACKET_0820: u32 = 0x820;
const REQUEST_0119: u16 = 0x119;

fn set<S: Services>(services: &mut S, state: State, result: i32) -> i32 {
    services.set_state(state);
    result
}

fn restart<S: Services>(services: &mut S, step: u32, cleared: bool) -> i32 {
    services.set_state(State::S4);
    services.call_with(CallWith::StartStep, step);
    services.call(Call::Fn1225a98);
    if cleared {
        services.call(Call::Fn1243868);
    }
    0
}

fn reset<S: Services>(services: &mut S, cleared: bool) -> i32 {
    services.set_state(State::S0);
    services.call_with(CallWith::StartStep, STEP_PRIMARY);
    if cleared {
        services.call(Call::Fn1243868);
    }
    12
}

fn phase_gate<S: Services>(services: &mut S) -> Option<i32> {
    match services.word(Word::Phase) {
        PHASE_START => Some(set(services, State::S0, if cfg!(playback_control_m30_gate_swapped) { 4 } else { 8 })),
        PHASE_SECOND => Some(set(services, State::S0, 4)),
        _ => None,
    }
}

fn status_or<S: Services>(services: &mut S, status: i32, result: i32) -> i32 {
    if status != 0 {
        set(services, State::S0, result)
    } else {
        0
    }
}

fn transfer_tail<S: Services>(services: &mut S) -> i32 {
    if services.word(Word::W6d0) == 1 {
        if services.word(Word::W6f4) == 1 {
            services.call(Call::Fn1229824);
            if services.call(Call::Dispatch3Events) == 0 {
                return set(services, State::S14, 0);
            }
            return reset(services, false);
        }
        services.call(Call::Fn1226014);
        let status = services.call(Call::Fn120ce60);
        return set(services, if status == 12 { State::S0 } else { State::S16 }, status);
    }
    if services.word(Word::Event) == 73 {
        services.set_state(State::S12);
        return services.call(Call::Fn1212a48);
    }
    services.set_state(State::S15);
    services.call(Call::Fn1225f64);
    0
}

fn event_72_73<S: Services>(services: &mut S, idle: State) -> i32 {
    if services.word(Word::Event).wrapping_sub(72) <= 1 {
        services.call(Call::Fn1226230);
        if services.call(Call::Dispatch3Events) == 0 {
            return set(services, idle, 0);
        }
        return restart(services, STEP_PRIMARY, false);
    }
    services.set_state(State::S15);
    services.call(Call::Fn1225f64);
    0
}

fn machine46_restart<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1226518);
    if services.call(Call::Dispatch46Events) == 0 {
        return set(services, State::S5, 0);
    }
    restart(services, STEP_SECONDARY, true)
}

fn bind_transfer<S: Services>(services: &mut S) -> i32 {
    let bound = services.call(Call::Fn1243e48);
    services.set_word(Word::W6d4, bound as u32);
    if bound == 1 {
        services.set_byte(Byte::B6be, 0);
        services.call(Call::Fn1243fa0);
    } else {
        services.call(Call::Fn12294cc);
    }
    if services.call(Call::Dispatch3Events) == 0 {
        return set(services, State::S6, 0);
    }
    reset(services, true)
}

fn latch_09c<S: Services>(services: &mut S) {
    let value = services.word(Word::W09c);
    services.set_byte(Byte::B6bd, value as u8);
}

fn fn120ec28_status<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn120ec28);
    if status == 0 {
        return set(services, State::S19, 0);
    }
    if services.word(Word::Phase) == PHASE_START {
        return set(services, State::S0, 8);
    }
    if status == 4 {
        return set(services, State::S0, status);
    }
    restart(services, STEP_PRIMARY, true)
}

fn machine3_gated<S: Services>(services: &mut S, on_ready: fn(&mut S) -> i32) -> i32 {
    let status = services.call(Call::Dispatch3Events);
    if let Some(result) = phase_gate(services) {
        return result;
    }
    match status {
        4 => on_ready(services),
        12 => restart(services, STEP_PRIMARY, true),
        _ => 0,
    }
}

pub fn action02<S: Services>(_services: &mut S) -> i32 {
    8
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_START);
    services.call(Call::Fn123c224);
    let status = services.call(Call::Dispatch46Events);
    status_or(services, status, 8)
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_START);
    services.call(Call::Fn12205e4);
    let status = services.call(Call::Dispatch3Events);
    status_or(services, status, 8)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_START);
    0
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_START);
    services.call(Call::Fn1212a48);
    0
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_START);
    services.call(Call::Fn12119a4);
    0
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_START);
    fn120ec28_status(services)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_SECOND);
    services.call(Call::Fn123c224);
    let status = services.call(Call::Dispatch46Events);
    status_or(services, status, 4)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_SECOND);
    services.call(Call::Fn12205e4);
    let status = services.call(Call::Dispatch3Events);
    status_or(services, status, 4)
}

pub fn action11<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_SECOND);
    0
}

pub fn action12<S: Services>(services: &mut S) -> i32 {
    services.call_with(CallWith::SetPhase, PHASE_SECOND);
    services.call(Call::Fn12119a4);
    0
}

pub fn action13<S: Services>(services: &mut S) -> i32 {
    services.call(Call::PrepareSelectedTransferEvent);
    if services.word(Word::Event) == 72 {
        services.set_state(State::S12);
        return services.call(Call::Fn1212a48);
    }
    services.call(Call::Fn122586c);
    services.call(Call::SelectSourceKind22);
    services.call(Call::Fn12258bc);
    services.call_with(CallWith::Fn12441d0, 0);
    let event = services.word(Word::Event);
    if event == 19 || services.word(Word::W6d0) != 0 || event.wrapping_sub(243) <= 1 {
        return machine46_restart(services);
    }
    if services.call(Call::Fn1225fb0) != 0 {
        services.set_state(State::S18);
        services.call(Call::Fn1225928);
        return services.call(Call::Fn12119a4);
    }
    services.call(Call::Fn12263ec);
    if services.call(Call::Dispatch46Events) == 0 {
        return set(services, State::S1, 0);
    }
    restart(services, STEP_SECONDARY, true)
}

pub fn action14<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Trace);
    0
}

pub fn action15<S: Services>(services: &mut S) -> i32 {
    services.call(Call::PrepareSelectedSourceEvent);
    services.call(Call::SelectSourceKind22);
    if services.call(Call::Fn12431b8) == 1 {
        services.call(Call::Fn1236698);
        return bind_transfer(services);
    }
    services.call(Call::Fn122586c);
    services.call_with(CallWith::Fn12441d0, 1);
    machine46_restart(services)
}

pub fn action16<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S17);
    services.call(Call::Fn12265cc);
    0
}

pub fn action17<S: Services>(services: &mut S) -> i32 {
    let result = action17_body(services);
    if cfg!(playback_control_m30_action17_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

fn action17_body<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Dispatch46Events);
    if status == 0 {
        return 0;
    }
    latch_09c(services);
    if let Some(result) = phase_gate(services) {
        return result;
    }
    if status == 4 {
        if services.word(Word::Step) == 1 {
            services.call(Call::Fn123c268);
            return action17_tail(services);
        }
        services.call_with(CallWith::Fn12296dc, 1);
        if services.word(Word::Event).wrapping_sub(72) <= 1 {
            services.call(Call::Fn1226230);
            if services.call(Call::Dispatch3Events) == 0 {
                return set(services, State::S2, 0);
            }
            return restart(services, STEP_PRIMARY, false);
        }
        services.set_state(State::S3);
        services.call(Call::Fn1225f64);
        return 0;
    }
    let pending = services.call(Call::Fn123c268);
    if status == 12 && pending != 2 {
        return restart(services, STEP_SECONDARY, true);
    }
    action17_tail(services)
}

fn action17_tail<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn12264fc);
    machine46_restart(services)
}

pub fn action18<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn12377d4);
    if let Some(result) = phase_gate(services) {
        return result;
    }
    services.set_state(State::S15);
    services.call(Call::Fn1225f64);
    0
}

pub fn action19<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Dispatch46Events);
    if status == 0 {
        return 0;
    }
    if let Some(result) = phase_gate(services) {
        return result;
    }
    if status != 4 {
        return restart(services, STEP_SECONDARY, true);
    }
    latch_09c(services);
    services.call_with(CallWith::Fn1243198, 1);
    if services.call(Call::Fn12441f0) == 1 {
        return bind_transfer(services);
    }
    if services.call(Call::Fn122962c) != 0 {
        let event = services.word(Word::Event);
        services.post_request(REQUEST_0119, event);
        if services.call(Call::Fn120ec28) == 0 {
            return set(services, State::S19, 0);
        }
        return restart(services, STEP_PRIMARY, true);
    }
    services.call(Call::Fn12377d4);
    services.call(Call::Fn12294cc);
    if services.call(Call::Dispatch3Events) == 0 {
        return set(services, State::S6, 0);
    }
    restart(services, STEP_PRIMARY, true)
}

pub fn action20<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = phase_gate(services) {
        return result;
    }
    services.call(Call::Fn1243178Step);
    services.call(Call::Fn1242fa4Step);
    services.call(Call::Fn12293a8Step);
    if services.byte(Byte::B6bd) != 0 {
        services.call(Call::Fn1225bfc);
        if services.call(Call::Dispatch3Events) == 0 {
            return set(services, State::S10, 0);
        }
        return restart(services, STEP_PRIMARY, true);
    }
    if services.word(Word::W0a4) == 4 {
        services.set_state(State::S8);
        services.call(Call::FillControlModeFrame);
        return 0;
    }
    transfer_tail(services)
}

pub fn action21<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = phase_gate(services) {
        return result;
    }
    services.set_state(State::S9);
    services.call(Call::Fn1229068);
    0
}

pub fn action22<S: Services>(services: &mut S) -> i32 {
    transfer_tail(services)
}

pub fn action23<S: Services>(services: &mut S) -> i32 {
    machine3_gated(services, |services| {
        if services.byte(Byte::B6be) & 2 != 0 {
            services.call(Call::Fn1225d08);
            if services.call(Call::Dispatch3Events) == 0 {
                return set(services, State::S11, 0);
            }
            return restart(services, STEP_PRIMARY, true);
        }
        transfer_tail(services)
    })
}

pub fn action24<S: Services>(services: &mut S) -> i32 {
    machine3_gated(services, transfer_tail)
}

pub fn action25<S: Services>(services: &mut S) -> i32 {
    services.call(Call::Fn1212a48);
    if let Some(result) = phase_gate(services) {
        return result;
    }
    event_72_73(services, State::S13)
}

pub fn action26<S: Services>(services: &mut S) -> i32 {
    services.set_state(State::S0);
    services.call(Call::Fn1226684);
    if services.word(Word::Phase) == PHASE_START {
        8
    } else {
        4
    }
}

pub fn action27<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Fn12119a4);
    if let Some(result) = phase_gate(services) {
        return result;
    }
    if status != 0 {
        services.set_state(State::S0);
        services.call(Call::Fn122669c);
        return 4;
    }
    0
}

pub fn action28<S: Services>(services: &mut S) -> i32 {
    fn120ec28_status(services)
}

pub fn action29<S: Services>(services: &mut S) -> i32 {
    if let Some(result) = phase_gate(services) {
        return result;
    }
    if services.word(Word::Event) == 16 {
        services.set_state(State::S0);
        services.notify_device(&DevicePacket::new(PACKET_0820, services.mode_value()));
        return 4;
    }
    services.notify_device(&DevicePacket::new(PACKET_0820, services.mode_value()));
    services.call(Call::Fn12264fc);
    machine46_restart(services)
}

fn release<S: Services>(services: &mut S) {
    services.set_state(State::S0);
    services.call_with(CallWith::Fn12296dc, 0);
    services.set_word(Word::W6f4, 0);
}

fn direction<S: Services>(services: &S) -> i32 {
    if services.word(Word::W004) == 0 {
        10
    } else {
        11
    }
}

pub fn action30<S: Services>(services: &mut S) -> i32 {
    match services.word(Word::Phase) {
        PHASE_START => set(services, State::S0, 8),
        PHASE_SECOND => {
            release(services);
            4
        }
        _ => {
            release(services);
            direction(services)
        }
    }
}

pub fn action31<S: Services>(services: &mut S) -> i32 {
    if services.call(Call::Fn120ce60) == 0 {
        return 0;
    }
    services.set_state(State::S0);
    services.call(Call::Fn122968c);
    services.call(Call::Fn121c9f8);
    services.call_with(CallWith::Fn12296dc, 0);
    services.call(Call::Fn1229800);
    if services.word(Word::Phase) == PHASE_START {
        return 8;
    }
    direction(services)
}

pub fn action32<S: Services>(services: &mut S) -> i32 {
    machine3_gated(services, |services| {
        services.set_state(State::S3);
        let event = services.word(Word::Event);
        services.call_with(CallWith::Fn12262fc, event);
        0
    })
}

pub fn action33<S: Services>(services: &mut S) -> i32 {
    machine3_gated(services, |services| {
        services.set_state(State::S7);
        services.call(Call::Fn1225b20);
        0
    })
}

pub fn action34<S: Services>(services: &mut S) -> i32 {
    machine3_gated(services, |services| {
        services.set_state(State::S15);
        let event = services.word(Word::Event);
        services.call_with(CallWith::Fn12262fc, event);
        0
    })
}

pub fn action35<S: Services>(services: &mut S) -> i32 {
    let status = services.call(Call::Dispatch3Events);
    if let Some(result) = phase_gate(services) {
        return result;
    }
    match status {
        4 => {
            services.call(Call::Fn12298a4);
            services.call(Call::Fn1226014);
            let status = services.call(Call::Fn120ce60);
            set(services, if status == 12 { State::S0 } else { State::S16 }, status)
        }
        12 => reset(services, false),
        _ => 0,
    }
}
