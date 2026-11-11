use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

#[path = "ui_completion_menu.rs"]
pub mod menu;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum UiRequest { PowerFamilyStart = 0xa00, PowerPrepare = 0xa01, Power61 = 0xa02, Power62 = 0xa03, PowerOff = 0xa04, PowerFinish = 0xa05, Power10 = 0xa06, Power67 = 0xa07, Power69 = 0xa08, ResumeMenu = 0x50b }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiCode(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sequence(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequenceMode { Unsequenced, Advance, Current }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestOutcome { Queued, Duplicate, Posted, Suppressed }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError { SuspendedTableFull }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationEvent(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendGate { pub disabled: bool, pub transition_active: bool, pub restrict_lower_family: bool }
impl FrontendGate {
    pub fn admits(self, request: UiRequest) -> bool {
        !self.disabled && !self.transition_active && (!self.restrict_lower_family || request as u32 >= 0xa00)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuMode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResumeMenuContext { pub current: UiCode, pub mode: MenuMode, pub alternative: bool, pub selection: MenuMode }

pub trait QueueTiming { fn delay_ticks(&mut self, ticks: u32); fn posted(&mut self, code: UiCode, phase: u32); }
pub trait MenuResume { fn context(&self) -> ResumeMenuContext; fn select_and_snapshot(&mut self, low_code: u8); }
pub trait ApplicationEvents { fn post(&mut self, event: ApplicationEvent, record: &[u8; 76]); }

pub struct CallerResidue { pub packet: [u8; 12], pub application_record: [u8; 76] }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiPacket(pub [u8; 12]);
impl UiPacket {
    pub fn code(&self) -> UiCode { UiCode(u16::from_le_bytes([self.0[0], self.0[1]])) }
    pub fn sequence(&self) -> Sequence { Sequence(self.0[2]) }
}
pub trait Queue33Receive { fn receive(&mut self) -> Result<UiPacket, KernelStatus>; }

pub struct UiCompletion {
    suspended: bool,
    requests: [UiRequest; 30],
    count: u8,
    last_translated: UiCode,
    current: UiCode,
    last_posted: UiCode,
    sequence: Sequence,
    expected: Sequence,
}
impl UiCompletion {
    pub const fn new() -> Self { Self { suspended: false, requests: [UiRequest::PowerOff; 30], count: 0, last_translated: UiCode(0), current: UiCode(0), last_posted: UiCode(0), sequence: Sequence(0), expected: Sequence(0) } }
    pub const fn sequence(&self) -> Sequence { self.sequence }
    pub const fn expected_sequence(&self) -> Sequence { self.expected }
    pub const fn suspended(&self) -> bool { self.suspended }
    pub const fn pending_count(&self) -> u8 { self.count }
    pub const fn last_posted(&self) -> UiCode { self.last_posted }
    pub fn frontend_request(&mut self, gate: FrontendGate, request: UiRequest, rtos: &mut impl ShutdownRtos, timing: &mut impl QueueTiming, menu: &mut impl MenuResume, residue: &mut CallerResidue) -> Result<RequestOutcome, RouteError> {
        if !gate.admits(request) { return Ok(RequestOutcome::Suppressed); }
        self.request(request, rtos, timing, menu, residue)
    }

    pub fn post(&mut self, code: UiCode, mode: SequenceMode, rtos: &mut impl ShutdownRtos, timing: &mut impl QueueTiming, residue: &mut CallerResidue) -> RequestOutcome {
        if mode == SequenceMode::Advance {
            if self.current == code && self.sequence != self.expected { return RequestOutcome::Suppressed; }
            self.current = code;
        } else { self.current = UiCode(0); }
        let sequence = match mode {
            SequenceMode::Advance => { self.sequence = Sequence(if self.sequence.0 == 255 { 1 } else { self.sequence.0 + 1 }); self.sequence },
            SequenceMode::Current => self.sequence,
            SequenceMode::Unsequenced => Sequence(0),
        };
        residue.packet[0..4].fill(0);
        residue.packet[8..12].fill(0);
        residue.packet[0..2].copy_from_slice(&code.0.to_le_bytes());
        residue.packet[2] = sequence.0;
        residue.packet[4..8].fill(0);
        self.last_posted = code;
        let sender = rtos.current_task();
        for _ in 0..10 {
            let status = rtos.post(ServiceMessage { operation: RequestId(0x3100), sender, destination: QueueId(33), payload: &residue.packet });
            if !matches!(status, Err(KernelStatus(-18 | -62))) { break; }
            timing.delay_ticks(100);
        }
        timing.posted(code, 5);
        RequestOutcome::Posted
    }

    pub fn request(&mut self, request: UiRequest, rtos: &mut impl ShutdownRtos, timing: &mut impl QueueTiming, menu: &mut impl MenuResume, residue: &mut CallerResidue) -> Result<RequestOutcome, RouteError> {
        if self.suspended {
            if self.count > 0x1d { return Err(RouteError::SuspendedTableFull); }
            if self.count != 0 && self.requests[usize::from(self.count - 1)] == request { return Ok(RequestOutcome::Duplicate); }
            self.requests[usize::from(self.count)] = request;
            self.count += 1;
            return Ok(RequestOutcome::Queued);
        }
        let code = match request {
            UiRequest::ResumeMenu => {
                let code = resume_code(menu.context());
                if code.0 != 0 { menu.select_and_snapshot(code.0 as u8); }
                UiCode(code.0 | 0x4000)
            },
            _ => UiCode(0x5000 | match request { UiRequest::PowerFamilyStart => 0xc, UiRequest::PowerPrepare => 0xd, UiRequest::Power61 => 0x61, UiRequest::Power62 => 0x62, UiRequest::PowerOff => 0xe, UiRequest::PowerFinish => 0xf, UiRequest::Power10 => 0x10, UiRequest::Power67 => 0x67, UiRequest::Power69 => 0x69, UiRequest::ResumeMenu => unreachable!() }),
        };
        let mode = if code.0 & 0xf000 == 0x5000 { self.last_translated = code; SequenceMode::Unsequenced } else { SequenceMode::Advance };
        Ok(self.post(code, mode, rtos, timing, residue))
    }

    pub fn initialize(&mut self, busy: bool, rtos: &mut impl ShutdownRtos, timing: &mut impl QueueTiming, residue: &mut CallerResidue) {
        if busy { self.suspended = true; return; }
        self.sequence = Sequence(0);
        self.expected = Sequence(0);
        self.current = UiCode(0);
        self.last_posted = UiCode(0);
        self.post(UiCode(0x1000), SequenceMode::Unsequenced, rtos, timing, residue);
    }

    pub fn resume(&mut self, busy: bool, rtos: &mut impl ShutdownRtos, timing: &mut impl QueueTiming, menu: &mut impl MenuResume, residue: &mut CallerResidue) -> Result<(), RouteError> {
        if !self.suspended { self.post(UiCode(0xb000), SequenceMode::Unsequenced, rtos, timing, residue); return Ok(()); }
        self.suspended = false;
        self.initialize(busy, rtos, timing, residue);
        let mut index = 0;
        let mut error = None;
        while index < self.count {
            if let Err(failure) = self.request(self.requests[usize::from(index)], rtos, timing, menu, residue) { error = Some(failure); }
            index += 1;
        }
        self.post(UiCode(0xb000), SequenceMode::Unsequenced, rtos, timing, residue);
        if let Err(failure) = self.request(UiRequest::ResumeMenu, rtos, timing, menu, residue) { error = Some(failure); }
        error.map_or(Ok(()), Err)
    }

    pub fn receive(&mut self, queue: &mut impl Queue33Receive, consumer: &mut PowerConsumer, events: &mut impl ApplicationEvents, residue: &mut CallerResidue) -> Result<Delivery, KernelStatus> {
        let packet = queue.receive()?;
        if packet.sequence().0 != 0 { self.expected = packet.sequence(); }
        Ok(consumer.deliver(packet, events, residue))
    }
}
impl Default for UiCompletion { fn default() -> Self { Self::new() } }

fn resume_code(context: ResumeMenuContext) -> UiCode {
    let current = context.current.0;
    UiCode(match current & 0xfff {
        0x11 | 1 | 9 | 0x12 | 0x1c | 0x1f | 0x41 | 0xc | 3 => 0x4002,
        0x21 => if context.mode.0.wrapping_sub(3) <= 3 { 0x4023 } else { 0x4002 },
        0x5e | 0x5b | 0x5d | 0x5f => 0x405c,
        0x62 => if context.alternative { 0x4063 } else { 0x405c },
        0xa4 | 0x64 => 0x4065, 0xa8 => 0x40a9, 0x6f => 0x4070,
        0x98 => 0x4099, 0x77 => 0x4078, 0x14c => 0x14b,
        0x96 | 0x151 => 0, 0x69 => 0x176,
        0x73 => if context.selection.0 == 0x27 { current } else { 0x4074 },
        0x42 => 0x4043, 0x53 => 0x4054, 0xae => 0x40af,
        0x8b => 0x408c, 0xde => 0x40dd, 0xdf => 0x40dc,
        _ => current,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery { PowerOff { events: u8 }, Other(UiPacket), NotInitialized }
pub struct PowerConsumer { pub initialized: bool, pub blocked: bool, pub block_on_delivery: bool }
impl PowerConsumer {
    pub fn deliver(&mut self, packet: UiPacket, events: &mut impl ApplicationEvents, residue: &mut CallerResidue) -> Delivery {
        if packet.code() != UiCode(0x500e) { return Delivery::Other(packet); }
        if !self.initialized { return Delivery::NotInitialized; }
        if self.block_on_delivery { self.blocked = true; }
        residue.application_record[..4].copy_from_slice(&0xa2fu32.to_le_bytes());
        residue.application_record[4..8].copy_from_slice(&0x110u32.to_le_bytes());
        events.post(ApplicationEvent(0xa2f), &residue.application_record);
        if self.blocked { return Delivery::PowerOff { events: 1 }; }
        // The 0x500e callback at 0x112710c has no body effects.
        events.post(ApplicationEvent(0xa2f), &residue.application_record);
        Delivery::PowerOff { events: 2 }
    }
}

#[cfg(target_arch = "arm")]
impl Queue33Receive for crate::shutdown_rtos::NativeShutdownRtos {
    fn receive(&mut self) -> Result<UiPacket, KernelStatus> {
        use crate::shutdown_receive::{PayloadAddress, ReceiveDescriptor, ReceiveTimeout};
        let mut descriptor = ReceiveDescriptor::new(PayloadAddress(0x15e46d0));
        unsafe { crate::shutdown_receive::receive_into(self, QueueId(33), &mut descriptor, ReceiveTimeout::Forever)?; }
        let header = descriptor.header();
        unsafe {
            core::ptr::write_volatile(0x15e46c4 as *mut u16, header.operation.0);
            core::ptr::write_volatile(0x15e46c6 as *mut u8, header.sender.0);
            core::ptr::write_volatile(0x15e46c7 as *mut u8, header.destination.0);
            core::ptr::write_volatile(0x15e46c8 as *mut u32, descriptor.length());
        }
        let mut packet = UiPacket([0; 12]);
        unsafe { core::ptr::copy_nonoverlapping(0x15e46d0 as *const u8, packet.0.as_mut_ptr(), 12); }
        Ok(packet)
    }
}

#[cfg(test)]
#[path = "ui_completion_tests.rs"]
mod tests;
