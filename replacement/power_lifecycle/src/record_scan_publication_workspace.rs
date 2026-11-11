use crate::record_address_space::{ByteCount,GuestAddress,GuestAddressSpace,MemoryError,RecordContext};
use crate::record_scan_publication_history::{copy,word,put_word};
pub trait CachePublication{fn clean_data_cache(&mut self,address:GuestAddress,length:ByteCount)->Result<(),MemoryError>;}
pub trait ScanPublicationLock{
    fn wait_scan_publication(&mut self)->Result<i32,MemoryError>;
    fn signal_scan_publication(&mut self)->Result<i32,MemoryError>;
}
fn publish<M:GuestAddressSpace,E:CachePublication>(c:&mut RecordContext<M,E>,output:GuestAddress,source:u32,length:u32)->Result<(),MemoryError>{
    copy(c,output.get(),source,length)?;
    let aligned_length=length.wrapping_add(source&31).wrapping_add(31)&!31;
    let(address,length)=if aligned_length>0x4000{(0,0)}else{(source&!0x4000_001f,aligned_length)};
    c.effects.clean_data_cache(GuestAddress::new(address),ByteCount(length))
}
pub fn helper_fb236c<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,selector:u32)->Result<(),MemoryError>{if word(c,0x4171_8364)?!=selector{put_word(c,0x4171_8364,selector)?;}Ok(())}
pub fn helper_fb2438<M:GuestAddressSpace,E:CachePublication>(c:&mut RecordContext<M,E>,output:GuestAddress)->Result<(),MemoryError>{publish(c,output,0x016e_8fc0,300)}
pub fn helper_fb259c<M:GuestAddressSpace,E:CachePublication>(c:&mut RecordContext<M,E>,output:GuestAddress)->Result<(),MemoryError>{publish(c,output,0x016e_90ec,0x1800)}
pub fn helper_fb26a4<M:GuestAddressSpace,E:CachePublication>(c:&mut RecordContext<M,E>,output:GuestAddress)->Result<(),MemoryError>{publish(c,output,0x016e_a8ec,0x300)}
pub fn helper_fb27dc<M:GuestAddressSpace,E:CachePublication>(c:&mut RecordContext<M,E>,output:GuestAddress)->Result<(),MemoryError>{publish(c,output,0x016e_abec,300)}
pub fn helper_fb35d8<M:GuestAddressSpace,E:CachePublication>(c:&mut RecordContext<M,E>,output:GuestAddress)->Result<(),MemoryError>{publish(c,output,0x016e_ad18,28)}
pub fn helper_f4eccc<M:GuestAddressSpace,E:CachePublication+ScanPublicationLock>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{
    c.effects.wait_scan_publication()?;
    helper_fb2438(c,GuestAddress::new(0x016c_73f0))?;helper_fb259c(c,GuestAddress::new(0x016c_751c))?;
    helper_fb26a4(c,GuestAddress::new(0x016c_8d1c))?;helper_fb27dc(c,GuestAddress::new(0x016c_901c))?;helper_fb35d8(c,GuestAddress::new(0x016c_9148))?;
    c.effects.signal_scan_publication()?;Ok(())
}
