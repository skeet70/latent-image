use super::{a, c, conv, cpu, lists, cached_mode, frame, get, input, projected, PropertyKey, SettingValue};
use crate::capture_initialization_state::*;

unsafe extern "C" {
    fn xm1_property_get(id: u32, bank: u32, defaults: u32, selection: u32) -> u32;
}

pub(crate) unsafe fn f_6426bc() {
    unsafe {
        write_word(INPUT + 0x368, (u32::from(frame(0xbf54) != 1)));
        write_word(INPUT + 0x36c, (if frame(0xbf54) == 1 { u32::from(c::f_63a244() == 1) } else { 1 }));
        write_word(INPUT + 0x370, (if frame(0xbf54) == 1 { u32::from(frame(0xbfa8) != 1) } else { 1 }));
        write_word(INPUT + 0xd8, (u32::from(frame(0xbfac) == 1)));
        write_word(INPUT + 0x39c, (u32::from(frame(0xbf88) == 1)));
        write_word(INPUT + 0x3d8, (u32::from(frame(0xbf94) != 0)));
        write_word(INPUT + 0x3e0, (u32::from(frame(0xbfa0) == 1)));
        write_half(FRAME + 0xbfb4, conv::f_64268c() as u16);
    }
}

pub(crate) unsafe fn f_6427bc() {
    unsafe {
        f_6426bc();
        write_word(INPUT + 0x138, (1));
        write_word(INPUT + 0xc, (projected(0x1b1)));
        write_word(INPUT + 0x2f0, (projected(0x48)));
        write_word(INPUT + 0x2f4, (projected(0x4b)));
        write_word(INPUT + 0x380, (projected(0x4e)));
        write_word(INPUT + 0x378, (projected(0xad)));
        write_word(INPUT + 0x37c, (projected(0x38)));
        write_word(INPUT + 0x304, conv::f_646798());
        write_word(INPUT + 0x10, (projected(0x1d9)));
        write_word(INPUT + 0x14, conv::f_643c4c(cached_mode()));
        write_word(INPUT + 0x1c, (0));
        write_word(INPUT + 0x18, (0));
        write_word(INPUT + 0x20, u32::from(cpu::flag(0x1c) != 1));
        write_word(INPUT + 0xfc, (0));
        write_word(INPUT + 0x3b8, (0));
        write_word(INPUT + 0x24, (0));
        write_word(INPUT + 0x28, (0));
        write_word(INPUT + 0x2c, (0));
        for bank in 0..2 {
            for group in 0..3 { write_word(INPUT + 0x34 + (bank + group * 2) * 4, 0); }
        }
        write_word(INPUT + 0x4c, (if input(0) == 0x2c { 6 } else { 3 }));
        write_word(INPUT + 0x50, conv::f_644004(0));
        write_word(INPUT + 0x54, (projected(0xf2)));
        write_word(INPUT + 0x320, (projected(0xf3)));
        if frame(0xda98) == 0 {
            write_word(INPUT + 0x58, conv::f_644054());
            write_word(INPUT + 0x330, conv::f_644054());
        } else {
        write_word(INPUT + 0x58, (0));
        }
        write_word(INPUT + 0x5c, conv::f_644080());
        write_word(INPUT + 0x60, conv::f_6440b4());
        write_word(INPUT + 0x64, (projected(0x1d)));
        write_word(INPUT + 0x68, (2));
        write_word(INPUT + 0x308, (2));
        write_word(INPUT + 0x30c, (2));
        write_word(INPUT + 0x318, (0));
        write_word(INPUT + 0x31c, (projected(0x24)));
        write_word(INPUT + 0x34c, (projected(0x16)));
        write_word(INPUT + 0x340, (projected(0x13)));
        write_word(INPUT + 0x344, (projected(0x17)));
        write_word(INPUT + 0x6c, (0));
        write_word(INPUT + 0x314, (0));
        write_word(INPUT + 0x70, (0));
        write_word(INPUT + 0x74, (0));
        write_word(INPUT + 0x78, (0));
        write_word(INPUT + 0x7c, (10));
        write_word(INPUT + 0x33c, (10));
        write_word(INPUT + 0x88, (projected(0x10e)));
        write_word(INPUT + 0x390, (projected(0x17a)));
        write_word(INPUT + 0x394, (projected(0x17b)));
        write_word(INPUT + 0x8c, (projected(0x1c5)));
        write_word(INPUT + 0x90, (projected(0x21)));
        write_word(INPUT + 0x94, (0));
        write_word(INPUT + 0x2ec, conv::f_6469a0());
        write_word(INPUT + 0x2e8, conv::f_646a04());
        write_word(INPUT + 0x388, (projected(0x20f)));
        write_word(INPUT + 0x2f8, conv::f_646a68(input(0xc)));
        write_word(INPUT + 0x2fc, conv::f_646b64());
        write_word(INPUT + 0x300, conv::f_646c9c());
        write_byte(INPUT + 0x98, (0) as u8);
        write_word(INPUT + 0x9c, (0xd));
        write_byte(INPUT + 0xb5, (0) as u8);
        write_byte(INPUT + 0xa9, (1) as u8);
        write_byte(INPUT + 0xaa, (1) as u8);
        write_byte(INPUT + 0xab, (1) as u8);
        write_byte(INPUT + 0xac, (1) as u8);
        write_byte(INPUT + 0xad, (1) as u8);
        write_byte(INPUT + 0xae, (1) as u8);
        write_byte(INPUT + 0xaf, (get(0x280)) as u8);
        write_byte(INPUT + 0xb0, (get(0x27f)) as u8);
        write_byte(INPUT + 0xb6, (get(0x1db).wrapping_sub(3)) as u8);
        write_byte(INPUT + 0xb7, (get(0x1dc).wrapping_sub(3)) as u8);
        write_word(INPUT + 0xb8, (projected(0x1dd)));
        write_byte(INPUT + 0x339, (0) as u8);
        write_byte(INPUT + 0x33a, (0) as u8);
        write_byte(INPUT + 0x338, (0) as u8);
        cpu::f_63b480(if conv::f_61d018() == 1 { 0xa5 } else { 0xa4 });
        write_word(INPUT + 0xc0, (projected(6)));
        if read_word(0x0091_8afc) == 3 { write_word(INPUT + 0xc0, 0x97); }
        write_word(INPUT + 0xc4, (0));
        write_word(INPUT + 0xc8, (projected(if input(0xfc) == 1 { 0x1e2 } else { 0x1d8 })));
        write_word(INPUT + 0x3b0, (0));
        write_word(INPUT + 0x148, (lists::f_646550()));
        let selection = u32::from(frame(0xcfe4) != 3);
        let enabled = u32::from(xm1_property_get(0x1d8, 0, 0, selection) == 7
            || xm1_property_get(0x1d8, 0, 0, selection) == 1);
        write_word(FRAME + 0xcf14, enabled);
        write_word(INPUT + 0xe8, enabled);
        write_word(FRAME + 0xcf14, enabled);
        let value = a::project(PropertyKey(0x1e4), SettingValue(input(0xe8)), 3, 1);
        a::save(PropertyKey(0x1e4), value);
        write_word(INPUT + 0xec, (0x41));
        write_word(INPUT + 0xf0, (0));
        write_byte(INPUT + 0x338, (0) as u8);
        write_word(INPUT + 0xf4, (0));
        write_word(INPUT + 0xf8, (0));
        write_word(INPUT + 0x334, (0));
        write_word(INPUT + 0xfc, (projected(0x1ec)));
        if cpu::flag(0x19) == 1 {
            write_word(INPUT + 0xfc, 1);
            write_word(FRAME + 0xbe04, 1);
        } else if cpu::flag(0x1a) == 1 && cpu::f_6326b8() != 0 {
            write_word(INPUT + 0xfc, 1);
            write_word(FRAME + 0xbe04, 2);
        } else {
            write_word(INPUT + 0xfc, 0);
            write_word(FRAME + 0xbe04, 0);
        }
        if cpu::flag(0x1a) == 1 && cpu::f_6326b8() != 0 {
            let mut pair = [0u32; 2];
            cpu::f_632f84(pair.as_mut_ptr());
            write_word(INPUT + 0x100, pair[0]);
        } else {
            write_word(INPUT + 0x100, projected(0x46));
        }
        write_word(INPUT + 0x108, (c::f_63c274()));
        write_word(INPUT + 0x10c, (projected(0x2b)));
        write_word(INPUT + 0x110, (projected(0x2c)));
        write_word(INPUT + 0x114, conv::f_6461a0());
        write_word(INPUT + 0x118, (0));
        write_word(INPUT + 0x11c, (0));
        write_word(INPUT + 0x120, (projected(0x1c)));
        write_word(INPUT + 0x154, conv::f_64666c());
        write_word(INPUT + 0x158, (projected(0xf)));
        write_word(INPUT + 0x170, (projected(0x12)));
        write_word(INPUT + 0x134, (0));
        c::f_63bc24();
        write_word(INPUT + 0x80, (0));
        write_word(INPUT + 0x35c, (projected(0x100)));
        write_word(INPUT + 0x360, (projected(0x101)));
        write_word(INPUT + 0x364, (projected(0x102)));
        write_word(INPUT + 0x84, (0));
        write_half(INPUT + 0x124, projected(0x1e5) as u16);
        write_word(INPUT + 0x13c, (projected(0x13a)));
        write_word(INPUT + 0x160, conv::f_64681c());
        write_byte(INPUT + 0xa8, (0xd) as u8);
        write_word(INPUT + 0x310, (0));
        write_word(INPUT + 0x164, (2));
        write_word(INPUT + 0x2e0, conv::f_6466d0());
        write_word(INPUT + 0x2e4, conv::f_646734());
        write_word(INPUT + 0x168, (projected(0x10)));
        write_word(INPUT + 0x16c, (projected(0x11)));
        write_word(INPUT + 0xa4, (projected(0xd3)));
        write_word(INPUT + 0x324, (0x21));
        write_word(INPUT + 0x348, (projected(0xaf)));
        write_word(INPUT + 0x384, (1));
        write_word(FRAME + 0xbee0, c::f_63a4f4());
        write_word(FRAME + 0xbee8, cpu::flag(0x425));
        write_half(FRAME + 0xbfb6, 0);
        write_word(INPUT + 0x398, (0));
        write_word(INPUT + 0xd4, (u32::from(get(0x50) == 0)));
        if get(0x1d4) != c::f_63b96c() { a::save(PropertyKey(0x1d1), SettingValue(0x1aff_ffff)); }
        if get(0x1d3) != u32::from(c::f_63b8bc(cached_mode()).0) {
            a::save(PropertyKey(0x1d0), SettingValue(0x1aff_ffff));
        }
        write_word(INPUT + 0x3e4, (0));
    }
}
