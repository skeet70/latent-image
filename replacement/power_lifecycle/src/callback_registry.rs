//! Admission of code words that stock ABI entries may enter.
//!
//! Stock keeps function words in interface tables and callback slots and enters them with `blx`.
//! The registry replaces that entry: an owned client registers its word, and `resolve` returns a
//! pointer only for a registered word of the same shape or for a word in the admitted text extents.
//! Kernel interface slots are not callbacks; they are read from the kernel root and entered
//! through `KernelSlot`, which bypasses the registry.

use core::cell::UnsafeCell;
#[cfg(not(target_arch = "arm"))]
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackWord(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackError {
    Invalid,
    Unadmitted,
    Shape,
    Full,
}
impl CallbackError {
    /// The status the leaf's ABI entries report for a refused entry.
    pub const fn status(self) -> i32 { -255 }
}

/// The call contract of a word: its argument count and whether it returns a word.
///
/// Every A32 integer, pointer and function argument occupies one core register, so signedness and
/// pointerness are not part of the contract. The word returned by 0x756d04 is entered with five
/// arguments at 0xf24734, 0xf30880 and 0xf50cc8; the word returned by 0x756b9c with thirteen at
/// 0xf19fa0, 0xf34704 and 0xf3664c.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shape {
    arguments: u8,
    returns: bool,
}
impl Shape {
    pub const fn new(arguments: u8, returns: bool) -> Self { Self { arguments, returns } }
    /// `extern "C" fn()`: software timer ticks, IRQ handlers and continuous detection handlers.
    pub const fn completion() -> Self { Self::new(0, false) }
    /// `extern "C" fn(*mut u8, u32)`: the settings text producer.
    pub const fn text_producer() -> Self { Self::new(2, false) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeRegion {
    start: u32,
    end: u32,
}
impl CodeRegion {
    pub const fn new(start: u32, end: u32) -> Self { Self { start, end } }
    /// Host addresses are 64-bit; a `u32` comparison would admit a host address whose low half
    /// falls in a region.
    pub const fn contains(self, address: usize) -> bool {
        self.start < self.end && address >= self.start as usize && address < self.end as usize
    }
}

/// The text extents of images 4, 5 and 6, split so that each range holds code.
///
/// Image 4: code A runs from the first vector-table body 0x61c420 to the last return word
/// 0x7748d8 (id4.dis); code B starts at the function at 0x829d14 and ends before the table word at
/// 0x89e7b0 (id4.dis). The range between them holds rodata and the writable slot 0x829a0c.
/// Image 5: code starts at the function at 0xbf1a34 after the 8-word record header and the
/// five-word table at 0xbf1a20 and ends after the `pop {r4, ..., pc}` at 0x107bcdc; it resumes at
/// the `push` at 0x1083954 and ends after the `ldm sp, {..., pc}` at 0x108df08 and its jump table
/// through 0x108df30; it resumes at the `mov r12, sp` at 0x10a8d08 and ends after the
/// `ldmib sp, {..., pc}` at 0x10af054; it resumes at 0x10e21c4 and ends after the
/// `ldm sp, {..., pc}` at 0x10e3668 (id5.dis). The runs between those ranges are the tables and
/// zero fill the block manifest shows at 0% code words.
/// Image 6: code starts at the first function 0x1100008 and ends after the `b` at 0x11ac594; it
/// resumes at the `str r11, [sp, #-4]!` at 0x11c0004 and ends after the `pop {..., pc}` at
/// 0x11dc398; it resumes after the 15-word pointer table at 0x11e0000, at 0x11e003c, and ends
/// after the `bx lr` at 0x1444914 (img6.dis, img6-funcs.txt). `decode/NOTES.md` names the three
/// sub-images at 0x1100000, 0x11c0000 and 0x11e0000.
///
/// These images have no section table, and their code ranges contain literal pools and read-only
/// tables; range admission cannot exclude those, so a data word inside a code range is admitted.
const ADMITTED: [CodeRegion; 9] = [
    CodeRegion::new(0x0061_c420, 0x0077_48dc),
    CodeRegion::new(0x0082_9d14, 0x0089_e7b0),
    CodeRegion::new(0x00bf_1a34, 0x0107_bce0),
    CodeRegion::new(0x0108_3954, 0x0108_df34),
    CodeRegion::new(0x010a_8d08, 0x010a_f058),
    CodeRegion::new(0x010e_21c4, 0x010e_366c),
    CodeRegion::new(0x0110_0008, 0x011a_c598),
    CodeRegion::new(0x011c_0004, 0x011d_c39c),
    CodeRegion::new(0x011e_003c, 0x0144_4918),
];

/// # Safety
/// `SHAPE` must name the argument count and result presence of the implementing type. `Shape`
/// does not distinguish signedness, pointerness or the width of a returned word.
pub unsafe trait CallbackFn: Copy {
    const SHAPE: Shape;
    fn word(self) -> CallbackWord;
    /// # Safety
    /// `address` is a nonzero aligned word `resolve` returned for `F`. `Shape` records only the
    /// argument count and whether the entry returns a word, so the caller must establish the
    /// callee's actual signature.
    unsafe fn from_address(address: usize) -> Self;
}

macro_rules! argument_count {
    () => { 0u8 };
    ($head:ty $(, $tail:ty)*) => { 1u8 + argument_count!($($tail),*) };
}
macro_rules! has_result {
    () => { false };
    ($result:ty) => { true };
}
macro_rules! callback_functions {
    ($(fn($($argument:ty),*) $(-> $result:ty)?),* $(,)?) => {
        $(
            unsafe impl CallbackFn for unsafe extern "C" fn($($argument),*) $(-> $result)? {
                const SHAPE: Shape = Shape::new(argument_count!($($argument),*), has_result!($($result)?));
                fn word(self) -> CallbackWord { CallbackWord(self as usize) }
                unsafe fn from_address(address: usize) -> Self {
                    unsafe { core::mem::transmute(address) }
                }
            }
        )*
    };
}

callback_functions! {
    fn(),
    fn() -> u32,
    fn() -> i32,
    fn(u32),
    fn(u32) -> i32,
    fn(u32, u32),
    fn(u32, u32) -> i32,
    fn(u32, *const u32) -> i32,
    fn(u32, *mut u32) -> i32,
    fn(i32, i32) -> i32,
    fn(i32) -> i32,
    fn(i32, *mut u32, i32) -> i32,
    fn(i32, u32) -> i32,
    fn(i32, u32),
    fn(i32, u32, u32, *mut u32, i32) -> i32,
    fn(*mut u8, u32),
    fn(i32),
    fn(*const u8) -> i32,
    fn(u32, *const u8),
    fn(u32, u32, u32, u32, u32),
    fn(u32, u32, u32, u32),
    fn(u32, u32, u32),
    fn(u32, *const u32),
    fn(*const u32),
    fn(i32, u32, u32, u32),
    fn(u32, u32, u32, u32, u32, u32),
    fn(i32, i32),
    fn(u32, u32, u32, u32, u32, u32, u32, u32, u32),
    fn(u32, i32, u32, i32, u32, u32, u32, u32, i32, i32, i32, i32, u32),
    fn(u32, u32, i32, i32, u32),
    fn(i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32),
}

/// A kernel interface word read from the kernel root.
///
/// The root at 0x96bd20 selects the per-core interface block: 0x725a0c..0x725a30 stores the two
/// loaders' blocks at 0x96bd20 and 0x96bd24. The block holds the core query at +0x1618 and the
/// block allocation at +0x170; 0x7250f0 reads +0x170 from the block, not from the root. Loader A
/// stores 0x412bc and 0x411cc, loader B 0xd12bc and 0xd11cc. No client registers these words, so a
/// slot is entered directly, without the registry, and `F` is the caller's declaration of the
/// kernel ABI at that offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSlot(CallbackWord);
impl KernelSlot {
    pub const fn word(self) -> CallbackWord { self.0 }
    /// # Safety
    /// `base + offset` must hold a kernel interface word.
    pub unsafe fn read(base: u32, offset: u32) -> Self {
        Self(CallbackWord(unsafe { core::ptr::read_volatile((base + offset) as *const u32) } as usize))
    }
    /// # Safety
    /// The slot must hold an entry with the signature `F`.
    pub unsafe fn entry<F: CallbackFn>(self) -> F { unsafe { F::from_address(self.0.0) } }
}

/// Reads the kernel interface word at `base + offset` and enters it as `F`.
///
/// # Safety
/// `base + offset` must hold a kernel interface word with the signature `F`.
pub(crate) unsafe fn kernel_slot<F: CallbackFn>(base: u32, offset: u32) -> F {
    unsafe { KernelSlot::read(base, offset).entry() }
}

#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32;
}

/// 0x7235d8 masks interrupts and takes the kernel's cross-core lock.
struct Shared<T> {
    #[cfg(not(target_arch = "arm"))]
    locked: AtomicU32,
    value: UnsafeCell<T>,
}
unsafe impl<T: Send> Sync for Shared<T> {}
impl<T> Shared<T> {
    const fn new(value: T) -> Self {
        Self {
            #[cfg(not(target_arch = "arm"))]
            locked: AtomicU32::new(0),
            value: UnsafeCell::new(value),
        }
    }
    #[cfg(target_arch = "arm")]
    fn with<R>(&self, operation: impl FnOnce(&mut T) -> R) -> R {
        let saved = unsafe { xm1_power_interrupt_lock() };
        let result = operation(unsafe { &mut *self.value.get() });
        unsafe { xm1_power_interrupt_unlock(saved) };
        result
    }
    #[cfg(not(target_arch = "arm"))]
    fn with<R>(&self, operation: impl FnOnce(&mut T) -> R) -> R {
        while self.locked.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() { core::hint::spin_loop(); }
        struct Unlock<'a>(&'a AtomicU32);
        impl Drop for Unlock<'_> { fn drop(&mut self) { self.0.store(0, Ordering::Release); } }
        let unlock = Unlock(&self.locked);
        let result = operation(unsafe { &mut *self.value.get() });
        drop(unlock);
        result
    }
}

/// The owned words the whole tree registers.
///
/// Leaf (24): `xm1_power_software_timer_service_irq`, `xm1_power_software_timer_tick`,
/// `detection_low_irq`, `detection_high_irq`, `xm1_power_software_delay_tick`, `irq_zero`,
/// `irq_one`, `irq_two`, `irq_three`, `signal0`..`signal4`, `xm1_power_owned_log_trace`,
/// `xm1_power_owned_service_trace`, `xm1_power_diagnostic_report_status`,
/// `xm1_power_settings_timer_notify`, `xm1_power_task6_timer_irq`,
/// `xm1_power_audio_operation_tick` (defined by the application crate and registered by the
/// leaf's owned entry), `xm1_power_task7_shutdown_callback`,
/// `xm1_power_task8_shutdown_callback`, `xm1_power_settings_shutdown_callback` and
/// `xm1_storage_shutdown_dma`.
///
/// Application (4): `xm1_power_audio_stream_receive_complete`,
/// `xm1_power_audio_stream_send_complete`, `xm1_power_photoflash_poll_timer` and
/// `xm1_power_recording_timer_expired`. Display (4): `xm1_scanout_panel_post_timer_command`,
/// `xm1_scanout_isr_zero`, `xm1_scanout_isr_one` and `xm1_scanout_commit_one`. NAND (1):
/// `xm1_app_nand_ready_irq`. Vendor (6): the four `TimerCallbackEntries` producers, the
/// continuous detection handler and the EEPROM detection handler.
///
/// That is 39 words. `ENTRIES` leaves 9 spare, about 23% headroom.
const ENTRIES: usize = 48;

#[derive(Clone, Copy)]
struct Entry { word: CallbackWord, shape: Shape }
struct State { entries: [Option<Entry>; ENTRIES] }

pub struct CallbackRegistry { state: Shared<State> }
impl CallbackRegistry {
    pub const fn new() -> Self {
        Self { state: Shared::new(State { entries: [None; ENTRIES] }) }
    }
    fn resolves<F: CallbackFn>(state: &State, word: CallbackWord) -> bool {
        state.entries.iter().flatten().any(|entry| entry.word == word && entry.shape == F::SHAPE)
    }
    /// # Safety
    /// `word` must be an owned entry whose argument count and result presence `shape` names.
    pub unsafe fn ensure_word(&self, word: CallbackWord, shape: Shape) -> Result<(), CallbackError> {
        if word.0 == 0 || word.0 & 3 != 0 { return Err(CallbackError::Invalid); }
        self.state.with(|state| {
            if state.entries.iter().flatten().any(|entry| entry.word == word && entry.shape == shape) { return Ok(()); }
            if state.entries.iter().flatten().any(|entry| entry.word == word) { return Err(CallbackError::Shape); }
            if ADMITTED.iter().any(|region| region.contains(word.0)) { return Ok(()); }
            let slot = state.entries.iter().position(Option::is_none).ok_or(CallbackError::Full)?;
            state.entries[slot] = Some(Entry { word, shape });
            Ok(())
        })
    }
    /// Registers `callback` unless its word already resolves for its shape.
    pub fn ensure<F: CallbackFn>(&self, callback: F) -> Result<(), CallbackError> {
        unsafe { self.ensure_word(callback.word(), F::SHAPE) }
    }
    /// Resolves `word` for this shape or reports why it is refused.
    pub fn resolve<F: CallbackFn>(&self, word: CallbackWord) -> Result<F, CallbackError> {
        if word.0 == 0 { return Err(CallbackError::Invalid); }
        if word.0 & 3 != 0 { return Err(CallbackError::Invalid); }
        self.state.with(|state| {
            if Self::resolves::<F>(state, word) { return Ok(unsafe { F::from_address(word.0) }); }
            if state.entries.iter().flatten().any(|entry| entry.word == word) { return Err(CallbackError::Shape); }
            if ADMITTED.iter().any(|region| region.contains(word.0)) {
                return Ok(unsafe { F::from_address(word.0) });
            }
            Err(CallbackError::Unadmitted)
        })
    }
}
impl Default for CallbackRegistry {
    fn default() -> Self { Self::new() }
}

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

/// Registers an owned handler word for the shape its consumer resolves.
///
/// This crate depends on `display_scanout` and `vendor_protocol`, so they declare this entry
/// instead of calling `ensure`. `Completion` (`extern "C" fn()`) is the shape of
/// `xm1_scanout_panel_post_timer_command` in display_scanout `panel_device.rs`, the four
/// `TimerCallbackEntries` producers in vendor_protocol `controller_registration.rs`, the
/// continuous detection handler in `updater_peripheral_gpio.rs` and the detection handler in
/// `eeprom_detection.rs`. `TextProducer` (`extern "C" fn(*mut u8, u32)`) is the settings text
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
