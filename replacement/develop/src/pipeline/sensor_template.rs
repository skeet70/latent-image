use core::mem::MaybeUninit;
use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{byte, configuration_byte, copy, half, store, store_byte, word, ConfigurationKey, PipelineDescriptor};
use super::sensor_descriptor::{DescriptorKind, LevelIndex, SettingsKind};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TemplateFlags(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TemplateIndex(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TemplateRequestFlags(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationMode(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationMask(pub u32);
#[repr(C)]
struct TemplateRecord { flags: [u8; 56], sections: u32 }
#[repr(C)]
struct TemplateInputs { enables: [u8; 6], components: [u8; 9], groups: [u8; 4], pairs: [u8; 2] }
#[derive(Clone, Copy)]
#[repr(u8)]
enum SelectionIndex { Special = 0, Default = 1, First = 2, Second = 3, Third = 4, Fourth = 5 }

unsafe fn key_at(table: Address32, index: u32) -> ConfigurationKey { ConfigurationKey(unsafe { half(table, index.wrapping_mul(2)) } as u32) }
unsafe fn table_byte(table: Address32, index: u32) -> u32 { unsafe { configuration_byte(key_at(table, index)) } }
unsafe fn selection_index(descriptor: PipelineDescriptor) -> SelectionIndex {
    unsafe {
        if word(descriptor.0, 0x19c) == 2 && word(descriptor.0, 0x228) != 0 { SelectionIndex::Special }
        else { match word(descriptor.0, 0x154) { 1 => SelectionIndex::First, 2 => SelectionIndex::Second, 3 => SelectionIndex::Third, 4 => SelectionIndex::Fourth, _ => SelectionIndex::Default } }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_selection_index(descriptor: PipelineDescriptor, output: Address32) { unsafe { store_byte(output, 0, selection_index(descriptor) as u8); } }

#[unsafe(no_mangle)]
pub extern "C" fn xm1_pipeline_sensor_select_type_mode_value(kind: DescriptorKind, mode: ConfigurationMode, value: LevelIndex, alternate: LevelIndex) -> LevelIndex {
    if kind.0 == 0x35 && mode.0 == 1 { alternate } else { value }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_selection_flag(descriptor: PipelineDescriptor) -> u8 {
    let kind = unsafe { word(descriptor.0, 0xe8) };
    if kind.wrapping_sub(8) <= 5 { unsafe { byte(Address32(0xd621c0), kind) } }
    else { unsafe { byte(Address32(0xd61c58), word(descriptor.0, 0x104)) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_configuration_flag8(descriptor: PipelineDescriptor) -> u32 {
    let mut flags = unsafe { configuration_byte(ConfigurationKey(0xaf93)) };
    if flags & 8 != 0 { flags = (flags & !8 | ((unsafe { xm1_pipeline_sensor_read_selection_flag(descriptor) } as u32) << 3)) & 255; }
    flags
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_primary_configuration(_descriptor: PipelineDescriptor) -> u32 { unsafe { configuration_byte(ConfigurationKey(0xa935)) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_secondary_configuration(_descriptor: PipelineDescriptor) -> u32 { unsafe { configuration_byte(ConfigurationKey(0xa936)) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_level_configuration(descriptor: PipelineDescriptor) -> u32 { unsafe { table_byte(Address32(0xd6960c), word(descriptor.0, 0xf4)) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_mode_configuration(descriptor: PipelineDescriptor) -> u32 { unsafe { table_byte(Address32(0xd61e78), word(descriptor.0, 0x160)) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_noise_configuration(_descriptor: PipelineDescriptor) -> u32 { unsafe { configuration_byte(ConfigurationKey(0xba9d)) } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_board_configuration_enabled() -> u32 {
    if unsafe { configuration_byte(ConfigurationKey(0x235)) } == 0 { 0 } else { u32::from(unsafe { byte(Address32(0x7c2de0), 0) } == 10) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_mode_selection_enabled(descriptor: PipelineDescriptor) -> u32 {
    let flags = unsafe { configuration_byte(ConfigurationKey(0xc647)) };
    let required = unsafe {
        if word(descriptor.0, 0xe4) == 0x35 || word(descriptor.0, 0x1d0) == 1 && word(descriptor.0, 0x1d4) != 0 { 2 }
        else { match word(descriptor.0, 0x160) { 2 => 2, 3 => if word(descriptor.0, 0x11c) == word(descriptor.0, 0x120) { 16 } else { 8 }, 1 => 4, _ => 1 } }
    };
    u32::from(flags & required == required)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_selection_mask_enabled(descriptor: PipelineDescriptor) -> u32 {
    let flags = unsafe { table_byte(Address32(0xd6839c), word(descriptor.0, 0xf4)) };
    let required = match unsafe { selection_index(descriptor) } { SelectionIndex::Special => 1, SelectionIndex::Default => 2, SelectionIndex::First => 4, SelectionIndex::Second => 8, SelectionIndex::Third => 16, SelectionIndex::Fourth => 32 };
    u32::from(flags & required == required)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_mode_selector(descriptor: PipelineDescriptor) -> u32 {
    let mode = unsafe { word(descriptor.0, 0x160) };
    let key = unsafe {
        if mode == 3 {
            let table = if word(descriptor.0, 0x11c) == word(descriptor.0, 0x120) { 0xd6a91c } else { 0xd6ace8 };
            key_at(Address32(table), word(descriptor.0, 0x15c))
        } else if word(descriptor.0, 0xe4) == 0x35 || word(descriptor.0, 0x1d0) == 1 && word(descriptor.0, 0x1d4) != 0 { ConfigurationKey(0xc631) }
        else { key_at(Address32(0xd6ace0), mode) }
    };
    unsafe { configuration_byte(key) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_alternate_selector(descriptor: PipelineDescriptor) -> u32 {
    if unsafe { word(descriptor.0, 0x160) } != 3 { return 0; }
    let key = unsafe { if word(descriptor.0, 0x11c) == word(descriptor.0, 0x120) { ConfigurationKey(0xc644) } else { key_at(Address32(0xd6a910), word(descriptor.0, 0x154)) } };
    unsafe { configuration_byte(key) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_threshold_enabled(descriptor: PipelineDescriptor) -> u32 {
    let band = unsafe { byte(descriptor.0, 0x16c) };
    if unsafe { configuration_byte(ConfigurationKey(0xc0)) } & 0x20 == 0 { return 0; }
    (unsafe { configuration_byte(ConfigurationKey(0x3e3dc8 + (band as u32 >> 3))) }) >> (band & 7) & 1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configuration_type_enabled(descriptor: PipelineDescriptor) -> u32 {
    let (mut key, mut mask, override_selection) = match unsafe { word(descriptor.0, 0xe4) } {
        1 => (0xae58,2,true), 5 | 0x23 | 0x24 => (0xae58,0x20,false), 6 => (0xae58,0x40,false), 0x25 => (0xae58,0x80,false),
        7 => (0xae59,0x20,false), 8 | 0x26 | 0x27 => (0xae59,1,true), 9 => (0xae59,2,true), 10 => (0xae59,4,false),
        0xb | 0xc => (0xae59,0x10,false), 0xd => (0xae59,0x40,false), 0xe => (0xae59,8,false), 0x1d => (0xae59,0x80,false),
        0x1e => (0xae5a,1,true), 0x1f => (0xae5a,2,true), 0x20 => (0xae5a,4,false), 0x21 => (0xae5a,8,false),
        0x22 => (0xae5a,0x10,false), 0x1c => (0xae5a,0x20,false), 0x19 => (0xae5a,0x40,false), 0x1a => (0xae5a,0x80,false),
        0x1b => (0xae5b,1,true), 0x18 => (0xae5b,2,true), 0x17 => (0xae5b,4,false), 0x14 => (0xae5b,8,false),
        0x16 => (0xae5b,0x10,false), 0x28 => (0xae5b,0x20,false), 0x29 => (0xae5b,0x40,false), 0x2a => (0xae5b,0x80,false),
        0x2b => (0xae5c,1,true), 0x2c => (0xae5c,2,true), 0x2d => (0xae5c,4,false), 0x2e => (0xae5c,8,false),
        0x2f => (0xae5c,0x10,false), 0x30 => (0xae5c,0x20,false), 0x31 | 0x32 => (0xae58,0x10,false), _ => (0xae58,1,true),
    };
    if override_selection {
        match unsafe { word(descriptor.0, 0xe8) } {
            selection @ 1..=7 => { key = 0xae5d; mask = 1 << (selection - 1); }
            selection @ 8..=13 => { key = 0xae5e; mask = 1 << (selection - 8); }
            _ => {}
        }
    }
    u32::from(unsafe { configuration_byte(ConfigurationKey(key)) } & mask != 0)
}

unsafe fn selected_level(descriptor: PipelineDescriptor) -> LevelIndex {
    unsafe { xm1_pipeline_sensor_select_type_mode_value(DescriptorKind(word(descriptor.0, 0xe4)), ConfigurationMode(word(descriptor.0, 0x1b0)), LevelIndex(word(descriptor.0, 0xf4)), LevelIndex(word(descriptor.0, 0x180))) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_read_selected_configuration(descriptor: PipelineDescriptor) -> u32 {
    let mut level = unsafe { selected_level(descriptor) }.0;
    let table = if unsafe { word(descriptor.0, 0xe4) } == 0x35 { level = level.wrapping_add(unsafe { word(descriptor.0, 0x1b0) }.wrapping_mul(10)); 0xd631c0 } else { 0xd6319c };
    unsafe { table_byte(Address32(table), level) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_compute_configuration_mask(descriptor: PipelineDescriptor, mask: ConfigurationMask) -> u32 {
    let mut level = unsafe { selected_level(descriptor) }.0;
    let mut result = 0;
    if mask.0 & 1 != 0 {
        let (table,index) = if unsafe { word(descriptor.0, 0xe4) } == 0x35 { (0xd640e4,level.wrapping_add(unsafe { word(descriptor.0, 0x1b0) }.wrapping_mul(10))) } else { (0xd640d0,level) };
        result = unsafe { table_byte(Address32(table), index) };
    }
    if mask.0 & 2 != 0 {
        let table = if unsafe { word(descriptor.0, 0xe4) } == 0x35 { level = level.wrapping_add(unsafe { word(descriptor.0, 0x1b0) }.wrapping_mul(10)); 0xd64120 } else { 0xd63d9c };
        result = unsafe { table_byte(Address32(table), level) };
    }
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_collect_template_inputs(descriptor: PipelineDescriptor, enables: Address32, components: Address32, groups: Address32, pairs: Address32) {
    let mode = unsafe { word(descriptor.0, 0x140) };
    let (level_key,group_key,feature_key) = unsafe {
        match mode {
            1 | 2 => {
                let selection = word(descriptor.0, 0x128);
                let extended = if word(descriptor.0, 0x1d0) == 1 && word(descriptor.0, 0x1d8) != 0 && word(descriptor.0, 0x144) == 0 { selection.wrapping_add(word(descriptor.0, 0x1d8).wrapping_mul(2)) } else { selection };
                (key_at(Address32(0xd666d4),word(descriptor.0,0xf4)),key_at(Address32(0xd66d3c),extended),key_at(Address32(0xd66af4),selection))
            }
            3 | 4 => { let selection=word(descriptor.0,0x128); (key_at(Address32(0xd666d4),word(descriptor.0,0xf4)),key_at(Address32(0xd66a9c),selection),key_at(Address32(0xd66b50),selection)) }
            5 => { let selection=word(descriptor.0,0x148); (key_at(Address32(0xd66aa0),word(descriptor.0,0xf4).wrapping_add(selection.wrapping_mul(10))),key_at(Address32(0xd667a0),selection),key_at(Address32(0xd6655c),selection)) }
            6 => { let selection=word(descriptor.0,0x1dc); (key_at(Address32(0xd66c40),word(descriptor.0,0xf4)),key_at(Address32(0xd66ccc),selection),key_at(Address32(0xd66d30),selection)) }
            _ => (key_at(Address32(0xd666c0),word(descriptor.0,0xf4)),ConfigurationKey(0xc315),ConfigurationKey(0xc5fe)),
        }
    };
    let enable = unsafe { configuration_byte(ConfigurationKey(0xc0be)) };
    let mut bits = unsafe { configuration_byte(ConfigurationKey(0xc105)) };
    let level = unsafe { configuration_byte(level_key) };
    let group = unsafe { configuration_byte(ConfigurationKey(0xc264)) };
    let subgroup = unsafe { configuration_byte(group_key) };
    let feature = unsafe { configuration_byte(feature_key) };
    let second_pair = unsafe { configuration_byte(ConfigurationKey(0xc0bf)) };
    let first_pair = unsafe { table_byte(Address32(0xd66564),word(descriptor.0,0xf4)) };
    if bits & 8 != 0 { bits = (bits & !8 | ((unsafe { xm1_pipeline_sensor_read_selection_flag(descriptor) } as u32) << 3)) & 255; }
    unsafe {
        store_byte(enables,0,0); store_byte(enables,1,enable as u8);
        for index in 2..6 { store_byte(enables,index,1); }
        store_byte(components,0,0);
        for index in 0..7 { store_byte(components,index+1,((bits >> index) & 1) as u8); }
        store_byte(components,8,level as u8);
        store_byte(groups,0,group as u8); store_byte(groups,1,subgroup as u8); store_byte(groups,2,feature as u8); store_byte(groups,3,0);
        store_byte(pairs,0,first_pair as u8); store_byte(pairs,1,second_pair as u8);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_update_template_flags(record: TemplateFlags, _source: TemplateRequestFlags, kind: SettingsKind, descriptor: PipelineDescriptor) {
    let output = record.0;
    let sections = unsafe { word(output,56) };
    unsafe {
        if sections & 1 != 0 {
            if byte(output,2) == 1 { store_byte(output,2,if configuration_byte(ConfigurationKey(0xbf)) == 0 { 0 } else { xm1_pipeline_sensor_read_primary_configuration(descriptor) as u8 }); }
            if word(output,0) & 0x00ff00ff == 0x10001 { store_byte(output,0,xm1_pipeline_sensor_read_secondary_configuration(descriptor) as u8); }
            if byte(output,2) == 1 && byte(output,1) != 0 { store_byte(output,1,xm1_pipeline_sensor_read_level_configuration(descriptor) as u8); }
        }
        if sections & 0x100 != 0 && byte(output,4) != 0 {
            let status=xm1_pipeline_sensor_read_mode_selector(descriptor);
            store_byte(output,4,status as u8);
            if status == 2 { store_byte(output,3,xm1_pipeline_sensor_read_alternate_selector(descriptor) as u8); }
        }
        if sections & 2 != 0 {
            if byte(output,7) == 1 { store_byte(output,7,xm1_pipeline_sensor_selection_mask_enabled(descriptor) as u8); }
            if byte(output,8) == 1 { store_byte(output,8,xm1_pipeline_sensor_mode_selection_enabled(descriptor) as u8); }
        }
        if sections & 8 != 0 && byte(output,9) == 1 { store_byte(output,9,xm1_pipeline_sensor_threshold_enabled(descriptor) as u8); store_byte(output,10,u8::from(sections & 2 == 0)); }
        if sections & 0x400 != 0 && byte(output,11) == 1 { store_byte(output,11,xm1_pipeline_sensor_configuration_type_enabled(descriptor) as u8); }
        if sections & 0x200 != 0 && byte(output,12) == 1 { store_byte(output,12,xm1_pipeline_sensor_board_configuration_enabled() as u8); }
        if sections & 0x20 != 0 {
            let bits=xm1_pipeline_sensor_read_configuration_flag8(descriptor);
            let flag=xm1_pipeline_sensor_read_mode_configuration(descriptor);
            for index in 0..7 { if byte(output,20-index) == 1 { store_byte(output,20-index,((bits >> index)&1) as u8); } }
            if byte(output,13) != 0 { store_byte(output,13,flag as u8); }
        }
        if sections & 0x40 != 0 {
            if kind.0 == 9 { store_byte(output,21,0); store_byte(output,23,0); }
            else {
                let bits=xm1_pipeline_sensor_read_selected_configuration(descriptor);
                if byte(output,23) == 1 { store_byte(output,23,(bits&1) as u8); }
                store_byte(output,22,((bits>>1)&1) as u8);
                if byte(output,21) == 1 { store_byte(output,21,((bits>>2)&1) as u8); }
            }
        }
        if sections & 0x10 != 0 {
            if kind.0 == 9 { for index in 24..32 { store_byte(output,index,0); } }
            else { for group in 0..2 {
                let bits=xm1_pipeline_sensor_compute_configuration_mask(descriptor,ConfigurationMask(1<<group));
                for index in 0..4 { let target=24+group*4+index; if byte(output,target) == 1 { store_byte(output,target,((bits>>index)&1) as u8); } }
            } }
        }
        if sections & 0x80 != 0 {
            if kind.0 == 3 || kind.0 == 13 || kind.0 == 9 { store_byte(output,32,0); store_byte(output,33,0); }
            else if byte(output,33) != 0 { store_byte(output,33,xm1_pipeline_sensor_read_noise_configuration(descriptor) as u8); }
        }
        if sections & 0x1000 != 0 {
            let mut scratch=MaybeUninit::<TemplateInputs>::uninit();
            let pointer=scratch.as_mut_ptr();
            xm1_pipeline_sensor_collect_template_inputs(descriptor,Address32(core::ptr::addr_of_mut!((*pointer).enables) as u32),Address32(core::ptr::addr_of_mut!((*pointer).components) as u32),Address32(core::ptr::addr_of_mut!((*pointer).groups) as u32),Address32(core::ptr::addr_of_mut!((*pointer).pairs) as u32));
            let scratch=scratch.assume_init();
            for index in 0..5 { store_byte(output,34+index,scratch.enables[index as usize+1]); }
            for index in 0..9 { store_byte(output,39+index,scratch.components[index as usize]); }
            for index in 0..4 { store_byte(output,48+index,scratch.groups[index as usize]); }
            for index in 0..2 { store_byte(output,52+index,scratch.pairs[index as usize]); }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_template_index(kind: SettingsKind) -> TemplateIndex {
    let mut selected=0;
    for index in 0..18 { if unsafe { word(Address32(0xd77ed0),index*8) } == kind.0 { selected=unsafe { word(Address32(0xd77ed0),index*8+4) }; } }
    TemplateIndex(selected)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_expand_template(record: TemplateFlags, output: Address32) {
    unsafe {
        for index in 0..39 { store(output,index*4,byte(record.0,index) as u32); }
        for index in (39..48).rev() { store(output,index*4,byte(record.0,index) as u32); }
        for index in 48..54 { store(output,index*4,byte(record.0,index) as u32); }
        store(output,216,word(record.0,56));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configure_descriptor(descriptor: PipelineDescriptor, kind: SettingsKind, flags: TemplateRequestFlags) {
    let index=unsafe { xm1_pipeline_sensor_template_index(kind) };
    let mut record=MaybeUninit::<TemplateRecord>::uninit();
    let pointer=Address32(record.as_mut_ptr() as u32);
    unsafe {
        copy(pointer,Address32(0xd77ad4).offset(ByteOffset(index.0.wrapping_mul(60))),60);
        xm1_pipeline_sensor_update_template_flags(TemplateFlags(pointer),flags,kind,descriptor);
        for index in 0..54 { store(descriptor.0,index*4,byte(pointer,index) as u32); }
        store(descriptor.0,216,word(pointer,56));
    }
}
