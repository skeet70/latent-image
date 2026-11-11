use crate::{commit::{CallbackId, CommitPending, CompletionList, CompletionSlot, RuntimeState}, Error};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitDeferral(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitEvent(u8);
impl CommitEvent {
    pub const fn parse(value: u32) -> Option<Self> { if value <= 6 { Some(Self(value as u8)) } else { None } }
    fn slot(self) -> CompletionSlot {
        if self.0 == 6 { CompletionSlot::new(CompletionList::Second, 0).unwrap() }
        else { CompletionSlot::new(CompletionList::First, self.0 as usize).unwrap() }
    }
}
pub trait Critical {
    type Token;
    fn enter(&mut self) -> Self::Token;
    fn leave(&mut self, token: Self::Token);
}
pub trait Storage {
    fn initialized(&self) -> bool;
    fn callback(&self, slot: CompletionSlot) -> Option<CallbackId>;
    fn set_callback(&mut self, slot: CompletionSlot, callback: Option<CallbackId>);
    fn deferral(&self) -> CommitDeferral;
    fn request_commit(&mut self);
}
pub trait CallbackPacket { fn read(&mut self, index: usize) -> Option<CallbackId>; }
impl CallbackPacket for [Option<CallbackId>; 6] {
    fn read(&mut self, index: usize) -> Option<CallbackId> { self[index] }
}
pub struct QueuedRuntime<'a> { pub runtime: &'a mut RuntimeState, pub deferral: CommitDeferral }
impl Storage for QueuedRuntime<'_> {
    fn initialized(&self) -> bool { self.runtime.capture_count != 0 }
    fn callback(&self, slot: CompletionSlot) -> Option<CallbackId> { match slot.list {
        CompletionList::First => self.runtime.completion_callbacks[slot.index()],
        CompletionList::Second => self.runtime.secondary_callbacks[slot.index()],
    } }
    fn set_callback(&mut self, slot: CompletionSlot, callback: Option<CallbackId>) { match slot.list {
        CompletionList::First => self.runtime.completion_callbacks[slot.index()] = callback,
        CompletionList::Second => self.runtime.secondary_callbacks[slot.index()] = callback,
    } }
    fn deferral(&self) -> CommitDeferral { self.deferral }
    fn request_commit(&mut self) { self.runtime.pending = CommitPending(1); }
}
pub fn event(storage: &mut impl Storage, event: CommitEvent, callback: Option<CallbackId>, critical: &mut impl Critical) -> Result<(), Error> {
    if !storage.initialized() { return Err(Error::Uninitialized); }
    let token = critical.enter();
    let slot = event.slot();
    if storage.callback(slot).is_none() { storage.set_callback(slot, callback); }
    if storage.deferral().0 == 0 { storage.request_commit(); }
    critical.leave(token);
    Ok(())
}
pub fn packet<P: CallbackPacket>(storage: &mut impl Storage, mut packet: Option<&mut P>, critical: &mut impl Critical) -> Result<(), Error> {
    if !storage.initialized() { return Err(Error::Uninitialized); }
    let token = critical.enter();
    if let Some(source) = packet.as_mut() {
        for index in 0..6 {
            let slot = CompletionSlot::new(CompletionList::Second, index).unwrap();
            if storage.callback(slot).is_none() { storage.set_callback(slot, source.read(index)); }
        }
    }
    if storage.deferral().0 == 0 { storage.request_commit(); }
    critical.leave(token);
    Ok(())
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
mod native {
    use super::*;
    use crate::Engine;
    use core::ptr::{read_volatile, write_volatile};
    unsafe extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(token: u32);
    }
    struct Interrupts;
    impl Critical for Interrupts {
        type Token = u32;
        fn enter(&mut self) -> u32 { unsafe { xm1_power_interrupt_lock() } }
        fn leave(&mut self, token: u32) { unsafe { xm1_power_interrupt_unlock(token); } }
    }
    struct LiveRuntime { base: usize }
    impl LiveRuntime {
        fn new(engine: Engine) -> Self { Self { base: 0x9553cc + engine.index() * 0x58 } }
        fn slot_address(&self, slot: CompletionSlot) -> *mut u32 {
            (self.base + match slot.list { CompletionList::First => 0x24, CompletionList::Second => 0x3c } + slot.index() * 4) as *mut u32
        }
    }
    impl Storage for LiveRuntime {
        fn initialized(&self) -> bool { unsafe { read_volatile((self.base + 0x54) as *const u8) != 0 } }
        fn callback(&self, slot: CompletionSlot) -> Option<CallbackId> { decode(unsafe { read_volatile(self.slot_address(slot)) }) }
        fn set_callback(&mut self, slot: CompletionSlot, callback: Option<CallbackId>) { unsafe { write_volatile(self.slot_address(slot), callback.map_or(0, |id| id.0)); } }
        fn deferral(&self) -> CommitDeferral { CommitDeferral(unsafe { read_volatile((self.base + 0x10) as *const u32) }) }
        fn request_commit(&mut self) { unsafe { write_volatile((self.base + 0xc) as *mut u32, 1); } }
    }
    fn decode(value: u32) -> Option<CallbackId> { if value == 0 { None } else { Some(CallbackId(value)) } }
    struct LivePacket(*const u32);
    impl CallbackPacket for LivePacket {
        fn read(&mut self, index: usize) -> Option<CallbackId> { decode(unsafe { read_volatile(self.0.wrapping_add(index)) }) }
    }
    fn status(result: Result<(), Error>) -> i32 { result.map_or_else(Error::stock, |()| 0) }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_completion_event(engine: u32, event_id: u32, callback: u32) -> i32 {
        let Some(engine) = Engine::parse(engine) else { return Error::InvalidArgument.stock(); };
        let mut storage = LiveRuntime::new(engine);
        let Some(event_id) = CommitEvent::parse(event_id) else {
            return if storage.initialized() { Error::InvalidArgument.stock() } else { Error::Uninitialized.stock() };
        };
        status(event(&mut storage, event_id, decode(callback), &mut Interrupts))
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn xm1_scanout_completion_packet(engine: u32, callbacks: *const u32) -> i32 {
        let Some(engine) = Engine::parse(engine) else { return Error::InvalidArgument.stock(); };
        let mut source = LivePacket(callbacks);
        status(packet(&mut LiveRuntime::new(engine), if callbacks.is_null() { None } else { Some(&mut source) }, &mut Interrupts))
    }
}
