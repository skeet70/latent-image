//! A role's task stacks compared relative to its own allocations, so a different cross-core
//! reservation order compares equal when every allocation and its contents agree.
use crate::composed_catalog::{ALLOCATION_RECORD_BYTES, AllocationPosition, Label, PoolAllocation, Role};
use serde::Serialize;

/// The physical spelling: `[0x40000000, 0x60000000)` clears `0x40000000`.
fn physical(word: u32) -> u32 {
    if (0x4000_0000..0x6000_0000).contains(&word) { word - 0x4000_0000 } else { word }
}

/// One reservation: the 1-based record index, the physical base and the byte size rounded down to a word.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Allocation {
    pub task: u32,
    pub base: u32,
    pub size: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AllocationProblem {
    /// The read is not captured exactly once.
    Capture { label: Label, captures: usize },
    OutsidePool { allocation: Allocation },
    /// Sorted by base, an allocation does not start where the previous one (or the pool) ends.
    Gap { expected: u32, allocation: Allocation },
    /// The cursor is not the end of the last allocation.
    Cursor { expected: u32, cursor: u32 },
}

/// A compared word: raw, or a pool address as (alias spelling, task, offset in its allocation).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Normalized {
    Raw(u32),
    Pool { alias: bool, task: u32, offset: u32 },
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

fn word_at(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes")) }

impl Allocations {
    /// Records with a nonzero base are allocations; they must tile the pool from its start to the cursor.
    pub fn of(relation: &PoolAllocation, captures: &impl Captures) -> Result<Self, AllocationProblem> {
        let records = single(captures, &relation.records)?;
        let cursor = single(captures, &relation.cursor)?;
        let stride = ALLOCATION_RECORD_BYTES as usize;
        let mut allocations: Vec<Allocation> = records.chunks_exact(stride).enumerate()
            .filter(|(_, record)| word_at(record, 0) != 0)
            .map(|(index, record)| Allocation {
                task: index as u32 + 1, base: physical(word_at(record, 0)), size: word_at(record, 4) & !3,
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
        let cursor = word_at(cursor, 0);
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

    /// The capture as words, the relation's positions of `label` normalized.
    pub fn words(&self, relation: &PoolAllocation, label: &Label, bytes: &[u8]) -> Vec<Normalized> {
        let positions: Vec<_> = relation.words.iter().filter(|words| &words.read == label).collect();
        bytes.chunks(4).enumerate().map(|(index, chunk)| {
            let mut padded = [0u8; 4];
            padded[..chunk.len()].copy_from_slice(chunk);
            let word = u32::from_le_bytes(padded);
            let offset = index as u32 * 4;
            match positions.iter().find(|words| offset % words.stride == words.offset) {
                Some(words) => self.normalize(word, words.position),
                None => Normalized::Raw(word),
            }
        }).collect()
    }
}

/// Both roles' allocations, retained first.
pub fn roles(relation: &PoolAllocation, retained: &impl Captures, owned: &impl Captures) -> Result<[Allocations; 2], (Role, AllocationProblem)> {
    Ok([
        Allocations::of(relation, retained).map_err(|problem| (Role::Retained, problem))?,
        Allocations::of(relation, owned).map_err(|problem| (Role::Composed, problem))?,
    ])
}
