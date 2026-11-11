#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureOuterHandler {
    BeginRecovery,
    Accept,
    Resume,
    PublishResume,
    Wait,
    RestartTimer,
    ArmTimer,
    Resource,
    ChildSelection,
    ChildScan,
    ChildCapture,
    ChildGeometry,
    ChildJob,
    RestoreJob,
    RestoreGeometry,
    ChildMode,
    ChildAuxiliary,
    ChildPrepare,
    Selection,
    Prepare,
    Defer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CaptureOperation(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CaptureOuterState(u8);

impl CaptureOuterState {
    pub const fn from_word(value: u32) -> Option<Self> {
        if value <= 16 { Some(Self(value as u8)) } else { None }
    }

    pub const fn word(self) -> u32 { self.0 as u32 }
}

pub fn route(state: CaptureOuterState, operation: CaptureOperation) -> Option<CaptureOuterHandler> {
    use CaptureOuterHandler::*;
    let operation = operation.0;
    match state.0 {
        0 => None,
        1 => match operation {
            0x3f13 => Some(Prepare),
            0x3f16 => Some(Selection),
            0x3f1a | 0x3f1e | 0x3f24 | 0x3f25 | 0x3f42 | 0x3f46 | 0x3f49 | 0x3f56 | 0x3f57 => Some(Defer),
            0x3f26 => Some(ChildScan),
            0x3f18 => Some(ChildAuxiliary),
            _ => None,
        },
        2 => match operation {
            0x3f13 => Some(Prepare),
            0x3f16 => Some(Selection),
            0x3f1a | 0x3f1e | 0x3f24 | 0x3f25 | 0x3f42 | 0x3f46 | 0x3f49 | 0x3f26 | 0x3f56 | 0x3f57 => Some(Defer),
            _ => Some(Prepare),
        },
        3 => match operation {
            0x3f13 => Some(Prepare),
            0x3f16 => Some(Selection),
            0x3f1a | 0x3f1e | 0x3f24 | 0x3f25 | 0x3f42 | 0x3f46 | 0x3f49 | 0x3f26 | 0x3f56 | 0x3f57 => Some(Defer),
            _ => Some(Selection),
        },
        4 => match operation {
            0x3f13 => Some(Prepare),
            0x3f16 => Some(Selection),
            0x3f1a => Some(ChildPrepare),
            0x3f1b | 0x3f1c | 0x3f01 | 0x3f00 | 0x3f07 | 0x3f08 | 0x3f09 | 0x3f0a => Some(Accept),
            0x3f1d => Some(ChildMode),
            0x3f1e => Some(ChildJob),
            0x3f24 | 0x3f25 => Some(Resume),
            0x3f42 => Some(ChildSelection),
            0x3f46 | 0x3f47 => Some(ChildGeometry),
            0x3f49 => Some(ChildCapture),
            0x3f26 => Some(ChildScan),
            0x3f56 | 0x3f57 => Some(Resource),
            _ => None,
        },
        5 => Some(ChildPrepare),
        6 => Some(ChildMode),
        7 => Some(ChildJob),
        8 => Some(Resume),
        9 => Some(RestoreGeometry),
        10 => Some(RestoreJob),
        11 => Some(ChildGeometry),
        12 => Some(ChildCapture),
        13 => Some(ChildScan),
        14 => Some(ChildAuxiliary),
        15 => Some(Resource),
        16 => match operation {
            0x3f00 => Some(BeginRecovery),
            0x3f01 => Some(Accept),
            0x3f02 => Some(ArmTimer),
            0x3f03 => Some(Wait),
            0x3f50 => Some(RestartTimer),
            0x3f39 => Some(PublishResume),
            _ => None,
        },
        _ => unreachable!(),
    }
}
