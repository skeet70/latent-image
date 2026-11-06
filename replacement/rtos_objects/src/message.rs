use crate::status::ObjectStatus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MessagePriority(u32);

impl MessagePriority {
    pub const fn new(raw: u32, limit: PriorityLimit) -> Result<Self, ObjectStatus> {
        if (raw as i32) <= 0 || (raw as i32) > (limit.0 as i32) {
            return Err(ObjectStatus::Param);
        }
        Ok(Self(raw))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A creation record's priority limit; any bit pattern is valid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct PriorityLimit(pub u32);

impl PriorityLimit {
    pub const fn from_word(word: u32) -> Self {
        Self(word)
    }
}
