use super::{BootError, BootOutcome, PowerMode, RadioBoot};
use crate::sd_controller::{Buffer, Completion, ControlError, ControlPlane, HalfRegister, Host, InterruptRouter, Mmio, TransferUnit, VendorCode, PhysicalResult, WordRegister};
use crate::vendor_wireless_adapter::{Adapter, AddressConfiguration, CountryCode, HardwareConfiguration, HardwareRevision, RadioCapabilities, regulatory::RegulatoryInput};
use crate::vendor_wireless_lifecycle_download::FirmwareAddress;
use crate::vendor_wireless_network::MacAddress;
use crate::vendor_wireless_power::RadioTick;
use crate::vendor_wireless_station_driver::receive::gate::StopFlags;

fn hardware() -> HardwareConfiguration {
    HardwareConfiguration { first_revision: HardwareRevision { major: 1, minor: 7 }, second_revision: HardwareRevision { major: 1, minor: 7 }, use_calibration: false, calibration: [0; 40], feature_requested: false, feature_supported: false, feature_disabled: false, country: CountryCode(0x5553), radio_parameters: [0; 144], regulatory: RegulatoryInput::country([0; 2]), antenna: None }
}
fn address() -> AddressConfiguration { AddressConfiguration { address: Some(MacAddress([2, 3, 4, 5, 6, 7])), preserve_invalid: false, timestamp: 42, capabilities: RadioCapabilities(0) } }
fn completed() -> Completion { Completion { code: VendorCode::Success, physical: PhysicalResult::Completed, transferred: 0, status: 0, response: 0 } }
const FLAGS: StopFlags = StopFlags { bus_removed: false, service_stopped: false };

struct RadioDevice { query: u8, owed_credits: u8, power: Option<u8>, writes: usize }
impl RadioDevice {
    fn new() -> Self { Self { query: 0, owed_credits: 0, power: None, writes: 0 } }
    fn step(&mut self, boot: &mut RadioBoot<'_, '_, '_, 0, 16>, now: u32) -> bool {
        let Some(request) = boot.request(RadioTick(now)) else { return false; };
        let argument = request.argument.wire();
        let register = (argument >> 9) & 0x1ffff;
        if let Buffer::Fifo(bytes) = request.buffer {
            if argument & 0x8000_0000 != 0 {
                self.writes += 1;
                if register == 0x2c {
                    self.owed_credits += 1;
                    if bytes[5] == 0 { self.query = bytes[4]; }
                    if bytes[4] == 6 { self.power = Some(bytes[9]); }
                }
            } else {
                match register {
                    0 => bytes.copy_from_slice(&0x0020_5931u32.to_le_bytes()),
                    4 => bytes.copy_from_slice(&0x100u32.to_le_bytes()),
                    0x8c => bytes.copy_from_slice(&0xa800u32.to_le_bytes()),
                    0x88 => bytes.copy_from_slice(&0x100u32.to_le_bytes()),
                    0xc => bytes.copy_from_slice(&0xffff_ffffu32.to_le_bytes()),
                    0x50 => bytes.copy_from_slice(&((if self.query == 0xc1 { 20u32 } else { 28 }) << 16).to_le_bytes()),
                    0x34 => {
                        bytes.fill(0);
                        bytes[2] = 1;
                        bytes[4] = if self.query == 0xc1 { 9 } else { 2 };
                        if self.query == 0xc1 { bytes[8..14].copy_from_slice(&[2, 8, 0x22, 9, 10, 11]); }
                    }
                    0x24 => { bytes.fill(0); bytes[0] = self.owed_credits; self.owed_credits = 0; }
                    _ => bytes.fill(0),
                }
            }
        }
        boot.receive(completed(), RadioTick(now), FLAGS);
        true
    }
}

struct Registers { half: [u16; 32] }
impl Mmio for Registers {
    fn read_half(&mut self, register: HalfRegister) -> u16 { self.half[register as usize / 2] }
    fn write_half(&mut self, register: HalfRegister, value: u16) { self.half[register as usize / 2] = value; }
    fn read_word(&mut self, _: WordRegister) -> u32 { 0 }
    fn write_word(&mut self, _: WordRegister, _: u32) {}
    fn response_half(&mut self, _: u8) -> u16 { 0 }
}
struct Router { installed: bool, fail: bool }
impl InterruptRouter for Router {
    fn remove(&mut self) -> Result<(), ControlError> { self.installed = false; Ok(()) }
    fn install(&mut self) -> Result<(), ControlError> { if self.fail { Err(ControlError::Os) } else { self.installed = true; Ok(()) } }
}

#[test]
fn firmware_completion_and_commands_do_not_publish_ready_before_irq_binding() {
    let mut adapter = Adapter::<0, 16>::new();
    let hardware = hardware();
    let mut boot = RadioBoot::new(&[0x5a; 17], FirmwareAddress(0x100000), &mut adapter, &hardware, address(), PowerMode::FastSave, RadioTick(0)).unwrap();
    let mut device = RadioDevice::new();
    for now in 0..10000 {
        device.step(&mut boot, now);
        if boot.awaiting_interrupt_binding() { break; }
        assert_eq!(boot.outcome(), None);
    }
    assert!(boot.awaiting_interrupt_binding());
    assert_eq!(boot.outcome(), None);
    assert_eq!(device.power, Some(2));
    assert!(device.writes > 4);
    let mut control = ControlPlane::new();
    let mut host = Host::new(TransferUnit::BLOCK_512);
    let mut registers = Registers { half: [0; 32] };
    let mut router = Router { installed: false, fail: false };
    boot.bind_interrupt(&mut control, &mut host, &mut registers, &mut router).unwrap();
    assert!(router.installed);
    let Some(BootOutcome::Ready(ready)) = boot.outcome() else { panic!() };
    assert_eq!(ready.address, MacAddress([2, 3, 4, 5, 6, 7]));
    assert_eq!(ready.firmware_address, MacAddress([2, 8, 0x22, 9, 10, 11]));
    assert_eq!(ready.chip.get(), 0);
}

#[test]
fn physical_failure_and_late_completion_do_not_publish_readiness() {
    let mut adapter = Adapter::<0, 16>::new();
    let hardware = hardware();
    let mut boot = RadioBoot::new(&[1], FirmwareAddress(0), &mut adapter, &hardware, address(), PowerMode::Awake, RadioTick(0)).unwrap();
    drop(boot.request(RadioTick(0)).unwrap());
    boot.cancel();
    assert_eq!(boot.outcome(), Some(BootOutcome::Failed(BootError::Cancelled)));
    assert!(!boot.receive(completed(), RadioTick(0), FLAGS));
    assert!(boot.request(RadioTick(1)).is_none());
}

#[test]
fn successful_device_work_still_fails_when_interrupt_registration_fails() {
    let mut adapter = Adapter::<0, 16>::new();
    let hardware = hardware();
    let mut boot = RadioBoot::new(&[], FirmwareAddress(0), &mut adapter, &hardware, address(), PowerMode::Awake, RadioTick(0)).unwrap();
    let mut device = RadioDevice::new();
    for now in 0..10000 {
        device.step(&mut boot, now);
        if boot.awaiting_interrupt_binding() { break; }
    }
    assert!(boot.awaiting_interrupt_binding());
    let mut control = ControlPlane::new();
    let mut host = Host::new(TransferUnit::BLOCK_512);
    let mut registers = Registers { half: [0; 32] };
    let mut router = Router { installed: false, fail: true };
    assert_eq!(boot.bind_interrupt(&mut control, &mut host, &mut registers, &mut router), Err(BootError::Interrupt(ControlError::Os)));
    assert_eq!(boot.outcome(), Some(BootOutcome::Failed(BootError::Interrupt(ControlError::Os))));
}
