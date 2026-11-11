//! Application radio initialization at X-M1 0x01292024 and teardown at 0x01292dcc.
//! Source qualification and integration contract: vendor_wireless_boot.evidence.json.

use super::sd_controller::{Argument, Buffer, CardInterruptRoute, Completion, ControlError, ControlPlane, Execution, Host, InterruptRouter, Mmio, Operation, PhysicalResult, Request, VendorCode};
use super::vendor_wireless_adapter::{Adapter, AdapterError, AddressConfiguration, DescriptorId, HardwareConfiguration, PhysicalWrite};
use super::vendor_wireless_boot_query::{FirmwareCapabilities, Query, QueryReply};
use super::vendor_wireless_boot_query_driver::{QueryDriver, QueryFailure};
use super::vendor_wireless_lifecycle::DriverStatus;
use super::vendor_wireless_lifecycle_download::{FirmwareAddress, FirmwareDownload, FirmwareDownloadError};
use super::vendor_wireless_network::MacAddress;
use super::vendor_wireless_parameter_driver::{TrafficClass, TransmitError};
use super::vendor_wireless_power::RadioTick;
use super::vendor_wireless_station_driver::receive::gate::{FirmwareOwnership, GateError, OwnershipGate, PhysicalOutcome, StopFlags, Tick};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootError { Download(FirmwareDownloadError), Firmware, Physical, ReadyTimeout, WrongChip, Gate(GateError), Query(QueryFailure), Adapter(AdapterError), Credits(TransmitError), CreditTimeout, Cancelled, Busy, Interrupt(ControlError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChipRevision(u8);
impl ChipRevision { pub const fn get(self) -> u8 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PowerMode { Awake = 0, MaximumSave = 1, FastSave = 2 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootReady { pub address: MacAddress, pub firmware_address: MacAddress, pub capabilities: FirmwareCapabilities, pub chip: ChipRevision }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootOutcome { Ready(BootReady), Stopped, Failed(BootError) }
#[derive(Clone, Copy, Eq, PartialEq)]
enum Command { Clear, Power, Filter, Stop }
#[derive(Clone, Copy, Eq, PartialEq)]
enum Target { Command(Command), Query(Query), Adapter }
#[derive(Clone, Copy)]
enum Phase { Download, DisableInterrupt, Ready, Delay(RadioTick), Diagnostic, Gate, GateDelay(RadioTick), Chip, ReadConfiguration, WriteConfiguration, InterruptMask, ReceiveConfiguration, ReadInterrupt, CreditLow, CreditHigh, Reserve(Target), RefillLow, RefillHigh, RefillDelay(RadioTick), Command(Command), Query(Query), Adapter, FinalReadTwo, FinalWriteTwo, FinalReadFour, FinalWriteFour, EnableInterrupt, BindInterrupt, StopDisable, StopReady, StopDelay(RadioTick), StopReleaseWrite, StopReleaseRead, StopReleaseRetry, Finished }

pub struct RadioBoot<'image, 'adapter, 'frames, const F: usize, const D: usize> {
    adapter: &'adapter mut Adapter<'frames, F, D>,
    hardware: &'adapter HardwareConfiguration,
    address: AddressConfiguration,
    power_mode: PowerMode,
    download: FirmwareDownload<'image>,
    query: Option<QueryDriver>,
    gate: OwnershipGate,
    phase: Phase,
    register: [u8; 4],
    packet: [u8; 32],
    query_packet: [u8; 28],
    chunk: [u8; 2560],
    credits: [u8; 8],
    refill_target: Target,
    refill_polls: u16,
    polls: u16,
    issued: bool,
    descriptor: Option<DescriptorId>,
    chip: ChipRevision,
    firmware_address: Option<MacAddress>,
    capabilities: Option<FirmwareCapabilities>,
    outcome: Option<BootOutcome>,
    physical: Option<Completion>,
    stopping: bool,
    download_physical_failure: bool,
}

impl<'image, 'adapter, 'frames, const F: usize, const D: usize> RadioBoot<'image, 'adapter, 'frames, F, D> {
    pub fn new(image: &'image [u8], destination: FirmwareAddress, adapter: &'adapter mut Adapter<'frames, F, D>, hardware: &'adapter HardwareConfiguration, address: AddressConfiguration, power_mode: PowerMode, now: RadioTick) -> Result<Self, BootError> {
        let download = FirmwareDownload::new(image, destination).map_err(BootError::Download)?;
        adapter.cancel();
        adapter.reset_protocols();
        adapter.reset_credits();
        Ok(Self { adapter, hardware, address, power_mode, download, query: None,
            gate: OwnershipGate::new(FirmwareOwnership::Firmware, Tick(now.0)), phase: Phase::Download,
            register: [0; 4], packet: [0; 32], query_packet: [0; 28], chunk: [0; 2560], credits: [0; 8],
            refill_target: Target::Adapter, refill_polls: 0, polls: 0, issued: false, descriptor: None,
            chip: ChipRevision(0), firmware_address: None, capabilities: None, outcome: None, physical: None, stopping: false, download_physical_failure: false })
    }

    pub const fn outcome(&self) -> Option<BootOutcome> { self.outcome }
    pub const fn last_physical(&self) -> Option<Completion> { self.physical }
    pub const fn awaiting_interrupt_binding(&self) -> bool { matches!(self.phase, Phase::BindInterrupt) }

    pub fn bind_interrupt(&mut self, control: &mut ControlPlane, host: &mut Host, mmio: &mut impl Mmio, router: &mut impl InterruptRouter) -> Result<(), BootError> {
        if !matches!(self.phase, Phase::BindInterrupt) { return Err(BootError::Busy); }
        if let Err(error) = control.set_card_interrupt(host, mmio, router, CardInterruptRoute::Wireless) {
            let error = BootError::Interrupt(error); self.fail(error); return Err(error);
        }
        self.outcome = Some(BootOutcome::Ready(BootReady { address: self.adapter.publication().address, firmware_address: self.firmware_address.unwrap(), capabilities: self.capabilities.unwrap(), chip: self.chip }));
        self.phase = Phase::Finished;
        Ok(())
    }

    pub fn begin_teardown(&mut self, now: RadioTick) -> Result<(), BootError> {
        if !matches!(self.outcome, Some(BootOutcome::Ready(_))) || self.issued { return Err(BootError::Busy); }
        self.outcome = None;
        self.stopping = true;
        self.adapter.stop_protocols();
        self.gate = OwnershipGate::new(FirmwareOwnership::Firmware, Tick(now.0));
        self.phase = Phase::StopDisable;
        self.polls = 0;
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.gate.cancel();
        self.adapter.stop_protocols();
        self.adapter.cancel();
        self.query = None;
        self.issued = false;
        self.outcome = Some(BootOutcome::Failed(BootError::Cancelled));
        self.phase = Phase::Finished;
    }

    fn fail(&mut self, error: BootError) {
        self.adapter.stop_protocols();
        self.adapter.cancel();
        self.query = None;
        self.outcome = Some(BootOutcome::Failed(error));
        self.phase = Phase::Finished;
    }

    fn reserve(&mut self, target: Target) {
        self.refill_target = target;
        self.refill_polls = 0;
        self.phase = Phase::Reserve(target);
    }

    fn issue_sequence(&mut self) -> u8 {
        let next = self.adapter.last_sequence().wrapping_add(1);
        self.adapter.set_last_sequence(next);
        next
    }

    pub fn request(&mut self, now: RadioTick) -> Option<Request<'_>> {
        if self.issued { return None; }
        loop {
            match self.phase {
                Phase::Download => {
                    if let Some(outcome) = self.download.outcome() {
                        if outcome != DriverStatus::SUCCESS { self.fail(BootError::Firmware); return None; }
                        if self.download_physical_failure { self.fail(BootError::Physical); return None; }
                        self.phase = Phase::DisableInterrupt;
                    } else { break; }
                }
                Phase::Delay(deadline) => {
                    if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
                    self.phase = Phase::Ready;
                }
                Phase::StopDelay(deadline) => {
                    if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
                    self.phase = Phase::StopReady;
                }
                Phase::GateDelay(deadline) => {
                    if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
                    if let Err(error) = self.gate.finish_delay() { self.fail(BootError::Gate(error)); return None; }
                    self.phase = Phase::Gate;
                }
                Phase::Gate => {
                    if let Some(outcome) = self.gate.outcome() {
                        match outcome {
                            Ok(()) if self.stopping => self.reserve(Target::Command(Command::Stop)),
                            Ok(()) => self.phase = Phase::Chip,
                            Err(error) => { self.fail(BootError::Gate(error)); return None; }
                        }
                    } else if self.gate.delay_ticks().is_some() {
                        self.phase = Phase::GateDelay(RadioTick(now.0.wrapping_add(1)));
                        return None;
                    } else { break; }
                }
                Phase::Reserve(target) => {
                    let mut credits = self.adapter.take_credits();
                    let available = credits.consume(TrafficClass::Command);
                    self.adapter.set_credits(credits);
                    if !available {
                        if self.refill_polls >= 256 { self.fail(BootError::CreditTimeout); return None; }
                        self.phase = Phase::RefillLow;
                        continue;
                    }
                    match target {
                        Target::Command(command) => { self.encode_command(command); self.phase = Phase::Command(command); }
                        Target::Query(query) => {
                            let sequence = self.issue_sequence();
                            self.query = Some(QueryDriver::new(query, sequence));
                            self.phase = Phase::Query(query);
                        }
                        Target::Adapter => unreachable!(),
                    }
                }
                Phase::RefillDelay(deadline) => {
                    if (now.0.wrapping_sub(deadline.0) as i32) < 0 { return None; }
                    self.phase = if self.refill_target == Target::Adapter { Phase::Adapter } else { Phase::Reserve(self.refill_target) };
                }
                Phase::Query(_) => {
                    if let Some(outcome) = self.query.as_ref().and_then(QueryDriver::outcome) {
                        match outcome {
                            Err(error) => { self.fail(BootError::Query(error)); return None; }
                            Ok(QueryReply::Acknowledged) => { self.fail(BootError::Firmware); return None; }
                            Ok(QueryReply::Address(address)) => {
                                self.firmware_address = Some(address);
                                self.reserve(Target::Query(Query::Capabilities));
                            }
                            Ok(QueryReply::Capabilities(capabilities)) => {
                                self.capabilities = Some(capabilities);
                                let mut address = self.address;
                                address.preserve_invalid = capabilities.preserve_invalid_address;
                                if let Err(error) = self.adapter.configure_address(address) { self.fail(BootError::Adapter(error)); return None; }
                                self.adapter.protocols().reset_owner_notification();
                                self.phase = Phase::Adapter;
                            }
                        }
                    } else { break; }
                }
                Phase::Adapter => {
                    if self.refill_polls >= 256 { self.fail(BootError::CreditTimeout); return None; }
                    if !self.adapter.has_pending_commands() {
                        if self.stopping { self.finish_stop(); return None; }
                        if let Err(error) = self.adapter.configure(self.hardware) { self.fail(BootError::Adapter(error)); return None; }
                        self.adapter.reset_protocols();
                        self.reserve(Target::Command(Command::Power));
                    } else { break; }
                }
                _ => break,
            }
        }
        match self.phase {
            Phase::Download => {
                let request = self.download.request(now, &mut self.register, &mut self.chunk)?;
                self.issued = true;
                return Some(request);
            }
            Phase::Gate => {
                let request = self.gate.request()?;
                self.issued = true;
                return Some(request);
            }
            Phase::Query(_) => {
                let request = self.query.as_mut()?.request(now, &mut self.register, &mut self.query_packet)?;
                self.issued = true;
                return Some(request);
            }
            Phase::Adapter => {
                if let Some(command) = self.adapter.command() {
                    self.descriptor = Some(command.identity());
                    self.issued = true;
                    return Some(Request { operation: Operation::Extended53, argument: Argument::from_wire(command.argument()), execution: Execution::Polling, buffer: Buffer::Fifo(command.bytes_mut()) });
                }
                self.refill_target = Target::Adapter;
                self.phase = Phase::RefillLow;
                return None;
            }
            Phase::Command(command) => {
                let length = match command { Command::Clear => 16, Command::Power | Command::Stop => 12, Command::Filter => 32 };
                self.issued = true;
                return Some(Request { operation: Operation::Extended53, argument: Argument::from_wire(0x9000_5800 | length as u32), execution: Execution::Polling, buffer: Buffer::Fifo(&mut self.packet[..length]) });
            }
            _ => {}
        }
        let (address, write, direct) = match self.phase {
            Phase::DisableInterrupt | Phase::StopDisable => (4, Some(2), true),
            Phase::Ready | Phase::Chip | Phase::StopReady => (0, None, false),
            Phase::Diagnostic => (0x40, None, false),
            Phase::ReadConfiguration | Phase::ReceiveConfiguration | Phase::FinalReadTwo | Phase::FinalReadFour => (0xc, None, false),
            Phase::WriteConfiguration | Phase::FinalWriteTwo | Phase::FinalWriteFour => (0xc, Some(u32::from_le_bytes(self.register)), false),
            Phase::InterruptMask => (0x14, Some(0xffff_ff0f), false),
            Phase::ReadInterrupt => (0x10, None, false),
            Phase::CreditLow | Phase::RefillLow => (0x20, None, false),
            Phase::CreditHigh | Phase::RefillHigh => (0x24, None, false),
            Phase::EnableInterrupt => (4, Some(1), true),
            Phase::StopReleaseWrite => (4, Some(0x100), false),
            Phase::StopReleaseRead => (4, None, false),
            Phase::StopReleaseRetry => (4, Some(0x200), false),
            _ => return None,
        };
        self.issued = true;
        if direct {
            return Some(Request { operation: Operation::Direct52, argument: Argument::from_wire(0x9000_0000 | (address << 9) | write.unwrap()), execution: Execution::Polling, buffer: Buffer::None });
        }
        if let Some(value) = write { self.register = value.to_le_bytes(); }
        Some(Request { operation: Operation::Extended53, argument: Argument::from_wire(0x1000_0004 | (address << 9) | if write.is_some() { 0x8000_0000 } else { 0 }), execution: Execution::Polling, buffer: Buffer::Fifo(&mut self.register) })
    }

    pub fn receive(&mut self, physical: Completion, now: RadioTick, flags: StopFlags) -> bool {
        if !self.issued { return false; }
        self.issued = false;
        self.physical = Some(physical);
        let cancelled = flags.bus_removed || flags.service_stopped;
        if matches!(self.phase, Phase::Download) {
            self.download_physical_failure |= physical.physical != PhysicalResult::Completed || physical.code != VendorCode::Success;
            self.download.receive(physical, self.register, now, cancelled);
            return true;
        }
        if cancelled { self.cancel(); return true; }
        if physical.physical != PhysicalResult::Completed || physical.code != VendorCode::Success {
            self.fail(BootError::Physical); return true;
        }
        let value = u32::from_le_bytes(self.register);
        match self.phase {
            Phase::DisableInterrupt => { self.polls = 0; self.phase = Phase::Ready; }
            Phase::Ready => {
                if value & 0x20_0000 != 0 {
                    self.gate = OwnershipGate::new(FirmwareOwnership::Firmware, Tick(now.0)); self.phase = Phase::Gate;
                } else if self.polls > 511 { self.phase = Phase::Diagnostic; }
                else { self.polls += 1; self.phase = Phase::Delay(RadioTick(now.0.wrapping_add(10))); }
            }
            Phase::Diagnostic => self.fail(BootError::ReadyTimeout),
            Phase::Gate => { if let Err(error) = self.gate.finish(PhysicalOutcome::Completed, Tick(now.0), flags) { self.fail(BootError::Gate(error)); } }
            Phase::Chip => {
                if value & 0xffff != 0x5931 { self.fail(BootError::WrongChip); }
                else { self.chip = ChipRevision(((value >> 16) & 15) as u8); self.phase = Phase::ReadConfiguration; }
            }
            Phase::ReadConfiguration => { self.register = (value & !0x1_00f0).to_le_bytes(); self.phase = Phase::WriteConfiguration; }
            Phase::WriteConfiguration => self.phase = Phase::InterruptMask,
            Phase::InterruptMask => self.phase = Phase::ReceiveConfiguration,
            Phase::ReceiveConfiguration => { self.register = (value & !0xf0).to_le_bytes(); self.phase = Phase::FinalWriteTwo; }
            Phase::ReadInterrupt => self.phase = if value & 1 != 0 { Phase::CreditLow } else { self.adapter.reset_credits(); self.reserve(Target::Command(Command::Clear)); return true; },
            Phase::CreditLow => { self.credits[..4].copy_from_slice(&self.register); self.phase = Phase::CreditHigh; }
            Phase::CreditHigh => { self.credits[4..].copy_from_slice(&self.register); self.adapter.reset_credits(); self.reserve(Target::Command(Command::Clear)); }
            Phase::Command(Command::Clear) => self.reserve(Target::Query(Query::Address)),
            Phase::Command(Command::Power) => {
                self.phase = Phase::Adapter;
                self.refill_target = Target::Command(Command::Filter);
            }
            Phase::Command(Command::Filter) => self.phase = Phase::FinalReadTwo,
            Phase::Command(Command::Stop) => { self.polls = 0; self.phase = Phase::StopReady; }
            Phase::Query(_) => { self.query.as_mut().unwrap().receive(physical, self.register, &self.query_packet, now, false); }
            Phase::Adapter => {
                let identity = self.descriptor.take().unwrap();
                self.adapter.finish_command(identity, PhysicalWrite::Written);
                self.refill_polls = 0;
                if !self.adapter.has_pending_commands() && self.refill_target == Target::Command(Command::Filter) { self.reserve(Target::Command(Command::Filter)); }
            }
            Phase::RefillLow => { self.credits[..4].copy_from_slice(&self.register); self.phase = Phase::RefillHigh; }
            Phase::RefillHigh => {
                self.credits[4..].copy_from_slice(&self.register);
                if let Err(error) = self.adapter.observe_credit_returns(self.credits) { self.fail(BootError::Credits(error)); }
                else {
                    let credits = self.adapter.take_credits();
                    let available = credits.available(TrafficClass::Command) != 0;
                    self.adapter.set_credits(credits);
                    self.refill_polls += 1;
                    self.phase = if available {
                        if self.refill_target == Target::Adapter { Phase::Adapter } else { Phase::Reserve(self.refill_target) }
                    } else { Phase::RefillDelay(RadioTick(now.0.wrapping_add(50))) };
                }
            }
            Phase::FinalReadTwo => { self.register = (value & !2).to_le_bytes(); self.phase = Phase::FinalWriteTwo; }
            Phase::FinalWriteTwo => self.phase = if self.capabilities.is_none() { Phase::ReadInterrupt } else { Phase::FinalReadFour },
            Phase::FinalReadFour => { self.register = (value & !4).to_le_bytes(); self.phase = Phase::FinalWriteFour; }
            Phase::FinalWriteFour => self.phase = Phase::EnableInterrupt,
            Phase::EnableInterrupt => self.phase = Phase::BindInterrupt,
            Phase::StopDisable => { self.gate = OwnershipGate::new(FirmwareOwnership::Firmware, Tick(now.0)); self.phase = Phase::Gate; }
            Phase::StopReady => {
                if value & 0x20_0000 == 0 { self.phase = Phase::StopReleaseWrite; }
                else if self.polls > 511 { self.fail(BootError::ReadyTimeout); }
                else { self.polls += 1; self.phase = Phase::StopDelay(RadioTick(now.0.wrapping_add(10))); }
            }
            Phase::StopReleaseWrite => self.phase = Phase::StopReleaseRead,
            Phase::StopReleaseRead => {
                if value & 0x100 == 0 { self.finish_stop(); }
                else { self.phase = Phase::StopReleaseRetry; }
            }
            Phase::StopReleaseRetry => self.finish_stop(),
            _ => return false,
        }
        true
    }

    fn finish_stop(&mut self) {
        self.adapter.cancel_pending_frames();
        self.adapter.cancel_commands();
        self.adapter.cancel_received_descriptors();
        self.adapter.cancel_queued_frames();
        self.adapter.stop_protocols();
        self.outcome = Some(BootOutcome::Stopped);
        self.phase = Phase::Finished;
    }

    fn encode_command(&mut self, command: Command) {
        let sequence = self.issue_sequence();
        self.packet.fill(0);
        let (length, opcode) = match command { Command::Clear => (16u16, 0xf2), Command::Power => (12, 6), Command::Filter => (32, 0xf1), Command::Stop => (12, 5) };
        self.packet[..2].copy_from_slice(&length.to_le_bytes());
        self.packet[3] = 0x50;
        self.packet[4] = opcode;
        self.packet[5] = 1;
        self.packet[6] = sequence;
        match command {
            Command::Power => self.packet[9] = self.power_mode as u8,
            Command::Filter => self.packet[12..16].copy_from_slice(&[0, 1, 0, 0x80]),
            Command::Stop => self.packet[8] = 1,
            Command::Clear => {}
        }
    }
}

#[cfg(test)]
#[path = "vendor_wireless_boot_tests.rs"]
mod tests;
