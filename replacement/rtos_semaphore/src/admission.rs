use crate::id::{own_slot, resolve, EncodedId, EntryDomain, SemaphoreId};
use crate::layout::Domain;
use crate::status::SemaphoreStatus;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Service {
    Create,
    Delete,
    Signal,
    InterruptSignal,
    Wait,
    Poll,
    TimedWait,
}

impl Service {
    #[inline(always)]
    pub const fn in_handler(self) -> bool {
        matches!(self, Self::InterruptSignal)
    }

    #[inline(always)]
    pub const fn blocks(self) -> bool {
        matches!(self, Self::Wait | Self::TimedWait)
    }

    #[inline(always)]
    pub const fn owns_its_table(self) -> bool {
        matches!(self, Self::Create | Self::Delete)
    }
}

/// The words a body reads before its first lock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub handler_context: u32,
    pub irq_masked: bool,
    pub dispatch_enabled: u32,
}

pub struct Request {
    pub service: Service,
    pub entry: Option<Entry>,
    pub entry_domain: EntryDomain,
    pub id: EncodedId,
}

/// The stock order: context, IRQ mask, dispatch word, class, index. The creation record's exists
/// word is read after the index resolves.
#[inline(always)]
pub fn admit(
    request: &Request,
    count: impl FnOnce(Domain) -> u32,
) -> Result<SemaphoreId, SemaphoreStatus> {
    if request.service.owns_its_table() {
        return Ok(SemaphoreId {
            domain: request.entry_domain.domain(),
            slot: own_slot(request.id)?,
        });
    }
    let entry = request.entry.ok_or(SemaphoreStatus::Context)?;
    if (entry.handler_context != 0) != request.service.in_handler() {
        return Err(SemaphoreStatus::Context);
    }
    if entry.irq_masked {
        return Err(SemaphoreStatus::Context);
    }
    if request.service.blocks() && entry.dispatch_enabled == 0 {
        return Err(SemaphoreStatus::Context);
    }
    let domain = crate::id::target(request.entry_domain, request.id.class())?;
    resolve(request.entry_domain, request.id, count(domain))
}

#[inline(always)]
pub fn require_created(exists: u32) -> Result<(), SemaphoreStatus> {
    if exists == 0 {
        Err(SemaphoreStatus::NotCreated)
    } else {
        Ok(())
    }
}
