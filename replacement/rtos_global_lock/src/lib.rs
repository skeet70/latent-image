//! Owned global-lock root extensions `+0x161c`, `+0x1620` and `+0x1624` of the X-M1
//! 1.50 kernels (#156): single-try acquire, unconditional release and plain observe
//! over a caller-selected word, default `0x400c0804`. None reads CPSR or allocates.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

pub use layout::GuestAddress;

/// The value `+0x161c` stores: the acquirer's identity, taken from the caller's `r1`.
/// The retained spellings are the IRQ vector's `(core + 1) | 0x10` and the image4
/// critical pair's `(core + 1) | 0x20`; the body checks neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct LockStamp(pub u32);


/// The word an entry operates on. The stock entry takes `r0` and substitutes
/// [`Self::DEFAULT_GLOBAL`] when it is zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct LockWord(GuestAddress);

impl LockWord {
    /// The cross-core word both stock IRQ vectors and the image4 critical pair hold.
    pub const DEFAULT_GLOBAL: Self = Self(GuestAddress(0x400c_0804));

    /// `r0`: zero selects the default word; any other value is the caller's word.
    pub const fn from_argument(argument: u32) -> Self {
        if argument == 0 {
            Self::DEFAULT_GLOBAL
        } else {
            Self(GuestAddress(argument))
        }
    }

    pub const fn address(self) -> GuestAddress {
        self.0
    }
}

/// What `+0x1624` returns: the word as read, without interpretation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct LockValue(pub u32);

impl LockValue {
    pub const FREE: Self = Self(0);

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn is_free(self) -> bool {
        self.0 == 0
    }
}

/// `+0x161c` returns 0 after a committed exclusive store and -1 at once on a held word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum TryResult {
    Acquired = 0,
    Held = -1,
}

impl TryResult {
    pub const fn status(self) -> i32 {
        self as i32
    }
}

/// `+0x1620` returns 0 after a committed exclusive store and -1 on an already free word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum ReleaseResult {
    Released = 0,
    AlreadyFree = -1,
}

impl ReleaseResult {
    pub const fn status(self) -> i32 {
        self as i32
    }
}

/// The guest operations the three services use: the target maps the exclusive pair and
/// the barrier to A32 instructions, the host model to a local monitor.
pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    /// `ldrex`: a word load that opens the local monitor on `at`.
    fn exclusive_load(&self, at: GuestAddress) -> u32;
    /// `strex`: 0 when the store committed, nonzero when the monitor was lost.
    fn exclusive_store(&self, at: GuestAddress, value: u32) -> u32;
    /// The data memory barrier the committing paths execute after their store.
    fn data_memory_barrier(&self);
}

/// `+0x161c`. A word that reads nonzero is refused without a store; a lost exclusive
/// store repeats the pair. The stamp is stored unchanged.
pub fn try_acquire<M: Machine>(m: &M, word: LockWord, stamp: LockStamp) -> TryResult {
    loop {
        if m.exclusive_load(word.address()) != 0 {
            return TryResult::Held;
        }
        if m.exclusive_store(word.address(), stamp.0) == 0 {
            m.data_memory_barrier();
            return TryResult::Acquired;
        }
    }
}

/// `+0x1620`. Any nonzero word is cleared, whoever stored it; a word that reads zero is
/// refused without a store. A lost exclusive store repeats the pair.
pub fn release<M: Machine>(m: &M, word: LockWord) -> ReleaseResult {
    loop {
        if m.exclusive_load(word.address()) == 0 {
            return ReleaseResult::AlreadyFree;
        }
        if m.exclusive_store(word.address(), 0) == 0 {
            m.data_memory_barrier();
            return ReleaseResult::Released;
        }
    }
}

/// `+0x1624`. One plain load, no exclusive access and no barrier.
pub fn observe<M: Machine>(m: &M, word: LockWord) -> LockValue {
    LockValue(m.load_word(word.address()))
}

#[cfg(test)]
mod tests;

#[cfg(all(target_arch = "arm", not(test), not(feature = "stage")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
