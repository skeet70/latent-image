//! Typed identities and status values of the stock root-slot ABI (manifest v5, E-RTOS-SLOT-MANIFEST-V5-150).

/// A kernel domain; class 1 is domain A (core 0), class 2 is domain B (core 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    A,
    B,
}

impl Domain {
    pub const fn class(self) -> u32 {
        match self {
            Domain::A => 1,
            Domain::B => 2,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Domain::A => 0,
            Domain::B => 1,
        }
    }

    /// The domain whose root `+0x1618` identity result is `identity` (0 on core 0).
    pub const fn from_identity(identity: u32) -> Domain {
        if identity == 0 {
            Domain::A
        } else {
            Domain::B
        }
    }
}

/// A task record index, 1..=120 in each domain's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskId(u16);

impl TaskId {
    pub const CAPACITY: u16 = 120;

    pub const fn new(id: u16) -> Option<TaskId> {
        if id >= 1 && id <= Self::CAPACITY {
            Some(TaskId(id))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    /// The byte stock code repeats over a task's stack and checks at its base.
    pub const fn fill_word(self) -> u32 {
        let byte = (self.0 & 0xff) as u32;
        byte | byte << 8 | byte << 16 | byte << 24
    }
}

/// `(class << 16) | id`, the form every class-checked slot decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodedId(u32);

impl EncodedId {
    pub const fn new(domain: Domain, id: TaskId) -> EncodedId {
        EncodedId(domain.class() << 16 | id.get() as u32)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotStatus {
    Ok,
    Parameter,
    Id,
    Context,
    IllegalUse,
    ObjectState,
    NotExisting,
    QueueOverflow,
    WaitReleased,
    Timeout,
    Other(i32),
}

impl SlotStatus {
    pub const fn from_raw(value: i32) -> SlotStatus {
        match value {
            0 => SlotStatus::Ok,
            -17 => SlotStatus::Parameter,
            -18 => SlotStatus::Id,
            -25 => SlotStatus::Context,
            -28 => SlotStatus::IllegalUse,
            -41 => SlotStatus::ObjectState,
            -42 => SlotStatus::NotExisting,
            -43 => SlotStatus::QueueOverflow,
            -49 => SlotStatus::WaitReleased,
            -50 => SlotStatus::Timeout,
            other => SlotStatus::Other(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_ids_outside_the_table_are_refused() {
        assert_eq!(TaskId::new(0), None);
        assert_eq!(TaskId::new(121), None);
        assert_eq!(TaskId::new(120).map(TaskId::get), Some(120));
    }

    #[test]
    fn encoded_ids_carry_the_class() {
        let id = TaskId::new(119).unwrap();
        assert_eq!(EncodedId::new(Domain::A, id).raw(), 0x0001_0077);
        assert_eq!(EncodedId::new(Domain::B, id).raw(), 0x0002_0077);
    }

    #[test]
    fn fill_word_repeats_the_id_byte() {
        assert_eq!(TaskId::new(119).unwrap().fill_word(), 0x7777_7777);
    }

    #[test]
    fn recovered_statuses_decode() {
        assert_eq!(SlotStatus::from_raw(-43), SlotStatus::QueueOverflow);
        assert_eq!(SlotStatus::from_raw(-50), SlotStatus::Timeout);
        assert_eq!(SlotStatus::from_raw(-7), SlotStatus::Other(-7));
    }
}
