#![cfg(target_arch = "arm")]

use core::mem::MaybeUninit;
use core::ptr::{copy_nonoverlapping, read_volatile, write_volatile};

extern "C" {
    fn xm1_power_record_helper_f160d0() -> u32;
    fn xm1_power_record_helper_f16184() -> u32;
    fn xm1_power_record_helper_f17838(value: u32);
    fn xm1_power_record_helper_f17984(kind: u32, destination: u32);
    fn xm1_power_record_helper_f15cbc(use_three: u32);
    fn xm1_power_record_helper_752714(profile: u32) -> u32;
    fn xm1_power_record_helper_74eba8(profile: u32) -> u32;
    fn xm1_power_record_helper_74fc64(profile: u32) -> u32;
    fn xm1_power_record_helper_74e648(profile: u32) -> u32;
    fn xm1_power_record_helper_74e718(profile: u32) -> u32;
    fn xm1_power_record_helper_75060c(profile: u32) -> u32;
    fn xm1_power_record_helper_75624c(profile: u32) -> u32;
    fn xm1_power_record_helper_7562e4(profile: u32, index: u32) -> u32;
    fn xm1_power_record_helper_756550(profile: u32) -> u32;
    fn xm1_power_record_helper_7565e0(profile: u32) -> u32;
    fn xm1_power_record_helper_756670(profile: u32) -> u32;
    fn xm1_power_record_helper_7563d4(profile: u32) -> u32;
    fn xm1_power_record_helper_75641c(profile: u32, destination: u32);
    fn xm1_power_record_helper_7564c0(profile: u32) -> u32;
    fn xm1_power_record_helper_7557dc(profile: u32, index: u32) -> u32;
    fn xm1_power_record_helper_752928(output: u32, profile: u32, index: u32);
    fn xm1_power_record_helper_750a4c(output: u32, profile: u32);
    fn xm1_power_record_helper_751d3c(output: u32, profile: u32);
    fn xm1_power_record_helper_7588e8(output: u32, profile: u32);
    fn xm1_power_record_helper_75ae3c(destination: u32, source: u32);
    fn xm1_power_record_helper_75ae78(destination: u32, source: u32);
    fn xm1_power_record_helper_75ae8c(destination: u32, source: u32);
    fn xm1_power_record_helper_75d490(destination: u32, value: u32);
    fn xm1_power_record_helper_76f394(a: u32, b: u32, c: u32) -> u32;
    fn xm1_power_record_helper_f4f838(profile: u32, kind: u32, value: u32, flags: u32);
    fn xm1_power_record_helper_fa6904(a: u32, b: u32) -> u32;
    fn xm1_power_record_helper_10517f8(context: u32) -> u32;
    fn xm1_power_record_helper_103b044(context: u32);
    fn xm1_power_record_helper_69f8fc(setting: u32) -> u32;
    fn xm1_power_record_helper_f60e28(profile: u32);
    fn xm1_power_record_helper_756734(profile: u32, output: u32);
    fn xm1_power_record_helper_756754(profile: u32, input: u32);
}

unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn put(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }
unsafe fn half(address: u32, value: u32) { unsafe { write_volatile(address as *mut u16, value as u16); } }

#[derive(Clone, Copy)]
enum Completion {
    Finished,
    Single,
    Multiple,
    Extended,
    Other,
}
impl Completion {
    fn parse(value: u32, flags: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Finished),
            3 if flags & 1 != 0 => Some(Self::Single),
            4 if flags & 2 != 0 => Some(Self::Multiple),
            5 if flags & 4 != 0 => Some(Self::Extended),
            6 if flags & 8 != 0 => Some(Self::Other),
            _ => None,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_efa3f8(profile: u32, event: u32) {
    unsafe {
        let slot = 0x0169_d05c;
        let parent = word(word(slot + 44));
        let processing = if parent == 0 { 0 } else { parent.wrapping_add(0x35c8) };
        let metadata_context = if parent == 0 { 0 } else { parent.wrapping_add(0xa69e0) };
        let image_context = if parent == 0 { 0 } else { parent.wrapping_add(0xb19f4) };
        let Some(completion) = Completion::parse(event, word(slot + 16)) else { return; };
        if matches!(completion, Completion::Finished) {
            write_volatile(0x0169_cf2a as *mut u8, 0);
        }
        let selection = xm1_power_record_helper_f160d0();
        let descriptor = xm1_power_record_helper_752714(profile);
        let mut selected = 0;
        let mut snapshot = MaybeUninit::<[u32; 42]>::uninit();
        let output = snapshot.as_mut_ptr() as u32;
        match completion {
            Completion::Finished => {
                let mut mode = xm1_power_record_helper_74eba8(profile);
                if mode == 0 {
                    xm1_power_record_helper_752928(output, profile, 0);
                    mode = word(output + 160);
                    xm1_power_record_helper_f17984(1, 0x0169_f5d0);
                    xm1_power_record_helper_f17984(2, 0x0169_f9e4);
                }
                xm1_power_record_helper_f17838(mode);
                if xm1_power_record_helper_74fc64(profile) == 0 { xm1_power_record_helper_f15cbc(0); }
                selected = xm1_power_record_helper_f16184();
                if selected != 0 { xm1_power_record_helper_76f394(selection, word(slot + 52), 1); }
                let destination = word(slot + 40);
                put(destination + 0x414, xm1_power_record_helper_75624c(profile));
                for index in 0..10 {
                    let source = xm1_power_record_helper_7562e4(profile, index);
                    copy_nonoverlapping(source as *const u8, (word(slot + 40) + 0x418 + index * 0x68) as *mut u8, 0x68);
                }
                let value = xm1_power_record_helper_756550(profile);
                half(word(slot + 40) + 0x1464, value);
                let value = xm1_power_record_helper_7565e0(profile);
                half(word(slot + 40) + 0x1466, value);
                let value = xm1_power_record_helper_756670(profile);
                half(word(slot + 40) + 0x1468, value);
                let value = xm1_power_record_helper_7563d4(profile);
                xm1_power_record_helper_75d490(word(slot + 40), value);
                xm1_power_record_helper_75641c(profile, word(slot + 40) + 0x1470);
                let value = xm1_power_record_helper_7564c0(profile);
                put(word(slot + 40) + 0x14ec, value);
                for index in 0..10 {
                    let destination = word(slot + 40);
                    half(destination + 0x14d8 + index * 2, xm1_power_record_helper_7557dc(profile, index));
                }
            }
            Completion::Single => {
                xm1_power_record_helper_750a4c(output, profile);
                xm1_power_record_helper_75ae3c(word(slot + 28), output);
            }
            Completion::Multiple => {
                if word(word(slot + 24)) != 0 {
                    let mut index = 0u32;
                    loop {
                        xm1_power_record_helper_752928(output, profile, index);
                        xm1_power_record_helper_75ae78(word(word(slot + 32) + index * 4), output);
                        index = index.wrapping_add(1) & 0xff;
                        if index >= word(word(slot + 24)) { break; }
                    }
                }
            }
            Completion::Extended => {
                xm1_power_record_helper_751d3c(output, profile);
                xm1_power_record_helper_75ae8c(word(slot + 36), output);
            }
            Completion::Other => {}
        }
        let mut resource = xm1_power_record_helper_74e648(profile);
        let mut flags = xm1_power_record_helper_74e718(profile) as u16;
        let mut value;
        let mut threshold = 0;
        if matches!(completion, Completion::Finished) {
            if word(slot + 4) == 5 {
                xm1_power_record_helper_7588e8(output, profile);
                if word(output + 8) != 0 {
                    xm1_power_record_helper_f4f838(profile, 1, &mut resource as *mut u32 as u32, &mut flags as *mut u16 as u32);
                }
            }
            value = xm1_power_record_helper_75060c(profile);
            if value == 1 {
                xm1_power_record_helper_752928(output, profile, 0);
                value = xm1_power_record_helper_fa6904(word(output + 36), word(output + 40));
            }
            if processing != 0 { put(processing.wrapping_add(0xe0d4), value); }
            if metadata_context != 0 { xm1_power_record_helper_10517f8(metadata_context); }
            if image_context != 0 { xm1_power_record_helper_103b044(image_context); }
            let limit = read_volatile(0x016a_048c as *const u8) as u32;
            threshold = u32::from(limit >= xm1_power_record_helper_69f8fc(0x2b81));
            if matches!(word(slot + 4), 0 | 3 | 6) { xm1_power_record_helper_f60e28(profile); }
            let mut metadata = MaybeUninit::<[u32; 33]>::uninit();
            let metadata = metadata.as_mut_ptr() as u32;
            xm1_power_record_helper_756734(profile, metadata);
            put(metadata + 104, 0);
            xm1_power_record_helper_756754(profile, metadata);
        } else {
            value = xm1_power_record_helper_75060c(profile);
        }
        let callback = word(slot);
        if callback != 0 {
            let callback: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, u32) = core::mem::transmute(callback);
            callback(event, selection, word(slot + 52), descriptor, selected, resource, flags as u32, value, threshold);
        }
    }
}
