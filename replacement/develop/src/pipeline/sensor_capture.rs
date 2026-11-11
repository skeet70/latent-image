use core::mem::MaybeUninit;
use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{byte, clear, copy, half, store, store_byte, word, DescriptorHardwareFlags, ExpandedDescriptor, PipelineDescriptor};
use super::sensor_descriptor::SettingsKind;
use super::sensor_configuration::{xm1_pipeline_sensor_copy_selected_words, WordCopyCount, WordCopyMap};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CaptureContext(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CaptureSequence(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct HardwareUsers(pub u32);

unsafe extern "C" {
    fn xm1_pipeline_sensor_acquire_hardware(flags: DescriptorHardwareFlags) -> HardwareUsers;
    fn xm1_pipeline_sensor_release_hardware(flags: DescriptorHardwareFlags, users: HardwareUsers);
    fn xm1_pipeline_sensor_capture_input_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_control_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_first_filter_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_second_filter_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_window_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_secondary_table_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_primary_control_registers(output: Address32);
    fn xm1_pipeline_sensor_commit_primary_control_registers(input: Address32);
    fn xm1_pipeline_sensor_capture_primary_table_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_region_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_coefficient_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_origin_registers(output: Address32);
    fn xm1_pipeline_sensor_capture_dimension_registers(output: Address32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_update_capture_metadata(context: CaptureContext, sequence: CaptureSequence, descriptor: PipelineDescriptor) {
    let metadata=context.0.offset(ByteOffset(0xbd468));
    unsafe {
        if sequence.0 == 0 {
            let preserved: [u32;11]=core::array::from_fn(|index|word(metadata,0x4c+index as u32*4));
            clear(metadata,0x5cf4);
            for (index,value) in preserved.into_iter().enumerate() { store(metadata,0x4c+index as u32*4,value); }
            store(metadata,0,0x53545943);
            store(metadata,0x5cf0,0x4c415943);
        }
        store(metadata,0xc,byte(descriptor.0,0x11b) as u32);
        store(metadata,0x10,word(descriptor.0,0xf4));
        store(metadata,0x14,word(descriptor.0,0x138));
        store(metadata,0x18,word(descriptor.0,0xf0));
        store(metadata,0x1c,((word(descriptor.0,0x130) as i32)/10) as u32);
        store(metadata,0x78,byte(descriptor.0,0x199) as u32);
        store(metadata,0x2c,word(descriptor.0,0x1bc));
        store(metadata,0x30,half(descriptor.0,0x1a0) as u32);
        store(metadata,0x34,byte(descriptor.0,0x1c4) as u32);
        store(metadata,0x38,byte(descriptor.0,0x1c5) as u32);
        store(metadata,0x3c,byte(descriptor.0,0x1c6) as u32);
        let level=word(descriptor.0,0x188) as i32;
        if level != 0 { store(metadata,0x20,(level/10) as u32); }
        let enabled=byte(descriptor.0,0x178);
        if enabled != 0 { store(metadata,0x24,enabled as u32); }
        let selection=byte(descriptor.0,0x190);
        if selection != 0 { store(metadata,0x28,selection as u32); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_copy_capture_descriptor(output: Address32, descriptor: PipelineDescriptor) {
    unsafe {
        copy(output,Address32(0xd6ba44),4);
        copy(output.offset(ByteOffset(4)),Address32(0xd6ba4c),4);
        copy(output.offset(ByteOffset(8)),descriptor.0,0x244);
    }
}

#[derive(Clone, Copy)]
enum CaptureGroup { Input, Control, Filters, Window, SecondaryTable, Region, Coefficients, Origin, Dimensions }
impl CaptureGroup {
    fn mask(self) -> u32 { match self { Self::Input=>1,Self::Control=>0x100,Self::Filters=>6,Self::Window=>8,Self::SecondaryTable=>0x400,Self::Region=>0x20,Self::Coefficients=>0x40,Self::Origin=>0x10,Self::Dimensions=>0x80 } }
    fn layout(self) -> (ByteOffset,Address32,WordCopyMap,WordCopyCount) {
        let (offset,tag,map,count)=match self {
            Self::Input=>(0x388,0xd6ba54,0xd6b64c,12),Self::Control=>(0x3bc,0xd6ba5c,0xd6b4b4,29),Self::Filters=>(0x434,0xd6ba64,0xd6b82c,67),
            Self::Window=>(0x544,0xd6ba6c,0xd6b6ac,32),Self::SecondaryTable=>(0x5c8,0xd6ba74,0xd6b804,5),Self::Region=>(0x7f0,0xd6ba84,0xd6bdac,31),
            Self::Coefficients=>(0x870,0xd6ba8c,0xd6b7ac,11),Self::Origin=>(0x8a0,0xd6ba94,0xd6c2bc,6),Self::Dimensions=>(0x8bc,0xd6ba9c,0xd6b59c,22),
        };
        (ByteOffset(offset),Address32(tag),WordCopyMap(Address32(map)),WordCopyCount(count))
    }
    unsafe fn capture(self, output: Address32) {
        unsafe { match self {
            Self::Input=>xm1_pipeline_sensor_capture_input_registers(output),Self::Control=>xm1_pipeline_sensor_capture_control_registers(output),
            Self::Filters=>{xm1_pipeline_sensor_capture_first_filter_registers(output);xm1_pipeline_sensor_capture_second_filter_registers(output);},
            Self::Window=>xm1_pipeline_sensor_capture_window_registers(output),Self::SecondaryTable=>xm1_pipeline_sensor_capture_secondary_table_registers(output),
            Self::Region=>xm1_pipeline_sensor_capture_region_registers(output),Self::Coefficients=>xm1_pipeline_sensor_capture_coefficient_registers(output),
            Self::Origin=>xm1_pipeline_sensor_capture_origin_registers(output),Self::Dimensions=>xm1_pipeline_sensor_capture_dimension_registers(output),
        } }
    }
}

unsafe fn capture_group(group: CaptureGroup, output: Address32, scratch: Address32) {
    let (tag_offset,tag,map,count)=group.layout();
    unsafe {
        match group {
            CaptureGroup::Input|CaptureGroup::Control|CaptureGroup::Filters=>{group.capture(scratch);copy(output.offset(tag_offset),tag,4);}
            _=>{copy(output.offset(tag_offset),tag,4);group.capture(scratch);}
        }
        xm1_pipeline_sensor_copy_selected_words(output,scratch,map,count);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_capture_descriptor(context: CaptureContext, sequence: CaptureSequence, format: SettingsKind, expanded: ExpandedDescriptor, descriptor: PipelineDescriptor) {
    if sequence.0 >= 10 { return; }
    let users=unsafe { xm1_pipeline_sensor_acquire_hardware(DescriptorHardwareFlags(0x800)) };
    unsafe { xm1_pipeline_sensor_update_capture_metadata(context,sequence,descriptor); }
    let flags=unsafe { word(expanded.0,0) };
    let output=context.0.offset(ByteOffset(sequence.0*0x918+0xbd668));
    unsafe {
        store(output,0x370,flags);store(output,0x374,format.0);
        for index in 0..4 { store(output,0x378+index*4,half(descriptor.0,0x23c+index*2) as u32); }
        xm1_pipeline_sensor_copy_capture_descriptor(output,descriptor);
    }
    let mut scratch=MaybeUninit::<[u32;67]>::uninit();
    let scratch=Address32(scratch.as_mut_ptr() as u32);
    for group in [CaptureGroup::Input,CaptureGroup::Control,CaptureGroup::Filters,CaptureGroup::Window,CaptureGroup::SecondaryTable] {
        if flags & group.mask() != 0 { unsafe { capture_group(group,output,scratch); } }
    }
    if flags & 0x200 != 0 {
        let shadow=Address32(0x1467ae0);
        unsafe {
            copy(output.offset(ByteOffset(0x5e0)),Address32(0xd6ba7c),4);
            xm1_pipeline_sensor_capture_primary_control_registers(shadow);
            let control=word(shadow,0);
            let enabled=byte(shadow,0)&1;
            store_byte(shadow,0,byte(shadow,0)&!1);
            xm1_pipeline_sensor_commit_primary_control_registers(shadow);
            xm1_pipeline_sensor_capture_primary_table_registers(shadow);
            xm1_pipeline_sensor_copy_selected_words(output,shadow,WordCopyMap(Address32(0xd6bea4)),WordCopyCount(131));
            store(output,0x5e4,enabled as u32);
            store(shadow,0,control);
            store_byte(shadow,0,(control as u8&!1)|enabled);
            xm1_pipeline_sensor_commit_primary_control_registers(shadow);
        }
    }
    for group in [CaptureGroup::Region,CaptureGroup::Coefficients,CaptureGroup::Origin,CaptureGroup::Dimensions] {
        if flags & group.mask() != 0 { unsafe { capture_group(group,output,scratch); } }
    }
    unsafe { xm1_pipeline_sensor_release_hardware(DescriptorHardwareFlags(0x800),users); }
}
