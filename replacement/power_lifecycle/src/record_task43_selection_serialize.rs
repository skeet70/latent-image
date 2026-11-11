//! Ordered X-M1 profile publication at 0xf29318..0xf2a11c.
//! Source pin: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, image-5 bytes.

use core::ptr::{read_volatile, write_volatile};

unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn byte(address: u32) -> u32 { unsafe { read_volatile(address as *const u8) as u32 } }
unsafe fn half(address: u32) -> u32 { unsafe { read_volatile(address as *const u16) as u32 } }
unsafe fn signed_half(address: u32) -> u32 { unsafe { read_volatile(address as *const i16) as i32 as u32 } }
unsafe fn put(output: u32, field: u32, value: u32) { unsafe { write_volatile(output.wrapping_add(field * 4) as *mut u32, value) } }

macro_rules! getters {
    ($($name:ident),* $(,)?) => { unsafe extern "C" { $(fn $name(item: u32) -> u32;)* } };
}
getters!(xm1_power_record_helper_74e648, xm1_power_record_helper_74e718, xm1_power_record_helper_7503ec,
    xm1_power_record_helper_74ea10, xm1_power_record_helper_75516c, xm1_power_record_helper_75502c,
    xm1_power_record_helper_754d9c, xm1_power_record_helper_74ec30, xm1_power_record_helper_74f894,
    xm1_power_record_helper_750034, xm1_power_record_helper_74f1ac, xm1_power_record_helper_74f454,
    xm1_power_record_helper_74f564, xm1_power_record_helper_74f674, xm1_power_record_helper_74f6fc,
    xm1_power_record_helper_74f784, xm1_power_record_helper_74f5ec, xm1_power_record_helper_74fcec,
    xm1_power_record_helper_74edf4, xm1_power_record_helper_74ed6c, xm1_power_record_helper_74fa2c,
    xm1_power_record_helper_74f9a4, xm1_power_record_helper_74e7f0, xm1_power_record_helper_74e878,
    xm1_power_record_helper_74e900, xm1_power_record_helper_752714, xm1_power_record_helper_74ef04,
    xm1_power_record_helper_74ee7c, xm1_power_record_helper_7500bc, xm1_power_record_helper_74eb20,
    xm1_power_record_helper_74f124, xm1_power_record_helper_74f09c, xm1_power_record_helper_75060c,
    xm1_power_record_helper_74eba8, xm1_power_record_helper_7527f8, xm1_power_record_helper_752890);
unsafe extern "C" {
    fn xm1_power_record_helper_69f8fc(key: u32) -> u32;
    fn xm1_power_record_helper_69fec8(key: u32) -> u32;
    fn xm1_power_record_helper_7582c0(output: *mut u32, item: u32);
    fn xm1_power_record_helper_753540(item: u32, index: u32) -> u32;
    fn xm1_power_record_helper_752a40(item: u32, index: u32) -> u32;
    fn xm1_power_record_helper_754300(item: u32, index: u32) -> u32;
    fn xm1_power_record_helper_7543b0(item: u32, index: u32) -> u32;
    fn xm1_power_record_helper_f160d0() -> i32;
    fn xm1_power_record_helper_f160f4() -> i32;
    fn xm1_power_record_helper_f16118() -> i32;
    fn xm1_power_record_helper_f1613c() -> i32;
    fn xm1_power_record_helper_f161a8() -> i32;
    fn xm1_power_record_helper_f161cc() -> i32;
    fn xm1_power_record_helper_f51b50(item: u32, value: u32) -> u32;
    fn xm1_power_record_helper_f51b94(item: u32, value: u32) -> u32;
    fn xm1_power_record_helper_1004438(output: *mut u32);
    fn xm1_power_record_helper_746054() -> u32;
    fn xm1_power_record_helper_752928(output: *mut u32, item: u32, index: u32);
    fn xm1_power_record_helper_dca798(output: *mut u32, value: u32, layout: *const u32);
    fn xm1_power_record_helper_74947c(value: u32, first: u32, second: u32, a: u32, b: u32, c: u32, enabled: u32) -> u32;
    fn xm1_power_record_helper_ee96e0(output: u32);
    fn xm1_power_record_helper_ee96f4(output: u32);
    fn xm1_power_record_helper_f2d6dc() -> u32;
    fn xm1_power_record_helper_748928(output: u32, value: u32);
    fn xm1_power_record_helper_1005044() -> u32;
}

macro_rules! fields {
    ($output:expr, $item:expr; $($index:expr => $getter:ident),* $(,)?) => {
        $(unsafe { put($output, $index, $getter($item)); })*
    };
}

unsafe fn table(output: u32, field: u32, source: u32, count: u32, stride: u32) {
    for index in 0..count { unsafe { put(output, field + index, byte(source + stride * index)); } }
}
unsafe fn bounded_count(key: u32) -> u32 { unsafe { xm1_power_record_helper_69f8fc(key).clamp(1, 20) } }

unsafe fn curve_header(output: u32, field: u32, source: u32, mode_offset: u32) {
    unsafe {
        put(output, field, word(source));
        put(output, field + 1, half(source + 4));
        for index in 0..5 { put(output, field + 2 + index, byte(source + 6 + index)); }
        put(output, field + 7, signed_half(source + 28));
        put(output, field + 8, word(source + mode_offset));
        let value = signed_half(source + 48);
        put(output, field + 9, value);
        put(output, field + 10, signed_half(source + 52));
        put(output, field + 11, signed_half(source + 54));
        let adjusted = xm1_power_record_helper_74947c(value, word(source + 60), word(source + 64), 0, 0, 0, 1);
        put(output, field + 12, adjusted.wrapping_sub(word(output + (field + 9) * 4) as i16 as i32 as u32));
        put(output, field + 13, 0);
        put(output, field + 14, word(source + 60).wrapping_add(1));
        for (index, offset) in [22, 24, 72, 74, 76, 78].into_iter().enumerate() { put(output, field + 15 + index as u32, signed_half(source + offset)); }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f29318() {
    unsafe {
        let request = 0x169d09c;
        let feature = xm1_power_record_helper_69f8fc(0xa8);
        if feature & 128 == 0 || word(request + 8) == 3 { return; }
        let output = word(word(request + 12) + 80);
        let item = word(0x16b7510);
        put(output, 0, 0x5354434d);
        let first = xm1_power_record_helper_74e648(item);
        let mut flags = xm1_power_record_helper_74e718(item);
        let mut profile = core::mem::MaybeUninit::<[u32; 10]>::uninit();
        xm1_power_record_helper_7582c0(profile.as_mut_ptr().cast(), item);
        if xm1_power_record_helper_7503ec(item) != 0 {
            flags = if profile.as_ptr().cast::<u32>().add(4).read() == 1 { (flags | 1) & 0xffff } else { flags & 0xfffe };
        }
        put(output, 1, first); put(output, 2, flags);
        fields!(output,item; 3=>xm1_power_record_helper_74ea10,4=>xm1_power_record_helper_75516c,5=>xm1_power_record_helper_75502c,6=>xm1_power_record_helper_754d9c);
        let pair = xm1_power_record_helper_74ec30(item);
        put(output,7,pair & 0xffff); put(output,8,pair >> 16);
        fields!(output,item;
            0xb=>xm1_power_record_helper_74f894,0xc=>xm1_power_record_helper_750034,0xd=>xm1_power_record_helper_74f1ac,
            0xe=>xm1_power_record_helper_74f454,0xf=>xm1_power_record_helper_74f564,0x10=>xm1_power_record_helper_74f674,
            0x11=>xm1_power_record_helper_74f6fc,0x12=>xm1_power_record_helper_74f784,0x13=>xm1_power_record_helper_74f5ec,
            0x14=>xm1_power_record_helper_74fcec,0x15=>xm1_power_record_helper_74edf4,0x16=>xm1_power_record_helper_74ed6c,
            0x17=>xm1_power_record_helper_74fa2c,0x18=>xm1_power_record_helper_74f9a4,0x19=>xm1_power_record_helper_74e7f0,
            0x1a=>xm1_power_record_helper_74e878,0x1b=>xm1_power_record_helper_74e900);
        put(output,0x1c,xm1_power_record_helper_753540(item,0)); put(output,0x1d,0);
        put(output,0x1e,xm1_power_record_helper_752a40(item,0)); put(output,0x1f,signed_half(word(request+56)+8));
        put(output,0x20,xm1_power_record_helper_f160d0() as u32); put(output,0x21,xm1_power_record_helper_f160f4() as u32);
        put(output,0x22,xm1_power_record_helper_f16118() as u32); put(output,0x23,xm1_power_record_helper_f1613c() as u32);
        put(output,0x24,xm1_power_record_helper_754300(item,0));
        put(output,0x28,xm1_power_record_helper_f161a8() as u32); put(output,0x29,xm1_power_record_helper_f161cc() as u32);
        put(output,0x2a,xm1_power_record_helper_7543b0(item,0)); put(output,0x25,xm1_power_record_helper_752714(item));
        put(output,0x26,xm1_power_record_helper_f51b50(item,word(output+0x84)));
        put(output,0x27,xm1_power_record_helper_f51b94(item,word(output+0x90)));
        for (field,key) in [(0x2b,0x204),(0x2c,0x1e8)] {
            let high=xm1_power_record_helper_69f8fc(key); let low=xm1_power_record_helper_69f8fc(key+1);
            put(output,field,low | high<<8);
        }
        let high=xm1_power_record_helper_69f8fc(0x1ea); let low=xm1_power_record_helper_69f8fc(0x1eb);
        put(output,0x2d,low.wrapping_add(high<<16));
        for (field,key) in [(0x2e,0x1ec),(0x2f,0x1f0),(0x30,0x1f4),(0x31,0x1f8)] {
            let a=xm1_power_record_helper_69f8fc(key); let b=xm1_power_record_helper_69f8fc(key+1);
            let c=xm1_power_record_helper_69f8fc(key+2); let d=xm1_power_record_helper_69f8fc(key+3);
            put(output,field,b<<16 | a<<24 | d | c<<8);
        }
        put(output,0x32,0);
        let mut coordinates=core::mem::MaybeUninit::<[u32;4]>::uninit();
        xm1_power_record_helper_1004438(coordinates.as_mut_ptr().cast());
        for index in 0..4 { put(output,0x33+index,coordinates.as_ptr().cast::<u32>().add(index as usize).read()); }
        fields!(output,item;0x37=>xm1_power_record_helper_74ef04,0x38=>xm1_power_record_helper_74ee7c,
            0x3d=>xm1_power_record_helper_7500bc,0x3e=>xm1_power_record_helper_74eb20,0x3f=>xm1_power_record_helper_74f124,
            0x40=>xm1_power_record_helper_74f09c,0x42=>xm1_power_record_helper_75060c);
        put(output,0x43,xm1_power_record_helper_746054());
        let mut mode=xm1_power_record_helper_74eba8(item);
        let mut layout=[0u32;4];
        layout[1]=word(output+0x9c); layout[2]=xm1_power_record_helper_7527f8(item); layout[3]=xm1_power_record_helper_752890(item);
        layout[0]=xm1_power_record_helper_74ec30(item);
        if mode==0 {
            let mut record=core::mem::MaybeUninit::<[u32;42]>::uninit();
            xm1_power_record_helper_752928(record.as_mut_ptr().cast(),item,0);
            mode=record.as_ptr().cast::<u32>().add(40).read();
        }
        let mut converted=core::mem::MaybeUninit::<[u32;2]>::uninit();
        xm1_power_record_helper_dca798(converted.as_mut_ptr().cast(),mode,layout.as_ptr());
        put(output,9,converted.as_ptr().cast::<u32>().read()); put(output,10,converted.as_ptr().cast::<u32>().add(1).read());
        put(output,0x41,xm1_power_record_helper_74eba8(item)); put(output,0x44,0x4c41434d); put(output,0x45,0x53545352);
        put(output,0x46,xm1_power_record_helper_7503ec(item)); put(output,0x47,xm1_power_record_helper_69f8fc(0x75b0));
        put(output,0x48,xm1_power_record_helper_69f8fc(0x75f9));
        let primary=0x16cbea8;
        curve_header(output,0x49,primary,40);
        let secondary=0x16cc820;
        curve_header(output,0x5e,secondary,44);
        put(output,0x73,word(secondary+68)); put(output,0x74,word(primary+128)); put(output,0x75,half(primary+132));
        put(output,0x76,bounded_count(0x75b1)); put(output,0x77,20);
        table(output,0x78,0x16c59e4,20,1);
        put(output,0x8c,word(primary+144)); put(output,0x8d,xm1_power_record_helper_69f8fc(0x75b2));
        put(output,0x8e,xm1_power_record_helper_69f8fc(0x75b5)); put(output,0x8f,word(primary+136)); put(output,0x90,half(primary+140));
        put(output,0x91,bounded_count(0x75b1)); put(output,0x92,20);
        table(output,0x93,0x16c2c40,20,0x214);
        put(output,0xa7,xm1_power_record_helper_69f8fc(0x75b4)); put(output,0xa8,xm1_power_record_helper_69f8fc(0x75b3)); put(output,0xa9,20);
        table(output,0xaa,0x16c5e0c,20,1);
        table(output,0xbe,primary+168,6,1); table(output,0xc4,primary+174,6,1);
        for index in 0..6 { put(output,0xca+index,word(primary+180+index*4)); }
        put(output,0xd0,byte(primary+204)); put(output,0xd1,xm1_power_record_helper_69f8fc(0x75c2)); put(output,0xd2,word(primary+208));
        put(output,0xd3,xm1_power_record_helper_69f8fc(0x75c3)); put(output,0xd4,xm1_power_record_helper_69f8fc(0x75c4).wrapping_mul(10));
        put(output,0xd5,signed_half(primary+0x4ec));
        for index in 0..256 { put(output,0xd6+index,signed_half(primary+214+index*2)); }
        let curves=0x16c5e20;
        for curve in 0..5 { for index in 0..256 { put(output,0x1d6+curve*256+index,signed_half(curves+curve*0x450+0x218+index*2)); } }
        put(output,0x6d6,0); put(output,0x6d7,half(primary+0x4f4)); put(output,0x6d8,signed_half(primary+0x2d6));
        for index in 0..5 { put(output,0x6d9+index,signed_half(curves+index*0x450+0x41e)); }
        put(output,0x6de,word(primary+0x2d8));
        for index in 0..5 { put(output,0x6df+index,word(curves+index*0x450+0x424)); }
        put(output,0x6e4,word(primary+0x2e0));
        for index in 0..5 { put(output,0x6e5+index,word(curves+index*0x450+0x428)); }
        let records=0x16c2970;
        for index in 0..20 { put(output,0x6ea+index,word(records+index*36+4)); }
        table(output,0x6fe,records+8,20,36);
        put(output,0x712,word(primary+0x2e4));
        for index in 0..20 { put(output,0x713+index,word(records+index*36)); }
        put(output,0x727,xm1_power_record_helper_69f8fc(0x76de)); put(output,0x728,xm1_power_record_helper_69f8fc(0x18ba8));
        for index in 0..3 { put(output,0x729+index,word(primary+0x50c+index*4)); }
        put(output,0x72c,half(primary+0x518)); put(output,0x72d,word(primary+0x51c)); put(output,0x72e,xm1_power_record_helper_69fec8(0x18ba9));
        put(output,0x72f,word(secondary+128)); put(output,0x730,half(secondary+132)); put(output,0x731,xm1_power_record_helper_69f8fc(0x75e0));
        put(output,0x732,bounded_count(0x75df)); put(output,0x733,20); table(output,0x734,0x16c73b0,20,1);
        let mapping=0x16cc8a8; let extra=0x16cc904;
        for index in 0..3 { put(output,0x748+index,word(mapping+index*4)); }
        put(output,0x74b,word(primary+164)); put(output,0x74c,word(mapping+12)); table(output,0x74d,mapping+16,20,1);
        for index in 0..4 { put(output,0x761+index,word(mapping+36+index*4)); }
        put(output,0x765,half(mapping+52));
        for index in 0..7 { put(output,0x768+index,word(mapping+56+index*4)); }
        put(output,0x766,word(mapping+84)); put(output,0x767,half(mapping+88));
        for index in 0..13 { put(output,0x76f+index,word(extra+index*4)); }
        put(output,0x77c,half(extra+52)); put(output,0x77d,word(extra+56)); put(output,0x77e,half(extra+60)); put(output,0x77f,0x4c415352);
        xm1_power_record_helper_ee96e0(output); xm1_power_record_helper_ee96f4(output);
        if xm1_power_record_helper_74f1ac(item)!=6 {
            put(output,0x281d3,0x53544344); put(output,0x2959f,xm1_power_record_helper_f2d6dc()); put(output,0x296b7,0x4c414344);
        }
        xm1_power_record_helper_748928(output,word(output+0x90)); put(output,0x2c6bf,0x53545447);
        let source=xm1_power_record_helper_1005044();
        for index in 0..0x51e0 { write_volatile((output+0xb1b00+index) as *mut u8,byte(source+index) as u8); }
        put(output,0x2db38,0x4c415447);
    }
}
