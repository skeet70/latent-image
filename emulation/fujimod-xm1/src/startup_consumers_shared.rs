use super::*;

#[derive(Clone)]
pub(crate) struct Sites {
    pub(super) owned: bool, completion: [u32; 3], pub(super) poll: [u32; 3],
    pub(super) poll_destination: u8, initializer: [u32; 3],
}
impl Sites {
    pub(crate) fn retained() -> Self {
        Self { owned:false,completion:[0x40a08,0x40a0c,0x40a10],poll:[0xd0a44,0xd0a48,0xd0a4c],
            poll_destination:3,initializer:[0xd0aac,0xd0430,0xd0ab0] }
    }
    pub(crate) fn owned(completion: [u32;3], poll: [u32;3], destination:u8, initializer:[u32;3]) -> Result<Self,String> {
        require(destination<13,"poll destination is not a captured general register")?;
        Ok(Self {owned:true,completion,poll,poll_destination:destination,initializer})
    }
    pub(super) fn kind(&self,pc:u32)->Option<ProbeKind> {
        for (sites,kinds) in [(self.completion,[ProbeKind::CompletionStore,ProbeKind::CompletionBeforeDsb,ProbeKind::CompletionAfterDsb]),
            (self.poll,[ProbeKind::PeerPoll,ProbeKind::PeerPostload,ProbeKind::PeerBranch]),
            (self.initializer,[ProbeKind::InitializerCall,ProbeKind::InitializerEntry,ProbeKind::InitializerReturn])] {
            if let Some(index)=sites.iter().position(|site|*site==pc) {return Some(kinds[index]);}
        }
        global_kind(pc).filter(|kind| !matches!(kind,ProbeKind::CompletionStore|ProbeKind::CompletionBeforeDsb|ProbeKind::CompletionAfterDsb
            |ProbeKind::PeerPoll|ProbeKind::PeerPostload|ProbeKind::PeerBranch|ProbeKind::InitializerCall|ProbeKind::InitializerEntry|ProbeKind::InitializerReturn))
    }
    fn points(&self)->Vec<u32> {
        GLOBAL_PCS.iter().copied().filter(|pc|self.kind(*pc).is_some())
            .chain(self.completion).chain(self.poll).chain(self.initializer).collect()
    }
}
#[derive(Serialize)]
struct Endpoint { instructions:u64, counts:[u64;2], trace_index:usize }
#[derive(Serialize)]
#[serde(rename_all="kebab-case")]
enum Termination { FirstFamiliesAndSecondaryRoot1 }
#[derive(Serialize)]
pub(crate) struct Recorder {
    begin:Option<Endpoint>, end:Option<Endpoint>, termination:Option<Termination>,
    #[serde(rename="window",serialize_with="serialize_window")]
    runtime:Runtime,
    #[serde(skip)]
    initialized:bool,
    #[serde(skip)]
    backings:Vec<WatchRange>,
}
fn serialize_window<S:serde::Serializer>(runtime:&Runtime,serializer:S)->Result<S::Ok,S::Error> {
    runtime.receipt.serialize(serializer)
}
pub(crate) struct Input<'a> {
    pub(crate) stage:&'a Path, pub(crate) owned:bool, pub(crate) run:&'a [u8], pub(crate) update:&'a [u8],
    pub(crate) rom:&'a [u8], pub(crate) checkpoint:&'a [u8], pub(crate) observer:&'a [u8],
    pub(crate) sites:Sites, pub(crate) carrier_sizes:[usize;3],
}
pub(crate) enum Event<'a> { Stop(usize), Watch(&'a [u8]) }
fn compact_classes(classes:Vec<WatchRange>)->Vec<WatchRange> {
    let mut groups:Vec<(WatchKinds,Vec<WatchRange>)>=Vec::new();
    for range in classes {
        if let Some((_,group))=groups.iter_mut().find(|(kinds,_)|*kinds==range.kinds){group.push(range);}
        else {groups.push((range.kinds,vec![range]));}
    }
    groups.into_iter().flat_map(|(_,ranges)|merge_watch_ranges(ranges)).collect()
}
impl Recorder {
    pub(crate) fn new(machine:&Machine,input:Input<'_>)->Result<Self,String> {
        let canonical=read(&input.stage.join("canonical.dat"))?;
        let contract=read(&input.stage.join("contract.json"))?;
        let components=[(ComponentKind::Dispatcher,"dispatcher"),(ComponentKind::Installer,"installer")].into_iter().map(|(kind,name)|{
            Ok(ComponentBinding {kind,image:digest(&read(&input.stage.join(format!("{name}.bin")))?),
                symbols:digest(&read(&input.stage.join(format!("{name}.symbols")))?),contract:digest(&contract)})
        }).collect::<Result<Vec<_>,String>>()?;
        let identity=Identity {run:digest(input.run),update:digest(input.update),canonical_dat:digest(&canonical),
            image4:digest(&read(&input.stage.join("canonical-image4.bin"))?),
            loader_a:digest(&read(&input.stage.join("canonical-loader-a.bin"))?),
            loader_b:digest(&read(&input.stage.join("canonical-loader-b.bin"))?),boot_rom:digest(input.rom),
            observer:digest(input.observer),joined_contract:digest(&contract),components};
        let receipt=Receipt {schema:1,role:if input.owned {Role::Joined}else{Role::Retained},outcome:Outcome::NonObservation,
            error:None,machine_failure:None,identity,checkpoint_sha256:digest(input.checkpoint),checkpoint_reused:false,instructions:0,
            limits:LIMITS,probes:Vec::new(),invocations:Vec::new(),resources:Vec::new(),segments:Vec::new(),
            census:FAMILIES.into_iter().flat_map(|family|[Core::Primary,Core::Secondary].into_iter().map(move|core|
                Census {family,core,entries:0,first_probe:None})).collect(),fetch_attempts:Vec::new(),payloads:Vec::new()};
        Ok(Self {begin:None,end:None,termination:None,initialized:false,backings:backed_ranges(&machine.document)?,
            runtime:Runtime {receipt,maps:run::memory_maps(&machine.document)?,frames:Vec::new(),calls:Vec::new(),global_returns:BTreeMap::new(),
                globals_seen:BTreeSet::new(),installed:false,outer_returned:false,
                carriers:[0x61c400,0x40000,0xd0000].into_iter().zip(input.carrier_sizes).map(|(base,size)|
                    WatchRange {start:base,end:base+size as u64,kinds:WatchKinds::WRITES}).collect(),
                peer_poll:None,peer_ready:false,retained_records:0,captured_bytes:0,captured_views:0,shared_sites:Some(input.sites),shared_owners:None}})
    }
    pub(crate) fn initial_sites(&self)->BTreeMap<u32,String> {
        let globals=self.runtime.shared_sites.as_ref().map(Sites::points).unwrap_or_default();
        FAMILIES.into_iter().map(|family|(family.entry(),format!("consumer-global-{:x}",family.entry())))
            .chain(globals.into_iter().map(|pc|(pc,format!("consumer-global-{pc:x}")))).collect()
    }
    pub(crate) fn attach_owners(&mut self,owners:StartupSites){self.runtime.shared_owners=Some(owners);}
    pub(crate) fn close_sites(&self,machine:&mut Machine)->Result<(),String>{
        self.runtime.shared_owners.as_ref().ok_or("shared observation owners absent")?.close(machine,Component::Consumer)
    }
    pub(crate) fn is_outer(machine:&Machine)->Result<bool,String> {
        let state=context(machine,Core::Primary)?;
        Ok(state.pc==Address(0x69cdf8) && state.registers[0]==Word(0))
    }
    pub(crate) fn reject_early_family(&self,machine:&Machine,start:usize)->Result<(),String> {
        for hit in &machine.traces.hits[start..] {
            let pc=u32::from_str_radix(hit.address.strip_prefix("0x").ok_or("trace PC prefix absent")?,16).map_err(|error|error.to_string())?;
            require(!FAMILIES.iter().any(|family|family.entry()==pc),"consumer entered before shared subobservation")?;
        }
        Ok(())
    }
    pub(crate) fn begin(&mut self,machine:&Machine,image4:&[u8],loader_a:&[u8],loader_b:&[u8])->Result<(),String> {
        require(self.begin.is_none() && self.end.is_none() && Self::is_outer(machine)?,"consumer subobservation start differs")?;
        let state=context(machine,Core::Primary)?;
        require(state.sp==Address(0x40063450),"outer natural service stack differs")?;
        verify_loaded(machine,&self.runtime.maps,&state,image4,loader_a,loader_b)?;
        self.begin=Some(Endpoint {instructions:machine.instructions(),counts:counts(machine)?,trace_index:machine.traces.hits.len()});
        Ok(())
    }
    pub(crate) fn active(&self)->bool {self.begin.is_some() && self.end.is_none()}
    pub(crate) fn complete(&self)->bool {self.end.is_some()}
    pub(crate) fn segment_count(&self)->usize {self.runtime.receipt.segments.len()}
    pub(crate) fn step<F>(&mut self,machine:&mut Machine,extra:Vec<WatchRange>,budget:u64,output:&Path,mut observe:F)->Result<(),String>
        where F:FnMut(&mut Machine,Event<'_>)->Result<(),String> {
        require(self.active(),"consumer step outside its subobservation")?;
        require(machine.instructions()<LIMITS.instructions && budget!=0,"consumer instruction cap reached")?;
        require(self.runtime.receipt.segments.len()<LIMITS.segments as usize,"aggregate segment bound reached")?;
        let available=LIMITS.records.checked_sub(self.runtime.retained_records).ok_or("aggregate record count overflow")?;
        require(available!=0,"aggregate memory record bound reached")?;
        let start_contexts=contexts(machine)?; let start_counts=counts(machine)?;
        let mut plan=self.runtime.watch_ranges(machine,&self.backings,&start_contexts)?;
        plan.classes.extend(extra);
        plan.classes=compact_classes(plan.classes);
        require(plan.classes.len()<=256,"shared classified range cap reached")?;
        plan.fetch_ranges=merge_watch_ranges(plan.classes.iter().filter(|range|range.kinds.contains(WatchKinds::CPU_FETCH))
            .map(|range|WatchRange {start:range.start,end:range.end,kinds:WatchKinds::CPU_FETCH}).collect())
            .into_iter().map(|range|FetchRange([range.start,range.end])).collect();
        let step=match plan.fetch_scope {FetchScope::Complete=>LIMITS.segment_instructions,FetchScope::Scoped=>LIMITS.scoped_segment_instructions.0};
        let probe_begin=ProbeSequence(self.runtime.receipt.probes.len() as u64);
        let attribution=FetchAttribution::new(CpuDataAttribution::ReadsAndWrites,self.backings.iter().map(|range|
            WatchRange {start:range.start,end:range.end,kinds:WatchKinds::CPU_FETCH}).collect())?;
        let mut watch=machine.session.board.memory_watch_begin_with_fetch_attribution(plan.classes,attribution,
            LIMITS.watch_records.min(available) as usize,self.runtime.maps.clone(),ViewPolicy::Fixed)?;
        let mut stops=Vec::new();
        let body=(||->Result<(),String>{
            if !self.initialized {
                self.runtime.initial_resources(machine,&start_contexts[0])?;
                for (address,size) in [(0x61c008,4),(0x7caf3c,4),(0x40000,32),(0xd0000,32)] {
                    self.runtime.admit(machine,&start_contexts[0],ResourceKind::Header,Lifetime::Observation,None,Address(address),size)?;
                }
                self.runtime.handle(machine,&start_contexts[0])?; self.initialized=true;
            }
            let trace_start=machine.traces.hits.len();
            let result=machine.run(step.min(budget).min(LIMITS.instructions-machine.instructions()));
            let hits=&machine.traces.hits[trace_start..];
            let mut cores=BTreeSet::new();
            for hit in hits {
                let core=Core::try_from(hit.core)?;
                let pc=u32::from_str_radix(hit.address.strip_prefix("0x").ok_or("trace PC prefix absent")?,16).map_err(|error|error.to_string())?;
                stops.push(StopPoint {core,pc:Address(pc),instructions:hit.instructions}); cores.insert(core);
            }
            observe(machine,Event::Stop(trace_start))?;
            for core in cores {let state=context(machine,core)?;self.runtime.handle(machine,&state)?;}
            require(matches!(result,RunOutcome::Stopped|RunOutcome::Completed),"shared consumer machine/capture failure")?;
            if self.runtime.finished() {
                let terminal=context(machine,Core::Primary)?;
                for owner in self.runtime.frames.iter().map(|frame|Some(frame.id)).chain(std::iter::once(None)).collect::<Vec<_>>() {
                    self.runtime.close(machine,&terminal,owner,Lifetime::Allocation)?;
                }
                self.runtime.close(machine,&terminal,None,Lifetime::Observation)?;
                self.end=Some(Endpoint {instructions:machine.instructions(),counts:counts(machine)?,trace_index:machine.traces.hits.len()});
                self.termination=Some(Termination::FirstFamiliesAndSecondaryRoot1);
            }
            Ok(())
        })();
        let end_contexts=contexts(machine);let end_counts=counts(machine);
        let raw=serde_json::to_vec(&watch.records.borrow().state()).map_err(|error|error.to_string());
        let end_error=machine.session.board.memory_watch_end(&mut watch).err();
        let raw=raw?;
        let guarded=observe(machine,Event::Watch(&raw));
        let state=match serde_json::from_slice::<WatchState>(&raw) {Ok(state)=>state,Err(error)=>{
            write(&output.join(format!("unparsed-shared-watch-{}.json",self.runtime.receipt.segments.len())),&raw)?;
            return Err(format!("watch serialization: {error}; teardown: {end_error:?}"));}};
        if end_contexts.is_err() || end_counts.is_err() {write(&output.join("unclosed-shared-watch.json"),&raw)?;}
        let segment=Segment {id:SegmentId(self.runtime.receipt.segments.len() as u64),start_counts,end_counts:end_counts?,
            start_contexts,end_contexts:end_contexts?,fetch_scope:plan.fetch_scope,fetch_ranges:plan.fetch_ranges,armed_sites:plan.armed_sites,
            probe_range:[probe_begin,ProbeSequence(self.runtime.receipt.probes.len() as u64)],watch:state,end_error,stops};
        self.runtime.retained_records=self.runtime.retained_records.checked_add(segment.watch.records.len() as u64).ok_or("aggregate record count overflow")?;
        let refusal=segment.watch.violation.clone().or_else(||segment.end_error.clone())
            .or_else(||(!segment.watch.enabled).then(||"shared consumer memory watch disabled".into()));
        let fetches=if refusal.is_none() && body.is_ok() {record_fetches(&self.runtime.maps,&mut self.runtime.receipt,&segment)}else{Ok(())};
        self.runtime.receipt.segments.push(segment);
        body?; guarded?; if let Some(error)=refusal {return Err(error);} fetches?;
        require(self.complete() || machine.instructions()<LIMITS.instructions,"consumer terminal deadline expired")
    }
    pub(crate) fn finalize(&mut self,machine:&Machine,error:Option<String>){
        self.runtime.receipt.instructions=self.end.as_ref().map_or(machine.instructions(),|end|end.instructions);
        self.runtime.receipt.error=error.clone();self.runtime.receipt.machine_failure=machine.failure.clone();
        self.runtime.receipt.outcome=if error.is_some() {Outcome::Refused}else if self.complete(){Outcome::Observed}else{Outcome::NonObservation};
        self.runtime.absent();
    }
}

#[cfg(test)]
mod tests {
    use super::Sites;
    #[test]
    fn poll_destination_outside_captured_registers_is_refused() {
        assert!(Sites::owned([1,2,3],[4,5,6],13,[7,8,9]).is_err());
    }
}
