use super::{Catalog, Core, Event, Guest, SourceKind};
use super::continuation_view::{self, ByteCount, Digest, EventId, FileBinding};
use super::image5_program::{CARRIER, Program};
use super::image5_wire::{Evidence, Frame, Limit, Progress, Region, Sample, SampleKind, StackLease};
use fujimod_emulator::board::{factory::MemoryMap, observation::{CpuDataAttribution, FetchAttribution, WatchKinds, WatchRange}};
use fujimod_xm1::machine::Machine;
use std::path::Path;

pub(super) const PAYLOAD_SHA256: &str = "8c6f806ef4554128e5be887a76e438a91b7d6cb030c940fc1c8fd6d32d5926c0";
/// Payload offset of the segment5 compressed record, flash offset 0x2c0000 after the 0x48-byte payload header.
pub(super) const RECORD: u32 = 0x2c0048;
pub(super) const RECORD_HEADER: [u32; 5] = [0x4f1e60, 0x2c66d1, 0xb2, 0x4000, 4];
pub(super) const STAGING: [u32; 2] = [0x3e40000, 0x4120000];
pub(super) const BLOCK: u32 = 0x20000;
pub(super) const DESTINATION: [u32; 2] = [0xbf1a00, 0x10e3860];
pub(super) const BSS: [u32; 2] = [0x1460000, 0x171be40];
pub(super) const HALO: u32 = 0x100;
pub(super) const DECODER_GLOBALS: [[u32; 2]; 2] = [[0x8e1900, 0x8e1920], [0x9c2f54, 0x9c2f5c]];
pub(super) const STACK_GUARD: [u32; 2] = [0xa1b500, 0xa1c300];
pub(super) const CALLER: Guest = Guest(0x73b4ac);
pub(super) const CALLER_RETURN: Guest = Guest(0x73b4b0);
pub(super) const IMAGE5_ENTRY: Guest = Guest(0xd985a4);
pub(super) const TASK1_ENTRY: Guest = Guest(0xc22f60);
pub(super) const SEGMENT_WORDS: usize = 9;
/// Decoder-internal resource 0x48 boundaries: IRQ98 completion entry, its give and return, then the take/give call sites in init and finish with their continuations.
pub(super) const DECODER_SITES: [u32; 11] = [0x6ae310, 0x6ae358, 0x6ae35c, 0x6ae3e8, 0x6ae3ec, 0x6ae440, 0x6ae444, 0x6ae4e0, 0x6ae4e4, 0x6ae4ec, 0x6ae4f0];
/// Flashloader publication after the producer returns: done store, resource release call, request callback BLX.
pub(super) const PUBLICATION_SITES: [u32; 3] = [0x73b6ac, 0x73b6c4, 0x73b6e8];

/// Largest admitted word projection.
pub(super) const CARRIER_CHUNK_WORDS: usize = 80;
pub(super) fn carrier_chunks() -> Vec<(Guest, usize)> {
    let words = ((CARRIER[1] - CARRIER[0]) / 4) as usize;
    (0..words).step_by(CARRIER_CHUNK_WORDS).map(|first| (Guest(CARRIER[0] + 4 * first as u32), CARRIER_CHUNK_WORDS.min(words - first))).collect()
}
pub(super) fn bind_payload(analysis: &Path, output: &Path) -> Result<(FileBinding, Vec<u8>), String> {
    let path = analysis.join("payload.bin");
    let bytes = fujimod_xm1::bridge::read(&path)?;
    let sha256 = Digest::of(&bytes);
    if sha256.as_str() != PAYLOAD_SHA256 { return Err("image5 canonical payload identity differs".into()); }
    let record = record(&bytes)?.to_vec();
    let name = "input-payload.bin";
    std::fs::hard_link(&path, output.join(name)).map_err(|error| format!("cannot retain canonical image5 payload: {error}"))?;
    Ok((FileBinding { path: name.into(), sha256 }, record))
}
/// The staged compressed bytes: the record and the rest of its 23 storage blocks.
pub(super) fn record(payload: &[u8]) -> Result<&[u8], String> {
    let staged = payload.get(RECORD as usize..(RECORD + STAGING[1] - STAGING[0]) as usize).ok_or("image5 record exceeds payload")?;
    let header: Vec<u32> = staged[..20].chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect();
    if header != RECORD_HEADER { return Err("image5 compressed record header differs".into()); }
    Ok(staged)
}
pub(super) struct Runtime {
    pub program: Program,
    pub evidence: Evidence,
    record: Vec<u8>,
    committed: u32,
    entry_sp: Option<Guest>,
    image5_seen: bool,
    task1_seen: bool,
}
impl Runtime {
    pub(super) fn new(program: Program, payload: FileBinding, record: Vec<u8>) -> Self {
        let binding = program.binding();
        Self { program, evidence: Evidence { binding, payload, code: None, caller_before: None, entry: None, caller_return: None, progress: Vec::new(), samples: Vec::new(), frames: Vec::new(), stack: None, limits: vec![Limit::PausedMappingSamplesNotTransientRemapExclusion, Limit::FrameSamplesNotContinuousStackConfinement, Limit::FiniteDestinationHaloNotWholeMachineWriteConfinement, Limit::RetainedServiceAndCallerLifetimeOnly] }, record, committed: 0, entry_sp: None, image5_seen: false, task1_seen: false }
    }
    pub(super) fn sites(&self) -> Vec<Guest> {
        let mut sites = vec![CALLER, CALLER_RETURN, IMAGE5_ENTRY, TASK1_ENTRY];
        sites.extend(DECODER_SITES.into_iter().chain(PUBLICATION_SITES).map(Guest));
        sites.extend((CARRIER[0]..CARRIER[1]).step_by(4).flat_map(|pc| [Guest(pc), Guest(pc | 0x40000000)]));
        sites
    }
    pub(super) fn classes(&self) -> Vec<WatchRange> {
        let range = |[start, end]: [u32; 2], kinds| WatchRange { start: u64::from(start), end: u64::from(end), kinds };
        let mut classes = vec![
            range(CARRIER, WatchKinds::CPU_FETCH.union(WatchKinds::WRITES)),
            range([0x61c000, 0x61c220], WatchKinds::CPU_READ),
            range([0x89f8ac, 0x89f8b0], WatchKinds::CPU_READ),
            range([STAGING[0] - HALO, STAGING[1] + HALO], WatchKinds::WRITES),
            range(STACK_GUARD, WatchKinds::WRITES),
        ];
        for [start, end] in [DESTINATION, BSS] {
            classes.push(range([start - HALO, start], WatchKinds::WRITES));
            classes.push(range([start, end], WatchKinds::HOST_WRITE.union(WatchKinds::MODEL_BUS_WRITE)));
            classes.push(range([end, end + HALO], WatchKinds::WRITES));
        }
        classes.extend(DECODER_GLOBALS.map(|span| range(span, WatchKinds::WRITES)));
        classes
    }
    pub(super) fn attribution(&self, catalog: &Catalog, machine: &Machine) -> Result<FetchAttribution, String> {
        let mut ranges: Vec<_> = catalog.0.iter().map(|image| WatchRange { start: u64::from(image.binding.start.0), end: u64::from(image.binding.start.0) + image.bytes.len() as u64, kinds: WatchKinds::CPU_FETCH }).collect();
        let rom = &machine.document.boot_rom;
        ranges.push(WatchRange { start: rom.address.0, end: rom.address.0.checked_add(rom.bytes as u64).ok_or("image5 ROM attribution range overflows")?, kinds: WatchKinds::CPU_FETCH });
        FetchAttribution::new(CpuDataAttribution::ReadsAndWrites, ranges)
    }
    fn region(&self, machine: &Machine, maps: &[MemoryMap], output: &Path, event: EventId, kind: SampleKind, region: Region, catalog: &Catalog) -> Result<Sample, String> {
        let ([start, end], expected, label) = match region {
            Region::Destination => {
                let source = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image5).ok_or("image5 source absent")?;
                (DESTINATION, Some(source.bytes.as_slice()), "image5-destination")
            }
            Region::Bss => (BSS, None, "image5-bss-range"),
        };
        let view = continuation_view::snapshot_range(&machine.session, maps, output, event, Guest(start), ByteCount(end - start), label, expected)?;
        Ok(Sample { event, kind, region, view })
    }
    fn interval(&self) -> bool { self.evidence.entry.is_some() && self.evidence.caller_return.is_none() }
    pub(super) fn capture(&mut self, machine: &Machine, maps: &[MemoryMap], output: &Path, event: &mut Event, catalog: &Catalog) -> Result<(), String> {
        let pc = event.pc;
        if !self.program.execution_allowed(pc) { return Err("image5 fetch lies outside reviewed emitted code or uses an alias".into()); }
        if pc == CALLER && self.evidence.entry.is_none() { self.evidence.caller_before = Some(event.id); }
        if pc == Guest(CARRIER[0]) {
            if self.evidence.entry.is_some() { return Err("image5 producer entered more than once".into()); }
            self.evidence.entry = Some(event.id);
            self.entry_sp = Some(event.registers.sp);
            for (start, count) in carrier_chunks() { event.state.push(continuation_view::words(&machine.session, maps, start, count)?); }
            let image = catalog.0.iter().find(|image| image.binding.kind == SourceKind::Image4).ok_or("image5 carrier source absent")?;
            let low = (CARRIER[0] - image.binding.start.0) as usize;
            let expected = image.bytes.get(low..low + (CARRIER[1] - CARRIER[0]) as usize).ok_or("image5 carrier source extent")?;
            let code = continuation_view::snapshot_range(&machine.session, maps, output, event.id, pc, ByteCount(CARRIER[1] - CARRIER[0]), "image5-carrier-entry", Some(expected))?;
            if !code.expected_equal { return Err("image5 entry carrier differs from every reviewed code byte".into()); }
            self.evidence.code = Some(code);
            let current = continuation_view::words(&machine.session, maps, Guest(0x40054760), 1)?;
            let task = continuation_view::words(&machine.session, maps, Guest(current.values[0]), 3)?;
            let creation = continuation_view::words(&machine.session, maps, Guest(task.values[2]), 10)?;
            let base = creation.values[5] & !0x40000000;
            let size = creation.values[4];
            let end = base.checked_add(size).ok_or("image5 task stack allocation overflows")?;
            if event.core != Core::Primary || creation.values[0] != 2 || creation.values[8] != 60 || size != 0xc00 || base != 0xa1b600 || end != 0xa1c200 || event.registers.sp.0 != 0xa1c1e0 { return Err("image5 caller is not the admitted flashloader task frame and stack allocation".into()); }
            let view = continuation_view::snapshot_range(&machine.session, maps, output, event.id, Guest(base), ByteCount(size), "image5-stack-entry", None)?;
            self.evidence.stack = Some(StackLease { event: event.id, current, task, creation, view });
        }
        if let (Some(sp), true) = (self.entry_sp, self.interval()) {
            event.state.push(continuation_view::words(&machine.session, maps, sp, 8)?);
            event.state.push(continuation_view::words(&machine.session, maps, Guest(0x40054760), 3)?);
            event.state.push(continuation_view::words(&machine.session, maps, Guest(0x400e4720), 3)?);
            let bytes = self.program.frame_bytes();
            let start = sp.0.checked_sub(bytes).ok_or("image5 frame guard underflows")?;
            let words = continuation_view::words(&machine.session, maps, Guest(start), (bytes / 4) as usize)?;
            self.evidence.frames.push(Frame { event: event.id, words });
        }
        if self.interval() {
            if self.program.returned_target(pc) == Some(Guest(0x657114)) && event.registers.r[0] != 0 {
                event.state.push(continuation_view::words(&machine.session, maps, Guest(event.registers.r[0]), SEGMENT_WORDS)?);
            }
            if pc == Guest(0x42188) && (CARRIER[0]..CARRIER[1]).contains(&event.registers.lr.0) {
                event.state.push(continuation_view::words(&machine.session, maps, Guest(event.registers.r[0]), 2)?);
            }
            if pc == super::image5_program::FEED_CALLBACK {
                let cumulative = event.registers.r[0];
                let fresh = if event.registers.lr == Guest(0x6f1a1c) && cumulative > self.committed {
                    if cumulative != self.committed + BLOCK || cumulative > STAGING[1] - STAGING[0] { return Err("image5 storage progress does not identify one fresh staging block".into()); }
                    let expected = self.record.get(self.committed as usize..cumulative as usize).ok_or("image5 staging block exceeds record")?;
                    let view = continuation_view::snapshot_range(&machine.session, maps, output, event.id, Guest(STAGING[0] + self.committed), ByteCount(BLOCK), "image5-fresh-block", Some(expected))?;
                    self.committed = cumulative;
                    Some(view)
                } else { None };
                self.evidence.progress.push(Progress { event: event.id, cumulative: ByteCount(cumulative), fresh });
            }
            if self.program.returned_target(pc) == Some(Guest(0x6ae3f4)) {
                let sample = self.region(machine, maps, output, event.id, SampleKind::DecoderFinish, Region::Destination, catalog)?;
                self.evidence.samples.push(sample);
            }
        }
        if pc == CALLER_RETURN && self.interval() {
            self.evidence.caller_return = Some(event.id);
            for region in [Region::Destination, Region::Bss] { let sample = self.region(machine, maps, output, event.id, SampleKind::ProducerReturn, region, catalog)?; self.evidence.samples.push(sample); }
        }
        if pc == IMAGE5_ENTRY && !self.image5_seen {
            self.image5_seen = true;
            for region in [Region::Destination, Region::Bss] { let sample = self.region(machine, maps, output, event.id, SampleKind::Image5Entry, region, catalog)?; self.evidence.samples.push(sample); }
        }
        if pc == TASK1_ENTRY && !self.task1_seen {
            self.task1_seen = true;
            let address = Guest(match event.core { Core::Primary => 0x40054760, Core::Secondary => 0x400e4720 });
            let current = continuation_view::words(&machine.session, maps, address, 3)?;
            let task = continuation_view::words(&machine.session, maps, Guest(current.values[0]), 3)?;
            let creation = continuation_view::words(&machine.session, maps, Guest(task.values[2]), 10)?;
            event.state.extend([current, task, creation]);
        }
        if pc == self.program.null_refusal() { return Err("image5 producer reached the missing-segment nonreturning path".into()); }
        Ok(())
    }
}
