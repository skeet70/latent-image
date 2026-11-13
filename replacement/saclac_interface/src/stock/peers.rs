//! X-M1 1.50: where saclac's peer requests go. Every stock sender posts through
//! 0x72503c(queue, descriptor) and writes the same queue number as the descriptor's destination
//! byte. 0x72503c polls for a block (-50 from slot +0x170 becomes -62), sends without waiting
//! (+0xac in task context, +0xb0 in handler context), and on any failure logs code 28, stores
//! the status and enters the error callback before returning it.

use core::num::NonZeroU8;

use super::shared::{image_processor_operation, lens_com_operation};
use super::{calc1_operation, calc_operation, flash_operation, img_dc_operation, lens_dc_operation, statistics_operation};
use crate::PeerOperation;

/// A peer task's message queue number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerQueue(NonZeroU8);

impl PeerQueue {
    const fn new(number: u8) -> Self {
        match NonZeroU8::new(number) {
            Some(number) => Self(number),
            None => panic!("queue 0 does not exist"),
        }
    }

    pub const fn number(self) -> u8 { self.0.get() }
}

/// How a sender fills the descriptor's sender byte from 0x723cb8, which returns the running
/// task's number, or 0 outside task context (slot +0x16c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SenderByte {
    RunningTask,
    /// 0 is replaced by 0xff.
    RunningTaskOrFf,
}

impl SenderByte {
    pub const fn byte(self, running_task: Option<NonZeroU8>) -> u8 {
        match (running_task, self) {
            (Some(task), _) => task.get(),
            (None, Self::RunningTask) => 0,
            (None, Self::RunningTaskOrFf) => 0xff,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerRoute {
    pub queue: PeerQueue,
    pub sender: SenderByte,
    /// The second word of the queue's creation descriptor (0x724cdc).
    pub block_bytes: u16,
}

impl PeerRoute {
    /// The longest payload 0x72503c posts to this queue; a longer one is refused with -60.
    pub const fn payload_capacity(self) -> usize { self.block_bytes as usize - 8 }
}

/// The lensdc senders 0x76d8f8..0x7700c8 pass 50 (e.g. 0x76dc70 in 0x76dbe4) and store 0xff when
/// 0x723cb8 returns 0 (0x76dc64..0x76dc6c); lensdc creates queue 50 with block 44 at 0x76fdb8.
pub const LENS_DC: PeerRoute = PeerRoute { queue: PeerQueue::new(50), sender: SenderByte::RunningTaskOrFf, block_bytes: 44 };

/// 0xfe9580 posts to the word at 0x16f0aa4 with 0x723cb8's byte (0xfe9598..0xfe959c); imgdc's
/// setup stores 47 there (0xfe9038..0xfe903c) and creates queue 47 with block 240 (0xfe9058).
pub const IMG_DC: PeerRoute = PeerRoute { queue: PeerQueue::new(47), sender: SenderByte::RunningTask, block_bytes: 240 };

/// 0xf49598 posts to the word at 0x16c236c with 0x723cb8's byte (0xf495b0..0xf495b4); calc's
/// setup stores 44 there (0xf49478..0xf4947c) and creates queue 44 with block 240 (0xf49498).
pub const CALC: PeerRoute = PeerRoute { queue: PeerQueue::new(44), sender: SenderByte::RunningTask, block_bytes: 240 };

/// 0xf49c18 passes 45 (0xf49c38) with 0x723cb8's byte (0xf49c30..0xf49c34); queue 45 is created
/// with block 240 at 0xf49ba4.
pub const CALC1: PeerRoute = PeerRoute { queue: PeerQueue::new(45), sender: SenderByte::RunningTask, block_bytes: 240 };

/// The statistics senders 0x1039638..0x103b0b4 pass 48 with 0x723cb8's byte (e.g.
/// 0x103975c..0x1039764 in 0x1039638); queue 48 is created with block 256 at 0x1039524.
pub const STATISTICS: PeerRoute = PeerRoute { queue: PeerQueue::new(48), sender: SenderByte::RunningTask, block_bytes: 256 };

/// 0x6b6d80 (0x1a0d) and 0x6b6dd4 (0x1a0e) pass 10 (0x6b6da0, 0x6b6df0) and store their first
/// argument as the sender byte, which 0x69db64 and 0x69db2c take from 0x723cb8 (0x69db88,
/// 0x69db50); queue 10 is created with block 20 at 0x6b68f4.
pub const FLASH: PeerRoute = PeerRoute { queue: PeerQueue::new(10), sender: SenderByte::RunningTask, block_bytes: 20 };

/// 0x6bbe60 passes 98 (0x6bbe84, 0x6bbed4) with 0x723cb8's byte (0x6bbe7c..0x6bbe80); queue 98
/// is created with block 56 at 0x6bd9e8.
pub const LENS_COM: PeerRoute = PeerRoute { queue: PeerQueue::new(98), sender: SenderByte::RunningTask, block_bytes: 56 };

/// 0x1066230 passes 55 (0x1066260) and stores 0xff when 0x723cb8 returns 0
/// (0x1066248..0x106625c); queue 55 is created with block 24 at 0x10662a4.
pub const IMAGE_PROCESSOR: PeerRoute = PeerRoute { queue: PeerQueue::new(55), sender: SenderByte::RunningTaskOrFf, block_bytes: 24 };

pub const fn route(operation: PeerOperation) -> PeerRoute {
    match operation {
        PeerOperation::LensDc(_) => LENS_DC,
        PeerOperation::ImgDc(_) => IMG_DC,
        PeerOperation::Calc(_) => CALC,
        PeerOperation::Calc1(_) => CALC1,
        PeerOperation::Statistics(_) => STATISTICS,
        PeerOperation::Flash(_) => FLASH,
        PeerOperation::LensCom(_) => LENS_COM,
        PeerOperation::ImageProcessor(_) => IMAGE_PROCESSOR,
    }
}

/// The descriptor's operation halfword.
pub fn operation_code(operation: PeerOperation) -> u16 {
    match operation {
        PeerOperation::LensDc(operation) => lens_dc_operation(operation),
        PeerOperation::ImgDc(operation) => img_dc_operation(operation),
        PeerOperation::Calc(operation) => calc_operation(operation),
        PeerOperation::Calc1(operation) => calc1_operation(operation),
        PeerOperation::Statistics(operation) => statistics_operation(operation),
        PeerOperation::Flash(operation) => flash_operation(operation),
        PeerOperation::LensCom(operation) => lens_com_operation(operation),
        PeerOperation::ImageProcessor(operation) => image_processor_operation(operation),
    }
}
