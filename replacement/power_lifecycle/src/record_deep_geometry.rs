use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError};
#[path = "record_deep_geometry_profiles.rs"]
pub mod profiles;
#[path = "record_deep_geometry_crop.rs"]
pub mod crop_transform;
#[path = "record_deep_geometry_tiles.rs"]
pub mod tiles;
#[path = "record_deep_geometry_context.rs"]
mod context;
pub use context::*;
use profiles::{ConfigurationKey, GeometryKind, ProfileGeometry, ProfileId};
type Result<T, E = MemoryError> = core::result::Result<T, E>;

#[derive(Clone, Copy)]
struct Address(GuestAddress);
impl Address {
    const fn new(value: u32) -> Self { Self(GuestAddress::new(value)) }
    fn at(self, offset: u32) -> Result<Self> { Ok(Self(self.0.checked_add(offset)?)) }
    fn word(self, memory: &mut impl GuestAddressSpace) -> Result<u32> { memory.read_u32(self.0) }
    fn half(self, memory: &mut impl GuestAddressSpace) -> Result<u32> { Ok(memory.read_u16(self.0)? as u32) }
    fn byte(self, memory: &mut impl GuestAddressSpace) -> Result<u32> { Ok(memory.read_u8(self.0)? as u32) }
    fn put(self, memory: &mut impl GuestAddressSpace, value: u32) -> Result<()> { memory.write_u32(self.0, value) }
    fn put_half(self, memory: &mut impl GuestAddressSpace, value: u32) -> Result<()> { memory.write_u16(self.0, value as u16) }
    fn put_byte(self, memory: &mut impl GuestAddressSpace, value: u32) -> Result<()> { memory.write_u8(self.0, value as u8) }
}
fn div(a: u32, b: u32) -> u32 { a.checked_div(b).unwrap_or(0) }
fn aligned(a: u32, b: u32) -> u32 { a.wrapping_sub(a.checked_rem(b).unwrap_or(a)) }

pub fn profile_geometry<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, profile: ProfileId) -> Result<[u32; 6]> { let rectangle = profiles.rectangle(memory, profile, GeometryKind(1))?;
let horizontal = profiles.horizontal_scale(memory, profile, GeometryKind(1))?;
let vertical = profiles.vertical_scale(memory, profile, GeometryKind(1))?;
let alignment = profiles.alignment(memory, profile)?;
Ok([rectangle[0], rectangle[1], horizontal, vertical, alignment[1], alignment[0]]) }

fn crop_input(memory: &mut impl GuestAddressSpace, parameters: Address) -> Result<[u32; 6]> {
    let low = parameters.at(30)?.half(memory)?;
    let high = parameters.at(32)?.half(memory)?;
    Ok([parameters.word(memory)?, parameters.at(16)?.word(memory)?, parameters.at(20)?.word(memory)?, parameters.at(24)?.word(memory)?, low | high << 16, 1])
}
fn crop<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters: Address, first_scale: u32, second_scale: u32) -> Result<([u32; 8], [u32; 6])> { let input = crop_input(memory, parameters)?;
crop_values(memory, profiles, input, first_scale, second_scale) }
fn crop_values<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, mut input: [u32;6], first_scale: u32, second_scale: u32) -> Result<([u32;8],[u32;6])> { if second_scale != 0 || first_scale != 0 {
    let scale = profiles.configuration_byte(memory, ConfigurationKey(if second_scale != 0 {0x879b} else {0x926c}))?;
    let mut rectangle = [input[1], input[2], input[3], input[4], input[5]];
    crop_transform::rescale(memory, profiles, scale, &mut rectangle)?;
    input[1..].copy_from_slice(&rectangle);
}
let rectangle = crop_transform::transform(memory, profiles, input)?;
let profile = profile_geometry(memory, profiles, ProfileId(input[0]))?;
Ok((rectangle, profile)) }

pub fn calculate_grid<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters: GuestAddress, selection: GuestAddress, output: GuestAddress) -> Result<()> { if output.get() == 0 { return Ok(()); }
let mut values = [0; 5];
if parameters.get() != 0 && selection.get() != 0 {
    for (index, value) in values.iter_mut().enumerate() { *value = Address(selection).at(index as u32 * 4)?.word(memory)?; }
    grid_values::calculate(memory, profiles, parameters, values, output)
} else {
    grid_values::calculate(memory, profiles, GuestAddress::new(0), values, output)
} }

#[derive(Clone, Copy)]
pub enum SelectionLayout { Full, Compact, Alternate }
fn selection_words(memory: &mut impl GuestAddressSpace, source: Address, variant: SelectionLayout) -> Result<[u32;5]> {
    let (first, second, third, scale_a, scale_b) = match variant {
        SelectionLayout::Compact => (source.word(memory)?, source.at(4)?.half(memory)?, source.at(6)?.half(memory)?, 0, 0),
        SelectionLayout::Full => (source.at(4)?.word(memory)?, source.at(8)?.word(memory)? & 0xffff, source.at(12)?.half(memory)?, source.at(52)?.word(memory)?, source.at(56)?.word(memory)?),
        SelectionLayout::Alternate => (source.at(4)?.word(memory)?, source.at(8)?.word(memory)? & 0xffff, source.at(10)?.half(memory)?, source.at(16)?.word(memory)?, source.at(20)?.word(memory)?),
    };
    let fourth = match variant {SelectionLayout::Compact => source.at(8)?.half(memory)?, SelectionLayout::Full => source.at(14)?.half(memory)?, SelectionLayout::Alternate => source.at(12)?.half(memory)?};
    Ok([first,second | third << 16,fourth,scale_a,scale_b])
}

pub fn calculate_selection<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters: GuestAddress, source: GuestAddress, output: GuestAddress, variant: SelectionLayout) -> Result<()> { let selection = selection_words(memory, Address(source), variant)?;
calculate_grid_values(memory, profiles, parameters, selection, output) }

fn calculate_grid_values<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters: GuestAddress, selection: [u32;5], output: GuestAddress) -> Result<()> { grid_values::calculate(memory, profiles, parameters, selection, output) }
#[path = "record_deep_geometry_grid.rs"]
mod grid_values;

pub fn normalize_rectangle<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, profile: ProfileId, rectangle: &mut [u32;4]) -> Result<()> { let full = profiles.full_rectangle(memory, profile)?;
if full[3] < rectangle[0].wrapping_add(rectangle[3]) {rectangle[0]=0;rectangle[3]=full[3];}
if full[2] < rectangle[1].wrapping_add(rectangle[2]) {rectangle[1]=0;rectangle[2]=full[2];}
let active=profiles.active_rectangle(memory,profile)?;
if rectangle[0]>=active[0] {rectangle[0]-=active[0];}
if rectangle[1]>=active[1] {rectangle[1]-=active[1];}
let original_x=rectangle[0];let original_width=rectangle[3];
let divisor=profiles.horizontal_divisor(memory,profile)?;
rectangle[0]=div(original_x,divisor);rectangle[3]=div(original_width,divisor)&!1;rectangle[2]&=!1;
if original_x.checked_rem(divisor.wrapping_mul(2)).unwrap_or(0)!=0 && original_width!=rectangle[3]{rectangle[0]=rectangle[0].wrapping_add(1);}
Ok(()) }

pub fn select_rectangle<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, kind: GeometryKind, profile: ProfileId) -> Result<[u32;4]> { let row=profiles.profile_row(memory,profile)?;
let base=match kind.0 {1=>0xee8764,2=>0xee8864,_=>0xee8664};
let mut result=[0;4];
for offset in [8,12,0,4] {
    let key=if row.wrapping_sub(16)<=1 {0}else{Address::new(base).at(row.wrapping_mul(16))?.at(offset)?.word(memory)?};
    result[offset as usize/4]=if key==0{1}else{profiles.configuration_half(memory,ConfigurationKey(key))?};
}
Ok(result) }

pub fn calculate_simple<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters: GuestAddress, output: GuestAddress, kind: GeometryKind) -> Result<()> {
    if output.get()==0{return Ok(());}
    if parameters.get()==0{return initialize_simple(memory,output,kind);}
    let profile=ProfileId(Address(parameters).word(memory)?);
    calculate_simple_values(memory,profiles,profile,output,kind)
}
fn initialize_simple(memory:&mut impl GuestAddressSpace,output:GuestAddress,kind:GeometryKind)->Result<()> {
    let out=Address(output);
    for offset in if kind.0==0{&[0,2,4,6][..]}else if kind.0==1{&[0,4,6,8][..]}else{&[0,2,4,8][..]}{out.at(*offset)?.put_half(memory,1)?;}
    if kind.0==1{out.at(2)?.put_byte(memory,1)?;}else if kind.0==2{out.at(6)?.put_byte(memory,1)?;}
    Ok(())
}
fn calculate_simple_values<M:GuestAddressSpace,E>(memory:&mut crate::record_address_space::RecordContext<M,E>,profiles:&mut ProfileGeometry,profile:ProfileId,output:GuestAddress,kind:GeometryKind)->Result<()> {
    if output.get()==0{return Ok(());}
    initialize_simple(memory,output,kind)?;
    let out=Address(output);
let mut rectangle=select_rectangle(memory,profiles,kind,profile)?;
normalize_rectangle(memory,profiles,profile,&mut rectangle)?;
if rectangle[3]==0{return Ok(());}
if kind.0==0{for(offset,value)in[(0,rectangle[0]),(2,rectangle[3]),(4,rectangle[1]),(6,rectangle[2])]{out.at(offset)?.put_half(memory,value)?;}}
else if kind.0==1{
    let width=(rectangle[3]>>4)&0xfffe;if width==0{return Ok(());}
    out.put_half(memory,rectangle[0].wrapping_add((rectangle[3].wrapping_sub(width<<4)>>1)&0xffff))?;
    out.at(4)?.put_half(memory,width)?;out.at(2)?.put_byte(memory,16)?;out.at(6)?.put_half(memory,rectangle[1])?;out.at(8)?.put_half(memory,rectangle[2])?;
}else{
    let height=(rectangle[2]>>4)&0xfffe;if height==0{return Ok(());}
    out.put_half(memory,rectangle[0])?;out.at(2)?.put_half(memory,rectangle[3])?;
    out.at(4)?.put_half(memory,rectangle[1].wrapping_add((rectangle[2].wrapping_sub(height<<4)>>1)&0xffff))?;out.at(8)?.put_half(memory,height)?;out.at(6)?.put_byte(memory,16)?;
}
Ok(()) }

pub fn calculate_explicit<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters:GuestAddress,selections:GuestAddress,output:GuestAddress) -> Result<()> { let out=Address(output);for index in 0..20{out.at(index*4)?.put_half(memory,1)?;out.at(index*4+2)?.put_half(memory,1)?;}
if parameters.get()==0{return Ok(());}
let profile=ProfileId(Address(parameters).word(memory)?);let count=Address(selections).byte(memory)?.min(10);
let alignment=profiles.alignment(memory,profile)?;let rectangle=profiles.rectangle(memory,profile,GeometryKind(1))?;
for index in 0..count{
    let source=Address(selections).at(index*16)?;let target=out.at(index*4)?;
    let x=rectangle[0].wrapping_add(source.at(4)?.word(memory)?);target.put_half(memory,x)?;
    let width=aligned(source.at(16)?.word(memory)?,alignment[1]).max(alignment[1]);target.at(2)?.put_half(memory,width)?;
    let y=rectangle[1].wrapping_add(source.at(8)?.word(memory)?);target.at(40)?.put_half(memory,y)?;
    let height=aligned(source.at(12)?.word(memory)?,alignment[0]).max(alignment[0]);target.at(42)?.put_half(memory,height)?;
}Ok(()) }

pub fn copy_tile_sizes(memory:&mut impl GuestAddressSpace,source:GuestAddress,output:GuestAddress)->Result<()> {
    for index in 0..10{let input=Address(source).at(index*4)?;let out=Address(output).at(index*8)?;
        let width=input.at(2)?.half(memory)?;out.at(4)?.put(memory,width)?;
        let height=input.at(42)?.half(memory)?;out.put(memory,height)?;
    }Ok(())
}

#[derive(Clone, Copy)]
pub struct GeometryRequest {
    pub flags:u32,pub parameters:GuestAddress,pub compact:GuestAddress,pub third:GuestAddress,pub fifth:GuestAddress,pub sixth:GuestAddress,pub report:GuestAddress,
}
fn calculate_requested<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, request:GeometryRequest,extended:bool) -> Result<()> { let base=Address::new(0x1699698);let out=Address(request.report);
for (mask,source,destination,layout)in[(1,request.third,0x16997ac,SelectionLayout::Full),(2,request.compact,0x169974c,SelectionLayout::Compact),(32,request.fifth,0x169980c,SelectionLayout::Alternate)]{
    if request.flags&mask!=0{calculate_selection(memory,profiles,request.parameters,source,GuestAddress::new(destination),layout)?;
        if extended{let offset=match mask{1=>16,2=>64,_=>88};copy_grid(memory,base.at(destination-0x1699698)?,out.at(offset)?)?;}
    }
}
for(mask,kind,destination,offset)in[(4,0,0x169981c,112),(8,1,0x1699824,136),(16,2,0x169982e,160)]{
    if request.flags&mask!=0{calculate_simple(memory,profiles,request.parameters,GuestAddress::new(destination),GeometryKind(kind))?;
        if extended{copy_simple(memory,Address::new(destination),out.at(offset)?,kind)?;}
    }
}
if request.flags&64!=0&&!extended{calculate_explicit(memory,profiles,request.parameters,request.sixth,GuestAddress::new(0x169975c))?;}
if request.flags&128!=0{
    tiles::calculate(memory,profiles,request.parameters,request.third,if extended{out.at(60)?.0}else{GuestAddress::new(0)},GuestAddress::new(0x16997bc))?;
    if extended{
        for(source,destination)in[(294,44),(334,40),(292,48),(332,52)]{let value=base.at(source)?.half(memory)?;out.at(destination)?.put(memory,value)?;}
        out.at(56)?.put_half(memory,0)?;out.at(58)?.put_half(memory,0)?;
    }
}Ok(()) }
fn copy_grid(memory:&mut impl GuestAddressSpace,source:Address,destination:Address)->Result<()> {
    for(input,output)in[(4,4),(10,0),(0,8),(6,12)]{let value=source.at(input)?.half(memory)?;destination.at(output)?.put(memory,value)?;}
    for(input,output)in[(12,16),(14,18)]{let value=source.at(input)?.half(memory)?;destination.at(output)?.put_half(memory,value)?;}
    for(input,output)in[(2,20),(8,21)]{let value=source.at(input)?.byte(memory)?;destination.at(output)?.put_byte(memory,value)?;}
    Ok(())
}
fn copy_simple(memory:&mut impl GuestAddressSpace,source:Address,destination:Address,kind:u32)->Result<()> {
    let offsets=if kind==0{[6,2,0,4]}else if kind==1{[8,4,0,6]}else{[8,2,0,4]};
    for(index,input)in offsets.into_iter().enumerate(){let value=source.at(input)?.half(memory)?;destination.at(index as u32*4)?.put(memory,value)?;}
    destination.at(16)?.put_half(memory,0)?;destination.at(18)?.put_half(memory,0)?;
    destination.at(20)?.put_byte(memory,if kind==1{16}else{1})?;destination.at(21)?.put_byte(memory,if kind==2{16}else{1})
}
pub fn calculate_full<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, request:GeometryRequest) -> Result<u32> { calculate_requested(memory,profiles,request,false)?;
for(source,destination)in[(280,4),(286,0),(184,12),(190,8),(376,44),(382,40),(390,20),(394,16),(400,28),(404,24),(408,36),(414,32)]{let value=Address::new(0x1699698).at(source)?.half(memory)?;Address(request.report).at(destination)?.put(memory,value)?;}
copy_tile_sizes(memory,GuestAddress::new(0x169975c),request.report.checked_add(128)?)?;
copy_tile_sizes(memory,GuestAddress::new(0x16997bc),request.report.checked_add(48)?)?;Ok(0) }
pub fn calculate_extended<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, request:GeometryRequest) -> Result<u32> { calculate_requested(memory,profiles,request,true)?;
let input=crop_input(memory,Address(request.parameters))?;let rectangle=crop_transform::transform(memory,profiles,input)?;
for(source,destination)in[(3,4),(2,0),(7,12),(6,8)]{Address(request.report).at(destination)?.put(memory,rectangle[source])?;}
Ok(0) }

#[cfg(test)]
#[path = "record_deep_geometry_tests.rs"]
mod tests;
