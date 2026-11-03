use super::*;
use serde_json::value::RawValue;
use std::path::PathBuf;

#[derive(Clone,Deserialize,Serialize)]
#[serde(transparent)]
struct Frozen(Box<RawValue>);
impl Frozen {
    fn parsed<T:serde::de::DeserializeOwned>(&self)->Result<T,String>{serde_json::from_str(self.0.get()).map_err(|error|error.to_string())}
    fn encoded<T:Serialize>(value:&T)->Result<Self,String>{serde_json::value::to_raw_value(value).map(Self).map_err(|error|error.to_string())}
}
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
#[derive(Clone,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct SegmentEnvelope {
    id:Frozen,start_counts:Frozen,end_counts:Frozen,start_contexts:Frozen,end_contexts:Frozen,
    fetch_scope:Frozen,fetch_ranges:Frozen,armed_sites:Frozen,probe_range:Frozen,watch:WatchEnvelope,end_error:Frozen,stops:Frozen,
}
#[derive(Clone,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct WatchEnvelope {
    enabled:bool,ranges:Frozen,classes:Vec<owner::Class>,views:owner::WatchViews,
    #[serde(default,skip_serializing_if="Option::is_none")]
    fetch_attribution:Option<Frozen>,
    records:Vec<AccessEnvelope>,violation:Frozen,
}
#[derive(Clone,Deserialize,Serialize)]
#[serde(rename_all="lowercase")]
enum ByteOrder {Little,Big}
#[derive(Clone,Deserialize,Serialize)]
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
    MissingCarrierFetch,MenuPulse,ResetBytes,TimerRead,MissingGuard,ModeCell,ModeBoundary,ModeRead,ModeAttribution,
    RepeatedZeroSegment,StopOwnCount,MissingAddedGuard,DispatcherOneFetch,DispatcherAttemptOnly,
    EpochEndData,EpochEndSecondFetch,EpochEndUnstopped,DispatcherReturnR4,
    PolicyMissingAcquisition,PolicyWrongAcquisition,PolicyPrematureSharedRelease,PolicyOmittedNoop,PolicyTraceOccurrence,PolicySegmentIndex,
}
impl Control {
    const ALL:[Self;31]=[Self::PhaseArgument,Self::LoaderLink,Self::CallbackResult,Self::DescriptorStore,Self::ForeignLiveWord,
        Self::SourceAlias,Self::SourceTcm,Self::MissingCarrierFetch,Self::MenuPulse,Self::ResetBytes,Self::TimerRead,Self::MissingGuard,
        Self::ModeCell,Self::ModeBoundary,Self::ModeRead,Self::ModeAttribution,Self::RepeatedZeroSegment,Self::StopOwnCount,
        Self::MissingAddedGuard,Self::DispatcherOneFetch,Self::DispatcherAttemptOnly,
        Self::EpochEndData,Self::EpochEndSecondFetch,Self::EpochEndUnstopped,Self::DispatcherReturnR4,
        Self::PolicyMissingAcquisition,Self::PolicyWrongAcquisition,Self::PolicyPrematureSharedRelease,Self::PolicyOmittedNoop,
        Self::PolicyTraceOccurrence,Self::PolicySegmentIndex];
    fn name(self)->&'static str {match self {Self::PhaseArgument=>"phase-argument",Self::LoaderLink=>"loader-link",Self::CallbackResult=>"callback-result",
        Self::DescriptorStore=>"descriptor-store",Self::ForeignLiveWord=>"foreign-live-word",Self::SourceAlias=>"source-alias",Self::SourceTcm=>"source-tcm",
        Self::MissingCarrierFetch=>"missing-carrier-fetch",Self::MenuPulse=>"menu-pulse",Self::ResetBytes=>"reset-bytes",Self::TimerRead=>"timer-read",Self::MissingGuard=>"missing-guard",
        Self::ModeCell=>"mode-cell",Self::ModeBoundary=>"mode-boundary",Self::ModeRead=>"mode-read",Self::ModeAttribution=>"mode-attribution",
        Self::RepeatedZeroSegment=>"repeated-zero-segment",Self::StopOwnCount=>"stop-own-count",Self::MissingAddedGuard=>"missing-added-guard",
        Self::DispatcherOneFetch=>"dispatcher-one-fetch",Self::DispatcherAttemptOnly=>"dispatcher-attempt-only",
        Self::EpochEndData=>"epoch-end-data",Self::EpochEndSecondFetch=>"epoch-end-second-fetch",Self::EpochEndUnstopped=>"epoch-end-unstopped",
        Self::DispatcherReturnR4=>"dispatcher-return-r4",
        Self::PolicyMissingAcquisition=>"policy-missing-acquisition",Self::PolicyWrongAcquisition=>"policy-wrong-acquisition",
        Self::PolicyPrematureSharedRelease=>"policy-premature-shared-release",Self::PolicyOmittedNoop=>"policy-omitted-noop",
        Self::PolicyTraceOccurrence=>"policy-trace-occurrence",Self::PolicySegmentIndex=>"policy-segment-index"}}
    fn policy(self)->bool {matches!(self,Self::PolicyMissingAcquisition|Self::PolicyWrongAcquisition|Self::PolicyPrematureSharedRelease
        |Self::PolicyOmittedNoop|Self::PolicyTraceOccurrence|Self::PolicySegmentIndex)}
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
#[derive(Clone,Copy,Deserialize,Serialize)]
#[serde(rename_all="kebab-case")]
enum AttemptClass {BoundaryAttempt}
#[derive(Clone,Deserialize,Serialize)]
struct AttemptEnvelope {segment:usize,row:usize,classification:AttemptClass}
fn watches<F>(capture:&mut Envelope,root:&Path,persist:bool,mutate:F)->Result<bool,String> where F:FnMut(&mut WatchEnvelope)->Result<bool,String>{
    walk_watches(capture,root,persist,false,mutate)
}
fn walk_watches<F>(capture:&mut Envelope,root:&Path,persist:bool,all:bool,mut mutate:F)->Result<bool,String> where F:FnMut(&mut WatchEnvelope)->Result<bool,String>{
    let mut attempts:Vec<AttemptEnvelope>=capture.consumer.window.fetch_attempts.parsed()?;
    let mut by_segment:BTreeMap<usize,Vec<usize>>=BTreeMap::new();
    for (index,attempt) in attempts.iter().enumerate(){by_segment.entry(attempt.segment).or_default().push(index);}
    let mut changed=false;
    for (index,epoch) in capture.epochs.iter_mut().enumerate(){match &mut epoch.evidence {
        Evidence::Watch {file,..}=>{
            let mut watch:WatchEnvelope=serde_json::from_slice(&read(&root.join(&file.path))?).map_err(|error|error.to_string())?;
            if mutate(&mut watch)? {
                if persist {
                    let bytes=serde_json::to_vec(&watch).map_err(|error|error.to_string())?;
                    *file=persist_bound(root,&format!("control-watch-{index}.json"),&bytes)?;
                }
                changed=true;
            }
        },Evidence::ConsumerSegment {segment}=>{
            let watch=&mut capture.consumer.window.segments.get_mut(*segment).ok_or("control consumer segment absent")?.watch;
            let mut references=Vec::new();
            for reference in by_segment.get(segment).into_iter().flatten() {
                let attempt=&attempts[*reference];let row=watch.records.get(attempt.row).ok_or("control original attempted-fetch row absent")?;
                references.push((*reference,row.core,row.pc,row.instructions,row.core_instructions,row.tick));
            }
            let before=watch.records.len();
            if mutate(watch)? {
                changed=true;
                if before!=watch.records.len() {
                    for (reference,core,pc,clock,own,tick) in references {
                        if let Some(row)=watch.records.iter().rposition(|row|row.source==Access::CpuFetch&&row.core==core&&row.pc==pc
                            &&row.instructions==clock&&row.core_instructions==own&&row.tick==tick) {attempts[reference].row=row;}
                    }
                }
            }
        },
    }if changed&&!all {break;}}
    if changed {capture.consumer.window.fetch_attempts=Frozen::encoded(&attempts)?;}
    Ok(changed)
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
            let document=fujimod_xm1::run::RunDocument::parse(CANONICAL_RUN)?;
            let reset=loader_reset_check::dirty_medium(&read(&source.join("reset.fujimod"))?,&document)?;
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
        Control::ModeCell|Control::ModeBoundary=>mode_sample(control,capture)?,
        Control::ModeRead=>{
            let changed=watches(capture,root,true,|watch|{
                let before=watch.records.len();watch.records.retain(|row|!(row.source==Access::CpuRead&&row.pc==Some(Guest(0x657194))));
                Ok(before!=watch.records.len())
            })?;require(changed,"control actual mode getter read absent")?;
        },
        Control::ModeAttribution=>{
            let changed=watches(capture,root,true,|watch|{
                if let Some(row)=watch.records.iter_mut().find(|row|row.source==Access::CpuRead&&row.pc==Some(Guest(0x657194))) {
                    let latest=row.latest_fetch.as_mut().ok_or("control mode getter latest-fetch evidence absent")?;
                    let mut stamp:FetchStamp=latest.parsed()?;
                    stamp.core_instructions=stamp.core_instructions.checked_add(1).ok_or("control mode fetch own-core mutation overflows")?;
                    *latest=Frozen::encoded(&stamp)?;Ok(true)
                }else{Ok(false)}
            })?;require(changed,"control actual mode getter read absent")?;
        },
        Control::RepeatedZeroSegment=>repeat_zero_segment(capture)?,
        Control::StopOwnCount=>wrong_stop_own_count(capture)?,
        Control::MissingAddedGuard=>missing_added_guard(capture,root)?,
        Control::DispatcherOneFetch|Control::DispatcherAttemptOnly=>dispatcher_fetch_mutation(control,capture,root,original)?,
        Control::EpochEndData|Control::EpochEndSecondFetch|Control::EpochEndUnstopped=>epoch_end_mutation(control,capture,root)?,
        Control::DispatcherReturnR4=>dispatcher_return_r4(capture,root)?,
        Control::PolicyMissingAcquisition|Control::PolicyWrongAcquisition|Control::PolicyPrematureSharedRelease
            |Control::PolicyOmittedNoop|Control::PolicyTraceOccurrence|Control::PolicySegmentIndex=>return Err("policy controls keep the admitted capture unchanged".into()),
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
        let derived=policy::Policy::decode(&read(&baseline.join("joined-observation-policy.json"))?)?;
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
                if control.policy() {
                    std::fs::hard_link(joined.join("startup-joined.json"),case_root.join("startup-joined.json")).map_err(|error|error.to_string())?;
                    receipt=Some(Digest::of(&read(&case_root.join("startup-joined.json"))?));
                    let mut supplied=derived.clone();policy_mutation(control,&mut supplied)?;
                    fujimod_xm1::bridge::write(&case_root.join("observation-policy.json"),&supplied.encode()?)?;
                    return Ok(match super::supplied_policy(&case_root,analysis){Ok(())=>Observed::Admitted,Err(error)=>Observed::Refused(error)});
                }
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

#[derive(Deserialize,Serialize)]
struct DispatcherEnvelope {invocations:Frozen,publications:Frozen,mode_reads:Vec<ModeSampleEnvelope>,live_words:Frozen}
#[derive(Deserialize,Serialize)]
struct ModeSampleEnvelope {boundary:ModeBoundaryEnvelope,cell:CellEnvelope}
#[derive(Deserialize,Serialize)]
struct ModeBoundaryEnvelope {instructions:owner::Count,core:Core,pc:Guest,registers:Frozen,frame:Frozen}
#[derive(Deserialize,Serialize)]
struct CellEnvelope {address:Guest,physical:Guest,words:Vec<u32>,other_words:Vec<u32>,physical_words:Vec<u32>,tcm:[[u32;2];2],decoder:u32}
fn mode_sample(control:Control,capture:&mut Envelope)->Result<(),String>{
    let mut dispatcher:DispatcherEnvelope=capture.dispatcher.parsed()?;
    require(dispatcher.mode_reads.len()==1,"control mode sample census differs")?;
    let sample=&mut dispatcher.mode_reads[0];
    match control {
        Control::ModeCell=>{sample.cell.address=Guest(0x61c00c);sample.cell.physical=Guest(0x61c00c);},
        Control::ModeBoundary=>{sample.boundary.instructions.0=sample.boundary.instructions.0.checked_add(1).ok_or("mode boundary mutation overflows")?;},
        _=>return Err("control is not a mode sample mutation".into()),
    }
    capture.dispatcher=Frozen::encoded(&dispatcher)?;Ok(())
}
fn repeat_zero_segment(capture:&mut Envelope)->Result<(),String>{
    let window=&mut capture.consumer.window;
    require(window.segments.len()<32_768,"control lacks room for a coherent duplicate zero segment")?;
    let mut attempts:Vec<AttemptEnvelope>=window.fetch_attempts.parsed()?;
    let source=attempts.first().ok_or("control actual attempted fetch absent")?.segment;
    let insertion=source+1;let original=window.segments.get(source).ok_or("control actual stop segment absent")?;
    let rows:BTreeSet<_>=attempts.iter().filter(|attempt|attempt.segment==source).map(|attempt|attempt.row).collect();
    let mut duplicate=original.clone();
    duplicate.id=Frozen::encoded(&insertion)?;
    duplicate.start_counts=original.end_counts.clone();duplicate.start_contexts=original.end_contexts.clone();
    let probes:[u64;2]=original.probe_range.parsed()?;duplicate.probe_range=Frozen::encoded(&[probes[1];2])?;
    duplicate.watch.records=duplicate.watch.records.into_iter().enumerate().filter(|(index,_)|rows.contains(index)).map(|(_,row)|row).collect();
    require(!duplicate.watch.records.is_empty(),"control actual stopped rows absent")?;
    for (index,segment) in window.segments.iter_mut().enumerate().skip(insertion){segment.id=Frozen::encoded(&(index+1))?;}
    for attempt in &mut attempts {if attempt.segment>=insertion {attempt.segment+=1;}}
    for row in 0..duplicate.watch.records.len(){attempts.push(AttemptEnvelope {segment:insertion,row,classification:AttemptClass::BoundaryAttempt});}
    attempts.sort_unstable_by_key(|attempt|(attempt.segment,attempt.row));window.fetch_attempts=Frozen::encoded(&attempts)?;
    window.segments.insert(insertion,duplicate);
    let source_epoch=capture.epochs.iter().position(|epoch|matches!(epoch.evidence,Evidence::ConsumerSegment {segment} if segment==source)).ok_or("control actual stop epoch absent")?;
    let mut duplicate_epoch=capture.epochs[source_epoch].clone();
    duplicate_epoch.start=duplicate_epoch.end;duplicate_epoch.start_counts=duplicate_epoch.end_counts;
    duplicate_epoch.evidence=Evidence::ConsumerSegment {segment:insertion};
    for epoch in capture.epochs.iter_mut().skip(source_epoch+1) {
        if let Evidence::ConsumerSegment {segment}=&mut epoch.evidence {if *segment>=insertion {*segment+=1;}}
    }
    capture.epochs.insert(source_epoch+1,duplicate_epoch);Ok(())
}
fn wrong_stop_own_count(capture:&mut Envelope)->Result<(),String>{
    let attempts:Vec<AttemptEnvelope>=capture.consumer.window.fetch_attempts.parsed()?;
    let attempt=attempts.first().ok_or("control actual attempted fetch absent")?;
    let row=capture.consumer.window.segments.get_mut(attempt.segment).ok_or("control actual stop segment absent")?.watch.records.get_mut(attempt.row).ok_or("control actual stop row absent")?;
    let own=row.core_instructions.as_mut().ok_or("control actual stopped own-core count absent")?;
    *own=own.checked_add(1).ok_or("control own-core count mutation overflows")?;Ok(())
}
fn missing_added_guard(capture:&mut Envelope,root:&Path)->Result<(),String>{
    for (index,epoch) in capture.epochs.iter_mut().enumerate() {
        let Evidence::Watch {file,..}=&mut epoch.evidence else{continue};
        let mut watch:WatchEnvelope=serde_json::from_slice(&read(&root.join(&file.path))?).map_err(|error|error.to_string())?;
        let before=watch.classes.len();watch.classes.retain(|class|class.range!=[0x61c008,0x61c00c]);
        if before==watch.classes.len(){continue;}
        watch.records.retain(|row|!(row.source==Access::CpuRead&&u64::from(row.physical.0)<0x61c00c&&0x61c008<u64::from(row.physical.0)+row.size));
        for (span,kinds) in super::watch_scope(owner::classes(epoch.stage)) {
            for kind in kinds {require(super::covered(&watch.classes,span,kind),"added-guard control removed a legacy guard")?;}
        }
        *file=persist_bound(root,&format!("control-added-guard-{index}.json"),&serde_json::to_vec(&watch).map_err(|error|error.to_string())?)?;
        return Ok(());
    }
    Err("control distinct shared mode guard absent".into())
}
fn dispatcher_fetch_mutation(control:Control,capture:&mut Envelope,root:&Path,original:&Capture)->Result<(),String>{
    let mut retained=[false;2];let mut removed=0usize;
    let changed=walk_watches(capture,root,true,true,|watch|{
        let before=watch.records.len();
        watch.records.retain(|row|{
            if row.source!=Access::CpuFetch||!(0x69cdf8..0x69cf84).contains(&row.physical.0){return true;}
            let keep=match control {
                Control::DispatcherOneFetch=>{
                    let Some(core)=row.core.map(usize::from) else{return true};
                    let keep=!retained[core];retained[core]=true;keep
                },
                Control::DispatcherAttemptOnly=>original.route.events.iter().any(|event|row.core==Some(event.core)
                    &&row.pc==Some(event.pc)&&row.instructions==Some(event.instructions)),
                _=>false,
            };
            if !keep {removed+=1;}keep
        });
        Ok(before!=watch.records.len())
    })?;
    require(changed&&removed!=0,"control dispatcher executed fetch stream absent")?;
    if matches!(control,Control::DispatcherOneFetch){require(retained==[true;2],"control dispatcher fetches lack a core")?;}
    Ok(())
}
/// Mutates the watch epoch whose last CPU row is the rolled-back fetch of its ending stop, stamped one past the epoch end.
fn epoch_end_mutation(control:Control,capture:&mut Envelope,root:&Path)->Result<(),String>{
    for (index,epoch) in capture.epochs.iter_mut().enumerate() {
        let Evidence::Watch {file,..}=&mut epoch.evidence else{continue};
        let mut watch:WatchEnvelope=serde_json::from_slice(&read(&root.join(&file.path))?).map_err(|error|error.to_string())?;
        let Some(position)=watch.records.iter().rposition(|row|matches!(row.source,Access::CpuRead|Access::CpuFetch|Access::CpuWrite)) else{continue};
        let terminal=watch.records[position].clone();
        if terminal.source!=Access::CpuFetch||terminal.instructions.map(|count|count.0)!=epoch.end.0.checked_add(1){continue;}
        match control {
            Control::EpochEndData=>{
                let data=watch.records.iter().rposition(|row|matches!(row.source,Access::CpuRead|Access::CpuWrite)).ok_or("control epoch CPU data access absent")?;
                let mut moved=watch.records.remove(data);moved.instructions=terminal.instructions;watch.records.push(moved);
            },
            Control::EpochEndSecondFetch=>watch.records.push(terminal),
            Control::EpochEndUnstopped=>{
                let row=&mut watch.records[position];
                let next=Guest(row.address.0.checked_add(4).ok_or("control fetch address overflows")?);
                row.address=next;row.physical=next;row.pc=Some(next);
            },
            _=>return Err("control is not an epoch-end mutation".into()),
        }
        *file=persist_bound(root,&format!("control-epoch-end-{index}.json"),&serde_json::to_vec(&watch).map_err(|error|error.to_string())?)?;
        return Ok(());
    }
    Err("control watch epoch ending at a stop absent".into())
}
/// Changes r4 at the loader's return from its first dispatcher call in the route event, the raw trace and the dispatcher boundary.
fn dispatcher_return_r4(capture:&mut Envelope,root:&Path)->Result<(),String>{
    let call=capture.route.calls.iter().find(|call|call.kind==CallKind::Loader&&call.target==Guest(0x69cdf8)).ok_or("loader dispatcher call absent")?;
    let id=call.returned.ok_or("loader dispatcher return absent")?.0;
    let event=capture.route.events.get_mut(id).ok_or("loader dispatcher return event absent")?;
    event.registers.r[4]^=1;let (core,pc,at,value)=(event.core,event.pc,event.instructions,event.registers.r[4]);
    rebind_trace(root,capture,id)?;
    let mut dispatcher:serde_json::Value=capture.dispatcher.parsed()?;
    let invocations=dispatcher.get_mut("invocations").and_then(serde_json::Value::as_array_mut).ok_or("dispatcher invocations absent")?;
    let returned=invocations.iter_mut().filter_map(|invocation|invocation.get_mut("returned")).find(|boundary|
        boundary.get("core")==Some(&serde_json::json!(core))&&boundary.get("pc")==Some(&serde_json::json!(pc))&&boundary.get("instructions")==Some(&serde_json::json!(at)))
        .ok_or("dispatcher return boundary absent")?;
    *returned.pointer_mut("/registers/r/4").ok_or("dispatcher return r4 absent")?=serde_json::json!(value);
    capture.dispatcher=Frozen::encoded(&dispatcher)?;Ok(())
}
fn pairs(sites:&[policy::OwnedSite])->BTreeSet<(u32,policy::Owner)>{
    sites.iter().flat_map(|site|site.owners.iter().map(|owner|(site.address,*owner))).collect()
}
/// Drops boundary releases of pairs no longer held after a mutation, so the mutated policy stays internally consistent.
fn rebuild(policy:&mut policy::Policy){
    let mut held=pairs(policy.initial.sites());
    for boundary in &mut policy.boundaries {
        let release:BTreeSet<_>=pairs(&boundary.release).intersection(&held).copied().collect();
        for pair in &release {held.remove(pair);}
        held.extend(pairs(&boundary.acquire));boundary.release=policy::grouped(&release);
    }
}
fn first_acquisition(policy:&policy::Policy)->Result<(usize,u32,policy::Owner),String>{
    policy.boundaries.iter().enumerate().find_map(|(index,boundary)|boundary.acquire.first().map(|site|(index,site.address,site.owners[0])))
        .ok_or_else(||"admitted policy has no in-window acquisition".into())
}
fn policy_mutation(control:Control,policy:&mut policy::Policy)->Result<(),String>{
    use policy::Owner;
    match control {
        Control::PolicyMissingAcquisition=>{
            let (index,address,owner)=first_acquisition(policy)?;
            let boundary=&mut policy.boundaries[index];
            let mut acquire=pairs(&boundary.acquire);acquire.remove(&(address,owner));boundary.acquire=policy::grouped(&acquire);
            rebuild(policy);
        },
        Control::PolicyWrongAcquisition=>{
            let (index,address,owner)=first_acquisition(policy)?;
            let wrong=match owner {
                Owner::RouteCall {origin}=>Owner::RouteScheduledTarget {origin},
                Owner::RouteScheduledTarget {origin}|Owner::DispatcherReturn {origin}=>Owner::RouteCall {origin},
                Owner::RoutePermanent|Owner::DispatcherPermanent=>return Err("admitted policy acquires a permanent owner".into()),
            };
            for (position,boundary) in policy.boundaries.iter_mut().enumerate().skip(index) {
                let list=if position==index {&mut boundary.acquire}else{&mut boundary.release};
                let mut set=pairs(list);
                if set.remove(&(address,owner)) {set.insert((address,wrong));*list=policy::grouped(&set);}
            }
            rebuild(policy);
        },
        Control::PolicyPrematureSharedRelease=>{
            let mut held=pairs(policy.initial.sites());let mut chosen=None;
            for (index,boundary) in policy.boundaries.iter().enumerate() {
                let release=pairs(&boundary.release);
                chosen=held.iter().find(|(address,owner)|!release.contains(&(*address,*owner))
                    &&held.range((*address,Owner::RoutePermanent)..).take_while(|(other,_)|other==address).count()>=2).map(|pair|(index,*pair));
                if chosen.is_some() {break;}
                for pair in &release {held.remove(pair);}
                held.extend(pairs(&boundary.acquire));
            }
            let (index,pair)=chosen.ok_or("admitted policy has no shared-owned site inside the window")?;
            let boundary=&mut policy.boundaries[index];
            let mut release=pairs(&boundary.release);release.insert(pair);boundary.release=policy::grouped(&release);
            rebuild(policy);
        },
        Control::PolicyOmittedNoop=>{
            let index=policy.boundaries.iter().position(|boundary|boundary.acquire.is_empty()&&boundary.release.is_empty()).ok_or("admitted policy has no no-action boundary")?;
            policy.boundaries.remove(index);
        },
        Control::PolicyTraceOccurrence=>{
            if let Some(index)=policy.boundaries.windows(2).position(|pair|pair[0].instructions==pair[1].instructions&&(pair[0].core,pair[0].pc)!=(pair[1].core,pair[1].pc)) {
                let (left,right)=policy.boundaries.split_at_mut(index+1);
                std::mem::swap(&mut left[index].core,&mut right[0].core);std::mem::swap(&mut left[index].pc,&mut right[0].pc);
            }else{
                let last=policy.boundaries.last_mut().ok_or("admitted policy has no trace boundary")?;
                last.trace_index=last.trace_index.checked_add(1).ok_or("trace index mutation overflows")?;
            }
        },
        Control::PolicySegmentIndex=>{
            let index=policy.segments.windows(2).position(|pair|pair[0].trace_end>pair[0].trace_begin).ok_or("admitted policy has no movable segment cut")?;
            policy.segments[index].trace_end-=1;policy.segments[index+1].trace_begin-=1;
        },
        _=>return Err("control does not mutate the observation policy".into()),
    }
    Ok(())
}
