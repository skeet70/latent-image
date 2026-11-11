use crate::record_address_space::{ByteCount,CallbackId,GuestAddress,GuestAddressSpace,MemoryError,RecordContext};
pub use crate::record_address_space::RecordError as JobError;


#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct JobEnvelope {pub operation:u16,pub source:u8,pub destination:u8,pub length:ByteCount}

pub trait RecordJobEffects {
    fn current_task(&mut self)->u32;
    // Successful submission owns the payload and complete callback generations.
    fn submit_guest<M:GuestAddressSpace>(&mut self,memory:&mut M,envelope:JobEnvelope,payload:GuestAddress,callbacks:&[CallbackId])->Result<(),JobError>;
    fn submit_owned(&mut self,envelope:JobEnvelope,payload:&[u8],callbacks:&[CallbackId])->Result<(),JobError>;
    fn invalidate(&mut self,address:GuestAddress,length:ByteCount);
    fn copy_profile_diagnostics(&mut self,kind:u32,source:GuestAddress,length:ByteCount,flags:u32)->Result<(),JobError>;
}

#[derive(Clone,Copy)]
pub(crate) struct RecordAddress(pub(crate) u32);
impl RecordAddress {
    pub(crate) fn at(self,offset:u32)->Self {Self(self.0.wrapping_add(offset))}
    pub(crate) fn guest(self)->GuestAddress {GuestAddress::new(self.0)}
    pub(crate) fn word<M:GuestAddressSpace,E>(self,ctx:&mut RecordContext<M,E>,offset:u32)->Result<u32,MemoryError> {ctx.read_u32(self.guest().checked_add(offset)?)}
    pub(crate) fn half<M:GuestAddressSpace,E>(self,ctx:&mut RecordContext<M,E>,offset:u32)->Result<u16,MemoryError> {ctx.read_u16(self.guest().checked_add(offset)?)}
    pub(crate) fn signed<M:GuestAddressSpace,E>(self,ctx:&mut RecordContext<M,E>,offset:u32)->Result<i32,MemoryError> {Ok(self.half(ctx,offset)? as i16 as i32)}
    pub(crate) fn byte<M:GuestAddressSpace,E>(self,ctx:&mut RecordContext<M,E>,offset:u32)->Result<u8,MemoryError> {ctx.read_u8(self.guest().checked_add(offset)?)}
    pub(crate) fn put<M:GuestAddressSpace,E>(self,offset:u32,value:u32,ctx:&mut RecordContext<M,E>)->Result<(),MemoryError> {ctx.write_u32(self.guest().checked_add(offset)?,value)}
    pub(crate) fn put_half<M:GuestAddressSpace,E>(self,offset:u32,value:u16,ctx:&mut RecordContext<M,E>)->Result<(),MemoryError> {ctx.write_u16(self.guest().checked_add(offset)?,value)}
    pub(crate) fn put_byte<M:GuestAddressSpace,E>(self,offset:u32,value:u8,ctx:&mut RecordContext<M,E>)->Result<(),MemoryError> {ctx.write_u8(self.guest().checked_add(offset)?,value)}
    pub(crate) fn clear<M:GuestAddressSpace,E>(self,ctx:&mut RecordContext<M,E>,offset:u32,length:u32)->Result<(),MemoryError> {for index in 0..length {self.put_byte(offset+index,0,ctx)?;}Ok(())}
}
pub(crate) use crate::record_configuration::helper_69f8fc as config;

pub fn helper_f49598<M:GuestAddressSpace,E:RecordJobEffects>(ctx:&mut RecordContext<M,E>,operation:u32,payload:GuestAddress,length:ByteCount,callbacks:&[CallbackId])->Result<(),JobError> {
    let source=ctx.effects.current_task() as u8;
    let destination=ctx.read_u32(GuestAddress::new(0x016c236c))?;
    let envelope=JobEnvelope {operation:operation as u16,source,destination:destination as u8,length};
    ctx.effects.submit_guest(&mut ctx.memory,envelope,payload,callbacks)
}
pub fn helper_f49c18<M:GuestAddressSpace,E:RecordJobEffects>(ctx:&mut RecordContext<M,E>,operation:u32,payload:GuestAddress,length:ByteCount,callbacks:&[CallbackId])->Result<(),JobError> {
    let envelope=JobEnvelope {operation:operation as u16,source:ctx.effects.current_task() as u8,destination:45,length};
    ctx.effects.submit_guest(&mut ctx.memory,envelope,payload,callbacks)
}
pub fn helper_f49598_owned<M:GuestAddressSpace,E:RecordJobEffects>(ctx:&mut RecordContext<M,E>,operation:u32,payload:&[u8],callbacks:&[CallbackId])->Result<(),JobError> {
    let source=ctx.effects.current_task() as u8;let destination=ctx.read_u32(GuestAddress::new(0x016c236c))? as u8;
    ctx.effects.submit_owned(JobEnvelope {operation:operation as u16,source,destination,length:ByteCount(payload.len() as u32)},payload,callbacks)
}

pub(crate) struct JobMessage(pub(crate) [u8;232]);
impl JobMessage {
    pub(crate) fn new()->Self {Self([0;232])}
    pub(crate) fn word(&mut self,offset:usize,value:u32) {self.0[offset..offset+4].copy_from_slice(&value.to_le_bytes());}
    pub(crate) fn copy<M:GuestAddressSpace,E>(&mut self,ctx:&mut RecordContext<M,E>,destination:usize,source:RecordAddress,length:usize)->Result<(),MemoryError> {
        for index in 0..length {self.0[destination+index]=source.byte(ctx,index as u32)?;}Ok(())
    }
    pub(crate) fn post<M:GuestAddressSpace,E:RecordJobEffects>(&self,ctx:&mut RecordContext<M,E>,operation:u32,callbacks:&[CallbackId])->Result<(),JobError> {helper_f49598_owned(ctx,operation,&self.0,callbacks)}
}
