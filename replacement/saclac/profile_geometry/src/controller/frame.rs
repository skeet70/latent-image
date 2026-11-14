#![forbid(unsafe_code)]

//! The frame handler the controller attaches to its first handler unit: it records the index
//! the frame began with, moves the channel and indexed operations' statistics stages to their
//! other buffer while keeping the results of the frame just ended, and clears the stages the
//! frame applied.

use super::{Operation, ProfileGeometry, Tables};
use crate::memory::{Location, Memory};
use crate::ports::{Ports, Stages, StatisticsDestinations, StatisticsRegion, StockData};
use crate::stock::layout::{controller as field, counters, work, STATISTICS_BANKS};

const CHANNEL_STAGES: Stages = Stages(0x3f);
const INDEXED_STAGES: Stages = Stages(0x40);

/// The buffer regions each statistics stage writes, by the stage's bit, in the order the stage
/// takes them.
const STAGE_REGIONS: [&[StatisticsRegion]; 7] = {
    const fn r(region: u32) -> StatisticsRegion { StatisticsRegion(region) }
    [&[r(3)], &[r(7)], &[r(0)], &[r(2)], &[r(11), r(12)], &[r(13), r(21), r(29)], &[r(37)]]
};

/// One of the two statistics buffers an operation's stages write in turn, and the slot that
/// keeps the results of the same number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bank {
    First,
    Second,
}

impl Bank {
    /// `None` for a cleared word.
    pub const fn from_word(word: u32) -> Option<Self> {
        match word {
            1 => Some(Bank::First),
            2 => Some(Bank::Second),
            _ => None,
        }
    }

    pub const fn word(self) -> u32 {
        match self {
            Bank::First => 1,
            Bank::Second => 2,
        }
    }

    const fn index(self) -> u16 {
        match self {
            Bank::First => 0,
            Bank::Second => 1,
        }
    }

    const fn after(current: Option<Self>) -> Self {
        match current {
            Some(Bank::First) => Bank::Second,
            _ => Bank::First,
        }
    }
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xed8634.
    pub fn handle_frame(&mut self) {
        self.memory.write_u32(counters::FRAME_INDEX, self.memory.read_u32(counters::INDEX));
        if !self.operation(Operation::Channel).idle() {
            let target = self.advance(field::CHANNEL_TARGET);
            self.point(CHANNEL_STAGES, target);
            let kept = self.bank(field::CHANNEL_KEPT);
            self.keep_channel_results(kept);
            self.memory.write_u32(field::CHANNEL_KEPT, Bank::after(kept).word());
        }
        if self.operation(Operation::Indexed).running() {
            self.memory.write_u32(field::INDEXED_POINTED, 1);
            let target = self.advance(field::INDEXED_TARGET);
            self.point(INDEXED_STAGES, target);
            if !self.operation(Operation::Indexed).idle() {
                let kept = self.bank(field::INDEXED_KEPT);
                self.keep_indexed_results(kept);
                self.memory.write_u32(field::INDEXED_KEPT, Bank::after(kept).word());
            }
        }
        for (_, applied) in work::APPLIED {
            self.memory.write_u32(applied, 0);
        }
    }

    fn bank(&self, at: Location) -> Option<Bank> { Bank::from_word(self.memory.read_u32(at)) }

    fn advance(&mut self, at: Location) -> Bank {
        let next = Bank::after(self.bank(at));
        self.memory.write_u32(at, next.word());
        next
    }

    fn point(&mut self, stages: Stages, bank: Bank) {
        let buffer = STATISTICS_BANKS[bank.index() as usize];
        let mut destinations = StatisticsDestinations::default();
        for (bit, regions) in STAGE_REGIONS.iter().enumerate() {
            if stages.0 & 1 << bit == 0 {
                continue;
            }
            for (slot, &region) in destinations.0[bit].iter_mut().zip(regions.iter()) {
                *slot = StockData(buffer.wrapping_add(self.ports.statistics_offset(region)));
            }
        }
        self.ports.point_statistics(self.pipeline_channel(), stages, &destinations);
    }

    fn keep_channel_results(&mut self, bank: Option<Bank>) {
        self.ports.collect_statistics(self.pipeline_channel(), CHANNEL_STAGES, work::results::RECORD);
        let Some(bank) = bank else {
            return;
        };
        let kept = field::CHANNEL_RESULTS.plus(field::CHANNEL_RESULTS_STRIDE * bank.index());
        for (result, offset) in work::results::CHANNEL_WORDS {
            self.memory.write_u32(kept.plus(offset), self.memory.read_u16(result) as u32);
        }
        for (table, offset) in work::results::CHANNEL_TABLES {
            for pair in 0..work::results::TABLE_PAIRS {
                let second_half = |entry: u16| table.plus(4 * entry + 2);
                let at = kept.plus(offset + 8 * pair);
                self.memory.write_u32(at.plus(4), self.memory.read_u16(second_half(pair)) as u32);
                let later = second_half(pair + work::results::TABLE_PAIRS);
                self.memory.write_u32(at, self.memory.read_u16(later) as u32);
            }
        }
    }

    fn keep_indexed_results(&mut self, bank: Option<Bank>) {
        self.ports.collect_statistics(self.pipeline_channel(), INDEXED_STAGES, work::results::RECORD);
        let Some(bank) = bank else {
            return;
        };
        let kept = field::INDEXED_RESULTS.plus(field::INDEXED_RESULTS_STRIDE * bank.index());
        for (result, offset) in work::results::INDEXED_WORDS {
            self.memory.write_u32(kept.plus(offset), self.memory.read_u16(result) as u32);
        }
    }
}
