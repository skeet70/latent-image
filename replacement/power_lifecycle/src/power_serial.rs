use crate::primitives::ServiceStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerPin(u8);
impl PowerPin {
    pub const fn from_abi(index: u32) -> Option<Self> { if index <= 0x22 { Some(Self(index as u8)) } else { None } }
    pub const fn index(self) -> u32 { self.0 as u32 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldWriteMode { WholeRegister, Masked }
impl FieldWriteMode { pub const fn from_abi(mode: u32) -> Self { if mode == 1 { Self::Masked } else { Self::WholeRegister } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SerialCommand(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SerialHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct FifoControlResidue(pub u32);
impl FifoControlResidue {
    pub const fn after_configure(status: ServiceStatus) -> Self { if status.0 == 0 { Self(0) } else { Self(0x16) } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerialStatus { pub fifo_flag9: u32, pub transmit_threshold: u32 }
impl SerialStatus { pub const fn complete(self) -> bool { self.transmit_threshold != 0 && self.fifo_flag9 == 1 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialObservation { Received(u8), NotOpened(ServiceStatus), Failed(ServiceStatus) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldWrite { pub status: ServiceStatus, pub observation: SerialObservation }

pub trait SerialController {
    fn open_power_endpoint(&mut self) -> Result<SerialHandle, ServiceStatus>;
    fn start(&mut self, handle: SerialHandle) -> ServiceStatus;
    fn command(&mut self, handle: SerialHandle, command: SerialCommand, residue: FifoControlResidue) -> ServiceStatus;
    fn poll(&mut self, handle: SerialHandle) -> Result<SerialStatus, ServiceStatus>;
    fn read(&mut self, handle: SerialHandle) -> Result<u16, ServiceStatus>;
    fn stop(&mut self, handle: SerialHandle) -> ServiceStatus;
    fn close(&mut self, handle: SerialHandle) -> ServiceStatus;
}

fn transfer(controller: &mut impl SerialController, handle: SerialHandle, command: SerialCommand) -> SerialObservation {
    let configured = controller.start(handle);
    let _ = controller.command(handle, command, FifoControlResidue::after_configure(configured));
    loop {
        match controller.poll(handle) {
            Ok(status) if status.complete() => break,
            Ok(_) => (),
            Err(status) => return SerialObservation::Failed(status),
        }
    }
    let output = match controller.read(handle) { Ok(value) => SerialObservation::Received(value as u8), Err(status) => SerialObservation::Failed(status) };
    let _ = controller.stop(handle);
    output
}

pub fn serial_byte(controller: &mut impl SerialController, command: SerialCommand) -> SerialObservation {
    let handle = match controller.open_power_endpoint() { Ok(handle) => handle, Err(status) => return SerialObservation::NotOpened(status) };
    let result = transfer(controller, handle, command);
    let _ = controller.close(handle);
    result
}

pub fn serial_mask(controller: &mut impl SerialController, command: SerialCommand, mask: u8) -> SerialObservation {
    if command.0 & 0x8000 == 0 {
        return match serial_byte(controller, SerialCommand(command.0 & 0x7f00)) {
            SerialObservation::Received(value) => SerialObservation::Received(value & mask),
            other => other,
        };
    }
    if mask == 0xff { return serial_byte(controller, command); }
    let handle = match controller.open_power_endpoint() { Ok(handle) => handle, Err(status) => return SerialObservation::NotOpened(status) };
    let old = transfer(controller, handle, SerialCommand(command.0 & 0x7f00));
    let result = match old {
        SerialObservation::Received(old) => {
            let merged = (u16::from(old) & !u16::from(mask)) | (command.0 & (0xff00 | u16::from(mask)));
            transfer(controller, handle, SerialCommand(merged))
        }
        other => other,
    };
    let _ = controller.close(handle);
    result
}

#[derive(Clone, Copy)]
struct PinDescriptor { command: SerialCommand, shift: u8, width: u8 }
const fn descriptor(command: u16, shift: u8, width: u8) -> PinDescriptor { PinDescriptor { command: SerialCommand(command), shift, width } }
const DESCRIPTORS: [PinDescriptor; 35] = [
    descriptor(0xc900,0,4), descriptor(0xca00,0,4), descriptor(0xcb00,4,4), descriptor(0xcc00,0,4), descriptor(0xce00,0,2),
    descriptor(0x6000,0,8), descriptor(0x6000,4,4), descriptor(0x6000,2,1), descriptor(0x6000,0,2),
    descriptor(0x4100,0,8), descriptor(0x4200,0,8), descriptor(0x4300,0,8), descriptor(0x4400,0,8), descriptor(0x4500,0,1), descriptor(0x4600,0,1),
    descriptor(0x5000,0,8), descriptor(0x5100,0,8), descriptor(0x5200,0,8), descriptor(0x5300,0,8), descriptor(0x5400,0,8), descriptor(0x5500,0,8),
    descriptor(0x5600,0,8), descriptor(0x5700,0,8), descriptor(0x5800,0,8), descriptor(0x5900,0,8), descriptor(0x5a00,0,8), descriptor(0x5b00,0,8),
    descriptor(0x5c00,0,8), descriptor(0x5d00,0,8), descriptor(0x5e00,0,8), descriptor(0x5f00,0,8), descriptor(0x6900,0,8), descriptor(0x6a00,0,8), descriptor(0x5800,0,8), descriptor(0x6800,0,1),
];

pub fn write_field(controller: &mut impl SerialController, pin: PowerPin, value: u32, mode: FieldWriteMode) -> FieldWrite {
    let descriptor = DESCRIPTORS[pin.index() as usize];
    let (value, mask) = match mode {
        FieldWriteMode::WholeRegister => (value as u16, 0xff),
        FieldWriteMode::Masked => (((value << descriptor.shift) & 0xff) as u16, (((1u16 << descriptor.width) - 1) << descriptor.shift) as u8),
    };
    let observation = serial_mask(controller, SerialCommand(descriptor.command.0 | 0x8000 | value), mask);
    FieldWrite { status: ServiceStatus(0), observation }
}

#[cfg(test)]
#[path = "power_serial_tests.rs"]
mod tests;

#[cfg(target_arch = "arm")]
pub struct NativeSerialController;
#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_serial_open(descriptor: u32, mode: u32, out: *mut u32) -> i32;
    fn xm1_power_serial_configure(handle: u32, delay: u32) -> i32;
    fn xm1_power_serial_write_with_residue(handle: u32, command: u32, residue: u32) -> i32;
    fn xm1_power_serial_status(handle: u32, out: *mut u32) -> i32;
    fn xm1_power_serial_read(handle: u32, out: *mut u16) -> i32;
    fn xm1_power_serial_stop(handle: u32) -> i32;
    fn xm1_power_serial_close(handle: u32) -> i32;
}
#[cfg(target_arch = "arm")]
impl SerialController for NativeSerialController {
    fn open_power_endpoint(&mut self) -> Result<SerialHandle, ServiceStatus> {
        let mut out = core::mem::MaybeUninit::<u32>::uninit();
        let status = unsafe { xm1_power_serial_open(9, 2, out.as_mut_ptr()) };
        if status == 0 { Ok(SerialHandle(unsafe { out.assume_init() })) } else { Err(ServiceStatus(status)) }
    }
    fn start(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_configure(handle.0, 1) }) }
    fn command(&mut self, handle: SerialHandle, command: SerialCommand, residue: FifoControlResidue) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_write_with_residue(handle.0, u32::from(command.0), residue.0) }) }
    fn poll(&mut self, handle: SerialHandle) -> Result<SerialStatus, ServiceStatus> {
        let mut out = core::mem::MaybeUninit::<[u32; 7]>::uninit();
        let status = unsafe { xm1_power_serial_status(handle.0, out.as_mut_ptr().cast()) };
        if status != 0 { return Err(ServiceStatus(status)); }
        let out = unsafe { out.assume_init() };
        Ok(SerialStatus { fifo_flag9: out[1], transmit_threshold: out[3] })
    }
    fn read(&mut self, handle: SerialHandle) -> Result<u16, ServiceStatus> {
        let mut out = core::mem::MaybeUninit::<u16>::uninit();
        let status = unsafe { xm1_power_serial_read(handle.0, out.as_mut_ptr()) };
        if status == 0 { Ok(unsafe { out.assume_init() }) } else { Err(ServiceStatus(status)) }
    }
    fn stop(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_stop(handle.0) }) }
    fn close(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_close(handle.0) }) }
}

pub trait PowerConfigurationSource { fn read(&mut self, field: crate::primitives::PowerConfiguration) -> u8; }
pub struct SerialTerminalPower<C, S> { pub controller: C, pub configuration: S }
impl<C: SerialController, S: PowerConfigurationSource> crate::primitives::TerminalPower for SerialTerminalPower<C, S> {
    fn read_configuration(&mut self, field: crate::primitives::PowerConfiguration) -> u8 { self.configuration.read(field) }
    fn write_field(&mut self, field: crate::primitives::PowerField, value: u8) -> ServiceStatus {
        let pin = PowerPin(field.index() as u8);
        write_field(&mut self.controller, pin, u32::from(value), FieldWriteMode::WholeRegister).status
    }
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_backup_write_field(index: u32, value: u32, mode: u32) -> i32 {
    let Some(pin) = PowerPin::from_abi(index) else { return -30; };
    write_field(&mut NativeSerialController, pin, value, FieldWriteMode::from_abi(mode)).status.0
}

#[cfg(target_arch = "arm")]
pub struct NativePowerConfigurationSource;
#[cfg(target_arch = "arm")]
extern "C" { fn xm1_power_configuration_byte(offset: u32) -> u32; }
#[cfg(target_arch = "arm")]
impl PowerConfigurationSource for NativePowerConfigurationSource {
    fn read(&mut self, field: crate::primitives::PowerConfiguration) -> u8 { unsafe { xm1_power_configuration_byte(field.offset()) as u8 } }
}
