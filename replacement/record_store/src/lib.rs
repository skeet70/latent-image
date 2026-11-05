#![no_std]

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

#[cfg(feature = "guest")]
mod guest;

mod lifecycle;
pub use lifecycle::{Handle, Registration, RegistrationError, Service};

pub const BLOCK_BYTES: usize = 0x20000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum RecordKind { Kind0, Kind1, Kind2, Kind3, Kind4 }
impl RecordKind {
    pub fn parse(value: u32) -> Option<Self> {
        Some(match value { 0 => Self::Kind0, 1 => Self::Kind1, 2 => Self::Kind2, 3 => Self::Kind3, 4 => Self::Kind4, _ => return None })
    }
    pub const fn program_bytes(self) -> ProgramBytes {
        ProgramBytes(match self { Self::Kind0 => 0x60, Self::Kind1 => 0x1000, Self::Kind2 => 0x200, Self::Kind3 | Self::Kind4 => 0x600 })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Bank { Zero, One }
impl Bank { pub const fn next(self) -> Self { match self { Self::Zero => Self::One, Self::One => Self::Zero } } }
macro_rules! quantity { ($($name:ident: $ty:ty),*) => { $(#[derive(Clone, Copy, Debug, Eq, PartialEq)] #[repr(transparent)] pub struct $name(pub $ty);)* }; }
quantity!(Sequence: u16, Seed: u16, RecordBytes: u16, RecordCount: u16, SequenceOffset: u16, ProgramBytes: u16, Position: u16, BytePosition: u32);
impl Sequence {
    pub fn successor(self) -> Self {
        let next = self.0.wrapping_add(1);
        Self(if cfg!(feature = "control-1") { next } else { next & 0x7fff })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryError { Size, Count, SequenceOffset }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Geometry { bytes: RecordBytes, count: RecordCount, sequence_offset: SequenceOffset }
impl Geometry {
    pub fn new(bytes: RecordBytes, count: RecordCount, sequence_offset: SequenceOffset) -> Result<Self, GeometryError> {
        let s = bytes.0 as usize;
        if s == 0 { return Err(GeometryError::Size); }
        if count.0 as usize > BLOCK_BYTES / s { return Err(GeometryError::Count); }
        if sequence_offset.0 & 1 != 0 || sequence_offset.0 as usize + 2 > s { return Err(GeometryError::SequenceOffset); }
        Ok(Self { bytes, count, sequence_offset })
    }
    pub const fn record_bytes(self) -> RecordBytes { self.bytes }
    pub const fn records_per_bank(self) -> RecordCount { self.count }
    pub const fn sequence_offset(self) -> SequenceOffset { self.sequence_offset }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediumError(pub i32);
pub trait RecordMedium {
    fn read(&mut self, kind: RecordKind, bank: Bank, offset: BytePosition, output: &mut [u8]) -> Result<(), MediumError>;
    fn program(&mut self, kind: RecordKind, bank: Bank, offset: BytePosition, bytes: &[u8]) -> Result<(), MediumError>;
    fn erase(&mut self, kind: RecordKind, bank: Bank) -> Result<(), MediumError>;
}
pub trait Staging {
    fn bytes(&mut self, kind: RecordKind) -> &mut [u8];
    fn read_bytes(&mut self, kind: RecordKind, length: usize) -> &mut [u8] { part_mut(self.bytes(kind), 0, length) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckVerdict { Accept, Reject }
pub trait Callbacks { fn check(&mut self, record: &mut [u8]) -> CheckVerdict; fn seal(&mut self, record: &mut [u8]); }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct Area { pub kind: RecordKind, pub geometry: Geometry, pub current: Bank }
impl Area { pub fn register(kind: RecordKind, geometry: Geometry) -> Self { Self { kind, geometry, current: Bank::Zero } } }

pub fn open(staging: &mut impl Staging) {
    for kind in [RecordKind::Kind0, RecordKind::Kind1, RecordKind::Kind2, RecordKind::Kind3, RecordKind::Kind4] { staging.bytes(kind).fill(0xff); }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Candidate { pub bank: Bank, pub position: Position, pub sequence: Sequence, pub skipped: u16 }
#[derive(Debug, Eq, PartialEq)]
pub enum Latest<'a> { Found { candidate: Candidate, record: &'a [u8] }, Empty, NoneValid { output: FailureOutput } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureOutput { Unchanged, Null }
#[derive(Clone, Copy)]
pub enum RecordSource<'a> { Caller(&'a [u8]), Staging }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Placement { InBank, Rotated, ErasedThenWritten }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Persisted { Confirmed, ProgramFailed(MediumError) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Appended { pub bank: Bank, pub position: Position, pub sequence: Sequence, pub placement: Placement, pub persisted: Persisted }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotAppended { DestinationFull { erase: Option<MediumError> } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Erased { pub errors: [Option<MediumError>; 2] }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError { OutsideBank, CrossesBank, Medium(MediumError) }

pub struct Entry<'a, M: RecordMedium, S: Staging> {
    medium: &'a mut M,
    staging: &'a mut S,
    block: &'a mut [u8; BLOCK_BYTES],
    loaded: Option<(RecordKind, Bank)>,
}
impl<'a, M: RecordMedium, S: Staging> Entry<'a, M, S> {
    pub fn new(medium: &'a mut M, staging: &'a mut S, block: &'a mut [u8; BLOCK_BYTES]) -> Self { Self { medium, staging, block, loaded: None } }
    fn load(&mut self, kind: RecordKind, bank: Bank) {
        if self.loaded != Some((kind, bank)) {
            let _ = self.medium.read(kind, bank, BytePosition(0), self.block);
            self.loaded = Some((kind, bank));
        }
    }
    fn seek(&mut self, area: &Area, position: Position) -> Sequence {
        self.load(area.kind, area.current);
        let s = area.geometry.bytes.0 as usize;
        let offset = position.0 as usize * s;
        let bytes = self.staging.bytes(area.kind);
        copy(part_mut(bytes, 0, s), part(self.block, offset, s));
        sequence(bytes, area.geometry.sequence_offset)
    }
    fn find_last(&mut self, area: &Area) -> Option<Position> {
        let mut low = 0i32;
        let mut high = area.geometry.count.0 as i32 - 1;
        loop {
            let p = Position(((low + high) / 2) as u16);
            let seq = self.seek(area, p);
            if low >= high {
                // RSC 0x6acfa8: the terminating midpoint remains the cursor.
                return if seq.0 != 0xffff { Some(p) } else if p.0 > 0 { let prev = Position(p.0 - 1); self.seek(area, prev); Some(prev) } else { None };
            }
            if seq.0 == 0xffff { high = p.0 as i32 - 1; } else { low = p.0 as i32 + 1; }
        }
    }
    fn valid(&mut self, area: &Area, callbacks: &mut impl Callbacks) -> (Option<Candidate>, u16, bool) {
        let Some(mut p) = self.find_last(area) else { return (None, 0, false) };
        let mut skipped = 0;
        loop {
            let bytes = self.staging.bytes(area.kind);
            let accepted = callbacks.check(part_mut(bytes, 0, area.geometry.bytes.0 as usize)) == CheckVerdict::Accept;
            if accepted || cfg!(feature = "control-5") {
                return (Some(Candidate { bank: area.current, position: p, sequence: sequence(bytes, area.geometry.sequence_offset), skipped }), skipped, true);
            }
            if p.0 == 0 || cfg!(feature = "control-4") { return (None, skipped, true); }
            p.0 -= 1;
            skipped += 1;
            self.seek(area, p);
        }
    }
    fn latest_candidate(&mut self, area: &mut Area, callbacks: &mut impl Callbacks, mut recovery: Option<&mut u16>) -> (Option<Candidate>, bool, FailureOutput) {
        let mut best: Option<Candidate> = None;
        let mut occupied = false;
        for _ in 0..3 {
            let (candidate, _, seen) = self.valid(area, callbacks);
            occupied |= seen;
            if let Some(candidate) = candidate {
                if let Some(previous) = best {
                    let c = candidate.sequence.0 as i32;
                    let b = previous.sequence.0 as i32;
                    let t = area.geometry.count.0 as i32 * 2;
                    let newer = if cfg!(feature = "control-7") { c > b } else { (c <= b && b - c >= t) || (c > b && c - b <= 0x8000 - t) };
                    if !newer { break; }
                }
                best = Some(candidate);
            }
            area.current = area.current.next();
        }
        if let Some(best) = best {
            if let Some(output) = recovery.as_deref_mut() { *output = best.skipped; }
            let full = if cfg!(feature = "control-6") { area.geometry.count.0 } else { area.geometry.count.0.wrapping_sub(1) };
            if recovery.is_some() && best.position.0 == full {
                area.current = best.bank.next();
                let (probe, skipped, _) = self.valid(area, callbacks);
                if probe.is_none() && skipped != 0 { if let Some(output) = recovery { *output = best.skipped; } }
            }
            area.current = best.bank;
            (self.valid(area, callbacks).0, occupied, FailureOutput::Null)
        } else { (None, occupied, FailureOutput::Unchanged) }
    }
    pub fn read_latest<'b>(&'b mut self, area: &mut Area, callbacks: &mut impl Callbacks, recovery: Option<&mut u16>) -> Latest<'b> {
        match self.latest_candidate(area, callbacks, recovery) {
            (Some(candidate), _, _) => Latest::Found { candidate, record: part(self.staging.bytes(area.kind), 0, area.geometry.bytes.0 as usize) },
            (None, false, _) => Latest::Empty,
            (None, true, output) => Latest::NoneValid { output },
        }
    }
    fn inner_append(&mut self, area: &Area, callbacks: &mut impl Callbacks, seed: Seed, source: RecordSource<'_>, placement: Placement) -> Option<Appended> {
        let (position, seq) = match self.find_last(area) {
            Some(p) => {
                if p.0 as i32 >= area.geometry.count.0 as i32 - 1 { return None; }
                let seq = sequence(self.staging.bytes(area.kind), area.geometry.sequence_offset).successor();
                (Position(p.0 + 1), seq)
            }
            None => (Position(0), Sequence(seed.0)),
        };
        self.seek(area, position);
        let s = area.geometry.bytes.0 as usize;
        let q = area.geometry.sequence_offset.0 as usize;
        let bytes = self.staging.bytes(area.kind);
        if let RecordSource::Caller(source) = source { copy(part_mut(bytes, 0, q), part(source, 0, q)); }
        copy(part_mut(bytes, q, 2), &seq.0.to_le_bytes());
        if let RecordSource::Caller(source) = source { copy(part_mut(bytes, q + 2, s - q - 2), part(source, q + 2, s - q - 2)); }
        callbacks.seal(part_mut(bytes, 0, s));
        let length = if cfg!(feature = "control-9") { s } else { area.kind.program_bytes().0 as usize };
        let result = self.medium.program(area.kind, area.current, BytePosition(position.0 as u32 * s as u32), part(bytes, 0, length));
        self.loaded = None;
        Some(Appended { bank: area.current, position, sequence: seq, placement, persisted: match result { Ok(()) => Persisted::Confirmed, Err(error) => Persisted::ProgramFailed(error) } })
    }
    pub fn append(&mut self, area: &mut Area, callbacks: &mut impl Callbacks, initial: Seed, source: &[u8]) -> Result<Appended, NotAppended> {
        self.append_source(area, callbacks, initial, RecordSource::Caller(source))
    }
    pub fn append_source(&mut self, area: &mut Area, callbacks: &mut impl Callbacks, initial: Seed, source: RecordSource<'_>) -> Result<Appended, NotAppended> {
        let queried = area.current;
        let latest = self.latest_candidate(area, callbacks, None).0;
        let seed = if let Some(latest) = latest {
            let seed = Seed(latest.sequence.successor().0);
            if let Some(result) = self.inner_append(area, callbacks, seed, source, Placement::InBank) { return Ok(result); }
            if cfg!(feature = "control-2") { return Err(NotAppended::DestinationFull { erase: None }); }
            area.current = if cfg!(feature = "control-3") { latest.bank.next() } else { queried.next() };
            seed
        } else { area.current = Bank::Zero; initial };
        if let Some(result) = self.inner_append(area, callbacks, seed, source, Placement::Rotated) { return Ok(result); }
        let erase = self.medium.erase(area.kind, area.current).err();
        self.loaded = None;
        self.inner_append(area, callbacks, seed, source, Placement::ErasedThenWritten).ok_or(NotAppended::DestinationFull { erase })
    }
    pub fn erase_all(&mut self, area: &mut Area) -> Erased {
        let mut errors = [None; 2];
        for error in errors.iter_mut().take(if cfg!(feature = "control-8") { 1 } else { 2 }) {
            *error = self.medium.erase(area.kind, area.current).err();
            self.loaded = None;
            area.current = area.current.next();
        }
        Erased { errors }
    }
    pub fn collect(&mut self, area: &mut Area, output: &mut [u8]) -> usize {
        let s = area.geometry.bytes.0 as usize;
        let n = area.geometry.count.0 as usize;
        let q = area.geometry.sequence_offset;
        let chunk = s * n;
        for bank in 0..2 {
            let mut seq = self.seek(area, Position(0));
            let mut unused = false;
            for p in 0..n {
                unused |= seq.0 == 0xffff;
                let dest = part_mut(output, bank * chunk + p * s, s);
                if unused { part_mut(dest, q.0 as usize, 2).fill(0xff); }
                else {
                    copy(dest, part(self.staging.bytes(area.kind), 0, s));
                    seq = self.seek(area, Position((p + 1) as u16));
                }
            }
            area.current = area.current.next();
        }
        if sequence(part(output, chunk, chunk), q).0 < sequence(output, q).0 {
            let Some((first, second)) = output.split_at_mut_checked(chunk) else { contract_violation() };
            for (left, right) in first.iter_mut().zip(second) { core::mem::swap(left, right); }
        }
        let mut written = 0;
        for p in 0..n * 2 {
            if sequence(part(output, p * s, s), q).0 != 0xffff {
                if written != p * s {
                    let Some((prefix, suffix)) = output.split_at_mut_checked(p * s) else { contract_violation() };
                    copy(part_mut(prefix, written, s), part(suffix, 0, s));
                }
                written += s;
            }
        }
        written
    }
    pub fn read_at(&mut self, kind: RecordKind, position: BytePosition, output: &mut [u8]) -> Result<(), ReadError> {
        if position.0 >= BLOCK_BYTES as u32 * 2 { if let Some(first) = output.first_mut() { *first = 0; } return Err(ReadError::OutsideBank); }
        let offset = position.0 as usize % BLOCK_BYTES;
        if output.len() > BLOCK_BYTES - offset { output.fill(0); return Err(ReadError::CrossesBank); }
        let bank = if position.0 < BLOCK_BYTES as u32 { Bank::Zero } else { Bank::One };
        self.load(kind, bank);
        let stage = self.staging.read_bytes(kind, output.len());
        copy(part_mut(stage, 0, output.len()), part(self.block, offset, output.len()));
        copy(output, part(stage, 0, output.len()));
        Ok(())
    }
}
fn sequence(bytes: &[u8], offset: SequenceOffset) -> Sequence {
    let [low, high] = part(bytes, offset.0 as usize, 2) else { contract_violation() };
    Sequence(u16::from_le_bytes([*low, *high]))
}
fn part(bytes: &[u8], start: usize, length: usize) -> &[u8] {
    bytes.get(start..start + length).unwrap_or_else(|| contract_violation())
}
fn part_mut(bytes: &mut [u8], start: usize, length: usize) -> &mut [u8] {
    bytes.get_mut(start..start + length).unwrap_or_else(|| contract_violation())
}
fn copy(dest: &mut [u8], source: &[u8]) {
    if dest.len() != source.len() { contract_violation(); }
    for (dest, source) in dest.iter_mut().zip(source) { *dest = *source; }
}
#[cold]
#[inline(never)]
fn contract_violation() -> ! {
    #[cfg(feature = "guest")]
    loop { core::hint::spin_loop(); }
    #[cfg(not(feature = "guest"))]
    panic!("record-store buffer contract violated");
}
