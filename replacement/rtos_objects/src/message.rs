use crate::status::ObjectStatus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MessagePriority(u32);

impl MessagePriority {
    pub const fn new(raw: u32, limit: u32) -> Result<Self, ObjectStatus> {
        if (raw as i32) <= 0 || (raw as i32) > (limit as i32) {
            return Err(ObjectStatus::Param);
        }
        Ok(Self(raw))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}
