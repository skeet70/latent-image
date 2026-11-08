/// A control block's count word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Count(u32);

impl Count {
    #[inline(always)]
    pub const fn from_word(word: u32) -> Self {
        Self(word)
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Signal's comparison against the configured maximum.
    #[inline(always)]
    pub const fn below(self, maximum: Maximum) -> bool {
        self.0 < maximum.0
    }

    #[inline(always)]
    pub const fn increment(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    /// Wait and poll call this only when the count is nonzero.
    #[inline(always)]
    pub const fn decrement(self) -> Self {
        Self(self.0.wrapping_sub(1))
    }
}

/// A creation record's maximum word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Maximum(u32);
impl Maximum {
    #[inline(always)]
    pub const fn from_word(word: u32) -> Self {
        Self(word)
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A creation record's attribute word, arbitrary bits preserved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SemaphoreAttributes(pub u32);
