#![forbid(unsafe_code)]

//! Image-pipeline stage control over the image-4 register layer (image 5, 0xd9e964..0xda027c):
//! enabling and disabling stages, resetting a transform channel and writing a region.

/// The pipeline units a stage operation holds while it programs them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    /// 0x65df68 and 0x65e0c8, which stay stock with the input unit.
    Input,
    /// 0x671aa8 and 0x671bf8.
    Transform,
    /// 0x684eb4 and 0x685004.
    Processing,
    /// 0x676104 and 0x676254.
    Filter,
}

/// Stages whose mode is set from the parameter block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    /// 0x685208, mode from block word 0x14.
    Stage400,
    /// 0x6852ec, block word 0x30.
    Stage4,
    /// 0x6853d0, block word 0x40.
    Stage8,
    /// 0x6854b4, block word 0x88.
    Stage10,
}

/// The parts of a region block `write_region` programs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionPart {
    /// 0x685b94.
    ProcessingFirst,
    /// 0x685dd0.
    ProcessingSecond,
    /// 0x65f3d8, which stays stock with the input unit; block word 0x30.
    InputMode,
    /// 0x6768f8.
    FilterRegion,
}

/// The register layer's operations the stage control uses; each returns the stock status.
pub trait ImageStages {
    type Session;
    /// The caller's parameter or region block, in the layout the register layer reads.
    type Block;

    fn acquire(&mut self, unit: Unit, handle: u32) -> (i32, Self::Session);
    fn release(&mut self, session: Self::Session) -> i32;
    /// 0x65fcb4.
    fn enable_input(&mut self, session: &Self::Session, enabled: bool) -> i32;
    fn stage_mode(&self, block: &Self::Block, stage: Stage) -> u32;
    fn set_stage(&mut self, session: &Self::Session, stage: Stage, mode: u32) -> i32;
    /// 0x6856cc then 0x685598.
    fn stage_20(&mut self, session: &Self::Session) -> (i32, [u32; 2]);
    fn set_stage_20(&mut self, session: &Self::Session, words: [u32; 2]) -> i32;
    /// 0x6858e0 then 0x68577c.
    fn stage_flags(&mut self, session: &Self::Session) -> (i32, [u32; 2]);
    fn set_stage_flags(&mut self, session: &Self::Session, words: [u32; 2]) -> i32;
    /// 0x6722ec.
    fn set_transform_scale(&mut self, session: &Self::Session, scale: [u16; 2], channel: u32) -> i32;
    /// 0x672a50.
    fn transform_strides(&mut self, session: &Self::Session, channel: u32) -> (i32, [u16; 4]);
    /// 0x672830.
    fn set_transform_strides(&mut self, session: &Self::Session, strides: [u16; 4], channel: u32) -> i32;
    /// 0x672b2c.
    fn set_transform_payload(&mut self, session: &Self::Session, payload: &[u32; 31], channel: u32) -> i32;
    fn write_region(&mut self, session: &Self::Session, block: &Self::Block, part: RegionPart) -> i32;
    /// 0x66049c.
    fn reset_input_unit(&mut self) -> i32;
}

const INPUT: u32 = 0x3;
const TRANSFORM: u32 = 0x300;
const PROCESSING: u32 = 0xfc;

/// Enables or disables the stages `flags` selects: input enable (bit 0), stage modes (0x400, 4,
/// 8, 0x10), stage 20 (0x20) and the two stage flags (0x40, 0x80). Enabling sets each stage's
/// mode to 2 where the block asks for 2, else 1, and sets the flag bit; disabling sets mode 0 and
/// toggles the flag bit as stock does (0xd9ea08). Returns the last status. Stage 400 is
/// programmed only while the processing unit is held; stock programs an unheld context when
/// `flags` selects 0x400 without any of 0xfc.
pub fn enable_stages<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, enabled: bool, block: Option<&S::Block>) -> i32 {
    let mut result = 0;
    let mut hold = |unit: Unit, mask: u32, stages: &mut S| {
        if flags & mask != 0 {
            let (status, session) = stages.acquire(unit, handle);
            result = status;
            Some(session)
        } else {
            None
        }
    };
    let input = hold(Unit::Input, INPUT, stages);
    let transform = hold(Unit::Transform, TRANSFORM, stages);
    let processing = hold(Unit::Processing, PROCESSING, stages);
    if flags & 1 != 0 {
        if let Some(input) = &input {
            result = stages.enable_input(input, enabled);
        }
    }
    if let Some(processing) = &processing {
        for (mask, stage) in [(0x400, Stage::Stage400), (0x4, Stage::Stage4), (0x8, Stage::Stage8), (0x10, Stage::Stage10)] {
            if flags & mask != 0 {
                let mode = match block {
                    Some(block) if enabled => if stages.stage_mode(block, stage) == 2 { 2 } else { 1 },
                    _ => 0,
                };
                result = stages.set_stage(processing, stage, mode);
            }
        }
        if flags & 0x20 != 0 {
            let (_, [_, second]) = stages.stage_20(processing);
            result = stages.set_stage_20(processing, [u32::from(enabled), second]);
        }
        for (mask, bit) in [(0x40, 1), (0x80, 2)] {
            if flags & mask != 0 {
                let (_, [first, second]) = stages.stage_flags(processing);
                let first = if enabled { first | bit } else { first ^ bit };
                result = stages.set_stage_flags(processing, [first, second]);
            }
        }
    }
    for session in [input, transform, processing].into_iter().flatten() {
        result = stages.release(session);
    }
    result
}

/// 0xd9f7c0.
pub fn enable_configured_stages<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, block: &S::Block) -> i32 {
    enable_stages(stages, handle, flags, true, Some(block))
}

/// Disables the stages and resets the transform channels `flags` names (0x100, 0x200)
/// (0xd9f7d8).
pub fn disable_stages<S: ImageStages>(stages: &mut S, handle: u32, flags: u32) -> i32 {
    let result = enable_stages(stages, handle, flags, false, None);
    for (mask, channel) in [(0x100, 0), (0x200, 1)] {
        if flags & mask != 0 {
            reset_transform(stages, handle, channel);
        }
    }
    result
}

/// Resets transform channel `channel`: unit scale, zero offsets and unit coefficients
/// (0xd9e964).
pub fn reset_transform<S: ImageStages>(stages: &mut S, handle: u32, channel: u32) {
    let (_, session) = stages.acquire(Unit::Transform, handle);
    stages.set_transform_scale(&session, [0x1000, 0x1000], channel);
    let (_, [_, _, third, fourth]) = stages.transform_strides(&session, channel);
    stages.set_transform_strides(&session, [0, 0, third, fourth], channel);
    stages.set_transform_payload(&session, &[0x1_0000; 31], channel);
    stages.release(session);
}

/// Writes the processing region and input mode (flag 4), then the filter region (flag 2)
/// (0xda027c). Returns the last release's status.
pub fn write_region<S: ImageStages>(stages: &mut S, handle: u32, flags: u32, block: &S::Block) -> i32 {
    let mut result = 0;
    if flags & 4 != 0 {
        let (_, processing) = stages.acquire(Unit::Processing, handle);
        stages.write_region(&processing, block, RegionPart::ProcessingFirst);
        stages.write_region(&processing, block, RegionPart::ProcessingSecond);
        stages.release(processing);
        let (_, input) = stages.acquire(Unit::Input, handle);
        stages.write_region(&input, block, RegionPart::InputMode);
        result = stages.release(input);
    }
    if flags & 2 != 0 {
        let (_, filter) = stages.acquire(Unit::Filter, handle);
        stages.write_region(&filter, block, RegionPart::FilterRegion);
        result = stages.release(filter);
    }
    result
}
