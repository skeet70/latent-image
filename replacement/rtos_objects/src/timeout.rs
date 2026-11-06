use crate::status::ObjectStatus;

/// A wait timeout in kernel clock units, 10 per tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timeout {
    Forever,
    Poll,
    Units(u32),
}

impl Timeout {
    pub const fn from_stock(value: i32) -> Result<Self, ObjectStatus> {
        if value == -1 {
            Ok(Self::Forever)
        } else if value == 0 {
            Ok(Self::Poll)
        } else if value > 0 {
            Ok(Self::Units(value as u32))
        } else {
            Err(ObjectStatus::Param)
        }
    }

    pub const fn deadline(self, now: u32) -> Option<u32> {
        match self {
            Self::Units(units) => Some(now.wrapping_add(units)),
            Self::Forever | Self::Poll => None,
        }
    }
}
