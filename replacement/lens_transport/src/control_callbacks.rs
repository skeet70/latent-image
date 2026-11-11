//! Task 99 completion callbacks. Stock invokes the registered word directly (0x006c1870, 0x006c194c).

use crate::control_runtime::Shared;
use crate::control_types::{CallbackToken, CompletionStatus, ControlError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCallbackAddress(pub u32);
impl NativeCallbackAddress {
    /// # Safety
    /// The address is in a region admitted to `CallbackRegistry` and mapped executable by the RootPackage.
    pub unsafe fn invoke(self, status: CompletionStatus) {
        let callback: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(self.0 as usize) };
        unsafe { callback(status.0 as u32) };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCallbackUse { Register, Setup, Teardown, Synchronize, StartPolling, Resume, Suspend }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeRegion { start: u32, end: u32 }
impl CodeRegion {
    pub const fn new(start: u32, end: u32) -> Self { Self { start, end } }
    pub const fn contains(self, address: u32) -> bool { self.start < self.end && address >= self.start && address < self.end }
}

#[derive(Clone, Copy)]
pub struct AdmittedCode<'a> { regions: &'a [CodeRegion] }
impl<'a> AdmittedCode<'a> {
    pub const fn new(regions: &'a [CodeRegion]) -> Self { Self { regions } }
    pub fn contains(&self, address: u32) -> bool { self.regions.iter().any(|region| region.contains(address)) }
}

pub trait NativeCallbackRegistry {
    fn register(&self, address: NativeCallbackAddress, usage: NativeCallbackUse) -> Result<CallbackToken, ControlError>;
    fn release(&self, token: CallbackToken);
}

const CAPACITY: usize = 16;
const FIRST_TOKEN: u32 = 0x4000_0001;

#[derive(Clone, Copy)]
struct Entry { token: CallbackToken, address: NativeCallbackAddress, usage: NativeCallbackUse }
struct RegistryState { entries: [Option<Entry>; CAPACITY], next: u32 }

pub struct CallbackRegistry { code: AdmittedCode<'static>, state: Shared<RegistryState> }
impl CallbackRegistry {
    pub const fn new(code: AdmittedCode<'static>) -> Self {
        Self { code, state: Shared::new(RegistryState { entries: [None; CAPACITY], next: FIRST_TOKEN }) }
    }
    pub fn register(&self, address: NativeCallbackAddress, usage: NativeCallbackUse) -> Result<CallbackToken, ControlError> {
        if address.0 == 0 || !self.code.contains(address.0) { return Err(ControlError::InvalidPayload); }
        self.state.with(|state| {
            let slot = state.entries.iter().position(Option::is_none).ok_or(ControlError::Busy)?;
            if state.next >= 0x8000_0000 { return Err(ControlError::Busy); }
            let token = CallbackToken(state.next);
            state.next += 1;
            state.entries[slot] = Some(Entry { token, address, usage });
            Ok(token)
        })
    }
    pub fn resolve(&self, token: CallbackToken) -> Option<(NativeCallbackAddress, NativeCallbackUse)> {
        self.state.with(|state| state.entries.iter().flatten().find(|entry| entry.token == token).map(|entry| (entry.address, entry.usage)))
    }
    /// The stock handlers clear the callback word after invoking it.
    pub fn take(&self, token: CallbackToken) -> Option<(NativeCallbackAddress, NativeCallbackUse)> {
        self.state.with(|state| {
            let slot = state.entries.iter().position(|entry| entry.is_some_and(|entry| entry.token == token))?;
            state.entries[slot].take().map(|entry| (entry.address, entry.usage))
        })
    }
    pub fn release(&self, token: CallbackToken) -> bool { self.take(token).is_some() }
}
impl NativeCallbackRegistry for CallbackRegistry {
    fn register(&self, address: NativeCallbackAddress, usage: NativeCallbackUse) -> Result<CallbackToken, ControlError> {
        CallbackRegistry::register(self, address, usage)
    }
    fn release(&self, token: CallbackToken) { CallbackRegistry::release(self, token); }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: [CodeRegion; 1] = [CodeRegion::new(0x0060_0000, 0x0080_0000)];
    fn registry() -> CallbackRegistry { CallbackRegistry::new(AdmittedCode::new(&CODE)) }

    #[test]
    fn only_admitted_code_registers_and_tokens_are_distinct() {
        let registry = registry();
        assert_eq!(registry.register(NativeCallbackAddress(0), NativeCallbackUse::Setup), Err(ControlError::InvalidPayload));
        assert_eq!(registry.register(NativeCallbackAddress(0x0040_0000), NativeCallbackUse::Setup), Err(ControlError::InvalidPayload));
        let first = registry.register(NativeCallbackAddress(0x0060_1001), NativeCallbackUse::Resume).unwrap();
        let second = registry.register(NativeCallbackAddress(0x0060_1001), NativeCallbackUse::Suspend).unwrap();
        assert_ne!(first, second);
        assert_eq!(registry.resolve(first), Some((NativeCallbackAddress(0x0060_1001), NativeCallbackUse::Resume)));
        assert_eq!(registry.resolve(second), Some((NativeCallbackAddress(0x0060_1001), NativeCallbackUse::Suspend)));
    }

    #[test]
    fn completion_takes_the_registration_and_release_is_idempotent() {
        let registry = registry();
        let token = registry.register(NativeCallbackAddress(0x0060_2002), NativeCallbackUse::Synchronize).unwrap();
        assert_eq!(registry.take(token), Some((NativeCallbackAddress(0x0060_2002), NativeCallbackUse::Synchronize)));
        assert_eq!(registry.take(token), None);
        assert!(!registry.release(token));
        assert_eq!(registry.resolve(token), None);
    }

    #[test]
    fn released_slots_are_reusable_and_capacity_is_bounded() {
        use std::vec::Vec;
        let registry = registry();
        let first = registry.register(NativeCallbackAddress(0x0060_3003), NativeCallbackUse::StartPolling).unwrap();
        assert!(registry.release(first));
        let mut tokens = Vec::new();
        loop {
            match registry.register(NativeCallbackAddress(0x0060_3003 + tokens.len() as u32), NativeCallbackUse::Register) {
                Ok(token) => { assert!(token.0 < 0x8000_0000); tokens.push(token); }
                Err(ControlError::Busy) => break,
                Err(error) => panic!("unexpected registration failure: {error:?}"),
            }
        }
        assert_eq!(tokens.len(), 16);
        for token in tokens { assert!(registry.release(token)); }
        assert!(registry.register(NativeCallbackAddress(0x0060_4004), NativeCallbackUse::Register).is_ok());
    }
}
