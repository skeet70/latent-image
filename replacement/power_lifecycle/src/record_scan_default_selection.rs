//! X-M1 1.50 grid selection at 0xf4f838..0xf51a28.
//! x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, image-5 ARM bytes.

use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

#[path = "record_scan_default_selection_primary.rs"]
mod primary;
#[path = "record_scan_default_selection_secondary.rs"]
mod secondary;
#[path = "record_scan_default_selection_diagnostics.rs"]
mod diagnostics;

const STATE: u32 = 0x016caf00;
#[derive(Clone, Copy)]
struct Profile(u32);
#[derive(Clone, Copy)]
struct Selection(u32);
#[derive(Clone, Copy)]
struct Flags(u16);
struct Choice { selection: Selection, flags: Flags }
impl Choice {
    fn empty() -> Self { Self { selection: Selection(0), flags: Flags(0) } }
    fn value_pointer(&mut self) -> *mut u32 { &mut self.selection.0 }
    fn flags_pointer(&mut self) -> *mut u16 { &mut self.flags.0 }
}
#[derive(Clone, Copy)]
struct Kind(u32);
#[derive(Clone, Copy)]
struct ConfigurationKey(u32);
struct Scratch(MaybeUninit<[u32; 22]>);
impl Scratch {
    fn new() -> Self { Self(MaybeUninit::uninit()) }
    fn pointer(&mut self) -> *mut u8 { self.0.as_mut_ptr().cast() }
    unsafe fn word(&self, offset: usize) -> u32 { unsafe { self.0.as_ptr().cast::<u8>().add(offset).cast::<u32>().read() } }
    unsafe fn half(&self, offset: usize) -> u16 { unsafe { self.0.as_ptr().cast::<u8>().add(offset).cast::<u16>().read() } }
}
unsafe fn word(address:u32)->u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn half(address:u32)->u16 { unsafe { read_volatile(address as *const u16) } }
unsafe fn byte(address:u32)->u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn put(address:u32,value:u32) { unsafe { write_volatile(address as *mut u32,value); } }
unsafe fn put_half(address:u32,value:u16) { unsafe { write_volatile(address as *mut u16,value); } }
unsafe fn put_byte(address:u32,value:u8) { unsafe { write_volatile(address as *mut u8,value); } }
unsafe fn copy(destination:u32,source:u32,bytes:u32) {
    for offset in 0..bytes { unsafe { put_byte(destination.wrapping_add(offset),byte(source.wrapping_add(offset))); } }
}
unsafe fn configuration(key:ConfigurationKey)->u32 { unsafe { xm1_power_record_helper_69f8fc(key.0) } }
unsafe fn query(profile:Profile,kind:Kind,choice:&mut Choice) {
    unsafe { xm1_power_record_helper_f4f1e8(profile.0,kind.0,choice.value_pointer(),choice.flags_pointer()); }
}
unsafe fn history(kind:Kind,choice:&mut Choice) {
    unsafe { xm1_power_record_helper_f4d790(kind.0,choice.value_pointer(),choice.flags_pointer()); }
}
unsafe fn adjust(profile:Profile,choice:&mut Choice) {
    unsafe {
        if xm1_power_record_helper_f4acd4(profile.0)!=0 { choice.selection=Selection(xm1_power_record_helper_f4acfc(choice.selection.0)); }
        if xm1_power_record_helper_f4ad54(profile.0)!=0 && word(STATE+0x74)==1 { choice.flags.0|=1; }
    }
}
unsafe fn bound_flags(profile:Profile,kind:Kind,choice:&mut Choice) {
    unsafe { xm1_power_record_helper_f4b49c(profile.0,kind.0,choice.selection.0,choice.flags_pointer()); }
}
unsafe fn override_selection(profile:Profile,key:ConfigurationKey,shift:u32,choice:&mut Choice) {
    unsafe {
        if configuration(key)!=255 {
            choice.selection=Selection(configuration(key));
            if configuration(ConfigurationKey(0x65a8)) & (1<<shift)!=0 || configuration(ConfigurationKey(0x65a8)) & (2<<shift)!=0 {
                choice.flags=Flags(((configuration(ConfigurationKey(0x65a8))>>shift)&3) as u16);
            }
        }
        if configuration(ConfigurationKey(0x7606))!=0 && xm1_power_record_helper_74e5c0(profile.0)==1 {
            let value=configuration(ConfigurationKey(0x7606));
            choice.selection=Selection(xm1_power_record_helper_f4a0e4(value));
        }
    }
}

unsafe extern "C" {
    fn xm1_power_record_helper_69f8fc(key:u32)->u32;
    fn xm1_power_record_helper_69fec8(key:u32)->u32;
    fn xm1_power_record_helper_69ffe8(key:u32)->i32;
    fn xm1_power_record_helper_74e5c0(profile:u32)->u32;
    fn xm1_power_record_helper_74e6ac(profile:u32,selection:u32);
    fn xm1_power_record_helper_74e7c4(profile:u32,flags:u32);
    fn xm1_power_record_helper_74f09c(profile:u32)->u32;
    fn xm1_power_record_helper_756ac4(profile:u32)->u32;
    fn xm1_power_record_helper_756d04(profile:u32)->u32;
    fn xm1_power_record_helper_7588e8(output:*mut u8,profile:u32);
    fn xm1_power_record_helper_746c2c(value:u32)->u32;
    fn xm1_power_record_helper_fa7edc(value:u32)->u32;
    fn xm1_power_record_helper_ef72d4(kind:u32,value:u32)->u32;
    fn xm1_power_record_helper_f4f1e8(profile:u32,kind:u32,selection:*mut u32,flags:*mut u16);
    fn xm1_power_record_helper_f4d790(kind:u32,selection:*mut u32,flags:*mut u16);
    fn xm1_power_record_helper_f4d8bc(selection:u32,flags:u32);
    fn xm1_power_record_helper_f4d968(selection:u32,flags:u32);
    fn xm1_power_record_helper_f4dea4(record:u32);
    fn xm1_power_record_helper_f4dc18();
    fn xm1_power_record_helper_f4dbac();
    fn xm1_power_record_helper_f4d884();
    fn xm1_power_record_helper_f4dc50(record:u32);
    fn xm1_power_record_helper_f49fd8(selection:u32)->u32;
    fn xm1_power_record_helper_f4a0e4(value:u32)->u32;
    fn xm1_power_record_helper_f4a520(profile:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4a554(profile:u32)->u32;
    fn xm1_power_record_helper_f4a57c(selection:u32)->u32;
    fn xm1_power_record_helper_f4acd4(profile:u32)->u32;
    fn xm1_power_record_helper_f4acfc(selection:u32)->u32;
    fn xm1_power_record_helper_f4ad54(profile:u32)->u32;
    fn xm1_power_record_helper_f4a014(selection:u32,output:*mut u32,flags:*mut u16);
    fn xm1_power_record_helper_f4a848(previous:u32,selection:u32,kind:u32)->u32;
    fn xm1_power_record_helper_f4a154(selection:u32,filtered:u32)->u32;
    fn xm1_power_record_helper_f4b49c(profile:u32,kind:u32,selection:u32,flags:*mut u16);
    fn xm1_power_record_helper_f4bc58(profile:u32,kind:u32,selection:u32)->u32;
    fn xm1_power_record_helper_f4c210(profile:u32,mode:u32)->u32;
    fn xm1_power_record_helper_f4e058(selection:u32,flags:u32);
}

unsafe fn telemetry(profile:Profile,scratch:&mut Scratch) {
    unsafe {
        if configuration(ConfigurationKey(0xa8))&128==0 || xm1_power_record_helper_74e5c0(profile.0)!=1 { return; }
        xm1_power_record_helper_7588e8(scratch.pointer(),profile.0);
        let first=scratch.half(40) as i16 as i32 as u32;
        let timing=scratch.word(56);
        let auxiliary=xm1_power_record_helper_756ac4(profile.0);
        let position=xm1_power_record_helper_fa7edc(auxiliary);
        let bias=xm1_power_record_helper_ef72d4(2,position);
        let rounded=xm1_power_record_helper_746c2c(timing);
        let base=0x5ffe7854;
        macro_rules! sample {
            ($offset:expr,$value:expr) => {{
                let index=half(STATE+0x191c) as u32;
                put(base+index*60+$offset,$value);
            }};
        }
        sample!(0,word(STATE)); sample!(4,half(STATE+4) as u32);
        sample!(8,word(STATE+0x1038)); sample!(12,timing);
        sample!(16,word(STATE+0xfe8)); sample!(20,word(STATE+0xfd0));
        sample!(24,half(STATE+0xfd8) as i16 as i32 as u32);
        sample!(28,half(STATE+0xfde) as i16 as i32 as u32);
        sample!(32,half(STATE+0xfdc) as i16 as i32 as u32);
        let first=bias.wrapping_add(first);
        sample!(36,first); sample!(40,bias.wrapping_add(scratch.half(42) as i16 as i32 as u32));
        sample!(44,first.wrapping_add(rounded)); sample!(48,bias.wrapping_add(scratch.half(48) as i16 as i32 as u32));
        sample!(52,scratch.half(74) as i16 as i32 as u32);
        sample!(56,u32::from(word(STATE+0xfb8)!=0));
        let next=half(STATE+0x191c).wrapping_add(1);
        put_half(STATE+0x191c,next);
        if next>100 { put_half(STATE+0x191c,0); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f4f838(profile:u32,kind:u32,value_pointer:u32,flags_pointer:u32) {
    unsafe {
        let profile=Profile(profile);
        let kind=Kind(kind);
        let mut choice=Choice::empty();
        let mut scratch=Scratch::new();
        if kind.0==1 { primary::select(profile,kind,&mut choice); }
        else { secondary::select(profile,kind,&mut choice,&mut scratch); }
        let selection=choice.selection;
        let flags=choice.flags;
        let _=xm1_power_record_helper_74e5c0(profile.0);
        xm1_power_record_helper_74e6ac(profile.0,selection.0);
        xm1_power_record_helper_74e7c4(profile.0,flags.0 as u32);
        telemetry(profile,&mut scratch);
        diagnostics::emit(profile,&mut scratch);
        if configuration(ConfigurationKey(0xeb))!=0 { xm1_power_record_helper_f4e058(choice.selection.0,choice.flags.0 as u32); }
        if value_pointer!=0 { put(value_pointer,choice.selection.0); }
        if flags_pointer!=0 { put_half(flags_pointer,choice.flags.0); }
    }
}
