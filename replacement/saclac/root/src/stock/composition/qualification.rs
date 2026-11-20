//! The qualification ABI (issue 117, X-M1 1.50).
//!
//! Compiled only with `--cfg saclac_qualification`; never part of a production artifact.
//! The field table, per-machine encoders/decoders and overlay codec live in
//! `stock::qualification` so they are host-testable; this module re-exports them and adds the
//! two exported entries that read `OWNER`/`READY`.
//!
//! `XM1_SACLAC_QUALIFICATION_FIELDS` lists `(stock address, byte length)` in overlay order;
//! `XM1_SACLAC_QUALIFICATION_FIELD_COUNT` is its length. `encode` writes the overlay from
//! Root's state at `owner`; `roundtrip` decodes a stock overlay and encodes it back. Refusal
//! codes: `1` Root not ready, `2` no typed counterpart, `3` encoder refused, `4` a `RefCell`
//! the view reads is mutably borrowed. `*gap` carries `field_index << 16 | detail`.

use core::ptr;

pub use crate::stock::qualification::*;

/// `(stock address, byte length)` per mapped field, in overlay order.
#[used]
#[no_mangle]
pub static XM1_SACLAC_QUALIFICATION_FIELDS: [[u32; 2]; FIELD_COUNT] = QUALIFICATION_FIELDS;

/// The number of entries in `XM1_SACLAC_QUALIFICATION_FIELDS`.
#[no_mangle]
pub static XM1_SACLAC_QUALIFICATION_FIELD_COUNT: u32 = FIELD_COUNT as u32;

unsafe fn write_gap(gap: *mut u32, field: usize, detail: u32) {
    if !gap.is_null() { unsafe { ptr::write_unaligned(gap, (field as u32) << 16 | detail) }; }
}

/// Writes the overlay from Root's state at `owner`; `ready` is the address of the
/// qualification build's `READY` byte. Returns 0 or a refusal code with `*gap` set.
///
/// A refused field is not written: the caller may prefill the overlay with the measured
/// stock bytes, and the gap names exactly what Root cannot supply.
///
/// # Safety
/// `owner` must be the address of the production `OWNER`, `ready` the address of `READY`,
/// and `out` must hold `OVERLAY_BYTES` bytes. `gap` may be null.
#[no_mangle]
pub unsafe extern "C" fn xm1_saclac_qualification_encode(owner: u32, ready: u32, out: *mut u8, gap: *mut u32) -> u32 {
    if ready == 0 || out.is_null() { return 3; }
    if unsafe { ptr::read_volatile(ready as *const u8) } == 0 { return 1; }
    if owner == 0 { return 1; }
    let owner = owner as *const super::Owner;
    // SAFETY: the caller passes the qualified build's OWNER address.
    let view = match unsafe { crate::runtime::qualification::view(&(*owner).root) } {
        Some(view) => view,
        None => { unsafe { write_gap(gap, 0, 4) }; return 4; }
    };
    let mut refusal = Refusal::new();
    unsafe { encode_overlay(&view, out, &mut refusal) };
    match refusal.first() {
        Some((field, detail)) => { unsafe { write_gap(gap, field, detail) }; refusal.code() }
        None => 0,
    }
}

/// Decodes a stock overlay into the typed views and encodes it back into `out`. Returns 0,
/// or 2 when a stock value has no typed counterpart, or 3 when a re-encode refused. `*gap`
/// carries `field_index << 16 | detail`.
///
/// # Safety
/// `stock` and `out` must each hold `OVERLAY_BYTES` bytes and must not overlap. `gap` may
/// be null.
#[no_mangle]
pub unsafe extern "C" fn xm1_saclac_qualification_roundtrip(stock: *const u8, out: *mut u8, gap: *mut u32) -> u32 {
    if stock.is_null() || out.is_null() { return 3; }
    let (code, gap_word) = unsafe { roundtrip_overlay(stock, out) };
    if let Some(word) = gap_word {
        if !gap.is_null() { unsafe { ptr::write_unaligned(gap, word) }; }
    }
    code
}
