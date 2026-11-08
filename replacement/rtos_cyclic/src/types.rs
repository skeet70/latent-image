/// The I/F bits the stock mask pair returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedInterrupts(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CyclicAttributes(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CyclicArgument(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CyclicHandler(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CyclicPhase(pub u32);

/// A request or creation-record period word before the zero check.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct PeriodWord(pub u32);

impl PeriodWord {
    #[inline(always)]
    pub const fn checked(self) -> Option<CyclicPeriod> {
        if self.0 == 0 {
            None
        } else {
            Some(CyclicPeriod(self.0))
        }
    }
}

/// A nonzero period in milliseconds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CyclicPeriod(u32);

impl CyclicPeriod {
    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Expiry(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CyclicCreate {
    pub attributes: CyclicAttributes,
    pub argument: CyclicArgument,
    pub handler: CyclicHandler,
    pub period: PeriodWord,
    pub phase: CyclicPhase,
}
