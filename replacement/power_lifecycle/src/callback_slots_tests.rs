use super::*;
use xm1_power_services::callback_registry::CallbackFn;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

// Tests in this binary run in parallel threads; a shared counter would race.

macro_rules! argument_recorder {
    ($callback:ident, $accessor:ident, $calls:ident, $value:ident) => {
        static $calls: AtomicU32 = AtomicU32::new(0);
        static $value: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn $callback(argument: u32) {
            $calls.fetch_add(1, Ordering::SeqCst);
            $value.store(argument, Ordering::SeqCst);
        }
        fn $accessor() -> unsafe extern "C" fn(u32) { $callback }
    };
}

macro_rules! pair_recorder {
    ($callback:ident, $accessor:ident, $calls:ident, $first:ident, $second:ident) => {
        static $calls: AtomicU32 = AtomicU32::new(0);
        static $first: AtomicU32 = AtomicU32::new(0);
        static $second: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn $callback(first: u32, second: u32) {
            $calls.fetch_add(1, Ordering::SeqCst);
            $first.store(first, Ordering::SeqCst);
            $second.store(second, Ordering::SeqCst);
        }
        fn $accessor() -> unsafe extern "C" fn(u32, u32) { $callback }
    };
}

macro_rules! signed_recorder {
    ($callback:ident, $accessor:ident, $calls:ident, $value:ident) => {
        static $calls: AtomicU32 = AtomicU32::new(0);
        static $value: AtomicI32 = AtomicI32::new(0);
        unsafe extern "C" fn $callback(argument: i32) {
            $calls.fetch_add(1, Ordering::SeqCst);
            $value.store(argument, Ordering::SeqCst);
        }
        fn $accessor() -> unsafe extern "C" fn(i32) { $callback }
    };
}

macro_rules! block_recorder {
    ($callback:ident, $accessor:ident, $calls:ident, $status:ident, $value:ident) => {
        static $calls: AtomicU32 = AtomicU32::new(0);
        static $status: AtomicI32 = AtomicI32::new(0);
        static $value: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn $callback(status: i32, value: u32) {
            $calls.fetch_add(1, Ordering::SeqCst);
            $status.store(status, Ordering::SeqCst);
            $value.store(value, Ordering::SeqCst);
        }
        fn $accessor() -> unsafe extern "C" fn(i32, u32) { $callback }
    };
}

macro_rules! completion_recorder {
    ($callback:ident, $accessor:ident, $calls:ident) => {
        static $calls: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn $callback() { $calls.fetch_add(1, Ordering::SeqCst); }
        fn $accessor() -> unsafe extern "C" fn() { $callback }
    };
}

argument_recorder!(record_argument, argument_callback, ARGUMENT_CALLS, ARGUMENT_VALUE);
argument_recorder!(refused_argument, refused_argument_callback, REFUSED_ARGUMENT_CALLS, REFUSED_ARGUMENT_VALUE);
pair_recorder!(record_pair, pair_callback, PAIR_CALLS, PAIR_FIRST, PAIR_SECOND);
pair_recorder!(refused_pair, refused_pair_callback, REFUSED_PAIR_CALLS, REFUSED_PAIR_FIRST, REFUSED_PAIR_SECOND);
signed_recorder!(record_signed, signed_callback, SIGNED_CALLS, SIGNED_VALUE);
signed_recorder!(refused_signed, refused_signed_callback, REFUSED_SIGNED_CALLS, REFUSED_SIGNED_VALUE);
block_recorder!(record_block, block_callback, BLOCK_CALLS, BLOCK_STATUS, BLOCK_VALUE);
block_recorder!(refused_block, refused_block_callback, REFUSED_BLOCK_CALLS, REFUSED_BLOCK_STATUS, REFUSED_BLOCK_VALUE);
completion_recorder!(record_no_arguments, no_arguments_callback, NO_ARGUMENT_CALLS);
completion_recorder!(refused_no_arguments, refused_no_arguments_callback, REFUSED_NO_ARGUMENT_CALLS);

#[test]
fn argument_callback_is_called_once_with_the_stock_argument() {
    let registry = CallbackRegistry::new();
    registry.ensure(argument_callback()).unwrap();
    assert_eq!(call_argument(&registry, argument_callback().word(), 0x1234), Ok(()));
    assert_eq!(ARGUMENT_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(ARGUMENT_VALUE.load(Ordering::SeqCst), 0x1234);
}

#[test]
fn unregistered_argument_word_is_refused_and_nothing_is_called() {
    let registry = CallbackRegistry::new();
    registry.ensure(refused_argument_callback()).unwrap();
    assert_eq!(call_argument(&registry, CallbackWord(0x0060_1000), 0x1234), Err(CallbackError::Unadmitted));
    assert_eq!(REFUSED_ARGUMENT_CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn pair_callback_is_called_once_with_the_stock_arguments() {
    let registry = CallbackRegistry::new();
    registry.ensure(pair_callback()).unwrap();
    assert_eq!(call_pair(&registry, pair_callback().word(), 9, 0x8000_0013), Ok(()));
    assert_eq!(PAIR_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(PAIR_FIRST.load(Ordering::SeqCst), 9);
    assert_eq!(PAIR_SECOND.load(Ordering::SeqCst), 0x8000_0013);
}

#[test]
fn unregistered_pair_word_is_refused_and_nothing_is_called() {
    let registry = CallbackRegistry::new();
    registry.ensure(refused_pair_callback()).unwrap();
    assert_eq!(call_pair(&registry, CallbackWord(0x0060_1000), 9, 0x8000_0013), Err(CallbackError::Unadmitted));
    assert_eq!(REFUSED_PAIR_CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn signed_argument_callback_is_called_once_with_the_stock_argument() {
    let registry = CallbackRegistry::new();
    registry.ensure(signed_callback()).unwrap();
    assert_eq!(call_signed_argument(&registry, signed_callback().word(), -17), Ok(()));
    assert_eq!(SIGNED_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(SIGNED_VALUE.load(Ordering::SeqCst), -17);
}

#[test]
fn unregistered_signed_argument_word_is_refused_and_nothing_is_called() {
    let registry = CallbackRegistry::new();
    registry.ensure(refused_signed_callback()).unwrap();
    assert_eq!(call_signed_argument(&registry, CallbackWord(0x0060_1000), -17), Err(CallbackError::Unadmitted));
    assert_eq!(REFUSED_SIGNED_CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn block_callback_is_called_once_with_the_stock_arguments() {
    let registry = CallbackRegistry::new();
    registry.ensure(block_callback()).unwrap();
    assert_eq!(call_block(&registry, block_callback().word(), -5, 0x2a), Ok(()));
    assert_eq!(BLOCK_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(BLOCK_STATUS.load(Ordering::SeqCst), -5);
    assert_eq!(BLOCK_VALUE.load(Ordering::SeqCst), 0x2a);
}

#[test]
fn unregistered_block_word_is_refused_and_nothing_is_called() {
    let registry = CallbackRegistry::new();
    registry.ensure(refused_block_callback()).unwrap();
    assert_eq!(call_block(&registry, CallbackWord(0x0060_1000), -5, 0x2a), Err(CallbackError::Unadmitted));
    assert_eq!(REFUSED_BLOCK_CALLS.load(Ordering::SeqCst), 0);
}

#[test]
fn no_argument_callback_is_called_once() {
    let registry = CallbackRegistry::new();
    registry.ensure(no_arguments_callback()).unwrap();
    assert_eq!(call_no_arguments(&registry, no_arguments_callback().word()), Ok(()));
    assert_eq!(NO_ARGUMENT_CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn unregistered_word_without_arguments_is_refused_and_nothing_is_called() {
    let registry = CallbackRegistry::new();
    registry.ensure(refused_no_arguments_callback()).unwrap();
    assert_eq!(call_no_arguments(&registry, CallbackWord(0x0060_1000)), Err(CallbackError::Unadmitted));
    assert_eq!(REFUSED_NO_ARGUMENT_CALLS.load(Ordering::SeqCst), 0);
}

unsafe extern "C" fn check_record(record: *const u8) -> i32 {
    if unsafe { *record } == 0x42 { 7 } else { 0 }
}

#[test]
fn registered_application_callback_is_called_once_with_the_record() {
    let registry = CallbackRegistry::new();
    register_application(&registry, check_record).unwrap();
    let word = (check_record as Application).word();
    assert_eq!(call_application(&registry, word, [0x42; 76].as_ptr()), Ok(7));
    assert_eq!(call_application(&registry, word, [0x41; 76].as_ptr()), Ok(0));
}

#[test]
fn unregistered_application_word_is_refused_and_nothing_is_called() {
    let registry = CallbackRegistry::new();
    assert_eq!(call_application(&registry, CallbackWord(0x0060_1000), [0x42; 76].as_ptr()), Err(CallbackError::Unadmitted));
}

#[test]
fn admitted_stock_application_word_resolves_at_the_consumer() {
    let registry = CallbackRegistry::new();
    let resolved = resolve_application(&registry, CallbackWord(0x0072_b210)).unwrap();
    assert_eq!(resolved as usize as u32, 0x0072_b210);
}
