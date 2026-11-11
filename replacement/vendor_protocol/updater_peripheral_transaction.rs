use super::{Advance, BlockIndex, BlockResult, ImageError, PassLimit, PeripheralSnapshot, Section, TransferCursor, UpdateImage};
use super::gpio::GpioError;
use super::io::PeripheralIo;
use crate::lens_queue_runtime::{LensQueueRuntime, PageDisposition, Publication, RuntimeError, Status};
use crate::lens_queue_runtime::packets::{CallbackId, PacketError, QueueRequest, SourceAddress, WriteRequest};
use xm1_lens_transport::frame::{PageId, PayloadLength};
use xm1_lens_transport::wire::{InterruptController, Mmio, Platform, TimerToken, TransferToken, Error as WireError};
use xm1_power_services::shutdown::RequestId;
use core::cell::RefCell;
use crate::upload_runtime::UploadPublications;

type DriverError<P, I> = RuntimeError<WireError<<P as Platform>::Error, <I as InterruptController>::Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PollStatus { Pending, Complete, Cancelled }
#[derive(Debug)]
pub(crate) enum Failure<E> { Image(ImageError), Gpio(GpioError), Packet(PacketError), Driver(E), Rejected, Status(Status), Timeout, LateCompletion }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    HandshakeMode, HandshakeControl, HandshakeInterrupt, HandshakeFinalize,
    TransferMode, BeforeReset, ResetHeld, ResetReleased, BeforeInitialize, ResetMode, ResetInterrupt,
    SetupCommand, SetupInterrupt, BlockCommand, BlockInterrupt,
    Complete, Cancelled, Failed,
}

#[derive(Clone, Copy)]
struct Deadline(u32);
impl Deadline {
    fn after(now: u32, ticks: u32) -> Self { Self(now.wrapping_add(ticks)) }
    fn expired(self, now: u32) -> bool { now.wrapping_sub(self.0) < 0x8000_0000 }
}


pub(crate) struct PeripheralUpdater<'a, 'r, M: Mmio, P: Platform, I: InterruptController, G: PeripheralIo> {
    runtime: &'r RefCell<LensQueueRuntime<M, P, I>>,
    publications: &'r UploadPublications,
    gpio: G,
    image: UpdateImage<'a>,
    cursor: TransferCursor,
    phase: Phase,
    deadline: Deadline,
    callback: CallbackId,
    request: RequestId,
    status: Status,
    interrupt_pending: bool,
}

impl<'a, 'r, M: Mmio, P: Platform, I: InterruptController, G: PeripheralIo> PeripheralUpdater<'a, 'r, M, P, I, G> {
    pub(crate) fn begin(runtime: &'r RefCell<LensQueueRuntime<M, P, I>>, publications: &'r UploadPublications, mut gpio: G, bytes: &'a [u8], snapshot: PeripheralSnapshot, limit: PassLimit) -> Result<Self, Failure<DriverError<P, I>>> {
        let image = UpdateImage::parse(bytes, snapshot).map_err(Failure::Image)?;
        let cursor = TransferCursor::new(&image, limit);
        gpio.arm().map_err(Failure::Gpio)?;
        let deadline = Deadline::after(gpio.tick(), 5000);
        let mut updater = Self { runtime, publications, gpio, image, cursor, phase: Phase::TransferMode, deadline, callback: CallbackId(0), request: RequestId(0), status: Status::Complete, interrupt_pending: false };
        let result = if snapshot.requires_handshake() { updater.control(Phase::HandshakeMode, RequestId(0x722b), &[0x8001_2800]) } else { updater.transfer_mode() };
        if let Err(error) = result { updater.abort(); return Err(error); }
        Ok(updater)
    }

    fn control(&mut self, phase: Phase, request: RequestId, words: &[u32]) -> Result<(), Failure<DriverError<P, I>>> {
        self.callback = self.runtime.borrow_mut().allocate_callback().map_err(Failure::Driver)?;
        if !self.publications.expect(self.callback) { return Err(Failure::LateCompletion); }
        let mut bytes = [0; 48];
        bytes[..4].copy_from_slice(&self.callback.0.to_le_bytes());
        for (index, word) in words.iter().enumerate() { bytes[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes()); }
        bytes[44] = words.len() as u8;
        let packet = QueueRequest::parse(request, &bytes).map_err(Failure::Packet)?;
        self.request = request;
        self.runtime.borrow_mut().submit(packet, None).map_err(Failure::Driver)?;
        self.phase = phase;
        self.deadline = Deadline::after(self.gpio.tick(), 5000);
        Ok(())
    }

    fn transfer_mode(&mut self) -> Result<(), Failure<DriverError<P, I>>> {
        self.interrupt_pending = false;
        let _ = self.gpio.take_interrupt();
        self.control(Phase::TransferMode, RequestId(0x722b), &[0x8020_2800])
    }
    fn setup(&mut self, section: Section) -> Result<(), Failure<DriverError<P, I>>> {
        let first = 0x600 | (u32::from(section.count().driver_word() & 0x7ff) << 16) | (u32::from(section.format().driver_byte()) << 27) | 0x8000_0000;
        let address = section.address().driver_word();
        self.control(Phase::SetupCommand, RequestId(0x722f), &[first, 0x3200 | (address & 0xffff_0000), 0x3240 | (address << 16)])
    }
    fn block(&mut self, index: BlockIndex) -> Result<(), Failure<DriverError<P, I>>> {
        let section = self.cursor.section().ok_or(Failure::Rejected)?;
        let bytes = self.image.block(section, index).ok_or(Failure::Rejected)?;
        let length = PayloadLength::new(bytes.len()).map_err(|_| Failure::Rejected)?;
        self.callback = self.runtime.borrow_mut().allocate_callback().map_err(Failure::Driver)?;
        if !self.publications.expect(self.callback) { return Err(Failure::LateCompletion); }
        let request = WriteRequest { page: PageId::new(index.driver_word()), source: SourceAddress(bytes.as_ptr() as u32), length, checksum: true, callback: self.callback };
        self.runtime.borrow_mut().submit(QueueRequest::Write(request), Some(bytes)).map_err(Failure::Driver)?;
        self.phase = Phase::BlockCommand;
        self.deadline = Deadline::after(self.gpio.tick(), 5000);
        Ok(())
    }
    fn advance(&mut self, advance: Advance) -> Result<(), Failure<DriverError<P, I>>> {
        match advance {
            Advance::AwaitInterrupt => { self.phase = Phase::BlockInterrupt; self.deadline = Deadline::after(self.gpio.tick(), 5000); Ok(()) }
            Advance::Block(index) => self.block(index),
            Advance::Setup(section) => self.setup(section),
            Advance::Complete => { self.cleanup()?; self.phase = Phase::Complete; Ok(()) }
            Advance::Rejected => Err(Failure::Rejected),
        }
    }
    fn wait_interrupt(&mut self, phase: Phase) { self.phase = phase; self.deadline = Deadline::after(self.gpio.tick(), 5000); }
    fn accepted(&self) -> Result<(), Failure<DriverError<P, I>>> { if self.status == Status::Complete { Ok(()) } else { Err(Failure::Status(self.status)) } }

    fn publication(&mut self, publication: Publication) -> Result<(), Failure<DriverError<P, I>>> {
        match publication {
            Publication::Command { callback, operation, status, .. } => {
                if callback != self.callback || operation.request_id() != self.request { return Err(Failure::LateCompletion); }
                self.status = status;
                match self.phase {
                    Phase::HandshakeMode => { self.accepted()?; self.control(Phase::HandshakeControl, RequestId(0x7218), &[0x0001_1000]) }
                    Phase::HandshakeControl => { self.accepted()?; self.wait_interrupt(Phase::HandshakeInterrupt); Ok(()) }
                    Phase::HandshakeFinalize => { self.accepted()?; self.transfer_mode() }
                    Phase::TransferMode => { self.accepted()?; self.phase = Phase::BeforeReset; self.deadline = Deadline::after(self.gpio.tick(), 1000); Ok(()) }
                    Phase::ResetMode => { self.wait_interrupt(Phase::ResetInterrupt); Ok(()) }
                    Phase::SetupCommand => { self.wait_interrupt(Phase::SetupInterrupt); Ok(()) }
                    _ => Err(Failure::LateCompletion),
                }
            }
            Publication::Write { callback, status, next } => {
                if callback != self.callback || self.phase != Phase::BlockCommand { return Err(Failure::LateCompletion); }
                let result = BlockResult::from_driver_status(status.driver_word() as i32);
                let next = BlockIndex::from_driver_word(next.get());
                let disposition = if result.driver_stop(next) { PageDisposition::Accept } else if matches!(result, BlockResult::Rejected) { PageDisposition::Stop } else { PageDisposition::Reject };
                self.runtime.borrow_mut().complete_page(disposition).map_err(Failure::Driver)?;
                let advance = self.cursor.completed_block(result, next);
                self.advance(advance)
            }
            _ => Err(Failure::LateCompletion),
        }
    }

    fn drive(&mut self) -> Result<PollStatus, Failure<DriverError<P, I>>> {
        match self.phase { Phase::Complete => return Ok(PollStatus::Complete), Phase::Cancelled => return Ok(PollStatus::Cancelled), Phase::Failed => return Err(Failure::Rejected), _ => {} }
        if self.deadline.expired(self.gpio.tick()) {
            match self.phase {
                Phase::BeforeReset => { self.gpio.output(0x8000_0000); self.gpio.output(0x4000_0000); self.phase = Phase::ResetHeld; self.deadline = Deadline::after(self.gpio.tick(), 1000); }
                Phase::ResetHeld => {
                    self.gpio.disarm();
                    self.interrupt_pending = false;
                    self.runtime.borrow_mut().submit(QueueRequest::Stop { callback: CallbackId(0) }, None).map_err(Failure::Driver)?;
                    self.gpio.output(0x4000_4000); self.phase = Phase::ResetReleased; self.deadline = Deadline::after(self.gpio.tick(), 20);
                }
                Phase::ResetReleased => {
                    self.gpio.output(0x8000_8000);
                    self.runtime.borrow_mut().submit(QueueRequest::Initialize { callback: CallbackId(0), observer: CallbackId(0) }, None).map_err(Failure::Driver)?;
                    self.phase = Phase::BeforeInitialize; self.deadline = Deadline::after(self.gpio.tick(), 3000);
                }
                Phase::BeforeInitialize => { self.gpio.arm().map_err(Failure::Gpio)?; self.control(Phase::ResetMode, RequestId(0x722b), &[0x8020_2800])?; }
                _ => return Err(Failure::Timeout),
            }
        }
        loop {
            let publication = self.publications.take();
            let Some(publication) = publication else { break; };
            self.publication(publication)?;
        }
        self.interrupt_pending |= self.gpio.take_interrupt();
        if self.interrupt_pending && matches!(self.phase, Phase::HandshakeInterrupt | Phase::ResetInterrupt | Phase::SetupInterrupt | Phase::BlockInterrupt) {
            self.interrupt_pending = false;
            match self.phase {
                Phase::HandshakeInterrupt => self.control(Phase::HandshakeFinalize, RequestId(0x720b), &[0x7c0])?,
                Phase::ResetInterrupt => { self.accepted()?; if let Some(section) = self.cursor.section() { self.setup(section)?; } else { self.cleanup()?; self.phase = Phase::Complete; } }
                Phase::SetupInterrupt => { self.accepted()?; self.block(BlockIndex::from_driver_word(0))?; }
                Phase::BlockInterrupt => { let advance = self.cursor.interrupt(); self.advance(advance)?; }
                _ => {}
            }
        }
        Ok(if self.phase == Phase::Complete { PollStatus::Complete } else { PollStatus::Pending })
    }

    pub(crate) fn poll(&mut self) -> Result<PollStatus, Failure<DriverError<P, I>>> {
        let result = self.drive();
        if result.is_err() { self.abort(); }
        result
    }
    pub(crate) fn wire_completed(&mut self, token: TransferToken) -> Result<PollStatus, Failure<DriverError<P, I>>> {
        if matches!(self.phase, Phase::Complete | Phase::Cancelled | Phase::Failed) { return Err(Failure::LateCompletion); }
        let result = self.runtime.borrow_mut().wire_completed(token);
        if let Err(error) = result { self.abort(); return Err(Failure::Driver(error)); }
        self.poll()
    }
    pub(crate) fn timer_expired(&mut self, token: TimerToken) -> Result<PollStatus, Failure<DriverError<P, I>>> {
        if matches!(self.phase, Phase::Complete | Phase::Cancelled | Phase::Failed) { return Err(Failure::LateCompletion); }
        let result = self.runtime.borrow_mut().timer_expired(token);
        if let Err(error) = result { self.abort(); return Err(Failure::Driver(error)); }
        self.poll()
    }
    fn cleanup(&mut self) -> Result<(), Failure<DriverError<P, I>>> {
        self.gpio.disarm();
        self.publications.clear();
        self.runtime.borrow_mut().cancel().map_err(Failure::Driver)
    }
    fn abort(&mut self) { self.gpio.disarm(); self.publications.clear(); let _ = self.runtime.borrow_mut().cancel(); self.phase = Phase::Failed; }
    pub(crate) fn cancel(&mut self) -> Result<(), Failure<DriverError<P, I>>> {
        if matches!(self.phase, Phase::Complete | Phase::Cancelled) { return Ok(()); }
        if let Err(error) = self.cleanup() { self.abort(); return Err(error); }
        self.phase = Phase::Cancelled;
        Ok(())
    }
}

impl<M: Mmio, P: Platform, I: InterruptController, G: PeripheralIo> Drop for PeripheralUpdater<'_, '_, M, P, I, G> {
    fn drop(&mut self) { if !matches!(self.phase, Phase::Complete | Phase::Cancelled | Phase::Failed) { self.abort(); } }
}
