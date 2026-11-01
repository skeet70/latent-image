use super::{Access, CallKind, Catalog, Core, Event, Guest, Key, RawWatch, ReleaseOutcome, Report};
use super::continuation_view::{Digest, FileBinding, Mapping, Resource, Words};
use fujimod_xm1::bridge::read;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, fs::File, io::Read, path::{Component, Path}};

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
enum Scope { CanonicalNaturalObservation }
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

fn projections(report:&Report)->Result<(),String>{
    let mut required=Vec::new();let mut image5=false;let mut plain=[false;3];
    for observed in &report.events {
        let resources:&[Resource]=match observed.pc.0{
            0x409ac=>&[Resource::LoaderA],0xd09ac=>&[Resource::LoaderB],
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

pub(super) fn check(root:&Path,catalog:&Catalog)->Result<Verification,String>{
    let receipt=read(&root.join("continuations.json"))?;let report:Report=serde_json::from_slice(&receipt).map_err(|error|error.to_string())?;
    require(report.schema==1&&report.error.is_none()&&report.machine_failure.is_none(),"observation did not complete")?;
    require(report.identity.update.as_str()=="43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"&&report.identity.run.as_str()=="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"&&report.identity.rom.as_str()=="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be","canonical input identity differs")?;
    require(report.identity.sources.len()==catalog.0.len(),"source catalog extent differs")?;
    for image in &catalog.0 {require(report.identity.sources.iter().filter(|source|source.kind==image.binding.kind&&source.start==image.binding.start&&source.bytes.0==image.binding.bytes.0&&source.sha256.as_str()==image.binding.sha256.as_str()).count()==1,"source catalog binding differs")?;}
    require(Digest::of(&read(&root.join("reset.fujimod"))?).as_str()==report.checkpoint.as_str(),"reset checkpoint identity differs")?;
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
        require(!matches!(event.pc.0,0x40aa8|0xd0aa8|0x69d2d8),"source nonreturning boundary was reached")?;
    }
    require(report.epochs.len()==2&&report.epochs[0].start.0==0&&report.epochs[0].end.0==8_000_000&&report.epochs[1].start==report.epochs[0].end&&report.epochs[1].end==report.instructions,"classified watch interval differs")?;
    for epoch in &report.epochs {
        let bytes=read(&artifact(root,&epoch.file)?)?;require(Digest::of(&bytes).as_str()==epoch.file.sha256.as_str(),"raw watch identity differs")?;let raw:RawWatch=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
        require(epoch.enabled&&!epoch.refused&&epoch.teardown_error.is_none()&&raw.enabled&&raw.violation.is_none()&&raw.records.len()==epoch.record_count,"classified watch is incomplete")?;
        for &(start,end) in &super::DIAGNOSTICS {require(guarded(&raw.classes,[u64::from(start),u64::from(end)],true),"diagnostic watch coverage absent")?;}
        for span in [[0x409ac,0x40b00],[0xd09ac,0xd0b00]]{require(guarded(&raw.classes,span,epoch.start.0!=0),"loader carrier watch coverage absent")?;}
        for span in [[0x61c000,0x61c220],[0x40000,0x40020],[0xd0000,0xd0020],[0x4f10c,0x4f118],[0xdf0cc,0xdf0d8],[0x89f794,0x89f798],[0xa00c4,0xa00c8],[0x5927c,0x59280],[0xe923c,0xe9240],[0x96bd20,0x96bd28],[0x89f8ac,0x89f8b0],[0x944440,0x944464],[0x914834,0x914838],[0x972e88,0x972e94],[0x14c100c,0x14c1010],[0x14c1018,0x14c101c],[0x14c113c,0x14c1140],[0x9662d4,0x9662d8],[0x89f784,0x89f788],[0x89f78c,0x89f790]]{require(guarded(&raw.classes,span,false),"shared publication watch coverage absent")?;}
        if epoch.start.0==0{for span in [[0x63450,0x63460],[0xf3410,0xf3420]]{require(guarded(&raw.classes,span,false),"saved loader frame watch coverage absent")?;}}
        let mut previous_cpu=epoch.start.0;
        for row in &raw.records {
            require(in_class(row,&raw.classes)?,"raw access lies outside its declared classes")?;
            if matches!(row.source,Access::CpuRead|Access::CpuFetch|Access::CpuWrite){let count=row.instructions.ok_or("CPU access clock absent")?.0;require(row.core.is_some()&&row.pc.is_some()&&epoch.start.0<count&&count<=epoch.end.0&&previous_cpu<=count,"CPU access attribution or clock differs")?;previous_cpu=count;}
            let end=u64::from(row.physical.0)+row.size;
            if matches!(row.source,Access::CpuFetch){for &(start,finish) in &super::DIAGNOSTICS{require(end<=u64::from(start)||u64::from(finish)<=u64::from(row.physical.0),"diagnostic instruction fetch observed")?;}}
            for (start,finish,core) in [(0x409ac,0x40b00,Core::Primary),(0xd09ac,0xd0b00,Core::Secondary)] {
                if u64::from(row.physical.0)>=finish||end<=u64::from(start){continue;}
                let first=report.events.iter().find(|event|event.pc.0==start&&event.core==core).ok_or("loader carrier origin absent")?;
                match row.source {
                    Access::CpuFetch=>return Err("loader continuation executes after the admitted prefix".into()),
                    Access::CpuWrite=>require(row.instructions.is_some_and(|count|count.0<first.instructions.0),"loader carrier changed after its entry projection")?,
                    Access::HostWrite|Access::ModelBusWrite=>return Err("unattributed writer could reach an entered loader carrier".into()),
                    _=>{},
                }
            }
            for (start,finish,pc,core) in [(0x63450,0x63460,0x409ac,Core::Primary),(0xf3410,0xf3420,0xd09ac,Core::Secondary)] {
                if u64::from(row.physical.0)>=finish||end<=start{continue;}
                require(!matches!(row.source,Access::HostWrite|Access::ModelBusWrite),"unattributed write could reach the live saved loader frame")?;
                let entry=report.events.iter().find(|event|event.pc.0==pc&&event.core==core).ok_or("loader frame origin absent")?;
                let transfer=report.events.iter().find(|event|event.pc.0==pc+0x2d3c&&event.core==core).ok_or("loader frame endpoint absent")?;
                if row.instructions.is_some_and(|count|entry.instructions.0<=count.0&&count.0<=transfer.instructions.0){require(matches!(row.source,Access::CpuWrite)&&row.pc==Some(Guest(pc))&&row.core==Some(core),"foreign writer reached the live saved loader frame")?;}
            }
        }
    }
    let mut loader_calls=0;let mut task_calls=0;
    for call in &report.calls {
        let entered=event(&report,call.entry.ok_or("observed call lacks its actual entry")?)?;
        require(entered.pc==call.target&&entered.core==call.core&&entered.registers.sp==call.sp&&entered.registers.lr==call.continuation,"callee entry context differs")?;
        if let Some(before)=call.before {let before=event(&report,before)?;require(before.core==call.core&&before.registers.sp==call.sp&&before.pc.0.checked_add(4)==Some(call.continuation.0)&&branch(before,catalog)?==call.target&&before.instructions.0<=entered.instructions.0&&before.registers.r==entered.registers.r&&before.registers.cpsr==entered.registers.cpsr,"source call/target argument context or chronology differs")?;
            let expected_kind=match before.pc.0{0xd985c0=>CallKind::Image5Wait,0x69d2e0=>CallKind::Image5Entry,0xd9858c=>CallKind::Image5Constructor,0x73b554=>CallKind::PlainProducer,0x73be5c=>CallKind::PlainHook,0x73beac=>CallKind::Deferred,pc if (0x409ac..0x40af4).contains(&pc)||(0xd09ac..0xd0af4).contains(&pc)=>CallKind::Loader,pc if (0xd985a4..0xd986ac).contains(&pc)=>CallKind::Image5Service,pc if (0x6a9844..0x6a9934).contains(&pc)=>CallKind::PlainService,_=>return Err("call origin is outside the observed source boundaries".into())};require(call.kind==expected_kind,"call kind does not match its raw source boundary")?;
            if call.kind==CallKind::Loader {
                loader_calls+=1;let (core,sp)=if (0x409ac..0x40af4).contains(&before.pc.0){(Core::Primary,0x40063450)}else if (0xd09ac..0xd0af4).contains(&before.pc.0){(Core::Secondary,0x400f3410)}else{return Err("loader call origin differs".into());};
                require(call.core==core&&before.registers.sp.0==sp&&before.registers.r[11]==sp+12&&before.registers.cpsr&0xff==0xd3,"canonical loader caller context differs")?;
                let phase=match before.pc.0&0xffff{0x09fc=>Some(0),0x0a34=>Some(1),0x0a3c=>Some(3),0x0a68=>Some(2),0x0aec=>Some(4),_=>None};if let Some(phase)=phase{require(call.target.0==0x69cdf8&&entered.registers.r[0]==phase,"phase argument or target differs")?;}
            }
            if call.kind==CallKind::PlainHook {let hook=Guest(before.registers.r[10]);let (_,expected)=catalog.word(hook)?;require(expected==call.target.0&&matches!(before.registers.r[7],0|3),"plain hook does not match its source list")?;}
            if call.kind==CallKind::Image5Entry {let descriptor=before.state.iter().find(|state|state.mapping.guest.0==before.registers.r[0]&&state.values.len()==2).ok_or("image5 descriptor view absent")?;let header=before.state.iter().find(|state|state.mapping.guest.0==descriptor.values[1]&&state.values.len()==4).ok_or("image5 header view absent")?;require(header.values[3]==call.target.0&&call.target.0==0xd985a4,"image5 header transfer differs")?;}
            if call.kind==CallKind::Image5Wait{require(call.target.0==0x73aca4&&entered.registers.r[0]==7,"image5 early wait service or argument differs")?;}
        }else{require(call.kind==CallKind::Task&&entered.pc.0==0x725dd8,"entry-only call is not a task submission")?;}
        if matches!(call.target.0,0x43688|0xd3688){require(call.returned.is_none(),"initial scheduler unexpectedly returned to the loader")?;}
        if let Some(returned)=call.returned {let returned=event(&report,returned)?;require(returned.pc==call.continuation&&returned.core==call.core&&returned.registers.sp==call.sp&&returned.instructions.0>=entered.instructions.0&&returned.registers.r[4..12]==entered.registers.r[4..12],"retained return state or chronology differs")?;
            if call.kind==CallKind::Loader{let before=event(&report,call.before.ok_or("loader pre-call event absent")?)?;require(projected_frame(before)?==projected_frame(returned)?,"loader saved frame changed across a call")?;if call.target.0==0x69cdf8{require(returned.registers.r[0]==if entered.registers.r[0]==2{0x69cc6c}else{0},"dispatcher result differs")?;}}
            if call.kind==CallKind::Task{task_calls+=1;let descriptor=entered.descriptor.as_ref().ok_or("task descriptor entry absent")?;let after=returned.descriptor.as_ref().ok_or("task descriptor return absent")?;require(descriptor.values.len()==7&&after.values.len()==7&&descriptor.mapping.guest.0==entered.registers.r[0]&&after.mapping.guest==descriptor.mapping.guest&&call.descriptor==Some(descriptor.mapping.guest),"task descriptor alias/extent differs")?;}
        }else{require(call.kind==CallKind::Loader&&matches!(call.target.0,0x43688|0xd3688),"a required retained call has not returned")?;}
    }
    let mut phases=[Vec::new(),Vec::new()];let mut constructors=Vec::new();
    for observed in &report.events {
        let counted=((0x409ac..0x40af4).contains(&observed.pc.0)||(0xd09ac..0xd0af4).contains(&observed.pc.0)||(0xd985a4..0xd986ac).contains(&observed.pc.0)||(0x6a9844..0x6a9934).contains(&observed.pc.0)||matches!(observed.pc.0,0x73b554|0x69d2e0|0xd9858c|0x73be5c|0x73beac))&&branch(observed,catalog).is_ok();
        if counted{require(report.calls.iter().filter(|call|call.before.is_some_and(|id|id.0==observed.id.0)).count()==1,"observed source call was omitted or repeated")?;}
        if observed.pc.0==0x725dd8{require(report.calls.iter().filter(|call|call.kind==CallKind::Task&&call.entry.is_some_and(|id|id.0==observed.id.0)).count()==1,"observed task entry was omitted or repeated")?;}
        if matches!(observed.pc.0,0x409fc|0x40a34|0x40a3c|0x40a68|0x40aec|0xd09fc|0xd0a34|0xd0a3c|0xd0a68|0xd0aec){phases[usize::from(observed.core)].push(observed.registers.r[0]);}
        if observed.pc.0==0xd9858c{constructors.push(branch(observed,catalog)?.0);}
    }
    require(phases==[vec![0,1,3,2],vec![4,2]],"natural loader phase sequence differs")?;
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
        let callback=report.events.iter().find(|event|event.pc.0==base+0xa70&&event.core==core).ok_or("callback publisher return absent")?;require(at(callback,cell)?==0x69cc6c,"committed diagnostic callback differs")?;
    }
    projections(&report)?;
    for sample in &report.snapshots {require(sample.event.0<report.events.len(),"snapshot event absent")?;snapshot(root,sample,catalog)?;}
    require(report.calls.iter().filter(|call|call.kind==CallKind::Image5Entry).count()==1,"image5 header call extent differs")?;
    let setters:Vec<_>=report.calls.iter().filter(|call|call.kind==CallKind::Image5Service&&call.target.0==0x69cf84).collect();require(setters.len()==1,"diagnostic callback setter call census differs")?;
    let setter=event(&report,setters[0].entry.ok_or("setter entry absent")?)?;let setter_return=event(&report,setters[0].returned.ok_or("setter return absent")?)?;require(setter.registers.r[0]==0xed4a8c&&at(setter_return,0x89f794)?==0xed4a8c,"installed diagnostic callback or committed cell differs")?;
    Ok(Verification {schema:1,scope:Scope::CanonicalNaturalObservation,receipt:Digest::of(&receipt),events:report.events.len(),loader_calls,task_calls,snapshots:report.snapshots.len(),source_differences:report.snapshots.iter().filter(|sample|!sample.expected_equal).count()})
}
