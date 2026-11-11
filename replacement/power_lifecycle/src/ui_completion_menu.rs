use super::{MenuMode, MenuResume, ResumeMenuContext, UiCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyId(pub u16);
pub trait Properties { fn read(&mut self, property: PropertyId) -> u32; }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionContext { pub category: MenuMode, pub side: MenuMode, pub bank: MenuMode, pub scene: MenuMode, pub scene_submode: MenuMode, pub bf88: MenuMode }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Field { F990, F994, F998, F99c, F9a0, F9a4, F9a8, F9ac, F9b0, F9b4, F9b8, F9bc, F9c0, F9c4, F9c8, F9cc, F9d0, F9d4, F9d8, F9dc, F9e0, F9e4, F9e8, F9ec, F9f0, F9f4, F9f8, F9fc, Fa00, Fa04 }
pub struct SelectionFlags(pub [u32; 30]);
impl SelectionFlags {
    fn set(&mut self, fields: &[Field], value: u32) { for field in fields { self.0[*field as usize] = value; } }
    fn property(&mut self, field: Field, property: u16, properties: &mut impl Properties) { self.0[field as usize] = u32::from(properties.read(PropertyId(property)) != 1); }
    fn base(&mut self, value: u32) { self.0[..16].fill(value); }
    pub fn select(&mut self, code: u8, context: SelectionContext, properties: &mut impl Properties) {
        use Field::*;
        self.set(&[F9d4,F9d8,F9dc,F9e4,F9e8,F9ec,F9f0,F9f4,F9f8,F9fc,Fa00,Fa04], 1);
        self.set(&[F9d0], 0);
        let bank = context.bank.0;
        let side = context.side.0;
        let category = context.category.0;
        let scene = context.scene.0;
        if matches!(code, 0x5b | 0x5c | 0x5f) {
            self.base(0);
            self.set(&[F9a0,F9b8,F9c8,F9a8,F99c,F9a4,F9c0], 1);
            if bank == 8 {
                for (field, id) in [(F994,0x18a),(F9a0,0x18c),(F9b8,0x18f),(F9c8,0x192),(F9a8,0x194),(F99c,0x18e)] { self.property(field,id,properties); }
            }
            return;
        }
        if code == 0x63 {
            let previous_99c = self.0[F99c as usize];
            self.base(0);
            self.set(&[F9b8,F9a8,F9a0,F9c0,F9c8], 1);
            if bank == 8 {
                for (field,id) in [(F994,0x18a),(F9a8,0x194),(F9a0,0x18c),(F9c8,0x192),(F9b8,0x18f)] { self.property(field,id,properties); }
                if side == 0 { self.property(F99c,0x18e,properties); } else { self.0[F99c as usize] = previous_99c; }
                self.set(&[F9a4],1);
            } else { self.set(&[F99c],u32::from(context.bf88.0 != 1)); }
            return;
        }
        if bank == 5 {
            self.base(1);
            self.set(&[F990,F994,F9ac,F998,F99c],0);
            if matches!(code,12|13) { self.base(0); self.set(&[F9d4,F9d8,F9dc],0); }
            else if matches!(code,0x13|0x14|0x1f) { self.set(&[F9b0,F9b4,F9a0,F9b8,F9bc,F9c0,F9c4,F9c8,F9cc,F9a8,F9d8,F9dc],0); }
        } else if matches!(bank,0|7|9|1|2) {
            self.base(1);
            self.set(&[F990,F994,F9ac],0);
            if side != 0 { self.set(&[F998],0); } else { self.set(&[F99c],u32::from(context.bf88.0 != 1)); }
        } else if bank == 8 && scene != 1 {
            if category <= 1 {
                for (field,id) in [(F9b0,0x186),(F9b4,0x187),(F990,0x188),(F9a4,0x189),(F994,0x18a),(F9ac,0x18b),(F9a0,0x18c)] { self.property(field,id,properties); }
                if side != 0 { self.property(F998,0x18d,properties); }
                for (field,id) in [(F9b8,0x18f),(F9c4,0x191),(F9c8,0x192),(F9cc,0x193)] { self.property(field,id,properties); }
                if scene == 0x24 || ((31..=32).contains(&scene) && context.scene_submode.0 == 3) { self.set(&[F9a8],1); }
                else { self.property(F9a8,0x194,properties); }
                if matches!(scene,2|3|4|0x1f|0x21|0x22|0x23|0x24|0x25) {
                    self.property(F9bc,0x190,properties);
                    self.0[F9c0 as usize] = self.0[F9bc as usize];
                } else { self.set(&[F9bc],0); self.set(&[F9c0],1); }
                if scene <= 29 && (1u32 << scene) & 0x27000000 != 0 { if side == 0 { self.set(&[F99c],0); } }
                else if side == 0 { self.property(F99c,0x18e,properties); }
            } else {
                for (field,id) in [(F9b0,0x1a1),(F9b4,0x1a2),(F990,0x1a3),(F9a4,0x1a4),(F994,0x1a5),(F9ac,0x1a6),(F9a0,0x1a7)] { self.property(field,id,properties); }
                if side != 0 { self.property(F998,0x1a8,properties); } else { self.property(F99c,0x1a9,properties); }
                for (field,id) in [(F9b8,0x1aa),(F9bc,0x1ab),(F9c4,0x1ac),(F9c8,0x1ad),(F9cc,0x1ae)] { self.property(field,id,properties); }
                if scene == 0x24 { self.set(&[F9a8],1); } else { self.property(F9a8,0x1b0,properties); }
            }
        } else { self.base(1); }
        if (0x28..=0x2a).contains(&code) {
            self.base(0);
            self.set(&[F9a4,F9d4],u32::from(category == 2));
        } else if code == 0x2b {
            self.set(&[F9b0,F9b4,F990,F994,F9ac,F9a0,F9c4,F9c8,F9cc,F9a8],0);
            self.set(&[F9a4,F9b8,F9bc,F9c0,F9d4],1);
            if side != 0 {
                if category == 1 { if properties.read(PropertyId(0x1e2)) == 1 { self.property(F998,0x18d,properties); } else { self.set(&[F998],0); } }
            } else { self.set(&[F99c],u32::from(context.bf88.0 != 1)); }
        } else if matches!(code,12|13) && bank != 5 && category != 2 { self.base(0); }
        else if code == 0x1d {
            self.set(&[F9a4,F9d4,F9b8],0);
            if bank != 1 {
                self.set(&[F9bc,F9c0],0);
                if bank == 5 { self.base(1); self.set(&[F990,F994,F998,F99c,F9a4],0); }
            }
        } else if code == 0x2e { self.base(0); self.set(&[F9a4,F9a8],1); }
        else if matches!(code,1|2) && scene == 0x1d {
            self.base(0); self.set(&[F9b0,F9b4,F9a4,F9a0,F9b8,F9c8,F9cc,F9a8],1);
        } else if matches!(code,0x1f|0x27|4) && scene == 0x1d { self.base(0); self.set(&[F9a4,F9b8,F9a8],1); }
        else if (0x15..=0x17).contains(&code) { self.base(0); self.set(&[F9a8],1); }
    }
}

#[cfg(target_arch = "arm")]
pub struct NativeMenuResume<P> { pub properties: P }
#[cfg(target_arch = "arm")]
impl<P: Properties> MenuResume for NativeMenuResume<P> {
    fn context(&self) -> ResumeMenuContext { unsafe {
        use core::ptr::read_volatile as read;
        let scene = read(0x93ade4 as *const u32);
        let settings = read(0x940de4 as *const u32);
        ResumeMenuContext { current: UiCode(read(0x15e46e8 as *const u16)), mode: MenuMode(read((scene + 0x13c) as *const u32)), alternative: read(0x15e4bec as *const u8) == 1, selection: MenuMode(read((settings + 0xbb94) as *const u32)) }
    } }
    fn select_and_snapshot(&mut self, code: u8) { unsafe {
        use core::ptr::{read_volatile as read, write_volatile as write};
        let scene = read(0x93ade4 as *const u32);
        let settings = read(0x940de4 as *const u32);
        let context = SelectionContext { category: MenuMode(read((scene + 0xfc) as *const u32)), side: MenuMode(read((scene + 0x4c) as *const u32)), bank: MenuMode(read((scene + 0xc8) as *const u32)), scene: MenuMode(read(scene as *const u32)), scene_submode: MenuMode(read((scene + 4) as *const u32)), bf88: MenuMode(read((settings + 0xbf88) as *const u32)) };
        let mut flags = SelectionFlags(core::array::from_fn(|index| read((0x15e5054 + index * 4) as *const u32)));
        flags.select(code, context, &mut self.properties);
        for (index,value) in flags.0.iter().enumerate() { write((0x15e5054 + index * 4) as *mut u32,*value); }
        snapshot();
    } }
}

#[cfg(target_arch = "arm")]
unsafe fn snapshot() { unsafe {
    use core::ptr::{read_volatile as read, write_volatile as write};
    let settings = read(0x940de4 as *const u32);
    if read((settings + 0xbb6c) as *const u32) != 3 { return; }
    let active = read(0x15da918 as *const u32);
    let candidate = active ^ 1;
    let occupied = read((0x15e5174 + candidate * 336) as *const u32);
    let selected = if occupied != 0 { active } else { candidate };
    let destination = 0x15e5174 + selected * 336;
    let source = read(0x972f38 as *const u32);
    write((destination + 4) as *mut u16, read((source + 0x126) as *const u16));
    write((destination + 8) as *mut u32, read((source + 0x1d8) as *const u32));
    write((destination + 12) as *mut u32, read((source + 0x1dc) as *const u32));
    for index in 0..10 {
        let source = read(0x972f38 as *const u32) + index * 184;
        let output = destination + index * 8;
        for (to,from) in [(0x10,0x208),(0x14,0x20c),(0x60,0x26c),(0x64,0x254)] { write((output + to) as *mut u32,read((source + from) as *const u32)); }
        core::ptr::copy_nonoverlapping((source + 0x1e8) as *const u8,(destination + 0xb0 + index * 16) as *mut u8,16);
    }
    if occupied == 0 { write(0x15da918 as *mut u32,selected); }
} }

#[cfg(target_arch = "arm")]
pub struct OwnedProperties;
#[cfg(target_arch = "arm")]
impl Properties for OwnedProperties {
    fn read(&mut self, property: PropertyId) -> u32 {
        extern "C" { fn xm1_public_get(property: u32, bank: u32, default_mode: u32, selection: u32) -> u32; }
        let selection = u32::from(unsafe { core::ptr::read_volatile(0x951564 as *const u32) } != 3);
        unsafe { xm1_public_get(u32::from(property.0), 0, 0, selection) }
    }
}
