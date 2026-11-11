use super::shared::{Address32, ByteOffset, ObjectGroup, ObjectKind, OperationCode, ServiceCompletion, StageContext, StagePlan, StageResult};
use crate::develop::framework;

#[derive(Clone, Copy)]
pub(super) enum Stage {
    FinishPlanes, BlendPlanes, DifferencePlanes, MergePlanes, PreparePlanes,
    SelectComposite, PrepareComposite, SplitComposite, FillChroma(ChromaPass),
    BlendCaptures, CropCapture, SplitCapture, RotateCapture(CapturePass),
    ReducePreview(CapturePass), TransformBorder(BorderPass), ReduceBorder, CombineBorder,
}
#[derive(Clone, Copy)]
pub(super) enum ChromaPass { Primary, Secondary }
#[derive(Clone, Copy)]
pub(super) enum CapturePass { Primary, Secondary, Invalid }
#[derive(Clone, Copy)]
pub(super) enum BorderPass { Previous, Current }

impl Stage {
    pub(super) fn from_command(command: OperationCode) -> Option<Self> {
        Some(match command.0 {
            0x48 => Self::CombineBorder,
            0x49 => Self::ReduceBorder,
            0x4a => Self::TransformBorder(BorderPass::Previous),
            0x4b => Self::TransformBorder(BorderPass::Current),
            0x50 => Self::ReducePreview(CapturePass::Primary),
            0x51 => Self::ReducePreview(CapturePass::Secondary),
            0x52 => Self::RotateCapture(CapturePass::Primary),
            0x53 => Self::RotateCapture(CapturePass::Secondary),
            0x54 => Self::CropCapture,
            0x63 => Self::SplitCapture,
            0x65 => Self::BlendCaptures,
            0x66 => Self::FillChroma(ChromaPass::Primary),
            0x67 => Self::FillChroma(ChromaPass::Secondary),
            0x68 => Self::SplitComposite,
            0x69 => Self::PrepareComposite,
            0x6a => Self::SelectComposite,
            0x6b => Self::PreparePlanes,
            0x6c => Self::MergePlanes,
            0x6d => Self::DifferencePlanes,
            0x6e => Self::BlendPlanes,
            0x6f => Self::FinishPlanes,
            _ => return None,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Image { format: u32, address: u32, length: u32, stride: u32, width: u32, height: u32, flags: u32 }
impl Image {
    unsafe fn load(address: Address32) -> Self { unsafe { core::ptr::read_volatile(address.0 as *const Self) } }
    unsafe fn store(self, address: Address32) { unsafe { core::ptr::write_volatile(address.0 as *mut Self, self) }; }
    fn area(&mut self) { self.length = self.width.wrapping_mul(self.height); }
}
#[derive(Clone, Copy)]
struct Frame(Address32);
impl Frame {
    fn at(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
    unsafe fn word(self, offset: u32) -> u32 { unsafe { self.at(offset).word() } }
    unsafe fn image(self, offset: u32) -> Image { unsafe { Image::load(self.at(offset)) } }
    unsafe fn bypass(self) -> bool { unsafe { self.word(0x27cc) != 0 } }
}
unsafe fn frame(context: StageContext) -> Frame { Frame(unsafe { context.frame() }.0) }
unsafe fn object(context: StageContext, group: u32, kind: i32) -> Image {
    unsafe { Image::load(context.object(ObjectGroup(group), ObjectKind(kind))) }
}
unsafe fn install(context: StageContext, group: u32, offset: u32) {
    let target = unsafe { frame(context) }.at(offset);
    unsafe { object(context, group, 0).store(target) };
}
unsafe fn debug(context: StageContext, kind: u32, offset: u32, number: u32) {
    let f = unsafe { frame(context) };
    unsafe { services::publish(f.0, kind, f.at(offset), number, 0) };
}
unsafe fn config_pair(high: u32, low: u32) -> u32 {
    let hi = unsafe { services::configuration(ConfigurationKey(high)) };
    let lo = unsafe { services::configuration(ConfigurationKey(low)) };
    lo | hi.wrapping_shl(8)
}
#[repr(transparent)]
struct ConfigurationKey(u32);
fn signed_div_pow2(value: u32, bits: u32) -> u32 { ((value as i32) / (1i32 << bits)) as u32 }
unsafe fn invalid_pass(context: StageContext) -> StageResult {
    let f = unsafe { frame(context) };
    let index = unsafe { context.address().offset(ByteOffset(0x34)).word() }.wrapping_sub(11);
    unsafe { framework::xm1_develop_store(context.0, f.word(index.wrapping_mul(4))) };
    StageResult(3)
}
unsafe fn store_operation(context: StageContext, operation: u32) -> StageResult {
    unsafe { framework::xm1_develop_store(context.0, operation) };
    StageResult::SUCCESS
}

pub(super) unsafe fn execute(context: StageContext, stage: Stage) -> StageResult {
    let f = unsafe { frame(context) };
    let mode = unsafe { f.word(0x530) };
    let operation = match stage {
        Stage::FinishPlanes => 0x6f, Stage::BlendPlanes => 0x6e, Stage::DifferencePlanes => 0x6d,
        Stage::MergePlanes => 0x6c, Stage::PreparePlanes => 0x6b, Stage::SelectComposite => 0x6a,
        Stage::PrepareComposite => 0x69, Stage::SplitComposite => 0x68,
        Stage::FillChroma(ChromaPass::Primary) => 0x66, Stage::FillChroma(ChromaPass::Secondary) => 0x67,
        Stage::BlendCaptures => 0x65, Stage::CropCapture => 0x54, Stage::SplitCapture => 0x63,
        Stage::RotateCapture(CapturePass::Primary) => 0x52,
        Stage::RotateCapture(CapturePass::Secondary) => {
            if unsafe { f.word(0x3218) } == 0 || unsafe { services::orientation(f.word(0x1604), f.word(0x5d0)) } != 3 {
                return unsafe { store_operation(context, 0xa7) };
            }
            0x53
        }
        Stage::ReducePreview(CapturePass::Primary) => 0x50,
        Stage::ReducePreview(CapturePass::Secondary) => {
            if unsafe { f.word(0x2784) } == 1 && unsafe { f.word(0x2788) } == 0 {
                return unsafe { store_operation(context, 0xa3) };
            }
            0x51
        }
        Stage::RotateCapture(CapturePass::Invalid) | Stage::ReducePreview(CapturePass::Invalid) => return unsafe { invalid_pass(context) },
        Stage::TransformBorder(BorderPass::Previous) => 0x4a, Stage::TransformBorder(BorderPass::Current) => 0x4b,
        Stage::ReduceBorder => 0x49, Stage::CombineBorder => 0x48,
    };
    let plan = unsafe { StagePlan::develop(OperationCode(operation)) };
    let enabled = match stage {
        Stage::FinishPlanes | Stage::BlendPlanes | Stage::DifferencePlanes | Stage::MergePlanes | Stage::PreparePlanes => mode == 5,
        Stage::SelectComposite => matches!(mode, 3 | 4 | 5 | 13),
        Stage::PrepareComposite => matches!(mode, 3 | 4), Stage::SplitComposite => matches!(mode, 4 | 5 | 13),
        Stage::FillChroma(_) => matches!(mode, 3 | 5),
        Stage::BlendCaptures => unsafe { f.word(0x4ac) == 1 && f.at(0x1682).half() != 1 },
        Stage::CropCapture => unsafe { f.word(0x2794) != 0 && (f.word(0x2794) != 2 || f.at(0x3194).byte() == 16) },
        Stage::SplitCapture => {
            if unsafe { f.word(0x5cc) } != 4 { false } else {
                let flags = Frame(Address32(unsafe { f.word(0x3228) }));
                unsafe { flags.word(0x110) != 1 && flags.word(0x114) != 1 && flags.word(0x10c) != 1 }
            }
        }
        _ => true,
    };
    if !enabled { return unsafe { plan.skip(context) }; }
    let begun = unsafe { plan.begin(context) };
    if begun != StageResult::SUCCESS { return begun; }
    let completion = unsafe { process(context, stage) };
    unsafe { plan.complete(context, ServiceCompletion(completion)) }
}

unsafe fn process(context: StageContext, stage: Stage) -> u32 {
    let f = unsafe { frame(context) };
    match stage {
        Stage::FinishPlanes => {
            if unsafe { f.bypass() } { return 1; }
            let first = unsafe { services::finish_primary(f.0, f.at(0x210), f.at(0x280), f.at(0x210)) };
            unsafe { debug(context, 0x32, 0x210, 0) };
            if first != 1 { return first; }
            let f = unsafe { frame(context) };
            let second = unsafe { services::finish_secondary(f.0, f.at(0x22c), f.at(0x248), f.at(0x22c)) };
            unsafe { debug(context, 0x31, 0x22c, 0) };
            second
        }
        Stage::BlendPlanes => {
            unsafe { install(context, 0x31, 0x264) };
            let f = unsafe { frame(context) };
            if unsafe { f.bypass() } { return 1; }
            let status = unsafe { services::blend_planes(f.0, f.at(0x248), f.at(0x22c), f.at(0x280), f.at(0x264)) };
            unsafe { debug(context, 0x2e, 0x264, 0) };
            if status != 1 { return status; }
            let f = unsafe { frame(context) };
            let status = unsafe { services::blend_secondary(f.0, f.at(0x22c), f.at(0x280), f.at(0x22c)) };
            unsafe { debug(context, 0x2f, 0x22c, 0) };
            if status != 1 { return status; }
            let f = unsafe { frame(context) };
            let status = unsafe { services::blend_finish(f.0, f.at(0x22c), f.at(0x264), f.at(0x248), f.at(0x2f0), f.at(0x22c)) };
            unsafe { debug(context, 0x30, 0x22c, 0) };
            status
        }
        Stage::DifferencePlanes => {
            unsafe { install(context, 0x30, 0x248) };
            let f = unsafe { frame(context) };
            if unsafe { f.bypass() } { return 1; }
            let status = unsafe { services::difference(f.0, f.at(0x210), f.at(0x22c), f.at(0x248), 0) };
            unsafe { debug(context, 0x2d, 0x248, 1) };
            if status != 1 { return status; }
            let f = unsafe { frame(context) };
            let status = unsafe { services::difference(f.0, f.at(0x22c), f.at(0x210), f.at(0x22c), 1) };
            unsafe { debug(context, 0x2d, 0x22c, 2) };
            status
        }
        Stage::MergePlanes => {
            unsafe { install(context, 0x2f, 0x22c) };
            let planes = [Address32(0x15b0cd8), Address32(0x15b0cf4), Address32(0x15b0d10), Address32(0x15b0d2c)];
            let f = unsafe { frame(context) };
            if unsafe { f.bypass() } { return 1; }
            let status = unsafe { services::merge(f.0, planes.as_ptr(), 4, f.at(0x22c)) };
            unsafe { debug(context, 0x2c, 0x22c, 0) };
            if status != 1 { return status; }
            unsafe { install(context, 0x34, 0x2f0) };
            let f = unsafe { frame(context) };
            let status = unsafe { services::merge_finish(f.0, f.at(0x22c), f.at(0x2f0)) };
            unsafe { debug(context, 0x2b, 0x2f0, 0) };
            status
        }
        Stage::PreparePlanes => {
            unsafe { f.image(0x184).store(f.at(0x210)); install(context, 0x33, 0x2b8) };
            let f = unsafe { frame(context) };
            if unsafe { f.bypass() } { return 1; }
            unsafe { services::prepare(f.0, f.at(0x2b8)); install(context, 0x31, 0x264) };
            let mut scratch = unsafe { object(context, 0x35, 0) };
            let f = unsafe { frame(context) };
            unsafe { services::prepare_finish(f.0, f.at(0x2b8), &mut scratch, f.at(0x264)) }
        }
        Stage::SelectComposite => unsafe { select_composite(context) },
        Stage::PrepareComposite => {
            unsafe { install(context, 0x2c, 0x248) };
            let f = unsafe { frame(context) };
            if !unsafe { f.bypass() } {
                match unsafe { f.word(0x530) } {
                    3 => unsafe { services::prepare_three(f.0, f.at(0x248)) },
                    4 => unsafe { services::prepare_four(f.0, f.at(0x248)) }, _ => (),
                }
            }
            unsafe { debug(context, 0x2b, 0x248, 0) };
            1
        }
        Stage::SplitComposite => {
            let mode = unsafe { f.word(0x530) };
            let count = if mode == 5 { 4 } else { 6 };
            let destination = unsafe { object(context, 0x36, 0) };
            unsafe { services::split_descriptors(context.0, Address32(0x15b0cd8), f.at(if mode == 5 { 0x264 } else { 0x184 }), &destination, count as u8) };
            let planes = plane_addresses();
            let f = unsafe { frame(context) };
            match unsafe { f.word(0x530) } {
                4 => unsafe { services::split_four(f.0, planes.as_ptr(), count) },
                5 => unsafe { services::split_five(f.0, planes.as_ptr(), count) },
                13 => unsafe { services::split_thirteen(f.0, planes.as_ptr(), count) }, _ => (),
            }
            1
        }
        Stage::FillChroma(pass) => {
            let (offset, first, second) = match (unsafe { f.word(0x530) }, pass) {
                (3, _) => (0x22c, 0, 128), (_, ChromaPass::Primary) => (0x280, 255, 0), _ => (0x248, 0, 255),
            };
            let mut image = unsafe { f.image(offset) };
            if !unsafe { frame(context).bypass() } { unsafe { services::fill_chroma(frame(context).0, first, second, &mut image) }; }
            1
        }
        Stage::BlendCaptures => {
            let mut sources = [Address32(unsafe { context.address().offset(ByteOffset(20)).word() }.wrapping_add(0x184)), f.at(0x184)];
            let mut output = unsafe { object(context, 0x1b, 0) };
            let f = unsafe { frame(context) };
            let status = if unsafe { f.bypass() } { 1 } else {
                let status = unsafe { services::blend_captures(f.0, sources.as_mut_ptr(), context.address().offset(ByteOffset(24)).half() as u32, &mut output) };
                if unsafe { sources[0].offset(ByteOffset(24)).word() & 0x2000000 != 0 && sources[1].offset(ByteOffset(24)).word() & 0x2000000 != 0 } { output.flags |= 0x2000000; }
                status
            };
            unsafe { output.store(frame(context).at(0x184)) };
            status
        }
        Stage::CropCapture => unsafe { crop_capture(context) },
        Stage::SplitCapture => unsafe { split_capture(context) },
        Stage::RotateCapture(pass) => unsafe { rotate_capture(context, pass) },
        Stage::ReducePreview(pass) => unsafe { reduce_preview(context, pass) },
        Stage::TransformBorder(pass) => {
            let previous = matches!(pass, BorderPass::Previous);
            let mut source = unsafe { Image::load(if previous { Address32(f.word(0x3098)) } else { f.at(0x184) }) };
            let mut target = unsafe { object(context, 0x49, if previous { 5 } else { 6 }) };
            let f = unsafe { frame(context) };
            let count = unsafe { Address32(f.word(0x3098)).offset(ByteOffset(0xcc)).word() };
            if count > 1 { unsafe { f.at(0x1d30).write_word(count) }; }
            let f = unsafe { frame(context) };
            let status = if unsafe { f.bypass() } { 1 } else { unsafe { services::transform_border(f.0, &mut source, &mut target, 0, (!previous) as u32) } };
            unsafe { source.store(frame(context).at(if previous { 0x1a0 } else { 0x184 })) };
            status
        }
        Stage::ReduceBorder => {
            let mut source = unsafe { f.image(0x184) };
            let mut target = unsafe { object(context, 0x47, 0) };
            let f = unsafe { frame(context) };
            let status = if unsafe { f.bypass() } { 1 } else { unsafe { services::reduce_border(f.0, &mut source, &mut target) } };
            unsafe { source.store(frame(context).at(0x184)) };
            status
        }
        Stage::CombineBorder => {
            let saved = Address32(unsafe { f.word(0x3098) });
            let mut output = unsafe { object(context, 0x48, 0) };
            let f = unsafe { frame(context) };
            let status = if unsafe { f.bypass() } { 1 } else { unsafe { services::combine_border(f.0, Address32(f.word(0x3098)).offset(ByteOffset(0x5c)), f.at(0x184), saved.offset(ByteOffset(0x78)), &mut output) } };
            unsafe { output.store(frame(context).at(0x184)) };
            let count = Address32(unsafe { frame(context).word(0x3098) }).offset(ByteOffset(0xcc));
            unsafe { count.write_word(count.word().wrapping_add(1)) };
            status
        }
    }
}
fn plane_addresses() -> [Address32; 6] {
    [Address32(0x15b0cd8), Address32(0x15b0cf4), Address32(0x15b0d10), Address32(0x15b0d2c), Address32(0x15b0d48), Address32(0x15b0d64)]
}
unsafe fn select_composite(context: StageContext) -> u32 {
    let f = unsafe { frame(context) };
    let mut status = 1;
    if !unsafe { f.bypass() } {
        let mode = unsafe { f.word(0x530) };
        let offset = match mode {
            3 => { status = unsafe { services::select_three(f.0, f.at(0x184), f.at(0x22c), f.at(0x248), f.at(0x22c)) }; 0x22c }
            5 => { status = unsafe { services::select_five(f.0, f.at(0x22c), f.at(0x210), f.at(0x210)) }; 0x210 }
            4 | 13 => {
                let planes = plane_addresses();
                let mut tail = unsafe { Image::load(planes[2]) };
                tail.address = tail.address.wrapping_add(tail.height.wrapping_mul(tail.stride));
                unsafe { install(context, 0x2d, 0x22c) };
                let f = unsafe { frame(context) };
                status = if mode == 4 { unsafe { services::select_four(f.0, planes.as_ptr(), 6, f.at(0x248), &mut tail, f.at(0x22c)) } } else { unsafe { services::select_thirteen(f.0, planes.as_ptr(), 6, &mut tail, f.at(0x22c)) } };
                0x22c
            }
            _ => 0,
        };
        if offset != 0 {
            let f = unsafe { frame(context) };
            unsafe { f.image(offset).store(f.at(0x184)) };
            let f = unsafe { frame(context) };
            unsafe { f.at(0x19c).write_word(f.word(0x19c) | 0x2000000) };
        }
    }
    unsafe { debug(context, 0x18, 0x184, 0) };
    status
}
unsafe fn crop_capture(context: StageContext) -> u32 {
    let mut source = unsafe { frame(context).image(0x184) };
    let mut target = unsafe { object(context, 0x28, 0) };
    let f = unsafe { frame(context) };
    let shift = unsafe { f.word(0x3198) } as i32;
    let positive = if shift < 0 { 0 } else { shift as u32 };
    let negative = if shift < 0 { shift.wrapping_neg() as u32 } else { 0 };
    if unsafe { f.at(0x1680).half() } != 1 {
        let x = unsafe { f.word(0x3174) } as i32;
        let half = (unsafe { f.word(0x3154) } as i32) / 2;
        source.address = source.address.wrapping_add(unsafe { f.word(0x190) }.wrapping_mul(negative)).wrapping_add(unsafe { f.word(0x194) } >> 1).wrapping_sub(((x % 8).wrapping_add(half) as u32).wrapping_mul(2));
        source.width = unsafe { f.word(0x3164) }.wrapping_mul(2);
        let limit = signed_div_pow2(unsafe { config_pair(0x9f01, 0x9f00) }, 1).wrapping_add(0x2a80).wrapping_add(unsafe { config_pair(0x9f15, 0x9f14) });
        if (target.width >> 1) < limit { loop { core::hint::spin_loop(); } }
        let f = unsafe { frame(context) };
        source.width = source.width.min(target.width.wrapping_sub(unsafe { f.word(0x3174) }.wrapping_mul(2)));
        source.height = unsafe { f.word(0x3168) };
    }
    source.width = source.width.wrapping_add(7) & !7;
    source.height = source.height.wrapping_add(7) & !7;
    source.area();
    let f = unsafe { frame(context) };
    let rows = if unsafe { f.at(0x1680).half() } == 1 {
        if unsafe { f.word(0x5d0) } <= 1 { 8 } else { signed_div_pow2(unsafe { config_pair(0x9f13, 0x9f12) }, 3).wrapping_add(8) }
    } else {
        let x = unsafe { f.word(0x3174) } as i32;
        target.address = target.address.wrapping_add((x.wrapping_sub(x % 8) as u32).wrapping_mul(2));
        positive.wrapping_add(unsafe { f.word(0x3178) }).wrapping_add(8)
    };
    target.address = target.address.wrapping_add(target.stride.wrapping_mul(rows));
    target.width = source.width; target.height = source.height; target.length = source.length;
    unsafe { services::copy_image(frame(context).0, &mut source, &mut target) }
}
unsafe fn split_capture(context: StageContext) -> u32 {
    let f = unsafe { frame(context) };
    let orientation = unsafe { services::orientation(f.word(0x1604), f.word(0x5d0)) };
    let mut targets = [core::mem::MaybeUninit::<Image>::uninit(); 2];
    let mut status = 1;
    for index in 0..2 {
        let f = unsafe { frame(context) };
        let mut source = unsafe { f.image(0x184) };
        let mut target = unsafe { object(context, 0x38, 0) };
        let half = source.width >> 1;
        source.width = half; source.area();
        if index == 1 { source.address = source.address.wrapping_add(unsafe { frame(context).word(0x194) } >> 1); }
        target.width = source.width; target.height = source.height; target.length = source.length;
        let extra = if (orientation == 3 && index == 1) || (orientation != 3 && index == 0) { (unsafe { frame(context).word(0x194) }) >> 1 } else { 0 };
        target.address = target.address.wrapping_add(extra).wrapping_add(16).wrapping_add(target.stride.wrapping_mul(8));
        if orientation == 3 {
            let height = unsafe { frame(context).word(0x198) };
            let padding = (height.wrapping_add(15) & !15).wrapping_sub(height);
            if padding != 0 { source.height = source.height.wrapping_add(padding); source.area(); target.height = source.height; target.length = source.length; target.address = target.address.wrapping_sub(target.stride.wrapping_mul(padding)); }
            unsafe { services::rotate_copy(frame(context).0, &mut source, &mut target) };
            if padding != 0 { target.address = target.address.wrapping_add(target.stride.wrapping_mul(padding)); target.height = target.height.wrapping_sub(padding); target.area(); }
            status = 1;
        } else { status = unsafe { services::copy_image(frame(context).0, &mut source, &mut target) }; }
        targets[index].write(target);
    }
    let first = unsafe { targets[0].assume_init() };
    let second = unsafe { targets[1].assume_init() };
    let mut target = if orientation == 3 { first } else { second };
    target.width = first.width.wrapping_add(second.width); target.area();
    unsafe { target.store(frame(context).at(0x184)) };
    status
}
unsafe fn rotate_capture(context: StageContext, pass: CapturePass) -> u32 {
    let secondary = matches!(pass, CapturePass::Secondary);
    let f = unsafe { frame(context) };
    let mut source = unsafe { f.image(0x184) };
    let mut target = unsafe { object(context, if secondary { 0x29 } else { 0x25 }, 0) };
    let f = unsafe { frame(context) };
    if secondary {
        target.width = unsafe { f.word(0x194) }; target.height = unsafe { f.word(0x198) }; target.area();
        target.address = target.address.wrapping_add(target.stride.wrapping_mul(8)).wrapping_add(16);
        let height = unsafe { f.word(0x198) };
        let padding = (height.wrapping_add(7) & !7).wrapping_sub(height);
        if padding != 0 { source.height = source.height.wrapping_add(padding); source.area(); target.address = target.address.wrapping_sub(target.stride.wrapping_mul(padding)); target.height = source.height; target.length = source.length; }
        unsafe { services::rotate_copy(frame(context).0, &mut source, &mut target) };
        if padding != 0 { target.address = target.address.wrapping_add(target.stride.wrapping_mul(padding)); target.height = target.height.wrapping_sub(padding); target.area(); }
        unsafe { target.store(frame(context).at(0x184)) };
        1
    } else {
        let mode = unsafe { f.word(0x5d0) };
        let transform = match mode {
            0 | 1 => {
                let half = source.width >> 1;
                if mode == 0 { source.address = source.address.wrapping_sub(source.stride.wrapping_mul(half.wrapping_sub(source.height))); }
                source.height = half; source.area();
                target.stride = target.width; target.height = unsafe { frame(context).word(0x194) } >> 1; target.area();
                if mode == 0 { 1 } else { 3 }
            }
            _ => { target.width = unsafe { f.word(0x194) }; target.height = unsafe { f.word(0x198) }; target.area(); if mode == 2 { 2 } else { 0 } }
        };
        let status = unsafe { services::rotate(frame(context).0, &mut source, &mut target, transform, 0) };
        let f = unsafe { frame(context) };
        let old_height = unsafe { f.word(0x198) };
        unsafe { target.store(f.at(0x184)) };
        if unsafe { f.word(0x5d0) } <= 1 {
            unsafe { f.at(0x194).write_word(old_height.wrapping_mul(2)) };
            target.address = target.address.wrapping_sub(signed_div_pow2(unsafe { config_pair(0x9f13, 0x9f12) }, 3).wrapping_mul(2));
            target.stride = unsafe { config_pair(0x9f13, 0x9f12) }.wrapping_mul(2); target.width = target.stride;
        } else {
            target.address = target.address.wrapping_sub(target.stride.wrapping_mul(signed_div_pow2(unsafe { config_pair(0x9f13, 0x9f12) }, 3)));
            target.height = unsafe { config_pair(0x9f13, 0x9f12) };
        }
        unsafe { target.store(frame(context).at(0x30dc)) };
        if unsafe { services::configuration(ConfigurationKey(0x9f6f)) } & 2 != 0 {
            let f = unsafe { frame(context) };
            unsafe { services::write_debug(0x18, f.word(0x30e0), f.word(0x30e8).wrapping_mul(f.word(0x30f0)), f.at(0x1680).byte() as u32, 0) };
        }
        status
    }
}
unsafe fn reduce_preview(context: StageContext, pass: CapturePass) -> u32 {
    let secondary = matches!(pass, CapturePass::Secondary);
    let f = unsafe { frame(context) };
    let mut source = unsafe { f.image(0x184) };
    let mut target = if secondary { unsafe { object(context, 0x26, 0) } } else { unsafe { f.image(0x30bc) } };
    let status;
    if secondary {
        target.width = (unsafe { f.word(0x194) } >> 2).wrapping_add(31) & 0x7fffffe0;
        target.height = unsafe { f.word(0x198) } >> 2; target.area();
        status = unsafe { services::reduce(frame(context).0, f.at(0x184).0 as *mut Image, &mut target, 0, 0) };
    } else {
        source.address = source.address.wrapping_add(15).wrapping_add(source.width.wrapping_sub(0xf00) >> 1).wrapping_add(source.stride.wrapping_mul(source.height.wrapping_sub(0x5a0) >> 1)) & !15;
        source.width = 0xf00; source.height = 0x5a0; source.length = 0x546000;
        target.stride = if unsafe { f.word(0x548) } == 1 { 0x500 } else { 0x5a0 };
        target.width = target.stride; target.height = 0x1e0; target.area();
        unsafe { target.store(f.at(0x30bc)) };
        status = unsafe { services::reduce(frame(context).0, &mut source, &mut target, 0, 0) };
    }
    if secondary {
        if unsafe { frame(context).word(0x5d0) } <= 1 {
            target.address = target.address.wrapping_sub(signed_div_pow2(unsafe { config_pair(0x9f13, 0x9f12) }, 5).wrapping_mul(2));
            target.stride = signed_div_pow2(unsafe { config_pair(0x9f13, 0x9f12) }, 2).wrapping_mul(2); target.width = target.stride;
        } else {
            target.address = target.address.wrapping_sub(target.stride.wrapping_mul(signed_div_pow2(unsafe { config_pair(0x9f13, 0x9f12) }, 5)));
            target.height = signed_div_pow2(unsafe { config_pair(0x9f13, 0x9f12) }, 2);
        }
        unsafe { target.store(frame(context).at(0x30f8)) };
    }
    status
}

macro_rules! entry {
    ($name:ident, $stage:expr) => { #[unsafe(no_mangle)] pub unsafe extern "C" fn $name(context: *mut u8) -> u32 { unsafe { execute(StageContext(context), $stage) }.0 } };
}
entry!(xm1_develop_pipeline_finish_planes, Stage::FinishPlanes);
entry!(xm1_develop_pipeline_blend_planes, Stage::BlendPlanes);
entry!(xm1_develop_pipeline_difference_planes, Stage::DifferencePlanes);
entry!(xm1_develop_pipeline_merge_planes, Stage::MergePlanes);
entry!(xm1_develop_pipeline_prepare_planes, Stage::PreparePlanes);
entry!(xm1_develop_pipeline_select_composite, Stage::SelectComposite);
entry!(xm1_develop_pipeline_prepare_composite, Stage::PrepareComposite);
entry!(xm1_develop_pipeline_split_composite, Stage::SplitComposite);
entry!(xm1_develop_pipeline_blend_captures, Stage::BlendCaptures);
entry!(xm1_develop_pipeline_crop_capture, Stage::CropCapture);
entry!(xm1_develop_pipeline_split_capture, Stage::SplitCapture);
entry!(xm1_develop_pipeline_reduce_border, Stage::ReduceBorder);
entry!(xm1_develop_pipeline_combine_border, Stage::CombineBorder);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_develop_pipeline_fill_chroma(context: *mut u8, pass: u32) -> u32 {
    unsafe { execute(StageContext(context), Stage::FillChroma(if pass == 2 { ChromaPass::Secondary } else { ChromaPass::Primary })) }.0
}
fn capture_pass(value: u32) -> CapturePass { match value { 0 => CapturePass::Primary, 1 => CapturePass::Secondary, _ => CapturePass::Invalid } }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_develop_pipeline_rotate_capture(context: *mut u8, pass: u32) -> u32 { unsafe { execute(StageContext(context), Stage::RotateCapture(capture_pass(pass))) }.0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_develop_pipeline_reduce_preview(context: *mut u8, pass: u32) -> u32 { unsafe { execute(StageContext(context), Stage::ReducePreview(capture_pass(pass))) }.0 }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_develop_pipeline_transform_border(context: *mut u8, pass: u32) -> u32 { unsafe { execute(StageContext(context), Stage::TransformBorder(if pass == 1 { BorderPass::Previous } else { BorderPass::Current })) }.0 }

mod services {
    use super::{Address32, ConfigurationKey, Image};
    unsafe extern "C" {
        #[link_name = "xm1_develop_split_planes"] pub(super) fn split_descriptors(context: *mut u8, planes: Address32, source: Address32, target: *const Image, count: u8);
        #[link_name = "xm1_pipeline_image_finish_primary"] pub(super) fn finish_primary(frame: Address32, source: Address32, target: Address32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_finish_secondary"] pub(super) fn finish_secondary(frame: Address32, source: Address32, target: Address32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_blend_planes"] pub(super) fn blend_planes(frame: Address32, first: Address32, second: Address32, third: Address32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_blend_secondary"] pub(super) fn blend_secondary(frame: Address32, first: Address32, second: Address32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_blend_finish"] pub(super) fn blend_finish(frame: Address32, first: Address32, second: Address32, third: Address32, scratch: Address32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_difference"] pub(super) fn difference(frame: Address32, first: Address32, second: Address32, output: Address32, pass: u32) -> u32;
        #[link_name = "xm1_pipeline_image_merge"] pub(super) fn merge(frame: Address32, planes: *const Address32, count: u32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_merge_finish"] pub(super) fn merge_finish(frame: Address32, source: Address32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_prepare"] pub(super) fn prepare(frame: Address32, output: Address32);
        #[link_name = "xm1_pipeline_image_prepare_finish"] pub(super) fn prepare_finish(frame: Address32, source: Address32, scratch: *mut Image, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_select_three"] pub(super) fn select_three(frame: Address32, first: Address32, second: Address32, third: Address32, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_select_five"] pub(super) fn select_five(frame: Address32, source: Address32, output: Address32, preserved_r3: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_select_four"] pub(super) fn select_four(frame: Address32, planes: *const Address32, count: u32, scratch: Address32, tail: *mut Image, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_select_thirteen"] pub(super) fn select_thirteen(frame: Address32, planes: *const Address32, count: u32, tail: *mut Image, output: Address32) -> u32;
        #[link_name = "xm1_pipeline_image_prepare_three"] pub(super) fn prepare_three(frame: Address32, output: Address32);
        #[link_name = "xm1_pipeline_image_prepare_four"] pub(super) fn prepare_four(frame: Address32, output: Address32);
        #[link_name = "xm1_pipeline_image_split_four"] pub(super) fn split_four(frame: Address32, planes: *const Address32, count: u32);
        #[link_name = "xm1_pipeline_image_split_five"] pub(super) fn split_five(frame: Address32, planes: *const Address32, count: u32);
        #[link_name = "xm1_pipeline_image_split_thirteen"] pub(super) fn split_thirteen(frame: Address32, planes: *const Address32, count: u32);
        #[link_name = "xm1_pipeline_image_fill_chroma"] pub(super) fn fill_chroma(frame: Address32, first: u32, second: u32, image: *mut Image);
        #[link_name = "xm1_pipeline_image_blend_overlapping"] pub(super) fn blend_captures(frame: Address32, sources: *mut Address32, count: u32, output: *mut Image) -> u32;
        #[link_name = "xm1_pipeline_image_copy_plane"] pub(super) fn copy_image(frame: Address32, source: *mut Image, target: *mut Image) -> u32;
        #[link_name = "xm1_develop_rotate_copy"] pub(super) fn rotate_copy(frame: Address32, source: *const Image, target: *mut Image);
        #[link_name = "xm1_develop_orientation"] pub(super) fn orientation(capture: u32, mode: u32) -> u32;
        #[link_name = "xm1_pipeline_image_rotate_plane"] pub(super) fn rotate(frame: Address32, source: *mut Image, target: *mut Image, transform: u32, option: u32) -> u32;
        #[link_name = "xm1_pipeline_image_resize_through_crop"] pub(super) fn reduce(frame: Address32, source: *mut Image, target: *mut Image, option: u32, flags: u32) -> u32;
        #[link_name = "xm1_pipeline_image_transform_border"] pub(super) fn transform_border(frame: Address32, source: *mut Image, target: *mut Image, option: u32, current: u32) -> u32;
        #[link_name = "xm1_pipeline_image_reduce_border"] pub(super) fn reduce_border(frame: Address32, source: *mut Image, target: *mut Image) -> u32;
        #[link_name = "xm1_pipeline_image_noise_reference"] pub(super) fn combine_border(frame: Address32, first: Address32, second: Address32, third: Address32, output: *mut Image) -> u32;
        #[link_name = "xm1_transforms_configuration"] pub(super) fn configuration(key: ConfigurationKey) -> u32;
        #[link_name = "xm1_pipeline_image_publish_debug_file"] pub(super) fn publish(frame: Address32, kind: u32, image: Address32, number: u32, suffix: u32);
        #[link_name = "xm1_pipeline_image_write_debug_file"] pub(super) fn write_debug(kind: u32, address: u32, length: u32, format: u32, suffix: u32);
    }
}
