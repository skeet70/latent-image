//! Polling query receive CPU at 0x012a47a0.

use super::sd_controller::{Argument, Buffer, Completion, Execution, Operation, PhysicalResult, Request, VendorCode};
use super::vendor_wireless_power::RadioTick;
use super::vendor_wireless_boot_query::{Query, QueryError, QueryReply};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryFailure { Timeout, Cancelled, Physical, Reply(QueryError), Oversized, WrongSequence }
#[derive(Clone, Copy)]
enum Phase { Send, Length, Delay(RadioTick), Read(usize), Finished }

pub struct QueryDriver {
    query: Query,
    sequence: u8,
    phase: Phase,
    started: Option<RadioTick>,
    issued: bool,
    outcome: Option<Result<QueryReply, QueryFailure>>,
    physical: Option<Completion>,
    correlate: bool,
}
impl QueryDriver {
    pub const fn new(query: Query, sequence: u8) -> Self {
        Self { query, sequence, phase: Phase::Send, started: None, issued: false, outcome: None, physical: None, correlate: false }
    }
    pub const fn correlated(query: Query, sequence: u8) -> Self { Self { correlate: true, ..Self::new(query, sequence) } }
    pub fn request<'a>(&mut self, now: RadioTick, register: &'a mut [u8; 4], packet: &'a mut [u8; 28]) -> Option<Request<'a>> {
        if self.issued { return None; }
        if let Phase::Delay(deadline) = self.phase {
            if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
            self.phase = Phase::Length;
        }
        let (argument, buffer) = match self.phase {
            Phase::Send => {
                let length = self.query.encode(self.sequence, packet);
                (0x9000_5800 | length as u32, Buffer::Fifo(&mut packet[..length]))
            }
            Phase::Length => (0x1000_a004, Buffer::Fifo(register)),
            Phase::Read(length) => (0x1000_6800 | length as u32, Buffer::Fifo(&mut packet[..length])),
            Phase::Finished | Phase::Delay(_) => return None,
        };
        self.issued = true;
        Some(Request { operation: Operation::Extended53, argument: Argument::from_wire(argument), execution: Execution::Polling, buffer })
    }
    pub fn receive(&mut self, physical: Completion, register: [u8; 4], packet: &[u8; 28], now: RadioTick, cancelled: bool) -> bool {
        if !self.issued { return false; }
        self.issued = false;
        self.physical = Some(physical);
        if cancelled { self.finish(Err(QueryFailure::Cancelled)); return true; }
        if physical.physical != PhysicalResult::Completed || physical.code != VendorCode::Success {
            self.finish(Err(QueryFailure::Physical)); return true;
        }
        match self.phase {
            Phase::Send => { self.started = Some(now); self.phase = Phase::Length; }
            Phase::Length => {
                let length = (u32::from_le_bytes(register) >> 16) as usize;
                if length != 0 {
                    if length > self.query.length() { self.finish(Err(QueryFailure::Oversized)); }
                    else { self.phase = Phase::Read(length); }
                } else if {
                    let started = self.started.unwrap().0;
                    let elapsed = if now.0 < started { (!started).wrapping_add(now.0) } else { now.0 - started };
                    elapsed > 1000
                } {
                    self.finish(Err(QueryFailure::Timeout));
                } else { self.phase = Phase::Delay(RadioTick(now.0.wrapping_add(1))); }
            }
            Phase::Read(length) => {
                if self.correlate && (length < 6 || packet[5] != self.sequence) { self.finish(Err(QueryFailure::WrongSequence)); }
                else { self.finish(self.query.parse(&packet[..length]).map_err(QueryFailure::Reply)); }
            }
            Phase::Finished | Phase::Delay(_) => return false,
        }
        true
    }
    fn finish(&mut self, outcome: Result<QueryReply, QueryFailure>) { self.outcome = Some(outcome); self.phase = Phase::Finished; }
    pub const fn outcome(&self) -> Option<Result<QueryReply, QueryFailure>> { self.outcome }
    pub const fn last_physical(&self) -> Option<Completion> { self.physical }
}

#[cfg(test)]
#[path = "vendor_wireless_boot_query_driver_tests.rs"]
mod tests;
