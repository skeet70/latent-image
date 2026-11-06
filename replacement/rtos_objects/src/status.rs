#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectStatus {
    /// Handler or task context mismatch, CPSR.I set, or dispatch disabled.
    Context,
    /// The class half is not 0, 1 or 2.
    InvalidClass,
    /// The index is outside the table.
    InvalidIndex,
    /// The creation record's exists word is zero.
    NotCreated,
    /// A family parameter is out of range.
    Param,
    /// The flag already has a waiter.
    QueueOverflow,
}

impl ObjectStatus {
    pub const fn code(self) -> i32 {
        match self {
            Self::Context => -25,
            Self::InvalidClass | Self::InvalidIndex => -18,
            Self::NotCreated => -42,
            Self::Param => -17,
            Self::QueueOverflow => -28,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome<T> {
    Satisfied(T),
    /// Another task released the wait or terminated the waiter.
    Released,
    /// The object was deleted.
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
