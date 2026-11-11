//! Admission of code words that stock ABI entries may enter.
//!
//! Stock keeps function words in interface tables and callback slots and enters them with `blx`.
//! The registry replaces that entry: an owned client registers its word, and `resolve` returns a
//! pointer only for a registered word of the same shape or for a word in the admitted text extents.
//! Kernel interface slots are not callbacks; they are read from the kernel root and entered
//! through `KernelSlot`, which bypasses the registry. The table, the extents and the kernel-slot
//! reader live in `xm1_callback_core` so the develop unit uses the same mechanism.

pub use xm1_callback_core::{CallbackError, CallbackFn, CallbackToken, CallbackWord, CodeRegion, KernelSlot, Shape, kernel_slot};
use xm1_callback_core::CallbackRegistry as CoreRegistry;

#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32;
}

/// 0x7235d8 masks interrupts and takes the kernel's cross-core lock.
pub struct PowerCritical;
#[cfg(target_arch = "arm")]
impl xm1_callback_core::CriticalSection for PowerCritical {
    fn enter() -> u32 { unsafe { xm1_power_interrupt_lock() } }
    fn leave(saved: u32) { unsafe { xm1_power_interrupt_unlock(saved) } }
}
#[cfg(not(target_arch = "arm"))]
impl xm1_callback_core::CriticalSection for PowerCritical {
    fn enter() -> u32 {
        while HOST_LOCK.compare_exchange_weak(0, 1, core::sync::atomic::Ordering::Acquire, core::sync::atomic::Ordering::Relaxed).is_err() { core::hint::spin_loop(); }
        0
    }
    fn leave(_: u32) { HOST_LOCK.store(0, core::sync::atomic::Ordering::Release); }
}
#[cfg(not(target_arch = "arm"))]
static HOST_LOCK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

pub type CallbackRegistry = CoreRegistry<PowerCritical>;

/// The leaf, application, display, NAND and vendor crates register 44 owned words; the shared
/// table holds those plus twenty spare entries.
static REGISTRY: CallbackRegistry = CallbackRegistry::new();

pub fn registry() -> &'static CallbackRegistry { &REGISTRY }

/// The shapes an upstream crate can register through [`xm1_power_owned_callback_register`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedShape {
    /// `extern "C" fn()`: software timer ticks, IRQ handlers and continuous detection handlers.
    Completion = 0,
    /// `extern "C" fn(*mut u8, u32)`: the settings text producer.
    TextProducer = 1,
}
impl OwnedShape {
    pub const fn from_abi(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Completion), 1 => Some(Self::TextProducer), _ => None }
    }
    pub const fn shape(self) -> Shape {
        match self { Self::Completion => Shape::completion(), Self::TextProducer => Shape::text_producer() }
    }
}

/// The ABI `shape` value of [`OwnedShape::Completion`].
pub const COMPLETION_SHAPE: u32 = OwnedShape::Completion as u32;

/// `display_scanout` and `vendor_protocol` depend on this crate, so they declare this entry
/// instead of calling `ensure`; vendor_protocol's `eeprom_detection.rs` calls `ensure` directly.
/// `Completion` (`extern "C" fn()`) is the shape of
/// `xm1_scanout_panel_post_timer_command` in display_scanout `panel_device.rs`, the four
/// `TimerCallbackEntries` producers in vendor_protocol `controller_registration.rs`, the
/// continuous detection handler in `updater_peripheral_gpio.rs` and the three detection handlers
/// in `eeprom_detection.rs`. `TextProducer` (`extern "C" fn(*mut u8, u32)`) is the settings text
/// producer.
///
/// # Safety
/// `word` must be an owned entry whose argument count and result presence `shape` names.
pub unsafe fn register_owned(word: CallbackWord, shape: OwnedShape) -> Result<(), CallbackError> {
    unsafe { registry().ensure_word(word, shape.shape()) }
}

/// 0x64e954..0x64e978 reports a refused interrupt handler as command event (2, 15).
fn refusal_diagnostic() {
    #[cfg(target_arch = "arm")]
    unsafe { xm1_power_command_diagnostic(2, 15); }
}

/// Reports a refused word once and returns the status its entry reports.
pub fn refused(error: CallbackError) -> i32 {
    refusal_diagnostic();
    error.status()
}

/// # Safety
/// `word` must be an owned entry with the signature `shape` names.
#[no_mangle]
pub unsafe extern "C" fn xm1_power_owned_callback_register(word: u32, shape: u32) -> i32 {
    let Some(shape) = OwnedShape::from_abi(shape) else { refusal_diagnostic(); return -30; };
    match unsafe { register_owned(CallbackWord(word as usize), shape) } {
        Ok(()) => 0,
        Err(error) => refused(error),
    }
}

#[cfg(test)]
#[path = "callback_registry_tests.rs"]
mod tests;
