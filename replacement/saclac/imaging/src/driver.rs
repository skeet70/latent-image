#![forbid(unsafe_code)]

//! The image-4 ISP driver's shared machinery: the clients and units its event log names, each
//! unit's session record, and the refusals a unit logs.

/// A driver client: the request word its callers pass. The log names the low halfwords 0..=5.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Client(pub u32);

impl Client {
    pub const S1: Self = Self(0);
    pub const S2: Self = Self(1);
    pub const MV: Self = Self(2);
    pub const TG: Self = Self(3);
    pub const REC: Self = Self(4);
    pub const JIG: Self = Self(5);
    /// The holder a release leaves in the record.
    pub const NONE: Self = Self(6);

    /// The halfword the log receives.
    pub const fn log_id(self) -> u16 { self.0 as u16 }
}

/// The units the log names that saclac programs; CCDIF and RAID are not among them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    Auto,
    Raid0,
    Mvrec,
}

/// The log's fault kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    /// An acquire while another client holds the unit.
    RepeatedCall,
    Parameter,
    /// A release with a session the unit did not hand out last.
    ResourceId,
}

/// A unit operation's code, the number the log prints and the record keeps as the holder's last
/// operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Operation(pub(crate) u32);

impl Operation {
    pub const ACQUIRED: Self = Self(0);
    pub const RELEASING: Self = Self(1);
    pub const RELEASED: Self = Self(0x5b);

    pub const fn code(self) -> u32 { self.0 }
}

/// A session a unit hands out; the handles run 1..=0x1000 and wrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionId(pub u32);

impl SessionId {
    const LAST: u32 = 0x1000;

    pub const fn following(self) -> Self {
        if self.0 >= Self::LAST { Self(1) } else { Self(self.0 + 1) }
    }
}

/// A unit's session record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record {
    pub holder: Client,
    pub operation: Operation,
    pub active: Option<SessionId>,
    pub next: SessionId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// A setting outside the field it programs; logged as a parameter fault.
    Parameter,
    /// Another client holds the unit.
    Busy,
    /// The released session is not the unit's active one.
    WrongSession,
}

/// An entry of the driver's event log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Refused { client: Client, unit: Unit, operation: Operation, fault: Fault },
    /// The holder, the client refused and the holder's last operation.
    Busy { holder: Client, requester: Client, unit: Unit, operation: Operation },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bank { First, Second }

/// A unit register, as a byte offset into the unit's register block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    Auto(u16),
    Raid0(u16),
    Mvrec(Bank, u16),
}

/// The hardware and shared memory a unit operation reaches.
pub trait Isp {
    fn read(&mut self, register: Register) -> u32;
    fn write(&mut self, register: Register, value: u32);
    fn record(&mut self, unit: Unit) -> Record;
    fn set_record(&mut self, unit: Unit, record: Record);
    /// Stores the record's last-operation field alone.
    fn note(&mut self, unit: Unit, operation: Operation);
    fn log(&mut self, event: Event);
    /// Runs `body` with this core's interrupts masked and the cross-core lock held.
    fn exclusive<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R
    where
        Self: Sized;
}

/// The cross-core critical section `Isp::exclusive` runs in, bound at saclac's composition root.
pub trait Exclusion {
    fn run<R>(&self, body: impl FnOnce() -> R) -> R;
}

/// Hands `client` the unit's next session, or logs the holder and refuses while one is active.
pub fn acquire<I: Isp>(isp: &mut I, unit: Unit, client: Client) -> Result<SessionId, Refusal> {
    isp.exclusive(|isp| {
        let record = isp.record(unit);
        if record.active.is_some() {
            isp.log(Event::Busy { holder: record.holder, requester: client, unit, operation: record.operation });
            return Err(Refusal::Busy);
        }
        let session = record.next;
        isp.set_record(unit, Record { holder: client, operation: Operation::ACQUIRED, active: Some(session), next: session.following() });
        Ok(session)
    })
}

/// Frees the unit when `session` is its active session; a failed acquire's `None` frees an idle
/// unit.
pub fn release<I: Isp>(isp: &mut I, unit: Unit, client: Client, session: Option<SessionId>) -> Result<(), Refusal> {
    isp.note(unit, Operation::RELEASING);
    isp.exclusive(|isp| {
        let record = isp.record(unit);
        if record.active != session {
            isp.log(Event::Refused { client, unit, operation: Operation::RELEASED, fault: Fault::ResourceId });
            return Err(Refusal::WrongSession);
        }
        isp.set_record(unit, Record { holder: Client::NONE, operation: Operation::RELEASED, active: None, next: record.next });
        Ok(())
    })
}

/// Logs `operation`'s parameter fault for `client`.
pub(crate) fn refuse<I: Isp>(isp: &mut I, unit: Unit, client: Client, operation: Operation) -> Refusal {
    isp.log(Event::Refused { client, unit, operation, fault: Fault::Parameter });
    Refusal::Parameter
}

/// A stock argument word outside the closed set its operation takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterFault {
    pub(crate) unit: Unit,
    pub(crate) operation: Operation,
}

impl ParameterFault {
    pub const fn unit(self) -> Unit { self.unit }

    pub const fn operation(self) -> Operation { self.operation }
}

/// Refuses `fault` as its stock entry refuses a word it does not take. RAID0 and MVREC entries
/// take no session.
pub fn refuse_parameter<I: Isp>(isp: &mut I, client: Client, session: Option<SessionId>, fault: ParameterFault) -> Refusal {
    match fault.unit {
        Unit::Auto => check_session(isp, fault.unit, client, session, fault.operation),
        Unit::Raid0 | Unit::Mvrec => isp.note(fault.unit, fault.operation),
    }
    refuse(isp, fault.unit, client, fault.operation)
}

/// Notes `operation` in the record, then refuses unless every check holds.
pub(crate) fn admit<I: Isp>(isp: &mut I, unit: Unit, client: Client, operation: Operation, checks: &[bool]) -> Result<(), Refusal> {
    isp.note(unit, operation);
    if checks.iter().all(|check| *check) { Ok(()) } else { Err(refuse(isp, unit, client, operation)) }
}

/// Notes `operation` and logs a resource-id fault when `session` is not the unit's active one;
/// the operation proceeds either way.
pub(crate) fn check_session<I: Isp>(isp: &mut I, unit: Unit, client: Client, session: Option<SessionId>, operation: Operation) {
    isp.note(unit, operation);
    if isp.record(unit).active != session {
        isp.log(Event::Refused { client, unit, operation, fault: Fault::ResourceId });
    }
}
