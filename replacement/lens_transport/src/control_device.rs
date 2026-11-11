//! Task 99 device CPU, X-M1 1.50 image 4, 0x006c2c08..0x006c5808.
//! Branches and packet fields follow the image bytes; donor C is nonmatching.

use crate::control_actor::{capability_write, paged_read, publish_status, reset_detached, stop};
use crate::control_types::{CallbackToken, CompletionStatus, ControlContext, ControlIo, ControlMessage, ControlOpcode, ControlPayload, ControlResponse, ControlState, ControlTimer, KeyEvent, MmioAddress, ObserverKind, ResponseKind, SetupMode, SetupRequest, TickCount, WaitRegistration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DeviceStatus { Complete = 0, Ready = 1, Connection = 2, Invalid = 3, Detection = 5, Failure = 6, Compatibility = 7, Capability = 8, Restricted = 9, Detached = 10 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeriodicIndex(u8);
impl PeriodicIndex {
    pub const fn new(value:u8)->Option<Self> { if value <= 32 { Some(Self(value)) } else { None } }
    pub const fn get(self)->usize { self.0 as usize }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceCallback {
    Response(ResponseKind), Initialized, Observer, DetectionOff, DetectionOn,
    Poll, Teardown, ModeResume, ModeSuspend, Configuration, CapabilityPage, Snapshot(ObserverKind),
}
impl DeviceCallback {
    pub const fn token(self)->u32 { match self {
        Self::Response(kind)=>kind.code() as u32, Self::Initialized=>0x100, Self::Observer=>0x101,
        Self::DetectionOff=>0x102, Self::DetectionOn=>0x103, Self::Poll=>0x104,
        Self::Teardown=>0x105, Self::ModeResume=>0x106, Self::ModeSuspend=>0x107,
        Self::Configuration=>0x108, Self::CapabilityPage=>0x109,
        Self::Snapshot(ObserverKind::TwoBytes)=>0x10a,Self::Snapshot(ObserverKind::ThreeBytes)=>0x10b,Self::Snapshot(ObserverKind::NineBytes)=>0x10c,
    } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum QueueOperation {
    BeginDetection=0x7202,
    SelectConfiguration=0x7207, SelectCapability=0x7208, Capability=0x7209,
    Restrict=0x720a, Status=0x720d, Query2=0x720e, Query32=0x720f,
    Query6=0x7210, Query5=0x7214, ExtendedStatus=0x7215, Query4=0x7216,
    Query13=0x7217, SelectSecondary=0x725c,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuePacket { pub operation:QueueOperation, pub callback:DeviceCallback, bytes:[u8;48] }
impl QueuePacket {
    pub fn bytes(&self)->&[u8;48] { &self.bytes }
    pub fn query(operation:QueueOperation,callback:DeviceCallback)->Self {
        let command=match operation { QueueOperation::BeginDetection=>0xff,QueueOperation::Capability=>0x16,QueueOperation::Restrict=>0x19,
            QueueOperation::Query5=>0x30,QueueOperation::ExtendedStatus=>0x31,QueueOperation::Query4=>0x32,
            QueueOperation::Query13=>0x3f,QueueOperation::Status=>0x21,QueueOperation::Query2=>0x22,
            QueueOperation::Query32=>0x23,QueueOperation::Query6=>0x24,
            QueueOperation::SelectConfiguration|QueueOperation::SelectCapability|QueueOperation::SelectSecondary=>0x18 };
        let mut bytes=[0;48];
        bytes[..4].copy_from_slice(&callback.token().to_le_bytes());
        bytes[4..8].copy_from_slice(&(command as u32 * 64).to_le_bytes());
        bytes[44]=1;
        Self { operation,callback,bytes }
    }
    pub fn selector(operation:QueueOperation,callback:DeviceCallback)->Self {
        let mut packet=Self::query(operation,callback);
        let word=(0x18u32<<6)|(1<<16)|(7<<27)|(1<<31);
        packet.bytes[4..8].copy_from_slice(&word.to_le_bytes());
        let command=match operation { QueueOperation::SelectConfiguration=>0x14u32,QueueOperation::SelectSecondary=>0x96u32,_=>0x15u32 };
        packet.bytes[8..12].copy_from_slice(&(command<<6).to_le_bytes());
        packet.bytes[44]=2;
        packet
    }
    pub fn selected_query(operation:QueueOperation,callback:DeviceCallback,selector:[u8;3])->Self {
        let mut packet=Self::query(operation,callback);
        packet.bytes[7]=if operation==QueueOperation::Status { selector[0] } else { selector[0]&15 };
        packet.bytes[6]=((selector[1]&1)<<4)|(selector[2]&1);
        packet
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueFailure { Full, InvalidState, Transport }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuePage(pub u16);

pub trait ControlQueue {
    fn initialize(&mut self,completion:DeviceCallback,observer:DeviceCallback)->Result<(),QueueFailure>;
    fn submit(&mut self,packet:QueuePacket)->Result<(),QueueFailure>;
    fn stop(&mut self,completion:DeviceCallback)->Result<(),QueueFailure>;
    fn write_configuration(&mut self,page:QueuePage,configuration:&[u8;128],completion:DeviceCallback)->Result<(),QueueFailure>;
    fn accept_page(&mut self,accept:bool,completion:DeviceCallback)->Result<(),QueueFailure>;
    fn finish(&mut self,model:crate::control_types::LensModelIdentifier)->Result<(),QueueFailure>;
    fn resume(&mut self,completion:DeviceCallback)->Result<(),QueueFailure>;
    fn suspend(&mut self,completion:DeviceCallback)->Result<(),QueueFailure>;
    fn poll(&mut self,completion:DeviceCallback)->Result<(),QueueFailure>;
    fn read_two(&mut self,completion:DeviceCallback)->Result<(),QueueFailure>;
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityCache { pub model:[u8;8],pub descriptor:[u8;8],pub trailing:[u8;10] }
impl IdentityCache { const EMPTY:Self=Self { model:[0;8],descriptor:[0;8],trailing:[0;10] }; }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityCache { pub model:u8,pub identifier:u16,pub protocol:u8,pub upgrade:u8,pub focus:u8,pub focus_copy:u8,pub stabilization:u8,pub special:bool,pub features:u8,pub extended:bool,pub flag:bool }
impl CapabilityCache { const EMPTY:Self=Self { model:0,identifier:0,protocol:0,upgrade:0,focus:0,focus_copy:0,stabilization:0,special:false,features:0,extended:false,flag:false }; }

pub struct ControlDevice {
    configuration:[u8;128],calibration:[u8;128],secondary_calibration:[u8;128],cache:[u8;60],
    identity:IdentityCache,capabilities:CapabilityCache,warm_start:bool,setup:Option<SetupRequest>,
    initialization_failures:u32,suppress_queries:bool,force_query5:bool,change_pending:bool,
    restricted:bool,invalid:bool,initialization_error:bool,
    capability_flags:[u8;3],snapshot_callbacks:[Option<CallbackToken>;3],errors:[u8;9],
}
impl ControlDevice {
    pub const fn new(configuration:[u8;128],warm_start:bool)->Self { Self {
        configuration,calibration:[0;128],secondary_calibration:[0;128],cache:[0;60],
        identity:IdentityCache::EMPTY,capabilities:CapabilityCache::EMPTY,warm_start,setup:None,
        initialization_failures:0,suppress_queries:false,force_query5:false,change_pending:false,
        restricted:false,invalid:false,initialization_error:false,
        capability_flags:[0;3],snapshot_callbacks:[None;3],errors:[0;9],
    } }
    pub fn cache(&self)->&[u8;60] { &self.cache }
    pub fn clear_change_pending(&mut self) { self.change_pending=false; }
    pub fn identity(&self)->&IdentityCache { &self.identity }
    pub fn capabilities(&self)->&CapabilityCache { &self.capabilities }
    pub fn calibration(&self)->&[u8;128] { &self.calibration }
    pub fn secondary_calibration(&self)->&[u8;128] { &self.secondary_calibration }
    pub fn version(&self)->[u8;2] { [self.configuration[0x53],self.configuration[0x54]] }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compatibility { Current, Unidentified, Older, Newer, Invalid }

impl ControlDevice {
    pub fn classify_calibration(&mut self)->Compatibility {
        let data=&self.calibration;
        let mut compatibility=if data[8..16].iter().all(|byte|*byte==255) {
            if self.setup.is_some_and(|request|request.mode==SetupMode::Primary) { Compatibility::Invalid } else { Compatibility::Unidentified }
        } else if data[9]!=b'L' || !matches!(data[10],b'S'|b'Z'|b'P') || !matches!(data[11],b'N'|b'G'|b'S') || !matches!(data[12],b'A'|b'F'|b'W') {
            Compatibility::Invalid
        } else {
            let version=[data[0x62],data[0x63]];
            let current=self.version();
            if version>current { Compatibility::Newer } else if version<current { Compatibility::Older } else { Compatibility::Current }
        };
        if matches!(compatibility,Compatibility::Current|Compatibility::Older|Compatibility::Newer) {
            self.restricted=data[0x64]&1!=0;
            let protocol=if compatibility==Compatibility::Newer { Some(4) } else {
                match (data[0x62],data[0x63]) {
                    (1,0|1|0x10)|(2,1|0x10|0x12|0x13)=>Some(4),
                    (2,0|0x20|0x21|0x30|0x31|0x32|0x40|0x50)=>Some(1),
                    _=>None,
                }
            };
            if let Some(protocol)=protocol {
                let model=match &data[..8] {
                    b"LX201B\0\0"=>0,b"LX202A\0\0"=>1,b"LX204A\0\0"=>2,b"LX210A\0\0"=>3,
                    b"LX206A\0\0"=>4,b"LX207A\0\0"=>5,b"LX208A\0\0"=>6,b"LX211A\0\0"=>7,
                    b"LX209A\0\0"=>8,b"LX212A\0\0"=>9,b"LX213A\0\0"=>10,b"LX215B\0\0"=>11,
                    b"LX216A\0\0"=>12,b"LX221A\0\0"=>13,_=>14,
                };
                let identifier=u16::from(data[0x62]>>4)*1000+u16::from(data[0x62]&15)*100+u16::from(data[0x63]>>4)*10+u16::from(data[0x63]&15);
                let focus=if data[0x64]&2!=0 { 0 } else if data[0x64]&4!=0 { 2 } else { 1 };
                let stabilization=match data[11] { b'N'=>0,b'G'=>if data[13]&3==0 { 1 } else { 2 },_=>self.capabilities.stabilization };
                self.capabilities=CapabilityCache { model,identifier,protocol,upgrade:u8::from(compatibility==Compatibility::Newer),focus,focus_copy:focus,stabilization,
                    special:data[13]&4!=0,features:(data[0x64]>>4)&3,extended:identifier>109,flag:data[0x64]&8!=0 };
                if compatibility==Compatibility::Older && (self.capabilities.upgrade==0 || (self.version()==[2,0x20] && identifier==110 && model!=1) || matches!(data[8],b'c'|b'C')) {
                    compatibility=Compatibility::Current;
                }
            } else { compatibility=Compatibility::Invalid; }
        }
        self.identity.model.copy_from_slice(&self.calibration[8..16]);
        self.identity.descriptor.copy_from_slice(&self.calibration[16..24]);
        self.identity.trailing.copy_from_slice(&self.calibration[0x50..0x5a]);
        compatibility
    }

    pub fn detached_present(&self,ctx:&ControlContext,io:&mut impl ControlIo)->bool {
        ctx.state==ControlState::Detached && io.read_mmio(MmioAddress(0xfff89014))&0x80==0
    }

    pub fn initialize(&mut self,ctx:&mut ControlContext,request:SetupRequest,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
        ctx.state=ControlState::Initializing;
        if !ctx.connected {
            if let Some(callback)=request.completion { io.complete(callback,CompletionStatus(-10),&[]); }
            self.initialization_error=true;
            return Ok(());
        }
        self.setup=Some(request);
        ctx.setup=Some(request);
        ctx.initialization_callback=request.completion;
        ctx.polling=request.mode==SetupMode::Primary;
        ctx.polling_supported=request.mode==SetupMode::Primary;
        ctx.owner=request.owner;
        ctx.capability.0=u32::from(request.mode==SetupMode::Secondary);
        self.suppress_queries=false;
        ctx.last_status=0;
        ctx.wait=WaitRegistration::Unregistered;
        ctx.readiness.reset();
        ctx.status=CompletionStatus(-12);
        ctx.attached=true;
        ctx.periodic[13].modes=0;
        ctx.periodic[11].modes=0;
        self.cache.fill(0);
        self.change_pending=false;
        self.identity=IdentityCache::EMPTY;
        if !self.warm_start {
            io.write_mmio(MmioAddress(0xfff89080),0x40004000);
            io.update_mmio(MmioAddress(0xfff89358),0xc00,0x400);
            io.update_mmio(MmioAddress(0xfff89358),0x30000,0x10000);
            if request.owner.0!=0 || ctx.capability.0!=0 {
                io.write_mmio(MmioAddress(0xfff89080),0x10000000);
                io.write_mmio(MmioAddress(0xfff89090),0x03000000);
                io.write_mmio(MmioAddress(0xfff89084),0x10001000);
            }
            io.delay_ticks(TickCount(19));
            let inputs=io.read_mmio(MmioAddress(0xfff89014));
            let mut faults=(inputs>>4)&1;
            if io.read_mmio(MmioAddress(0xfff89014))&0x80==0 { faults|=2; }
            if ctx.capability.0==0 {
                let level=io.read_mmio(MmioAddress(0xfff89094))&0x1000!=0;
                if level==(request.owner.0!=0) { faults|=4; }
            }
            if io.read_mmio(MmioAddress(0xfff89354))&0x100==0 { faults|=8; }
            if io.read_mmio(MmioAddress(0xfff89354))&4!=0 { faults|=16; }
            if io.read_mmio(MmioAddress(0xfff89354))&0x20==0 { faults|=32; }
            if faults!=0 {
                let mut alternate=(io.read_mmio(MmioAddress(0xfff89014))>>4)&1;
                if ctx.capability.0==0 && io.read_mmio(MmioAddress(0xfff89094))&0x1000==0 { alternate|=4; }
                if io.read_mmio(MmioAddress(0xfff89354))&0x100!=0 { alternate|=8; }
                if io.read_mmio(MmioAddress(0xfff89354))&4==0 { alternate|=16; }
                if io.read_mmio(MmioAddress(0xfff89354))&0x20==0 { alternate|=32; }
                if alternate==0 {
                    io.write_mmio(MmioAddress(0xfff89080),0x80008000);
                    reset_detached(ctx,io,queue);
                    self.reset_detached_cache();
                    self.finish_initialization(ctx,DeviceStatus::Detached,io,queue)?;
                    ctx.state=ControlState::Detached;
                } else {
                    self.initialization_error=true;
                    self.initialization_failures=self.initialization_failures.wrapping_add(1);
                    if self.initialization_failures>2 { publish_status(ctx,DeviceStatus::Connection,io); }
                    self.finish_initialization(ctx,DeviceStatus::Detection,io,queue)?;
                }
                return Ok(());
            }
            self.initialization_failures=0;
            io.write_mmio(MmioAddress(0xfff89080),0x80008000);
            io.write_mmio(MmioAddress(0xfff8900c),0xc0004000);
            if ctx.capability.0==0 { Self::configure_detect_pin(io); }
            for (address,value) in [(0xfff89350,0x30001),(0xfff89344,0x01000100),(0xfff8934c,0x00300010),(0xfff89344,0x00040004),(0xfff8934c,0x0c000400),(0xfff89344,0x00200000)] {
                io.write_mmio(MmioAddress(address),value);
            }
        }
        queue.initialize(DeviceCallback::Initialized,DeviceCallback::Observer)
    }

    fn configure_detect_pin(io:&mut impl ControlIo) {
        io.write_mmio(MmioAddress(0xfff89080),0x10001000);
        io.write_mmio(MmioAddress(0xfff89090),0x03000000);
        io.write_mmio(MmioAddress(0xfff89084),0x10001000);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationProfile { pub identity:[u8;8],pub trailing:[u8;10],pub lens_version:[u8;2],pub body_version:[u8;2] }
impl CalibrationProfile {
    pub fn parse(bytes:&[u8;128])->Self {
        Self { identity:bytes[0x18..0x20].try_into().unwrap(),trailing:bytes[0x20..0x2a].try_into().unwrap(),
            lens_version:bytes[0x32..0x34].try_into().unwrap(),body_version:bytes[0x50..0x52].try_into().unwrap() }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyPublication { pub key:KeyEvent,pub argument:u32,pub data:[u8;68] }

impl ControlDevice {
    pub fn compatibility_supported(&self,profiles:&[CalibrationProfile])->bool {
        let body=[self.configuration[0x4d],self.configuration[0x4e]];
        let lens=[self.calibration[0x5a],self.calibration[0x5b]];
        let matches=|profile:&CalibrationProfile|profile.identity==self.calibration[8..16] && profile.trailing==self.calibration[0x50..0x5a] && profile.lens_version==lens;
        profiles.iter().find(|profile|matches(profile) && profile.body_version==body).is_some()
            && profiles.iter().any(|profile|profile.identity==self.calibration[8..16] && profile.trailing==self.calibration[0x50..0x5a] && profile.lens_version==lens)
    }

    fn publish_focus_key(&self,io:&mut impl ControlIo) {
        let position=u16::from_le_bytes([self.cache[14],self.cache[15]])&0xfff;
        match self.capabilities.focus {
            1 if position<=31=>io.key_release(KeyEvent(0x400+u32::from(position))),
            2 if position!=0=>{
                let signed=((position as i16)<<4)>>4;
                let mut data=[0;68];
                data[..2].copy_from_slice(&signed.wrapping_neg().to_le_bytes());
                io.publish_key(KeyPublication { key:KeyEvent(0x424),argument:0,data });
            }
            _=>{}
        }
    }

    fn post_response(kind:ResponseKind,status:CompletionStatus,data:&[u8],io:&mut impl ControlIo) {
        let mut response=ControlResponse { kind,status,data:[0;16],page:None };
        let length=data.len().min(16);
        response.data[..length].copy_from_slice(&data[..length]);
        let _=io.post(ControlMessage { opcode:ControlOpcode::Response,payload:ControlPayload::Response(response) });
    }

    pub fn finish_initialization(&mut self,ctx:&mut ControlContext,status:DeviceStatus,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
        let compatible=self.compatibility_supported(io.calibration_profiles());
        if status!=DeviceStatus::Compatibility || !compatible { publish_status(ctx,status,io); }
        let completion=match status {
            DeviceStatus::Complete|DeviceStatus::Compatibility|DeviceStatus::Capability=>{
                io.key_release(KeyEvent(0x420));
                ctx.status=CompletionStatus::SUCCESS;
                ctx.attached=true;
                if ctx.polling {
                    if ctx.poll_timer.is_none() {
                        let _=io.post(ControlMessage { opcode:ControlOpcode::Poll,payload:ControlPayload::Poll });
                        ctx.poll_timer=Some(io.start_timer(TickCount(40),true,ControlTimer::Poll));
                        if self.capabilities.model>2 { Self::set_periodic(ctx,13); }
                        if self.capabilities.flag { Self::set_periodic(ctx,11); }
                    }
                    ctx.state=ControlState::Active;
                } else { ctx.state=ControlState::Suspended; }
                ctx.public_status=2;
                CompletionStatus::SUCCESS
            }
            DeviceStatus::Restricted|DeviceStatus::Detached=>{
                io.key_release(KeyEvent(0x420));
                if !ctx.polling_supported {
                    ctx.status=CompletionStatus::SUCCESS;
                    if status==DeviceStatus::Restricted { ctx.public_status=2; }
                } else if status==DeviceStatus::Restricted && !self.restricted {
                    ctx.restricted_delays=500;
                    self.continue_restricted_initialization(ctx,io,queue);
                    return Ok(());
                } else {
                    ctx.polling=false;
                    ctx.polling_supported=false;
                    ctx.status=CompletionStatus::REJECTED;
                    if status==DeviceStatus::Restricted { ctx.public_status=0; }
                    queue.stop(DeviceCallback::Teardown)?;
                    if let Some(callback)=ctx.periodic[2].callback.take() { io.complete(callback,CompletionStatus::SUCCESS,&[]); }
                    ctx.first_response_received=false;
                    if let Some(callback)=ctx.first_response_callback.take() { io.complete(callback,CompletionStatus::SUCCESS,&[]); }
                }
                ctx.attached=true;
                ctx.state=ControlState::Suspended;
                if status==DeviceStatus::Detached { ctx.public_status=2; }
                CompletionStatus(-40)
            }
            _=>{ stop(ctx,io,queue); CompletionStatus(-10) }
        };
        if let Some(callback)=ctx.initialization_callback.take() { io.complete(callback,completion,&[]); }
        Ok(())
    }
    pub fn restricted_delay_elapsed(&mut self,ctx:&mut ControlContext,io:&mut impl ControlIo,queue:&mut impl ControlQueue) {
        if ctx.restricted_timer.take().is_none() { return; }
        ctx.restricted_delays-=1;
        self.continue_restricted_initialization(ctx,io,queue);
    }

    fn continue_restricted_initialization(&mut self,ctx:&mut ControlContext,io:&mut impl ControlIo,queue:&mut impl ControlQueue) {
        if ctx.restricted_delays!=0 && !ctx.readiness.is_ready() {
            ctx.restricted_timer=Some(io.start_timer(TickCount(10),false,ControlTimer::RestrictedWait));
            return;
        }
        ctx.restricted_delays=0;
        io.key_release(KeyEvent(0xb8));
        let completion=ctx.initialization_callback.take();
        stop(ctx,io,queue);
        if let Some(callback)=completion { io.complete(callback,CompletionStatus::SUCCESS,&[]); }
    }


    fn set_periodic(ctx:&mut ControlContext,index:usize) {
        ctx.periodic[index].interval=40;
        ctx.periodic[index].remaining=40;
        ctx.periodic[index].modes=10;
        ctx.periodic[index].callback=None;
    }

    pub fn initialization_finished(&mut self,ctx:&mut ControlContext,success:bool,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
        if success {
            ctx.readiness.publish();
            if let WaitRegistration::Waiting(target)=ctx.wait { ctx.wait=WaitRegistration::Completed; io.notify(target); }
            publish_status(ctx,DeviceStatus::Failure,io);
            publish_status(ctx,DeviceStatus::Ready,io);
            if ctx.restricted_timer.is_some() { self.continue_restricted_initialization(ctx,io,queue); }
            else { stop(ctx,io,queue); }
        } else {
            publish_status(ctx,DeviceStatus::Connection,io);
            self.finish_initialization(ctx,DeviceStatus::Failure,io,queue)?;
        }
        self.invalid=true;
        Ok(())
    }

    pub fn ready(&mut self,ctx:&mut ControlContext,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
        if ctx.capability.0!=0 {
            Self::configure_detect_pin(io);
            if ctx.state==ControlState::Initializing {
                queue.submit(QueuePacket::query(QueueOperation::BeginDetection,DeviceCallback::Response(ResponseKind::Reply17)))?;
            }
        }
        if !ctx.readiness.is_ready() {
            ctx.readiness.publish();
            if ctx.restricted_timer.is_none() && !self.restricted && self.capability_flags[2]!=0 && ctx.status==CompletionStatus::REJECTED {
                io.key_release(KeyEvent(0xb8));
                if self.capability_flags[0]==1 { publish_status(ctx,DeviceStatus::Capability,io); }
                else if self.capability_flags[1]==1 && !self.compatibility_supported(io.calibration_profiles()) { publish_status(ctx,DeviceStatus::Compatibility,io); }
                stop(ctx,io,queue);
            }
        }
        if let WaitRegistration::Waiting(target)=ctx.wait { ctx.wait=WaitRegistration::Completed; io.notify(target); }
        if ctx.restricted_timer.is_some() { self.continue_restricted_initialization(ctx,io,queue); }
        Ok(())
    }

    pub fn init_response(&mut self,ctx:&mut ControlContext,response:ControlResponse,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
        let ok=response.status==CompletionStatus::SUCCESS;
        match response.kind.code() as u32 {
            13=>{
                if self.warm_start {
                    self.warm_start=false;
                    if io.read_mmio(MmioAddress(0xfffb3040))&0x8000!=0 {
                        return self.callback(ctx,DeviceCallback::DetectionOff,response.status,&[],io,queue).map(|_|());
                    }
                }
                crate::control_actor::detection_output(false,Some(DeviceCallback::DetectionOff),io);
                ctx.initialization_timer=Some(io.start_timer(TickCount(if self.setup.is_some_and(|setup|setup.polling) { 5000 } else { 550 }),false,ControlTimer::InitializationTimeoutOff));
            }
            17 if ok=>queue.submit(QueuePacket::selector(QueueOperation::SelectConfiguration,DeviceCallback::Response(ResponseKind::Reply18)))?,
            18 if ok=>{ let _=paged_read(response,true,DeviceCallback::Response(ResponseKind::Reply19),&mut self.calibration,io,queue); }
            19=>{
                let result=paged_read(response,false,DeviceCallback::Response(ResponseKind::Reply19),&mut self.calibration,io,queue);
                if result==CompletionStatus::SUCCESS {
                    if self.calibration[8..16].iter().any(|byte|*byte!=255) && self.calibration[0x65]&2!=0 {
                        if ok { queue.submit(QueuePacket::selector(QueueOperation::SelectSecondary,DeviceCallback::Response(ResponseKind::Reply20)))?; }
                        else { self.finish_initialization(ctx,DeviceStatus::Failure,io,queue)?; }
                    } else { Self::post_response(ResponseKind::Reply22, CompletionStatus::SUCCESS,&[],io); }
                } else if result!=CompletionStatus(-12) { self.finish_initialization(ctx,DeviceStatus::Failure,io,queue)?; }
            }
            20 if ok=>{ let _=paged_read(response,true,DeviceCallback::Response(ResponseKind::Reply21),&mut self.secondary_calibration,io,queue); }
            21=>{
                let result=paged_read(response,false,DeviceCallback::Response(ResponseKind::Reply21),&mut self.secondary_calibration,io,queue);
                if result==CompletionStatus::SUCCESS { Self::post_response(ResponseKind::Reply22, CompletionStatus::SUCCESS,&[],io); }
                else if result!=CompletionStatus(-12) { self.finish_initialization(ctx,DeviceStatus::Failure,io,queue)?; }
            }
            22=>{
                let compatibility=self.classify_calibration();
                self.publish_focus_key(io);
                match compatibility {
                    Compatibility::Current|Compatibility::Unidentified=>{
                        if self.capabilities.features==1 {
                            queue.submit(QueuePacket::selected_query(QueueOperation::Query32,DeviceCallback::Configuration,[0,0,1]))?;
                        }
                        self.finish_initialization(ctx,DeviceStatus::Complete,io,queue)?;
                    }
                    Compatibility::Newer=>{
                        queue.submit(QueuePacket::selector(QueueOperation::SelectCapability,DeviceCallback::Response(ResponseKind::Reply23)))?;
                        self.initialization_error=true;
                    }
                    Compatibility::Older=>{
                        if self.capabilities.protocol==2 {
                            if self.restricted { queue.submit(QueuePacket::query(QueueOperation::Restrict,DeviceCallback::Response(ResponseKind::Reply26)))?; }
                            else {
                                self.capability_flags[2]=1;
                                if !self.compatibility_supported(io.calibration_profiles()) { publish_status(ctx,DeviceStatus::Compatibility,io); }
                                self.finish_initialization(ctx,DeviceStatus::Restricted,io,queue)?;
                            }
                        } else { self.finish_initialization(ctx,DeviceStatus::Compatibility,io,queue)?; }
                        self.initialization_error=true;
                    }
                    Compatibility::Invalid=>{
                        self.invalid=true;
                        publish_status(ctx,DeviceStatus::Failure,io);
                        self.finish_initialization(ctx,DeviceStatus::Invalid,io,queue)?;
                    }
                }
            }
            23 if ok=>{
                if capability_write(response,true,&self.configuration,queue)!=CompletionStatus(-12) { self.finish_initialization(ctx,DeviceStatus::Failure,io,queue)?; }
            }
            24=>{
                let result=capability_write(response,false,&self.configuration,queue);
                if result==CompletionStatus::SUCCESS { queue.submit(QueuePacket::query(QueueOperation::Capability,DeviceCallback::Response(ResponseKind::Reply25)))?; }
                else if result!=CompletionStatus(-12) { self.finish_initialization(ctx,DeviceStatus::Failure,io,queue)?; }
            }
            25 if ok=>{
                self.capability_flags.copy_from_slice(&response.data[..3]);
                let status=if self.capability_flags[2]!=0 {
                    if self.capability_flags[0]==1 { publish_status(ctx,DeviceStatus::Capability,io); }
                    else if self.capability_flags[1]==1 && !self.compatibility_supported(io.calibration_profiles()) { publish_status(ctx,DeviceStatus::Compatibility,io); }
                    self.initialization_error=true;
                    DeviceStatus::Restricted
                } else if self.capability_flags[0]==1 { DeviceStatus::Capability }
                else if self.capability_flags[1]==1 && !matches!(self.calibration[8],b'c'|b'C') { DeviceStatus::Compatibility }
                else { DeviceStatus::Complete };
                self.finish_initialization(ctx,status,io,queue)?;
            }
            26=>{
                if !self.compatibility_supported(io.calibration_profiles()) { publish_status(ctx,DeviceStatus::Compatibility,io); }
                self.finish_initialization(ctx,DeviceStatus::Restricted,io,queue)?;
            }
            17|18|20|23|25|27=>self.finish_initialization(ctx,DeviceStatus::Failure,io,queue)?,
            _=>{}
        }
        Ok(())
    }
}

impl ControlDevice {
    pub fn response(&mut self,ctx:&mut ControlContext,response:ControlResponse,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
        if response.status!=CompletionStatus::SUCCESS {
            ctx.public_status=0;
            ctx.status=CompletionStatus::REJECTED;
            for (destination,source) in self.errors.iter_mut().zip(response.data) { *destination|=source; }
            if response.status==CompletionStatus(1) || (response.status==CompletionStatus(4) && response.data[6]==1) {
                publish_status(ctx,DeviceStatus::Failure,io);
            }
            if response.status==CompletionStatus(4) && response.data[8]==1 {
                if let Some(callback)=ctx.power_callback { io.complete(callback,CompletionStatus(4),&[]); }
            }
            stop(ctx,io,queue);
            return Ok(());
        }
        let mut notify=[false;33];
        match response.kind.code() as u32 {
            1=>{
                ctx.in_flight[0]=false;
                for index in 1..=14 {
                    let slot=if index==9 && self.capabilities.special && self.version()[0]>1 && self.version()[1]>79 { 15 } else { index };
                    let offset=14-index;
                    let selected=ctx.periodic[slot].select(self.cache[offset]!=0,response.data[offset]!=0);
                    ctx.periodic[slot].argument=u32::from(selected);
                }
                let selected=ctx.periodic[32].select(self.cache[11]!=0,response.data[11]!=0);
                ctx.periodic[32].argument=u32::from(selected);
                if self.force_query5 { ctx.periodic[5].argument=1; self.force_query5=false; }
                if ctx.periodic[1].argument==1 && !self.suppress_queries {
                    queue.submit(QueuePacket::selected_query(QueueOperation::Status,DeviceCallback::Response(ResponseKind::Reply02),[0,0,1]))?;
                    ctx.in_flight[1]=true;
                }
                if ctx.periodic[2].argument==1 && !self.suppress_queries {
                    queue.submit(QueuePacket::selected_query(QueueOperation::Query2,DeviceCallback::Response(ResponseKind::Reply04),[0,0,1]))?;
                    ctx.in_flight[2]=true;
                }
                if ctx.periodic[3].argument==1 { notify[3]=true; ctx.last_status=0; }
                if ctx.periodic[32].argument==1 && !self.suppress_queries {
                    queue.submit(QueuePacket::selected_query(QueueOperation::Query32,DeviceCallback::Response(ResponseKind::Reply05),[0,0,1]))?;
                    ctx.in_flight[32]=true;
                }
                if ctx.periodic[4].argument==1 {
                    if self.capabilities.extended {
                        if response.data[10]!=0 {
                            queue.submit(QueuePacket::query(QueueOperation::Query4,DeviceCallback::Response(ResponseKind::Reply03)))?;
                            ctx.in_flight[4]=true;
                        }
                    } else { notify[4]=true; }
                }
                if ctx.periodic[5].argument==1 {
                    queue.submit(QueuePacket::query(QueueOperation::Query5,DeviceCallback::Response(ResponseKind::Reply09)))?;
                    ctx.in_flight[5]=true;
                }
                if ctx.periodic[6].argument==1 {
                    if !self.suppress_queries {
                        queue.submit(QueuePacket::selected_query(QueueOperation::Query6,DeviceCallback::Response(ResponseKind::Reply06),[0,0,1]))?;
                        ctx.in_flight[6]=true;
                    }
                    self.change_pending=false;
                } else if self.change_pending {
                    if ctx.periodic[6].modes==0 { self.change_pending=false; }
                    else if ctx.periodic[6].remaining==ctx.periodic[6].interval { notify[6]=true; self.change_pending=false; }
                }
                for index in [7,8,9,10,14,15] {
                    if ctx.periodic[index].argument==1 { notify[index]=true; }
                }
                if ctx.periodic[11].argument==1 {
                    queue.submit(QueuePacket::query(QueueOperation::ExtendedStatus,DeviceCallback::Response(ResponseKind::Reply10)))?;
                    ctx.in_flight[11]=true;
                }
                if ctx.periodic[13].argument==1 && !self.suppress_queries {
                    queue.submit(QueuePacket::query(QueueOperation::Query13,DeviceCallback::Response(ResponseKind::Reply11)))?;
                    ctx.in_flight[13]=true;
                }
            }
            2=>{ ctx.in_flight[1]=false; notify[1]=true; }
            3=>{ ctx.in_flight[4]=false; notify[4]=true; }
            4=>{ ctx.in_flight[2]=false; notify[2]=true; }
            5=>{ ctx.in_flight[32]=false; notify[32]=true; }
            6=>{ ctx.in_flight[6]=false; notify[6]=true; }
            7=>{ ctx.in_flight[31]=false; notify[31]=true; }
            9=>{ ctx.in_flight[5]=false; notify[5]=true; }
            10=>{
                for index in 16..=30 {
                    let offset=if index<=26 { index-11 } else { 30-index };
                    let selected=ctx.periodic[index].select(self.cache[16+offset]!=0,response.data[offset]!=0);
                    ctx.periodic[index].argument=u32::from(selected);
                }
                ctx.in_flight[11]=false;
            }
            11=>{ ctx.in_flight[13]=false; notify[13]=true; }
            _=>{}
        }
        let destination=match response.kind.code() as u32 {
            1=>Some((0,14)),2=>Some((32,2)),3=>Some((34,2)),4=>Some((36,4)),
            5=>Some((40,4)),6=>Some((44,3)),7=>Some((56,2)),8=>Some((58,2)),
            9=>Some((14,2)),10=>Some((16,16)),11=>Some((47,9)),_=>None,
        };
        if let Some((offset,length))=destination { self.cache[offset..offset+length].copy_from_slice(&response.data[..length]); }
        if response.kind.code() as u32==1 {
            ctx.first_response_received=true;
            if let Some(callback)=ctx.first_response_callback.take() { io.complete(callback,CompletionStatus::SUCCESS,&[]); }
        }
        for (index,notify) in notify.into_iter().enumerate() {
            if notify {
                let callback=if index==2 { ctx.periodic[index].callback.take() } else { ctx.periodic[index].callback };
                if let Some(callback)=callback { io.complete(callback,CompletionStatus::SUCCESS,&[]); }
            }
        }
        if ctx.in_flight.iter().all(|pending|!*pending) {
            if let Some(callback)=ctx.synchronize_callback.take() { io.complete(callback,CompletionStatus::SUCCESS,&[]); }
        }
        Ok(())
    }

    pub fn callback(&mut self,ctx:&mut ControlContext,callback:DeviceCallback,status:CompletionStatus,data:&[u8],io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<CallbackAcceptance,QueueFailure> {
        match callback {
            DeviceCallback::Initialized=>Self::post_response(ResponseKind::Reply13, CompletionStatus::SUCCESS,&[],io),
            DeviceCallback::Observer=>Self::post_response(ResponseKind::Reply27, status,&data[..data.len().min(9)],io),
            DeviceCallback::Response(kind)=>{
                let length=match kind.code() as u32 { 1=>14,2|3|7|8|9|24=>2,4|5=>4,6|25=>3,10=>16,11|27=>9,19|21=>8,_=>0 };
                Self::post_response(kind,status,&data[..data.len().min(length)],io);
                if matches!(kind,ResponseKind::Reply19|ResponseKind::Reply21|ResponseKind::Reply24) {
                    return Ok(if status!=CompletionStatus::SUCCESS { CallbackAcceptance::Error }
                        else if data.len()>=2 && u16::from_le_bytes([data[0],data[1]])==if kind.code() as u32==24 { 0xff00 } else { 0xffff } { CallbackAcceptance::Finished }
                        else { CallbackAcceptance::Continue });
                }
            }
            DeviceCallback::DetectionOff|DeviceCallback::DetectionOn=>{
                let active=callback==DeviceCallback::DetectionOn;
                crate::control_actor::detection_output(active,None,io);
                if let Some(timer)=ctx.initialization_timer.take() { io.cancel_timer(timer); }
                let _=io.post(ControlMessage { opcode:if active { ControlOpcode::Ready } else { ControlOpcode::BeginDetection },payload:ControlPayload::Empty });
            }
            DeviceCallback::Poll=>{ let _=io.post(ControlMessage { opcode:ControlOpcode::Poll,payload:ControlPayload::Poll }); }
            DeviceCallback::Teardown=>{ let _=io.post(ControlMessage { opcode:ControlOpcode::TeardownComplete,payload:ControlPayload::Empty }); }
            DeviceCallback::ModeResume|DeviceCallback::ModeSuspend=>{
                let _=io.post(ControlMessage { opcode:ControlOpcode::ModeComplete,payload:ControlPayload::ModeComplete(if callback==DeviceCallback::ModeResume { crate::control_types::ModeCompletion::Resume } else { crate::control_types::ModeCompletion::Suspend }) });
            }
            DeviceCallback::Configuration=>self.configuration_complete(data),
            DeviceCallback::CapabilityPage=>{
                Self::post_response(ResponseKind::Reply24, status,&data[..data.len().min(2)],io);
                return Ok(if status!=CompletionStatus::SUCCESS { CallbackAcceptance::Error } else if data.len()>=2 && data[..2]==[0,255] { CallbackAcceptance::Finished } else { CallbackAcceptance::Continue });
            }
            DeviceCallback::Snapshot(kind)=>self.receive_snapshot(ctx,kind,status,data,io),
        }
        let _=queue;
        Ok(CallbackAcceptance::Continue)
    }

    pub fn initialization_timeout(&mut self,ctx:&mut ControlContext,active:bool,io:&mut impl ControlIo,queue:&mut impl ControlQueue)->Result<(),QueueFailure> {
        if self.setup.is_some_and(|setup|setup.polling) {
            if active {
                if let Some(timer)=ctx.initialization_timer.take() { io.cancel_timer(timer); }
                let _=io.post(ControlMessage { opcode:ControlOpcode::Ready,payload:ControlPayload::Empty });
            } else { self.callback(ctx,DeviceCallback::DetectionOff,CompletionStatus::SUCCESS,&[],io,queue)?; }
        } else {
            ctx.initialization_timer=None;
            if !active { crate::control_actor::detection_output(false,None,io); }
            let _=io.post(ControlMessage { opcode:ControlOpcode::InitializationFinished,payload:ControlPayload::InitializationFinished(active) });
        }
        Ok(())
    }

    pub fn detection_completed(&mut self,ctx:&mut ControlContext,io:&mut impl ControlIo) {
        crate::control_actor::set_detection_hook(false,io);
        let _=io.post(ControlMessage { opcode:ControlOpcode::Detached,payload:ControlPayload::Empty });
        ctx.detection_enabled=true;
    }

    pub fn sample_detection(&mut self,ctx:&mut ControlContext,io:&mut impl ControlIo) {
        if !ctx.detection_enabled { return; }
        if io.read_mmio(MmioAddress(0xfff89014))&0x10!=0 { ctx.detection_samples=0; return; }
        ctx.detection_samples=ctx.detection_samples.wrapping_add(1);
        if ctx.detection_samples<4 { return; }
        io.write_mmio(MmioAddress(0xfff8900c),0xc0004000);
        ctx.detection_enabled=false;
        ctx.detection_samples=0;
        let _=io.post(ControlMessage { opcode:ControlOpcode::Attached,payload:ControlPayload::Empty });
        crate::control_actor::set_detection_hook(true,io);
    }

    fn receive_snapshot(&mut self,ctx:&mut ControlContext,kind:ObserverKind,status:CompletionStatus,data:&[u8],io:&mut impl ControlIo) {
        let (slot,offset,length)=Self::snapshot_range(kind);
        if status==CompletionStatus::SUCCESS && data.len()>=length {
            if kind==ObserverKind::ThreeBytes && self.cache[45]!=data[1] && ctx.periodic[6].modes&10!=0 { self.change_pending=true; }
            self.cache[offset..offset+length].copy_from_slice(&data[..length]);
        }
        if let Some(callback)=self.snapshot_callbacks[slot].take() { io.complete(callback,status,&self.cache[offset..offset+length]); }
    }

    const fn snapshot_range(kind:ObserverKind)->(usize,usize,usize) {
        match kind { ObserverKind::TwoBytes=>(0,56,2),ObserverKind::ThreeBytes=>(1,44,3),ObserverKind::NineBytes=>(2,47,9) }
    }
    pub fn register_snapshot(&mut self,kind:ObserverKind,callback:CallbackToken)->Result<(),crate::control_types::ControlError> {
        let slot=&mut self.snapshot_callbacks[Self::snapshot_range(kind).0];
        if slot.is_some() { return Err(crate::control_types::ControlError::Busy); }
        *slot=Some(callback);
        Ok(())
    }
    pub fn request_snapshot(&mut self,kind:ObserverKind,callback:CallbackToken,queue:&mut impl ControlQueue)->Result<(),crate::control_types::ControlError> {
        self.register_snapshot(kind,callback)?;
        let completion=DeviceCallback::Snapshot(kind);
        let result=match kind {
            ObserverKind::TwoBytes=>queue.read_two(completion),
            ObserverKind::ThreeBytes=>queue.submit(QueuePacket::selected_query(QueueOperation::Query6,completion,[0,0,1])),
            ObserverKind::NineBytes=>queue.submit(QueuePacket::query(QueueOperation::Query13,completion)),
        };
        if result.is_err() { self.snapshot_callbacks[Self::snapshot_range(kind).0]=None; }
        result.map_err(|_|crate::control_types::ControlError::Queue)
    }
    pub fn cache_snapshot(&self,kind:ObserverKind)->&[u8] {
        let (_,offset,length)=Self::snapshot_range(kind);
        &self.cache[offset..offset+length]
    }
    pub fn configuration_complete(&mut self,data:&[u8]) {
        if let Some(data)=data.get(..4) { self.cache[40..44].copy_from_slice(data); }
    }
    pub fn force_next_query5(&mut self) { self.force_query5=true; }
    pub fn set_query_suppression(&mut self,suppressed:bool) { self.suppress_queries=suppressed; }
    pub fn errors(&self)->&[u8;9] { &self.errors }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackAcceptance { Continue, Finished, Error }

impl ControlDevice {
    pub fn reset_detached_cache(&mut self) { self.cache[..48].fill(0); }

    pub fn callback_page(kind:ResponseKind,status:CompletionStatus,marker:u16,page:Option<crate::control_types::CalibrationPage>,io:&mut impl ControlIo)->CallbackAcceptance {
        let mut data=[0;16];
        data[..2].copy_from_slice(&marker.to_le_bytes());
        let response=ControlResponse { kind,status,data,page };
        let _=io.post(ControlMessage { opcode:ControlOpcode::Response,payload:ControlPayload::Response(response) });
        if status!=CompletionStatus::SUCCESS { CallbackAcceptance::Error }
        else if marker==if kind==ResponseKind::Reply24 { 0xff00 } else { 0xffff } { CallbackAcceptance::Finished }
        else { CallbackAcceptance::Continue }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::control_types::{CalibrationPage,ControlError,ControlPower,ControlProfiles,ControlServices,DetectionCompletion,DetectionEdge,DetectionMode,KeyPublicationSink,PeriodicEntry,SharedReadiness,TimerToken,WaitTarget};
    use std::vec::Vec;

    #[derive(Default)]
    struct Output {
        messages:Vec<ControlMessage>,completions:Vec<(CallbackToken,CompletionStatus,Vec<u8>)>,keys:Vec<KeyEvent>,
        timers:Vec<(TickCount,bool,ControlTimer)>,
    }
    impl ControlServices for Output {
        fn delay_ticks(&mut self,_:TickCount) { panic!("unexpected delay") }
        fn post(&mut self,message:ControlMessage)->Result<(),ControlError> { self.messages.push(message); Ok(()) }
        fn complete(&mut self,callback:CallbackToken,status:CompletionStatus,data:&[u8]) { self.completions.push((callback,status,data.to_vec())); }
        fn notify(&mut self,_:WaitTarget) { panic!("unexpected notification") }
        fn key_release(&mut self,key:KeyEvent) { self.keys.push(key); }
        fn diagnostic(&mut self,_:u32,_:u32) {}
        fn start_timer(&mut self,ticks:TickCount,repeating:bool,kind:ControlTimer)->TimerToken { self.timers.push((ticks,repeating,kind)); TimerToken(self.timers.len() as u32) }
        fn cancel_timer(&mut self,_:TimerToken) {}
        fn set_detection_callback(&mut self,_:DetectionEdge,_:bool) {}
        fn reset_gpio(&mut self) { panic!("unexpected GPIO reset") }
    }
    impl ControlProfiles for Output { fn calibration_profiles(&self)->&[CalibrationProfile] { &[] } }
    impl ControlPower for Output {
        fn set_detection_mode(&mut self,_:DetectionMode) {}
        fn detection_output(&mut self,_:DetectionMode,_:Option<DetectionCompletion>) { panic!("unexpected detection output") }
    }
    impl KeyPublicationSink for Output { fn publish_key(&mut self,_:KeyPublication) { panic!("unexpected key data") } }
    impl ControlIo for Output {
        fn read_mmio(&mut self,_:MmioAddress)->u32 { panic!("unexpected register read") }
        fn write_mmio(&mut self,_:MmioAddress,_:u32) {}
        fn update_mmio(&mut self,_:MmioAddress,_:u32,_:u32) {}
    }
    #[derive(Default)]
    struct Packets { packets:Vec<QueuePacket>,pages:Vec<(bool,DeviceCallback)>,reject:bool,stops:usize }
    impl ControlQueue for Packets {
        fn initialize(&mut self,_:DeviceCallback,_:DeviceCallback)->Result<(),QueueFailure> { panic!("unexpected initialization") }
        fn submit(&mut self,packet:QueuePacket)->Result<(),QueueFailure> {
            if self.reject { return Err(QueueFailure::Full); }
            self.packets.push(packet); Ok(())
        }
        fn stop(&mut self,_:DeviceCallback)->Result<(),QueueFailure> { self.stops+=1; Ok(()) }
        fn write_configuration(&mut self,_:QueuePage,_:&[u8;128],_:DeviceCallback)->Result<(),QueueFailure> { panic!("unexpected configuration write") }
        fn accept_page(&mut self,accept:bool,callback:DeviceCallback)->Result<(),QueueFailure> { self.pages.push((accept,callback)); Ok(()) }
        fn finish(&mut self,_:crate::control_types::LensModelIdentifier)->Result<(),QueueFailure> { panic!("unexpected finish") }
        fn resume(&mut self,_:DeviceCallback)->Result<(),QueueFailure> { panic!("unexpected resume") }
        fn suspend(&mut self,_:DeviceCallback)->Result<(),QueueFailure> { panic!("unexpected suspend") }
        fn poll(&mut self,_:DeviceCallback)->Result<(),QueueFailure> { panic!("unexpected poll") }
        fn read_two(&mut self,_:DeviceCallback)->Result<(),QueueFailure> { panic!("unexpected observer query") }
    }
    fn reply(kind:ResponseKind,data:[u8;16])->ControlResponse { ControlResponse { kind,status:CompletionStatus::SUCCESS,data,page:None } }

    #[test]
    fn restricted_initialization_completes_after_live_readiness_publication() {
        let readiness=SharedReadiness::new();
        let mut context=ControlContext::new(&readiness);
        context.polling_supported=true;
        context.initialization_callback=Some(CallbackToken(41));
        let mut device=ControlDevice::new([0;128],false);
        let mut output=Output::default();
        let mut queue=Packets::default();
        device.finish_initialization(&mut context,DeviceStatus::Restricted,&mut output,&mut queue).unwrap();
        assert!(output.completions.is_empty());
        assert_eq!(output.timers,[(TickCount(10),false,ControlTimer::RestrictedWait)]);
        readiness.publish();
        device.restricted_delay_elapsed(&mut context,&mut output,&mut queue);
        assert_eq!(output.completions,[(CallbackToken(41),CompletionStatus::SUCCESS,Vec::new())]);
        assert_eq!(queue.stops,1);
        assert!(output.keys.contains(&KeyEvent(0xb8)));
    }

    #[test]
    fn restricted_initialization_expires_after_five_thousand_ticks() {
        let readiness=SharedReadiness::new();
        let mut context=ControlContext::new(&readiness);
        context.polling_supported=true;
        context.initialization_callback=Some(CallbackToken(42));
        let mut device=ControlDevice::new([0;128],false);
        let mut output=Output::default();
        let mut queue=Packets::default();
        device.finish_initialization(&mut context,DeviceStatus::Restricted,&mut output,&mut queue).unwrap();
        for _ in 0..499 { device.restricted_delay_elapsed(&mut context,&mut output,&mut queue); }
        assert!(output.completions.is_empty());
        device.restricted_delay_elapsed(&mut context,&mut output,&mut queue);
        assert_eq!(output.completions,[(CallbackToken(42),CompletionStatus::SUCCESS,Vec::new())]);
        assert_eq!(output.timers.iter().map(|(ticks,_,_)|ticks.0).sum::<u32>(),5000);
        assert!(!readiness.is_ready());
    }

    #[test]
    fn initialization_finished_completes_pending_restricted_setup_once() {
        let readiness=SharedReadiness::new();
        let mut context=ControlContext::new(&readiness);
        context.polling_supported=true;
        context.initialization_callback=Some(CallbackToken(43));
        let mut device=ControlDevice::new([0;128],false);
        let mut output=Output::default();
        let mut queue=Packets::default();
        device.finish_initialization(&mut context,DeviceStatus::Restricted,&mut output,&mut queue).unwrap();
        device.initialization_finished(&mut context,true,&mut output,&mut queue).unwrap();
        device.restricted_delay_elapsed(&mut context,&mut output,&mut queue);
        assert_eq!(output.completions,[(CallbackToken(43),CompletionStatus::SUCCESS,Vec::new())]);
        assert_eq!(queue.stops,1);
        assert_eq!(output.keys.iter().filter(|key|**key==KeyEvent(0xb8)).count(),1);
        assert!(readiness.is_ready());
    }

    #[test]
    fn periodic_completion_targets_the_configured_entry() {
        let readiness=SharedReadiness::new();
        let mut ctx=ControlContext::new(&readiness);
        let mut device=ControlDevice::new([0;128],false);
        let mut io=Output::default();
        let mut queue=Packets::default();
        ctx.periodic[2]=PeriodicEntry { interval:1,remaining:0,modes:1,callback:Some(CallbackToken(77)),selected:0,argument:0 };
        device.response(&mut ctx,reply(ResponseKind::Reply01,[0;16]),&mut io,&mut queue).unwrap();
        assert_eq!(ctx.periodic[2].argument,1);
        assert_eq!(queue.packets.len(),1);
        device.response(&mut ctx,reply(ResponseKind::Reply04,[0;16]),&mut io,&mut queue).unwrap();
        assert_eq!(io.completions,[(CallbackToken(77),CompletionStatus::SUCCESS,Vec::new())]);
        assert_eq!(ctx.periodic[2].callback,None);
    }

    #[test]
    fn selectors_publish_actual_command_words() {
        let primary=QueuePacket::selector(QueueOperation::SelectConfiguration,DeviceCallback::Response(ResponseKind::Reply18));
        assert_eq!(&primary.bytes()[4..12],&[0x00,0x06,0x01,0xb8,0x00,0x05,0x00,0x00]);
        let secondary=QueuePacket::selector(QueueOperation::SelectSecondary,DeviceCallback::Response(ResponseKind::Reply20));
        assert_eq!(&secondary.bytes()[8..12],&[0x80,0x25,0x00,0x00]);
        assert_eq!(secondary.bytes()[44],2);
        let query=QueuePacket::selected_query(QueueOperation::Query6,DeviceCallback::Response(ResponseKind::Reply06),[0,0,1]);
        assert_eq!(&query.bytes()[4..8],&[0x00,0x09,0x01,0x00]);
    }

    #[test]
    fn page_delivery_preserves_calibration_and_derives_capabilities() {
        let mut config=[0;128]; config[0x53]=2; config[0x54]=0x50;
        let mut device=ControlDevice::new(config,false);
        let readiness=SharedReadiness::new();
        let mut ctx=ControlContext::new(&readiness);
        let mut io=Output::default();
        let mut queue=Packets::default();
        let mut bytes=[0;128];
        bytes[..8].copy_from_slice(b"LX204A\0\0");
        bytes[8..16].copy_from_slice(b"cLSGF123");
        bytes[16..24].copy_from_slice(b"identity");
        bytes[0x50..0x5a].copy_from_slice(b"0123456789");
        bytes[0x62]=2; bytes[0x63]=0x50; bytes[0x64]=0x2c;
        let disposition=ControlDevice::callback_page(ResponseKind::Reply19,CompletionStatus::SUCCESS,0,Some(CalibrationPage(bytes)),&mut io);
        assert_eq!(disposition,CallbackAcceptance::Continue);
        let ControlPayload::Response(page)=io.messages.remove(0).payload else { panic!("expected page") };
        device.init_response(&mut ctx,page,&mut io,&mut queue).unwrap();
        assert_eq!(device.calibration(),&bytes);
        assert_eq!(queue.pages,[(true,DeviceCallback::Response(ResponseKind::Reply19))]);
        let mut end=reply(ResponseKind::Reply19,[0;16]); end.data[..2].copy_from_slice(&0xffffu16.to_le_bytes());
        device.init_response(&mut ctx,end,&mut io,&mut queue).unwrap();
        let ControlPayload::Response(classify)=io.messages.remove(0).payload else { panic!("expected classification") };
        device.init_response(&mut ctx,classify,&mut io,&mut queue).unwrap();
        assert_eq!(device.identity().descriptor,*b"identity");
        assert_eq!(device.capabilities().model,2);
        assert_eq!(device.capabilities().identifier,250);
        assert_eq!(device.capabilities().focus,2);
        assert!(device.capabilities().extended);
        assert_eq!(ctx.status,CompletionStatus::SUCCESS);
    }

    #[test]
    fn responses_update_public_cache_before_completing_observers() {
        let mut device=ControlDevice::new([0;128],false);
        let readiness=SharedReadiness::new();
        let mut ctx=ControlContext::new(&readiness);
        let mut io=Output::default();
        let mut queue=Packets::default();
        let data=[1,2,3,4,5,6,7,8,9,10,11,12,13,14,0,0];
        device.response(&mut ctx,reply(ResponseKind::Reply01,data),&mut io,&mut queue).unwrap();
        assert_eq!(&device.cache()[..14],&data[..14]);
        device.register_snapshot(ObserverKind::ThreeBytes,CallbackToken(71)).unwrap();
        assert_eq!(device.register_snapshot(ObserverKind::ThreeBytes,CallbackToken(72)),Err(ControlError::Busy));
        device.callback(&mut ctx,DeviceCallback::Snapshot(ObserverKind::ThreeBytes),CompletionStatus::SUCCESS,&[8,7,6],&mut io,&mut queue).unwrap();
        assert_eq!(device.cache_snapshot(ObserverKind::ThreeBytes),[8,7,6]);
        assert_eq!(io.completions,[(CallbackToken(71),CompletionStatus::SUCCESS,Vec::from([8,7,6]))]);
        device.register_snapshot(ObserverKind::ThreeBytes,CallbackToken(73)).unwrap();
        device.callback(&mut ctx,DeviceCallback::Snapshot(ObserverKind::ThreeBytes),CompletionStatus(4),&[0,0,0],&mut io,&mut queue).unwrap();
        assert_eq!(io.completions.last(),Some(&(CallbackToken(73),CompletionStatus(4),Vec::from([8,7,6]))));
    }
    #[test]
    fn failed_snapshot_admission_releases_the_callback_registration() {
        let mut device=ControlDevice::new([0;128],false);
        let readiness=SharedReadiness::new();
        let mut context=ControlContext::new(&readiness);
        let mut output=Output::default();
        let mut queue=Packets { reject:true,..Packets::default() };
        assert_eq!(device.request_snapshot(ObserverKind::ThreeBytes,CallbackToken(71),&mut queue),Err(ControlError::Queue));
        queue.reject=false;
        assert_eq!(device.request_snapshot(ObserverKind::ThreeBytes,CallbackToken(72),&mut queue),Ok(()));
        device.callback(&mut context,DeviceCallback::Snapshot(ObserverKind::ThreeBytes),CompletionStatus::SUCCESS,&[1,2,3],&mut output,&mut queue).unwrap();
        assert_eq!(output.completions,[(CallbackToken(72),CompletionStatus::SUCCESS,Vec::from([1,2,3]))]);
    }

    #[test]
    fn attachment_publishes_connection_without_fabricating_readiness() {
        let readiness=SharedReadiness::new();
        let mut actor=crate::control_actor::ControlActor::new([0;128],false,&readiness);
        let mut output=Output::default();
        let mut queue=Packets::default();
        assert!(!actor.connected());
        actor.dispatch(ControlMessage { opcode:ControlOpcode::Attached,payload:ControlPayload::Empty },&mut output,&mut queue).unwrap();
        assert!(actor.connected());
        assert!(!actor.readiness());
        assert_eq!(output.keys,[KeyEvent(0x38)]);
    }
}
