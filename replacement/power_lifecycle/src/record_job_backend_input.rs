//! X-M1 1.50 f69884/f699e4/f69b08 input projections.
use crate::record_job_backend_packet::Address;
use crate::record_job_backend_dependencies::*;
unsafe extern "C" {
    fn xm1_power_record_helper_f72214(out:u32,packet:u32,a:u32,b:u32,c:u32,d:u32,e:i32);
    fn xm1_power_record_helper_757878(item:u32)->i32;
    fn xm1_power_record_helper_7578c8(item:u32)->i32;
    fn xm1_power_record_helper_757968(item:u32)->i32;
    fn xm1_power_record_helper_7579b8(item:u32)->i32;
    fn xm1_power_record_helper_757a08(out:u32,item:u32);
    fn xm1_power_record_helper_757a74(out:u32,item:u32);
    fn xm1_power_record_helper_757b0c(out:u32,item:u32);
    fn xm1_power_record_helper_758a0c(item:u32)->i32;
    fn xm1_power_record_helper_758a5c(item:u32)->i32;
    fn xm1_power_record_helper_758aac(item:u32)->i32;
    fn xm1_power_record_helper_758afc(item:u32)->i32;
    fn xm1_power_record_helper_758b4c(item:u32)->i32;
    fn xm1_power_record_helper_758b9c(item:u32)->i32;
    fn xm1_power_record_helper_758bec(item:u32)->u32;
}
#[repr(C,align(4))]
struct InputPacket([u8;76]);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f69884(item:u32,reference:u32,pair:u32,kind:u32,a:u32,b:u32,profile:u32,out:u32) {
    let mut packet=InputPacket([0;76]);let p=Address(packet.0.as_mut_ptr() as usize as u32);let mut result=[0u32;10];
    unsafe {
        p.put(0,xm1_power_record_helper_f612dc(item,profile));p.put_half(4,xm1_power_record_helper_f61584(item,profile) as i32);p.put(8,xm1_power_record_helper_74e7f0(item));p.put(12,b);p.put(16,(Address(reference).word(12)==1) as u32);p.put(20,xm1_power_record_helper_750034(item));p.put(24,xm1_power_record_helper_7500bc(item));p.put(28,xm1_power_record_helper_7509c4(item));p.put(32,xm1_power_record_helper_7503ec(item));p.put(36,xm1_power_record_helper_74f1f0(item));p.put(40,xm1_power_record_helper_750474(item));p.put_byte(44,kind as u8);p.put_byte(45,Address(pair).byte(0));p.put_byte(46,Address(pair).byte(1));p.put(48,a);p.put_half(52,xm1_power_record_helper_74f6fc(item) as i32);p.put_half(54,xm1_power_record_helper_74f80c(item) as i32);p.put(56,xm1_power_record_helper_74f784(item));p.put(60,0);p.put(64,Address(reference).word(8));p.put_half(68,xm1_power_record_helper_74f91c(item) as i32);p.put(72,xm1_power_record_helper_f6171c(item,profile));
        let r=Address(result.as_mut_ptr() as usize as u32);xm1_power_record_helper_f72c3c(p.0,r.0);let out=Address(out);out.put_half(0,r.half(0) as i32);out.put_half(2,r.half(2) as i32);out.put(4,r.word(4));out.put(8,r.word(8));out.put_half(12,r.half(34) as i32);out.put_half(14,r.half(36) as i32);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f699e4(a:u32,b:i32,c:u32,d:u32,out:u32,e:u32,f:u32,g:u32,h:u32,i:i32) {
    let mut packet=[0u32;7];let p=Address(packet.as_mut_ptr() as usize as u32);
    unsafe {p.put(0,a);p.put_half(4,b);p.put(8,c);p.put(12,d);p.put(16,1);p.put_half(20,0x7fff);p.put(24,0);xm1_power_record_helper_f72214(out,p.0,e,f,g,h,i);}
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f69b08(context:u32,item:u32) {
    unsafe {
        let c=Address(context);
        c.put_half(0x90,xm1_power_record_helper_757878(item));c.put_half(0x92,xm1_power_record_helper_7578c8(item));c.put_half(0x94,xm1_power_record_helper_757968(item));c.put_half(0x96,xm1_power_record_helper_7579b8(item));
        let mut buffer=[0u32;261];let p=Address(buffer.as_mut_ptr() as usize as u32);xm1_power_record_helper_757a08(p.0,item);c.at(0x98).copy(p,0x414);xm1_power_record_helper_757a74(p.0,item);c.at(0x4ac).copy(p,40);
        c.put_half(0x524,xm1_power_record_helper_758a0c(item));c.put_half(0x526,xm1_power_record_helper_758a5c(item));c.put_half(0x528,xm1_power_record_helper_758aac(item));c.put_half(0x52a,xm1_power_record_helper_758afc(item));c.put_half(0x52c,xm1_power_record_helper_758b4c(item));c.put_half(0x52e,xm1_power_record_helper_758b9c(item));c.put(0x530,xm1_power_record_helper_758bec(item));xm1_power_record_helper_757b0c(p.0,item);c.at(0x4d4).copy(p,80);
    }
}
