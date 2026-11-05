//! Trace sites and a classified write watch armed at the zero-instruction reset of the RAM-push
//! bridge, with one watch segment per attach phase.
use crate::bridge::sha256;
use crate::ff80::encode_hex;
use crate::machine::Machine;
use crate::run;
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_emulator::board::observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use crate::trace::calls::{Calls, Capture};

const SCHEMA: u32 = 1;
const WATCH_LIMIT: usize = 1 << 20;
const WRITE_RANGE_LIMIT: usize = 8;

/// The `--reset-observation` document. Sites map `0x`-prefixed hexadecimal guest addresses to labels;
/// write ranges are half-open physical ranges.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSpec {
    schema: u32,
    sites: BTreeMap<String, String>,
    hit_limit: NonZeroUsize,
    write_ranges: Vec<[u32; 2]>,
    watch_limit: usize,
    #[serde(default)]
    read_ranges: Vec<[u32; 2]>,
    #[serde(default)]
    fetch_ranges: Vec<[u32; 2]>,
    #[serde(default)]
    captures: BTreeMap<String, Capture>,
}

pub struct Spec {
    sites: BTreeMap<u32, String>,
    hit_limit: NonZeroUsize,
    write_ranges: Vec<[u32; 2]>,
    watch_limit: usize,
    read_ranges: Vec<[u32; 2]>,
    fetch_ranges: Vec<[u32; 2]>,
    captures: BTreeMap<u32, Capture>,
}

impl Spec {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let raw: RawSpec = serde_json::from_slice(bytes).map_err(|error| format!("reset observation: {error}"))?;
        if raw.schema != SCHEMA {
            return Err("reset observation schema differs".into());
        }
        if raw.sites.is_empty() {
            return Err("reset observation arms no site".into());
        }
        let sites = raw.sites.into_iter().map(|(address, label)| {
            let digits = address.strip_prefix("0x").ok_or_else(|| format!("site address {address} lacks 0x"))?;
            let parsed = u32::from_str_radix(digits, 16).map_err(|_| format!("site address {address} is not hexadecimal"))?;
            Ok((parsed, label))
        }).collect::<Result<BTreeMap<_, _>, String>>()?;
        if raw.write_ranges.is_empty() || raw.write_ranges.len() > WRITE_RANGE_LIMIT
            || raw.write_ranges.iter().any(|range| range[0] >= range[1]) {
            return Err(format!("reset observation needs 1..{WRITE_RANGE_LIMIT} increasing write ranges"));
        }
        if !(1..=WATCH_LIMIT).contains(&raw.watch_limit) {
            return Err(format!("reset observation watch limit must be 1..{WATCH_LIMIT}"));
        }
        for range in raw.read_ranges.iter().chain(&raw.fetch_ranges) {
            if range[0] >= range[1] { return Err("invalid reset read/fetch range".into()); }
        }
        let captures = raw.captures.into_iter().map(|(site, capture)| {
            let address = u32::from_str_radix(site.strip_prefix("0x").ok_or("capture site lacks 0x")?, 16)
                .map_err(|_| "capture site is not hexadecimal")?;
            if !sites.contains_key(&address) { return Err("capture site is not armed"); }
            Ok((address, capture))
        }).collect::<Result<BTreeMap<_, _>, _>>()?;
        Calls::validate(&captures)?;
        Ok(Self { sites, hit_limit: raw.hit_limit, write_ranges: raw.write_ranges, watch_limit: raw.watch_limit,
            read_ranges: raw.read_ranges, fetch_ranges: raw.fetch_ranges, captures })
    }

    pub fn sites(&self) -> &BTreeMap<u32, String> {
        &self.sites
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttachPhase {
    /// The first-start screen is up.
    FirstStart,
    /// The first-start keys are pressed and live view has settled.
    LiveView,
    /// The USB device is enumerated.
    Attached,
}

#[derive(Serialize)]
struct Region {
    start: u32,
    bytes: String,
}

#[derive(Serialize)]
struct Segment {
    phase: AttachPhase,
    instructions: u64,
    /// The trace hit count when the phase ended.
    hits_end: usize,
    watch: Value,
    regions: Vec<Region>,
}

#[derive(Serialize)]
struct Reset {
    instructions: u64,
    snapshot_sha256: String,
    snapshot_bytes: usize,
}

#[derive(Serialize)]
pub struct Record {
    schema: u32,
    reset: Reset,
    regions_at_reset: Vec<Region>,
    segments: Vec<Segment>,
}

pub struct ResetObserver {
    ranges: Vec<WatchRange>,
    limit: usize,
    maps: Vec<MemoryMap>,
    watch: Option<Box<MemoryWatch>>,
    record: Record,
    region_ranges: Vec<WatchRange>,
}

fn regions(machine: &Machine, ranges: &[WatchRange]) -> Result<Vec<Region>, String> {
    ranges.iter().map(|range| {
        let start = u32::try_from(range.start).map_err(|_| "write range exceeds 32 bits")?;
        let count = usize::try_from(range.end - range.start).map_err(|_| "write range exceeds the host")?;
        Ok(Region { start, bytes: encode_hex(&machine.read(range.start, count)?) })
    }).collect()
}

impl ResetObserver {
    /// Exports and restores the unexecuted session, then arms the spec's sites with its hit bound and
    /// the first watch segment.
    pub fn begin(machine: &mut Machine, spec: &Spec) -> Result<Self, String> {
        if machine.instructions() != 0 {
            return Err("reset observation requires an unexecuted machine".into());
        }
        machine.disarm()?;
        let exported = machine.session.checkpoint(None).and_then(|snapshot| snapshot.export());
        let restored = exported.and_then(|bytes| {
            let snapshot = machine.session.import_snapshot(&bytes)?;
            machine.session.restore(&snapshot)?;
            Ok(bytes)
        });
        machine.rearm()?;
        let bytes = restored?;
        if machine.instructions() != 0 {
            return Err("restored reset executed instructions".into());
        }
        machine.traces.bound_hits(spec.hit_limit);
        machine.traces.arm(&machine.session, spec.sites.clone())?;
        machine.traces.calls.captures = spec.captures.clone();
        let region_ranges: Vec<_> = spec.write_ranges.iter().map(|range| WatchRange {
            start: u64::from(range[0]), end: u64::from(range[1]), kinds: WatchKinds::WRITES,
        }).collect();
        let ranges = region_ranges.iter().cloned()
            .chain(spec.read_ranges.iter().map(|range| WatchRange {
                start: u64::from(range[0]), end: u64::from(range[1]), kinds: WatchKinds::CPU_READ,
            }))
            .chain(spec.fetch_ranges.iter().map(|range| WatchRange {
                start: u64::from(range[0]), end: u64::from(range[1]), kinds: WatchKinds::CPU_FETCH,
            })).collect();
        let mut observer = Self {
            maps: run::memory_maps(&machine.document)?,
            limit: spec.watch_limit,
            watch: None,
            record: Record {
                schema: SCHEMA,
                reset: Reset { instructions: 0, snapshot_sha256: sha256(&bytes), snapshot_bytes: bytes.len() },
                regions_at_reset: regions(machine, &region_ranges)?,
                segments: Vec::new(),
            },
            ranges,
            region_ranges,
        };
        observer.start_watch(machine)?;
        Ok(observer)
    }

    fn start_watch(&mut self, machine: &Machine) -> Result<(), String> {
        self.watch = Some(machine.session.board.memory_watch_begin_classified(
            self.ranges.clone(), self.limit, self.maps.clone(), ViewPolicy::Observe)?);
        Ok(())
    }

    /// Ends the current watch segment at `phase`; every phase before `Attached` starts the next one.
    pub fn boundary(&mut self, machine: &Machine, phase: AttachPhase) -> Result<(), String> {
        let mut watch = self.watch.take().ok_or("reset observation watch is not armed")?;
        let state = watch.records.borrow().state();
        machine.session.board.memory_watch_end(&mut watch)?;
        self.record.segments.push(Segment {
            phase,
            instructions: machine.instructions(),
            hits_end: machine.traces.hits.len(),
            watch: state,
            regions: regions(machine, &self.region_ranges)?,
        });
        if phase != AttachPhase::Attached {
            self.start_watch(machine)?;
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Record, String> {
        if self.watch.is_some() || self.record.segments.last().map(|segment| segment.phase) != Some(AttachPhase::Attached) {
            return Err("reset observation ended before the attached phase".into());
        }
        Ok(self.record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_refuses_unbounded_or_malformed_inputs() {
        let valid = br#"{"schema":1,"sites":{"0x00000004":"loop"},"hit_limit":4,"write_ranges":[[0,16]],"watch_limit":8}"#;
        let spec = Spec::parse(valid).unwrap();
        assert_eq!(spec.sites, BTreeMap::from([(4, "loop".to_owned())]));
        for invalid in [
            &br#"{"schema":2,"sites":{"0x4":"loop"},"hit_limit":4,"write_ranges":[[0,16]],"watch_limit":8}"#[..],
            br#"{"schema":1,"sites":{},"hit_limit":4,"write_ranges":[[0,16]],"watch_limit":8}"#,
            br#"{"schema":1,"sites":{"4":"loop"},"hit_limit":4,"write_ranges":[[0,16]],"watch_limit":8}"#,
            br#"{"schema":1,"sites":{"0x4":"loop"},"hit_limit":0,"write_ranges":[[0,16]],"watch_limit":8}"#,
            br#"{"schema":1,"sites":{"0x4":"loop"},"hit_limit":4,"write_ranges":[[16,16]],"watch_limit":8}"#,
            br#"{"schema":1,"sites":{"0x4":"loop"},"hit_limit":4,"write_ranges":[[0,16]],"watch_limit":0}"#,
            br#"{"schema":1,"sites":{"0x4":"loop"},"hit_limit":4,"write_ranges":[[0,16]],"watch_limit":8,"extra":1}"#,
        ] {
            assert!(Spec::parse(invalid).is_err());
        }
    }
}
