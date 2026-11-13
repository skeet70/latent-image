//! The stock boundary: X-M1 1.50 addresses of the ISP units' registers and session records, the
//! driver's event log entries, the stock words for the unit, fault and refusal codes, and in
//! `capture` the capture layer's stock tables, state and services.

use core::ptr::{read_volatile, write_volatile};

use crate::driver::{Bank, Client, Event, Exclusion, Fault, Isp, Operation, Record, Refusal, Register, SessionId, Unit};

macro_rules! entry {
    ($address:expr, fn($($argument:ty),*) $(-> $result:ty)?) => {{
        let entry: extern "C" fn($($argument),*) $(-> $result)? = unsafe { core::mem::transmute($address as usize as *const ()) };
        entry
    }};
}

fn read_u32(address: u32) -> u32 { unsafe { read_volatile(address as usize as *const u32) } }

fn write_u32(address: u32, value: u32) { unsafe { write_volatile(address as usize as *mut u32, value) } }

pub mod capture;

/// `hw_log_event` through its one-client form (client, unit, operation, fault).
const LOG_REFUSED: u32 = 0x68_4908;
/// `hw_log_event` through its two-client form (holder, requester, unit, operation, fault).
const LOG_BUSY: u32 = 0x68_4948;

const FREE: u32 = 0xffff;

const fn register_address(register: Register) -> u32 {
    match register {
        Register::Auto(offset) => 0xfff1_8000 + offset as u32,
        Register::Raid0(offset) => 0xfff2_8000 + offset as u32,
        Register::Mvrec(Bank::First, offset) => 0xfff1_2000 + offset as u32,
        Register::Mvrec(Bank::Second, offset) => 0xfff1_3000 + offset as u32,
    }
}

/// Holder, last operation, active session (0xffff when free), next session.
const fn record_address(unit: Unit) -> u32 {
    match unit {
        Unit::Auto => 0x82_0f40,
        Unit::Raid0 => 0x82_0f88,
        Unit::Mvrec => 0x82_0f30,
    }
}

/// The log's unit index (0x820f68: AUTO, CCDIF, RAID, RAID0, MVREC).
pub const fn unit_code(unit: Unit) -> u32 {
    match unit {
        Unit::Auto => 0,
        Unit::Raid0 => 3,
        Unit::Mvrec => 4,
    }
}

/// The log's fault index (0x820f7c).
pub const fn fault_code(fault: Fault) -> u32 {
    match fault {
        Fault::RepeatedCall => 0,
        Fault::Parameter => 1,
        Fault::ResourceId => 2,
    }
}

/// The status a stock unit entry returns for the refusal.
pub const fn refusal_status(refusal: Refusal) -> i32 {
    match refusal {
        Refusal::Parameter => -30,
        Refusal::Busy | Refusal::WrongSession => -20,
    }
}

/// The session word a stock request carries after an acquire.
pub const fn session_word(session: Option<SessionId>) -> u32 {
    match session {
        Some(session) => session.0,
        None => FREE,
    }
}

pub const fn parse_session(word: u32) -> Option<SessionId> {
    if word == FREE { None } else { Some(SessionId(word)) }
}

/// The units' registers and records in place, with the composition root's critical section.
pub struct StockIsp<X> {
    pub exclusion: X,
}

impl<X: Exclusion + Copy> Isp for StockIsp<X> {
    fn read(&mut self, register: Register) -> u32 { read_u32(register_address(register)) }

    fn write(&mut self, register: Register, value: u32) { write_u32(register_address(register), value) }

    fn record(&mut self, unit: Unit) -> Record {
        let base = record_address(unit);
        Record {
            holder: Client(read_u32(base)),
            operation: Operation(read_u32(base + 4)),
            active: parse_session(read_u32(base + 8)),
            next: SessionId(read_u32(base + 12)),
        }
    }

    fn set_record(&mut self, unit: Unit, record: Record) {
        let base = record_address(unit);
        write_u32(base, record.holder.0);
        write_u32(base + 4, record.operation.code());
        write_u32(base + 8, session_word(record.active));
        write_u32(base + 12, record.next.0);
    }

    fn note(&mut self, unit: Unit, operation: Operation) { write_u32(record_address(unit) + 4, operation.code()) }

    fn log(&mut self, event: Event) {
        match event {
            Event::Refused { client, unit, operation, fault } => entry!(LOG_REFUSED, fn(u32, u32, u32, u32))(
                u32::from(client.log_id()),
                unit_code(unit),
                operation.code(),
                fault_code(fault),
            ),
            Event::Busy { holder, requester, unit, operation } => entry!(LOG_BUSY, fn(u32, u32, u32, u32, u32))(
                u32::from(holder.log_id()),
                u32::from(requester.log_id()),
                unit_code(unit),
                operation.code(),
                fault_code(Fault::RepeatedCall),
            ),
        }
    }

    fn exclusive<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        let exclusion = self.exclusion;
        exclusion.run(|| body(self))
    }
}
