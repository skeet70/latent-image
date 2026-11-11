//! X-M1 enhanced interrupt snapshot at 0x0129ad50 and dispatch at 0x0129afdc.
use super::{ReceiveError, ReceivePlan, ReceivePort};
use super::super::super::sd_controller::{Argument, Buffer, Execution, Operation, Request};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptPolicy { pub mailbox_enabled: bool, pub receive_suppressed: bool, pub bus_removed: bool, pub service_stopped: bool }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptAction { ReadAbnormalStatus, ReturnedCredits([u8; 8]), ReceiveQueues }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptStatus(u32);
impl InterruptStatus { pub const fn wire(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotState { Prepared, Issued, Ready, Failed }
pub struct SnapshotRead { bytes: [u8; 88], state: SnapshotState, remaining: u32, policy: InterruptPolicy, next: u8 }
impl SnapshotRead {
    pub const fn new(policy: InterruptPolicy) -> Self {
        Self { bytes: [0; 88], state: SnapshotState::Prepared, remaining: 0, policy, next: 0 }
    }
    pub fn request(&mut self) -> Result<Request<'_>, ReceiveError> {
        if self.state != SnapshotState::Prepared { return Err(ReceiveError::Stale); }
        self.state = SnapshotState::Issued;
        Ok(Request { operation: Operation::Extended53, argument: Argument::from_wire(0x1000_2058), execution: Execution::Polling, buffer: Buffer::Fifo(&mut self.bytes) })
    }
    pub fn finish(&mut self, physical: Result<(), ReceiveError>) -> Result<(), ReceiveError> {
        if self.state != SnapshotState::Issued { return Err(ReceiveError::Stale); }
        if let Err(error) = physical { self.state = SnapshotState::Failed; return Err(error); }
        self.state = SnapshotState::Ready;
        let mut status = u32::from_le_bytes(self.bytes[..4].try_into().unwrap());
        if self.policy.bus_removed || self.policy.service_stopped { status = 0; }
        else {
            if status & 1 == 0 && self.bytes[4..12].iter().any(|byte| *byte != 0) { status |= 1; }
            if status & 0x8000_0000 == 0 && self.policy.mailbox_enabled && self.bytes[80..88].iter().any(|byte| *byte != 0) { status |= 0x8000_0000; }
            if status & 0xe0 != 0 { status &= !0xf0; }
        }
        self.remaining = status;
        Ok(())
    }
    pub fn next_action(&mut self) -> Option<InterruptAction> {
        if self.state != SnapshotState::Ready { return None; }
        while self.next < 4 {
            let index = self.next;
            self.next += 1;
            let mask = match index { 0 => 8, 1 => 0xffff_ff00, 2 => 1, _ => 6 };
            let active = self.remaining & mask != 0;
            self.remaining &= !mask;
            if !active { continue; }
            match index {
                0 => return Some(InterruptAction::ReadAbnormalStatus),
                1 => continue,
                2 => {
                    let returned = self.bytes[4..12].try_into().unwrap();
                    self.bytes[4..12].fill(0);
                    return Some(InterruptAction::ReturnedCredits(returned));
                }
                _ if !self.policy.receive_suppressed => return Some(InterruptAction::ReceiveQueues),
                _ => continue,
            }
        }
        None
    }
    pub fn plan(&self, port: ReceivePort, available_packets: usize) -> Result<ReceivePlan, ReceiveError> {
        if self.state != SnapshotState::Ready { return Err(ReceiveError::Stale); }
        ReceivePlan::from_interrupt_snapshot(&self.bytes, port, available_packets)
    }
    pub fn finish_port(&mut self, port: ReceivePort) -> Result<(), ReceiveError> {
        if self.state != SnapshotState::Ready { return Err(ReceiveError::Stale); }
        let offset = match port { ReceivePort::Zero => 12, ReceivePort::One => 14 };
        self.bytes[offset..offset + 2].fill(0);
        Ok(())
    }
    pub const fn remaining(&self) -> InterruptStatus { InterruptStatus(self.remaining) }
    pub fn cancel(&mut self) { self.bytes.fill(0); self.state = SnapshotState::Failed; self.remaining = 0; }
    pub fn disable_request() -> Request<'static> {
        Request { operation: Operation::Direct52, argument: Argument::from_wire(0x9000_0802), execution: Execution::Polling, buffer: Buffer::None }
    }
    pub fn abnormal_status_request(buffer: &mut [u8; 4]) -> Request<'_> {
        Request { operation: Operation::Extended53, argument: Argument::from_wire(0x1000_3004), execution: Execution::Polling, buffer: Buffer::Fifo(buffer) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterruptEnable { Enabled, Disabled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepRequest { Absent, Pending }
pub struct InterruptControl { enable: InterruptEnable, sleep: SleepRequest, register: [u8; 4] }
impl InterruptControl {
    pub const fn new(enable: InterruptEnable) -> Self { Self { enable, sleep: SleepRequest::Absent, register: [0; 4] } }
    pub fn request_sleep(&mut self) { self.sleep = SleepRequest::Pending; }
    pub fn disable(&mut self) -> Request<'static> { self.enable = InterruptEnable::Disabled; SnapshotRead::disable_request() }
    pub fn enable(&mut self) -> Option<Request<'_>> {
        let previous = self.enable;
        self.enable = InterruptEnable::Enabled;
        match self.sleep {
            SleepRequest::Pending => {
                self.sleep = SleepRequest::Absent;
                let value = match previous { InterruptEnable::Enabled => 0x100u32, InterruptEnable::Disabled => 0x101 };
                self.register = value.to_le_bytes();
                Some(Request { operation: Operation::Extended53, argument: Argument::from_wire(0x9000_0804), execution: Execution::Polling, buffer: Buffer::Fifo(&mut self.register) })
            }
            SleepRequest::Absent if previous == InterruptEnable::Disabled => Some(Request { operation: Operation::Direct52, argument: Argument::from_wire(0x9000_0801), execution: Execution::Polling, buffer: Buffer::None }),
            SleepRequest::Absent => None,
        }
    }
    pub const fn firmware_ownership(&self) -> super::gate::FirmwareOwnership {
        if u32::from_le_bytes(self.register) & 0x100 != 0 { super::gate::FirmwareOwnership::Firmware } else { super::gate::FirmwareOwnership::Driver }
    }
}
