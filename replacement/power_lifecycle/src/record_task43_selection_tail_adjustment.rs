//! X-M1 1.50 adjustment tails, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 and image-5 bytes.

use core::ptr::{read_volatile,write_volatile};
#[repr(transparent)]
#[derive(Clone,Copy)]
struct ProfileHandle(u32);
#[repr(C)]
struct StatisticsDescriptor { enabled:u32, index:u8, reserved:[u8;3], records:u32 }
#[repr(C,align(4))]
struct StatisticsSnapshot { bytes:[u8;1480] }
impl StatisticsSnapshot {
    unsafe fn word(&self,offset:usize)->u32 {unsafe{read_volatile(self.bytes.as_ptr().add(offset).cast())}}
    unsafe fn half(&self,offset:usize)->u16 {unsafe{read_volatile(self.bytes.as_ptr().add(offset).cast())}}
}
#[repr(C)]
struct GeometryRequest {
    scales:[u8;2],percent:[u16;2],zero:[u16;2],pair:[u16;2],kinds:[u8;2],limits:[u16;4],saved_pair:[u16;2],curve:u32,descriptor:[u32;2]
}
unsafe fn aux_handle()->ProfileHandle {ProfileHandle(unsafe{read_volatile(0x016b_e01c as *const u32)})}
unsafe fn handle()->ProfileHandle {ProfileHandle(unsafe{read_volatile(0x016b_e054 as *const u32)})}
unsafe fn put(offset:usize,value:u32) {unsafe{write_volatile((0x016b_e044+offset) as *mut u32,value)}}
unsafe fn word(offset:usize)->u32 {unsafe{read_volatile((0x016b_e044+offset) as *const u32)}}
unsafe extern "C" {
    fn xm1_power_record_helper_ef9fb8()->u32;
    fn xm1_power_record_helper_fead6c(callback:unsafe extern "C" fn(u32),value:u32);
    fn xm1_power_record_helper_f2f65c(value:u32);
    fn xm1_power_record_helper_ef7d68()->u32;
    fn xm1_power_record_helper_ef7d1c(channel:u32)->u32;
    fn xm1_power_record_helper_ef7eb4(channel:u32);
    fn xm1_power_record_helper_ef87b0(value:u32);
    fn xm1_power_record_helper_7554ac(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_75540c(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_7554fc(handle:ProfileHandle,value:u32);
    fn xm1_power_record_helper_efa1ac()->u32;
    fn xm1_power_record_helper_76ff38(value:u32);
    fn xm1_power_record_helper_fead98(callback:unsafe extern "C" fn(u32,u32,u32,u32,u32,u32),first:u32,flag:u8,second:u32,a:u32,b:u32,c:u32,d:u32,e:u32,f:u32);
    fn xm1_power_record_helper_f2f56c(a:u32,b:u32,c:u32,d:u32,e:u32,f:u32);
    fn xm1_power_record_helper_feadfc();
    fn xm1_power_record_helper_74e020(handle:ProfileHandle,mask:u64)->u32;
    fn xm1_power_record_helper_fea79c(client:u32);
    fn xm1_power_record_helper_fea7ac(client:u32,out:*mut StatisticsSnapshot);
    fn xm1_power_record_helper_7567ec(handle:ProfileHandle,out:*mut StatisticsDescriptor);
    fn xm1_power_record_helper_756814(handle:ProfileHandle,descriptor:*const StatisticsDescriptor)->u32;
    fn xm1_power_record_helper_f5206c(out:*mut u32,handle:ProfileHandle);
    fn xm1_power_record_helper_757650(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_746bf8(first:u32,second:u32)->u32;
    fn xm1_power_record_helper_772f28(value:u32)->u32;
    fn xm1_power_record_helper_ef5310(value:u32)->u32;
    fn xm1_power_record_helper_756b0c(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_74f1f0(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_103b064();
    fn xm1_power_record_helper_757e38(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_f4f16c(handle:ProfileHandle,mode:u32)->u32;
    fn xm1_power_record_helper_7527f8(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_752890(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_74ec30(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_69f8fc(key:u32)->u32;
    fn xm1_power_record_helper_fea57c(request:*const GeometryRequest);
    fn xm1_power_record_helper_74e5c0(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_fea550(callback:unsafe extern "C" fn(u32),value:u32);
    fn xm1_power_record_helper_f32508(value:u32);
    fn xm1_power_record_helper_efb4b8(change:i32)->u32;
    fn xm1_power_record_helper_7569a4(handle:ProfileHandle)->u32;
    fn xm1_power_record_helper_756734(handle:ProfileHandle,out:*mut u32);
    fn xm1_power_record_helper_756754(handle:ProfileHandle,record:*const u32)->u32;
    fn xm1_power_record_helper_756788(handle:ProfileHandle,out:*mut u32);
    fn xm1_power_record_helper_7567b8(handle:ProfileHandle,record:*const u32)->u32;
    fn xm1_power_record_helper_756e24(out:*mut u32,handle:ProfileHandle);
    fn xm1_power_record_helper_756e5c(handle:ProfileHandle,record:*const u32)->u32;
    fn xm1_power_record_helper_756e90(out:*mut u32,handle:ProfileHandle);
    fn xm1_power_record_helper_756ebc(handle:ProfileHandle,record:*const u32)->u32;
    fn xm1_power_record_helper_756f3c(out:*mut u32,handle:ProfileHandle);
    fn xm1_power_record_helper_756f60(handle:ProfileHandle,record:*const u32)->u32;
    fn xm1_power_record_helper_fa7c24();
    fn xm1_power_record_helper_f32c74();
    fn xm1_power_record_helper_f32f80(first:u32,second:u32)->u32;
    fn xm1_power_record_helper_fa8018()->u32;
    fn xm1_power_record_helper_756b74(handle:ProfileHandle,value:u32);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f322dc() {unsafe{
    let p=xm1_power_record_helper_ef9fb8(); let value=read_volatile((p+4) as *const u32);
    xm1_power_record_helper_fead6c(xm1_power_record_helper_f2f65c,value);
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f322fc() {unsafe{
    if xm1_power_record_helper_ef7d68()==0 && xm1_power_record_helper_ef7d1c(2)!=0 {xm1_power_record_helper_ef7eb4(2);}
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3232c() {unsafe{if xm1_power_record_helper_ef7d1c(2)!=0 {xm1_power_record_helper_ef87b0(u32::MAX);}}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32350()->u32 {unsafe{xm1_power_record_helper_ef7d68()}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32360()->u32 {unsafe{
    let flags=xm1_power_record_helper_7554ac(aux_handle()); let enabled=xm1_power_record_helper_75540c(aux_handle());
    if enabled&1!=0 {flags&1} else {0}
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32394() {unsafe{let flags=xm1_power_record_helper_7554ac(aux_handle());xm1_power_record_helper_7554fc(aux_handle(),flags&!1);}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f323bc() {unsafe{let flags=xm1_power_record_helper_7554ac(aux_handle());xm1_power_record_helper_7554fc(aux_handle(),flags|1);}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f323e4(value:u32) {unsafe{write_volatile(0x016b_e038 as *mut u8,value as u8)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32404()->u32 {unsafe{
    let p=xm1_power_record_helper_efa1ac();
    let get=|offset:usize|read_volatile((p as usize+offset) as *const u32);
    write_volatile(0x016b_e040 as *mut u32,get(0));
    xm1_power_record_helper_76ff38(1);
    xm1_power_record_helper_fead98(xm1_power_record_helper_f2f56c,get(8),read_volatile((p+12) as *const u8),get(16),get(20),get(24),get(28),get(32),get(36),get(40)); 2
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32484()->u32 {unsafe{xm1_power_record_helper_76ff38(0);xm1_power_record_helper_feadfc();1}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f324a0() {unsafe{write_volatile(0x016b_e03c as *mut u32,1)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f324c4() {unsafe{write_volatile(0x016b_e03c as *mut u32,0)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f324e8()->u32 {unsafe{read_volatile(0x016b_e03c as *const u32)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f325d0() {unsafe{put(8,0)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f325f4() {unsafe{put(8,1)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32618(kind:u32)->u32 {unsafe{if kind==13 {u32::from(xm1_power_record_helper_74e020(handle(),128)!=0)} else {0}}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32654() {unsafe{
    xm1_power_record_helper_fea79c(4);
    let mut snapshot=StatisticsSnapshot {bytes:[0;1480]}; xm1_power_record_helper_fea7ac(4,&mut snapshot);
    if snapshot.word(0)&5!=5 {return;}
    let mut descriptor=StatisticsDescriptor {enabled:0,index:0,reserved:[0;3],records:0};
    xm1_power_record_helper_7567ec(handle(),&mut descriptor);
    if descriptor.enabled==0 {
        descriptor.enabled=1;descriptor.index=0;descriptor.records=0x5ffe_e718;
        for index in 0..6 {write_volatile((descriptor.records as usize+index*0x228) as *mut u32,0);}
    } else {descriptor.index=descriptor.index.wrapping_add(1);if descriptor.index>4 {descriptor.index=0;}}
    let record=descriptor.records as usize+descriptor.index as usize*0x228;
    let mut coordinate=[0u32;2];xm1_power_record_helper_f5206c(coordinate.as_mut_ptr(),handle());
    write_volatile(record as *mut u32,1);
    let cell=coordinate[0].wrapping_add(coordinate[1].wrapping_mul(8));
    write_volatile((record+0x204) as *mut u16,read_volatile((0x5ffe_e518usize.wrapping_add(cell as usize*2)) as *const u16));
    write_volatile((record+0x206) as *mut u16,read_volatile((0x5ffe_e518usize.wrapping_add(cell.wrapping_add(64) as usize*2)) as *const u16));
    write_volatile((record+0x208) as *mut u16,snapshot.half(264));write_volatile((record+0x20a) as *mut u16,snapshot.half(254));
    for (offset,source) in [(0x20c,4),(0x210,8),(0x214,220),(0x218,224)] {write_volatile((record+offset) as *mut u32,snapshot.word(source));}
    let base=xm1_power_record_helper_757650(handle());
    let aperture=xm1_power_record_helper_746bf8(snapshot.word(248),snapshot.word(272));
    let channel=xm1_power_record_helper_772f28(snapshot.word(244));
    let exposure=xm1_power_record_helper_ef5310(snapshot.word(236));
    let delta=exposure.wrapping_sub(channel).wrapping_add(base.wrapping_sub((snapshot.half(252) as u32).wrapping_sub(snapshot.half(254) as u32)).wrapping_sub(aperture));
    write_volatile((record+0x208) as *mut u16,delta as u16);
    core::ptr::copy_nonoverlapping(0x5ffe_e414 as *const u8,(record+4) as *mut u8,256);
    core::ptr::copy_nonoverlapping(0x5ffe_e314 as *const u8,(record+0x104) as *mut u8,256);
    write_volatile((record+0x21c) as *mut u32,snapshot.word(52));write_volatile((record+0x220) as *mut u32,snapshot.word(56));
    write_volatile((record+0x224) as *mut u32,read_volatile(0x5ffe_e514 as *const u32));
    xm1_power_record_helper_756814(handle(),&descriptor);
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32894()->u32 {unsafe{
    let flags=xm1_power_record_helper_756b0c(handle());
    if flags&1!=0 {xm1_power_record_helper_fea79c(0);if xm1_power_record_helper_74f1f0(handle())==6 {xm1_power_record_helper_103b064();}}
    if flags&2!=0 {xm1_power_record_helper_fea79c(1);} if flags&4!=0 {xm1_power_record_helper_fea79c(2);}
    xm1_power_record_helper_f32654();1
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32908() {unsafe{put(12,1)}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f3292c() {unsafe{
    let mode=xm1_power_record_helper_757e38(handle());let curve=xm1_power_record_helper_f4f16c(handle(),mode);
    let descriptor=[xm1_power_record_helper_7527f8(handle()),xm1_power_record_helper_752890(handle())];
    let pair=xm1_power_record_helper_74ec30(handle());let pair=[pair as u16,(pair>>16) as u16];
    let first_b=xm1_power_record_helper_69f8fc(0x8797) as u16;let first_a=xm1_power_record_helper_69f8fc(0x8798) as u16;
    let second_b=xm1_power_record_helper_69f8fc(0x8799) as u16;let second_a=xm1_power_record_helper_69f8fc(0x879a) as u16;
    let request=GeometryRequest {scales:[16,16],percent:[100,100],zero:[0,0],pair,kinds:[7,7],limits:[first_a,first_b,second_a,second_b],saved_pair:pair,curve,descriptor};
    xm1_power_record_helper_fea57c(&request);
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32a04(kind:u32) {unsafe{
    let mode=xm1_power_record_helper_74e5c0(handle());let value=if kind!=2 {4} else if mode!=0 {3} else {2};
    xm1_power_record_helper_fea550(xm1_power_record_helper_f32508,value);
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32a48(incoming:u32)->u32 {unsafe{
    let mut result=incoming;
    if word(12)==1 {
        xm1_power_record_helper_efb4b8(0);
        let flags=xm1_power_record_helper_7569a4(handle());result=flags;
        if flags&1==0 {let mut record=[0u32;33];xm1_power_record_helper_756734(handle(),record.as_mut_ptr());record[1]=0;result=xm1_power_record_helper_756754(handle(),record.as_ptr());}
        if flags&2==0 {let mut record=[0u32;8];xm1_power_record_helper_756788(handle(),record.as_mut_ptr());record[0]=0;result=xm1_power_record_helper_7567b8(handle(),record.as_ptr());}
        if flags&4==0 {let mut record=[0u32;5];xm1_power_record_helper_756e24(record.as_mut_ptr(),handle());record[3]=0;result=xm1_power_record_helper_756e5c(handle(),record.as_ptr());}
        if flags&32==0 {let mut record=[0u32;4];xm1_power_record_helper_756e90(record.as_mut_ptr(),handle());record[0]=0;result=xm1_power_record_helper_756ebc(handle(),record.as_ptr());}
        if flags==0 {let mut record=[0u32;20];xm1_power_record_helper_756f3c(record.as_mut_ptr(),aux_handle());record[0]=0;result=xm1_power_record_helper_756f60(aux_handle(),record.as_ptr());}
    }
    put(12,0);result
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f32d9c(profile:u32) {unsafe{
    let profile=ProfileHandle(profile);let mut descriptor=StatisticsDescriptor {enabled:0,index:0,reserved:[0;3],records:0};
    xm1_power_record_helper_7567ec(profile,&mut descriptor);descriptor.enabled=0;
    xm1_power_record_helper_756814(profile,&descriptor);xm1_power_record_helper_fa7c24();xm1_power_record_helper_f32c74();
}}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_record_helper_f33094() {unsafe{
    let flags=xm1_power_record_helper_7569a4(handle());let mask=xm1_power_record_helper_f32f80(47,flags);
    write_volatile(0x016b_e044 as *mut u8,mask as u8);write_volatile(0x016b_e047 as *mut u8,0);write_volatile(0x016b_e048 as *mut u8,0);
    if xm1_power_record_helper_fa8018()==0 {xm1_power_record_helper_756b74(handle(),0);}
}}
