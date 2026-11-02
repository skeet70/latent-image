use super::*;
use serde_json::value::RawValue;
use std::path::PathBuf;

#[derive(Deserialize,Serialize)]
#[serde(transparent)]
struct Frozen(Box<RawValue>);
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema:Frozen,role:Frozen,program:Frozen,artifacts:Frozen,identity:owner::InputIdentity,checkpoint:Digest,
    instructions:Frozen,counts:Frozen,menu:Frozen,error:Frozen,machine_failure:Frozen,route:RouteCapture,
    epochs:Vec<Epoch>,dispatcher:Frozen,consumer:ConsumerEnvelope,trace:FileBinding,timer_transactions:FileBinding,
}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct ConsumerEnvelope {begin:Frozen,end:Frozen,termination:Frozen,window:WindowEnvelope}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct WindowEnvelope {
    schema:Frozen,role:Frozen,outcome:Frozen,error:Frozen,machine_failure:Frozen,identity:Frozen,
    checkpoint_sha256:Frozen,checkpoint_reused:Frozen,instructions:Frozen,limits:Frozen,
    probes:Frozen,invocations:Frozen,resources:Frozen,segments:Vec<SegmentEnvelope>,census:Frozen,fetch_attempts:Frozen,payloads:Frozen,
}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct SegmentEnvelope {
    id:Frozen,start_counts:Frozen,end_counts:Frozen,start_contexts:Frozen,end_contexts:Frozen,
    fetch_scope:Frozen,fetch_ranges:Frozen,armed_sites:Frozen,probe_range:Frozen,watch:WatchEnvelope,end_error:Frozen,stops:Frozen,
}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct WatchEnvelope {
    enabled:bool,ranges:Frozen,classes:Vec<owner::Class>,views:owner::WatchViews,
    #[serde(default,skip_serializing_if="Option::is_none")]
    fetch_attribution:Option<Frozen>,
    records:Vec<AccessEnvelope>,violation:Frozen,
}
#[derive(Deserialize,Serialize)]
#[serde(rename_all="lowercase")]
enum ByteOrder {Little,Big}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct AccessEnvelope {
    source:Access,address:Guest,physical:Guest,size:u64,value:Option<u64>,before:Option<Vec<u8>>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    byte_order:Option<ByteOrder>,
    core:Option<Core>,pc:Option<Guest>,instructions:Option<owner::Count>,tick:Option<owner::Tick>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    core_instructions:Option<u64>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    latest_fetch:Option<Frozen>,
}
#[derive(Deserialize,Serialize,PartialEq,Eq,PartialOrd,Ord)]
#[serde(rename_all="lowercase")]
enum Gpr {R0,R1,R2,R3,R4,R5,R6,R7,R8,R9,R10,R11,R12}
#[derive(Clone,Copy,Deserialize)]
#[serde(try_from="String")]
struct Hex(u32);
impl TryFrom<String> for Hex {
    type Error=String;
    fn try_from(value:String)->Result<Self,String>{Ok(Self(u32::from_str_radix(value.strip_prefix("0x").ok_or("trace register prefix absent")?,16).map_err(|error|error.to_string())?))}
}
impl Serialize for Hex {fn serialize<S:serde::Serializer>(&self,serializer:S)->Result<S::Ok,S::Error>{serializer.serialize_str(&format!("{:#x}",self.0))}}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct TraceHit {site:String,address:Hex,core:Core,instructions:owner::Count,registers:BTreeMap<Gpr,Hex>,sp:Hex,lr:Hex,cpsr:u32,
    #[serde(default,skip_serializing_if="Option::is_none")]
    code_word:Option<Frozen>}
#[derive(Clone,Copy,Deserialize,Serialize)]
#[serde(rename_all="kebab-case")]
enum Control {
    PhaseArgument,LoaderLink,CallbackResult,DescriptorStore,ForeignLiveWord,SourceAlias,SourceTcm,
    MissingCarrierFetch,MenuPulse,ResetBytes,TimerRead,MissingGuard,
}
impl Control {
    const ALL:[Self;12]=[Self::PhaseArgument,Self::LoaderLink,Self::CallbackResult,Self::DescriptorStore,Self::ForeignLiveWord,
        Self::SourceAlias,Self::SourceTcm,Self::MissingCarrierFetch,Self::MenuPulse,Self::ResetBytes,Self::TimerRead,Self::MissingGuard];
    fn name(self)->&'static str {match self {Self::PhaseArgument=>"phase-argument",Self::LoaderLink=>"loader-link",Self::CallbackResult=>"callback-result",
        Self::DescriptorStore=>"descriptor-store",Self::ForeignLiveWord=>"foreign-live-word",Self::SourceAlias=>"source-alias",Self::SourceTcm=>"source-tcm",
        Self::MissingCarrierFetch=>"missing-carrier-fetch",Self::MenuPulse=>"menu-pulse",Self::ResetBytes=>"reset-bytes",Self::TimerRead=>"timer-read",Self::MissingGuard=>"missing-guard"}}
}
#[derive(Serialize)]
#[serde(tag="outcome",content="error",rename_all="kebab-case")]
enum Observed {Refused(String),Admitted,SetupFailure(String)}
#[derive(Serialize)]
struct Case {name:Control,passed:bool,directory:PathBuf,receipt:Option<Digest>,observed:Observed}
#[derive(Serialize)]
struct NestedPush {core:Core,call_sp:Guest,call_start:u64,call_end:u64,address:Guest,physical:Guest,pc:Guest,instructions:owner::Count}
#[derive(Serialize)]
struct Report {schema:u32,scope:HostScope,accepted:bool,error:Option<String>,baseline:Option<FileBinding>,nested_push:Option<NestedPush>,cases:Vec<Case>}
fn write_capture(root:&Path,capture:&Envelope)->Result<Digest,String>{
    let bytes=serde_json::to_vec(capture).map_err(|error|error.to_string())?;
    fujimod_xm1::bridge::write(&root.join("startup-joined.json"),&bytes)?;Ok(Digest::of(&bytes))
}
fn copy_files(source:&Path,destination:&Path,capture:&Capture,skip_reset:bool)->Result<(),String>{
    let mut files=BTreeSet::new();
    files.insert(capture.identity.update_file.path.clone());files.insert(capture.trace.path.clone());files.insert(capture.timer_transactions.path.clone());
    for file in &capture.artifacts.files {files.insert(file.path.clone());}
    for snapshot in &capture.route.snapshots {for file in &snapshot.files {files.insert(file.path.clone());}}
    for epoch in &capture.epochs {if let Evidence::Watch {file,..}=&epoch.evidence {files.insert(file.path.clone());}}
    if !skip_reset {files.insert("reset.fujimod".into());}
    for path in files {
        let mut components=path.components();require(matches!(components.next(),Some(Component::Normal(_)))&&components.next().is_none(),"control input filename is not a single relative component")?;
        std::fs::hard_link(source.join(&path),destination.join(&path)).map_err(|error|error.to_string())?;
    }
    Ok(())
}
fn watches<F>(capture:&mut Envelope,root:&Path,persist:bool,mut mutate:F)->Result<bool,String> where F:FnMut(&mut WatchEnvelope)->Result<bool,String>{
    for (index,epoch) in capture.epochs.iter_mut().enumerate(){match &mut epoch.evidence {
        Evidence::Watch {file,..}=>{
            let mut watch:WatchEnvelope=serde_json::from_slice(&read(&root.join(&file.path))?).map_err(|error|error.to_string())?;
            if mutate(&mut watch)? {
                if persist {
                    let bytes=serde_json::to_vec(&watch).map_err(|error|error.to_string())?;
                    *file=persist_bound(root,&format!("control-watch-{index}.json"),&bytes)?;
                }
                return Ok(true);
            }
        },Evidence::ConsumerSegment {segment}=>{
            let watch=&mut capture.consumer.window.segments.get_mut(*segment).ok_or("control consumer segment absent")?.watch;
            if mutate(watch)? {return Ok(true);}
        },
    }}
    Ok(false)
}
fn nested_push(capture:&mut Envelope,root:&Path,original:&Capture,replace:bool)->Result<Option<NestedPush>,String>{
    let calls=original.dispatcher.retained_calls();let mut found=None;
    watches(capture,root,replace,|watch|{
        for row in &mut watch.records {
            if row.source!=Access::CpuWrite || row.size!=4 {continue;}
            let (Some(core),Some(pc),Some(at))=(row.core,row.pc,row.instructions) else{continue};
            if (0x69cdf8..0x69cf84).contains(&pc.0)||(0x725a04..0x725b54).contains(&pc.0){continue;}
            if let Some(call)=calls.iter().find(|call|call.core==usize::from(core)&&call.start<at.0&&at.0<=call.end
                &&u64::from(row.physical.0)+row.size<=u64::from(call.sp.0&!0x40000000)&&!call.live.is_empty()) {
                found=Some(NestedPush {core,call_sp:call.sp,call_start:call.start,call_end:call.end,address:row.address,physical:row.physical,pc,instructions:at});
                if replace {let live=call.live[0];row.address=live;row.physical=Guest(live.0&!0x40000000);}
                return Ok(true);
            }
        }
        Ok(false)
    })?;
    Ok(found)
}
fn rebind_trace(root:&Path,capture:&mut Envelope,event:usize)->Result<(),String>{
    let mut trace:Vec<TraceHit>=serde_json::from_slice(&read(&root.join(&capture.trace.path))?).map_err(|error|error.to_string())?;
    let observed=capture.route.events.get(event).ok_or("mutated event absent")?;
    let raw=trace.get_mut(event).ok_or("mutated trace event absent")?;
    raw.address=Hex(observed.pc.0);raw.core=observed.core;raw.instructions=observed.instructions;
    raw.sp=Hex(observed.registers.sp.0);raw.lr=Hex(observed.registers.lr.0);raw.cpsr=observed.registers.cpsr;
    for (register,value) in [Gpr::R0,Gpr::R1,Gpr::R2,Gpr::R3,Gpr::R4,Gpr::R5,Gpr::R6,Gpr::R7,Gpr::R8,Gpr::R9,Gpr::R10,Gpr::R11,Gpr::R12].into_iter().zip(observed.registers.r){raw.registers.insert(register,Hex(value));}
    let bytes=serde_json::to_vec(&trace).map_err(|error|error.to_string())?;capture.trace=persist_bound(root,"control-trace.json",&bytes)?;Ok(())
}
#[derive(Deserialize,Serialize)]
#[serde(rename_all="lowercase")]
enum Operation {Read,Write}
#[derive(Deserialize,Serialize)]
struct Transaction {cpu:Core,pc:Guest,address:Guest,size:u8,operation:Operation,value:u64,accepted:bool,tick:owner::Tick}
#[derive(Deserialize,Serialize)]
struct Batch {transactions:Vec<Transaction>,omitted:u64}
#[derive(Deserialize,Serialize)]
struct Timer {start:owner::Count,end:owner::Count,range:[Guest;2],limit:usize,batch:Option<Batch>,error:Option<String>}
fn mutate(control:Control,source:&Path,root:&Path,capture:&mut Envelope,original:&Capture)->Result<(),String>{
    match control {
        Control::PhaseArgument=>{
            let event=capture.route.events.iter_mut().find(|event|event.core==Core::Primary&&event.pc==Guest(0x69cdf8)&&event.registers.r[0]==0).ok_or("outer0 event absent")?;
            event.registers.r[0]=4;let id=event.id.0;rebind_trace(root,capture,id)?;
        },Control::LoaderLink=>{
            let call=capture.route.calls.iter_mut().find(|call|call.kind==CallKind::Loader&&call.target==Guest(0x69cdf8)).ok_or("loader dispatcher call absent")?;
            call.continuation.0=call.continuation.0.checked_add(4).ok_or("mutated continuation overflows")?;
        },Control::CallbackResult=>{
            let call=capture.route.calls.iter().find(|call|call.kind==CallKind::Loader&&call.target==Guest(0x69cdf8)
                &&call.entry.is_some_and(|id|capture.route.events[id.0].registers.r[0]==2)).ok_or("diagnostic-result call absent")?;
            let id=call.returned.ok_or("diagnostic-result return absent")?.0;
            capture.route.events[id].registers.r[0]^=4;rebind_trace(root,capture,id)?;
        },Control::DescriptorStore=>{
            let changed=watches(capture,root,true,|watch|{
                let row=watch.records.iter().position(|row|row.source==Access::CpuWrite&&row.size==4&&row.pc.is_some_and(|pc|(0x69cdf8..0x69cf84).contains(&pc.0))
                    &&[(0x63424,0x6343c),(0xf33e4,0xf33fc)].iter().any(|(low,high)|*low<=row.physical.0&&row.physical.0<*high));
                if let Some(index)=row {watch.records.remove(index);Ok(true)}else{Ok(false)}
            })?;require(changed,"descriptor source store absent")?;
        },Control::ForeignLiveWord=>{require(nested_push(capture,root,original,true)?.is_some(),"retained callee push witness absent")?;},
        Control::SourceAlias=>{
            let origin=capture.route.origin.ok_or("loader origin absent")?.0;
            capture.route.events[origin].code.mapping.physical.0^=4;
        },Control::SourceTcm=>{
            let origin=capture.route.origin.ok_or("loader origin absent")?.0;
            let code=&mut capture.route.events[origin].code;code.mapping.tcm[0][0]=(code.mapping.guest.0&!0xfff)|13;
        },Control::MissingCarrierFetch=>{
            let origin=capture.route.events[capture.route.origin.ok_or("loader origin absent")?.0].clone();
            let changed=watches(capture,root,true,|watch|{
                let before=watch.records.len();watch.records.retain(|row|!(row.source==Access::CpuFetch&&row.core==Some(origin.core)&&row.pc==Some(origin.pc)
                    &&row.instructions.is_some_and(|at|origin.instructions.0.checked_add(1)==Some(at.0))));Ok(before!=watch.records.len())
            })?;require(changed,"executed first-carrier fetch absent")?;
        },Control::MenuPulse=>{
            let pulse=capture.route.pulses.first_mut().ok_or("route pulse absent")?;
            let released=pulse.released.as_mut().ok_or("route release absent")?;released.0=released.0.checked_add(1).ok_or("release mutation overflows")?;
        },Control::ResetBytes=>{
            let mut reset=read(&source.join("reset.fujimod"))?;let first=reset.first_mut().ok_or("reset checkpoint empty")?;*first^=0xff;
            fujimod_xm1::bridge::write(&root.join("reset.fujimod"),&reset)?;capture.checkpoint=Digest::of(&reset);
        },Control::TimerRead=>{
            let mut timer:Timer=serde_json::from_slice(&read(&root.join(&capture.timer_transactions.path))?).map_err(|error|error.to_string())?;
            let read=timer.batch.as_mut().ok_or("timer transaction batch absent")?.transactions.iter_mut().find(|row|matches!(row.operation,Operation::Read)&&matches!(row.pc.0,0x41ad8|0x41adc)).ok_or("timer source read absent")?;
            read.value^=1;capture.timer_transactions=persist_bound(root,"control-timer.json",&serde_json::to_vec(&timer).map_err(|error|error.to_string())?)?;
        },Control::MissingGuard=>{
            let changed=watches(capture,root,true,|watch|{
                let present=watch.classes.iter().any(|class|class.kinds.contains(&Access::CpuFetch));
                for class in &mut watch.classes {class.kinds.retain(|source|*source!=Access::CpuFetch);}
                Ok(present)
            })?;require(changed,"classified fetch guard absent")?;
        },
    }
    Ok(())
}
pub(super) fn run(retained:&Path,joined:&Path,analysis:&Path,output:&Path)->Result<(),String>{
    std::fs::create_dir(output).map_err(|error|error.to_string())?;
    let mut report=Report {schema:1,scope:HostScope::SharedRouteAndDispatcher,accepted:false,error:None,baseline:None,nested_push:None,cases:Vec::new()};
    let result=(||->Result<(),String>{
        let baseline=output.join("baseline-pair");super::compare(retained,joined,analysis,&baseline)?;
        let baseline_bytes=read(&baseline.join("shared-comparison.json"))?;
        report.baseline=Some(FileBinding {path:"baseline-pair/shared-comparison.json".into(),sha256:Digest::of(&baseline_bytes)});
        let source_bytes=read(&joined.join("startup-joined.json"))?;
        let original:Capture=serde_json::from_slice(&source_bytes).map_err(|error|error.to_string())?;
        let mut positive:Envelope=serde_json::from_slice(&source_bytes).map_err(|error|error.to_string())?;
        report.nested_push=nested_push(&mut positive,joined,&original,false)?;
        require(report.nested_push.is_some(),"strict baseline lacks an observed retained callee push below its actual incoming SP")?;
        for control in Control::ALL {
            let directory=PathBuf::from(control.name());let case_root=output.join(&directory);
            std::fs::create_dir(&case_root).map_err(|error|error.to_string())?;
            let mut receipt=None;
            let observed=(||->Result<Observed,String>{
                copy_files(joined,&case_root,&original,matches!(control,Control::ResetBytes))?;
                let mut capture:Envelope=serde_json::from_slice(&source_bytes).map_err(|error|error.to_string())?;
                mutate(control,joined,&case_root,&mut capture,&original)?;
                receipt=Some(write_capture(&case_root,&capture)?);
                Ok(match super::verified(&case_root,analysis){Ok(_)=>Observed::Admitted,Err(error)=>Observed::Refused(error)})
            })().unwrap_or_else(Observed::SetupFailure);
            let passed=matches!(observed,Observed::Refused(_));
            report.cases.push(Case {name:control,passed,directory,receipt,observed});
        }
        require(report.cases.iter().all(|case|case.passed),"serialized shared refusal controls did not all refuse")
    })();
    report.accepted=result.is_ok();report.error=result.as_ref().err().cloned();
    owner::persist(&output.join("shared-refusal-controls.json"),&report)?;result
}
