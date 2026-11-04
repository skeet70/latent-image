//! A role's task stacks compared relative to its own allocations, so a different cross-core
//! reservation order compares equal when every allocation, its contents and each core's own order agree.
use crate::composed_catalog::{ALLOCATION_RECORD_BYTES, AllocationPosition, Core, Label, PoolAllocation, Role};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// The physical spelling: `[0x40000000, 0x60000000)` clears `0x40000000`.
fn physical(word: u32) -> u32 {
    if (0x4000_0000..0x6000_0000).contains(&word) { word - 0x4000_0000 } else { word }
}

/// A storage record's 1-based index, the task id the wrapper reserved it for.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Task(pub u32);

/// One reservation: its task, the physical base and the byte size rounded down to a word.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Allocation {
    pub task: Task,
    pub base: u32,
    pub size: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AllocationProblem {
    /// The read is not captured exactly once.
    Capture { label: Label, captures: usize },
    /// The capture is not whole records, words or one cursor word.
    CaptureLength { label: Label, length: usize },
    OutsidePool { allocation: Allocation },
    /// Sorted by base, an allocation does not start where the previous one (or the pool) ends.
    Gap { expected: u32, allocation: Allocation },
    /// The cursor is not the end of the last allocation.
    Cursor { expected: u32, cursor: u32 },
    /// The owner reads attribute the allocation to no core or to both.
    Owner { allocation: Allocation, cores: Vec<Core> },
    /// A core's tasks, ordered by base, differ between the roles.
    CoreOrder { core: Core, retained: Vec<Task>, composed: Vec<Task> },
}

/// A compared word: raw, or a pool address as (alias spelling, task, offset in its allocation).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Normalized {
    Raw(u32),
    Pool { alias: bool, task: Task, offset: u32 },
}

/// One role's allocations, sorted by base.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Allocations(Vec<Allocation>);

/// Read captures by label.
pub trait Captures {
    fn captures(&self, label: &Label) -> Vec<&[u8]>;
}

fn single<'a>(captures: &'a impl Captures, label: &Label) -> Result<&'a [u8], AllocationProblem> {
    match captures.captures(label).as_slice() {
        [bytes] => Ok(bytes),
        other => Err(AllocationProblem::Capture { label: label.clone(), captures: other.len() }),
    }
}

/// The capture, refused unless its length is a whole number of `unit`-byte items.
fn whole<'a>(captures: &'a impl Captures, label: &Label, unit: usize) -> Result<&'a [u8], AllocationProblem> {
    let bytes = single(captures, label)?;
    if unit == 0 || bytes.len() % unit != 0 {
        return Err(AllocationProblem::CaptureLength { label: label.clone(), length: bytes.len() });
    }
    Ok(bytes)
}

fn word_at(bytes: &[u8], offset: usize) -> Option<u32> { bytes.get(offset..offset + 4).map(|word| u32::from_le_bytes(word.try_into().expect("four bytes"))) }

impl Allocations {
    /// Records with a nonzero base are allocations; they must tile the pool from its start to the cursor.
    pub fn of(relation: &PoolAllocation, captures: &impl Captures) -> Result<Self, AllocationProblem> {
        let records = whole(captures, &relation.records, ALLOCATION_RECORD_BYTES as usize)?;
        let cursor = single(captures, &relation.cursor)?;
        let cursor = match cursor {
            [_, _, _, _] => word_at(cursor, 0).expect("one word"),
            _ => return Err(AllocationProblem::CaptureLength { label: relation.cursor.clone(), length: cursor.len() }),
        };
        let mut allocations: Vec<Allocation> = records.chunks_exact(ALLOCATION_RECORD_BYTES as usize).enumerate()
            .filter_map(|(index, record)| {
                let base = word_at(record, 0)?;
                let size = word_at(record, 4)? & !3;
                (base != 0).then_some(Allocation { task: Task(index as u32 + 1), base: physical(base), size })
            }).collect();
        allocations.sort_by_key(|allocation| allocation.base);
        let mut expected = relation.pool.start;
        for allocation in &allocations {
            let end = u64::from(allocation.base) + u64::from(allocation.size);
            if allocation.base < relation.pool.start || end > u64::from(relation.pool.end) {
                return Err(AllocationProblem::OutsidePool { allocation: *allocation });
            }
            if allocation.base != expected {
                return Err(AllocationProblem::Gap { expected, allocation: *allocation });
            }
            expected = allocation.base + allocation.size;
        }
        if physical(cursor) != expected {
            return Err(AllocationProblem::Cursor { expected, cursor });
        }
        Ok(Self(allocations))
    }

    pub fn normalize(&self, word: u32, position: AllocationPosition) -> Normalized {
        let address = physical(word);
        let inside = |allocation: &&Allocation| {
            let end = allocation.base + allocation.size;
            match position {
                AllocationPosition::Base => allocation.base <= address && address < end,
                AllocationPosition::Top => allocation.base < address && address <= end,
            }
        };
        match self.0.iter().find(inside) {
            Some(allocation) => Normalized::Pool { alias: address != word, task: allocation.task, offset: address - allocation.base },
            None => Normalized::Raw(word),
        }
    }

    /// The capture as words, the relation's positions of `label` normalized; `None` unless whole words.
    pub fn words(&self, relation: &PoolAllocation, label: &Label, bytes: &[u8]) -> Option<Vec<Normalized>> {
        if bytes.len() % 4 != 0 {
            return None;
        }
        let positions = relation.words.iter().find(|words| &words.read == label);
        Some(bytes.chunks_exact(4).enumerate().map(|(index, chunk)| {
            let word = u32::from_le_bytes(chunk.try_into().expect("four bytes"));
            let offset = index as u32 * 4;
            match positions {
                Some(words) if offset % words.stride == words.offset => self.normalize(word, words.position),
                _ => Normalized::Raw(word),
            }
        }).collect())
    }

    /// Each core's tasks in base order: every allocation must be named by the owner reads of exactly one core.
    fn core_order(&self, relation: &PoolAllocation, captures: &impl Captures) -> Result<BTreeMap<Core, Vec<Task>>, AllocationProblem> {
        let mut owners: BTreeMap<Task, BTreeSet<Core>> = BTreeMap::new();
        for owner in &relation.owners {
            let bytes = whole(captures, &owner.read, owner.stride as usize)?;
            for record in bytes.chunks_exact(owner.stride as usize) {
                if let Some(word) = word_at(record, owner.offset as usize)
                    && let Normalized::Pool { task, .. } = self.normalize(word, AllocationPosition::Base)
                {
                    owners.entry(task).or_default().insert(owner.core);
                }
            }
        }
        let mut order: BTreeMap<Core, Vec<Task>> = BTreeMap::new();
        for allocation in &self.0 {
            let cores: Vec<Core> = owners.get(&allocation.task).into_iter().flatten().copied().collect();
            match cores.as_slice() {
                [core] => order.entry(*core).or_default().push(allocation.task),
                _ => return Err(AllocationProblem::Owner { allocation: *allocation, cores }),
            }
        }
        Ok(order)
    }
}

/// Both roles' allocations, retained first, refused unless each core reserved its tasks in the same order.
pub fn roles(relation: &PoolAllocation, retained: &impl Captures, owned: &impl Captures) -> Result<[Allocations; 2], (Role, AllocationProblem)> {
    let left = Allocations::of(relation, retained).map_err(|problem| (Role::Retained, problem))?;
    let right = Allocations::of(relation, owned).map_err(|problem| (Role::Composed, problem))?;
    let left_order = left.core_order(relation, retained).map_err(|problem| (Role::Retained, problem))?;
    let right_order = right.core_order(relation, owned).map_err(|problem| (Role::Composed, problem))?;
    for core in Core::ALL {
        let (a, b) = (left_order.get(&core).cloned().unwrap_or_default(), right_order.get(&core).cloned().unwrap_or_default());
        if a != b {
            return Err((Role::Composed, AllocationProblem::CoreOrder { core, retained: a, composed: b }));
        }
    }
    Ok([left, right])
}
