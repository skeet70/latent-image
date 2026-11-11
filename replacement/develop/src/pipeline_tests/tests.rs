use super::font_script_store::GuestStore;
use super::font_scripts::{xm1_pipeline_image_font_detect_mode, xm1_pipeline_image_font_detect_script_run};
use super::shared::Address32;
use crate::develop::font_core::shaping::{
    CharacterCode, CharacterIndex, GlyphIndex, RunIndex, ShapingStore,
};
use crate::develop::font_core::FontError;

#[cfg(target_os = "linux")]
fn map_guest_page(base: usize, length: usize) -> GuestStore {
    unsafe extern "C" {
        fn mmap(address: *mut core::ffi::c_void, length: usize, protection: i32, flags: i32, fd: i32, offset: isize) -> *mut core::ffi::c_void;
    }
    unsafe {
        let mapped = mmap(base as *mut _, length, 3, 0x100022, -1, 0);
        assert_eq!(mapped as usize, base);
        GuestStore::new(Address32(base as u32))
    }
}

#[cfg(target_os = "linux")]
#[test]
fn store_accessors_stop_at_the_fiftieth_record() {
    let mut store = map_guest_page(0x0094_3000, 0x10000);
    let base = Address32(0x0094_3000);
    unsafe { base.offset(super::shared::ByteOffset(0x53c)).write_word(0x0094_b000) };
    unsafe { base.offset(super::shared::ByteOffset(0x606)).write_half(0x1234) };

    assert_eq!(store.main_run(RunIndex(49)).map(|_| ()), Ok(()));
    assert_eq!(store.main_run(RunIndex(50)), Err(FontError::Truncated));
    assert_eq!(store.position(GlyphIndex(49)).map(|_| ()), Ok(()));
    assert_eq!(store.position(GlyphIndex(50)), Err(FontError::Truncated));
    assert_eq!(store.output_code(GlyphIndex(49)), Ok(CharacterCode(0)));
    assert_eq!(store.output_code(GlyphIndex(50)), Err(FontError::Truncated));
    assert_eq!(store.set_output_code(GlyphIndex(50), CharacterCode(0x41)), Err(FontError::Truncated));
    assert_eq!(store.mapping(CharacterIndex(50)).map(|_| ()), Err(FontError::Truncated));
    assert_eq!(store.metric(CharacterIndex(50)).map(|_| ()), Err(FontError::Truncated));
}

#[cfg(target_os = "linux")]
#[test]
fn glyph_before_zero_reads_the_halfword_before_the_first_mapping() {
    let mut store = map_guest_page(0x0096_0000, 0x1000);
    unsafe { Address32(0x0096_0000).offset(super::shared::ByteOffset(0x606)).write_half(0x1234) };
    assert_eq!(store.glyph_before(CharacterIndex(0)), Ok(GlyphIndex(0x1234)));
    assert_eq!(store.glyph_before(CharacterIndex(51)), Err(FontError::Truncated));
}

#[test]
fn detect_mode_accepts_a_null_text_pointer() {
    let mut mode = -1;
    let mut language = -1;
    let direction = unsafe { xm1_pipeline_image_font_detect_mode(core::ptr::null(), 0, &mut mode, &mut language) };
    assert_eq!((mode, language, direction), (0, 0, 0));
    let direction = unsafe { xm1_pipeline_image_font_detect_mode(core::ptr::null(), 4, &mut mode, &mut language) };
    assert_eq!((mode, language, direction), (0, 0, 0));
    let mut mode = -1;
    let mut language = -1;
    let direction = unsafe { xm1_pipeline_image_font_detect_mode(core::ptr::null(), -3, &mut mode, &mut language) };
    assert_eq!((mode, language, direction), (0, 0, 0));
}

#[test]
fn detect_script_run_accepts_a_null_text_pointer() {
    let mut end = 0xffffu16;
    let mut mode = -1;
    let mut language = -1;
    let mut direction = 0xffffu16;
    let length = unsafe {
        xm1_pipeline_image_font_detect_script_run(core::ptr::null(), 0, 0, &mut end, 0x200e, &mut mode, &mut language, &mut direction)
    };
    assert_eq!((end, mode, language, direction, length), (0, 1, 1, 0x200e, 0));
    let length = unsafe {
        xm1_pipeline_image_font_detect_script_run(core::ptr::null(), 0, 0, &mut end, 0x200f, &mut mode, &mut language, &mut direction)
    };
    assert_eq!((end, mode, language, direction, length), (0, 2, 5, 0x200f, 0));
}

#[test]
fn detect_script_run_writes_only_the_outputs_it_is_given() {
    let text = [0x41u16];
    let length = unsafe {
        xm1_pipeline_image_font_detect_script_run(text.as_ptr(), 1, 0, core::ptr::null_mut(), 0x200e, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut())
    };
    assert_eq!(length, 1);
}
