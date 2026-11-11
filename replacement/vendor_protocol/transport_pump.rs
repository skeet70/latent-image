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
    /// Bit 46, 0x00d7d2fc: resets the completion handle to zero and resets the interrupt block.
    CompletionReset = 46,
    /// Bit 44, 0x00d7d360: clears the control and channel state and reinitializes the controller.
    TransferReset = 44,
    /// Bit 31, 0x00d7d544: reads 0xfff54018 and acknowledges.
    BusState = 31,
    /// Bit 33, 0x00d7d5a0: sets endpoint-control bit 2 for an input control transfer.
    ControlComplete = 33,
    /// Bit 13, 0x00d7d674: control packet at 0xfff54188.
    ControlPacket = 13,
    /// Bit 0, 0x00d7d848: control completion at 0x00d7cf50.
    ControlTransferZero = 0,
    /// Bit 1, 0x00d7d898: control completion at 0x00d7cf50.
    ControlTransferOne = 1,
    /// Bit 4, 0x00d7d8e8: requests callback slot 3.
    ChannelThree = 4,
    /// Bit 38, 0x00d7d940: completes the transfer block at 0x01469798.
    ChannelOneComplete = 38,
    /// Bit 3, 0x00d7d9d0: sets shadow bit 3 and requests callback slot 2.
    ChannelTwoRequest = 3,
    /// Bit 2, 0x00d7da3c: transfers through the block at 0x014697fc.
    ChannelTwoTransfer = 2,
    /// Bit 18, 0x00d7db80: transfers through the block at 0x01469798.
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
