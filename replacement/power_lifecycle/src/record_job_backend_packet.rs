//! X-M1 1.50 job packet layout, recovered from f692dc..f69a4c.

#[derive(Clone, Copy)]
pub(super) struct Address(pub u32);
impl Address {
    pub(super) fn at(self, offset: u32) -> Self { Self(self.0.wrapping_add(offset)) }
    pub(super) unsafe fn word(self, offset: u32) -> u32 { unsafe { core::ptr::read_volatile(self.at(offset).0 as *const u32) } }
    pub(super) unsafe fn half(self, offset: u32) -> u16 { unsafe { core::ptr::read_volatile(self.at(offset).0 as *const u16) } }
    pub(super) unsafe fn signed(self, offset: u32) -> i32 { unsafe { self.half(offset) as i16 as i32 } }
    pub(super) unsafe fn byte(self, offset: u32) -> u8 { unsafe { core::ptr::read_volatile(self.at(offset).0 as *const u8) } }
    pub(super) unsafe fn put(self, offset: u32, value: u32) { unsafe { core::ptr::write_volatile(self.at(offset).0 as *mut u32, value) } }
    pub(super) unsafe fn put_half(self, offset: u32, value: i32) { unsafe { core::ptr::write_volatile(self.at(offset).0 as *mut u16, value as u16) } }
    pub(super) unsafe fn put_byte(self, offset: u32, value: u8) { unsafe { core::ptr::write_volatile(self.at(offset).0 as *mut u8, value) } }
    pub(super) unsafe fn clear(self, size: u32) { for offset in 0..size { unsafe { self.put_byte(offset, 0) }; } }
    pub(super) unsafe fn copy(self, source: Self, size: u32) { for offset in 0..size { unsafe { self.put_byte(offset, source.byte(offset)) }; } }
}

unsafe extern "C" {
    fn xm1_power_record_helper_ef6790(a:i32,b:u32,c:u32,d:u32,e:u32,f:u32)->u32;
    fn xm1_power_record_helper_69f8fc(key:u32)->u32;
    fn xm1_power_record_helper_772f28(kind:u32)->i32;
    fn xm1_power_record_helper_f5e72c(packet:u32,count:u32,output:u32)->u32;
    fn xm1_power_record_helper_f5ea70(packet:u32,output:u32);
    fn xm1_power_record_helper_746c2c(item:u32)->i32;
    fn xm1_power_record_helper_746bf8(value:u32,item:u32)->i32;
    fn xm1_power_record_helper_69fffc(key:u32)->i32;
    fn xm1_power_record_helper_69ffe8(key:u32)->i32;
    fn xm1_power_record_helper_756734(item:u32,output:u32);
}

#[repr(C, align(4))]
struct JobPacket([u8; 104]);
impl JobPacket {
    fn address(&mut self) -> Address { Address(self.0.as_mut_ptr() as usize as u32) }
}

unsafe fn populate(dst: Address, src: Address, value:u32, mode:u32, halves:[i32;5], reference:Address) {
    unsafe {
        for (target, source) in [(0,0),(4,4),(12,76),(16,12),(20,40),(32,88),(36,104),(40,92),(44,36),(52,60),(84,72)] { dst.put(target,src.word(source)); }
        for (target, source) in [(8,8),(48,64),(50,66),(56,56),(100,68)] { dst.put_half(target,src.half(source) as i32); }
        dst.put_half(24,reference.half(2) as i32);
        dst.put(28,0);
        dst.put(60,xm1_power_record_helper_ef6790(src.signed(66),src.word(4),src.half(8) as u32,src.word(12),src.byte(16) as u32,src.half(18) as u32));
        dst.put(64,value); dst.put(68,mode);
        for (offset, half) in [72,74,76,92,94].into_iter().zip(halves) { dst.put_half(offset,half); }
        dst.put_byte(78,0); dst.put_half(80,0); dst.put(88,4);
        let enabled=xm1_power_record_helper_69f8fc(0x5256)&3 != 0 && src.word(96)!=0;
        dst.put(96,enabled as u32);
    }
}

unsafe fn adjust(packet:Address, reference:Address, extension:i32, force:bool) {
    unsafe {
        if reference.word(12)==1 {
            packet.put_byte(78,2);
            if extension!=0 { packet.put_half(24,extension+reference.half(2) as i32); }
            else if force || !matches!(packet.word(4),0x2d|0x37) {
                packet.put_half(72,packet.half(72) as i32-reference.half(2) as i32+xm1_power_record_helper_772f28(3));
                packet.put_half(24,xm1_power_record_helper_772f28(3));
                packet.put_half(80,xm1_power_record_helper_772f28(3)-reference.half(2) as i32);
            }
        } else { packet.put_half(24,reference.half(4) as i32); }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f692dc(out:u32,src:u32,value:u32,mode:u32,a:i32,b:i32,c:i32,d:i32,e:i32,reference:u32) {
    unsafe { populate(Address(out),Address(src),value,mode,[a,b,c,d,e],Address(reference)); }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f69418(reference:u32,value:u32,src:u32,a:i32,b:i32,c:i32,d:i32,e:i32,tail:u32,extension:i32,out:u32)->u32 {
    let mut packet=JobPacket([0;104]); let p=packet.address();
    unsafe { populate(p,Address(src),value,1,[a,b,c,d,e],Address(reference)); p.put(88,tail); p.put_half(80,extension); adjust(p,Address(reference),extension,false); (xm1_power_record_helper_f5e72c(p.0,1,out)>>3)&1 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f69514(out:u32,reference:u32,rows:u32,mode:u32,value:u32,src:u32,a:i32,b:i32,c:i32,d:i32,e:i32,tail:u32,extension:i32) {
    unsafe {
        let rows=Address(rows); let out=Address(out); let source=Address(src); let reference=Address(reference);
        let per_row=source.word(12)==5 && rows.word(4)!=0;
        if per_row {
            for row in 0..rows.word(0) {
                let target=out.at(row*16);
                if Address(rows.word(4)).word(row*4)==0 { target.put_byte(0,0); }
                else {
                    let mut packet=JobPacket([0;104]); let p=packet.address();
                    populate(p,source,value,mode,[a,b,c,d,e],reference); p.put(88,tail);p.put_half(80,extension);adjust(p,reference,extension,true);
                    xm1_power_record_helper_f5ea70(p.0,target.0);
                }
            }
        } else {
            let mut packet=JobPacket([0;104]); let p=packet.address();
            populate(p,source,value,mode,[a,b,c,d,e],reference);p.put(88,tail);p.put_half(80,extension);adjust(p,reference,extension,false);
            xm1_power_record_helper_f5ea70(p.0,out.0);
            for row in 1..rows.word(0) { out.at(row*16).copy(out,16); }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f69788(item:u32,packet:u32,a:i32,b:i32,out:u32) {
    unsafe {
        let source=Address(packet);let out=Address(out);
        let (status,x,y,upper,lower)=if out.byte(0)==0 { (0,0,0,0,0) } else {
            let timing=xm1_power_record_helper_772f28(3);
            let x=(a-source.half(68) as i32-b+timing+xm1_power_record_helper_746c2c(item)) as i16 as i32;
            let y=(source.half(72) as i32+source.half(0) as i32-source.half(10) as i32-xm1_power_record_helper_746bf8(source.word(4),item)) as i16 as i32;
            let upper=(xm1_power_record_helper_69fffc(0x6391)*10) as i16 as i32;
            let lower=(xm1_power_record_helper_69ffe8(0x6390)*10) as i16 as i32;
            (if x>upper {2} else if y>lower {3} else {1},x,y,upper,lower)
        };
        out.put(4,status);for (offset,value) in [(8,x),(10,y),(12,upper),(14,lower)] {out.put_half(offset,value);}
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f69a4c(context:u32,expected:u32,item:u32) {
    unsafe {
        let c=Address(context);xm1_power_record_helper_756734(item,c.at(12).0);
        let matched=c.word(12)==expected;c.put(0,if matched {c.word(16)} else {0});c.put(4,if matched {c.word(108)} else {0});
        if c.word(0)!=0 {
            let adjustment=c.half(30) as i32+c.word(28) as i32+c.half(26) as i32;
            c.put_half(8,c.word(32) as i32+c.word(24) as i32-c.word(28) as i32-c.half(26) as i32-c.half(34) as i32-c.half(30) as i32+xm1_power_record_helper_746bf8(c.word(20),c.word(12)));
            let table=Address(0x5ffc0340);for index in 0..256 {table.put_half(index*2,table.half(index*2) as i32-adjustment);}
        }
    }
}
