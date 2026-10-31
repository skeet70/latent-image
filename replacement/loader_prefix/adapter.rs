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

/// Loader A stores its completion here at `0x40a08`.
pub(crate) const COMPLETION: *const u32 = 0x400a_00c4 as *const u32;
/// Stock `0xd09c8`: NOP iterations between completion polls.
pub(crate) const COMPLETION_DELAY_ITERATIONS: u32 = 100;

/// A retained loader-B address the prefix exits to.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct Continuation(u32);

impl Continuation {
    pub(crate) fn of(entry: unsafe extern "C" fn()) -> Self {
        Continuation(entry as *const () as u32)
    }
}

/// The vector base the entry assembly keeps in `r6`.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct VectorBase(pub(crate) u32);

extern "C" {
    /// `0x400f3420`, also the loader-B frame's first word.
    pub(crate) static xm1_loader_b_stack_top: [u32; 0];

    /// `0xd0aac`: stock core-1 exit from the completion wait.
    pub(crate) fn xm1_loader_b_core1_continuation();
    /// `0xd09d0`: stock continuation on the `0x4003` identity.
    pub(crate) fn xm1_loader_b_core0_continuation();
}
