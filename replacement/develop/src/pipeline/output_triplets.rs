use super::shared::{Address32, ByteOffset};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct StatisticsAddress(pub(super) Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ByteStride(pub(super) u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ExposurePercent(pub(super) u32);

unsafe fn byte(base: Address32, offset: u32) -> u32 {
    unsafe { base.offset(ByteOffset(offset)).byte() as u32 }
}
unsafe fn half(base: Address32, offset: u32) -> u32 {
    unsafe { base.offset(ByteOffset(offset)).half() as u32 }
}
unsafe fn word(base: Address32, offset: u32) -> u32 {
    unsafe { base.offset(ByteOffset(offset)).word() }
}
fn arm_lsl(value: u32, shift: u32) -> u32 {
    if shift & 255 < 32 { value << (shift & 255) } else { 0 }
}

unsafe extern "C" {
    fn xm1_pipeline_image_output_arithmetic_fault(value: u32) -> !;
}

fn unsigned_quotient(numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 {
        unsafe { xm1_pipeline_image_output_arithmetic_fault(if numerator == 0 { 0 } else { u32::MAX }) }
    }
    numerator / denominator
}

fn signed_quotient(numerator: i32, denominator: i32) -> i32 {
    if denominator == 0 {
        let value = if numerator > 0 { i32::MAX } else if numerator < 0 { i32::MIN } else { 0 };
        unsafe { xm1_pipeline_image_output_arithmetic_fault(value as u32) }
    }
    numerator.wrapping_div(denominator)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_statistics_gamma(value: u32, table: Address32) -> u32 {
    let mut boundaries = [0u32; 10];
    for index in 1..10 {
        boundaries[index] = boundaries[index - 1].wrapping_add(arm_lsl(1, unsafe { byte(table, index as u32) } + 6));
    }
    let value = value.wrapping_add(unsafe { half(table, 66) });
    let maximum = unsafe { half(table, 68) };
    if value >= boundaries[9] {
        return unsafe { half(table, 64) }.min(maximum);
    }
    let mut segment = 1usize;
    while value >= boundaries[segment] { segment += 1; }
    let step = boundaries[segment].wrapping_sub(boundaries[segment - 1]) / 3;
    let displacement = value.wrapping_sub(boundaries[segment - 1]);
    let third = if displacement < step { 0 } else if displacement < step.wrapping_mul(2) { 1 } else { 2 };
    let fraction = displacement.wrapping_sub(step.wrapping_mul(third));
    let sample = (segment as u32 - 1) * 3 + third;
    let lower = unsafe { half(table, 10 + sample * 2) };
    let upper = unsafe { half(table, 12 + sample * 2) };
    unsigned_quotient(lower.wrapping_mul(step.wrapping_sub(fraction)).wrapping_add(upper.wrapping_mul(fraction)), step).min(maximum)
}

fn signed_round(value: i32, bits: u32) -> i32 {
    value.wrapping_add(if value > 0 { 1 << (bits - 1) } else { -(1 << (bits - 1)) }) / (1 << bits)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_write_noise_pixels(
    output: Address32,
    stride: ByteStride,
    statistics: StatisticsAddress,
    exposure: ExposurePercent,
    gains: Address32,
    matrix: Address32,
    gamma: Address32,
    chroma: Address32,
) {
    let statistics = statistics.0;
    let width = unsafe { byte(statistics, 16) };
    let height = unsafe { byte(statistics, 17) };
    let count = width * height;
    let workspace = Address32(0x41716618);
    if count != 0 {
        let area = unsafe { half(statistics, 18) * half(statistics, 20) };
        let red = unsafe { byte(statistics, 22) };
        let green = unsafe { byte(statistics, 23) + byte(statistics, 25) };
        let blue = unsafe { byte(statistics, 24) };
        let total = (red + green + blue) & 255;
        let counts = [red, green, blue].map(|v| signed_quotient(v.wrapping_mul(area).wrapping_add(total >> 1) as i32, total as i32) as u32);
        let planes = [unsafe { word(statistics, 0) }, unsafe { word(statistics, 4) }, unsafe { word(statistics, 8) }, unsafe { word(statistics, 12) }];
        let use_gains = unsafe { byte(gains, 0) } == 1;
        let use_matrix = unsafe { byte(matrix, 0) } == 1;
        let use_gamma = unsafe { byte(gamma, 0) } == 1;
        let use_chroma = unsafe { byte(chroma, 0) } != 0;
        for index in 0..count {
            let offset = index * 4;
            let mut rgb = [
                unsigned_quotient(unsafe { word(Address32(planes[0]), offset) }.wrapping_add(counts[0] >> 1), counts[0]),
                unsigned_quotient(unsafe { word(Address32(planes[1]), offset) }.wrapping_add(unsafe { word(Address32(planes[3]), offset) }).wrapping_add(counts[1] >> 1), counts[1]),
                unsigned_quotient(unsafe { word(Address32(planes[2]), offset) }.wrapping_add(counts[2] >> 1), counts[2]),
            ];
            if use_gains {
                let scale = [unsafe { half(gains, 2) }, unsafe { (half(gains, 4) + half(gains, 8)) >> 1 }, unsafe { half(gains, 6) }];
                for channel in 0..3 { rgb[channel] = rgb[channel].wrapping_mul(scale[channel]).wrapping_add(128) >> 8; }
            }
            for channel in &mut rgb { *channel = channel.wrapping_mul(exposure.0) / 100; }
            if use_matrix {
                let mut converted = [0u32; 3];
                for row in 0..3 {
                    let mut sum = 512u32;
                    for column in 0..3 {
                        let coefficient = unsafe { half(matrix, 4 + (row * 3 + column) * 2) } as i16 as i32 as u32;
                        sum = sum.wrapping_add(coefficient.wrapping_mul(rgb[column as usize]));
                    }
                    converted[row as usize] = (sum >> 10).min(unsafe { half(matrix, 2) });
                }
                rgb = converted;
            }
            if use_gamma {
                for channel in &mut rgb { *channel = unsafe { xm1_pipeline_image_statistics_gamma(*channel, gamma) }; }
            }
            let [red, green, blue] = rgb.map(|value| value.wrapping_add(32) >> 6);
            let y = red.wrapping_mul(77).wrapping_add(green.wrapping_mul(151)).wrapping_add(128).wrapping_add(blue.wrapping_mul(28)) >> 8;
            let mut cr = signed_round(red.wrapping_sub(y).wrapping_mul(183) as i32, 8);
            let mut cb = signed_round(blue.wrapping_sub(y).wrapping_mul(144) as i32, 8);
            if use_chroma {
                let a = unsafe { half(chroma, 2) } as i16 as i32;
                let b = unsafe { half(chroma, 4) } as i16 as i32;
                let c = unsafe { half(chroma, 6) } as i16 as i32;
                let d = unsafe { half(chroma, 8) } as i16 as i32;
                let next_cr = signed_round(cr.wrapping_mul(a).wrapping_add(cb.wrapping_mul(b)), 10);
                cb = signed_round(cr.wrapping_mul(d).wrapping_add(cb.wrapping_mul(c)), 10);
                cr = next_cr;
            }
            unsafe {
                workspace.offset(ByteOffset(index)).write_byte(y.min(255) as u8);
                workspace.offset(ByteOffset(index + 256)).write_byte(cr.wrapping_add(128).clamp(0, 255) as u8);
                workspace.offset(ByteOffset(index + 512)).write_byte(cb.wrapping_add(128).clamp(0, 255) as u8);
            }
        }
    }
    let mut output = output;
    let mut row_offset = 0u32;
    for _ in 0..height {
        let mut column = 0u32;
        while column < width {
            let index = row_offset.wrapping_add(column) & 65535;
            unsafe {
                output.write_byte(byte(workspace, index) as u8);
                output.offset(ByteOffset(1)).write_byte(byte(workspace, index + 1) as u8);
                output.offset(ByteOffset(2)).write_byte(((byte(workspace, index + 512) + byte(workspace, index + 513) + 1) >> 1) as u8);
                output.offset(ByteOffset(3)).write_byte(((byte(workspace, index + 256) + byte(workspace, index + 257) + 1) >> 1) as u8);
            }
            column = column.wrapping_add(2) & 255;
            output = output.offset(ByteOffset(4));
        }
        output = output.offset(ByteOffset(stride.0.wrapping_sub(width * 2)));
        row_offset = row_offset.wrapping_add(width) & 65535;
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ConversionIndex(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum CompressionBand {
    Unspecified = 0,
    First = 1,
    Second = 2,
    Third = 3,
    Fourth = 4,
    Fifth = 5,
    Sixth = 6,
    Seventh = 7,
    Eighth = 8,
    Ninth = 9,
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_compression_band(index: ConversionIndex) -> CompressionBand {
    match index.0 {
        6..=8 => CompressionBand::First,
        10..=12 => CompressionBand::Second,
        13..=15 => CompressionBand::Third,
        16..=18 => CompressionBand::Fourth,
        19..=21 => CompressionBand::Fifth,
        22..=24 => CompressionBand::Sixth,
        25 => CompressionBand::Seventh,
        26..=27 => CompressionBand::Eighth,
        28 => CompressionBand::Ninth,
        _ => CompressionBand::Unspecified,
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CalibrationRate(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TenthsFactor(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
struct RateCurveIndex(u8);

impl RateCurveIndex {
    fn from_rate(rate: CalibrationRate) -> Self {
        let index = rate.0.wrapping_sub(10) as u8;
        Self(if index <= 40 { index } else { 0 })
    }

    unsafe fn adjustment(self) -> i32 {
        let value = unsafe { Address32(0x00ef50e8).offset(ByteOffset(self.0 as u32)).byte() } as i8;
        i32::from(value) * 10
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_rate_adjustment(rate: CalibrationRate) -> i32 {
    unsafe { RateCurveIndex::from_rate(rate).adjustment() }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_scale_triplet_tenths(values: Address32, factor: TenthsFactor) {
    for offset in [0, 2, 4] {
        let field = values.offset(ByteOffset(offset));
        let value = u32::from(unsafe { field.half() }).wrapping_mul(factor.0) / 10;
        unsafe { field.write_half(value as u16) };
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ImageOperation(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum OutputFormat {
    Unspecified = 0,
    Selected = 1,
    Standard = 2,
    Special = 3,
    Alternate = 5,
    Paired = 7,
    Raw = 8,
    Ninth = 9,
    Tenth = 10,
    Eleventh = 11,
}

unsafe extern "C" {
    fn xm1_develop_capture_option_selection() -> u32;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_output_format(job: Address32, operation: ImageOperation) -> OutputFormat {
    match operation.0 {
        1 | 2 => OutputFormat::Paired,
        3 => if unsafe { xm1_develop_capture_option_selection() } == 1 { OutputFormat::Selected } else { OutputFormat::Alternate },
        4 => OutputFormat::Eleventh,
        6..=8 | 13..=14 => {
            let capture = unsafe { word(job, 0x4b4) };
            if capture == 25 || (capture == 1 && unsafe { word(job, 0x710) } & 0x400 != 0) {
                OutputFormat::Special
            } else {
                OutputFormat::Standard
            }
        }
        9 => OutputFormat::Ninth,
        10 => OutputFormat::Tenth,
        11 | 12 => OutputFormat::Raw,
        _ => OutputFormat::Unspecified,
    }
}

unsafe extern "C" {
    fn xm1_pipeline_sensor_collect_settings(stage: Address32, descriptor: Address32, packet: Address32, operation: ImageOperation, option: u32);
    fn xm1_pipeline_sensor_configure_descriptor(descriptor: Address32, format: OutputFormat, flags: u32);
    fn xm1_pipeline_sensor_submit_descriptor(output_record: Address32, image_descriptor: Address32);
}

#[derive(Clone, Copy)]
enum NoiseTablePass {
    Prepare,
    Finalize,
}

unsafe fn construct_noise_tables(job: Address32, output_record: Address32, image_descriptor: Address32, pass: NoiseTablePass) {
    let (operation, packet) = match pass {
        NoiseTablePass::Prepare => (ImageOperation(13), Address32(0x01568328)),
        NoiseTablePass::Finalize => (ImageOperation(14), Address32(0x015689a8)),
    };
    let format = unsafe { xm1_pipeline_image_output_format(job, operation) };
    unsafe {
        xm1_pipeline_sensor_collect_settings(job, image_descriptor, packet, operation, 0);
        image_descriptor.offset(ByteOffset(0xf0)).write_word(0);
        image_descriptor.offset(ByteOffset(0x140)).write_word(0);
        xm1_pipeline_sensor_configure_descriptor(image_descriptor, format, 0);
        xm1_pipeline_sensor_submit_descriptor(output_record, image_descriptor);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_prepare_noise_tables(job: Address32, output_record: Address32, image_descriptor: Address32) {
    unsafe { construct_noise_tables(job, output_record, image_descriptor, NoiseTablePass::Prepare) };
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_finalize_noise_tables(job: Address32, output_record: Address32, image_descriptor: Address32) {
    unsafe { construct_noise_tables(job, output_record, image_descriptor, NoiseTablePass::Finalize) };
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct WindowSelection(pub(super) u32);

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_window_selection(value: WindowSelection) -> u32 {
    match value.0 { 1 => 1, 2 => 2, _ => 0 }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_window_zero_selection(value: WindowSelection) -> u32 {
    match value.0 { 0 => 1, 1 => 2, _ => 0 }
}

unsafe extern "C" {
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_configure_task(task: u32, priority: u32) -> i32;
    fn xm1_pipeline_image_rate_grid_weights(mode: u32, profile: i32, output: *mut u16);
    fn xm1_pipeline_image_buffer_views(stage: Address32, source: Address32, output: *mut u32, phase: *mut u32, enabled: *mut u16);
    fn xm1_pipeline_image_scan_components(context: Address32, components: *mut u16);
}

unsafe fn copy_bytes(destination: Address32, source: Address32, count: u32) {
    for offset in 0..count {
        unsafe { destination.offset(ByteOffset(offset)).write_byte(source.offset(ByteOffset(offset)).byte()) };
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct TripletOverride(pub(super) u32);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_apply_triplet_tuning(
    job: Address32,
    image: Address32,
    primary: Address32,
    secondary: Address32,
    source: Address32,
    components: *mut u16,
    use_levels: TripletOverride,
) -> u32 {
    let task = unsafe { xm1_pipeline_image_current_task() };
    let mut weights = core::mem::MaybeUninit::<[u16; 3]>::uninit();
    unsafe { xm1_pipeline_image_rate_grid_weights(5, half(job, 0x736) as i16 as i32, weights.as_mut_ptr().cast()) };
    let mut views = core::mem::MaybeUninit::<[u32; 7]>::uninit();
    let mut phase = core::mem::MaybeUninit::<[u32; 2]>::uninit();
    let parameters = Address32(0x01569778);
    unsafe {
        xm1_pipeline_image_buffer_views(job, image, views.as_mut_ptr().cast(), phase.as_mut_ptr().cast(), parameters.offset(ByteOffset(0x23e)).0 as *mut u16);
    }
    let mut raw_source = core::mem::MaybeUninit::<[u32; 2]>::uninit();
    let raw_source_address = Address32(raw_source.as_mut_ptr() as u32);
    unsafe {
        raw_source_address.write_word(word(source, 4));
        raw_source_address.offset(ByteOffset(4)).write_half(half(job, 0x19b6) as u16);
        copy_bytes(parameters, job.offset(ByteOffset(0x19b8)), 0x1bc);
        for channel in 0..3 {
            parameters.offset(ByteOffset(0x1c2 + channel * 2)).write_half(half(job, 0x21f8 + channel * 2) as u16);
        }
        parameters.offset(ByteOffset(0x1c8)).write_byte(word(job, 0x1658) as u8);
        let weights = weights.assume_init();
        for (channel, weight) in weights.into_iter().enumerate() {
            parameters.offset(ByteOffset(0x1ca + channel as u32 * 2)).write_half(weight);
        }
        parameters.offset(ByteOffset(0x1d0)).write_word(word(job, 0x718));
        copy_bytes(parameters.offset(ByteOffset(0x1d4)), job.offset(ByteOffset(0x1d40)), 32);
        parameters.offset(ByteOffset(0x1f4)).write_half(half(job, 0x6b4) as u16);
        parameters.offset(ByteOffset(0x1f6)).write_half(half(job, 0x2208) as u16);
        parameters.offset(ByteOffset(0x1f8)).write_word(word(primary, 4));
        parameters.offset(ByteOffset(0x1fc)).write_word(word(secondary, 4));
        parameters.offset(ByteOffset(0x204)).write_word(views.as_mut_ptr() as u32);
        parameters.offset(ByteOffset(0x21c)).write_word(raw_source_address.0);
        for (destination, origin) in [(0x200, 0x220c), (0x208, 0x6b8), (0x20c, 0x6bc), (0x228, 0x508), (0x22c, 0x51c), (0x230, 0x2254), (0x234, 0x5c4), (0x244, 0x504)] {
            parameters.offset(ByteOffset(destination)).write_word(word(job, origin));
        }
        let levels = if use_levels.0 == 1 { 0x71c } else { 0x2224 };
        for channel in 0..3 {
            parameters.offset(ByteOffset(0x238 + channel * 2)).write_half(half(job, levels + channel * 2) as u16);
        }
        parameters.offset(ByteOffset(0x240)).write_word(1);
        parameters.offset(ByteOffset(0x24c)).write_word(xm1_pipeline_image_window_selection(WindowSelection(word(job, 0x2200))));
        parameters.offset(ByteOffset(0x250)).write_word(xm1_pipeline_image_window_zero_selection(WindowSelection(word(job, 0x2204))));
        let flags = word(job, 0x49c);
        let table = Address32(0x00e54068);
        let mut selected = 3;
        for row in 0..3 {
            let mask = word(table, row * 24 + 4);
            if flags & mask == mask { selected = row; break; }
        }
        parameters.offset(ByteOffset(0x248)).write_word(word(table, selected * 24 + 12));
        let capture = word(job, 0x4b4);
        let mode = if capture != 1 { capture } else if word(job, 0x710) & 0x400 != 0 { 25 } else { word(job, 0x638) };
        parameters.offset(ByteOffset(0x224)).write_word(mode);
        let phase = phase.assume_init();
        parameters.offset(ByteOffset(0x210)).write_word(phase[0]);
        parameters.offset(ByteOffset(0x214)).write_word(phase[1]);
        xm1_pipeline_image_configure_task(task, 12);
        xm1_pipeline_image_scan_components(parameters, components);
        xm1_pipeline_image_configure_task(task, 6);
    }
    1
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ProfileDelta(pub(super) i32);

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_profile_delta_index(value: ProfileDelta) -> u32 {
    let shifted = (value.0 as u32).wrapping_add(9);
    if shifted <= 18 { shifted } else { 9 }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_profile_thirds(value: ProfileDelta) -> i32 {
    match value.0 {
        -3 => -100, -2 => -66, -1 => -33,
        1 => 33, 2 => 66, 3 => 100, 4 => 133, 5 => 166,
        6 => 200, 7 => 233, 8 => 266, 9 => 300,
        _ => 0,
    }
}

#[derive(Clone, Copy)]
enum ProfileSelectorTable {
    Feature,
    Geometry,
}

impl ProfileSelectorTable {
    fn layout(self) -> (Address32, u32) {
        match self {
            Self::Feature => (Address32(0x00817688), 14),
            Self::Geometry => (Address32(0x008171f0), 62),
        }
    }

    unsafe fn select(self, value: u32) -> u32 {
        let (base, rows) = self.layout();
        if value == 0 { return unsafe { word(base, 4) }; }
        for row in 1..rows {
            if unsafe { word(base, row * 8) } == value {
                return unsafe { word(base, row * 8 + 4) };
            }
        }
        0
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ProfileSelection(pub(super) u32);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_feature_index(value: ProfileSelection) -> u32 {
    unsafe { ProfileSelectorTable::Feature.select(value.0) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_profile_geometry_index(value: ProfileSelection) -> u32 {
    unsafe { ProfileSelectorTable::Geometry.select(value.0) }
}

unsafe extern "C" {
    fn xm1_pipeline_image_metadata_sensitivity_threshold(value: u32) -> u32;
}


#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_stamp_threshold(value: CalibrationRate) -> u32 {
    unsafe { xm1_pipeline_image_metadata_sensitivity_threshold(value.0.wrapping_add(50)) }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CalibrationBankId(pub(super) u32);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_calibration_data(bank: CalibrationBankId) -> Address32 {
    let offset = match bank.0 {
        1 => 0x11c840,
        2 => 0x2bb8e0,
        3 => 0x3b4940,
        4 => 0x3b8ba8,
        5 => 0x3c31b0,
        6 => 0x76500,
        7 => 0x76600,
        8 => 0x76700,
        _ => 0x76800,
    };
    Address32(unsafe { Address32(0x0096ca50).word() }).offset(ByteOffset(offset))
}

#[repr(C)]
pub(super) struct CalibrationTriplet {
    channels: [u16; 3],
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct CalibrationRequest(pub(super) u32);

unsafe extern "C" {
    fn xm1_pipeline_image_conversion_index(value: u32) -> u32;
    fn xm1_pipeline_image_calibrate_profile(input: Address32, output: *mut CalibrationTriplet, metadata: Address32);
    fn xm1_pipeline_image_clamp_geometry(unused: u32, input: *const CalibrationTriplet, rate: u32, output: *mut CalibrationTriplet, effective: *mut u8, exposure: i16, metadata: Address32);
    fn xm1_pipeline_image_clamp_profile_output(input: *mut CalibrationTriplet, output: *mut CalibrationTriplet, metadata: Address32);
    fn xm1_pipeline_image_apply_negative_profile_gain(value: i32, values: *mut CalibrationTriplet, output: *mut u8);
    fn xm1_pipeline_image_profile_apply_positive(value: i32, values: *mut CalibrationTriplet);
    fn xm1_pipeline_image_profile_apply_difference(first: i32, second: i32, values: *mut CalibrationTriplet);
}

unsafe fn update_triplet_exposure(job: Address32) {
    let rate = CalibrationRate(unsafe { byte(job, 0x1658) });
    let adjustment = unsafe { xm1_pipeline_image_calibration_rate_adjustment(rate) };
    let value = (unsafe { half(job, 0x2244) }).wrapping_add(adjustment as u32) as u16;
    unsafe {
        job.offset(ByteOffset(0x2244)).write_half(value);
        job.offset(ByteOffset(0x732)).write_half(value);
        job.offset(ByteOffset(0x66c)).write_word(xm1_pipeline_image_metadata_sensitivity_threshold(u32::from(value)));
    }
}

unsafe fn store_triplet(job: Address32, offset: u32, values: &CalibrationTriplet) {
    for (channel, value) in values.channels.iter().enumerate() {
        unsafe { job.offset(ByteOffset(offset + channel as u32 * 2)).write_half(*value) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_process_output_triplets(job: Address32, request: CalibrationRequest) -> u32 {
    let task = unsafe { xm1_pipeline_image_current_task() };
    let mut metadata = Address32(unsafe { word(job, 0x220c) });
    if unsafe { word(job, 0x2748) } != 1 && request.0 != 1 {
        if (unsafe { word(job, 0x4a8) }).wrapping_sub(4) <= 2 {
            unsafe { update_triplet_exposure(job) };
        }
        return 1;
    }
    let parameters = Address32(0x0156ae98);
    unsafe {
        parameters.write_word(xm1_pipeline_image_profile_delta_index(ProfileDelta(word(job, 0x4f8) as i32)));
        parameters.offset(ByteOffset(4)).write_word(xm1_pipeline_image_profile_delta_index(ProfileDelta(word(job, 0x4fc) as i32)));
        parameters.offset(ByteOffset(8)).write_word(xm1_pipeline_image_conversion_index(byte(job, 0x65c)));
        parameters.offset(ByteOffset(12)).write_word(xm1_pipeline_image_profile_feature_index(ProfileSelection(word(job, 0x530))));
        for channel in 0..3 {
            parameters.offset(ByteOffset(16 + channel * 2)).write_half(half(job, 0x2224 + channel * 2) as u16);
        }
        parameters.offset(ByteOffset(24)).write_word(0);
    }
    let alternate = request.0 == 1;
    if alternate {
        metadata = Address32(0);
    }
    if !alternate || unsafe { word(job, 0x4a8) } == 0 {
        unsafe { xm1_pipeline_image_configure_task(task, 12) };
    }
    let rate = unsafe { word(job, 0x1654) };
    unsafe { xm1_pipeline_image_scale_triplet_tenths(parameters.offset(ByteOffset(16)), TenthsFactor(rate)) };
    let mut calibrated = core::mem::MaybeUninit::<CalibrationTriplet>::uninit();
    unsafe { xm1_pipeline_image_calibrate_profile(parameters, calibrated.as_mut_ptr(), metadata) };
    let mut adjusted = core::mem::MaybeUninit::<CalibrationTriplet>::uninit();
    let mut effective = core::mem::MaybeUninit::<u8>::uninit();
    unsafe {
        xm1_pipeline_image_clamp_geometry(
            xm1_pipeline_image_profile_geometry_index(ProfileSelection(word(job, 0x2250))),
            calibrated.as_ptr(),
            rate & 255,
            adjusted.as_mut_ptr(),
            effective.as_mut_ptr(),
            half(job, 0x736) as i16,
            metadata,
        );
        xm1_pipeline_image_profile_apply_positive(half(job, 0x164c) as i16 as i32, adjusted.as_mut_ptr());
    }
    if unsafe { word(job, 0x78) } == 11 {
        let first = unsafe { word(job, 0x1888) };
        let second = unsafe { word(job, 0x6c8) };
        let suppress_difference = unsafe { word(job, 0x63c) } == 2
            && unsafe { word(job, 0x64c) } != unsafe { word(job, 0x648) }
            && ((first == 3 && second.wrapping_sub(1) <= 1) || (first == 2 && second == 1));
        if !suppress_difference {
            unsafe { xm1_pipeline_image_profile_apply_difference(first as i32, second as i32, adjusted.as_mut_ptr()) };
        }
    }
    if unsafe { word(job, 0x4a8) } == 8 || unsafe { word(job, 0x78) } == 11 {
        unsafe {
            xm1_pipeline_image_apply_negative_profile_gain(
                xm1_pipeline_image_profile_thirds(ProfileDelta(word(job, 0x1d3c) as i32)),
                adjusted.as_mut_ptr(),
                job.offset(ByteOffset(0x270e)).0 as *mut u8,
            );
        }
    }
    let mut output = core::mem::MaybeUninit::<CalibrationTriplet>::uninit();
    unsafe { xm1_pipeline_image_clamp_profile_output(adjusted.as_mut_ptr(), output.as_mut_ptr(), metadata) };
    let mut output = unsafe { output.assume_init() };
    if alternate {
        unsafe { store_triplet(job, 0x1d34, &output) };
    } else {
        if unsafe { word(job, 0x78) } == 11 {
            let adjustment = unsafe { xm1_pipeline_image_calibration_rate_adjustment(CalibrationRate(byte(job, 0x1658))) };
            let value = unsafe { half(job, 0x2244) }.wrapping_sub(adjustment as u32) as u16;
            unsafe { job.offset(ByteOffset(0x2244)).write_half(value) };
        }
        unsafe {
            job.offset(ByteOffset(0x1658)).write_word(u32::from(effective.assume_init()));
            store_triplet(job, 0x2224, &output);
        }
    }
    if unsafe { word(job, 0x604) } == 1 {
        if unsafe { word(job, 0x63c) } == 2 && unsafe { word(job, 0x1888) } == 3 && unsafe { word(job, 0x6c8) } == 2 {
            let mut adjusted = unsafe { adjusted.assume_init() };
            for value in &mut adjusted.channels { *value = value.wrapping_mul(2); }
            unsafe {
                xm1_pipeline_image_clamp_profile_output(&mut adjusted, &mut output, metadata);
                store_triplet(job, 0x222a, &output);
            }
        } else if unsafe { byte(job, 0x19b4) } == 0 {
            unsafe { store_triplet(job, 0x222a, &output) };
        }
    }
    if !alternate || unsafe { word(job, 0x4a8) } == 0 {
        unsafe { xm1_pipeline_image_configure_task(task, 6) };
    }
    if request.0 == 0 {
        unsafe { update_triplet_exposure(job) };
    }
    1
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct OverlayId(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct OverlayTimeout(pub(super) u32);

#[repr(C)]
struct OverlayMessage {
    event: u16,
    sender: u8,
    destination: u8,
    length: u32,
    payload: Address32,
}

unsafe extern "C" {
    fn xm1_pipeline_image_wait_reserved(handle: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(handle: u32) -> i32;
    fn xm1_pipeline_image_send_message(channel: u32, message: *const OverlayMessage) -> i32;
    fn xm1_pipeline_image_overlay_trace(text: Address32);
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn xm1_pipeline_image_boot_descriptor() -> Address32 {
    Address32(0x0061c000)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_overlay_group() -> Address32 {
    Address32(unsafe { xm1_pipeline_image_boot_descriptor().offset(ByteOffset(4)).word() })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_find_overlay_unit(overlay: OverlayId) -> Address32 {
    let group = unsafe { xm1_pipeline_image_overlay_group() };
    let mut unit = Address32(unsafe { group.word() });
    loop {
        let index = unsafe { unit.word() };
        if index == u32::MAX { return Address32(0); }
        if index == overlay.0 { return unit; }
        unit = unit.offset(ByteOffset(24));
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_wait_overlay(overlay: OverlayId, timeout: OverlayTimeout) -> i32 {
    if overlay.0 > 3 { return 0; }
    let unit = unsafe { xm1_pipeline_image_find_overlay_unit(overlay) };
    if unit.0 == 0 { loop {} }
    if unsafe { word(unit, 16) } != 0 { return 0; }
    if timeout.0 == 0 { return -50; }
    let result = unsafe { xm1_pipeline_image_wait_reserved(word(unit, 20), timeout.0) };
    if result != 0 {
        if result == -50 { return -50; }
        loop {}
    }
    if unsafe { xm1_pipeline_image_signal_reserved(word(unit, 20)) } != 0 { loop {} }
    unsafe { xm1_pipeline_image_overlay_trace(Address32(0x00782070)) };
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_wait_overlay_channel(channel: OverlayId) {
    let semaphore = unsafe { Address32(0x00817700).offset(ByteOffset(channel.0.wrapping_mul(4))).word() };
    if semaphore == 0 { loop {} }
    unsafe {
        xm1_pipeline_image_wait_reserved(semaphore, u32::MAX);
        xm1_pipeline_image_signal_reserved(semaphore);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_cancel_overlay_channel(channel: OverlayId) -> i32 {
    let flag = unsafe { Address32(0x00944440).offset(ByteOffset(channel.0.wrapping_mul(4))).word() };
    if flag == 1 { return 0; }
    if channel.0 > 8 { return -30; }
    let request = [0u32; 2];
    let message = OverlayMessage {
        event: 0x4c00 + channel.0 as u16,
        sender: unsafe { xm1_pipeline_image_current_task() } as u8,
        destination: 0x3c,
        length: 8,
        payload: Address32(request.as_ptr() as u32),
    };
    if unsafe { xm1_pipeline_image_send_message(0x3c, &message) } != 0 { return -255; }
    unsafe { xm1_pipeline_image_wait_overlay_channel(channel) };
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_cancel_overlay(channel: OverlayId) -> i32 {
    unsafe { xm1_pipeline_image_cancel_overlay_channel(channel) }
}

unsafe extern "C" {
    fn xm1_buffers_configuration_byte(key: u32) -> u32;
    fn xm1_buffers_configuration_half(key: u32) -> u16;
    fn xm1_pipeline_image_configuration_signed_byte(key: u32) -> i32;
    fn xm1_pipeline_image_configuration_u24(key: u32) -> u32;
    fn xm1_pipeline_image_process_calibration_overlay(job: Address32, request: Address32) -> i32;
    fn xm1_pipeline_image_send_notification(task: u32, mask: u32) -> i32;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_notify_output_result(result: i32) {
    unsafe {
        Address32(0x0154d960).write_word(if result == 0 { 0 } else { 7 });
        xm1_pipeline_image_send_notification(word(Address32(0x0155ccac), 8), 16);
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct StampRequestOption(pub(super) u32);

struct StampBand(u8);

#[derive(Clone, Copy)]
enum StampFamily {
    Normal = 0,
    Alternate = 1,
}

#[derive(Clone, Copy)]
enum StampThreshold {
    First = 0,
    Second = 2,
    Third = 4,
}

struct StampThresholdRow(Address32);

impl StampThresholdRow {
    fn new(family: StampFamily, band: StampBand) -> Self {
        Self(Address32(0x00e58bb0 + family as u32 * 54 + u32::from(band.0) * 6))
    }

    unsafe fn key(&self, threshold: StampThreshold) -> ConfigurationKey {
        ConfigurationKey(unsafe { half(self.0, threshold as u32) })
    }
}

fn stamp_band(index: u32) -> StampBand {
    StampBand(match index {
        9..=11 => 1,
        12..=14 => 2,
        15..=17 => 3,
        18..=20 => 4,
        21..=23 => 5,
        24..=25 => 6,
        26..=27 => 7,
        28 => 8,
        _ => 0,
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_prepare_output_stamp(
    job: Address32,
    image: Address32,
    _option: StampRequestOption,
    _argument: Address32,
) -> u32 {
    let request = Address32(0x0156aeb4);
    for offset in (0..32).step_by(4) {
        unsafe { request.offset(ByteOffset(offset)).write_word(0) };
    }
    if unsafe { xm1_develop_capture_option_selection() } == 3 { return 1; }
    unsafe { xm1_pipeline_image_cancel_overlay(OverlayId(6)) };
    let packed = unsafe { word(job, 0x49c) } & 16 != 0;
    let offset = unsafe { word(job, 0x498) };
    let pixel_offset = if packed { (offset.wrapping_mul(3) as i32).wrapping_div(2) as u32 } else { offset.wrapping_mul(2) };
    let stride = unsafe { word(image, 12) };
    unsafe {
        request.write_word(1);
        request.offset(ByteOffset(4)).write_word(u32::from(!packed));
        request.offset(ByteOffset(8)).write_word(word(image, 4).wrapping_add(pixel_offset));
        request.offset(ByteOffset(12)).write_word(if packed { stride.wrapping_mul(2) / 3 } else { stride >> 1 });
        request.offset(ByteOffset(28)).write_word(0x00e58890);
    }
    let band = stamp_band(unsafe { xm1_pipeline_image_stamp_threshold(CalibrationRate(half(job, 0x732))) });
    let family = if unsafe { word(job, 0x4a8) } == 0 { StampFamily::Normal } else { StampFamily::Alternate };
    let keys = StampThresholdRow::new(family, band);
    let first = unsafe { xm1_pipeline_image_configuration_signed_byte(keys.key(StampThreshold::First).0) }.wrapping_mul(10);
    let second = unsafe { xm1_pipeline_image_configuration_signed_byte(keys.key(StampThreshold::Second).0) }.wrapping_mul(10);
    let third = unsafe { xm1_pipeline_image_configuration_signed_byte(keys.key(StampThreshold::Third).0) }.wrapping_mul(10);
    let value = unsafe { word(job, 0x648) } as i32;
    if value > first { return 1; }
    let bank = if value > second { 0 } else if value > third || third == -128 { 1 } else { 2 };
    let data = unsafe { xm1_pipeline_image_calibration_data(CalibrationBankId(bank)) };
    let first_count = unsafe { xm1_pipeline_image_configuration_u24(0x28e0 + bank * 6) };
    let second_count = unsafe { xm1_pipeline_image_configuration_u24(0x28e3 + bank * 6) };
    if unsafe { xm1_buffers_configuration_byte(0xb4) } & 4 != 0 {
        unsafe {
            if xm1_buffers_configuration_byte(0xc9) & 4 != 0 { request.write_word(4); }
            request.offset(ByteOffset(16)).write_word(data.0);
            request.offset(ByteOffset(20)).write_word(first_count);
            xm1_pipeline_image_process_calibration_overlay(job, request);
            let pixels = word(request, 8);
            let delta = u32::from(xm1_buffers_configuration_half(0x2906));
            let distance = if word(job, 0x49c) & 16 != 0 { delta.wrapping_mul(3) >> 1 } else { delta.wrapping_mul(2) };
            request.offset(ByteOffset(8)).write_word(pixels.wrapping_add(distance));
            let stencil_bytes = (first_count.wrapping_mul(34).wrapping_add(63) & !63) >> 3;
            request.offset(ByteOffset(16)).write_word(data.0.wrapping_add(stencil_bytes));
            request.offset(ByteOffset(20)).write_word(second_count);
            xm1_pipeline_image_process_calibration_overlay(job, request);
            request.offset(ByteOffset(8)).write_word(pixels);
        }
    }
    1
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct PreviewMode(pub(super) u32);

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct ResizeOption(pub(super) u32);

unsafe extern "C" {
    fn xm1_develop_lookup_width(profile: u32, aspect: u32) -> u32;
    fn xm1_develop_lookup_height(profile: u32, aspect: u32) -> u32;
    fn xm1_pipeline_image_metadata_apply_sensitivity_delta(value: u32, delta: i32) -> u32;
    fn xm1_pipeline_image_resize_planes(job: Address32, option: ResizeOption, mode: u32, source: Address32, destination: *mut u32, auxiliary: Address32, configuration: Address32, operation: PreviewMode) -> u32;
    fn xm1_pipeline_image_initialize_metadata_identity();
    fn xm1_pipeline_image_select_compression_tables(kind: i32, width: u32, height: u32, slot: i8, unused: i32, limits: *mut u32, geometry: *mut u32, parameters: *mut u32) -> i32;
    fn xm1_pipeline_image_setup_descriptors(image: Address32, source: Address32, thumbnail: Address32, primary: Address32, secondary: Address32, scale: u32, order: u32, flags: u32, kind: u32, initialize: i32);
    fn xm1_pipeline_image_initialize_stream(image: Address32, source: Address32, owner: Address32, thumbnail: Address32, primary: Address32, secondary: Address32, scale: i32, order: i32, flags: i32, kind: i32);
    fn xm1_pipeline_image_start_compact_request(id: u32, image: Address32, completion: Address32, mode: i32, compact: i32);
    fn xm1_pipeline_image_wait_all(mask: u32, events: *mut u32, timeout: u32) -> i32;
}

unsafe fn compression_parameters(job: Address32, request: Address32) -> (u32, u32, CompressionBand) {
    let mode = unsafe { word(job, 0x508) };
    let kind = if matches!(mode, 4 | 7..=10)
        || (unsafe { word(job, 0x4b4) } == 32 && unsafe { word(job, 0x530) }.wrapping_sub(6) <= 5) { 3 } else { 1 };
    let alignment = if unsafe { request.word() } == 2 { 1 } else {
        let phase = unsafe { word(job, if matches!(word(request, 4), 2 | 3) { 0x514 } else { 0x50c }) };
        match phase { 1 => 0, 2 => 2, _ => 1 }
    };
    let value = unsafe { word(job, 0x66c) };
    let value = if unsafe { word(job, 0x4a8) } == 8 {
        unsafe { xm1_pipeline_image_metadata_apply_sensitivity_delta(value, word(job, 0x1d3c) as i32) }
    } else { value };
    (kind, alignment, xm1_pipeline_image_compression_band(ConversionIndex(value)))
}

struct PatternCode(u8);
struct CompressionAlignment(u32);
struct CompressionSelection {
    pattern: PatternCode,
    alignment: CompressionAlignment,
}

unsafe fn compression_tables(job: Address32, request: Address32, layout: Address32, width: u32, height: u32) -> CompressionSelection {
    let (kind, alignment, band) = unsafe { compression_parameters(job, request) };
    if unsafe { xm1_pipeline_image_wait_overlay(OverlayId(2), OverlayTimeout(u32::MAX)) } != 0 { loop {} }
    unsafe { xm1_pipeline_image_initialize_metadata_identity() };
    let mut limits = core::mem::MaybeUninit::<u32>::uninit();
    let mut geometry = core::mem::MaybeUninit::<u32>::uninit();
    let mut parameters = core::mem::MaybeUninit::<u32>::uninit();
    unsafe {
        xm1_pipeline_image_select_compression_tables(kind as i32, width & 65535, height & 65535, alignment as i8, band as i32, limits.as_mut_ptr(), geometry.as_mut_ptr(), parameters.as_mut_ptr());
        layout.offset(ByteOffset(28)).write_word(limits.assume_init());
        layout.offset(ByteOffset(32)).write_word(geometry.assume_init());
        CompressionSelection { pattern: PatternCode(Address32(parameters.assume_init()).byte()), alignment: CompressionAlignment(alignment) }
    }
}

unsafe fn initialize_output_stream(job: Address32, request: Address32, image: Address32, layout: Address32, timing: Address32, deferred: u32, selection: CompressionSelection) -> (i32, i32) {
    let pattern = u32::from(selection.pattern.0);
    let alignment = selection.alignment.0;
    let kind = unsafe { request.word() };
    let (thumbnail, primary, secondary, initialize, mode) = match kind {
        1 => (layout, Address32(0), Address32(0), 1, 3),
        2 => (Address32(0), Address32(0), layout, 0, 4),
        _ => (Address32(0), layout, Address32(0), 0, 2),
    };
    unsafe {
        image.offset(ByteOffset(0xf2f4)).write_word(0);
        image.offset(ByteOffset(0xf300)).write_byte(1);
    }
    if unsafe { word(request, 4) } != 4 {
        unsafe { xm1_pipeline_image_setup_descriptors(image, Address32(0), thumbnail, primary, secondary, 10, pattern, 0, alignment, initialize) };
        return (mode, 0);
    }
    let capture = unsafe { word(job, 0x4b4) };
    let special = capture == 30 || capture.wrapping_sub(41) <= 1;
    let values = if deferred == 0 {
        if special { [0,1,1,0,1,0,1] } else { [0,1,1,0,1,75,1000] }
    } else if special { [1,2,1,u32::MAX,u32::MAX,u32::MAX,u32::MAX] }
    else { [1,2,1,0,1,75,1000] };
    for (index, value) in values.into_iter().enumerate() {
        unsafe { timing.offset(ByteOffset(index as u32 * 4)).write_word(value) };
    }
    unsafe { xm1_pipeline_image_initialize_stream(image, Address32(0), timing, thumbnail, primary, secondary, 10, pattern as i32, 0, alignment as i32) };
    (mode, 2)
}

unsafe fn start_output_request(image: Address32, mode: i32, compact: i32, result: Address32) {
    let notifications = Address32(0x0155ccac);
    unsafe {
        notifications.offset(ByteOffset(8)).write_word(xm1_pipeline_image_current_task());
        xm1_pipeline_image_start_compact_request(2, image, Address32(xm1_pipeline_image_notify_output_result as *const () as u32), mode, compact);
        let mut events = core::mem::MaybeUninit::<[u32; 2]>::uninit();
        xm1_pipeline_image_wait_all(16, events.as_mut_ptr().cast(), u32::MAX);
        notifications.offset(ByteOffset(8)).write_word(0);
        if result.0 != 0 { result.write_word(Address32(0x0154d960).word()); }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_tune_triplets(job: Address32, request: Address32, source: Address32, output: Address32, result: Address32) -> u32 {
    let deferred = unsafe { word(job, 0x3230) };
    if unsafe { xm1_develop_capture_option_selection() } != 0 {
        unsafe { result.write_word(0) };
        return 1;
    }
    let supplied = unsafe { word(request, 8) };
    let image = Address32(if supplied == 0 { 0x0154d974 } else { supplied });
    let kind = unsafe { request.word() };
    let (width, height) = if kind == 1 { (160, 120) } else { (unsafe { word(source, 16) } >> 1, unsafe { word(source, 20) }) };
    let layout = Address32(0x0154cd84);
    let pattern = unsafe { compression_tables(job, request, layout, width, height) };
    unsafe {
        layout.write_word(width);
        layout.offset(ByteOffset(4)).write_word(if kind == 1 { 0 } else { (word(source, 12) >> 1).wrapping_sub(width) });
        layout.offset(ByteOffset(8)).write_word(height);
        layout.offset(ByteOffset(12)).write_word(word(if kind == 1 { output } else { source }, 4));
        layout.offset(ByteOffset(16)).write_word(if kind == 2 { word(output, 4) } else { 0 });
        layout.offset(ByteOffset(20)).write_word(if kind == 2 { word(output, 8) } else { 0 });
        layout.offset(ByteOffset(24)).write_word(0);
        layout.offset(ByteOffset(36)).write_word(0);
        layout.offset(ByteOffset(40)).write_word(if matches!(kind, 1 | 2) { 0 } else { 4 });
    }
    let (mode, compact) = unsafe { initialize_output_stream(job, request, image, layout, Address32(0x0154cc20), deferred, pattern) };
    if kind == 1 {
        unsafe {
            image.offset(ByteOffset(0x2d4)).write_word(word(source, 4));
            image.offset(ByteOffset(0x2c8)).write_word(word(source, 16) >> 1);
            image.offset(ByteOffset(0x2d0)).write_word(word(source, 12).wrapping_sub(word(source, 16)) >> 1);
            image.offset(ByteOffset(0x2cc)).write_word(word(source, 20));
        }
    }
    unsafe { start_output_request(image, mode, compact, result) };
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_prepare_triplets(job: Address32, source: Address32, option: ResizeOption, request: Address32, output: Address32, result: Address32, preview: PreviewMode) -> u32 {
    let deferred = unsafe { word(job, 0x3230) };
    if unsafe { xm1_develop_capture_option_selection() } != 0 {
        unsafe { result.write_word(0) };
        return 1;
    }
    let kind = unsafe { request.word() };
    let mut plane = [0u32; 7];
    let (width, height, scale) = if kind != 0 {
        if kind == 2 { (640,240,0) } else { (320,120,0) }
    } else if preview.0 == 1 {
        (unsafe { word(source, 16) }, unsafe { word(source, 20) }, 0)
    } else if unsafe { word(request, 4) } == 2 {
        (unsafe { xm1_develop_lookup_width(33, 1) }.wrapping_mul(2), unsafe { xm1_develop_lookup_height(33, 1) }, 0)
    } else {
        let profile = unsafe { word(job, 0x644) };
        let aspect = unsafe { word(job, 0x4f0) };
        (unsafe { xm1_develop_lookup_width(profile, aspect) }.wrapping_mul(2), unsafe { xm1_develop_lookup_height(profile, aspect) }, if aspect == 3 { 2 } else { 1 })
    };
    plane[3] = width;
    plane[4] = width;
    plane[5] = height;
    let configuration = Address32(0x0154cc90);
    unsafe { xm1_pipeline_image_resize_planes(job, option, scale, source, plane.as_mut_ptr(), source, configuration, preview) };
    let supplied = unsafe { word(request, 8) };
    let image = Address32(if supplied == 0 { 0x0154d974 } else { supplied });
    let (width, height) = if kind == 1 { (160,120) } else { (unsafe { word(configuration, 56) }, unsafe { word(configuration, 60) }) };
    let layout = Address32(0x0154cf18);
    let pattern = unsafe { compression_tables(job, request, layout, width, height) };
    unsafe {
        layout.write_word(width);
        layout.offset(ByteOffset(4)).write_word(0);
        layout.offset(ByteOffset(8)).write_word(height);
        layout.offset(ByteOffset(12)).write_word(word(source, 4));
        layout.offset(ByteOffset(16)).write_word(if kind == 2 { word(output, 4) } else { 0 });
        layout.offset(ByteOffset(20)).write_word(if kind == 2 { word(output, 8) } else { 0 });
        layout.offset(ByteOffset(24)).write_word(0);
        layout.offset(ByteOffset(36)).write_word(configuration.0);
        layout.offset(ByteOffset(40)).write_word(if matches!(kind, 1 | 2) { 0 } else { 4 });
    }
    let (mode, compact) = unsafe { initialize_output_stream(job, request, image, layout, Address32(0x0154c764), deferred, pattern) };
    unsafe { start_output_request(image, mode, compact, result) };
    if preview.0 == 0 && unsafe { word(job, 0x4b4) } == 32 {
        unsafe { xm1_pipeline_image_finish_output_preview(job) };
    }
    1
}

#[derive(Clone, Copy)]
enum SnapshotWidth {
    Byte,
    Half,
    SignedByte,
    SignedHalf,
}

#[derive(Clone, Copy)]
struct ConfigurationKey(u32);

#[derive(Clone, Copy)]
struct SnapshotFields(u8);

struct SnapshotRange {
    key: ConfigurationKey,
    output: ByteOffset,
    fields: SnapshotFields,
    width: SnapshotWidth,
}

const fn snapshot_range(key: u32, output: u32, fields: u8, width: SnapshotWidth) -> SnapshotRange {
    SnapshotRange { key: ConfigurationKey(key), output: ByteOffset(output), fields: SnapshotFields(fields), width }
}

const PREVIEW_SNAPSHOT: &[SnapshotRange] = &[
    snapshot_range(0x8c00, 0xdbf64, 2, SnapshotWidth::Byte),
    snapshot_range(0x8c08, 0xdbf6c, 11, SnapshotWidth::Byte),
    snapshot_range(0x8c13, 0xdbf98, 1, SnapshotWidth::Half),
    snapshot_range(0x8c15, 0xdbf9c, 5, SnapshotWidth::SignedByte),
    snapshot_range(0x8c1a, 0xdbfb0, 11, SnapshotWidth::Byte),
    snapshot_range(0x8c25, 0xdbfdc, 6, SnapshotWidth::SignedByte),
    snapshot_range(0x8c2b, 0xdbff4, 6, SnapshotWidth::SignedHalf),
    snapshot_range(0x8c37, 0xdc00c, 11, SnapshotWidth::Byte),
    snapshot_range(0x8c42, 0xdc038, 6, SnapshotWidth::SignedByte),
    snapshot_range(0x8c48, 0xdc050, 6, SnapshotWidth::SignedHalf),
    snapshot_range(0x8c54, 0xdc068, 11, SnapshotWidth::Byte),
    snapshot_range(0x8c5f, 0xdc094, 6, SnapshotWidth::SignedByte),
    snapshot_range(0x8c65, 0xdc0ac, 6, SnapshotWidth::SignedHalf),
    snapshot_range(0x8c78, 0xdc0c4, 9, SnapshotWidth::Byte),
    snapshot_range(0x8c81, 0xdc0e8, 30, SnapshotWidth::Half),
    snapshot_range(0x8cbd, 0xdc160, 1, SnapshotWidth::Byte),
    snapshot_range(0x8cc4, 0xdc164, 1, SnapshotWidth::Byte),
    snapshot_range(0x8cbe, 0xdc168, 1, SnapshotWidth::Half),
    snapshot_range(0x8cc5, 0xdc16c, 1, SnapshotWidth::Half),
    snapshot_range(0x8cc0, 0xdc170, 1, SnapshotWidth::Half),
    snapshot_range(0x8cc7, 0xdc174, 1, SnapshotWidth::Half),
    snapshot_range(0x8cc2, 0xdc178, 1, SnapshotWidth::Half),
    snapshot_range(0x8cc9, 0xdc17c, 1, SnapshotWidth::Half),
    snapshot_range(0x8cd0, 0xdc180, 8, SnapshotWidth::Byte),
    snapshot_range(0x8cd8, 0xdc1a0, 1, SnapshotWidth::Half),
    snapshot_range(0x8cda, 0xdc1a4, 1, SnapshotWidth::Byte),
];

unsafe extern "C" {
    fn xm1_pipeline_image_configuration_signed_half(key: u32) -> i32;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_pipeline_image_finish_output_preview(job: Address32) {
    if unsafe { xm1_buffers_configuration_byte(0xa8) } & 128 == 0 { return; }
    unsafe {
        Address32(word(job, 0x220c)).offset(ByteOffset(0xdbf58)).write_word(0x53544146);
        Address32(word(job, 0x220c)).offset(ByteOffset(0xdf280)).write_word(0x4c414146);
    }
    for range in PREVIEW_SNAPSHOT {
        let stride = match range.width { SnapshotWidth::Byte | SnapshotWidth::SignedByte => 1, SnapshotWidth::Half | SnapshotWidth::SignedHalf => 2 };
        for field in 0..u32::from(range.fields.0) {
            let output = Address32(unsafe { word(job, 0x220c) }).offset(ByteOffset(range.output.0 + field * 4));
            let key = range.key.0 + field * stride;
            let value = match range.width {
                SnapshotWidth::Byte => unsafe { xm1_buffers_configuration_byte(key) },
                SnapshotWidth::Half => u32::from(unsafe { xm1_buffers_configuration_half(key) }),
                SnapshotWidth::SignedByte => (unsafe { xm1_pipeline_image_configuration_signed_byte(key) }) as u32,
                SnapshotWidth::SignedHalf => (unsafe { xm1_pipeline_image_configuration_signed_half(key) }) as u32,
            };
            unsafe { output.write_word(value) };
        }
    }
    unsafe {
        Address32(word(job, 0x220c)).offset(ByteOffset(0xdbf5c)).write_word(word(job, 0x530));
        Address32(word(job, 0x220c)).offset(ByteOffset(0xdbf60)).write_word(word(job, 0x4f0));
    }
}
