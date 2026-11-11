use crate::recording_capture_setup::{self as setup, CaptureOutputKind, CaptureOutputMode, CaptureOutputParameters, CaptureOutputSource, CaptureParameterBank, ConfigurationByteKey};

core::arch::global_asm!(include_str!("recording_capture_setup_residue.S"), options(raw));

extern "C" {
    fn xm1_power_configuration_byte(offset: u32) -> u32;
    fn xm1_calibration_i16(offset: u32) -> i32;
    fn xm1_power_record_helper_ef9a94(command: u32, payload: u32, length: u32);
}

struct NativeOutputSource;
impl CaptureOutputSource for NativeOutputSource {
    fn configuration_byte(&mut self, key: ConfigurationByteKey) -> u8 { unsafe { xm1_power_configuration_byte(key.0) as u8 } }
    fn parameter_bank(&mut self, bank: CaptureParameterBank) -> CaptureOutputParameters {
        let address = 0x409c_2944 + bank as u32 * 6;
        let half = |offset| unsafe { core::ptr::read_volatile((address + offset) as *const u16) };
        CaptureOutputParameters { first: half(0), second: half(2), third: half(4) }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_recording_capture_output_parameters(mode: u32, kind: u32, first: *mut u16, second: *mut u16, third: *mut u16) {
    if unsafe { xm1_power_configuration_byte(0xa902) } == 2 && mode == 1 {
        for (destination, high, low) in [(first, 0xa904, 0xa903), (second, 0xa906, 0xa905), (third, 0xa908, 0xa907)] {
            if !destination.is_null() {
                let high = unsafe { xm1_power_configuration_byte(high) };
                let low = unsafe { xm1_power_configuration_byte(low) };
                unsafe { core::ptr::write_volatile(destination, (low | (high << 8)) as u16) };
            }
        }
        return;
    }
    let mode = if mode == 1 { CaptureOutputMode::Still } else { CaptureOutputMode::Movie };
    let kind = if kind == 0 { CaptureOutputKind::Primary } else { CaptureOutputKind::Secondary };
    let bank = match (mode, kind) {
        (CaptureOutputMode::Still, CaptureOutputKind::Primary) => CaptureParameterBank::StillPrimary,
        (CaptureOutputMode::Still, CaptureOutputKind::Secondary) => CaptureParameterBank::StillSecondary,
        (CaptureOutputMode::Movie, CaptureOutputKind::Primary) => CaptureParameterBank::MoviePrimary,
        (CaptureOutputMode::Movie, CaptureOutputKind::Secondary) => CaptureParameterBank::MovieSecondary,
    };
    let values = NativeOutputSource.parameter_bank(bank);
    unsafe {
        core::ptr::write_volatile(first, values.first);
        core::ptr::write_volatile(second, values.second);
        core::ptr::write_volatile(third, values.third);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_recording_capture_initialize_profile_with_residue(destination: *mut [u8; 0x28], residue: u32) {
    setup::initialize_capture_profile(unsafe { &mut *destination }, residue);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_recording_capture_post_start_with_residue(callback: u32, argument: u32, primary: *mut [u8; 0x90], secondary: u32, limits: u32, profile: u32, residue: u32) {
    let mut payload = [0; 0xa8];
    payload[0xa4..].copy_from_slice(&residue.to_le_bytes());
    let primary = unsafe { &mut *primary };
    let special = if u16::from_le_bytes([primary[0x30], primary[0x31]]) == 0x1800 { unsafe { xm1_calibration_i16(0x6904) as i16 } } else { 0 };
    setup::prepare_capture_packet(primary, &mut payload, callback, argument, secondary, limits, profile, special);
    unsafe { xm1_power_record_helper_ef9a94(0, payload.as_ptr() as u32, 0xa8) };
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_capture_snapshot_conversion() {
    let value = unsafe { core::ptr::read_volatile(0x0097_377dusize as *const u8) };
    unsafe { core::ptr::write_volatile(0x0097_377eusize as *mut u8, value) };
}
