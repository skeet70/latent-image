//! X-M1 1.50 configuration actions, pinned to x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Shared completion values require the service owner to execute the notification path.
//! Owns actions 2..8, 11..13, 21, 30, and 31. Profile action 9 belongs to the persistent queue.
//! GPIO output locking uses the shared Power CPU implementation at 0x7235d8/0x7236b0.
//! Peripheral roots 0xdba6c4..0xdba9d4 and memory root 0xdbc924 use authenticated image bytes.
//! Stack residue inputs preserve the stock empty-list and invalid-channel behavior.
//! No action in this module registers an asynchronous continuation.

use crate::backend::{BackendError, ConfigImages, ConfigSpan};
use xm1_power_services::osal::{self, CriticalKernel};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuReturn(i32);
impl CpuReturn {
    pub const SUCCESS: Self = Self(0);
    pub const REJECTED: Self = Self(-255);
    pub const fn from_abi(value: i32) -> Self { Self(value) }
    pub const fn get(self) -> i32 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultFlags(u32);
impl ResultFlags {
    pub const SUCCESS: Self = Self(0);
    pub const FAILURE: Self = Self(0x80000000);
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn get(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionNotification { None, ServiceResult(ResultFlags), ScanEvent4(ResultFlags) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionCompletion { pub return_code: CpuReturn, pub notification: ActionNotification }
impl ActionCompletion {
    pub const fn service_success() -> Self { Self { return_code: CpuReturn::SUCCESS, notification: ActionNotification::ServiceResult(ResultFlags::SUCCESS) } }
    pub const fn service_rejected() -> Self { Self { return_code: CpuReturn::REJECTED, notification: ActionNotification::ServiceResult(ResultFlags::FAILURE) } }
    pub const fn silent_success() -> Self { Self { return_code: CpuReturn::SUCCESS, notification: ActionNotification::None } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationKey(u16);
impl ConfigurationKey {
    pub const fn from_abi(value: u16) -> Self { Self(value) }
    pub const fn get(self) -> u16 { self.0 }
    pub fn read(self, configuration: &ConfigImages<'_>) -> Result<u8, BackendError> {
        configuration.byte(u32::from(self.0)).ok_or(BackendError::Capacity)
    }
    pub fn write(self, configuration: &mut ConfigImages<'_>, value: u8) -> Result<(), BackendError> {
        configuration.write(ConfigSpan::new(u32::from(self.0), 1)?, &[value])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LowAction {
    WritePins,
    RejectScan,
    ReadPins,
    ReadChannel,
    WriteChannel,
    BatteryAcknowledgment,
    PeripheralAcknowledgment,
    RegisterAcknowledgment,
    Silent12,
    Silent13,
    MediaAcknowledgment,
    MemoryAcknowledgment,
    MemoryWord,
}
impl LowAction {
    pub fn parse(action: u16) -> Result<Self, BackendError> {
        Ok(match action {
            2 => Self::WritePins,
            3 => Self::RejectScan,
            4 => Self::ReadPins,
            5 => Self::ReadChannel,
            6 => Self::WriteChannel,
            7 => Self::BatteryAcknowledgment,
            8 => Self::PeripheralAcknowledgment,
            11 => Self::RegisterAcknowledgment,
            12 => Self::Silent12,
            13 => Self::Silent13,
            21 => Self::MediaAcknowledgment,
            30 => Self::MemoryAcknowledgment,
            31 => Self::MemoryWord,
            _ => return Err(BackendError::InvalidCommand),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalAddress(u32);
impl PhysicalAddress { pub const fn get(self) -> u32 { self.0 } }

pub trait PhysicalBus {
    fn read_word(&mut self, address: PhysicalAddress) -> u32;
    fn write_word(&mut self, address: PhysicalAddress, value: u32);
    fn read_byte(&mut self, address: PhysicalAddress) -> u8;
    fn write_byte(&mut self, address: PhysicalAddress, value: u8);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpioBank(u8);
impl GpioBank {
    pub fn parse(value: u8) -> Result<Self, CpuReturn> {
        if value < 16 { Ok(Self(value)) } else { Err(CpuReturn::REJECTED) }
    }
    const fn address(self, offset: u32) -> PhysicalAddress {
        PhysicalAddress(0xfff89000 + self.0 as u32 * 0x40 + offset)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinLevels(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExcludedPins(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaskedOutputWord(u32);
impl MaskedOutputWord {
    pub const fn new(levels: PinLevels, excluded: ExcludedPins) -> Self {
        let mask = !excluded.0;
        Self(((mask as u32) << 16) | (levels.0 & mask) as u32)
    }
    pub const fn get(self) -> u32 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootTimerState { Running, NotRunning }
impl BootTimerState {
    pub const fn from_abi(value: u32) -> Self {
        if value == 1 { Self::Running } else { Self::NotRunning }
    }
}

/// Stack values read by stock when an empty pin list or invalid channel leaves its output untouched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeripheralFrameResidue { pub first_pin: u32, pub channel_halfword: u16 }

pub fn write_pin_outputs(
    bus: &mut impl PhysicalBus,
    kernel: &mut impl CriticalKernel,
    bank: GpioBank,
    levels: PinLevels,
    excluded: ExcludedPins,
    frame: PeripheralFrameResidue,
) {
    let bank = if excluded.0 == u16::MAX { GpioBank(((frame.first_pin >> 16) & 15) as u8) } else { bank };
    let output = MaskedOutputWord::new(levels, excluded);
    let saved = osal::interrupt_lock(kernel);
    bus.write_word(bank.address(0), output.get());
    osal::interrupt_unlock(kernel, saved);
}

fn key(offset: u16) -> ConfigurationKey { ConfigurationKey::from_abi(offset) }
fn input(configuration: &ConfigImages<'_>, offset: u16) -> Result<u8, BackendError> { key(offset).read(configuration) }
fn output(configuration: &mut ConfigImages<'_>, offset: u16, value: u8) -> Result<(), BackendError> { key(offset).write(configuration, value) }
fn input_halfword(configuration: &ConfigImages<'_>, high: u16, low: u16) -> Result<u16, BackendError> {
    Ok(u16::from_be_bytes([input(configuration, high)?, input(configuration, low)?]))
}

pub fn execute(
    action: LowAction,
    configuration: &mut ConfigImages<'_>,
    bus: &mut impl PhysicalBus,
    kernel: &mut impl CriticalKernel,
    frame: PeripheralFrameResidue,
) -> Result<ActionCompletion, BackendError> {
    match action {
        LowAction::RejectScan => return Ok(ActionCompletion { return_code: CpuReturn::SUCCESS, notification: ActionNotification::ScanEvent4(ResultFlags::FAILURE) }),
        LowAction::Silent12 | LowAction::Silent13 => return Ok(ActionCompletion::silent_success()),
        LowAction::BatteryAcknowledgment | LowAction::PeripheralAcknowledgment | LowAction::RegisterAcknowledgment |
        LowAction::MediaAcknowledgment | LowAction::MemoryAcknowledgment => {}
        LowAction::WritePins => {
            let bank = GpioBank::parse(input(configuration, 129)?);
            let levels = PinLevels(input_halfword(configuration, 130, 131)?);
            let excluded = ExcludedPins(input_halfword(configuration, 132, 133)?);
            let Ok(bank) = bank else { return Ok(ActionCompletion::service_rejected()); };
            write_pin_outputs(bus, kernel, bank, levels, excluded, frame);
        }
        LowAction::ReadPins => {
            let Ok(bank) = GpioBank::parse(input(configuration, 129)?) else { return Ok(ActionCompletion::service_rejected()); };
            let value = bus.read_word(bank.address(0x14)) as u16;
            output(configuration, 131, value as u8)?;
            output(configuration, 130, (value >> 8) as u8)?;
        }
        LowAction::ReadChannel => {
            let channel = input(configuration, 129)?;
            let value = if channel <= 11 {
                bus.read_word(PhysicalAddress(0xfff8a030 + u32::from(channel) * 4)) as u16
            } else { frame.channel_halfword };
            output(configuration, 131, value as u8)?;
            output(configuration, 130, (value >> 8) as u8)?;
        }
        LowAction::WriteChannel => {
            let channel = input(configuration, 129)?.wrapping_sub(16);
            let level = input_halfword(configuration, 130, 131)?;
            let timer = BootTimerState::from_abi(bus.read_word(PhysicalAddress(0x009743e8)));
            if timer == BootTimerState::Running && channel == 0 {
                bus.write_word(PhysicalAddress(0xfff86008), u32::from(level & 0x3ff));
            }
        }
        LowAction::MemoryWord => return memory_word(configuration, bus),
    }
    Ok(ActionCompletion::service_success())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryWordMode { Read, Write, NoOperation }
impl MemoryWordMode {
    pub const fn from_abi(value: u8) -> Self { match value { 0 => Self::Read, 1 => Self::Write, _ => Self::NoOperation } }
}

pub fn memory_word(configuration: &mut ConfigImages<'_>, bus: &mut impl PhysicalBus) -> Result<ActionCompletion, BackendError> {
    let address = u32::from_be_bytes([
        input(configuration, 131)?, input(configuration, 132)?,
        input(configuration, 133)?, input(configuration, 134)?,
    ]);
    if address & 3 != 0 || !(address <= 0x1fffffff || address.wrapping_add(0xc0000000) <= 0x1fffffff) {
        output(configuration, 130, 0)?;
        return Ok(ActionCompletion::service_success());
    }
    match MemoryWordMode::from_abi(input(configuration, 129)?) {
        MemoryWordMode::Read => {
            output(configuration, 135, (bus.read_word(PhysicalAddress(address)) >> 24) as u8)?;
            output(configuration, 136, (bus.read_word(PhysicalAddress(address)) >> 16) as u8)?;
            output(configuration, 137, (bus.read_word(PhysicalAddress(address)) >> 8) as u8)?;
            output(configuration, 138, bus.read_byte(PhysicalAddress(address)))?;
            output(configuration, 130, 1)?;
        }
        MemoryWordMode::Write => {
            for (index, offset) in (135..=138).rev().enumerate() {
                bus.write_byte(PhysicalAddress(address + index as u32), input(configuration, offset)?);
            }
            let mut matches = true;
            for (index, offset) in (135..=138).rev().enumerate() {
                let actual = bus.read_byte(PhysicalAddress(address + index as u32));
                if actual != input(configuration, offset)? { matches = false; break; }
            }
            output(configuration, 130, u8::from(matches))?;
        }
        MemoryWordMode::NoOperation => {}
    }
    Ok(ActionCompletion::service_success())
}

#[cfg(target_arch = "arm")]
pub struct NativePhysicalBus { _private: () }
#[cfg(target_arch = "arm")]
impl NativePhysicalBus {
    /// # Safety
    /// The caller owns access to the X-M1 physical address space accepted by these actions.
    pub unsafe fn adopt() -> Self { Self { _private: () } }
}
#[cfg(target_arch = "arm")]
impl PhysicalBus for NativePhysicalBus {
    fn read_word(&mut self, address: PhysicalAddress) -> u32 { unsafe { core::ptr::read_volatile(address.get() as *const u32) } }
    fn write_word(&mut self, address: PhysicalAddress, value: u32) { unsafe { core::ptr::write_volatile(address.get() as *mut u32, value); } }
    fn read_byte(&mut self, address: PhysicalAddress) -> u8 { unsafe { core::ptr::read_volatile(address.get() as *const u8) } }
    fn write_byte(&mut self, address: PhysicalAddress, value: u8) { unsafe { core::ptr::write_volatile(address.get() as *mut u8, value); } }
}

#[cfg(test)]
#[path = "configuration_actions_low_tests.rs"]
mod tests;
