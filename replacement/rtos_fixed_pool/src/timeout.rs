use crate::status::PoolStatus;

/// A wait timeout in kernel clock units, 10 per tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timeout {
    Forever,
    Poll,
    Units(u32),
}

impl Timeout {
#[inline(always)]
    pub const fn from_stock(value: i32) -> Result<Self, PoolStatus> {
        if value == -1 {
            Ok(Self::Forever)
        } else if value == 0 {
            Ok(Self::Poll)
        } else if value > 0 {
            Ok(Self::Units(value as u32))
        } else {
            Err(PoolStatus::Param)
        }
    }

    #[inline(always)]
    pub const fn raw(self) -> i32 {
        match self {
            Self::Forever => -1,
            Self::Poll => 0,
            Self::Units(units) => units as i32,
        }
    }
}
