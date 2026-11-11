use crate::record_address_space::{GuestAddress, GuestAddressSpace, RecordContext};
use super::profiles::ProfileGeometry as CalibrationProfiles;
use super::{Address, GeometryRequest, Result, SelectionLayout};
use super::profiles::{GeometryKind, ProfileId};

pub fn helper_eda4c4<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,flags:u32,parameters:GuestAddress,compact:GuestAddress,third:GuestAddress,fifth:GuestAddress,sixth:GuestAddress,report:GuestAddress)->Result<u32>{
    super::calculate_full(context,&mut CalibrationProfiles::new(),GeometryRequest{flags,parameters,compact,third,fifth,sixth,report})
}
pub fn helper_eda668<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,flags:u32,parameters:GuestAddress,compact:GuestAddress,third:GuestAddress,fifth:GuestAddress,sixth:GuestAddress,report:GuestAddress)->Result<u32>{
    super::calculate_extended(context,&mut CalibrationProfiles::new(),GeometryRequest{flags,parameters,compact,third,fifth,sixth,report})
}
pub fn helper_edeba0<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,selection:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_grid(context,&mut CalibrationProfiles::new(),parameters,selection,output)}
pub fn helper_edfa28<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,selection:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_selection(context,&mut CalibrationProfiles::new(),parameters,selection,output,SelectionLayout::Full)}
pub fn helper_edfeb4<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,selection:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_selection(context,&mut CalibrationProfiles::new(),parameters,selection,output,SelectionLayout::Compact)}
pub fn helper_ee0030<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,selection:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_selection(context,&mut CalibrationProfiles::new(),parameters,selection,output,SelectionLayout::Alternate)}
pub fn helper_edfa7c<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,selection:GuestAddress,counts:GuestAddress,output:GuestAddress)->Result<()>{super::tiles::calculate(context,&mut CalibrationProfiles::new(),parameters,selection,counts,output)}
pub fn helper_edff0c<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,selection:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_explicit(context,&mut CalibrationProfiles::new(),parameters,selection,output)}
pub fn helper_ee0084<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_simple(context,&mut CalibrationProfiles::new(),parameters,output,GeometryKind(0))}
pub fn helper_ee0104<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_simple(context,&mut CalibrationProfiles::new(),parameters,output,GeometryKind(1))}
pub fn helper_ee01b0<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,output:GuestAddress)->Result<()>{super::calculate_simple(context,&mut CalibrationProfiles::new(),parameters,output,GeometryKind(2))}
pub fn helper_ee7e9c<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,source:GuestAddress,output:GuestAddress)->Result<()>{super::copy_tile_sizes(context,source,output)}
pub fn helper_edeb24<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,profile:u32,output:GuestAddress)->Result<()>{
    let values=super::profile_geometry(context,&mut CalibrationProfiles::new(),ProfileId(profile))?;
    store_words(context,output,&values)
}
pub fn helper_ee7c98<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,kind:u32,profile:u32,output:GuestAddress)->Result<()>{
    if output.get()==0{return Ok(());}
    let values=super::select_rectangle(context,&mut CalibrationProfiles::new(),GeometryKind(kind),ProfileId(profile))?;
    store_words(context,output,&values)
}
pub fn helper_edea20<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,profile:u32,output:GuestAddress)->Result<()>{
    let mut values=load_words(context,output)?;
    super::normalize_rectangle(context,&mut CalibrationProfiles::new(),ProfileId(profile),&mut values)?;
    store_words(context,output,&values)
}
pub fn transform_values<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,input:[u32;6])->Result<[u32;8]>{
    super::crop_transform::transform(context,&mut CalibrationProfiles::new(),input)
}
pub fn helper_ef6030<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,input:GuestAddress,output:GuestAddress)->Result<()>{
    let values=load_words(context,input)?;
    let result=transform_values(context,values)?;
    store_words(context,output,&result)
}
pub fn helper_ef65cc<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,scale:u32,rectangle:GuestAddress)->Result<()>{
    let out=Address(rectangle);
    let denominator=out.at(8)?.word(context)?;
    if denominator==0{return Ok(());}
    let original=out.word(context)?;
    let mut values=[original,0,denominator,0,0];
    super::crop_transform::rescale(context,&mut CalibrationProfiles::new(),scale,&mut values)?;
    if values[0]!=original{out.put(context,values[0])?;}
    Ok(())
}
pub fn helper_ee02e4<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:GuestAddress,outer:GuestAddress,inner:GuestAddress)->Result<()>{
    let input=super::crop_input(context,Address(parameters))?;
    let values=super::crop_transform::transform(context,&mut CalibrationProfiles::new(),input)?;
    store_words(context,outer,&values[2..4])?;store_words(context,inner,&values[6..8])
}
fn load_words<const N:usize>(memory:&mut impl GuestAddressSpace,input:GuestAddress)->Result<[u32;N]>{
    let mut values=[0;N];for(index,value)in values.iter_mut().enumerate(){*value=Address(input).at(index as u32*4)?.word(memory)?;}Ok(values)
}
fn store_words(memory:&mut impl GuestAddressSpace,output:GuestAddress,values:&[u32])->Result<()>{
    for(index,value)in values.iter().enumerate(){Address(output).at(index as u32*4)?.put(memory,*value)?;}Ok(())
}

fn half(bytes:&[u8],offset:usize)->u32{u16::from_le_bytes([bytes[offset],bytes[offset+1]]) as u32}
fn word(bytes:&[u8],offset:usize)->u32{u32::from_le_bytes([bytes[offset],bytes[offset+1],bytes[offset+2],bytes[offset+3]])}
fn put_word(bytes:&mut[u8],offset:usize,value:u32){bytes[offset..offset+4].copy_from_slice(&value.to_le_bytes());}
fn input_values(bytes:&[u8])->[u32;6]{[word(bytes,0),word(bytes,16),word(bytes,20),word(bytes,24),half(bytes,30)|half(bytes,32)<<16,1]}

pub fn calculate_entry_report<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,parameters:&[u8;72],compact:&[u8;12],report:&mut[u8;208])->Result<()>{
    let input=input_values(parameters);
    let selection=[word(compact,0),half(compact,4)|half(compact,6)<<16,half(compact,8),0,0];
    let grid=super::grid_values::values(context,&mut CalibrationProfiles::new(),input,selection)?;
    grid.store(context,GuestAddress::new(0x169974c))?;
    super::calculate_simple_values(context,&mut CalibrationProfiles::new(),ProfileId(input[0]),GuestAddress::new(0x169981c),GeometryKind(0))?;
    for(source,destination)in[(280,4),(286,0),(184,12),(190,8),(376,44),(382,40),(390,20),(394,16),(400,28),(404,24),(408,36),(414,32)]{
        let value=Address::new(0x1699698).at(source)?.half(context)?;put_word(report,destination,value);
    }
    for(source,destination)in[(0x169975c,128),(0x16997bc,48)]{
        for index in 0..10{let input=Address::new(source).at(index*4)?;put_word(report,destination+index as usize*8+4,input.at(2)?.half(context)?);put_word(report,destination+index as usize*8,input.at(42)?.half(context)?);}
    }Ok(())
}

pub fn calculate_extended_values<M:GuestAddressSpace,E>(context:&mut RecordContext<M,E>,input:&[u8;34],config:&[u8;10],report:&mut[u8;184])->Result<u32>{
    let parameters=input_values(input);
    let selection=[word(config,0),half(config,4)|half(config,6)<<16,half(config,8),0,0];
    let grid=super::grid_values::values(context,&mut CalibrationProfiles::new(),parameters,selection)?;
    grid.store(context,GuestAddress::new(0x169974c))?;
    for(offset,value)in[(68,grid.width as u32),(64,grid.height as u32),(72,grid.x as u32),(76,grid.y as u32)]{put_word(report,offset,value);}
    report[80..82].copy_from_slice(&grid.horizontal_gap.to_le_bytes());report[82..84].copy_from_slice(&grid.vertical_gap.to_le_bytes());report[84]=grid.columns;report[85]=grid.rows;
    let rectangle=super::crop_transform::transform(context,&mut CalibrationProfiles::new(),parameters)?;
    for(source,destination)in[(3,4),(2,0),(7,12),(6,8)]{put_word(report,destination,rectangle[source]);}Ok(0)
}
