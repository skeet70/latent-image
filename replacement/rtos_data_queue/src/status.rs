#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectStatus {
    /// Handler or task context mismatch, CPSR.I set, or dispatch disabled.
    Context,
    /// The class half is not 0, 1 or 2.
    InvalidClass,
    InvalidIndex,
    /// The creation record's exists word is nonzero.
    AlreadyExists,
    /// The creation record's exists word is zero.
    NotCreated,
    Param,
    /// A poll or zero timeout found no buffered word, or the ring was full.
    QueueOverflow,
    /// Force send on a zero-capacity ring.
    Capacity,
}

impl ObjectStatus {
    #[inline(always)]
    pub const fn code(self) -> i32 {
        match self {
            Self::Context => -25,
            Self::InvalidClass | Self::InvalidIndex => -18,
            Self::AlreadyExists => -41,
            Self::NotCreated => -42,
            Self::Param => -17,
            Self::QueueOverflow => -50,
            Self::Capacity => -28,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome<T> {
    Satisfied(T),
    /// Another task released the wait or terminated the waiter.
    Released,
    Deleted,
    /// A poll found nothing, or the timer expired.
    TimedOut,
}

impl<T> WaitOutcome<T> {
    pub fn code(&self) -> i32 {
        match self {
            Self::Satisfied(_) => 0,
            Self::Released | Self::Deleted => -49,
            Self::TimedOut => -50,
        }
    }
}
