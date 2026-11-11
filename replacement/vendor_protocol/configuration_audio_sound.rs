use crate::{backend::{BackendError, ConfigImages}, configuration_actions_low::{ActionCompletion, ConfigurationKey}};
use super::packets::{PcmAddress, PcmByteLength};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoundLevel(u8);
impl SoundLevel {
    pub fn from_sample(value: u8) -> Self { Self((u16::from(value) + 1).min(3) as u8) }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackOperation { Operation0, Operation1, Operation2 }
impl TrackOperation { const fn index(self) -> usize { match self { Self::Operation0 => 0, Self::Operation1 => 1, Self::Operation2 => 2 } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipId(u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackId(u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoundRequest { Clip { clip: ClipId, level: SoundLevel }, Track { track: TrackId, operation: TrackOperation, level: SoundLevel } }
impl SoundRequest {
    pub fn from_samples(enabled: u8, sound: u8, level: u8) -> Result<Option<Self>, BackendError> {
        if enabled != 1 { return Ok(None); }
        let level = SoundLevel::from_sample(level);
        if sound <= 21 { return Ok(Some(Self::Clip { clip: ClipId(sound), level })); }
        let adjusted = sound.wrapping_sub(22);
        if adjusted > 14 { return Err(BackendError::Range); }
        let (track, operation) = match adjusted {
            0 => (2, TrackOperation::Operation0), 1 => (2, TrackOperation::Operation1), 2 => (2, TrackOperation::Operation2),
            3 => (3, TrackOperation::Operation0), 4 => (3, TrackOperation::Operation1), 5 => (3, TrackOperation::Operation2),
            6 => (4, TrackOperation::Operation0), 7 => (4, TrackOperation::Operation1), _ => (4, TrackOperation::Operation2),
        };
        Ok(Some(Self::Track { track: TrackId(track), operation, level }))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoundClip { position: u32, length: u32 }
impl SoundClip {
    pub const fn from_asset(position: PcmAddress, length: PcmByteLength) -> Self { Self { position: position.get(), length: length.get() } }
    pub const fn position(self) -> PcmAddress { PcmAddress::from_abi(self.position) }
    pub const fn length(self) -> PcmByteLength { PcmByteLength::from_abi(self.length) }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TrackSegments { pub begin: SoundClip, pub end: SoundClip }
#[repr(C)]
pub struct SoundTrack { pub operations: [TrackSegments; 3] }
pub struct SoundCatalog<'a> { pub clips: &'a [SoundClip; 22], pub tracks: &'a [SoundTrack; 5] }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoundMode { Clip, Track }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind { Free, Primary, Paired, End }
#[derive(Clone, Copy)]
struct Entry { kind: Kind, mode: SoundMode, operation: TrackOperation, level: SoundLevel, clip: SoundClip }
impl Entry {
    const EMPTY: Self = Self { kind: Kind::Free, mode: SoundMode::Clip, operation: TrackOperation::Operation0, level: SoundLevel(1), clip: SoundClip { position: 0, length: 0 } };
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, Single, Paired, End }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SoundPlayback { pub mode: SoundMode, pub level: SoundLevel, pub clip: SoundClip, pub configure: bool, pub stop_after: bool }

pub struct SoundQueue { entries: [Entry; 16], read: usize, write: usize, phase: Phase }
impl SoundQueue {
    pub const fn new() -> Self { Self { entries: [Entry::EMPTY; 16], read: 0, write: 0, phase: Phase::Idle } }
    fn next(index: usize) -> usize { (index + 1) & 15 }
    fn submit(&mut self, mode: SoundMode, operation: TrackOperation, level: SoundLevel, clip: SoundClip) -> bool {
        if clip.position == 0 || clip.length == 0 { return false; }
        let mut candidate = self.write;
        if self.entries[candidate].kind == Kind::Primary {
            let selected = (1..16).map(|offset| (candidate + offset) & 15).find(|index| self.entries[*index].kind == Kind::Free);
            match selected { Some(index) => candidate = index, None if operation != TrackOperation::Operation1 => return false, None => {} }
        }
        self.write = candidate;
        let primary = Entry { kind: Kind::Primary, mode, operation, level, clip };
        if operation == TrackOperation::Operation1 {
            let next = Self::next(candidate);
            if self.entries[candidate].kind != Kind::Free || self.entries[next].kind != Kind::Free { return false; }
            self.entries[candidate] = primary;
            self.write = next;
            let retained_mode = self.entries[next].mode;
            self.entries[next] = Entry { kind: Kind::Paired, mode: retained_mode, ..primary };
            true
        } else if self.entries[candidate].kind == Kind::Free {
            self.entries[candidate] = primary;
            self.write = Self::next(candidate);
            true
        } else if self.entries[candidate].kind == Kind::Paired {
            if let Some(index) = (1..16).map(|offset| (candidate + offset) & 15).find(|index| self.entries[*index].kind == Kind::Free) {
                self.entries[index] = primary;
                true
            } else { false }
        } else { false }
    }
    pub fn enqueue(&mut self, request: SoundRequest, catalog: &SoundCatalog<'_>) -> bool {
        match request {
            SoundRequest::Clip { clip, level } => {
                let clip = catalog.clips[usize::from(clip.0)];
                if clip.length < 3 { return true; }
                self.submit(SoundMode::Clip, TrackOperation::Operation2, level, clip)
            },
            SoundRequest::Track { track, operation, level } => {
                let segment = catalog.tracks[usize::from(track.0)].operations[operation.index()];
                let accepted = if segment.begin.length < 3 { true } else { self.submit(SoundMode::Track, operation, level, segment.begin) };
                if segment.end.length >= 3 && self.entries[self.write].kind == Kind::Paired {
                    self.entries[self.write].kind = Kind::End;
                    self.entries[self.write].operation = operation;
                    self.entries[self.write].clip = segment.end;
                    self.write = Self::next(self.write);
                }
                accepted
            },
        }
    }
    pub fn current_playback(&mut self) -> Option<SoundPlayback> {
        let entry = self.entries[self.read];
        match entry.kind {
            Kind::Primary => {
                self.phase = if entry.operation == TrackOperation::Operation1 { Phase::Paired } else { Phase::Single };
                Some(SoundPlayback { mode: entry.mode, level: entry.level, clip: entry.clip, configure: true, stop_after: entry.operation != TrackOperation::Operation1 })
            },
            Kind::End if entry.operation == TrackOperation::Operation1 => {
                self.phase = Phase::End;
                Some(SoundPlayback { mode: entry.mode, level: entry.level, clip: entry.clip, configure: false, stop_after: true })
            },
            _ => None,
        }
    }
    pub fn stream_completion(&mut self, completion: StreamSoundCompletion) -> bool {
        let advance = match completion { StreamSoundCompletion::Stopped => self.phase == Phase::Paired, StreamSoundCompletion::Finished => matches!(self.phase, Phase::Single | Phase::End), StreamSoundCompletion::Buffer => false };
        if !advance { return false; }
        self.entries[self.read].kind = Kind::Free;
        self.read = Self::next(self.read);
        if !matches!(self.entries[self.read].kind, Kind::Primary | Kind::End) { self.phase = Phase::Idle; }
        true
    }
    pub fn idle(&self) -> bool { self.phase == Phase::Idle }
    pub fn discard_pending(&mut self) {
        if self.phase == Phase::Idle { return; }
        let paired = Self::next(self.read);
        for index in 0..16 {
            if index != self.read && (self.phase != Phase::Paired || index != paired) { self.entries[index].kind = Kind::Free; }
        }
        self.write = if self.phase == Phase::Paired { Self::next(paired) } else { paired };
    }
}
impl Default for SoundQueue { fn default() -> Self { Self::new() } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamSoundCompletion { Buffer, Stopped, Finished }

pub fn action22_sound(keys: &[ConfigurationKey; 3], configuration: &ConfigImages<'_>, queue: &mut SoundQueue, catalog: &SoundCatalog<'_>) -> Result<ActionCompletion, BackendError> {
    let enabled = keys[0].read(configuration)?;
    if enabled != 1 { return Ok(ActionCompletion::service_success()); }
    let sound = keys[1].read(configuration)?;
    let level = keys[2].read(configuration)?;
    match SoundRequest::from_samples(enabled, sound, level) {
        Ok(Some(request)) => { queue.enqueue(request, catalog); Ok(ActionCompletion::service_success()) },
        Ok(None) => Ok(ActionCompletion::service_success()),
        Err(BackendError::Range) => Ok(ActionCompletion::service_rejected()),
        Err(error) => Err(error),
    }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::{SoundCatalog, SoundClip, SoundTrack};
    /// # Safety
    /// The pinned X-M1 ROM asset tables must be mapped at their installed addresses.
    pub unsafe fn catalog() -> SoundCatalog<'static> {
        SoundCatalog { clips: unsafe { &*(0x7dceb0 as *const [SoundClip; 22]) }, tracks: unsafe { &*(0x810f3c as *const [SoundTrack; 5]) } }
    }
}
