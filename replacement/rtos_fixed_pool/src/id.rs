use crate::layout::{Domain, FixedPoolSlot};
use crate::status::PoolStatus;

/// The raw r0 word a stock slot receives; only admission decodes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EncodedId(pub i32);

impl EncodedId {
    #[inline(always)]
    pub const fn of(class: u16, index: u16) -> Self {
        Self(((class as i32) << 16) | index as i32)
    }

    #[inline(always)]
    pub const fn class(self) -> i32 {
        self.0 >> 16
    }

    #[inline(always)]
    pub const fn index(self) -> i32 {
        (self.0 << 16) >> 16
    }
}

/// The loader whose slot body runs; a loader-A slot runs on either core.
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
pub struct PoolId {
    pub domain: Domain,
    pub slot: FixedPoolSlot,
}

fn target(entry: EntryDomain, class: i32) -> Result<Domain, PoolStatus> {
    match class {
        0 => Ok(entry.domain()),
        1 => Ok(Domain::A),
        2 => Ok(Domain::B),
        _ => Err(PoolStatus::InvalidClass),
    }
}

#[inline(always)]
pub fn resolve_pool(entry: EntryDomain, id: EncodedId, count: u32) -> Result<PoolId, PoolStatus> {
    let domain = target(entry, id.class())?;
    let slot = FixedPoolSlot::new(index(id)?, count).ok_or(PoolStatus::InvalidIndex)?;
    Ok(PoolId { domain, slot })
}

/// Create and delete ignore the class half: `uxth(id)` indexes the entry domain's own table.
pub fn own_pool_slot(id: EncodedId, count: u32) -> Result<FixedPoolSlot, PoolStatus> {
    FixedPoolSlot::new((id.0 as u32) & 0xffff, count).ok_or(PoolStatus::InvalidIndex)
}

fn index(id: EncodedId) -> Result<u32, PoolStatus> {
    let index = id.index();
    if index <= 0 {
        return Err(PoolStatus::InvalidIndex);
    }
    Ok(index as u32)
}
