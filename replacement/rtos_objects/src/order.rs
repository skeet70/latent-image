#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueOrder {
    Fifo,
    Priority,
}

impl QueueOrder {
    /// The creation record's attribute bit 0.
    pub const fn from_attribute(attributes: u32) -> Self {
        if attributes & 1 != 0 {
            Self::Priority
        } else {
            Self::Fifo
        }
    }
}

/// Priority order stops at the first strictly greater priority, so equal priorities keep arrival
/// order; FIFO always appends.
pub const fn precedes(order: QueueOrder, candidate: u32, existing: u32) -> bool {
    match order {
        QueueOrder::Fifo => false,
        QueueOrder::Priority => candidate < existing,
    }
}
