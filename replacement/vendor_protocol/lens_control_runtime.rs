//! Task 99 requests and publications on the canonical queue 98 runtime.

use core::cell::RefCell;
use xm1_lens_transport::control_runtime::ControlSender;
use xm1_lens_transport::control_device::{ControlQueue,DeviceCallback,QueueFailure,QueuePacket,QueuePage};
use xm1_lens_transport::control_types::{CalibrationPage,CompletionStatus,ControlError,ControlMessage,ControlOpcode,ControlPayload,ControlResponse,ResponseKind};
use xm1_lens_transport::frame::{CommandWord,PageId,PayloadLength};
use xm1_lens_transport::wire::{InterruptController,Mmio,Platform};
use xm1_power_services::shutdown::RequestId;
use crate::lens_queue_runtime::{LensQueueRuntime,PageDisposition,Publication,RuntimeError,Status};
use crate::lens_queue_runtime::packets::{CallbackId,ControlOperation,ControlPacket,QueueRequest,ReadRequest,WriteRequest,SourceAddress};
use crate::lens_queue_runtime::replies::ReplyData;

const CALLBACK_BASE:u32=0x2000_0000;
fn callback_id(callback:DeviceCallback)->CallbackId { CallbackId(CALLBACK_BASE|callback.token()) }
fn callback(id:CallbackId)->Option<DeviceCallback> {
    if id.0&0xffff_f000!=CALLBACK_BASE { return None; }
    Some(match id.0&0xfff {
        1..=27=>DeviceCallback::Response(ResponseKind::try_from(id.0&0xfff).ok()?),
        0x100=>DeviceCallback::Initialized,0x101=>DeviceCallback::Observer,
        0x102=>DeviceCallback::DetectionOff,0x103=>DeviceCallback::DetectionOn,
        0x104=>DeviceCallback::Poll,0x105=>DeviceCallback::Teardown,
        0x106=>DeviceCallback::ModeResume,0x107=>DeviceCallback::ModeSuspend,
        0x108=>DeviceCallback::Configuration,0x109=>DeviceCallback::CapabilityPage,
        0x10a=>DeviceCallback::Snapshot(xm1_lens_transport::control_types::ObserverKind::TwoBytes),
        0x10b=>DeviceCallback::Snapshot(xm1_lens_transport::control_types::ObserverKind::ThreeBytes),
        0x10c=>DeviceCallback::Snapshot(xm1_lens_transport::control_types::ObserverKind::NineBytes),
        _=>return None,
    })
}
fn failure<E>(error:RuntimeError<E>)->QueueFailure {
    match error { RuntimeError::Backpressure|RuntimeError::QueueFull=>QueueFailure::Full,
        RuntimeError::InvalidState|RuntimeError::PendingPage|RuntimeError::LateCompletion=>QueueFailure::InvalidState,
        _=>QueueFailure::Transport }
}
fn status(status:Status)->CompletionStatus { CompletionStatus(status.driver_word() as i32) }

pub struct LensControlQueue<'a,M:Mmio,P:Platform,I:InterruptController> { runtime:&'a RefCell<LensQueueRuntime<M,P,I>> }
impl<'a,M:Mmio,P:Platform,I:InterruptController> LensControlQueue<'a,M,P,I> {
    pub fn new(runtime:&'a RefCell<LensQueueRuntime<M,P,I>>)->Self { Self { runtime } }
    fn request(&mut self,request:QueueRequest)->Result<(),QueueFailure> { self.runtime.borrow_mut().submit(request,None).map_err(failure) }
    fn command(&mut self,operation:RequestId,word:CommandWord,completion:DeviceCallback)->Result<(),QueueFailure> {
        let operation=ControlOperation::parse(operation).ok_or(QueueFailure::InvalidState)?;
        let packet=ControlPacket::new(operation,callback_id(completion),&[word]).map_err(|_|QueueFailure::InvalidState)?;
        self.request(QueueRequest::Control(packet))
    }
    fn read_page(&mut self,page:QueuePage,completion:DeviceCallback)->Result<(),QueueFailure> {
        let length=PayloadLength::new(128).map_err(|_|QueueFailure::InvalidState)?;
        self.request(QueueRequest::Read(ReadRequest {page:PageId::new(page.0),length,checksum:true,marker:true,callback:callback_id(completion)}))
    }
    pub fn publication(&mut self,publication:Publication,sender:&dyn ControlSender)->Result<bool,ControlError> {
        let id=match publication {
            Publication::Lifecycle {callback}|Publication::Command {callback,..}|Publication::Write {callback,..}|Publication::Read {callback,..}|Publication::Observer {callback,..}=>callback,
        };
        let Some(completion)=callback(id) else { return Ok(false); };
        match publication {
            Publication::Lifecycle {..}=>sender.post(ControlMessage {opcode:ControlOpcode::Poll,payload:ControlPayload::DeviceCompletion {callback:completion,status:CompletionStatus::SUCCESS,data:[0;16]}})?,
            Publication::Command {status:result,data,..}=>{
                let bytes=reply_bytes(data);
                sender.post(ControlMessage {opcode:ControlOpcode::Poll,payload:ControlPayload::DeviceCompletion {callback:completion,status:status(result),data:bytes}})?;
            }
            Publication::Observer {status:result,flags,..}=>{
                let mut bytes=[0;16];
                for (destination,flag) in bytes.iter_mut().zip(flags) { *destination=u8::from(flag); }
                sender.post(ControlMessage {opcode:ControlOpcode::Poll,payload:ControlPayload::DeviceCompletion {callback:completion,status:status(result),data:bytes}})?;
            }
            Publication::Read {status:result,page,..}=>{
                let copied={
                    let runtime=self.runtime.borrow();
                    let bytes=runtime.read_data();
                    if page.get()!=0 || bytes.len()<128 { None } else { let mut copied=[0;128];copied.copy_from_slice(&bytes[..128]);Some(CalibrationPage(copied)) }
                };
                let kind=match completion { DeviceCallback::Response(kind)=>kind,DeviceCallback::CapabilityPage=>ResponseKind::Reply24,_=>return Err(ControlError::InvalidPayload) };
                let mut data=[0;16];data[..2].copy_from_slice(&page.get().to_le_bytes());
                let response=ControlResponse {kind,status:status(result),data,page:copied};
                sender.post(ControlMessage {opcode:ControlOpcode::Response,payload:ControlPayload::Response(response)})?;
                let disposition=if result!=Status::Complete {PageDisposition::Stop}
                    else if page.get()==if kind==ResponseKind::Reply24 {0xff00}else{0xffff} {PageDisposition::Accept}
                    else {PageDisposition::Reject};
                let mut runtime=self.runtime.borrow_mut();
                if runtime.awaiting_page_acceptance() { runtime.complete_page(disposition).map_err(|_|ControlError::Queue)?; }
            }
            Publication::Write {status:result,next,..}=>{
                if completion!=DeviceCallback::CapabilityPage {return Err(ControlError::InvalidPayload);}
                let mut data=[0;16];data[..2].copy_from_slice(&next.get().to_le_bytes());
                sender.post(ControlMessage {opcode:ControlOpcode::Response,payload:ControlPayload::Response(ControlResponse {kind:ResponseKind::Reply24,status:status(result),data,page:None})})?;
                let disposition=if result!=Status::Complete {PageDisposition::Stop}
                    else if next.get()==0xff00 {PageDisposition::Accept}else{PageDisposition::Reject};
                let mut runtime=self.runtime.borrow_mut();
                if runtime.awaiting_page_acceptance() {runtime.complete_page(disposition).map_err(|_|ControlError::Queue)?;}
            }
        }
        Ok(true)
    }
}
impl<M:Mmio,P:Platform,I:InterruptController> ControlQueue for LensControlQueue<'_,M,P,I> {
    fn initialize(&mut self,completion:DeviceCallback,observer:DeviceCallback)->Result<(),QueueFailure> { self.request(QueueRequest::Initialize {callback:callback_id(completion),observer:callback_id(observer)}) }
    fn submit(&mut self,packet:QueuePacket)->Result<(),QueueFailure> {
        let request=QueueRequest::parse(RequestId(packet.operation as u16),packet.bytes()).map_err(|_|QueueFailure::InvalidState)?;
        let QueueRequest::Control(mut request)=request else { return Err(QueueFailure::InvalidState); };
        request.callback=callback_id(packet.callback);
        self.request(QueueRequest::Control(request))
    }
    fn stop(&mut self,completion:DeviceCallback)->Result<(),QueueFailure> { self.request(QueueRequest::Stop {callback:callback_id(completion)}) }
    fn write_configuration(&mut self,page:QueuePage,configuration:&[u8;128],completion:DeviceCallback)->Result<(),QueueFailure> {
        let length=PayloadLength::new(128).map_err(|_|QueueFailure::InvalidState)?;
        let request=WriteRequest {page:PageId::new(page.0),source:SourceAddress(configuration.as_ptr() as usize as u32),length,checksum:true,callback:callback_id(completion)};
        self.runtime.borrow_mut().submit(QueueRequest::Write(request),Some(configuration)).map_err(failure)
    }
    fn accept_page(&mut self,accept:bool,completion:DeviceCallback)->Result<(),QueueFailure> { self.read_page(QueuePage(if accept {0xff00}else{0xffff}),completion) }
    fn resume(&mut self,completion:DeviceCallback)->Result<(),QueueFailure> { self.command(RequestId(0x7205),CommandWord::new(0x480),completion) }
    fn suspend(&mut self,completion:DeviceCallback)->Result<(),QueueFailure> { self.command(RequestId(0x7206),CommandWord::new(0x4c0),completion) }
    fn poll(&mut self,completion:DeviceCallback)->Result<(),QueueFailure> { self.command(RequestId(0x720c),CommandWord::new(0x800),completion) }
    fn read_two(&mut self,completion:DeviceCallback)->Result<(),QueueFailure> { self.command(RequestId(0x7211),CommandWord::new(0x980),completion) }
    fn finish(&mut self,model:xm1_lens_transport::control_types::LensModelIdentifier)->Result<(),QueueFailure> {
        self.runtime.borrow_mut().finish_configuration(crate::lens_queue_runtime::configuration::LensModelIdentifier(model.0)).map_err(failure)
    }
}

fn reply_bytes(data:ReplyData)->[u8;16] {
    let mut bytes=[0;16];
    match data {
        ReplyData::StatusOnly=>(),
        ReplyData::ThreeFlags(flags)=>for (destination,flag) in bytes.iter_mut().zip(flags) { *destination=u8::from(flag); },
        ReplyData::Capabilities {flags,mode}=>{
            for (destination,flag) in bytes[..5].iter_mut().zip(flags[..5].iter()) { *destination=u8::from(*flag); }
            bytes[5]=mode;
            for (destination,flag) in bytes[6..14].iter_mut().zip(flags[5..].iter()) { *destination=u8::from(*flag); }
        }
        ReplyData::Halfword(value)=>bytes[..2].copy_from_slice(&value.0.to_le_bytes()),
        ReplyData::FlaggedUnsigned {flag,value}=>{bytes[0]=u8::from(flag);bytes[2..4].copy_from_slice(&value.0.to_le_bytes());}
        ReplyData::FlaggedSigned {flag,value}=>{bytes[0]=u8::from(flag);bytes[2..4].copy_from_slice(&value.to_le_bytes());}
        ReplyData::FlaggedBytes {flag,upper,lower}=>{bytes[0]=u8::from(flag);bytes[1]=upper;bytes[2]=lower;}
        ReplyData::ByteFlag {byte,flag}=>{bytes[0]=byte;bytes[1]=u8::from(flag);}
        ReplyData::Flags(flags)=>for (destination,flag) in bytes.iter_mut().zip(flags.0) { *destination=u8::from(flag); },
        ReplyData::ObserverFlags(flags)=>for (destination,flag) in bytes.iter_mut().zip(flags) { *destination=u8::from(flag); },
        ReplyData::Channel {pages,format,present}=>{bytes[..2].copy_from_slice(&pages.to_le_bytes());bytes[2]=format;bytes[3]=u8::from(present);}
    }
    bytes
}

#[cfg(test)]
#[path="lens_control_runtime_tests.rs"]
mod tests;
