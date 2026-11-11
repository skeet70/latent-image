use super::*;
use xm1_power_services::callback_registry::CallbackFn;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

static CALLS: AtomicU32 = AtomicU32::new(0);
static FIRST: AtomicU32 = AtomicU32::new(0);
static SECOND: AtomicU32 = AtomicU32::new(0);
static SIGNED: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" fn record_argument(argument: u32) {
    CALLS.fetch_add(1, Ordering::SeqCst);
    FIRST.store(argument, Ordering::SeqCst);
}
unsafe extern "C" fn record_pair(first: u32, second: u32) {
    CALLS.fetch_add(1, Ordering::SeqCst);
    FIRST.store(first, Ordering::SeqCst);
    SECOND.store(second, Ordering::SeqCst);
}
unsafe extern "C" fn record_signed(argument: i32) {
    CALLS.fetch_add(1, Ordering::SeqCst);
    SIGNED.store(argument, Ordering::SeqCst);
}
unsafe extern "C" fn record_block(status: i32, value: u32) {
    CALLS.fetch_add(1, Ordering::SeqCst);
    SIGNED.store(status, Ordering::SeqCst);
    FIRST.store(value, Ordering::SeqCst);
}
unsafe extern "C" fn record_no_arguments() { CALLS.fetch_add(1, Ordering::SeqCst); }

fn argument_callback() -> unsafe extern "C" fn(u32) { record_argument }
fn pair_callback() -> unsafe extern "C" fn(u32, u32) { record_pair }
fn signed_callback() -> unsafe extern "C" fn(i32) { record_signed }
fn block_callback() -> unsafe extern "C" fn(i32, u32) { record_block }
fn no_arguments_callback() -> unsafe extern "C" fn() { record_no_arguments }

fn reset() {
    CALLS.store(0, Ordering::SeqCst);
    FIRST.store(0, Ordering::SeqCst);
    SECOND.store(0, Ordering::SeqCst);
    SIGNED.store(0, Ordering::SeqCst);
}

#[test]
fn argument_callback_is_called_once_with_the_stock_argument() {
    reset();
    let registry = CallbackRegistry::new();
    registry.register(argument_callback()).unwrap();
    assert_eq!(call_argument(&registry, argument_callback().word().0, 0x1234), Ok(()));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(FIRST.load(Ordering::SeqCst), 0x1234);
}

#[test]
fn unregistered_argument_word_is_refused_and_nothing_is_called() {
    reset();
    let registry = CallbackRegistry::new();
    assert_eq!(call_argument(&registry, 0x0060_1000, 0x1234), Err(CallbackError::Unadmitted));
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn pair_callback_is_called_once_with_the_stock_arguments() {
    reset();
    let registry = CallbackRegistry::new();
    registry.register(pair_callback()).unwrap();
    assert_eq!(call_pair(&registry, pair_callback().word().0, 9, 0x8000_0013), Ok(()));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(FIRST.load(Ordering::SeqCst), 9);
    assert_eq!(SECOND.load(Ordering::SeqCst), 0x8000_0013);
}

#[test]
fn unregistered_pair_word_is_refused_and_nothing_is_called() {
    reset();
    let registry = CallbackRegistry::new();
    assert_eq!(call_pair(&registry, 0x0060_1000, 9, 0x8000_0013), Err(CallbackError::Unadmitted));
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn signed_argument_callback_is_called_once_with_the_stock_argument() {
    reset();
    let registry = CallbackRegistry::new();
    registry.register(signed_callback()).unwrap();
    assert_eq!(call_signed_argument(&registry, signed_callback().word().0, -17), Ok(()));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(SIGNED.load(Ordering::SeqCst), -17);
}

#[test]
fn unregistered_signed_argument_word_is_refused_and_nothing_is_called() {
    reset();
    let registry = CallbackRegistry::new();
    assert_eq!(call_signed_argument(&registry, 0x0060_1000, -17), Err(CallbackError::Unadmitted));
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn block_callback_is_called_once_with_the_stock_arguments() {
    reset();
    let registry = CallbackRegistry::new();
    registry.register(block_callback()).unwrap();
    assert_eq!(call_block(&registry, block_callback().word().0, -5, 0x2a), Ok(()));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(SIGNED.load(Ordering::SeqCst), -5);
    assert_eq!(FIRST.load(Ordering::SeqCst), 0x2a);
}

#[test]
fn unregistered_block_word_is_refused_and_nothing_is_called() {
    reset();
    let registry = CallbackRegistry::new();
    assert_eq!(call_block(&registry, 0x0060_1000, -5, 0x2a), Err(CallbackError::Unadmitted));
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn no_argument_callback_is_called_once() {
    reset();
    let registry = CallbackRegistry::new();
    registry.register(no_arguments_callback()).unwrap();
    assert_eq!(call_no_arguments(&registry, no_arguments_callback().word().0), Ok(()));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn unregistered_word_without_arguments_is_refused_and_nothing_is_called() {
    reset();
    let registry = CallbackRegistry::new();
    assert_eq!(call_no_arguments(&registry, 0x0060_1000), Err(CallbackError::Unadmitted));
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

unsafe extern "C" fn check_record(record: *const u8) -> i32 {
    if unsafe { *record } == 0x42 { 7 } else { 0 }
}

#[test]
fn registered_application_callback_is_called_once_with_the_record() {
    let registry = CallbackRegistry::new();
    register_application(&registry, check_record).unwrap();
    let word = (check_record as Application).word().0;
    assert_eq!(call_application(&registry, word, [0x42; 76].as_ptr()), Ok(7));
    assert_eq!(call_application(&registry, word, [0x41; 76].as_ptr()), Ok(0));
}

#[test]
fn unregistered_application_word_is_refused_and_nothing_is_called() {
    let registry = CallbackRegistry::new();
    assert_eq!(call_application(&registry, 0x0060_1000, [0x42; 76].as_ptr()), Err(CallbackError::Unadmitted));
}
