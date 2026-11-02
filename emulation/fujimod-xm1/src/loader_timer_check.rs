use super::{artifact,at,require,Access,Core,Digest,Event,Guest,Report};
use super::super::{Count,Row,Tick};
use super::super::continuation_view::EventId;
use fujimod_xm1::bridge::read;
use serde::{Deserialize,Serialize};
use std::{collections::BTreeMap,path::Path};

const RING:[u64;2]=[0xa0800,0xc0900];
const LATCH:u32=0xa0830;
const PCS:[u32;9]=[0x41ad4,0x41ad8,0x41adc,0x41ae0,0x41ae4,0x41ae8,0x41b78,0x41b7c,0x41b80];
const CODE:[u32;9]=[0xe3e02a6e,0xe5121ffb,0xe5122fff,0xe0622001,0xe5832040,0xebfffe9e,0xe5982040,0xe5832030,0xe1a00007];

#[derive(Deserialize)]
#[serde(rename_all="lowercase")]
enum Operation {Read,Write}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {cpu:Core,pc:Guest,address:Guest,size:u8,operation:Operation,value:u64,accepted:bool,tick:Tick}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {transactions:Vec<Transaction>,omitted:u64}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transactions {start:Count,end:Count,range:[u64;2],limit:usize,batch:Option<Batch>,error:Option<String>}

#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
#[serde(transparent)]
struct Occurrence(usize);
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
pub(super) struct SampleId {core:Core,occurrence:Occurrence,ring:Guest}
struct Sample {id:SampleId,value:u32,store:EventId,after:EventId}
#[derive(Clone,Copy)]
struct Live {sample:SampleId,value:u32}
struct ProvenWrite {sample:SampleId,core:Core,pc:Guest,address:Guest,value:u64}
#[derive(Default)]
pub(super) struct Admission {states:BTreeMap<(usize,Guest),SampleId>,writes:BTreeMap<(Count,Guest),ProvenWrite>}
impl Admission {
    pub(super) fn state(&self,event:EventId,address:Guest)->Option<SampleId>{self.states.get(&(event.0,Guest(address.0&!0x40000000))).copied()}
    pub(super) fn write(&self,write:&super::Publication)->Option<SampleId>{
        let proven=self.writes.get(&(write.clock?,write.physical))?;
        (write.source==Access::CpuWrite&&write.core==Some(proven.core)&&matches!(&write.writer,super::Writer::Retained(pc) if *pc==proven.pc)&&write.address==proven.address&&write.size==4&&write.value==Some(proven.value)).then_some(proven.sample)
    }
}
pub(super) fn relevant(row:&Row)->bool {
    let start=u64::from(row.physical.0);let end=start.saturating_add(row.size);
    matches!(row.source,Access::CpuWrite|Access::HostWrite|Access::ModelBusWrite)&&start<RING[1]&&RING[0]<end || row.source==Access::CpuFetch&&PCS.contains(&row.address.0)
}
fn ring(event:&Event,register:usize)->Result<Guest,String>{
    let address=event.registers.r[register].checked_add(0x40).ok_or("timer-ring provenance")?;
    let physical=address&!0x40000000;
    require(address&0x40000000!=0&&physical>=0xa0840&&u64::from(physical)+4<=RING[1]&&physical%16==0,"timer-ring provenance")?;
    Ok(Guest(address))
}
fn fetch<'a>(rows:&'a [Row],event:&Event)->Result<&'a Row,String>{
    let mut candidates=rows.iter().filter(|row|row.source==Access::CpuFetch&&row.address==event.pc&&row.pc==Some(event.pc)&&row.core==Some(event.core)&&row.instructions.is_some_and(|clock|Some(clock.0)==event.instructions.0.checked_add(1)));
    let row=candidates.next().ok_or("timer-read provenance")?;
    require(candidates.next().is_none()&&row.physical==event.pc&&row.size==4&&row.tick.is_some(),"timer-read provenance")?;Ok(row)
}
fn source(event:&Event,index:usize)->Result<(),String>{require(event.pc.0==PCS[index]&&event.code.values==[CODE[index]],"timer-source provenance")}
fn next<'a>(report:&'a Report,event:&Event,index:usize)->Result<&'a Event,String>{
    let following=report.events.iter().skip(event.id.0+1).find(|next|next.core==event.core).ok_or("timer-source provenance")?;
    source(following,index)?;require(event.instructions.0<following.instructions.0,"timer-source provenance")?;Ok(following)
}
fn actual<'a>(transactions:&'a [Transaction],fetch:&Row,event:&Event,address:Guest)->Result<&'a Transaction,String>{
    let mut matching=transactions.iter().filter(|read|read.pc==event.pc&&read.cpu==event.core&&Some(read.tick)==fetch.tick);
    let read=matching.next().ok_or("timer-read provenance")?;
    require(matching.next().is_none()&&read.address==address&&read.size==4&&matches!(read.operation,Operation::Read)&&read.accepted&&read.value<=u64::from(u32::MAX),"timer-read provenance")?;Ok(read)
}
fn write_matches(row:&Row,event:&Event,address:Guest,value:u32,rows:&[Row])->Result<(),String>{
    let executed=fetch(rows,event)?;
    require(row.source==Access::CpuWrite&&row.address==address&&row.physical.0==address.0&!0x40000000&&row.size==4&&row.pc==Some(event.pc)&&row.core==Some(event.core)&&row.instructions==executed.instructions&&row.tick==executed.tick&&row.value==Some(u64::from(value)),"timer-write provenance")
}

pub(super) fn admit(root:&Path,report:&Report,rows:&[Row])->Result<Admission,String>{
    let bytes=read(&artifact(root,&report.timer_transactions)?)?;
    require(Digest::of(&bytes).as_str()==report.timer_transactions.sha256.as_str(),"timer-transactions identity differs")?;
    let observed:Transactions=serde_json::from_slice(&bytes).map_err(|_|"timer-transactions schema differs")?;
    let startup=report.epochs.iter().find(|epoch|epoch.stage==super::super::WatchStage::Startup).ok_or("timer-transactions coverage differs")?;
    require(observed.start==startup.start&&observed.end==startup.end&&observed.range==[0xfff91000,0xfff91008]&&observed.limit==3_000_000&&observed.error.is_none(),"timer-transactions coverage differs")?;
    let mut batch=observed.batch.ok_or("timer-transactions coverage differs")?;
    require(batch.omitted==0&&batch.transactions.len()<=observed.limit,"timer-transactions coverage differs")?;
    require(batch.transactions.windows(2).all(|pair|pair[0].tick<=pair[1].tick)&&batch.transactions.iter().all(|read|u64::from(read.address.0)>=observed.range[0]&&u64::from(read.address.0)+u64::from(read.size)<=observed.range[1]&&read.size>0),"timer-transactions order differs")?;
    batch.transactions.retain(|read|matches!(read.pc.0,0x41ad8|0x41adc));
    let mut samples=Vec::new();let mut occurrences=[0usize;2];
    for begin in report.events.iter().filter(|event|event.pc.0==PCS[0]) {
        source(begin,0)?;let reload=next(report,begin,1)?;let count=next(report,reload,2)?;let subtract=next(report,count,3)?;let store=next(report,subtract,4)?;let after=next(report,store,5)?;
        require(startup.start.0<=begin.instructions.0&&after.instructions.0<=startup.end.0,"timer-source provenance")?;
        let slot=ring(begin,3)?;
        require([reload,count,subtract,store,after].iter().all(|event|event.registers.r[3]==begin.registers.r[3]),"timer-ring provenance")?;
        require(reload.registers.r[2]==0xfff91fff&&count.registers.r[2]==0xfff91fff,"timer-read provenance")?;
        let reload_fetch=fetch(rows,reload)?;let count_fetch=fetch(rows,count)?;
        let mut previous_tick=None;
        for boundary in [begin,reload,count,subtract,store,after] {
            let tick=fetch(rows,boundary)?.tick.ok_or("timer-read provenance")?;
            require(previous_tick.is_none_or(|previous|previous<=tick.0),"timer-read provenance")?;previous_tick=Some(tick.0);
        }
        let reload_read=actual(&batch.transactions,reload_fetch,reload,Guest(0xfff91004))?;let count_read=actual(&batch.transactions,count_fetch,count,Guest(0xfff91000))?;
        let reload_index=batch.transactions.iter().position(|read|std::ptr::eq(read,reload_read)).ok_or("timer-read provenance")?;
        let count_index=batch.transactions.iter().position(|read|std::ptr::eq(read,count_read)).ok_or("timer-read provenance")?;
        require(reload_index<count_index&&reload_read.tick.0<=count_read.tick.0,"timer-read provenance")?;
        let reload_value=reload_read.value as u32;let count_value=count_read.value as u32;let value=reload_value.wrapping_sub(count_value);
        require(count.registers.r[1]==reload_value&&subtract.registers.r[1]==reload_value&&subtract.registers.r[2]==count_value,"timer-read provenance")?;
        require(store.registers.r[1]==reload_value&&store.registers.r[2]==value&&after.registers.r[2]==value,"timer-ring provenance")?;
        require(at(after,slot.0)?==value,"timer-ring provenance")?;
        let core=usize::from(begin.core);samples.push(Sample {id:SampleId {core:begin.core,occurrence:Occurrence(occurrences[core]),ring:slot},value,store:store.id,after:after.id});occurrences[core]+=1;
    }
    require(!samples.is_empty()&&report.events.iter().filter(|event|event.pc.0==PCS[4]).count()==samples.len(),"timer-ring provenance")?;
    require(batch.transactions.len()==2*samples.len(),"timer-read provenance")?;
    let mut admission=Admission::default();let mut live:BTreeMap<u32,Live>=BTreeMap::new();let mut known:BTreeMap<u32,u32>=BTreeMap::new();let mut cursor=0;let mut pending_copy:Option<(EventId,Live,Guest)>=None;
    let mut copies=0;
    for event in &report.events {
        while let Some(row)=rows.get(cursor) {
            if row.source==Access::CpuFetch{cursor+=1;continue;}
            let clock=row.instructions.map(|clock|clock.0);
            if clock.is_some_and(|clock|clock>event.instructions.0){break;}
            let start=u64::from(row.physical.0);let end=start+row.size;
            live.retain(|address,_|u64::from(*address)+4<=start||end<=u64::from(*address));
            known.retain(|address,_|u64::from(*address)+4<=start||end<=u64::from(*address));
            if row.instructions.is_some()&&row.size==4&&(row.physical.0==LATCH||samples.iter().any(|sample|sample.id.ring.0&!0x40000000==row.physical.0)){
                if let Some(value)=row.value.and_then(|value|u32::try_from(value).ok()){known.insert(row.physical.0,value);}
            }
            if let Some(sample)=samples.iter().find(|sample|row.instructions.is_some_and(|clock|report.events[sample.store.0].instructions.0.checked_add(1)==Some(clock.0))&&row.pc==Some(Guest(PCS[4]))) {
                write_matches(row,&report.events[sample.store.0],sample.id.ring,sample.value,rows)?;
                require(admission.writes.insert((row.instructions.ok_or("timer-write provenance")?,row.physical),ProvenWrite {sample:sample.id,core:sample.id.core,pc:Guest(PCS[4]),address:sample.id.ring,value:u64::from(sample.value)}).is_none(),"timer-write provenance")?;
                live.insert(row.physical.0,Live {sample:sample.id,value:sample.value});
            }else if row.pc==Some(Guest(PCS[7])) {
                let (source,proven,address)=pending_copy.take().ok_or("timer-copy provenance")?;
                write_matches(row,&report.events[source.0],address,proven.value,rows)?;
                require(admission.writes.insert((row.instructions.ok_or("timer-write provenance")?,Guest(LATCH)),ProvenWrite {sample:proven.sample,core:report.events[source.0].core,pc:Guest(PCS[7]),address,value:u64::from(proven.value)}).is_none(),"timer-write provenance")?;live.insert(LATCH,proven);
                copies+=1;
            }else if row.pc==Some(Guest(PCS[4])) {return Err("timer-write provenance".into());}
            cursor+=1;
        }
        if event.instructions.0>startup.end.0{live.retain(|address,_|*address==LATCH);known.retain(|address,_|*address==LATCH);}
        if let Some(index)=PCS.iter().position(|pc|*pc==event.pc.0){source(event,index)?;}
        if event.pc.0==PCS[6] {
            let slot=ring(event,8)?;let proven=*live.get(&(slot.0&!0x40000000)).ok_or("timer-copy provenance")?;
            let copied=next(report,event,7)?;let after=next(report,copied,8)?;
            let loaded=fetch(rows,event)?.tick.ok_or("timer-read provenance")?;let stored=fetch(rows,copied)?.tick.ok_or("timer-read provenance")?;let finished=fetch(rows,after)?.tick.ok_or("timer-read provenance")?;
            require(loaded<=stored&&stored<=finished,"timer-copy provenance")?;
            require(copied.registers.r[8]==event.registers.r[8]&&after.registers.r[8]==event.registers.r[8]&&event.registers.r[3]==0x400a0800&&copied.registers.r[3]==event.registers.r[3]&&after.registers.r[3]==event.registers.r[3]&&copied.registers.r[2]==proven.value&&after.registers.r[2]==proven.value&&at(event,slot.0)?==proven.value&&at(after, 0xa0830)?==proven.value,"timer-copy provenance")?;
            require(pending_copy.is_none(),"timer-copy provenance")?;pending_copy=Some((copied.id,proven,Guest(0x400a0830)));
        }
        for view in &event.state {
            for (offset,value) in view.values.iter().enumerate() {
                let address=view.mapping.guest.0.checked_add(4*offset as u32).ok_or("timer-state provenance")?&!0x40000000;
                if let Some(expected)=known.get(&address){require(*value==*expected,"timer-state provenance")?;}
                if let Some(proven)=live.get(&address) {require(*value==proven.value,"timer-state provenance")?;admission.states.insert((event.id.0,Guest(address)),proven.sample);}
            }
        }
        if let Some(sample)=samples.iter().find(|sample|sample.after.0==event.id.0){require(live.get(&(sample.id.ring.0&!0x40000000)).is_some_and(|live|live.sample==sample.id),"timer-write provenance")?;}
    }
    require(pending_copy.is_none()&&copies>0&&copies==report.events.iter().filter(|event|event.pc.0==PCS[6]).count()&&copies==report.events.iter().filter(|event|event.pc.0==PCS[7]).count(),"timer-copy provenance")?;
    Ok(admission)
}
