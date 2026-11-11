use core::ptr::{copy_nonoverlapping, read_volatile, write_volatile};

extern "C" {
    fn xm1_power_record_helper_ffc3fc(first: u32, second: u32, third: u32, fourth: u32);
    fn xm1_power_record_helper_ffc97c(first: u32, second: u32, third: u32, fourth: u32, source: u32, dimensions: u32, gain: i32);
    fn xm1_power_record_helper_ffc668(first: u32, second: u32, report: u32);
    fn xm1_power_record_helper_edae88(selector: u32, output: u32) -> u32;
    fn xm1_power_record_helper_edb05c(configure: u32, parameters: u32, source: u32, zero: u32, metadata: u32, primary: u32, control: u32);
    fn xm1_power_record_helper_edba78(gain: i32, profile: u32, output: u32);
    fn xm1_power_record_helper_edb0ac(gain: i32, profile: u32, output: u32);
    fn xm1_power_record_helper_edb9fc(gain: i32, profile: u32) -> i32;
    fn xm1_power_record_helper_ef5310(profile: u32) -> i32;
    fn xm1_power_record_helper_ef53d0(profile: u32) -> u32;
    fn xm1_power_record_helper_ef5768(output: u32);
    fn xm1_power_record_helper_ef56cc(output: u32, area: u32);
    fn xm1_power_record_helper_ef73e4() -> u32;
    fn xm1_power_record_helper_ef743c(gain: i32, zero: u32);
    fn xm1_power_record_helper_772f28(item: u32) -> i32;
    fn xm1_power_record_helper_69fec8(item: u32) -> u32;
    fn xm1_power_record_helper_ee9ab0(gain: i32) -> u32;
}
fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
fn put(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
fn half(address: u32) -> u16 { unsafe { read_volatile(address as *const u16) } }
fn put_half(address: u32, value: u16) { unsafe { write_volatile(address as *mut u16, value); } }
fn byte(address: u32) -> u8 { unsafe { read_volatile(address as *const u8) } }
fn address<T>(value: &mut T) -> u32 { value as *mut T as u32 }
fn copy(destination: u32, source: u32, count: usize) { unsafe { copy_nonoverlapping(source as *const u8, destination as *mut u8, count); } }
fn offset(selector: u32) -> u32 { let mut value = 0; unsafe { xm1_power_record_helper_edae88(selector, address(&mut value)); } value }
fn dimensions() -> [u32; 2] { let mut pair = [0; 2]; unsafe { xm1_power_record_helper_ef5768(address(&mut pair)); } pair }
fn scales(area: u32) -> [u32; 4] { let mut values = [0; 4]; unsafe { xm1_power_record_helper_ef56cc(address(&mut values), area); } values }
fn transform_grid(grid: u32, mut transform: impl FnMut(usize, u32) -> u32) {
    for channel in 0..4 { for cell in 0..256 { let target = grid + (channel * 1024 + cell * 4) as u32; put(target, transform(channel, word(target))); } }
}
fn amplify(grid: u32, factor: u32, shift: u32, area: u32) {
    let limits = scales(area).map(|value| value.wrapping_mul(0x3fff));
    transform_grid(grid, |channel, value| (((u64::from(value) * u64::from(factor)) >> shift) as u32).min(limits[channel]));
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_ffd04c(item: u32, destination: u32, report: u32) -> i32 {
    if destination == 0 { return 0; }
    let mut source = 0;
    if unsafe { xm1_power_record_helper_ffcfd0(item, &mut source, report, core::ptr::null_mut()) } == 0 { return 0; }
    for channel in 0..4 { copy(destination + channel * 1024, source + offset(3 + channel), 1024); }
    unsafe { xm1_power_record_helper_ffc3fc(destination, report, 0, 0); }
    let black = source + offset(13);
    let gain = (i32::from(half(report + 102) as i16) + unsafe { xm1_power_record_helper_ef5310(word(report + 116)) }) as i16 as i32;
    unsafe { xm1_power_record_helper_ffc97c(destination, report, 0, 0, black, report + 8, gain); }
    let regions = source + offset(7);
    for region in 0..3 { for entry in 0..8 { let index = if entry >= 6 { entry - 4 } else { entry }; put(destination + 4096 + region * 32 + entry * 4, word(regions + region * 32 + index * 4)); } }
    unsafe {
        xm1_power_record_helper_ffc3fc(0, 0, destination + 4096, report + 16);
        xm1_power_record_helper_ffc97c(0, 0, destination + 4096, report + 16, black, report + 8, gain);
        xm1_power_record_helper_ffc668(destination, destination + 4096, report);
    }
    1
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_ffd3a0(item: u32, grid: u32, report: u32, configure: u32, debug: u32, control: u32) -> i32 {
    let mut source = 0;
    let mut metadata = [0u32; 54];
    if unsafe { xm1_power_record_helper_ffcfd0(item, &mut source, report, metadata.as_mut_ptr().cast()) } == 0 { return 0; }
    let primary = byte(report + 140) == 1;
    let source_copy = match byte(report + 140) { 1 => 0x5ffb9120, 2 => 0x5ffbbad0, _ => 0 };
    if source_copy != 0 { copy(source_copy, source, 0x29b0); }
    if primary { put(0x016f8154, 0); }
    unsafe { xm1_power_record_helper_ef53d0(word(report + 116)); }
    let paired = word(control + 28) == 3;
    if !paired { copy(if primary { 0x5ffbe480 } else { 0x5ffc0e30 }, if primary { 0x5ffb9120 } else { 0x5ffbbad0 }, 0x29b0); }
    let mut parameters = [0u32; 20];
    let parameter_address = address(&mut parameters);
    unsafe { xm1_power_record_helper_ed8af4(parameter_address); }
    put(parameter_address, word(report + 116));
    put_half(parameter_address + 4, half(report + 100));
    put_half(parameter_address + 6, half(report + 104).wrapping_add(half(report + 102)));
    put(parameter_address + 8, word(report + 120)); put(parameter_address + 12, word(report + 124));
    put(parameter_address + 36, word(report + 112));
    put_half(parameter_address + 40, half(report + 106)); put_half(parameter_address + 42, half(report + 108));
    if configure != 0 {
        put(control + 36, if paired { 0x5ffbe480 } else { 0 }); put(control + 40, if paired { 0x5ffc0e30 } else { 0 });
        unsafe { xm1_power_record_helper_edb05c(configure, parameter_address, source_copy, 0, metadata.as_mut_ptr() as u32, u32::from(primary), control); }
        if primary { put(0x016f8154, 1); }
    }
    if paired && primary {
        let grid_offset = offset(3);
        for index in 0..1024 { let target = 0x5ffb9120 + grid_offset + index * 4; put(target, word(0x5ffbe480 + grid_offset + index * 4).saturating_sub(word(target))); }
        let black_offset = offset(13);
        for index in 0..4 { let target = 0x5ffb9120 + black_offset + index * 4; put(target, word(0x5ffbe480 + black_offset + index * 4).saturating_sub(word(target))); }
    }
    if unsafe { xm1_power_record_helper_ffd244(source_copy, grid) } == 0 { return 1; }
    let geometry_address = metadata.as_ptr() as u32;
    let sample_area = word(geometry_address + 8).wrapping_mul(word(geometry_address + 12));
    if word(control + 32) == 0 { remove_black(grid, source_copy, report, debug, geometry_address, sample_area); }
    let pair = dimensions();
    let area = pair[0].wrapping_mul(pair[1]).max(sample_area);
    let gain = half(report + 106).wrapping_add(half(report + 102)).wrapping_add(unsafe { xm1_power_record_helper_ef5310(word(report + 116)) } as u16) as i16 as i32;
    unsafe { xm1_power_record_helper_ef743c(i32::from(half(report + 106) as i16), 0); }
    let mode = unsafe { xm1_power_record_helper_ef73e4() } as usize;
    let parameter = if gain == unsafe { xm1_power_record_helper_772f28(2) } { 0x80fa } else { [0x80f4, 0x80f6, 0x80f8][mode] };
    let multiplier = unsafe { xm1_power_record_helper_69fec8(parameter) };
    if multiplier != 0 && multiplier != 128 { amplify(grid, multiplier, 7, area); }
    let pair = dimensions();
    let multiplier = unsafe { xm1_power_record_helper_ee9ab0(i32::from(half(report + 150) as i16)) };
    if multiplier != 100 { amplify(grid, multiplier.wrapping_shl(10) / 100, 10, pair[0].wrapping_mul(pair[1]).max(sample_area)); }
    unsafe { xm1_power_record_helper_ffc668(grid, 0, report); }
    1
}

fn remove_black(grid: u32, source: u32, report: u32, debug: u32, geometry: u32, sample_area: u32) {
    let mut black = [0u32; 4]; let mut pedestal = [0u32; 4];
    let gain = i32::from(half(report + 102) as i16); let profile = word(report + 116);
    unsafe { xm1_power_record_helper_edba78(gain, profile, address(&mut black)); xm1_power_record_helper_edb0ac(gain, profile, address(&mut pedestal)); }
    if debug != 0 {
        for (offset, value) in [(0x9e564, black[1]), (0x9e568, black[0]), (0x9e56c, black[3]), (0x9e570, black[2]), (0x9e584, (pedestal[1] as i32).wrapping_add(pedestal[3] as i32).wrapping_div(2) as u32), (0x9e588, pedestal[0]), (0x9e58c, pedestal[2])] { put(debug + offset, value); }
    }
    let black_source = source + offset(13);
    let black_area = word(geometry + 16).wrapping_mul(word(geometry + 20)) >> 2;
    let mut baseline = [0u32; 4];
    for channel in 0..4 { baseline[channel] = word(black_source + channel as u32 * 4).saturating_sub(black[channel].wrapping_mul(black_area)); }
    let common = if unsafe { xm1_power_record_helper_edb9fc(gain, profile) } == 1 { (baseline.iter().map(|value| u64::from(*value)).sum::<u64>() / u64::from(black_area.wrapping_mul(4))) as u32 } else { 0 };
    put(report + 8, word(geometry + 16)); put(report + 12, word(geometry + 20));
    if debug != 0 {
        put(debug + 0x9cedc, word(geometry + 20)); put(debug + 0x9cee0, word(geometry + 16));
        for region in 0..4 { for index in 0..16 { put(debug + 0x9deec + region * 64 + index * 4, 0); } }
    }
    let raw = [word(black_source), word(black_source + 4), word(black_source + 8), word(black_source + 12)];
    if raw.iter().any(|value| *value != 0) { for channel in 0..4 { put(grid + 0x1010 + channel as u32 * 4, raw[channel]); } }
    if debug != 0 { for channel in 0..4 { put(debug + 0x9ced0 + [0x105c, 0x101c, 0x10dc, 0x109c][channel], raw[channel]); } }
    let area = word(geometry + 16).wrapping_mul(word(geometry + 20)) >> 2;
    for channel in 0..4 { let target = grid + 0x1010 + channel as u32 * 4; put(target, word(target).saturating_sub(black[channel].wrapping_mul(area))); }
    let pair = dimensions();
    let scales = scales(pair[0].wrapping_mul(pair[1]).max(sample_area));
    let subtraction = core::array::from_fn::<_, 4, _>(|channel| scales[channel].wrapping_mul(pedestal[channel].wrapping_add(common)));
    transform_grid(grid, |channel, value| value.saturating_sub(subtraction[channel]));
    for channel in 0..4 { put(grid + 0x1010 + channel * 4, 0); }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_ed8af4(output: u32) {
    copy(output, 0x0169953c, 72);
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_ffde64(item: u32, grid: u32, report: u32, configure: u32, debug: u32, control: u32) -> i32 {
    xm1_power_record_helper_ffd3a0(item, grid, report, configure, debug, control)
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_ffde8c() -> u32 { word(0x016f8154) }

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_ffcfd0(item: u32, source: *mut u32, report: u32, metadata: *mut u8) -> i32 {
    if u32::from(byte(0x016f8158)) != item { return 0; }
    unsafe { source.write(word(0x016f815c)); }
    if report != 0 { copy(report, 0x016f8160, 152); }
    if !metadata.is_null() { copy(metadata as u32, 0x016f81f8, 216); }
    1
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_ffd244(source: u32, destination: u32) -> i32 {
    if destination == 0 { return 0; }
    for channel in 0..4 { copy(destination + channel * 1024, source + offset(3 + channel), 1024); }
    for channel in 0..4 { put(destination + 4096 + channel * 4, 0); }
    for channel in 0..4 { put(destination + 4112 + channel * 4, word(source + offset(13 + channel))); }
    1
}
