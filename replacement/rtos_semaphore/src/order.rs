use crate::layout::SemaphoreConfig;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueOrder {
    Fifo,
    Priority,
}

impl QueueOrder {
    #[inline(always)]
    pub const fn from_attribute(attributes: u32) -> Self {
        if attributes & SemaphoreConfig::PRIORITY_ORDER != 0 {
            Self::Priority
        } else {
            Self::Fifo
        }
    }
}
