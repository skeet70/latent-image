//! Machine 8. Its 10 actions (table indices 2..=11 of the action table at 0x0121a390) read no
//! guest data; they set other machines' state halfwords, call retained services and return the
//! dispatcher's result. None writes machine 8's own state halfword.

/// The state halfword of machine 8 (0x014ea5bc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum State {
    S0 = 0,
}

/// Retained services called without arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// 0x0121de50
    Fn121de50,
    /// 0x0121de64
    Fn121de64,
    /// 0x0121df20
    Fn121df20,
    /// 0x0121df30
    Fn121df30,
}

/// Retained services called with one word in r0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallWith {
    /// 0x0121de80
    Fn121de80,
    /// 0x0121ded4
    Fn121ded4,
}

/// Machines whose state halfword (0x014ea5ac + 2 * index) machine 8 sets, by index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Peer {
    Machine2 = 2,
    Machine9 = 9,
    Machine22 = 22,
    Machine34 = 34,
    Machine52 = 52,
    Machine53 = 53,
    Machine66 = 66,
    Machine70 = 70,
}

/// What machine 8 reaches outside itself.
pub trait Services {
    fn call(&mut self, call: Call);
    fn call_with(&mut self, call: CallWith, argument: u32);
    fn set_peer_state(&mut self, peer: Peer, state: u16);
}

const REFUSED: i32 = 255;

fn flag<S: Services>(services: &mut S, peer: Peer, call: Call) -> i32 {
    services.set_peer_state(peer, 1);
    services.call(call);
    0
}

fn submit_dpof<S: Services>(services: &mut S, argument: u32) -> i32 {
    services.set_peer_state(Peer::Machine34, 3);
    services.set_peer_state(Peer::Machine2, 5);
    services.call_with(CallWith::Fn121de80, argument);
    0
}

fn submit_transfer<S: Services>(services: &mut S, argument: u32) -> i32 {
    services.set_peer_state(Peer::Machine34, 9);
    services.set_peer_state(Peer::Machine66, 4);
    services.set_peer_state(Peer::Machine70, 8);
    services.call_with(CallWith::Fn121ded4, argument);
    0
}

pub fn action02<S: Services>(services: &mut S) -> i32 {
    flag(services, Peer::Machine9, Call::Fn121de50)
}

pub fn action03<S: Services>(services: &mut S) -> i32 {
    let result = submit_dpof(services, 0);
    if cfg!(playback_control_m8_action03_result_shifted) {
        result.wrapping_add(1)
    } else {
        result
    }
}

pub fn action04<S: Services>(services: &mut S) -> i32 {
    submit_dpof(services, 1)
}

pub fn action05<S: Services>(services: &mut S) -> i32 {
    submit_transfer(services, 0)
}

pub fn action06<S: Services>(services: &mut S) -> i32 {
    submit_transfer(services, 1)
}

pub fn action07<S: Services>(services: &mut S) -> i32 {
    flag(services, Peer::Machine22, Call::Fn121df20)
}

pub fn action08<S: Services>(services: &mut S) -> i32 {
    flag(services, Peer::Machine53, Call::Fn121df30)
}

pub fn action09<S: Services>(services: &mut S) -> i32 {
    flag(services, Peer::Machine52, Call::Fn121df30)
}

pub fn action10<S: Services>(services: &mut S) -> i32 {
    flag(services, Peer::Machine9, Call::Fn121de64)
}

pub fn action11<S: Services>(_services: &mut S) -> i32 {
    REFUSED
}
