use xm1_power_services::{charge_timers::{ChargeTransport, DelayMicros}, serial_driver::StatusResidue};

pub trait Hardware: ChargeTransport {
    fn read_word(&mut self, handle: xm1_power_services::charge_timers::SerialHandle) -> u16;
    fn set_receive_clock(&mut self, value: u32);
    fn set_receive_direction(&mut self, value: u32);
}

pub fn write(hardware: &mut impl Hardware, packet: &[u8]) -> i32 {
    xm1_power_services::charge_timers::transmit_charge(hardware, packet).0
}

pub fn read(hardware: &mut impl Hardware, command: u8, output: &mut [u8]) -> i32 {
    let opened = hardware.open_charge_serial();
    hardware.write_charge_pin(0x0004_0004);
    let result = match opened {
        Err(error) => error.0,
        Ok(handle) => {
            let (_, residue) = hardware.configure(handle);
            hardware.send_byte(handle, command, residue);
            let available = loop {
                let (_, status) = hardware.status(handle);
                if status.state == 1 && status.available != 0 { break status.available; }
            };
            for _ in 0..available { hardware.read_word(handle); }
            if let Some(first) = output.first_mut() { *first = command; }
            for _ in 0..4 { hardware.delay(DelayMicros(200)); }
            let mut residue = StatusResidue(0x0001_0000);
            hardware.set_receive_clock(0x0003_0000);
            hardware.set_receive_direction(0x0001_0000);
            let count = output.len();
            for (index, byte) in output.iter_mut().enumerate().skip(1) {
                hardware.send_byte(handle, 0xaa, residue);
                loop { let (_, status) = hardware.status(handle); if status.available != 0 { break; } }
                *byte = hardware.read_word(handle) as u8;
                if index + 1 < count { for _ in 0..4 { residue = StatusResidue(hardware.delay(DelayMicros(200)).0); } }
            }
            hardware.set_receive_direction(0x0001_0001);
            hardware.set_receive_clock(0x0003_0001);
            hardware.delay(DelayMicros(40));
            hardware.stop(handle);
            hardware.close(handle).0
        }
    };
    hardware.write_charge_pin(0x0004_0000);
    result
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use xm1_power_services::{charge_timers::{SerialHandle, SerialStatus}, primitives::ServiceStatus, osal::TimerReload};
    unsafe extern "C" {
        fn xm1_power_serial_open(port: u32, mode: u32, output: *mut u32) -> i32;
        fn xm1_power_serial_configure(handle: u32, value: u32) -> i32;
        fn xm1_power_serial_write_with_residue(handle: u32, word: u32, residue: u32) -> i32;
        fn xm1_power_serial_status(handle: u32, output: *mut u32) -> i32;
        fn xm1_power_serial_read(handle: u32, output: *mut u16) -> i32;
        fn xm1_power_serial_stop(handle: u32) -> i32;
        fn xm1_power_serial_close(handle: u32) -> i32;
        fn xm1_power_delay_us_with_residue(duration: u32) -> u32;
    }
    pub struct NativeHardware;
    impl ChargeTransport for NativeHardware {
        fn open_charge_serial(&mut self) -> Result<SerialHandle, ServiceStatus> {
            let mut handle = 0;
            let status = unsafe { xm1_power_serial_open(10, 2, &mut handle) };
            if status == 0 { Ok(SerialHandle(handle)) } else { Err(ServiceStatus(status)) }
        }
        fn write_charge_pin(&mut self, value: u32) { unsafe { core::ptr::write_volatile(0xfff8_92c4 as *mut u32, value); } }
        fn configure(&mut self, handle: SerialHandle) -> (ServiceStatus, StatusResidue) {
            let status = unsafe { xm1_power_serial_configure(handle.0, 100) };
            (ServiceStatus(status), StatusResidue(if status == 0 { 0 } else { 0x15 }))
        }
        fn send_byte(&mut self, handle: SerialHandle, byte: u8, residue: StatusResidue) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_write_with_residue(handle.0, u32::from(byte), residue.0) }) }
        fn status(&mut self, handle: SerialHandle) -> (ServiceStatus, SerialStatus) {
            let mut words = [0; 7];
            let status = unsafe { xm1_power_serial_status(handle.0, words.as_mut_ptr()) };
            (ServiceStatus(status), SerialStatus { state: words[1], available: words[3] })
        }
        fn read_response(&mut self, handle: SerialHandle) -> ServiceStatus {
            let mut word = 0;
            ServiceStatus(unsafe { xm1_power_serial_read(handle.0, &mut word) })
        }
        fn delay(&mut self, duration: DelayMicros) -> TimerReload { TimerReload(unsafe { xm1_power_delay_us_with_residue(duration.0) }) }
        fn stop(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_stop(handle.0) }) }
        fn close(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_close(handle.0) }) }
    }
    impl Hardware for NativeHardware {
        fn read_word(&mut self, handle: SerialHandle) -> u16 { let mut word = 0; unsafe { xm1_power_serial_read(handle.0, &mut word); } word }
        fn set_receive_clock(&mut self, value: u32) { unsafe { core::ptr::write_volatile(0xfff8_934c as *mut u32, value); } }
        fn set_receive_direction(&mut self, value: u32) { unsafe { core::ptr::write_volatile(0xfff8_9344 as *mut u32, value); } }
    }
}
#[cfg(target_arch = "arm")]
pub use native::NativeHardware;

#[cfg(target_arch = "arm")]
#[no_mangle]
/// # Safety
/// `output` exclusively maps `length` writable bytes for the duration of the call.
pub unsafe extern "C" fn xm1_vendor_device_serial_read(command: u32, output: *mut u8, length: u32) -> i32 {
    let Ok(command) = u8::try_from(command) else { return -255; };
    if length == 0 { return read(&mut NativeHardware, command, &mut []); }
    if output.is_null() || length > isize::MAX as u32 || (output as usize).checked_add(length as usize).is_none() { return -255; }
    let output = unsafe { core::slice::from_raw_parts_mut(output, length as usize) };
    read(&mut NativeHardware, command, output)
}

#[cfg(target_arch = "arm")]
#[no_mangle]
/// # Safety
/// `packet` maps `length` readable bytes for the duration of the call.
pub unsafe extern "C" fn xm1_vendor_device_serial_write(packet: *const u8, length: u32) -> i32 {
    if length == 0 { return write(&mut NativeHardware, &[]); }
    if packet.is_null() || length > isize::MAX as u32 || (packet as usize).checked_add(length as usize).is_none() { return -255; }
    let packet = unsafe { core::slice::from_raw_parts(packet, length as usize) };
    write(&mut NativeHardware, packet)
}
