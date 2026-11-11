#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(u32)]
pub enum PanelCommand { Command0,Command1,Command2,Command3,Command4,Command5,Command6 }
impl PanelCommand {
    pub const fn parse(value:u32)->Option<Self>{match value{0=>Some(Self::Command0),1=>Some(Self::Command1),2=>Some(Self::Command2),3=>Some(Self::Command3),4=>Some(Self::Command4),5=>Some(Self::Command5),6=>Some(Self::Command6),_=>None}}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Completion(core::num::NonZeroU32);
impl Completion {
    pub fn parse(value:u32)->Option<Self>{core::num::NonZeroU32::new(value).map(Self)}
    pub const fn address(self)->u32{self.0.get()}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TaskId(pub u8);
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct QueueStatus(pub i32);
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(u32)]
pub enum QueueId { LcdEvf=0x22 }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PanelRequest {pub sender:TaskId,pub command:PanelCommand,pub completion:Option<Completion>}
pub trait Transport {
    fn current_task(&mut self)->TaskId;
    fn post(&mut self,request:PanelRequest)->QueueStatus;
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SendError {Submission}
impl SendError {pub const fn stock(self)->i32{-255}}
pub fn send(command:PanelCommand,completion:Option<Completion>,transport:&mut impl Transport)->Result<(),SendError>{
    let sender=transport.current_task();
    let status=transport.post(PanelRequest{sender,command,completion});
    if status.0==0{Ok(())}else{Err(SendError::Submission)}
}

#[cfg(all(target_arch="arm",feature="panel"))]
pub(crate) unsafe fn configuration_byte(offset:calibration_liveview::CalibrationOffset)->calibration_liveview::CalibrationByte {
    let pointer=core::ptr::read_volatile(0x96ca50 as *const *mut u8);
    let cache=calibration_liveview::InitializedCalibrationBase::from_initialized_base(core::ptr::NonNull::new_unchecked(pointer));
    cache.read_byte(offset)
}
#[cfg(all(target_arch="arm",feature="panel"))]
pub(crate) unsafe fn native_reset_enabled()->bool {
    configuration_byte(calibration_liveview::CalibrationOffset::parse(0xfb2).unwrap()).0!=0
}

#[cfg(all(target_arch="arm",feature="panel"))]
#[repr(C)]
pub(crate) struct ServiceDescriptor {
    pub(crate) operation:u16,
    pub(crate) sender:u8,
    pub(crate) destination:u8,
    pub(crate) length:u32,
    pub(crate) payload:u32,
}

#[cfg(all(target_arch="arm",feature="panel"))]
mod abi {
    use super::*;
    #[repr(C)]
    struct Payload {command:u32,completion:u32}
    extern "C" {
        fn xm1_power_current_task()->u32;
        fn xm1_power_service_post(queue:u32,descriptor:*const ServiceDescriptor)->i32;
    }
    struct OwnedTransport;
    impl Transport for OwnedTransport {
        fn current_task(&mut self)->TaskId{TaskId(unsafe{xm1_power_current_task()} as u8)}
        fn post(&mut self,request:PanelRequest)->QueueStatus{
            let payload=Payload{command:request.command as u32,completion:request.completion.map_or(0,Completion::address)};
            let descriptor=ServiceDescriptor{operation:0x3200,sender:request.sender.0,destination:QueueId::LcdEvf as u8,length:8,payload:(&payload as *const Payload) as u32};
            QueueStatus(unsafe{xm1_power_service_post(QueueId::LcdEvf as u32,&descriptor)})
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_command(command:u32,completion:u32)->i32{
        let Some(command)=PanelCommand::parse(command)else{return -30;};
        send(command,Completion::parse(completion),&mut OwnedTransport).map_or_else(SendError::stock,|()|0)
    }
}
#[cfg(all(target_arch="arm",feature="panel"))]
pub use abi::xm1_scanout_panel_command;
