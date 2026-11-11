use core::mem::MaybeUninit;
use super::shared::{Address32, ByteOffset};
use super::sensor_requests::{byte, half, store, store_byte, word, DescriptorHardwareFlags, ExpandedDescriptor};
use super::sensor_capture::HardwareUsers;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RegisterShadow(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RegisterPayload(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RegisterTransferMap(pub Address32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RegisterTransferCount(pub u32);
#[repr(transparent)]
struct RouteIndex(u32);
#[repr(transparent)]
struct RouteHandle(u32);
#[repr(transparent)]
struct CriticalToken(u32);

unsafe extern "C" {
    fn xm1_pipeline_image_route_acquire(index: RouteIndex, output: Address32) -> i32;
    fn xm1_pipeline_image_route_release(handle: RouteHandle) -> i32;
    fn xm1_develop_enter_critical() -> CriticalToken;
    fn xm1_develop_leave_critical(token: CriticalToken);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_acquire_hardware(flags: DescriptorHardwareFlags) -> HardwareUsers {
    let handles=Address32(0x14c6cc0);
    let mut channels=0;
    for index in 0..13 {
        if unsafe { half(Address32(0xda24d4),index*4) } as u32 & flags.0 != 0 { channels|=unsafe { half(Address32(0xda24d4),index*4+2) } as u32; }
    }
    let mut users=0;
    if channels & 0x80 != 0 {
        if flags.0 & 0x800 != 0 { unsafe { store_byte(Address32(0x14c6cbc),0,byte(Address32(0x14c6cbc),0)|1); } }
        if unsafe { word(handles,28) } == 0 && unsafe { xm1_pipeline_image_route_acquire(RouteIndex(15),handles.offset(ByteOffset(28))) } != -20 { users=unsafe { byte(Address32(0x14c6cbc),0) } as u32; }
    }
    if channels & 1 != 0 {
        if flags.0 & 0x1000 != 0 { unsafe { store_byte(Address32(0x14c6ca4),0,byte(Address32(0x14c6ca4),0)|2); } }
        if unsafe { word(handles,0) } == 0 && unsafe { xm1_pipeline_image_route_acquire(RouteIndex(4),handles) } != -20 { users=unsafe { byte(Address32(0x14c6ca4),0) } as u32; }
    }
    for (mask,index,route) in [(2,1,8),(4,2,17),(8,3,18),(16,4,19),(32,5,20),(64,6,16)] {
        if channels & mask != 0 && unsafe { word(handles,index*4) } == 0 { let _=unsafe { xm1_pipeline_image_route_acquire(RouteIndex(route),handles.offset(ByteOffset(index*4))) }; }
    }
    HardwareUsers(users)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_release_hardware(flags: DescriptorHardwareFlags, users: HardwareUsers) {
    let handles=Address32(0x14c6cc0);
    for (flag,user,offset,state,bit) in [(0x800,1,28,0x14c6cbc,1u8),(0x1000,2,0,0x14c6ca4,2u8)] {
        if flags.0 & flag != 0 && users.0 == user && unsafe { word(handles,offset) } != 0 {
            let state=Address32(state);
            let remaining=unsafe { byte(state,0) } & !bit;
            unsafe { store_byte(state,0,remaining); }
            if remaining == 0 {
                let token=unsafe { xm1_develop_enter_critical() };
                let _=unsafe { xm1_pipeline_image_route_release(RouteHandle(word(handles,offset))) };
                unsafe { store(handles,offset,0);xm1_develop_leave_critical(token); }
            }
        }
    }
    for (mask,offset) in [(1,4),(0x522,8),(4,12),(8,16),(0xd0,20),(0x200,24)] {
        if flags.0 & mask != 0 {
            let handle=unsafe { word(handles,offset) };
            if handle != 0 { let _=unsafe { xm1_pipeline_image_route_release(RouteHandle(handle)) };unsafe { store(handles,offset,0); } }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_descriptor_acquire(flags: DescriptorHardwareFlags) { let _=unsafe { xm1_pipeline_sensor_acquire_hardware(flags) }; }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_descriptor_release(flags: DescriptorHardwareFlags) { unsafe { xm1_pipeline_sensor_release_hardware(flags,HardwareUsers(1)); } }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_apply_register_transfers(image: RegisterShadow, transfers: RegisterTransferMap, count: RegisterTransferCount) {
    for index in 0..count.0 {
        let transfer=transfers.0.offset(ByteOffset(index.wrapping_mul(12)));
        let mut destination=Address32(unsafe { word(transfer,0) });
        let mut source=image.0.offset(ByteOffset(unsafe { word(transfer,4) }));
        if unsafe { half(transfer,8) } != 0 {
            let mut completed=0u16;
            loop {
                let value=unsafe { core::ptr::read_volatile(source.0 as *const MaybeUninit<u32>) };
                unsafe { core::ptr::write_volatile(destination.0 as *mut MaybeUninit<u32>,value); }
                source=source.offset(ByteOffset(4));destination=destination.offset(ByteOffset(4));completed=completed.wrapping_add(1);
                if completed >= unsafe { half(transfer,8) } { break; }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_capture_register_transfers(image: RegisterShadow, transfers: RegisterTransferMap, count: RegisterTransferCount) {
    for index in 0..count.0 {
        let transfer=transfers.0.offset(ByteOffset(index.wrapping_mul(12)));
        let mut source=Address32(unsafe { word(transfer,0) });
        let mut destination=image.0.offset(ByteOffset(unsafe { word(transfer,4) }));
        if unsafe { half(transfer,8) } != 0 {
            let mut completed=0u16;
            loop {
                let value=unsafe { core::ptr::read_volatile(source.0 as *const MaybeUninit<u32>) };
                unsafe { core::ptr::write_volatile(destination.0 as *mut MaybeUninit<u32>,value); }
                source=source.offset(ByteOffset(4));destination=destination.offset(ByteOffset(4));completed=completed.wrapping_add(1);
                if completed >= unsafe { half(transfer,8) } { break; }
            }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SignedFieldValue(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct SignedFieldWidth(pub i32);
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum FieldClipping { Clamped=0, WithinRange=1 }

fn clipped_field(value: SignedFieldValue, width: SignedFieldWidth) -> (u32,FieldClipping) {
    let positive_bits=width.0.wrapping_sub(1);
    let maximum=if positive_bits <= 0 { 0 } else if positive_bits >= 32 { u32::MAX } else { (1u32<<positive_bits)-1 };
    let (value,status)=if value.0 > maximum as i32 { (maximum,FieldClipping::Clamped) }
        else if value.0 < (!maximum) as i32 { (!maximum,FieldClipping::Clamped) } else { (value.0 as u32,FieldClipping::WithinRange) };
    let shift=(positive_bits as u32)&255;
    let sign=if shift >= 32 { 0 } else { 1u32<<shift };
    (value&(maximum|sign),status)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_pipeline_sensor_clip_signed_field(value: SignedFieldValue, width: SignedFieldWidth, output: Address32) -> FieldClipping {
    let (packed,status)=clipped_field(value,width);
    unsafe { store(output,0,packed); }
    status
}
