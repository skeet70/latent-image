use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

struct TestCritical;
impl CriticalSection for TestCritical {
    fn enter() -> u32 { 0 }
    fn leave(_: u32) {}
}
type Registry = CallbackRegistry<TestCritical>;

macro_rules! distinct_callbacks {
    ($($name:ident),* $(,)?) => {
        $(unsafe extern "C" fn $name() {})*
        const DISTINCT: [unsafe extern "C" fn(); 64] = [$($name),*];
    };
}
distinct_callbacks!(
    c00, c01, c02, c03, c04, c05, c06, c07, c08, c09, c10, c11, c12, c13, c14, c15,
    c16, c17, c18, c19, c20, c21, c22, c23, c24, c25, c26, c27, c28, c29, c30, c31,
    c32, c33, c34, c35, c36, c37, c38, c39, c40, c41, c42, c43, c44, c45, c46, c47,
    c48, c49, c50, c51, c52, c53, c54, c55, c56, c57, c58, c59, c60, c61, c62, c63,
);
unsafe extern "C" fn spare() {}

#[test]
fn a_registered_callback_is_resolved_for_its_shape() {
    static CALLS: AtomicU32 = AtomicU32::new(0);
    unsafe extern "C" fn record_call() { CALLS.fetch_add(1, Ordering::SeqCst); }
    let registry = Registry::new();
    registry.ensure(record_call as unsafe extern "C" fn()).unwrap();
    let callback: unsafe extern "C" fn() = registry.resolve(CallbackWord(record_call as usize)).unwrap();
    unsafe { callback() };
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(record_call as usize)), Err(CallbackError::Shape));
}

#[test]
fn admitted_text_is_resolved_without_a_registration() {
    let registry = Registry::new();
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0061_c420 as usize)).is_ok());
    assert!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0077_48d8 as usize)).is_ok());
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x0077_48dc as usize)), Err(CallbackError::Unadmitted));
}

#[test]
fn a_full_table_refuses_a_new_entry() {
    let registry = Registry::new();
    for callback in DISTINCT { registry.ensure(callback).unwrap(); }
    assert_eq!(registry.ensure(spare as unsafe extern "C" fn()), Err(CallbackError::Full));
}

#[test]
fn every_enumerated_owned_word_fits_and_resolves() {
    // The leaf, application, display, NAND and vendor crates register 44 owned words; the table
    // holds those plus twenty spare entries.
    let registry = Registry::new();
    for callback in &DISTINCT[..44] {
        registry.ensure(*callback).unwrap();
        assert!(registry.resolve::<unsafe extern "C" fn()>(callback.word()).is_ok());
    }
    for callback in &DISTINCT[44..] { registry.ensure(*callback).unwrap(); }
    assert_eq!(registry.ensure(spare as unsafe extern "C" fn()), Err(CallbackError::Full));
}

/// The maximal runs of 4 KiB blocks whose manifest cond-AL word percentage is 0%, restricted to
/// [`IMAGES`].
const ZERO_BLOCKS: [(u32, u32); 12] = [
    (0x0087_2400, 0x0087_3400),
    (0x00cd_1a00, 0x00d0_9a00),
    (0x00e3_aa00, 0x00e4_1a00),
    (0x00e4_7a00, 0x00e4_da00),
    (0x00e8_9a00, 0x00e8_ca00),
    (0x00ea_3a00, 0x00ea_8a00),
    (0x00f1_0a00, 0x00f1_3a00),
    (0x00f5_aa00, 0x00f5_ba00),
    (0x00f8_0a00, 0x00f8_6a00),
    (0x00fb_8a00, 0x00fd_aa00),
    (0x0100_9a00, 0x0102_6a00),
    (0x0102_aa00, 0x0102_ba00),
];

fn admitted_from_images_and_excluded() -> std::vec::Vec<CodeRegion> {
    let mut runs = std::vec::Vec::new();
    for image in IMAGES {
        let mut cursor = image.start;
        for excluded in EXCLUDED {
            if excluded.end <= cursor || excluded.start >= image.end { continue; }
            if excluded.start > cursor { runs.push(CodeRegion::new(cursor, excluded.start.min(image.end))); }
            cursor = cursor.max(excluded.end);
        }
        if cursor < image.end { runs.push(CodeRegion::new(cursor, image.end)); }
    }
    runs
}

#[test]
fn admitted_extents_are_the_images_without_the_derived_data_runs() {
    assert_eq!(admitted_from_images_and_excluded(), ADMITTED.to_vec());
    for pair in EXCLUDED.windows(2) { assert!(pair[0].end <= pair[1].start, "{pair:?}"); }
    for run in EXCLUDED {
        assert!(IMAGES.iter().any(|image| image.contains(run.start as usize) && run.end <= image.end), "{run:?}");
    }
}

#[test]
fn every_manifest_zero_block_run_is_inside_a_derived_data_run() {
    for (start, end) in ZERO_BLOCKS {
        assert!(EXCLUDED.iter().any(|run| run.start <= start && end <= run.end), "{start:#x}..{end:#x}");
    }
}

#[test]
fn one_word_inside_every_derived_data_run_is_refused() {
    let registry = Registry::new();
    for run in EXCLUDED {
        let address = (run.start + (run.end - run.start) / 2) & !3;
        assert_eq!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(address as usize)), Err(CallbackError::Unadmitted), "{address:#x}");
    }
}

#[test]
fn every_stock_callback_word_both_reviews_sampled_stays_admitted() {
    let registry = Registry::new();
    for word in [
        // Image 5: the two handlers at 0xda9bcc/0xda9e44, the timer at 0xd81c98, the detection
        // output at 0xdc8214, the eight producers 0xec4618 posts, and the nearest word to a
        // removed run.
        0x00da_9da0, 0x00d8_13ac, 0x00dc_8400, 0x00ec_4618, 0x00ec_554c, 0x00ec_59e4,
        0x00ec_97f0, 0x00ec_5a78, 0x00ec_5abc, 0x00ec_5b40, 0x00ec_5b84, 0x00ec_5bc8,
        // Image 6: the timer passed at 0x12528a8, the word just before the 0x125f264 run, and the
        // text producer passed at 0x13ed5e4.
        0x0121_ab88, 0x0125_f1e4, 0x013e_b548,
    ] {
        assert!(registry.resolve::<unsafe extern "C" fn()>(CallbackWord(word as usize)).is_ok(), "{word:#x}");
    }
}

#[test]
fn a_host_address_whose_low_half_is_admitted_is_refused() {
    let registry = Registry::new();
    assert_eq!(registry.resolve::<unsafe extern "C" fn(u32)>(CallbackWord(0x7f00_0000_0061_c420)), Err(CallbackError::Unadmitted));
}
