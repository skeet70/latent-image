//! X-M1 profile selection, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, confirmed against image-5 bytes.
use core::ptr::{addr_of, read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ExposureProfile(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SceneCode(pub u32);

#[repr(C)]
pub struct ExposureRequest {
    pub mode: u32,
    pub scene: SceneCode,
    pub family: u32,
    pub selector: u32,
    pub enabled: u32,
    reserved14: [u32; 2],
    pub alternate: u32,
    reserved20: u32,
    pub level: u16,
    pub flag26: u8,
    pub variant27: u8,
}
const _: () = assert!(core::mem::size_of::<ExposureRequest>() == 0x28);

#[derive(Clone, Copy, Eq, PartialEq)]
enum SelectionFamily { Primary, Secondary, ExpandedPrimary, ExpandedSecondary }
impl SelectionFamily {
    fn secondary(self) -> u32 { matches!(self, Self::Secondary | Self::ExpandedSecondary) as u32 }
    fn expanded(self) -> bool { matches!(self, Self::ExpandedPrimary | Self::ExpandedSecondary) }
}
extern "C" { fn xm1_power_configuration_byte(offset: u32) -> u32; }

unsafe fn select(request: *const ExposureRequest, family: SelectionFamily) -> ExposureProfile {
    let secondary = family.secondary();
    if read_volatile(addr_of!((*request).mode)) == 0 {
        let alternate = read_volatile(addr_of!((*request).alternate));
        if alternate == 1 {
            let scene = read_volatile(addr_of!((*request).scene)).0;
            let level = read_volatile(addr_of!((*request).level));
            let fourth_limit = if secondary == 0 { 51 } else { 41 };
            let bucket = if level <= 10 { 0 } else if level <= 20 { 1 } else if level <= 30 { 2 } else if level < fourth_limit { 3 } else { 4 };
            if matches!(scene, 20 | 13) {
                let values = if secondary == 0 { [0xb1, 0xaf, 0xad, 0xa9, 0xa7] } else { [0xb2, 0xb0, 0xae, 0xac, 0xa8] };
                return ExposureProfile(values[bucket]);
            }
            if matches!(scene, 19 | 2 | 27) {
                let values = if secondary == 0 { [0xa5, 0xa3, 0xa1, 0x9d, 0x9b] } else { [0xa6, 0xa4, 0xa2, 0xa0, 0x9c] };
                return ExposureProfile(values[bucket]);
            }
            if family != SelectionFamily::ExpandedSecondary || level <= 30 {
                return ExposureProfile(if level <= 10 { 0xc5 + secondary } else if level <= 20 { 0xc3 + secondary } else { 0xc1 + secondary });
            }
        }
        if read_volatile(addr_of!((*request).flag26)) != 0 {
            let scene = read_volatile(addr_of!((*request).scene)).0;
            let shift = if family == SelectionFamily::Primary { 0 } else { 1 };
            return ExposureProfile(match scene {
                14 => 0x97 + shift,
                20 | 13 => 0x99 + shift,
                _ => if read_volatile(addr_of!((*request).variant27)) == 2 { 0x95 + shift } else { 0x93 + shift },
            });
        }
        let enabled = read_volatile(addr_of!((*request).enabled));
        let scene = read_volatile(addr_of!((*request).scene)).0;
        let value = if enabled == 1 {
            if family.expanded() {
                match scene { 2 | 17 => 0x61, 11 => 0x69, 12 => 0x67, 14 => 0x65, 18 | 31 => 0x5f, 19 => 0x63, 26 => 0xbf, 27 => 0xd0, _ => 0x5d }
            } else {
                match scene { 2 | 17 => 0x14, 11 => 0x1c, 12 => 0x1a, 14 => 0x1e, 18 | 31 => 0x16, 19 => 0x18, 26 => 0xbd, 27 => 0xce, _ => 0x12 }
            }
        } else {
            let standard = match scene { 20 => 0x22, 13 => 0x20, 17 => 10, 10 => 8, 11 => 14, 12 => 12, 26 => 0xbd, 14 => 16, 19 => 6, 27 => 0xca, 31 | 18 => 2, 2 => 4, _ => 0 };
            if family.expanded() { standard + if matches!(scene, 26 | 27) { 2 } else { 0x4b } } else { standard }
        };
        return ExposureProfile(value + secondary);
    }
    if read_volatile(addr_of!((*request).alternate)) == 1 {
        let scene = read_volatile(addr_of!((*request).scene)).0;
        return ExposureProfile(match scene { 4 | 22 => 0xb9, 5 | 24 => 0xb5, 15 | 23 => 0xbb, 16 | 25 => 0xb7, _ => 0xb3 } + secondary);
    }
    if read_volatile(addr_of!((*request).selector)) == 46 {
        let scene = read_volatile(addr_of!((*request).scene)).0;
        return ExposureProfile(match scene { 4 => 0x3c, 5 => 0x2c, 11 => 0x1c, 15 => 0x44, 16 => 0x34, 22 => 0x3e, 23 => 0x46, 24 => 0x2e, 25 => 0x36, _ => 0x1a } + secondary);
    }
    let scene = read_volatile(addr_of!((*request).scene)).0;
    let value = match scene { 7 | 28 => return ExposureProfile(0x48), 8 | 29 => return ExposureProfile(0x49), 9 | 30 => return ExposureProfile(0x4a), 4 => 0x38, 5 => 0x28, 11 => 14, 12 => 12, 15 => 0x40, 16 => 0x30, 21 => 0x26, 22 => 0x3a, 23 => 0x42, 24 => 0x2a, 25 => 0x32, _ => 0x24 };
    ExposureProfile(value + secondary)
}
macro_rules! selection {
    ($name:ident, $family:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(request: *const ExposureRequest, output: *mut ExposureProfile) {
            write_volatile(output, select(request, SelectionFamily::$family));
        }
    };
}
selection!(xm1_power_record_helper_f76414, Primary);
selection!(xm1_power_record_helper_f76a1c, Secondary);
selection!(xm1_power_record_helper_f77068, ExpandedPrimary);
selection!(xm1_power_record_helper_f77674, ExpandedSecondary);
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f77028(request: *const ExposureRequest, output: *mut ExposureProfile) {
    let family = if xm1_power_configuration_byte(0x6c08) == 1 { SelectionFamily::Secondary } else { SelectionFamily::Primary };
    write_volatile(output, select(request, family));
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f77c8c(request: *const ExposureRequest, output: *mut ExposureProfile) {
    match read_volatile(addr_of!((*request).family)) {
        3 => xm1_power_record_helper_f76414(request, output),
        4 => xm1_power_record_helper_f76a1c(request, output),
        1 => xm1_power_record_helper_f77068(request, output),
        2 => xm1_power_record_helper_f77674(request, output),
        _ => xm1_power_record_helper_f77028(request, output),
    }
}
