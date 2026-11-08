use crate::layout::{DataQueueSlot, Domain};
use crate::status::ObjectStatus;

/// The raw r0 word a stock slot receives; only the entry decodes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EncodedId(pub i32);

impl EncodedId {
    pub const fn of(class: u16, index: u16) -> Self {
        Self(((class as i32) << 16) | index as i32)
    }

    pub const fn class(self) -> i32 {
        self.0 >> 16
    }

    pub const fn index(self) -> i32 {
        (self.0 << 16) >> 16
    }
}

/// The loader whose slot body runs; a slot of loader A runs on either core.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EntryDomain(Domain);

impl EntryDomain {
    pub const fn new(domain: Domain) -> Self {
        Self(domain)
    }

    pub const fn domain(self) -> Domain {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DataQueueId {
    pub domain: Domain,
    pub slot: DataQueueSlot,
}

fn target(entry: EntryDomain, class: i32) -> Result<Domain, ObjectStatus> {
    match class {
        0 => Ok(entry.domain()),
        1 => Ok(Domain::A),
        2 => Ok(Domain::B),
        _ => Err(ObjectStatus::InvalidClass),
    }
}

pub fn resolve_data_queue(
    entry: EntryDomain,
    id: EncodedId,
    count: u32,
) -> Result<DataQueueId, ObjectStatus> {
    let domain = target(entry, id.class())?;
    let slot = DataQueueSlot::new(index(id)?, count).ok_or(ObjectStatus::InvalidIndex)?;
    Ok(DataQueueId { domain, slot })
}

/// Create and delete ignore the class half.
pub fn own_data_queue_slot(id: EncodedId, count: u32) -> Result<DataQueueSlot, ObjectStatus> {
    DataQueueSlot::new(uxth(id), count).ok_or(ObjectStatus::InvalidIndex)
}

fn index(id: EncodedId) -> Result<u32, ObjectStatus> {
    let index = id.index();
    if index <= 0 {
        return Err(ObjectStatus::InvalidIndex);
    }
    Ok(index as u32)
}

fn uxth(id: EncodedId) -> u32 {
    (id.0 as u32) & 0xffff
}
