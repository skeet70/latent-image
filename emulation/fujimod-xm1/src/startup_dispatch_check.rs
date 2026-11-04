use super::{Attributes, Boundary, Case, Core, Digest, Invocation, PerturbationKind, Phase, RamView, Report, Role, Row, Service, Source};
use fujimod_xm1::bridge::{read, sha256};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::collections::BTreeMap;

const LOW:u32=0x69cdf8;
const HIGH:u32=0x69cf84;
#[derive(Deserialize)]
struct Word {guest:u32,physical:u32,expected:u32,tcm:[[u32;2];2]}
#[derive(Deserialize)]
pub(super) struct Hit {address:String,core:usize,instructions:u64,cpsr:u32,registers:BTreeMap<String,String>,sp:String,lr:String,code_word:Option<Word>}
#[derive(Deserialize)]
struct Class {range:[u64;2],kinds:Vec<Source>}
#[derive(Deserialize)]
struct RawWatch {enabled:bool,violation:Option<String>,records:Vec<Row>,classes:Vec<Class>}
#[derive(Debug,PartialEq,Serialize)]
struct Task {fields:[u32;6],before:u32,after:u32,status:u32,attribute:Option<u32>}
#[derive(Debug,PartialEq,Serialize)]
struct Outcome {core:usize,phase:Phase,sp:u32,live:[u32;8],result:u32,cpsr:u32,tasks:Vec<Task>}
#[derive(Serialize)]
struct Comparison {schema:u32,case:Option<Case>,accepted:bool,retained:Digest,owned:Digest,outcomes:Vec<Outcome>,error:Option<String>}
fn require(condition:bool,message:&str)->Result<(),String>{if condition{Ok(())}else{Err(message.into())}}
fn number(value:&str)->Result<u32,String>{u32::from_str_radix(value.strip_prefix("0x").ok_or("register prefix absent")?,16).map_err(|error|error.to_string())}
fn correlate(boundary:&Boundary,trace:&[Hit])->Result<(),String>{
    let hit=trace.iter().find(|hit|hit.core==usize::from(boundary.core)&&number(&hit.address)==Ok(boundary.pc.0)&&hit.instructions==boundary.instructions.0).ok_or("boundary has no raw trace event")?;
    require(number(&hit.sp)?==boundary.registers.sp.0 && number(&hit.lr)?==boundary.registers.lr.0 && hit.cpsr==boundary.registers.cpsr,"boundary register metadata differs from raw trace")?;
    for index in 0..13 {let name=format!("r{index}");require(number(hit.registers.get(&name).ok_or("raw register absent")?)?==boundary.registers.r[index],"boundary GPR differs from raw trace")?;}
    Ok(())
}
fn word(view:&RamView,address:u32)->Result<u32,String>{
    require(view.words==view.physical_words && view.other_words==view.physical_words,"committed views disagree")?;
    require(matches!(view.decoder,1|2) && view.physical.0==view.address.0 & !0x40000000,"RAM backing differs")?;
    require(address>=view.address.0 && (address-view.address.0)%4==0,"sample address is outside view")?;
    for banks in view.tcm {for bank in banks {if bank&1!=0 && (bank>>2)&31!=0 {let size=(bank>>2)&31;require((3..=14).contains(&size),"reserved TCM size")?;let low=u64::from(bank&0xfffff000);let end=u64::from(view.address.0)+4*view.words.len() as u64;require(low>=end || low+(512u64<<size)<=u64::from(view.address.0),"view overlaps TCM")?;}}}
    view.physical_words.get(((address-view.address.0)/4) as usize).copied().ok_or_else(||"sample word is absent".into())
}
fn caller(boundary:&Boundary)->Result<(),String>{
    require(boundary.registers.sp.0>=256 && boundary.frame.address.0==boundary.registers.sp.0-256 && boundary.frame.words.len()==68,"caller stack view is absent")?;
    word(&boundary.frame,boundary.registers.sp.0)?;
    Ok(())
}
fn fields(phase:Phase,index:usize)->Result<[u32;6],String>{match(phase,index){
    (Phase::Root,0)=>Ok([104,0x781960,1,1024,0x69d160,0]),
    (Phase::Idle,0)=>Ok([102,0x781968,15,1024,0x69cbf0,0]),
    (Phase::Peer,0)=>Ok([103,0x781970,15,1024,0x69cbf0,0]),
    (Phase::Peer,1)=>Ok([93,0x781978,13,1024,0x69cd58,0]),
    _=>Err("unexpected task submission".into())}}
fn attribute(value:&Attributes,descriptor:u32,rows:&[Row])->Result<u32,String>{
    caller(&value.boundary)?;
    require(value.boundary.pc.0==0x725ed0 && value.boundary.registers.r[4]==descriptor,"attribute consumer context differs")?;
    let observed=word(&value.word,descriptor+24)?;
    let read=rows.iter().rev().find(|row|row.source==Source::CpuRead && row.core==Some(value.boundary.core) && row.pc.is_some_and(|pc|pc.0==0x725ebc) && row.physical.0==(descriptor+24)&!0x40000000 && row.instructions.is_some_and(|count|count.0<=value.boundary.instructions.0)).ok_or("attribute lacks an actual consumer read")?;
    require(read.size==4 && read.value==Some(u64::from(observed)),"attribute sample differs from consumed word")?;
    let expected=if observed==0xa5f0a5f0 {0}else{4};
    require(value.boundary.registers.r[3]==expected,"retained attribute projection differs")?;
    Ok(expected)
}
struct Interval {after:super::Count,through:super::Count}
fn watch_contract(classes:&[Class],epoch:usize,case:Case)->Result<(),String>{
    let require_range=|range:[u64;2],fetch:bool|{
        let kinds=&[Source::CpuRead,Source::CpuWrite,Source::HostWrite,Source::ModelBusWrite];
        require(classes.iter().any(|class|class.range==range && kinds.iter().all(|kind|class.kinds.contains(kind)) && (!fetch || class.kinds.contains(&Source::CpuFetch))),"required physical watch or access class absent")
    };
    require_range([u64::from(LOW),u64::from(HIGH)],true)?;
    for range in [[0x4f114,0x4f118],[0xdf0d4,0xdf0d8]]{require_range(range,false)?;}
    if epoch==0 {for range in [[0x63350,0x63460],[0xf3310,0xf3420]]{require_range(range,false)?;}
        if case==Case::ReservedAlias{require_range([0xa1010c,0xa10118],false)?;}
    }
    Ok(())
}
fn scratch_overlap(invocation:&Invocation,role:Role,row:&Row,end:u64)->Result<bool,String>{
    let physical=u64::from(invocation.entry.registers.sp.0&!0x40000000);
    let mut low=physical-match role{Role::Retained=>48,Role::Owned=>72};
    if let Some(call)=row.instructions.and_then(|count|invocation.calls.iter().find(|call|call.entry.instructions.0<=count.0 && call.returned.as_ref().is_none_or(|returned|count.0<=returned.instructions.0))){
        let incoming=u64::from(call.entry.registers.sp.0&!0x40000000);
        require(low<=incoming && incoming<=physical-48,"retained SP exceeds the admitted frame")?;
        low=incoming;
    }
    Ok((u64::from(row.physical.0)<physical-44 && low<end)||(u64::from(row.physical.0)<physical && physical-16<end))
}
fn ordered_stores(rows:&[Row],core:Core,descriptor:u32,fields:[u32;6],interval:Interval)->Result<(),String>{
    let mut positions=[0;6];
    for (index,expected) in fields.into_iter().enumerate(){
        let address=(descriptor+4*index as u32)&!0x40000000;
        let (position,stored)=rows.iter().enumerate().rev().find(|(_,row)|row.source==Source::CpuWrite && row.core==Some(core) && row.physical.0==address && row.instructions.is_some_and(|count|interval.after.0<count.0 && count.0<=interval.through.0) && row.pc.is_some_and(|pc|(LOW..HIGH).contains(&pc.0))).ok_or("assigned field lacks a store in this submission interval")?;
        require(stored.size==4 && stored.value==Some(u64::from(expected)),"assigned-field store differs")?;
        positions[index]=position;
    }
    require(positions.windows(2).all(|pair|pair[0]<pair[1]),"descriptor stores are not ordered")
}
fn service_interval(service:Service,entry:&Boundary,returned:&Boundary,interval:Interval)->Result<(),String>{
    let pc=match service{Service::Mode=>0x657184,Service::Clear=>0x7221a0,Service::Install=>0x725a04,Service::Task=>0x725dd8};
    require(entry.pc.0==pc && interval.after.0<=entry.instructions.0 && entry.instructions.0<=returned.instructions.0 && returned.instructions.0<=interval.through.0,"service label or chronological containment differs")
}

fn project(invocation:&Invocation,rows:&[Row])->Result<Outcome,String>{
    let lr=match(invocation.entry.core,invocation.phase){(Core::Primary,Phase::Install)=>0x40a00,(Core::Primary,Phase::Root)=>0x40a38,(Core::Primary,Phase::Idle)=>0x40a40,(Core::Primary,Phase::Diagnostic)=>0x40a6c,(Core::Secondary,Phase::Peer)=>0xd0af0,(Core::Secondary,Phase::Diagnostic)=>0xd0a6c,_=>return Err("unadmitted caller".into())};
    project_at(invocation,rows,lr)
}
fn project_at(invocation:&Invocation,rows:&[Row],lr:u32)->Result<Outcome,String>{
    let entered=&invocation.entry; let returned=invocation.returned.as_ref().ok_or("caller return absent")?;
    caller(entered)?;caller(returned)?;
    let sp=if entered.core==Core::Primary {0x40063450}else{0x400f3410};
    require(entered.pc.0==LOW && entered.registers.sp.0==sp && entered.registers.lr.0==lr && returned.pc.0==lr && returned.core==entered.core && returned.registers.sp.0==sp,"caller frame or continuation differs")?;
    require(entered.registers.r[4..12]==returned.registers.r[4..12],"caller-live registers changed")?;
    require(returned.registers.r[0]==if invocation.phase==Phase::Diagnostic{0x69cc6c}else{0},"consumed return differs")?;
    let expected:&[Service]=match invocation.phase{Phase::Install=>&[Service::Mode,Service::Install],Phase::Diagnostic=>&[],Phase::Peer=>&[Service::Task,Service::Task],_=>&[Service::Task]};
    require(invocation.calls.iter().map(|call|call.service).eq(expected.iter().copied()),"retained call order differs")?;
    let mut tasks=Vec::new();let mut control=entered.registers.cpsr;let mut previous=entered.instructions;
    for call in &invocation.calls {
        let after=call.returned.as_ref().ok_or("retained return absent")?;caller(&call.entry)?;caller(after)?;
        service_interval(call.service,&call.entry,after,Interval {after:previous,through:returned.instructions})?;
        require(after.pc==call.entry.registers.lr && (LOW..HIGH).contains(&after.pc.0) && after.core==entered.core && call.entry.core==entered.core && after.registers.sp==call.entry.registers.sp && after.registers.r[4..12]==call.entry.registers.r[4..12],"retained return premise differs")?;
        control=after.registers.cpsr;
        if call.service==Service::Task {
            let descriptor=sp-44;let before=call.descriptor_before.as_ref().ok_or("descriptor preimage absent")?;let post=call.descriptor_after.as_ref().ok_or("descriptor postimage absent")?;
            let expected=fields(invocation.phase,tasks.len())?;
            require(call.entry.pc.0==0x725dd8 && call.entry.registers.r[0]==descriptor && before.address.0==descriptor && post.address.0==descriptor,"descriptor storage was not reused")?;
            for (index,expected) in expected.iter().enumerate(){require(word(before,descriptor+4*index as u32)?==*expected,"consumer descriptor differs")?;}
            ordered_stores(rows,entered.core,descriptor,expected,Interval {after:previous,through:call.entry.instructions})?;
            let domain=call.domain.as_ref().ok_or("domain view absent")?;let roots=call.roots.as_ref().ok_or("roots view absent")?;
            require(word(domain,0x7cad9c+4*(expected[0]-1))?==if entered.core==Core::Primary{1}else{2},"task domain differs")?;
            require(word(roots,0x96bd20)?==0x4004dae0 && word(roots,0x96bd24)?==0x400ddaa0,"installed roots differ")?;
            let flags=call.attributes.as_ref().map(|value|attribute(value,descriptor,rows)).transpose()?;
            tasks.push(Task {fields:expected,before:word(before,descriptor+24)?,after:word(post,descriptor+24)?,status:after.registers.r[0],attribute:flags});
        }else{require(after.registers.r[0]==0,"selected mode0/setup outcome differs")?;}
        previous=after.instructions;
    }
    require(returned.registers.cpsr&0x0fffffff==control&0x0fffffff,"non-NZCV caller state differs beyond retained effects")?;
    Ok(Outcome {core:usize::from(entered.core),phase:invocation.phase,sp,live:entered.registers.r[4..12].try_into().map_err(|_|"live-register extent")?,result:returned.registers.r[0],cpsr:returned.registers.cpsr&0x0fffffff,tasks})
}
pub(super) fn control_fields(report:&Report)->Result<(),String>{
    require(report.error.is_none() && report.machine_failure.is_none() && report.case==Case::ReservedAlias,"perturbed control failed")?;
    let peer=report.invocations.iter().find(|invocation|invocation.entry.core==Core::Secondary && invocation.phase==Phase::Peer).ok_or("phase4 control absent")?;
    require(peer.returned.as_ref().is_some_and(|boundary|boundary.pc.0==0xd0af0 && boundary.registers.r[0]==0 && boundary.registers.sp==peer.entry.registers.sp),"phase4 control did not return after both submissions")?;
    require(peer.calls.len()==2 && peer.calls[0].returned.as_ref().is_some_and(|boundary|boundary.registers.r[0]==(-18i32) as u32),"first retained rejection was not observed")?;
    require(peer.calls[0].attributes.is_none(),"rejected submission reached attribute consumer")?;
    let attribute=peer.calls[1].attributes.as_ref().ok_or("second submission did not consume the alias word")?;
    require(word(&attribute.word,0x400f33fc)?==0xa5f0a5f0 && attribute.boundary.registers.r[3]==0,"second consumer did not observe the alias mutation")?;
    require(report.perturbations.len()==2,"control perturbation count differs")?;
    let reserved=&report.perturbations[0];let alias=&report.perturbations[1];
    require(reserved.kind==PerturbationKind::ReservedRecord && reserved.pc.0==0x725dd8 && reserved.core==Core::Secondary && reserved.after.address.0==0x40a1010c && reserved.after.words==[0xa10200,0x400,1],"reserved-error control differs")?;
    require(reserved.instructions.0==peer.calls[0].entry.instructions.0,"reserved perturbation is not at first task entry")?;
    require(alias.kind==PerturbationKind::DescriptorAlias && alias.core==Core::Secondary && alias.after.address.0==0x400f33fc && alias.after.words==[0xa5f0a5f0] && peer.calls[0].returned.as_ref().is_some_and(|boundary|boundary.pc==alias.pc && boundary.instructions.0==alias.instructions.0),"alias control is not at the first retained return")?;
    Ok(())
}
fn check(directory:&Path,role:Role,source:&[u8],linked:&[u8])->Result<(Case,Vec<Outcome>,Digest),String>{
    let bytes=read(&directory.join("dispatch.json"))?;let digest=Digest(sha256(&bytes));let report:Report=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
    require(report.schema==1 && report.role==role && report.error.is_none() && report.machine_failure.is_none(),"receipt did not complete")?;
    let update=match role{Role::Retained=>"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490",Role::Owned=>"0638ea6db8f3326660e67a4db8ac5ef065eb4d921ae921ffd1c24c925448523c"};
    require(report.identity.update.0==update && report.identity.source_image.0==sha256(source) && report.identity.linked_image.0==sha256(linked) && report.identity.run.0=="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" && report.identity.rom.0=="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be","input identity differs")?;
    require(super::file_digest(&directory.join("reset.fujimod"))?.0==report.identity.checkpoint.0,"checkpoint identity differs")?;
    let trace_bytes=read(&directory.join("trace.json"))?;require(sha256(&trace_bytes)==report.files.trace.0,"trace sidecar differs")?;
    let trace:Vec<Hit>=serde_json::from_slice(&trace_bytes).map_err(|error|error.to_string())?;
    let mut expected=if role==Role::Retained{source[(LOW-0x61c400) as usize..(HIGH-0x61c400) as usize].to_vec()}else{linked.to_vec()};while expected.len()<396{expected.extend_from_slice(&0xe320f000u32.to_le_bytes());}
    require(super::file_digest(&directory.join("bound-code.bin"))?.0==sha256(&expected) && report.files.code.0==sha256(&expected),"bound carrier differs")?;
    let mut sampled=[[false;99];2];let mut counts=[0;2];
    for hit in &trace {let pc=u32::from_str_radix(hit.address.strip_prefix("0x").ok_or("trace address prefix")?,16).map_err(|error|error.to_string())?;
        if !(LOW..HIGH).contains(&pc){continue;}require(hit.core<2 && pc%4==0 && hit.cpsr&0x20==0,"carrier execution state differs")?;
        let code=hit.code_word.as_ref().ok_or("carrier word was not sampled")?;let offset=(pc-LOW) as usize;let expected=u32::from_le_bytes(expected[offset..offset+4].try_into().map_err(|_|"carrier word extent")?);
        require([code.guest,code.physical,code.expected]==[expected;3],"code word differs from independently bound bytes")?;
        for bank in code.tcm.into_iter().flatten(){if bank&1!=0 && (bank>>2)&31!=0{let size=(bank>>2)&31;require((3..=14).contains(&size),"code TCM size reserved")?;let low=u64::from(bank&0xfffff000);require(low>=u64::from(HIGH)||low+(512u64<<size)<=u64::from(LOW),"code TCM overlap")?;}}
        require(hit.instructions<=report.instructions.0,"trace extends beyond endpoint")?;sampled[hit.core][offset/4]=true;counts[hit.core]+=1;
    }
    require(counts==report.code_samples && !counts.contains(&0),"both-core code sample accounting differs")?;
    let mut rows=Vec::new();let mut fetches=[0;2];let mut fetched=false;
    let sidecars=if report.case==Case::Natural{vec![("watch-prefix.json",&report.files.prefix),("watch-tail.json",report.files.tail.as_ref().ok_or("tail digest absent")?)]}else{vec![("watch-prefix.json",&report.files.prefix)]};
    require(report.segments.len()==sidecars.len() && report.segments.first().is_some_and(|segment|segment.start.0==0) && report.segments.last().is_some_and(|segment|segment.end.0==report.instructions.0),"watch interval differs")?;
    for (index,(name,digest)) in sidecars.iter().enumerate(){let bytes=read(&directory.join(name))?;require(sha256(&bytes)==digest.0,"access sidecar identity differs")?;let watch:RawWatch=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
        require(watch.enabled && watch.violation.is_none() && report.segments[index].teardown_error.is_none(),"watch refused")?;
        if index>0{require(report.segments[index-1].end.0==report.segments[index].start.0,"watch execution gap")?;}
        watch_contract(&watch.classes,index,report.case)?;
        for row in watch.records {let end=u64::from(row.physical.0).checked_add(row.size).ok_or("access overflow")?;
            if row.physical.0<HIGH && u64::from(LOW)<end{match row.source{Source::CpuFetch=>{let core=usize::from(row.core.ok_or("fetch core absent")?);require(row.size==4 && row.address==row.physical && row.physical.0%4==0 && sampled[core][((row.physical.0-LOW)/4) as usize],"unsampled carrier fetch")?;require(row.instructions.is_some_and(|count|count.0<=report.segments[0].end.0),"late carrier execution")?;fetches[core]+=1;fetched=true;},Source::CpuWrite|Source::HostWrite|Source::ModelBusWrite if fetched=>return Err("carrier rewritten after first fetch".into()),_=>{}}}
            for invocation in &report.invocations {let sp=invocation.entry.registers.sp.0;let physical=u64::from(sp&!0x40000000);let unknown=physical-20;
                if matches!(row.source,Source::CpuRead|Source::CpuWrite) && row.pc.is_some_and(|pc|(LOW..HIGH).contains(&pc.0)){require(!(u64::from(row.physical.0)<unknown+4 && unknown<end),"dispatcher accessed D+24")?;}
                let overlap=scratch_overlap(invocation,role,&row,end)?;
                if !overlap{continue;}let finish=invocation.returned.as_ref().map(|boundary|boundary.instructions.0).unwrap_or(report.instructions.0);
                if row.source==Source::CpuWrite && row.instructions.is_some_and(|count|invocation.entry.instructions.0<=count.0 && count.0<=finish){require(row.core==Some(invocation.entry.core)&&row.pc.is_some_and(|pc|(LOW..HIGH).contains(&pc.0)),"foreign writer reached live dispatcher scratch")?;}
                if fetched && matches!(row.source,Source::HostWrite|Source::ModelBusWrite){return Err("unattributed writer reached live dispatcher scratch".into());}
            }
            rows.push(row);
        }
    }
    require(fetches==report.code_fetches,"fetch accounting differs")?;
    let mut outcomes=Vec::new();
    for invocation in &report.invocations{
        correlate(&invocation.entry,&trace)?;
        if let Some(returned)=&invocation.returned{correlate(returned,&trace)?;}
        for call in &invocation.calls{correlate(&call.entry,&trace)?;if let Some(returned)=&call.returned{correlate(returned,&trace)?;}if let Some(value)=&call.attributes{correlate(&value.boundary,&trace)?;}}
    }
    for publication in &report.publications{correlate(&publication.boundary,&trace)?;}
    if report.case==Case::Natural{
        require(report.menu==Ok(Some([1,5,2])) && report.invocations.len()==6 && report.perturbations.is_empty(),"natural endpoint or labelling differs")?;
        for (core,phases) in [(Core::Primary,&[Phase::Install,Phase::Root,Phase::Idle,Phase::Diagnostic][..]),(Core::Secondary,&[Phase::Peer,Phase::Diagnostic][..])]{let selected:Vec<_>=report.invocations.iter().filter(|invocation|invocation.entry.core==core).collect();require(selected.iter().map(|invocation|invocation.phase).eq(phases.iter().copied()),"phase order differs")?;for invocation in selected{outcomes.push(project(invocation,&rows)?);}}
        require(report.publications.len()==2,"both publications absent")?;for core in [Core::Primary,Core::Secondary]{let publication=report.publications.iter().find(|value|value.boundary.core==core).ok_or("publication core absent")?;let address=if core==Core::Primary{0x4004f114}else{0x400df0d4};require(word(&publication.cell,address)?==0x69cc6c,"published callback differs")?;require(rows.iter().any(|row|row.source==Source::CpuWrite && row.physical.0==address&!0x40000000 && row.value==Some(0x69cc6c)),"publication lacks CPU store")?;}
    }else{
        control_fields(&report)?;let peer=report.invocations.iter().find(|invocation|invocation.entry.core==Core::Secondary&&invocation.phase==Phase::Peer).ok_or("control peer absent")?;outcomes.push(project(peer,&rows)?);
        for address in [0xa1010c,0xa10110,0xa10114,0xf33fc]{require(rows.iter().any(|row|row.source==Source::HostWrite && row.physical.0==address && row.size==4),"declared control write absent")?;}
        for mutation in &report.perturbations{word(&mutation.before,mutation.before.address.0)?;word(&mutation.after,mutation.after.address.0)?;}
    }
    Ok((report.case,outcomes,digest))
}
/// project_at requires the dispatcher to preserve the caller's r4-r11; their values are private to the loader provider and are not compared.
#[derive(Serialize,PartialEq)]
struct SharedOutcome {core:usize,phase:Phase,sp:u32,result:u32,cpsr:u32,tasks:Vec<Task>}
impl From<Outcome> for SharedOutcome {
    fn from(outcome:Outcome)->Self {Self {core:outcome.core,phase:outcome.phase,sp:outcome.sp,result:outcome.result,cpsr:outcome.cpsr,tasks:outcome.tasks}}
}
#[derive(Serialize,PartialEq)]
pub(crate) struct SharedProjection {outcomes:Vec<SharedOutcome>}
pub(super) fn shared(invocations:&[Invocation],publications:&[super::Publication],rows:&[Row],trace_bytes:&[u8],callers:&[(usize,u32,u32)])->Result<SharedProjection,String>{
    let trace:Vec<Hit>=serde_json::from_slice(trace_bytes).map_err(|error|error.to_string())?;
    require(invocations.len()==6 && publications.len()==2 && callers.len()==6,"shared dispatcher census differs")?;
    let mut outcomes=Vec::new();
    for (core,phases) in [(Core::Primary,&[Phase::Install,Phase::Root,Phase::Idle,Phase::Diagnostic][..]),(Core::Secondary,&[Phase::Peer,Phase::Diagnostic][..])] {
        let selected:Vec<_>=invocations.iter().filter(|invocation|invocation.entry.core==core).collect();
        require(selected.iter().map(|invocation|invocation.phase).eq(phases.iter().copied()),"shared dispatcher phase order differs")?;
        for invocation in selected {
            correlate(&invocation.entry,&trace)?;
            correlate(invocation.returned.as_ref().ok_or("shared dispatcher return absent")?,&trace)?;
            for call in &invocation.calls {
                correlate(&call.entry,&trace)?;correlate(call.returned.as_ref().ok_or("shared service return absent")?,&trace)?;
                if let Some(attributes)=&call.attributes {correlate(&attributes.boundary,&trace)?;}
            }
            let phase=invocation.entry.registers.r[0];
            let matching:Vec<_>=callers.iter().filter(|(owner,argument,_)|*owner==usize::from(core) && *argument==phase).collect();
            require(matching.len()==1,"source-bound shared dispatcher caller is absent or repeated")?;
            outcomes.push(project_at(invocation,rows,matching[0].2)?.into());
        }
    }
    for core in [Core::Primary,Core::Secondary] {
        let publication=publications.iter().find(|publication|publication.boundary.core==core).ok_or("shared callback publication absent")?;
        correlate(&publication.boundary,&trace)?;
        let address=if core==Core::Primary {0x4004f114}else{0x400df0d4};
        require(word(&publication.cell,address)?==0x69cc6c && rows.iter().any(|row|row.source==Source::CpuWrite
            && row.core==Some(core) && row.physical.0==address&!0x40000000 && row.size==4 && row.value==Some(0x69cc6c)),
            "shared callback publication lacks committed value or source store")?;
    }
    Ok(SharedProjection {outcomes})
}
pub(super) type SharedHit=Hit;
pub(super) fn shared_hits(bytes:&[u8],code:&[u8])->Result<Vec<SharedHit>,String>{
    require(code.len()==(HIGH-LOW) as usize,"shared dispatcher code extent differs")?;
    let hits:Vec<Hit>=serde_json::from_slice(bytes).map_err(|error|error.to_string())?;
    let mut sampled=[false;2];
    for hit in &hits {
        let pc=number(&hit.address)?;
        if !(LOW..HIGH).contains(&pc){continue;}
        require(hit.core<2 && pc%4==0 && hit.cpsr&0x20==0,"shared dispatcher execution state differs")?;
        let sample=hit.code_word.as_ref().ok_or("shared dispatcher source-word sample absent")?;
        let offset=(pc-LOW) as usize;let expected=u32::from_le_bytes(code[offset..offset+4].try_into().map_err(|_|"dispatcher source word extent")?);
        require([sample.guest,sample.physical,sample.expected]==[expected;3],"shared dispatcher source word differs")?;
        for bank in sample.tcm.into_iter().flatten(){if bank&1!=0 && (bank>>2)&31!=0 {
            let size=(bank>>2)&31;require((3..=14).contains(&size),"shared dispatcher TCM size reserved")?;
            let low=u64::from(bank&0xfffff000);require(low>=u64::from(HIGH)||low+(512u64<<size)<=u64::from(LOW),"shared dispatcher TCM overlap")?;
        }}
        sampled[hit.core]=true;
    }
    require(sampled==[true;2],"shared dispatcher source sampling lacks a core")?;
    Ok(hits)
}
pub(super) fn shared_boundary(boundary:&Boundary,hits:&[SharedHit])->Result<(),String>{correlate(boundary,hits)}
pub(super) fn shared_fetches(records:&[super::shared::FetchRecord],hits:&[SharedHit],endpoint:u64)->Result<(),String>{
    let samples=hits.iter().map(|hit|Ok((hit.core,number(&hit.address)?,hit.instructions))).collect::<Result<std::collections::BTreeSet<_>,String>>()?;
    let mut executed=std::collections::BTreeSet::new();let mut attempts=std::collections::BTreeSet::new();
    let mut retries:BTreeMap<(usize,u32,u64,bool),&super::shared::FetchRecord>=BTreeMap::new();
    let mut cores=[false;2];
    for record in records {
        let row=&record.row;
        let core=usize::from(row.core.ok_or("shared dispatcher fetch core absent")?);
        let at=row.instructions.as_ref().ok_or("shared dispatcher fetch clock absent")?.0;
        require(row.source==Source::CpuFetch&&row.size==4&&row.address==row.physical&&row.pc==Some(row.address)
            &&record.order==Some(super::shared::ByteOrder::Little)&&row.value.is_none()&&at<=endpoint,
            "shared dispatcher fetch identity or instruction state differs")?;
        let key=(core,row.address.0,at,record.attempt);
        if let Some(previous)=retries.insert(key,record) {
            require(previous.own==record.own&&previous.order==record.order
                &&previous.row.tick.as_ref().map(|tick|tick.0)==row.tick.as_ref().map(|tick|tick.0)
                &&previous.row.value==row.value,"shared dispatcher fetch retry contradicts prior evidence")?;
        }
        if record.attempt {
            require(samples.contains(&(core,row.address.0,at)),"dispatcher attempted fetch lacks its stopped sample")?;
            attempts.insert((core,row.address.0,at));
        }else{
            let before=at.checked_sub(1).ok_or("dispatcher executed fetch clock underflows")?;
            require(samples.contains(&(core,row.address.0,before)),"dispatcher executed fetch lacks its N+1 source sample")?;
            executed.insert((core,row.address.0,before));
        }
        cores[core]=true;
    }
    for sample in samples.iter().filter(|(_,pc,_)|(LOW..HIGH).contains(pc)) {
        require(sample.2<=endpoint,"dispatcher sample exceeds actual endpoint")?;
        if sample.2==endpoint {require(attempts.contains(sample),"terminal dispatcher sample lacks preexecution attempt")?;}
        else {require(executed.contains(sample),"completed dispatcher sample lacks executed N+1 fetch")?;}
    }
    require(cores==[true;2],"shared dispatcher fetch coverage lacks a core")
}
pub(super) fn compare(retained:&Path,owned:&Path,source_path:&Path,linked_path:&Path,output:&Path)->Result<(),String>{
    std::fs::create_dir(output).map_err(|error|error.to_string())?;
    let source=read(source_path)?;let linked=read(linked_path)?;
    require(sha256(&source)=="f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16" && sha256(&linked)=="8a592c4eeeedb3dc3977055639cf12742cd19f5526bb4f53d5c02d5589e2d1c0","comparison sources differ")?;
    let (retained_digest,left)=match check(retained,Role::Retained,&source,&linked){Ok((case,outcomes,digest))=>(digest,Ok((case,outcomes))),Err(error)=>(super::file_digest(&retained.join("dispatch.json"))?,Err(error))};
    let (owned_digest,right)=match check(owned,Role::Owned,&source,&linked){Ok((case,outcomes,digest))=>(digest,Ok((case,outcomes))),Err(error)=>(super::file_digest(&owned.join("dispatch.json"))?,Err(error))};
    let known=left.as_ref().map(|(case,_)|*case).or_else(|_|right.as_ref().map(|(case,_)|*case)).ok();
    let result=left.and_then(|(case,outcomes)|right.and_then(|(other,owned)|{require(case==other && outcomes==owned,"retained/owned consumer outcomes differ")?;Ok((case,outcomes))}));
    let (case,outcomes,error)=match result{Ok((case,outcomes))=>(Some(case),outcomes,None),Err(error)=>(known,Vec::new(),Some(error))};
    let comparison=Comparison {schema:1,case,accepted:error.is_none(),retained:retained_digest,owned:owned_digest,outcomes,error};
    super::persist(&output.join("comparison.json"),&comparison)?;
    match comparison.error {Some(error)=>Err(error),None=>Ok(())}
}

#[cfg(test)]
mod tests{
    use super::*;
    use super::super::{Count,Registers};
    use fujimod_xm1::provenance::Guest;
    fn classes()->Vec<Class>{
        [[u64::from(LOW),u64::from(HIGH)],[0x4f114,0x4f118],[0xdf0d4,0xdf0d8],[0x63350,0x63460],[0xf3310,0xf3420],[0xa1010c,0xa10118]].into_iter().map(|range|Class {range,kinds:vec![Source::CpuRead,Source::CpuWrite,Source::HostWrite,Source::ModelBusWrite,Source::CpuFetch]}).collect()
    }
    #[test]
    fn refuses_missing_callback_frame_and_control_watch_kinds(){
        let complete=classes();assert!(watch_contract(&complete,0,Case::ReservedAlias).is_ok());
        for index in 1..6 {for kind in [Source::CpuRead,Source::CpuWrite,Source::HostWrite,Source::ModelBusWrite]{
            let mut changed=classes();changed[index].kinds.retain(|value|*value!=kind);
            assert!(watch_contract(&changed,0,Case::ReservedAlias).is_err());
        }}
        let mut missing=classes();missing.remove(1);assert!(watch_contract(&missing,1,Case::Natural).is_err());
    }
    fn row(index:usize,value:u32,count:u64)->Row{Row {source:Source::CpuWrite,address:Guest(0x400f33e4+4*index as u32),physical:Guest(0xf33e4+4*index as u32),size:4,core:Some(Core::Secondary),pc:Some(Guest(LOW+4)),instructions:Some(Count(count)),tick:None,value:Some(u64::from(value)),before:None}}
    #[test]
    fn refuses_borrowed_first_submission_stores_and_reordering(){
        let first=[103,0x781970,15,0x400,0x69cbf0,0];let second=[93,0x781978,13,0x400,0x69cd58,0];
        let mut rows:Vec<_>=first.into_iter().enumerate().map(|(index,value)|row(index,value,index as u64+1)).collect();
        for index in [0,1,2,4]{rows.push(row(index,second[index],11+index as u64));}
        assert!(ordered_stores(&rows,Core::Secondary,0x400f33e4,second,Interval {after:Count(10),through:Count(20)}).is_err());
        let mut complete:Vec<_>=second.into_iter().enumerate().map(|(index,value)|row(index,value,11+index as u64)).collect();
        assert!(ordered_stores(&complete,Core::Secondary,0x400f33e4,second,Interval {after:Count(10),through:Count(20)}).is_ok());
        complete.swap(2,3);assert!(ordered_stores(&complete,Core::Secondary,0x400f33e4,second,Interval {after:Count(10),through:Count(20)}).is_err());
    }
    fn boundary(pc:u32,count:u64)->Boundary{Boundary {instructions:Count(count),core:Core::Primary,pc:Guest(pc),registers:Registers {r:[0;13],sp:Guest(0),lr:Guest(LOW+8),cpsr:0},frame:RamView {address:Guest(0),physical:Guest(0),words:Vec::new(),other_words:Vec::new(),physical_words:Vec::new(),tcm:[[0;2];2],decoder:1}}}
    #[test]
    fn retained_stack_below_its_entry_does_not_lease_live_caller_scratch(){
        let mut entry=boundary(LOW,10);entry.registers.sp=Guest(0x400f3410);
        let mut service=boundary(0x725dd8,20);service.registers.sp=Guest(0x400f33d0);
        let call=super::super::Call {service:Service::Task,entry:service,returned:Some(boundary(LOW+8,30)),descriptor_before:None,descriptor_after:None,domain:None,roots:None,attributes:None};
        let invocation=Invocation {phase:Phase::Peer,entry,calls:vec![call],returned:Some(boundary(0xd0af0,40))};
        let mut access=row(0,0,25);access.physical=Guest(0xf33c8);
        assert!(!scratch_overlap(&invocation,Role::Owned,&access,0xf33cc).unwrap());
        access.physical=Guest(0xf33d0);
        assert!(scratch_overlap(&invocation,Role::Owned,&access,0xf33d4).unwrap());
        access.physical=Guest(0xf33c8);access.instructions=Some(Count(15));
        assert!(scratch_overlap(&invocation,Role::Owned,&access,0xf33cc).unwrap());
    }
    #[test]
    fn refuses_mislabeled_and_out_of_order_setup_calls(){
        let mode=boundary(0x657184,3);let returned=boundary(LOW+8,5);
        assert!(service_interval(Service::Mode,&mode,&returned,Interval {after:Count(1),through:Count(10)}).is_ok());
        assert!(service_interval(Service::Install,&mode,&returned,Interval {after:Count(1),through:Count(10)}).is_err());
        assert!(service_interval(Service::Mode,&mode,&returned,Interval {after:Count(4),through:Count(10)}).is_err());
        assert!(service_interval(Service::Mode,&mode,&boundary(LOW+8,2),Interval {after:Count(1),through:Count(10)}).is_err());
        assert!(service_interval(Service::Mode,&mode,&returned,Interval {after:Count(1),through:Count(4)}).is_err());
    }
    fn hit(core:usize,pc:u32,count:u64)->SharedHit{
        serde_json::from_str(&format!(r#"{{"address":"{pc:#010x}","core":{core},"instructions":{count},"cpsr":211,"registers":{{}},"sp":"0x0","lr":"0x0","code_word":null}}"#)).expect("synthetic dispatcher sample")
    }
    fn fetch(core:Core,pc:u32,count:u64,attempt:bool)->super::super::shared::FetchRecord{
        super::super::shared::FetchRecord {row:Row {source:Source::CpuFetch,address:Guest(pc),physical:Guest(pc),size:4,core:Some(core),pc:Some(Guest(pc)),
            instructions:Some(Count(count)),tick:Some(Count(1)),value:None,before:None},own:Some(count),order:Some(super::super::shared::ByteOrder::Little),attempt}
    }
    #[test]
    fn shared_dispatcher_samples_need_their_executed_and_terminal_fetches(){
        let samples=[hit(0,LOW,10),hit(0,LOW+4,11),hit(1,LOW,10)];
        let complete=[fetch(Core::Primary,LOW,11,false),fetch(Core::Primary,LOW+4,11,true),fetch(Core::Secondary,LOW,11,false)];
        assert!(shared_fetches(&complete,&samples,11).is_ok());
        let attempt_only=[fetch(Core::Primary,LOW+4,11,true),fetch(Core::Secondary,LOW,11,false)];
        assert_eq!(shared_fetches(&attempt_only,&samples,11).err().as_deref(),Some("completed dispatcher sample lacks executed N+1 fetch"));
        let executed_only=[fetch(Core::Primary,LOW,11,false),fetch(Core::Secondary,LOW,11,false)];
        assert_eq!(shared_fetches(&executed_only,&samples,11).err().as_deref(),Some("terminal dispatcher sample lacks preexecution attempt"));
    }
    fn framed(pc:u32,count:u64,r4:u32,r0:u32)->Boundary{
        let sp=0x40063450;let mut value=boundary(pc,count);value.registers.sp=Guest(sp);value.registers.lr=Guest(0x40a6c);value.registers.r[4]=r4;value.registers.r[0]=r0;
        value.frame=RamView {address:Guest(sp-256),physical:Guest((sp-256)&!0x40000000),words:vec![0;68],other_words:vec![0;68],physical_words:vec![0;68],tcm:[[0;2];2],decoder:1};value
    }
    #[test]
    fn dispatcher_return_must_preserve_caller_live_registers(){
        let invocation=|returned_r4:u32|Invocation {phase:Phase::Diagnostic,entry:framed(LOW,10,7,2),calls:Vec::new(),returned:Some(framed(0x40a6c,20,returned_r4,0x69cc6c))};
        assert!(project_at(&invocation(7),&[],0x40a6c).is_ok());
        assert_eq!(project_at(&invocation(6),&[],0x40a6c).err().as_deref(),Some("caller-live registers changed"));
    }
}
