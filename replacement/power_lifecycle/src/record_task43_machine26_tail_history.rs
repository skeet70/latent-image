//! x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; image5 f453d4..f45a80.
//! NONMATCHING storage widths, signed comparisons and history record offsets are byte-confirmed.

use crate::record_address_space::{ByteCount, GuestAddressSpace, GuestFrame, MemoryError, RecordContext, RecordError, StackAlignment};
use crate::record_task43_effects::RecordingEffects;
use crate::record_task43_machine26_analysis_history::{helper_f43b48, helper_f42a6c, helper_f44afc, helper_f44e38, helper_f43e90};
use crate::record_task43_machine26_state::{b, h, pb, ph, put, s, store, w, word};

#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum ConfigurationKey {
    MinimumSamples = 0x87f0,
    HistoryDirection = 0x8b12,
    DirectionMode = 0x8849,
    DirectionThreshold = 0x884c,
    FirstMargin = 0x884b,
    SecondMargin = 0x884a,
    ExactComparison = 0x8778,
    HistoryMinimumDelta = 0x8b17,
    HistoryRiseDelta = 0x8b1b,
    CrossingLimit = 0x8b1a,
    RevisionFlags = 0x8877,
    RevisionEnabled = 0x876d,
    ScaleMode = 0x3e2713,
    ExposureMode = 0x3e0002,
    ThresholdHighest = 0x8782,
    ThresholdHigh = 0x8781,
    ThresholdMiddle = 0x8780,
    ThresholdLow = 0x877f,
    ManualScaleHighest = 0x8754,
    ManualScaleHigh = 0x8753,
    ManualScaleMiddle = 0x8752,
    ManualScaleLow = 0x8751,
    ManualScaleLowest = 0x8750,
    AutomaticScaleHighest = 0x877d,
    AutomaticScaleHigh = 0x877c,
    AutomaticScaleMiddle = 0x877b,
    AutomaticScaleLow = 0x877a,
    AutomaticScaleLowest = 0x8779,
    StepDivisor = 0x8776,
    AdjustReversal = 0x878c,
    ReversalAmount = 0x3e0240,
    PrimaryRevision = 0x883e,
    DefaultRevision = 0x883d,
}
impl ConfigurationKey {
    pub(super) fn byte<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        crate::record_configuration::helper_69f8fc(context, self as u32)
    }
    pub(super) fn half<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
        crate::record_configuration::helper_69fec8(context, self as u32)
    }
    pub(super) fn signed_half<M: GuestAddressSpace, E>(self, context: &mut RecordContext<M, E>) -> Result<i32, MemoryError> {
        crate::record_configuration::helper_69ff7c(context, self as u32)
    }
}

#[derive(Clone, Copy)]
struct HistoryIndex(i8);
impl HistoryIndex {
    fn current<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<Self, MemoryError> { Ok(Self(b(context, 0x121c)? as i8)) }
    fn previous(self, count: i8) -> Self { Self(self.0.wrapping_sub(count)) }
}

fn load_metric<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestFrame, index: HistoryIndex) -> Result<i16, RecordError> {
    helper_f42a6c(context, record.base(), index.0 as i32 as u32)?;
    Ok(context.read_u16(record.at(ByteCount(12), ByteCount(2))?)? as i16)
}

pub fn helper_f453d4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, position: i32, direction: i32) -> Result<i32, RecordError> {
    context.with_guest_frame(ByteCount(40), StackAlignment::Doubleword, |context, frame| {
        if b(context, 0x121d)? as i8 == -1 { return Ok(0); }
        let record = frame.subframe(ByteCount(4), ByteCount(20))?;
        let index = b(context, 0x121f)? as i8 as i32 as u32;
        helper_f43b48(context, record.base(), index)?;
        let previous = context.read_u16(record.at(ByteCount(14), ByteCount(2))?)? as i16 as i32;
        Ok(i32::from(match direction { -1 => previous > position, 1 => previous < position, _ => false }))
    })
}

#[derive(Clone, Copy)]
enum Trend { Falling, Rising }

fn direction<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, trend: Trend) -> Result<(), MemoryError> {
    let previous_is_lower = s(context, 0x238)? > s(context, 0x23a)?;
    let negative = match trend { Trend::Falling => previous_is_lower, Trend::Rising => !previous_is_lower };
    pb(context, 0x2c, if negative { 255 } else { 1 })
}

fn falling<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    if b(context, 0x203)? != 0 {
        pb(context, 0x203, 0)?;
        let count = b(context, 0x201)?.wrapping_add(1);
        pb(context, 0x201, count)?;
    }
    pb(context, 0x200, 0)?;
    direction(context, Trend::Falling)
}

fn rising<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    pb(context, 0x203, 1)?;
    let count = b(context, 0x200)?.wrapping_add(1);
    pb(context, 0x200, count)?;
    direction(context, Trend::Rising)
}

fn update_history_direction<M: GuestAddressSpace, E: RecordingEffects<M>>(context: &mut RecordContext<M, E>, record: GuestFrame) -> Result<(), RecordError> {
    let index = HistoryIndex::current(context)?;
    if index.0 <= 0 { return Ok(()); }
    let current = load_metric(context, record, index)?;
    let previous_index = HistoryIndex::current(context)?.previous(1);
    let previous = load_metric(context, record, previous_index)?;
    if (previous.wrapping_sub(current) as i32) < ConfigurationKey::HistoryMinimumDelta.signed_half(context)? {
        put(context, 0x123c, 1)?;
        let reversed = b(context, 0x2d)?.wrapping_neg();
        pb(context, 0x2c, reversed)?;
        helper_f44e38(context)?;
        helper_f43e90(context)?;
        ph(context, 0x122a, 0)?;
        return Ok(());
    }
    if HistoryIndex::current(context)?.0 <= 1 { return Ok(()); }
    put(context, 0x123c, 0)?;
    let previous_direction = b(context, 0x2d)?;
    pb(context, 0x2c, previous_direction)?;
    let updated = helper_f44e38(context)?;
    helper_f43e90(context)?;
    if current.wrapping_sub(previous) as i32 > ConfigurationKey::HistoryRiseDelta.signed_half(context)? {
        let older_index = HistoryIndex::current(context)?.previous(2);
        let older = load_metric(context, record, older_index)?;
        if older.wrapping_sub(previous) as i32 > ConfigurationKey::HistoryRiseDelta.signed_half(context)? {
            let count = h(context, 0x122e)?.wrapping_add(1);
            ph(context, 0x122e, count)?;
        }
    }
    if updated == 1 {
        ph(context, 0x122a, 0)?;
        ConfigurationKey::CrossingLimit.byte(context)?;
        return Ok(());
    }
    let position = s(context, 0x238)?;
    let selected_direction = b(context, 0x2c)? as i8 as i32;
    if helper_f453d4(context, position, selected_direction)? == 1 {
        let count = h(context, 0x122a)?.wrapping_add(1);
        ph(context, 0x122a, count)?;
    }
    let crossings = h(context, 0x122a)? as u32;
    if crossings > ConfigurationKey::CrossingLimit.byte(context)? {
        put(context, 0x123c, 1)?;
        let reversed = b(context, 0x2d)?.wrapping_neg();
        pb(context, 0x2c, reversed)?;
        ph(context, 0x122a, 0)?;
    }
    Ok(())
}

pub fn helper_f45460<M: GuestAddressSpace, E: RecordingEffects<M>>(context: &mut RecordContext<M, E>) -> Result<(), RecordError> {
    context.with_guest_frame(ByteCount(48), StackAlignment::Doubleword, |context, frame| {
        let old = word(context, 0x016c_0f88)?;
        let next = helper_f44afc(context, old)?;
        store(context, 0x016c_0f88, next)?;
        let samples = b(context, 4)? as u32;
        if samples < ConfigurationKey::MinimumSamples.byte(context)? || s(context, 0x238)? == s(context, 0x23a)? {
            let previous = b(context, 0x2d)?;
            pb(context, 0x2c, previous)?;
            return Ok(());
        }
        if ConfigurationKey::HistoryDirection.byte(context)? != 0 && word(context, 0x016c_0f88)? != 0 {
            return update_history_direction(context, frame.subframe(ByteCount(4), ByteCount(20))?);
        }
        let direction_mode = ConfigurationKey::DirectionMode.byte(context)?;
        let use_threshold = (direction_mode > 2 || ConfigurationKey::DirectionMode.byte(context)? == 1) && w(context, 0xf9c)? == 1;
        let use_threshold = if use_threshold {
            let current = w(context, 0x224)?;
            current < ConfigurationKey::DirectionThreshold.half(context)?.wrapping_mul(100)
        } else { false };
        if use_threshold {
            let current = w(context, 0x224)?;
            let previous = w(context, 0x228)?;
            let margin = previous.wrapping_mul(ConfigurationKey::FirstMargin.byte(context)?) / 100;
            if previous > current.wrapping_add(margin) { falling(context)?; }
            else {
                if w(context, 0x224)? >= w(context, 0x228)? {
                    pb(context, 0x203, 1)?;
                    let count = b(context, 0x200)?.wrapping_add(1);
                    pb(context, 0x200, count)?;
                }
                direction(context, Trend::Rising)?;
            }
            return Ok(());
        }
        if ConfigurationKey::ExactComparison.byte(context)? == 1 {
            pb(context, 0x211, 0)?;
            let current = w(context, 0x224)?;
            let previous = w(context, 0x228)?;
            if current < previous {
                if b(context, 0x203)? != 0 {
                    pb(context, 0x203, 0)?;
                    let count = b(context, 0x201)?.wrapping_add(1);
                    pb(context, 0x201, count)?;
                }
                pb(context, 0x200, 0)?;
                pb(context, 0x211, 1)?;
                direction(context, Trend::Falling)?;
            } else if current > previous { rising(context)?; }
            else {
                pb(context, 0x203, 0)?;
                pb(context, 0x200, 0)?;
                direction(context, Trend::Falling)?;
            }
        } else {
            let current = w(context, 0x224)?;
            let previous = w(context, 0x228)?;
            let margin = previous.wrapping_mul(ConfigurationKey::SecondMargin.byte(context)?) / 100;
            if previous > current.wrapping_add(margin) { falling(context)?; } else { rising(context)?; }
        }
        Ok(())
    })
}
