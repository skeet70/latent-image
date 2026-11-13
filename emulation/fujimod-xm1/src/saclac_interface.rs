use super::{Address, Machine, Stop, parse_hex, read_u32, require};
use fujimod_emulator::board::observation::{WatchKinds, WatchRange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const EVENT_LIMIT: usize = 100_000;
const PAYLOAD_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind { Callback, ClientReply }

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetRegister { R3, R4, R5, R6, R12 }
impl TargetRegister {
    fn name(self) -> &'static str {
        match self { Self::R3 => "r3", Self::R4 => "r4", Self::R5 => "r5", Self::R6 => "r6", Self::R12 => "r12" }
    }
}

#[derive(Debug, Deserialize)]
pub struct Entry {
    address: String,
    kind: EntryKind,
    target: Option<TargetRegister>,
}

#[derive(Debug, Deserialize)]
pub struct Shared {
    start: String,
    end: String,
}

#[derive(Debug, Deserialize)]
pub struct Declaration {
    post: String,
    entries: Vec<Entry>,
    shared: Vec<Shared>,
}

pub struct Plan {
    post: u32,
    entries: Vec<(u32, EntryKind, Option<TargetRegister>)>,
    shared: Vec<(u32, u32)>,
}

impl Declaration {
    pub fn validate(self) -> Result<Plan, String> {
        require(self.entries.len() <= 512 && self.shared.len() <= 64, "interface declaration exceeds bounds")?;
        let post = parse_hex(&self.post)?;
        let mut addresses = BTreeSet::from([post]);
        let mut entries = Vec::new();
        for entry in self.entries {
            let address = parse_hex(&entry.address)?;
            require(address % 4 == 0 && addresses.insert(address), "interface entry is unaligned or repeated")?;
            require(matches!((entry.kind, entry.target), (EntryKind::Callback, None) | (EntryKind::ClientReply, Some(_))), "interface reply target register missing or unexpected")?;
            entries.push((address, entry.kind, entry.target));
        }
        let mut shared = Vec::new();
        for span in self.shared {
            let (start, end) = (parse_hex(&span.start)?, parse_hex(&span.end)?);
            require(start < end && end - start <= 65536, "interface shared span exceeds bounds")?;
            shared.push((start, end));
        }
        shared.sort_unstable();
        require(shared.windows(2).all(|pair| pair[0].1 <= pair[1].0), "interface shared spans overlap")?;
        Ok(Plan { post, entries, shared })
    }
}

impl Plan {
    pub fn sites(&self) -> impl Iterator<Item = u32> + '_ {
        std::iter::once(self.post).chain(self.entries.iter().map(|entry| entry.0))
    }

    pub fn classes(&self) -> impl Iterator<Item = WatchRange> + '_ {
        self.shared.iter().map(|&(start, end)| WatchRange {
            start: u64::from(start), end: u64::from(end), kinds: WatchKinds::CPU_WRITE,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Event {
    QueueIn { operation: u16, sender: u8, payload: Vec<u8> },
    PeerRequest { queue: u8, operation: u16, payload: Vec<u8> },
    Callback { entry: Address },
    ClientReply { entry: Address, target: Address, status: u32 },
    Store { address: Address, bytes: Vec<u8> },
}

#[derive(Serialize)]
pub struct Observed {
    pub instructions: u64,
    pub core: usize,
    pub event: Event,
}

#[derive(Default, Serialize)]
pub struct Stream {
    pub events: Vec<Observed>,
    #[serde(skip)]
    cursor: usize,
}

fn bytes(machine: &Machine, core: usize, address: u32, length: usize) -> Result<Vec<u8>, String> {
    require(length <= PAYLOAD_LIMIT && address.checked_add(length as u32).is_some(), "interface payload exceeds bounds")?;
    let mut bytes = vec![0; length];
    if length != 0 { machine.session.board.read_memory(core, u64::from(address), &mut bytes)?; }
    Ok(bytes)
}

impl Stream {
    fn push(&mut self, observed: Observed) -> Result<(), String> {
        require(self.events.len() < EVENT_LIMIT, "interface event limit exceeded")?;
        self.events.push(observed);
        Ok(())
    }

    pub fn entry(&mut self, machine: &Machine, plan: &Plan, stop: &Stop) -> Result<(), String> {
        let register = |name: &str| stop.registers.get(name).ok_or_else(|| "interface register missing".to_owned()).and_then(|text| parse_hex(text));
        let event = if stop.address == plan.post {
            let descriptor = bytes(machine, stop.core, register("r1")?, 12)?;
            let operation = u16::from_le_bytes([descriptor[0], descriptor[1]]);
            let sender = descriptor[2];
            let queue = u8::try_from(register("r0")?).map_err(|_| "interface queue exceeds byte")?;
            if queue != 43 && sender != 43 { return Ok(()); }
            let payload = bytes(machine, stop.core, read_u32(&descriptor, 8), read_u32(&descriptor, 4) as usize)?;
            if queue == 43 { Event::QueueIn { operation, sender, payload } }
            else { Event::PeerRequest { queue, operation, payload } }
        } else {
            match plan.entries.iter().find(|entry| entry.0 == stop.address) {
                Some((_, EntryKind::Callback, _)) => Event::Callback { entry: Address(stop.address) },
                Some((_, EntryKind::ClientReply, Some(target))) => Event::ClientReply {
                    entry: Address(stop.address), target: Address(register(target.name())?), status: register("r0")?,
                },
                Some(_) => return Err("interface reply target register missing".into()),
                None => return Ok(()),
            }
        };
        self.push(Observed { instructions: stop.instructions, core: stop.core, event })
    }

    pub fn stores(&mut self, plan: &Plan, state: &serde_json::Value) -> Result<(), String> {
        require(state.get("violation").is_none_or(serde_json::Value::is_null), "interface watch overflowed")?;
        let records = state.get("records").and_then(serde_json::Value::as_array).ok_or("interface watch records missing")?;
        require(self.cursor <= records.len(), "interface watch cursor invalid")?;
        #[derive(Deserialize)]
        struct Write { source: String, address: u64, size: usize, value: Option<u64>, instructions: Option<u64>, core: Option<usize> }
        for record in &records[self.cursor..] {
            let row: Write = serde_json::from_value(record.clone()).map_err(|error| error.to_string())?;
            if row.source != "cpu-write" || !plan.shared.iter().any(|&(start, end)| u64::from(start) <= row.address && row.address < u64::from(end)) { continue; }
            require(row.size <= 8, "interface write is wider than its recorded value")?;
            let value = row.value.ok_or("interface write value missing")?;
            let address = u32::try_from(row.address).map_err(|_| "interface write exceeds guest address")?;
            require(plan.shared.iter().any(|&(start, end)| start <= address && u64::from(address) + row.size as u64 <= u64::from(end)), "interface write crosses shared span")?;
            self.push(Observed {
                instructions: row.instructions.ok_or("interface write clock missing")?,
                core: row.core.ok_or("interface write core missing")?,
                event: Event::Store { address: Address(address), bytes: value.to_le_bytes()[..row.size].to_vec() },
            })?;
        }
        self.cursor = records.len();
        Ok(())
    }

    pub fn restart(&mut self) { self.cursor = 0; }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Declaration { post: "0x1000".into(), entries: Vec::new(), shared: vec![Shared { start: "0x2000".into(), end: "0x2010".into() }] }.validate().unwrap()
    }

    #[test]
    fn writes_remain_ordered_across_entries_and_watch_epochs() {
        let mut stream = Stream::default();
        let first = serde_json::json!({"records":[
            {"source":"cpu-write","address":8192,"size":2,"value":4660,"instructions":10,"core":0}
        ],"violation":null});
        stream.stores(&plan(), &first).unwrap();
        stream.push(Observed { instructions: 11, core: 1, event: Event::Callback { entry: Address(0x1004) } }).unwrap();
        stream.stores(&plan(), &first).unwrap();
        stream.restart();
        let second = serde_json::json!({"records":[
            {"source":"cpu-write","address":8194,"size":1,"value":86,"instructions":12,"core":1}
        ],"violation":null});
        stream.stores(&plan(), &second).unwrap();
        assert_eq!(stream.events.iter().map(|row| &row.event).collect::<Vec<_>>(), vec![
            &Event::Store { address: Address(0x2000), bytes: vec![0x34, 0x12] },
            &Event::Callback { entry: Address(0x1004) },
            &Event::Store { address: Address(0x2002), bytes: vec![0x56] },
        ]);
    }

    #[test]
    fn incomplete_write_evidence_is_refused() {
        for record in [
            serde_json::json!({"source":"cpu-write","address":8192,"size":4,"instructions":10,"core":0}),
            serde_json::json!({"source":"cpu-write","address":8207,"size":4,"value":0,"instructions":10,"core":0}),
        ] {
            assert!(Stream::default().stores(&plan(), &serde_json::json!({"records":[record],"violation":null})).is_err());
        }
        assert!(Stream::default().stores(&plan(), &serde_json::json!({"records":[],"violation":"overflow"})).is_err());
    }
}
