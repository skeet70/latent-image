//! Shared external observation policy: route and dispatcher site ownership replayed from admitted source, raw trace contexts and committed words.
use super::*;
use owner::shared_dispatch::shared::{CLEAR,CLEAR_LIFETIME};
use serde::de::Deserializer;
use std::rc::Rc;

const DISPATCHER:(u32,u32)=(0x69cdf8,0x69cf84);

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord,Deserialize,Serialize)]
#[serde(tag="kind",rename_all="kebab-case")]
pub(super) enum Owner {RoutePermanent,DispatcherPermanent,RouteCall {origin:u64},RouteScheduledTarget {origin:u64},DispatcherReturn {origin:u64}}
impl Owner {
    fn route(self)->bool {matches!(self,Self::RoutePermanent|Self::RouteCall {..}|Self::RouteScheduledTarget {..})}
    fn route_dynamic(self)->bool {matches!(self,Self::RouteCall {..}|Self::RouteScheduledTarget {..})}
    fn dispatcher(self)->bool {matches!(self,Self::DispatcherPermanent|Self::DispatcherReturn {..})}
}
#[derive(Clone,Debug,PartialEq,Eq,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnedSite {pub(super) address:u32,pub(super) owners:Vec<Owner>}
/// Owned sites ascending by address, shared between equal consecutive snapshots.
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Snapshot(Rc<[OwnedSite]>);
impl Serialize for Snapshot {fn serialize<S:serde::Serializer>(&self,serializer:S)->Result<S::Ok,S::Error>{self.0.serialize(serializer)}}
impl<'de> Deserialize<'de> for Snapshot {fn deserialize<D:Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{Ok(Self(Vec::<OwnedSite>::deserialize(deserializer)?.into()))}}
impl Snapshot {
    pub(super) fn sites(&self)->&[OwnedSite] {&self.0}
    pub(super) fn new(sites:Vec<OwnedSite>)->Self {Self(sites.into())}
}
#[derive(Clone,Debug,PartialEq,Eq,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundaryPolicy {pub(super) trace_index:u64,pub(super) core:Core,pub(super) pc:u32,pub(super) instructions:u64,pub(super) acquire:Vec<OwnedSite>,pub(super) release:Vec<OwnedSite>}
/// One consumer segment's slice [trace_begin,trace_end) of the boundaries; its owner sets are the replay of `initial` and the boundaries before each cut.
#[derive(Clone,Debug,PartialEq,Eq,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SegmentPolicy {pub(super) id:u64,pub(super) trace_begin:u64,pub(super) trace_end:u64}
#[derive(Clone,Copy,Deserialize,Serialize)]
#[serde(rename_all="kebab-case")]
enum PolicyScope {SharedExternalObservationPolicy}
#[derive(Clone,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Policy {
    schema:u32,scope:PolicyScope,capture_sha256:Digest,trace_sha256:Digest,contract_sha256:Digest,watch_inputs:Vec<FileBinding>,
    begin_trace_index:u64,pub(super) end_trace_index:u64,pub(super) initial:Snapshot,pub(super) boundaries:Vec<BoundaryPolicy>,pub(super) segments:Vec<SegmentPolicy>,
}
impl Policy {
    pub(super) fn encode(&self)->Result<Vec<u8>,String>{serde_json::to_vec(self).map_err(|error|error.to_string())}
    pub(super) fn decode(bytes:&[u8])->Result<Self,String>{serde_json::from_slice(bytes).map_err(|error|error.to_string())}
}
pub(super) fn grouped(pairs:&BTreeSet<(u32,Owner)>)->Vec<OwnedSite>{
    let mut sites:Vec<OwnedSite>=Vec::new();
    for &(address,owner) in pairs {
        match sites.last_mut() {Some(site) if site.address==address=>site.owners.push(owner),_=>sites.push(OwnedSite {address,owners:vec![owner]})}
    }
    sites
}
struct RouteCall {core:Core,target:u32,continuation:u32,sp:u32,entry:Option<usize>,returned:Option<usize>}
#[derive(Default)]
struct Replay {
    owners:BTreeMap<u32,BTreeSet<Owner>>,added:BTreeSet<(u32,Owner)>,removed:BTreeSet<(u32,Owner)>,snapshot:Option<Snapshot>,
    calls:Vec<RouteCall>,active:[Option<u32>;2],entries:usize,publications:usize,clear_revoked:bool,
}
impl Replay {
    fn acquire(&mut self,address:u32,owner:Owner)->Result<(),String>{
        require(address%4==0,"external observation site is not A32 aligned")?;
        if self.owners.entry(address).or_default().insert(owner) {
            self.snapshot=None;
            if !self.removed.remove(&(address,owner)) {self.added.insert((address,owner));}
        }
        Ok(())
    }
    fn withdraw(&mut self,address:u32,select:fn(Owner)->bool){
        let Some(held)=self.owners.get_mut(&address) else {return};
        let gone:Vec<Owner>=held.iter().copied().filter(|owner|select(*owner)).collect();
        for owner in gone {
            held.remove(&owner);self.snapshot=None;
            if !self.added.remove(&(address,owner)) {self.removed.insert((address,owner));}
        }
        if held.is_empty() {self.owners.remove(&address);}
    }
    fn snapshot(&mut self)->Snapshot {
        self.snapshot.get_or_insert_with(||Snapshot::new(self.owners.iter().map(|(address,owners)|
            OwnedSite {address:*address,owners:owners.iter().copied().collect()}).collect())).clone()
    }
    fn route(&mut self,event:&Event,static_calls:&BTreeMap<u32,&owner::StaticCall>,catalog:&Catalog)->Result<(),String>{
        let (id,pc,core,registers)=(event.id.0,event.pc.0,event.core,&event.registers);let origin=id as u64;
        for call in self.calls.iter_mut().filter(|call|call.core==core) {
            if call.entry.is_none()&&call.target==pc&&call.sp==registers.sp.0&&call.continuation==registers.lr.0 {call.entry=Some(id);}
            if call.entry.is_some()&&call.returned.is_none()&&call.continuation==pc&&call.sp==registers.sp.0 {call.returned=Some(id);}
        }
        let consumed=self.calls.iter().any(|call|call.entry==Some(id)||call.returned==Some(id));
        let needed=self.calls.iter().any(|call|(call.entry.is_none()&&call.target==pc)||(call.entry.is_some()&&call.returned.is_none()&&call.continuation==pc));
        if consumed&&!needed {self.withdraw(pc,Owner::route_dynamic);}
        let spec=match static_calls.get(&pc) {
            Some(call)=>Some((*call).clone()),
            None if matches!(pc,0xd9858c|0x69d2e0|0x73be5c|0x73beac)=>owner::decode_call(event.pc,catalog.word(event.pc)?.1)?,
            None=>None,
        };
        if let Some(spec)=spec {
            let target=match spec.target {owner::CallTarget::Direct(target)=>target.0,owner::CallTarget::Register(register)=>registers.r[usize::from(register.0)]};
            self.acquire(target,Owner::RouteCall {origin})?;self.acquire(spec.continuation.0,Owner::RouteCall {origin})?;
            self.calls.push(RouteCall {core,target,continuation:spec.continuation.0,sp:registers.sp.0,entry:None,returned:None});
        }
        if pc==0x725dd8 {
            self.acquire(registers.lr.0,Owner::RouteCall {origin})?;
            self.calls.push(RouteCall {core,target:pc,continuation:registers.lr.0,sp:registers.sp.0,entry:Some(id),returned:None});
        }
        if matches!(pc,0x436e8|0xd36e8) {
            let context=registers.r[1].checked_add(0x18).ok_or("saved scheduler context address overflows")?;
            require(at(event,context)?==registers.sp.0,"scheduler transfer stack differs from its committed saved context")?;
            self.acquire(at(event,context.checked_add(4).ok_or("saved scheduler target address overflows")?)?,Owner::RouteScheduledTarget {origin})?;
            for address in if pc==0x436e8 {[0x43688,0x436e4,0x436e8]}else{[0xd3688,0xd36e4,0xd36e8]} {self.withdraw(address,Owner::route);}
        }
        if matches!(pc,0x43758|0xd3758) {
            require(at(event,registers.r[1].checked_add(8).ok_or("task creation-record link overflows")?)?==registers.r[2],"committed task creation-record link differs")?;
            self.acquire(at(event,registers.r[2].checked_add(8).ok_or("task target address overflows")?)?,Owner::RouteScheduledTarget {origin})?;
            self.withdraw(pc,Owner::route);
        }
        Ok(())
    }
    fn dispatcher(&mut self,event:&Event)->Result<(),String>{
        let (pc,slot,lr)=(event.pc.0,usize::from(event.core),event.registers.lr.0);let origin=event.id.0 as u64;
        if pc==DISPATCHER.0 {
            require(self.active[slot].is_none(),"dispatcher reentry overlaps a live invocation")?;
            self.active[slot]=Some(lr);self.entries+=1;self.acquire(lr,Owner::DispatcherReturn {origin})?;
        }else if let Some(back)=self.active[slot] {
            if Dispatcher::service_entry(pc)&&(DISPATCHER.0..DISPATCHER.1).contains(&lr) {self.acquire(lr,Owner::DispatcherReturn {origin})?;}
            if pc==back {self.active[slot]=None;}
        }
        if matches!(pc,0x40c80|0xd0c80) {self.publications+=1;}
        if !self.clear_revoked&&event.instructions.0>=CLEAR_LIFETIME {
            require(self.entries==6&&self.active==[None;2]&&self.publications==2,"dispatcher initial frame interval is incomplete at the Clear lifetime bound")?;
            self.withdraw(CLEAR,Owner::dispatcher);self.clear_revoked=true;
        }
        Ok(())
    }
}
/// Derives the external policy of an admitted shared capture whose raw trace, events, source graph and consumer endpoints are already checked.
pub(super) fn derive(capture:&Capture,receipt:&Digest,catalog:&Catalog,program:&Program,calls:&[owner::StaticCall])->Result<Policy,String>{
    let events=&capture.route.events;let window=&capture.consumer.window;
    let begin=capture.consumer.begin.as_ref().ok_or("shared consumer begin absent")?.trace_index;
    let end=capture.consumer.end.as_ref().ok_or("shared consumer end absent")?.trace_index;
    require(0<begin&&begin<=end&&end<=events.len(),"external policy trace interval differs")?;
    let mut parts=Vec::with_capacity(window.segments.len());let mut cursor=begin;
    for segment in &window.segments {
        let first=cursor;cursor=cursor.checked_add(segment.stops.len()).filter(|next|*next<=end).ok_or("consumer stops exceed the consumer trace interval")?;
        for (stop,event) in segment.stops.iter().zip(&events[first..cursor]) {
            require(stop.core==event.core&&stop.pc==event.pc&&stop.instructions==event.instructions.0,"consumer stop differs from its global trace occurrence")?;
        }
        parts.push((segment.id,first,cursor));
    }
    require(cursor==end,"consumer stops do not partition the consumer trace interval")?;
    let static_calls:BTreeMap<u32,&owner::StaticCall>=calls.iter().map(|call|(call.site.0,call)).collect();
    let mut replay=Replay::default();
    for address in owner::sites(calls,program).into_keys() {replay.owners.entry(address).or_default().insert(Owner::RoutePermanent);}
    for address in Dispatcher::sites().into_keys() {replay.owners.entry(address).or_default().insert(Owner::DispatcherPermanent);}
    let mut initial=None;let mut boundaries=Vec::with_capacity(end-begin);
    for (index,event) in events[..end].iter().enumerate() {
        require(event.id.0==index,"external policy event order differs")?;
        if index==begin {initial=Some(replay.snapshot());}
        replay.added.clear();replay.removed.clear();
        replay.route(event,&static_calls,catalog)?;replay.dispatcher(event)?;
        if index>=begin {
            boundaries.push(BoundaryPolicy {trace_index:index as u64,core:event.core,pc:event.pc.0,instructions:event.instructions.0,
                acquire:grouped(&replay.added),release:grouped(&replay.removed)});
        }
    }
    let segments=parts.into_iter().map(|(id,first,next)|SegmentPolicy {id,trace_begin:first as u64,trace_end:next as u64}).collect();
    let contracts:Vec<&FileBinding>=capture.artifacts.files.iter().filter(|file|file.path.as_os_str()=="staged-contract.json").collect();
    let [contract]=contracts.as_slice() else {return Err("staged contract binding census differs".into())};
    let mut watch_inputs:Vec<FileBinding>=capture.epochs.iter().filter_map(|epoch|match &epoch.evidence {Evidence::Watch {file,..}=>Some(file.clone()),Evidence::ConsumerSegment {..}=>None}).collect();
    watch_inputs.sort_by(|left,right|left.path.as_os_str().cmp(right.path.as_os_str()));
    require(watch_inputs.windows(2).all(|pair|pair[0].path!=pair[1].path),"standalone watch sidecar is bound twice")?;
    Ok(Policy {schema:2,scope:PolicyScope::SharedExternalObservationPolicy,capture_sha256:receipt.clone(),trace_sha256:capture.trace.sha256.clone(),
        contract_sha256:contract.sha256.clone(),watch_inputs,begin_trace_index:begin as u64,end_trace_index:end as u64,
        initial:initial.ok_or("external policy initial snapshot absent")?,boundaries,segments})
}
/// Refuses a supplied policy file unless it is byte-identical to the independent derivation.
pub(super) fn admit(supplied:&[u8],derived:&Policy)->Result<(),String>{
    let expected=derived.encode()?;
    if supplied==expected.as_slice() {return Ok(());}
    let policy=Policy::decode(supplied).map_err(|error|format!("supplied observation policy does not decode: {error}"))?;
    if policy.initial!=derived.initial {return Err("supplied observation policy initial ownership differs from the independent derivation".into());}
    if policy.boundaries.len()!=derived.boundaries.len() {return Err("supplied observation policy omits or adds a trace boundary".into());}
    if let Some((boundary,_))=policy.boundaries.iter().zip(&derived.boundaries).find(|(left,right)|left!=right) {
        return Err(format!("supplied observation policy boundary at trace index {} differs from the independent derivation",boundary.trace_index));
    }
    if let Some((segment,_))=policy.segments.iter().zip(&derived.segments).find(|(left,right)|left!=right) {
        return Err(format!("supplied observation policy segment {} differs from the independent derivation",segment.id));
    }
    if policy.segments.len()!=derived.segments.len() {return Err("supplied observation policy segment census differs".into());}
    Err("supplied observation policy bindings or encoding differ from the independent derivation".into())
}
