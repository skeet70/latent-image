//! X-M1 1.50 display-buffer teardown at eeb5b4.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 and actual image-5 bytes.

use crate::record_address_space::{GuestAddressSpace, RecordContext};
use crate::record_image_init_state::{put, word, ImageInitError};
const DISPLAY: usize = 0x0169b458;
unsafe extern "C" {
    fn xm1_scanout_mode_window(selector: u32, packet: *const u16) -> i32;
    fn xm1_scanout_mode_window_size(selector: u32, packet: *const u32) -> i32;
    fn xm1_scanout_mode_scale(packet: *const u8) -> i32;
    fn xm1_scanout_mode_backing(value: u32) -> i32;
    fn xm1_scanout_mode_colour_packet(packet: *const u8) -> i32;
    fn xm1_scanout_mode_select(value: u32) -> i32;
    fn xm1_scanout_mode_colour(value: u32) -> i32;
    fn xm1_scanout_mode_enabled(value: u32) -> i32;
    fn xm1_scanout_mode_commit(callback: u32) -> i32;
}

pub(crate) unsafe fn reset_display_path<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, ImageInitError> {
    if word(context, DISPLAY)? == 0 { return Ok(0x81); }
    if word(context, DISPLAY)? & 4 != 0 {
        if unsafe { xm1_scanout_mode_window(0, 0x0169b484 as *const u16) } != 0 { return Ok(0x81); }
        if unsafe { xm1_scanout_mode_window_size(0, 0x0169b460 as *const u32) } != 0 { return Ok(0x81); }
        if unsafe { xm1_scanout_mode_scale(0x0169b48c as *const u8) } != 0 { return Ok(0x81); }
        if unsafe { xm1_scanout_mode_backing(word(context, DISPLAY + 0x4c)?) } != 0 { return Ok(0x81); }
        if unsafe { xm1_scanout_mode_colour_packet(0x0169b44c as *const u8) } != 0 { return Ok(0x81); }
        let cleared = word(context, DISPLAY)? & !4;
        put(context, DISPLAY, cleared)?;
    }
    if word(context, DISPLAY)? & 0x10 != 0 {
        if unsafe { xm1_scanout_mode_select(word(context, DISPLAY + 0x10)?) } != 0 { return Ok(0x81); }
        let cleared = word(context, DISPLAY)? & !0x10;
        put(context, DISPLAY, cleared)?;
    }
    if word(context, DISPLAY)? & 0x40 != 0 {
        unsafe { xm1_scanout_mode_colour(word(context, DISPLAY + 0x20)?) };
        let cleared = word(context, DISPLAY)? & !0x40;
        put(context, DISPLAY, cleared)?;
    }
    if word(context, DISPLAY)? & 1 != 0 {
        if unsafe { xm1_scanout_mode_enabled(word(context, DISPLAY + 0x28)?) } != 0 { return Ok(0x81); }
        let cleared = word(context, DISPLAY)? & !1;
        put(context, DISPLAY, cleared)?;
    }
    Ok(if unsafe { xm1_scanout_mode_commit(0) } != 0 { 0x81 } else { 0 })
}
