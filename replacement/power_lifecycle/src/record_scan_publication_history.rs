use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
const STATE: u32 = 0x016c_af00;
const RECENT: u32 = 0x016c_59e4;
const REFERENCES: u32 = 0x016c_2c40;

pub(crate) fn byte<M: GuestAddressSpace, E>(c: &mut RecordContext<M,E>, at: u32) -> Result<u8,MemoryError> { c.read_u8(GuestAddress::new(at)) }
pub(crate) fn half<M: GuestAddressSpace, E>(c: &mut RecordContext<M,E>, at: u32) -> Result<u16,MemoryError> { c.read_u16(GuestAddress::new(at)) }
pub(crate) fn word<M: GuestAddressSpace, E>(c: &mut RecordContext<M,E>, at: u32) -> Result<u32,MemoryError> { c.read_u32(GuestAddress::new(at)) }
pub(crate) fn put_byte<M: GuestAddressSpace, E>(c: &mut RecordContext<M,E>, at: u32, value: u8) -> Result<(),MemoryError> { c.write_u8(GuestAddress::new(at),value) }
pub(crate) fn put_half<M: GuestAddressSpace, E>(c: &mut RecordContext<M,E>, at: u32, value: u16) -> Result<(),MemoryError> { c.write_u16(GuestAddress::new(at),value) }
pub(crate) fn put_word<M: GuestAddressSpace, E>(c: &mut RecordContext<M,E>, at: u32, value: u32) -> Result<(),MemoryError> { c.write_u32(GuestAddress::new(at),value) }
pub(crate) fn copy<M: GuestAddressSpace,E>(c: &mut RecordContext<M,E>, out:u32,input:u32,bytes:u32)->Result<(),MemoryError>{ c.copy_bytes(GuestAddress::new(out),GuestAddress::new(input),ByteCount(bytes)) }
pub(crate) fn fill<M: GuestAddressSpace,E>(c:&mut RecordContext<M,E>,out:u32,value:u8,bytes:u32)->Result<(),MemoryError>{ for offset in 0..bytes { put_byte(c,out+offset,value)?; } Ok(()) }
pub(crate) fn configuration_byte<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,key:u32)->Result<u8,MemoryError>{crate::record_configuration::read_byte(c,crate::record_configuration::ConfigurationKey(key))}
pub(crate) fn configuration_half<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,key:u32)->Result<u16,MemoryError>{crate::record_configuration::read_halfword(c,crate::record_configuration::ConfigurationKey(key))}

pub fn helper_f4a014<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,code:u32)->Result<(u32,u16),MemoryError>{
    if code>86 { return Ok((0,0)); }
    Ok((u32::from(byte(c,0x00f5_3404+code%29)?),(code/29) as u16))
}
pub fn helper_f4a070<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,mode:u32,flags:u32)->Result<u32,MemoryError>{
    if mode==0 { return Ok((flags&3)*29); }
    for index in 1..29 { if u32::from(byte(c,0x00f5_3404+index)?)==mode { return Ok((index+(flags&3)*29) as u8 as u32); } }
    Ok(0)
}
pub fn helper_f4d774<M:GuestAddressSpace,E>(_:&mut RecordContext<M,E>)->Result<GuestAddress,MemoryError>{Ok(GuestAddress::new(RECENT))}
pub fn helper_f4d790<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,mode:u32)->Result<(u32,u16),MemoryError>{
    let count=u32::from(configuration_byte(c,if mode==1{0x75df}else{0x75b1})?).clamp(1,20);
    let mut selected=0usize;
    if byte(c,RECENT+count-1)?!=100 {
        let mut counts=[0u8;87];
        for index in (0..count).rev(){ let value=byte(c,RECENT+index)? as usize; counts[value]=counts[value].wrapping_add(1); if counts[selected]<=counts[value]{selected=value;} }
    }
    helper_f4a014(c,selected as u32)
}
fn insert_identifier<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,at:u32,mut value:u8)->Result<(),MemoryError>{
    for index in 0..20 { let previous=byte(c,at+index)?; put_byte(c,at+index,value)?; if previous==100{break;} value=previous; } Ok(())
}
pub fn helper_f4d884<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{fill(c,RECENT,100,20)}
pub fn helper_f4d930<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{fill(c,0x016c_73b0,100,20)}
pub fn helper_f4dc18<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{fill(c,0x016c_5e0c,100,20)}
pub fn helper_f4d8bc<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,mode:u32,flags:u32)->Result<(),MemoryError>{
    let code=helper_f4a070(c,mode,flags)?; if code!=100{insert_identifier(c,RECENT,code as u8)?;} copy(c,0x016c_73b0,RECENT,20)
}
pub fn helper_f4d968<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,mode:u32,flags:u32)->Result<(),MemoryError>{
    let code=helper_f4a070(c,mode,flags)?;
    if code!=100{
        let mut grid=[0u16;256]; let source=word(c,STATE+0x2fc)?;
        for (index,value) in grid.iter_mut().enumerate(){*value=half(c,source+index as u32*2)?;}
        let mut identifier=code as u8; let mut level=half(c,STATE+0x504)?;
        let mut metadata=[u32::from(word(c,STATE+0x24)?!=0),word(c,STATE+0x50)?,word(c,STATE+0x510)?,word(c,STATE+0x514)?];
        for index in 0..20{
            let at=REFERENCES+index*0x214; let old_identifier=byte(c,at)?; put_byte(c,at,identifier)?;
            for (element,value) in grid.iter_mut().enumerate(){let cell=at+2+element as u32*2;let old=half(c,cell)?;put_half(c,cell,*value)?;*value=old;}
            let old=half(c,at+0x202)?;put_half(c,at+0x202,level)?;level=old;
            for(element,value)in metadata.iter_mut().enumerate(){let cell=at+0x204+element as u32*4;let old=word(c,cell)?;put_word(c,cell,*value)?;*value=old;}
            if old_identifier==100{break;}identifier=old_identifier;
        }
    }
    for index in 0..20{let value=byte(c,REFERENCES+index*0x214)?;put_byte(c,RECENT+index,value)?;}
    let mode=word(c,STATE+0x98c)?;let code=helper_f4a070(c,mode,0)?;
    if code!=100{insert_identifier(c,STATE+0x19b8,code as u8)?;}Ok(())
}
pub fn helper_f4dbac<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{fill(c,REFERENCES,0,0x2990)?;for index in 0..20{put_byte(c,REFERENCES+index*0x214,100)?;}fill(c,STATE+0x19b8,100,20)}
fn rotate<const N:usize,M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,history:u32,input:GuestAddress,entries:u32)->Result<(),MemoryError>{
    let mut previous=[0u8;N];for(offset,value)in previous.iter_mut().enumerate(){*value=byte(c,input.checked_add(offset as u32)?.get())?;}
    for entry in 0..entries{let at=history+entry*N as u32;for(offset,value)in previous.iter_mut().enumerate(){let cell=at+offset as u32;let old=byte(c,cell)?;put_byte(c,cell,*value)?;*value=old;}}Ok(())
}
pub fn helper_f4dc50<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,input:GuestAddress)->Result<(),MemoryError>{rotate::<36,_,_>(c,0x016c_2970,input,20)}
pub fn helper_f4dea4<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,input:GuestAddress)->Result<(),MemoryError>{rotate::<1104,_,_>(c,0x016c_5e20,input,5)}
pub fn helper_f4dd50<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,source:GuestAddress)->Result<(),MemoryError>{put_word(c,STATE+0x2fc,source.get())?;copy(c,STATE+0xba0,source.get(),512)}
pub fn helper_f4dd98<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,value:u16)->Result<(),MemoryError>{put_half(c,STATE+0x504,value)}
pub fn helper_f4ddbc<M:GuestAddressSpace,E>(_:&mut RecordContext<M,E>,mode:u32,flags:u32)->Result<u32,MemoryError>{Ok(u32::from(flags&2==0&&!matches!(mode,5|11|12|18|21|25|34|37|40)))}
pub fn helper_f4df30<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{fill(c,0x016c_5e20,0,0x1590)}
pub fn helper_f4df4c<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{
    fill(c,STATE,0,0x1a44)?;put_byte(c,STATE+0x51c,255)?;helper_f4d884(c)?;helper_f4d930(c)?;helper_f4dbac(c)?;helper_f4dc18(c)?;helper_f4df30(c)?;
    put_word(c,STATE+0x98c,58)?;put_word(c,STATE+0x988,1)?;put_word(c,STATE+0xb94,1)?;fill(c,0x5ffe_7854,0,0x1618)
}
