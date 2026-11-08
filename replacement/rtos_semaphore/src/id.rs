use crate::layout::{Domain, SemaphoreSlot};
use crate::status::SemaphoreStatus;

/// The raw r0 word a stock slot receives; only the entry decodes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EncodedId(pub i32);

impl EncodedId {
    #[inline(always)]
    pub const fn class(self) -> i32 {
        self.0 >> 16
    }

    #[inline(always)]
    pub const fn index(self) -> i32 {
        (self.0 << 16) >> 16
    }

    #[inline(always)]
    pub const fn short(self) -> u32 {
        (self.0 as u32) & 0xffff
    }
}

/// The loader whose slot body runs; a slot of loader A runs on either core.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EntryDomain(Domain);

impl EntryDomain {
    #[inline(always)]
    pub const fn new(domain: Domain) -> Self {
        Self(domain)
    }

    #[inline(always)]
    pub const fn domain(self) -> Domain {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemaphoreId {
    pub domain: Domain,
    pub slot: SemaphoreSlot,
}

#[inline(always)]
pub fn target(entry: EntryDomain, class: i32) -> Result<Domain, SemaphoreStatus> {
    match class {
        0 => Ok(entry.domain()),
        1 => Ok(Domain::A),
        2 => Ok(Domain::B),
        _ => Err(SemaphoreStatus::InvalidId),
    }
}

#[inline(always)]
pub fn resolve(
    entry: EntryDomain,
    id: EncodedId,
    count: u32,
) -> Result<SemaphoreId, SemaphoreStatus> {
    let domain = target(entry, id.class())?;
    let index = id.index();
    if index <= 0 {
        return Err(SemaphoreStatus::InvalidId);
    }
    let slot = SemaphoreSlot::new(index as u32, count).ok_or(SemaphoreStatus::InvalidId)?;
    Ok(SemaphoreId { domain, slot })
}

/// Create and delete ignore the class half; stock indexes `uxth(id)` without a range check.
#[inline(always)]
pub fn own_slot(id: EncodedId) -> Result<SemaphoreSlot, SemaphoreStatus> {
    SemaphoreSlot::new(id.short(), SemaphoreSlot::COUNT).ok_or(SemaphoreStatus::InvalidId)
}
