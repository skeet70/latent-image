#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolStatus {
    /// Handler context, CPSR.I set, or dispatch disabled.
    Context,
    /// The class half is not 0, 1 or 2.
    InvalidClass,
    /// The index is outside the selected table.
    InvalidIndex,
    /// The creation record's exists word is nonzero.
    AlreadyExists,
    /// The creation record's exists word is zero.
    NotCreated,
    /// A timeout, block address, or block ownership is invalid.
    Param,
    /// The pool has no free block.
    Exhausted,
}

impl PoolStatus {
    #[inline(always)]
    pub const fn code(self) -> i32 {
        match self {
            Self::Context => -25,
            Self::InvalidClass | Self::InvalidIndex => -18,
            Self::AlreadyExists => -41,
            Self::NotCreated => -42,
            Self::Param => -17,
            Self::Exhausted => -50,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome<T> {
    Satisfied(T),
    /// A forced release or delete.
    Released,
    /// A poll found nothing, or the timer expired.
    TimedOut,
}

impl<T> WaitOutcome<T> {
    #[inline(always)]
    pub fn code(&self) -> i32 {
        match self {
            Self::Satisfied(_) => 0,
            Self::Released => -49,
            Self::TimedOut => -50,
        }
    }
}
