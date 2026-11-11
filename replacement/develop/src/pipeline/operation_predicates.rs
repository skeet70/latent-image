use super::shared::{Address32, ByteOffset, ObjectGroup, StageContext};

#[derive(Clone, Copy)]
pub(super) enum Predicate {
    Always,
    FormatSplit,
    FormatFamily,
    PrimaryPlaneEnabled,
    SecondaryPlaneEnabled,
    CaptureOperation,
    ObjectOperation,
    AlternateObjectOperation,
    HalfwordModeSplit,
    Never,
    ProcessingOperation,
    FeatureEnabled,
    FeatureDisabled,
    CorrectionDisabled,
    CorrectionEnabled,
    AlternateFeatureDisabled,
    GroupModeSplit,
    OutputModeSplit,
}

impl Predicate {
    pub(super) unsafe fn accepts(self, context: StageContext, group: ObjectGroup) -> bool {
        unsafe {
            match self {
                Self::Always => true,
                Self::Never => false,
                Self::FormatSplit => match group.0 {
                    0x1a => field(context, 0x530) != 5,
                    0x2e => field(context, 0x530) == 5,
                    _ => false,
                },
                Self::FormatFamily => match group.0 {
                    0x1a => matches!(field(context, 0x530), 3 | 4 | 5 | 13),
                    0x2e => field(context, 0x530) != 5,
                    _ => false,
                },
                Self::PrimaryPlaneEnabled | Self::SecondaryPlaneEnabled => {
                    let expected = match self { Self::PrimaryPlaneEnabled => 0x2f, _ => 0x30 };
                    group.0 == expected && pointed(context, 0x3228, 0x110) == 1
                }
                Self::CaptureOperation => {
                    let operation = crate::develop::framework::xm1_develop_current(context.0);
                    if group.0 != 0x19 { return false; }
                    let frame = context.frame();
                    let read = |offset| frame.field(ByteOffset(offset)).word();
                    let mode = read(0x578);
                    let expected = if mode == 1 {
                        if read(0x4a8) == 0 && read(0x550).wrapping_sub(0x97) <= 1 { 0x2d } else { 0x49 }
                    } else if read(0x5a8) == 1
                        || (read(0x4a8) == 0 && read(0x550).wrapping_sub(0x97) <= 1)
                        || read(0x4ac) == 1 || read(0x614) != 0 || read(0x61c) != 0 {
                        if mode == 0 { 0x2d } else { 0x49 }
                    } else { 0x45 };
                    operation == expected
                }
                Self::ObjectOperation | Self::AlternateObjectOperation => {
                    let operation = crate::develop::framework::xm1_develop_current(context.0);
                    if !matches!(group.0, 0x2b | 0x14) { return false; }
                    let frame = context.frame();
                    let expected = if frame.field(ByteOffset(0x578)).word() != 0 { 0x65 }
                        else if (matches!(self, Self::ObjectOperation) && frame.field(ByteOffset(0x63c)).word() != 0)
                            || frame.field(ByteOffset(0x19b4)).byte() != 0 { 0x25 }
                        else if matches!(self, Self::ObjectOperation) { 0x21 } else { 0x23 };
                    operation == expected
                }
                Self::HalfwordModeSplit => match group.0 {
                    0x1a => context.frame().field(ByteOffset(0x1682)).half() == 1,
                    0x1b => context.frame().field(ByteOffset(0x1682)).half() != 1,
                    _ => false,
                },
                Self::ProcessingOperation => {
                    let operation = crate::develop::framework::xm1_develop_current(context.0);
                    if group.0 != 0x20 { return false; }
                    let mode = field(context, 0x4a8);
                    operation == if mode == 8 || mode.wrapping_sub(4) <= 2 || mode == 9 { 0x3d } else { 0x39 }
                }
                Self::FeatureEnabled => field(context, 0x2fb8) == 1,
                Self::FeatureDisabled => field(context, 0x2fb8) != 1,
                Self::CorrectionDisabled | Self::CorrectionEnabled => {
                    let frame = context.frame();
                    let object = Address32(frame.field(ByteOffset(0x3098)).word());
                    let enabled = object.offset(ByteOffset(0x58)).word() == 1;
                    if !enabled { return matches!(self, Self::CorrectionDisabled); }
                    let mode = frame.field(ByteOffset(0x4b4)).word();
                    let offset = match mode { 0x19 => 0x5b8, 0x1a => 0x5b4, _ => return false };
                    (frame.field(ByteOffset(offset)).word() == 1) == matches!(self, Self::CorrectionEnabled)
                }
                Self::AlternateFeatureDisabled => field(context, 0x2fbc) != 1,
                Self::GroupModeSplit => match group.0 {
                    0x19 => field(context, 0x63c) == 0,
                    0x1b => field(context, 0x63c) != 0,
                    _ => false,
                },
                Self::OutputModeSplit => match group.0 {
                    0x29 => field(context, 0x63c) == 0,
                    0x2a => field(context, 0x63c) == 2,
                    _ => false,
                },
            }
        }
    }
}

unsafe fn field(context: StageContext, offset: u32) -> u32 {
    unsafe { context.frame().field(ByteOffset(offset)).word() }
}

unsafe fn pointed(context: StageContext, pointer: u32, offset: u32) -> u32 {
    unsafe { Address32(field(context, pointer)).offset(ByteOffset(offset)).word() }
}

macro_rules! publish {
    ($($name:ident => $variant:ident),* $(,)?) => { $(
        #[no_mangle]
        pub unsafe extern "C" fn $name(context: *mut u8, group: u32) -> u32 {
            unsafe { Predicate::$variant.accepts(StageContext(context), ObjectGroup(group)) as u32 }
        }
    )* };
}

publish! {
    xm1_develop_predicate_format_split => FormatSplit,
    xm1_develop_predicate_format_family => FormatFamily,
    xm1_develop_predicate_primary_plane_enabled => PrimaryPlaneEnabled,
    xm1_develop_predicate_secondary_plane_enabled => SecondaryPlaneEnabled,
    xm1_develop_predicate_capture_operation => CaptureOperation,
    xm1_develop_predicate_object_operation => ObjectOperation,
    xm1_develop_predicate_alternate_object_operation => AlternateObjectOperation,
    xm1_develop_predicate_halfword_mode_split => HalfwordModeSplit,
    xm1_develop_predicate_never => Never,
    xm1_develop_predicate_processing_operation => ProcessingOperation,
    xm1_develop_predicate_feature_enabled => FeatureEnabled,
    xm1_develop_predicate_feature_disabled => FeatureDisabled,
    xm1_develop_predicate_correction_disabled => CorrectionDisabled,
    xm1_develop_predicate_correction_enabled => CorrectionEnabled,
    xm1_develop_predicate_alternate_feature_disabled => AlternateFeatureDisabled,
    xm1_develop_predicate_group_mode_split => GroupModeSplit,
    xm1_develop_predicate_output_mode_split => OutputModeSplit,
}
