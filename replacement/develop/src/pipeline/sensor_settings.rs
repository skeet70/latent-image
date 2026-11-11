use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{byte, clear, copy, half, store, store_byte, store_half, word, PipelineDescriptor, ProcessingOperation, SensorStage, SettingsPacket};

unsafe extern "C" {
    fn xm1_pipeline_sensor_apply_settings(descriptor: PipelineDescriptor, packet: SettingsPacket, parameters: Address32);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_copy_capture_settings(stage: SensorStage, output: Address32) {
    unsafe {
        store_byte(output, 0, byte(Address32(word(stage.0, 0x1884)), 0x14c));
        store_byte(output, 1, byte(Address32(word(stage.0, 0x1884)), 0x14d));
        copy(output.offset(ByteOffset(2)), Address32(word(stage.0, 0x1884)).offset(ByteOffset(0x14e)), 55);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_collect_settings(stage: SensorStage, descriptor: PipelineDescriptor, packet: SettingsPacket, operation: ProcessingOperation, _option: u32) {
    let input = stage.0;
    let output = packet.0;
    unsafe {
        store(output, 0, 0);
        store(output, 4, 0);
        for (target, origin) in [(8,0x4a8),(12,0x504),(16,0x500)] { store(output, target, word(input, origin)); }
        let format = word(input, 0x4b4);
        if format == 1 {
            store(output, 20, if word(input, 0x710) & 0x400 != 0 { 25 } else { word(input, 0x638) });
            store(output, 0x50, 1);
        } else {
            store(output, 20, format);
            store(output, 0x50, 0);
        }
        for (target, origin) in [(0x18,0x4bc),(0x1c,0x518),(0x20,0x51c),(0x24,0x508),(0x28,0x548),(0x2c,0x524),(0x30,0x5f4),(0x34,0x5f8),(0x38,0x5f0),(0x3c,0x530)] { store(output, target, word(input, origin)); }
        store_half(output, 0x40, half(input, 0x1682));
        store(output, 0x48, input.offset(ByteOffset(0x1b74)).0);
        store(output, 0x4c, 0);
        store(output, 0x54, 1);
        store(output, 0x58, 0);
        store(output, 0x5c, 0);
        store_half(output, 0x60, 0);
        store_half(output, 0x62, 0);
        let format = word(input, 0x4b4);
        if format == 25 || format == 1 && word(input, 0x710) & 0x400 != 0 {
            store_byte(output, 0x64, match operation.0 { 12 => byte(input, 0x1d30), 11 => 1, _ => 0 });
            store(output, 0x68, u32::from(word(input, 0x2fb8) != 0));
        } else {
            store_byte(output, 0x64, 0);
            store(output, 0x68, 1);
        }
        store(output, 0x6c, u32::from(operation.0 == 8));
        if word(input, 0x4a8) == 8 || word(input, 0x78) == 11 {
            store_byte(output, 0x71, 1);
            let adjustment = word(input, 0x1d3c) as i32;
            store(output, 0x74, match adjustment { -3 => 12, -2 => 11, -1 => 10, 1..=9 => (9 - adjustment) as u32, _ => 9 });
        } else {
            store_byte(output, 0x71, 0);
            store(output, 0x74, 9);
        }
        if word(input, 0x78) == 11 {
            store_byte(output, 0x78, 1);
            store_byte(output, 0x79, byte(input, 0x188c));
            xm1_pipeline_sensor_copy_capture_settings(stage, output.offset(ByteOffset(0xb0)));
        } else {
            store_byte(output, 0x78, 0);
            store_byte(output, 0x79, 0);
            clear(output.offset(ByteOffset(0xb0)), 57);
        }
        store(output, 0x7c, match operation.0 { 10 => 2, 11 | 12 => 1, _ => 0 });
        store(output, 0x80, u32::from(half(input, 0x6c2) != 0));
        store_half(output, 0x94, 0);
        store_half(output, 0x96, 0);
        store(output, 0x98, u32::from(word(input, 0x4ac) != 0));
        store(output, 0x9c, u32::from(word(input, 0x62c) != 0));
        store_half(output, 0xa0, word(input, 0x668) as u16);
        store_half(output, 0xa2, word(input, 0x664) as u16);
        store(output, 0xa4, 0);
        store(output, 0xa8, 0);
        store(output, 0xac, 0);
        store_half(output, 0xea, word(input, 0x448) as u16);
        store_half(output, 0xec, word(input, 0x44c) as u16);
        store(output, 0xf0, word(input, 0x2710));
        store_byte(output, 0xf4, byte(input, 0x2220));
        store_half(output, 0xf6, half(input, 0x2222));
        store(output, 0xf8, word(input, 0x223c));
        store(output, 0xfc, word(input, 0x2240));
        store_half(output, 0x100, half(input, 0x2244));
        copy(output.offset(ByteOffset(0x102)), input.offset(ByteOffset(0x2246)), 4);
        store(output, 0x108, word(input, 0x224c));
        store(output, 0x10c, word(input, 0x2250));
        store(output, 0x110, word(input, 0x2254));
        store_byte(output, 0x114, byte(input, 0x2258));
        for index in 0..5 {
            store(output, 0x118 + index * 4, word(input, 0x6cc + index * 4));
            store(output, 0x12c + index * 8, word(input, 0x6e0 + index * 8));
            store(output, 0x130 + index * 8, word(input, 0x6e4 + index * 8));
        }
        store_half(output, 0x154, half(input, 0x2264));
        for index in 0..3 { copy(output.offset(ByteOffset(0x158 + index * 0x188)), input.offset(ByteOffset(0x2268 + index * 0x188)), 0x188); }
        store(output, 0x5f0, word(input, 0x640));
        store_half(output, 0x5f4, half(input, 0x2700));
        store_byte(output, 0x5f6, byte(input, 0x2702));
        store_half(output, 0x5f8, half(input, 0x2704));
        store(output, 0x5fc, word(input, 0x2708));
        store_byte(output, 0x600, byte(input, 0x270e));
        xm1_pipeline_sensor_apply_settings(descriptor, packet, input.offset(ByteOffset(0x2220)));
    }
}
