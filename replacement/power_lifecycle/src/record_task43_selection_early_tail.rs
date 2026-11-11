use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub(super) const STATE: GuestAddress = GuestAddress::new(0x016b5a40);
#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum Word { Handle=0, Mode=4, Current=8, Dimensions=0xc, Width=0x10, Height=0x14, Window0=0x18, Window1=0x1c, Window2=0x20, Window3=0x24, Selected=0x28, Reference=0x2c, Alternate=0x30, Saved=0x34, CachedReference=0x38, CachedAlternate=0x3c, Format=0x40, Policy=0x44, Option=0x48, Limit=0x4c, Baseline=0x54, Parameter0=0x58, Parameter1=0x5c, Parameter2=0x60, Parameter3=0x64, Kind=0x68, CommandEnabled=0x6c, Resource=0x70, CallbackEnabled=0x74, Value78=0x78, Value7c=0x7c, Value84=0x84, Value88=0x88, Value8c=0x8c, Value98=0x98, Value9c=0x9c, ValueA0=0xa0, Selection=0xa8, Global=0xac, CallbackOption=0xb0, ValueB4=0xb4, ValueB8=0xb8, Pending=0xbc, Count0=0xc4, Count1=0xc8, Result0=0xcc, Result1=0xd0, ValueD8=0xd8, ValueDc=0xdc, ValueE4=0xe4, ValueE8=0xe8 }
#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum Half { Value80=0x80, Value90=0x90, Value92=0x92, Value94=0x94, ValueA4=0xa4, Scale=0x50 }
#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum Byte { ValueA6=0xa6, FlagC0=0xc0, Weight=0xe0, Flags=0xec }

pub(super) fn r<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,field:Word)->Result<u32,MemoryError> { context.read_u32(STATE.checked_add(field as u32)?) }
pub(super) fn w<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,field:Word,value:u32)->Result<(),MemoryError> { context.write_u32(STATE.checked_add(field as u32)?,value) }
pub(super) fn h<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,field:Half)->Result<u16,MemoryError> { context.read_u16(STATE.checked_add(field as u32)?) }
pub(super) fn wh<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,field:Half,value:u32)->Result<(),MemoryError> { context.write_u16(STATE.checked_add(field as u32)?,value as u16) }
pub(super) fn b<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,field:Byte)->Result<u8,MemoryError> { context.read_u8(STATE.checked_add(field as u32)?) }
pub(super) fn wb<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,field:Byte,value:u32)->Result<(),MemoryError> { context.write_u8(STATE.checked_add(field as u32)?,value as u8) }

pub fn helper_f23314<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<(),MemoryError> { w(context,Word::CommandEnabled,1) }
pub fn helper_f234d8<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<u32,MemoryError> { r(context,Word::Resource) }
pub fn helper_f234f8<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<(),MemoryError> { w(context,Word::CallbackEnabled,1) }
pub fn helper_f23564<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,value:u32)->Result<(),MemoryError> { w(context,Word::ValueB8,value) }
pub fn helper_f23624<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<(),MemoryError> { w(context,Word::Pending,1) }
pub fn helper_f23648<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<u32,MemoryError> { r(context,Word::Pending) }
pub fn helper_f23668<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<u32,MemoryError> {
    let handle=context.read_u32(GuestAddress::new(0x016be01c))?;
    crate::record_profile_accessors::helper_74e020(context,handle,0,0x10000,0x80000000)
}
pub fn helper_f2368c<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,value:u32)->Result<(),MemoryError> { w(context,Word::Result0,value) }
pub fn helper_f236ac<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<u32,MemoryError> { r(context,Word::Result0) }
pub fn helper_f236cc<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,handle:u32)->Result<(),MemoryError> {
    w(context,Word::Handle,handle)?;
    w(context,Word::Pending,0)?;
    helper_f2368c(context,0)
}
pub fn helper_f23880<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,value:u32)->Result<(),MemoryError> { w(context,Word::Result1,value) }
pub fn helper_f238a0<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>)->Result<u32,MemoryError> { r(context,Word::Result1) }
