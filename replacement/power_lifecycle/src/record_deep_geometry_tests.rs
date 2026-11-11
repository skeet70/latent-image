use super::*;
use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RecordContext, RegionDescriptor, RegionDomain, RegionPermission};

fn region(base:u32,bytes:&mut[u8])->MemoryRegion<'_>{
    MemoryRegion::new(RegionDescriptor::new(GuestAddress::new(base),ByteCount(bytes.len() as u32),RegionDomain::Shared,RegionPermission::ReadWrite).unwrap(),bytes).unwrap()
}

#[test]
fn missing_grid_input_preserves_padding_and_sets_unit_rectangle(){
    let mut bytes=[0xa5;16];let mut regions=[region(0x1000,&mut bytes)];
    let memory=HostAddressSpace::new(GuestDomain::A,&mut regions).unwrap();let mut context=RecordContext::new(memory,());
    helper_edeba0(&mut context,GuestAddress::new(0),GuestAddress::new(0),GuestAddress::new(0x1000)).unwrap();
    let expected=[0,0,1,0xa5,1,0,0,0,1,0xa5,1,0,0,0,0,0];
    for(index,value)in expected.into_iter().enumerate(){assert_eq!(context.read_u8(GuestAddress::new(0x1000+index as u32)).unwrap(),value);}
}

#[test]
fn tile_size_copy_reads_both_planes_and_keeps_surrounding_bytes(){
    let mut input=[0;80];let mut output=[0xa5;88];
    for index in 0..10{input[index*4+2..index*4+4].copy_from_slice(&(100+index as u16).to_le_bytes());input[42+index*4..44+index*4].copy_from_slice(&(200+index as u16).to_le_bytes());}
    let mut regions=[region(0x1000,&mut input),region(0x2000,&mut output)];let memory=HostAddressSpace::new(GuestDomain::A,&mut regions).unwrap();let mut context=RecordContext::new(memory,());
    helper_ee7e9c(&mut context,GuestAddress::new(0x1000),GuestAddress::new(0x2004)).unwrap();
    for index in 0..10{assert_eq!(context.read_u32(GuestAddress::new(0x2004+index*8)).unwrap(),200+index);assert_eq!(context.read_u32(GuestAddress::new(0x2008+index*8)).unwrap(),100+index);}
    assert_eq!(context.read_u32(GuestAddress::new(0x2000)).unwrap(),0xa5a5a5a5);assert_eq!(context.read_u32(GuestAddress::new(0x2054)).unwrap(),0xa5a5a5a5);
}

#[test]
fn square_crop_centers_with_real_profile_configuration(){
    let mut profile=[0;20];let mut scales=[0;48];scales[0x24]=1;scales[0x25]=1;
    let mut rectangle=[0;16];rectangle[8..12].copy_from_slice(&0x100u32.to_le_bytes());rectangle[12..16].copy_from_slice(&0x102u32.to_le_bytes());
    let mut configuration=[0;0x104];configuration[0x100..0x102].copy_from_slice(&480u16.to_le_bytes());configuration[0x102..0x104].copy_from_slice(&640u16.to_le_bytes());
    let mut pointer=0x1000u32.to_le_bytes();
    let mut regions=[region(0xef6de4,&mut profile),region(0xef6aa4,&mut scales),region(0xef71bc,&mut rectangle),region(0x1000,&mut configuration),region(0x96ca50,&mut pointer)];
    let memory=HostAddressSpace::new(GuestDomain::A,&mut regions).unwrap();let mut context=RecordContext::new(memory,());
    assert_eq!(transform_values(&mut context,[0,0,0,0,0x10001,0]).unwrap(),[1,1,480,640,81,1,480,480]);
}

#[test]
fn absent_tile_input_initializes_twenty_pairs_without_reading_selection(){
    let mut bytes=[0xa5;80];let mut counts=[0xa5;2];let mut regions=[region(0x1000,&mut bytes),region(0x2000,&mut counts)];
    let memory=HostAddressSpace::new(GuestDomain::A,&mut regions).unwrap();let mut context=RecordContext::new(memory,());
    helper_edfa7c(&mut context,GuestAddress::new(0),GuestAddress::new(0),GuestAddress::new(0x2000),GuestAddress::new(0x1000)).unwrap();
    for index in 0..20{assert_eq!(context.read_u32(GuestAddress::new(0x1000+index*4)).unwrap(),0x10000);}
    assert_eq!(context.read_u16(GuestAddress::new(0x2000)).unwrap(),0x101);
}
