//! Admission of code words that stock ABI entries may enter.
//!
//! Stock keeps function words in interface tables and callback slots and enters them with `blx`.
//! The registry replaces that entry: an owned client registers its typed pointer, the composition
//! admits the stock code regions it supplies, and `resolve` returns a pointer only for one of those.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackWord(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackToken(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackShape {
    None,
    Task,
    Status,
    Argument,
    ArgumentStatus,
    Pair,
    PairStatus,
    ConfigureStatus,
    ReadStatus,
    WaitStatus,
    SignalStatus,
    ReceiveStatus,
    BlockStatus,
    Block,
    EventStatus,
    Producer,
    SignedArgument,
    BufferStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackError {
    Invalid,
    Unadmitted,
    Shape,
    Full,
}
impl CallbackError {
    /// The status the leaf's ABI entries report for a refused entry.
    pub const fn status(self) -> i32 { -255 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeRegion {
    start: u32,
    end: u32,
}
impl CodeRegion {
    pub const fn new(start: u32, end: u32) -> Self { Self { start, end } }
    pub const fn contains(self, address: u32) -> bool {
        self.start < self.end && address >= self.start && address < self.end
    }
}

/// # Safety
/// `SHAPE` must name the argument and result layout of the implementing type exactly.
pub unsafe trait CallbackFn: Copy {
    const SHAPE: CallbackShape;
    fn word(self) -> CallbackWord;
    fn address(self) -> usize;
    /// # Safety
    /// `address` was returned by `resolve`, which admits it for this shape.
    unsafe fn from_address(address: usize) -> Self;
}

macro_rules! callback_functions {
    ($($shape:ident => $signature:ty),* $(,)?) => {
        $(
            unsafe impl CallbackFn for $signature {
                const SHAPE: CallbackShape = CallbackShape::$shape;
                fn word(self) -> CallbackWord { CallbackWord(self as usize as u32) }
                fn address(self) -> usize { self as usize }
                unsafe fn from_address(address: usize) -> Self {
                    unsafe { core::mem::transmute(address) }
                }
            }
        )*
    };
}

callback_functions! {
    None => unsafe extern "C" fn(),
    Task => unsafe extern "C" fn() -> u32,
    Status => unsafe extern "C" fn() -> i32,
    Argument => unsafe extern "C" fn(u32),
    ArgumentStatus => unsafe extern "C" fn(u32) -> i32,
    Pair => unsafe extern "C" fn(u32, u32),
    PairStatus => unsafe extern "C" fn(u32, u32) -> i32,
    ConfigureStatus => unsafe extern "C" fn(u32, *const u32) -> i32,
    ReadStatus => unsafe extern "C" fn(u32, *mut u32) -> i32,
    WaitStatus => unsafe extern "C" fn(i32, i32) -> i32,
    SignalStatus => unsafe extern "C" fn(i32) -> i32,
    ReceiveStatus => unsafe extern "C" fn(i32, *mut u32, i32) -> i32,
    BlockStatus => unsafe extern "C" fn(i32, u32) -> i32,
    Block => unsafe extern "C" fn(i32, u32),
    EventStatus => unsafe extern "C" fn(i32, u32, u32, *mut u32, i32) -> i32,
    Producer => unsafe extern "C" fn(*mut u8, u32),
    SignedArgument => unsafe extern "C" fn(i32),
    BufferStatus => unsafe extern "C" fn(*const u8) -> i32,
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

const ENTRIES: usize = 32;
const REGIONS: usize = 8;
const FIRST_TOKEN: u32 = 0x4000_0001;

#[derive(Clone, Copy)]
struct Entry { token: CallbackToken, word: CallbackWord, address: usize, shape: CallbackShape }
struct State { entries: [Option<Entry>; ENTRIES], regions: [Option<CodeRegion>; REGIONS], next: u32 }

pub struct CallbackRegistry { state: Shared<State> }
impl CallbackRegistry {
    pub const fn new() -> Self {
        Self { state: Shared::new(State { entries: [None; ENTRIES], regions: [None; REGIONS], next: FIRST_TOKEN }) }
    }
    /// Admits a stock code region supplied by the composition.
    pub fn admit(&self, region: CodeRegion) -> Result<(), CallbackError> {
        if region.start >= region.end { return Err(CallbackError::Invalid); }
        self.state.with(|state| {
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
            state.entries[slot] = Some(Entry { token, word, address: callback.address(), shape: F::SHAPE });
            Ok(token)
        })
    }
    /// Registers `callback` unless its word already resolves for this shape.
    pub fn ensure<F: CallbackFn>(&self, callback: F) -> Result<(), CallbackError> {
        if self.resolve::<F>(callback.word()).is_ok() { return Ok(()); }
        self.register(callback).map(|_| ())
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

pub fn registry() -> &'static CallbackRegistry { &REGISTRY }

#[cfg(test)]
#[path = "callback_registry_tests.rs"]
mod tests;
