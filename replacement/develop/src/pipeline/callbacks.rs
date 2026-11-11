//! Typed admission of the code words stock keeps in interface tables and callback slots.
//!
//! Stock enters those words with `blx`. The table, admitted extents, registry and kernel-slot
//! reader are the shared `xm1_callback_core`; this module supplies the develop unit's interrupt
//! primitive and its call helpers.

pub use xm1_callback_core::{CallbackError, CallbackFn, CallbackToken, CallbackWord, CodeRegion, KernelSlot, Shape, kernel_slot};
use xm1_callback_core::CallbackRegistry as CoreRegistry;

/// The develop unit's entries take the kernel interrupt lock stock uses for its own tables
/// (`dispatch_enter_nested_critical`).
pub struct DevelopCritical;
#[cfg(target_arch = "arm")]
impl xm1_callback_core::CriticalSection for DevelopCritical {
    fn enter() -> u32 { unsafe { xm1_develop_enter_critical() } }
    fn leave(token: u32) { unsafe { xm1_develop_leave_critical(token) } }
}
#[cfg(not(target_arch = "arm"))]
impl xm1_callback_core::CriticalSection for DevelopCritical {
    fn enter() -> u32 {
        while HOST_LOCK.compare_exchange_weak(0, 1, core::sync::atomic::Ordering::Acquire, core::sync::atomic::Ordering::Relaxed).is_err() { core::hint::spin_loop(); }
        0
    }
    fn leave(_: u32) { HOST_LOCK.store(0, core::sync::atomic::Ordering::Release); }
}
#[cfg(not(target_arch = "arm"))]
static HOST_LOCK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[cfg(target_arch = "arm")]
unsafe extern "C" {
    fn xm1_develop_enter_critical() -> u32;
    fn xm1_develop_leave_critical(token: u32);
}

pub type CallbackRegistry = CoreRegistry<DevelopCritical>;

static REGISTRY: CallbackRegistry = CallbackRegistry::new();

pub fn registry() -> &'static CallbackRegistry {
    &REGISTRY
}

pub fn call_no_arguments(registry: &CallbackRegistry, word: CallbackWord) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn() = registry.resolve(word)?;
    unsafe { callback() };
    Ok(())
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

/// Resolves a word stock installs from outside this unit. A word no client registered is entered
/// directly, as stock enters it; a word registered for another shape stays refused.
pub fn interface_call<F: CallbackFn>(registry: &CallbackRegistry, word: CallbackWord) -> Result<F, CallbackError> {
    if word.0 == 0 { return Err(CallbackError::Invalid); }
    match registry.resolve::<F>(word) {
        Ok(callback) => Ok(callback),
        Err(CallbackError::Unadmitted) => Ok(unsafe { F::from_address(word.0) }),
        Err(error) => Err(error),
    }
}

pub fn call_interface_signed_argument(registry: &CallbackRegistry, word: CallbackWord, argument: i32) -> Result<(), CallbackError> {
    let callback: unsafe extern "C" fn(i32) = interface_call(registry, word)?;
    unsafe { callback(argument) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;
    use std::sync::atomic::{AtomicI32, AtomicU32};

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
        assert_eq!(call_signed_argument(&registry, CallbackWord(record_signed as usize), -17), Ok(()));
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
    fn admitted_code_word_resolves_but_a_data_run_does_not() {
        let registry = CallbackRegistry::new();
        assert!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0x00e4_3000)).is_ok());
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0x00e4_1234)), Err(CallbackError::Unadmitted));
        assert!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0x0061_c420)).is_ok());
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0x0107_bce0)), Err(CallbackError::Unadmitted));
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0x0040_0000)), Err(CallbackError::Unadmitted));
    }

    #[test]
    fn registration_for_another_shape_is_refused() {
        let registry = CallbackRegistry::new();
        let word = CallbackWord(record_pair as usize);
        registry.register(record_pair as unsafe extern "C" fn(u32, u32) -> i32).unwrap();
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(word), Err(CallbackError::Shape));
        assert_eq!(call_pair_status(&registry, word, 9, 0x8000_0013), Ok(7));
    }

    #[test]
    fn guest_interface_word_is_entered_directly_and_a_registered_word_keeps_its_shape() {
        let registry = CallbackRegistry::new();
        let word = CallbackWord(0x0072_3cb8);
        assert!(interface_call::<unsafe extern "C" fn() -> u32>(&registry, word).is_ok());
        registry.admit_word::<unsafe extern "C" fn() -> u32>(word).unwrap();
        assert_eq!(interface_call::<unsafe extern "C" fn(u32, u32) -> i32>(&registry, word), Err(CallbackError::Shape));
    }

    #[test]
    fn release_removes_the_entry() {
        let registry = CallbackRegistry::new();
        let token = registry.register(record_no_arguments as unsafe extern "C" fn()).unwrap();
        assert!(registry.release(token));
        assert!(!registry.release(token));
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(record_no_arguments as usize)), Err(CallbackError::Unadmitted));
    }
}
