//! Admission and entry of the code words stock keeps in interface tables and callback slots.
//!
//! Stock enters those words with `blx`. A word an owned client registers is resolved for the
//! shape it registered; a word in an admitted text extent is entered directly. Kernel interface
//! slots are not callbacks: they are read from the kernel root and entered through [`KernelSlot`],
//! which never consults the registry.

#![cfg_attr(not(test), no_std)]

use core::cell::UnsafeCell;
use core::marker::PhantomData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackWord(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackToken(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackError {
    Invalid,
    Unadmitted,
    Shape,
    Full,
}
impl CallbackError {
    /// The status the units' ABI entries report for a refused entry.
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
/// `ldm sp, {..., pc}` at 0x10e3668 (id5.dis).
/// Image 6: code starts at the first function 0x1100008 and ends after the `b` at 0x11ac594; it
/// resumes at the `str r11, [sp, #-4]!` at 0x11c0004 and ends after the `pop {..., pc}` at
/// 0x11dc398; it resumes after the 15-word pointer table at 0x11e0000, at 0x11e003c, and ends
/// after the `bx lr` at 0x1444914 (img6.dis, img6-funcs.txt). `decode/NOTES.md` names the three
/// sub-images at 0x1100000, 0x11c0000 and 0x11e0000.
///
/// [`IMAGES`] holds those nine ranges; [`EXCLUDED`] holds the data runs inside them and
/// [`ADMITTED`] is their complement. The runs come from the 4 KiB block manifests
/// `private-data/analysis/x-m1-1.50/lz/blocks-id4-0061c400.txt` (sha256
/// 6a1d3631928ed676bcd7fd46eab2f3933c59d7110c226d19598037d9538de607) and
/// `blocks-id5-00bf1a00.txt` (sha256
/// 892a055039025d7b25b7ef929204c607f2a9d93af02eb6d38f74947fff9c7cfe): a maximal run of blocks
/// whose cond-AL word percentage is 0%. Each run is widened to the fence the listing shows, the
/// word after the last return and the first prologue or function start, in `id4.dis` (sha256
/// 190e77ba7dce714c577ed9fbb92d06169ee56660f54f94da055a67b2eb0979c1) and `id5.dis` (sha256
/// d9e83e29698af87f60147093d5256c1ed74be35413c6ce47b846de30780cf74c). Image 6 has no block
/// manifest; its runs are the gaps between the functions in `img6-funcs.txt` (sha256
/// a67a0c29f09eea31b3e1aa574f1f205001048674a33709caa882823fa6d9aede), bounded in `img6.dis`
/// (sha256 186f84e0a58c4906add917f5660780c647a33483f6fe3d9707660e1bde2833bd) and
/// `plain-01100000.bin` (sha256 ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d).
/// The images have no section table, and their remaining code ranges contain literal pools and
/// read-only tables; range admission cannot exclude those, so a data word inside a code range is
/// admitted.
#[cfg(test)]
const IMAGES: [CodeRegion; 9] = [
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

/// The data runs inside [`IMAGES`], in address order.
#[cfg(test)]
const EXCLUDED: [CodeRegion; 16] = [
    CodeRegion::new(0x0087_1acc, 0x0087_378c),
    CodeRegion::new(0x00cd_0c78, 0x00d0_a238),
    CodeRegion::new(0x00e3_a028, 0x00e4_2638),
    CodeRegion::new(0x00e4_728c, 0x00e4_e0b4),
    CodeRegion::new(0x00e8_8f5c, 0x00e8_d1ec),
    CodeRegion::new(0x00ea_2b4c, 0x00ea_a430),
    CodeRegion::new(0x00f1_0808, 0x00f1_4668),
    CodeRegion::new(0x00f5_a608, 0x00f5_e420),
    CodeRegion::new(0x00f8_0850, 0x00f8_7124),
    CodeRegion::new(0x00fb_8374, 0x00fd_b0d8),
    CodeRegion::new(0x0100_9988, 0x0102_7890),
    CodeRegion::new(0x0102_a3c8, 0x0102_c8c8),
    CodeRegion::new(0x0110_915c, 0x0110_f960),
    CodeRegion::new(0x0112_0448, 0x0112_08f0),
    CodeRegion::new(0x0125_f264, 0x0127_2164),
    CodeRegion::new(0x012b_2cb0, 0x012d_4474),
];

/// [`IMAGES`] with every [`EXCLUDED`] run removed.
const ADMITTED: [CodeRegion; 25] = [
    CodeRegion::new(0x0061_c420, 0x0077_48dc),
    CodeRegion::new(0x0082_9d14, 0x0087_1acc),
    CodeRegion::new(0x0087_378c, 0x0089_e7b0),
    CodeRegion::new(0x00bf_1a34, 0x00cd_0c78),
    CodeRegion::new(0x00d0_a238, 0x00e3_a028),
    CodeRegion::new(0x00e4_2638, 0x00e4_728c),
    CodeRegion::new(0x00e4_e0b4, 0x00e8_8f5c),
    CodeRegion::new(0x00e8_d1ec, 0x00ea_2b4c),
    CodeRegion::new(0x00ea_a430, 0x00f1_0808),
    CodeRegion::new(0x00f1_4668, 0x00f5_a608),
    CodeRegion::new(0x00f5_e420, 0x00f8_0850),
    CodeRegion::new(0x00f8_7124, 0x00fb_8374),
    CodeRegion::new(0x00fd_b0d8, 0x0100_9988),
    CodeRegion::new(0x0102_7890, 0x0102_a3c8),
    CodeRegion::new(0x0102_c8c8, 0x0107_bce0),
    CodeRegion::new(0x0108_3954, 0x0108_df34),
    CodeRegion::new(0x010a_8d08, 0x010a_f058),
    CodeRegion::new(0x010e_21c4, 0x010e_366c),
    CodeRegion::new(0x0110_0008, 0x0110_915c),
    CodeRegion::new(0x0110_f960, 0x0112_0448),
    CodeRegion::new(0x0112_08f0, 0x011a_c598),
    CodeRegion::new(0x011c_0004, 0x011d_c39c),
    CodeRegion::new(0x011e_003c, 0x0125_f264),
    CodeRegion::new(0x0127_2164, 0x012b_2cb0),
    CodeRegion::new(0x012d_4474, 0x0144_4918),
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
    fn(u32) -> u32,
    fn(u32, u32),
    fn(u32, u32) -> i32,
    fn(u32, u32) -> u32,
    fn(u32, *const u32) -> i32,
    fn(u32, *mut u32) -> i32,
    fn(u32, *mut u32, i32) -> i32,
    fn(i32, i32) -> i32,
    fn(i32) -> i32,
    fn(i32, *mut u32, i32) -> i32,
    fn(i32, u32) -> i32,
    fn(i32, u32),
    fn(i32, u32, u32, *mut u32, i32) -> i32,
    fn(u32, u32, u32, *mut u32, i32) -> i32,
    fn(u32, u32, u32) -> i32,
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
    fn(u32, i32, i32),
    fn(i32, u32, u32),
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
pub struct KernelSlot(pub CallbackWord);
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
pub unsafe fn kernel_slot<F: CallbackFn>(base: u32, offset: u32) -> F {
    unsafe { KernelSlot::read(base, offset).entry() }
}

/// The interrupt primitive a registry takes around a table access.
///
/// The Power services leaf masks IRQ and FIQ and takes the kernel's cross-core lock (0x7235d8);
/// the develop unit takes the kernel interrupt lock its own entries use. Both save and restore, so
/// the access is nestable.
pub trait CriticalSection {
    fn enter() -> u32;
    fn leave(token: u32);
}

struct Shared<T> { value: UnsafeCell<T> }
unsafe impl<T: Send> Sync for Shared<T> {}
impl<T> Shared<T> {
    const fn new(value: T) -> Self { Self { value: UnsafeCell::new(value) } }
    fn with<C: CriticalSection, R>(&self, operation: impl FnOnce(&mut T) -> R) -> R {
        let token = C::enter();
        let result = operation(unsafe { &mut *self.value.get() });
        C::leave(token);
        result
    }
}

/// The leaf, application, display, NAND and vendor crates register 44 owned words; the table
/// holds those plus twenty spare entries.
const ENTRIES: usize = 64;
const FIRST_TOKEN: u32 = 0x4000_0001;

#[derive(Clone, Copy)]
struct Entry { token: CallbackToken, word: CallbackWord, address: usize, shape: Shape }
struct State { entries: [Option<Entry>; ENTRIES], next: u32 }

pub struct CallbackRegistry<C: CriticalSection> { state: Shared<State>, critical: PhantomData<fn() -> C> }
impl<C: CriticalSection> CallbackRegistry<C> {
    pub const fn new() -> Self {
        Self { state: Shared::new(State { entries: [None; ENTRIES], next: FIRST_TOKEN }), critical: PhantomData }
    }
    fn entry_for(state: &State, word: CallbackWord) -> Option<Entry> {
        state.entries.iter().flatten().copied().find(|entry| entry.word == word)
    }
    /// # Safety
    /// `word` must be an owned entry whose argument count and result presence `shape` names.
    pub unsafe fn ensure_word(&self, word: CallbackWord, shape: Shape) -> Result<(), CallbackError> {
        if word.0 == 0 || word.0 & 3 != 0 { return Err(CallbackError::Invalid); }
        self.state.with::<C, _>(|state| {
            if let Some(entry) = Self::entry_for(state, word) {
                return if entry.shape == shape { Ok(()) } else { Err(CallbackError::Shape) };
            }
            if ADMITTED.iter().any(|region| region.contains(word.0)) { return Ok(()); }
            Self::insert(state, word, word.0, shape).map(|_| ())
        })
    }
    pub fn ensure<F: CallbackFn>(&self, callback: F) -> Result<(), CallbackError> {
        unsafe { self.ensure_word(callback.word(), F::SHAPE) }
    }
    pub fn register<F: CallbackFn>(&self, callback: F) -> Result<CallbackToken, CallbackError> {
        let word = callback.word();
        if word.0 == 0 || word.0 & 3 != 0 { return Err(CallbackError::Invalid); }
        self.state.with::<C, _>(|state| Self::insert(state, word, word.0, F::SHAPE))
    }
    /// Records a word the guest supplies, such as an interface-table entry, for one shape.
    pub fn admit_word<F: CallbackFn>(&self, word: CallbackWord) -> Result<(), CallbackError> {
        if word.0 == 0 || word.0 & 3 != 0 { return Err(CallbackError::Invalid); }
        self.state.with::<C, _>(|state| {
            if let Some(entry) = Self::entry_for(state, word) {
                return if entry.shape == F::SHAPE { Ok(()) } else { Err(CallbackError::Shape) };
            }
            Self::insert(state, word, word.0, F::SHAPE).map(|_| ())
        })
    }
    fn insert(state: &mut State, word: CallbackWord, address: usize, shape: Shape) -> Result<CallbackToken, CallbackError> {
        let slot = state.entries.iter().position(Option::is_none).ok_or(CallbackError::Full)?;
        if state.next >= 0x8000_0000 { return Err(CallbackError::Full); }
        let token = CallbackToken(state.next);
        state.next += 1;
        state.entries[slot] = Some(Entry { token, word, address, shape });
        Ok(token)
    }
    pub fn release(&self, token: CallbackToken) -> bool {
        self.state.with::<C, _>(|state| {
            let slot = state.entries.iter().position(|entry| entry.is_some_and(|entry| entry.token == token));
            slot.is_some_and(|slot| state.entries[slot].take().is_some())
        })
    }
    pub fn resolve<F: CallbackFn>(&self, word: CallbackWord) -> Result<F, CallbackError> {
        if word.0 == 0 || word.0 & 3 != 0 { return Err(CallbackError::Invalid); }
        self.state.with::<C, _>(|state| {
            if let Some(entry) = Self::entry_for(state, word) {
                return if entry.shape == F::SHAPE {
                    Ok(unsafe { F::from_address(entry.address) })
                } else { Err(CallbackError::Shape) };
            }
            if ADMITTED.iter().any(|region| region.contains(word.0)) {
                return Ok(unsafe { F::from_address(word.0) });
            }
            Err(CallbackError::Unadmitted)
        })
    }
}
impl<C: CriticalSection> Default for CallbackRegistry<C> {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
#[path = "callback_core_tests.rs"]
mod tests;
