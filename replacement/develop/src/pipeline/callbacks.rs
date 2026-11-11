//! Typed admission of the code words stock keeps in interface tables and callback slots.
//!
//! Stock enters those words with `blx`.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackWord(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackToken(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackShape { None, Task, ArgumentStatus, PairStatus, WaitStatus, SignedArgument, Reclaim }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackError { Invalid, Unadmitted, Shape, Full }
impl CallbackError {
    /// The status an ABI entry reports when the registry refuses its word.
    pub const fn status(self) -> i32 { -255 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeRegion { start: u32, end: u32 }
impl CodeRegion {
    pub const fn new(start: u32, end: u32) -> Self { Self { start, end } }
    pub const fn contains(self, address: u32) -> bool { self.start < self.end && address >= self.start && address < self.end }
}

/// # Safety
/// `SHAPE` must name the argument and result layout of the implementing type exactly.
pub unsafe trait CallbackFn: Copy {
    const SHAPE: CallbackShape;
    fn word(self) -> CallbackWord;
    fn address(self) -> usize;
    /// # Safety
    /// `address` was returned by `resolve` for this shape or came from an admitted region.
    unsafe fn from_address(address: usize) -> Self;
}

macro_rules! callback_functions {
    ($($shape:ident => $signature:ty),* $(,)?) => {
        $(
            unsafe impl CallbackFn for $signature {
                const SHAPE: CallbackShape = CallbackShape::$shape;
                fn word(self) -> CallbackWord { CallbackWord(self as usize as u32) }
                fn address(self) -> usize { self as usize }
                unsafe fn from_address(address: usize) -> Self { unsafe { core::mem::transmute(address) } }
            }
        )*
    };
}

callback_functions! {
    None => unsafe extern "C" fn(),
    Task => unsafe extern "C" fn() -> u32,
    ArgumentStatus => unsafe extern "C" fn(u32) -> i32,
    PairStatus => unsafe extern "C" fn(u32, u32) -> i32,
    WaitStatus => unsafe extern "C" fn(u32, u32, u32, *mut u32, i32) -> i32,
    SignedArgument => unsafe extern "C" fn(i32),
    Reclaim => unsafe extern "C" fn(u32, u32, u32) -> i32,
}

struct IrqMask {
    #[cfg(target_arch = "arm")]
    saved: u32,
}
impl IrqMask {
    fn enter() -> Self {
        #[cfg(target_arch = "arm")]
        {
            let saved: u32;
            unsafe { core::arch::asm!("mrs {saved}, cpsr", "cpsid if", saved = out(reg) saved, options(nostack, preserves_flags)); }
            Self { saved }
        }
        #[cfg(not(target_arch = "arm"))]
        Self {}
    }
}
impl Drop for IrqMask {
    fn drop(&mut self) {
        #[cfg(target_arch = "arm")]
        unsafe {
            if self.saved & 0x40 == 0 { core::arch::asm!("cpsie f", options(nostack, preserves_flags)); }
            if self.saved & 0x80 == 0 { core::arch::asm!("cpsie i", options(nostack, preserves_flags)); }
        }
    }
}

struct Shared<T> { locked: AtomicU32, value: UnsafeCell<T> }
unsafe impl<T: Send> Sync for Shared<T> {}
impl<T> Shared<T> {
    const fn new(value: T) -> Self { Self { locked: AtomicU32::new(0), value: UnsafeCell::new(value) } }
    fn with<R>(&self, operation: impl FnOnce(&mut T) -> R) -> R {
        let irq = IrqMask::enter();
        while self.locked.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() { core::hint::spin_loop(); }
        struct Unlock<'a>(&'a AtomicU32);
        impl Drop for Unlock<'_> { fn drop(&mut self) { self.0.store(0, Ordering::Release); } }
        let unlock = Unlock(&self.locked);
        let result = operation(unsafe { &mut *self.value.get() });
        drop(unlock);
        drop(irq);
        result
    }
}

const ENTRIES: usize = 64;
const REGIONS: usize = 4;
const FIRST_TOKEN: u32 = 0x4000_0001;

#[derive(Clone, Copy)]
struct CallbackEntry { token: CallbackToken, word: CallbackWord, address: usize, shape: CallbackShape }
struct State { entries: [Option<CallbackEntry>; ENTRIES], regions: [Option<CodeRegion>; REGIONS], next: u32 }

pub struct CallbackRegistry { state: Shared<State> }
impl CallbackRegistry {
    pub const fn new() -> Self {
        Self { state: Shared::new(State { entries: [None; ENTRIES], regions: [None; REGIONS], next: FIRST_TOKEN }) }
    }
    /// Admits a stock code region supplied by the composition.
    pub fn admit(&self, region: CodeRegion) -> Result<(), CallbackError> {
        if region.start >= region.end { return Err(CallbackError::Invalid); }
        self.state.with(|state| {
            if state.regions.iter().flatten().any(|admitted| *admitted == region) { return Ok(()); }
            let slot = state.regions.iter().position(Option::is_none).ok_or(CallbackError::Full)?;
            state.regions[slot] = Some(region);
            Ok(())
        })
    }
    pub fn register<F: CallbackFn>(&self, callback: F) -> Result<CallbackToken, CallbackError> {
        let word = callback.word();
        if word.0 == 0 { return Err(CallbackError::Invalid); }
        self.state.with(|state| {
            let slot = state.entries.iter().position(Option::is_none).ok_or(CallbackError::Full)?;
            if state.next >= 0x8000_0000 { return Err(CallbackError::Full); }
            let token = CallbackToken(state.next);
            state.next += 1;
            state.entries[slot] = Some(CallbackEntry { token, word, address: callback.address(), shape: F::SHAPE });
            Ok(token)
        })
    }
    pub fn ensure<F: CallbackFn>(&self, callback: F) -> Result<(), CallbackError> {
        if self.resolve::<F>(callback.word()).is_ok() { return Ok(()); }
        self.register(callback).map(|_| ())
    }
    /// Records a word the guest supplies, such as an interface-table entry, for one shape.
    pub fn admit_word<F: CallbackFn>(&self, word: CallbackWord) -> Result<(), CallbackError> {
        if word.0 == 0 { return Err(CallbackError::Invalid); }
        self.state.with(|state| {
            if let Some(entry) = state.entries.iter().flatten().find(|entry| entry.word == word) {
                return if entry.shape == F::SHAPE { Ok(()) } else { Err(CallbackError::Shape) };
            }
            let slot = state.entries.iter().position(Option::is_none).ok_or(CallbackError::Full)?;
            if state.next >= 0x8000_0000 { return Err(CallbackError::Full); }
            let token = CallbackToken(state.next);
            state.next += 1;
            state.entries[slot] = Some(CallbackEntry { token, word, address: word.0 as usize, shape: F::SHAPE });
            Ok(())
        })
    }
    pub fn release(&self, token: CallbackToken) -> bool {
        self.state.with(|state| {
            let slot = state.entries.iter().position(|entry| entry.is_some_and(|entry| entry.token == token));
            slot.is_some_and(|slot| state.entries[slot].take().is_some())
        })
    }
    /// Returns the entry for `word` only when an owned client registered it for this shape or the
    /// composition admitted the region that contains it.
    pub fn resolve<F: CallbackFn>(&self, word: CallbackWord) -> Result<F, CallbackError> {
        if word.0 == 0 { return Err(CallbackError::Invalid); }
        self.state.with(|state| {
            if let Some(entry) = state.entries.iter().flatten().find(|entry| entry.word == word && entry.shape == F::SHAPE) {
                return Ok(unsafe { F::from_address(entry.address) });
            }
            if state.entries.iter().flatten().any(|entry| entry.word == word) { return Err(CallbackError::Shape); }
            if state.regions.iter().flatten().any(|region| region.contains(word.0)) {
                return Ok(unsafe { F::from_address(word.0 as usize) });
            }
            Err(CallbackError::Unadmitted)
        })
    }
}
impl Default for CallbackRegistry {
    fn default() -> Self { Self::new() }
}

static REGISTRY: CallbackRegistry = CallbackRegistry::new();

/// The develop unit's own stock extents; stock installs crop and font callbacks from there.
pub const DEVELOP_STOCK_REGION: CodeRegion = CodeRegion::new(0x00e4_0000, 0x00eb_2000);

static ADMITTED: AtomicU32 = AtomicU32::new(0);

pub fn registry() -> &'static CallbackRegistry {
    if ADMITTED.swap(1, Ordering::AcqRel) == 0 { let _ = REGISTRY.admit(DEVELOP_STOCK_REGION); }
    &REGISTRY
}

pub fn call_no_arguments(registry: &CallbackRegistry, word: CallbackWord) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn() = registry.resolve(word)?;
    unsafe { callback() };
    Ok(())
}

pub fn call_task(registry: &CallbackRegistry, word: CallbackWord) -> Result<u32, CallbackError> {
    let callback: unsafe extern "C" fn() -> u32 = registry.resolve(word)?;
    Ok(unsafe { callback() })
}

pub fn call_argument_status(registry: &CallbackRegistry, word: CallbackWord, argument: u32) -> Result<i32, CallbackError> {
    let callback: unsafe extern "C" fn(u32) -> i32 = registry.resolve(word)?;
    Ok(unsafe { callback(argument) })
}

pub fn call_pair_status(registry: &CallbackRegistry, word: CallbackWord, first: u32, second: u32) -> Result<i32, CallbackError> {
    let callback: unsafe extern "C" fn(u32, u32) -> i32 = registry.resolve(word)?;
    Ok(unsafe { callback(first, second) })
}

pub fn call_wait_status(registry: &CallbackRegistry, word: CallbackWord, first: u32, second: u32, third: u32, output: *mut u32, timeout: i32) -> Result<i32, CallbackError> {
    let callback: unsafe extern "C" fn(u32, u32, u32, *mut u32, i32) -> i32 = registry.resolve(word)?;
    Ok(unsafe { callback(first, second, third, output, timeout) })
}

pub fn call_signed_argument(registry: &CallbackRegistry, word: CallbackWord, argument: i32) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn(i32) = registry.resolve(word)?;
    unsafe { callback(argument) };
    Ok(())
}

pub fn call_reclaim(registry: &CallbackRegistry, word: CallbackWord, heap: u32, amount: u32, attempt: u32) -> Result<i32, CallbackError> {
    let callback: unsafe extern "C" fn(u32, u32, u32) -> i32 = registry.resolve(word)?;
    Ok(unsafe { callback(heap, amount, attempt) })
}

pub fn interface_call<F: CallbackFn>(registry: &CallbackRegistry, word: CallbackWord) -> Result<F, CallbackError> {
    registry.admit_word::<F>(word)?;
    registry.resolve::<F>(word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI32;

    static CALLS: AtomicU32 = AtomicU32::new(0);
    static SEEN: AtomicI32 = AtomicI32::new(0);

    unsafe extern "C" fn record_no_arguments() { CALLS.fetch_add(1, Ordering::SeqCst); }
    unsafe extern "C" fn record_signed(argument: i32) { CALLS.fetch_add(1, Ordering::SeqCst); SEEN.store(argument, Ordering::SeqCst); }
    unsafe extern "C" fn record_pair(first: u32, second: u32) -> i32 { CALLS.fetch_add(1, Ordering::SeqCst); SEEN.store((first ^ second) as i32, Ordering::SeqCst); 7 }

    fn reset() { CALLS.store(0, Ordering::SeqCst); SEEN.store(0, Ordering::SeqCst); }

    #[test]
    fn registered_callback_is_called_once_with_the_stock_arguments() {
        reset();
        let registry = CallbackRegistry::new();
        registry.register(record_signed as unsafe extern "C" fn(i32)).unwrap();
        assert_eq!(call_signed_argument(&registry, CallbackWord(record_signed as usize as u32), -17), Ok(()));
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(SEEN.load(Ordering::SeqCst), -17);
    }

    #[test]
    fn unregistered_word_is_refused_and_nothing_is_called() {
        reset();
        let registry = CallbackRegistry::new();
        assert_eq!(call_no_arguments(&registry, CallbackWord(0x0060_1000)), Err(CallbackError::Unadmitted));
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(call_no_arguments(&registry, CallbackWord(0)), Err(CallbackError::Invalid));
    }

    #[test]
    fn admitted_region_word_resolves_but_an_outside_word_does_not() {
        let registry = CallbackRegistry::new();
        registry.admit(CodeRegion::new(0x00e4_0000, 0x00eb_2000)).unwrap();
        assert_eq!(registry.admit(CodeRegion::new(0x10, 0x10)), Err(CallbackError::Invalid));
        assert!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0x00e4_1234)).is_ok());
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0x00ec_0000)), Err(CallbackError::Unadmitted));
    }

    #[test]
    fn registration_for_another_shape_is_refused() {
        let registry = CallbackRegistry::new();
        let word = CallbackWord(record_pair as usize as u32);
        registry.register(record_pair as unsafe extern "C" fn(u32, u32) -> i32).unwrap();
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(word), Err(CallbackError::Shape));
        assert_eq!(call_pair_status(&registry, word, 9, 0x8000_0013), Ok(7));
    }

    #[test]
    fn guest_interface_word_is_admitted_for_one_shape_only() {
        let registry = CallbackRegistry::new();
        let word = CallbackWord(0x0072_3cb8);
        assert_eq!(interface_call::<unsafe extern "C" fn() -> u32>(&registry, word).is_ok(), true);
        assert_eq!(interface_call::<unsafe extern "C" fn(u32, u32) -> i32>(&registry, word), Err(CallbackError::Shape));
    }

    #[test]
    fn release_removes_the_entry() {
        let registry = CallbackRegistry::new();
        let token = registry.register(record_no_arguments as unsafe extern "C" fn()).unwrap();
        assert!(registry.release(token));
        assert!(!registry.release(token));
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(record_no_arguments as usize as u32)), Err(CallbackError::Unadmitted));
    }
}
