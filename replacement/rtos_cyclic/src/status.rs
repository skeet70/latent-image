#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CyclicStatus {
    Ok,
    /// Handler context nonzero or CPSR.I set.
    Context,
    InvalidClass,
    InvalidIndex,
    AlreadyExists,
    NotCreated,
    /// A zero period.
    Param,
    /// The owned timer heap refused the entry.
    Timer,
}

impl CyclicStatus {
    #[inline(always)]
    pub const fn code(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Context => -25,
            Self::InvalidClass | Self::InvalidIndex => -18,
            Self::AlreadyExists => -41,
            Self::NotCreated => -42,
            Self::Param => -17,
            Self::Timer => -5,
        }
    }

    #[inline(always)]
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}
