use super::shared::{Address32, ByteOffset, ObjectGroup, ObjectKind, OperationCode, ServiceCompletion, StageContext, StageResult};
use super::stage::StagePlan;
use crate::develop::framework;
use core::ptr::{read_volatile, write_volatile};

#[repr(transparent)]
#[derive(Clone, Copy)]
pub(super) struct Selection(pub(super) u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(super) struct ProcessingMode(pub(super) u32);
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Plane {
    format: u32,
    address: Address32,
    length: u32,
    stride: u32,
    width: u32,
    height: u32,
    flags: u32,
}

#[repr(C)]
struct OutputRequest {
    mode: ProcessingMode,
    selection: Selection,
    reserved: u32,
}

#[repr(C)]
struct FinishRequest {
    prefix: core::mem::MaybeUninit<u32>,
    selection: Selection,
    configuration: Address32,
    primary: core::mem::MaybeUninit<Address32>,
    secondary: core::mem::MaybeUninit<Address32>,
    reserved: core::mem::MaybeUninit<u32>,
}

#[repr(C)]
#[derive(Default)]
struct BorderAmounts {
    top: u32,
    bottom: u32,
    left: u32,
    right: u32,
}
#[derive(Clone, Copy)]
pub(super) enum Stage {
    MergeReferences, AppendReference, FilterReference, AlignReference, ApplyAlignment,
    FusePyramids(ProcessingMode), FilterPyramid, PreparePyramid, MergePyramid,
    ResolveReferences, BuildPyramid, CombinePlanes, ConfigureReferences,
    ConvertReference(ProcessingMode), FinishOutput(Selection),
    ResizeOutput(ProcessingMode, Selection), CopyOutput(ProcessingMode, Selection),
    PrepareOutput(Selection, ProcessingMode), ApplyOutput, ApplyOutputPlanes,
    PrepareOutputPlanes, ResizePreview, PreparePreview, ConvertOutput(Selection),
    ProcessCapture(ProcessingMode), PrepareCapture(Selection, ProcessingMode), FilterCapture(Selection),
}

impl Stage {
    pub(super) fn from_command(command: OperationCode) -> Option<Self> {
        Some(match command.0 {
            0x0e => Self::FilterCapture(Selection(0)),
            0x0f => Self::FilterCapture(Selection(1)),
            0x10 => Self::PrepareCapture(Selection(0), ProcessingMode(0)),
            0x11 => Self::PrepareCapture(Selection(1), ProcessingMode(0)),
            0x12 => Self::ProcessCapture(ProcessingMode(0)),
            0x16 => Self::ConvertOutput(Selection(1)),
            0x17 => Self::ConvertOutput(Selection(0)),
            0x18 => Self::PreparePreview,
            0x19 => Self::PrepareOutputPlanes,
            0x1a => Self::ApplyOutputPlanes,
            0x1b => Self::ApplyOutput,
            0x1c => Self::PrepareOutput(Selection(1), ProcessingMode(0)),
            0x1d => Self::PrepareOutput(Selection(0), ProcessingMode(0)),
            0x1e => Self::ResizePreview,
            0x1f => Self::CopyOutput(ProcessingMode(0), Selection(2)),
            0x20 => Self::CopyOutput(ProcessingMode(0), Selection(1)),
            0x21 => Self::CopyOutput(ProcessingMode(0), Selection(0)),
            0x22 => Self::ResizeOutput(ProcessingMode(0), Selection(1)),
            0x23 => Self::ResizeOutput(ProcessingMode(0), Selection(0)),
            0x24 => Self::ResizeOutput(ProcessingMode(0), Selection(2)),
            0x25 => Self::ResizeOutput(ProcessingMode(1), Selection(1)),
            0x26 => Self::ResizeOutput(ProcessingMode(1), Selection(0)),
            0x27 => Self::ResizeOutput(ProcessingMode(1), Selection(2)),
            0x28 => Self::CopyOutput(ProcessingMode(2), Selection(1)),
            0x29 => Self::CopyOutput(ProcessingMode(2), Selection(0)),
            0x2e => Self::FinishOutput(Selection(2)),
            0x2f => Self::FinishOutput(Selection(4)),
            0x30 => Self::FinishOutput(Selection(1)),
            0x31 => Self::FinishOutput(Selection(0)),
            0x38 => Self::ConvertReference(ProcessingMode(0)),
            0x39 => Self::ConvertReference(ProcessingMode(2)),
            0x3a => Self::ProcessCapture(ProcessingMode(2)),
            0x3b => Self::ConfigureReferences,
            0x3c => Self::CombinePlanes,
            0x3d => Self::BuildPyramid,
            0x3e => Self::ResolveReferences,
            0x3f => Self::MergePyramid,
            0x40 => Self::PreparePyramid,
            0x41 => Self::FilterPyramid,
            0x42 => Self::FusePyramids(ProcessingMode(0)),
            0x43 => Self::ApplyAlignment,
            0x44 => Self::AlignReference,
            0x45 => Self::FilterReference,
            0x46 => Self::AppendReference,
            0x47 => Self::MergeReferences,
            0x64 => Self::PrepareOutput(Selection(0), ProcessingMode(1)),
            _ => return None,
        })
    }
}

impl Address32 {
    fn at(self, offset: u32) -> Self { self.offset(ByteOffset(offset)) }
}
unsafe fn load_plane(address: Address32) -> Plane { unsafe { read_volatile(address.0 as *const Plane) } }
unsafe fn store_plane(address: Address32, plane: Plane) { unsafe { write_volatile(address.0 as *mut Plane, plane) }; }
unsafe fn frame(context: StageContext) -> Address32 { unsafe { context.frame() }.field(ByteOffset(0)) }
unsafe fn auxiliary(context: StageContext) -> Address32 { Address32(unsafe { frame(context).at(0x3098).word() }) }
unsafe fn field(context: StageContext, offset: u32) -> u32 { unsafe { frame(context).at(offset).word() } }
unsafe fn bypass(context: StageContext) -> bool { (unsafe { field(context, 0x27cc) }) != 0 }
unsafe fn multi(context: StageContext) -> bool { (unsafe { field(context, 0x4b4) }) == 0x19 || (unsafe { field(context, 0x710) }) & 0x400 != 0 }
unsafe fn paired(context: StageContext) -> bool { (unsafe { multi(context) }) || (unsafe { field(context, 0x4b4) }) == 0x1a }
unsafe fn object(context: StageContext, group: u32, kind: i32) -> Address32 { unsafe { context.object(ObjectGroup(group), ObjectKind(kind)) } }
unsafe fn mapped_object(context: StageContext, group: u32) -> Address32 {
    let group = unsafe { processing_remap_object(context.0, group) };
    unsafe { object(context, group, 0) }
}
unsafe fn complete(context: StageContext, plan: StagePlan, status: u32) -> StageResult { unsafe { plan.complete(context, ServiceCompletion(status)) } }
unsafe fn configuration_levels(context: StageContext) -> u32 {
    let key = if unsafe { multi(context) } { 0x472f } else { 0x493f };
    unsafe { processing_configuration_byte(key) }.wrapping_add(1)
}
unsafe fn output_plane(context: StageContext, selection: Selection) -> Address32 {
    unsafe { frame(context) }.at(if unsafe { paired(context) } && selection.0 == 1 { 0x1a0 } else { 0x184 })
}
unsafe fn linear_input(context: StageContext, plane: Address32) -> u32 {
    (unsafe { field(context, 0x4b4) } != 0x1d && unsafe { plane.at(24).word() } & 0x2000000 == 0) as u32
}
fn four_levels(base: Plane, address: Address32) -> [Plane; 4] {
    let mut planes = [base; 4];
    for index in 1..4 {
        planes[index].address = if index == 1 { address } else if index == 2 { Address32(planes[1].address.0.wrapping_add(planes[1].width)) } else { Address32(planes[2].address.0.wrapping_add(planes[2].height.wrapping_mul(planes[2].stride))) };
        planes[index].width = ((planes[index-1].width >> 1).wrapping_add(15)) & !15;
        planes[index].height = ((planes[index-1].height >> 1).wrapping_add(1)) & !1;
        planes[index].length = planes[index].stride.wrapping_mul(planes[index].height);
    }
    planes
}
fn plane_pointers<const N: usize>(planes: &[Plane; N]) -> [*const Plane; 7] {
    core::array::from_fn(|index| if index < N { &planes[index] as *const Plane } else { core::ptr::null() })
}

#[inline(always)]
unsafe fn expand_seven(context: StageContext, source: &Plane, reference: &Plane, alternate: u32) -> [Plane; 7] {
    let mut planes = core::mem::MaybeUninit::<[Plane; 7]>::uninit();
    unsafe { processing_expand_pyramid(context.0, planes.as_mut_ptr().cast(), source, reference, 7, alternate) };
    unsafe { planes.assume_init() }
}

pub(super) unsafe fn execute(context: StageContext, stage: Stage) -> StageResult {
    unsafe { execute_with_snapshot(context, stage, None) }
}

unsafe fn execute_with_snapshot(context: StageContext, stage: Stage, legacy_snapshot: Option<&core::mem::MaybeUninit<Plane>>) -> StageResult {
    unsafe {
        match stage {
            Stage::ResizeOutput(mode, selection) => return transform_output(context, mode, selection, false),
            Stage::CopyOutput(mode, selection) => return transform_output(context, mode, selection, true),
            Stage::PrepareOutput(selection, mode) => return prepare_output(context, selection, mode),
            Stage::ConvertOutput(selection) => return convert_output(context, selection),
            Stage::ProcessCapture(mode) => return process_capture(context, mode),
            Stage::PrepareCapture(selection, mode) => return prepare_capture(context, selection, mode),
            Stage::FilterCapture(selection) => return filter_capture(context, selection),
            _ => (),
        }
        let operation = match stage {
            Stage::MergeReferences => 0x47, Stage::AppendReference => 0x46, Stage::FilterReference => 0x45,
            Stage::AlignReference => 0x44, Stage::ApplyAlignment => 0x43, Stage::FusePyramids(_) => 0x42,
            Stage::FilterPyramid => 0x41, Stage::PreparePyramid => 0x40, Stage::MergePyramid => 0x3f,
            Stage::ResolveReferences => 0x3e, Stage::BuildPyramid => 0x3d, Stage::CombinePlanes => 0x3c,
            Stage::ConfigureReferences => 0x3b, Stage::ConvertReference(mode) => if mode.0 == 2 { 0x39 } else { 0x38 },
            Stage::FinishOutput(selection) => match selection.0 { 2 => 0x2e, 4 => 0x2f, 1 => 0x30, _ => 0x31 },
            Stage::ApplyOutput => 0x1b, Stage::ApplyOutputPlanes => 0x1a, Stage::PrepareOutputPlanes => 0x19,
            Stage::ResizePreview => 0x1e, Stage::PreparePreview => 0x18,
            _ => unreachable!(),
        };
        let preview_source = if matches!(stage, Stage::PreparePreview) { Some(load_plane(frame(context).at(0x184))) } else { None };
        let plan = StagePlan::develop(OperationCode(operation));
        let skip = match stage {
            Stage::AppendReference | Stage::FilterReference | Stage::AlignReference => field(context, 0x2fb8) == 1 || field(context, 0x2fbc) == 0,
            Stage::ApplyAlignment => field(context, 0x2fb8) == 1,
            Stage::FusePyramids(mode) => mode.0 == 0 && field(context, 0x2fb8) == 1,
            Stage::PreparePyramid => field(context, 0x2fb8) != 1,
            Stage::CombinePlanes => field(context, 0x2fb8) != 1 && field(context, 0x2fbc) == 0,
            Stage::FinishOutput(selection) => framework::xm1_develop_precondition(context.0, selection.0) == 0 || (selection.0 == 4 && field(context, 0x322c) != 2),
            Stage::ApplyOutput => field(context, 0x275c) == 0,
            Stage::ResizePreview => !matches!(field(context, 0x4a8), 4..=6 | 8 | 9),
            Stage::PreparePreview => field(context, 0x578) == 0,
            _ => false,
        };
        if skip { return plan.skip(context); }
        let levels = if matches!(stage, Stage::FusePyramids(_)) { configuration_levels(context) } else { 0 };
        let started = plan.begin(context);
        if started.0 != 0 { return started; }
        let status = match stage {
            Stage::MergeReferences => {
                let mut destination = load_plane(object(context, 0x50, 9));
                let count = auxiliary(context).at(0xcc).word();
                let mut status = 1;
                if count > 1 {
                    let pointers = core::array::from_fn::<_, 3, _>(|index| if (index as u32) < count { auxiliary(context).at(0x78 + index as u32 * 28).0 as *const Plane } else { core::ptr::null() });
                    if !bypass(context) { status = processing_merge_references(frame(context).0, pointers.as_ptr(), count, &mut destination); }
                    store_plane(auxiliary(context).at(0x78), destination);
                }
                status
            }
            Stage::AppendReference => {
                let source_frame = frame(context);
                let parameters = object(context, 0x4f, 0);
                let reference = object(context, 0x50, 9);
                let mut destination = load_plane(object(context, 0x46, 0));
                let count = auxiliary(context).at(0xcc).word();
                let status = if bypass(context) { 1 } else { processing_append_reference(frame(context).0, auxiliary(context).at(0x5c).0, source_frame.at(0x184).0, parameters.0, reference.0, &mut destination, count.wrapping_add(1)) };
                store_plane(auxiliary(context).at(0x5c), destination);
                store_plane(auxiliary(context).at(0x78 + auxiliary(context).at(0xcc).word().wrapping_mul(28)), load_plane(reference));
                auxiliary(context).at(0xcc).write_word(count.wrapping_add(1));
                status
            }
            Stage::FilterReference => {
                let mut destination = load_plane(mapped_object(context, 0x45));
                let status = if bypass(context) { 1 } else { processing_filter_reference(frame(context).0, frame(context).at(0x184).0, &mut destination) };
                store_plane(frame(context).at(0x184), destination); status
            }
            Stage::AlignReference | Stage::ApplyAlignment => {
                let status = if matches!(stage, Stage::AlignReference) {
                    let parameters = mapped_object(context, 0x45);
                    let transform = mapped_object(context, 0x4d).at(4).word();
                    if !bypass(context) { processing_align_reference(frame(context).0, frame(context).at(0x184).0, parameters.0, transform); }
                    if field(context, 0x4b4) == 0x1a && field(context, 0x2fbc) == 1 { let count = auxiliary(context).at(0xcc); count.write_word(count.word().wrapping_add(1)); }
                    1
                } else {
                    let reference = auxiliary(context).at(0x54).word();
                    let filter = mapped_object(context, 0x4b).at(4).word();
                    let transform = mapped_object(context, 0x4d).at(4).word();
                    if bypass(context) { 1 } else { processing_apply_alignment(frame(context).0, frame(context).at(0x184).0, reference, filter, transform, 0) }
                };
                if multi(context) && field(context, 0x2fbc) != 1 { processing_open_object(context.0, 0x46, 0, 1); }
                status
            }
            Stage::FusePyramids(_) => {
                let reference_id = auxiliary(context).at(0x54).word();
                let filter = mapped_object(context, 0x4b).at(4).word();
                let first = load_plane(auxiliary(context));
                let output = load_plane(frame(context).at(0x184));
                let reference = load_plane(auxiliary(context).at(0x38));
                let source = expand_seven(context, &first, &reference, 0);
                let destination = expand_seven(context, &output, &reference, 1);
                let source_pointers = plane_pointers(&source);
                let destination_pointers = plane_pointers(&destination);
                if bypass(context) { 1 } else { processing_fuse_pyramids(frame(context).0, source_pointers.as_ptr(), levels, reference_id, destination_pointers.as_ptr(), levels, filter, 0) }
            }
            Stage::FilterPyramid | Stage::PreparePyramid => {
                let levels = if matches!(stage, Stage::FilterPyramid) { configuration_levels(context) } else { 0 };
                let alternate = field(context, 0x2fb8) != 1;
                if matches!(stage, Stage::FilterPyramid) && !alternate { store_plane(auxiliary(context).at(0x38), load_plane(mapped_object(context, 0x4c))); }
                let first = load_plane(frame(context).at(0x184));
                let reference = load_plane(auxiliary(context).at(0x38));
                let planes = expand_seven(context, &first, &reference, alternate as u32);
                let pointers = plane_pointers(&planes);
                if matches!(stage, Stage::FilterPyramid) {
                    if bypass(context) { 1 } else { processing_filter_pyramid(frame(context).0, pointers.as_ptr(), levels, 0, alternate as u32) }
                } else {
                    let reference_id = mapped_object(context, 0x4a).at(4).word();
                    let status = if bypass(context) { 1 } else { processing_prepare_pyramid(frame(context).0, pointers.as_ptr(), reference_id, 0) };
                    auxiliary(context).at(0x54).write_word(reference_id); status
                }
            }
            Stage::MergePyramid | Stage::BuildPyramid => {
                let mut planes = four_levels(load_plane(auxiliary(context).at(0x5c)), Address32(auxiliary(context).at(0x3c).word()));
                let mut destination = planes[3];
                planes[3].address.0 = planes[3].address.0.wrapping_add(planes[3].width);
                let pointers = plane_pointers(&planes);
                if matches!(stage, Stage::BuildPyramid) {
                    if bypass(context) { 1 } else { processing_build_pyramid(frame(context).0, pointers.as_ptr(), &mut destination, 4) }
                } else {
                    let parameters = object(context, 0x30, 0);
                    destination = load_plane(object(context, 0x39, 0));
                    let status = if bypass(context) { 1 } else { processing_merge_pyramid(frame(context).0, pointers.as_ptr(), 4, frame(context).at(0x184).0, parameters.0, &mut destination) };
                    store_plane(frame(context).at(0x184), destination);
                    let count = auxiliary(context).at(0xcc); count.write_word(count.word().wrapping_add(1)); status
                }
            }
            Stage::ResolveReferences => {
                let count = auxiliary(context).at(0xcc).word().wrapping_add(1);
                let pointers = core::array::from_fn::<_, 3, _>(|index| if (index as u32) < count { auxiliary(context).at(0x78 + index as u32 * 28).0 as *const Plane } else { core::ptr::null() });
                let mut destination = load_plane(object(context, 0x3d, 0));
                let status = if bypass(context) { 1 } else { processing_resolve_references(frame(context).0, pointers.as_ptr(), auxiliary(context).at(0xcc).word().wrapping_add(1), field(context, 0x2fe8), &mut destination) };
                store_plane(frame(context).at(0x184), destination); status
            }
            Stage::CombinePlanes => {
                let mut intermediate = load_plane(object(context, 0x3a, 0));
                let mut destination = load_plane(object(context, 0x3b, 0));
                let status = if bypass(context) { 1 } else { processing_combine_planes(frame(context).0, frame(context).at(0x184).0, &mut intermediate, &mut destination) };
                store_plane(frame(context).at(0x184), destination); status
            }
            Stage::ConfigureReferences => {
                let parameters = object(context, 0x30, 0);
                if bypass(context) { 1 } else { processing_configure_references(frame(context).0, auxiliary(context).at(0xcc).word().wrapping_add(1), field(context, 0x2fe8), parameters.0) }
            }
            Stage::ConvertReference(mode) => {
                let source = if mode.0 == 2 { frame(context).at(0x184) } else { auxiliary(context).at(0x5c) };
                let group = if mode.0 == 2 { if field(context, 0x2fb8) == 1 { 0x33 } else { 0x35 } } else { 0x38 };
                let mut destination = load_plane(object(context, group, 0));
                let status = if bypass(context) { 1 } else { processing_convert_plane(frame(context).0, source.0, &mut destination, 1, 0, 1, 0, 4) };
                if mode.0 == 2 {
                    store_plane(frame(context).at(0x184), destination);
                    if field(context, 0x2fb8) == 1 { store_plane(auxiliary(context), destination); }
                } else { store_plane(auxiliary(context).at(0x5c), destination); store_plane(frame(context).at(0x1a0), destination); }
                status
            }
            Stage::FinishOutput(selection) => {
                let mut request = FinishRequest {
                    prefix: core::mem::MaybeUninit::uninit(),
                    selection,
                    configuration: Address32(0),
                    primary: if selection.0 == 4 { core::mem::MaybeUninit::new(Address32(0)) } else { core::mem::MaybeUninit::uninit() },
                    secondary: if selection.0 == 4 { core::mem::MaybeUninit::new(Address32(0)) } else { core::mem::MaybeUninit::uninit() },
                    reserved: core::mem::MaybeUninit::uninit(),
                };
                if !bypass(context) { processing_prepare_finish(frame(context).0, &mut request); processing_finish_output(frame(context).0, &mut request); }
                1
            }
            Stage::ApplyOutput | Stage::ApplyOutputPlanes | Stage::PrepareOutputPlanes => {
                if matches!(stage, Stage::PrepareOutputPlanes) {
                    for index in 0..5 { let target = frame(context).at(0x210 + index * 28); let plane = load_plane(object(context, 0x51 + index, 0)); store_plane(target, plane); }
                }
                if bypass(context) { 1 } else {
                    let state = frame(context);
                    if matches!(stage, Stage::ApplyOutput) { let parameters = object(context, 0x56, 0); processing_apply_output(state.0, state.at(0x184).0, parameters.0) }
                    else if matches!(stage, Stage::ApplyOutputPlanes) { let parameters = object(context, 0x56, 0); processing_apply_output_planes(state.0, state.at(0x184).0, state.at(0x210).0, state.at(0x22c).0, state.at(0x248).0, state.at(0x264).0, state.at(0x280).0, parameters.0) }
                    else { processing_prepare_output_planes(state.0, state.at(0x184).0, state.at(0x210).0, state.at(0x22c).0, state.at(0x248).0, state.at(0x264).0, state.at(0x280).0) }
                }
            }
            Stage::ResizePreview => {
                let mut destination = load_plane(object(context, 0x1e, 0));
                let mut width = 0; let mut height = 0;
                processing_dimensions(3, field(context, 0x4f0), 0, &mut width, &mut height);
                destination.stride = width.wrapping_mul(2); destination.width = destination.stride; destination.height = height; destination.length = height.wrapping_mul(destination.stride);
                let status = if bypass(context) { 1 } else { processing_resize_preview(frame(context).at(0x3d0).0, &destination) };
                let result = complete(context, plan, status);
                if status == 1 {
                    store_plane(frame(context).at(0x3ec), destination);
                    if let Some(snapshot) = legacy_snapshot {
                        core::ptr::copy_nonoverlapping(snapshot, frame(context).at(0x408).0 as *mut core::mem::MaybeUninit<Plane>, 1);
                    }
                }
                return result;
            }
            Stage::PreparePreview => {
                let mut source = preview_source.unwrap();
                let mut destination = load_plane(object(context, 0x1c, 0));
                destination.stride = processing_width(4, 0).wrapping_mul(2); destination.width = destination.stride; destination.height = processing_height(4, 0); destination.length = destination.height.wrapping_mul(destination.stride);
                let mut intermediate = destination;
                let mut crop = BorderAmounts::default();
                processing_geometry(frame(context).at(0x184).0, &mut intermediate, &mut crop, field(context, 0x644), 1, field(context, 0x4b4), 0, 0);
                if !bypass(context) { let linear = (source.flags & 0x2000000 == 0) as u32; source.flags &= !0x4000000; processing_convert_plane(frame(context).0, &source as *const Plane as u32, &mut intermediate, linear, 0, 1, 0, 2); destination.flags = intermediate.flags; }
                let status = if bypass(context) { 1 } else { processing_fill_border(context.0, &destination, field(context, 0x644), 1, 0) };
                store_plane(frame(context).at(0x1bc), destination); store_plane(frame(context).at(0x3d0), intermediate); status
            }
            _ => unreachable!(),
        };
        complete(context, plan, status)
    }
}

unsafe fn transform_output(context: StageContext, mode: ProcessingMode, selection: Selection, copy: bool) -> StageResult {
    unsafe {
        if (!copy && mode.0 == 2) || (copy && mode.0 == 1) { loop { core::hint::spin_loop(); } }
        if !copy && selection.0 > 2 { return StageResult::SUCCESS; }
        let (operation, source) = if (!copy && mode.0 == 1) || (copy && mode.0 == 2) {
            let operation = if copy { if selection.0 == 1 { 0x28 } else { 0x29 } } else { match selection.0 { 1 => 0x25, 0 => 0x26, _ => 0x27 } };
            let source = if selection.0 == 1 && paired(context) { auxiliary(context).at(0xd0) } else { frame(context).at(if !copy && selection.0 == 2 { 0x1bc } else { 0x1d8 }) };
            (operation, source)
        } else {
            let operation = if copy { match selection.0 { 2 => 0x1f, 1 | 4 => 0x20, _ => 0x21 } } else { match selection.0 { 1 => 0x22, 0 => 0x23, _ => 0x24 } };
            let source = frame(context).at(if paired(context) { if selection.0 == 1 { 0x1a0 } else { 0x184 } } else if selection.0 == 2 { 0x1f4 } else { 0x184 });
            (operation, source)
        };
        let plan = StagePlan::develop(OperationCode(operation));
        if framework::xm1_develop_precondition(context.0, selection.0) == 0 { return plan.skip(context); }
        let matches = processing_output_matches(context.0, source.0, mode.0, selection.0);
        if (copy && matches == 0) || (!copy && matches == 1) || (copy && mode.0 == 2 && field(context, 0x2764) == 0) { return plan.skip(context); }
        let started = plan.begin(context);
        if started.0 != 0 { return started; }
        let parameters = object(context, if field(context, 0x4b4) == 0x1d && selection.0 == 0 { 0x3e } else { 0x22 }, 0);
        let mut request = OutputRequest { mode, selection, reserved: 0 };
        let mut status_out = 0u32;
        let mut storage_flags = 0u32;
        processing_storage_snapshot(1, &mut storage_flags);
        let status = if storage_flags & 1 != 0 || bypass(context) { 1 } else if copy {
            processing_copy_output(frame(context).0, &mut request, source.0, parameters.0, &mut status_out)
        } else {
            processing_resize_output(frame(context).0, source.0, linear_input(context, source), &mut request, parameters.0, &mut status_out, (field(context, 0x4b4) == 0x1d) as u32)
        };
        if storage_flags & 1 == 0 {
            if mode.0 == 0 && status == 1 && selection.0 == 0 && field(context, 0x4b4) == 0x1d && field(context, 0x27bc) == 1 {
                store_plane(frame(context).at(0x184), load_plane(Address32(field(context, 0x3228))));
            }
            if status_out != 0 { processing_trace_end(plan.address().at(24).word()); plan.skip(context); return StageResult(7); }
        }
        complete(context, plan, status)
    }
}

unsafe fn prepare_output(context: StageContext, selection: Selection, mode: ProcessingMode) -> StageResult {
    unsafe {
        let (operation, group, kind, auxiliary_group) = match selection.0 {
            1 => if field(context, 0x4b4) == 0x1d { (0x1c, 0x1b, 0, 0x20) } else { (0x1c, 0x25, 5, 0x20) },
            0 => if mode.0 == 1 { (100, 0x1b, 0, 0x3c) } else { (0x1d, 0x25, 6, 0x20) },
            _ => return StageResult::SUCCESS,
        };
        let plan = StagePlan::develop(OperationCode(operation));
        if processing_special_setting() == 0xa5 || processing_special_setting() == 0xa7 || processing_special_setting() == 0xa9 {
            plan.skip(context); framework::xm1_develop_callback(context.0, plan.address().at(8).word(), 0, 0); return StageResult::SUCCESS;
        }
        if field(context, 0x2764) == 0 || framework::xm1_develop_precondition(context.0, selection.0) == 0 { return plan.skip(context); }
        let started = plan.begin(context);
        if started.0 != 0 { return started; }
        let source = output_plane(context, selection);
        let mut destination = load_plane(object(context, group, kind));
        let mut preview = load_plane(object(context, auxiliary_group, 0));
        let linear = linear_input(context, source);
        let mut intermediate = destination;
        let mut crop = BorderAmounts::default();
        if processing_option_selection() == 0 { processing_geometry(source.0, &mut intermediate, &mut crop, field(context, 0x644), field(context, 0x4f0), field(context, 0x4b4), 1, 0); }
        if !bypass(context) { processing_convert_output(frame(context).0, source.0, &mut intermediate, &mut preview, linear); destination.flags = intermediate.flags; }
        let status = if bypass(context) { 1 } else { processing_fill_border(context.0, &destination, field(context, 0x644), field(context, 0x4f0), 1) };
        let target = if paired(context) && selection.0 == 1 { auxiliary(context).at(0xd0) } else { frame(context).at(0x1d8) };
        store_plane(target, destination); store_plane(frame(context).at(0x3d0), preview);
        complete(context, plan, status)
    }
}

unsafe fn convert_output(context: StageContext, selection: Selection) -> StageResult {
    unsafe {
        let source = output_plane(context, selection);
        let linear = linear_input(context, source);
        let operation = if selection.0 == 1 { 0x16 } else { 0x17 };
        let conversion = if selection.0 == 1 && field(context, 0x4b4) == 0x1d { 2 } else { 0 };
        if (field(context, 0x27ac) == 1 && field(context, 0x600) == 1) || processing_conversion_required(context.0, source.0, selection.0) == 0 { return StagePlan::develop(OperationCode(operation)).skip(context); }
        let plan = StagePlan::develop(OperationCode(operation));
        let started = plan.begin(context); if started.0 != 0 { return started; }
        let address = if paired(context) {
            let group = processing_remap_object(context.0, 0x42);
            object(context, group, if selection.0 == 1 { 5 } else { 6 })
        } else if selection.0 == 1 {
            object(context, if field(context, 0x530) == 5 { 0x2e } else if frame(context).at(0x1682).half() == 1 { 0x1a } else { 0x1b }, 0)
        } else { object(context, 0x1a, 0) };
        let mut destination = load_plane(address);
        let status = if bypass(context) { 1 } else { processing_convert_plane(frame(context).0, source.0, &mut destination, linear, 0, 1, 0, conversion) };
        store_plane(output_plane(context, selection), destination);
        if field(context, 0x578) == 1 { store_plane(frame(context).at(0x1f4), load_plane(frame(context).at(0x184))); }
        if status == 1 && field(context, 0x4b4) == 0x20 && matches!(field(context, 0x530), 3..=5 | 13) { processing_publish_debug(frame(context).0, 0x2a, &destination as *const Plane as u32, 0, 0); }
        complete(context, plan, status)
    }
}

unsafe fn capture_auxiliary(context: StageContext, selection: Selection) -> Address32 {
    unsafe {
        let destination = frame(context).at(if selection.0 == 0 { 0xc0 } else { 0xdc });
        let kind = if selection.0 == 0 { 10 } else { 11 };
        let group = if object(context, 0x14, kind).0 == 0 { 0x2b } else { 0x14 };
        let source = object(context, group, kind);
        store_plane(destination, load_plane(source)); destination
    }
}

unsafe fn process_capture(context: StageContext, mut mode: ProcessingMode) -> StageResult {
    unsafe {
        let plan = StagePlan::develop(OperationCode(if mode.0 == 2 { 0x3a } else { 0x12 }));
        if mode.0 == 0 && field(context, 0x4b4) == 0xf { plan.address().at(20).write_word(0); }
        let started = plan.begin(context); if started.0 != 0 { return started; }
        let capture_mode = field(context, 0x4b4);
        let group = if multi(context) && capture_mode != 0x1a {
            if field(context, 0x2fb8) == 1 { 0x43 } else { 0x44 }
        } else if capture_mode == 0x1a {
            if mode.0 == 2 { if field(context, 0x2fb8) == 1 { 0x32 } else { 0x34 } } else { 0x37 }
        } else { 0x19 };
        let parameters = object(context, if capture_mode == 0x1d { 0x27 } else { 0x28 }, 0);
        let second = if !paired(context) && capture_mode != 0x1d && field(context, 0xa8) != 0 { frame(context).at(0xa4).0 } else { 0 };
        let mut destination = load_plane(object(context, group, 0));
        let capture_aux = if frame(context).at(0x19b4).byte() != 0 && field(context, 0x578) != 0 { capture_auxiliary(context, Selection(0)).0 } else { 0 };
        if Address32(0x15b0d80 + 0x41fc).word() == 11 { mode = ProcessingMode(1); }
        let state = frame(context);
        let status = if bypass(context) { 1 } else { processing_process_capture(state.0, state.at(0x88).0, second, capture_aux, &mut destination, 0, parameters.0, 0, 1, 0, mode.0) };
        let capture_mode = field(context, 0x4b4);
        if multi(context) {
            if field(context, 0x2fb8) == 1 {
                let reference = object(context, 0x4e, 0).at(4).word(); frame(context).at(0x3098).write_word(reference);
                store_plane(Address32(reference), destination); store_plane(auxiliary(context).at(0x5c), destination);
            }
            store_plane(frame(context).at(0x184), destination);
        } else if capture_mode == 0x1a {
            if mode.0 == 2 {
                if field(context, 0x2fb8) == 1 { let reference = object(context, 0x3e, 0).at(4).word(); frame(context).at(0x3098).write_word(reference); store_plane(Address32(reference), destination); }
                store_plane(frame(context).at(0x184), destination);
            } else { store_plane(auxiliary(context).at(0x5c), destination); }
        } else {
            if capture_mode == 0x1d {
                destination.address.0 = destination.address.0.wrapping_add((destination.width.wrapping_sub(0x2220)) >> 1).wrapping_add(destination.stride.wrapping_mul((destination.height.wrapping_sub(0xccc)) >> 1));
                destination.width = 0x2220; destination.height = 0xccc; destination.length = 0x1b4b180;
            }
            store_plane(frame(context).at(0x184), destination);
        }
        let result = complete(context, plan, status);
        if status == 1 {
            processing_store_object(frame(context).at(0x7c).0, group, 0, &destination);
            if mode.0 == 0 {
                if frame(context).at(0x6c2).half() != 0 { store_plane(frame(context).at(0xa4), destination); }
                if field(context, 0x578) != 0 { store_plane(frame(context).at(0x1f4), destination); }
                let state = frame(context);
                let source = object(context, group, 0);
                processing_publish_debug(state.0, 0x18, source.0, 0, 0);
            }
        }
        result
    }
}


unsafe fn prepare_capture(context: StageContext, selection: Selection, mode: ProcessingMode) -> StageResult {
    unsafe {
        if selection.0 > 1 { return StageResult::SUCCESS; }
        let source = frame(context).at(if selection.0 == 0 { 0x88 } else { 0xa4 });
        let (group, kind) = if selection.0 == 0 { (0x29, 1) } else if field(context, 0x63c) == 2 { (0x2a, 0) } else { (0x29, 2) };
        let plan = StagePlan::develop(OperationCode(0x10 + selection.0));
        if frame(context).at(0x19b4).byte() != 0 || (selection.0 == 0 && field(context, 0x63c) == 2) { return plan.skip(context); }
        let started = plan.begin(context); if started.0 != 0 { return started; }
        let destination = object(context, group, kind);
        let auxiliary = if field(context, 0x578) != 0 && field(context, 0x63c) == 0 { capture_auxiliary(context, selection).0 } else { 0 };
        let parameters = object(context, if field(context, 0x4b4) == 0x1d { 0x27 } else { 0x28 }, 0);
        let status = if bypass(context) { 1 } else { processing_prepare_capture(frame(context).0, source.0, destination.0, auxiliary, parameters.0, selection.0, mode.0) };
        let result = complete(context, plan, status);
        if status == 1 { processing_publish_debug(frame(context).0, 0x12 + selection.0, source.0, 0, 0); }
        result
    }
}

unsafe fn filter_capture(context: StageContext, selection: Selection) -> StageResult {
    unsafe {
        if selection.0 > 1 { return StageResult::SUCCESS; }
        let source = frame(context).at(if selection.0 == 0 { 0x88 } else { 0xa4 });
        let plan = StagePlan::develop(OperationCode(0xe + selection.0));
        if processing_capture_active(frame(context).0, selection.0) == 0 || (selection.0 == 1 && frame(context).at(0x19b4).byte() != 0) { return plan.skip(context); }
        let started = plan.begin(context); if started.0 != 0 { return started; }
        let status = if bypass(context) { 1 } else { processing_filter_capture(frame(context).0, source.0, selection.0) };
        let result = complete(context, plan, status);
        if status == 1 { store_plane(frame(context).at(if selection.0 == 0 { 0xc0 } else { 0xdc }), load_plane(source)); }
        result
    }
}

macro_rules! entry {
    ($name:ident, $stage:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(context: *mut u8) -> u32 {
            unsafe { execute(StageContext(context), $stage) }.0
        }
    };
}
entry!(xm1_develop_pipeline_merge_references, Stage::MergeReferences);
entry!(xm1_develop_pipeline_append_reference, Stage::AppendReference);
entry!(xm1_develop_pipeline_filter_reference, Stage::FilterReference);
entry!(xm1_develop_pipeline_align_reference, Stage::AlignReference);
entry!(xm1_develop_pipeline_apply_alignment, Stage::ApplyAlignment);
entry!(xm1_develop_pipeline_filter_pyramid, Stage::FilterPyramid);
entry!(xm1_develop_pipeline_prepare_pyramid, Stage::PreparePyramid);
entry!(xm1_develop_pipeline_merge_pyramid, Stage::MergePyramid);
entry!(xm1_develop_pipeline_resolve_references, Stage::ResolveReferences);
entry!(xm1_develop_pipeline_build_pyramid, Stage::BuildPyramid);
entry!(xm1_develop_pipeline_combine_planes, Stage::CombinePlanes);
entry!(xm1_develop_pipeline_configure_references, Stage::ConfigureReferences);
entry!(xm1_develop_pipeline_apply_output, Stage::ApplyOutput);
entry!(xm1_develop_pipeline_apply_output_planes, Stage::ApplyOutputPlanes);
entry!(xm1_develop_pipeline_prepare_output_planes, Stage::PrepareOutputPlanes);
entry!(xm1_develop_pipeline_prepare_preview, Stage::PreparePreview);

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_fuse_pyramids(context: *mut u8, mode: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::FusePyramids(ProcessingMode(mode))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_convert_reference(context: *mut u8, mode: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::ConvertReference(ProcessingMode(mode))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_finish_output(context: *mut u8, selection: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::FinishOutput(Selection(selection))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_resize_output(context: *mut u8, mode: u32, selection: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::ResizeOutput(ProcessingMode(mode), Selection(selection))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_copy_output(context: *mut u8, mode: u32, selection: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::CopyOutput(ProcessingMode(mode), Selection(selection))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_prepare_output(context: *mut u8, selection: u32, mode: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::PrepareOutput(Selection(selection), ProcessingMode(mode))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_convert_output(context: *mut u8, selection: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::ConvertOutput(Selection(selection))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_process_capture(context: *mut u8, mode: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::ProcessCapture(ProcessingMode(mode))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_prepare_capture(context: *mut u8, selection: u32, mode: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::PrepareCapture(Selection(selection), ProcessingMode(mode))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_filter_capture(context: *mut u8, selection: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::FilterCapture(Selection(selection))) }.0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_develop_pipeline_resize_preview_snapshot(context: *mut u8, scratch: *const core::mem::MaybeUninit<Plane>) -> u32 {
    unsafe { execute_with_snapshot(StageContext(context), Stage::ResizePreview, Some(&*scratch)) }.0
}

unsafe extern "C" {
    #[link_name = "xm1_develop_remap_object"]
    fn processing_remap_object(context: *mut u8, group: u32) -> u32;
    #[link_name = "xm1_develop_expand_planes"]
    fn processing_expand_pyramid(context: *mut u8, destination: *mut Plane, source: *const Plane, reference: *const Plane, count: u8, alternate: u32);
    #[link_name = "xm1_develop_fill_border"]
    fn processing_fill_border(context: *mut u8, plane: *const Plane, lookup: u32, selector: u32, mode: u32) -> u32;
    #[link_name = "xm1_develop_output_matches"]
    fn processing_output_matches(context: *mut u8, plane: u32, mode: u32, selection: u32) -> u32;
    #[link_name = "xm1_develop_conversion_required"]
    fn processing_conversion_required(context: *mut u8, plane: u32, selection: u32) -> u32;
    #[link_name = "xm1_develop_open_object"]
    fn processing_open_object(context: *mut u8, group: u32, kind: u32, mode: u32) -> u32;
    #[link_name = "xm1_develop_geometry"]
    fn processing_geometry(source: u32, destination: *mut Plane, crop: *mut BorderAmounts, lookup: u32, selector: u32, capture_mode: u32, mode: u32, flags: u32);
    #[link_name = "xm1_develop_dimensions"]
    fn processing_dimensions(lookup: u32, selector: u32, mode: u32, width: *mut u32, height: *mut u32);
    #[link_name = "xm1_buffers_configuration_byte"]
    fn processing_configuration_byte(key: u32) -> u32;
    #[link_name = "xm1_develop_lookup_width"]
    fn processing_width(lookup: u32, selector: u32) -> u32;
    #[link_name = "xm1_develop_lookup_height"]
    fn processing_height(lookup: u32, selector: u32) -> u32;
    #[link_name = "xm1_develop_loadtest_setting"]
    fn processing_special_setting() -> u32;
    #[link_name = "xm1_develop_capture_option_selection"]
    fn processing_option_selection() -> u32;
    fn processing_storage_snapshot(kind: u32, destination: *mut u32);
    #[link_name = "xm1_pipeline_image_trace_end"]
    fn processing_trace_end(operation: u32);
    #[link_name = "xm1_develop_store_object"]
    fn processing_store_object(objects: u32, group: u32, kind: u32, plane: *const Plane);
    #[link_name = "xm1_pipeline_image_publish_debug_file"]
    fn processing_publish_debug(frame: u32, kind: u32, plane: u32, argument: u32, flags: u32);
    #[link_name = "xm1_pipeline_image_blend_reference"]
    fn processing_merge_references(frame: u32, planes: *const *const Plane, count: u32, destination: *mut Plane) -> u32;
    #[link_name = "xm1_pipeline_image_noise_pass"]
    fn processing_append_reference(frame: u32, source: u32, reference: u32, parameters: u32, filtered: u32, destination: *mut Plane, pass: u32) -> u32;
    #[link_name = "xm1_pipeline_image_prepare_affine"]
    fn processing_filter_reference(frame: u32, source: u32, destination: *mut Plane) -> u32;
    #[link_name = "xm1_pipeline_image_validate_affine"]
    fn processing_align_reference(frame: u32, source: u32, parameters: u32, transform: u32) -> u32;
    #[link_name = "xm1_pipeline_image_estimate_affine"]
    fn processing_apply_alignment(frame: u32, source: u32, previous: u32, current: u32, reference: u32, flags: u32) -> u32;
    #[link_name = "xm1_pipeline_image_motion_secondary"]
    fn processing_fuse_pyramids(frame: u32, sources: *const *const Plane, source_count: u32, reference: u32, destinations: *const *const Plane, destination_count: u32, filter: u32, flags: u32) -> u32;
    #[link_name = "xm1_pipeline_image_reduce_planes"]
    fn processing_filter_pyramid(frame: u32, planes: *const *const Plane, count: u32, argument: u32, alternate: u32) -> u32;
    #[link_name = "xm1_pipeline_image_motion_primary"]
    fn processing_prepare_pyramid(frame: u32, planes: *const *const Plane, reference: u32, flags: u32) -> u32;
    #[link_name = "xm1_pipeline_image_lut_reference"]
    fn processing_merge_pyramid(frame: u32, planes: *const *const Plane, count: u32, mask: u32, parameters: u32, destination: *mut Plane) -> u32;
    #[link_name = "xm1_pipeline_image_reference_filter"]
    fn processing_resolve_references(frame: u32, planes: *const *const Plane, count: u32, argument: u32, destination: *mut Plane) -> u32;
    #[link_name = "xm1_pipeline_image_transform_planes"]
    fn processing_build_pyramid(frame: u32, planes: *const *const Plane, destination: *mut Plane, count: u32) -> u32;
    #[link_name = "xm1_pipeline_image_prepare_border"]
    fn processing_combine_planes(frame: u32, source: u32, intermediate: *mut Plane, destination: *mut Plane) -> u32;
    #[link_name = "xm1_pipeline_image_reference_curve"]
    fn processing_configure_references(frame: u32, count: u32, argument: u32, parameters: u32) -> u32;
    #[link_name = "xm1_pipeline_image_resize_capture"]
    fn processing_convert_plane(frame: u32, source: u32, destination: *mut Plane, linear: u32, argument: u32, enabled: u32, flags: u32, mode: u32) -> u32;
    #[link_name = "xm1_pipeline_image_configure_output"]
    fn processing_prepare_finish(frame: u32, request: *mut FinishRequest);
    #[link_name = "xm1_pipeline_image_prepare_output_stage"]
    fn processing_finish_output(frame: u32, request: *mut FinishRequest);
    #[link_name = "xm1_pipeline_image_finish_copy"]
    fn processing_apply_output(frame: u32, plane: u32, parameters: u32) -> u32;
    #[link_name = "xm1_pipeline_image_start_work"]
    fn processing_apply_output_planes(frame: u32, source: u32, first: u32, second: u32, third: u32, fourth: u32, fifth: u32, parameters: u32) -> u32;
    #[link_name = "xm1_pipeline_image_prepare_geometry"]
    fn processing_prepare_output_planes(frame: u32, source: u32, first: u32, second: u32, third: u32, fourth: u32, fifth: u32) -> u32;
    #[link_name = "xm1_pipeline_image_resize_software"]
    fn processing_resize_preview(source: u32, destination: *const Plane) -> u32;
    #[link_name = "xm1_pipeline_image_prepare_triplets"]
    fn processing_resize_output(frame: u32, source: u32, linear: u32, request: *mut OutputRequest, parameters: u32, status: *mut u32, special: u32) -> u32;
    #[link_name = "xm1_pipeline_image_tune_triplets"]
    fn processing_copy_output(frame: u32, request: *mut OutputRequest, source: u32, parameters: u32, status: *mut u32) -> u32;
    #[link_name = "xm1_pipeline_image_resize_through_crop"]
    fn processing_convert_output(frame: u32, source: u32, intermediate: *mut Plane, preview: *mut Plane, linear: u32) -> u32;
    #[link_name = "xm1_pipeline_image_prepare_output_planes"]
    fn processing_process_capture(frame: u32, source: u32, second: u32, auxiliary: u32, destination: *mut Plane, argument: u32, parameters: u32, flags: u32, enabled: u32, reserved: u32, mode: u32) -> u32;
    #[link_name = "xm1_pipeline_image_prepare_output_mask"]
    fn processing_prepare_capture(frame: u32, source: u32, destination: u32, auxiliary: u32, parameters: u32, selection: u32, mode: u32) -> u32;
    #[link_name = "xm1_pipeline_image_stage_inactive"]
    fn processing_capture_active(frame: u32, selection: u32) -> u32;
    #[link_name = "xm1_pipeline_image_configure_planes"]
    fn processing_filter_capture(frame: u32, source: u32, selection: u32) -> u32;
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_loadtest_current() -> u32 {
    unsafe { Address32(0x009134f8).word() }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_loadtest_setting() -> u32 {
    unsafe { processing_configuration_byte(0x80) }
}

#[repr(u32)]
enum CaptureOption { Normal = 0, First = 1, Second = 2, Third = 3 }

unsafe fn capture_option() -> CaptureOption {
    unsafe {
        if xm1_develop_loadtest_current() != 0 && xm1_develop_loadtest_setting() == 0xa5 { return CaptureOption::First; }
        if xm1_develop_loadtest_current() != 0 && xm1_develop_loadtest_setting() == 0xa7 { return CaptureOption::Second; }
        if xm1_develop_loadtest_current() != 0 && xm1_develop_loadtest_setting() == 0xa9 { CaptureOption::Third } else { CaptureOption::Normal }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_develop_capture_option_selection() -> u32 {
    (unsafe { capture_option() }) as u32
}
