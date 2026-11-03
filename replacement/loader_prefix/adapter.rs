//! Recovered X-M1 1.50 loader-B (segment id 1, loaded at `0xd0000`) values the
//! owned core-1 prefix replaces. Retained loader-B addresses are linker
//! symbols in `loader-prefix.ld`.

pub(crate) const DISCRIMINATOR_PRIMARY: u32 = 0x4003;

/// Stock `0xd0028`-`0xd0030`.
pub(crate) const VECTOR_BASE_PRIMARY: u32 = 0;
pub(crate) const VECTOR_BASE_SECONDARY: u32 = 0xffff_0000;

/// Stock `0xd0034`-`0xd0084`: handler entries stored at base `+0x24`..`+0x3c`.
pub(crate) const VECTOR_SLOTS_OFFSET: u32 = 0x24;
#[cfg(not(loader_prefix_vector_fault))]
pub(crate) const VECTOR_HANDLERS: [u32; 7] = [0xd0100, 0xd01b4, 0xd0128, 0xd0148, 0xd0168, 0xd0204, 0xd0184];
#[cfg(loader_prefix_vector_fault)]
pub(crate) const VECTOR_HANDLERS: [u32; 7] = [0xd0100, 0xd01b4, 0xd0128, 0xd0148, 0xd0168, 0xd0204, 0xd0188];

/// Stock `0xd00a0`: cleared on the entry stack up to the loader-B stack top.
pub(crate) const STACK_WINDOW_START: u32 = 0x400f_2c14;
/// Stock `0xd00bc`: cleared on the loader-B stack up to [`STACK_WINDOW_START`].
pub(crate) const WORKSPACE_START: u32 = 0x400d_daa0;

/// The vector base the entry assembly keeps in `r6`.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct VectorBase(pub(crate) u32);

extern "C" {
    /// `0x400f3420`.
    pub(crate) static xm1_loader_b_stack_top: [u32; 0];

    /// Zeroes `[start, end)`, eight words per store (`entry.S`).
    pub(crate) fn xm1_loader_prefix_clear(start: u32, end: u32);
}
