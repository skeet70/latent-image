use super::shared::Address32;
use core::mem::MaybeUninit;
use super::sensor_requests::{byte, configuration_byte, configuration_half, half, store_byte, store_half, word, ConfigurationKey};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ProfileId(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConversionId(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConversionInput(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConversionBand(pub u32);


#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configuration_word(key: ConfigurationKey) -> u32 {
    let first = unsafe { configuration_byte(key) };
    let second = unsafe { configuration_byte(ConfigurationKey(key.0.wrapping_add(1))) };
    let third = unsafe { configuration_byte(ConfigurationKey(key.0.wrapping_add(2))) };
    let fourth = unsafe { configuration_byte(ConfigurationKey(key.0.wrapping_add(3))) };
    first | (second << 8) | (third << 16) | (fourth << 24)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_profile_feature(profile: ProfileId) -> u32 {
    unsafe { byte(Address32(0xef6de4), profile.0.wrapping_mul(20).wrapping_add(13)) as u32 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_select_conversion(value: ConversionInput) -> ConversionId {
    let table = Address32(0x816e20);
    let adjusted = value.0.wrapping_add(50);
    let mut index = 1;
    while index < 20 {
        let key = ConfigurationKey(unsafe { word(table, index * 16) });
        let threshold = unsafe { configuration_half(key).wrapping_add(word(table, index * 16 + 4)) };
        if adjusted < threshold { break; }
        index += 1;
    }
    ConversionId(unsafe { word(table, index * 16 - 8) } as i32)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_conversion_band(conversion: ConversionId) -> ConversionBand {
    let table = Address32(0x816e20);
    let mut selected = 2;
    if conversion.0 == 7 { selected = 0; }
    else {
        for index in 1..20 {
            if unsafe { word(table, index * 16 + 8) } == conversion.0 as u32 { selected = index; break; }
        }
    }
    ConversionBand(unsafe { word(table, selected * 16 + 12) })
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationChannel(pub u32);
#[repr(transparent)]
struct TaskId(u32);
#[repr(transparent)]
struct TaskByte(u8);
#[repr(transparent)]
struct MessageCode(u16);
#[repr(transparent)]
struct MessageBytes(u32);
#[repr(transparent)]
struct ReservedResource(u32);
#[repr(transparent)]
struct MessageQueue(u32);
#[repr(transparent)]
struct WaitTicks(u32);
#[repr(C)]
struct CancellationPayload { callback: Address32, argument: Address32 }
#[repr(C)]
struct CancellationMessage { code: MessageCode, sender: TaskByte, destination: TaskByte, length: MessageBytes, payload: Address32 }

unsafe extern "C" {
    fn xm1_pipeline_image_current_task() -> TaskId;
    fn xm1_pipeline_image_send_message(channel: MessageQueue, message: *const CancellationMessage) -> i32;
    fn xm1_pipeline_image_wait_reserved(resource: ReservedResource, timeout: WaitTicks) -> i32;
    fn xm1_pipeline_image_signal_reserved(resource: ReservedResource) -> i32;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_wait_configuration(channel: ConfigurationChannel) -> i32 {
    let handle = unsafe { word(Address32(0x817700), channel.0.wrapping_mul(4)) };
    if handle == 0 { super::sensor_backend::xm1_pipeline_sensor_fault(); }
    let _ = unsafe { xm1_pipeline_image_wait_reserved(ReservedResource(handle), WaitTicks(u32::MAX)) };
    unsafe { xm1_pipeline_image_signal_reserved(ReservedResource(handle)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_cancel_configuration_request(channel: ConfigurationChannel) -> i32 {
    if unsafe { word(Address32(0x944440), channel.0.wrapping_mul(4)) } == 1 { return 0; }
    let payload = CancellationPayload { callback: Address32(0), argument: Address32(0) };
    if channel.0 > 8 { return -30; }
    let sender = unsafe { xm1_pipeline_image_current_task() };
    let message = CancellationMessage {
        code: MessageCode(0x4c00 + channel.0 as u16), sender: TaskByte(sender.0 as u8), destination: TaskByte(60),
        length: MessageBytes(8), payload: Address32(core::ptr::addr_of!(payload) as u32),
    };
    if unsafe { xm1_pipeline_image_send_message(MessageQueue(60), &message) } != 0 { return -255; }
    let _ = unsafe { xm1_pipeline_sensor_wait_configuration(channel) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_cancel_configuration(channel: ConfigurationChannel) -> i32 {
    unsafe { xm1_pipeline_sensor_cancel_configuration_request(channel) }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationKeySequence(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigurationKeyCount(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct WordCopyMap(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct WordCopyCount(pub u32);
#[derive(Clone, Copy)]
#[repr(C)]
struct WordCopyOffsets { destination: super::shared::ByteOffset, source: super::shared::ByteOffset }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configuration_key_bytes(output: Address32, keys: ConfigurationKeySequence, count: ConfigurationKeyCount) {
    for index in 0..count.0 {
        let key=ConfigurationKey(unsafe { half(keys.0,index.wrapping_mul(2)) } as u32);
        unsafe { store_byte(output,index,configuration_byte(key) as u8); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_configuration_key_halves(output: Address32, keys: ConfigurationKeySequence, count: ConfigurationKeyCount) {
    for index in 0..count.0 {
        let key=ConfigurationKey(unsafe { half(keys.0,index.wrapping_mul(2)) } as u32);
        unsafe { store_half(output,index.wrapping_mul(2),configuration_half(key) as u16); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_copy_selected_words(output: Address32, source: Address32, offsets: WordCopyMap, count: WordCopyCount) {
    for index in 0..count.0 {
        let entry=offsets.0.offset(super::shared::ByteOffset(index.wrapping_mul(8)));
        let offsets=unsafe { core::ptr::read_volatile(entry.0 as *const WordCopyOffsets) };
        let value=unsafe { core::ptr::read_volatile(source.offset(offsets.source).0 as *const MaybeUninit<u32>) };
        unsafe { core::ptr::write_volatile(output.offset(offsets.destination).0 as *mut MaybeUninit<u32>,value); }
    }
}
