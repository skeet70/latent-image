use core::ptr::{read_volatile, write_volatile};
use crate::{Calibration, CalibrationOffset, CalibrationRegion, CalibrationStorage, Defaults, ReloadWindow, Width, CACHE_SIZE};
use crate::publication::*;

extern "C" {
    fn retained_primary(destination: *mut u8, length: u32) -> u32;
    fn retained_backup(destination: *mut u8, offset: u32, length: u32) -> u32;
    fn retained_fixed(destination: *mut u8, offset: u32, length: u32) -> u32;
    fn retained_copy(destination: *mut u8, source: *const u8, length: u32);
    fn retained_wb_convert(bytes: *mut u8, mode: i32);
    fn retained_wb_publish(bytes: *mut u8);
    fn retained_halfword(value: u32);
    fn retained_lock(domain: u32, service: u32, acquire: u32);
    fn retained_rebuild();
}
struct Storage;
impl CalibrationStorage for Storage {
    fn read_primary(&mut self, destination: &mut [u8]) { unsafe { retained_primary(destination.as_mut_ptr(), destination.len() as u32); } }
    fn read_backup(&mut self, destination: &mut [u8]) { unsafe { retained_backup(destination.as_mut_ptr(), 0x400000, destination.len() as u32); } }
    fn read_fixed(&mut self, offset: CalibrationOffset, destination: &mut [u8]) { unsafe { retained_fixed(destination.as_mut_ptr(), offset.0 as u32, destination.len() as u32); } }
}
unsafe fn cache() -> Calibration<'static> { Calibration::borrow(&mut *retained_cache().cast::<[u8; CACHE_SIZE]>()) }
unsafe fn defaults() -> Defaults<'static> {
    let base = 0x7c8a44 as *const u8;
    match Defaults::parse(core::slice::from_raw_parts(base, 504)) {
        Ok(resource) => resource,
        Err(_) => loop { core::hint::spin_loop(); },
    }
}
#[cfg(control_caller_stack)]
#[inline(always)]
unsafe fn corrupt_caller() {
    let frame: usize;
    core::arch::asm!("mov {}, r11", out(reg) frame, options(nomem, nostack, preserves_flags));
    write_volatile((frame + 12) as *mut u32, 0xdeadbeef);
}
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_initialize() -> u32 {
    #[cfg(control_caller_stack)]
    corrupt_caller();
    write_volatile(0x96ca50 as *mut u32, 0x41acd800);
    cache().initialize(&mut Storage, &defaults());
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_defaults() -> u32 { cache().load_defaults(&defaults()); 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_reload(kind: u32) -> u32 { cache().reload(&mut Storage, ReloadWindow::parse(kind)); 0 }

unsafe fn load(base: usize, offset: usize, width: Width) -> ParameterBits {
    let pointer = (base + offset) as *const u8;
    let value = match width {
        Width::Byte => u32::from(read_volatile(pointer)),
        Width::Halfword => u32::from(read_volatile(pointer.cast::<u16>())),
        Width::Word => read_volatile(pointer.cast::<u32>()),
        Width::ThreeBytes => u32::from(read_volatile(pointer)) | (u32::from(read_volatile(pointer.add(1))) << 8) | (u32::from(read_volatile(pointer.add(2))) << 16),
    };
    ParameterBits(value)
}
struct LiveView { input: usize }
impl Publication for LiveView {
    fn input(&self, offset: ParameterOffset, width: Width) -> ParameterBits { unsafe { load(self.input, offset.0, width) } }
    fn current(&self, offset: ParameterOffset, width: Width) -> ParameterBits { unsafe { load(0x1699584, offset.0, width) } }
    fn store(&mut self, offset: ParameterOffset, width: Width, value: ParameterBits) {
        unsafe {
            let pointer = (0x1699584 + offset.0) as *mut u8;
            match width {
                Width::Byte => write_volatile(pointer, value.0 as u8),
                Width::Halfword => write_volatile(pointer.cast::<u16>(), value.0 as u16),
                Width::Word => write_volatile(pointer.cast::<u32>(), value.0),
                Width::ThreeBytes => unreachable!(),
            }
        }
    }
    fn gate(&self) -> bool { unsafe { read_volatile(0x1698a40 as *const u8) & 0x80 != 0 } }
    fn dirty(&self, cell: DirtyCell) -> DirtyBits { unsafe { DirtyBits(read_volatile((0x1698a34 + cell.offset()) as *const u8)) } }
    fn raise(&mut self, cell: DirtyCell, bits: DirtyBits) { unsafe {
        let pointer = (0x1698a34 + cell.offset()) as *mut u8;
        write_volatile(pointer, read_volatile(pointer) | bits.0);
    } }
    fn descriptor(&self, descriptor: Descriptor) -> bool { unsafe { read_volatile((0x1698a50 + descriptor.offset()) as *const u32) == 1 } }
    fn lane(&self, lane: ProcessingLane) -> bool { unsafe { read_volatile((0x169964c + match lane { ProcessingLane::First => 8, ProcessingLane::Second => 12 }) as *const u32) == 1 } }
    fn white_balance_mode(&self) -> WhiteBalanceMode { unsafe { WhiteBalanceMode(read_volatile(0x1699540 as *const i16)) } }
    fn white_balance_copy(&mut self) {
        #[cfg(control_copy_zero)]
        let length = 0;
        #[cfg(control_copy_five)]
        let length = 5;
        #[cfg(not(any(control_copy_zero, control_copy_five)))]
        let length = 6;
        unsafe { retained_copy(0x1699ce0 as *mut u8, (self.input + 0x3c) as *const u8, length); }
    }
    fn white_balance_convert(&mut self, mode: WhiteBalanceMode) { unsafe { retained_wb_convert(0x1699ce0 as *mut u8, i32::from(mode.0)); } }
    fn white_balance_publish(&mut self) { unsafe { retained_wb_publish(0x1699ce0 as *mut u8); } }
    fn halfword_publish(&mut self, value: HalfwordParameter) { unsafe { retained_halfword(u32::from(value.0)); } }
    fn auxiliary_mode(&self) -> AuxiliaryMode { unsafe { AuxiliaryMode::parse(read_volatile(0x1699688 as *const u32)) } }
    fn auxiliary_value(&self) -> ParameterBits { unsafe { ParameterBits(read_volatile(0x169968c as *const u32)) } }
    fn store_auxiliary_value(&mut self, value: ParameterBits) { unsafe { write_volatile(0x169968c as *mut u32, value.0); } }
    fn lock(&mut self) { unsafe { retained_lock(43, 87, 1); } }
    fn rebuild(&mut self) { unsafe { retained_rebuild(); } }
    fn unlock(&mut self) { unsafe { retained_lock(43, 87, 0); } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_liveview_publish(input: *const u8) { publish(&mut LiveView { input: input as usize }); }
#[no_mangle]
pub unsafe extern "C" fn xm1_liveview_service(input: *const u8) -> u32 {
    #[cfg(control_caller_stack)]
    corrupt_caller();
    publish_and_rebuild(&mut LiveView { input: input as usize });
    0
}

unsafe fn retained_cache() -> *mut u8 { read_volatile(0x96ca50 as *const *mut u8) }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_u8(offset: u32) -> u32 { u32::from(read_volatile(retained_cache().add(offset as usize))) }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_u16(offset: u32) -> u32 { xm1_calibration_u8(offset) | (xm1_calibration_u8(offset.wrapping_add(1)) << 8) }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_u24(offset: u32) -> u32 { xm1_calibration_u16(offset) | (xm1_calibration_u8(offset.wrapping_add(2)) << 16) }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_u32(offset: u32) -> u32 { xm1_calibration_u24(offset) | (xm1_calibration_u8(offset.wrapping_add(3)) << 24) }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_i8(offset: u32) -> i32 { xm1_calibration_u8(offset) as i8 as i32 }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_i16(offset: u32) -> i32 { xm1_calibration_u16(offset) as i16 as i32 }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_byte_halfword(offset: u32) -> i32 { xm1_calibration_u8(offset) as i32 }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_set_byte(offset: u32, value: u32) { write_volatile(retained_cache().add(offset as usize), value as u8); }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_pointer(offset: u32) -> *mut u8 { retained_cache().wrapping_add(offset as usize) }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_region(region: u32) -> *mut u8 {
    xm1_calibration_pointer(CalibrationRegion::parse(region).offset() as u32)
}
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_fixed_halfword() -> u32 { xm1_calibration_u16(0x102) }
#[no_mangle]
pub extern "C" fn xm1_calibration_zero() -> u32 { 0 }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_base() -> *mut u8 { retained_cache() }
#[no_mangle]
pub unsafe extern "C" fn xm1_calibration_resource_base() -> *mut u8 { xm1_calibration_pointer(CalibrationRegion::Resource.offset() as u32) }
