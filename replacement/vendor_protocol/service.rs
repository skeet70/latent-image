use crate::{commands::{self,Backend,BackendStatus,ByteCount,CommandError,Commands,Dispatch,ImageDescriptor,PayloadRegion,PayloadSelector,PreparedPayload,ProviderError,ProviderRegions,TransferEffect,UploadCompletion,UploadMessages,UploadProvider},session::{ControlAction,ControlFrame,ControlOutcome,FrameError,ReplySource,Session,StatusDelivery,Tick},transport::{CompletionAction,CompletionStatus,PublishedAperture,PublishedRegion,ReceiveProgramming,RegionSpan,ResponseKind,SourceAction,TransportError,TransportState}};

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ServiceError {Frame(FrameError),Command(CommandError),Provider(ProviderError),Transport(TransportError),Completion(commands::CompletionError),Controller(crate::controller::ControllerError),Metadata(crate::metadata::MetadataError),NoReceive,WrongReceiveLength,NoDownload,NoProviderRequest,NoUpload}
impl From<FrameError> for ServiceError {fn from(error:FrameError)->Self{Self::Frame(error)}}
impl From<CommandError> for ServiceError {fn from(error:CommandError)->Self{Self::Command(error)}}
impl From<ProviderError> for ServiceError {fn from(error:ProviderError)->Self{Self::Provider(error)}}
impl From<TransportError> for ServiceError {fn from(error:TransportError)->Self{Self::Transport(error)}}

pub struct ServiceBuffers<'a> {
    request:&'a mut[u8;512],request_region:PublishedRegion,
    reply:&'a mut[u8;512],reply_region:PublishedRegion,
    control:&'a mut[u8;256],control_region:PublishedRegion,
    workspace:PublishedAperture<'a>,
}
impl<'a> ServiceBuffers<'a> {
    pub fn new(request:(&'a mut[u8;512],PublishedRegion),reply:(&'a mut[u8;512],PublishedRegion),control:(&'a mut[u8;256],PublishedRegion),workspace:PublishedAperture<'a>)->Result<Self,ServiceError>{
        if request.1.length()!=512 || reply.1.length()!=512 || control.1.length()!=256{return Err(ServiceError::Transport(TransportError::OutsideRegion));}
        Ok(Self{request:request.0,request_region:request.1,reply:reply.0,reply_region:reply.1,control:control.0,control_region:control.1,workspace})
    }
}
#[derive(Clone,Copy)]
enum Download<'a>{Workspace(usize),Capabilities,Prepared(PreparedPayload<'a>)}
#[derive(Clone,Copy)]
enum Receive{Command(usize),Payload(usize)}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum UploadProgress{Pending{received:ByteCount,total:ByteCount,transport:CompletionAction},Complete{completion:UploadCompletion,transport:CompletionAction},TransferFailed(i32)}
pub struct CommandCompletion{pub dispatch:Dispatch,pub transport:CompletionAction}
pub struct BulkRead<'a>{pub span:RegionSpan,pub bytes:&'a[u8]}
pub enum BulkIn<'a>{NotReady,Read(BulkRead<'a>),EmptyReply(PublishedRegion)}

pub struct Service<'a,B>{
    session:Session<'a>,commands:Commands,request:&'a mut[u8;512],request_region:PublishedRegion,
    reply:&'a mut[u8;512],reply_region:PublishedRegion,control_region:PublishedRegion,reply_length:usize,
    workspace:PublishedAperture<'a>,transport:TransportState,receive_programming:ReceiveProgramming,backend:B,
    receive:Option<Receive>,download:Option<Download<'a>>,pending_provider:Option<PayloadSelector>,upload_total:Option<usize>,upload_received:usize,
}
impl<'a,B:Backend> Service<'a,B>{
    pub fn new(buffers:ServiceBuffers<'a>,backend:B,receive_programming:ReceiveProgramming)->Self{
        Self{session:Session::new(buffers.control),commands:Commands::new(),request:buffers.request,request_region:buffers.request_region,reply:buffers.reply,reply_region:buffers.reply_region,control_region:buffers.control_region,reply_length:0,workspace:buffers.workspace,transport:TransportState::new(),receive_programming,backend,receive:None,download:None,pending_provider:None,upload_total:None,upload_received:0}
    }
    pub fn backend(&self)->&B{&self.backend}
    pub fn backend_mut(&mut self)->&mut B{&mut self.backend}
    pub fn session(&self)->&Session<'a>{&self.session}
    pub fn commands_mut(&mut self)->&mut Commands{&mut self.commands}
    pub fn transport(&self)->&TransportState{&self.transport}
    pub fn transport_mut(&mut self)->&mut TransportState{&mut self.transport}
    pub const fn receive_programming(&self)->ReceiveProgramming{self.receive_programming}
    fn download_region(&self)->Result<Option<PublishedRegion>,ServiceError>{
        Ok(match self.download{
            Some(Download::Workspace(length)) if length!=0=>Some(PublishedRegion::new(self.workspace.region().base(),length as u32)?),
            Some(Download::Capabilities)=>{let region=commands::capabilities_region();Some(PublishedRegion::new(region.address(),region.length().0)?)},
            Some(Download::Prepared(payload)) if payload.length().0!=0=>Some(PublishedRegion::new(payload.address(),payload.length().0)?),
            _=>None,
        })
    }
    pub fn control(&mut self,bytes:&[u8],tick:Tick)->Result<ControlOutcome,ServiceError>{
        let frame=ControlFrame::parse(bytes)?;
        if matches!(frame.opcode(),crate::session::Opcode::Inquiry|crate::session::Opcode::Open|crate::session::Opcode::Validate|crate::session::Opcode::Close|crate::session::Opcode::Empty) {
            self.session.synchronize_reply_history(self.reply);
        }
        let region=self.download_region()?;
        let outcome=self.session.control(frame,tick,region)?;
        match outcome.action{
            ControlAction::ReceiveCommand{length}=>{
                self.transport.clear_receive();
                if length!=0{self.transport.arm_receive(self.request_region,0,length,length,false)?;}
                self.receive_programming.begin_receive();self.receive=Some(Receive::Command(length as usize));
            }
            ControlAction::ReceivePayload{length}=>{
                let total=self.upload_total.ok_or(ServiceError::NoUpload)?;
                let remaining=total.checked_sub(self.upload_received).ok_or(ServiceError::WrongReceiveLength)?;
                if length as usize>remaining || total>self.workspace.region().length() as usize{return Err(ServiceError::Transport(TransportError::OutsideRegion));}
                if length==0 && remaining!=0{return Err(ServiceError::Frame(FrameError::InvalidPublishedSpan));}
                self.transport.clear_receive();
                if remaining!=0{self.transport.arm_receive(self.workspace.region(),self.upload_received as u32,remaining as u32,length,true)?;}
                self.receive=Some(Receive::Payload(length as usize));self.receive_programming.begin_receive();
            }
            _=>{}
        }
        match outcome.status{
            StatusDelivery::Accept(status)=>self.transport.set_response(ResponseKind::Bulk,(status as u16).to_le_bytes()),
            StatusDelivery::Reject(status)=>self.transport.set_response(ResponseKind::Control,(status as u16).to_le_bytes()),
            StatusDelivery::Deferred=>{}
        }
        Ok(outcome)
    }
    pub fn receive_command(&mut self,bytes:&[u8])->Result<CommandCompletion,ServiceError>{
        let Some(Receive::Command(expected))=self.receive else{return Err(ServiceError::NoReceive);};
        if bytes.len()!=expected{return Err(ServiceError::WrongReceiveLength);}
        self.request[..bytes.len()].copy_from_slice(bytes);
        self.complete_command_dma(CompletionStatus::Success)
    }
    pub fn complete_command_dma(&mut self,status:CompletionStatus)->Result<CommandCompletion,ServiceError>{
        let Some(Receive::Command(length))=self.receive else{return Err(ServiceError::NoReceive);};
        if let CompletionStatus::Failed(_)=status{return Err(ServiceError::WrongReceiveLength);}
        let transport=self.transport.bulk_out_complete(status)?;
        let region=self.workspace.region();
        let workspace=self.workspace.bytes_mut(region.span(0,region.length())?)?;
        let dispatch=self.commands.dispatch(self.request,length,self.reply,workspace,&mut self.backend)?;
        self.apply_dispatch(dispatch)?;self.receive=None;
        Ok(CommandCompletion{dispatch,transport})
    }
    pub fn receive_payload(&mut self,bytes:&[u8],provider:UploadProvider,messages:&mut impl UploadMessages)->Result<UploadProgress,ServiceError>{
        let Some(Receive::Payload(expected))=self.receive else{return Err(ServiceError::NoReceive);};
        if bytes.len()!=expected{return Err(ServiceError::WrongReceiveLength);}
        let total=self.upload_total.ok_or(ServiceError::NoUpload)?;
        let end=self.upload_received.checked_add(bytes.len()).ok_or(ServiceError::Transport(TransportError::AddressOverflow))?;
        if end>total{return Err(ServiceError::WrongReceiveLength);}
        let region=self.workspace.region();
        self.workspace.bytes_mut(region.span(0,region.length())?)?[self.upload_received..end].copy_from_slice(bytes);
        self.complete_payload_dma(bytes.len(),CompletionStatus::Success,provider,messages)
    }
    pub fn complete_payload_dma(&mut self,length:usize,status:CompletionStatus,provider:UploadProvider,messages:&mut impl UploadMessages)->Result<UploadProgress,ServiceError>{
        let Some(Receive::Payload(expected))=self.receive else{return Err(ServiceError::NoReceive);};
        if let CompletionStatus::Failed(code)=status{return Ok(UploadProgress::TransferFailed(code));}
        if length!=expected{return Err(ServiceError::WrongReceiveLength);}
        let total=self.upload_total.ok_or(ServiceError::NoUpload)?;
        let end=self.upload_received.checked_add(length).ok_or(ServiceError::Transport(TransportError::AddressOverflow))?;
        if end>total{return Err(ServiceError::WrongReceiveLength);}
        let transport=self.transport.bulk_out_complete(status)?;self.upload_received=end;
        if end<total{self.receive=Some(Receive::Payload(expected.min(total-end)));return Ok(UploadProgress::Pending{received:ByteCount(end as u32),total:ByteCount(total as u32),transport});}
        let region=self.workspace.region();let workspace=self.workspace.bytes(region.span(0,region.length())?)?;
        let completion=self.commands.complete_upload(&workspace[..total],provider,self.reply,messages)?;
        if completion.clear_receive_state{self.receive_programming.clear_receive_state();}
        self.reply_length=completion.reply_length;self.session.publish_reply(completion.reply_length,completion.ready)?;
        self.transport.set_response(ResponseKind::Bulk,[0,0]);
        self.receive=None;self.upload_total=None;
        Ok(UploadProgress::Complete{completion,transport})
    }
    fn apply_dispatch(&mut self,dispatch:Dispatch)->Result<(),ServiceError>{
        if dispatch.continuation{return Ok(());}
        if let Some(length)=dispatch.reply_length{self.reply_length=length;}
        self.session.publish_reply(self.reply_length,dispatch.ready)?;
        self.transport.set_response(ResponseKind::Bulk,[0,0]);
        match dispatch.transfer{
            TransferEffect::None=>{}
            TransferEffect::Upload{length}=>{self.upload_total=Some(length.0 as usize);self.upload_received=0;}
            TransferEffect::Download{region:PayloadRegion::CallerBuffer,length}=>self.download=Some(Download::Workspace(length.0 as usize)),
            TransferEffect::Download{region:PayloadRegion::Capabilities,..}=>self.download=Some(Download::Capabilities),
            TransferEffect::ProviderRequest{selector}=>self.pending_provider=Some(selector),
        }
        Ok(())
    }
    pub fn reply(&mut self)->Result<&[u8],ServiceError>{Ok(self.session.take_reply(self.reply)?)}
    pub fn payload(&self)->Result<&[u8],ServiceError>{
        let span=self.session.payload_span().ok_or(ServiceError::NoDownload)?;
        let bytes=match self.download{
            Some(Download::Workspace(length))=>{let region=self.workspace.region();&self.workspace.bytes(region.span(0,region.length())?)?[..length]},
            Some(Download::Capabilities)=>commands::capabilities(),Some(Download::Prepared(payload))=>payload.bytes(),None=>return Err(ServiceError::NoDownload),
        };
        let start=span.offset() as usize;let end=start.checked_add(span.length() as usize).ok_or(ServiceError::Transport(TransportError::AddressOverflow))?;
        bytes.get(start..end).ok_or(ServiceError::Transport(TransportError::OutsideRegion))
    }
    pub fn bulk_in_source(&mut self)->Result<BulkIn<'_>,ServiceError>{
        let mut armed=self.session.armed();let mut ready=self.session.ready();
        if let Some(span)=self.session.payload_span(){
            let region=self.download_region()?.ok_or(ServiceError::NoDownload)?;
            return match self.transport.bulk_in_source(region,span.offset(),span.length(),span.length(),true,&mut armed,&mut ready)?{
                SourceAction::Transfer(span)=>Ok(BulkIn::Read(BulkRead{span,bytes:self.payload()?})),_=>Ok(BulkIn::NotReady),
            };
        }
        let region=match self.session.reply_source(){ReplySource::Control=>self.control_region,ReplySource::Application=>self.reply_region};
        match self.transport.bulk_in_source(region,0,self.session.available_length() as u32,self.session.requested_length(),false,&mut armed,&mut ready)?{
            SourceAction::Transfer(span)=>Ok(BulkIn::Read(BulkRead{span,bytes:self.session.take_reply(self.reply)?})),
            SourceAction::EmptyReply(region)=>{self.session.take_reply(self.reply)?;Ok(BulkIn::EmptyReply(region))},
            _=>Ok(BulkIn::NotReady),
        }
    }
    pub fn bulk_out_source(&mut self)->Result<SourceAction,ServiceError>{Ok(self.transport.bulk_out_source()?)}
    /// Stock 0x00d7f2cc and 0x00d7f334 over the session block 0x01469864, the receive and upload descriptor 0x01469884 and status word 0x01469894.
    pub fn begin_connection(&mut self){
        self.session.begin_connection();
        self.transport.clear_receive();
        self.transport.set_response(ResponseKind::None,[0,0]);
        self.commands.clear_upload();
        self.receive=None;self.download=None;self.upload_total=None;self.upload_received=0;
    }
    /// Stock 0x00d7fd64.
    pub fn open_connection(&mut self){self.commands.open();self.pending_provider=None;}
    pub fn consume_queue_completion(&mut self,controller:&mut crate::controller::QueueController,completion:commands::VendorCompletion,prepared:Option<PreparedPayload<'a>>)->Result<commands::CompletionDisposition,ServiceError>{
        let provider_pending=self.pending_provider.is_some();
        let provider_success=matches!(completion,commands::VendorCompletion::Command{status:commands::CompletionCode(0)});
        self.session.begin_reply_completion(self.reply);
        let disposition=controller.consume_completion(completion,&mut self.commands,self.reply,prepared.as_ref()).map_err(ServiceError::Controller)?;
        if let commands::CompletionDisposition::Command{ready}=disposition{
            if provider_pending&&ready{
                self.pending_provider=None;
                self.download=if provider_success{prepared.map(Download::Prepared)}else{None};
            }
            self.session.finish_reply_completion(self.reply,ready);
        }
        Ok(disposition)
    }
    fn complete(&mut self,outcome:BackendStatus)->Result<(),ServiceError>{
        self.session.begin_reply_completion(self.reply);
        let ready=self.commands.complete(outcome,self.reply)?;
        self.session.finish_reply_completion(self.reply,ready);
        Ok(())
    }
    pub fn provide(&mut self,current:ImageDescriptor,regions:&ProviderRegions<'a>)->Result<(),ServiceError>{
        let selector=self.pending_provider.ok_or(ServiceError::NoProviderRequest)?;
        let route=match commands::provider_route(selector,current,&self.backend){Ok(route)=>route,Err(ProviderError::InvalidSelector|ProviderError::Unavailable)=>{self.pending_provider=None;return self.complete(BackendStatus::Unavailable);},Err(error)=>return Err(error.into())};
        let payload=match commands::provider_bytes(route,regions,&self.backend){Ok(payload)=>payload,Err(ProviderError::Unavailable)=>{self.pending_provider=None;return self.complete(BackendStatus::Unavailable);},Err(error)=>return Err(error.into())};
        self.publish_prepared_provider(payload)
    }
    pub fn provide_device_info(&mut self,calibration:&mut crate::metadata_calibration::MetadataCalibration<'_>,snapshot:&crate::metadata::DeviceInfoSnapshot<'_>,output:&'a mut[u8],address:commands::DebugAddress)->Result<(),ServiceError>{
        if self.pending_provider!=Some(PayloadSelector(0x2b2)){return Err(ServiceError::NoProviderRequest);}
        let payload=crate::metadata::prepare_device_info(calibration,output,address,snapshot).map_err(ServiceError::Metadata)?;
        self.publish_prepared_provider(payload)
    }
    pub fn publish_prepared_provider(&mut self,payload:PreparedPayload<'a>)->Result<(),ServiceError>{
        if self.pending_provider.is_none(){return Err(ServiceError::NoProviderRequest);}
        self.session.begin_reply_completion(self.reply);
        self.commands.publish_provider_payload(&payload,self.reply)?;self.download=Some(Download::Prepared(payload));self.pending_provider=None;
        self.session.finish_reply_completion(self.reply,true);Ok(())
    }
}
