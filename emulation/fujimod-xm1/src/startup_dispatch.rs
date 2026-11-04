#[path = "memory_view.rs"]
mod memory_view;
#[path = "startup_dispatch_check.rs"]
mod startup_dispatch_check;
#[path = "startup_dispatch_shared.rs"]
pub(crate) mod shared;

use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Register, arm};
use fujimod_emulator_devices::Device;
use fujimod_emulator_exr2_system::boot::Block98000;
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::guest::Press;
use fujimod_xm1::machine::{KeyPulse, Machine};
use fujimod_xm1::provenance::Guest;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::io::{BufWriter, Read, Write};
use sha2::Digest as _;
use std::process::ExitCode;

const START: u32 = 0x69cdf8;
const END: u32 = 0x69cf84;
const CALLBACK: u32 = 0x69cc6c;
const USAGE: &str = "startup-dispatch --run RUN --update DAT --boot-rom ROM --source-image IMAGE4 --linked-image IMAGE --case natural|reserved-alias --output NEW_DIR; --verify RECEIPT performs consumer-field validation only; --compare-retained DIR --compare-owned DIR --source-image IMAGE4 --linked-image IMAGE --output NEW_DIR performs independent comparison";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Role { Retained, Owned }
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Case { #[default] Natural, ReservedAlias }
impl TryFrom<std::ffi::OsString> for Case {
    type Error=String;
    fn try_from(value:std::ffi::OsString)->Result<Self,String>{match value.to_str(){Some("natural")=>Ok(Self::Natural),Some("reserved-alias")=>Ok(Self::ReservedAlias),_=>Err(USAGE.into())}}
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OptionKey { Run, Update, Rom, SourceImage, LinkedImage, Output, Verify, Case, CompareRetained, CompareOwned }
impl TryFrom<std::ffi::OsString> for OptionKey {
    type Error = String;
    fn try_from(value: std::ffi::OsString) -> Result<Self,String> {
        match value.to_str() {
            Some("--run") => Ok(Self::Run), Some("--update") => Ok(Self::Update),
            Some("--boot-rom") => Ok(Self::Rom), Some("--source-image") => Ok(Self::SourceImage),
            Some("--linked-image") => Ok(Self::LinkedImage), Some("--output") => Ok(Self::Output),
            Some("--verify") => Ok(Self::Verify), Some("--case") => Ok(Self::Case),
            Some("--compare-retained")=>Ok(Self::CompareRetained),Some("--compare-owned")=>Ok(Self::CompareOwned),_ => Err(USAGE.into()),
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "usize", into = "usize")]
enum Core { Primary, Secondary }
impl TryFrom<usize> for Core {
    type Error = String;
    fn try_from(value: usize) -> Result<Self, String> { match value { 0 => Ok(Self::Primary), 1 => Ok(Self::Secondary), _ => Err("core exceeds the two-core boundary".into()) } }
}
impl From<Core> for usize { fn from(core: Core) -> usize { match core { Core::Primary => 0, Core::Secondary => 1 } } }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum Phase { Install, Root, Diagnostic, Idle, Peer }
impl TryFrom<u32> for Phase {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, String> { match value { 0 => Ok(Self::Install), 1 => Ok(Self::Root), 2 => Ok(Self::Diagnostic), 3 => Ok(Self::Idle), 4 => Ok(Self::Peer), _ => Err("phase is outside the admitted natural route".into()) } }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum Service { Mode, Clear, Install, Task }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Source { CpuFetch, CpuRead, CpuWrite, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(transparent)]
struct Count(u64);
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
struct Digest(String);
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Registers { r: [u32; 13], sp: Guest, lr: Guest, cpsr: u32 }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct RamView { address: Guest, physical: Guest, words: Vec<u32>, other_words: Vec<u32>, physical_words: Vec<u32>, tcm: [[u32; 2]; 2], decoder: u32 }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Boundary { instructions: Count, core: Core, pc: Guest, registers: Registers, frame: RamView }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Attributes { boundary: Boundary, word: RamView }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum PerturbationKind { ReservedRecord, DescriptorAlias }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Perturbation { kind: PerturbationKind, instructions: Count, core: Core, pc: Guest, before: RamView, after: RamView }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Call { service: Service, entry: Boundary, returned: Option<Boundary>, descriptor_before: Option<RamView>, descriptor_after: Option<RamView>, domain: Option<RamView>, roots: Option<RamView>, attributes: Option<Attributes> }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Invocation { phase: Phase, entry: Boundary, calls: Vec<Call>, returned: Option<Boundary> }
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Publication { boundary: Boundary, cell: RamView }
#[derive(Deserialize, Serialize)]
struct Row { source: Source, address: Guest, physical: Guest, size: u64, core: Option<Core>, pc: Option<Guest>, instructions: Option<Count>, tick: Option<Count>, value: Option<u64>, before: Option<Vec<u8>> }
#[derive(Deserialize, Serialize)]
struct WatchState { enabled: bool, violation: Option<String>, records: Vec<Row> }
#[derive(Deserialize, Serialize)]
struct Segment { start: Count, end: Count, state: Result<WatchState,String>, teardown_error: Option<String> }
#[derive(Deserialize, Serialize)]
struct Identity { run: Digest, update: Digest, rom: Digest, source_image: Digest, linked_image: Digest, executable: Digest, checkpoint: Digest }
#[derive(Deserialize, Serialize)]
struct Files { trace: Digest, prefix: Digest, tail: Option<Digest>, code: Digest }
#[derive(Serialize)]
struct ProcessFailure<'a> { error: &'a str, primary_receipt_present: bool }
#[derive(Deserialize, Serialize)]
struct Report { schema: u32, role: Role, case: Case, identity: Identity, files: Files, instructions: Count, menu: Result<Option<[u32; 3]>, String>, error: Option<String>, machine_failure: Option<String>, invocations: Vec<Invocation>, publications: Vec<Publication>, perturbations: Vec<Perturbation>, segments: Vec<Segment>, code_samples: [usize; 2], code_fetches: [usize; 2] }

fn reg(session: &Session, core: Core, register: arm::Register) -> Result<u32, String> {
    u32::try_from(session.board.read_register(usize::from(core), Register::Arm32(register))?).map_err(|_| "register exceeds 32 bits".into())
}
fn registers(session: &Session, core: Core) -> Result<Registers, String> {
    let mut r = [0; 13];
    for (index, value) in r.iter_mut().enumerate() { *value = reg(session, core, arm::Register::R(index as u8))?; }
    Ok(Registers { r, sp: Guest(reg(session, core, arm::Register::R(13))?), lr: Guest(reg(session, core, arm::Register::R(14))?), cpsr: reg(session, core, arm::Register::Cpsr)? })
}
fn view(session: &Session, maps: &[MemoryMap], core: Core, address: Guest, words: usize) -> Result<RamView, String> {
    if address.0 % 4 != 0 || words == 0 || words > 80 { return Err("RAM view is not a bounded aligned word extent".into()); }
    let length = words * 4;
    let end = u64::from(address.0).checked_add(length as u64).ok_or("RAM view overflow")?;
    let span = static_span(maps, MemoryCore(usize::from(core)), MemoryAddress(u64::from(address.0))..MemoryAddress(end))?;
    if span.single.is_none() { return Err("RAM view crosses backing mappings".into()); }
    let other=1-usize::from(core);
    let other_span=static_span(maps,MemoryCore(other),MemoryAddress(u64::from(address.0))..MemoryAddress(end))?;
    if other_span.physical.0!=span.physical.0 || other_span.single.is_none(){return Err("both cores do not share this RAM backing".into());}
    let mut tcm = [[0; 2]; 2];
    for (index, banks) in tcm.iter_mut().enumerate() {
        let cpu = session.board.cpu(index)?;
        for (bank, value) in banks.iter_mut().enumerate() {
            *value = u32::try_from(cpu.coprocessor(arm::Coprocessor { cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank as u32, secure: 0, wide: false }, None)?).map_err(|_| "TCM register exceeds 32 bits")?;
            if tcm_window(TcmRegister(*value))?.is_some_and(|(low, high)| low < end && u64::from(address.0) < high) { return Err("RAM view overlaps a current TCM window".into()); }
        }
    }
    #[derive(Deserialize)]
    struct DecoderState {registers:[u32;5]}
    #[derive(Deserialize)]
    struct DecoderSnapshot {state:DecoderState}
    let snapshot:DecoderSnapshot=serde_json::from_value(session.board.inspect_device::<Block98000,_>(session.node("block98000")?,|device|device.fingerprint())?).map_err(|error|error.to_string())?;
    let decoder=snapshot.state.registers[1];
    if !matches!(decoder, 1 | 2) { return Err("RAM decoder does not select the admitted DRAM choice".into()); }
    let mut guest = [0; 320];
    let mut other_guest = [0; 320];
    let mut physical = [0; 320];
    session.board.read_memory(usize::from(core), u64::from(address.0), &mut guest[..length])?;
    session.board.read_memory(other,u64::from(address.0),&mut other_guest[..length])?;
    session.board.read_physical(span.physical.0, &mut physical[..length])?;
    if guest[..length] != physical[..length] || other_guest[..length]!=physical[..length] { return Err("both guest and physical committed RAM views differ".into()); }
    let words=|bytes:&[u8]|bytes.chunks_exact(4).map(|word|u32::from_le_bytes([word[0],word[1],word[2],word[3]])).collect();
    Ok(RamView { address, physical: Guest(u32::try_from(span.physical.0).map_err(|_| "physical address exceeds 32 bits")?), words:words(&guest[..length]),other_words:words(&other_guest[..length]),physical_words:words(&physical[..length]),tcm,decoder })
}
fn boundary(machine: &Machine, maps: &[MemoryMap], core: Core, pc: Guest) -> Result<Boundary, String> {
    if machine.session.board.read_register(usize::from(core), Register::Pc)? != u64::from(pc.0) { return Err("boundary is not at its live preexecution PC".into()); }
    let registers = registers(&machine.session, core)?;
    let low = registers.sp.0.checked_sub(256).ok_or("stack view underflow")?;
    let frame = view(&machine.session, maps, core, Guest(low), 68)?;
    Ok(Boundary { instructions: Count(machine.instructions()), core, pc, registers, frame })
}
fn descriptor(phase: Phase, index: usize) -> Result<[u32; 6], String> {
    match (phase, index) {
        (Phase::Root,0) => Ok([104,0x781960,1,0x400,0x69d160,0]),
        (Phase::Idle,0) => Ok([102,0x781968,15,0x400,0x69cbf0,0]),
        (Phase::Peer,0) => Ok([103,0x781970,15,0x400,0x69cbf0,0]),
        (Phase::Peer,1) => Ok([93,0x781978,13,0x400,0x69cd58,0]),
        _ => Err("task submission is outside the phase contract".into()),
    }
}
fn caller(core: Core, phase: Phase) -> Result<(Guest, Guest), String> {
    match (core, phase) {
        (Core::Primary,Phase::Install) => Ok((Guest(0x40063450),Guest(0x40a00))),
        (Core::Primary,Phase::Root) => Ok((Guest(0x40063450),Guest(0x40a38))),
        (Core::Primary,Phase::Idle) => Ok((Guest(0x40063450),Guest(0x40a40))),
        (Core::Primary,Phase::Diagnostic) => Ok((Guest(0x40063450),Guest(0x40a6c))),
        (Core::Secondary,Phase::Peer) => Ok((Guest(0x400f3410),Guest(0xd0af0))),
        (Core::Secondary,Phase::Diagnostic) => Ok((Guest(0x400f3410),Guest(0xd0a6c))),
        _ => Err("caller is outside the admitted natural route".into()),
    }
}
fn service(pc: u32) -> Option<Service> { match pc { 0x657184 => Some(Service::Mode), 0x7221a0 => Some(Service::Clear), 0x725a04 => Some(Service::Install), 0x725dd8 => Some(Service::Task), _ => None } }
fn is_return(pc: u32) -> bool { matches!(pc,0x40a00|0x40a38|0x40a40|0x40a6c|0xd0af0|0xd0a6c) }

fn perturb(machine:&Machine,maps:&[MemoryMap],core:Core,pc:Guest,kind:PerturbationKind,address:Guest,words:&[u32])->Result<Perturbation,String>{
    let before=view(&machine.session,maps,core,address,words.len())?;
    for (index,word) in words.iter().enumerate(){machine.session.board.write_physical(u64::from(before.physical.0)+4*index as u64,&word.to_le_bytes())?;}
    let after=view(&machine.session,maps,core,address,words.len())?;
    if after.words!=words {return Err("declared RAM perturbation did not commit".into());}
    Ok(Perturbation {kind,instructions:Count(machine.instructions()),core,pc,before,after})
}
#[derive(Default)]
struct Observation { case: Case, control_complete: bool, invocations: Vec<Invocation>, active: [Option<usize>; 2], publications: Vec<Publication>, perturbations: Vec<Perturbation>, cursor: usize }
impl Observation {
    fn collect(&mut self, machine: &Machine, maps: &[MemoryMap]) -> Result<(), String> {
        for hit in &machine.traces.hits[self.cursor..] {
            let pc = Guest(u32::from_str_radix(hit.address.strip_prefix("0x").ok_or("trace address has no prefix")?,16).map_err(|error| error.to_string())?);
            let core = Core::try_from(hit.core)?;
            if hit.code_word.is_some() && hit.cpsr&0x20!=0 {return Err("dispatcher word is entered in Thumb state".into());}
            let slot = usize::from(core);
            if pc.0 == START {
                if self.active[slot].is_some() { return Err("dispatcher reentry overlaps an active caller".into()); }
                let entry = boundary(machine,maps,core,pc)?;
                let phase = Phase::try_from(entry.registers.r[0])?;
                let expected = caller(core,phase)?;
                if (entry.registers.sp,entry.registers.lr) != expected { return Err("dispatcher caller frame or return differs".into()); }
                self.active[slot] = Some(self.invocations.len());
                self.invocations.push(Invocation { phase, entry, calls: Vec::new(), returned: None });
            } else if let Some(index) = self.active[slot] {
                let invocation = &mut self.invocations[index];
                let pending = invocation.calls.last().filter(|call| call.returned.is_none()).map(|call| call.entry.registers.lr);
                if pending == Some(pc) {
                    let returned = boundary(machine,maps,core,pc)?;
                    let call = invocation.calls.last_mut().ok_or("pending retained call disappeared")?;
                    if call.service == Service::Task { call.descriptor_after = Some(view(&machine.session,maps,core,Guest(invocation.entry.registers.sp.0-44),7)?); }
                    call.returned = Some(returned);
                    if self.case==Case::ReservedAlias && core==Core::Secondary && invocation.phase==Phase::Peer && invocation.calls.len()==1 {
                        self.perturbations.push(perturb(machine,maps,core,pc,PerturbationKind::DescriptorAlias,Guest(invocation.entry.registers.sp.0-20),&[0xa5f0a5f0])?);
                    }
                }
                if let Some(service) = service(pc.0) {
                    let entry = boundary(machine,maps,core,pc)?;
                    if (START..END).contains(&entry.registers.lr.0) {
                        if invocation.calls.last().is_some_and(|call| call.returned.is_none()) { return Err("retained calls overlap in one dispatcher context".into()); }
                        let (before,domain,roots) = if service == Service::Task {
                            let count = invocation.calls.iter().filter(|call| call.service == Service::Task).count();
                            let expected = descriptor(invocation.phase,count)?;
                            let address = Guest(invocation.entry.registers.sp.0-44);
                            if entry.registers.r[0] != address.0 { return Err("task wrapper does not receive the original descriptor address".into()); }
                            let before = view(&machine.session,maps,core,address,7)?;
                            if before.words[..6] != expected { return Err("consumer descriptor fields differ".into()); }
                            if self.case==Case::ReservedAlias && core==Core::Secondary && invocation.phase==Phase::Peer && count==0 {
                                self.perturbations.push(perturb(machine,maps,core,pc,PerturbationKind::ReservedRecord,Guest(0x40a1010c),&[0xa10200,0x400,1])?);
                            }
                            (Some(before),Some(view(&machine.session,maps,core,Guest(0x7cad9c+4*(expected[0]-1)),1)?),Some(view(&machine.session,maps,core,Guest(0x96bd20),2)?))
                        } else { (None,None,None) };
                        invocation.calls.push(Call { service,entry,returned:None,descriptor_before:before,descriptor_after:None,domain,roots,attributes:None });
                    }
                }
                if pc.0==0x725ed0 {
                    if let Some(call)=invocation.calls.last_mut().filter(|call|call.service==Service::Task && call.returned.is_none()){
                        let current=boundary(machine,maps,core,pc)?;
                        if current.registers.r[4]!=invocation.entry.registers.sp.0-44 {return Err("attribute consumer is not using this descriptor".into());}
                        call.attributes=Some(Attributes {boundary:current,word:view(&machine.session,maps,core,Guest(invocation.entry.registers.sp.0-20),1)?});
                    }
                }
                if is_return(pc.0) {
                    invocation.returned = Some(boundary(machine,maps,core,pc)?);
                    self.active[slot] = None;
                    if self.case==Case::ReservedAlias && core==Core::Secondary && invocation.phase==Phase::Peer {self.control_complete=true;}
                }
            }
            if matches!(pc.0,0x40c80|0xd0c80) {
                let address = if core == Core::Primary { Guest(0x4004f114) } else { Guest(0x400df0d4) };
                self.publications.push(Publication { boundary: boundary(machine,maps,core,pc)?, cell: view(&machine.session,maps,core,address,1)? });
            }
        }
        self.cursor = machine.traces.hits.len();
        Ok(())
    }
}

fn validate_view(view: &RamView) -> Result<(),String> {
    if view.words!=view.other_words || view.words!=view.physical_words {return Err("committed RAM views disagree".into());}
    let start=u64::from(view.address.0); let length=view.words.len()*4;
    if view.address.0%4!=0 || view.words.is_empty() || view.words.len()>80 || !matches!(view.decoder,1|2) || view.physical.0 != view.address.0 & 0x3fff_ffff {return Err("committed RAM view is outside the frozen mapping contract".into());}
    for banks in view.tcm {for bank in banks {if tcm_window(TcmRegister(bank))?.is_some_and(|(low,high)|low<start+length as u64 && start<high){return Err("committed view overlaps its recorded TCM window".into());}}}
    Ok(())
}
fn validate_boundary(boundary: &Boundary) -> Result<(),String> {
    validate_view(&boundary.frame)?;
    if boundary.frame.address.0 != boundary.registers.sp.0.checked_sub(256).ok_or("boundary stack underflow")? || boundary.frame.words.len()!=68 {return Err("boundary stack view differs from its live SP".into());}
    Ok(())
}

fn validate(report: &Report) -> Result<(), String> {
    if report.case==Case::ReservedAlias {
        let peer=report.invocations.iter().find(|invocation|invocation.entry.core==Core::Secondary && invocation.phase==Phase::Peer).ok_or("control phase4 is absent")?;
        if report.error.is_none() && report.machine_failure.is_none() && peer.returned.as_ref().is_some_and(|boundary|boundary.pc.0==0xd0af0) && peer.calls.len()==2 && peer.calls.iter().all(|call|call.returned.is_some()) && report.perturbations.len()==2 {return Ok(());}
        return Err("perturbed collection did not reach both phase4 returns".into());
    }
    if report.schema != 1 || report.error.is_some() || report.machine_failure.is_some() || report.menu != Ok(Some([1,5,2])) || !(330_000_000..=450_000_000).contains(&report.instructions.0) { return Err("route did not reach the complete admitted endpoint".into()); }
    let expected_update=match report.role {Role::Retained=>"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490",Role::Owned=>"0638ea6db8f3326660e67a4db8ac5ef065eb4d921ae921ffd1c24c925448523c"};
    if report.identity.update.0!=expected_update || report.identity.run.0!="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" || report.identity.rom.0!="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be" || report.identity.source_image.0!="f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16" || report.identity.linked_image.0!="8a592c4eeeedb3dc3977055639cf12742cd19f5526bb4f53d5c02d5589e2d1c0" {return Err("receipt identities differ from the admitted pair".into());}
    if report.invocations.len() != 6 { return Err("natural route does not have six dispatcher invocations".into()); }
    for (core, phases) in [(Core::Primary,&[Phase::Install,Phase::Root,Phase::Idle,Phase::Diagnostic][..]),(Core::Secondary,&[Phase::Peer,Phase::Diagnostic][..])] {
        if !report.invocations.iter().filter(|invocation| invocation.entry.core == core).map(|invocation| invocation.phase).eq(phases.iter().copied()) { return Err("per-core phase order differs".into()); }
    }
    for invocation in &report.invocations {
        validate_boundary(&invocation.entry)?;
        let exit=invocation.returned.as_ref().ok_or("dispatcher caller return is absent")?;
        validate_boundary(exit)?;
        if invocation.entry.pc.0!=START || exit.pc!=invocation.entry.registers.lr || exit.core!=invocation.entry.core || exit.instructions.0<invocation.entry.instructions.0 {return Err("dispatcher return does not match its caller context".into());}
        let entry = &invocation.entry.registers;
        let returned = &invocation.returned.as_ref().ok_or("dispatcher caller return is absent")?.registers;
        if (entry.sp,entry.lr) != caller(invocation.entry.core,invocation.phase)? || returned.sp != entry.sp || returned.r[4..12] != entry.r[4..12] { return Err("caller-live SP or registers differ".into()); }
        if returned.r[0] != if invocation.phase == Phase::Diagnostic { CALLBACK } else { 0 } { return Err("caller-visible dispatcher result differs".into()); }
        let expected_services: &[Service] = match invocation.phase { Phase::Install => &[Service::Mode,Service::Install], Phase::Root|Phase::Idle => &[Service::Task], Phase::Peer => &[Service::Task,Service::Task], Phase::Diagnostic => &[] };
        if !invocation.calls.iter().map(|call| call.service).eq(expected_services.iter().copied()) { return Err("retained call order differs from the selected mode0 route".into()); }
        let mut task = 0;
        for call in &invocation.calls {
            let after = call.returned.as_ref().ok_or("retained call return is absent")?;
            validate_boundary(&call.entry)?; validate_boundary(after)?;
            if service(call.entry.pc.0)!=Some(call.service) || call.entry.core!=invocation.entry.core || after.core!=call.entry.core || after.pc!=call.entry.registers.lr || !(START..END).contains(&after.pc.0) || call.entry.instructions.0<invocation.entry.instructions.0 || after.instructions.0<call.entry.instructions.0 || after.instructions.0>exit.instructions.0 {return Err("retained-call boundaries do not match the dispatcher context".into());}
            if after.registers.sp != call.entry.registers.sp || after.registers.r[4..12] != call.entry.registers.r[4..12] { return Err("retained return breaks its declared register/frame premise".into()); }
            if call.service == Service::Mode || call.service == Service::Install { if after.registers.r[0] != 0 { return Err("selected retained setup outcome is not zero".into()); } }
            if call.service == Service::Task {
                let expected = descriptor(invocation.phase,task)?;
                let before = call.descriptor_before.as_ref().ok_or("task descriptor preimage is absent")?;
                let after = call.descriptor_after.as_ref().ok_or("task descriptor committed postimage is absent")?;
                validate_view(before)?; validate_view(after)?;
                if before.address.0 != entry.sp.0-44 || before.words.len()!=7 || after.address != before.address || after.words.len()!=7 || before.words[..6]!=expected { return Err("task descriptor address, extent or fields differ".into()); }
                let domain = call.domain.as_ref().ok_or("task domain view is absent")?;
                let roots = call.roots.as_ref().ok_or("installed roots view is absent")?;
                validate_view(domain)?; validate_view(roots)?;
                if domain.words != [if invocation.entry.core==Core::Primary {1} else {2}] || roots.words != [0x4004dae0,0x400ddaa0] { return Err("task domain or installed roots differ".into()); }
                task += 1;
            }
        }
        let last = invocation.calls.last().and_then(|call| call.returned.as_ref()).map(|boundary| boundary.registers.cpsr).unwrap_or(entry.cpsr);
        if returned.cpsr & 0x0fff_ffff != last & 0x0fff_ffff { return Err("dispatcher changes caller-live CPSR bits beyond retained effects".into()); }
    }
    if report.publications.len()!=2 || report.publications.iter().filter(|publication| publication.boundary.core==Core::Primary).count()!=1 { return Err("both callback publications are not observed".into()); }
    for publication in &report.publications {
        validate_boundary(&publication.boundary)?; validate_view(&publication.cell)?;
        let address = if publication.boundary.core==Core::Primary {0x4004f114} else {0x400df0d4};
        if publication.cell.address.0!=address || publication.cell.words != [CALLBACK] { return Err("committed loader callback differs".into()); }
    }
    if report.segments.len()!=2 || report.segments[0].end.0 != report.segments[1].start.0 || report.segments[0].start.0!=0 || report.segments[1].end.0!=report.instructions.0 { return Err("watch segments have a guest-execution gap".into()); }
    let mut fetched = false;
    let mut fetches = [0;2];
    for segment in &report.segments {
        let state=segment.state.as_ref().map_err(|error|format!("classified watch cannot be decoded: {error}"))?;
        if !state.enabled || state.violation.is_some() || segment.teardown_error.is_some() { return Err("classified watch is incomplete".into()); }
        for row in &state.records {
            let end = u64::from(row.physical.0).checked_add(row.size).ok_or("watch extent overflow")?;
            if row.physical.0 < END && u64::from(START) < end {
                match row.source {
                    Source::CpuFetch => { if row.instructions.as_ref().ok_or("code fetch has no instruction counter")?.0 > report.segments[0].end.0 {return Err("dispatcher executes after its qualified frame interval".into());} if row.address != row.physical { return Err("dispatcher fetch uses an unsampled executable alias".into()); } fetches[usize::from(row.core.ok_or("fetch has no core")?)] += 1; fetched = true; },
                    Source::CpuWrite|Source::HostWrite|Source::ModelBusWrite if fetched => return Err("dispatcher carrier is written after its first fetch".into()),
                    _ => {},
                }
            }
            if matches!(row.source,Source::CpuRead|Source::CpuWrite) && row.pc.is_some_and(|pc| (START..END).contains(&pc.0)) {
                for invocation in &report.invocations {
                    let frame = &invocation.entry.frame;
                    let offset = invocation.entry.registers.sp.0.checked_sub(20).and_then(|address| address.checked_sub(frame.address.0)).ok_or("untouched-word frame offset underflow")?;
                    let unknown = u64::from(frame.physical.0).checked_add(u64::from(offset)).ok_or("untouched-word physical address overflow")?;
                    if u64::from(row.physical.0) < unknown+4 && unknown<end { return Err("dispatcher accesses the untouched descriptor word".into()); }
                }
            }
        }
    }
    if !fetched || report.code_fetches!=fetches || report.code_samples.iter().sum::<usize>()==0 || report.code_samples.contains(&0) { return Err("both-core dispatcher byte observations are absent or inconsistent".into()); }
    Ok(())
}

fn sites() -> BTreeMap<u32,String> {
    let mut sites: BTreeMap<_,_> = (START..END).step_by(4).map(|pc|(pc,"dispatcher-word".into())).collect();
    sites.extend([0x657184,0x7221a0,0x725a04,0x725dd8].map(|pc|(pc,"retained-entry".into())));
    sites.extend([0x40a00,0x40a38,0x40a40,0x40a6c,0xd0af0,0xd0a6c].map(|pc|(pc,"caller-return".into())));
    sites.insert(0x725ed0,"task-attributes".into());
    sites.extend([0x40c80,0xd0c80].map(|pc|(pc,"callback-published".into())));
    sites
}
fn classes(frames: bool) -> Vec<WatchRange> {
    let mut ranges = vec![WatchRange { start: u64::from(START), end:u64::from(END), kinds:WatchKinds::CPU_FETCH.union(WatchKinds::CPU_READ).union(WatchKinds::WRITES) }];
    ranges.extend([(0x4f114,0x4f118),(0xdf0d4,0xdf0d8)].map(|(start,end)| WatchRange {start,end,kinds:WatchKinds::CPU_READ.union(WatchKinds::WRITES)}));
    if frames { ranges.extend([(0x63350,0x63460),(0xf3310,0xf3420),(0xa1010c,0xa10118)].map(|(start,end)| WatchRange {start,end,kinds:WatchKinds::CPU_READ.union(WatchKinds::WRITES)})); }
    ranges
}
fn advance(machine: &mut Machine, observation: &mut Observation, maps: &[MemoryMap], count: u64) -> Result<(),String> {
    let end = machine.instructions().checked_add(count).ok_or("route instruction deadline overflow")?;
    while machine.instructions()<end { machine.run(end-machine.instructions()); machine.require_running()?; observation.collect(machine,maps)?; if observation.case==Case::ReservedAlias && observation.control_complete {break;} }
    Ok(())
}
fn route(machine: &mut Machine, observation: &mut Observation, maps: &[MemoryMap]) -> Result<(),String> {
    advance(machine,observation,maps,80_000_000u64.checked_sub(machine.instructions()).ok_or("startup prefix exceeds the initial cursor deadline")?)?;
    if !machine.menu()?.is_some_and(|menu|(menu.menu_id,menu.setting_id,menu.cursor)==(69,52,12)) { return Err("initial natural cursor differs".into()); }
    machine.traces.stop_labels.clear();
    let pulse=KeyPulse::instructions(8_000_000, 8_000_000);
    for key in std::iter::repeat_n("up",12).chain(["ok","ok"]) { machine.press_once(Press::parse(key)?,pulse)?; }
    machine.run(40_000_000); machine.require_running()?;
    machine.press_once(Press::parse("menu")?,pulse)?;
    machine.run(90_000_000); machine.require_running()?;
    Ok(())
}

fn persist<T:Serialize>(path:&Path,value:&T)->Result<(),String>{
    let file=std::fs::File::create(path).map_err(|error|error.to_string())?;
    let mut writer=BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer,value).map_err(|error|error.to_string())?;
    writer.flush().map_err(|error|error.to_string())
}

fn file_digest(path:&Path)->Result<Digest,String>{
    let mut file=std::fs::File::open(path).map_err(|error|error.to_string())?;
    let mut digest=sha2::Sha256::new();let mut buffer=[0;8192];
    loop {let count=file.read(&mut buffer).map_err(|error|error.to_string())?;if count==0{break;}digest.update(&buffer[..count]);}
    Ok(Digest(format!("{:x}",digest.finalize())))
}

fn drive() -> Result<(),String> {
    let mut args=std::env::args_os().skip(1);
    let mut options=BTreeMap::new();
    while let Some(flag)=args.next() {
        let flag=OptionKey::try_from(flag)?;
        if options.insert(flag,args.next().ok_or(USAGE)?).is_some() { return Err(USAGE.into()); }
    }
    if let Some(path)=options.remove(&OptionKey::Verify) { if !options.is_empty() {return Err(USAGE.into());} let report:Report=serde_json::from_slice(&read(&PathBuf::from(path))?).map_err(|error|error.to_string())?; return validate(&report); }
    if options.contains_key(&OptionKey::CompareRetained)||options.contains_key(&OptionKey::CompareOwned){
        let mut take=|key|options.remove(&key).map(PathBuf::from).ok_or_else(||USAGE.to_owned());
        let retained=take(OptionKey::CompareRetained)?;let owned=take(OptionKey::CompareOwned)?;let source=take(OptionKey::SourceImage)?;let linked=take(OptionKey::LinkedImage)?;let output=take(OptionKey::Output)?;
        if !options.is_empty(){return Err(USAGE.into());}
        return startup_dispatch_check::compare(&retained,&owned,&source,&linked,&output);
    }
    let case=options.remove(&OptionKey::Case).map(Case::try_from).transpose()?.unwrap_or_default();
    let mut take=|flag|options.remove(&flag).map(PathBuf::from).ok_or_else(||USAGE.to_owned());
    let run_path=take(OptionKey::Run)?; let update_path=take(OptionKey::Update)?; let rom_path=take(OptionKey::Rom)?; let source_path=take(OptionKey::SourceImage)?; let linked_path=take(OptionKey::LinkedImage)?; let output=take(OptionKey::Output)?;
    std::fs::create_dir(&output).map_err(|error|error.to_string())?;
    let outcome=(||->Result<(),String>{
    let run_bytes=read(&run_path)?; let update=read(&update_path)?; let rom=read(&rom_path)?; let source=read(&source_path)?; let linked=read(&linked_path)?;
    let run_digest=Digest(sha256(&run_bytes)); let update_digest=Digest(sha256(&update)); let rom_digest=Digest(sha256(&rom)); let source_digest=Digest(sha256(&source)); let linked_digest=Digest(sha256(&linked));
    let role=match update_digest.0.as_str() {"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"=>Role::Retained,"0638ea6db8f3326660e67a4db8ac5ef065eb4d921ae921ffd1c24c925448523c"=>Role::Owned,_=>return Err("DAT is outside the frozen original/owned dispatcher pair".into())};
    if run_digest.0!="efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" || rom_digest.0!="4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be" || source_digest.0!="f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16" || linked_digest.0!="8a592c4eeeedb3dc3977055639cf12742cd19f5526bb4f53d5c02d5589e2d1c0" || linked.len()!=376 {return Err("dispatcher observer inputs differ".into());}
    let mut code=if role==Role::Retained {source[(START-0x61c400) as usize..(END-0x61c400) as usize].to_vec()} else {linked};
    while code.len()<(END-START) as usize {code.extend_from_slice(&0xe320f000u32.to_le_bytes());}
    write(&output.join("bound-code.bin"),&code)?;
    fujimod_emulator_exr2_board::register_board_builder(); fujimod_emulator_models::register();
    let document=RunDocument::parse(&run_bytes)?; let maps=run::memory_maps(&document)?;
    let mut session=run::open(&document,&ValidatedUpdate(update),&rom)?; session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine=Machine::new(document,session)?;
    machine.disarm()?; let snapshot=machine.session.checkpoint(None)?.export()?; write(&output.join("reset.fujimod"),&snapshot)?;
    let restored=machine.session.import_snapshot(&read(&output.join("reset.fujimod"))?)?; machine.session.restore(&restored)?; machine.rearm()?;
    if machine.instructions()!=0 {return Err("reset checkpoint is not preexecution".into());}
    let points=sites(); machine.traces.bind_code_words(NonZeroUsize::new(16_384).ok_or("trace limit is zero")?,Guest(START),code,maps.clone())?;
    machine.traces.stop_labels=points.values().cloned().collect(); machine.traces.arm(&machine.session,points)?;
    let mut watch=machine.session.board.memory_watch_begin_classified(classes(true),65_536,maps.clone(),ViewPolicy::Observe)?;
    let mut observation=Observation {case,..Observation::default()}; let mut segments=Vec::new();
    let mut result=advance(&mut machine,&mut observation,&maps,8_000_000);
    if result.is_ok() && case==Case::Natural && (observation.invocations.len()!=6 || observation.active.iter().any(Option::is_some) || observation.publications.len()!=2) {result=Err("initial frame interval does not cover all six returns and both publications".into());}
    let state=watch.records.borrow().state(); let end=machine.instructions();
    persist(&output.join("watch-prefix.json"),&state)?;
    let teardown_error=machine.session.board.memory_watch_end(&mut watch).err();
    segments.push(Segment {start:Count(0),end:Count(end),state:serde_json::from_value(state).map_err(|error|error.to_string()),teardown_error});
    if result.is_ok() && (!segments[0].state.as_ref().is_ok_and(|state|state.enabled && state.violation.is_none()) || segments[0].teardown_error.is_some()) {result=Err("prefix memory watch is incomplete".into());}
    if result.is_ok() && case==Case::Natural {
        match machine.session.board.memory_watch_begin_classified(classes(false),65_536,maps.clone(),ViewPolicy::Observe) {
            Ok(mut second)=>{
                let mut tail_sites=machine.traces.sites.clone();
                tail_sites.remove(&0x7221a0);
                result=machine.traces.arm(&machine.session,tail_sites).and_then(|()|route(&mut machine,&mut observation,&maps));
                let state=second.records.borrow().state();
                persist(&output.join("watch-tail.json"),&state)?;
                let teardown_error=machine.session.board.memory_watch_end(&mut second).err();
                segments.push(Segment {start:Count(end),end:Count(machine.instructions()),state:serde_json::from_value(state).map_err(|error|error.to_string()),teardown_error});
            },
            Err(error)=>result=Err(error),
        }
    }
    let mut code_samples=[0;2]; for hit in &machine.traces.hits {if hit.code_word.is_some(){code_samples[hit.core]+=1;}}
    let mut code_fetches=[0;2]; for segment in &segments {if let Ok(state)=&segment.state {for row in &state.records {if row.source==Source::CpuFetch && (START..END).contains(&row.physical.0) {if let Some(core)=row.core {code_fetches[usize::from(core)]+=1;}}}}}
    persist(&output.join("trace.json"),&machine.traces.hits)?;
    let files=Files {trace:file_digest(&output.join("trace.json"))?,prefix:file_digest(&output.join("watch-prefix.json"))?,tail:if case==Case::Natural && output.join("watch-tail.json").is_file(){Some(file_digest(&output.join("watch-tail.json"))?)}else{None},code:file_digest(&output.join("bound-code.bin"))?};
    let report=Report {schema:1,role,case,identity:Identity {run:run_digest,update:update_digest,rom:rom_digest,source_image:source_digest,linked_image:linked_digest,executable:Digest(sha256(&read(&std::env::current_exe().map_err(|error|error.to_string())?)?)),checkpoint:Digest(sha256(&snapshot))},files,instructions:Count(machine.instructions()),menu:machine.menu().map(|menu|menu.map(|menu|[menu.menu_id,menu.setting_id,menu.cursor])),error:result.err(),machine_failure:machine.failure.clone(),invocations:observation.invocations,publications:observation.publications,perturbations:observation.perturbations,segments,code_samples,code_fetches};
    persist(&output.join("dispatch.json"),&report)?;
    validate(&report)
    })();
    if let Err(error)=&outcome {persist(&output.join("observer-error.json"),&ProcessFailure {error,primary_receipt_present:output.join("dispatch.json").is_file()})?;}
    outcome
}
fn main() -> ExitCode {match drive(){Ok(())=>ExitCode::SUCCESS,Err(error)=>{eprintln!("{error}");ExitCode::FAILURE}}}
