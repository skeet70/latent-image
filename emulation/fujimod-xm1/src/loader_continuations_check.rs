use super::{Access, CallKind, Catalog, Core, Event, Guest, Key, RawWatch, ReleaseOutcome, Report};
use super::continuation_view::{Digest, FileBinding, Mapping, Resource, Words};
use super::loader_orchestration_owned::{Program, Service, Side};
use fujimod_xm1::bridge::read;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, fs::File, io::Read, path::{Component, Path}};

#[path="loader_reset_check.rs"]
mod loader_reset_check;
const CANONICAL_RUN:&[u8]=include_bytes!("../data/xm1-150.json");

#[derive(Deserialize)]
#[serde(try_from="String")]
struct HexWord(u32);
impl TryFrom<String> for HexWord {
    type Error=String;
    fn try_from(value:String)->Result<Self,String>{Ok(Self(u32::from_str_radix(value.strip_prefix("0x").ok_or("hex word prefix absent")?,16).map_err(|error|error.to_string())?))}
}
#[derive(PartialEq,Eq,PartialOrd,Ord,Deserialize)]
#[serde(try_from="String")]
struct RegisterNumber(u8);
impl TryFrom<String> for RegisterNumber {
    type Error=String;
    fn try_from(value:String)->Result<Self,String>{let number=value.strip_prefix('r').ok_or("general-register name absent")?.parse::<u8>().map_err(|error|error.to_string())?;if number>12{return Err("register outside captured set".into());}Ok(Self(number))}
}
#[derive(Deserialize)]
struct Hit {address:HexWord,core:Core,instructions:super::Count,cpsr:u32,registers:BTreeMap<RegisterNumber,HexWord>,sp:HexWord,lr:HexWord}
#[derive(Serialize)]
#[serde(rename_all="kebab-case")]
enum Scope { CanonicalNaturalObservation, OwnedNaturalObservation }
#[derive(Serialize)]
pub(super) struct Verification {schema:u32,scope:Scope,receipt:Digest,events:usize,loader_calls:usize,task_calls:usize,snapshots:usize,source_differences:usize}
fn require(condition:bool,message:&str)->Result<(),String>{if condition{Ok(())}else{Err(message.into())}}
fn artifact(root:&Path,binding:&FileBinding)->Result<std::path::PathBuf,String>{
    let mut components=binding.path.components();require(matches!(components.next(),Some(Component::Normal(_)))&&components.next().is_none(),"artifact is not a single relative filename")?;Ok(root.join(&binding.path))
}
fn mapping(value:&Mapping)->Result<(),String>{
    require(value.bytes.0>0 && u64::from(value.guest.0)+u64::from(value.bytes.0)<=1u64<<32,"projection extent differs")?;
    require(value.physical.0==value.guest.0&!0x40000000,"projection physical translation differs")?;
    for bank in value.tcm.into_iter().flatten(){if bank&1==0{continue;}let size=(bank>>2)&31;if size==0{continue;}require((3..=14).contains(&size),"projection has a reserved TCM size")?;let bytes=512u64<<size;let low=u64::from(bank&0xfffff000);let end=low+bytes;require(low%bytes==0&&end<=1u64<<32,"projection TCM window has invalid alignment or endpoint")?;require(end<=u64::from(value.guest.0)||u64::from(value.guest.0)+u64::from(value.bytes.0)<=low,"projection intersects TCM")?;}
    Ok(())
}
fn words(value:&Words)->Result<(),String>{mapping(&value.mapping)?;require(value.mapping.bytes.0 as usize==4*value.values.len()&&value.guest_equal==[true;2],"word projection lacks equal committed views")}
fn at(event:&Event,address:u32)->Result<u32,String>{
    for view in &event.state {if let Some(offset)=address.checked_sub(view.mapping.guest.0){if offset%4==0{if let Some(value)=view.values.get((offset/4) as usize){return Ok(*value);}}}}
    Err(format!("committed scalar {address:#x} absent"))
}
fn branch(event:&Event,catalog:&Catalog)->Result<Guest,String>{
    let instruction=catalog.word(event.pc)?.1;
    if instruction>>24==0xeb {let immediate=i64::from(instruction&0xffffff);let signed=if immediate>=0x800000{immediate-0x1000000}else{immediate};let target=i64::from(event.pc.0)+8+4*signed;return Ok(Guest(u32::try_from(target).map_err(|_|"branch target overflow")?));}
    require(instruction&0xfffffff0==0xe12fff30,"call boundary is not BL or BLX register")?;
    let register=(instruction&15) as usize;require(register<13,"call register outside captured set")?;Ok(Guest(event.registers.r[register]))
}
fn projected_frame(event:&Event)->Result<&[u32],String>{
    let frame=event.frame.as_ref().map_err(|_|"loader caller frame was not projected")?;words(frame)?;
    require(frame.mapping.guest.0.checked_add(64)==Some(event.registers.sp.0)&&frame.values.len()==20,"loader frame extent differs")?;Ok(&frame.values[16..20])
}
fn snapshot(root:&Path,sample:&super::Snapshot,catalog:&Catalog)->Result<(),String>{
    mapping(&sample.mapping)?;let (start,end)=sample.resource.extent();require(sample.mapping.guest.0==start&&sample.mapping.bytes.0==end-start,"resource projection extent differs")?;
    let expected=catalog.expected(sample.resource)?;let mut first_difference=None;let mut digests=Vec::with_capacity(3);
    for (index,binding) in sample.files.iter().enumerate(){
        let mut file=File::open(artifact(root,binding)?).map_err(|error|error.to_string())?;require(file.metadata().map_err(|error|error.to_string())?.len()==u64::from(end-start),"projection file extent differs")?;
        let mut hasher=Sha256::new();let mut offset=0usize;loop{let mut bytes=[0;4096];let count=file.read(&mut bytes).map_err(|error|error.to_string())?;if count==0{break;}require(offset.checked_add(count).is_some_and(|next|next<=(end-start) as usize),"projection grew during consumption")?;hasher.update(&bytes[..count]);if index==2 && first_difference.is_none(){first_difference=bytes[..count].iter().enumerate().find(|(inside,value)|**value!=expected.map_or(0,|expected|expected[offset+inside])).map(|(inside,_)|Guest(start+(offset+inside) as u32));}offset+=count;}
        let digest=format!("{:x}",hasher.finalize());require(digest==binding.sha256.as_str(),"projection file identity differs")?;digests.push(digest);
    }
    require(sample.guest_equal==[true;2]&&digests[0]==digests[2]&&digests[1]==digests[2],"resource guest and physical bytes differ")?;
    require(sample.expected_equal==first_difference.is_none()&&sample.first_difference==first_difference,"resource/source comparison differs")?;
    if matches!(sample.resource,Resource::LoaderA|Resource::LoaderB){require(sample.expected_equal,"loader carrier differs from canonical source")?;}
    Ok(())
}
fn event<'a>(report:&'a Report,id:super::EventId)->Result<&'a Event,String>{report.events.get(id.0).filter(|event|event.id.0==id.0).ok_or_else(||"call event reference absent".into())}
fn in_class(row:&super::Row,classes:&[super::Class])->Result<bool,String>{
    let end=u64::from(row.physical.0).checked_add(row.size).ok_or("access extent overflow")?;require(row.size>0 && end<=1u64<<32,"access extent is not a positive 32-bit span")?;
    Ok(classes.iter().any(|class|class.kinds.contains(&row.source)&&u64::from(row.physical.0)<class.range[1]&&class.range[0]<end))
}
fn guarded(classes:&[super::Class],span:[u64;2],fetch:bool)->bool{classes.iter().any(|class|class.range==span&&[Access::CpuWrite,Access::HostWrite,Access::ModelBusWrite].iter().all(|kind|class.kinds.contains(kind))&&(!fetch||class.kinds.contains(&Access::CpuFetch)))}

fn route(report:&Report)->Result<(),String>{
    require(report.pulses.len()==15,"canonical ADC pulse count differs")?;let mut position=80_000_000u64;
    for (index,pulse) in report.pulses.iter().enumerate(){
        let expected=match index{0..=11=>Key::Up,12|13=>Key::Ok,_=>Key::Menu};if index==14{position=position.checked_add(40_000_000).ok_or("menu-settle endpoint overflows")?;}
        let released=pulse.released.ok_or("ADC pad release absent")?;let finished=pulse.finished.ok_or("ADC release observation incomplete")?;
        require(pulse.key==expected&&pulse.pressed.0==position&&position.checked_add(8_000_000)==Some(released.0),"canonical ADC edge or hold differs")?;
        let elapsed=finished.0.checked_sub(released.0).ok_or("ADC release chronology differs")?;require(elapsed<=8_000_000&&elapsed%250_000==0,"ADC release poll budget differs")?;
        require(matches!(pulse.outcome,Some(ReleaseOutcome::Debounced))||(elapsed==8_000_000&&matches!(pulse.outcome,Some(ReleaseOutcome::Deadline))),"ADC release outcome differs")?;position=finished.0;
    }
    require(position.checked_add(90_000_000)==Some(report.instructions.0)&&matches!(&report.menu,Ok(Some([1,5,2]))),"natural menu endpoint differs")
}

struct LoaderBounds<'a> {entry:&'a Event,transfer:&'a Event,copy:Guest,code:[u64;2],frame:[u64;2],core:Core}
fn loader_bounds(report:&Report,base:Guest,frame:Guest,core:Core)->Result<LoaderBounds<'_>,String>{
    let pc=Guest(base.0+0x9ac);let entry=report.events.iter().find(|event|event.pc==pc&&event.core==core).ok_or("loader origin absent")?;
    let transfer=report.events.iter().find(|event|event.pc.0==base.0+0x36e8&&event.core==core).ok_or("loader frame endpoint absent")?;
    require(entry.instructions.0<transfer.instructions.0&&transfer.instructions.0<=8_000_000,"loader lifetime is outside the prefix watch")?;
    Ok(LoaderBounds {entry,transfer,copy:base,code:[u64::from(pc.0),u64::from(pc.0)+340],frame:[u64::from(frame.0),u64::from(frame.0)+16],core})
}
fn loader_copy(row:&super::Row,base:Guest)->bool{
    row.source==Access::ModelBusWrite&&row.address==base&&row.physical==base&&row.size==0x20000&&row.core.is_none()&&row.pc.is_none()&&row.instructions.is_none()&&row.value.is_none()
}

fn projections(report:&Report)->Result<(),String>{
    for pc in [0x6a9840,0x6a9934]{require(report.events.iter().any(|event|event.pc.0==pc),"required producer return boundary absent")?;}
    let mut required=Vec::new();let mut image5=false;let mut plain=[false;3];
    for observed in &report.events {
        let resources:&[Resource]=match observed.pc.0{
            0x409ac=>&[Resource::LoaderA],0xd09ac if report.origin.is_some_and(|id|id.0==observed.id.0)=>&[Resource::LoaderA,Resource::LoaderB],0xd09ac=>&[Resource::LoaderB],
            0x6a8c04|0x6a9734=>&[Resource::Image5Bss],
            0x6a9840=>&[Resource::Image5,Resource::Image5Bss],0x6a9934=>&[Resource::Plain],
            0xd985a4 if !image5=>{image5=true;&[Resource::Image5,Resource::Image5Bss,Resource::Plain]},_=>&[],
        };
        required.extend(resources.iter().map(|resource|(observed.id.0,*resource)));
        if matches!(observed.pc.0,0x73be5c|0x73be70){
            let (index,resource)=match observed.registers.r[7]{0=>(0,Resource::Plain0),2=>(1,Resource::Plain2),3=>(2,Resource::Plain3),_=>return Err("observed extent is outside the canonical three-entry table".into())};
            if !plain[index]{plain[index]=true;required.push((observed.id.0,resource));}
        }
    }
    require(image5&&plain==[true;3],"first image5 entry or plain extent publication absent")?;
    require(required.len()==report.snapshots.len(),"resource projection census differs")?;
    for (event,resource) in required{require(report.snapshots.iter().filter(|sample|sample.event.0==event&&sample.resource==resource).count()==1,"required boundary/resource projection absent or repeated")?;}
    Ok(())
}

pub(super) fn check(root:&Path,analysis:&Path)->Result<Verification,String>{Ok(verified(root,analysis)?.verification)}

struct Verified {report:Report,program:Program,verification:Verification,publications:Vec<Publication>,loader_effects:Vec<LoaderEffect>,reset:loader_reset_check::Reset}
fn verified(root:&Path,analysis:&Path)->Result<Verified,String>{verified_receipt(root,analysis,read(&root.join("continuations.json"))?)}
fn verified_receipt(root:&Path,analysis:&Path,receipt:Vec<u8>)->Result<Verified,String>{
    let report:Report=serde_json::from_slice(&receipt).map_err(|error|error.to_string())?;
    require(report.schema==2&&report.error.is_none()&&report.machine_failure.is_none(),"observation did not complete")?;
    let mut catalog=Catalog::open(analysis)?;let program=Program::from_binding(report.program.clone(),root,&mut catalog)?;let catalog=&catalog;
    require(report.identity.update.as_str()==program.update()&&report.identity.run.as_str()=="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"&&report.identity.rom.as_str()=="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be","canonical input identity differs")?;
    require(report.identity.sources.len()==catalog.0.len(),"source catalog extent differs")?;
    for image in &catalog.0 {require(report.identity.sources.iter().filter(|source|source.kind==image.binding.kind&&source.start==image.binding.start&&source.bytes.0==image.binding.bytes.0&&source.sha256.as_str()==image.binding.sha256.as_str()).count()==1,"source catalog binding differs")?;}
    require(report.identity.update_file.sha256.as_str()==report.identity.update.as_str(),"bound update identity differs")?;
    let reset={
        require(Digest::of(CANONICAL_RUN).as_str()==report.identity.run.as_str(),"canonical run document identity differs")?;
        let document=fujimod_xm1::run::RunDocument::parse(CANONICAL_RUN)?;
        let update=read(&artifact(root,&report.identity.update_file)?)?;
        let digest=Digest::of(&update);
        require(digest.as_str()==report.identity.update_file.sha256.as_str()&&digest.as_str()==program.update(),"bound update identity differs")?;
        let media=fujimod_xm1::run::nand_base(&document,&fujimod_xm1::run::ValidatedUpdate(update))?;
        let bytes=read(&root.join("reset.fujimod"))?;
        require(Digest::of(&bytes).as_str()==report.checkpoint.as_str(),"reset checkpoint identity differs")?;
        loader_reset_check::admit(&bytes,&document,&media)?
    };
    route(&report)?;
    let trace_bytes=read(&artifact(root,&report.trace)?)?;require(Digest::of(&trace_bytes).as_str()==report.trace.sha256.as_str(),"raw trace identity differs")?;
    let trace:Vec<Hit>=serde_json::from_slice(&trace_bytes).map_err(|error|error.to_string())?;require(trace.len()==report.events.len()&&trace.len()<16384,"raw/event trace coverage differs")?;
    let mut previous=0;
    for (index,(hit,event)) in trace.iter().zip(&report.events).enumerate(){
        require(event.id.0==index&&event.instructions.0>=previous&&event.instructions.0<=report.instructions.0,"event chronology differs")?;previous=event.instructions.0;
        require(hit.address.0==event.pc.0&&hit.core==event.core&&hit.instructions==event.instructions&&hit.cpsr==event.registers.cpsr&&hit.sp.0==event.registers.sp.0&&hit.lr.0==event.registers.lr.0&&hit.registers.len()==13,"raw boundary differs")?;
        for register in 0..13u8{require(hit.registers.get(&RegisterNumber(register)).is_some_and(|value|value.0==event.registers.r[usize::from(register)]),"raw general register differs")?;}
        let (source,expected)=catalog.word(event.pc)?;words(&event.code)?;require(source==event.source&&event.code.mapping.guest==event.pc&&event.code.values==[expected]&&event.registers.cpsr&0x20==0,"entered source instruction differs")?;
        for value in event.state.iter().chain(event.descriptor.iter()).chain(event.payload.iter()){words(value)?;}if let Ok(frame)=&event.frame{words(frame)?;}
        let physical=event.pc.0&!0x40000000;
        require(super::DIAGNOSTICS.iter().all(|&(start,end)|physical<start||physical>=end),"diagnostic entry lies in the claimed natural route")?;
        require(program.execution_allowed(event.pc),"loader carrier execution point is not admitted")?;
        if let Some(side)=carrier_side(event.pc){require(event.core==side.core(),"loader carrier entry core differs")?;}
        require(event.pc.0!=0x69d2d8&&(!matches!(event.pc.0,0x40aa8|0xd0aa8)||program.owned()),"source nonreturning boundary was reached")?;
    }
    let origin=event(&report,report.origin.ok_or("loader origin absent")?)?;
    require(origin.pc==Guest(0xd09ac)&&origin.core==Core::Secondary&&origin.instructions.0==5_861_929&&report.events.iter().find(|observed|carrier_side(observed.pc).is_some()).is_some_and(|first|first.id.0==origin.id.0),"first carrier entry differs")?;
    require(report.epochs.len()==3&&report.epochs[0].stage==super::WatchStage::Bootstrap&&report.epochs[0].start.0==0&&report.epochs[0].end==origin.instructions&&report.epochs[1].stage==super::WatchStage::Startup&&report.epochs[1].start==origin.instructions&&report.epochs[1].end.0==8_000_000&&report.epochs[2].stage==super::WatchStage::Tail&&report.epochs[2].start==report.epochs[1].end&&report.epochs[2].end==report.instructions,"classified watch interval differs")?;
    let loaders=[loader_bounds(&report,Guest(0x40000),Guest(0x63450),Core::Primary)?,loader_bounds(&report,Guest(0xd0000),Guest(0xf3410),Core::Secondary)?];
    let mut publications=Vec::new();
    let mut missing_fetches=std::collections::BTreeSet::new();
    // Exact execution stamps the resumed instruction at the stopped count plus one.
    for boundary in report.events.iter().filter(|boundary|carrier_side(boundary.pc).is_some()){
        require(origin.instructions<=boundary.instructions&&boundary.instructions.0<8_000_000,"loader continuation executes after the admitted prefix")?;
        missing_fetches.insert((boundary.core,boundary.pc,boundary.instructions.0.checked_add(1).ok_or("carrier fetch clock overflow")?));
    }
    for epoch in &report.epochs {
        let bytes=read(&artifact(root,&epoch.file)?)?;require(Digest::of(&bytes).as_str()==epoch.file.sha256.as_str(),"raw watch identity differs")?;let raw:RawWatch=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
        require(epoch.enabled&&!epoch.refused&&epoch.teardown_error.is_none()&&raw.enabled&&raw.violation.is_none()&&raw.records.len()==epoch.record_count,"classified watch is incomplete")?;
        let bootstrap=epoch.stage==super::WatchStage::Bootstrap;let startup=epoch.stage==super::WatchStage::Startup;
        let expected_views=if bootstrap{super::WatchViews::Observe}else{super::WatchViews::Fixed};
        require(epoch.views==expected_views&&raw.views==epoch.views,"raw watch view policy differs")?;
        require(raw.records.len()<=3_000_000,"classified watch row cap differs")?;
        require(epoch.classes.len()==raw.classes.len()&&epoch.classes.iter().zip(&raw.classes).all(|(declared,observed)|declared.range==observed.range&&declared.kinds==observed.kinds),"raw watch class binding differs")?;
        for &(start,end) in &super::DIAGNOSTICS {require(guarded(&raw.classes,[u64::from(start),u64::from(end)],true),"diagnostic watch coverage absent")?;}
        for span in [[0x409ac,0x40b00],[0xd09ac,0xd0b00]]{require(guarded(&raw.classes,span,!bootstrap),"loader carrier watch coverage absent")?;}
        for span in [[0x61c000,0x61c220],[0x40000,0x40020],[0xd0000,0xd0020],[0x4f10c,0x4f118],[0xdf0cc,0xdf0d8],[0x89f794,0x89f798],[0xa00c4,0xa00c8],[0x5927c,0x59280],[0xe923c,0xe9240],[0x96bd20,0x96bd28],[0x89f8ac,0x89f8b0],[0x944440,0x944464],[0x914834,0x914838],[0x972e88,0x972e94],[0x14c100c,0x14c1010],[0x14c1018,0x14c101c],[0x14c113c,0x14c1140],[0x9662d4,0x9662d8],[0x89f784,0x89f788],[0x89f78c,0x89f790]]{require(guarded(&raw.classes,span,false),"shared publication watch coverage absent")?;}
        if bootstrap||startup{for span in [[0x63450,0x63460],[0xf3410,0xf3420]]{require(guarded(&raw.classes,span,false),"saved loader frame watch coverage absent")?;}}
        if bootstrap{for loader in &loaders{require(raw.records.iter().filter(|row|loader_copy(row,loader.copy)).count()==1,"canonical loader-copy census differs")?;}}
        let mut future=raw.records.iter().enumerate().filter(|(_,row)|matches!(row.source,Access::CpuRead|Access::CpuFetch|Access::CpuWrite)).peekable();
        let mut previous_cpu=epoch.start.0;let mut lower=None;
        for (index,row) in raw.records.iter().enumerate() {
            require(in_class(row,&raw.classes)?,"raw access lies outside its declared classes")?;
            if matches!(row.source,Access::CpuRead|Access::CpuFetch|Access::CpuWrite){
                let count=row.instructions.ok_or("CPU access clock absent")?.0;
                if startup&&count==epoch.start.0&&row.source==Access::CpuFetch{require(row.core==Some(origin.core)&&row.pc==Some(origin.pc)&&row.address==origin.pc&&row.physical==origin.pc,"loader carrier fetch attribution differs")?;}
                let stopped_origin=startup&&count==epoch.start.0&&row.source==Access::CpuFetch&&row.core==Some(origin.core)&&row.pc==Some(origin.pc)&&row.address==origin.pc&&row.physical==origin.pc;
                require(row.core.is_some()&&row.pc.is_some()&&(epoch.start.0<count||stopped_origin)&&count<=epoch.end.0&&previous_cpu<=count,"CPU access attribution or clock differs")?;
                previous_cpu=count;lower=Some(count);require(future.next().is_some_and(|(position,_)|position==index),"CPU clock stream differs")?;
            }
            let upper=future.peek().and_then(|(_,next)|next.instructions).map(|count|count.0);
            let end=u64::from(row.physical.0)+row.size;
            if matches!(row.source,Access::CpuFetch){for &(start,finish) in &super::DIAGNOSTICS{require(end<=u64::from(start)||u64::from(finish)<=u64::from(row.physical.0),"diagnostic instruction fetch observed")?;}}
            if matches!(row.source,Access::CpuWrite|Access::HostWrite|Access::ModelBusWrite)&&PUBLICATIONS.iter().any(|span|u64::from(row.physical.0)<span[1]&&span[0]<end){
                publications.push(Publication {source:row.source,address:row.address,physical:row.physical,size:row.size,core:row.core,value:row.value,clock:row.instructions,writer:match row.pc {Some(pc)=>match carrier_side(pc) {Some(side)=>Writer::Carrier(side),None=>Writer::Retained(pc)},None=>Writer::Untimed}});
            }
            for loader in &loaders {
                if u64::from(row.physical.0)<loader.code[1]&&loader.code[0]<end {
                    match row.source {
                        Access::CpuFetch=>{
                            require(startup,"loader continuation executes after the admitted prefix")?;
                            require(row.size==4&&row.address==row.physical&&row.pc==Some(row.address)&&row.core==Some(loader.core)&&row.address.0%4==0,"loader carrier fetch attribution differs")?;
                            require(program.execution_allowed(row.address),"loader carrier execution point is not admitted")?;
                            missing_fetches.remove(&(loader.core,row.address,row.instructions.ok_or("CPU access clock absent")?.0));
                        },
                        Access::CpuWrite|Access::HostWrite|Access::ModelBusWrite=>require(bootstrap&&loader_copy(row,loader.copy)&&upper.is_some_and(|count|count<loader.entry.instructions.0),"loader carrier write is not a proven earlier canonical copy")?,
                        _=>{},
                    }
                }
                if u64::from(row.physical.0)<loader.frame[1]&&loader.frame[0]<end {
                    if matches!(row.source,Access::HostWrite|Access::ModelBusWrite){require(upper.is_some_and(|count|count<loader.entry.instructions.0)||lower.is_some_and(|count|count>loader.transfer.instructions.0),"untimed writer could reach the live saved loader frame")?;}
                    if row.instructions.is_some_and(|count|loader.entry.instructions.0<=count.0&&count.0<=loader.transfer.instructions.0){require(matches!(row.source,Access::CpuWrite)&&row.pc==Some(loader.entry.pc)&&row.core==Some(loader.core),"foreign writer reached the live saved loader frame")?;}
                }
            }
        }
    }
    require(missing_fetches.is_empty(),"observed loader boundary lacks raw carrier fetch")?;
    let mut loader_calls=0;let mut task_calls=0;
    for call in &report.calls {
        let entered=event(&report,call.entry.ok_or("observed call lacks its actual entry")?)?;
        require(entered.pc==call.target&&entered.core==call.core&&entered.registers.sp==call.sp&&entered.registers.lr==call.continuation,"callee entry context differs")?;
        if let Some(before)=call.before {let before=event(&report,before)?;require(before.core==call.core&&before.registers.sp==call.sp&&before.pc.0.checked_add(4)==Some(call.continuation.0)&&branch(before,catalog)?==call.target&&before.instructions.0<=entered.instructions.0&&before.registers.r==entered.registers.r&&before.registers.cpsr==entered.registers.cpsr,"source call/target argument context or chronology differs")?;
            let expected_kind=match before.pc.0{0xd985c0=>CallKind::Image5Wait,0x69d2e0=>CallKind::Image5Entry,0xd9858c=>CallKind::Image5Constructor,0x73b554=>CallKind::PlainProducer,0x73be5c=>CallKind::PlainHook,0x73beac=>CallKind::Deferred,pc if carrier_side(Guest(pc)).is_some()=>CallKind::Loader,pc if (0xd985a4..0xd986ac).contains(&pc)=>CallKind::Image5Service,pc if (0x6a9844..0x6a9934).contains(&pc)=>CallKind::PlainService,_=>return Err("call origin is outside the observed source boundaries".into())};require(call.kind==expected_kind,"call kind does not match its raw source boundary")?;
            if call.kind==CallKind::Loader {
                loader_calls+=1;let side=carrier_side(before.pc).ok_or("loader call origin differs")?;let sp=side.frame().0;
                require(call.core==side.core()&&before.registers.sp.0==sp&&before.registers.r[11]==sp+12&&before.registers.cpsr&0xff==0xd3,"canonical loader caller context differs")?;
                let origin=report.events.iter().find(|observed|observed.pc==Guest(side.base().0+0x9ac)&&observed.core==side.core()).ok_or("startup function entry absent")?;
                require(projected_frame(before)?==[origin.registers.r[4],origin.registers.r[5],origin.registers.r[11],origin.registers.lr.0],"loader saved frame differs from entry")?;
            }
            if call.kind==CallKind::PlainHook {let hook=Guest(before.registers.r[10]);let (_,expected)=catalog.word(hook)?;require(expected==call.target.0&&matches!(before.registers.r[7],0|3),"plain hook does not match its source list")?;}
            if call.kind==CallKind::Image5Entry {let descriptor=before.state.iter().find(|state|state.mapping.guest.0==before.registers.r[0]&&state.values.len()==2).ok_or("image5 descriptor view absent")?;let header=before.state.iter().find(|state|state.mapping.guest.0==descriptor.values[1]&&state.values.len()==4).ok_or("image5 header view absent")?;require(header.values[3]==call.target.0&&call.target.0==0xd985a4,"image5 header transfer differs")?;}
            if call.kind==CallKind::Image5Wait{require(call.target.0==0x73aca4&&entered.registers.r[0]==7,"image5 early wait service or argument differs")?;}
        }else{require(call.kind==CallKind::Task&&entered.pc.0==0x725dd8,"entry-only call is not a task submission")?;}
        if matches!(call.target.0,0x43688|0xd3688){require(call.returned.is_none(),"initial scheduler unexpectedly returned to the loader")?;}
        if let Some(returned)=call.returned {let returned=event(&report,returned)?;require(entered.id.0<returned.id.0&&returned.pc==call.continuation&&returned.core==call.core&&returned.registers.sp==call.sp&&returned.instructions.0>=entered.instructions.0&&returned.registers.r[4..12]==entered.registers.r[4..12],"retained return state or chronology differs")?;
            if call.kind==CallKind::Loader{let before=event(&report,call.before.ok_or("loader pre-call event absent")?)?;require(projected_frame(before)?==projected_frame(returned)?,"loader saved frame changed across a call")?;if call.target.0==0x69cdf8{require(returned.registers.r[0]==if entered.registers.r[0]==2{0x69cc6c}else{0},"dispatcher result differs")?;}}
            if call.kind==CallKind::Task{task_calls+=1;let descriptor=entered.descriptor.as_ref().ok_or("task descriptor entry absent")?;let after=returned.descriptor.as_ref().ok_or("task descriptor return absent")?;require(descriptor.values.len()==7&&after.values.len()==7&&descriptor.mapping.guest.0==entered.registers.r[0]&&after.mapping.guest==descriptor.mapping.guest&&call.descriptor==Some(descriptor.mapping.guest),"task descriptor alias/extent differs")?;}
        }else{require(call.kind==CallKind::Loader&&matches!(call.target.0,0x43688|0xd3688),"a required retained call has not returned")?;}
    }
    let loader_effects=loader_semantics(&report)?;bind_publications(&report,&publications)?;
    let mut constructors=Vec::new();
    for observed in &report.events {
        let counted=(carrier_side(observed.pc).is_some()||(0xd985a4..0xd986ac).contains(&observed.pc.0)||(0x6a9844..0x6a9934).contains(&observed.pc.0)||matches!(observed.pc.0,0x73b554|0x69d2e0|0xd9858c|0x73be5c|0x73beac))&&branch(observed,catalog).is_ok();
        if counted{require(report.calls.iter().filter(|call|call.before.is_some_and(|id|id.0==observed.id.0)).count()==1,"observed source call was omitted or repeated")?;}
        if observed.pc.0==0x725dd8{require(report.calls.iter().filter(|call|call.kind==CallKind::Task&&call.entry.is_some_and(|id|id.0==observed.id.0)).count()==1,"observed task entry was omitted or repeated")?;}
        if observed.pc.0==0xd9858c{constructors.push(branch(observed,catalog)?.0);}
    }
    require(constructors==[0x107277c,0x1066304,0x1062114],"image5 reverse constructor order differs")?;
    for (base,core,sp,cell) in [(0x40000,Core::Primary,0x40063450,0x4f114),(0xd0000,Core::Secondary,0x400f3410,0xdf0d4)] {
        let entry=report.events.iter().find(|event|event.pc.0==base+0x9ac&&event.core==core).ok_or("startup function entry absent")?;require(entry.registers.r[0]==0x69cdf8&&entry.registers.sp.0==sp+16,"startup caller argument/frame differs")?;
        let handoff=report.events.iter().find(|event|event.pc.0==base+0x36e8&&event.core==core).ok_or("initial scheduler context transfer absent")?;
        let saved=handoff.state.iter().find(|state|state.mapping.guest.0==sp&&state.values.len()==4).ok_or("saved loader frame at handoff absent")?;require(saved.values==[entry.registers.r[4],entry.registers.r[5],entry.registers.r[11],entry.registers.lr.0],"saved loader frame changed before handoff")?;
        let context_address=handoff.registers.r[1].checked_add(24).ok_or("selected context address overflows")?;let context=handoff.state.iter().find(|state|state.mapping.guest.0==context_address&&state.values.len()==2).ok_or("selected context absent")?;require(context.values[0]==handoff.registers.sp.0&&context.values[1]==base+0x3728,"first loaded context differs")?;
        let load=report.events.iter().find(|event|event.pc.0==base+0x36e4&&event.core==core).ok_or("initial saved-SP load absent")?;
        require(load.id.0<handoff.id.0&&load.instructions.0<handoff.instructions.0&&load.registers.r==handoff.registers.r&&load.registers.lr==handoff.registers.lr&&load.registers.cpsr==handoff.registers.cpsr,"saved-SP load context or chronology differs")?;
        let trampoline=report.events.iter().find(|event|event.pc.0==base+0x3728&&event.core==core&&event.id.0>handoff.id.0).ok_or("initial trampoline entry absent")?;
        require(trampoline.instructions.0>handoff.instructions.0&&trampoline.registers.sp==handoff.registers.sp&&trampoline.registers.r==handoff.registers.r&&trampoline.registers.lr==handoff.registers.lr&&trampoline.registers.cpsr==handoff.registers.cpsr,"loaded trampoline context differs")?;
        let jump=report.events.iter().find(|event|event.pc.0==base+0x3758&&event.core==core&&event.id.0>trampoline.id.0).ok_or("initial C-entry transfer absent")?;
        let creation=jump.state.iter().find(|view|view.mapping.guest.0==jump.registers.r[2]&&view.values.len()==3).ok_or("initial creation record absent")?;
        let link=jump.registers.r[1].checked_add(8).ok_or("creation link address overflows")?;
        let cleanup=if core==Core::Primary{0x453f0}else{0xd53b0};
        require(jump.instructions.0>trampoline.instructions.0&&jump.registers.r[1]==handoff.registers.r[1]&&at(jump,link)?==jump.registers.r[2]&&jump.registers.sp==trampoline.registers.sp&&jump.registers.r[0]==creation.values[1]&&jump.registers.lr.0==cleanup,"C-record argument, caller or cleanup context differs")?;
        let entered=report.events.iter().find(|event|event.core==core&&event.pc.0==creation.values[2]&&event.id.0>jump.id.0).ok_or("initial C function entry absent")?;
        require(entered.instructions.0>jump.instructions.0&&entered.registers.sp==jump.registers.sp&&entered.registers.r==jump.registers.r&&entered.registers.lr==jump.registers.lr&&entered.registers.cpsr==jump.registers.cpsr,"initial C-entry target or context differs")?;
        let publisher=report.calls.iter().find(|call|call.kind==CallKind::Loader&&call.core==core&&call.target==Service::PublishCallback.address(if core==Core::Primary{Side::LoaderA}else{Side::LoaderB})).ok_or("callback publisher absent")?;
        let callback=event(&report,publisher.returned.ok_or("callback publisher return absent")?)?;require(at(callback,cell)?==0x69cc6c,"committed diagnostic callback differs")?;
    }
    projections(&report)?;
    for sample in &report.snapshots {require(sample.event.0<report.events.len(),"snapshot event absent")?;snapshot(root,sample,catalog)?;}
    require(report.calls.iter().filter(|call|call.kind==CallKind::Image5Entry).count()==1,"image5 header call extent differs")?;
    let setters:Vec<_>=report.calls.iter().filter(|call|call.kind==CallKind::Image5Service&&call.target.0==0x69cf84).collect();require(setters.len()==1,"diagnostic callback setter call census differs")?;
    let setter=event(&report,setters[0].entry.ok_or("setter entry absent")?)?;let setter_return=event(&report,setters[0].returned.ok_or("setter return absent")?)?;require(setter.registers.r[0]==0xed4a8c&&at(setter_return,0x89f794)?==0xed4a8c,"installed diagnostic callback or committed cell differs")?;
    let verification=Verification {schema:3,scope:if program.owned(){Scope::OwnedNaturalObservation}else{Scope::CanonicalNaturalObservation},receipt:Digest::of(&receipt),events:report.events.len(),loader_calls,task_calls,snapshots:report.snapshots.len(),source_differences:report.snapshots.iter().filter(|sample|!sample.expected_equal).count()};
    Ok(Verified {report,program,verification,publications,loader_effects,reset})
}

fn carrier_side(pc:Guest)->Option<Side>{
    let physical=pc.0&!0x40000000;
    [Side::LoaderA,Side::LoaderB].into_iter().find(|side|(side.base().0+0x9ac..side.base().0+0xb00).contains(&physical))
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum DispatchPhase { Setup,Root,Idle,Diagnostic,Peer }
impl DispatchPhase {
    fn argument(self)->u32{match self{Self::Setup=>0,Self::Root=>1,Self::Idle=>3,Self::Diagnostic=>2,Self::Peer=>4}}
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag="kind",content="operation",rename_all="kebab-case")]
enum LoaderStep { Service(Service),Dispatch(DispatchPhase) }
fn sequence(side:Side)->&'static [LoaderStep]{
    use Service::*;use LoaderStep::{Service as S,Dispatch as D};
    match side{
        Side::LoaderA=>&[S(Identity),S(ClearShared),S(PrivateInitialize),S(Log),S(Prepare1),S(Prepare2),S(Prepare3),S(Prepare4),D(DispatchPhase::Setup),S(PrimaryContinue),S(Continue1),S(Continue2),S(Continue3),S(Continue4),S(Continue5),S(Continue6),S(Continue7),D(DispatchPhase::Root),D(DispatchPhase::Idle),D(DispatchPhase::Diagnostic),S(PublishCallback),S(AutoReady),S(ActivateTimers),S(Log),S(Schedule)],
        Side::LoaderB=>&[S(Identity),S(PrivateInitialize),S(Log),S(Prepare1),S(Prepare2),S(Prepare3),S(Prepare4),S(PeerContinue),S(Continue1),S(Continue2),S(Continue3),S(Continue4),S(Continue5),S(Continue6),S(Continue7),D(DispatchPhase::Peer),D(DispatchPhase::Diagnostic),S(PublishCallback),S(AutoReady),S(ActivateTimers),S(Log),S(Schedule)],
    }
}
#[derive(Serialize, PartialEq, Eq)]
struct LiveInputs {r0:Option<u32>,r1:Option<u32>}
#[derive(Serialize, PartialEq, Eq)]
struct LoaderEffect {side:Side,step:LoaderStep,inputs:LiveInputs,saved:[u32;4],returned:bool,dispatcher_result:Option<u32>}
fn loader_semantics(report:&Report)->Result<Vec<LoaderEffect>,String>{
    let mut effects=Vec::new();
    for side in [Side::LoaderA,Side::LoaderB]{
        let mut calls:Vec<_>=report.calls.iter().filter(|call|call.kind==CallKind::Loader&&call.core==side.core()).collect();
        calls.sort_unstable_by_key(|call|call.before.map(|id|id.0));
        let expected=sequence(side);require(calls.len()==expected.len(),"loader service sequence differs")?;
        let mut previous=None;let mut logs=0;
        for (call,step) in calls.into_iter().zip(expected.iter().copied()){
            let before=event(report,call.before.ok_or("loader pre-call event absent")?)?;let entry=event(report,call.entry.ok_or("loader service entry absent")?)?;
            require(previous.is_none_or(|id|id<=before.id.0)&&before.id.0<entry.id.0,"loader per-core service ordering differs")?;
            let target=match step{LoaderStep::Service(service)=>service.address(side),LoaderStep::Dispatch(_)=>Guest(0x69cdf8)};
            require(call.target==target&&carrier_side(before.pc)==Some(side),if matches!(step,LoaderStep::Dispatch(_)){"phase argument or target differs"}else{"loader service sequence differs"})?;
            let inputs=match step{
                LoaderStep::Dispatch(phase)=>{require(entry.registers.r[0]==phase.argument(),"phase argument or target differs")?;LiveInputs {r0:Some(phase.argument()),r1:None}},
                LoaderStep::Service(Service::Log)=>{let nonzero=entry.registers.r[0]!=0;require(nonzero==(logs==1),"loader service live-in differs")?;logs+=1;LiveInputs {r0:Some(u32::from(nonzero)),r1:None}},
                LoaderStep::Service(service)=>{
                    if service==Service::ClearShared{require(entry.registers.r[0]==0x400a0800&&entry.registers.r[1]==0x20100,"loader service live-in differs")?;}
                    if service==Service::PublishCallback{require(entry.registers.r[0]==0x69cc6c,"loader service live-in differs")?;}
                    if service==Service::Schedule{require(entry.registers.r[0]==(-10i32) as u32,"scheduler assignment differs")?;}
                    let mut inputs=LiveInputs {r0:None,r1:None};
                    for register in service.live(){match register.0{0=>inputs.r0=Some(entry.registers.r[0]),1=>inputs.r1=Some(entry.registers.r[1]),_=>return Err("source live-in is outside the loader argument set".into())}}
                    inputs
                },
            };
            let saved: [u32;4]=projected_frame(before)?.try_into().map_err(|_|"loader saved frame extent differs")?;
            let dispatcher_result=if matches!(step,LoaderStep::Dispatch(_)){Some(event(report,call.returned.ok_or("dispatcher return absent")?)?.registers.r[0])}else{None};
            if step==LoaderStep::Service(Service::Schedule){require(call.returned.is_none(),"initial scheduler unexpectedly returned to the loader")?;previous=Some(entry.id.0);}else{previous=Some(call.returned.ok_or("a required retained call has not returned")?.0);}
            effects.push(LoaderEffect {side,step,inputs,saved,returned:call.returned.is_some(),dispatcher_result});
        }
    }
    Ok(effects)
}

const PUBLICATIONS:[[u64;2];21]=[[0x61c000,0x61c220],[0x40000,0x40020],[0xd0000,0xd0020],[0x4f10c,0x4f118],[0xdf0cc,0xdf0d8],[0x89f794,0x89f798],[0xa00c4,0xa00c8],[0x5927c,0x59280],[0xe923c,0xe9240],[0x96bd20,0x96bd28],[0x89f8ac,0x89f8b0],[0x944440,0x944464],[0x914834,0x914838],[0x972e88,0x972e94],[0x14c100c,0x14c1010],[0x14c1018,0x14c101c],[0x14c113c,0x14c1140],[0x9662d4,0x9662d8],[0x89f784,0x89f788],[0x89f78c,0x89f790],[0xa0800,0xa0838]];
#[derive(Serialize, PartialEq, Eq)]
#[serde(tag="kind",content="source",rename_all="kebab-case")]
enum Writer { Carrier(Side),Retained(Guest),Untimed }
#[derive(Serialize, PartialEq, Eq)]
struct Publication {source:Access,address:Guest,physical:Guest,size:u64,core:Option<Core>,value:Option<u64>,writer:Writer,#[serde(skip)]clock:Option<super::Count>}

fn bind_publications(report:&Report,writes:&[Publication])->Result<(),String>{
    let observed=|core,cell,value,low,high|writes.iter().any(|write|write.source==Access::CpuWrite&&write.core==Some(core)&&write.physical==Guest(cell)&&write.size==4&&write.value==Some(value)&&write.clock.is_some_and(|clock|low<clock.0&&clock.0<=high));
    for side in [Side::LoaderA,Side::LoaderB]{
        let call=|service:Service|report.calls.iter().find(|call|call.kind==CallKind::Loader&&call.core==side.core()&&call.target==service.address(side)).ok_or("required loader service absent");
        let publisher=call(Service::PublishCallback)?;let entered=event(report,publisher.entry.ok_or("callback publisher entry absent")?)?;let returned=event(report,publisher.returned.ok_or("callback publisher return absent")?)?;
        let cell=if side==Side::LoaderA{0x4f114}else{0xdf0d4};
        require(observed(side.core(),cell,0x69cc6c,entered.instructions.0,returned.instructions.0),"raw callback publication absent")?;
        let scheduler=event(report,call(Service::Schedule)?.entry.ok_or("scheduler entry absent")?)?;
        let log=report.calls.iter().filter(|call|call.kind==CallKind::Loader&&call.core==side.core()&&call.target==Service::Log.address(side)).max_by_key(|call|call.entry.map(|id|id.0)).ok_or("final loader log absent")?;
        let log_return=event(report,log.returned.ok_or("final loader log return absent")?)?;
        let ready=if side==Side::LoaderA{0x5927c}else{0xe923c};
        require(observed(side.core(),ready,1,log_return.instructions.0,scheduler.instructions.0)&&at(scheduler,ready)?==1,"raw ready publication absent")?;
    }
    let primary=report.calls.iter().filter(|call|call.kind==CallKind::Loader&&call.core==Core::Primary&&call.target==Guest(0x69cdf8)).min_by_key(|call|call.entry.map(|id|id.0)).ok_or("setup dispatcher absent")?;
    let setup=event(report,primary.returned.ok_or("setup dispatcher return absent")?)?;
    let continuation=report.calls.iter().find(|call|call.kind==CallKind::Loader&&call.target==Service::PrimaryContinue.address(Side::LoaderA)).ok_or("primary continuation absent")?;
    let continued=event(report,continuation.entry.ok_or("primary continuation entry absent")?)?;
    require(observed(Core::Primary,0xa00c4,1,setup.instructions.0,continued.instructions.0),"raw completion publication absent")?;
    let peer=report.calls.iter().find(|call|call.kind==CallKind::Loader&&call.target==Service::PrivateInitialize.address(Side::LoaderB)).ok_or("peer private initialization absent")?;
    let private=event(report,peer.before.ok_or("peer private initialization source absent")?)?;
    require(at(private,0xa00c4)?==1&&observed(Core::Primary,0xa00c4,1,setup.instructions.0,private.instructions.0),"peer completion wait evidence differs")
}

#[derive(Serialize)]
struct ScalarEffect<'a> {address:Guest,values:&'a [u32]}
fn scalar_effect(value:&Words)->ScalarEffect<'_>{ScalarEffect {address:value.mapping.guest,values:&value.values}}
#[derive(Serialize)]
struct BoundaryEffect<'a> {state:Vec<ScalarEffect<'a>>,descriptor:Option<ScalarEffect<'a>>,payload:Option<ScalarEffect<'a>>}
fn boundary_effect(observed:&Event)->BoundaryEffect<'_>{
    let mut state:Vec<_>=observed.state.iter().map(scalar_effect).collect();state.sort_unstable_by_key(|value|value.address.0);
    BoundaryEffect {state,descriptor:observed.descriptor.as_ref().map(scalar_effect),payload:observed.payload.as_ref().map(scalar_effect)}
}
#[derive(Serialize)]
#[serde(tag="kind",rename_all="kebab-case")]
enum ConsumerInput {
    Task {descriptor:Guest},
    Image5Wait {reason:u32},
    Image5Entry {descriptor:Guest},
    Callback {address:Guest},
    PlainHook {index:u32,descriptor:Guest,hook:Guest},
    SourceBoundProjections,
}
#[derive(Serialize)]
struct CallEffect<'a> {core:Core,kind:CallKind,target:Guest,source:Option<Guest>,input:ConsumerInput,entry:BoundaryEffect<'a>,returned:BoundaryEffect<'a>}
fn consumer_calls(report:&Report)->Result<Vec<CallEffect<'_>>,String>{
    let mut effects=Vec::new();
    for core in [Core::Primary,Core::Secondary]{
        let mut calls:Vec<_>=report.calls.iter().filter(|call|call.core==core&&call.kind!=CallKind::Loader).collect();
        calls.sort_unstable_by_key(|call|call.entry.map(|id|id.0));
        for call in calls{
            let entry=event(report,call.entry.ok_or("consumer call entry absent")?)?;
            let returned=event(report,call.returned.ok_or("consumer call return absent")?)?;
            let before=call.before.map(|id|event(report,id)).transpose()?;
            let input=match call.kind{
                CallKind::Task=>ConsumerInput::Task {descriptor:Guest(entry.registers.r[0])},
                CallKind::Image5Wait=>ConsumerInput::Image5Wait {reason:entry.registers.r[0]},
                CallKind::Image5Entry=>ConsumerInput::Image5Entry {descriptor:Guest(entry.registers.r[0])},
                CallKind::Image5Service if call.target==Guest(0x69cf84)=>ConsumerInput::Callback {address:Guest(entry.registers.r[0])},
                CallKind::PlainHook=>{let source=before.ok_or("plain hook source absent")?;ConsumerInput::PlainHook {index:source.registers.r[7],descriptor:Guest(source.registers.r[4]),hook:Guest(source.registers.r[10])}},
                _=>ConsumerInput::SourceBoundProjections,
            };
            effects.push(CallEffect {core,kind:call.kind,target:call.target,source:before.map(|before|before.pc),input,entry:boundary_effect(entry),returned:boundary_effect(returned)});
        }
    }
    Ok(effects)
}
#[derive(Serialize)]
struct TransferEffect<'a> {side:Side,selected_task:Guest,selected_sp:Guest,saved_context:ScalarEffect<'a>,creation:ScalarEffect<'a>,entry:Guest,argument:u32,cleanup:Guest}
fn transfers(report:&Report)->Result<Vec<TransferEffect<'_>>,String>{
    [Side::LoaderA,Side::LoaderB].into_iter().map(|side|{
        let stopped=|offset|report.events.iter().find(|observed|observed.pc==Guest(side.base().0+offset)&&observed.core==side.core()).ok_or("first task transfer boundary absent");
        let handoff=stopped(0x36e8)?;let jump=stopped(0x3758)?;
        let context_address=handoff.registers.r[1].checked_add(24).ok_or("selected context address overflows")?;
        let context=handoff.state.iter().find(|view|view.mapping.guest.0==context_address&&view.values.len()==2).ok_or("selected context absent")?;
        let creation=jump.state.iter().find(|view|view.mapping.guest.0==jump.registers.r[2]&&view.values.len()==3).ok_or("initial creation record absent")?;
        Ok(TransferEffect {side,selected_task:Guest(handoff.registers.r[1]),selected_sp:handoff.registers.sp,saved_context:scalar_effect(context),creation:scalar_effect(creation),entry:Guest(creation.values[2]),argument:jump.registers.r[0],cleanup:jump.registers.lr})
    }).collect()
}
#[derive(Serialize)]
struct ResourceEffect<'a> {core:Core,pc:Guest,resource:Resource,physical_digest:&'a Digest,expected_equal:bool,first_difference:Option<Guest>}
#[derive(Serialize)]
struct SemanticEffects<'a> {loaders:&'a [LoaderEffect],calls:Vec<CallEffect<'a>>,transfers:Vec<TransferEffect<'a>>,publications:BTreeMap<u32,Vec<&'a Publication>>,resources:Vec<ResourceEffect<'a>>,menu:[u32;3]}
fn semantic_effects(observed:&Verified)->Result<SemanticEffects<'_>,String>{
    let mut publications:BTreeMap<u32,Vec<&Publication>>=BTreeMap::new();
    for write in &observed.publications{publications.entry(write.physical.0).or_default().push(write);}
    let mut snapshots:Vec<_>=observed.report.snapshots.iter().filter(|sample|!matches!(sample.resource,Resource::LoaderA|Resource::LoaderB)).collect();
    snapshots.sort_unstable_by_key(|sample|(sample.event.0,sample.resource as u8));
    let resources=snapshots.into_iter().map(|sample|{
        let boundary=event(&observed.report,sample.event)?;
        Ok(ResourceEffect {core:boundary.core,pc:boundary.pc,resource:sample.resource,physical_digest:&sample.files[2].sha256,expected_equal:sample.expected_equal,first_difference:sample.first_difference})
    }).collect::<Result<_,String>>()?;
    Ok(SemanticEffects {loaders:&observed.loader_effects,calls:consumer_calls(&observed.report)?,transfers:transfers(&observed.report)?,publications,resources,menu:[1,5,2]})
}
#[derive(Serialize)]
#[serde(rename_all="kebab-case")]
enum ComparisonScope { PairedNaturalLoaderOrchestration }
#[derive(Serialize)]
#[serde(rename_all="kebab-case")]
enum AdmittedTiming { SourceLocalCoreOrder,PeerCompletionWait }
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all="kebab-case")]
enum ComparisonOutcome { Equivalent,RefusedSemanticMismatch,RefusedRetainedObservation,RefusedOwnedObservation,RefusedRoles,RefusedIdentity }
#[derive(Serialize)]
struct ComparedObservation {receipt:FileBinding,verification:Option<Verification>}
#[derive(Serialize)]
struct Comparison {schema:u32,scope:ComparisonScope,accepted:bool,outcome:ComparisonOutcome,refusal:Option<String>,retained:ComparedObservation,owned:ComparedObservation,effects:Option<FileBinding>,owned_effects:Option<FileBinding>,admitted_timing:[AdmittedTiming;2],menu:Option<[u32;3]>}
fn persist_bound(output:&Path,name:&str,bytes:&[u8])->Result<FileBinding,String>{fujimod_xm1::bridge::write(&output.join(name),bytes)?;Ok(FileBinding {path:name.into(),sha256:Digest::of(bytes)})}
fn persist_comparison(output:&Path,comparison:&Comparison)->Result<(),String>{fujimod_xm1::bridge::write(&output.join("comparison.json"),&serde_json::to_vec_pretty(comparison).map_err(|error|error.to_string())?)}
fn refuse_comparison(output:&Path,comparison:&mut Comparison,outcome:ComparisonOutcome,error:String)->Result<(),String>{
    comparison.outcome=outcome;comparison.refusal=Some(error.clone());persist_comparison(output,comparison)?;Err(error)
}
pub(super) fn compare(retained:&Path,owned:&Path,analysis:&Path,output:&Path)->Result<(),String>{
    require(!output.try_exists().map_err(|error|error.to_string())?,"comparison output already exists")?;
    let retained_bytes=read(&retained.join("continuations.json"))?;let owned_bytes=read(&owned.join("continuations.json"))?;
    std::fs::create_dir(output).map_err(|error|error.to_string())?;
    let retained_receipt=persist_bound(output,"retained.json",&retained_bytes)?;let owned_receipt=persist_bound(output,"owned.json",&owned_bytes)?;
    let mut comparison=Comparison {schema:1,scope:ComparisonScope::PairedNaturalLoaderOrchestration,accepted:false,outcome:ComparisonOutcome::RefusedRetainedObservation,refusal:None,retained:ComparedObservation {receipt:retained_receipt,verification:None},owned:ComparedObservation {receipt:owned_receipt,verification:None},effects:None,owned_effects:None,admitted_timing:[AdmittedTiming::SourceLocalCoreOrder,AdmittedTiming::PeerCompletionWait],menu:None};
    let retained=match verified_receipt(retained,analysis,retained_bytes){Ok(observed)=>observed,Err(error)=>return refuse_comparison(output,&mut comparison,ComparisonOutcome::RefusedRetainedObservation,error)};
    let owned=match verified_receipt(owned,analysis,owned_bytes){Ok(observed)=>observed,Err(error)=>{comparison.retained.verification=Some(retained.verification);return refuse_comparison(output,&mut comparison,ComparisonOutcome::RefusedOwnedObservation,error)}};
    let roles=!retained.program.owned()&&owned.program.owned();
    let identities=retained.report.identity.run.as_str()==owned.report.identity.run.as_str()&&retained.report.identity.rom.as_str()==owned.report.identity.rom.as_str()&&retained.report.identity.executable.as_str()==owned.report.identity.executable.as_str();
    if !roles{comparison.retained.verification=Some(retained.verification);comparison.owned.verification=Some(owned.verification);return refuse_comparison(output,&mut comparison,ComparisonOutcome::RefusedRoles,"paired observation roles differ".into());}
    if !identities{comparison.retained.verification=Some(retained.verification);comparison.owned.verification=Some(owned.verification);return refuse_comparison(output,&mut comparison,ComparisonOutcome::RefusedIdentity,"paired reset or observer identity differs".into());}
    if let Err(error)=loader_reset_check::compare(&retained.reset,&owned.reset){comparison.retained.verification=Some(retained.verification);comparison.owned.verification=Some(owned.verification);return refuse_comparison(output,&mut comparison,ComparisonOutcome::RefusedIdentity,error);}
    let effects=serde_json::to_vec_pretty(&semantic_effects(&retained)?).map_err(|error|error.to_string())?;
    let owned_effects=serde_json::to_vec_pretty(&semantic_effects(&owned)?).map_err(|error|error.to_string())?;
    let equivalent=effects==owned_effects;
    comparison.effects=Some(persist_bound(output,"semantic-effects.json",&effects)?);comparison.owned_effects=Some(persist_bound(output,"owned-semantic-effects.json",&owned_effects)?);
    comparison.retained.verification=Some(retained.verification);comparison.owned.verification=Some(owned.verification);comparison.menu=Some([1,5,2]);
    if !equivalent{return refuse_comparison(output,&mut comparison,ComparisonOutcome::RefusedSemanticMismatch,"paired semantic effects differ".into());}
    comparison.accepted=true;comparison.outcome=ComparisonOutcome::Equivalent;persist_comparison(output,&comparison)
}

#[cfg(all(test,unix))]
mod tests {
    use super::*;
    use super::super::{Class,Count,InputIdentity,Row,WatchStage,WatchViews};
    use super::super::loader_orchestration_owned::ProgramBinding;
    use std::{path::PathBuf,sync::atomic::{AtomicU64,Ordering}};

    struct Temporary(PathBuf);
    impl Temporary {
        fn new()->Self{Self::under(&std::env::temp_dir())}
        fn under(parent:&Path)->Self{
            static NEXT:AtomicU64=AtomicU64::new(0);
            let path=parent.join(format!("loader-continuations-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
            std::fs::create_dir(&path).expect("create test directory");Self(path)
        }
        fn receipt(&self,report:&Report){std::fs::write(self.0.join("continuations.json"),serde_json::to_vec(report).expect("serialize receipt")).expect("write receipt");}
    }
    impl Drop for Temporary {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
    fn synthetic_unqualified_report()->Report{
        let digest=Digest::of(b"synthetic unqualified loader observation");
        Report {schema:1,program:ProgramBinding::Retained,origin:None,identity:InputIdentity {run:digest.clone(),update:digest.clone(),update_file:FileBinding {path:"input-update.DAT".into(),sha256:digest.clone()},rom:digest.clone(),executable:digest.clone(),sources:Vec::new()},checkpoint:digest.clone(),instructions:Count(0),menu:Ok(Some([1,5,2])),error:None,machine_failure:None,events:Vec::new(),calls:Vec::new(),snapshots:Vec::new(),pulses:Vec::new(),epochs:Vec::new(),trace:FileBinding {path:"trace.json".into(),sha256:digest},static_calls:Vec::new()}
    }
    fn admitted_fixture(variable:&str)->(Temporary,Report,PathBuf){
        let source=PathBuf::from(std::env::var_os(variable).expect("set schema2 observation fixture path"));
        let analysis=PathBuf::from(std::env::var_os("LOADER_CONTINUATIONS_ANALYSIS").expect("set canonical analysis fixture path"));
        assert!(check(&source,&analysis).is_ok(),"fixture must independently admit");
        let output=Temporary::under(source.parent().expect("private fixture parent"));
        for entry in std::fs::read_dir(&source).expect("read fixture directory"){
            let entry=entry.expect("read fixture entry");
            if entry.file_name()!="continuations.json"&&entry.path().is_file(){std::os::unix::fs::symlink(entry.path(),output.0.join(entry.file_name())).expect("link immutable fixture artifact");}
        }
        let report:Report=serde_json::from_slice(&read(&source.join("continuations.json")).expect("read fixture receipt")).expect("parse fixture receipt");
        output.receipt(&report);(output,report,analysis)
    }
    fn watch(root:&Temporary,report:&Report,stage:WatchStage)->(usize,RawWatch){
        let index=report.epochs.iter().position(|epoch|epoch.stage==stage).expect("watch stage");
        let bytes=read(&root.0.join(&report.epochs[index].file.path)).expect("read raw watch");
        (index,serde_json::from_slice(&bytes).expect("parse raw watch"))
    }
    fn replace_watch(root:&Temporary,report:&mut Report,index:usize,raw:&RawWatch){
        #[derive(Serialize)]
        struct RewrittenWatch<'a> {enabled:bool,views:WatchViews,violation:Option<()>,classes:&'a [Class],records:&'a [Row]}
        let bytes=serde_json::to_vec(&RewrittenWatch {enabled:raw.enabled,views:raw.views,violation:None,classes:&raw.classes,records:&raw.records}).expect("serialize raw watch");
        let name="mutated-watch.json";std::fs::write(root.0.join(name),&bytes).expect("write raw watch");
        report.epochs[index].file=FileBinding {path:name.into(),sha256:Digest::of(&bytes)};report.epochs[index].record_count=raw.records.len();root.receipt(report);
    }
    #[derive(Deserialize)]
    struct ComparisonView {accepted:bool,outcome:ComparisonOutcome,effects:Option<FileBinding>,owned_effects:Option<FileBinding>}
    fn comparison(path:&Path)->ComparisonView{serde_json::from_slice(&read(&path.join("comparison.json")).expect("comparison receipt")).expect("parse comparison receipt")}
    fn bound_effect(path:&Path,binding:&FileBinding)->Digest{
        let digest=Digest::of(&read(&path.join(&binding.path)).expect("semantic effects"));
        assert_eq!(digest.as_str(),binding.sha256.as_str());digest
    }

    #[test]
    fn schema_one_cannot_admit_as_an_independent_observation(){
        let root=Temporary::new();root.receipt(&synthetic_unqualified_report());
        assert!(matches!(check(&root.0,&root.0.join("absent-analysis")),Err(reason) if reason=="observation did not complete"));
    }
    #[test]
    fn comparison_never_overwrites_an_existing_output_directory(){
        let root=Temporary::new();let sentinel=root.0.join("sentinel");std::fs::write(&sentinel,b"preserve").expect("write sentinel");
        assert!(compare(&root.0,&root.0,&root.0,&root.0).is_err());
        assert_eq!(read(&sentinel).expect("read sentinel"),b"preserve");
    }
    #[test]
    fn invalid_observation_comparison_binds_its_refused_input(){
        let root=Temporary::new();root.receipt(&synthetic_unqualified_report());let output=root.0.join("comparison");
        assert!(compare(&root.0,&root.0,&root.0.join("absent-analysis"),&output).is_err());
        let receipt=comparison(&output);assert!(!receipt.accepted);assert_eq!(receipt.outcome,ComparisonOutcome::RefusedRetainedObservation);assert!(receipt.effects.is_none()&&receipt.owned_effects.is_none());
        assert_eq!(read(&output.join("retained.json")).expect("bound retained receipt"),read(&root.0.join("continuations.json")).expect("input receipt"));
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn missing_origin_refuses_an_otherwise_admitted_observation(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");report.origin=None;root.receipt(&report);
        assert!(check(&root.0,&analysis).is_err());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn observed_carrier_fetch_on_the_other_core_is_refused(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");let (index,mut raw)=watch(&root,&report,WatchStage::Startup);
        let fetch=raw.records.iter_mut().find(|row|row.source==Access::CpuFetch&&carrier_side(row.address).is_some()).expect("carrier fetch");
        fetch.core=Some(if fetch.core==Some(Core::Primary){Core::Secondary}else{Core::Primary});replace_watch(&root,&mut report,index,&raw);
        assert!(check(&root.0,&analysis).is_err());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn relaxed_startup_views_cannot_be_rebound_into_admission(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");let (index,mut raw)=watch(&root,&report,WatchStage::Startup);
        report.epochs[index].views=WatchViews::Observe;raw.views=WatchViews::Observe;replace_watch(&root,&mut report,index,&raw);
        assert!(check(&root.0,&analysis).is_err());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn matching_forged_call_frames_do_not_replace_the_original_saved_frame(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");
        let call=report.calls.iter().find(|call|call.kind==CallKind::Loader&&call.returned.is_some()).expect("returning loader call").clone();
        for id in [call.before,call.entry,call.returned].into_iter().flatten(){report.events[id.0].frame.as_mut().expect("projected frame").values[16]^=1;}
        root.receipt(&report);assert!(check(&root.0,&analysis).is_err());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn carrier_execution_in_the_menu_tail_is_refused(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");
        let (_,startup)=watch(&root,&report,WatchStage::Startup);let source=startup.records.iter().find(|row|row.source==Access::CpuFetch&&carrier_side(row.address).is_some()).expect("carrier fetch");
        let (index,mut raw)=watch(&root,&report,WatchStage::Tail);
        raw.records.push(Row {source:source.source,address:source.address,physical:source.physical,size:source.size,core:source.core,pc:source.pc,instructions:Some(report.epochs[index].end),value:source.value});
        replace_watch(&root,&mut report,index,&raw);assert!(check(&root.0,&analysis).is_err());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED, LOADER_CONTINUATIONS_OWNED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn swapping_independently_admitted_roles_refuses_the_pair(){
        let (retained,_,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");let (owned,_,_)=admitted_fixture("LOADER_CONTINUATIONS_OWNED");let output=retained.0.join("swapped");
        assert!(compare(&owned.0,&retained.0,&analysis,&output).is_err());let receipt=comparison(&output);assert!(!receipt.accepted);assert_eq!(receipt.outcome,ComparisonOutcome::RefusedRoles);
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED, LOADER_CONTINUATIONS_OWNED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn admitted_raw_publication_difference_refuses_with_bound_effects(){
        let (retained,_,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");let (owned,mut report,_)=admitted_fixture("LOADER_CONTINUATIONS_OWNED");
        let baseline=retained.0.join("baseline");assert!(compare(&retained.0,&owned.0,&analysis,&baseline).is_ok());assert!(comparison(&baseline).accepted);
        let (index,mut raw)=watch(&owned,&report,WatchStage::Tail);
        let write=raw.records.iter_mut().find(|row|row.source==Access::CpuWrite&&row.physical==Guest(0x9662d4)&&row.value.is_some()).expect("later publication");
        write.value=write.value.map(|value|value^1);replace_watch(&owned,&mut report,index,&raw);assert!(check(&owned.0,&analysis).is_ok());
        let output=retained.0.join("mismatch");assert!(compare(&retained.0,&owned.0,&analysis,&output).is_err());let receipt=comparison(&output);
        assert!(!receipt.accepted);assert_eq!(receipt.outcome,ComparisonOutcome::RefusedSemanticMismatch);
        let retained_digest=bound_effect(&output,receipt.effects.as_ref().expect("retained effects"));let owned_digest=bound_effect(&output,receipt.owned_effects.as_ref().expect("owned effects"));assert_ne!(retained_digest.as_str(),owned_digest.as_str());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn repaired_hashes_cannot_replace_a_missing_executed_origin_fetch(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");let (index,mut raw)=watch(&root,&report,WatchStage::Startup);
        let origin=&report.events[report.origin.expect("carrier origin").0];let clock=origin.instructions.0+1;
        let before=raw.records.len();raw.records.retain(|row|!(row.source==Access::CpuFetch&&row.address==origin.pc&&row.core==Some(origin.core)&&row.instructions==Some(Count(clock))));assert!(raw.records.len()<before);
        replace_watch(&root,&mut report,index,&raw);assert!(check(&root.0,&analysis).is_err());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_OWNED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn rebinding_a_modified_owned_manifest_does_not_replace_the_reviewed_program(){
        #[derive(Serialize,Deserialize)]
        enum OwnedRole {#[serde(rename="owned")] Owned}
        #[derive(Serialize,Deserialize)]
        struct BoundFiles {manifest:FileBinding,loaders:[FileBinding;2]}
        #[derive(Serialize,Deserialize)]
        struct OwnedWire {role:OwnedRole,artifacts:BoundFiles}
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_OWNED");
        let mut program:OwnedWire=serde_json::from_slice(&serde_json::to_vec(&report.program).expect("serialize program binding")).expect("owned program binding");
        let mut bytes=read(&root.0.join(&program.artifacts.manifest.path)).expect("read reviewed manifest");bytes.push(b'\n');
        let path="rebound-manifest.json";std::fs::write(root.0.join(path),&bytes).expect("write modified manifest");program.artifacts.manifest=FileBinding {path:path.into(),sha256:Digest::of(&bytes)};
        report.program=serde_json::from_slice(&serde_json::to_vec(&program).expect("serialize rebound program")).expect("parse rebound program");root.receipt(&report);
        assert!(check(&root.0,&analysis).is_err());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn origin_retry_fetch_at_the_rotation_clock_keeps_its_executed_fetch_obligation(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");let (index,mut raw)=watch(&root,&report,WatchStage::Startup);
        let origin=&report.events[report.origin.expect("carrier origin").0];
        raw.records.insert(0,Row {source:Access::CpuFetch,address:origin.pc,physical:origin.pc,size:4,core:Some(origin.core),pc:Some(origin.pc),instructions:Some(origin.instructions),value:None});
        replace_watch(&root,&mut report,index,&raw);assert!(check(&root.0,&analysis).is_ok());
    }
    #[test]
    #[ignore="requires LOADER_CONTINUATIONS_RETAINED and LOADER_CONTINUATIONS_ANALYSIS"]
    fn changed_update_bytes_cannot_be_admitted_by_rebinding_the_leaf(){
        let (root,mut report,analysis)=admitted_fixture("LOADER_CONTINUATIONS_RETAINED");
        let bytes=b"synthetic substituted DAT";let path="substituted-update.DAT";
        std::fs::write(root.0.join(path),bytes).expect("write substituted update");report.identity.update_file.path=path.into();
        root.receipt(&report);assert!(check(&root.0,&analysis).is_err());
        report.identity.update_file.sha256=Digest::of(bytes);root.receipt(&report);assert!(check(&root.0,&analysis).is_err());
    }
}
