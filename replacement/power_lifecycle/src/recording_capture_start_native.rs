use crate::recording_capture_resources_native as resources;

const RECORDING: u32 = 0x0152_6a78;
fn word(address: u32) -> u32 { unsafe { core::ptr::read_volatile(address as *const u32) } }
fn put(address: u32, value: u32) { unsafe { core::ptr::write_volatile(address as *mut u32, value) }; }

extern "C" {
    fn xm1_power_record_helper_73964c(record: u32, settings: u32, outputs: u32, conversion: u32, burst: u32, option: u32, state: u32, config65c: u32, config678: u32, config_extra: u32);
    fn xm1_power_record_helper_738800(record: u32, settings: u32, work: u32);
    fn xm1_power_recording_capture_output_parameters(mode: u32, kind: u32, first: *mut u16, second: *mut u16, third: *mut u16);
    fn xm1_power_recording_capture_initialize_profile(destination: u32);
    fn xm1_power_recording_capture_snapshot_conversion();
    fn xm1_power_recording_capture_post_start(callback: u32, argument: u32, primary: u32, secondary: u32, limits: u32, profile: u32);
    fn xm1_power_recording_timer_initialize(owner: u32);
    fn xm1_power_configuration_byte(key: u32) -> u32;
}

#[derive(Clone, Copy)]
struct SettingsAddress(u32);
#[derive(Clone, Copy)]
struct WorkAddress(u32);

fn current_settings() -> (SettingsAddress, WorkAddress) {
    let settings = word(RECORDING + 0x81e4);
    if settings != 0 { return (SettingsAddress(settings), WorkAddress(word(RECORDING + 0x3540))); }
    let settings = 0x0153_317c;
    let work = 0x0152_ec74;
    xm1_power_record_helper_dee430(settings, work);
    (SettingsAddress(settings), WorkAddress(work))
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_dee430(settings: u32, work: u32) {
    crate::recording_capture_defaults::initialize(unsafe { &mut *(settings as *mut [u8; 0x3e8]) }, unsafe { &mut *(work as *mut [u8; 0x9f0]) });
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_72daac() -> u32 { word(0x0091_6614 + 0x2c4) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_72dafc() -> u32 { word(0x0091_6614 + 0x2e0) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_72db7c() -> u32 { word(0x0091_6614 + 0x2cc) }

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_737854(first: u32, second: u32) -> u32 {
    if second != 0 { 0 } else { match first { 0 => 1, 1 => 2, _ => 3 } }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_dd0e14(settings: u32, work: u32, outputs: u32, conversion: u32, option: u32, config65c: u32, config678: u32, config_extra: u32) {
    let handle = word(RECORDING + 0x3544);
    let record = resources::xm1_power_record_helper_eb276c(handle);
    unsafe { xm1_power_record_helper_73964c(record, settings, outputs, conversion, word(RECORDING + 0xd7e4), option, word(RECORDING + 0xc17c), config65c, config678, config_extra) };
    let auxiliary = resources::xm1_power_record_helper_eb34bc(word(RECORDING + 0x3544));
    unsafe { xm1_power_record_helper_738800(auxiliary, settings, work) };
    let selection = xm1_power_record_helper_737854(word(settings + 0xfc), word(settings + 0x100));
    put(word(RECORDING + 0x3544) + 0x1c, selection);
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_dd0e6c(work: u32) {
    let limit = if work == 0 { 0 } else { u32::from(unsafe { core::ptr::read_volatile((work + 0xea) as *const u16) }) };
    let descriptor = resources::xm1_power_record_helper_eb288c(word(RECORDING + 0x3544));
    xm1_power_record_helper_7389b8(descriptor, limit);
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_7389b8(descriptor: u32, limit: u32) {
    if word(descriptor) != limit { put(descriptor, limit); }
    if word(descriptor + 4) != 16 { put(descriptor + 4, 16); }
    let previous = word(descriptor + 8);
    if previous != unsafe { xm1_power_configuration_byte(0x254) } {
        put(descriptor + 8, unsafe { xm1_power_configuration_byte(0x254) });
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_capture_prepare_resource() {
    resources::xm1_power_record_helper_eb25d8();
    put(RECORDING + 0x3544, resources::xm1_power_record_helper_eb26ac(0));
    let (settings, work) = current_settings();
    let outputs = [0u32; 3];
    let option = word(RECORDING + 0x3d48);
    let config65c = xm1_power_record_helper_72daac();
    let config678 = xm1_power_record_helper_72dafc();
    let config_extra = xm1_power_record_helper_72db7c();
    xm1_power_record_helper_dd0e14(settings.0, work.0, outputs.as_ptr() as u32, 0, option, config65c, config678, config_extra);
    for offset in (0x3548..=0x3558).step_by(4) { put(RECORDING + offset, 0); }
}

#[no_mangle]
pub extern "C" fn xm1_power_recording_capture_start_resource(callback: extern "C" fn(u32)) {
    let (settings, work) = current_settings();
    for (kind, output) in [(0, RECORDING + 0x3562), (1, RECORDING + 0x3568)] {
        let mode = if word(settings.0 + 0x2c) == 9 { 2 } else { 1 };
        unsafe { xm1_power_recording_capture_output_parameters(mode, kind, output as *mut u16, (output - 2) as *mut u16, (output + 2) as *mut u16) };
    }
    let conversion = unsafe { core::ptr::read_volatile(0x0097_377dusize as *const u8) } as u32;
    let option = word(RECORDING + 0x3d48);
    let config65c = xm1_power_record_helper_72daac();
    let config678 = xm1_power_record_helper_72dafc();
    let config_extra = xm1_power_record_helper_72db7c();
    xm1_power_record_helper_dd0e14(settings.0, work.0, RECORDING + 0x3560, conversion, option, config65c, config678, config_extra);
    xm1_power_record_helper_dd0e6c(work.0);
    let profile = RECORDING + 0x9388;
    unsafe {
        xm1_power_recording_capture_initialize_profile(profile);
        xm1_power_recording_capture_snapshot_conversion();
    }
    let primary = resources::xm1_power_record_helper_eb2838(word(RECORDING + 0x3544));
    let secondary = resources::xm1_power_record_helper_eb35ec(word(RECORDING + 0x3544));
    let limits = resources::xm1_power_record_helper_eb2958(word(RECORDING + 0x3544));
    unsafe {
        xm1_power_recording_capture_post_start(callback as *const () as u32, 0, primary, secondary, limits, profile);
        xm1_power_recording_timer_initialize(word(settings.0 + 0x4c));
    }
}
