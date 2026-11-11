use super::*;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

#[test]
fn registered_callback_is_called_once_with_its_argument() {
    static CALLS: AtomicU32 = AtomicU32::new(0);
    static ARGUMENT: AtomicU32 = AtomicU32::new(0);
    unsafe extern "C" fn record(value: u32) {
        CALLS.fetch_add(1, Ordering::SeqCst);
        ARGUMENT.store(value, Ordering::SeqCst);
    }
    let registry = CallbackRegistry::new();
    let callback: unsafe extern "C" fn(u32) = record;
    registry.ensure(callback).unwrap();
    let resolved: unsafe extern "C" fn(u32) = registry.resolve(callback.word()).unwrap();
    unsafe { resolved(0x1234) };
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(ARGUMENT.load(Ordering::SeqCst), 0x1234);
}

#[test]
fn unregistered_word_is_refused() {
    let registry = CallbackRegistry::new();
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0060_1000 as usize)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(0 as usize)), Err(CallbackError::Invalid));
}

#[test]
fn admitted_text_is_resolved_without_a_registration() {
    let registry = CallbackRegistry::new();
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0061_c420 as usize)).is_ok());
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0077_48d8 as usize)).is_ok());
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0082_9d14 as usize)).is_ok());
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0089_e6e8 as usize)).is_ok());
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0089_e7ac as usize)).is_ok());
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0061_c41c as usize)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0082_9a0c as usize)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0082_9d10 as usize)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0077_48dc as usize)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0089_e7b0 as usize)), Err(CallbackError::Unadmitted));
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0060_0000 as usize)), Err(CallbackError::Unadmitted));
}

#[test]
fn image_five_and_image_six_stock_words_are_admitted() {
    let registry = CallbackRegistry::new();
    // Image 5: the handler at 0xda9bcc/0xda9e44, the timer at 0xd81c98, the detection output at
    // 0xdc8214, and the eight producers 0xec4618 posts, 0xec554c through 0xec5bc8.
    for word in [
        0x00da_9da0, 0x00d8_13ac, 0x00dc_8400, 0x00ec_4618,
        0x00ec_554c, 0x00ec_59e4, 0x00ec_97f0, 0x00ec_5a78,
        0x00ec_5abc, 0x00ec_5b40, 0x00ec_5b84, 0x00ec_5bc8,
    ] {
        assert!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(word as usize)).is_ok(), "{word:#x}");
    }
    // Image 6: the timer passed at 0x12528a8 and the text producer passed at 0x13ed5e4.
    for word in [0x0121_ab88, 0x013e_b548] {
        assert!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(word as usize)).is_ok(), "{word:#x}");
    }
}

#[test]
fn image_text_bounds_refuse_data_that_the_image_extent_would_admit() {
    let registry = CallbackRegistry::new();
    // Image 5's record header, the data run at 0x107bce0, the run at 0x108df34, the run at
    // 0x10af058 and the trailing table; image 6's header word, zero fill, table, pointer table and
    // string table.
    for word in [
        0x00bf_1a30, 0x0108_0000, 0x010a_0000, 0x010b_0000, 0x010e_366c,
        0x0110_0004, 0x011b_0000, 0x011e_0000, 0x0144_4918, 0x0144_a5b0,
    ] {
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(word as usize)), Err(CallbackError::Unadmitted), "{word:#x}");
    }
}

#[test]
fn a_host_address_whose_low_half_is_admitted_is_refused() {
    let registry = CallbackRegistry::new();
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x7f00_0000_0061_c420)), Err(CallbackError::Unadmitted));
}

#[test]
fn a_word_with_low_bits_set_is_refused() {
    let registry = CallbackRegistry::new();
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0061_c421 as usize)), Err(CallbackError::Invalid));
}

#[test]
fn registration_for_another_shape_is_refused() {
    unsafe extern "C" fn record() {}
    let registry = CallbackRegistry::new();
    let callback: unsafe extern "C" fn() = record;
    registry.ensure(callback).unwrap();
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(callback.word()), Err(CallbackError::Shape));
    assert!(registry.resolve::<unsafe extern "C" fn()>(callback.word()).is_ok());
}

#[test]
fn ensure_is_idempotent_for_the_same_signature() {
    unsafe extern "C" fn record(_: u32) {}
    let registry = CallbackRegistry::new();
    let callback: unsafe extern "C" fn(u32) = record;
    assert_eq!(registry.ensure(callback), Ok(()));
    assert_eq!(registry.ensure(callback), Ok(()));
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(callback.word()).is_ok());
}

#[test]
fn a_kernel_slot_is_entered_without_registration() {
    static CALLS: AtomicU32 = AtomicU32::new(0);
    static ARGUMENT: AtomicU32 = AtomicU32::new(0);
    unsafe extern "C" fn record_argument(value: u32) {
        CALLS.fetch_add(1, Ordering::SeqCst);
        ARGUMENT.store(value, Ordering::SeqCst);
    }
    let slot = KernelSlot(CallbackWord(record_argument as *const () as usize));
    assert_eq!(CallbackRegistry::new().resolve::<unsafe extern "C" fn(u32)>(slot.word()), Err(CallbackError::Unadmitted));
    let entry: unsafe extern "C" fn(u32) = unsafe { slot.entry() };
    unsafe { entry(0x77) };
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(ARGUMENT.load(Ordering::SeqCst), 0x77);
}

#[test]
fn one_shape_covers_every_five_argument_site() {
    static SUM: AtomicI64 = AtomicI64::new(0);
    unsafe extern "C" fn slot(a: u32, b: u32, c: i32, d: i32, e: u32) {
        SUM.store(a as i64 + b as i64 + c as i64 + d as i64 + e as i64, Ordering::SeqCst);
    }
    let registry = CallbackRegistry::new();
    let word = CallbackWord(slot as *const () as usize);
    registry.ensure(slot as unsafe extern "C" fn(u32, u32, i32, i32, u32)).unwrap();
    let signed: unsafe extern "C" fn(u32, u32, i32, i32, u32) = registry.resolve(word).unwrap();
    let unsigned: unsafe extern "C" fn(u32, u32, u32, u32, u32) = registry.resolve(word).unwrap();
    unsafe { signed(1, 2, 3, 4, 5) };
    assert_eq!(SUM.load(Ordering::SeqCst), 15);
    unsafe { unsigned(1, 2, 3, 4, 5) };
    assert_eq!(SUM.load(Ordering::SeqCst), 15);
}

#[test]
fn one_shape_covers_every_thirteen_argument_site() {
    static SUM: AtomicI64 = AtomicI64::new(0);
    unsafe extern "C" fn slot(a: u32, b: i32, c: u32, d: i32, e: u32, f: u32, g: u32, h: u32, i: i32, j: i32, k: i32, l: i32, m: u32) {
        SUM.store(a as i64 + b as i64 + c as i64 + d as i64 + e as i64 + f as i64 + g as i64 + h as i64
            + i as i64 + j as i64 + k as i64 + l as i64 + m as i64, Ordering::SeqCst);
    }
    let registry = CallbackRegistry::new();
    let word = CallbackWord(slot as *const () as usize);
    registry.ensure(slot as unsafe extern "C" fn(u32, i32, u32, i32, u32, u32, u32, u32, i32, i32, i32, i32, u32)).unwrap();
    let mixed: unsafe extern "C" fn(u32, i32, u32, i32, u32, u32, u32, u32, i32, i32, i32, i32, u32) = registry.resolve(word).unwrap();
    let signed: unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32) = registry.resolve(word).unwrap();
    unsafe { mixed(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13) };
    assert_eq!(SUM.load(Ordering::SeqCst), 91);
    unsafe { signed(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13) };
    assert_eq!(SUM.load(Ordering::SeqCst), 91);
}

#[test]
fn an_upstream_handler_registered_through_the_abi_entry_resolves() {
    // The entry takes a target word; the test resolves a word it never enters. A fixed word
    // outside the admitted stock extents keeps the Shape and Invalid answers independent of
    // where the host maps this test's functions.
    let word = 0x1000u32;
    assert_eq!(unsafe { xm1_power_owned_callback_register(word, OwnedShape::Completion as u32) }, 0);
    let entry: unsafe extern "C" fn() = registry().resolve(CallbackWord(word as usize)).unwrap();
    assert_eq!(entry as usize, word as usize);
    assert_eq!(unsafe { xm1_power_owned_callback_register(word, OwnedShape::TextProducer as u32) }, CallbackError::Shape.status());
    assert_eq!(unsafe { xm1_power_owned_callback_register(word, 7) }, -30);
    assert_eq!(unsafe { xm1_power_owned_callback_register(word + 1, OwnedShape::Completion as u32) }, CallbackError::Invalid.status());
}

#[test]
fn an_owned_handler_registered_for_its_shape_is_called() {
    static CALLS: AtomicU32 = AtomicU32::new(0);
    unsafe extern "C" fn record_call() { CALLS.fetch_add(1, Ordering::SeqCst); }
    let word = CallbackWord(record_call as *const () as usize);
    assert_eq!(unsafe { register_owned(word, OwnedShape::Completion) }, Ok(()));
    let entry: unsafe extern "C" fn() = registry().resolve(word).unwrap();
    unsafe { entry() };
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

macro_rules! shape_case {
    ($name:ident, $signature:ty, |$($arg:ident : $ty:ty),*| ($($value:expr),*), $expected:expr) => {
        #[test]
        fn $name() {
            static CALLS: AtomicU32 = AtomicU32::new(0);
            static SUM: AtomicI64 = AtomicI64::new(0);
            unsafe extern "C" fn record($($arg: $ty),*) {
                CALLS.fetch_add(1, Ordering::SeqCst);
                SUM.fetch_add(0i64 $(+ $arg as i64)*, Ordering::SeqCst);
            }
            let registry = CallbackRegistry::new();
            let callback: $signature = record;
            registry.ensure(callback).unwrap();
            let resolved: $signature = registry.resolve(callback.word()).unwrap();
            unsafe { resolved($($value),*) };
            assert_eq!(CALLS.load(Ordering::SeqCst), 1);
            assert_eq!(SUM.load(Ordering::SeqCst), $expected);
            assert_eq!(registry.resolve::<unsafe extern "C" fn()>(callback.word()), Err(CallbackError::Shape));
        }
    };
}

shape_case!(argument_buffer, unsafe extern "C" fn(u32, *const u8), |a: u32, b: *const u8| (5u32, 0x1000usize as *const u8), 0x1005);
shape_case!(completion_five, unsafe extern "C" fn(u32, u32, u32, u32, u32), |a: u32, b: u32, c: u32, d: u32, e: u32| (1u32, 2u32, 3u32, 4u32, 5u32), 15);
shape_case!(completion_four, unsafe extern "C" fn(u32, u32, u32, u32), |a: u32, b: u32, c: u32, d: u32| (1u32, 2u32, 3u32, 4u32), 10);
shape_case!(completion_three, unsafe extern "C" fn(u32, u32, u32), |a: u32, b: u32, c: u32| (1u32, 2u32, 3u32), 6);
shape_case!(argument_pointer, unsafe extern "C" fn(u32, *const u32), |a: u32, b: *const u32| (2u32, 0x2000usize as *const u32), 0x2002);
shape_case!(pointer, unsafe extern "C" fn(*const u32), |a: *const u32| (0x3000usize as *const u32), 0x3000);
shape_case!(block_triple, unsafe extern "C" fn(i32, u32, u32, u32), |a: i32, b: u32, c: u32, d: u32| (1i32, 2u32, 3u32, 4u32), 10);
shape_case!(completion_six, unsafe extern "C" fn(u32, u32, u32, u32, u32, u32), |a: u32, b: u32, c: u32, d: u32, e: u32, f: u32| (1u32, 2u32, 3u32, 4u32, 5u32, 6u32), 21);
shape_case!(signed_pair, unsafe extern "C" fn(i32, i32), |a: i32, b: i32| (2i32, 3i32), 5);
shape_case!(completion_nine, unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32), |a: u32, b: u32, c: u32, d: u32, e: u32, f: u32, g: u32, h: u32, i: u32| (1u32, 2u32, 3u32, 4u32, 5u32, 6u32, 7u32, 8u32, 9u32), 45);
shape_case!(completion_thirteen, unsafe extern "C" fn(u32, i32, u32, i32, u32, u32, u32, u32, i32, i32, i32, i32, u32), |a: u32, b: i32, c: u32, d: i32, e: u32, f: u32, g: u32, h: u32, i: i32, j: i32, k: i32, l: i32, m: u32| (1u32, 2i32, 3u32, 4i32, 5u32, 6u32, 7u32, 8u32, 9i32, 10i32, 11i32, 12i32, 13u32), 91);
shape_case!(publish, unsafe extern "C" fn(u32, u32, i32, i32, u32), |a: u32, b: u32, c: i32, d: i32, e: u32| (1u32, 2u32, 3i32, 4i32, 5u32), 15);
shape_case!(signed_completion_thirteen, unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32), |a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32, i: i32, j: i32, k: i32, l: i32, m: i32| (1i32, 2i32, 3i32, 4i32, 5i32, 6i32, 7i32, 8i32, 9i32, 10i32, 11i32, 12i32, 13i32), 91);
