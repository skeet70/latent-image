use crate::{session::{ControlAction,ControlFrame,ControlStatus,Session,StatusDelivery,Tick},transport::PublishedRegion};

#[repr(C)]
pub struct ControlObservation {
    pub delivery:u32,
    pub status:u32,
    pub action:u32,
    pub active:u32,
    pub sid:u32,
    pub requested:u32,
    pub armed:u32,
    pub ready:u32,
    pub payload_offset:u32,
    pub payload_length:u32,
    pub reply_length:u32,
    pub reply:[u8;512],
}

impl ControlObservation {
    pub const fn new()->Self {
        Self {delivery:0,status:0,action:0,active:0,sid:0,requested:0,armed:0,ready:0,
            payload_offset:0,payload_length:0,reply_length:0,reply:[0;512]}
    }
}
impl Default for ControlObservation {fn default()->Self {Self::new()}}

pub fn observe(session:&mut Session<'_>,frame:&[u8],tick:Tick,region:Option<PublishedRegion>,reply:&[u8],output:&mut ControlObservation)->Result<(),crate::session::FrameError> {
    let parsed=ControlFrame::parse(frame)?;
    let result=session.control(parsed,tick,region)?;
    *output=ControlObservation::new();
    (output.delivery,output.status)=match result.status {
        StatusDelivery::Reject(status)=>(1,status as u32),
        StatusDelivery::Accept(status)=>(2,status as u32),
        StatusDelivery::Deferred=>(0,0),
    };
    output.action=match result.action {
        ControlAction::None=>0,ControlAction::ReceiveCommand{..}=>1,
        ControlAction::ReceivePayload{..}=>2,ControlAction::SendReply=>3,
        ControlAction::SendPayload(_)=>4,
    };
    output.active=u32::from(session.active());
    output.sid=session.id().map_or(0,|sid|u32::from(sid.get()));
    output.requested=session.requested_length();
    output.armed=u32::from(session.armed());
    output.ready=u32::from(session.ready());
    if let Some(span)=session.payload_span() {
        output.payload_offset=span.offset();output.payload_length=span.length();
    }
    if matches!(result.action,ControlAction::SendReply) && session.armed() && session.ready() {
        let bytes=session.take_reply(reply)?;
        let length=bytes.len();
        output.reply[..length].copy_from_slice(&bytes[..length]);
        output.reply_length=length as u32;
    }
    Ok(())
}

#[repr(C)]
pub struct SessionContext<'a> { session:Session<'a> }

impl<'a> SessionContext<'a> {pub fn new(buffer:&'a mut[u8;256])->Self {Self{session:Session::new(buffer)}}}

#[no_mangle]
pub extern "C" fn vendor_session_context_size() -> usize {
    core::mem::size_of::<SessionContext<'static>>()
}

#[no_mangle]
pub extern "C" fn vendor_session_context_alignment() -> usize {
    core::mem::align_of::<SessionContext<'static>>()
}

/// # Safety
/// Both pointers are exclusive, aligned storage. The buffer outlives the initialized context.
#[no_mangle]
pub unsafe extern "C" fn vendor_session_initialize(context:*mut SessionContext<'static>,buffer:*mut[u8;256]) {
    context.write(SessionContext::new(&mut *buffer));
}

/// # Safety
/// Context is initialized and exclusive. Frame names twelve readable bytes; output is initialized, exclusive aligned storage, disjoint from both.
/// Output is published only when the return value is zero.
#[no_mangle]
pub unsafe extern "C" fn vendor_session_control(context:*mut SessionContext<'_>,frame:*const u8,tick:u32,base:u32,length:u32,output:*mut ControlObservation)->u32 {
    let region=if length==0 {None}else {match PublishedRegion::new(base,length) {Ok(region)=>Some(region),Err(_)=>return ControlStatus::InvalidLength as u32}};
    let frame=core::slice::from_raw_parts(frame,12);
    match observe(&mut (*context).session,frame,Tick(tick),region,&[],&mut *output) {
        Ok(())=>0,
        Err(crate::session::FrameError::Truncated)=>ControlStatus::UnknownCommand as u32,
        Err(crate::session::FrameError::ReservedFlags)=>ControlStatus::InvalidFlags as u32,
        Err(_)=>ControlStatus::InvalidLength as u32,
    }
}
