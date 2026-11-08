//! Continuous constructor allocation lifetimes for X-M1 1.50 render observations.
use fujimod_emulator::board::observation::InstructionPoints;
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Register, arm};
use serde::{Serialize, Serializer};

const SITES: [u64; 6] = [0x6dcc44, 0x6dce5c, 0x6dcf20, 0x117d804, 0x117d760, 0x89cdc4];
const EVENT_LIMIT: usize = 65536;

#[derive(Clone, Copy, Serialize)]
pub struct Boundary { instructions: u64, tick: u64 }
impl Boundary {
    pub(crate) fn sample(session: &Session) -> Result<Self, String> {
        Ok(Self { instructions: crate::trace::InstructionCount::from_board(session)?.value(), tick: session.board.now() })
    }
}
#[derive(Clone, Copy)]
struct Address(u32);
impl Serialize for Address {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("0x{:08x}", self.0))
    }
}
#[derive(Clone, Copy, Serialize)]
enum Constructor {
    #[serde(rename = "image_d4")] ImageD4,
    #[serde(rename = "vector_14c")] Vector14c,
    #[serde(rename = "vector_d0")] VectorD0,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Kind {
    Allocate { constructor: Constructor, allocation_base: Address, extent: u32, ordinal: u64 },
    Bind { object_base: Address, allocation_base: Address },
    Free { allocation_base: Address },
}
#[derive(Serialize)]
struct Event {
    sequence: u64,
    #[serde(flatten)] boundary: Boundary,
    core: usize,
    #[serde(flatten)] kind: Kind,
}
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum Coverage { Continuous, Refused }
#[derive(Serialize)]
pub struct Batch {
    version: u32, start: Boundary, end: Boundary,
    sequence_start: u64, sequence_end: u64,
    overflow: bool, coverage: Coverage, events: Vec<Event>,
}
struct CorePoints { points: Box<InstructionPoints>, resume: Option<(u64, u64)> }
pub struct Collector {
    points: Vec<CorePoints>, start: Boundary, sequence_start: u64, sequence: u64,
    maps: Vec<MemoryMap>,
    suspended_resume: Vec<Option<(u64, u64)>>,
    ordinals: [u64; 3], events: Vec<Event>, overflow: bool, refused: bool,
}
impl Collector {
    pub(crate) fn begin(session: &Session, maps: Vec<MemoryMap>) -> Result<Self, String> {
        let mut collector = Self { points: Vec::new(), start: Boundary::sample(session)?, sequence_start: 0,
            sequence: 0, ordinals: [0; 3], events: Vec::new(), overflow: false, refused: false, maps,
            suspended_resume: vec![None; session.board.instruction_counts().len()] };
        collector.rearm(session)?;
        Ok(collector)
    }
    pub(crate) fn disarm(&mut self, session: &Session) -> Result<(), String> {
        while let Some(core) = self.points.len().checked_sub(1) {
            session.board.instruction_points_end(core, &mut self.points[core].points)?;
            self.suspended_resume[core] = self.points[core].resume;
            self.points.pop();
        }
        Ok(())
    }
    pub(crate) fn rearm(&mut self, session: &Session) -> Result<(), String> {
        for core in 0..session.board.instruction_counts().len() {
            match session.board.instruction_points_begin(core, SITES.to_vec()) {
                Ok(points) => self.points.push(CorePoints { points, resume: self.suspended_resume[core] }),
                Err(error) => { self.disarm(session)?; return Err(error); }
            }
        }
        Ok(())
    }
    pub fn refuse_coverage(&mut self) { self.refused = true; }
    pub(crate) fn sequence(&self) -> u64 { self.sequence }
    pub(crate) fn begin_advance(&self, session: &Session) -> Result<(), String> {
        let counts = session.board.instruction_counts();
        for (core, state) in self.points.iter().enumerate() {
            let pc = session.board.read_register(core, Register::Pc)?;
            let skip = state.resume.filter(|&(address, count)| address == pc && count == counts[core]);
            state.points.resume(pc, skip.map_or(!pc, |(address, _)| address));
        }
        Ok(())
    }
    pub(crate) fn collect(&mut self, session: &Session) -> Result<(), String> {
        let boundary = Boundary::sample(session)?;
        let counts = session.board.instruction_counts();
        for core in 0..self.points.len() {
            let Some(site) = self.points[core].points.event() else { continue };
            let expected: u32 = match site {
                0x6dcc44 | 0x6dce5c | 0x6dcf20 => 0xe2505000,
                0x117d804 | 0x117d760 => 0xe5840004,
                0x89cdc4 => 0xe92d4008,
                _ => return Err("unknown render provenance source".into()),
            };
            let mut bytes = [0; 4];
            crate::trace::read_identity_bytes(session, core, site as u32, &mut bytes, &self.maps)?;
            if u32::from_le_bytes(bytes) != expected {
                self.refused = true;
                return Err("render provenance source instruction differs".into());
            }
            let read = |index| session.board.read_register(core, Register::Arm32(arm::Register::R(index)))
                .and_then(|value| u32::try_from(value).map_err(|_| "render register exceeds32bits".into()));
            let base = read(0)?;
            let object = if site == SITES[3] || site == SITES[4] { read(4)? } else { 0 };
            self.record(site, base, object, core, boundary)?;
            let pc = session.board.read_register(core, Register::Pc)?;
            self.points[core].points.resume(pc, site);
            self.points[core].resume = (pc == site).then_some((site, counts[core]));
        }
        Ok(())
    }
    fn record(&mut self, site: u64, base: u32, object: u32, core: usize, boundary: Boundary) -> Result<(), String> {
        if base == 0 { return Ok(()) }
        let kind = match site {
            0x6dcc44 | 0x6dce5c | 0x6dcf20 => {
                let (index, constructor, extent) = match site {
                    0x6dcc44 => (0, Constructor::ImageD4, 212),
                    0x6dce5c => (1, Constructor::Vector14c, 332),
                    _ => (2, Constructor::VectorD0, 208),
                };
                self.ordinals[index] = self.ordinals[index].checked_add(1).ok_or("render ordinal overflow")?;
                Kind::Allocate { constructor, allocation_base: Address(base), extent, ordinal: self.ordinals[index] }
            }
            0x117d804 | 0x117d760 if object != 0 => Kind::Bind { object_base: Address(object), allocation_base: Address(base) },
            0x89cdc4 => Kind::Free { allocation_base: Address(base) },
            _ => return Ok(()),
        };
        if self.events.len() == EVENT_LIMIT || self.sequence == u64::MAX {
            self.overflow = true;
            self.refused = true;
            return Err("render provenance event bound exceeded".into());
        }
        self.events.push(Event { sequence: self.sequence, boundary, core, kind });
        self.sequence += 1;
        Ok(())
    }
    pub fn drain(&mut self, session: &Session) -> Result<Batch, String> {
        let end = Boundary::sample(session)?;
        let batch = Batch { version: 1, start: self.start, end, sequence_start: self.sequence_start,
            sequence_end: self.sequence, overflow: self.overflow,
            coverage: if self.refused { Coverage::Refused } else { Coverage::Continuous }, events: std::mem::take(&mut self.events) };
        self.start = end;
        self.sequence_start = self.sequence;
        Ok(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_refuses_without_discarding_prior_evidence() {
        let boundary = Boundary { instructions: 0, tick: 0 };
        let mut collector = Collector { points: Vec::new(), start: boundary, sequence_start: 0,
            sequence: 0, ordinals: [0; 3], events: Vec::new(), overflow: false, refused: false,
            maps: Vec::new(), suspended_resume: Vec::new() };
        for _ in 0..EVENT_LIMIT {
            collector.record(0x89cdc4, 0x1234, 0, 0, boundary).unwrap();
        }
        assert!(collector.record(0x89cdc4, 0x1234, 0, 0, boundary).is_err());
        let evidence = serde_json::to_value(Batch { version: 1, start: boundary, end: boundary,
            sequence_start: 0, sequence_end: collector.sequence, overflow: collector.overflow,
            coverage: if collector.refused { Coverage::Refused } else { Coverage::Continuous },
            events: collector.events }).unwrap();
        assert_eq!(evidence["coverage"], "refused");
        assert_eq!(evidence["overflow"], true);
        assert_eq!(evidence["events"].as_array().unwrap().len(), EVENT_LIMIT);
        assert_eq!(evidence["sequence_end"], EVENT_LIMIT as u64);
    }
}
