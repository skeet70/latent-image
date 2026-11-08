/// A `+0x7c` timeout: 0 polls, positive arms the task timer, negative waits without one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timeout {
    Poll,
    Forever,
    Units(u32),
}

impl Timeout {
    #[inline(always)]
    pub const fn from_stock(raw: i32) -> Self {
        if raw == 0 {
            Self::Poll
        } else if raw < 0 {
            Self::Forever
        } else {
            Self::Units(raw as u32)
        }
    }

    #[inline(always)]
    pub const fn units(self) -> Option<u32> {
        match self {
            Self::Units(units) => Some(units),
            _ => None,
        }
    }

    #[inline(always)]
    pub const fn raw(self) -> i32 {
        match self {
            Self::Poll => 0,
            Self::Forever => -1,
            Self::Units(units) => units as i32,
        }
    }
}
