use super::*;

#[derive(Clone,Copy)]
struct FormatAddress(u32);
const LABELS:[FormatAddress;57]=[
    FormatAddress(0xf52f58),FormatAddress(0xf52f68),FormatAddress(0xf52f84),FormatAddress(0xf52f90),
    FormatAddress(0xf52fb0),FormatAddress(0xf52fc0),FormatAddress(0xf52fdc),FormatAddress(0xf52ffc),
    FormatAddress(0xf53008),FormatAddress(0xf53018),FormatAddress(0xf5302c),FormatAddress(0xf53048),
    FormatAddress(0xf53060),FormatAddress(0xf53080),FormatAddress(0xf530a0),FormatAddress(0xf530c4),
    FormatAddress(0xf530d8),FormatAddress(0xf530f0),FormatAddress(0xf53100),FormatAddress(0xf53110),
    FormatAddress(0xf53124),FormatAddress(0xf53134),FormatAddress(0xf53144),FormatAddress(0xf53158),
    FormatAddress(0xf53168),FormatAddress(0xf53178),FormatAddress(0xf53188),FormatAddress(0xf5319c),
    FormatAddress(0xf531b0),FormatAddress(0xf531c4),FormatAddress(0xf531d4),FormatAddress(0xf531e4),
    FormatAddress(0xf531f4),FormatAddress(0xf53204),FormatAddress(0xf5320c),FormatAddress(0xf53218),
    FormatAddress(0xf53228),FormatAddress(0xf5322c),FormatAddress(0xf53238),FormatAddress(0xf53248),
    FormatAddress(0xf53250),FormatAddress(0xf53260),FormatAddress(0xf53274),FormatAddress(0xf53294),
    FormatAddress(0xf532a4),FormatAddress(0xf532b8),FormatAddress(0xf532c4),FormatAddress(0xf532d4),
    FormatAddress(0xf532e4),FormatAddress(0xf532f4),FormatAddress(0xf53300),FormatAddress(0xf5330c),
    FormatAddress(0xf53318),FormatAddress(0xf5332c),FormatAddress(0xf53340),FormatAddress(0xf53354),FormatAddress(0xf53368),
];
unsafe extern "C" {
    fn xm1_power_record_diagnostic_words(context:u32,format:u32,arguments:*const u32,count:u32);
}
unsafe fn emit_format(format:FormatAddress,arguments:&[u32]) {
    unsafe { xm1_power_record_diagnostic_words(0xffb104,format.0,arguments.as_ptr(),arguments.len() as u32); }
}
unsafe fn copy_string(destination:*mut u8,source:FormatAddress) {
    let mut offset=0u32;
    unsafe {
        loop {
            let value=byte(source.0+offset);
            destination.add(offset as usize).write(value);
            if value==0 { break; }
            offset=offset.wrapping_add(1);
        }
    }
}
unsafe fn append_string(destination:*mut u8,source:FormatAddress) {
    let mut offset=0usize;
    unsafe {
        while destination.add(offset).read()!=0 { offset+=1; }
        copy_string(destination.add(offset),source);
    }
}

pub(super) unsafe fn emit(profile:Profile,scratch:&mut Scratch) {
    unsafe {
        if xm1_power_record_helper_69fec8(0xe8)&0x400==0 { return; }
        let first_key=if xm1_power_record_helper_74e5c0(profile.0)==1 { 0x7601 } else { 0x75f0 };
        let second_key=if xm1_power_record_helper_74e5c0(profile.0)==1 { 0x7602 } else { 0x75f2 };
        let third_key=if xm1_power_record_helper_74e5c0(profile.0)==1 { 0x7603 } else { 0x75f7 };
        let fourth_key=if xm1_power_record_helper_74e5c0(profile.0)==1 { 0x7600 } else { 0x75fd };
        let fifth_key=if xm1_power_record_helper_74e5c0(profile.0)==1 { 0x75ff } else { 0x75fc };
        scratch.pointer().write(0);
        let selection=word(STATE);
        let label=if (1..=57).contains(&selection) { LABELS[(selection-1) as usize] } else { FormatAddress(0xf52f50) };
        emit_format(label,&[]);
        if half(STATE+4)&1!=0 { copy_string(scratch.pointer(),FormatAddress(0xf53378)); }
        if half(STATE+4)&2!=0 {
            if scratch.pointer().read()==0 { copy_string(scratch.pointer(),FormatAddress(0xf52f38)); }
            else { append_string(scratch.pointer(),FormatAddress(0xf52f38)); }
        }
        if half(STATE+4)==0 { copy_string(scratch.pointer(),FormatAddress(0xf53380)); }
        emit_format(FormatAddress(scratch.pointer() as usize as u32),&[]);
        let (bound,mut magnitude,position)=if word(STATE)==11 {
            (half(STATE+0x2c),half(STATE+0x62),half(STATE+0x60))
        } else { (half(STATE+0x2a),half(STATE+0x5e),half(STATE+0x5c)) };
        let signed=half(STATE+0x4a);
        if signed&0x8000!=0 { put_half(STATE+0x4a,signed.wrapping_neg()); }
        if magnitude&0x8000!=0 { magnitude=magnitude.wrapping_neg(); }
        let first=word(STATE+0x24);
        let second=word(STATE+0x50);
        let third=word(STATE+0x54);
        let fourth=half(STATE+0x28) as i16 as i32 as u32;
        let configured_first=xm1_power_record_helper_69ffe8(first_key).wrapping_mul(10) as u32;
        let configured_second=xm1_power_record_helper_69ffe8(second_key).wrapping_mul(10) as u32;
        let configured_third=xm1_power_record_helper_69ffe8(third_key).wrapping_mul(10) as u32;
        let configured_fourth=xm1_power_record_helper_69ffe8(fourth_key).wrapping_mul(10) as u32;
        let lower=half(STATE+0x30) as i16 as i32 as u32;
        let upper=half(STATE+0x32) as i16 as i32 as u32;
        let configured_fifth=xm1_power_record_helper_69ffe8(fifth_key).wrapping_mul(10) as u32;
        let arguments=[first,second,third,fourth,configured_first,configured_second,configured_third,configured_fourth,
            lower,upper,configured_fifth,half(STATE+0x44) as i16 as i32 as u32,half(STATE+0x48) as i16 as i32 as u32,
            half(STATE+0x4a) as i16 as i32 as u32,bound as i16 as i32 as u32,magnitude as i16 as i32 as u32,
            position as i16 as i32 as u32,word(STATE+0x74),word(STATE+0x8c),word(STATE),half(STATE+4) as u32];
        emit_format(FormatAddress(0xf53390),&arguments);
    }
}
