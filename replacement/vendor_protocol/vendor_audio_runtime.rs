use crate::{
    audio_zoom_actor::{AudioZoomActor, Channels, Completion, Gain, InputMode, Operation, OutputMode, Rate, Request, RequestId, Tick, Timeout, WorkerFields},
    backend::{BackendError, ConfigImages},
    configuration_actions_low::{ActionCompletion, ConfigurationKey},
    configuration_audio_actors::{RecordedPcm, RecordingSeconds, ServicePcmWindow, PCM_BYTES_PER_SECOND, sound::{SoundPlayback, SoundMode}},
};
use xm1_power_lifecycle::audio_release::{
    executor::{ExecutorIo, ExecutorMemory},
    packet_ring::{self, RingMemory, Stride},
    stream::{ByteLength, PcmAddress, StreamEvent},
    stream_pipeline::{self, EventHandle, Field, Pipeline, PipelineLifecycle, Submission},
    stream_construction::{self, Address, ConstructionIo, ConstructionMemory},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction { Capture, Playback }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Busy, Codec(crate::audio_zoom_actor::Error), CodecCompletion(Completion), Device(i32), UnexpectedEvent }
impl From<crate::audio_zoom_actor::Error> for Error { fn from(value: crate::audio_zoom_actor::Error) -> Self { Self::Codec(value) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress { Waiting, SegmentStopped, Complete(ActionCompletion) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureAvMode { Mode80, Mode81 }
impl CaptureAvMode {
    pub const fn from_sample(value: u8) -> Self { if value == 0 { Self::Mode81 } else { Self::Mode80 } }
    pub const fn request(self) -> [u32; 2] { [match self { Self::Mode80 => 0x80, Self::Mode81 => 0x81 }, 0] }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamSlot(u8);
impl StreamSlot {
    pub const fn from_abi(value: u8) -> Option<Self> { if value < 16 { Some(Self(value)) } else { None } }
}
pub fn decode_stream_event(slot: StreamSlot, message: &[u8]) -> Option<StreamEvent> {
    if message.len() != 16 || message[2] != slot.0 { return None; }
    match u16::from_le_bytes([message[0], message[1]]) {
        0x2e05 => Some(StreamEvent::Packet), 0x2e06 => Some(StreamEvent::Empty), 0x2e07 => Some(StreamEvent::Finished), _ => None,
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, Initialize(RequestId), Configure(RequestId), Streaming, Holding, Draining, StopCodec(RequestId) }

pub struct VendorAudioRuntime {
    codec: AudioZoomActor,
    phase: Phase,
    direction: Direction,
    window: ServicePcmWindow,
    base: PcmAddress,
    result_key: Option<ConfigurationKey>,
    configured: bool,
    command_active: bool,
    sound: Option<SoundPlayback>,
}
impl VendorAudioRuntime {
    pub const fn new() -> Self {
        Self { codec: AudioZoomActor::new(), phase: Phase::Idle, direction: Direction::Capture,
            window: ServicePcmWindow::new(RecordingSeconds::zero()), base: PcmAddress(0), result_key: None, configured: false, command_active: false, sound: None }
    }
    pub fn busy(&self) -> bool { self.phase != Phase::Idle }
    pub fn begin(&mut self, direction: Direction, seconds: RecordingSeconds, base: PcmAddress, result_key: ConfigurationKey,
        memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, worker: &mut impl WorkerFields, now: Tick, timeout: Timeout) -> Result<(), Error> {
        self.start(direction, seconds, base, Some(result_key), None, memory, io, worker, now, timeout)
    }
    pub fn begin_sound(&mut self, playback: SoundPlayback, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo,
        worker: &mut impl WorkerFields, now: Tick, timeout: Timeout) -> Result<(), Error> {
        self.start(Direction::Playback, RecordingSeconds::parse(1).unwrap(), PcmAddress(playback.clip.position().get()), None, Some(playback), memory, io, worker, now, timeout)
    }
    fn start(&mut self, direction: Direction, seconds: RecordingSeconds, base: PcmAddress, result_key: Option<ConfigurationKey>, sound: Option<SoundPlayback>,
        memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, worker: &mut impl WorkerFields, now: Tick, timeout: Timeout) -> Result<(), Error> {
        if self.busy() { return Err(Error::Busy); }
        self.direction = direction;
        self.window = ServicePcmWindow::new(seconds);
        self.base = base;
        self.result_key = result_key;
        self.sound = sound;
        self.command_active = false;
        self.phase = if self.configured {
            Phase::Configure(self.codec.submit(memory, io, worker, self.codec_request(Operation::Start), now, timeout)?)
        } else {
            Phase::Initialize(self.codec.submit(memory, io, worker, Request::Initialize, now, timeout)?)
        };
        Ok(())
    }
    fn codec_request(&self, operation: Operation) -> Request {
        match self.direction {
            Direction::Capture => Request::Input { operation, mode: InputMode::Format8, rate: Rate::Hz48000 },
            Direction::Playback => Request::Output {
                operation,
                mode: match self.sound { None => OutputMode::Format8, Some(sound) if sound.mode == SoundMode::Track => OutputMode::Sound, Some(_) => OutputMode::Playback },
                gain: Gain::new(self.sound.map_or(7, |sound| sound.level.get())).unwrap(),
                channels: Channels::Primary, rate: Rate::Hz48000,
            },
        }
    }
    fn feed(&mut self, ring: &mut impl RingMemory) {
        while let Some(second) = self.window.next_second() {
            packet_ring::enqueue(ring, PcmAddress(self.base.0.wrapping_add(second.byte_offset() as u32)),
                ByteLength(self.sound.map_or(PCM_BYTES_PER_SECOND as u32, |sound| sound.clip.length().get())), Stride(1));
        }
    }
    pub fn poll(&mut self, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, worker: &mut impl WorkerFields,
        pipeline: &mut impl PipelineLifecycle, ring: &mut impl RingMemory, configuration: &mut ConfigImages<'_>, now: Tick, timeout: Timeout) -> Result<Progress, Error> {
        self.codec.observe(memory, io, worker, now)?;
        let pending = match self.phase { Phase::Initialize(id) | Phase::Configure(id) | Phase::StopCodec(id) => Some(id), _ => None };
        if let Some(id) = pending {
            let Some(completion) = self.codec.result(id)? else { return Ok(Progress::Waiting); };
            if completion != Completion::Complete { return Err(Error::CodecCompletion(completion)); }
            match self.phase {
                Phase::Initialize(_) => {
                    self.configured = true;
                    self.phase = Phase::Configure(self.codec.submit(memory, io, worker, self.codec_request(Operation::Start), now, timeout)?);
                }
                Phase::Configure(_) => {
                    self.feed(ring);
                    self.phase = Phase::Streaming;
                    if let Some(index) = packet_ring::front(ring) {
                        let packet = ring.packet(index);
                        self.command_active = true;
                        let event = pipeline.get(Field::Event);
                        let status = stream_pipeline::submit(pipeline, EventHandle(event), packet.position, packet.length, Submission::First);
                        if status.0 != 0 { return Err(Error::Device(status.0)); }
                    }
                }
                Phase::StopCodec(_) => {
                    if let Some(key) = self.result_key.take() { key.write(configuration, 0).map_err(|_| Error::Device(-30))?; }
                    self.phase = Phase::Idle;
                    return Ok(Progress::Complete(ActionCompletion::service_success()));
                }
                _ => return Err(Error::UnexpectedEvent),
            }
        }
        if self.phase == Phase::Holding { return Ok(Progress::SegmentStopped); }
        if self.ready_to_stop() {
            self.draining()?;
            let event = EventHandle(pipeline.get(Field::Event));
            let status = stream_pipeline::cancel(pipeline, event);
            if status.0 != 0 { return Err(Error::Device(status.0)); }
        }
        Ok(Progress::Waiting)
    }
    pub fn event(&mut self, event: StreamEvent, pipeline: &mut impl Pipeline, ring: &mut impl RingMemory) -> Result<(), Error> {
        if !matches!(self.phase, Phase::Streaming | Phase::Draining) { return Err(Error::UnexpectedEvent); }
        match event {
            StreamEvent::Packet => {
                if self.command_active { self.window.stream_completion(); }
                self.command_active = true;
                self.feed(ring);
            }
            StreamEvent::Empty => {
                if self.command_active { self.window.stream_completion(); self.command_active = false; }
                self.feed(ring);
                if self.sound.is_some_and(|sound| !sound.stop_after) && self.window.all_completed() { self.phase = Phase::Holding; }
            }
            StreamEvent::Finished => {
                if self.phase != Phase::Draining { return Err(Error::UnexpectedEvent); }
            }
        }
        if event == StreamEvent::Empty && self.phase == Phase::Streaming {
            if let Some(index) = packet_ring::front(ring) {
                let packet = ring.packet(index);
                let event = EventHandle(pipeline.get(Field::Event));
                let status = stream_pipeline::submit(pipeline, event, packet.position, packet.length, Submission::Continuation);
                if status.0 != 0 { return Err(Error::Device(status.0)); }
                self.command_active = true;
            }
        }
        Ok(())
    }
    pub fn ready_to_stop(&self) -> bool { self.phase == Phase::Streaming && self.window.all_completed() }
    pub fn draining(&mut self) -> Result<(), Error> {
        if !self.ready_to_stop() { return Err(Error::UnexpectedEvent); }
        self.phase = Phase::Draining;
        Ok(())
    }
    pub fn transfer_finished(&mut self, memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, worker: &mut impl WorkerFields, now: Tick, timeout: Timeout) -> Result<(), Error> {
        if self.phase != Phase::Draining { return Err(Error::UnexpectedEvent); }
        self.phase = Phase::StopCodec(self.codec.submit(memory, io, worker, self.codec_request(Operation::Stop), now, timeout)?);
        Ok(())
    }
    pub fn continue_sound(&mut self, playback: SoundPlayback, pipeline: &mut impl Pipeline, ring: &mut impl RingMemory) -> Result<(), Error> {
        if self.phase != Phase::Holding || playback.configure { return Err(Error::UnexpectedEvent); }
        self.sound = Some(playback);
        self.window = ServicePcmWindow::new(RecordingSeconds::parse(1).unwrap());
        self.base = PcmAddress(playback.clip.position().get());
        self.feed(ring);
        let index = packet_ring::front(ring).ok_or(Error::UnexpectedEvent)?;
        let packet = ring.packet(index);
        let event = EventHandle(pipeline.get(Field::Event));
        let status = stream_pipeline::submit(pipeline, event, packet.position, packet.length, Submission::Continuation);
        if status.0 != 0 { return Err(Error::Device(status.0)); }
        self.command_active = true;
        self.phase = Phase::Streaming;
        Ok(())
    }
}
impl Default for VendorAudioRuntime { fn default() -> Self { Self::new() } }

pub fn prepare_playback<'a>(selector: u8, seconds: RecordingSeconds, recording: &[u8], destination: &'a mut [u8]) -> Result<&'a [u8], BackendError> {
    RecordedPcm::adopt(seconds, recording)?.prepare_playback(selector, destination)?;
    Ok(&destination[..seconds.byte_length()])
}

pub fn prepare_transfer(memory: &mut impl ConstructionMemory, io: &mut impl ConstructionIo, direction: Direction, sound: Option<SoundPlayback>) -> Result<EventHandle, Error> {
    let transmit = direction == Direction::Playback;
    let request = Address(0x956720);
    let configuration = Address(0x14e5af8);
    let mode = sound.map_or(0, |sound| if sound.mode == SoundMode::Track { 4 } else { 7 });
    let gain = sound.map_or(7, |sound| u32::from(sound.level.get()));
    for (index, value) in [if transmit { 1 } else { 2 }, 8, u32::from(sound.is_none()), 1, mode, 48000, gain, 0].into_iter().enumerate() { memory.put(configuration.offset(index as u32 * 4), value); }
    memory.put(Address(0x9566bc), configuration.0);
    for (offset, value) in [(0, u32::from(transmit)), (4, 2), (8, 48000), (12, 0), (16, u32::from(sound.is_some())), (20, 0), (24, if transmit { 0x6ec1bc } else { 0x6ec418 }), (28, 0x6ec18c), (32, 0)] { memory.put(request.offset(offset), value); }
    let status = stream_construction::resource_acquire(memory, io, request);
    if status.0 != 0 { return Err(Error::Device(status.0)); }
    let status = stream_construction::resource_configure(memory);
    if status.0 != 0 { let _ = stream_construction::resource_release(memory); return Err(Error::Device(status.0)); }
    let output = Address(0x956744);
    let status = stream_construction::pipeline_acquire(memory, io, request, output);
    if status.0 != 0 { let _ = stream_construction::resource_release(memory); return Err(Error::Device(status.0)); }
    for address in [0x956748, 0x95674c, 0x966998, 0x96699c] { memory.put(Address(address), 0); }
    for slot in 0..packet_ring::CAPACITY { memory.put(Address(0x9669a4 + slot as u32 * 20), u32::MAX); }
    Ok(EventHandle(memory.word(output)))
}

pub fn register_stream(memory: &mut impl ConstructionMemory) -> Result<StreamSlot, Error> {
    let registry = Address(0x96e04c);
    if memory.word(registry.offset(4)) == 0 {
        memory.put(registry, 0);
        memory.put(registry.offset(4), 16);
        memory.put(registry.offset(8), 0x95761c);
        for slot in 0..16 { memory.put(Address(0x95761c + slot * 8), u32::MAX); }
    }
    if memory.word(registry.offset(4)) != 16 { return Err(Error::Device(-30)); }
    let entries = Address(memory.word(registry.offset(8)));
    let cursor = u32::from(memory.half(registry));
    if cursor >= 16 { return Err(Error::Device(-30)); }
    let Some(slot) = (0..16).map(|offset| (cursor + offset) % 16).find(|slot| memory.word(entries.offset(slot * 8)) == u32::MAX) else { return Err(Error::Device(-12)); };
    let request = Address(0x9566c0);
    for offset in (0..32).step_by(4) { memory.put(request.offset(offset), 0); }
    memory.put_half(request.offset(4), slot as u16);
    memory.put_half(request.offset(6), 1);
    memory.put(request.offset(8), 0x13);
    memory.put(request.offset(16), 0x7d20f8);
    memory.put(request.offset(28), 0x9566b0);
    memory.put(entries.offset(slot * 8 + 4), request.0);
    memory.put(entries.offset(slot * 8), 0);
    memory.put_half(registry, ((slot + 1) % 16) as u16);
    Ok(StreamSlot(slot as u8))
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use crate::audio_zoom_actor::{InstalledWorker, native::worker};
    use crate::configuration_audio_actors::sound::{self, SoundQueue, StreamSoundCompletion};
    use xm1_power_lifecycle::audio_release::{executor::native::{NativeExecutorMemory, NativeExecutorIo, NativeOperationDiagnostics}, packet_ring::native::InstalledRing, stream_pipeline::native::NativePipeline};
    use xm1_power_lifecycle::audio_release::stream_construction::native::{NativeMemory, NativeIo};
    unsafe fn put(address: u32, value: u32) { core::ptr::write_volatile(address as *mut u32, value); }
    unsafe fn word(address: u32) -> u32 { core::ptr::read_volatile(address as *const u32) }
    pub struct NativeVendorAudio {
        runtime: VendorAudioRuntime,
        worker: InstalledWorker<NativeMemory>,
        slot: Option<StreamSlot>,
        sounds: SoundQueue,
        sound_active: bool,
    }
    impl NativeVendorAudio {
        /// # Safety
        /// The caller exclusively owns the shared audio domain, installed stream state, and service PCM buffers until this runtime is retired.
        pub unsafe fn new() -> Self {
            let worker = worker();
            let mut runtime = VendorAudioRuntime::new();
            runtime.configured = matches!(worker.bank_phase(), crate::audio_zoom_actor::BankPhase::Idle | crate::audio_zoom_actor::BankPhase::Active);
            runtime.codec = AudioZoomActor::adopt(&worker);
            Self { runtime, worker, slot: None, sounds: SoundQueue::new(), sound_active: false }
        }
        pub fn busy(&self) -> bool { self.runtime.busy() || self.runtime.codec.busy() }
        pub fn codec_domain(&mut self) -> (&mut AudioZoomActor, &mut InstalledWorker<NativeMemory>) { (&mut self.runtime.codec, &mut self.worker) }
        pub fn begin_capture(&mut self, seconds: RecordingSeconds, result: ConfigurationKey, now: Tick, timeout: Timeout) -> Result<(), Error> {
            self.begin(Direction::Capture, seconds, result, now, timeout)
        }
        pub fn begin_playback(&mut self, selector: u8, result: ConfigurationKey, now: Tick, timeout: Timeout) -> Result<(), Error> {
            if self.busy() { return Err(Error::Busy); }
            let seconds = RecordingSeconds::parse(unsafe { core::ptr::read_volatile(0x42e40000 as *const u8) }).map_err(|_| Error::Device(-30))?;
            let recording = unsafe { core::slice::from_raw_parts(0x42e40010 as *const u8, seconds.byte_length()) };
            let destination = unsafe { core::slice::from_raw_parts_mut(0x43014c10 as *mut u8, seconds.byte_length()) };
            prepare_playback(selector, seconds, recording, destination).map_err(|_| Error::Device(-30))?;
            self.begin(Direction::Playback, seconds, result, now, timeout)
        }
        pub fn action25(&mut self, keys: &[ConfigurationKey; 2], configuration: &mut ConfigImages<'_>, now: Tick, timeout: Timeout) -> Result<Progress, Error> {
            let selector = keys[0].read(configuration).map_err(|_| Error::Device(-30))?;
            match self.begin_playback(selector, keys[1], now, timeout) {
                Ok(()) => Ok(Progress::Waiting),
                Err(Error::Device(-30)) if selector > 2 => {
                    keys[1].write(configuration, 1).map_err(|_| Error::Device(-30))?;
                    Ok(Progress::Complete(ActionCompletion::service_rejected()))
                }
                Err(error) => Err(error),
            }
        }
        fn begin(&mut self, direction: Direction, seconds: RecordingSeconds, result: ConfigurationKey, now: Tick, timeout: Timeout) -> Result<(), Error> {
            if self.busy() { return Err(Error::Busy); }
            self.prepare_device(direction, None)?;
            if direction == Direction::Capture { unsafe { core::ptr::write_volatile(0x42e40000 as *mut u8, seconds.get()); } }
            self.runtime.begin(direction, seconds, PcmAddress(if direction == Direction::Playback { 0x43014c10 } else { 0x42e40010 }), result,
                &mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, &mut self.worker, now, timeout)
        }
        fn prepare_device(&mut self, direction: Direction, sound: Option<SoundPlayback>) -> Result<(), Error> {
            if self.slot.is_none() { self.slot = Some(register_stream(&mut NativeMemory)?); }
            prepare_transfer(&mut NativeMemory, &mut NativeIo, direction, sound)?;
            Ok(())
        }
        pub fn action22(&mut self, keys: &[ConfigurationKey; 3], configuration: &ConfigImages<'_>) -> Result<ActionCompletion, BackendError> {
            sound::action22_sound(keys, configuration, &mut self.sounds, &unsafe { sound::native::catalog() })
        }
        fn start_sound(&mut self, now: Tick, timeout: Timeout) -> Result<(), Error> {
            let Some(playback) = self.sounds.current_playback() else { return Ok(()); };
            if playback.configure {
                self.prepare_device(Direction::Playback, Some(playback))?;
                self.runtime.begin_sound(playback, &mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, &mut self.worker, now, timeout)?;
            } else {
                self.runtime.continue_sound(playback, &mut NativePipeline::continuation(), &mut InstalledRing)?;
            }
            self.sound_active = true;
            Ok(())
        }
        pub fn poll(&mut self, configuration: &mut ConfigImages<'_>, now: Tick, timeout: Timeout) -> Result<Progress, Error> {
            if !self.busy() || (!self.sound_active && self.runtime.phase == Phase::Holding) { self.start_sound(now, timeout)?; }
            let progress = self.runtime.poll(&mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, &mut self.worker,
                &mut NativePipeline::continuation(), &mut InstalledRing, configuration, now, timeout);
            if !self.runtime.codec.busy() && unsafe { word(0x8faef0) } == 13 { unsafe { put(0x8faef0, word(0x8fc778)); } }
            let progress = progress?;
            if self.sound_active {
                match progress {
                    Progress::SegmentStopped => {
                        self.sound_active = false;
                        self.sounds.stream_completion(StreamSoundCompletion::Stopped);
                        self.start_sound(now, timeout)?;
                        return Ok(Progress::Waiting);
                    }
                    Progress::Complete(_) => {
                        self.sounds.stream_completion(StreamSoundCompletion::Finished);
                        self.sound_active = false;
                        return Ok(Progress::Waiting);
                    }
                    Progress::Waiting => (),
                }
            }
            Ok(progress)
        }
        pub fn event(&mut self, event: StreamEvent, now: Tick, timeout: Timeout) -> Result<(), Error> {
            self.runtime.event(event, &mut NativePipeline::continuation(), &mut InstalledRing)?;
            if event == StreamEvent::Finished {
                let status = stream_construction::resource_release(&mut NativeMemory).0;
                if status != 0 { return Err(Error::Device(status)); }
                self.runtime.transfer_finished(&mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, &mut self.worker, now, timeout)?;
            }
            Ok(())
        }
        pub fn consume_message(&mut self, message: &[u8], now: Tick, timeout: Timeout) -> Result<bool, Error> {
            let Some(event) = self.slot.and_then(|slot| decode_stream_event(slot, message)) else { return Ok(false); };
            self.event(event, now, timeout)?;
            Ok(true)
        }
    }
}

#[cfg(test)]
#[path = "vendor_audio_runtime_tests.rs"]
mod tests;
