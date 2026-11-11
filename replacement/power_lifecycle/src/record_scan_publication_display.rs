use crate::record_address_space::{GuestAddressSpace,MemoryError,RecordContext};
use crate::record_scan_publication_history::byte;
pub trait SceneTextPublisher{
    fn draw_scene_text(&mut self,text:&[u8],column:u32,row:u32,font:u32,color:u32)->Result<(),MemoryError>;
}
pub fn helper_f4e058<M:GuestAddressSpace,E:SceneTextPublisher>(c:&mut RecordContext<M,E>,mode:u32,flags:u32)->Result<(),MemoryError>{
    let text=match mode{
        1=>0xf52c90,4=>0xf52ca8,5=>0xf52cc0,8=>0xf52cd8,9=>0xf52cf0,11=>0xf52d08,12=>0xf52d20,14=>0xf52d38,
        15=>0xf52d50,18=>0xf52d68,19=>0xf52d80,20=>0xf52d98,21=>0xf52db0,22=>0xf52dc8,23=>0xf52de0,25=>0xf52df8,
        26=>0xf52e10,27=>0xf52e28,34=>0xf52e40,35=>0xf52e58,36=>0xf52e70,37=>0xf52e88,38=>0xf52e9c,39=>0xf52eb0,
        40=>0xf52ec4,41=>0xf52ed8,42=>0xf52eec,48=>0xf52f00,_=>0xf52f18,
    };
    let mut output=[0u8;32];let mut length=0usize;
    loop{let value=byte(c,text+length as u32)?;if value==0{break;}output[length]=value;length+=1;}
    c.effects.draw_scene_text(&output[..length],2,8,2,9)?;
    output.fill(0);length=0;
    for(enabled,source)in[(flags&1!=0,0xf52f30),(flags&2!=0,0xf52f38),(flags==0,0xf52f40)]{
        if !enabled{continue;}let mut index=0;
        loop{let value=byte(c,source+index)?;if value==0{break;}output[length]=value;length+=1;index+=1;}
    }
    c.effects.draw_scene_text(&output[..length],2,9,2,9)
}
