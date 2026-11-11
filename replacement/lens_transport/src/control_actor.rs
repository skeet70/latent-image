//! Task 99 dispatcher and public lifecycle, X-M1 1.50 image 4.

use crate::control_device::{ControlDevice,ControlQueue,DeviceCallback,DeviceStatus,QueueFailure};
use crate::control_types::*;

pub trait ControlConstructor {
    fn create_queue(&mut self,task:u8,message_bytes:usize,capacity:usize)->Result<(),ControlError>;
    fn create_task(&mut self,task:u8,priority:u8,stack_bytes:usize)->Result<(),ControlError>;
}

pub struct ControlActor<'a> {
    context:ControlContext<'a>,
    device:ControlDevice,
    descriptor_generation:u32,
    cache_generation:u32,
}
impl<'a> ControlActor<'a> {
    pub fn new(configuration:[u8;128],warm_start:bool,readiness:&'a SharedReadiness)->Self {
        let mut context=ControlContext::new(readiness);
        context.warm_start=warm_start;
        Self { context,device:ControlDevice::new(configuration,warm_start),descriptor_generation:0,cache_generation:0 }
    }
    pub fn descriptor_generation(&self)->u32 { self.descriptor_generation }
    pub fn cache_generation(&self)->u32 { self.cache_generation }
    pub fn start(source:&impl ConfigurationSource,io:&mut impl ControlIo,readiness:&'a SharedReadiness)->Self {
        let mut configuration=[0;128];
        for (destination,start,length) in [(0,0x1c7,10),(10,0x106,20),(30,0x15c,15),(45,0x11b,30)] {
            for index in 0..length { configuration[destination+index]=source.configuration_byte(ConfigurationOffset(start+index as u32)); }
        }
        configuration[0x4b]=1;configuration[0x4c]=0x50;
        configuration[0x4d]=source.configuration_byte(ConfigurationOffset(0x204));
        configuration[0x4e]=source.configuration_byte(ConfigurationOffset(0x205));
        for index in 0..6 { configuration[0x4f+index]=source.configuration_byte(ConfigurationOffset(0x4069+index as u32)); }
        let warm_start=io.read_mmio(MmioAddress(0x400a0140))!=0;
        if warm_start { io.write_mmio(MmioAddress(0x400a0140),0); }
        io.set_detection_mode(DetectionMode::Off);
        if !warm_start { reset_pins(io); }
        Self::new(configuration,warm_start,readiness)
    }
    pub fn construct(platform:&mut impl ControlConstructor)->Result<(),ControlError> {
        platform.create_queue(99,48,32)?;
        platform.create_task(99,4,3072)
    }
    pub fn state(&self)->ControlState { self.context.state }
    pub fn readiness(&self)->bool { self.context.readiness.is_ready() }
    pub fn capability(&self)->Capability { self.context.capability }
    pub fn connected(&self)->bool { self.context.connected }
    pub fn available(&self)->bool { self.context.attached }
    pub fn force_next_query5(&mut self) { self.device.force_next_query5(); }
    pub fn status(&self)->CompletionStatus { self.context.status }
    pub fn mode_status(&self)->ModeStatus { match self.context.public_status { 1=>ModeStatus::Resuming,2=>ModeStatus::Suspended,_=>ModeStatus::Unavailable } }
    pub fn device(&self)->&ControlDevice { &self.device }
    pub fn apply_mode_completion(&mut self,mode:ModeCompletion,services:&mut impl ControlServices) {
        mode_complete(&mut self.context,mode,services);
    }
    pub fn callback(&mut self,callback:DeviceCallback,status:CompletionStatus,data:&[u8],io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<crate::control_device::CallbackAcceptance,ControlError> {
        self.device.callback(&mut self.context,callback,status,data,io,queue).map_err(queue_error)
    }
    pub fn request_snapshot(&mut self,kind:ObserverKind,callback:CallbackToken,queue:&mut impl ControlQueue)->Result<(),ControlError> {
        self.device.request_snapshot(kind,callback,queue)
    }
    pub fn callback_page(kind:ResponseKind,status:CompletionStatus,marker:u16,page:Option<CalibrationPage>,io:&mut impl ControlIo)->crate::control_device::CallbackAcceptance {
        ControlDevice::callback_page(kind,status,marker,page,io)
    }
    pub fn configure_periodic(&mut self,index:crate::control_device::PeriodicIndex,ticks:TickCount,modes:PeriodicModes,callback:Option<CallbackToken>) {
        let interval=(ticks.0.wrapping_add(39)/40) as u16;
        self.context.periodic[index.get()]=PeriodicEntry { interval,remaining:interval,modes:u32::from(modes.0),callback,selected:0,argument:0 };
        if index.get()==5 { self.device.force_next_query5(); }
    }
    pub fn cancel_periodic(&mut self,index:crate::control_device::PeriodicIndex) {
        self.context.periodic[index.get()]=PeriodicEntry::EMPTY;
        if index.get()==6 { self.device.clear_change_pending(); }
    }
    pub fn setup(request:SetupRequest,io:&mut impl ControlServices)->Result<(),ControlError> {
        io.post(ControlMessage { opcode:ControlOpcode::Setup,payload:ControlPayload::Setup(request) })
    }
    pub fn synchronize(callback:Option<CallbackToken>,io:&mut impl ControlServices)->Result<(),ControlError> {
        io.post(ControlMessage { opcode:ControlOpcode::Synchronize,payload:ControlPayload::Callback(callback) })
    }
    pub fn teardown(callback:Option<CallbackToken>,io:&mut impl ControlServices)->Result<(),ControlError> {
        io.post(ControlMessage { opcode:ControlOpcode::Teardown,payload:ControlPayload::Callback(callback) })
    }
    pub fn start_polling(io:&mut impl ControlServices)->Result<(),ControlError> {
        io.post(ControlMessage { opcode:ControlOpcode::StartPolling,payload:ControlPayload::Callback(None) })
    }
    pub fn finish(io:&mut impl ControlServices)->Result<(),ControlError> {
        io.post(ControlMessage { opcode:ControlOpcode::Finish,payload:ControlPayload::Empty })
    }
    pub fn register_wait(&mut self,target:WaitTarget)->Result<bool,ControlError> {
        if !self.context.connected { return Ok(true); }
        if self.context.wait!=WaitRegistration::Unregistered { return Err(ControlError::Busy); }
        if self.context.readiness.is_ready() { self.context.wait=WaitRegistration::Completed; Ok(true) }
        else { self.context.wait=WaitRegistration::Waiting(target); Ok(false) }
    }
    pub fn register_first_response(&mut self,callback:Option<CallbackToken>,io:&mut impl ControlServices) {
        if self.context.first_response_received { complete(callback,CompletionStatus::SUCCESS,io); }
        else { self.context.first_response_callback=callback; }
    }
    #[cfg(feature = "native-actor")]
    pub fn native_context(&mut self) -> &mut ControlContext<'a> { &mut self.context }
    #[cfg(feature = "native-actor")]
    pub fn native_device(&mut self) -> &mut ControlDevice { &mut self.device }
    pub fn dispatch(&mut self,message:ControlMessage,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),ControlError> {
        use ControlOpcode::*;
        let cache_changed=match message.payload {
            ControlPayload::Response(response)=>matches!(self.context.state,ControlState::Active|ControlState::Suspended) && response.status==CompletionStatus::SUCCESS && response.kind.code()<=11,
            ControlPayload::DeviceCompletion {callback:DeviceCallback::Snapshot(_),status:CompletionStatus::SUCCESS,..}=>true,
            ControlPayload::Setup(_)=>self.context.state==ControlState::Idle && self.context.connected,
            ControlPayload::Empty=>message.opcode==ControlOpcode::Detached,
            _=>false,
        };
        if cache_changed { self.cache_generation=self.cache_generation.wrapping_add(1); }
        if let (ControlOpcode::Poll,ControlPayload::Timer { kind,token })=(message.opcode,message.payload) {
            return self.handle_timer(kind,token,io,queue);
        }
        if message.opcode==ControlOpcode::Poll {
            match message.payload {
                ControlPayload::ResetLens=>{ self.force_next_query5();return Ok(()); }
                ControlPayload::ConfigurePeriodic {index,ticks,modes,callback}=>{self.configure_periodic(index,ticks,modes,callback);return Ok(());}
                ControlPayload::CancelPeriodic(index)=>{self.cancel_periodic(index);return Ok(());}
                ControlPayload::Snapshot {kind,callback}=>{
                    if let Err(error)=self.device.request_snapshot(kind,callback,queue) {io.complete(callback,CompletionStatus::REJECTED,&[]);return Err(error);}
                    return Ok(());
                }
                ControlPayload::DeviceCompletion {callback,status,data}=>return self.callback(callback,status,&data,io,queue).map(|_|()),
                _=>(),
            }
        }
        if self.context.state==ControlState::Initializing {
            if let ControlPayload::Response(response)=message.payload {
                if matches!(response.kind,ResponseKind::Reply19|ResponseKind::Reply21) && response.status==CompletionStatus::SUCCESS && response.data[..2]==[0,0] && response.page.is_some() {
                    self.descriptor_generation=self.descriptor_generation.wrapping_add(1);
                }
            }
        }
        let ctx=&mut self.context;
        let active=matches!(ctx.state,ControlState::Active|ControlState::Suspended);
        match (message.opcode,message.payload) {
            (Register,ControlPayload::Register { enabled,completion })=> {
                if !ctx.registered {
                    ctx.registered=true;ctx.connected=enabled;ctx.detection_enabled=!enabled;ctx.detection_samples=0;
                    ctx.detection_timer=Some(io.start_timer(TickCount(25),true,ControlTimer::Detection));
                    if enabled { set_detection_hook(true,io); } else { ctx.warm_start=false; }
                }
                complete(completion,CompletionStatus::SUCCESS,io);
            }
            (Setup,ControlPayload::Setup(request))=> {
                if ctx.state==ControlState::Idle { self.device.initialize(ctx,request,io,queue).map_err(queue_error)?; }
                else { complete(request.completion,CompletionStatus::REJECTED,io); }
            }
            (Teardown,ControlPayload::Callback(callback))=> {
                if ctx.teardown_callback.is_none() { ctx.teardown_callback=callback;stop(ctx,io,queue); }
                else { complete(callback,CompletionStatus::REJECTED,io); }
            }
            (Synchronize,ControlPayload::Callback(callback))=> {
                if active {
                    ctx.polling=false;
                    if let Some(timer)=ctx.poll_timer.take() { io.cancel_timer(timer); }
                    if ctx.in_flight.iter().any(|pending|*pending) && ctx.synchronize_callback.is_none() { ctx.synchronize_callback=callback; }
                    else { complete(callback,CompletionStatus::SUCCESS,io); }
                } else { complete(callback,CompletionStatus::SUCCESS,io); }
            }
            (StartPolling,ControlPayload::Callback(callback))=> {
                if active && ctx.polling_supported { ctx.polling=true;start_poll_timer(ctx,io); }
                complete(callback,CompletionStatus::SUCCESS,io);
            }
            (Resume,ControlPayload::Callback(callback))|(Suspend,ControlPayload::Callback(callback))=> {
                if ctx.state!=ControlState::Suspended { complete(callback,CompletionStatus::REJECTED,io); }
                else if message.opcode==Resume {
                    if ctx.resume_callback.is_some() { complete(callback,CompletionStatus::REJECTED,io); }
                    else { ctx.resume_callback=callback;ctx.public_status=1;queue.resume(DeviceCallback::ModeResume).map_err(queue_error)?;ctx.mode_resumed=true; }
                } else if ctx.suspend_callback.is_some() { complete(callback,CompletionStatus::REJECTED,io); }
                else { ctx.suspend_callback=callback;queue.suspend(DeviceCallback::ModeSuspend).map_err(queue_error)?;ctx.mode_resumed=false; }
            }
            (Finish,ControlPayload::Empty)=>if ctx.state==ControlState::Active { ctx.state=ControlState::Suspended;queue.finish(LensModelIdentifier(self.device.capabilities().identifier)).map_err(queue_error)?; },
            (Response,ControlPayload::Response(response))=>match ctx.state {
                ControlState::Initializing=>self.device.init_response(ctx,response,io,queue).map_err(queue_error)?,
                ControlState::Active|ControlState::Suspended=>self.device.response(ctx,response,io,queue).map_err(queue_error)?,
                _=>(),
            },
            (Attached,ControlPayload::Empty)=> { ctx.connected=true;io.key_release(KeyEvent(0x38)); }
            (Detached,ControlPayload::Empty)=>{ self.device.reset_detached_cache();reset_detached(ctx,io,queue); },
            (Ready,ControlPayload::Empty)=>if matches!(ctx.state,ControlState::Idle|ControlState::Initializing|ControlState::Active|ControlState::Suspended) { self.device.ready(ctx,io,queue).map_err(queue_error)?; },
            (InitializationFinished,ControlPayload::InitializationFinished(failed))=> {
                if matches!(ctx.state,ControlState::Initializing|ControlState::Active|ControlState::Suspended) {
                    self.device.initialization_finished(ctx,!failed,io,queue).map_err(queue_error)?;
                }
            }
            (Poll,ControlPayload::Poll)=>if active && ctx.polling {
                for entry in &mut ctx.periodic { if entry.modes!=0 && entry.remaining!=0 { entry.remaining-=1; } }
                if !ctx.in_flight.iter().any(|pending|*pending) {
                    ctx.in_flight[0]=true;queue.poll(DeviceCallback::Response(ResponseKind::Reply01)).map_err(queue_error)?;
                    if !ctx.mode_resumed && ctx.periodic[31].modes!=0 && ctx.periodic[31].remaining==0 {
                        ctx.periodic[31].remaining=ctx.periodic[31].interval;ctx.in_flight[31]=true;
                        queue.read_two(DeviceCallback::Response(ResponseKind::Reply07)).map_err(queue_error)?;
                    }
                }
            },
            (ModeComplete,ControlPayload::ModeComplete(mode))=>mode_complete(ctx,mode,io),
            (TeardownComplete,ControlPayload::Empty)=>if ctx.state!=ControlState::Detached { teardown_complete(ctx,io); },
            (BeginDetection,ControlPayload::Empty)=>if ctx.state==ControlState::Initializing { begin_detection(ctx,io,queue).map_err(queue_error)?; },
            _=>return Err(ControlError::InvalidPayload),
        }
        Ok(())
    }
    pub fn handle_timer(&mut self,timer:ControlTimer,token:TimerToken,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),ControlError> {
        let expected=match timer {
            ControlTimer::Poll=>self.context.poll_timer,
            ControlTimer::Detection=>self.context.detection_timer,
            ControlTimer::InitializationTimeoutOff|ControlTimer::InitializationTimeoutOn=>self.context.initialization_timer,
            ControlTimer::RestrictedWait=>self.context.restricted_timer,
        };
        if expected!=Some(token) { return Ok(()); }
        match timer {
            ControlTimer::Poll=>self.dispatch(ControlMessage { opcode:ControlOpcode::Poll,payload:ControlPayload::Poll },io,queue)?,
            ControlTimer::Detection=>self.device.sample_detection(&mut self.context,io),
            ControlTimer::InitializationTimeoutOff|ControlTimer::InitializationTimeoutOn=>{
                self.device.initialization_timeout(&mut self.context,timer==ControlTimer::InitializationTimeoutOn,io,queue).map_err(queue_error)?;
            }
            ControlTimer::RestrictedWait=>self.device.restricted_delay_elapsed(&mut self.context,io,queue),
        }
        Ok(())
    }
}
fn queue_error(_:QueueFailure)->ControlError { ControlError::Queue }
fn complete(callback:Option<CallbackToken>,status:CompletionStatus,io:&mut impl ControlServices) { if let Some(callback)=callback { io.complete(callback,status,&[]); } }
pub(crate) fn notify_waiter(ctx:&mut ControlContext,io:&mut impl ControlIo) {
    if let WaitRegistration::Waiting(target)=ctx.wait { ctx.wait=WaitRegistration::Completed;io.notify(target); }
}
fn start_poll_timer(ctx:&mut ControlContext,io:&mut impl ControlIo) { if ctx.poll_timer.is_none() { ctx.poll_timer=Some(io.start_timer(TickCount(40),true,ControlTimer::Poll)); } }

pub fn publish_status(_ctx:&mut ControlContext,status:DeviceStatus,io:&mut impl ControlIo) {
    match status {
        DeviceStatus::Ready=>io.diagnostic(2,0xe1),DeviceStatus::Connection=>io.diagnostic(2,0xe0),DeviceStatus::Invalid=>io.diagnostic(2,0xe2),
        DeviceStatus::Detection=>io.key_release(KeyEvent(0x938)),DeviceStatus::Failure=>io.key_release(KeyEvent(0x939)),
        DeviceStatus::Compatibility=>io.key_release(KeyEvent(0x932)),DeviceStatus::Capability=>io.key_release(KeyEvent(0x933)),
        DeviceStatus::Restricted=>io.key_release(KeyEvent(0x934)),DeviceStatus::Detached=>io.key_release(KeyEvent(0x422)),DeviceStatus::Complete=>(),
    }
}
pub fn set_detection_hook(enabled:bool,io:&mut impl ControlIo) { io.set_detection_callback(DetectionEdge::Attach,enabled); }
fn reset_pins(io:&mut impl ControlIo) {
    for (address,value) in [(0xfff8900c,0xc0000000),(0xfff89084,0x10000000),(0xfff89090,0x03000000),(0xfff89080,0x10001000),(0xfff89344,0x01000000),(0xfff89350,0x00030000)] { io.write_mmio(MmioAddress(address),value); }
    io.update_mmio(MmioAddress(0xfff89358),0x30000,0);
    for (address,value) in [(0xfff89340,0x00010001),(0xfff89344,0x00040000),(0xfff8934c,0x00300000),(0xfff89340,0x00040004),(0xfff89344,0x00200000),(0xfff8934c,0x0c000000)] { io.write_mmio(MmioAddress(address),value); }
    io.update_mmio(MmioAddress(0xfff89358),0xc00,0);
    io.write_mmio(MmioAddress(0xfff89340),0x00200020);
}
pub fn stop(ctx:&mut ControlContext,io:&mut impl ControlIo,queue:&mut impl ControlQueue) {
    // 0x006c1f28 stores 0xffffff01 at the shared block's status word.
    ctx.status=CompletionStatus::REJECTED;ctx.attached=false;ctx.public_status=0;ctx.in_flight.fill(false);ctx.last_status=0xffff;
    if let Some(timer)=ctx.poll_timer.take() { io.cancel_timer(timer); }
    if let Some(timer)=ctx.initialization_timer.take() { io.cancel_timer(timer); }
    if let Some(timer)=ctx.restricted_timer.take() { io.cancel_timer(timer); }
    ctx.restricted_delays=0;
    io.write_mmio(MmioAddress(0xfff89080),0x80000000);io.write_mmio(MmioAddress(0xfff89080),0x40000000);
    io.set_detection_mode(DetectionMode::Off);
    reset_pins(io);ctx.detection_enabled=false;ctx.detection_samples=0;
    io.key_release(KeyEvent(0x421));
    let _=queue.stop(DeviceCallback::Teardown);
    complete(ctx.periodic[2].callback.take(),CompletionStatus::SUCCESS,io);
    ctx.first_response_received=false;
    complete(ctx.first_response_callback.take(),CompletionStatus::SUCCESS,io);
    ctx.state=ControlState::Stopping;
}
pub fn reset_detached(ctx:&mut ControlContext,io:&mut impl ControlIo,queue:&mut impl ControlQueue) {
    ctx.connected=false;ctx.warm_start=false;ctx.detection_enabled=false;ctx.detection_samples=0;
    io.key_release(KeyEvent(0xb8));ctx.periodic[13]=PeriodicEntry::EMPTY;ctx.periodic[11]=PeriodicEntry::EMPTY;
    if ctx.state==ControlState::Detached { io.key_release(KeyEvent(0xb9)); }
    stop(ctx,io,queue);if !ctx.gpio_preserved { io.reset_gpio(); }
}
fn teardown_complete(ctx:&mut ControlContext,io:&mut impl ControlIo) {
    complete(ctx.initialization_callback.take(),CompletionStatus::REJECTED,io);
    complete(ctx.resume_callback.take(),CompletionStatus::REJECTED,io);
    complete(ctx.suspend_callback.take(),CompletionStatus::REJECTED,io);
    complete(ctx.synchronize_callback.take(),CompletionStatus::SUCCESS,io);
    notify_waiter(ctx,io);complete(ctx.teardown_callback.take(),CompletionStatus::SUCCESS,io);ctx.state=ControlState::Idle;
}

pub fn paged_read(response:ControlResponse,begin:bool,callback:DeviceCallback,target:&mut [u8;128],_io:&mut impl ControlIo,queue:&mut impl ControlQueue)->CompletionStatus {
    if begin {
        return if queue.accept_page(false,callback).is_ok() { CompletionStatus(-12) } else { CompletionStatus::REJECTED };
    }
    if response.status!=CompletionStatus::SUCCESS { return CompletionStatus::REJECTED; }
    if u16::from_le_bytes([response.data[0],response.data[1]])!=0 { return CompletionStatus::SUCCESS; }
    let Some(page)=response.page else { return CompletionStatus::REJECTED; };
    target.copy_from_slice(&page.0);
    if queue.accept_page(true,callback).is_ok() { CompletionStatus(-12) } else { CompletionStatus::REJECTED }
}

pub fn capability_write(response:ControlResponse,begin:bool,configuration:&[u8;128],queue:&mut impl ControlQueue)->CompletionStatus {
    let marker=if begin { 0 } else {
        if response.status!=CompletionStatus::SUCCESS { return CompletionStatus::REJECTED; }
        if u16::from_le_bytes([response.data[0],response.data[1]])!=0xffff { return CompletionStatus::SUCCESS; }
        0xffff
    };
    if queue.write_configuration(crate::control_device::QueuePage(marker),configuration,DeviceCallback::CapabilityPage).is_ok() { CompletionStatus(-12) } else { CompletionStatus::REJECTED }
}

pub fn detection_output(active:bool,callback:Option<DeviceCallback>,io:&mut impl ControlIo) {
    let completion=match callback {
        Some(DeviceCallback::DetectionOff)=>Some(DetectionCompletion::Off),
        Some(DeviceCallback::DetectionOn)=>Some(DetectionCompletion::On),
        _=>None,
    };
    io.detection_output(if active { DetectionMode::On } else { DetectionMode::Off },completion);
}
pub fn begin_detection(ctx:&mut ControlContext,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
    if ctx.capability.0==0 {
        io.update_mmio(MmioAddress(0xfff89358),0xc00,0);
        io.update_mmio(MmioAddress(0xfff89358),0x30000,0);
    }
    ctx.initialization_timer=Some(io.start_timer(TickCount(5000),false,ControlTimer::InitializationTimeoutOn));
    io.set_detection_callback(DetectionEdge::Probe,true);
    if ctx.capability.0==0 {
        queue.submit(crate::control_device::QueuePacket::query(crate::control_device::QueueOperation::BeginDetection,DeviceCallback::Response(ResponseKind::Reply17)))?;
    }
    Ok(())
}

fn mode_complete(ctx:&mut ControlContext,mode:ModeCompletion,services:&mut impl ControlServices) {
    if ctx.state==ControlState::Detached { return; }
    let callback=match mode {
        ModeCompletion::Resume=>{ ctx.public_status=1;ctx.resume_callback.take() }
        ModeCompletion::Suspend=>{ ctx.public_status=2;ctx.suspend_callback.take() }
    };
    complete(callback,CompletionStatus::SUCCESS,services);
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use self::std::vec::Vec;

    #[derive(Default)]
    struct Services { messages:Vec<ControlMessage>,completions:Vec<(CallbackToken,CompletionStatus)>,reject:bool }
    impl ControlServices for Services {
        fn delay_ticks(&mut self,_ticks:TickCount) {}
        fn post(&mut self,message:ControlMessage)->Result<(),ControlError> {
            if self.reject { Err(ControlError::Queue) } else { self.messages.push(message);Ok(()) }
        }
        fn complete(&mut self,callback:CallbackToken,status:CompletionStatus,_data:&[u8]) { self.completions.push((callback,status)); }
        fn notify(&mut self,_target:WaitTarget) {}
        fn key_release(&mut self,_event:KeyEvent) {}
        fn diagnostic(&mut self,_category:u32,_event:u32) {}
        fn start_timer(&mut self,_ticks:TickCount,_repeating:bool,_callback:ControlTimer)->TimerToken { TimerToken(1) }
        fn cancel_timer(&mut self,_timer:TimerToken) {}
        fn set_detection_callback(&mut self,_edge:DetectionEdge,_enabled:bool) {}
        fn reset_gpio(&mut self) {}
    }

    #[test]
    fn setup_is_queued_without_fabricating_readiness() {
        let readiness=SharedReadiness::new();
        let actor=ControlActor::new([0;128],false,&readiness);
        let mut services=Services::default();
        let request=SetupRequest { completion:Some(CallbackToken(7)),polling:true,mode:SetupMode::Primary,owner:ControlOwner(12) };
        assert_eq!(ControlActor::setup(request,&mut services),Ok(()));
        assert_eq!(services.messages.as_slice(),[ControlMessage { opcode:ControlOpcode::Setup,payload:ControlPayload::Setup(request) }]);
        assert!(!actor.readiness());
        assert_eq!(actor.capability(),Capability(0));
        assert!(services.completions.is_empty());
    }

    #[test]
    fn producers_preserve_order_and_queue_errors() {
        let mut services=Services::default();
        assert_eq!(ControlActor::synchronize(Some(CallbackToken(3)),&mut services),Ok(()));
        assert_eq!(ControlActor::finish(&mut services),Ok(()));
        assert_eq!(ControlActor::teardown(Some(CallbackToken(4)),&mut services),Ok(()));
        assert_eq!(services.messages.iter().map(|message|message.opcode).collect::<Vec<_>>(),[ControlOpcode::Synchronize,ControlOpcode::Finish,ControlOpcode::Teardown]);
        services.reject=true;
        assert_eq!(ControlActor::start_polling(&mut services),Err(ControlError::Queue));
        assert_eq!(services.messages.len(),3);
    }

    #[test]
    fn mode_completion_updates_the_actual_public_status() {
        let readiness=SharedReadiness::new();
        let mut actor=ControlActor::new([0;128],false,&readiness);
        let mut services=Services::default();
        assert_eq!(actor.mode_status(),ModeStatus::Unavailable);
        actor.apply_mode_completion(ModeCompletion::Suspend,&mut services);
        assert_eq!(actor.mode_status(),ModeStatus::Suspended);
        actor.apply_mode_completion(ModeCompletion::Resume,&mut services);
        assert_eq!(actor.mode_status(),ModeStatus::Resuming);
        assert!(!actor.readiness());
        assert!(services.completions.is_empty());
    }

    #[test]
    fn periodic_selection_obeys_mode_and_countdown() {
        let mut entry=PeriodicEntry { interval:2,remaining:0,modes:10,callback:None,selected:0,argument:0 };
        assert!(entry.select(false,true));
        assert_eq!(entry.selected,2);
        assert_eq!(entry.remaining,2);
        assert!(!entry.select(true,true));
        let mut disabled=PeriodicEntry::EMPTY;
        assert!(!disabled.select(false,false));
    }
}
