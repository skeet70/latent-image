use super::shared::{Address32, ByteOffset, ObjectGroup, ObjectKind, OperationCode, ServiceCompletion, StageContext, StageResult};
use super::stage::StagePlan;
use core::mem::MaybeUninit;

#[derive(Clone, Copy)]
pub(super) enum Stage {
    ResizeAlignedPair,
    CopyAlignedPair,
    BlendAlignedPair,
    CorrectPairShear,
    BlendReference,
    AnalyzePairFeatures,
    ExtractPairFeatures,
    FindPairAlignment,
    BlendFrameEdge,
    MeasureFrameMotion,
    MeasurePredictedMotion,
}

impl Stage {
    pub(super) fn from_command(operation: OperationCode) -> Option<Self> {
        Some(match operation.0 {
            0x62 => Self::ResizeAlignedPair, 0x61 => Self::CopyAlignedPair,
            0x60 => Self::BlendAlignedPair, 0x5f => Self::CorrectPairShear,
            0x5e => Self::BlendReference, 0x5b => Self::AnalyzePairFeatures,
            0x5a => Self::ExtractPairFeatures, 0x59 => Self::FindPairAlignment,
            0x56 => Self::BlendFrameEdge, 0x55 => Self::MeasureFrameMotion,
            0x58 => Self::MeasurePredictedMotion, _ => return None,
        })
    }

    fn operation(self) -> OperationCode {
        OperationCode(match self {
            Self::ResizeAlignedPair => 0x62, Self::CopyAlignedPair => 0x61,
            Self::BlendAlignedPair => 0x60, Self::CorrectPairShear => 0x5f,
            Self::BlendReference => 0x5e, Self::AnalyzePairFeatures => 0x5b,
            Self::ExtractPairFeatures => 0x5a, Self::FindPairAlignment => 0x59,
            Self::BlendFrameEdge => 0x56, Self::MeasureFrameMotion => 0x55,
            Self::MeasurePredictedMotion => 0x58,
        })
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct ConfigurationKey(u32);
#[repr(C)]
#[derive(Clone, Copy)]
struct ImagePlane {
    format: u32,
    address: u32,
    size: u32,
    stride: u32,
    width: u32,
    height: u32,
    flags: u32,
}
impl ImagePlane {
    unsafe fn load(address: Address32) -> Self {
        unsafe { core::ptr::read_volatile(address.0 as *const Self) }
    }
    unsafe fn store(self, address: Address32) {
        unsafe { core::ptr::write_volatile(address.0 as *mut Self, self) };
    }
}
#[derive(Clone, Copy)]
struct PreparationState(Address32);
impl PreparationState {
    unsafe fn from_context(context: StageContext) -> Self {
        Self(Address32(unsafe { context.frame().field(ByteOffset(0x3228)).word() }))
    }
    fn field(self, offset: u32) -> Address32 { self.0.offset(ByteOffset(offset)) }
    unsafe fn word(self, offset: u32) -> u32 { unsafe { self.field(offset).word() } }
    unsafe fn put(self, offset: u32, value: u32) { unsafe { self.field(offset).write_word(value) }; }
    unsafe fn plane(self, offset: u32) -> ImagePlane { unsafe { ImagePlane::load(self.field(offset)) } }
}
unsafe fn config(key: ConfigurationKey) -> u32 { unsafe { xm1_preparation_configuration_byte(key.0) } }
unsafe fn config_pair(key: ConfigurationKey) -> u32 {
    let high = unsafe { config(ConfigurationKey(key.0 + 1)) };
    (unsafe { config(key) }) | high << 8
}
unsafe fn config_half(key: ConfigurationKey) -> u32 { unsafe { xm1_preparation_configuration_half(key.0) } }
unsafe fn object(context: StageContext, group: u32) -> ImagePlane {
    unsafe { ImagePlane::load(context.object(ObjectGroup(group), ObjectKind(0))) }
}
unsafe fn frame_word(context: StageContext, offset: u32) -> u32 {
    unsafe { context.frame().field(ByteOffset(offset)).word() }
}
unsafe fn frame_put(context: StageContext, offset: u32, value: u32) {
    unsafe { context.frame().field(ByteOffset(offset)).write_word(value) };
}
unsafe fn frame_plane(context: StageContext) -> ImagePlane {
    unsafe { ImagePlane::load(context.frame().field(ByteOffset(0x184))) }
}
unsafe fn frame_count(context: StageContext) -> u32 {
    let key = match unsafe { frame_word(context, 0x5cc) } { 0 => 0x9f64, 1 => 0x9f65, _ => 0x9f66 };
    unsafe { config(ConfigurationKey(key)) }
}
unsafe fn pair_enabled(context: StageContext) -> bool {
    if unsafe { frame_word(context, 0x5cc) } != 4 { return false; }
    let state = unsafe { PreparationState::from_context(context) };
    unsafe { state.word(0x110) != 1 && state.word(0x114) != 1 && state.word(0x10c) != 1 }
}
unsafe fn enabled(context: StageContext, stage: Stage) -> bool {
    match stage {
        Stage::BlendFrameEdge => unsafe { frame_word(context, 0x2790) != 0 },
        Stage::MeasureFrameMotion => unsafe { frame_word(context, 0x2780) != 0 && frame_word(context, 0x2784) == 0 },
        Stage::MeasurePredictedMotion => unsafe { frame_word(context, 0x2780) != 0 && frame_word(context, 0x2784) != 0 },
        _ => unsafe { pair_enabled(context) },
    }
}

pub(super) unsafe fn execute(context: StageContext, stage: Stage) -> StageResult {
    let plan = unsafe { StagePlan::develop(stage.operation()) };
    if !unsafe { enabled(context, stage) } { return unsafe { plan.skip(context) }; }
    let result = unsafe { plan.begin(context) };
    if result != StageResult::SUCCESS { return result; }
    let completion = unsafe {
        match stage {
            Stage::ResizeAlignedPair => resize_pair(context),
            Stage::CopyAlignedPair => copy_pair(context),
            Stage::BlendAlignedPair => blend_pair(context),
            Stage::CorrectPairShear => correct_shear(context),
            Stage::BlendReference => blend_reference(context),
            Stage::AnalyzePairFeatures => analyze_features(context),
            Stage::ExtractPairFeatures => extract_features(context),
            Stage::FindPairAlignment => find_alignment(context),
            Stage::BlendFrameEdge => blend_edge(context),
            Stage::MeasureFrameMotion => measure_motion(context, false),
            Stage::MeasurePredictedMotion => measure_motion(context, true),
        }
    };
    unsafe { plan.complete(context, ServiceCompletion(completion)) }
}

unsafe fn copy_pair(context: StageContext) -> u32 {
    let state = unsafe { PreparationState::from_context(context) };
    let mut first = unsafe { state.plane(0x1c) };
    let mut second = unsafe { state.plane(0x38) };
    let destination = unsafe { object(context, 0x36) };
    let displacement = unsafe { state.word(0x100) } as i32;
    let (positive, negative) = if displacement < 0 { (0, displacement.wrapping_neg() as u32) } else { (displacement as u32, 0) };
    first.height = unsafe { config_half(ConfigurationKey(0x9f12)) };
    let mut output = destination;
    output.address = output.address.wrapping_add(output.stride.wrapping_mul(negative));
    unsafe { xm1_preparation_copy_plane(context.frame().0.0, &mut first, &mut output) };
    let adjustment = unsafe { state.word(0x11c).wrapping_sub(state.word(0xfc)) }.wrapping_mul(2);
    second.address = second.address.wrapping_add(adjustment);
    second.width = second.width.wrapping_add(7).wrapping_sub(adjustment) & !7;
    second.height = unsafe { config_half(ConfigurationKey(0x9f12)) };
    output = destination;
    output.address = output.address.wrapping_add(output.stride.wrapping_mul(positive)).wrapping_add(unsafe { state.word(0x11c) }.wrapping_mul(2));
    let result = unsafe { xm1_preparation_copy_plane(context.frame().0.0, &mut second, &mut output) };
    let state = unsafe { PreparationState::from_context(context) };
    unsafe { second.store(state.field(0x38)); state.put(0xfc, state.word(0x11c)); }
    result
}
unsafe fn blend_pair(context: StageContext) -> u32 {
    let state = unsafe { PreparationState::from_context(context) };
    let mut first = unsafe { state.plane(0x1c) };
    let mut second = unsafe { state.plane(0x38) };
    first.address = first.address.wrapping_add(unsafe { state.word(0x11c) }.wrapping_mul(2));
    first.width = unsafe { config(ConfigurationKey(0x9fa9)) }.wrapping_mul(2);
    second.address = second.address.wrapping_add(unsafe { state.word(0x11c).wrapping_sub(state.word(0xfc)) }.wrapping_mul(2));
    second.width = unsafe { config(ConfigurationKey(0x9fa9)) }.wrapping_mul(2);
    unsafe { xm1_preparation_blend_planes(context.frame().0.0, &mut first, &mut second, state.word(0x100) as i32, 1) };
    1
}
unsafe fn correct_shear(context: StageContext) -> u32 {
    let mut first = unsafe { object(context, 0x34) };
    let mut second = unsafe { object(context, 0x35) };
    let state = unsafe { PreparationState::from_context(context) };
    unsafe { xm1_preparation_shear_plane(context.frame().0.0, state.field(0x1c).0 as *mut ImagePlane, &mut first, state.field(0x130).0 as *mut i32) };
    let state = unsafe { PreparationState::from_context(context) };
    let result = unsafe { xm1_preparation_shear_plane(context.frame().0.0, state.field(0x38).0 as *mut ImagePlane, &mut second, state.field(0x130).0 as *mut i32) };
    let state = unsafe { PreparationState::from_context(context) };
    let slope = unsafe { state.word(0x130) } as i32;
    let product = (unsafe { state.word(0x48) } >> 1).wrapping_mul(slope as u32) as i32;
    let rounded = if slope > 0 {
        let value = product.wrapping_add(0x8000);
        if value < 0 { value.wrapping_add(0xffff) } else { value }
    } else {
        let value = product.wrapping_sub(0x8000);
        if value < 0 { product.wrapping_add(0x7fff) } else { value }
    };
    unsafe { state.put(0x100, (rounded >> 16) as u32) };
    result
}
unsafe fn blend_reference(context: StageContext) -> u32 {
    let state = unsafe { PreparationState::from_context(context) };
    let mut reference = unsafe { state.plane(0x8c) };
    let mut source = unsafe { state.plane(0x1c) };
    let mut destination = unsafe { object(context, 0x33) };
    unsafe { xm1_preparation_copy_plane(context.frame().0.0, &mut source, &mut destination) };
    let state = unsafe { PreparationState::from_context(context) };
    unsafe { source.store(state.field(0x1c)) };
    source.address = source.address.wrapping_add(unsafe { state.word(0x120) }.wrapping_mul(2));
    source.width = unsafe { config(ConfigurationKey(0x9fa9)) }.wrapping_mul(2);
    reference.width = unsafe { config(ConfigurationKey(0x9fa9)) }.wrapping_mul(2);
    unsafe { xm1_preparation_blend_planes(context.frame().0.0, &mut source, &mut reference, state.word(0x124) as i32, 1) };
    reference = unsafe { PreparationState::from_context(context).plane(0x8c) };
    unsafe { xm1_preparation_copy_plane(context.frame().0.0, &mut reference, &mut source) }
}

unsafe fn feature_planes(context: StageContext, source: Address32, parity: u32, destination: &mut [MaybeUninit<ImagePlane>; 7]) {
    let scratch = unsafe { object(context, 0x3a) };
    unsafe { xm1_develop_expand_planes(context.0, destination.as_mut_ptr().cast(), source.0 as *const ImagePlane, &scratch, 7, parity) };
}
unsafe fn analyze_features(context: StageContext) -> u32 {
    let levels = unsafe { config(ConfigurationKey(0x9fc5)) }.wrapping_add(1);
    let mut planes = [MaybeUninit::<ImagePlane>::uninit(); 7];
    let mut pointers = [core::ptr::null_mut::<ImagePlane>(); 7];
    let mut result = 0;
    for parity in 0..2 {
        let source = unsafe { PreparationState::from_context(context) }.field(if parity == 0 { 0x54 } else { 0x70 });
        unsafe { feature_planes(context, source, parity, &mut planes) };
        for (index, pointer) in pointers.iter_mut().enumerate() { *pointer = planes[index].as_mut_ptr(); }
        result = unsafe { xm1_preparation_analyze_features(context.frame().0.0, pointers.as_ptr(), levels, 1, parity) };
    }
    result
}
unsafe fn extract_features(context: StageContext) -> u32 {
    let mut planes = [MaybeUninit::<ImagePlane>::uninit(); 7];
    let source = unsafe { PreparationState::from_context(context) }.field(0x54);
    unsafe { feature_planes(context, source, 0, &mut planes) };
    let mut pointers = [core::ptr::null_mut::<ImagePlane>(); 7];
    for (index, pointer) in pointers.iter_mut().enumerate() { *pointer = planes[index].as_mut_ptr(); }
    let output = unsafe { context.object(ObjectGroup(0x2c), ObjectKind(0)).offset(ByteOffset(4)).word() };
    unsafe { xm1_preparation_extract_features(context.frame().0.0, pointers.as_ptr(), output, 1) }
}

unsafe fn blend_edge(context: StageContext) -> u32 {
    let displacement = unsafe { frame_word(context, 0x3198) } as i32;
    let (positive, negative) = if displacement < 0 { (0, displacement.wrapping_neg() as u32) } else { (displacement as u32, 0) };
    let count = unsafe { context.frame().field(ByteOffset(0x1680)).half() };
    if count <= 1 || (unsafe { frame_count(context) } as i32) < count as i32 { return 1; }
    let edge = unsafe { frame_word(context, 0x3178) };
    if unsafe { context.frame().field(ByteOffset(0x3194)).byte() } != 0x10 { return 1; }
    let mut destination = unsafe { object(context, 0x28) };
    let x = unsafe { frame_word(context, 0x3174) } as i32;
    destination.address = destination.address.wrapping_add((x.wrapping_sub(x % 8) as u32).wrapping_mul(2)).wrapping_add(destination.stride.wrapping_mul(positive.wrapping_add(edge).wrapping_add(8))).wrapping_sub(unsafe { config(ConfigurationKey(0x9f0f)) }.wrapping_mul(2));
    destination.width = unsafe { config(ConfigurationKey(0x9f0f)) }.wrapping_mul(2);
    destination.height = unsafe { frame_word(context, 0x3168) };
    destination.size = destination.width.wrapping_mul(destination.height);
    let mut source = unsafe { frame_plane(context) };
    let address = source.address.wrapping_add(unsafe { frame_word(context, 0x194) } >> 1).wrapping_add(source.stride.wrapping_mul(negative));
    let border = unsafe { config(ConfigurationKey(0x9f0f)) };
    let x = unsafe { frame_word(context, 0x3174) } as i32;
    let half_width = unsafe { frame_word(context, 0x3154) } as i32 / 2;
    source.address = address.wrapping_sub(border.wrapping_add((x % 8).wrapping_add(half_width) as u32).wrapping_mul(2));
    source.width = destination.width;
    source.height = destination.height;
    source.size = destination.size;
    unsafe { xm1_preparation_blend_planes(context.frame().0.0, &mut destination, &mut source, 0, 0) };
    1
}


unsafe fn resize_pair(context: StageContext) -> u32 {
    let mut source = unsafe { object(context, 0x36) };
    let mut destination = unsafe { object(context, 0x37) };
    let frames = Address32(unsafe { frame_word(context, 0x3150) });
    let mut top = unsafe { frames.offset(ByteOffset(0x48)).word() };
    let mut bottom = unsafe { frames.offset(ByteOffset(0x4c)).word() };
    let mut index = 0;
    while index < unsafe { frame_count(context) } {
        let entry = Address32(unsafe { frame_word(context, 0x3150) }).offset(ByteOffset(index * 0x60));
        if unsafe { entry.offset(ByteOffset(0x40)).byte() } == 0x10 {
            top = unsafe { entry.offset(ByteOffset(0x48)).word() };
            bottom = unsafe { entry.offset(ByteOffset(0x4c)).word() };
        }
        index += 1;
    }
    let state = unsafe { PreparationState::from_context(context) };
    let width = unsafe { state.word(0xfc).wrapping_add(state.word(0x48) >> 1) }.wrapping_sub(16);
    let vertical = (unsafe { state.word(0x100) } as i32).wrapping_abs() as u32;
    let portrait = unsafe { frame_word(context, 0x5d0) }.wrapping_sub(2) < 2;
    let profile = unsafe { frame_word(context, 0x3214) }.min(4);
    let border = unsafe { config_pair(ConfigurationKey(0x9f16 + profile * 16 + if portrait { 0 } else { 4 })) };
    top = top.wrapping_add(border);
    bottom = bottom.wrapping_sub(border);
    let slope = unsafe { state.word(0x130) } as i32;
    let inset = (unsafe { state.word(0x2c) } >> 1).wrapping_mul(slope.wrapping_abs() as u32).wrapping_add(0x8000) >> 16;
    let inset = inset.wrapping_add(2);
    let remainder = inset.wrapping_mul(2) & 7;
    if remainder != 0 { bottom = bottom.wrapping_add(remainder).wrapping_sub(8); }
    if slope < 0 {
        top = top.wrapping_add(inset);
        if bottom as i32 >= (inset as i32).wrapping_neg() { bottom = inset.wrapping_neg(); }
    } else {
        if (top as i32) < inset as i32 { top = inset; }
        bottom = bottom.wrapping_sub(inset);
    }
    top = top.wrapping_add(vertical);
    let height = unsafe { config_half(ConfigurationKey(0x9f12)) }.wrapping_add(bottom.wrapping_sub(top)).wrapping_sub(8);
    let output_height_key = ConfigurationKey(if portrait { 0x9f5e } else { 0x9f62 });
    let numerator = width.wrapping_mul(unsafe { config_half(output_height_key) });
    let divisor = unsafe { config_half(ConfigurationKey(0x9f5c)) };
    let resized_height = unsafe { xm1_preparation_unsigned_divide(numerator, divisor) };
    source.address = source.address.wrapping_add(16).wrapping_add(source.stride.wrapping_mul(top.wrapping_add(4)));
    source.width = width.wrapping_mul(2);
    source.height = height;
    source.size = source.width.wrapping_mul(height);
    destination.address = destination.address.wrapping_add(destination.stride.wrapping_mul(8)).wrapping_add(16);
    destination.width = unsafe { config_half(ConfigurationKey(0x9f5c)) }.wrapping_mul(2);
    destination.height = unsafe { config_half(output_height_key) };
    destination.size = destination.width.wrapping_mul(destination.height);
    unsafe { Address32(0x015b0cd4).write_word(0) };
    if height < resized_height {
        let mut cancel = 0u8;
        let high = unsafe { config(ConfigurationKey(0x9fac)) };
        let color = unsafe { config(ConfigurationKey(0x9fac)) } | 0x80800000 | high << 8;
        let leading = resized_height.wrapping_sub(height) >> 1;
        let trailing = resized_height.wrapping_sub(height).wrapping_sub(leading);
        if leading != trailing { unsafe { Address32(0x015b0cd4).write_word(1) }; }
        let mut fill = source;
        fill.address = fill.address.wrapping_sub(16).wrapping_add(fill.stride.wrapping_mul(leading.wrapping_add(2).wrapping_neg()));
        fill.width = fill.width.wrapping_add(32);
        fill.height = leading.wrapping_add(2);
        fill.size = fill.width.wrapping_mul(fill.height);
        let task = unsafe { context.address().offset(ByteOffset(0x34)).word() };
        let flags = unsafe { context.address().offset(ByteOffset(0x3c)).word() };
        unsafe { xm1_preparation_fill_plane(&mut cancel, &mut fill, 6, color, task, flags) };
        fill = source;
        fill.address = fill.address.wrapping_add(fill.stride.wrapping_mul(fill.height)).wrapping_sub(16);
        fill.width = fill.width.wrapping_add(32);
        fill.height = trailing.wrapping_add(2);
        fill.size = fill.width.wrapping_mul(fill.height);
        unsafe { xm1_preparation_fill_plane(&mut cancel, &mut fill, 6, color, task, flags) };
        source.address = source.address.wrapping_sub(source.stride.wrapping_mul(leading));
    } else {
        source.address = source.address.wrapping_add(source.stride.wrapping_mul(height.wrapping_sub(resized_height) >> 1));
    }
    source.height = resized_height;
    source.size = source.width.wrapping_mul(resized_height);
    let result = unsafe { xm1_preparation_resize_plane(context.frame().0.0, &mut source, &mut destination, 0, 1, 1, 0, 1) };
    unsafe { destination.store(context.frame().field(ByteOffset(0x184))) };
    result
}

#[derive(Clone, Copy)]
struct MotionEstimate {
    x: i32,
    y: i32,
    vectors: [i32; 4],
    score: u32,
}
unsafe fn measure_motion(context: StageContext, predicted: bool) -> u32 {
    let mut prediction_x = 0i32;
    let mut prediction_y = 0i32;
    let mut search_x = 0i32;
    let mut search_y = 0i32;
    if predicted {
        let packed = unsafe { frame_word(context, 0x3224) };
        let x = (packed >> 16) as i32 - 0x8000;
        let y = (packed & 0xffff) as i32 - 0x8000;
        (prediction_x, prediction_y) = match unsafe { frame_word(context, 0x5d0) } {
            1 => (x, y), 0 => (x.wrapping_neg(), y.wrapping_neg()),
            2 => (y, x.wrapping_neg()), _ => (y.wrapping_neg(), x),
        };
        prediction_x = prediction_x.wrapping_add(unsafe { frame_word(context, 0x31ac) } as i32);
        prediction_y = prediction_y.wrapping_add(unsafe { frame_word(context, 0x31b0) } as i32);
        let base = if unsafe { frame_word(context, 0x53c) } == 0 { 0x9f77 } else { 0x9f7c };
        let gain_x = unsafe { config(ConfigurationKey(base)) } as i32;
        let offset_x = unsafe { config(ConfigurationKey(base + 2)) } as i32;
        let value = prediction_x.wrapping_mul(gain_x).wrapping_add(128);
        search_x = offset_x.wrapping_add(value / 256);
        let gain_y = unsafe { config(ConfigurationKey(base + 1)) } as i32;
        let offset_y = unsafe { config(ConfigurationKey(base + 3)) } as i32;
        let value = prediction_y.wrapping_mul(gain_y).wrapping_add(128);
        search_y = offset_y.wrapping_add(value / 256);
        let divisor = unsafe { config_pair(ConfigurationKey(0x9f84)) } as i32;
        let scale = unsafe { frame_word(context, 0x1760) } as i32;
        search_x = unsafe { xm1_preparation_signed_divide(scale.wrapping_mul(search_x).wrapping_mul(100), divisor) } / 100;
        search_y = unsafe { xm1_preparation_signed_divide(scale.wrapping_mul(search_y).wrapping_mul(100), divisor) } / 100;
        if unsafe { config(ConfigurationKey(0x9f94)) } != 0 { search_x = search_x.wrapping_neg(); }
        if unsafe { config(ConfigurationKey(0x9f95)) } != 0 { search_y = search_y.wrapping_neg(); }
    }
    let frame = unsafe { context.frame() };
    let references = !predicted || unsafe { frame_word(context, 0x2788) } == 1;
    let mut estimate = MotionEstimate { x: 0, y: 0, vectors: [0; 4], score: 0 };
    let [v0, v1, v2, v3] = &mut estimate.vectors;
    estimate.score = unsafe {
        xm1_preparation_motion(frame.0.0, frame.field(ByteOffset(0x30dc)).0 as *const ImagePlane,
            if references { frame.field(ByteOffset(0x30f8)).0 as *const ImagePlane } else { core::ptr::null() },
            frame.field(ByteOffset(0x3114)).0 as *const ImagePlane,
            if references { frame.field(ByteOffset(0x3130)).0 as *const ImagePlane } else { core::ptr::null() },
            frame.field(ByteOffset(0x1680)).half() as u32,
            if predicted { &mut search_x } else { core::ptr::null_mut() },
            if predicted { &mut search_y } else { core::ptr::null_mut() },
            if !predicted { 2 } else if references { 0 } else { 1 },
            &mut estimate.x, &mut estimate.y, v0, v1, v2, v3)
    };
    unsafe { frame_put(context, 0x314c, estimate.score) };
    let half_x = estimate.x / 2;
    let portrait = unsafe { frame_word(context, 0x5d0) }.wrapping_sub(2) < 2;
    let width = unsafe { config_pair(ConfigurationKey(if portrait { 0x9f00 } else { 0x9f02 })) }.wrapping_sub(half_x as u32);
    let initial_x = estimate.x.wrapping_add(unsafe { frame_word(context, 0x315c) } as i32);
    let initial_y = estimate.y.wrapping_add(unsafe { frame_word(context, 0x3160) } as i32);
    if predicted && estimate.x > 0 { prediction_x = 0; prediction_y = 0; }
    let width = width.max(if portrait { 0x500 } else { 0x280 }).wrapping_add(half_x as u32) as i32;
    let mut width = (if width < 0 { width.wrapping_add(7) } else { width }) as u32 & !7;
    let allowance = unsafe { config_pair(ConfigurationKey(if portrait { 0x9f6b } else { 0x9f6d })) }.wrapping_mul(2) as i32;
    let previous_y = unsafe { frame_word(context, 0x3198) } as i32;
    if estimate.y < 0 {
        let lower = (unsafe { frame_word(context, 0x319c) } as i32).wrapping_sub(allowance);
        if estimate.y.wrapping_add(previous_y) <= lower { estimate.y = lower.wrapping_sub(previous_y); }
    } else {
        let upper = allowance.wrapping_add(unsafe { frame_word(context, 0x31a0) } as i32);
        if upper <= estimate.y.wrapping_add(previous_y) { estimate.y = upper.wrapping_sub(previous_y); }
    }
    let y = estimate.y.wrapping_add(unsafe { frame_word(context, 0x3198) } as i32);
    let lower = (unsafe { frame_word(context, 0x319c) } as i32).max(y);
    let upper = (unsafe { frame_word(context, 0x31a0) } as i32).min(y);
    let height = (unsafe { frame_word(context, 0x198) } as i32).wrapping_sub(y.wrapping_abs());
    let offset = if unsafe { context.frame().field(ByteOffset(0x1680)).half() } == 1 || unsafe { frame_word(context, 0x278c) } == 1 {
        (unsafe { frame_word(context, 0x194) }) >> 2
    } else {
        unsafe { frame_word(context, 0x3174) }.wrapping_add(((unsafe { frame_word(context, 0x3154) } as i32) / 2) as u32)
    };
    let offset = offset.wrapping_add(half_x as u32);
    let border = if unsafe { frame_word(context, 0x5d0) } < 2 { 0 } else { (unsafe { config_pair(ConfigurationKey(0x9f12)) } as i32 / 8) as u32 };
    let mode = unsafe { frame_word(context, 0x5cc) };
    let frequency: i32 = match mode { 0 => 120, 1 => 180, _ => 360 };
    let profile = match mode { 0 => 0, 1 => 0x10, _ => 0x40 };
    let base = if unsafe { frame_word(context, 0x5d0) } < 2 { 0x9f18 } else { 0x9f14 };
    let first = unsafe { config_pair(ConfigurationKey(base + profile)) };
    let second = unsafe { config_pair(ConfigurationKey(base + profile + 8)) };
    let scaled_offset = unsafe { xm1_preparation_signed_divide(frequency.wrapping_mul(offset as i32), first.wrapping_add(second) as i32) };
    let ceiling_base = unsafe { config_pair(ConfigurationKey(0x9f00)) } as i32 / 2;
    let ceiling_extra = unsafe { config_pair(ConfigurationKey(0x9f14)) };
    let ceiling = ceiling_base.wrapping_add(0x2a80).wrapping_add(ceiling_extra as i32).wrapping_sub(offset as i32);
    if ceiling <= width as i32 {
        let first = unsafe { config_pair(ConfigurationKey(0x9f00)) } as i32 / 2;
        let second = unsafe { config_pair(ConfigurationKey(0x9f14)) };
        width = first.wrapping_add(0x2a80).wrapping_add(second as i32).wrapping_sub(offset as i32) as u32;
    }
    let fields = [
        estimate.x as u32, estimate.y as u32, initial_x as u32, initial_y as u32, width, height as u32,
        half_x as u32, (height / 2) as u32, offset, border,
        estimate.vectors[0] as u32, estimate.vectors[1] as u32, estimate.vectors[2] as u32, estimate.vectors[3] as u32,
        estimate.score, scaled_offset as u32,
    ];
    for (index, value) in fields.into_iter().enumerate() { unsafe { frame_put(context, 0x31b4 + index as u32 * 4, value) }; }
    unsafe { context.frame().field(ByteOffset(0x31f4)).write_byte(0) };
    let tail = [y as u32, lower as u32, upper as u32, search_y as u32, search_x as u32, prediction_x as u32, prediction_y as u32];
    for (index, value) in tail.into_iter().enumerate() { unsafe { frame_put(context, 0x31f8 + index as u32 * 4, value) }; }
    1
}

unsafe fn find_alignment(context: StageContext) -> u32 {
    let original = unsafe { object(context, 0x28) };
    let stride = original.stride;
    let right_address = original.address.wrapping_add(stride.wrapping_mul(8));
    let aligned_half = ((unsafe { frame_word(context, 0x321c) } >> 1) & !7).wrapping_add(8);
    let width = aligned_half.wrapping_mul(2);
    let height = original.height.wrapping_sub(16);
    let left_address = right_address.wrapping_sub(32).wrapping_add(width);
    let state = unsafe { PreparationState::from_context(context) };
    let mut right = original;
    right.address = right_address; right.size = height.wrapping_mul(width);
    right.width = width; right.height = height;
    unsafe { right.store(state.field(0x38)); }
    unsafe { state.put(0x20, left_address); state.put(0x24, height.wrapping_mul(width)); state.put(0x28, stride); state.put(0x2c, width); state.put(0x30, height); }
    if unsafe { config(ConfigurationKey(0x9f6f)) } & 8 != 0 {
        unsafe {
            xm1_preparation_debug_file(0x18, left_address, stride.wrapping_mul(height), 0, 0);
            xm1_preparation_debug_file(0x18, right_address, stride.wrapping_mul(height), 0, 0);
        }
    }
    let excess = width.wrapping_sub(unsafe { frame_word(context, 0x321c) } & 0x7fffffff);
    unsafe { PreparationState::from_context(context).put(0x104, width.wrapping_sub(excess) >> 1) };
    let left_start = left_address.wrapping_add(width).wrapping_sub(unsafe { config_pair(ConfigurationKey(0x9f12)) }.wrapping_mul(2));
    let search_width = unsafe { config_pair(ConfigurationKey(0x9f12)) }.wrapping_mul(2);
    let search_height = unsafe { config_pair(ConfigurationKey(0x9f12)) };
    let mut current = ImagePlane { format: original.format, address: left_start, size: search_width.wrapping_mul(search_height), stride, width: search_width, height: search_height, flags: original.flags };
    let mut previous = current;
    previous.address = right_address;
    let frames = Address32(unsafe { frame_word(context, 0x3150) });
    let mut search_offset = unsafe { frames.offset(ByteOffset(0xc)).word() } as i32;
    let mut index = 0;
    while index < unsafe { frame_count(context) } {
        let frame = Address32(unsafe { frame_word(context, 0x3150) }).offset(ByteOffset(index * 0x60));
        if unsafe { frame.offset(ByteOffset(0x40)).byte() } == 0x10 { search_offset = unsafe { frame.offset(ByteOffset(0xc)).word() } as i32; }
        index += 1;
    }
    let mut search_count = 0i32;
    let current_reference = unsafe { context.object(ObjectGroup(0x2a), ObjectKind(0)) };
    let previous_reference = unsafe { context.object(ObjectGroup(0x2b), ObjectKind(0)) };
    let increment = unsafe { config_pair(ConfigurationKey(0x9fa2)) };
    unsafe { xm1_preparation_reduce_plane(context.frame().0.0, &mut previous, previous_reference.0 as *mut ImagePlane, 0, 0) };
    let mut shift = 0u32;
    let mut best_score = 0i32;
    let mut found = false;
    let mut index = 0i32;
    while index < unsafe { config(ConfigurationKey(0x9fa4)) } as i32 {
        unsafe { xm1_preparation_reduce_plane(context.frame().0.0, &mut current, current_reference.0 as *mut ImagePlane, 0, 0) };
        let mut x = 0i32;
        let mut y = 0i32;
        let mut vectors = [MaybeUninit::<i32>::uninit(); 4];
        let score = unsafe { xm1_preparation_motion(context.frame().0.0, &current, current_reference.0 as *const ImagePlane, &previous, previous_reference.0 as *const ImagePlane, 1, &mut search_count, &mut search_offset, 3, &mut x, &mut y, vectors[0].as_mut_ptr(), vectors[1].as_mut_ptr(), vectors[2].as_mut_ptr(), vectors[3].as_mut_ptr()) } as i32;
        if index == 0 { best_score = score; }
        if x > 0 && (score == 0 || score < best_score || index == 0) {
            let state = unsafe { PreparationState::from_context(context) };
            let border = unsafe { config_pair(ConfigurationKey(0x9f12)) };
            let x = (x as u32).wrapping_add(aligned_half.wrapping_sub(border).wrapping_sub(shift));
            unsafe { state.put(0xfc, x); PreparationState::from_context(context).put(0x100, y as u32); }
            let state = unsafe { PreparationState::from_context(context) };
            let x = unsafe { state.word(0xfc) };
            let y = unsafe { state.word(0x100) } as i32;
            let positive = (y as u32) & !((y >> 31) as u32);
            let negative = if y <= 0 { y.wrapping_neg() as u32 } else { 0 };
            let mut left = unsafe { state.plane(0x1c) };
            left.address = left.address.wrapping_add(x.wrapping_mul(2)).wrapping_add(left.stride.wrapping_mul(positive));
            let border = unsafe { config_pair(ConfigurationKey(0x9f12)) };
            let available = unsafe { PreparationState::from_context(context).word(0x2c) };
            left.width = if available < border.wrapping_add(x).wrapping_mul(2) {
                available.wrapping_sub(x.wrapping_mul(2))
            } else { unsafe { config_pair(ConfigurationKey(0x9f12)) }.wrapping_mul(2) };
            let height = unsafe { config_pair(ConfigurationKey(0x9f12)) }.wrapping_sub(positive).wrapping_add(1) as i32;
            left.height = height.wrapping_add((height as u32 >> 31) as i32) as u32 & !1;
            left.size = left.width.wrapping_mul(left.height);
            let state = unsafe { PreparationState::from_context(context) };
            let mut right = unsafe { state.plane(0x38) };
            right.address = right.address.wrapping_add(right.stride.wrapping_mul(negative));
            right.width = left.width; right.height = left.height; right.size = left.size;
            unsafe { left.store(state.field(0x54)); right.store(PreparationState::from_context(context).field(0x70)); }
            let available = width.wrapping_sub(excess.wrapping_add(x.wrapping_mul(2)));
            unsafe { PreparationState::from_context(context).put(0x108, left.width.min(available) >> 1) };
            best_score = score;
            found = true;
        }
        current.address = current.address.wrapping_sub(increment.wrapping_mul(2));
        shift = shift.wrapping_add(increment);
        index += 1;
    }
    let y = (unsafe { PreparationState::from_context(context).word(0x100) } as i32).wrapping_abs();
    let limit = unsafe { config_pair(ConfigurationKey(0x9fa0)) } as i32;
    if y <= limit {
        let score_limit = unsafe { config_pair(ConfigurationKey(0x9fe8)) } as i32;
        if best_score <= score_limit && found {
            let state = unsafe { PreparationState::from_context(context) };
            let mut left = unsafe { state.plane(0x54) };
            let mut right = unsafe { state.plane(0x70) };
            let mut left_destination = unsafe { object(context, 0x3f) };
            left_destination.width = left.width; left_destination.height = left.height; left_destination.size = left.size;
            let mut right_destination = unsafe { object(context, 0x40) };
            right_destination.width = right.width; right_destination.height = right.height; right_destination.size = right.size;
            unsafe { xm1_preparation_copy_plane(context.frame().0.0, &mut left, &mut left_destination) };
            let result = unsafe { xm1_preparation_copy_plane(context.frame().0.0, &mut right, &mut right_destination) };
            unsafe { left_destination.store(PreparationState::from_context(context).field(0x54)); right_destination.store(PreparationState::from_context(context).field(0x70)); }
            return result;
        }
    }
    unsafe { PreparationState::from_context(context).put(0x110, 1) };
    1
}

macro_rules! public_stage {
    ($name:ident, $stage:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(context: *mut u8) -> u32 {
            unsafe { execute(StageContext(context), Stage::$stage).0 }
        }
    };
}
public_stage!(xm1_develop_pipeline_resize_aligned_pair, ResizeAlignedPair);
public_stage!(xm1_develop_pipeline_copy_aligned_pair, CopyAlignedPair);
public_stage!(xm1_develop_pipeline_blend_aligned_pair, BlendAlignedPair);
public_stage!(xm1_develop_pipeline_correct_pair_shear, CorrectPairShear);
public_stage!(xm1_develop_pipeline_blend_reference, BlendReference);
public_stage!(xm1_develop_pipeline_analyze_pair_features, AnalyzePairFeatures);
public_stage!(xm1_develop_pipeline_extract_pair_features, ExtractPairFeatures);
public_stage!(xm1_develop_pipeline_find_pair_alignment, FindPairAlignment);
public_stage!(xm1_develop_pipeline_blend_frame_edge, BlendFrameEdge);
public_stage!(xm1_develop_pipeline_measure_frame_motion, MeasureFrameMotion);
public_stage!(xm1_develop_pipeline_measure_predicted_motion, MeasurePredictedMotion);

unsafe extern "C" {
    fn xm1_preparation_configuration_byte(key: u32) -> u32;
    fn xm1_preparation_configuration_half(key: u32) -> u32;
    #[link_name = "xm1_pipeline_image_copy_plane"]
    fn xm1_preparation_copy_plane(frame: u32, source: *mut ImagePlane, destination: *mut ImagePlane) -> u32;
    #[link_name = "xm1_pipeline_image_blend_pixels"]
    fn xm1_preparation_blend_planes(frame: u32, first: *mut ImagePlane, second: *mut ImagePlane, displacement: i32, destination: u32);
    #[link_name = "xm1_pipeline_image_shear_plane"]
    fn xm1_preparation_shear_plane(frame: u32, source: *mut ImagePlane, destination: *mut ImagePlane, slope: *mut i32) -> u32;
    #[link_name = "xm1_pipeline_image_reduce_planes"]
    fn xm1_preparation_analyze_features(frame: u32, planes: *const *mut ImagePlane, levels: u32, enabled: u32, parity: u32) -> u32;
    #[link_name = "xm1_pipeline_image_motion_primary"]
    fn xm1_preparation_extract_features(frame: u32, planes: *const *mut ImagePlane, output: u32, enabled: u32) -> u32;
    fn xm1_develop_expand_planes(context: *mut u8, planes: *mut ImagePlane, source: *const ImagePlane, destination: *const ImagePlane, count: u8, parity: u32);
    #[link_name = "xm1_pipeline_image_resize_capture"]
    fn xm1_preparation_resize_plane(frame: u32, source: *mut ImagePlane, destination: *mut ImagePlane, mode: u32, enabled: u32, resize: u32, reserved: u32, kind: u32) -> u32;
    #[link_name = "xm1_pipeline_image_fill_pattern"]
    fn xm1_preparation_fill_plane(cancel: *mut u8, plane: *mut ImagePlane, pattern: u32, color: u32, task: u32, flags: u32) -> u32;
    #[link_name = "xm1_pipeline_image_resize_through_crop"]
    fn xm1_preparation_reduce_plane(frame: u32, source: *mut ImagePlane, destination: *mut ImagePlane, reference: u32, kind: u32) -> u32;
    #[link_name = "xm1_pipeline_image_motion"]
    fn xm1_preparation_motion(frame: u32, current: *const ImagePlane, previous: *const ImagePlane, current_reference: *const ImagePlane, previous_reference: *const ImagePlane, count: u32, search_count: *mut i32, search_offset: *mut i32, mode: u32, x: *mut i32, y: *mut i32, vector0: *mut i32, vector1: *mut i32, vector2: *mut i32, vector3: *mut i32) -> u32;
    #[link_name = "xm1_pipeline_image_write_debug_file"]
    fn xm1_preparation_debug_file(kind: u32, address: u32, size: u32, number: u32, suffix: u32);
    fn xm1_preparation_signed_divide(numerator: i32, denominator: i32) -> i32;
    fn xm1_preparation_unsigned_divide(numerator: u32, denominator: u32) -> u32;
}
