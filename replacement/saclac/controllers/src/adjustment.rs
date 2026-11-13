#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensPosition(pub i16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriveAmount(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionLimit(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriveScale(pub u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensRequest {
    ReserveOnly,
    Move { position: LensPosition, amount: DriveAmount, scale: DriveScale },
    SetPosition { position: LensPosition, amount: DriveAmount },
    SetPercentage { position: LensPosition, percent: Percentage },
    Stop,
    Synchronize,
    Query,
    Configure { selection: Selection, value: DriveAmount },
    ConfigureDefault,
    SetValue(DriveAmount),
    ConfigureValue(DriveAmount),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Percentage(u8);
impl Percentage {
    pub fn saturating(value: u32) -> Self {
        Self(value.min(100) as u8)
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reply {
    Success,
    Busy,
    SelectionOutOfRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect<C> {
    Request(LensRequest),
    Reply { callback: C, result: Reply },
    PublishQuery { high: u32, low: u8 },
    SelectionRejected,
}

pub trait Effects<C> {
    fn emit(&mut self, effect: Effect<C>);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    Progress,
    Finished,
    Query { success: bool, value: u32 },
}

pub struct Adjustment<C> {
    callback: Option<C>,
    busy: bool,
    selection_rejected: bool,
}

impl<C: Copy> Default for Adjustment<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Copy> Adjustment<C> {
    pub const fn new() -> Self {
        Self { callback: None, busy: false, selection_rejected: false }
    }

    pub fn begin(&mut self, callback: C, request: LensRequest, effects: &mut impl Effects<C>) {
        if self.busy {
            effects.emit(Effect::Reply { callback, result: Reply::Busy });
            return;
        }
        self.callback = Some(callback);
        self.busy = true;
        if request != LensRequest::ReserveOnly {
            effects.emit(Effect::Request(request));
        }
    }

    pub fn configure(
        &mut self,
        callback: C,
        selection: Selection,
        limit: SelectionLimit,
        value: DriveAmount,
        effects: &mut impl Effects<C>,
    ) {
        if self.busy {
            effects.emit(Effect::Reply { callback, result: Reply::Busy });
        } else if selection.0 >= limit.0 {
            self.selection_rejected = true;
            effects.emit(Effect::SelectionRejected);
            effects.emit(Effect::Reply { callback, result: Reply::SelectionOutOfRange });
        } else {
            self.selection_rejected = false;
            self.begin(callback, LensRequest::Configure { selection, value }, effects);
        }
    }

    pub fn configure_default(&mut self, callback: C, effects: &mut impl Effects<C>) {
        if !self.busy {
            self.selection_rejected = false;
        }
        self.begin(callback, LensRequest::ConfigureDefault, effects);
    }

    pub const fn selection_rejected(&self) -> bool {
        self.selection_rejected
    }

    pub fn complete(&mut self, completion: Completion, effects: &mut impl Effects<C>) {
        let Some(callback) = self.callback else { return };
        match completion {
            Completion::Progress => return,
            Completion::Finished => {}
            Completion::Query { success, value } => {
                let value = if success { value } else { 0 };
                effects.emit(Effect::PublishQuery { high: value >> 8, low: value as u8 });
            }
        }
        effects.emit(Effect::Reply { callback, result: Reply::Success });
        self.busy = false;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExposureMode {
    Off,
    Bounded,
    Maximum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureDuration(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureLimits {
    pub minimum: ExposureDuration,
    pub maximum: ExposureDuration,
}

pub fn exposure_duration(
    mode: ExposureMode,
    requested: ExposureDuration,
    limits: ExposureLimits,
) -> ExposureDuration {
    match mode {
        ExposureMode::Off => ExposureDuration(0),
        ExposureMode::Bounded if requested.0 < limits.minimum.0 => limits.minimum,
        ExposureMode::Bounded if requested.0 < limits.maximum.0 => requested,
        ExposureMode::Bounded | ExposureMode::Maximum => limits.maximum,
    }
}
