use crate::status::ObjectStatus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timeout {
    Forever,
    Poll,
    Units(u32),
}

impl Timeout {
    /// The stock `r2` word: below `-1` is refused, `-1` waits forever, `0` polls.
    pub const fn from_stock(raw: i32) -> Result<Self, ObjectStatus> {
        if raw < -1 {
            Err(ObjectStatus::Param)
        } else if raw == -1 {
            Ok(Self::Forever)
        } else if raw == 0 {
            Ok(Self::Poll)
        } else {
            Ok(Self::Units(raw as u32))
        }
    }
}
