#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum SemaphoreStatus {
    /// Handler or task context mismatch, CPSR.I set, or dispatch disabled.
    Context = -25,
    /// The class half is not 0, 1 or 2, or the index is outside the table.
    InvalidId = -18,
    /// The creation record's exists word is nonzero.
    AlreadyExists = -41,
    /// The creation record's exists word is zero.
    NotCreated = -42,
    /// No waiter and the count already equals the maximum.
    Overflow = -43,
}

impl SemaphoreStatus {
    #[inline(always)]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum WaitOutcome {
    /// The count was taken, or the waiter was readied by a signal.
    Acquired = 0,
    /// A forced release or a delete ended the wait.
    Released = -49,
    /// A poll found no count, or the timer expired.
    TimedOut = -50,
}

impl WaitOutcome {
    #[inline(always)]
    pub const fn code(self) -> i32 {
        self as i32
    }
}
