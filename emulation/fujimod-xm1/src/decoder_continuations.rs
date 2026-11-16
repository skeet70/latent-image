use super::*;
use fujimod_emulator_cpu::{Hook, HookId};
use std::{ffi::c_void, sync::atomic::{AtomicU64, Ordering}};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum Entry { Init, Feed, Progress, Complete, Finish }
#[derive(Deserialize)]
struct Export { kind: Entry, symbol: String, veneer: Guest, address: Guest, bytes: ByteCount }
#[derive(Deserialize)]
struct Publication { address: Guest, old: u32, new: u32 }
#[derive(Deserialize)]
struct Contract {
    schema: u32, firmware_sha256: Digest, candidate_dat_sha256: Digest, candidate_image4_sha256: Digest,
    body_sha256: Digest, symbols_sha256: Digest, body_bytes: ByteCount, body_start: Guest,
    bss: [Guest;2], entries: Vec<Export>, retired: Vec<[Guest;2]>, publications: Vec<Publication>,
    producer_sha256: Digest, producer_symbols_sha256: Digest,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum Role { Retained, Owned }
#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all="kebab-case")]
enum IrqEntryMode { Natural, ProgressAlignedFixture, CompleteAlignedFixture }
impl IrqEntryMode { fn target(self)->Option<Entry>{match self {Self::Natural=>None,Self::ProgressAlignedFixture=>Some(Entry::Progress),Self::CompleteAlignedFixture=>Some(Entry::Complete)}} }
#[derive(Serialize)]
#[serde(rename_all="kebab-case")]
enum QualificationScope { NaturalProducerIrqImage5Entry, SyntheticAlignedEntryControl }
#[derive(Serialize)]
struct Inputs { role: Role, irq_entry_mode: IrqEntryMode, run: FileBinding, update: FileBinding, rom: FileBinding, executable: Digest, contract: Option<FileBinding>, body: Option<FileBinding>, symbols: Option<FileBinding>, sources: Vec<SourceBinding> }
#[derive(Serialize)]
struct DecoderEvent { id: EventId, instructions: Count, pc: Guest, core: Core, registers: Registers, irq_exceptions: [u64;2], state: Vec<Words>, publication: Words, busy: Words }
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
enum Service { Task, Take, Give, Route, ClockTake, ClockGive, Attach, Detach, Marker }
const SERVICES: [(u32, Service);9] = [(0x723cb8,Service::Task),(0x724b68,Service::Take),(0x724c00,Service::Give),(0x6499d0,Service::Route),(0x649c4c,Service::ClockTake),(0x649ddc,Service::ClockGive),(0x64e918,Service::Attach),(0x64eb20,Service::Detach),(0x42188,Service::Marker)];
#[derive(Serialize)]
struct Invocation { service: Service, entered: EventId, returned: Option<EventId>, continuation: Guest, core: Core, sp: Guest, output: Option<Words> }
#[derive(Serialize)]
struct CallbackInvocation { kind: Entry, entered: EventId, returned: Option<EventId>, continuation: Guest, core: Core, sp: Guest, original_sp: Guest, padding: ByteCount, padding_origin: Option<PaddingOrigin>, caller: Words, frame_before: FrameWord, frame_after: Option<FrameWord>, restored_sp: Option<Guest> }
#[derive(Serialize)]
struct Evidence { schema: u32, inputs: Inputs, events: Vec<DecoderEvent>, calls: Vec<Invocation>, callbacks: Vec<CallbackInvocation>, snapshots: Vec<Snapshot>, ranges: Vec<continuation_view::RangeSnapshot>, transactions: TransactionBatch, irq_exceptions: [u64;2], instructions: Count, error: Option<String>, machine_failure: Option<String>, limits: Vec<Limit> }
#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
struct FrameWord { address: Guest, value: u32 }
fn frame_word(machine:&Machine,core:Core,address:Guest)->Result<FrameWord,String>{
    let mut bytes=[0;4];machine.session.board.read_memory(usize::from(core),u64::from(address.0),&mut bytes)?;
    Ok(FrameWord {address,value:u32::from_le_bytes(bytes)})
}
#[derive(Serialize)]
struct PaddingOrigin { kind: Entry, core: Core, instructions: Count, site: Guest, original_registers: Registers, padded_sp: Guest, frame: FrameWord }
#[derive(Serialize)]
#[serde(rename_all="kebab-case")]
enum Limit { CanonicalMethod4ReadOnly, SingleNaturalProducerSession, NoDeviceFailureOrCancellation, NoConcurrentEngineClients, NoWholeMachineWriteConfinement, NoRetiredComputedCallerClosure }
// Callback storage remains allocated until every hook is retired.
struct Exceptions { counters: [Box<AtomicU64>;2], hooks: Vec<(usize,HookId)> }
unsafe fn exception(kind: arm::Exception, data: *mut c_void) {
    if kind == arm::Exception::Irq { unsafe { &*data.cast::<AtomicU64>() }.fetch_add(1,Ordering::Relaxed); }
}
impl Exceptions {
    fn open(machine: &Machine) -> Result<Self,String> {
        let mut value=Self {counters: std::array::from_fn(|_|Box::new(AtomicU64::new(0))),hooks:Vec::with_capacity(2)};
        for core in 0..2 {
            let hook=machine.session.board.cpu(core).and_then(|cpu|unsafe { cpu.install_hook(Hook::Exception {callback:exception,data:(&*value.counters[core] as *const AtomicU64).cast_mut().cast()}) });
            match hook {Ok(hook)=>value.hooks.push((core,hook)),Err(error)=>{value.close(machine)?;return Err(error);}}
        }
        Ok(value)
    }
    fn counts(&self)->[u64;2] { std::array::from_fn(|core|self.counters[core].load(Ordering::Relaxed)) }
    fn close(mut self,machine:&Machine)->Result<(),String> {
        let mut error=None;
        for (core,hook) in self.hooks.drain(..) { if let Err(problem)=machine.session.board.cpu(core).and_then(|cpu|cpu.remove_hook(hook)) {error=Some(problem);} }
        if let Some(error)=error {std::mem::forget(self);return Err(error);}
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum ReturnSite { Service, Callback }
impl ReturnSite { fn label(self)->&'static str {match self {Self::Service=>"decoder-return",Self::Callback=>"decoder-callback-return"}} }
fn arm_return(machine:&mut Machine,address:Guest,purpose:ReturnSite)->Result<(),String>{
    match machine.traces.sites.get(&address.0) {
        Some(label) if label==purpose.label()=>Ok(()),
        Some(_)=>Err("decoder return address conflicts with a different-purpose trace site".into()),
        None=>machine.traces.add_sites(&machine.session,[(address.0,purpose.label().into())].into()),
    }
}
fn require(condition:bool,message:&str)->Result<(),String>{if condition {Ok(())}else{Err(message.into())}}
fn retain(source:&Path,output:&Path,name:&str)->Result<FileBinding,String>{let bytes=read(source)?;write(&output.join(name),&bytes)?;Ok(FileBinding {path:name.into(),sha256:Digest::of(&bytes)})}
struct CarrierPreimage([u32;97]);
fn bound_body(contract:&Contract,body:&[u8],symbols:&[u8],producer:&[u8],producer_symbols:&[u8],preimage:&CarrierPreimage,catalog:&mut Catalog)->Result<(),String>{
    require(contract.schema==1&&contract.firmware_sha256.as_str()==loader_orchestration_owned::SOURCE_UPDATE,"decoder canonical source contract differs")?;
    require(contract.body_start==Guest(0xba0020)&&contract.body_bytes.0 as usize==body.len()&&Digest::of(body).as_str()==contract.body_sha256.as_str()&&Digest::of(symbols).as_str()==contract.symbols_sha256.as_str(),"decoder body identity differs")?;
    require(contract.bss[0].0>=contract.body_start.0+contract.body_bytes.0&&contract.bss[1].0==contract.bss[0].0+52&&contract.bss[1].0<=0xbe0000,"decoder zero-once state bounds differ")?;
    require(Digest::of(producer).as_str()==contract.producer_sha256.as_str()&&contract.producer_sha256.as_str()=="c4002c01c67a18647ffe5e0b2bc561af3e212c59c5ebcec0dac77000c6ea7703"&&Digest::of(producer_symbols).as_str()==contract.producer_symbols_sha256.as_str()&&contract.producer_symbols_sha256.as_str()=="eebd59f77cf2a33eeb10e5b040e981df18c43099866e9626e21016a63d5952ce","decoder actual producer identity differs")?;
    let entries=[(Entry::Init,0x6ae360,"xm1_decoder_init"),(Entry::Feed,0x6ae500,"xm1_decoder_feed"),(Entry::Progress,0x6ae1dc,"xm1_decoder_progress"),(Entry::Complete,0x6ae310,"xm1_decoder_complete"),(Entry::Finish,0x6ae3f4,"xm1_decoder_finish")];
    require(contract.entries.len()==entries.len(),"decoder entry count differs")?;
    for (kind,veneer,symbol) in entries {let mut matches=contract.entries.iter().filter(|entry|entry.kind==kind);let entry=matches.next().ok_or("decoder entry absent")?;require(matches.next().is_none(),"decoder entry is not unique")?;require(entry.veneer==Guest(veneer)&&entry.symbol==symbol&&entry.address.0>=contract.body_start.0&&entry.address.0+entry.bytes.0<=contract.body_start.0+contract.body_bytes.0&&entry.bytes.0>0&&entry.address.0%4==0,"decoder external ABI differs")?;}
    require(contract.retired==[[Guest(0x6841f4),Guest(0x6848d8)],[Guest(0x6ae18c),Guest(0x6ae638)]],"decoder retirement extents differ")?;
    let image=catalog.0.iter_mut().find(|image|image.binding.kind==SourceKind::Image4).ok_or("canonical image4 absent")?;
    let mut addresses=BTreeSet::new();
    for row in &contract.publications {
        require(addresses.insert(row.address.0),"duplicate decoder publication")?;
        require(row.address.0%4==0&&(contract.retired.iter().any(|[low,high]|(low.0..high.0).contains(&row.address.0))||(image5_program::CARRIER[0]..image5_program::CARRIER[1]).contains(&row.address.0)||[0x72b12c,0x72b130,0x69ce18,0x69ce1c].contains(&row.address.0)),"decoder publication leaves owned surfaces")?;
        let offset=row.address.0.checked_sub(image.binding.start.0).ok_or("decoder publication precedes image4")? as usize;
        let word=image.bytes.get_mut(offset..offset+4).ok_or("decoder publication leaves image4")?;
        if !(image5_program::CARRIER[0]..image5_program::CARRIER[1]).contains(&row.address.0) {require(word==row.old.to_le_bytes(),"decoder publication preimage differs")?;}
        else {require(word==row.new.to_le_bytes()&&preimage.0[((row.address.0-image5_program::CARRIER[0])/4)as usize]==row.old,"decoder producer publication differs from canonical preimage or admitted artifact")?;}
        word.copy_from_slice(&row.new.to_le_bytes());
    }
    require([0x72b12c,0x72b130,0x69ce18,0x69ce1c].into_iter().all(|address|addresses.contains(&address)),"fixed image reservation publication is incomplete")?;
    require(Digest::of(&image.bytes).as_str()==contract.candidate_image4_sha256.as_str(),"decoder image4 publication identity differs")?;
    image.binding.sha256=Digest::of(&image.bytes);
    for [low,high] in &contract.retired {for address in (low.0..high.0).step_by(4){let offset=(address-image.binding.start.0)as usize;let word=u32::from_le_bytes(image.bytes[offset..offset+4].try_into().map_err(|_|"retired word size")?);if let Some(entry)=contract.entries.iter().find(|entry|entry.veneer==Guest(address)){let displacement=(((word&0xffffff)<<8)as i32>>6)as u32;require(word>>24==0xea&&address.wrapping_add(8).wrapping_add(displacement)==entry.address.0,"decoder veneer target differs")?;}else{require(word==0xe7f000f0,"retired decoder instruction remains executable")?;}}}
    Ok(())
}
fn state(machine:&Machine,maps:&[MemoryMap],contract:Option<&Contract>)->Result<Vec<Words>,String>{
    if let Some(contract)=contract {Ok(vec![continuation_view::words(&machine.session,maps,contract.bss[0],13)?])}
    else {[(Guest(0x89f750),3),(Guest(0x8e1900),8),(Guest(0x9c2f54),2)].into_iter().map(|(address,count)|continuation_view::words(&machine.session,maps,address,count)).collect()}
}
fn check(evidence:&Evidence,contract:Option<&Contract>)->Result<(),String>{
    require(evidence.error.is_none()&&evidence.machine_failure.is_none(),"decoder execution failed")?;
    let find=|pc:u32|evidence.events.iter().find(|event|event.pc==Guest(pc)).ok_or_else(||format!("decoder boundary {pc:#x} was not entered"));
    let init=find(0x6ae360)?;let complete=find(0x6ae310)?;let finish=find(0x6ae3f4)?;let returned=find(0x73b4b0)?;let done=find(0x73b6ac)?;let release=find(0x73b6c4)?;let callback=find(0x73b6e8)?;let image5=find(0xd985a4)?;
    require(init.registers.r[0]==0xbf1a00&&init.registers.r[1]==0x43e40000,"decoder initialization arguments differ")?;
    require(returned.registers.r[0]==0&&returned.id.0<done.id.0&&done.id.0<release.id.0&&release.id.0<callback.id.0&&callback.id.0<image5.id.0,"producer publication precedes successful decoding or consumer order differs")?;
    require(done.publication.values[0]==0&&release.publication.values[0]==1&&callback.publication.values[0]==1&&release.registers.r[0]==0x4c&&release.registers.r[5]==0&&callback.registers.r[1]==0&&callback.registers.r[3]==0x73c56c,"flashloader done/release/error ownership differs")?;
    require(complete.irq_exceptions[usize::from(complete.core)]>init.irq_exceptions[usize::from(complete.core)]&&complete.id.0<returned.id.0&&finish.id.0<returned.id.0,"actual IRQ completion is absent before producer return")?;
    require(evidence.snapshots.len()==2&&evidence.snapshots.iter().all(|snapshot|snapshot.expected_equal&&snapshot.guest_equal==[true;2]),"consumer image5 or BSS differs from canonical expectation")?;
    require(evidence.ranges.iter().all(|snapshot|snapshot.expected_equal&&snapshot.guest_equal==[true;4]),"owned bank, retired code or compressed input differs")?;
    let feeds=evidence.events.iter().filter(|event|event.pc==Guest(0x6ae500));
    require(feeds.clone().count()==23&&feeds.enumerate().all(|(index,event)|event.registers.r[0]==0x20000*(index as u32+1)),"actual storage feed coverage differs")?;
    require(returned.busy.values==[1]&&callback.busy.values==[0],"flashloader busy ownership differs")?;
    if contract.is_some(){
        let init_state=init.state.first().ok_or("private initialization state absent")?;require(init_state.values==[0;13],"private decoder state was not zero once before initialization")?;
        require(evidence.calls.iter().all(|call|call.returned.is_some()),"decoder service call did not return")?;
        let calls=evidence.calls.iter();
        let mut task_calls=calls.clone().filter(|call|call.service==Service::Task);
        let task_call=task_calls.next().ok_or("initialization-time task query absent")?;
        let task_return=task_call.returned.ok_or("task query did not return")?;
        require(task_calls.next().is_none()&&task_return.0<complete.id.0&&evidence.events[task_return.0].registers.r[0]!=0,"decoder task identity was absent or queried again during callbacks")?;
        let takes=calls.clone().filter(|call|call.service==Service::Take&&evidence.events[call.entered.0].registers.r[0]==0x48);
        let mut gives=calls.clone().filter(|call|call.service==Service::Give&&evidence.events[call.entered.0].registers.r[0]==0x48);
        require(takes.count()==2&&gives.clone().count()==2&&gives.any(|call|call.entered.0>complete.id.0&&call.entered.0<returned.id.0),"decoder resource48 completion/wait closure differs")?;
        let attaches=calls.clone().filter(|call|call.service==Service::Attach);let detaches=calls.filter(|call|call.service==Service::Detach);
        let registrations=attaches.clone().count();
        require(registrations>0&&registrations==detaches.clone().count(),"IRQ registrations were not released")?;
        for (attach,detach) in attaches.zip(detaches){let entered=&evidence.events[attach.entered.0];let handle=attach.output.as_ref().and_then(|output|output.values.first()).ok_or("IRQ attachment output absent")?;require(entered.registers.r[0]==98&&matches!(entered.registers.r[1],0x6ae1dc|0x6ae310)&&*handle!=0&&evidence.events[detach.entered.0].registers.r[0]==*handle&&attach.returned.is_some_and(|after|after.0<detach.entered.0),"source98 registration handle ownership differs")?;}
    }
    require(evidence.transactions.omitted==0,"decoder MMIO transaction bound exhausted")?;
    for callback in &evidence.callbacks {
        let entered=&evidence.events[callback.entered.0];let returned=&evidence.events[callback.returned.ok_or("IRQ callback did not return to its actual caller")?.0];
        require(entered.registers.sp==returned.registers.sp&&entered.registers.r[4..12]==returned.registers.r[4..12]&&entered.registers.sp.0%4==0,"IRQ callback changed caller-live SP or callee-saved registers")?;
        require(callback.caller.values==[0xe3500000,0x11a0e00f,0x11a0f000]&&callback.frame_after==Some(callback.frame_before),"IRQ caller instructions or preserved frame word differ")?;
        if callback.padding.0==0 {require(callback.original_sp==callback.sp&&callback.restored_sp.is_none()&&callback.padding_origin.is_none(),"natural IRQ entry was changed")?;}
        else {
            let origin=callback.padding_origin.as_ref().ok_or("aligned-entry fixture origin absent")?;
            require(origin.core==callback.core&&origin.kind==callback.kind&&origin.site.0+12==callback.continuation.0&&origin.instructions.0<entered.instructions.0&&origin.original_registers.sp==callback.original_sp&&origin.padded_sp==callback.sp&&origin.frame==callback.frame_before,"aligned-entry fixture does not bind the original caller")?;
            require(callback.padding.0==4&&callback.original_sp.0%8==4&&callback.sp.0.checked_add(4)==Some(callback.original_sp.0)&&callback.sp.0%8==0&&callback.restored_sp==Some(callback.original_sp),"aligned-entry fixture does not restore its declared four-byte padding")?;
        }
    }
    require(evidence.callbacks.iter().any(|callback|callback.kind==Entry::Progress)&&evidence.callbacks.iter().filter(|callback|callback.kind==Entry::Complete).count()==1,"IRQ callback coverage differs")?;
    let padded=evidence.callbacks.iter().filter(|callback|callback.padding.0!=0);
    match evidence.inputs.irq_entry_mode.target() {
        None=>require(padded.count()==0,"natural decoder evidence contains a stack fixture")?,
        Some(kind)=>require(padded.clone().count()==1&&padded.into_iter().all(|callback|callback.kind==kind),"aligned-entry fixture does not contain exactly one selected padding operation")?,
    }
    if contract.is_some(){require(evidence.calls.iter().all(|call|evidence.events[call.entered.0].registers.sp.0%8==0),"owned decoder reached a retained C service with unaligned SP")?;}
    let mut source=None;let mut destination=None;let mut length=None;let mut next_source=0x43e40014u64;let mut next_destination=0x40bf1a00u64;let mut submitted=0u64;let mut produced=0u64;let mut jobs=0;
    for row in &evidence.transactions.transactions {
        require(row.accepted,"decoder MMIO transaction was refused")?;
        if row.operation=="write" {match row.address {0xfff43040=>source=Some(row.value),0xfff43044=>length=Some(row.value),0xfff43048=>destination=Some(row.value),0xfff43004 if row.value==2=>{require(source==Some(next_source)&&destination==Some(next_destination),"decoder DMA source/output continuity differs")?;let bytes=length.ok_or("decoder job length absent")?;require(bytes>0&&bytes<=0x7ffff,"decoder Method4 job length differs")?;submitted+=bytes;next_source+=bytes;jobs+=1;},_=>{}}}
        if row.operation=="read"&&row.address==0xfff4304c {produced+=row.value;next_destination+=row.value;}
    }
    require(jobs>1&&submitted==0x2c66d1-20&&produced==0x4f1e60,"decoder consumed/output extent differs from canonical compressed record")?;
    Ok(())
}

pub(super) fn drive(mut options:BTreeMap<OptionKey,PathBuf>)->Result<(),String>{
    let role_path=options.remove(&OptionKey::DecoderWitness).ok_or("decoder witness role absent")?;
    let role=match role_path.to_str(){Some("retained")=>Role::Retained,Some("owned")=>Role::Owned,_=>return Err("decoder witness must be retained or owned".into())};
    let mode_path=options.remove(&OptionKey::DecoderIrqAlignment);
    let mode=match mode_path.as_deref().map(Path::to_str) {None|Some(Some("natural"))=>IrqEntryMode::Natural,Some(Some("progress-aligned-fixture"))=>IrqEntryMode::ProgressAlignedFixture,Some(Some("complete-aligned-fixture"))=>IrqEntryMode::CompleteAlignedFixture,_=>return Err("unknown decoder IRQ alignment mode".into())};
    require(mode==IrqEntryMode::Natural||role==Role::Owned,"stack fixtures require the same owned staged package")?;
    let artifact=options.remove(&OptionKey::DecoderArtifact);let producer=options.remove(&OptionKey::Image5Artifact).ok_or("decoder witness needs admitted image5 producer artifact")?;
    let mut take=|key|options.remove(&key).ok_or_else(||USAGE.to_owned());let run_path=take(OptionKey::Run)?;let update_path=take(OptionKey::Update)?;let rom_path=take(OptionKey::Rom)?;let analysis=take(OptionKey::Analysis)?;let output=take(OptionKey::Output)?;
    require(options.is_empty()&&(role==Role::Owned)==artifact.is_some(),"decoder witness options differ")?;
    std::fs::create_dir(&output).map_err(|error|error.to_string())?;
    let mut catalog=Catalog::open(&analysis)?;
    let mut carrier_preimage=CarrierPreimage([0;97]);for (index,word) in carrier_preimage.0.iter_mut().enumerate(){*word=catalog.word(Guest(image5_program::CARRIER[0]+index as u32*4))?.1;}
    let program=image5_program::Program::from_staged(image5_wire::Role::Owned,Some(&producer),&output,&mut catalog)?;
    let (contract,contract_binding,body_binding,symbols_binding)=if let Some(root)=artifact {
        let binding=retain(&root.join("decoder-contract.json"),&output,"decoder-contract.json")?;let raw=read(&output.join(&binding.path))?;let contract:Contract=serde_json::from_slice(&raw).map_err(|error|error.to_string())?;
        let body=retain(&root.join("decoder.bin"),&output,"decoder.bin")?;let symbols=retain(&root.join("decoder.symbols"),&output,"decoder.symbols")?;
        bound_body(&contract,&read(&output.join(&body.path))?,&read(&output.join(&symbols.path))?,&read(&output.join("image5-linked.bin"))?,&read(&output.join("image5-symbols.txt"))?,&carrier_preimage,&mut catalog)?;
        (Some(contract),Some(binding),Some(body),Some(symbols))
    }else{(None,None,None,None)};
    let run_binding=retain(&run_path,&output,"input-run.json")?;let update_binding=retain(&update_path,&output,"input-update.DAT")?;let rom_binding=retain(&rom_path,&output,"input-rom.bin")?;
    require(run_binding.sha256.as_str()=="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"&&rom_binding.sha256.as_str()=="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"&&update_binding.sha256.as_str()==contract.as_ref().map_or(program.update(),|contract|contract.candidate_dat_sha256.as_str()),"decoder run inputs differ")?;
    let inputs=Inputs {role,irq_entry_mode:mode,run:run_binding,update:update_binding,rom:rom_binding,executable:Digest::of(&read(&std::env::current_exe().map_err(|error|error.to_string())?)?),contract:contract_binding,body:body_binding,symbols:symbols_binding,sources:catalog.0.iter().map(|image|image.binding.clone()).collect()};
    persist(&output.join("inputs.json"),&inputs)?;
    let run_bytes=read(&run_path)?;fujimod_emulator_exr2_board::register_board_builder();fujimod_emulator_models::register();let document=RunDocument::parse(&run_bytes)?;let maps=run::memory_maps(&document)?;let mut session=run::open(&document,&ValidatedUpdate(read(&update_path)?),&read(&rom_path)?)?;session.board.set_execution_policy(document.execution_policy.into())?;let mut machine=Machine::new(document,session)?;
    require(machine.instructions()==0,"decoder qualifier did not start from fresh reset")?;
    let counters=Exceptions::open(&machine)?;
    let mut sites:BTreeMap<u32,String>=[0x73b4ac,0x6a96c0,0x6ae360,0x6ae500,0x6ae1dc,0x6ae310,0x6ae3f4,0x73b4b0,0x73b6ac,0x73b6c4,0x73b6e8,0xd985a4].into_iter().map(|pc|(pc,"decoder-boundary".into())).collect();
    if let Some(contract)=&contract {for entry in &contract.entries {sites.insert(entry.address.0,"decoder-body".into());}}
    if mode!=IrqEntryMode::Natural {sites.insert(0x402d4,"decoder-call-pre".into());sites.insert(0xd02d4,"decoder-call-pre".into());}
    machine.traces.bound_full_registers(NonZeroUsize::new(32768).ok_or("zero decoder trace bound")?);machine.traces.stop_labels=["decoder-boundary".into(),"decoder-body".into(),"decoder-service".into(),"decoder-return".into(),"decoder-callback-return".into(),"decoder-call-pre".into()].into();
    let mut events=Vec::new();let mut calls:Vec<Invocation>=Vec::new();let mut callbacks:Vec<CallbackInvocation>=Vec::new();let mut snapshots=Vec::new();let mut ranges=Vec::new();let mut cursor=0;let mut armed_services=false;let mut memory=None;let mut transactions=false;
    let mut padding_applied=false;
    let mut pending_padding:Option<PaddingOrigin>=None;
    let outcome=(||->Result<(),String>{
        machine.traces.arm(&machine.session,sites)?;
        loop {
            require(machine.instructions()<80_000_000,"decoder consumer deadline exhausted")?;
            let before=machine.instructions();machine.run(80_000_000-before);machine.require_running()?;
            while cursor<machine.traces.hits.len(){let hit=&machine.traces.hits[cursor];cursor+=1;let core=Core::try_from(hit.core)?;let pc=Guest(u32::from_str_radix(hit.address.trim_start_matches("0x"),16).map_err(|_|"decoder trace address malformed")?);let regs=registers(&machine,core)?;
                require(machine.session.board.read_register(usize::from(core),Register::Pc)?==u64::from(pc.0),"decoder event is not at the live stopped PC")?;
                if matches!(pc.0,0x402d4|0xd02d4) {
                    let kind=match regs.r[0] {0x6ae1dc=>Some(Entry::Progress),0x6ae310=>Some(Entry::Complete),_=>None};
                    if !padding_applied&&kind.is_some()&&mode.target()==kind {
                        require(regs.sp.0%8==4,"aligned-entry fixture requires the original word-aligned IRQ frame")?;
                        let frame=frame_word(&machine,core,regs.sp)?;
                        let padded_sp=Guest(regs.sp.0.checked_sub(4).ok_or("IRQ padding underflows SP")?);
                        machine.session.board.write_register(usize::from(core),Register::Arm32(arm::Register::R(13)),u64::from(padded_sp.0))?;
                        pending_padding=Some(PaddingOrigin {kind:kind.ok_or("IRQ fixture target absent")?,core,instructions:Count(machine.instructions()),site:pc,original_registers:regs,padded_sp,frame});
                        padding_applied=true;
                    }
                    continue;
                }
                let body_caller=contract.as_ref().map_or((0x6841f4..0x6848d8).contains(&regs.lr.0)||(0x6ae18c..0x6ae638).contains(&regs.lr.0),|contract|regs.lr.0>=contract.body_start.0&&regs.lr.0<contract.body_start.0+contract.body_bytes.0);
                let service=SERVICES.iter().find(|(address,_)|*address==pc.0).map(|(_,service)|*service);
                if service.is_some()&&!body_caller {continue;}
                let callback_kind=match pc.0 {0x6ae1dc=>Some(Entry::Progress),0x6ae310=>Some(Entry::Complete),_=>None};
                let id=EventId(events.len());let current=DecoderEvent {id,instructions:Count(machine.instructions()),pc,core,registers:regs,irq_exceptions:counters.counts(),state:state(&machine,&maps,contract.as_ref())?,publication:continuation_view::words(&machine.session,&maps,Guest(0x944440),9)?,busy:continuation_view::words(&machine.session,&maps,Guest(0x957600),1)?};
                for call in &mut calls {if call.returned.is_none()&&call.continuation==pc&&call.core==core&&call.sp==current.registers.sp {call.returned=Some(id);if matches!(call.service,Service::Attach|Service::ClockTake){let entered:&DecoderEvent=&events[call.entered.0];let pointer=entered.registers.r[if call.service==Service::Attach {2}else{1}];call.output=Some(continuation_view::words(&machine.session,&maps,Guest(pointer),1)?);}}}
                if let Some(index)=callbacks.iter().position(|callback|callback.returned.is_none()&&callback.continuation==pc&&callback.core==core&&callback.sp==current.registers.sp) {
                    let callback=&mut callbacks[index];let before:&DecoderEvent=&events[callback.entered.0];
                    require(before.registers.r[4..12]==current.registers.r[4..12],"IRQ callback changed callee-saved registers")?;
                    let frame=frame_word(&machine,core,callback.original_sp)?;
                    require(frame==callback.frame_before,"IRQ callback changed its caller frame word")?;
                    callback.frame_after=Some(frame);callback.returned=Some(id);
                    if callback.padding.0!=0 {machine.session.board.write_register(usize::from(core),Register::Arm32(arm::Register::R(13)),u64::from(callback.original_sp.0))?;callback.restored_sp=Some(callback.original_sp);}
                }else if callbacks.iter().any(|callback|callback.returned.is_none()&&callback.continuation==pc&&callback.core==core) {return Err("IRQ callback returned with a different SP".into());}
                if let Some(kind)=callback_kind {
                    require(!callbacks.iter().any(|callback|callback.returned.is_none()&&callback.core==core&&callback.sp==current.registers.sp&&callback.continuation==current.registers.lr),"IRQ callback return identity overlaps")?;
                    let caller_start=Guest(current.registers.lr.0.checked_sub(12).ok_or("IRQ caller address underflows")?);
                    let caller=continuation_view::words(&machine.session,&maps,caller_start,3)?;
                    for (index,word) in caller.values.iter().enumerate(){require(*word==catalog.word(Guest(caller_start.0+index as u32*4))?.1,"IRQ caller differs from canonical instructions")?;}
                    let padding_origin=if pending_padding.as_ref().is_some_and(|origin|origin.core==core&&origin.kind==kind){pending_padding.take()}else{None};
                    let (original_sp,padding,frame)=if let Some(origin)=&padding_origin {
                        require(current.registers.sp==origin.padded_sp&&current.registers.lr.0==origin.site.0+12&&current.registers.r==origin.original_registers.r,"padded IRQ call lost its original core/link/register binding")?;
                        (origin.original_registers.sp,ByteCount(4),origin.frame)
                    }else{(current.registers.sp,ByteCount(0),frame_word(&machine,core,current.registers.sp)?)};
                    callbacks.push(CallbackInvocation {kind,entered:id,returned:None,continuation:current.registers.lr,core,sp:current.registers.sp,original_sp,padding,padding_origin,caller,frame_before:frame,frame_after:None,restored_sp:None});
                    arm_return(&mut machine,current.registers.lr,ReturnSite::Callback)?;
                }
                if let Some(service)=service {
                    require(!calls.iter().any(|call|call.returned.is_none()&&call.core==core&&call.sp==current.registers.sp&&call.continuation==current.registers.lr),"decoder invocation return identity overlaps")?;
                    calls.push(Invocation {service,entered:id,returned:None,continuation:current.registers.lr,core,sp:current.registers.sp,output:None});
                    arm_return(&mut machine,current.registers.lr,ReturnSite::Service)?;
                }
                if pc==Guest(0x6a96c0)&&!armed_services {machine.traces.add_sites(&machine.session,SERVICES.into_iter().map(|(address,_)|(address,"decoder-service".into())).collect())?;armed_services=true;machine.session.board.transaction_watch_begin(vec![(0xfff43000,0xfff43100)],100000)?;transactions=true;
                    if let Some(contract)=&contract {let ranges=contract.retired.iter().map(|[low,high]|WatchRange {start:u64::from(low.0),end:u64::from(high.0),kinds:WatchKinds::CPU_FETCH.union(WatchKinds::WRITES)}).chain([WatchRange {start:u64::from(contract.body_start.0),end:u64::from(contract.bss[1].0),kinds:WatchKinds::CPU_FETCH.union(WatchKinds::WRITES)}]).collect();memory=Some(machine.session.board.memory_watch_begin_classified(ranges,500000,maps.clone(),ViewPolicy::Observe)?);}
                }
                if pc==Guest(0x6ae360){if let Some(contract)=&contract {let body=read(&output.join("decoder.bin"))?;ranges.push(continuation_view::snapshot_range(&machine.session,&maps,&output,id,contract.body_start,contract.body_bytes,"decoder-body",Some(&body))?);for (index,[low,high]) in contract.retired.iter().enumerate(){let image=catalog.0.iter().find(|image|image.binding.kind==SourceKind::Image4).ok_or("image4 absent")?;let low_offset=(low.0-image.binding.start.0)as usize;let expected=&image.bytes[low_offset..low_offset+(high.0-low.0)as usize];ranges.push(continuation_view::snapshot_range(&machine.session,&maps,&output,id,*low,ByteCount(high.0-low.0),&format!("decoder-retired-{index}"),Some(expected))?);}}}
                if pc==Guest(0x73b4b0){let payload=read(&analysis.join("payload.bin"))?;require(Digest::of(&payload).as_str()==image5_runtime::PAYLOAD_SHA256,"canonical storage payload differs")?;let record=image5_runtime::record(&payload)?;ranges.push(continuation_view::snapshot_range(&machine.session,&maps,&output,id,Guest(image5_runtime::STAGING[0]),ByteCount(record.len()as u32),"decoder-compressed-input",Some(record))?);}
                if pc==Guest(0xd985a4){for resource in [Resource::Image5,Resource::Image5Bss]{snapshots.push(continuation_view::snapshot(&machine.session,&maps,&output,id,resource,catalog.expected(resource)?)?);}}
                events.push(current);
                if pc==Guest(0xd985a4){return Ok(());}
            }
            require(machine.instructions()>before,"decoder observer made no progress")?;
        }
    })();
    let batch=if transactions {machine.session.board.transaction_watch_end()}else{Ok(TransactionBatch {transactions:Vec::new(),omitted:0})};
    let memory_result=if let Some(mut watch)=memory {
        let raw=watch.records.borrow().state();
        let ended=machine.session.board.memory_watch_end(&mut watch);
        ended.and_then(|()|{persist(&output.join("decoder-memory.json"),&raw)?;let state:RawWatch=serde_json::from_value(raw).map_err(|error|error.to_string())?;require(state.enabled&&state.violation.is_none(),"decoder memory observation was refused or exhausted")?;
            if let Some(contract)=&contract {for row in state.records {if matches!(row.source,Access::CpuWrite|Access::HostWrite|Access::ModelBusWrite)&&row.physical.0<contract.bss[0].0 {return Err("published decoder code was written during its producer interval".into());}if row.source==Access::CpuFetch&&contract.retired.iter().any(|[low,high]|(low.0..high.0).contains(&row.physical.0)){require(contract.entries.iter().any(|entry|entry.veneer==row.physical),"retired decoder interior was entered")?;}}}Ok(())})
    }else{Ok(())};
    let irq_exceptions=counters.counts();let hooks_result=counters.close(&machine);
    let batch=batch?;
    persist(&output.join("trace.json"),&machine.traces.hits)?;
    let evidence=Evidence {schema:1,inputs,events,calls,callbacks,snapshots,ranges,transactions:batch,irq_exceptions,instructions:Count(machine.instructions()),error:outcome.and(memory_result).and(hooks_result).err(),machine_failure:machine.failure.clone(),limits:vec![Limit::CanonicalMethod4ReadOnly,Limit::SingleNaturalProducerSession,Limit::NoDeviceFailureOrCancellation,Limit::NoConcurrentEngineClients,Limit::NoWholeMachineWriteConfinement,Limit::NoRetiredComputedCallerClosure]};
    persist(&output.join("decoder-continuations.json"),&evidence)?;
    check(&evidence,contract.as_ref())?;
    let scope=if mode==IrqEntryMode::Natural {QualificationScope::NaturalProducerIrqImage5Entry}else{QualificationScope::SyntheticAlignedEntryControl};
    persist(&output.join("decoder-verification.json"),&serde_json::json!({"schema":1,"accepted":true,"firmware_acceptance":mode==IrqEntryMode::Natural,"role":role,"irq_entry_mode":mode,"events":evidence.events.len(),"instructions":evidence.instructions,"irq_exceptions":evidence.irq_exceptions,"scope":scope,"limits":evidence.limits}))
}
