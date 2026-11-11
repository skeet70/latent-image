use crate::record_address_space::{GuestAddress, GuestAddressSpace};
use super::{Address, Result, aligned, crop, div};
use super::profiles::{ConfigurationKey, ProfileGeometry};

#[derive(Clone, Copy)]
enum Direction { Up, Down, Left, Right, Other }
impl From<u32> for Direction {
    fn from(value:u32)->Self {match value {0=>Self::Up,2=>Self::Left,3=>Self::Right,1=>Self::Down,_=>Self::Other}}
}

pub fn calculate<M: GuestAddressSpace, E>(memory: &mut crate::record_address_space::RecordContext<M,E>, profiles: &mut ProfileGeometry, parameters: GuestAddress, selection: GuestAddress, counts: GuestAddress, output: GuestAddress) -> Result<()> { if output.get()==0{return Ok(());}
let out=Address(output);let count_out=Address(counts);
if counts.get()!=0{count_out.put_byte(memory,1)?;count_out.at(1)?.put_byte(memory,1)?;}
for index in 0..10{let target=out.at(index*4)?;target.put_half(memory,0)?;target.at(2)?.put_half(memory,1)?;target.at(40)?.put_half(memory,0)?;target.at(42)?.put_half(memory,1)?;}
if parameters.get()==0{return Ok(());}
let select=Address(selection);if select.at(44)?.word(memory)?==0{return Ok(());}
let scale_a=select.at(52)?.word(memory)?;let scale_b=select.at(56)?.word(memory)?;
let (rectangle,profile)=crop(memory,profiles,Address(parameters),scale_a,scale_b)?;
let origin_x=rectangle[3].wrapping_sub(rectangle[7])>>1;let origin_y=rectangle[2].wrapping_sub(rectangle[6])>>1;
let mut x=select.at(24)?.word(memory)?;let mut y=select.at(28)?.word(memory)?;
let mut width=aligned(select.at(36)?.word(memory)?,profile[4]);let mut height=aligned(select.at(32)?.word(memory)?,profile[5]);
if profiles.configuration_byte(memory,ConfigurationKey(0xa485))?>width{
    let minimum=profiles.configuration_byte(memory,ConfigurationKey(0xa485))?;
    height=div(height.wrapping_mul(minimum),width);width=profiles.configuration_byte(memory,ConfigurationKey(0xa485))?;
    x=select.at(16)?.word(memory)?.saturating_sub(width>>1);y=select.at(20)?.word(memory)?.saturating_sub(height>>1);
}else if width>512{width=512;x=select.at(16)?.word(memory)?.saturating_sub(256);}
width=width.max(profile[4]);if profile[5]>height{height=profile[4];}
x=x.max(origin_x);y=y.max(origin_y);
let right=origin_x.wrapping_add(rectangle[7]);let bottom=origin_y.wrapping_add(rectangle[6]);
if width.wrapping_add(x)>right{
    if right>=x.wrapping_add(profile[4]){width=right.wrapping_sub(x);}else{x=right.saturating_sub(profile[4]);width=profile[4];}
}
if height.wrapping_add(y)>bottom{
    if bottom>=y.wrapping_add(profile[5]){height=bottom.wrapping_sub(y);}else{y=bottom.saturating_sub(profile[5]);height=profile[5];}
}
let direction=Direction::from(select.at(40)?.word(memory)?);
let(horizontal,count_distance)=match direction{
    Direction::Left=>(true,x.wrapping_sub(origin_x)),Direction::Right=>(true,right.wrapping_sub(x.wrapping_add(width))),
    Direction::Up=>(false,y.wrapping_sub(origin_y)),Direction::Down|Direction::Other=>(false,bottom.wrapping_sub(y.wrapping_add(height))),
};
let count=((div(count_distance,if horizontal{width}else{height}).wrapping_add(1) as u8) as u32).clamp(1,10);
let(columns,rows)=if horizontal{(count,1)}else{(1,count)};
if matches!(direction,Direction::Up|Direction::Left){x=x.wrapping_sub(width.wrapping_mul(columns-1));y=y.wrapping_sub(height.wrapping_mul(rows-1));}
let mut x=(x as u16).wrapping_add(profile[0] as u16);let initial_y=(y as u16).wrapping_add(profile[1] as u16);let mut index=0;
for _ in 0..columns{
    let mut y=initial_y;
    for _ in 0..rows{
        let target=out.at(index*4)?;target.put_half(memory,x as u32)?;target.at(2)?.put_half(memory,width)?;
        target.at(40)?.put_half(memory,y as u32)?;target.at(42)?.put_half(memory,height)?;
        index+=1;y=y.wrapping_add(height as u16);
    }x=x.wrapping_add(width as u16);
}
if counts.get()!=0{count_out.put_byte(memory,columns)?;count_out.at(1)?.put_byte(memory,rows)?;}
Ok(()) }
