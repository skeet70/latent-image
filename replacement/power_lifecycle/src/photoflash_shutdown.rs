#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Address(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TimerHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Callback(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ChargeStep {
    Idle = 0, Check = 1, Quick = 2, Record = 3, RecordDeferred = 4,
    Calculation = 5, CalculationDeferred = 6, Light = 7, Stop = 8,
    Trickle = 9, TrickleDeferred = 10, Jig = 11, JigDeferred = 12,
}
impl ChargeStep {
    pub const fn from_abi(value: u16) -> Option<Self> {
        Some(match value {
            0 => Self::Idle, 1 => Self::Check, 2 => Self::Quick, 3 => Self::Record,
            4 => Self::RecordDeferred, 5 => Self::Calculation, 6 => Self::CalculationDeferred,
            7 => Self::Light, 8 => Self::Stop, 9 => Self::Trickle,
            10 => Self::TrickleDeferred, 11 => Self::Jig, 12 => Self::JigDeferred,
            _ => return None,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchError { DifferentCommand, InvalidChargeStep }

pub const CONTEXT: Address = Address(0x0091_7e64);
pub const STATUS: Address = Address(0x4089_ea5c);
pub const CHARGE_STEP: Address = Address(0x0096_b56c);
pub const EVENT_SELECTION: Address = Address(0x0095_6750);
const CHARGE_INPUT: Address = Address(0xfff8_91d4);
const CHARGE_OUTPUT: Address = Address(0xfff8_91c0);

pub trait Runtime {
    fn read8(&self, address: Address) -> u8;
    fn read16(&self, address: Address) -> u16;
    fn read32(&self, address: Address) -> u32;
    fn write8(&mut self, address: Address, value: u8);
    fn write16(&mut self, address: Address, value: u16);
    fn write32(&mut self, address: Address, value: u32);
    fn cancel_software_timer(&mut self, handle: TimerHandle) -> i32;
    fn stop_device_timer(&mut self, handle: TimerHandle) -> i32;
    fn delete_device_timer(&mut self, handle: TimerHandle) -> i32;
    fn invoke(&mut self, callback: Callback, argument: Option<Address>);
    fn log_stop(&mut self, message: Address);
}
const fn at(base: Address, offset: u32) -> Address { Address(base.0 + offset) }

pub fn reset(runtime: &mut impl Runtime) {
    for offset in [0x34, 0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0x50, 0x54, 0x58, 0x64] {
        runtime.write32(at(CONTEXT, offset), 0);
    }
    runtime.write32(at(CONTEXT, 4), 12);
    runtime.write32(at(CONTEXT, 8), at(CONTEXT, 12).0);
    runtime.write32(at(CONTEXT, 0x20), 12);
    runtime.write32(at(CONTEXT, 0x24), at(CONTEXT, 0x28).0);
    for offset in [0, 4, 8, 0x18, 0x1c, 0x20, 0x2c] { runtime.write32(at(STATUS, offset), 0); }
    runtime.write32(at(STATUS, 12), 2);
    runtime.write32(at(STATUS, 0x14), 4);
    runtime.write16(CHARGE_STEP, 0);
}

pub fn receive_failure(runtime: &mut impl Runtime) {
    let callback = Callback(runtime.read32(at(CONTEXT, 0x60)));
    if callback.0 != 0 { runtime.invoke(callback, None); }
}

fn cancel(runtime: &mut impl Runtime, offset: u32, skip_zero: bool) {
    let slot = at(CONTEXT, offset);
    let handle = TimerHandle(runtime.read32(slot));
    if !skip_zero || handle.0 != 0 {
        let _ = runtime.cancel_software_timer(handle);
        runtime.write32(slot, 0);
    }
}
fn stop(runtime: &mut impl Runtime, configuration: Address, message: Address, sample: bool) -> u32 {
    let status = if sample {
        let high = runtime.read32(at(configuration, 4)) != 0;
        let input = runtime.read32(CHARGE_INPUT) & 4 != 0;
        if input == high { 0 } else { 2 }
    } else { 2 };
    let inactive = if runtime.read32(at(configuration, 8)) != 0 { 0x0010_0000 } else { 0x0010_0010 };
    runtime.write32(CHARGE_OUTPUT, inactive);
    runtime.write32(Address(0x008e_1f30), 0);
    let handle = TimerHandle(runtime.read32(at(STATUS, 0x2c)));
    if handle.0 != 0 && runtime.stop_device_timer(handle) == 0 {
        let _ = runtime.delete_device_timer(handle);
        runtime.write32(at(STATUS, 0x2c), 0);
    }
    runtime.log_stop(message);
    status
}
fn invoke_saved(runtime: &mut impl Runtime, offset: u32, argument: Option<Address>) {
    let payload = Address(runtime.read32(at(CONTEXT, 0x24)));
    let callback = Callback(runtime.read32(at(payload, offset)));
    if callback.0 != 0 {
        runtime.invoke(callback, argument);
        let payload = Address(runtime.read32(at(CONTEXT, 0x24)));
        runtime.write32(at(payload, offset), 0);
    }
}
fn save_request(runtime: &mut impl Runtime) {
    runtime.write16(at(CONTEXT, 0x1c), runtime.read16(CONTEXT));
    runtime.write8(at(CONTEXT, 0x1e), runtime.read8(at(CONTEXT, 2)));
    runtime.write8(at(CONTEXT, 0x1f), runtime.read8(at(CONTEXT, 3)));
    let length = runtime.read32(at(CONTEXT, 4));
    runtime.write32(at(CONTEXT, 0x20), length);
    let source = Address(runtime.read32(at(CONTEXT, 8)));
    let destination = Address(runtime.read32(at(CONTEXT, 0x24)));
    for offset in 0..length { runtime.write8(at(destination, offset), runtime.read8(at(source, offset))); }
}

/// Enters the saved completion word as `extern "C" fn() -> i32`, whose status stock discards.
pub fn invoke_completion(
    registry: &crate::callback_registry::CallbackRegistry,
    callback: Callback,
) -> Result<(), crate::callback_registry::CallbackError> {
    crate::shutdown_requests::invoke_status_completion(registry, crate::callback_registry::CallbackWord(callback.0 as usize))
}

pub fn dispatch_shutdown(runtime: &mut impl Runtime, configuration: Address) -> Result<(), DispatchError> {
    if runtime.read16(CONTEXT) != 0x1a0c { return Err(DispatchError::DifferentCommand); }
    let step = ChargeStep::from_abi(runtime.read16(CHARGE_STEP)).ok_or(DispatchError::InvalidChargeStep)?;
    runtime.write16(EVENT_SELECTION, 0);
    match step {
        ChargeStep::Idle => {},
        ChargeStep::Check => {
            let _ = stop(runtime, configuration, Address(0x0078_1b28), true);
            cancel(runtime, 0x34, false);
        },
        ChargeStep::Quick => {
            stop(runtime, configuration, Address(0x0078_1a70), false);
            cancel(runtime, 0x38, false);
        },
        ChargeStep::Record | ChargeStep::Calculation => {
            let message = if step == ChargeStep::Record { Address(0x0078_1ab0) } else { Address(0x0078_1acc) };
            let status = stop(runtime, configuration, message, true);
            cancel(runtime, if step == ChargeStep::Record { 0x40 } else { 0x44 }, true);
            runtime.write32(at(CONTEXT, 0x18), status);
            invoke_saved(runtime, 8, Some(at(CONTEXT, 0x18)));
        },
        ChargeStep::RecordDeferred | ChargeStep::CalculationDeferred => {
            cancel(runtime, if step == ChargeStep::RecordDeferred { 0x40 } else { 0x44 }, true);
            runtime.write32(at(CONTEXT, 0x18), 2);
            invoke_saved(runtime, 8, Some(at(CONTEXT, 0x18)));
        },
        ChargeStep::Light => {
            stop(runtime, configuration, Address(0x0078_1aec), false);
            cancel(runtime, 0x4c, false);
        },
        ChargeStep::Stop => cancel(runtime, 0x48, false),
        ChargeStep::Trickle | ChargeStep::TrickleDeferred => {
            if step == ChargeStep::Trickle { stop(runtime, configuration, Address(0x0078_1a90), false); }
            cancel(runtime, 0x3c, false);
            runtime.write32(at(STATUS, 4), 0);
        },
        ChargeStep::Jig | ChargeStep::JigDeferred => {
            stop(runtime, configuration, Address(0x0078_1b0c), false);
            cancel(runtime, 0x50, false);
        },
    }
    save_request(runtime);
    invoke_saved(runtime, 0, None);
    if step != ChargeStep::Idle { runtime.write16(CHARGE_STEP, 0); }
    runtime.write32(STATUS, u32::from(runtime.read16(CHARGE_STEP)));
    Ok(())
}

#[path = "photoflash_shutdown_timer.rs"]
pub mod timer;

#[cfg(target_arch = "arm")]
#[path = "photoflash_shutdown_native.rs"]
mod native;
#[cfg(test)]
#[path = "photoflash_shutdown_tests.rs"]
mod tests;
