use super::shared::{Address32, ByteOffset, FrameAddress, ObjectGroup, ObjectKind, OperationCode, ServiceCompletion, StageContext, StagePlan, StageResult};
use crate::develop::framework;

#[derive(Clone, Copy)]
pub(super) enum Stage {
    First,
    Second,
    Auxiliary,
}

#[derive(Clone, Copy)]
struct SelectionWord(u32);

#[derive(Clone, Copy)]
enum InputSelection {
    First,
    Second,
    Other(SelectionWord),
}

impl InputSelection {
    fn parse(value: u32) -> Self {
        match value {
            0 => Self::First,
            1 => Self::Second,
            other => Self::Other(SelectionWord(other)),
        }
    }

    fn word(self) -> u32 {
        match self {
            Self::First => 0,
            Self::Second => 1,
            Self::Other(value) => value.0,
        }
    }
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum Destination {
    Main = 0,
    Auxiliary = 1,
}

impl Destination {
    fn parse(value: u32) -> Self {
        if value == 1 { Self::Auxiliary } else { Self::Main }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModeClass {
    PreferAlternate,
    AllowAlternate,
    Suppressed,
    Ordinary,
}

impl ModeClass {
    fn parse(value: u32) -> Self {
        match value {
            0x19 => Self::PreferAlternate,
            0x1a => Self::AllowAlternate,
            0x1d | 0x1e | 0x2c => Self::Suppressed,
            _ => Self::Ordinary,
        }
    }
}

#[repr(transparent)]
struct ConversionProgram(u32);

unsafe extern "C" {
    fn xm1_pipeline_image_transform_output(frame: FrameAddress, image: Address32, destination: Destination, program: ConversionProgram) -> i32;
}

unsafe fn suppressed(frame: FrameAddress) -> bool {
    let mode = ModeClass::parse(unsafe { frame.field(ByteOffset(0x4b4)).word() });
    mode == ModeClass::Suppressed
        || matches!(unsafe { frame.field(ByteOffset(0x578)).word() }, 1)
        || matches!(unsafe { frame.field(ByteOffset(0x4a8)).word() }, 7)
}

unsafe fn source_image(context: StageContext, input: InputSelection, destination: Destination) -> Address32 {
    let frame = unsafe { context.frame() };
    match destination {
        Destination::Auxiliary => frame.field(ByteOffset(0x2d4)),
        Destination::Main => {
            let mode = ModeClass::parse(unsafe { frame.field(ByteOffset(0x4b4)).word() });
            let alternate = mode == ModeClass::PreferAlternate
                || unsafe { frame.field(ByteOffset(0x710)).word() } & 0x400 != 0
                || mode == ModeClass::AllowAlternate;
            if alternate && matches!(input, InputSelection::Second) {
                frame.field(ByteOffset(0x1a0))
            } else {
                frame.field(ByteOffset(0x184))
            }
        }
    }
}

unsafe fn transform(context: StageContext, input: InputSelection, destination: Destination) -> StageResult {
    let operation = match destination {
        Destination::Auxiliary => OperationCode(0x15),
        Destination::Main => OperationCode(if matches!(input, InputSelection::Second) { 0x13 } else { 0x14 }),
    };
    let plan = unsafe { StagePlan::develop(operation) };
    if unsafe { framework::xm1_develop_precondition(context.0, input.word()) } == 0 {
        return unsafe { plan.skip(context) };
    }
    if unsafe { framework::xm1_develop_transform_required(context.0, input.word()) } == 0 {
        return unsafe { plan.skip(context) };
    }
    if unsafe { suppressed(context.frame()) } {
        return unsafe { plan.skip(context) };
    }
    let result = unsafe { plan.begin(context) };
    if result != StageResult::SUCCESS {
        return result;
    }
    let image = unsafe { source_image(context, input, destination) };
    let object = unsafe { context.object(ObjectGroup(0x23), ObjectKind(0)) };
    let program = ConversionProgram(unsafe { object.offset(ByteOffset(4)).word() });
    let frame = unsafe { context.frame() };
    if unsafe { frame.field(ByteOffset(0x27cc)).word() } == 0 {
        let _ = unsafe { xm1_pipeline_image_transform_output(frame, image, destination, program) };
    }
    unsafe { plan.complete(context, ServiceCompletion(1)) }
}

pub(super) unsafe fn execute(context: StageContext, stage: Stage) -> StageResult {
    let (input, destination) = match stage {
        Stage::First => (InputSelection::First, Destination::Main),
        Stage::Second => (InputSelection::Second, Destination::Main),
        Stage::Auxiliary => (InputSelection::Second, Destination::Auxiliary),
    };
    unsafe { transform(context, input, destination) }
}

#[no_mangle]
#[link_section = ".text.xm1_develop_pipeline_transform_source"]
pub unsafe extern "C" fn xm1_develop_pipeline_transform_source(context: *mut u8, input: u32, destination: u32) -> u32 {
    unsafe { transform(StageContext(context), InputSelection::parse(input), Destination::parse(destination)) }.0
}
