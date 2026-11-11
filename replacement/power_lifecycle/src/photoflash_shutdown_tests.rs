use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::vec::Vec;

const CONFIGURATION: Address = Address(0x1000);
const PAYLOAD: Address = Address(0x2000);
const COMPLETION: Callback = Callback(0x3000);
const CHARGE_COMPLETION: Callback = Callback(0x4000);

#[derive(Default)]
struct Device {
    bytes: BTreeMap<u32, u8>,
    software: BTreeSet<u32>,
    hardware: BTreeSet<u32>,
    callbacks: Vec<(Callback, Option<u32>, u32)>,
    stop_status: i32,
    delete_status: i32,
    cancel_status: i32,
    logs: Vec<Address>,
    irq_handles: BTreeSet<u32>,
    irq_release_status: i32,
    interrupt_state: u32,
    diagnostics: Vec<(u32, u32)>,
}
impl Runtime for Device {
    fn read8(&self, address: Address) -> u8 { self.bytes.get(&address.0).copied().unwrap_or(0) }
    fn read16(&self, address: Address) -> u16 { u16::from_le_bytes([self.read8(address), self.read8(at(address, 1))]) }
    fn read32(&self, address: Address) -> u32 { u32::from_le_bytes(core::array::from_fn(|i| self.read8(at(address, i as u32)))) }
    fn write8(&mut self, address: Address, value: u8) { self.bytes.insert(address.0, value); }
    fn write16(&mut self, address: Address, value: u16) { for (i, byte) in value.to_le_bytes().into_iter().enumerate() { self.write8(at(address, i as u32), byte); } }
    fn write32(&mut self, address: Address, value: u32) { for (i, byte) in value.to_le_bytes().into_iter().enumerate() { self.write8(at(address, i as u32), byte); } }
    fn cancel_software_timer(&mut self, handle: TimerHandle) -> i32 {
        if self.cancel_status == 0 { self.software.remove(&handle.0); }
        self.cancel_status
    }
    fn stop_device_timer(&mut self, _: TimerHandle) -> i32 { self.stop_status }
    fn delete_device_timer(&mut self, handle: TimerHandle) -> i32 {
        if self.delete_status == 0 { self.hardware.remove(&handle.0); }
        self.delete_status
    }
    fn invoke(&mut self, callback: Callback, argument: Option<Address>) {
        self.callbacks.push((callback, argument.map(|address| self.read32(address)), self.read32(CHARGE_OUTPUT)));
    }
    fn log_stop(&mut self, message: Address) { self.logs.push(message); }
}
fn device(step: ChargeStep) -> Device {
    let mut device = Device::default();
    reset(&mut device);
    device.write16(CHARGE_STEP, step as u16);
    device.write16(CONTEXT, 0x1a0c);
    device.write32(at(CONTEXT, 4), 4);
    device.write32(at(CONTEXT, 8), PAYLOAD.0);
    device.write32(PAYLOAD, COMPLETION.0);
    device.write32(at(CONFIGURATION, 4), 1);
    device.write32(at(CONFIGURATION, 8), 1);
    device.write32(CHARGE_INPUT, 4);
    device
}

#[test]
fn shutdown_completes_every_stock_controller_state() {
    for number in 0..13 {
        let step = ChargeStep::from_abi(number).unwrap();
        let mut device = device(step);
        assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Ok(()));
        assert_eq!(device.callbacks.last().map(|entry| entry.0), Some(COMPLETION));
        assert_eq!(device.read32(STATUS), 0);
        assert_eq!(device.read32(Address(device.read32(at(CONTEXT, 0x24)))), 0);
        if matches!(step, ChargeStep::Check | ChargeStep::Quick | ChargeStep::Record | ChargeStep::Calculation | ChargeStep::Light | ChargeStep::Trickle | ChargeStep::Jig | ChargeStep::JigDeferred) {
            assert_eq!(device.callbacks.last().unwrap().2, 0x10_0000);
        } else { assert_eq!(device.read32(CHARGE_OUTPUT), 0); }
    }
}
#[test]
fn active_charge_finishes_before_shutdown_callback_and_preserves_polarity() {
    for (high, input, expected) in [(1, 4, 0), (1, 0, 2), (0, 4, 2), (0, 0, 0)] {
        for step in [ChargeStep::Record, ChargeStep::Calculation] {
            let mut device = device(step);
            device.write32(at(CONFIGURATION, 4), high);
            device.write32(CHARGE_INPUT, input);
            device.write32(at(CONTEXT, 0x30), CHARGE_COMPLETION.0);
            device.write32(at(CONFIGURATION, 8), 0);
            assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Ok(()));
            assert_eq!(device.callbacks, [(CHARGE_COMPLETION, Some(expected), 0x10_0010), (COMPLETION, None, 0x10_0010)]);
            assert_eq!(device.read32(at(CONTEXT, 0x30)), 0);
        }
    }
}
#[test]
fn deferred_charge_finishes_without_stopping_physical_charge() {
    for step in [ChargeStep::RecordDeferred, ChargeStep::CalculationDeferred] {
        let mut device = device(step);
        device.write32(at(CONTEXT, 0x30), CHARGE_COMPLETION.0);
        assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Ok(()));
        assert_eq!(device.callbacks, [(CHARGE_COMPLETION, Some(2), 0), (COMPLETION, None, 0)]);
    }
}
#[test]
fn releases_only_the_active_charge_timer() {
    for (step, offset) in [(ChargeStep::Check, 0x34), (ChargeStep::Quick, 0x38), (ChargeStep::Record, 0x40), (ChargeStep::RecordDeferred, 0x40), (ChargeStep::Calculation, 0x44), (ChargeStep::CalculationDeferred, 0x44), (ChargeStep::Light, 0x4c), (ChargeStep::Stop, 0x48), (ChargeStep::Trickle, 0x3c), (ChargeStep::TrickleDeferred, 0x3c), (ChargeStep::Jig, 0x50), (ChargeStep::JigDeferred, 0x50)] {
        let mut device = device(step);
        device.software.extend([91, 92]);
        device.write32(at(CONTEXT, offset), 91);
        assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Ok(()));
        assert_eq!(device.software, BTreeSet::from([92]));
        assert_eq!(device.read32(at(CONTEXT, offset)), 0);
    }
}
#[test]
fn stop_error_retains_device_timer_but_does_not_suppress_completion() {
    for status in [0, -20, -11] {
        let mut device = device(ChargeStep::Quick);
        device.stop_status = status;
        device.hardware.insert(73);
        device.write32(at(STATUS, 0x2c), 73);
        assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Ok(()));
        assert_eq!(device.hardware.contains(&73), status != 0);
        assert_eq!(device.read32(at(STATUS, 0x2c)), if status == 0 { 0 } else { 73 });
        assert_eq!(device.callbacks.last().unwrap().0, COMPLETION);
    }
}
#[test]
fn cancellation_and_delete_errors_keep_stock_completion_semantics() {
    let mut device = device(ChargeStep::Quick);
    device.cancel_status = -20;
    device.delete_status = -20;
    device.write32(at(CONTEXT, 0x38), 91);
    device.write32(at(STATUS, 0x2c), 73);
    device.software.insert(91);
    device.hardware.insert(73);
    assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Ok(()));
    assert_eq!(device.read32(at(CONTEXT, 0x38)), 0);
    assert_eq!(device.read32(at(STATUS, 0x2c)), 0);
    assert!(device.hardware.contains(&73));
    assert!(device.software.contains(&91));
    assert_eq!(device.callbacks.last().unwrap().0, COMPLETION);
}
#[test]
fn receive_failure_notifies_task_callback_without_consuming_it() {
    let mut device = device(ChargeStep::Idle);
    device.write32(at(CONTEXT, 0x60), CHARGE_COMPLETION.0);
    receive_failure(&mut device);
    receive_failure(&mut device);
    assert_eq!(device.callbacks, [(CHARGE_COMPLETION, None, 0), (CHARGE_COMPLETION, None, 0)]);
    assert_eq!(device.read32(at(CONTEXT, 0x60)), CHARGE_COMPLETION.0);
}
#[test]
fn unsupported_commands_and_invalid_steps_never_produce_completion() {
    let mut device = device(ChargeStep::Idle);
    device.write16(CONTEXT, 0x1a18);
    assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Err(DispatchError::DifferentCommand));
    device.write16(CONTEXT, 0x1a0c);
    device.write16(CHARGE_STEP, 13);
    assert_eq!(dispatch_shutdown(&mut device, CONFIGURATION), Err(DispatchError::InvalidChargeStep));
    assert!(device.callbacks.is_empty());
}

#[path = "photoflash_shutdown_timer_tests.rs"]
mod timer_tests;
