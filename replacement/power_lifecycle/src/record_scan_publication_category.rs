use crate::record_address_space::{GuestAddress,GuestAddressSpace,GuestDomain,MemoryError,RecordContext};
use crate::record_scan_publication_history::{half,word,put_word,configuration_byte};
use crate::record_scan_publication_workspace::{CachePublication,helper_fb27dc};
pub fn helper_fde08c<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,request:GuestAddress)->Result<(),MemoryError>{
    let request=request.get();put_word(c,request+0xd4,0)?;if word(c,request+0xd0)?!=1{return Ok(());}
    let category=word(c,request+4)?;
    let(category,initial)=match category{
        48=>(0,0),1|4=>(1,0),14|15=>(if configuration_byte(c,0x7951)?!=0{category-3}else{1},0),
        26|27=>(if configuration_byte(c,0x7952)?!=0{25}else{1},25),22|23=>(if configuration_byte(c,0x7953)?!=0{21}else{1},21),
        19|20=>(if configuration_byte(c,0x7954)?!=0{18}else{1},18),8|9=>(if configuration_byte(c,0x7955)?!=0{5}else{1},0),
        18|21|25|34|37|40=>(category,category),35|36=>(1,34),38|39=>(1,37),41|42=>(1,40),_=>return Ok(()),
    };put_word(c,request+4,category)?;put_word(c,request+0xd4,initial)
}
pub fn helper_fe0020<M:GuestAddressSpace,E>(_:&mut RecordContext<M,E>,category:u32,override_value:u32,variant:u32,enabled:u32)->Result<u32,MemoryError>{
    let mut selected=match category{
        1..=4=>3,0|43|44|57=>match variant{0=>28,1=>14,3=>13,4=>2,_=>1},45=>match variant{0=>30,1=>20,3=>22,4=>23,_=>21},
        46=>match variant{0=>29,1=>16,3=>18,4=>19,_=>17},47=>match variant{0=>31,1=>24,3=>26,4=>27,_=>25},
        10=>4,11=>15,16=>5,17=>6,18=>9,21=>8,25=>7,28=>10,30=>11,32=>12,51=>40,52=>41,_=>42,
    };if enabled!=0&&category==0{selected=0;}
    Ok(match override_value{1=>32,2=>33,3=>34,4=>35,5=>36,6=>37,7..=12=>39,14=>38,_=>selected})
}
pub fn helper_fe044c<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>,category:u32,override_value:u32,channel:u32,variant:u32,enabled:u32)->Result<u32,MemoryError>{
    let category=helper_fe0020(c,category,override_value,variant,enabled)?;if category>41{return Ok(10);}
    let key=half(c,0x00fe_0694+(category*3+channel)*2)?;let selection=configuration_byte(c,key as u32)?;
    if selection.wrapping_sub(1)>14{return Ok(10);}let key=half(c,0x00fe_0790+u32::from(selection-1)*6)?;Ok(u32::from(configuration_byte(c,key as u32)?))
}
pub fn helper_fe07ec<M:GuestAddressSpace,E>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{put_word(c,0x016e_f6d0,3)}
pub fn helper_fe3d08<M:GuestAddressSpace,E:CachePublication>(c:&mut RecordContext<M,E>)->Result<(),MemoryError>{
    if word(c,0x016e_f6d0)?==1{return Ok(());}put_word(c,0x016e_f6d0,2)?;helper_fb27dc(c,GuestAddress::new(0x016e_f6d4))?;
    let index=match c.domain(){GuestDomain::A=>0,GuestDomain::B=>1};let root=word(c,0x0096_bd20+index*4)?;let pointer=word(c,root+0x174)?;let ticks=word(c,pointer)?;put_word(c,0x016e_f800,ticks)?;helper_fe07ec(c)
}
