use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static CALLS: AtomicU32 = AtomicU32::new(0);
static ARGUMENT: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn record_argument(value: u32) {
    CALLS.fetch_add(1, Ordering::SeqCst);
    ARGUMENT.store(value, Ordering::SeqCst);
}
unsafe extern "C" fn record_call() { CALLS.fetch_add(1, Ordering::SeqCst); }

fn argument_callback() -> unsafe extern "C" fn(u32) { record_argument }
fn call_callback() -> unsafe extern "C" fn() { record_call }

fn reset() {
    CALLS.store(0, Ordering::SeqCst);
    ARGUMENT.store(0, Ordering::SeqCst);
}

#[test]
fn registered_callback_is_called_once_with_its_argument() {
    reset();
    let registry = CallbackRegistry::new();
    let token = registry.register(argument_callback()).unwrap();
    assert!(token.0 >= FIRST_TOKEN);
    let callback: unsafe extern "C" fn(u32) = registry.resolve(CallbackWord(argument_callback().word().0)).unwrap();
    unsafe { callback(0x1234) };
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(ARGUMENT.load(Ordering::SeqCst), 0x1234);
}

#[test]
fn unregistered_word_is_refused_and_nothing_is_called() {
    reset();
    let registry = CallbackRegistry::new();
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0060_1000)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0)), Err(CallbackError::Invalid));
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn admitted_region_is_resolved_without_a_registration() {
    let registry = CallbackRegistry::new();
    assert_eq!(registry.admit(CodeRegion::new(0x0060_0000, 0x0080_0000)), Ok(()));
    assert_eq!(registry.admit(CodeRegion::new(0x0080_0000, 0x0080_0000)), Err(CallbackError::Invalid));
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0060_1000)).is_ok());
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x005f_ffff)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0080_0000)), Err(CallbackError::Unadmitted));
}

#[test]
fn registration_for_another_shape_is_refused() {
    let registry = CallbackRegistry::new();
    let word = call_callback().word();
    registry.register(call_callback()).unwrap();
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(word), Err(CallbackError::Shape));
    assert!(registry.resolve::<unsafe extern "C" fn()>(word).is_ok());
}

#[test]
fn release_then_reuse_of_a_slot() {
    let registry = CallbackRegistry::new();
    let first = registry.register(argument_callback()).unwrap();
    assert!(registry.release(first));
    assert!(!registry.release(first));
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(argument_callback().word()), Err(CallbackError::Unadmitted));
    let second = registry.register(call_callback()).unwrap();
    assert_ne!(first, second);
    assert!(registry.resolve::<unsafe extern "C" fn()>(call_callback().word()).is_ok());
}

#[test]
fn released_slot_is_reusable_at_capacity() {
    let registry = CallbackRegistry::new();
    let mut tokens = std::vec::Vec::new();
    for _ in 0..ENTRIES {
        tokens.push(registry.register(call_callback()).unwrap());
    }
    assert_eq!(registry.register(call_callback()), Err(CallbackError::Full));
    assert!(registry.release(tokens[7]));
    let replacement = registry.register(call_callback()).unwrap();
    assert!(replacement.0 > tokens[ENTRIES - 1].0);
    assert_eq!(registry.register(call_callback()), Err(CallbackError::Full));
}
