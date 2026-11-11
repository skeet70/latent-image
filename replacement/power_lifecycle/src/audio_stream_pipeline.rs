use super::stream::{ByteLength, PcmAddress};
use crate::primitives::ServiceStatus;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion { Receive, Send }
impl Completion {
    pub const fn from_abi(address: u32) -> Option<Self> { match address { 0x6e5298 => Some(Self::Receive), 0x6e53e8 => Some(Self::Send), _ => None } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Submission { First, Continuation }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Field { Request = 0, Phase = 4, Flags = 12, Event = 16, Transfer = 20, Timer = 24, Completion = 28, Source = 32, Destination = 36, Length = 40, Argument = 44, Callback = 48, Control = 52, Register34 = 56, Register30 = 60, DmaSource = 64, DmaDestination = 68, DmaLength = 72, AlternateSource = 76, AlternateDestination = 80, AlternateLength = 84, Priority = 92 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestField { Direction, InputPacking, OutputPacking, Ready, Finished, Error }
impl RequestField {
    pub const fn offset(self) -> u32 { match self { Self::Direction => 0, Self::InputPacking => 12, Self::OutputPacking => 16, Self::Ready => 24, Self::Finished => 28, Self::Error => 32 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bank { First, Second }
impl Bank { pub const fn index(self) -> u32 { match self { Self::First => 0, Self::Second => 1 } } }
pub trait Pipeline {
    fn get(&self, field: Field) -> u32;
    fn set(&mut self, field: Field, value: u32);
    fn request(&self, field: RequestField) -> u32;
    fn set_error(&mut self, error: u32);
    fn selected_bank(&self) -> Bank;
    fn channel_index(&mut self) -> u32;
    fn program(&mut self) -> ServiceStatus;
    fn start_dma(&mut self) -> ServiceStatus;
    fn dma_pending(&mut self) -> bool;
    fn close_dma(&mut self) -> ServiceStatus;
    fn start_resource(&mut self) -> ServiceStatus;
    fn ready(&mut self);
    fn schedule_ready(&mut self);
    fn fill(&mut self, address: PcmAddress, length: ByteLength);
}
pub trait PipelineLifecycle: Pipeline {
    type Guard;
    fn stop(&mut self) -> ServiceStatus;
    fn release(&mut self) -> ServiceStatus;
    fn finished(&mut self);
    fn enter(&mut self) -> Self::Guard;
    fn leave(&mut self, guard: Self::Guard);
}

pub fn update(pipeline: &mut impl Pipeline) -> ServiceStatus {
    if pipeline.request(RequestField::Direction) == 0 {
        pipeline.set(Field::AlternateDestination, pipeline.get(Field::Destination));
        pipeline.set(Field::AlternateLength, pipeline.get(Field::Length));
    } else {
        let remainder = pipeline.get(Field::Length) & 7;
        if remainder != 0 {
            pipeline.fill(PcmAddress(pipeline.get(Field::Source).wrapping_add(pipeline.get(Field::Length))), ByteLength(remainder));
            pipeline.set(Field::Length, pipeline.get(Field::Length).wrapping_add(remainder));
        }
        pipeline.set(Field::AlternateSource, pipeline.get(Field::Source));
        pipeline.set(Field::AlternateLength, pipeline.get(Field::Length));
    }
    pipeline.program()
}

pub fn start(pipeline: &mut impl Pipeline, bank: Bank) -> ServiceStatus {
    pipeline.set(Field::Register34, 0);
    pipeline.set(Field::Register30, 0);
    pipeline.set(Field::Control, 0);
    let channel = pipeline.channel_index();
    pipeline.set(Field::Priority, if bank == Bank::First { 13 } else { 12 });
    pipeline.set(Field::Control, channel & 7 | 0x40000010);
    let packing = match (pipeline.request(RequestField::InputPacking) != 0, pipeline.request(RequestField::OutputPacking) != 0) {
        (false, false) => 2, (true, true) => 0, _ => 1,
    };
    if pipeline.request(RequestField::Direction) == 0 {
        pipeline.set(Field::Control, pipeline.get(Field::Control) | packing << 12 | 0x120000);
        pipeline.set(Field::DmaDestination, pipeline.get(Field::Destination));
        pipeline.set(Field::DmaLength, pipeline.get(Field::Length));
        pipeline.set(Field::AlternateDestination, 0x93adf8);
        pipeline.set(Field::AlternateLength, 0x800);
        let fifo = 0xfffa0020 + bank.index() * 0x1000;
        pipeline.set(Field::DmaSource, fifo);
        pipeline.set(Field::AlternateSource, fifo);
    } else {
        pipeline.set(Field::Control, pipeline.get(Field::Control) | 0x220000);
        pipeline.set(Field::DmaSource, pipeline.get(Field::Source));
        pipeline.set(Field::DmaLength, pipeline.get(Field::Length));
        let mono = pipeline.request(RequestField::InputPacking) != 0;
        let narrow = pipeline.request(RequestField::OutputPacking) != 0;
        pipeline.set(Field::Source, if mono { 0x7d08b4 } else { 0x7d0c74 });
        pipeline.set(Field::Length, 1920 >> (u32::from(mono) + u32::from(narrow)));
        pipeline.set(Field::Control, pipeline.get(Field::Control) | packing << 12);
        pipeline.set(Field::AlternateSource, pipeline.get(Field::Source));
        pipeline.set(Field::AlternateLength, pipeline.get(Field::Length));
        let fifo = 0xfffa0010 + bank.index() * 0x1000;
        pipeline.set(Field::DmaDestination, fifo);
        pipeline.set(Field::AlternateDestination, fifo);
    }
    let status = pipeline.program();
    if status.0 != 0 { let _ = pipeline.close_dma(); return status; }
    pipeline.set(Field::Argument, 38);
    pipeline.set(Field::Callback, pipeline.get(Field::Completion));
    let _ = pipeline.start_dma();
    while !pipeline.dma_pending() { core::hint::spin_loop(); }
    pipeline.set(Field::Control, pipeline.get(Field::Control) | 0x10000000);
    let status = pipeline.program();
    if status.0 != 0 { return status; }
    pipeline.set(Field::Flags, pipeline.get(Field::Flags) | 2);
    pipeline.start_resource()
}

fn failed(pipeline: &mut impl Pipeline, code: u32) -> ServiceStatus {
    pipeline.set(Field::Phase, 8);
    pipeline.set_error(code);
    ServiceStatus(-255)
}
pub fn submit(pipeline: &mut impl Pipeline, event: EventHandle, position: PcmAddress, length: ByteLength, submission: Submission) -> ServiceStatus {
    if pipeline.get(Field::Event) != event.0 || event.0 == 0 { pipeline.set_error(2); return ServiceStatus(-20); }
    let transmit = pipeline.request(RequestField::Direction) != 0;
    if submission == Submission::First {
        match pipeline.get(Field::Phase) {
            1 => {
                let bank = pipeline.selected_bank();
                pipeline.set(Field::Completion, if transmit { 0x6e53e8 } else { 0x6e5298 });
                if transmit {
                    pipeline.set(Field::Source, position.0);
                    pipeline.set(Field::Destination, 0xfffa0010 + bank.index() * 0x1000);
                } else {
                    pipeline.set(Field::Source, 0xfffa0020 + bank.index() * 0x1000);
                    pipeline.set(Field::Destination, position.0);
                }
                pipeline.set(Field::Length, length.0);
                let status = start(pipeline, bank);
                pipeline.set(Field::Phase, if status.0 == 0 { 2 } else { 8 });
                if status.0 != 0 { pipeline.set_error(3); }
                if transmit { pipeline.schedule_ready(); }
                else if status.0 == 0 { pipeline.ready(); }
                status
            }
            2 if transmit => { pipeline.set(Field::Phase, 3); ServiceStatus(0) }
            _ => failed(pipeline, 5),
        }
    } else if pipeline.get(Field::Phase) == 2 {
        pipeline.set(if transmit { Field::Source } else { Field::Destination }, position.0);
        pipeline.set(Field::Length, length.0);
        let status = update(pipeline);
        pipeline.set(Field::Phase, if status.0 == 0 { 3 } else { 8 });
        if status.0 != 0 { pipeline.set_error(4); }
        status
    } else { failed(pipeline, 5) }
}

fn padding(pipeline: &mut impl Pipeline) {
    let mono = pipeline.request(RequestField::InputPacking) != 0;
    let narrow = pipeline.request(RequestField::OutputPacking) != 0;
    pipeline.set(Field::Source, if mono { 0x7d08b4 } else { 0x7d0c74 });
    pipeline.set(Field::Length, 1920 >> (u32::from(mono) + u32::from(narrow)));
}
fn finish(pipeline: &mut impl PipelineLifecycle) {
    if pipeline.stop().0 != 0 { pipeline.set_error(6); }
    if pipeline.release().0 != 0 && pipeline.request(RequestField::Error) == 0 { pipeline.set_error(2); }
    pipeline.finished();
    pipeline.set(Field::Phase, 0);
}
pub fn complete(pipeline: &mut impl PipelineLifecycle, completion: Completion) {
    let phase = pipeline.get(Field::Phase);
    match completion {
        Completion::Send => {
            if !matches!(phase, 2 | 3 | 6 | 7) { return; }
            padding(pipeline);
            if !matches!(phase, 6 | 7) { let _ = update(pipeline); }
            match pipeline.get(Field::Phase) {
                3 => { pipeline.set(Field::Phase, 2); pipeline.ready(); },
                6 => pipeline.set(Field::Phase, 7),
                7 => finish(pipeline),
                _ => (),
            }
        }
        Completion::Receive => match phase {
            2 | 3 => {
                pipeline.set(Field::Destination, 0x93adf8);
                pipeline.set(Field::Length, 0x800);
                let _ = update(pipeline);
                if pipeline.get(Field::Phase) == 3 { pipeline.set(Field::Phase, 2); pipeline.ready(); }
            }
            4 => { pipeline.set(Field::Phase, 6); pipeline.ready(); },
            6 => pipeline.set(Field::Phase, 7),
            7 => finish(pipeline),
            _ => (),
        },
    }
}
fn abort_guarded<P: PipelineLifecycle>(pipeline: &mut P, guard: P::Guard) -> ServiceStatus {
    pipeline.set(Field::Phase, 7);
    if pipeline.stop().0 != 0 && pipeline.request(RequestField::Error) == 0 { pipeline.set_error(6); }
    pipeline.leave(guard);
    let status = pipeline.release();
    if status.0 != 0 && pipeline.request(RequestField::Error) == 0 { pipeline.set_error(2); }
    pipeline.set(Field::Phase, 0);
    pipeline.finished();
    status
}
pub fn cancel(pipeline: &mut impl PipelineLifecycle, event: EventHandle) -> ServiceStatus {
    if pipeline.get(Field::Event) != event.0 || event.0 == 0 { pipeline.set_error(2); return ServiceStatus(-20); }
    let transmit = pipeline.request(RequestField::Direction) != 0;
    let guard = pipeline.enter();
    let phase = pipeline.get(Field::Phase);
    if phase < 2 {
        pipeline.leave(guard);
        let status = pipeline.release();
        if status.0 != 0 { pipeline.set_error(2); }
        pipeline.set(Field::Phase, 0);
        pipeline.finished();
        status
    } else if phase == 2 {
        if transmit && matches!(pipeline.get(Field::Source), 0x7d0c74 | 0x7d08b4) {
            pipeline.set(Field::Length, 8);
            pipeline.set(Field::Control, pipeline.get(Field::Control) & !0x40000000);
            let _ = update(pipeline);
            pipeline.set(Field::Phase, 7);
        } else { pipeline.set(Field::Phase, 6); }
        pipeline.leave(guard);
        ServiceStatus(0)
    } else if phase == 3 {
        if !transmit { pipeline.set(Field::Phase, 4); pipeline.leave(guard); return ServiceStatus(0); }
        padding(pipeline);
        let status = update(pipeline);
        if status.0 == 0 { pipeline.set(Field::Phase, 6); pipeline.leave(guard); ServiceStatus(0) }
        else { pipeline.set_error(4); abort_guarded(pipeline, guard) }
    } else if phase == 8 || (!transmit && phase == 4) {
        abort_guarded(pipeline, guard)
    } else {
        if pipeline.request(RequestField::Error) == 0 { pipeline.set_error(5); }
        pipeline.leave(guard);
        ServiceStatus(-255)
    }
}

#[cfg(target_arch = "arm")]
#[path = "audio_stream_pipeline_native.rs"]
pub mod native;
