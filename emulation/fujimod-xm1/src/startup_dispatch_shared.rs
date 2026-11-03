use super::*;
use std::collections::BTreeSet;
use fujimod_xm1::startup_sites::{Component,StartupSites};

/// Clear service entry: dispatcher-permanent until the first stopped occurrence at or after `CLEAR_LIFETIME` aggregate instructions.
pub(crate) const CLEAR:u32=0x7221a0;
pub(crate) const CLEAR_LIFETIME:u64=8_000_000;

#[derive(Clone,Copy,Deserialize,PartialEq,Eq)]
enum ProvenPhase { Setup,Root,Diagnostic,Idle0,Idle1AndStandby }
impl ProvenPhase {
    fn matches(self,phase:Phase)->bool {matches!((self,phase),(Self::Setup,Phase::Install)|(Self::Root,Phase::Root)
        |(Self::Diagnostic,Phase::Diagnostic)|(Self::Idle0,Phase::Idle)|(Self::Idle1AndStandby,Phase::Peer))}
}
#[derive(Clone,Deserialize,PartialEq,Eq)]
pub(crate) struct CallFact {
    phase:ProvenPhase,instruction:Guest,continuation:Guest,target:Guest,
    stack_from_dispatcher_entry:i32,caller_live_words:Vec<i32>,
}
impl CallFact {
    pub(crate) fn validate(&self,image4:&[u8])->Result<(),String> {
        let word=|address:u32|->Result<u32,String>{
            let offset=address.checked_sub(0x61c400).ok_or("dispatcher call precedes image4")? as usize;
            Ok(u32::from_le_bytes(image4.get(offset..offset+4).ok_or("dispatcher call exceeds image4")?.try_into().map_err(|_|"dispatcher call word")?))
        };
        let destination=|address:u32,raw:u32|address.wrapping_add(8).wrapping_add((((raw&0xffffff)<<8) as i32>>6) as u32);
        let raw=word(self.instruction.0)?;
        // A BL returns to its next word; a B tail call returns to the word after the BL that entered the helper containing it.
        let linked=match raw&0xff000000 {
            0xeb000000=>self.instruction.0.checked_add(4)==Some(self.continuation.0),
            0xea000000=>match self.continuation.0.checked_sub(4) {
                Some(caller) if (START..END).contains(&caller)=>{
                    let entry=word(caller)?;
                    entry&0xff000000==0xeb000000 && (START..=self.instruction.0).contains(&destination(caller,entry))
                },
                _=>false,
            },
            _=>false,
        };
        if !(START..END).contains(&self.instruction.0) || !linked || destination(self.instruction.0,raw)!=self.target.0
            || !(-4096..=-48).contains(&self.stack_from_dispatcher_entry)
            || self.stack_from_dispatcher_entry%8!=0 || self.caller_live_words.len()>1024
            || self.caller_live_words.windows(2).any(|pair|pair[0]>=pair[1])
            || self.caller_live_words.iter().any(|offset|*offset<self.stack_from_dispatcher_entry || *offset>=0 || *offset%4!=0 || *offset == -20) {
            return Err("dispatcher live-call source facts differ".into());
        }
        Ok(())
    }
}

#[derive(Deserialize,Serialize)]
struct ModeRead { boundary:Boundary, cell:RamView }
#[derive(Deserialize,Serialize)]
struct LiveWords { core:Core, start:Count, end:Option<Count>, words:Vec<Guest>, preimage:Vec<RamView> }
#[derive(Deserialize,Serialize)]
pub(crate) struct Capture {
    invocations:Vec<Invocation>, publications:Vec<Publication>, mode_reads:Vec<ModeRead>,
    live_words:Vec<LiveWords>,
    #[serde(skip)]
    active:[Option<usize>;2],
    #[serde(skip)]
    call_protection:[Option<usize>;2],
    #[serde(skip)]
    loader_protection:[Option<usize>;2],
    #[serde(skip)]
    dispatcher_protection:[Option<usize>;2],
    #[serde(skip)]
    owned:bool,
    #[serde(skip)]
    facts:Vec<CallFact>,
    #[serde(skip)]
    owners:Option<StartupSites>,
    #[serde(skip)]
    clear_revoked:bool,
}
impl Capture {
    pub(crate) fn new(owned:bool,facts:Vec<CallFact>)->Self {
        Self {invocations:Vec::new(),publications:Vec::new(),mode_reads:Vec::new(),live_words:Vec::new(),
            active:[None;2],call_protection:[None;2],loader_protection:[None;2],dispatcher_protection:[None;2],owned,facts,owners:None,clear_revoked:false}
    }
    pub(crate) fn sites()->BTreeMap<u32,String> {
        [START,0x657184,0x657194,0x657198,0x65719c,CLEAR,0x725a04,0x725dd8,0x725ed0,0x40c80,0xd0c80]
            .into_iter().chain((START..END).step_by(4)).map(|pc|(pc,"shared-dispatch".into())).collect()
    }
    pub(crate) fn attach_owners(&mut self,owners:StartupSites){self.owners=Some(owners);}
    pub(crate) fn service_entry(pc:u32)->bool {service(pc).is_some()}
    fn arm(&self,machine:&mut Machine,pc:u32)->Result<(),String> {
        self.owners.as_ref().ok_or("shared dispatcher observation owners absent")?.acquire(machine,Component::Dispatcher,Guest(pc))
    }
    fn protect(&mut self,machine:&Machine,maps:&[MemoryMap],core:Core,addresses:Vec<Guest>)->Result<usize,String> {
        if self.live_words.len()>=64 || addresses.len()>1024 {return Err("live caller protection cap reached".into());}
        let preimage=addresses.iter().map(|address|view(&machine.session,maps,core,*address,1)).collect::<Result<Vec<_>,_>>()?;
        let index=self.live_words.len();
        self.live_words.push(LiveWords {core,start:Count(machine.instructions()),end:None,words:addresses,preimage});
        Ok(index)
    }
    pub(crate) fn collect(&mut self,machine:&mut Machine,maps:&[MemoryMap],start:usize,carrier_calls:&BTreeSet<u32>)->Result<(),String> {
        let hits=machine.traces.hits[start..].iter().map(|hit|{
            let pc=u32::from_str_radix(hit.address.strip_prefix("0x").ok_or("trace address prefix absent")?,16).map_err(|error|error.to_string())?;
            Ok((Core::try_from(hit.core)?,Guest(pc),hit.instructions))
        }).collect::<Result<Vec<_>,String>>()?;
        for (core,pc,instructions) in hits {
            if instructions!=machine.instructions(){return Err("dispatcher projection is not at a live stopped boundary".into());}
            let slot=usize::from(core);
            if carrier_calls.contains(&pc.0) && self.loader_protection[slot].is_none() {
                let regs=registers(&machine.session,core)?;
                let expected=if slot==0 {0x40063450}else{0x400f3410};
                if regs.sp.0!=expected {return Err("loader service frame differs from natural incoming SP".into());}
                self.loader_protection[slot]=Some(self.protect(machine,maps,core,(0..4).map(|i|Guest(expected+4*i)).collect())?);
            }
            if matches!(pc.0,0x436e8|0xd36e8) {
                if let Some(index)=self.loader_protection[slot].take(){self.live_words[index].end=Some(Count(instructions));}
            }
            if pc.0==START {
                if self.active[slot].is_some(){return Err("dispatcher reentry overlaps a live invocation".into());}
                if self.invocations.len()>=6 {return Err("dispatcher invocation cap reached".into());}
                let entry=boundary(machine,maps,core,pc)?;
                let phase=Phase::try_from(entry.registers.r[0])?;
                let phases=if core==Core::Primary {&[Phase::Install,Phase::Root,Phase::Idle,Phase::Diagnostic][..]}else{&[Phase::Peer,Phase::Diagnostic][..]};
                let ordinal=self.invocations.iter().filter(|invocation|invocation.entry.core==core).count();
                if phases.get(ordinal)!=Some(&phase){return Err("dispatcher natural phase order differs".into());}
                let expected=if slot==0 {0x40063450}else{0x400f3410};
                if entry.registers.sp.0!=expected || !carrier_calls.contains(&entry.registers.lr.0.checked_sub(4).ok_or("dispatcher return underflows")?) {
                    return Err("dispatcher caller does not come from admitted loader source".into());
                }
                let continuation=entry.registers.lr.0;
                self.active[slot]=Some(self.invocations.len());
                self.invocations.push(Invocation {phase,entry,calls:Vec::new(),returned:None});
                self.arm(machine,continuation)?;
            } else if let Some(index)=self.active[slot] {
                let pending=self.invocations[index].calls.last().filter(|call|call.returned.is_none()).map(|call|call.entry.registers.lr);
                if pending==Some(pc) {
                    let returned=boundary(machine,maps,core,pc)?;
                    if let Some(protection)=self.call_protection[slot].take(){self.live_words[protection].end=Some(Count(instructions));}
                    let invocation=&mut self.invocations[index];
                    let call=invocation.calls.last_mut().ok_or("pending dispatcher service absent")?;
                    if returned.registers.sp!=call.entry.registers.sp {return Err("dispatcher service returned with a different SP".into());}
                    if call.service==Service::Task {
                        let descriptor=call.descriptor_before.as_ref().ok_or("task descriptor preimage absent")?.address;
                        call.descriptor_after=Some(view(&machine.session,maps,core,descriptor,7)?);
                    }
                    call.returned=Some(returned);
                }
                if let Some(service)=service(pc.0) {
                    let entry=boundary(machine,maps,core,pc)?;
                    if (START..END).contains(&entry.registers.lr.0) {
                        let invocation=&self.invocations[index];
                        if invocation.calls.last().is_some_and(|call|call.returned.is_none()){return Err("dispatcher services overlap".into());}
                        let (before,domain,roots)=if service==Service::Task {
                            let ordinal=invocation.calls.iter().filter(|call|call.service==Service::Task).count();
                            let expected=descriptor(invocation.phase,ordinal)?;
                            let address=Guest(invocation.entry.registers.sp.0-44);
                            if entry.registers.r[0]!=address.0{return Err("task descriptor address differs".into());}
                            let before=view(&machine.session,maps,core,address,7)?;
                            if before.words[..6]!=expected{return Err("task descriptor fields differ".into());}
                            (Some(before),Some(view(&machine.session,maps,core,Guest(0x7cad9c+4*(expected[0]-1)),1)?),Some(view(&machine.session,maps,core,Guest(0x96bd20),2)?))
                        }else{(None,None,None)};
                        let dispatcher_sp=invocation.entry.registers.sp.0;
                        let incoming=entry.registers.sp.0;
                        if incoming>dispatcher_sp || incoming%8!=0 {return Err("dispatcher service stack is not aligned below entry".into());}
                        let live=if self.owned {
                            let mut candidates=self.facts.iter().filter(|fact|fact.phase.matches(invocation.phase)
                                && fact.target==pc && fact.continuation==entry.registers.lr);
                            let fact=candidates.next().ok_or("dispatcher live-call proof absent")?;
                            if candidates.any(|other|other!=fact) || incoming!=dispatcher_sp.wrapping_add_signed(fact.stack_from_dispatcher_entry) {
                                return Err("dispatcher live-call proof is ambiguous or has a different incoming SP".into());
                            }
                            fact.caller_live_words.iter().map(|offset|Guest(dispatcher_sp.wrapping_add_signed(*offset))).collect()
                        }else{
                            [-16i32,-12,-8,-4].into_iter().map(|offset|Guest(dispatcher_sp.wrapping_add_signed(offset))).collect()
                        };
                        let continuation=entry.registers.lr.0;
                        if self.dispatcher_protection[slot].is_none() {
                            self.dispatcher_protection[slot]=Some(self.protect(machine,maps,core,
                                [-16i32,-12,-8,-4].into_iter().map(|offset|Guest(dispatcher_sp.wrapping_add_signed(offset))).collect())?);
                        }
                        self.call_protection[slot]=Some(self.protect(machine,maps,core,live)?);
                        self.invocations[index].calls.push(Call {service,entry,returned:None,descriptor_before:before,descriptor_after:None,domain,roots,attributes:None});
                        self.arm(machine,continuation)?;
                    }
                }
                if pc.0==0x725ed0 {
                    let invocation=&mut self.invocations[index];
                    if let Some(call)=invocation.calls.last_mut().filter(|call|call.service==Service::Task && call.returned.is_none()) {
                        let current=boundary(machine,maps,core,pc)?;
                        let descriptor=call.descriptor_before.as_ref().ok_or("task descriptor preimage absent")?.address;
                        if current.registers.r[4]!=descriptor.0 {return Err("attribute consumer descriptor differs".into());}
                        call.attributes=Some(Attributes {boundary:current,word:view(&machine.session,maps,core,Guest(descriptor.0+24),1)?});
                    }
                }
                if pc==self.invocations[index].entry.registers.lr {
                    let returned=boundary(machine,maps,core,pc)?;
                    let entry=&self.invocations[index].entry.registers;
                    if returned.registers.sp!=entry.sp || returned.registers.r[4..12]!=entry.r[4..12] {return Err("dispatcher live caller state changed".into());}
                    if let Some(protection)=self.dispatcher_protection[slot].take(){self.live_words[protection].end=Some(Count(instructions));}
                    self.invocations[index].returned=Some(returned);self.active[slot]=None;
                }
            }
            if pc.0==0x657194 && self.active[slot].is_some_and(|index|self.invocations[index].phase==Phase::Install
                &&self.invocations[index].calls.last().is_some_and(|call|call.service==Service::Mode&&call.returned.is_none())) {
                self.mode_reads.push(ModeRead {boundary:boundary(machine,maps,core,pc)?,cell:view(&machine.session,maps,core,Guest(0x61c008),1)?});
            }
            if matches!(pc.0,0x40c80|0xd0c80) {
                let address=Guest(if slot==0 {0x4004f114}else{0x400df0d4});
                self.publications.push(Publication {boundary:boundary(machine,maps,core,pc)?,cell:view(&machine.session,maps,core,address,1)?});
            }
            if !self.clear_revoked&&instructions>=CLEAR_LIFETIME {
                if self.invocations.len()!=6||self.active.iter().any(Option::is_some)||self.publications.len()!=2
                    ||self.invocations.iter().any(|invocation|invocation.returned.is_none()||invocation.calls.iter().any(|call|call.returned.is_none())) {
                    return Err("dispatcher initial8M frame interval is incomplete".into());
                }
                self.owners.as_ref().ok_or("shared dispatcher observation owners absent")?.revoke(machine,Component::Dispatcher,&[Guest(CLEAR)])?;
                self.clear_revoked=true;
            }
        }
        Ok(())
    }
    pub(crate) fn check_writes(&self,raw:&[u8])->Result<(),String> {
        #[derive(Deserialize)]struct Watch {records:Vec<Row>}
        let watch:Watch=serde_json::from_slice(raw).map_err(|error|error.to_string())?;
        for row in watch.records.iter().filter(|row|matches!(row.source,Source::CpuWrite|Source::HostWrite|Source::ModelBusWrite)) {
            let end=u64::from(row.physical.0).checked_add(row.size).ok_or("live caller write extent overflow")?;
            for protection in &self.live_words {
                if protection.words.iter().any(|word|u64::from(word.0&!0x40000000)<end && u64::from(row.physical.0)<u64::from(word.0&!0x40000000)+4) {
                    let at=row.instructions.as_ref().ok_or("live caller write clock absent")?.0;
                    if at<=protection.start.0 || protection.end.as_ref().is_some_and(|end|at>end.0){continue;}
                    return Err("write overlaps an actual live caller word".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn finish(&self)->Result<(),String> {
        if self.invocations.len()!=6 || self.invocations.iter().any(|invocation|invocation.returned.is_none() || invocation.calls.iter().any(|call|call.returned.is_none()))
            || self.mode_reads.len()!=1 || self.loader_protection.iter().any(Option::is_some) {
            return Err("dispatcher or caller subobservation is incomplete".into());
        }
        Ok(())
    }
}

#[derive(Clone,Copy,Deserialize,Serialize,PartialEq,Eq)]
#[serde(rename_all="lowercase")]
pub(crate) enum ByteOrder {Little,Big}
#[derive(Clone,Deserialize,Serialize,PartialEq,Eq)]
pub(crate) struct FetchStamp {
    pub(crate) source:Source,pub(crate) address:Guest,pub(crate) physical:Option<Guest>,pub(crate) size:u64,
    pub(crate) value:Option<u64>,pub(crate) before:Option<Vec<u8>>,pub(crate) byte_order:Option<ByteOrder>,
    pub(crate) core:Option<usize>,pub(crate) pc:Option<Guest>,pub(crate) instructions:Option<u64>,
    pub(crate) tick:Option<u64>,pub(crate) core_instructions:u64,
}
pub(crate) struct DataAttribution {
    pub(crate) own:u64,pub(crate) before:Option<[u8;4]>,pub(crate) byte_order:Option<ByteOrder>,pub(crate) latest:Option<FetchStamp>,
}
pub(super) struct FetchRecord {pub(super) row:Row,pub(super) own:Option<u64>,pub(super) order:Option<ByteOrder>,pub(super) attempt:bool}
#[derive(Default)]
pub(crate) struct Rows {
    rows:Vec<Row>,fetches:Vec<FetchRecord>,getter_fetches:Vec<FetchStamp>,mode_reads:Vec<(Row,Option<DataAttribution>)>,
    mode_writes:Vec<(Option<u64>,[u64;2])>,fetched:bool,
}
pub(crate) struct AccessRow {
    pub(crate) source:Source,pub(crate) address:Guest,pub(crate) physical:Guest,pub(crate) size:u64,
    pub(crate) core:Option<usize>,pub(crate) pc:Option<Guest>,pub(crate) instructions:Option<u64>,
    pub(crate) tick:Option<u64>,pub(crate) value:Option<u64>,pub(crate) own:Option<u64>,
    pub(crate) order:Option<ByteOrder>,pub(crate) attribution:Option<DataAttribution>,pub(crate) attempt:bool,pub(crate) interval:[u64;2],
}
impl Rows {
    pub(crate) fn push(&mut self,row:AccessRow)->Result<(),String>{
        let end=u64::from(row.physical.0).checked_add(row.size).ok_or("dispatcher access extent overflows")?;
        let code=u64::from(row.physical.0)<u64::from(END) && u64::from(START)<end;
        if row.source==Source::CpuFetch && !row.attempt && matches!(row.pc.map(|pc|pc.0),Some(0x657184|0x657194|0x657198|0x65719c)) {
            self.getter_fetches.push(FetchStamp {source:row.source,address:row.address,physical:Some(row.physical),size:row.size,
                value:row.value,before:None,byte_order:row.order,core:row.core,pc:row.pc,instructions:row.instructions,
                tick:row.tick,core_instructions:row.own.ok_or("getter fetch own-core count absent")?});
        }
        if row.source==Source::CpuRead && row.pc==Some(Guest(0x657194)) {
            self.mode_reads.push((Row {source:row.source,address:row.address,physical:row.physical,size:row.size,
                core:row.core.map(Core::try_from).transpose()?,pc:row.pc,instructions:row.instructions.map(Count),tick:row.tick.map(Count),
                value:row.value,before:None},row.attribution));
        }
        if u64::from(row.physical.0)<0x61c00c && 0x61c008<end && matches!(row.source,Source::CpuWrite|Source::HostWrite|Source::ModelBusWrite) {
            self.mode_writes.push((row.instructions,row.interval));
        }
        if code && row.source==Source::CpuFetch {
            self.fetches.push(FetchRecord {row:Row {source:row.source,address:row.address,physical:row.physical,size:row.size,
                core:row.core.map(Core::try_from).transpose()?,pc:row.pc,instructions:row.instructions.map(Count),tick:row.tick.map(Count),
                value:row.value,before:None},own:row.own,order:row.order,attempt:row.attempt});
        }
        if code && row.source==Source::CpuFetch {self.fetched=true;}
        if code && self.fetched && matches!(row.source,Source::CpuWrite|Source::HostWrite|Source::ModelBusWrite) {
            return Err("shared dispatcher code rewritten after first fetch".into());
        }
        let relevant=code || [(0x63300,0x63480),(0xf3300,0xf3440),(0x4f114,0x4f118),(0xdf0d4,0xdf0d8),
            (0x7cad9c,0x7cb000),(0x96bd20,0x96bd28),(0x61c008,0x61c00c)].into_iter()
            .any(|(low,high)|u64::from(row.physical.0)<high && low<end);
        if relevant {
            self.rows.push(Row {source:row.source,address:row.address,physical:row.physical,size:row.size,
                core:row.core.map(Core::try_from).transpose()?,pc:row.pc,instructions:row.instructions.map(Count),
                tick:row.tick.map(Count),value:row.value,before:None});
        }
        Ok(())
    }
}
#[derive(Serialize,PartialEq)]
pub(crate) struct Checked {projection:startup_dispatch_check::SharedProjection}
impl Capture {
    pub(crate) fn verify(&self,rows:&Rows,trace_bytes:&[u8],code:&[u8],callers:&[(usize,u32,u32)],facts:&[CallFact],owned:bool,endpoint:u64)->Result<Checked,String>{
        let trace:Vec<startup_dispatch_check::SharedHit>=startup_dispatch_check::shared_hits(trace_bytes,code)?;
        startup_dispatch_check::shared_fetches(&rows.fetches,&trace,endpoint)?;
        let projection=startup_dispatch_check::shared(&self.invocations,&self.publications,&rows.rows,trace_bytes,callers)?;
        self.verify_mode(rows,&trace,code)?;
        for invocation in &self.invocations {
            let sp=invocation.entry.registers.sp.0;
            let returned=invocation.returned.as_ref().ok_or("dispatcher return absent")?;
            for call in &invocation.calls {
                let finish=call.returned.as_ref().ok_or("dispatcher service return absent")?;
                let live=if owned {
                    let mut selected=facts.iter().filter(|fact|fact.phase.matches(invocation.phase)&&fact.target==call.entry.pc && fact.continuation==call.entry.registers.lr);
                    let fact=selected.next().ok_or("dispatcher call proof absent")?;
                    if selected.any(|other|other!=fact) || call.entry.registers.sp.0!=sp.wrapping_add_signed(fact.stack_from_dispatcher_entry) {
                        return Err("dispatcher call proof differs from actual frame".into());
                    }
                    fact.caller_live_words.iter().map(|offset|Guest(sp.wrapping_add_signed(*offset))).collect::<Vec<_>>()
                }else{[-16i32,-12,-8,-4].into_iter().map(|offset|Guest(sp.wrapping_add_signed(offset))).collect()};
                let protected=self.live_words.iter().find(|protected|protected.core==call.entry.core && protected.start.0==call.entry.instructions.0
                    && protected.end.as_ref().is_some_and(|end|end.0==finish.instructions.0) && protected.words==live).ok_or("actual dispatcher live-word subobservation absent")?;
                if protected.preimage.len()!=live.len(){return Err("dispatcher live-word preimages absent".into());}
                for (sample,address) in protected.preimage.iter().zip(&live){validate_view(sample)?;if sample.address!=*address || sample.words.len()!=1{return Err("dispatcher live-word address differs".into());}}
                for row in &rows.rows {
                    let end=u64::from(row.physical.0)+row.size;
                    if !matches!(row.source,Source::CpuWrite|Source::HostWrite|Source::ModelBusWrite)
                        || !live.iter().any(|word|u64::from(word.0&!0x40000000)<end && u64::from(row.physical.0)<u64::from(word.0&!0x40000000)+4){continue;}
                    let at=row.instructions.as_ref().ok_or("caller write clock absent")?.0;
                    if call.entry.instructions.0<at && at<=finish.instructions.0{return Err("writer reached actual dispatcher caller word".into());}
                }
            }
            for row in &rows.rows {
                let end=u64::from(row.physical.0)+row.size;let physical=u64::from(sp&!0x40000000);
                if matches!(row.source,Source::CpuRead|Source::CpuWrite) && row.pc.is_some_and(|pc|(START..END).contains(&pc.0))
                    && u64::from(row.physical.0)<physical-16 && physical-20<end {return Err("dispatcher accessed alias-visible D+24".into());}
                if !matches!(row.source,Source::CpuWrite|Source::HostWrite|Source::ModelBusWrite)
                    || end<=physical-16 || physical<=u64::from(row.physical.0){continue;}
                let at=row.instructions.as_ref().ok_or("saved dispatcher word write clock absent")?.0;
                if invocation.entry.instructions.0<at && at<=returned.instructions.0
                    && (row.source!=Source::CpuWrite || row.core!=Some(invocation.entry.core) || !row.pc.is_some_and(|pc|(START..END).contains(&pc.0))) {
                    return Err("foreign writer reached saved dispatcher frame".into());
                }
            }
        }
        Ok(Checked {projection})
    }
}

impl Capture {
    fn verify_mode(&self,rows:&Rows,trace:&[startup_dispatch_check::SharedHit],code:&[u8])->Result<(),String>{
        let invocation=self.invocations.iter().find(|invocation|invocation.entry.core==Core::Primary&&invocation.phase==Phase::Install)
            .ok_or("mode getter outer0 invocation absent")?;
        let call=invocation.calls.iter().find(|call|call.service==Service::Mode).ok_or("mode getter call absent")?;
        let returned=call.returned.as_ref().ok_or("mode getter return absent")?;
        let installer=invocation.calls.iter().find(|call|call.service==Service::Install).ok_or("mode getter installation successor absent")?;
        if self.mode_reads.len()!=1 {return Err("mode getter committed read sample absent or repeated".into());}
        let sample=&self.mode_reads[0];
        validate_boundary(&sample.boundary)?;validate_view(&sample.cell)?;
        startup_dispatch_check::shared_boundary(&sample.boundary,trace)?;
        if sample.boundary.core!=Core::Primary||sample.boundary.pc!=Guest(0x657194)
            ||sample.cell.address!=Guest(0x61c008)||sample.cell.physical!=Guest(0x61c008)||sample.cell.words!=[0]
            ||sample.cell.other_words.len()!=1||sample.cell.physical_words.len()!=1
            ||call.entry.instructions.0>sample.boundary.instructions.0||sample.boundary.instructions.0>=returned.instructions.0
            // The getter pushes {r11,lr} and calls 0x6570f4 for the cell base before the load, so LR is that call's continuation.
            ||sample.boundary.registers.lr!=Guest(0x657190)||Some(sample.boundary.registers.sp.0)!=call.entry.registers.sp.0.checked_sub(8)
            ||sample.boundary.registers.r[3]!=0x61c000
            ||returned.registers.r[0]!=0||installer.entry.registers.r[0]!=0 {
            return Err("mode getter committed sample is not the source cell in its enclosing call".into());
        }
        let site=call.entry.registers.lr.0.checked_sub(4).ok_or("mode getter callsite underflows")?;
        let offset=site.checked_sub(START).ok_or("mode getter callsite precedes dispatcher")? as usize;
        let instruction=u32::from_le_bytes(code.get(offset..offset+4).ok_or("mode getter callsite exceeds dispatcher")?.try_into().map_err(|_|"mode getter callsite word")?);
        let displacement=(((instruction&0xffffff)<<8) as i32>>6) as u32;
        if instruction&0xff000000!=0xeb000000||site.wrapping_add(8).wrapping_add(displacement)!=0x657184 {
            return Err("mode getter lacks a direct source-bound dispatcher call".into());
        }
        let within=|clock:Option<u64>|clock.is_some_and(|clock|invocation.entry.instructions.0<clock&&clock<=installer.entry.instructions.0);
        let source_fetch=|pc:u32|->Result<FetchStamp,String>{
            let mut candidates=rows.fetches.iter().filter(|fetch|!fetch.attempt&&fetch.row.core==Some(Core::Primary)
                &&fetch.row.pc==Some(Guest(pc))&&within(fetch.row.instructions.as_ref().map(|clock|clock.0)));
            let first=candidates.next().ok_or("mode getter source call/continuation executed fetch absent")?;
            if candidates.any(|other|other.own!=first.own||other.row.tick.as_ref().map(|tick|tick.0)!=first.row.tick.as_ref().map(|tick|tick.0)) {
                return Err("mode getter source fetch is ambiguous".into());
            }
            Ok(FetchStamp {source:Source::CpuFetch,address:first.row.address,physical:Some(first.row.physical),size:first.row.size,
                value:first.row.value,before:None,byte_order:first.order,core:Some(0),pc:first.row.pc,
                instructions:first.row.instructions.as_ref().map(|clock|clock.0),tick:first.row.tick.as_ref().map(|tick|tick.0),
                core_instructions:first.own.ok_or("mode getter source fetch own-core count absent")?})
        };
        let getter_fetch=|pc:u32|->Result<&FetchStamp,String>{
            let mut candidates=rows.getter_fetches.iter().filter(|fetch|fetch.core==Some(0)&&fetch.pc==Some(Guest(pc))&&within(fetch.instructions));
            let first=candidates.next().ok_or("mode getter executed entry/load/move/return fetch absent")?;
            if candidates.any(|other|other!=first){return Err("mode getter executed fetch is ambiguous".into());}
            if first.source!=Source::CpuFetch||first.address!=Guest(pc)||first.physical!=Some(Guest(pc))||first.size!=4
                ||first.byte_order!=Some(ByteOrder::Little){return Err("mode getter executed fetch identity differs".into());}
            Ok(first)
        };
        let called=source_fetch(site)?;let entered=getter_fetch(0x657184)?;let loaded=getter_fetch(0x657194)?;
        let moved=getter_fetch(0x657198)?;let completed=getter_fetch(0x65719c)?;let continued=source_fetch(site+4)?;
        if called.core_instructions.checked_add(1)!=Some(entered.core_instructions)||entered.core_instructions>=loaded.core_instructions
            ||loaded.core_instructions.checked_add(1)!=Some(moved.core_instructions)
            ||moved.core_instructions.checked_add(1)!=Some(completed.core_instructions)
            ||completed.core_instructions.checked_add(1)!=Some(continued.core_instructions) {
            return Err("mode getter own-core call/read/return sequence differs".into());
        }
        let mut reads=rows.mode_reads.iter().filter(|(row,_)|row.core==Some(Core::Primary)
            &&within(row.instructions.as_ref().map(|clock|clock.0)));
        let (read,attribution)=reads.next().ok_or("mode getter actual CPU read absent")?;
        if reads.next().is_some(){return Err("mode getter actual CPU read repeated".into());}
        let attribution=attribution.as_ref().ok_or("mode getter actual fetch attribution absent")?;
        if read.address!=Guest(0x61c008)||read.physical!=Guest(0x61c008)||read.size!=4||read.value!=Some(0)
            ||read.pc!=Some(Guest(0x657194))||read.instructions.as_ref().map(|clock|clock.0)!=loaded.instructions
            ||read.tick.as_ref().map(|tick|tick.0)!=loaded.tick||attribution.own!=loaded.core_instructions
            ||attribution.before!=Some([0;4])||attribution.byte_order!=Some(ByteOrder::Little)||attribution.latest.as_ref()!=Some(loaded)
            ||!loaded.instructions.is_some_and(|clock|sample.boundary.instructions.0<clock&&clock<=returned.instructions.0) {
            return Err("mode getter actual read and executed source fetch attribution differ".into());
        }
        for (clock,interval) in &rows.mode_writes {
            if clock.is_some_and(|clock|invocation.entry.instructions.0<clock&&clock<=installer.entry.instructions.0)
                ||(clock.is_none()&&interval[0]<installer.entry.instructions.0&&invocation.entry.instructions.0<interval[1]) {
                return Err("mode getter source cell overwritten during setup".into());
            }
        }
        Ok(())
    }
}

pub(crate) struct RetainedCall {
    pub(crate) core:usize,pub(crate) sp:Guest,pub(crate) start:u64,pub(crate) end:u64,pub(crate) live:Vec<Guest>,
}
impl Capture {
    pub(crate) fn retained_calls(&self)->Vec<RetainedCall>{
        self.invocations.iter().flat_map(|invocation|invocation.calls.iter()).filter_map(|call|{
            let end=call.returned.as_ref()?.instructions.0;
            let live=self.live_words.iter().find(|protected|protected.core==call.entry.core&&protected.start.0==call.entry.instructions.0
                &&protected.end.as_ref().is_some_and(|at|at.0==end))?.words.clone();
            Some(RetainedCall {core:usize::from(call.entry.core),sp:call.entry.registers.sp,start:call.entry.instructions.0,end,live})
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{CallFact,Capture};
    fn source()->Vec<u8> {
        let mut image=vec![0;0x69cf84-0x61c400];
        image[0x69cdf8-0x61c400..0x69cdfc-0x61c400].copy_from_slice(&0xebfee8e1u32.to_le_bytes());
        image
    }
    fn call(words:&str)->CallFact {
        serde_json::from_str(&format!(r#"{{"phase":"Setup","instruction":6934008,"continuation":6934012,"target":6648196,"stack_from_dispatcher_entry":-64,"caller_live_words":{words}}}"#)).expect("source-bound call input")
    }
    #[test]
    fn call_admission_binds_native_branch_and_live_word_offsets() {
        let image=source();
        assert!(call("[-64,-60,-56,-52,-16,-12,-8,-4]").validate(&image).is_ok());
        assert!(call("[-68,-16,-12,-8,-4]").validate(&image).is_err());
        assert!(call("[-64,-20,-16,-12,-8,-4]").validate(&image).is_err());
        let mut foreign=image;foreign[0x69cdf8-0x61c400..0x69cdfc-0x61c400].copy_from_slice(&0xeb000000u32.to_le_bytes());
        assert!(call("[-64,-60,-56,-52,-16,-12,-8,-4]").validate(&foreign).is_err());
    }
    fn tail(continuation:u32)->CallFact {
        serde_json::from_str(&format!(r#"{{"phase":"Root","instruction":6934192,"continuation":{continuation},"target":7495128,"stack_from_dispatcher_entry":-64,"caller_live_words":[-64,-60,-56,-52,-44,-40,-36,-32,-28,-24,-16,-12,-8,-4]}}"#)).expect("source-bound tail call input")
    }
    fn helper_image(caller_word:u32)->Vec<u8> {
        let mut image=source();
        image[0x69ceb0-0x61c400..0x69ceb4-0x61c400].copy_from_slice(&0xea0223c8u32.to_le_bytes());
        image[0x69cf60-0x61c400..0x69cf64-0x61c400].copy_from_slice(&caller_word.to_le_bytes());
        image
    }
    #[test]
    fn tail_call_admission_binds_the_helper_entry_link() {
        assert!(tail(0x69cf64).validate(&helper_image(0xebffffab)).is_ok());
        assert!(tail(0x69cf68).validate(&helper_image(0xebffffab)).is_err());
        assert!(tail(0x69ceb4).validate(&helper_image(0xebffffab)).is_err());
        assert!(tail(0x69cf64).validate(&helper_image(0xebffffd5)).is_err());
        assert!(tail(0x69cf64).validate(&helper_image(0xeaffffab)).is_err());
    }
    fn captured()->Capture {
        serde_json::from_str(r#"{"invocations":[],"publications":[],"mode_reads":[],"live_words":[{"core":0,"start":10,"end":20,"words":[1074140144],"preimage":[]}]}"#).expect("live caller subobservation")
    }
    fn write(address:u32,clock:u64)->Vec<u8> {
        format!(r#"{{"records":[{{"source":"cpu-write","address":{address},"physical":{},"size":4,"core":0,"pc":272436,"instructions":{clock},"tick":1,"value":0,"before":[0,0,0,0]}}]}}"#,address&!0x40000000).into_bytes()
    }
    #[test]
    fn callee_push_below_live_caller_is_admitted_but_caller_overwrite_is_refused() {
        let capture=captured();
        assert!(capture.check_writes(&write(1074140136,15)).is_ok());
        assert!(capture.check_writes(&write(1074140144,15)).is_err());
        assert!(capture.check_writes(&write(1074140144,21)).is_ok());
    }
}
