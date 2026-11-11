use crate::af_motion_runtime::{FocusDelta, FocusPosition, PositionSelection, ZoomPosition};
use xm1_lens_transport::control_runtime::Task99Client;
use xm1_calibration_core::{BiasBank, Calibration, ChannelIndex, CurveIndex};
use xm1_calibration_core::configuration::CfgKey;
use crate::metadata_calibration::RegionError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryContext { Default, Enabled, ModeOne, ModeTwo }

impl GeometryContext {
    pub const fn from_settings(enabled: bool, mode: u8) -> Self {
        if !enabled { Self::Default } else {
            match mode { 1 => Self::ModeOne, 2 => Self::ModeTwo, _ => Self::Enabled }
        }
    }
}

#[derive(Clone, Copy)]
enum ProfileBank { Primary, Secondary }
#[derive(Clone, Copy)]
enum Expansion { Nominal, Expanded }
#[derive(Clone, Copy)]
enum Anchor { Origin, Near, Far, ExtendedNear, ExtendedFar }
impl Anchor {
    const fn key(self) -> CfgKey {
        CfgKey(match self { Self::Origin => 0x3e0317, Self::Near => 0x3e0572,
            Self::Far => 0x3e063b, Self::ExtendedNear => 0x3e0704, Self::ExtendedFar => 0x3e07cd })
    }
}
#[derive(Clone, Copy)]
struct Profile { lower: Anchor, upper: Anchor, lower_margin: CfgKey, upper_margin: CfgKey }
#[derive(Clone, Copy)]
struct Limits { lower: i16, upper: i16 }
#[derive(Clone, Copy)]
enum SelectionDirection { Increasing, Decreasing, Center }
#[derive(Clone, Copy)]
struct SelectionDistance { magnitude: i32, direction: SelectionDirection }
impl SelectionDistance {
    fn parse(selection: PositionSelection) -> Option<Self> {
        if selection.0 as u32 & 0xc0000000 != 0 { return None; }
        let position = (selection.0 as i8 as i32).clamp(-5, 55);
        Some(if position == 25 { Self { magnitude: 0, direction: SelectionDirection::Center } }
            else if position > 25 { Self { magnitude: position - 25, direction: SelectionDirection::Increasing } }
            else { Self { magnitude: 25 - position, direction: SelectionDirection::Decreasing } })
    }
    fn coefficient_key(self, increasing: u32, decreasing: u32) -> CfgKey {
        match self.direction {
            SelectionDirection::Increasing => CfgKey(increasing.wrapping_add(self.magnitude as u32)),
            SelectionDirection::Decreasing => CfgKey(decreasing.wrapping_add(self.magnitude as u32)),
            SelectionDirection::Center => CfgKey(increasing),
        }
    }
}

pub struct PositionGeometry<'calibration, 'data> { calibration: &'calibration mut Calibration<'data> }
impl<'calibration, 'data> PositionGeometry<'calibration, 'data> {
    pub fn new(calibration: &'calibration mut Calibration<'data>) -> Self { Self { calibration } }

    pub fn context(&self, task99: &Task99Client<'_>) -> GeometryContext {
        if task99.special_encoding() { GeometryContext::from_settings(true, task99.cached_mode_byte()) } else { GeometryContext::Default }
    }

    pub fn iris_position(&mut self, index: CurveIndex) -> Result<u16, RegionError> {
        Ok(((self.calibration.collapse_curve_index(index)?.0 & 0x3ff) << 6) as u16)
    }

    pub fn target_delta(&mut self, requested: FocusDelta, current: FocusPosition, zoom: ZoomPosition,
        selection: PositionSelection, context: GeometryContext) -> Result<FocusDelta, RegionError>
    {
        if self.byte(CfgKey(0xa8))? & 0x20 == 0 { return Ok(requested); }
        let primary = self.limits(zoom, selection, context, ProfileBank::Primary, Expansion::Nominal)?;
        let primary_expanded = self.limits(zoom, selection, context, ProfileBank::Primary, Expansion::Expanded)?;
        let primary_middle = self.middle(zoom, selection, ProfileBank::Primary)?;
        let secondary = self.limits(zoom, selection, context, ProfileBank::Secondary, Expansion::Nominal)?;
        let secondary_expanded = self.limits(zoom, selection, context, ProfileBank::Secondary, Expansion::Expanded)?;
        let secondary_middle = self.middle(zoom, selection, ProfileBank::Secondary)?;
        let selected = match self.byte(CfgKey(0xbd))? {
            1 => primary.lower, 2 => primary_middle, 3 => primary.upper, 4 => primary_expanded.upper,
            0x10 => secondary_expanded.lower, 0x11 => secondary.lower, 0x12 => secondary_middle,
            0x13 => secondary.upper, 0x14 => secondary_expanded.upper, _ => primary_expanded.lower,
        };
        Ok(FocusDelta(selected.wrapping_sub(current.0)))
    }

    fn byte(&mut self, key: CfgKey) -> Result<u8, RegionError> { self.calibration.configuration_mut().read_byte(key) }
    fn half(&mut self, zoom: ZoomPosition, key: CfgKey) -> Result<i16, RegionError> {
        Ok(self.calibration.configuration_mut().read_half(key.wrapping_add((zoom.0 as u32).wrapping_mul(2)))? as i16)
    }
    fn in_range(&mut self, zoom: ZoomPosition, key: CfgKey) -> Result<bool, RegionError> {
        Ok(zoom.0 <= self.byte(key)? as i32 - 1)
    }
    fn anchor(&mut self, zoom: ZoomPosition, anchor: Anchor) -> Result<i16, RegionError> {
        let anchor = match anchor {
            Anchor::ExtendedFar if self.byte(CfgKey(0x3e0313))? == 0 || !self.in_range(zoom, CfgKey(0x3e1214))? => Anchor::Far,
            other => other,
        };
        let anchor = match anchor {
            Anchor::ExtendedNear if !self.in_range(zoom, CfgKey(0x3e1213))? => Anchor::Origin,
            Anchor::Far if !self.in_range(zoom, CfgKey(0x3e1213))? => Anchor::Near,
            other => other,
        };
        let bias = self.calibration.channel_bias(ChannelIndex(zoom.0), BiasBank::Primary)?.0;
        if matches!(anchor, Anchor::Origin) { Ok(bias) } else { Ok(bias.wrapping_add(self.half(zoom, anchor.key())?)) }
    }
    fn profile(bank: ProfileBank, context: GeometryContext) -> Profile {
        let (lower, upper, lower_margin, upper_margin) = match context {
            GeometryContext::Default => match bank {
                ProfileBank::Primary => (Anchor::Origin, Anchor::Near, 0x3e0b40, 0x3e0c09),
                ProfileBank::Secondary => (Anchor::ExtendedNear, Anchor::Far, 0x3e0cd2, 0x3e0d9b),
            },
            GeometryContext::Enabled => (Anchor::Origin, Anchor::Far, 0x3e0b40, 0x3e0d9b),
            GeometryContext::ModeOne => (Anchor::Origin, Anchor::Near, 0x3e0b40, 0x3e0c09),
            GeometryContext::ModeTwo => (Anchor::Near, Anchor::Far, 0x3e0cd2, 0x3e0d9b),
        };
        Profile { lower, upper, lower_margin: CfgKey(lower_margin), upper_margin: CfgKey(upper_margin) }
    }
    fn raw_limits(&mut self, zoom: ZoomPosition, profile: Profile) -> Result<Limits, RegionError> {
        Ok(Limits { lower: self.anchor(zoom, profile.lower)?, upper: self.anchor(zoom, profile.upper)? })
    }
    fn correction(&mut self, zoom: ZoomPosition, anchor_index: usize, distance: SelectionDistance) -> Result<i16, RegionError> {
        let side = match distance.direction { SelectionDirection::Increasing => 0, SelectionDirection::Decreasing => 1, SelectionDirection::Center => return Ok(0) };
        let keys = [[0x3e2800, 0x3e28c9], [0x3e2992, 0x3e2a5b], [0x3e2b24, 0x3e2bed], [0x3e2b24, 0x3e2bed]];
        let value = self.half(zoom, CfgKey(keys[anchor_index][side]))? as i32;
        let coefficient = self.byte(distance.coefficient_key(0x3e2cb5, 0x3e2cd4))?.min(240) as i32;
        Ok((value * coefficient / 240) as i16)
    }
    fn adjust(&mut self, zoom: ZoomPosition, selection: PositionSelection, value: i16) -> Result<i16, RegionError> {
        let Some(distance) = SelectionDistance::parse(selection) else { return Ok(value); };
        if matches!(distance.direction, SelectionDirection::Center) { return Ok(value); }
        let base = self.raw_limits(zoom, Self::profile(ProfileBank::Primary, GeometryContext::Default))?;
        let far = self.anchor(zoom, Anchor::Far)?;
        let extended = self.anchor(zoom, Anchor::ExtendedFar)?;
        let shifted_lower = base.lower.wrapping_add(self.correction(zoom, 0, distance)?);
        let shifted_upper = base.upper.wrapping_add(self.correction(zoom, 1, distance)?);
        let shifted_far = far.wrapping_add(self.correction(zoom, 2, distance)?);
        let (origin, shifted, width, adjusted_width) = if base.upper < value && self.in_range(zoom, CfgKey(0x3e1213))? {
            if self.byte(CfgKey(0x3e0313))? != 0 && (value < base.upper || far < value) && self.in_range(zoom, CfgKey(0x3e1214))? {
                (far, shifted_far, extended.wrapping_sub(far), extended.wrapping_sub(shifted_far).wrapping_add(self.correction(zoom, 3, distance)?))
            } else { (base.upper, shifted_upper, far.wrapping_sub(base.upper), shifted_far.wrapping_sub(shifted_upper)) }
        } else { (base.lower, shifted_lower, base.upper.wrapping_sub(base.lower), shifted_upper.wrapping_sub(shifted_lower)) };
        if width <= 0 || adjusted_width < 0 { return Ok(value); }
        let numerator = (adjusted_width as i32).wrapping_mul((value as i32).wrapping_sub(origin as i32));
        let rounded_up = (numerator.wrapping_mul(10) / width as i32) % 10 > 4;
        Ok(shifted.wrapping_add((numerator / width as i32) as i16).wrapping_add(i16::from(rounded_up)))
    }
    fn middle(&mut self, zoom: ZoomPosition, selection: PositionSelection, bank: ProfileBank) -> Result<i16, RegionError> {
        let key = match bank { ProfileBank::Primary => CfgKey(0x3e07f7), ProfileBank::Secondary => CfgKey(0x3e08c0) };
        let bias = self.calibration.channel_bias(ChannelIndex(zoom.0), BiasBank::Primary)?.0;
        let offset = self.half(zoom, key)?;
        self.adjust(zoom, selection, bias.wrapping_add(offset))
    }
    fn margin(&mut self, zoom: ZoomPosition, value: i16, distance: SelectionDistance) -> Result<i16, RegionError> {
        let origin = self.anchor(zoom, Anchor::Origin)?;
        let near = self.anchor(zoom, Anchor::Near)?;
        let far = self.anchor(zoom, Anchor::Far)?;
        let origin_distance = absolute_distance(value, origin);
        let near_distance = absolute_distance(value, near);
        let far_distance = absolute_distance(value, far);
        let (key, coefficient) = if far_distance < origin_distance.min(near_distance) {
            (0x3e2e86, 0x3e2fe9)
        } else if near_distance < origin_distance { (0x3e2dbd, 0x3e2fab) } else { (0x3e2cf4, 0x3e2f6d) };
        let coefficient_key = match distance.direction {
            SelectionDirection::Increasing | SelectionDirection::Center => CfgKey(coefficient + distance.magnitude as u32),
            SelectionDirection::Decreasing => CfgKey(coefficient - distance.magnitude as u32),
        };
        Ok((self.half(zoom, CfgKey(key))? as i32 * self.byte(coefficient_key)?.min(240) as i32 / 240) as i16)
    }
    fn limits(&mut self, zoom: ZoomPosition, selection: PositionSelection, context: GeometryContext, bank: ProfileBank, expansion: Expansion) -> Result<Limits, RegionError> {
        let profile = Self::profile(bank, context);
        let raw = self.raw_limits(zoom, profile)?;
        let distance = SelectionDistance::parse(selection);
        let mut result = Limits { lower: self.adjust(zoom, selection, raw.lower)?, upper: self.adjust(zoom, selection, raw.upper)? };
        if matches!(expansion, Expansion::Expanded) {
            let margins = match distance {
                None => Limits { lower: self.half(zoom, profile.lower_margin)?, upper: self.half(zoom, profile.upper_margin)? },
                Some(distance) => Limits { lower: self.margin(zoom, raw.lower, distance)?, upper: self.margin(zoom, raw.upper, distance)? },
            };
            result.lower = result.lower.wrapping_sub(margins.lower);
            result.upper = result.upper.wrapping_add(margins.upper);
        }
        if distance.is_some() {
            let alternate_lower = match profile.lower {
                Anchor::ExtendedNear => true, Anchor::Origin => false,
                _ => absolute_distance(raw.lower, self.anchor(zoom, Anchor::ExtendedNear)?) < absolute_distance(raw.lower, self.anchor(zoom, Anchor::Origin)?),
            };
            let alternate_upper = match profile.upper {
                Anchor::Far | Anchor::ExtendedFar => true, Anchor::Near => false,
                _ => absolute_distance(raw.upper, self.anchor(zoom, Anchor::Far)?) < absolute_distance(raw.upper, self.anchor(zoom, Anchor::Near)?),
            };
            let alternate_lower = alternate_lower && self.in_range(zoom, CfgKey(0x3e1213))?;
            let alternate_upper = alternate_upper && self.in_range(zoom, CfgKey(0x3e1213))?;
            let lower_margin = self.half(zoom, CfgKey(if alternate_lower { 0x3e0cd2 } else { 0x3e0b40 }))?;
            let upper_margin = self.half(zoom, CfgKey(if alternate_upper { 0x3e0d9b } else { 0x3e0c09 }))?;
            result.lower = result.lower.max(raw.lower.wrapping_sub(lower_margin));
            result.upper = result.upper.min(raw.upper.wrapping_add(upper_margin));
        }
        Ok(result)
    }
}

fn absolute_distance(first: i16, second: i16) -> u16 { ((first as i32 - second as i32).abs()) as u16 }

#[cfg(test)]
#[path = "af_position_geometry_tests.rs"]
mod tests;
