use crate::transport::{PublishedRegion, RegionSpan};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tick(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionId(u16);

impl SessionId {
    pub const fn get(self) -> u16 { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ControlStatus {
    Success = 0,
    UnknownCommand = 0x20,
    InvalidLength = 0x21,
    InvalidSession = 0x24,
    InvalidFlags = 0x25,
    Reset = 0x29,
    AlreadyActive = 0x80,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusDelivery {
    Reject(ControlStatus),
    Accept(ControlStatus),
    Deferred,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlAction {
    None,
    ReceiveCommand { length: u32 },
    ReceivePayload { length: u32 },
    SendReply,
    SendPayload(RegionSpan),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlOutcome {
    pub status: StatusDelivery,
    pub action: ControlAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    Truncated,
    ReservedFlags,
    MissingPublishedRegion,
    InvalidPublishedSpan,
    ReplyCapacity,
    ReplyNotReady,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Opcode {
    Inquiry,
    Reset,
    Open,
    Validate,
    Close,
    Command,
    Reply,
    Upload,
    Download,
    Empty,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlFrame {
    opcode: Opcode,
    flags: u8,
    inquiry_length: u8,
    offset: u32,
    length: u32,
    token: u16,
}

impl ControlFrame {
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let bytes = bytes.get(..12).ok_or(FrameError::Truncated)?;
        if bytes[1] & 0xe0 != 0 { return Err(FrameError::ReservedFlags); }
        let opcode = match bytes[0] {
            0x03 => Opcode::Inquiry, 0x1d => Opcode::Reset,
            0xc0 => Opcode::Open, 0xc2 => Opcode::Validate,
            0xc4 => Opcode::Close, 0xd0 => Opcode::Command,
            0xd1 => Opcode::Reply, 0xe0 => Opcode::Upload,
            0xe1 => Opcode::Download, 0xe8 => Opcode::Empty,
            _ => Opcode::Unknown,
        };
        Ok(Self {
            opcode, flags: bytes[1], inquiry_length: bytes[4],
            offset: u32::from_be_bytes([bytes[2],bytes[3],bytes[4],bytes[5]]),
            length: u32::from_be_bytes([bytes[6],bytes[7],bytes[8],bytes[9]]),
            token: u16::from_be_bytes([bytes[10],bytes[11]]),
        })
    }
    pub const fn opcode(self) -> Opcode { self.opcode }
}

pub struct Session<'a> {
    active: bool,
    sid: u16,
    requested: u32,
    available: usize,
    armed: bool,
    ready: bool,
    payload_span: Option<RegionSpan>,
    receiving: Option<u32>,
    control_reply: &'a mut [u8;256],
    source: ReplySource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplySource { Control, Application }


impl<'a> Session<'a> {
    pub fn new(control_reply:&'a mut[u8;256]) -> Self {
        Self { active:false,sid:0,requested:0,available:0,armed:false,ready:false,
            payload_span:None,receiving:None,control_reply,source:ReplySource::Control }
    }
    pub const fn active(&self) -> bool { self.active }
    pub const fn reply_source(&self)->ReplySource{self.source}
    pub const fn available_length(&self)->usize{self.available}
    pub fn id(&self) -> Option<SessionId> {
        if self.sid == 0 { None } else { Some(SessionId(self.sid)) }
    }
    pub const fn requested_length(&self) -> u32 { self.requested }
    pub const fn armed(&self) -> bool { self.armed }
    pub const fn ready(&self) -> bool { self.ready }
    pub const fn payload_span(&self) -> Option<RegionSpan> { self.payload_span }
    pub const fn receiving_length(&self) -> Option<u32> { self.receiving }

    /// Stock 0x00d7f2cc and 0x00d7f334: clears the block at 0x01469864, the active byte 0x01469898 and the payload flag 0x01469c9c. The identifier seed 0x0146989a persists.
    pub fn begin_connection(&mut self) {
        self.active = false;
        self.requested = 0;
        self.available = 0;
        self.armed = false;
        self.ready = false;
        self.payload_span = None;
        self.receiving = None;
        self.source = ReplySource::Control;
    }

    pub fn synchronize_reply_history(&mut self, application_reply: &[u8;512]) {
        if self.source == ReplySource::Application {
            self.control_reply.copy_from_slice(&application_reply[..256]);
        }
    }

    pub fn begin_reply_completion(&self, application_reply: &mut [u8;512]) {
        if self.source == ReplySource::Control {
            application_reply[..256].copy_from_slice(self.control_reply);
        }
    }

    pub fn finish_reply_completion(&mut self, application_reply: &[u8;512], ready: bool) {
        if self.source == ReplySource::Control {
            self.control_reply.copy_from_slice(&application_reply[..256]);
        }
        self.ready = ready;
    }

    fn prepared(&mut self, requested:u32, available:usize) {
        self.requested=requested;
        self.available=available;
        self.armed=true;
        self.ready=true;
        self.source=ReplySource::Control;
    }
    fn outcome(status: StatusDelivery, action: ControlAction) -> ControlOutcome {
        ControlOutcome {status,action}
    }
    fn reject(status:ControlStatus) -> ControlOutcome {
        Self::outcome(StatusDelivery::Reject(status),ControlAction::None)
    }
    fn authenticated(&self, token:u16) -> bool { self.active && self.sid == token }

    pub fn control(&mut self, frame:ControlFrame, tick:Tick, region:Option<PublishedRegion>) -> Result<ControlOutcome,FrameError> {
        self.payload_span=None;
        let success=StatusDelivery::Accept(ControlStatus::Success);
        let action=match frame.opcode {
            Opcode::Open => {
                let status=if self.active { ControlStatus::AlreadyActive }
                    else if frame.token != 0 { ControlStatus::InvalidSession }
                    else if frame.length != 8 { ControlStatus::InvalidLength }
                    else {
                        if self.sid == 0 { self.sid=tick.0 as u16; }
                        self.sid=self.sid.wrapping_add(1)&0x1fff;
                        if self.sid == 0 { self.sid=1; }
                        self.active=true;
                        ControlStatus::Success
                    };
                let token=if status==ControlStatus::Success {self.sid} else {0};
                self.control_reply[..8].copy_from_slice(&[token as u8,(token>>8)as u8,0,0,0,0,1,0]);
                self.prepared(frame.length,8);
                return Ok(Self::outcome(StatusDelivery::Accept(status),ControlAction::SendReply));
            }
            Opcode::Validate => {
                let status=if !self.active {ControlStatus::InvalidSession}
                    else if frame.length != 2 {ControlStatus::InvalidLength}
                    else if frame.token != self.sid {ControlStatus::InvalidSession}
                    else {ControlStatus::Success};
                let token=if status==ControlStatus::Success {self.sid} else {0};
                self.control_reply[..2].copy_from_slice(&token.to_le_bytes());
                self.prepared(frame.length,2);
                return Ok(Self::outcome(StatusDelivery::Accept(status),ControlAction::SendReply));
            }
            Opcode::Close => {
                let status=if frame.token != self.sid {ControlStatus::InvalidSession}
                    else if frame.length != 2 {ControlStatus::InvalidLength}
                    else {self.active=false;ControlStatus::Success};
                let token=if status==ControlStatus::Success {self.sid} else {0};
                self.control_reply[..2].copy_from_slice(&token.to_le_bytes());
                self.prepared(frame.length,2);
                return Ok(Self::outcome(StatusDelivery::Accept(status),ControlAction::SendReply));
            }
            Opcode::Command | Opcode::Upload => {
                if !self.authenticated(frame.token) {return Ok(Self::reject(ControlStatus::InvalidSession));}
                if frame.opcode==Opcode::Command && frame.length>512 {return Ok(Self::reject(ControlStatus::InvalidLength));}
                self.receiving=Some(frame.length);
                self.armed=false;
                self.ready=false;
                if frame.opcode==Opcode::Command {ControlAction::ReceiveCommand{length:frame.length}}
                else {ControlAction::ReceivePayload{length:frame.length}}
            }
            Opcode::Reply | Opcode::Empty | Opcode::Download => {
                if !self.authenticated(frame.token) {return Ok(Self::reject(ControlStatus::InvalidSession));}
                if frame.length==0 {return Ok(Self::reject(ControlStatus::InvalidLength));}
                match frame.opcode {
                    Opcode::Reply => {
                        self.requested=frame.length;
                        self.armed=true;
                        return Ok(Self::outcome(StatusDelivery::Deferred,ControlAction::SendReply));
                    }
                    Opcode::Empty => {
                        self.control_reply[..4].fill(0);
                        self.prepared(frame.length,4);
                        ControlAction::SendReply
                    }
                    _ => {
                        let region=region.ok_or(FrameError::MissingPublishedRegion)?;
                        let span=region.span(frame.offset,frame.length).map_err(|_|FrameError::InvalidPublishedSpan)?;
                        self.requested=frame.length;
                        self.payload_span=Some(span);
                        ControlAction::SendPayload(span)
                    }
                }
            }
            Opcode::Reset => {
                if frame.flags&0x10 != 0 {return Ok(Self::reject(ControlStatus::InvalidSession));}
                self.active=false;
                self.armed=false;
                self.ready=false;
                return Ok(Self::reject(ControlStatus::Reset));
            }
            Opcode::Inquiry => {
                let len=frame.inquiry_length as usize;
                if len!=0 {self.control_reply[len]=0;}
                self.control_reply[0]=0x70;
                self.control_reply[2]=0;
                self.control_reply[7]=10;
                self.control_reply[12]=0;
                self.control_reply[13]=0;
                self.control_reply[15]=0;
                self.prepared(len as u32,len);
                ControlAction::SendReply
            }
            Opcode::Unknown => return Ok(Self::reject(ControlStatus::UnknownCommand)),
        };
        Ok(Self::outcome(success,action))
    }

    pub fn publish_reply(&mut self, length:usize, ready:bool) -> Result<(),FrameError> {
        if length>512 {return Err(FrameError::ReplyCapacity);}
        self.available=length;
        self.ready=ready;
        self.source=ReplySource::Application;
        self.receiving=None;
        Ok(())
    }
    pub fn take_reply<'reply>(&'reply mut self, application_reply:&'reply [u8]) -> Result<&'reply [u8],FrameError> {
        if !self.armed || !self.ready {return Err(FrameError::ReplyNotReady);}
        let source=match self.source {ReplySource::Control=>&self.control_reply[..],ReplySource::Application=>application_reply};
        let available=source.get(..self.available).ok_or(FrameError::ReplyCapacity)?;
        self.armed=false;
        self.ready=false;
        Ok(&available[..core::cmp::min(self.requested as usize,self.available)])
    }
}
