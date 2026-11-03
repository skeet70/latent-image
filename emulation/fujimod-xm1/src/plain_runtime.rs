use super::{Catalog, Event, Guest, SourceKind};
use super::continuation_view::{self, ByteCount, Digest, EventId, FileBinding, RangeSnapshot};
use super::plain_program::Program;
use super::plain_wire::{Evidence, Frame, Limit, Progress, Sample, SampleKind, StackLease};
use fujimod_emulator::board::{factory::MemoryMap, observation::{CpuDataAttribution, FetchAttribution, WatchKinds, WatchRange}};
use fujimod_xm1::machine::Machine;
use std::{collections::BTreeSet, path::Path};

const TASKS: [u32; 14] = [0x1109130,0x11e6e14,0x125cdfc,0x1273f60,0x1279828,0x127aabc,0x127bdf4,0x127cbb0,0x127da94,0x127f658,0x12812c4,0x128becc,0x13e36fc,0x13e9c0c];
pub(super) fn bind_payload(analysis: &Path, output: &Path, catalog: &Catalog) -> Result<FileBinding, String> {
    let path=analysis.join("payload.bin");
    let bytes=fujimod_xm1::bridge::read(&path)?;
    let sha256=Digest::of(&bytes);
    if sha256.as_str()!="8c6f806ef4554128e5be887a76e438a91b7d6cb030c940fc1c8fd6d32d5926c0" {return Err("plain canonical payload identity differs".into());}
    let plain=catalog.0.iter().find(|image|image.binding.kind==SourceKind::Plain).ok_or("plain source absent")?;
    if bytes.get(0x5a0048..0x900048)!=Some(plain.bytes.as_slice()){return Err("plain source differs from its independent raw payload interval".into());}
    let name="input-payload.bin";
    std::fs::hard_link(&path,output.join(name)).map_err(|error|format!("cannot retain canonical plain payload: {error}"))?;
    Ok(FileBinding {path:name.into(),sha256})
}
pub(super) struct Runtime {
    pub program: Program,
    pub evidence: Evidence,
    committed: u32,
    entry_sp: Option<Guest>,
    consumers: BTreeSet<u32>,
}
impl Runtime {
    pub(super) fn new(program: Program, payload: FileBinding) -> Self {
        let binding = program.binding();
        Self { program, evidence: Evidence { binding, payload, code: None, caller_before: None, entry: None, caller_return: None, raw_calls: Vec::new(), progress: Vec::new(), samples: Vec::new(), frames: Vec::new(), stack: None, limits: vec![Limit::PausedMappingSamplesNotTransientRemapExclusion, Limit::FrameSamplesNotContinuousStackConfinement, Limit::FiniteDestinationHaloNotWholeMachineWriteConfinement, Limit::RetainedServiceAndCallerLifetimeOnly] }, committed: 0, entry_sp: None, consumers: BTreeSet::new() }
    }
    pub(super) fn sites(&self) -> Vec<Guest> {
        let mut sites = vec![Guest(0x73b558), Guest(0x6a8f1c), Guest(0x73be78), self.program.return_pc()];
        if self.program.owned() { sites.push(Guest(0x6a98d4)); }
        sites.extend((0x6a9844..0x6a9938).step_by(4).flat_map(|pc| [Guest(pc), Guest(pc | 0x40000000)]));
        sites
    }
    pub(super) fn classes(&self) -> Vec<WatchRange> {
        vec![
            WatchRange { start: 0x6a9844, end: 0x6a9938, kinds: WatchKinds::CPU_FETCH.union(WatchKinds::WRITES) },
            WatchRange { start: 0x61c000, end: 0x61c220, kinds: WatchKinds::CPU_READ },
            WatchRange { start: 0x61c400, end: 0x61c600, kinds: WatchKinds::CPU_READ.union(WatchKinds::WRITES) },
            WatchRange { start: 0x10fff00, end: 0x1460100, kinds: WatchKinds::WRITES },
            WatchRange { start: 0xa1b500, end: 0xa1c300, kinds: WatchKinds::WRITES },
        ]
    }
    pub(super) fn attribution(&self, catalog: &Catalog, machine: &Machine) -> Result<FetchAttribution, String> {
        let mut ranges:Vec<_>=catalog.0.iter().map(|image| WatchRange { start: u64::from(image.binding.start.0), end: u64::from(image.binding.start.0) + image.bytes.len() as u64, kinds: WatchKinds::CPU_FETCH }).collect();
        let rom=&machine.document.boot_rom;
        ranges.push(WatchRange {start:rom.address.0,end:rom.address.0.checked_add(rom.bytes as u64).ok_or("plain ROM attribution range overflows")?,kinds:WatchKinds::CPU_FETCH});
        FetchAttribution::new(CpuDataAttribution::ReadsAndWrites,ranges)
    }
    fn view(&self, machine: &Machine, maps: &[MemoryMap], output: &Path, event: EventId, start: u32, end: u32, label: &str, catalog: &Catalog) -> Result<RangeSnapshot, String> {
        let source = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Plain).ok_or("plain source absent")?;
        let offset = start.checked_sub(source.binding.start.0).ok_or("plain snapshot precedes source")? as usize;
        let expected = source.bytes.get(offset..offset + (end-start) as usize).ok_or("plain snapshot exceeds source")?;
        continuation_view::snapshot_range(&machine.session, maps, output, event, Guest(start), ByteCount(end-start), label, Some(expected))
    }
    pub(super) fn capture(&mut self, machine: &Machine, maps: &[MemoryMap], output: &Path, event: &mut Event, catalog: &Catalog) -> Result<(), String> {
        let pc = event.pc.0;
        if pc==0x73beac {
            let slot=event.registers.r[5].checked_add(event.registers.r[4]).ok_or("plain deferred slot address overflows")?;
            event.state.push(continuation_view::words(&machine.session,maps,Guest(slot),2)?);
        }
        if !self.program.execution_allowed(event.pc) { return Err("plain fetch lies outside reviewed emitted code or uses an alias".into()); }
        if pc == 0x73b554 { self.evidence.caller_before = Some(event.id); }
        if pc == 0x6a9844 {
            if self.evidence.entry.is_some() { return Err("plain producer entered more than once".into()); }
            self.evidence.entry = Some(event.id);
            self.entry_sp = Some(event.registers.sp);
            event.state.push(continuation_view::words(&machine.session,maps,Guest(0x6a9844),61)?);
            let image=catalog.0.iter().find(|image|image.binding.kind==SourceKind::Image4).ok_or("plain carrier source absent")?;
            let low=(0x6a9844-image.binding.start.0) as usize;
            let expected=image.bytes.get(low..low+244).ok_or("plain carrier source extent")?;
            self.evidence.code=Some(continuation_view::snapshot_range(&machine.session,maps,output,event.id,Guest(0x6a9844),ByteCount(244),"plain-carrier-entry",Some(expected))?);
            if self.evidence.code.as_ref().is_some_and(|view|!view.expected_equal){return Err("plain entry carrier differs from every reviewed code and literal byte".into());}
            let current=continuation_view::words(&machine.session,maps,Guest(0x40054760),1)?;
            let task=continuation_view::words(&machine.session,maps,Guest(current.values[0]),3)?;
            let creation=continuation_view::words(&machine.session,maps,Guest(task.values[2]),10)?;
            let base=creation.values[5]&!0x40000000;
            let size=creation.values[4];
            let end=base.checked_add(size).ok_or("plain task stack allocation overflows")?;
            if event.core!=super::Core::Primary||current.values[0]!=1074138100||task.values[2]!=1074120676||creation.values[0]!=2||creation.values[8]!=60||size!=0xc00||base!=0xa1b600||end!=0xa1c200||event.registers.sp.0!=0xa1c1e0{return Err("plain caller is not the aligned admitted flashloader task frame and stack allocation".into());}
            let view=continuation_view::snapshot_range(&machine.session,maps,output,event.id,Guest(base),ByteCount(size),"plain-stack-entry",None)?;
            self.evidence.stack=Some(StackLease {event:event.id,current,task,creation,view});
        }
        if let Some(sp) = self.entry_sp {
            if self.evidence.caller_return.is_none() {
                event.state.push(continuation_view::words(&machine.session,maps,sp,8)?);
                event.state.push(continuation_view::words(&machine.session,maps,Guest(0x40054760),3)?);
                event.state.push(continuation_view::words(&machine.session,maps,Guest(0x400e4720),3)?);
                let start = sp.0.checked_sub(40).ok_or("plain frame guard underflows")?;
                let words = continuation_view::words(&machine.session, maps, Guest(start), 10)?;
                self.evidence.frames.push(Frame { event: event.id, words });
            }
        }
        if TASKS.contains(&pc)&&!self.consumers.contains(&pc) {
            let address=Guest(match event.core {super::Core::Primary=>0x40054760,super::Core::Secondary=>0x400e4720});
            let node=if let Some(current)=event.state.iter().find(|words|words.mapping.guest==address&&words.values.len()==3) {current.values[0]} else {
                let current=continuation_view::words(&machine.session,maps,address,3)?;
                let node=current.values[0];event.state.push(current);node
            };
            let task=continuation_view::words(&machine.session,maps,Guest(node),3)?;
            let creation=continuation_view::words(&machine.session,maps,Guest(task.values[2]),10)?;
            let base=creation.values[5]&!0x40000000;
            let top=base.checked_add(creation.values[4]).ok_or("plain consumer task stack overflows")?;
            if node!=event.registers.r[1]||task.values[2]!=event.registers.r[2]||event.registers.sp.0!=top||event.registers.cpsr&0x010002bf!=0x13{return Err("plain consumer entry differs from its live task trampoline and stack context".into());}
            event.state.extend([task,creation]);
        }
        if pc == 0x6a8f1c && self.evidence.entry.is_some() && self.evidence.caller_return.is_none() {
            self.evidence.raw_calls.push(event.id);
        }
        if self.evidence.entry.is_some()&&self.evidence.caller_return.is_none() {
            if self.program.returned_target(event.pc)==Some(Guest(0x657114))&&event.registers.r[0]!=0 {
                event.state.push(continuation_view::words(&machine.session,maps,Guest(event.registers.r[0]),8)?);
            }
            if self.program.returned_target(event.pc)==Some(Guest(0x6571a0))&&event.registers.r[0]!=0 {
                event.state.push(continuation_view::words(&machine.session,maps,Guest(event.registers.r[0]),24)?);
            }
        }
        if pc == 0x73b924 && self.evidence.entry.is_some() && self.evidence.caller_return.is_none() {
            let cumulative = event.registers.r[0];
            let raw = event.registers.lr == Guest(0x6f1a1c);
            let fresh = if raw && cumulative > self.committed {
                if cumulative != self.committed + 0x20000 || cumulative > 0x360000 { return Err("plain raw progress does not identify one fresh transfer block".into()); }
                let view = self.view(machine, maps, output, event.id, 0x1100000 + self.committed, 0x1100000 + cumulative, "plain-fresh-block", catalog)?;
                self.committed = cumulative;
                Some(view)
            } else { None };
            self.evidence.progress.push(Progress { event: event.id, cumulative: ByteCount(cumulative), fresh });
        }
        let extent = |index| match index { 0 => Ok((0x1100000,0x11ac5a0)), 2 => Ok((0x11c0000,0x11dcca0)), 3 => Ok((0x11e0000,0x144a5e0)), _ => Err("plain extent is not in the canonical table".to_owned()) };
        let sample = if pc == 0x6a9844 { Some((SampleKind::Initial, 0x1100000, 0x1460000, "plain-initial")) }
            else if pc == 0x73b558 && self.evidence.entry.is_some() { self.evidence.caller_return = Some(event.id); Some((SampleKind::ProducerReturn,0x1100000,0x1460000,"plain-caller-return")) }
            else if pc==0x73be70&&event.registers.r[7]==2 {let (start,end)=extent(2)?;Some((SampleKind::ExtentReady,start,end,"plain-extent-ready"))}
            else if matches!(pc,0x73be5c|0x73be74) { let (start,end)=extent(event.registers.r[7])?; Some((if pc==0x73be5c {SampleKind::ExtentHook} else {SampleKind::ExtentPublished},start,end,if pc==0x73be5c {"plain-extent-hook"} else {"plain-extent-published"})) }
            else if TASKS.contains(&pc) && self.consumers.insert(pc) { let (start,end)=if pc < 0x11c0000 {extent(0)?} else if pc < 0x11e0000 {extent(2)?} else {extent(3)?}; Some((SampleKind::TaskEntry,start,end,"plain-task-entry")) }
            else if pc == 0xd985a4 && self.consumers.insert(pc) { Some((SampleKind::Image5Entry,0x1100000,0x1460000,"plain-image5-entry")) }
            else { None };
        if let Some((kind,start,end,label)) = sample {
            let view = self.view(machine,maps,output,event.id,start,end,label,catalog)?;
            self.evidence.samples.push(Sample { event:event.id,kind,view });
        }
        if pc == 0x6a98d4 && self.program.owned() { return Err("plain producer reached the admitted missing-segment nonreturning path".into()); }
        Ok(())
    }
}
