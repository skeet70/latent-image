use crate::id::{own_pool_slot, resolve_pool, EncodedId, EntryDomain, PoolId};
use crate::layout::Domain;
use crate::status::PoolStatus;
use crate::timeout::Timeout;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Service {
    Create,
    Delete,
    Get,
    Poll,
    TimedGet,
    Release,
    /// The `+0x170` extension: no context, mask, dispatch or existence check.
    PollUnchecked,
}

impl Service {
    #[inline(always)]
    fn blocks(self) -> bool {
        matches!(self, Self::Get | Self::TimedGet)
    }

    #[inline(always)]
    fn owns_its_table(self) -> bool {
        matches!(self, Self::Create | Self::Delete)
    }

    #[inline(always)]
    fn checks_context(self) -> bool {
        !self.owns_its_table() && self != Self::PollUnchecked
    }

    #[inline(always)]
    fn checks_exists(self) -> bool {
        self.checks_context()
    }
}

/// The words a body reads before its first lock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub handler_context: u32,
    pub irq_masked: bool,
    pub dispatch_enabled: u32,
}

/// The selected context record's published pool counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Counts {
    pub a: u32,
    pub b: u32,
}

impl Counts {
    #[inline(always)]
    pub const fn of(self, domain: Domain) -> u32 {
        match domain {
            Domain::A => self.a,
            Domain::B => self.b,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Request {
    pub service: Service,
    pub entry: Option<Entry>,
    pub entry_domain: EntryDomain,
    pub id: EncodedId,
    pub counts: Counts,
    pub timeout: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Admitted {
    pub pool: PoolId,
    pub timeout: Option<Timeout>,
}

/// The stock order: context, IRQ mask, dispatch word, class, index, timeout, then exists.
#[inline(never)]
#[link_section = ".text.fp_body_admit"]
pub fn admit(
    request: &Request,
    exists: impl FnOnce(PoolId) -> bool,
) -> Result<Admitted, PoolStatus> {
    if request.service.owns_its_table() {
        let slot = own_pool_slot(request.id, request.counts.of(request.entry_domain.domain()))?;
        return Ok(Admitted {
            pool: PoolId {
                domain: request.entry_domain.domain(),
                slot,
            },
            timeout: None,
        });
    }
    if request.service.checks_context() {
        let entry = request.entry.ok_or(PoolStatus::Context)?;
        if entry.handler_context != 0 || entry.irq_masked {
            return Err(PoolStatus::Context);
        }
        if request.service.blocks() && entry.dispatch_enabled == 0 {
            return Err(PoolStatus::Context);
        }
    }
    let domain = match request.id.class() {
        0 => request.entry_domain.domain(),
        1 => Domain::A,
        2 => Domain::B,
        _ => return Err(PoolStatus::InvalidClass),
    };
    let pool = resolve_pool(
        request.entry_domain,
        request.id,
        request.counts.of(domain),
    )?;
    let timeout = if request.service == Service::TimedGet {
        let timeout = Timeout::from_stock(request.timeout)?;
        Some(timeout)
    } else {
        None
    };
    if request.service.checks_exists() && !exists(pool) {
        return Err(PoolStatus::NotCreated);
    }
    Ok(Admitted { pool, timeout })
}
