//! X-M1 1.50 publication at 0xf2a468..0xf2ab5c, pinned image-5 byte semantics.

use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};
unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn byte(address: u32) -> u8 { unsafe { read_volatile(address as *const u8) } }
unsafe fn set(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32,value) } }
unsafe fn item() -> u32 { unsafe { word(0x16b7510) } }
unsafe fn count(request: u32) -> u8 { unsafe { byte(word(request+56)+1) } }
unsafe fn selected_item(request: u32, index: u8) -> u32 { unsafe { word(word(request+20)+index as u32*4) } }

unsafe extern "C" {
    fn xm1_power_record_helper_72872c(event:u32);
    fn xm1_power_record_helper_f2a200(count:u32,limit:u32)->u32;
    fn xm1_power_record_helper_74e1a4(item:u32,mode:u32);
    fn xm1_power_record_helper_74e284(item:u32,selection:u32,index:u32);
    fn xm1_power_record_helper_75a238(item:u32)->u32;
    fn xm1_power_record_helper_76d9c8()->u32;
    fn xm1_power_record_helper_76dbb4()->u32;
    fn xm1_power_record_helper_76da84()->u32;
    fn xm1_power_record_helper_76dad4()->u32;
    fn xm1_power_record_helper_76f340()->u32;
    fn xm1_power_record_helper_f60efc(item:u32,reference:u32,rate:u32,direction:u32,limit:u32,mode:u32,flags:u32)->u32;
    fn xm1_power_record_helper_f4e620(record:*const u8,reference:u32,direction:u32)->u32;
    fn xm1_power_record_helper_754098(item:u32,index:u32,value:u32);
    fn xm1_power_record_helper_754148(item:u32,index:u32,value:u32);
    fn xm1_power_record_helper_752928(output:*mut u8,item:u32,index:u32);
    fn xm1_power_record_helper_75ae78(item:u32,record:*const u8);
    fn xm1_power_record_helper_f15708()->i32;
    fn xm1_power_record_helper_7582c0(output:*mut u8,item:u32);
    fn xm1_power_record_helper_f1db30(item:u32,record:*mut u8,output:u32)->u32;
    fn xm1_power_record_helper_75060c(item:u32)->u32;
    fn xm1_power_record_helper_fa6904(first:u32,second:u32)->u32;
    fn xm1_power_record_helper_100585c(value:*mut u16,adjusted:*mut u16);
    fn xm1_power_record_helper_752eb8(item:u32,index:u32,value:u32);
    fn xm1_power_record_helper_752e08(item:u32,index:u32,value:u32);
    fn xm1_power_record_helper_753c78(item:u32,index:u32,value:u32);
    fn xm1_power_record_helper_69f8fc(key:u32)->u32;
    fn xm1_power_record_helper_74f388(item:u32,value:u32);
    fn xm1_power_record_helper_753960(item:u32,index:u32)->u32;
    fn xm1_power_record_helper_753a10(item:u32,index:u32)->u32;
    fn xm1_power_record_helper_ee8bb0()->u32;
    fn xm1_power_record_helper_74f1ac(item:u32)->u32;
    fn xm1_power_record_helper_74f344(item:u32)->u32;
    fn xm1_power_record_helper_74f278(item:u32,value:u32);
    fn xm1_power_record_helper_740888(output:*mut u32,a:u32,b:u32,c:u32);
    fn xm1_power_record_helper_753e88(item:u32,index:u32,value:u32);
    fn xm1_power_record_helper_74f6fc(item:u32)->u32;
    fn xm1_power_record_helper_752bf8(item:u32,index:u32,value:u32);
    fn xm1_power_record_helper_ef8934(output:u32);
    fn xm1_power_record_helper_f4ed28(output:u32);
    fn xm1_power_record_helper_103b054(output:u32);
    fn xm1_power_record_helper_740a00();
    fn xm1_power_record_helper_758360(item:u32,value:u32);
    fn xm1_power_record_helper_755b58(item:u32,index:u32);
    fn xm1_power_record_helper_755c00(item:u32,index:u32,record:*const u8);
    fn xm1_power_record_helper_f290d8();
}

// The callback extent 0x10029a8..0x10029bc changes no application state.
#[no_mangle]
pub extern "C" fn xm1_power_record_helper_10029a8() {}

unsafe fn load_record(index:u8,record:*mut u8) {
    let mut source=MaybeUninit::<[u32;42]>::uninit();
    unsafe {
        xm1_power_record_helper_752928(source.as_mut_ptr().cast(),item(),index as u32);
        core::ptr::copy_nonoverlapping(source.as_ptr().cast::<u8>(),record,168);
    }
}
unsafe fn publish_records(request:u32) {
    let mut record=MaybeUninit::<[u32;42]>::uninit();
    let mut index=0u8;
    unsafe {
        while index<count(request) {
            load_record(index, record.as_mut_ptr().cast());
            xm1_power_record_helper_75ae78(selected_item(request,index),record.as_ptr().cast());
            index=index.wrapping_add(1);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2a468() {
    unsafe {
        xm1_power_record_helper_72872c(0x03000000);
        let request=0x169d09c;
        let selection=word(request+56);
        let limited=xm1_power_record_helper_f2a200(byte(selection) as u32,count(request) as u32);
        let flags=word(request+4);
        if flags&10!=0 { xm1_power_record_helper_74e1a4(item(),word(request+16)); }
        if flags&10!=0 || flags&0x1b5!=0 {
            xm1_power_record_helper_74e284(item(),word(word(request+20)+limited.wrapping_sub(1)*4),0);
        }
        let mode=xm1_power_record_helper_75a238(item());
        if mode==1 && limited==1 {
            let reference=xm1_power_record_helper_76d9c8();
            let direction=xm1_power_record_helper_76dbb4();
            let rate=xm1_power_record_helper_76da84();
            let limit=xm1_power_record_helper_76dad4();
            let flags=xm1_power_record_helper_76f340();
            let mut record=MaybeUninit::<[u32;4]>::uninit();
            let pointer=record.as_mut_ptr().cast::<u32>();
            pointer.cast::<u16>().write(rate as u16); pointer.add(1).write(reference); pointer.add(2).write(direction); pointer.add(3).write(limit);
            let category=xm1_power_record_helper_f60efc(item(),reference,rate,direction,limit,flags,0);
            let direction_category=xm1_power_record_helper_f4e620(pointer.cast(),reference,direction);
            for index in 0..3 { xm1_power_record_helper_754098(item(),index,category); }
            for index in 0..3 { xm1_power_record_helper_754148(item(),index,direction_category); }
            publish_records(request);
        }
        if xm1_power_record_helper_f15708()!=0 && byte(selection)==1 && word(request+8)!=3 {
            let mut source=MaybeUninit::<[u32;42]>::uninit();
            xm1_power_record_helper_7582c0(source.as_mut_ptr().cast(),item());
            let mut record=MaybeUninit::<[u32;10]>::uninit();
            core::ptr::copy_nonoverlapping(source.as_ptr().cast::<u32>(),record.as_mut_ptr().cast::<u32>(),10);
            let changed=xm1_power_record_helper_f1db30(item(),record.as_mut_ptr().cast(),word(word(request+12)+80));
            if changed!=0 { publish_records(request); }
            let mode=xm1_power_record_helper_75060c(item());
            let value=if mode==1 {
                let mut record=MaybeUninit::<[u32;42]>::uninit();
                load_record(0, record.as_mut_ptr().cast());
                let pointer=record.as_ptr().cast::<u32>();
                xm1_power_record_helper_fa6904(pointer.add(9).read(),pointer.add(10).read())
            } else { mode };
            set(selection+100,value);
        }
        if limited==1 {
            let mut index=0u8;
            while index<count(request) {
                let mut record=MaybeUninit::<[u32;42]>::uninit();
                load_record(index, record.as_mut_ptr().cast());
                let halves=record.as_mut_ptr().cast::<u16>();
                let baseline=halves.add(4).read();
                let mut value=baseline.wrapping_add(halves.add(6).read()).wrapping_add(halves.add(7).read());
                let mut adjusted=MaybeUninit::<u16>::uninit();
                xm1_power_record_helper_100585c(&mut value,adjusted.as_mut_ptr());
                let adjusted=adjusted.assume_init();
                let delta=value.wrapping_sub(baseline);
                halves.add(24).write(adjusted); halves.add(23).write(delta);
                xm1_power_record_helper_752eb8(item(),index as u32,adjusted as i16 as i32 as u32);
                xm1_power_record_helper_752e08(item(),index as u32,delta as i16 as i32 as u32);
                xm1_power_record_helper_75ae78(selected_item(request,index),record.as_ptr().cast());
                index=index.wrapping_add(1);
            }
        }
        set(0x16b7514,0); set(0x16b7518,1);
        if word(request+8)!=3 {
            xm1_power_record_helper_f2c0e4(); xm1_power_record_helper_f2bdf0(item()); xm1_power_record_helper_753c78(item(),0,0);
            if xm1_power_record_helper_69f8fc(0xe1)&1!=0 { xm1_power_record_helper_74f388(item(),1); }
            if xm1_power_record_helper_753960(item(),0)!=0 && xm1_power_record_helper_753a10(item(),0)!=0
                && (xm1_power_record_helper_ee8bb0()!=0 || xm1_power_record_helper_74f1ac(item())==7 || xm1_power_record_helper_74f344(item())==1) {
                xm1_power_record_helper_f2c0c4(1);
            }
            if xm1_power_record_helper_69f8fc(0xa8)&1!=0 {
                let (mode,enabled)=match xm1_power_record_helper_69f8fc(0xb1) { 1|4|5|6|7=>(0,1),3=>(7,1),_=>(6,0) };
                xm1_power_record_helper_74f278(item(),mode); xm1_power_record_helper_f2c0c4(enabled);
            }
            let mut value=MaybeUninit::<u32>::uninit();
            if xm1_power_record_helper_74f344(item())==1 { xm1_power_record_helper_740888(value.as_mut_ptr(),0,0,0); }
            else { value.write(0); }
            xm1_power_record_helper_753e88(item(),0,value.assume_init());
            if xm1_power_record_helper_69f8fc(0xa8)&128!=0 {
                let output=word(word(request+12)+80);
                let saved=[word(output+0xa5680),word(output+0xa5684),word(output+0xa5688),word(output+0xa568c)];
                for offset in 0..0x5394 { write_volatile((output+0xa074c+offset) as *mut u8,0); }
                for (index,value) in saved.into_iter().enumerate() { set(output+0xa5680+index as u32*4,value); }
            }
        }
        if word(request+4)&16!=0 && xm1_power_record_helper_74f6fc(item()) as i32== -32768 { xm1_power_record_helper_752bf8(item(),0,(-500i32) as u32); }
        if xm1_power_record_helper_69f8fc(0xa8)&128!=0 {
            xm1_power_record_helper_ef8934(word(word(request+12)+80));
            xm1_power_record_helper_f4ed28(word(word(request+12)+80));
            xm1_power_record_helper_103b054(word(word(request+12)+80));
        }
        xm1_power_record_helper_740a00();
        set(word(request+56)+104,xm1_power_record_helper_10029a8 as *const () as usize as u32);
        xm1_power_record_helper_758360(item(),1);
        xm1_power_record_helper_f290d8();
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2bdf0(item:u32) {
    let record=[0u32;26];
    unsafe {
        xm1_power_record_helper_755b58(item,0);
        xm1_power_record_helper_755c00(item,0,record.as_ptr().cast());
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2c0c4(value:u32) {
    unsafe { set(0x16b7660,value); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f2c0e4() {
    unsafe {
        xm1_power_record_helper_f2c0c4(0);
        for offset in 0..84 { write_volatile((0x16b7668+offset) as *mut u8,0); }
        write_volatile(0x16b7666 as *mut u16,0x8000);
        write_volatile(0x16b7664 as *mut u16,0x8000);
    }
}
