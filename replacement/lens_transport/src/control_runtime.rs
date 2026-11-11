//! Task 99 clients, coherent publications and completion ownership.

use core::cell::UnsafeCell;
use core::future::poll_fn;
use core::sync::atomic::{AtomicU32,Ordering};
use core::task::{Poll,Waker};
use crate::control_actor::ControlActor;
use crate::control_device::PeriodicIndex;
use crate::control_types::*;

pub trait ControlSender:Sync {
    fn post(&self,message:ControlMessage)->Result<(),ControlError>;
}

struct IrqMask {
    #[cfg(target_arch="arm")]
    saved:u32,
}
impl IrqMask {
    fn enter()->Self {
        #[cfg(target_arch="arm")]
        {
            let saved:u32;
            unsafe { core::arch::asm!("mrs {saved}, cpsr", "cpsid if",saved=out(reg)saved,options(nostack,preserves_flags)); }
            Self {saved}
        }
        #[cfg(not(target_arch="arm"))]
        Self {}
    }
}
impl Drop for IrqMask {
    fn drop(&mut self) {
        #[cfg(target_arch="arm")]
        unsafe {
            if self.saved&0x40==0 { core::arch::asm!("cpsie f",options(nostack,preserves_flags)); }
            if self.saved&0x80==0 { core::arch::asm!("cpsie i",options(nostack,preserves_flags)); }
        }
    }
}
pub(crate) struct Shared<T> { locked:AtomicU32,value:UnsafeCell<T> }
// Interrupts stay masked until the word lock is released; references do not escape with().
unsafe impl<T:Send> Sync for Shared<T> {}
impl<T> Shared<T> {
    pub(crate) const fn new(value:T)->Self { Self {locked:AtomicU32::new(0),value:UnsafeCell::new(value)} }
    pub(crate) fn with<R>(&self,operation:impl FnOnce(&mut T)->R)->R {
        let irq=IrqMask::enter();
        while self.locked.compare_exchange_weak(0,1,Ordering::Acquire,Ordering::Relaxed).is_err() { core::hint::spin_loop(); }
        struct Unlock<'a>(&'a AtomicU32);
        impl Drop for Unlock<'_> { fn drop(&mut self) { self.0.store(0,Ordering::Release); } }
        let unlock=Unlock(&self.locked);
        let result=operation(unsafe { &mut *self.value.get() });
        drop(unlock);
        drop(irq);
        result
    }
}

#[derive(Clone,Copy)]
struct CompletionValue { status:CompletionStatus,bytes:[u8;9],length:usize }
struct Completion { token:CallbackToken,value:Option<CompletionValue>,waiter:Option<Waker> }
struct CompletionState { slots:[Option<Completion>;16],next:u32 }
pub struct CompletionMailbox(Shared<CompletionState>);
impl CompletionMailbox {
    pub const fn new()->Self { Self(Shared::new(CompletionState {slots:[const {None};16],next:0x8000_0000})) }
    fn reserve(&self)->Result<Registration<'_>,ControlError> {
        self.0.with(|state| {
            let index=state.slots.iter().position(Option::is_none).ok_or(ControlError::Busy)?;
            let next=state.next.checked_add(1).ok_or(ControlError::Busy)?;
            state.next=next;
            let token=CallbackToken(next);
            state.slots[index]=Some(Completion {token,value:None,waiter:None});
            Ok(Registration {mailbox:self,index,token})
        })
    }
    pub fn complete(&self,token:CallbackToken,status:CompletionStatus,data:&[u8])->bool {
        if data.len()>9 { return false; }
        let Some(waiter)=self.0.with(|state| {
            let completion=state.slots.iter_mut().flatten().find(|completion|completion.token==token)?;
            if completion.value.is_some() { return None; }
            let mut bytes=[0;9];
            bytes[..data.len()].copy_from_slice(data);
            completion.value=Some(CompletionValue {status,bytes,length:data.len()});
            Some(completion.waiter.take())
        }) else { return false; };
        if let Some(waiter)=waiter { waiter.wake(); }
        true
    }
    pub fn owns(&self,token:CallbackToken)->bool {
        self.0.with(|state| state.slots.iter().flatten().any(|completion|completion.token==token))
    }
}
struct Registration<'a> {mailbox:&'a CompletionMailbox,index:usize,token:CallbackToken}
impl Registration<'_> {
    fn try_complete(&self)->Result<Option<CompletionValue>,ControlError> {
        self.mailbox.0.with(|state| {
            let completion=state.slots[self.index].as_ref().ok_or(ControlError::InvalidPayload)?;
            if completion.token!=self.token {return Err(ControlError::InvalidPayload);}
            Ok(completion.value)
        })
    }
    async fn wait(&self)->CompletionValue {
        poll_fn(|cx| {
            let (ready,registered)=self.mailbox.0.with(|state| {
                let completion=state.slots[self.index].as_ref().expect("registered completion");
                (completion.value,completion.waiter.as_ref().is_some_and(|waiter|waiter.will_wake(cx.waker())))
            });
            if let Some(value)=ready { return Poll::Ready(value); }
            if registered { return Poll::Pending; }
            let waiter=cx.waker().clone();
            let (ready,old)=self.mailbox.0.with(|state| {
                let completion=state.slots[self.index].as_mut().expect("registered completion");
                (completion.value,completion.waiter.replace(waiter))
            });
            drop(old);
            match ready {Some(value)=>Poll::Ready(value),None=>Poll::Pending}
        }).await
    }
}
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        let completion=self.mailbox.0.with(|state| state.slots[self.index].take());
        drop(completion);
    }
}

pub struct SnapshotRequest<'a> {registration:Registration<'a>,kind:ObserverKind}
impl SnapshotRequest<'_> {
    fn decode(&self,completion:CompletionValue)->Result<SnapshotResult,ControlError> {
        let data=match self.kind {
            ObserverKind::TwoBytes=>SnapshotData::TwoBytes(completion.bytes[..2].try_into().map_err(|_|ControlError::InvalidPayload)?),
            ObserverKind::ThreeBytes=>SnapshotData::ThreeBytes(completion.bytes[..3].try_into().map_err(|_|ControlError::InvalidPayload)?),
            ObserverKind::NineBytes=>SnapshotData::NineBytes(completion.bytes),
        };
        let expected=match self.kind {ObserverKind::TwoBytes=>2,ObserverKind::ThreeBytes=>3,ObserverKind::NineBytes=>9};
        if completion.status==CompletionStatus::SUCCESS && completion.length!=expected {return Err(ControlError::InvalidPayload);}
        Ok(SnapshotResult {status:completion.status,data})
    }
    pub fn try_complete(&self)->Result<Option<SnapshotResult>,ControlError> {
        self.registration.try_complete()?.map(|completion|self.decode(completion)).transpose()
    }
    async fn wait(self)->Result<SnapshotResult,ControlError> {
        self.decode(self.registration.wait().await)
    }
}
struct Published {
    available:bool,status:CompletionStatus,mode:ModeStatus,model:LensModelIdentifier,
    descriptor:[u8;128],auxiliary_size:u16,descriptor_generation:u32,
    cache:[u8;60],cache_generation:u32,special_encoding:bool,
}
pub struct ControlPublications(Shared<Published>);
impl ControlPublications {
    pub const fn new()->Self { Self(Shared::new(Published {available:false,status:CompletionStatus::REJECTED,mode:ModeStatus::Unavailable,model:LensModelIdentifier(0),descriptor:[0;128],auxiliary_size:0,descriptor_generation:0,cache:[0;60],cache_generation:0,special_encoding:false})) }
    /// The stock shared block at 0x0095e1d0 carries available, connected, status and public mode.
    #[cfg(feature = "native-actor")]
    pub fn native_snapshot(&self,actor:&ControlActor<'_>)->[u32;4] {
        self.0.with(|state| [u32::from(state.available),u32::from(actor.connected()),state.status.0 as u32,state.mode as u32])
    }
    pub fn publish(&self,actor:&ControlActor<'_>) {
        self.0.with(|state| {
            state.available=actor.available();state.status=actor.status();state.mode=actor.mode_status();
            state.model=LensModelIdentifier(actor.device().capabilities().identifier);
            state.special_encoding=actor.device().capabilities().special;
            if state.cache_generation!=actor.cache_generation() {
                state.cache.copy_from_slice(actor.device().cache());
                state.cache_generation=actor.cache_generation();
            }
            if state.descriptor_generation!=actor.descriptor_generation() {
                state.descriptor.copy_from_slice(actor.device().calibration());
                let auxiliary=actor.device().secondary_calibration();
                state.auxiliary_size=u16::from_le_bytes([auxiliary[0x42],auxiliary[0x43]]);
                state.descriptor_generation=actor.descriptor_generation();
            }
        });
    }
}

pub struct Task99Client<'a> {
    sender:&'a dyn ControlSender,completions:&'a CompletionMailbox,
    publications:&'a ControlPublications,readiness:&'a SharedReadiness,
}
impl<'a> Task99Client<'a> {
    pub fn new(sender:&'a dyn ControlSender,completions:&'a CompletionMailbox,publications:&'a ControlPublications,readiness:&'a SharedReadiness)->Self { Self {sender,completions,publications,readiness} }
    async fn request(&self,opcode:ControlOpcode)->Result<CompletionStatus,ControlError> {
        let registration=self.completions.reserve()?;
        self.sender.post(ControlMessage {opcode,payload:ControlPayload::Callback(Some(registration.token))})?;
        Ok(registration.wait().await.status)
    }
    pub async fn synchronize(&self)->Result<CompletionStatus,ControlError> { self.request(ControlOpcode::Synchronize).await }
    pub async fn teardown(&self)->Result<CompletionStatus,ControlError> { self.request(ControlOpcode::Teardown).await }
    pub async fn resume(&self)->Result<CompletionStatus,ControlError> { self.request(ControlOpcode::Resume).await }
    pub async fn suspend(&self)->Result<CompletionStatus,ControlError> { self.request(ControlOpcode::Suspend).await }
    pub async fn setup(&self,polling:bool,mode:SetupMode,owner:ControlOwner)->Result<CompletionStatus,ControlError> {
        let registration=self.completions.reserve()?;
        self.sender.post(ControlMessage {opcode:ControlOpcode::Setup,payload:ControlPayload::Setup(SetupRequest {completion:Some(registration.token),polling,mode,owner})})?;
        Ok(registration.wait().await.status)
    }
    pub fn release(&self)->Result<(),ControlError> { self.sender.post(ControlMessage {opcode:ControlOpcode::StartPolling,payload:ControlPayload::Callback(None)}) }
    pub fn finish(&self)->Result<(),ControlError> { self.sender.post(ControlMessage {opcode:ControlOpcode::Finish,payload:ControlPayload::Empty}) }
    pub fn available(&self)->bool { self.publications.0.with(|state|state.available) }
    pub fn readiness(&self)->bool { self.readiness.is_ready() }
    pub fn mode_status(&self)->ModeStatus { self.publications.0.with(|state|state.mode) }
    pub fn model_identifier(&self)->LensModelIdentifier { self.publications.0.with(|state|state.model) }
    pub fn snapshot_descriptor(&self,destination:&mut[u8;128])->CompletionStatus {
        self.publications.0.with(|state| {destination.copy_from_slice(&state.descriptor);state.status})
    }
    pub fn auxiliary_size(&self)->u16 { self.publications.0.with(|state|state.auxiliary_size) }
    pub fn cached_two_bytes(&self)->[u8;2] { self.publications.0.with(|state|[state.cache[56],state.cache[57]]) }
    pub fn actuator_snapshot_bytes(&self)->[u8;3] {self.publications.0.with(|state|[state.cache[44],state.cache[45],state.cache[46]])}
    pub fn cached_mode_byte(&self)->u8 { self.publications.0.with(|state|state.cache[5]) }
    pub fn special_encoding(&self)->bool { self.publications.0.with(|state|state.special_encoding) }
    pub fn cached_metadata(&self,destination:&mut[u8;60]) {self.publications.0.with(|state|destination.copy_from_slice(&state.cache));}
    pub fn firmware_version(&self)->FirmwareVersion {
        self.publications.0.with(|state| {
            let first=state.descriptor[0x5e];let second=state.descriptor[0x5f];
            FirmwareVersion(u16::from(first>>4)*1000+u16::from(first&15)*100+u16::from(second>>4)*10+u16::from(second&15))
        })
    }
    pub fn request_snapshot(&self,kind:ObserverKind,callback:CallbackToken)->Result<(),ControlError> {
        self.sender.post(ControlMessage {opcode:ControlOpcode::Poll,payload:ControlPayload::Snapshot {kind,callback}})
    }
    pub fn begin_snapshot(&self,kind:ObserverKind)->Result<SnapshotRequest<'a>,ControlError> {
        let registration=self.completions.reserve()?;
        self.request_snapshot(kind,registration.token)?;
        Ok(SnapshotRequest {registration,kind})
    }
    pub async fn snapshot(&self,kind:ObserverKind)->Result<SnapshotResult,ControlError> {
        self.begin_snapshot(kind)?.wait().await
    }
    pub fn reset_lens(&self)->Result<(),ControlError> { self.sender.post(ControlMessage {opcode:ControlOpcode::Poll,payload:ControlPayload::ResetLens}) }
    pub fn configure_periodic(&self,index:PeriodicIndex,ticks:TickCount,modes:PeriodicModes,callback:Option<CallbackToken>)->Result<(),ControlError> {
        self.sender.post(ControlMessage {opcode:ControlOpcode::Poll,payload:ControlPayload::ConfigurePeriodic {index,ticks,modes,callback}})
    }
    pub fn cancel_periodic(&self,index:PeriodicIndex)->Result<(),ControlError> { self.sender.post(ControlMessage {opcode:ControlOpcode::Poll,payload:ControlPayload::CancelPeriodic(index)}) }
}

pub fn dispatch_task99(actor:&mut ControlActor<'_>,message:ControlMessage,io:&mut impl ControlIo,queue:&mut impl crate::control_device::ControlQueue,publications:&ControlPublications,outbox:&mut crate::control_publication::ControlOutbox)->Result<(),ControlError> {
    let result=actor.dispatch(message,&mut crate::control_publication::PublicationIo {io,outbox},queue);
    publications.publish(actor);
    outbox.flush(io);
    result
}

#[cfg(test)]
#[path="control_runtime_tests.rs"]
mod tests;
