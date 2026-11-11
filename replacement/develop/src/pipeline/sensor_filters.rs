use super::shared::Address32;
use super::sensor_backend::{mode_record, SensorConfiguration, SensorMode, SensorRequest};
use super::sensor_requests::{copy, store, word};

unsafe extern "C" {
    fn xm1_pipeline_sensor_filter_primary_enable(mode: u32, first: u32, second: u32);
    fn xm1_pipeline_sensor_filter_primary_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_filter_second_enable(enabled: u32);
    fn xm1_pipeline_sensor_filter_second_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_filter_third_enable(enabled: u32);
    fn xm1_pipeline_sensor_filter_third_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_filter_fourth_enable(enabled: u32);
    fn xm1_pipeline_sensor_filter_fourth_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_filter_fifth_enable(enabled: u32);
    fn xm1_pipeline_sensor_filter_fifth_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_filter_lookup_enable(enabled: u32, length: u32);
    fn xm1_pipeline_sensor_filter_lookup_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_filter_large_enable(enabled: u32, length: u32);
    fn xm1_pipeline_sensor_filter_large_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_filter_routing(mode: u32);
    fn xm1_pipeline_sensor_runtime_first_enable(enabled: u32);
    fn xm1_pipeline_sensor_runtime_first_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_runtime_second_enable(enabled: u32);
    fn xm1_pipeline_sensor_runtime_second_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_runtime_third_enable(enabled: u32);
    fn xm1_pipeline_sensor_runtime_third_parameters(parameters: Address32);
    fn xm1_pipeline_sensor_runtime_commit(first: u32, second: u32);
    fn xm1_pipeline_sensor_device_center(vertical: u32, horizontal: u32);
    fn xm1_pipeline_sensor_device_secondary_center(vertical: u32, horizontal: u32);
    fn xm1_pipeline_sensor_program_configuration(request: SensorRequest, configuration: SensorConfiguration) -> i32;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configure_filters(request: SensorRequest, configuration: SensorConfiguration) -> i32 {
    let record = mode_record(SensorMode(unsafe { word(request.0, 0) }));
    let lookup = Address32(0x14c7c50);
    let large = Address32(0x14c8940);
    unsafe { copy(lookup, Address32(word(request.0, 0xd8)), 0xc68); copy(large, Address32(word(request.0, 0xdc)), 0x5424); }
    let primary = Address32(unsafe { word(request.0, 0xc4) });
    if primary.0 != 0 {
        unsafe { xm1_pipeline_sensor_filter_primary_enable(word(record, 12), word(primary, 0), word(primary, 4)); xm1_pipeline_sensor_filter_primary_parameters(primary); }
    } else { unsafe { xm1_pipeline_sensor_filter_primary_enable(0, 0, 0) }; }
    let second = Address32(unsafe { word(request.0, 0xc8) });
    if second.0 != 0 { unsafe { xm1_pipeline_sensor_filter_second_enable(word(second, 0)); xm1_pipeline_sensor_filter_second_parameters(second); } }
    else { unsafe { xm1_pipeline_sensor_filter_second_enable(0) }; }
    let third = Address32(unsafe { word(request.0, 0xcc) });
    if third.0 != 0 { unsafe { xm1_pipeline_sensor_filter_third_enable(1); xm1_pipeline_sensor_filter_third_parameters(third); } }
    else { unsafe { xm1_pipeline_sensor_filter_third_enable(0) }; }
    let fourth = Address32(unsafe { word(request.0, 0xd0) });
    if fourth.0 != 0 { unsafe { xm1_pipeline_sensor_filter_fourth_enable(word(fourth, 0)); xm1_pipeline_sensor_filter_fourth_parameters(fourth); } }
    else { unsafe { xm1_pipeline_sensor_filter_fourth_enable(0) }; }
    let fifth = Address32(unsafe { word(request.0, 0xd4) });
    if fifth.0 != 0 { unsafe { xm1_pipeline_sensor_filter_fifth_enable(word(fifth, 0)); xm1_pipeline_sensor_filter_fifth_parameters(fifth); } }
    else { unsafe { xm1_pipeline_sensor_filter_fifth_enable(0) }; }
    if unsafe { word(request.0, 0xd8) } != 0 {
        unsafe { store(lookup, 8, word(configuration.0, 0x2c)); store(lookup, 12, word(configuration.0, 0x30)); xm1_pipeline_sensor_filter_lookup_parameters(lookup); xm1_pipeline_sensor_filter_lookup_enable(1, word(lookup, 4)); }
    } else { unsafe { xm1_pipeline_sensor_filter_lookup_enable(0, 0) }; }
    if unsafe { word(request.0, 0xdc) } != 0 {
        unsafe { store(large, 8, word(configuration.0, 0x2c)); store(large, 12, word(configuration.0, 0x30)); xm1_pipeline_sensor_filter_large_parameters(large); xm1_pipeline_sensor_filter_large_enable(1, word(large, 4)); }
    } else { unsafe { xm1_pipeline_sensor_filter_large_enable(0, 0) }; }
    unsafe { xm1_pipeline_sensor_filter_routing(word(record, 0x18)) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configure_runtime_filters(request: SensorRequest, configuration: SensorConfiguration) -> i32 {
    let parameters = Address32(0x14c88b8);
    unsafe { copy(parameters, Address32(word(request.0, 0xe4)), 0x88) };
    let primary = Address32(unsafe { word(request.0, 0xe0) });
    if primary.0 != 0 { unsafe { xm1_pipeline_sensor_runtime_first_enable(1); xm1_pipeline_sensor_runtime_first_parameters(primary); } }
    else { unsafe { xm1_pipeline_sensor_runtime_first_enable(0) }; }
    if unsafe { word(request.0, 0xe4) } != 0 {
        unsafe { store(parameters, 0, word(configuration.0, 0x2c)); store(parameters, 4, word(configuration.0, 0x30)); xm1_pipeline_sensor_runtime_second_enable(1); xm1_pipeline_sensor_runtime_second_parameters(parameters); }
    } else { unsafe { xm1_pipeline_sensor_runtime_second_enable(0) }; }
    let third = Address32(unsafe { word(request.0, 0xe8) });
    if third.0 != 0 { unsafe { xm1_pipeline_sensor_runtime_third_enable(1); xm1_pipeline_sensor_runtime_third_parameters(third); } }
    else { unsafe { xm1_pipeline_sensor_runtime_third_enable(0) }; }
    unsafe { xm1_pipeline_sensor_runtime_commit(0, 0) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configure_channels(request: SensorRequest, configuration: SensorConfiguration) -> i32 {
    let record = mode_record(SensorMode(unsafe { word(request.0, 0) }));
    if unsafe { word(record, 0x10) } != 0 { unsafe { xm1_pipeline_sensor_configure_filters(request, configuration) }; }
    if unsafe { word(record, 0x14) } != 0 { unsafe { xm1_pipeline_sensor_configure_runtime_filters(request, configuration) }; }
    if unsafe { word(request.0, 0x98) } != 0 { unsafe { xm1_pipeline_sensor_update_center(word(configuration.0, 0x38) & 0xffff, word(configuration.0, 0x34) & 0xffff) }; }
    if unsafe { word(request.0, 0xc0) } != 0 { unsafe { xm1_pipeline_sensor_update_secondary_center(word(configuration.0, 0x40) & 0xffff, word(configuration.0, 0x3c) & 0xffff) }; }
    unsafe { xm1_pipeline_sensor_program_configuration(request, configuration) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_update_center(vertical: u32, horizontal: u32) { unsafe { xm1_pipeline_sensor_set_center(vertical, horizontal) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_set_center(vertical: u32, horizontal: u32) { unsafe { xm1_pipeline_sensor_device_center(vertical, horizontal) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_update_secondary_center(vertical: u32, horizontal: u32) { unsafe { xm1_pipeline_sensor_set_secondary_center(vertical, horizontal) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_set_secondary_center(vertical: u32, horizontal: u32) { unsafe { xm1_pipeline_sensor_device_secondary_center(vertical, horizontal) }; }
