use core::mem::MaybeUninit;
use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{byte, configuration_byte, half, store, store_byte, store_half, word, CaptureSelection, ConfigurationKey, ImagePlane, SensorStage};
use super::sensor_configuration::{xm1_pipeline_sensor_select_conversion, xm1_pipeline_sensor_conversion_band, xm1_pipeline_sensor_profile_feature, xm1_pipeline_sensor_cancel_configuration, xm1_pipeline_sensor_configuration_word, ConfigurationChannel, ConversionInput, ProfileId};
use super::output_triplets::{CalibrationBankId, xm1_pipeline_image_calibration_data};
use super::image_geometry::xm1_pipeline_image_geometry_delta;

#[derive(Clone, Copy)]
#[repr(transparent)]
struct TaskId(u32);
#[repr(transparent)]
struct TaskPriority(u32);
#[repr(transparent)]
struct RowStride(u32);
#[repr(C)]
struct PackedRows {
    mode: MaybeUninit<u32>, address: Address32, length: MaybeUninit<u32>, stride: RowStride,
    width: MaybeUninit<u32>, height: MaybeUninit<u32>, flags: MaybeUninit<u32>,
}

unsafe extern "C" {
    fn xm1_pipeline_image_write_packed12(image: *const PackedRows, x: i32, y: i32, value: u32);
    fn xm1_pipeline_image_current_task() -> TaskId;
    fn xm1_pipeline_image_configure_task(task: TaskId, priority: TaskPriority) -> i32;
}

#[unsafe(no_mangle)]
pub extern "C" fn xm1_pipeline_sensor_unused_operation() -> u32 { 0 }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_defect_rows(stage: SensorStage, original: ImagePlane, selection: CaptureSelection, secondary: i32, output: Address32) -> u32 {
    let mode = unsafe { word(stage.0, 0x640) };
    let enabled = if unsafe { configuration_byte(ConfigurationKey(0xb3)) } & 1 != 0 && mode == 1 { 2 } else { 0 };
    unsafe { store(output, 0, enabled); store(output, 8, 0); store(output, 12, 0); }
    let parameters = stage.0.offset(ByteOffset(if secondary == 1 { 0x2578 } else if selection.0 == 0 { 0x2268 } else { 0x23f0 }));
    let mut delta = MaybeUninit::<[i32; 2]>::uninit();
    unsafe { xm1_pipeline_image_geometry_delta(stage.0, secondary as u32, parameters, delta.as_mut_ptr().cast()) };
    let delta = unsafe { core::ptr::read_volatile(delta.as_ptr().cast::<u32>()) };
    let bank = if selection.0 == 0 { 1u32 } else { 2u32 };
    let capture = Address32(unsafe { word(stage.0, 0x220c) });
    unsafe { store(capture, bank.wrapping_mul(0x4594).wrapping_add(0xca4f0), delta) };
    let task = unsafe { xm1_pipeline_image_current_task() };
    unsafe { xm1_pipeline_image_configure_task(task, TaskPriority(12)) };
    let stride = unsafe { word(original.0, 12) };
    unsafe { store(output, 16, stride) };
    let top = unsafe { word(stage.0, 0x498) };
    if unsafe { word(stage.0, 0x49c) } & 0x10 != 0 {
        let image = PackedRows { mode: MaybeUninit::uninit(), address: Address32(unsafe { word(output, 4) }), length: MaybeUninit::uninit(), stride: RowStride(stride), width: MaybeUninit::uninit(), height: MaybeUninit::uninit(), flags: MaybeUninit::uninit() };
        for index in 0..top { unsafe { xm1_pipeline_image_write_packed12(&image, index as i32, 0, 0); xm1_pipeline_image_write_packed12(&image, index as i32, 1, 0); } }
        let end = top.wrapping_add(unsafe { word(stage.0, 0x424) });
        let mut filled = top;
        if filled < end {
            while filled < end {
                unsafe { xm1_pipeline_image_write_packed12(&image, filled as i32, 0, xm1_pipeline_sensor_unused_operation() >> 2); xm1_pipeline_image_write_packed12(&image, filled as i32, 1, delta >> 2); }
                filled += 1;
            }
            filled = end;
        }
        let extent = unsafe { word(original.0, 12) }.wrapping_mul(2) / 3;
        while filled < extent { unsafe { xm1_pipeline_image_write_packed12(&image, filled as i32, 0, 0); xm1_pipeline_image_write_packed12(&image, filled as i32, 1, 0); } filled += 1; }
    } else {
        let half_stride = stride >> 1;
        let address = Address32(unsafe { word(output, 4) });
        let second = address.offset(ByteOffset(half_stride.wrapping_mul(2)));
        for index in 0..top { unsafe { store_half(address, index.wrapping_mul(2), 0); store_half(second, index.wrapping_mul(2), 0); } }
        let end = top.wrapping_add(unsafe { word(stage.0, 0x424) });
        let mut filled = top;
        if filled < end {
            while filled < end { unsafe { store_half(address, filled.wrapping_mul(2), xm1_pipeline_sensor_unused_operation() as u16); store_half(second, filled.wrapping_mul(2), delta as u16); } filled += 1; }
            filled = end;
        }
        let extent = unsafe { word(output, 16) } >> 1;
        while filled < extent { unsafe { store_half(address, filled.wrapping_mul(2), 0); store_half(second, filled.wrapping_mul(2), 0); } filled += 1; }
    }
    unsafe { xm1_pipeline_image_configure_task(task, TaskPriority(6)) };
    1
}

#[derive(Clone, Copy)]
#[repr(i32)]
pub enum GainAdjustment { MinusOne = -100, MinusTwoThirds = -66, MinusOneThird = -33, Neutral = 0, OneThird = 33, TwoThirds = 66, One = 100, FourThirds = 133, FiveThirds = 166, Two = 200, SevenThirds = 233, EightThirds = 266, Three = 300 }

#[unsafe(no_mangle)]
pub extern "C" fn xm1_pipeline_sensor_gain_adjustment(adjustment: i32) -> GainAdjustment {
    match adjustment { -3 => GainAdjustment::MinusOne, -2 => GainAdjustment::MinusTwoThirds, -1 => GainAdjustment::MinusOneThird, 1 => GainAdjustment::OneThird, 2 => GainAdjustment::TwoThirds, 3 => GainAdjustment::One, 4 => GainAdjustment::FourThirds, 5 => GainAdjustment::FiveThirds, 6 => GainAdjustment::Two, 7 => GainAdjustment::SevenThirds, 8 => GainAdjustment::EightThirds, 9 => GainAdjustment::Three, _ => GainAdjustment::Neutral }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_capture_gain(stage: SensorStage, output: Address32) {
    unsafe { store(output, 0, half(stage.0, 0x2226) as u32); store(output, 4, half(stage.0, 0x2224) as u32); store(output, 8, half(stage.0, 0x2228) as u32); store(output, 12, 256); }
    if unsafe { word(stage.0, 0x4a8) } == 8 || unsafe { word(stage.0, 0x78) } == 11 {
        let adjustment = xm1_pipeline_sensor_gain_adjustment(unsafe { word(stage.0, 0x1d3c) } as i32);
        let gain = match adjustment {
            GainAdjustment::MinusOne | GainAdjustment::MinusTwoThirds | GainAdjustment::MinusOneThird => (unsafe { word(output, 12).wrapping_mul(byte(stage.0, 0x270e) as u32) }) / 100,
            _ => {
                let percent = match adjustment { GainAdjustment::OneThird => 126u32, GainAdjustment::TwoThirds => 158, GainAdjustment::One => 200, GainAdjustment::FourThirds => 251, GainAdjustment::FiveThirds => 316, GainAdjustment::Two => 400, GainAdjustment::SevenThirds => 503, GainAdjustment::EightThirds => 632, GainAdjustment::Three => 800, _ => 100 };
                unsafe {
                    store(output, 4, word(output, 4) as u16 as u32);
                    store(output, 0, word(output, 0) as u16 as u32);
                    store(output, 8, word(output, 8) as u16 as u32);
                }
                unsafe { word(output, 12) }.wrapping_mul((percent << 8) / 100) >> 8
            }
        };
        unsafe { store(output, 12, gain); };
        if unsafe { word(output, 12) } >= 4096 { unsafe { store(output, 12, 4095) }; }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct FilterLevel(u32);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_filter_parameters(stage: SensorStage, _secondary: i32, _flags: u32, output: Address32) {
    let conversion = unsafe { xm1_pipeline_sensor_select_conversion(ConversionInput(half(stage.0, 0x732) as u32)) };
    let band = unsafe { xm1_pipeline_sensor_conversion_band(conversion) }.0;
    let exposure = unsafe { word(stage.0, 0x648) } as i32;
    let mut level = FilterLevel(0);
    for (index, offset) in [12,8,4].into_iter().enumerate() {
        let key = ConfigurationKey(unsafe { word(Address32(0xe584e0), band.wrapping_mul(16).wrapping_add(offset)) });
        let threshold = unsafe { configuration_byte(key) } as i8 as i32 * 10;
        if exposure <= threshold && unsafe { configuration_byte(key) } != 0x80 { level = FilterLevel(3 - index as u32); break; }
    }
    if unsafe { word(stage.0, 0x5ec) } == 0 {
        level = FilterLevel(unsafe { configuration_byte(ConfigurationKey(word(Address32(0xe58580), level.0.wrapping_mul(4)))) });
    }
    if unsafe { configuration_byte(ConfigurationKey(0xc9)) } & 1 != 0 { unsafe { store(output, 0, 2) }; }
    else if level.0 != 0 && unsafe { configuration_byte(ConfigurationKey(0xb4)) } & 1 != 0 { unsafe { store(output, 0, 1) }; }
    else { unsafe { store(output, 0, 0) }; return; }
    let lookup = |index: u32| unsafe { configuration_byte(ConfigurationKey(word(Address32(0xe58644), level.0.wrapping_add(index.wrapping_mul(4)).wrapping_mul(4)))) as u8 };
    let first = [lookup(0),lookup(1),lookup(2),lookup(3)];
    unsafe {
        store_half(output, 4, 0);
        for offset in 6..10 { store_byte(output, offset, 0); }
        store_half(output, 10, 0);
        store_half(output, 12, 0);
        for (index, value) in first.into_iter().enumerate() { store_byte(output, 14 + index as u32, value); }
    }
    let second = [lookup(4),lookup(5),lookup(6),lookup(7)];
    unsafe {
        store_half(output, 18, 0);
        for offset in 20..24 { store_byte(output, offset, 0); }
        store_half(output, 24, 0);
        store_half(output, 26, 0);
        for (index, value) in second.into_iter().enumerate() { store_byte(output, 28 + index as u32, value); }
    }
    let final_values = [lookup(8),lookup(9),lookup(10),lookup(11),lookup(12),lookup(13),lookup(14),lookup(15),lookup(16),lookup(17)];
    for (index, value) in final_values.into_iter().enumerate() { unsafe { store_byte(output, 32 + index as u32, value) }; }
}

#[derive(Clone, Copy)]
enum ProfileGeometryFamily { First, Second, Third }
impl ProfileGeometryFamily {
    fn parse(feature: u32) -> Self { match feature { 0 => Self::First, 1 => Self::Second, _ => Self::Third } }
    fn index(self) -> u32 { match self { Self::First => 0, Self::Second => 1, Self::Third => 2 } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_profile_geometry(stage: SensorStage, _source: ImagePlane, selection: CaptureSelection, output: Address32) {
    let feature = unsafe { xm1_pipeline_sensor_profile_feature(ProfileId(word(stage.0, 0x2260))) };
    unsafe { store(output, 0, if configuration_byte(ConfigurationKey(0xc9)) & 2 != 0 { 2 } else { 1 }); }
    let family = ProfileGeometryFamily::parse(feature);
    let index = selection.0.wrapping_add(family.index() * 2);
    let count_key = ConfigurationKey(unsafe { word(Address32(0xe585d8), index.wrapping_mul(4)) });
    let region = CalibrationBankId(unsafe { word(Address32(0xe5862c), index.wrapping_mul(4)) });
    let _ = unsafe { xm1_pipeline_sensor_cancel_configuration(ConfigurationChannel(6)) };
    let count = unsafe { configuration_byte(count_key) };
    let mut source = unsafe { xm1_pipeline_image_calibration_data(region) };
    if selection.0 == 1 {
        let first_key = ConfigurationKey(unsafe { word(Address32(0xe585d8), family.index() * 8) });
        source = source.offset(ByteOffset(unsafe { configuration_byte(first_key) }.wrapping_mul(4)));
    }
    for index in 0..64 {
        if index < count {
            let offset = unsafe { word(source, index * 4) } & 0x1fff;
            unsafe {
                store(output, 44 + index * 4, offset);
                let adjusted = word(stage.0, 0x498).wrapping_add(offset);
                store(output, 44 + index * 4, adjusted);
            }
        } else { unsafe { store(output, 44 + index * 4, 0) }; }
    }
    unsafe { store(output, 4, 0); store(output, 8, 0); store(output, 12, 0); }
    for index in 0..7 {
        let value = unsafe { xm1_pipeline_sensor_configuration_word(ConfigurationKey(0x2601 + index * 2)) };
        unsafe { store(output, 16 + index * 4, value); }
    }
}
