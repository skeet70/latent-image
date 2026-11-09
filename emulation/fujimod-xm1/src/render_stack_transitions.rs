use super::{Transition, RECORD_LIMIT, TRANSITION_LIMIT};
use serde::{Serialize, Serializer, ser::SerializeSeq};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Index(usize);
#[derive(Clone, Copy, Serialize)]
struct Sample { ordinal: u64, context: Transition }
#[derive(Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Entry {
    Sample { ordinal: u64, context: Transition },
    Pair { first: Sample, second: Sample, stride: u64, pairs: u64, trailing_first: bool },
}
#[derive(Clone, Copy)]
enum Slot { Occupied { entry: Entry, previous: Option<Index>, next: Option<Index> }, Free(Option<Index>) }
pub(super) struct Arena { slots: Vec<Slot>, free: Option<Index>, pub(super) retained: usize, pub(super) logical: u64 }
#[derive(Default)]
pub(super) struct Trace { first: Option<Index>, last: Option<Index>, retained: usize, pub(super) count: u64, last_ordinal: Option<u64> }
impl Arena {
    pub(super) fn new() -> Self { Self { slots: Vec::with_capacity(TRANSITION_LIMIT), free: None, retained: 0, logical: 0 } }
    pub(super) fn allocated_entries(&self) -> usize { self.slots.capacity() }
    fn entry(&self, index: Index) -> (Entry, Option<Index>, Option<Index>) {
        match self.slots[index.0] { Slot::Occupied { entry, previous, next } => (entry, previous, next), Slot::Free(_) => unreachable!() }
    }
    pub(super) fn push(&mut self, trace: &mut Trace, ordinal: u64, context: Transition) -> Result<(), String> {
        if ordinal == 0 || trace.last_ordinal.is_some_and(|last| ordinal <= last) {
            return Err("stack diagnostic transition ordinals are not strictly ordered".into());
        }
        let count = trace.count.checked_add(1).ok_or("stack diagnostic logical transition overflow")?;
        let logical = self.logical.checked_add(1).ok_or("stack diagnostic logical transition overflow")?;
        let mut replacement = None;
        if let Some(last) = trace.last {
            let (entry, previous, _) = self.entry(last);
            match entry {
                Entry::Pair { first, second, stride, pairs, trailing_first } => {
                    let base = if trailing_first { second } else { first };
                    let expected = stride.checked_mul(pairs).and_then(|delta| base.ordinal.checked_add(delta))
                        .ok_or("stack diagnostic pair ordinal overflow")?;
                    if ordinal == expected && context == base.context {
                        replacement = Some((last, Entry::Pair { first, second, stride,
                            pairs: if trailing_first { pairs.checked_add(1).ok_or("stack diagnostic pair count overflow")? } else { pairs },
                            trailing_first: !trailing_first }, None));
                    }
                }
                Entry::Sample { ordinal: second_ordinal, context: second_context } => {
                    if let Some(previous) = previous {
                        if let (Entry::Sample { ordinal: first_ordinal, context: first_context }, _, _) = self.entry(previous) {
                            if context == first_context && ordinal > second_ordinal {
                                let stride = ordinal.checked_sub(first_ordinal).ok_or("stack diagnostic pair stride underflow")?;
                                if first_ordinal < second_ordinal && second_ordinal < ordinal {
                                    replacement = Some((previous, Entry::Pair { first: Sample { ordinal: first_ordinal, context: first_context },
                                        second: Sample { ordinal: second_ordinal, context: second_context }, stride, pairs: 1, trailing_first: true }, Some(last)));
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some((index, entry, release)) = replacement {
            let (_, previous, _) = self.entry(index);
            self.slots[index.0] = Slot::Occupied { entry, previous, next: None };
            if let Some(release) = release {
                self.slots[release.0] = Slot::Free(self.free);
                self.free = Some(release);
                self.retained -= 1;
                trace.retained -= 1;
            }
            trace.last = Some(index);
        } else {
            if trace.retained == RECORD_LIMIT || self.retained == TRANSITION_LIMIT {
                return Err("stack diagnostic transition storage bound exceeded".into());
            }
            let slot = Slot::Occupied { entry: Entry::Sample { ordinal, context }, previous: trace.last, next: None };
            let index = if let Some(index) = self.free {
                let Slot::Free(next) = self.slots[index.0] else { unreachable!() };
                self.free = next;
                self.slots[index.0] = slot;
                index
            } else {
                let index = Index(self.slots.len());
                self.slots.push(slot);
                index
            };
            if let Some(last) = trace.last {
                let (entry, previous, _) = self.entry(last);
                self.slots[last.0] = Slot::Occupied { entry, previous, next: Some(index) };
            } else { trace.first = Some(index); }
            trace.last = Some(index);
            trace.retained += 1;
            self.retained += 1;
        }
        trace.count = count;
        trace.last_ordinal = Some(ordinal);
        self.logical = logical;
        Ok(())
    }
    pub(super) fn rows<'a>(&'a self, trace: &'a Trace) -> Rows<'a> { Rows { arena: self, trace } }
}
pub(super) struct Rows<'a> { arena: &'a Arena, trace: &'a Trace }
impl Serialize for Rows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut rows = serializer.serialize_seq(Some(self.trace.retained))?;
        let mut index = self.trace.first;
        while let Some(current) = index {
            let (entry, _, next) = self.arena.entry(current);
            rows.serialize_element(&entry)?;
            index = next;
        }
        rows.end()
    }
}

#[cfg(test)]
mod tests {
    use super::{Arena, Trace};
    use crate::render_stack::{ModeSource, Transition};
    use serde_json::{Value, json};

    fn foreign(pc: u32, cpsr: u32) -> Transition {
        Transition { pc, cpsr, context_depth: 0, current_node: 0x40060614, owner_node: Some(0x40060614),
            scheduler: false, mode_source: Some(ModeSource { pc: pc - 4, word: 0xe121f000 }) }
    }
    fn expand(rows: &Value) -> Vec<Value> {
        let mut decoded = Vec::new();
        for row in rows.as_array().unwrap() {
            if row["kind"] == "sample" {
                decoded.push(json!({"ordinal":row["ordinal"],"context":row["context"]}));
            } else {
                let stride = row["stride"].as_u64().unwrap();
                let pairs = row["pairs"].as_u64().unwrap();
                for pair in 0..pairs {
                    for side in ["first", "second"] {
                        decoded.push(json!({"ordinal":row[side]["ordinal"].as_u64().unwrap() + pair * stride,
                            "context":row[side]["context"]}));
                    }
                }
                if row["trailing_first"] == true {
                    decoded.push(json!({"ordinal":row["first"]["ordinal"].as_u64().unwrap() + pairs * stride,
                        "context":row["first"]["context"]}));
                }
            }
        }
        decoded
    }
    #[test]
    fn foreign_task_cycles_reconstruct_every_ordinal_and_late_changed_context() {
        let mut arena = Arena::new();
        let mut trace = Trace::default();
        let mut expected = Vec::new();
        for cycle in 0..100_000u64 {
            for (ordinal, context) in [(10 + cycle * 7, foreign(0x425a8, 0x13)), (13 + cycle * 7, foreign(0x425c4, 0x93))] {
                arena.push(&mut trace, ordinal, context).unwrap();
                expected.push(json!({"ordinal":ordinal,"context":context}));
            }
        }
        let changed = foreign(0x425c4, 0xd3);
        arena.push(&mut trace, 700_010, changed).unwrap();
        expected.push(json!({"ordinal":700_010,"context":changed}));
        let rows = serde_json::to_value(arena.rows(&trace)).unwrap();
        assert!(rows.as_array().unwrap().len() < 4);
        assert_eq!(expand(&rows), expected);
    }
    #[test]
    fn odd_cycle_and_broken_stride_preserve_the_complete_sequence() {
        let mut arena = Arena::new();
        let mut trace = Trace::default();
        let input = [(2, foreign(8, 0x13)), (5, foreign(12, 0x93)), (9, foreign(8, 0x13)),
            (12, foreign(12, 0x93)), (16, foreign(8, 0x13)), (21, foreign(12, 0x93))];
        for (ordinal, context) in input { arena.push(&mut trace, ordinal, context).unwrap(); }
        let expected: Vec<_> = input.iter().map(|(ordinal, context)| json!({"ordinal":ordinal,"context":context})).collect();
        assert_eq!(expand(&serde_json::to_value(arena.rows(&trace)).unwrap()), expected);
    }
    #[test]
    fn pair_ordinal_overflow_refuses_without_changing_the_retained_prefix() {
        let mut arena = Arena::new();
        let mut trace = Trace::default();
        for (ordinal, context) in [(1, foreign(8, 0x13)), (u64::MAX - 2, foreign(12, 0x93)),
            (u64::MAX - 1, foreign(8, 0x13))] {
            arena.push(&mut trace, ordinal, context).unwrap();
        }
        let before = serde_json::to_value(arena.rows(&trace)).unwrap();
        assert!(arena.push(&mut trace, u64::MAX, foreign(12, 0x93)).is_err());
        assert_eq!(serde_json::to_value(arena.rows(&trace)).unwrap(), before);
    }
}
