//! Task 99 domain state recovered from X-M1 1.50 image 4.

use core::sync::atomic::{AtomicBool,Ordering};

#[derive(Debug)]
pub struct SharedReadiness(AtomicBool);
impl SharedReadiness {
    pub const fn new()->Self { Self(AtomicBool::new(false)) }
    pub fn is_ready(&self)->bool { self.0.load(Ordering::Acquire) }
    pub fn publish(&self) { self.0.store(true,Ordering::Release); }
    pub fn reset(&self) { self.0.store(false,Ordering::Release); }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ControlState { Idle = 0, Initializing = 1, Active = 2, Stopping = 3, Suspended = 4, Detached = 5 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ControlOpcode {
    Register = 0x7300, Setup = 0x7301, Teardown = 0x7302, Synchronize = 0x7303,
    StartPolling = 0x7304, BeginDetection = 0x7305, Ready = 0x7306, InitializationFinished = 0x7307,
    Resume = 0x7308, Suspend = 0x7309, Finish = 0x730a, Response = 0x730b,
    Attached = 0x730c, Detached = 0x730d, Poll = 0x730e, ModeComplete = 0x730f, TeardownComplete = 0x7310,
}
impl TryFrom<u16> for ControlOpcode {
    type Error = ControlError;
    fn try_from(value:u16)->Result<Self,Self::Error> {
        Ok(match value {
            0x7300=>Self::Register,0x7301=>Self::Setup,0x7302=>Self::Teardown,0x7303=>Self::Synchronize,
            0x7304=>Self::StartPolling,0x7305=>Self::BeginDetection,0x7306=>Self::Ready,0x7307=>Self::InitializationFinished,
            0x7308=>Self::Resume,0x7309=>Self::Suspend,0x730a=>Self::Finish,0x730b=>Self::Response,
            0x730c=>Self::Attached,0x730d=>Self::Detached,0x730e=>Self::Poll,0x730f=>Self::ModeComplete,0x7310=>Self::TeardownComplete,
            _=>return Err(ControlError::UnknownOpcode),
        })
    }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ControlError { UnknownOpcode, InvalidPayload, Busy, Queue, Constructor }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct CallbackToken(pub u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ControlOwner(pub u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Capability(pub u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct LensModelIdentifier(pub u16);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct FirmwareVersion(pub u16);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum SnapshotData { TwoBytes([u8;2]),ThreeBytes([u8;3]),NineBytes([u8;9]) }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct SnapshotResult { pub status:CompletionStatus,pub data:SnapshotData }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct CompletionStatus(pub i32);
impl CompletionStatus {
    pub const SUCCESS:Self=Self(0);
    pub const REJECTED:Self=Self(-255);
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum SetupMode { Primary, Secondary, Other }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct SetupRequest { pub completion:Option<CallbackToken>,pub polling:bool,pub mode:SetupMode,pub owner:ControlOwner }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
#[repr(u8)]
pub enum ResponseKind {
    Reply01=1,Reply02=2,Reply03=3,Reply04=4,Reply05=5,Reply06=6,Reply07=7,Reply08=8,Reply09=9,
    Reply10=10,Reply11=11,Reply12=12,Reply13=13,Reply14=14,Reply15=15,Reply16=16,Reply17=17,
    Reply18=18,Reply19=19,Reply20=20,Reply21=21,Reply22=22,Reply23=23,Reply24=24,Reply25=25,Reply26=26,Reply27=27,
}
impl ResponseKind {
    pub const fn code(self)->u8 { self as u8 }
}
impl TryFrom<u32> for ResponseKind {
    type Error=ControlError;
    fn try_from(value:u32)->Result<Self,Self::Error> { Ok(match value {
        1=>Self::Reply01,2=>Self::Reply02,3=>Self::Reply03,4=>Self::Reply04,5=>Self::Reply05,6=>Self::Reply06,
        7=>Self::Reply07,8=>Self::Reply08,9=>Self::Reply09,10=>Self::Reply10,11=>Self::Reply11,12=>Self::Reply12,
        13=>Self::Reply13,14=>Self::Reply14,15=>Self::Reply15,16=>Self::Reply16,17=>Self::Reply17,18=>Self::Reply18,
        19=>Self::Reply19,20=>Self::Reply20,21=>Self::Reply21,22=>Self::Reply22,23=>Self::Reply23,24=>Self::Reply24,
        25=>Self::Reply25,26=>Self::Reply26,27=>Self::Reply27,_=>return Err(ControlError::InvalidPayload),
    }) }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ControlResponse { pub kind:ResponseKind,pub status:CompletionStatus,pub data:[u8;16],pub page:Option<CalibrationPage> }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct CalibrationPage(pub [u8;128]);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ControlPayload {
    Register { enabled:bool,completion:Option<CallbackToken> }, Setup(SetupRequest), Callback(Option<CallbackToken>),
    Response(ControlResponse), ModeComplete(ModeCompletion), InitializationFinished(bool), Poll, Timer { kind:ControlTimer,token:TimerToken }, Empty,
    ResetLens,CancelPeriodic(crate::control_device::PeriodicIndex),
    ConfigurePeriodic { index:crate::control_device::PeriodicIndex,ticks:TickCount,modes:PeriodicModes,callback:Option<CallbackToken> },
    Snapshot { kind:ObserverKind,callback:CallbackToken },
    DeviceCompletion { callback:crate::control_device::DeviceCallback,status:CompletionStatus,data:[u8;16] },
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ModeCompletion { Resume, Suspend }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ControlMessage { pub opcode:ControlOpcode,pub payload:ControlPayload }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct PeriodicEntry { pub interval:u16,pub remaining:u16,pub modes:u32,pub callback:Option<CallbackToken>,pub selected:u32,pub argument:u32 }
impl PeriodicEntry {
    pub const EMPTY:Self=Self { interval:0,remaining:0,modes:0,callback:None,selected:0,argument:0 };
    pub fn select(&mut self,first:bool,second:bool)->bool {
        if self.modes==0 || self.remaining!=0 { return false; }
        self.remaining=self.interval;
        let (mask,selected)=match (first,second) { (false,false)=>(0xaaaa,1),(false,true)=>(0xff00,2),(true,false)=>(0xf0f0,4),(true,true)=>(0xcccc,8) };
        if self.modes<16 && (mask & (1u32<<self.modes))!=0 { self.selected=selected; true } else { false }
    }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct WaitTarget { pub task:u8,pub bits:u32 }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum WaitRegistration { Unregistered, Waiting(WaitTarget), Completed }
#[derive(Debug)]
pub struct ControlContext<'a> {
    pub state:ControlState,pub readiness:&'a SharedReadiness,pub status:CompletionStatus,pub public_status:u32,
    pub connected:bool,pub attached:bool,pub capability:Capability,pub polling:bool,pub polling_supported:bool,
    pub periodic:[PeriodicEntry;33],pub in_flight:[bool;33],
    pub initialization_callback:Option<CallbackToken>,pub status_callback:Option<CallbackToken>,
    pub first_response_callback:Option<CallbackToken>,
    pub first_response_received:bool,pub resume_callback:Option<CallbackToken>,pub suspend_callback:Option<CallbackToken>,
    pub synchronize_callback:Option<CallbackToken>,pub teardown_callback:Option<CallbackToken>,pub mode_resumed:bool,
    pub wait:WaitRegistration,pub owner:ControlOwner,pub setup:Option<SetupRequest>,
    pub poll_timer:Option<TimerToken>,pub initialization_timer:Option<TimerToken>,pub detection_timer:Option<TimerToken>,
    pub restricted_timer:Option<TimerToken>,pub restricted_delays:u16,
    pub registered:bool,pub detection_enabled:bool,pub detection_samples:u32,pub warm_start:bool,pub gpio_preserved:bool,
    pub last_status:u16,
}
impl<'a> ControlContext<'a> {
    pub fn new(readiness:&'a SharedReadiness)->Self { Self {
        state:ControlState::Idle,readiness,status:CompletionStatus::REJECTED,public_status:0,
        connected:false,attached:false,capability:Capability(0),polling:false,polling_supported:false,
        periodic:[PeriodicEntry::EMPTY;33],in_flight:[false;33],
        initialization_callback:None,status_callback:None,first_response_callback:None,
        first_response_received:false,resume_callback:None,suspend_callback:None,synchronize_callback:None,teardown_callback:None,
        mode_resumed:false,wait:WaitRegistration::Unregistered,owner:ControlOwner(0),setup:None,
        poll_timer:None,initialization_timer:None,detection_timer:None,registered:false,detection_enabled:false,
        restricted_timer:None,restricted_delays:0,
        detection_samples:0,warm_start:false,gpio_preserved:false,last_status:0,
    } }
}

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct MmioAddress(pub u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct TimerToken(pub u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct TickCount(pub u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct KeyEvent(pub u32);
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ControlTimer { Poll, Detection, InitializationTimeoutOff, InitializationTimeoutOn, RestrictedWait }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DetectionEdge {
    Attach,
    Detach,
    /// The mode 0x006c23d8 passes to the detection callback entry 0x006b8284.
    Probe = 2,
}

pub trait ControlIo: ControlServices + ControlPower + ControlProfiles + KeyPublicationSink {
    fn read_mmio(&mut self,address:MmioAddress)->u32;
    fn write_mmio(&mut self,address:MmioAddress,value:u32);
    fn update_mmio(&mut self,address:MmioAddress,clear:u32,set:u32);
}

pub trait ControlServices {
    fn delay_ticks(&mut self,ticks:TickCount);
    fn post(&mut self,message:ControlMessage)->Result<(),ControlError>;
    fn complete(&mut self,callback:CallbackToken,status:CompletionStatus,data:&[u8]);
    fn notify(&mut self,target:WaitTarget);
    fn key_release(&mut self,event:KeyEvent);
    fn diagnostic(&mut self,category:u32,event:u32);
    fn start_timer(&mut self,ticks:TickCount,repeating:bool,callback:ControlTimer)->TimerToken;
    fn cancel_timer(&mut self,timer:TimerToken);
    fn set_detection_callback(&mut self,edge:DetectionEdge,enabled:bool);
    fn reset_gpio(&mut self);
}
pub trait ControlProfiles {
    fn calibration_profiles(&self)->&[crate::control_device::CalibrationProfile];
}
pub trait ControlPower {
    fn set_detection_mode(&mut self,mode:DetectionMode);
    fn detection_output(&mut self,mode:DetectionMode,completion:Option<DetectionCompletion>);
}
pub trait KeyPublicationSink {
    fn publish_key(&mut self,publication:crate::control_device::KeyPublication);
}

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ModeStatus { Unavailable, Resuming, Suspended }

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ObserverKind { TwoBytes, ThreeBytes, NineBytes }

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DetectionMode { Off, On, Probe }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DetectionCompletion { Off, On, Probe }

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ConfigurationOffset(pub u32);
pub trait ConfigurationSource {
    fn configuration_byte(&self,offset:ConfigurationOffset)->u8;
}

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct PeriodicModes(pub(crate) u8);
impl PeriodicModes {
    pub const fn new(value:u8)->Option<Self> { if value<16 { Some(Self(value)) } else { None } }
}
