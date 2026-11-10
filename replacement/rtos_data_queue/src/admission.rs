use crate::id::{resolve_data_queue, DataQueueId, EncodedId, EntryDomain};
use crate::layout::Domain;
use crate::status::ObjectStatus;
use crate::timeout::Timeout;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Service {
    Send,
    SendPoll,
    SendPollHandler,
    SendTimeout,
    ForceSend,
    ForceSendHandler,
    Receive,
    ReceivePoll,
    ReceiveTimeout,
}

/// The words a body reads before its first lock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub handler_context: u32,
    pub irq_masked: bool,
    pub dispatch_enabled: u32,
}

/// The selected context record's published table count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Records {
    pub data_queues: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Facts {
    pub exists: bool,
    pub attributes: u32,
    pub capacity: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Params {
    pub timeout: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Admitted {
    pub object: DataQueueId,
    pub timeout: Timeout,
    pub handler: bool,
}

pub struct Prechecked {
    service: Service,
    admitted: Admitted,
}

impl Prechecked {
    pub fn object(&self) -> DataQueueId {
        self.admitted.object
    }

    pub fn finish(self, facts: Facts) -> Result<Admitted, ObjectStatus> {
        exists(facts)?;
        if matches!(self.service, Service::ForceSend | Service::ForceSendHandler) && facts.capacity == 0 {
            return Err(ObjectStatus::Capacity);
        }
        Ok(self.admitted)
    }
}

pub struct Request {
    pub service: Service,
    pub entry_domain: Domain,
    pub entry: Entry,
    pub id: EncodedId,
    pub records: Records,
    pub facts: Facts,
    pub params: Params,
}

fn context(entry: Entry, handler: bool) -> Result<(), ObjectStatus> {
    if (entry.handler_context != 0) != handler {
        return Err(ObjectStatus::Context);
    }
    Ok(())
}

fn task_blocking(entry: Entry) -> Result<(), ObjectStatus> {
    if entry.handler_context != 0 || entry.irq_masked || entry.dispatch_enabled == 0 {
        return Err(ObjectStatus::Context);
    }
    Ok(())
}

fn task_not_masked(entry: Entry) -> Result<(), ObjectStatus> {
    if entry.handler_context != 0 || entry.irq_masked {
        return Err(ObjectStatus::Context);
    }
    Ok(())
}

fn handler_not_masked(entry: Entry) -> Result<(), ObjectStatus> {
    if entry.handler_context == 0 || entry.irq_masked {
        return Err(ObjectStatus::Context);
    }
    Ok(())
}

fn exists(facts: Facts) -> Result<(), ObjectStatus> {
    if facts.exists {
        Ok(())
    } else {
        Err(ObjectStatus::NotCreated)
    }
}

pub fn context_check(service: Service, entry: Entry) -> Result<(), ObjectStatus> {
    match service {
        Service::Send => task_blocking(entry),
        Service::SendPoll => context(entry, false),
        Service::SendPollHandler => context(entry, true),
        Service::SendTimeout | Service::Receive | Service::ReceiveTimeout => task_blocking(entry),
        Service::ForceSend => task_not_masked(entry),
        Service::ForceSendHandler => handler_not_masked(entry),
        Service::ReceivePoll => task_not_masked(entry),
    }
}

/// The stock order before the creation record: context, class, index, timeout.
pub fn admit_precheck(
    service: Service,
    entry_domain: Domain,
    entry: Entry,
    id: EncodedId,
    records: Records,
    params: Params,
) -> Result<Prechecked, ObjectStatus> {
    let handler = matches!(service, Service::SendPollHandler | Service::ForceSendHandler);
    context_check(service, entry)?;
    let object = resolve_data_queue(EntryDomain::new(entry_domain), id, records.data_queues)?;
    if service == Service::ForceSendHandler
        && (id.0 <= 0 || id.0 as u32 > records.data_queues)
    {
        return Err(ObjectStatus::InvalidIndex);
    }
    let timeout = match service {
        Service::SendTimeout | Service::ReceiveTimeout => {
            Timeout::from_stock(params.timeout.ok_or(ObjectStatus::Param)?)?
        }
        _ => Timeout::Forever,
    };
    Ok(Prechecked { service, admitted: Admitted { object, timeout, handler } })
}

/// The stock order: context, class, index, timeout, existence, capacity.
pub fn admit(request: &Request) -> Result<Admitted, ObjectStatus> {
    admit_precheck(
        request.service,
        request.entry_domain,
        request.entry,
        request.id,
        request.records,
        request.params,
    )?.finish(request.facts)
}

