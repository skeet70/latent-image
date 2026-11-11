#![cfg(target_arch = "arm")]

use core::ptr::{read_volatile, write_volatile};
use crate::record_profile_accessors::ProfileWords;
use crate::record_statistics_queries::GeometryQuery;

#[derive(Clone, Copy)]
struct ConfigurationKey(u32);
#[derive(Clone, Copy)]
struct ProfileHandle(u32);
#[derive(Clone, Copy)]
struct NativeAddress(u32);

fn word(address: NativeAddress) -> u32 { unsafe { read_volatile(address.0 as *const u32) } }
fn byte(address: NativeAddress) -> u8 { unsafe { read_volatile(address.0 as *const u8) } }
fn half(address: NativeAddress) -> u16 { unsafe { read_volatile(address.0 as *const u16) } }
fn put(address: NativeAddress, value: u32) { unsafe { write_volatile(address.0 as *mut u32, value); } }
fn config(key: ConfigurationKey) -> u32 { u32::from(byte(NativeAddress(word(NativeAddress(0x0096ca50)).wrapping_add(key.0)))) }
fn config_half(key: ConfigurationKey) -> u32 {
    let base = word(NativeAddress(0x0096ca50)).wrapping_add(key.0);
    u32::from(u16::from_le_bytes([byte(NativeAddress(base)), byte(NativeAddress(base.wrapping_add(1)))]))
}
fn state(offset: u32) -> u32 { word(NativeAddress(0x016d8708 + offset)) }
fn set_state(offset: u32, value: u32) { put(NativeAddress(0x016d8708 + offset), value); }

unsafe extern "C" {
    #[link_name = "xm1_power_record_helper_7503ec"] fn profile_mode(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74e5c0"] fn profile_enabled(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74e648"] fn profile_position(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74e68c"] fn shared_position(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74e718"] fn profile_flags(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74e7a0"] fn shared_flags(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74f894"] fn profile_default_position() -> u32;
    #[link_name = "xm1_power_record_helper_772f28"] fn mapped_default_position(mode: u32) -> u32;
    #[link_name = "xm1_power_record_helper_772ff8"] fn mapped_kind(kind: i32) -> u32;
    #[link_name = "xm1_power_record_helper_74f9a4"] fn profile_kind(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74fc64"] fn profile_auxiliary(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_76f340"] fn lens_flags() -> u32;
    #[link_name = "xm1_power_record_helper_76d9c8"] fn lens_context() -> u32;
    #[link_name = "xm1_power_record_helper_76dbb4"] fn lens_reference() -> u32;
    #[link_name = "xm1_power_record_helper_747374"] fn channel_bias(channel: u32, mode: u32) -> u32;
    #[link_name = "xm1_power_record_helper_746864"] fn channel_value(channel: u32, mode: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74f124"] fn profile_rate(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74bd64"] fn export_limits(context: u32, reserved: u32, position: u32, kind: u32, mode: u32, lower: *mut u16, upper: *mut u16, enabled: u32, zero: u32, flags: u32, auxiliary: u32, last: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74c568"] fn export_bounds(context: u32, reserved: u32, position: u32, kind: u32, mode: u32, reference: u32, first: *mut u16, second: *mut u16, enabled: u32, zero: u32, flags: u32, option: u32, auxiliary: u32, last: u32) -> u32;
    #[link_name = "xm1_power_record_helper_04d978"] fn unsigned_divide(numerator: u32, denominator: u32) -> u32;
    #[link_name = "xm1_power_record_helper_748818"] fn scaled_channel_curve(channel: u32) -> u32;
    #[link_name = "xm1_power_record_helper_748398"] fn adjust_selection(channel: u32, selector: u32, position: u32, value: u32) -> u32;
    #[link_name = "xm1_power_record_helper_74947c"] fn remap_channel(selected: u32, channel: u32, selector: u32, mode: u32, first: u32, second: u32, enabled: u32) -> u32;
    #[link_name = "xm1_power_record_helper_746834"] fn normalized_channel(value: u32) -> u32;
    #[link_name = "xm1_power_record_helper_7492ac"] fn calculate_rate(context: u32, index: u32, limit: u32, scale: u32, maximum: u32, zero: u32, output: *mut u16, extra: u32, parameter: u32, kind: u32, enabled: u32) -> u32;
    #[link_name = "xm1_power_record_helper_fa95a8"] fn project_clock(key: u32, output: *mut u32);
    #[link_name = "xm1_power_record_helper_74eba8"] fn profile_snapshot_mode(handle: u32) -> u32;
    #[link_name = "xm1_power_record_helper_756e24"] fn read_mode_snapshot(output: *mut ProfileWords<5>) -> *mut ProfileWords<5>;
    #[link_name = "xm1_power_record_helper_756e5c"] fn write_mode_snapshot(handle: u32, input: *const ProfileWords<5>) -> u32;
    #[link_name = "xm1_power_record_helper_89aa90"] fn signed_divide(numerator: u32, denominator: u32) -> u32;
    #[link_name = "xm1_power_record_helper_eedf48"] fn extended_geometry(bank: u32, mode: u32, query: *const GeometryQuery, output: u32, flags: u32) -> u32;
}

fn mode_position(handle: ProfileHandle) -> u32 {
    unsafe {
        if profile_mode(handle.0) != 1 { profile_position(handle.0) }
        else if profile_enabled(handle.0) == 1 { shared_position(handle.0) }
        else { 0 }
    }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa6904(active: u32, profile: u32) -> u32 {
    if active != 0 { 4 }
    else if byte(NativeAddress(0x00ef6df2u32.wrapping_add(profile.wrapping_mul(20)))) == 0 { 2 }
    else { 3 }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7634(handle: u32) -> u32 { mode_position(ProfileHandle(handle)) }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa7954(handle: u32) -> u32 {
    if profile_mode(handle) == 1 { shared_position(handle) } else { profile_position(handle) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa7984(handle: u32) -> u32 {
    if profile_mode(handle) == 1 { shared_flags(handle) } else { profile_flags(handle) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa7928(handle: u32) -> u32 {
    if profile_mode(handle) == 1 { 0 } else { profile_position(handle) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa6bb4(handle: u32) -> u32 {
    let initial = profile_default_position();
    let selected = if profile_enabled(handle) == 1 || initial == 0 { mapped_default_position(3) } else { initial };
    selected as i16 as i32 as u32
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7edc() -> u32 { 3 }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa8018() -> u32 { state(0x48) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa9588() -> u32 { state(0x44) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_faa790() -> u32 { state(0x58) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_faa770(value: u32) { set_state(0x58, value); }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7f40() -> u32 { state(4) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7fd8() -> u32 { state(0x54) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7fb8(value: u32) { set_state(0x54, value); }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7ff8(value: u32) { set_state(0x48, value); }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa8038(value: u32) { set_state(0x4c, value); }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa8058() -> u32 { state(0x4c) }

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7614(_: u32, minimum: u32) -> u32 { config(ConfigurationKey(0x8806)).max(minimum) }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fab43c(channel: u32) -> u32 {
    let value = channel_value(channel, 0);
    let key = match value { 0..400 => 0x8ba1, 400..560 => 0x8ba2, 560..800 => 0x8ba3, _ => 0x8ba4 };
    config(ConfigurationKey(key)) as i16 as i32 as u32
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fab250(mode: u32, category: u32, _: u32, enabled: u32) -> u32 {
    if enabled != 1 || config(ConfigurationKey(0xa7e4)) & 1 == 0 { return 0; }
    u32::from(mode == 1 || matches!(category, 0..=4 | 8..=9 | 14..=23 | 26..=27 | 30..=31 | 33 | 35..=36 | 38..=39 | 41..=50 | 57))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_faa7b0(handle: u32, record: *const u32, position: *mut u32, flags: *mut u16) {
    write_volatile(position, profile_position(handle));
    let value = profile_flags(handle) as u16;
    write_volatile(flags, value);
    let value = if read_volatile(record.add(4)) == 1 { value | 1 } else { value & !1 };
    write_volatile(flags, value);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_faaae8(handle: u32) -> u32 {
    if profile_enabled(handle) == 0 { 1 } else { profile_rate(handle).max(1) }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa9320(limit: u32, first: u32, baseline: u32, second: u32) -> u32 {
    u32::from(baseline.wrapping_add(second.wrapping_mul(first) / 100).wrapping_add(config(ConfigurationKey(0x8823)).wrapping_mul(10)) < limit)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa936c(history: *const u32, index: u32, cursor: *mut u32, threshold: u32, phase: u32) -> u32 {
    let required_phase = config(ConfigurationKey(0x8833));
    let required_other = config(ConfigurationKey(0x8831));
    let offset = config(ConfigurationKey(0x8823));
    if index == 0 { return 0; }
    let current = read_volatile(history.add(index as usize));
    let mut position = index;
    let mut count = 0u8;
    while position != 0 {
        let previous = read_volatile(history.add(position as usize - 1));
        if read_volatile(history.add(position as usize)) >= previous { break; }
        write_volatile(cursor, previous);
        position -= 1;
        count = count.wrapping_add(1);
        if u32::from(count) == index { break; }
    }
    if count == 0 { return 0; }
    let required = if phase == 1 { required_phase } else { required_other };
    if required <= u32::from(count) { return 1; }
    let scaled = (u64::from(read_volatile(cursor)) * u64::from(threshold) / 100) as u32;
    let difference = read_volatile(history.add(position as usize)).wrapping_sub(current);
    u32::from(offset.wrapping_mul(10).wrapping_add(scaled) <= difference)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa9488(history: *const u32, index: u32) -> u32 {
    let required = config(ConfigurationKey(0x8832));
    let threshold = config(ConfigurationKey(0x8830));
    if index == 0 { return 0; }
    let current = read_volatile(history.add(index as usize));
    let mut position = index;
    let mut count = 0u8;
    while position != 0 {
        if read_volatile(history.add(position as usize)) >= read_volatile(history.add(position as usize - 1)) { break; }
        position -= 1;
        count = count.wrapping_add(1);
        if u32::from(count) == index { break; }
    }
    if count == 0 || u32::from(count) < required { return 0; }
    u32::from(threshold.wrapping_mul(100) <= read_volatile(history.add(position as usize)).wrapping_sub(current))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa79b4(handle: u32, selector: u32, first: *mut i16, second: *mut i16) -> u32 {
    let mut kind = profile_kind(handle);
    let position = profile_position(handle);
    let flags = lens_flags();
    let auxiliary = profile_auxiliary(handle);
    if kind == 2 && (config(ConfigurationKey(0x3e1214)) as i32).wrapping_sub(1) < selector as i32 { kind = 1; }
    export_limits(selector, 0, position, kind, 0, first.cast(), second.cast(), 1, 0, flags, auxiliary, 0);
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa767c(handle: u32, first: *mut i16, second: *mut i16, lower: *mut i16, upper: *mut i16, output: *mut u16, force: u32, last: u32) -> u32 {
    let mut kind = profile_kind(handle);
    let position = mode_position(ProfileHandle(handle));
    let reference = lens_reference();
    let flags = lens_flags();
    let context = lens_context();
    if (context as i32) < 0 { return 0; }
    if kind == 2 && (context as i32) > (config(ConfigurationKey(0x3e1214)) as i32).wrapping_sub(1) { kind = 1; }
    let bias = channel_bias(context, 0);
    let auxiliary = profile_auxiliary(handle);
    let (export_kind, mode) = if profile_mode(handle) == 1 || force == 1 { (0, 0) } else { (kind, 6) };
    export_limits(context, 0, position, export_kind, mode, lower.cast(), upper.cast(), 1, 0, flags, auxiliary, last);
    export_bounds(context, 0, position, export_kind, mode, reference, first.cast(), second.cast(), 1, 0, flags, 0, auxiliary, last);
    let key = if profile_mode(handle) == 1 { 0x3e07f7 } else if kind == 1 { 0x3e08c0 } else if kind == 2 { 0x3e0989 } else { 0x3e07f7 };
    write_volatile(output, config_half(ConfigurationKey(key + context * 2)).wrapping_add(bias) as u16);
    1
}

fn rate_policy_index(first: u32, second: u32) -> u32 {
    if first == 1 {
        match second { 22 | 4 | 6 => 4, 23 | 15 => 5, 24 | 5 => 6, 25 | 16 => 7, _ => 2 }
    } else if matches!(second, 19 | 2 | 26 | 17 | 27) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7a7c(first: u32, second: u32, group: u32) -> u32 {
    let index = rate_policy_index(first, second).wrapping_add(group.wrapping_mul(8));
    config(ConfigurationKey(word(NativeAddress(0x00faaed8u32.wrapping_add(index.wrapping_mul(4))))))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa7aa4(first: u32, second: u32, alternate: u32, category: u32, output: *mut u8) {
    let index = rate_policy_index(first, second);
    let table = match (alternate == 0, category == 2) { (true, true) => 0x00fab1f0, (true, false) => 0x00fab050, (false, true) => 0x00faafd0, (false, false) => 0x00fab0b0 };
    let address = NativeAddress(table + index * 12);
    write_volatile(output.add(0x40), config(ConfigurationKey(word(address))) as u8);
    write_volatile(output.add(0x3e).cast::<u16>(), config(ConfigurationKey(word(NativeAddress(address.0 + 4)))).wrapping_mul(10) as u16);
    write_volatile(output.add(0x3c).cast::<u16>(), config(ConfigurationKey(word(NativeAddress(address.0 + 8)))).wrapping_mul(10) as u16);
    write_volatile(output.add(0x3a).cast::<u16>(), config(ConfigurationKey(0x63b8)) as u16);
}

#[repr(C)]
pub struct DefaultListQuery {
    pub kind: i16,
    pub reserved: u16,
    pub state: u32,
    pub policy: u32,
    pub mode: u32,
    pub first: u8,
    pub reserved_first: u8,
    pub second: u16,
    pub third: u32,
    pub fourth: u32,
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa67a8(query: *const DefaultListQuery) -> u32 {
    let query = &*query;
    if query.state == 1 { return query.fourth; }
    let kind = mapped_kind(i32::from(query.kind));
    if kind <= 10 && (1u32 << kind) & 0x606 != 0 { return 1; }
    if query.policy == 57 { return 2; }
    if query.third.wrapping_sub(2) <= 1 { return 1; }
    // The X-M1 ef68a4 callee returns zero for every input.
    query.fourth
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa6590(value: u32) -> u32 {
    let high = config(ConfigurationKey(0x6399));
    let middle = config(ConfigurationKey(0x6398));
    let low = config(ConfigurationKey(0x6397));
    value.wrapping_mul((high << 16) | (middle << 8) | low)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa6c04(first: u32, second: u32, third: u32, selection: u32, weights: *const u16) -> u32 {
    if weights.is_null() { return 0; }
    let second_weight = u32::from(read_volatile(weights));
    let first_weight = u32::from(read_volatile(weights.add(1)));
    let third_weight = u32::from(read_volatile(weights.add(2)));
    let weighted = |value: u32, weight: u32| ((u64::from(value) * u64::from(weight)) >> 8) as u32;
    match selection {
        2 => weighted(second, second_weight),
        3 => weighted(third, third_weight),
        1 => weighted(first, first_weight),
        _ => weighted(second.wrapping_mul(3), second_weight).wrapping_add(weighted(third, third_weight)).wrapping_add(weighted(first.wrapping_mul(3), first_weight)),
    }
}

#[derive(Clone, Copy)]
enum DispatchMode { Zero, One }

#[derive(Clone, Copy)]
struct DispatchPolicy {
    active: u32,
    enabled: u32,
    first: u32,
    second: u32,
    third: u32,
    fourth: u32,
    state: u32,
    option: u8,
}

fn dispatch_allowed(mode: DispatchMode, policy: DispatchPolicy) -> u32 {
    if policy.active | policy.state == 0 || policy.enabled == 0 { return 0; }
    let category = if policy.state != 0 { 4 }
        else if policy.option == 1 { 5 }
        else if policy.third == 0 { 2 }
        else if policy.third.wrapping_sub(4) <= 2 { 3 }
        else { 0 };
    let address = if policy.first != 0 { 0x00fa6a50 + category }
        else if policy.fourth != 0 { 0x00fa6a48 + category }
        else { 0x00fa6a58u32.wrapping_add(policy.second.wrapping_mul(6)).wrapping_add(category) };
    let mask = match mode { DispatchMode::Zero => 2, DispatchMode::One => 1 };
    u32::from(u32::from(byte(NativeAddress(address))) & mask == mask)
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa66d4(active: u32, enabled: u32, first: u32, second: u32, third: u32, fourth: u32, state: u32, option: u8) -> u32 {
    dispatch_allowed(DispatchMode::One, DispatchPolicy { active, enabled, first, second, third, fourth, state, option })
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa667c(active: u32, enabled: u32, first: u32, second: u32, third: u32, fourth: u32, state: u32, option: u8) -> u32 {
    dispatch_allowed(DispatchMode::Zero, DispatchPolicy { active, enabled, first, second, third, fourth, state, option })
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa672c(active: u32, enabled: u32, first: u32, second: u32, third: u32, fourth: u32, state: u32, option: u8) -> u32 {
    let policy = DispatchPolicy { active, enabled, first, second, third, fourth, state, option };
    let one = dispatch_allowed(DispatchMode::One, policy);
    let zero = dispatch_allowed(DispatchMode::Zero, policy);
    u32::from(one | zero != 0)
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7e8c(value: u32) { set_state(0x40, value); }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7eac() -> u32 { state(0x40) }
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7ef4() -> u32 { u32::from(state(0) == 1) }

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_741fa0(value: i32) -> i32 {
    let quotient = value / 10;
    let remainder = value % 10;
    if remainder > 4 { quotient + 1 } else if remainder < -4 { quotient - 1 } else { quotient }
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_747d48(channel: u32) -> u32 {
    if (channel as i32) < 0 { return 0xffff; }
    let count = config(ConfigurationKey(0x3e0f80)).clamp(1, 100);
    if channel >= count { return 0xffff; }
    let key = 0x3e2007 + channel * 3;
    config(ConfigurationKey(key)) | (config(ConfigurationKey(key + 1)) << 8) | (config(ConfigurationKey(key + 2)) << 16)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa7ce0(_: u32, channel: u32, amount: u32, minimum: i32, maximum: i32) -> u32 {
    let factor = config(ConfigurationKey(0xa491));
    let base_channel = config(ConfigurationKey(0xa492));
    let baseline = xm1_power_record_helper_747d48(base_channel).wrapping_mul(100);
    let numerator = (factor.wrapping_mul(amount) / 100).wrapping_mul(baseline);
    let reference = xm1_power_record_helper_747d48(channel);
    let scaled = unsigned_divide(numerator, reference);
    let duration = config_half(ConfigurationKey(0xa493));
    let first = xm1_power_record_helper_747d48(channel);
    let second = xm1_power_record_helper_747d48(channel);
    let numerator = (duration.wrapping_mul(scaled) / 10000).wrapping_mul(first.wrapping_mul(second));
    let curve = scaled_channel_curve(channel);
    let bias = channel_bias(channel, 0);
    let offset = unsigned_divide(numerator, curve.wrapping_mul(10000)) as u16;
    let shifted = offset.wrapping_add(bias as u16);
    let direction = lens_flags();
    let selected = adjust_selection(channel, 0, direction, shifted.wrapping_mul(2) as i16 as i32 as u32);
    let mode = lens_reference();
    let target = remap_channel(selected, channel, 0, mode, 0, 0, 1) as i32;
    let target = target.max(minimum) as i16;
    i32::from(target.min(maximum as i16)) as u32
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fab390(handle: u32, mode: u32, _: u32) -> u32 {
    let enabled = profile_enabled(handle) == 1;
    let key = match mode {
        0 => if enabled { 0x87e1 } else { 0x87df },
        1 => if enabled { 0x87ed } else { 0x87eb },
        5 => 0x8882,
        6 => 0x8884,
        7 => 0x8883,
        _ => if enabled { 0x87e7 } else { 0x87e5 },
    };
    config(ConfigurationKey(key)).wrapping_mul(10) & 0xfffe
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_faab3c(handle: u32, mode: u32, channel: u32, amount: u32, key: u32, extra: u32, output: *mut u32) -> u32 {
    let context = lens_context();
    let limit = if matches!(mode, 1 | 6) && amount < config(ConfigurationKey(0x8814)).wrapping_mul(10) {
        config(ConfigurationKey(0x87f1)).wrapping_mul(10) & 0xffff
    } else { xm1_power_record_helper_fab390(handle, mode, context) };
    let index = normalized_channel(channel);
    let parameter = state(0x48);
    let kind = if parameter == 0 { 9 } else { 10 };
    let mut clock = 0;
    project_clock(key, &mut clock);
    let shift = config(ConfigurationKey(0x87da)) & 0xff;
    clock = if shift < 32 { clock >> shift } else { 0 };
    let mut result = 0;
    let rate = calculate_rate(context, index, limit, clock / 100, 0x7fff, 0, &mut result, extra, parameter, kind, 1);
    write_volatile(output, rate);
    u32::from(result)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa80f4(handle: u32) {
    if profile_snapshot_mode(handle).wrapping_sub(8) > 2 { return; }
    let mut snapshot = ProfileWords([0; 5]);
    read_mode_snapshot(&mut snapshot);
    if snapshot.0[4] != 0 { return; }
    let coordinates = snapshot.0.as_mut_ptr().cast::<u16>();
    let first = coordinates.add(4).read();
    let second = coordinates.add(3).read();
    let third = coordinates.add(5).read();
    coordinates.add(1).write(first);
    coordinates.write(second);
    coordinates.add(2).write(third);
    write_mode_snapshot(handle, &snapshot);
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_fa7bb8(parameters: *const u32, output: *mut u32) {
    let query = GeometryQuery {
        kind: read_volatile(parameters.add(1)),
        first: read_volatile(parameters.add(2)),
        second: read_volatile(parameters.add(3)),
        coordinates: read_volatile(parameters.add(4)),
    };
    extended_geometry(0, read_volatile(parameters), &query, output.cast::<u8>().add(4) as u32, output.cast::<u8>().add(0x430) as u32);
}

fn signed_half(address: NativeAddress) -> i32 { i32::from(half(address) as i16) }

fn scaled_profile_rate(value: i32, category: u32) -> u32 {
    if value < 1000 { return 128; }
    let selected = value.min(0x73a) as i16;
    let key = match category { 1 | 4 | 6 => 0x221c, 2 => 0x2240, 3 => 0x21b0, 5 => 0x21f8, _ => 0x21d4 };
    let mut index = 0u32;
    if selected > 1000 {
        index = 1;
        while index < 18 {
            let sample = signed_half(NativeAddress(0x00ef50c4 + index * 2));
            if sample == i32::from(selected) { return config_half(ConfigurationKey(key + index * 2)); }
            if sample > i32::from(selected) { break; }
            index += 1;
        }
        if index == 18 { return config_half(ConfigurationKey(key + index * 2)); }
    } else if selected == 1000 { return config_half(ConfigurationKey(key)); }
    let previous_key = key.wrapping_add(index.wrapping_sub(1).wrapping_mul(2));
    let previous = config_half(ConfigurationKey(previous_key)).wrapping_mul(10) as u16;
    let current = config_half(ConfigurationKey(key + index * 2));
    let difference = previous.wrapping_sub(current.wrapping_mul(10) as u16) as i16;
    let previous_position = signed_half(NativeAddress(0x00ef50c4u32.wrapping_add(index.wrapping_sub(1).wrapping_mul(2))));
    let current_position = u32::from(half(NativeAddress(0x00ef50c4 + index * 2)));
    let span = previous_position.wrapping_sub(current_position as i32) as i16;
    let first = unsafe { signed_divide(i32::from(selected).wrapping_mul(i32::from(difference)) as u32, i32::from(span) as u32) } as i32;
    let second = unsafe { signed_divide(previous_position.wrapping_mul(i32::from(difference)) as u32, i32::from(span) as u32) } as i32;
    xm1_power_record_helper_741fa0(first.wrapping_add(i32::from(previous as i16).wrapping_sub(second))) as u32
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa7e20(value: i32, alternate: u32, mode: u32) -> u32 {
    let category = if alternate != 0 { 3 }
        else if mode >= 32 { 0 }
        else if (1u32 << mode) & 0x0c080004 != 0 { 1 }
        else if (1u32 << mode) & 0x80040002 != 0 { 2 }
        else if mode == 3 { 3 }
        else { 0 };
    scaled_profile_rate(value, category)
}

#[no_mangle]
pub extern "C" fn xm1_power_record_helper_fa8078(first: u32, second: u32, profile: u32) -> u32 {
    let category = byte(NativeAddress(0x00ef6df0u32.wrapping_add(profile.wrapping_mul(20))));
    let key = if first == 1 {
        match second { 6 => 0x82bf, 7 => 0x82ba, 5 => 0x82bd, _ => 0x82be }
    } else { word(NativeAddress(0x00faaf18 + u32::from(category) * 4)) };
    if key == 0 { 0 } else { config(ConfigurationKey(key)) }
}
