//! Stock 0x00d7d154: the USB controller event pump over the pending word at 0xfff5402c/30/34.
//!
//! The pump subtracts the shadow mask at 0x01469768+0x18 from the three pending halfwords and
//! tests the distinguished bits in the order below. Stock handles the first set bit and returns;
//! this model reports every set bit in the same order so one pass drains the word. The
//! acknowledge halfwords 0xfff54054/58/5c carry a zero in every handled bit and always in bit 18
//! (0x00d7dc58).

use crate::transport::ControllerIo;
use crate::transport_start_cpu::{StartIo, UsbStart};

/// The pending word 0xfff5402c/30/34 with the shadow mask at 0x01469768+0x18 removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsbStatus(u64);

impl UsbStatus {
    pub const NONE: Self = Self(0);
    pub const fn new(pending: u64, mask: u64) -> Self { Self(pending & !mask & 0x0000_ffff_ffff_ffff) }
    pub const fn contains(self, event: UsbEvent) -> bool { self.0 & (1u64 << event.bit()) != 0 }
    pub const fn bits(self) -> u64 { self.0 }
}

/// A pending bit 0x00d7d154 distinguishes, in the order it tests them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UsbEvent {
    /// Bit 46, 0x00d7d2fc: writes 0x81810303 to 0xfff5512c and calls 0x00d814f4(0).
    CompletionReset = 46,
    /// Bit 44, 0x00d7d360: calls 0x00d7ae94, releases both channel handles at
    /// 0x01469768+4/+8, writes 0xfff5510c=0, 0xfff54070&=0xfffc, 0xfff54004|=0xc,
    /// 0xfff55114=0x101, delays 1 ms, pulses 0xfff54080=0xfff, clears shadow bits
    /// 2, 18 and 38, copies bus-speed bit 14 to 0xfff55108 bit 12 and calls 0x00d81598.
    TransferReset = 44,
    /// Bit 31, 0x00d7d544: reads 0xfff54018 and acknowledges.
    BusState = 31,
    /// Bit 33, 0x00d7d5a0: clears bits 33 and 1; a device-to-host direction sets
    /// 0xfff54004 bit 2; a status phase calls 0x00d7e970(0) or callback slot +8.
    ControlComplete = 33,
    /// Bit 13, 0x00d7d674: reads the eight-byte setup packet at 0xfff55188, writes the
    /// control block at 0x014697b0, dispatches callback slot +0 or 0x00d7e910, applies
    /// completion 0 and starts the data stage through 0x00d7c92c/0x00d7bd10.
    ControlPacket = 13,
    /// Bit 0, 0x00d7d848: clears bit 0 and calls 0x00d7cf50.
    ControlTransferZero = 0,
    /// Bit 1, 0x00d7d898: clears bit 1 and calls 0x00d7cf50.
    ControlTransferOne = 1,
    /// Bit 4, 0x00d7d8e8: clears bit 4 and requests callback slot 3 through 0x00d7c6c8(0,3).
    ChannelThree = 4,
    /// Bit 38, 0x00d7d940: clears bit 38; when 0x01469798+0x10 is one, clears it,
    /// requests callback slot 2 and clears shadow byte 0x1a bit 2.
    ChannelOneComplete = 38,
    /// Bit 3, 0x00d7d9d0: clears bit 3, sets shadow byte 0x18 bit 3 and requests
    /// callback slot 2 through 0x00d7c6c8(0,2).
    ChannelTwoRequest = 3,
    /// Bit 2, 0x00d7da3c: clears bit 2; while 0x01469768+0x20 is one, sets 0xfff54004
    /// bit 3, reads 0xfff54194, calls callback slot +0xc, applies completion 1, then
    /// transmits through 0x00d7cc60/0x00d7c92c and clears 0xfff54004 bit 3.
    ChannelTwoTransfer = 2,
    /// Bit 18, 0x00d7db80: clears bit 18; when 0x01469798+0x10 is zero and callback
    /// slot +0x14 is set, calls it, applies completion 2 and transmits through 0xd7c92c.
    ChannelOneTransfer = 18,
}

impl UsbEvent {
    pub const ORDER: [Self; 12] = [
        Self::CompletionReset, Self::TransferReset, Self::BusState, Self::ControlComplete,
        Self::ControlPacket, Self::ControlTransferZero, Self::ControlTransferOne, Self::ChannelThree,
        Self::ChannelOneComplete, Self::ChannelTwoRequest, Self::ChannelTwoTransfer,
        Self::ChannelOneTransfer,
    ];
    pub const fn bit(self) -> u8 { self as u8 }
}

/// The acknowledge halfwords 0xfff54054/58/5c; a zero bit clears its pending bit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Acknowledge { pub low: u16, pub middle: u16, pub high: u16 }

impl Acknowledge {
    pub const fn all() -> Self { Self { low: 0xffff, middle: 0xffff, high: 0xffff } }
    fn clear(&mut self, bit: u8) {
        match bit {
            0..=15 => self.low &= !(1u16 << bit),
            16..=31 => self.middle &= !(1u16 << (bit - 16)),
            32..=47 => self.high &= !(1u16 << (bit - 32)),
            _ => {}
        }
    }
}

/// The events one pump pass reports and the acknowledge it writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pumped {
    events: [Option<UsbEvent>; 12],
    len: usize,
    acknowledge: Option<Acknowledge>,
}

impl Pumped {
    /// 0x00d7d174 returns before the acknowledge tail when the stop flag is set.
    pub const STOPPED: Self = Self { events: [None; 12], len: 0, acknowledge: None };

    pub fn events(&self) -> impl Iterator<Item = UsbEvent> + '_ {
        self.events[..self.len].iter().filter_map(|event| *event)
    }
    pub const fn len(&self) -> usize { self.len }
    pub const fn is_empty(&self) -> bool { self.len == 0 }
    pub const fn acknowledge(self) -> Option<Acknowledge> { self.acknowledge }
}

/// `usb_irq_controller.pending` at 0x01469768+0x24: the stop path at 0x00d7d174.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PumpState { pub stopping: bool }

impl PumpState {
    pub const RUNNING: Self = Self { stopping: false };
}

pub fn pump(status: UsbStatus, state: PumpState) -> Pumped {
    if state.stopping { return Pumped::STOPPED; }
    let mut events = [None; 12];
    let mut len = 0;
    let mut acknowledge = Acknowledge::all();
    acknowledge.clear(18);
    for event in UsbEvent::ORDER {
        if status.contains(event) {
            events[len] = Some(event);
            len += 1;
            acknowledge.clear(event.bit());
        }
    }
    Pumped { events, len, acknowledge: Some(acknowledge) }
}

/// The eight bytes the controller delivers at 0xfff55188.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl SetupPacket {
    pub fn parse(bytes: [u8; 8]) -> Self {
        Self {
            request_type: bytes[0], request: bytes[1],
            value: u16::from_le_bytes([bytes[2], bytes[3]]),
            index: u16::from_le_bytes([bytes[4], bytes[5]]),
            length: u16::from_le_bytes([bytes[6], bytes[7]]),
        }
    }
    /// 0x00d7d71c: bits 5 and 6 of bmRequestType.
    pub const fn kind(self) -> ControlKind {
        match self.request_type & 0x60 {
            0x00 => ControlKind::Standard, 0x20 => ControlKind::Class,
            0x40 => ControlKind::Vendor, _ => ControlKind::Reserved,
        }
    }
    /// 0x00d7d6e4..0x00d7d710: one for a device-to-host request, two for a nonempty
    /// host-to-device data stage and zero for a zero-length one.
    pub const fn direction(self) -> ControlDirection {
        if self.request_type & 0x80 != 0 { ControlDirection::In }
        else if self.length != 0 { ControlDirection::Out }
        else { ControlDirection::None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlKind { Standard, Class, Vendor, Reserved }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlDirection { None, In, Out }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlPhase { Idle, Data, Status }

/// The control transfer block at 0x014697b0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlBlock {
    /// 0x014697b4: the length the setup entry arms; 12 for a vendor host-to-device request.
    pub target_length: u32,
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
    /// 0x014697c8: 0 standard, 1 class, 2 vendor.
    pub kind: ControlKind,
    /// 0x014697cc: 0 none, 1 device-to-host, 2 host-to-device.
    pub direction: ControlDirection,
    /// 0x014697d0: 0 idle, 1 data, 2 status.
    pub phase: ControlPhase,
}

impl ControlBlock {
    pub const EMPTY: Self = Self {
        target_length: 0, request_type: 0, request: 0, value: 0, index: 0, length: 0,
        kind: ControlKind::Standard, direction: ControlDirection::None, phase: ControlPhase::Idle,
    };

    /// 0x00d7d674 writes the setup fields, the kind and the direction; the phase word at
    /// 0x014697d0 is written by 0x00d7c20c from the setup entry's result.
    pub fn apply_setup(&mut self, packet: SetupPacket) {
        self.request_type = packet.request_type;
        self.request = packet.request;
        self.value = packet.value;
        self.index = packet.index;
        self.length = packet.length;
        self.kind = packet.kind();
        self.direction = packet.direction();
    }
}

/// The two DMA descriptor blocks the pump's channel events test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Descriptor { Transmit, Receive }

/// 0x01469798 and 0x014697fc.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferDescriptor {
    /// +0x08: the state word 0x00d7c20c writes.
    pub state: u32,
    /// +0x10: the active flag the channel events test and clear.
    pub active: bool,
}

impl TransferDescriptor {
    pub const IDLE: Self = Self { state: 0, active: false };
}

/// The pump's register half over `UsbStart` and the controller registers.
pub struct TransportPump { last: Pumped }

impl TransportPump {
    pub const fn new() -> Self { Self { last: Pumped::STOPPED } }
    pub const fn last(&self) -> Pumped { self.last }

    /// Reads the pending word and runs `pump`. The caller applies the events, then calls `finish`.
    pub fn read<I: StartIo>(&mut self, start: &mut UsbStart<'_>, io: &mut I) -> Pumped {
        let status = start.pending_status(io);
        let pumped = pump(status, PumpState { stopping: start.pump_stopping() });
        self.last = pumped;
        pumped
    }

    /// 0x00d7dc58: writes the acknowledge halfwords, then restores the interrupt block and the
    /// shadow masks while the controller is active.
    pub fn finish<I: ControllerIo>(start: &mut UsbStart<'_>, io: &mut I, pumped: Pumped) {
        let Some(acknowledge) = pumped.acknowledge() else { return; };
        start.acknowledge(io, acknowledge);
        start.restore_event_masks(io);
    }
}

impl Default for TransportPump {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
#[path = "transport_pump_tests.rs"]
mod tests;
