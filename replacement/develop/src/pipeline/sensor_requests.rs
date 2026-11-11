use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SensorStage(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProcessingRequest(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ImagePlane(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProcessingFeatures(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProcessingOperation(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CaptureSelection(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationKey(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SettingsPacket(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PipelineDescriptor(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ExpandedDescriptor(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DescriptorHardwareFlags(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProcessingScale(pub u32);

pub(super) unsafe fn word(address: Address32, offset: u32) -> u32 { unsafe { address.offset(ByteOffset(offset)).word() } }
pub(super) unsafe fn half(address: Address32, offset: u32) -> u16 { unsafe { address.offset(ByteOffset(offset)).half() } }
pub(super) unsafe fn byte(address: Address32, offset: u32) -> u8 { unsafe { address.offset(ByteOffset(offset)).byte() } }
pub(super) unsafe fn store(address: Address32, offset: u32, value: u32) { unsafe { address.offset(ByteOffset(offset)).write_word(value) }; }
pub(super) unsafe fn store_half(address: Address32, offset: u32, value: u16) { unsafe { address.offset(ByteOffset(offset)).write_half(value) }; }
pub(super) unsafe fn store_byte(address: Address32, offset: u32, value: u8) { unsafe { address.offset(ByteOffset(offset)).write_byte(value) }; }
pub(super) unsafe fn copy(destination: Address32, source: Address32, count: u32) {
    let mut offset = 0;
    if (destination.0 | source.0) & 3 == 0 {
        while count - offset >= 4 { unsafe { store(destination, offset, word(source, offset)) }; offset += 4; }
    }
    while offset < count { unsafe { store_byte(destination, offset, byte(source, offset)) }; offset += 1; }
}
pub(super) unsafe fn clear(destination: Address32, count: u32) {
    let mut offset = 0;
    if destination.0 & 3 == 0 {
        while count - offset >= 4 { unsafe { store(destination, offset, 0) }; offset += 4; }
    }
    while offset < count { unsafe { store_byte(destination, offset, 0) }; offset += 1; }
}

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: ConfigurationKey) -> u32;
    fn xm1_buffers_configuration_half(key: ConfigurationKey) -> u16;
    fn xm1_pipeline_image_geometry_levels(stage: u32, alternate: u32, secondary: u32, output: *mut u32);
    fn xm1_pipeline_sensor_defect_rows(stage: SensorStage, original: ImagePlane, selection: CaptureSelection, secondary: i32, output: Address32) -> u32;
    fn xm1_pipeline_sensor_filter_parameters(stage: SensorStage, secondary: i32, flags: u32, output: Address32);
    fn xm1_pipeline_sensor_profile_geometry(stage: SensorStage, source: ImagePlane, selection: CaptureSelection, output: Address32);
    fn xm1_pipeline_sensor_primary_geometry(stage: SensorStage, output: Address32, operation: ProcessingOperation, selection: CaptureSelection, secondary: i32) -> u32;
    fn xm1_pipeline_sensor_secondary_geometry(stage: SensorStage, output: Address32, operation: ProcessingOperation, selection: CaptureSelection, secondary: i32) -> u32;
    fn xm1_pipeline_sensor_capture_gain(stage: SensorStage, output: Address32);
    fn xm1_pipeline_image_output_format(stage: SensorStage, operation: ProcessingOperation) -> super::sensor_descriptor::SettingsKind;
    fn xm1_pipeline_sensor_collect_settings(stage: SensorStage, descriptor: PipelineDescriptor, packet: SettingsPacket, operation: ProcessingOperation, option: u32);
    fn xm1_pipeline_sensor_configure_descriptor(descriptor: PipelineDescriptor, format: super::sensor_descriptor::SettingsKind, flags: super::sensor_template::TemplateRequestFlags);
    fn xm1_pipeline_sensor_submit_descriptor(saved: ExpandedDescriptor, descriptor: PipelineDescriptor);
    fn xm1_pipeline_sensor_descriptor_acquire(flags: DescriptorHardwareFlags);
    fn xm1_pipeline_sensor_descriptor_release(flags: DescriptorHardwareFlags);
    fn xm1_pipeline_sensor_publish_descriptor(descriptor: ExpandedDescriptor);
    fn xm1_pipeline_sensor_reset_descriptor(descriptor: ExpandedDescriptor);
    fn xm1_pipeline_sensor_capture_descriptor(destination: Address32, index: u32, format: super::sensor_descriptor::SettingsKind, saved: ExpandedDescriptor, descriptor: PipelineDescriptor);
}

pub(super) unsafe fn configuration_byte(key: ConfigurationKey) -> u32 { unsafe { xm1_buffers_configuration_byte(key) } }
pub(super) unsafe fn configuration_half(key: ConfigurationKey) -> u32 { unsafe { xm1_buffers_configuration_half(key) as u32 } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_processing(
    stage: SensorStage, request: ProcessingRequest, source: ImagePlane, features: ProcessingFeatures,
    operation: ProcessingOperation, selection: CaptureSelection, secondary: i32, _output: Address32,
    output_pointers: Address32, original: ImagePlane,
) {
    let configuration = unsafe { configuration_byte(ConfigurationKey(0xc0)) };
    let mut shading = Address32(0);
    if unsafe { word(features.0, 0) } & 0x200 != 0 && unsafe { word(stage.0, 0x640) } == 2 {
        let bank = stage.0.offset(ByteOffset(if selection.0 == 0 { 0x2268 } else { 0x23f0 }));
        let threshold = unsafe { configuration_half(ConfigurationKey(0x2f8d)) } as i16 as i32;
        let key = if unsafe { word(stage.0, 0x648) } as i32 > threshold { 0x1dea } else { 0x1deb };
        shading = Address32(0x15682e8);
        unsafe {
            store(shading, 0, configuration_byte(ConfigurationKey(key)));
            store(shading, 8, half(bank, 0x3c) as u32);
            store(shading, 12, configuration_half(ConfigurationKey(0x1dd7)));
            store(shading, 24, configuration_byte(ConfigurationKey(0x1dda)));
            store(shading, 20, configuration_byte(ConfigurationKey(0x1dd9)));
            store(shading, 4, configuration_byte(ConfigurationKey(0x1dec)));
            store(shading, 16, configuration_half(ConfigurationKey(0x1ded)));
        }
    }
    unsafe { store(output_pointers, 0, shading.0) };
    let auxiliary = Address32(unsafe { word(request.0, 24) });
    unsafe { store(Address32(0x1568304), 4, word(auxiliary, 4)) };
    let defects = if unsafe { word(features.0, 0) } & 0x80 != 0 && unsafe { xm1_pipeline_sensor_defect_rows(stage, original, selection, secondary, Address32(0x1568304)) } != 0 { 0x1568304 } else { 0 };
    unsafe { store(output_pointers, 4, defects) };
    let levels = if unsafe { word(features.0, 0) } & 0x809 == 0 || unsafe { configuration_byte(ConfigurationKey(0xb2)) } & 4 != 0 { 0 }
        else if operation.0 == 7 { stage.0.0.wrapping_add(0x160c).wrapping_add(selection.0.wrapping_mul(32)) }
        else {
            unsafe { xm1_pipeline_image_geometry_levels(stage.0.0, selection.0, secondary as u32, 0x1569a54 as *mut u32) };
            0x1569a54
        };
    unsafe { store(output_pointers, 28, levels); store(output_pointers, 8, 0) };
    let filter = if unsafe { word(features.0, 0) } & 2 == 0 { 0 } else {
        unsafe { xm1_pipeline_sensor_filter_parameters(stage, secondary, word(source.0, 24), Address32(0x156aed4)) }; 0x156aed4
    };
    unsafe { store(output_pointers, 16, filter) };
    let profile = if unsafe { word(features.0, 0) } & 0x20 == 0 { 0 } else {
        unsafe { xm1_pipeline_sensor_profile_geometry(stage, source, selection, Address32(0x156ad6c)) }; 0x156ad6c
    };
    unsafe { store(output_pointers, 12, profile); store(output_pointers, 20, 0) };
    let primary = if unsafe { word(features.0, 0) } & 0x400 != 0 && configuration & 0x10 != 0 && unsafe { xm1_pipeline_sensor_primary_geometry(stage, Address32(0x1570568), operation, selection, secondary) } != 0 { 0x1570568 } else {
        unsafe { store(features.0, 0, word(features.0, 0) & !0x400) }; 0
    };
    unsafe { store(output_pointers, 32, primary) };
    let secondary_geometry = if unsafe { word(features.0, 0) } & 0x800 != 0 && configuration & 2 != 0 && unsafe { xm1_pipeline_sensor_secondary_geometry(stage, Address32(0x156b144), operation, selection, secondary) } != 0 { 0x156b144 } else {
        unsafe { store(features.0, 0, word(features.0, 0) & !0x800) }; 0
    };
    unsafe { store(output_pointers, 24, secondary_geometry) };
    let gain = if unsafe { word(features.0, 0) } & 0x40 != 0 && configuration & 4 != 0 {
        unsafe { xm1_pipeline_sensor_capture_gain(stage, Address32(0x1569600)) }; 0x1569600
    } else { 0 };
    unsafe { store(output_pointers, 36, gain) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_collect_processing_flags(_stage: SensorStage, saved: ExpandedDescriptor, output: Address32) {
    for (index, offset) in [4, 0x30, 0x1a6, 0x226, 0x234, 0xf28, 0xf9c, 0xfd8, 0xfd8, 0xf25, 0xfc6, 0xfcf, 0xfd9].into_iter().enumerate() {
        let value = unsafe { byte(saved.0, offset) };
        unsafe { store(output, index as u32 * 4, if index == 8 { u32::from(value == 2) } else { u32::from(value != 0) }) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_setup_processing(
    stage: SensorStage, packet: SettingsPacket, saved: ExpandedDescriptor, descriptor: PipelineDescriptor,
    operation: ProcessingOperation, scale: ProcessingScale, option: u32, _first_flags: u32,
    _third_flags: u32, request_option: u32, output: Address32,
) {
    let format = unsafe { xm1_pipeline_image_output_format(stage, operation) };
    unsafe {
        xm1_pipeline_sensor_collect_settings(stage, descriptor, packet, operation, request_option);
        store(descriptor.0, 0xf0, (scale.0 / 1024).min(40));
        store(descriptor.0, 0x140, 0);
        xm1_pipeline_sensor_configure_descriptor(descriptor, format, super::sensor_template::TemplateRequestFlags(0));
        xm1_pipeline_sensor_submit_descriptor(saved, descriptor);
    }
    if unsafe { word(stage.0, 0x78) } == 11 {
        let capture = Address32(unsafe { word(stage.0, 0x1884) });
        for (target, origin) in [(0x1a6,0x42),(0x1a7,0x43),(0x1a8,0x44)] { unsafe { store_byte(saved.0, target, byte(capture, origin)) }; }
        for (target, origin) in [(0x1aa,0x48),(0x1ac,0x46),(0x1ae,0x4c)] { unsafe { store_half(saved.0, target, half(capture, origin)) }; }
        unsafe { store_half(saved.0, 0x1b2, byte(capture, 0x4a) as u16); store_half(saved.0, 0x1b0, byte(capture, 0x4b) as u16); }
        for index in 0..28 {
            unsafe { store_half(saved.0, 0x1b4 + index * 2, half(capture, 0x4e + index * 2)); store_half(saved.0, 0x1ec + index * 2, half(capture, 0x86 + index * 2)); }
        }
    }
    unsafe {
        xm1_pipeline_image_collect_processing_flags(stage, saved, output);
        xm1_pipeline_sensor_descriptor_acquire(DescriptorHardwareFlags(word(descriptor.0, 0xd8)));
        xm1_pipeline_sensor_publish_descriptor(saved);
        xm1_pipeline_sensor_reset_descriptor(saved);
    }
    if option == 1 {
        let sequence = Address32(unsafe { word(stage.0, 0x2210) });
        unsafe {
            xm1_pipeline_sensor_capture_descriptor(Address32(word(stage.0, 0x220c)), word(sequence, 0), format, saved, descriptor);
            store(sequence, 0, word(sequence, 0).wrapping_add(1));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_image_saved_request_submit(saved: PipelineDescriptor) {
    unsafe { xm1_pipeline_sensor_descriptor_release(DescriptorHardwareFlags(word(saved.0, 0xd8))) };
}
