use crate::record_address_space::{GuestAddress, GuestAddressSpace};
use super::{Address, Result, aligned, crop_input, crop_values, div};
use super::profiles::{ConfigurationKey, ProfileGeometry};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grid {
    pub x:u16,pub y:u16,pub width:u16,pub height:u16,pub horizontal_gap:u16,pub vertical_gap:u16,pub columns:u8,pub rows:u8,
}
impl Grid {
    pub const fn unit()->Self{Self{x:0,y:0,width:1,height:1,horizontal_gap:0,vertical_gap:0,columns:1,rows:1}}
    pub fn store(self,memory:&mut impl GuestAddressSpace,output:GuestAddress)->Result<()>{
        let out=Address(output);
        for(offset,value)in[(0,self.x),(4,self.width),(6,self.y),(10,self.height),(12,self.horizontal_gap),(14,self.vertical_gap)]{out.at(offset)?.put_half(memory,value as u32)?;}
        out.at(2)?.put_byte(memory,self.columns as u32)?;out.at(8)?.put_byte(memory,self.rows as u32)
    }
}

pub fn values<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, input:[u32;6],selection:[u32;5]) -> Result<Grid> { let(rectangle,profile)=crop_values(memory,profiles,input,selection[3],selection[4])?;
let count=(selection[1]&0xffff).clamp(1,16);
let width=rectangle[7].wrapping_mul((selection[0]&0xffff).clamp(1,100))/100;
let height=rectangle[6].wrapping_mul((selection[0]>>16).clamp(1,100))/100;
let selected_x=selection[1]>>16;let selected_y=selection[2]&0xffff;
let mut gap_y=profiles.configuration_byte(memory,ConfigurationKey(0x243))?;
let max_x=div(width,count) as u16 as u32;let max_y=div(height.wrapping_sub((count-1).wrapping_mul(gap_y)),count) as u16 as u32;
let tile_x=if selected_x==0{max_x}else{(rectangle[7].wrapping_mul(selected_x)/100).min(max_x)};
let tile_y=if selected_y==0{max_y}else{(rectangle[6].wrapping_mul(selected_y)/100).min(max_y)};
let tile_x=aligned(tile_x,profile[4]).max(profile[4]);let tile_y=aligned(tile_y,profile[5]).max(profile[5]);
let used_x=count.wrapping_mul(tile_x);let used_y=count.wrapping_mul(tile_y);
let gap_x=if count>1&&selected_x!=0&&used_x<width{div(width-used_x,count-1)}else{0};
if count>1&&selected_y!=0&&used_y<height{gap_y=div(height-used_y,count-1);}
Ok(Grid{
    x:profile[0].wrapping_add(rectangle[3].wrapping_sub(used_x).wrapping_sub((count-1).wrapping_mul(gap_x))>>1) as u16,
    y:profile[1].wrapping_add(rectangle[2].wrapping_sub(used_y).wrapping_sub((count-1).wrapping_mul(gap_y))>>1) as u16,
    width:tile_x as u16,height:tile_y as u16,horizontal_gap:gap_x as u16,vertical_gap:gap_y as u16,columns:count as u8,rows:count as u8,
}) }
pub fn calculate<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters:GuestAddress,selection:[u32;5],output:GuestAddress) -> Result<()> { if output.get()==0{return Ok(());}
let grid=if parameters.get()==0{Grid::unit()}else{let input=crop_input(memory,Address(parameters))?;values(memory,profiles,input,selection)?};
grid.store(memory,output) }
