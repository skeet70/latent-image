//! X-M1 1.50 bytes f300d0..f308d4, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use core::ptr::{read_volatile, write_volatile};
#[repr(transparent)]
#[derive(Clone, Copy)]
struct ProfileHandle(u32);
#[repr(C, align(4))]
struct Snapshot<const N: usize> { bytes: [u8; N] }
impl<const N: usize> Snapshot<N> {
    fn new() -> Self { Self { bytes: [0; N] } }
    unsafe fn word(&mut self, offset: usize, value: u32) { unsafe { write_volatile(self.bytes.as_mut_ptr().add(offset).cast::<u32>(), value) } }
    unsafe fn half(&mut self, offset: usize, value: u16) { unsafe { write_volatile(self.bytes.as_mut_ptr().add(offset).cast::<u16>(), value) } }
}
unsafe fn word(offset: usize) -> u32 { unsafe { read_volatile((0x016b_e004 + offset) as *const u32) } }
unsafe fn put(offset: usize, value: u32) { unsafe { write_volatile((0x016b_e004 + offset) as *mut u32, value) } }
unsafe fn handle() -> ProfileHandle { ProfileHandle(unsafe { word(24) }) }
unsafe fn field(pointer: u32, offset: usize) -> u32 { unsafe { read_volatile((pointer as usize + offset) as *const u32) } }

macro_rules! getters { ($($name:ident),* $(,)?) => { unsafe extern "C" { $(fn $name(handle: ProfileHandle) -> u32;)* } }; }
macro_rules! setters { ($($name:ident),* $(,)?) => { unsafe extern "C" { $(fn $name(handle: ProfileHandle, value: u32);)* } }; }
macro_rules! parameters { ($($name:ident),* $(,)?) => { unsafe extern "C" { $(fn $name() -> u32;)* } }; }
getters!(xm1_power_record_helper_74ec30,xm1_power_record_helper_74e5c0,xm1_power_record_helper_fa7634,xm1_power_record_helper_74ece4,xm1_power_record_helper_74ed6c,xm1_power_record_helper_74eba8,xm1_power_record_helper_faaae8,xm1_power_record_helper_74ef04,xm1_power_record_helper_fa7a5c,xm1_power_record_helper_74ef8c,xm1_power_record_helper_74f014,xm1_power_record_helper_750c00,xm1_power_record_helper_758cf0,xm1_power_record_helper_758f20,xm1_power_record_helper_758d78,xm1_power_record_helper_754d9c,xm1_power_record_helper_7569a4,xm1_power_record_helper_75540c,xm1_power_record_helper_7503ec,xm1_power_record_helper_756d04);
setters!(xm1_power_record_helper_7583d0,xm1_power_record_helper_7527ac,xm1_power_record_helper_7556ec,xm1_power_record_helper_75578c,xm1_power_record_helper_752844,xm1_power_record_helper_7528dc,xm1_power_record_helper_7553bc,xm1_power_record_helper_75511c,xm1_power_record_helper_754dec,xm1_power_record_helper_754e8c,xm1_power_record_helper_754fdc,xm1_power_record_helper_75507c,xm1_power_record_helper_7551bc,xm1_power_record_helper_756bbc,xm1_power_record_helper_756c04,xm1_power_record_helper_756c4c,xm1_power_record_helper_756c94,xm1_power_record_helper_756d24,xm1_power_record_helper_756d6c,xm1_power_record_helper_7569c4,xm1_power_record_helper_75545c,xm1_power_record_helper_7554fc,xm1_power_record_helper_75559c,xm1_power_record_helper_75564c);
parameters!(xm1_power_record_helper_ef9b50,xm1_power_record_helper_ef9b6c,xm1_power_record_helper_ef9f10,xm1_power_record_helper_ef9d18,xm1_power_record_helper_ef9ddc,xm1_power_record_helper_ef9e4c,xm1_power_record_helper_ef9e68,xm1_power_record_helper_ef9e84,xm1_power_record_helper_ef9ea0,xm1_power_record_helper_ef9ebc,xm1_power_record_helper_ef9ed8,xm1_power_record_helper_ef9f64,xm1_power_record_helper_ef9dc0,xm1_power_record_helper_ef9e14);
unsafe extern "C" {
    fn xm1_power_record_helper_ed8af4(out: *mut u8);
    fn xm1_power_record_helper_ed8b10(out: *mut u8);
    fn xm1_power_record_helper_ed8b2c(record: *const u8);
    fn xm1_power_record_helper_ed8b6c(record: *const u8);
    fn xm1_power_record_helper_dca798(out: *mut u8, kind: u32, pair: *const u16);
    fn xm1_power_record_helper_758e00(handle: ProfileHandle, out: *mut u32);
    fn xm1_power_record_helper_758e90(handle: ProfileHandle, out: *mut u32);
    fn xm1_power_record_helper_faae0c(handle: ProfileHandle, kind: u32, profile: u32) -> u32;
    fn xm1_power_record_helper_75526c(handle: ProfileHandle, source: *const u8);
    fn xm1_power_record_helper_75531c(handle: ProfileHandle, source: *const u8);
    fn xm1_power_record_helper_754f3c(handle: ProfileHandle, source: *const u32);
    fn xm1_power_record_helper_758cc4(handle: ProfileHandle, source: *const u32);
    fn xm1_power_record_helper_fa7c0c(mask: u32, value: u32) -> u32;
    fn xm1_power_record_helper_f30014() -> u32;
    fn xm1_power_record_helper_f4df4c();
    fn xm1_power_record_helper_fa898c();
    fn xm1_power_record_helper_fea524(callback: unsafe extern "C" fn(u32), value: u32);
    fn xm1_power_record_helper_f2f700(value: u32);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f300d0() -> u32 { unsafe {
    put(8, 1);
    xm1_power_record_helper_7583d0(handle(), 1);
    let parameter = xm1_power_record_helper_ef9b50();
    let descriptor = field(parameter, 8);
    if descriptor != u32::MAX { xm1_power_record_helper_7527ac(handle(), descriptor); }
    let mut first = Snapshot::<76>::new();
    let mut second = Snapshot::<200>::new();
    xm1_power_record_helper_ed8af4(first.bytes.as_mut_ptr());
    xm1_power_record_helper_ed8b10(second.bytes.as_mut_ptr());
    let pair = xm1_power_record_helper_74ec30(handle());
    first.half(30, pair as u16); first.half(32, (pair >> 16) as u16);
    second.word(12, xm1_power_record_helper_74e5c0(handle()));
    let mode = xm1_power_record_helper_fa7634(handle());
    second.word(16, mode);
    second.word(20, xm1_power_record_helper_74ece4(handle()));
    second.word(24, xm1_power_record_helper_74ed6c(handle()));
    let kind = xm1_power_record_helper_74eba8(handle());
    xm1_power_record_helper_dca798(second.bytes.as_mut_ptr().add(28), kind, first.bytes.as_ptr().add(30).cast());
    second.word(96, xm1_power_record_helper_faaae8(handle()));
    second.word(100, xm1_power_record_helper_74ef04(handle()));
    second.word(92, xm1_power_record_helper_fa7a5c(handle()));
    second.word(104, xm1_power_record_helper_74ef8c(handle()));
    second.word(108, xm1_power_record_helper_74f014(handle()));
    second.bytes[160] = xm1_power_record_helper_750c00(handle()) as u8;
    first.word(56, xm1_power_record_helper_758cf0(handle()));
    xm1_power_record_helper_758e00(handle(), first.bytes.as_mut_ptr().add(60).cast());
    xm1_power_record_helper_758e90(handle(), first.bytes.as_mut_ptr().add(64).cast());
    first.bytes[68] = xm1_power_record_helper_758f20(handle()) as u8;
    second.word(192, xm1_power_record_helper_758d78(handle()));
    xm1_power_record_helper_ed8b2c(first.bytes.as_ptr());
    xm1_power_record_helper_ed8b6c(second.bytes.as_ptr());
    put(44, mode);
    1
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30230() -> u32 { unsafe {
    put(8, 0); xm1_power_record_helper_7583d0(handle(), 0);
    let descriptor = field(xm1_power_record_helper_ef9b6c(), 12);
    if descriptor != u32::MAX { xm1_power_record_helper_7527ac(handle(), descriptor); }
    1
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30278() -> u32 { unsafe {
    let p = xm1_power_record_helper_ef9f10();
    xm1_power_record_helper_7556ec(handle(), field(p,4)); xm1_power_record_helper_75578c(handle(), field(p,8));
    let changed = xm1_power_record_helper_faae0c(handle(),5,word(16));
    let mode = xm1_power_record_helper_754d9c(handle());
    if changed == 1 || mode == 3 { 4 } else { 1 }
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f302e4() -> u32 { unsafe {
    let p = xm1_power_record_helper_ef9d18(); let value = field(p,4);
    if value != u32::MAX { xm1_power_record_helper_7527ac(handle(),value); }
    xm1_power_record_helper_752844(handle(),field(p,8)); xm1_power_record_helper_7528dc(handle(),field(p,12)); 1
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30338() -> u32 { unsafe {
    let p = xm1_power_record_helper_ef9ddc();
    xm1_power_record_helper_75526c(handle(),(p+4) as *const u8); xm1_power_record_helper_75531c(handle(),(p+20) as *const u8);
    xm1_power_record_helper_7553bc(handle(),field(field(p,28),0)); 1
} }
macro_rules! apply_word { ($name:ident,$parameter:ident,$setter:ident) => {
    #[unsafe(no_mangle)] pub unsafe extern "C" fn $name() -> u32 { unsafe { let p=$parameter(); $setter(handle(),field(p,4)); 1 } }
}; }
apply_word!(xm1_power_record_helper_f30380,xm1_power_record_helper_ef9e4c,xm1_power_record_helper_75511c);
apply_word!(xm1_power_record_helper_f303e0,xm1_power_record_helper_ef9e84,xm1_power_record_helper_754fdc);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f303a8() -> u32 { unsafe {
    let p=xm1_power_record_helper_ef9e68(); xm1_power_record_helper_754dec(handle(),field(p,4)); xm1_power_record_helper_754e8c(handle(),field(p,8)); 1
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30408() -> u32 { unsafe {
    let p=xm1_power_record_helper_ef9ea0(); let mut record=[0u32;4];
    for (index,value) in record.iter_mut().enumerate() { *value=field(p,4+index*4); }
    xm1_power_record_helper_754f3c(handle(),record.as_ptr()); 1
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3045c() -> u32 { unsafe {
    let p=xm1_power_record_helper_ef9ebc(); xm1_power_record_helper_75507c(handle(),field(p,4));
    if xm1_power_record_helper_faae0c(handle(),2,word(16))==1 {4} else {1}
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3049c() -> u32 { unsafe {
    let p=xm1_power_record_helper_ef9ed8(); xm1_power_record_helper_7551bc(handle(),field(p,4));
    xm1_power_record_helper_faae0c(handle(),4,word(16)); 4
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f304d4(value:u32) -> u32 { unsafe {
    let record=[value,1]; xm1_power_record_helper_758cc4(handle(),record.as_ptr()); 1
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3050c() -> u32 { unsafe { let p=xm1_power_record_helper_ef9f64(); xm1_power_record_helper_f304d4(field(p,4)); 1 } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30528(parameters:u32) { unsafe {
    let mut flags=xm1_power_record_helper_7569a4(handle());
    let mask=field(parameters,4); let enabled=field(parameters,8);
    macro_rules! update { ($bit:expr,$offset:expr,$setter:ident) => { if mask & $bit != 0 { let active=enabled & $bit != 0; flags=(flags & !$bit) | (enabled & $bit); $setter(handle(),if active {field(parameters,$offset)} else {0}); } }; }
    update!(1,12,xm1_power_record_helper_756bbc); update!(2,16,xm1_power_record_helper_756c04); update!(4,20,xm1_power_record_helper_756c4c);
    if mask & 32 != 0 { flags=(flags & !32) | (enabled & 32); }
    update!(8,24,xm1_power_record_helper_756c94);
    xm1_power_record_helper_756d24(handle(),field(parameters,32)); xm1_power_record_helper_756d6c(handle(),field(parameters,36));
    put(4,u32::from(flags!=0)); xm1_power_record_helper_7569c4(handle(),flags);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3069c() -> u32 { unsafe {
    let p=xm1_power_record_helper_ef9dc0();
    if xm1_power_record_helper_fa7c0c(field(p,4),field(p,8))==0 {return 0;}
    xm1_power_record_helper_f30528(p);
    let mut result=1;
    if field(p,4)==2 && xm1_power_record_helper_faae0c(handle(),1,word(16))==1 {result=4;}
    if xm1_power_record_helper_f30014()==0 {result=4;}
    result
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30714(mask:u32, enabled:u32) { unsafe {
    let flags=xm1_power_record_helper_75540c(handle());
    let bits=mask & 15; let flags=(flags & !bits) | (enabled & bits);
    xm1_power_record_helper_75545c(handle(),mask); xm1_power_record_helper_7554fc(handle(),flags);
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f307ac() -> u32 { unsafe {
    let p=xm1_power_record_helper_ef9e14(); xm1_power_record_helper_f30714(field(p,4),field(p,8));
    xm1_power_record_helper_75559c(handle(),field(p,12)); xm1_power_record_helper_75564c(handle(),field(p,16)); 1
} }
#[unsafe(no_mangle)]
pub extern "C" fn xm1_power_record_helper_f307f0() -> u32 {0}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30808(value:u32) { unsafe {put(24,value)} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f30828() { unsafe {
    xm1_power_record_helper_f4df4c();
    if xm1_power_record_helper_7503ec(handle())==1 {
        let callback=xm1_power_record_helper_756d04(handle());
        if callback!=0 {
            let callback:unsafe extern "C" fn(u32,u32,u32,u32,u32)=core::mem::transmute(callback as usize);
            callback(0,0,0,0,1);
        }
    }
} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3088c() { unsafe {xm1_power_record_helper_fa898c()} }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3089c() { unsafe {
    xm1_power_record_helper_fea524(xm1_power_record_helper_f2f700,0);
    if xm1_power_record_helper_74e5c0(handle())!=1 {xm1_power_record_helper_f3088c();}
} }
