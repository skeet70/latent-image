use crate::exposure::ports::{Properties, Property};
use crate::exposure::settings;

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Key {
    Plain = 0xde,
    NoSearch = 0x3e0009,
    DriveMotion = 0x926e,
    ScanChannels = 0x9290,
    ScanModes = 0x9291,
    ScanMissing = 0x9292,
    ScanSpeedRatio = 0x9293,
    ScanDuration = 0x9294,
    ScanFlip = 0x9297,
    ScanDepth = 0x9299,
    ScanFollowDepth = 0x92a1,
    FixedDepth = 0x949a,
    DepthCeiling = 0x949b,
    DepthEnabled = 0x3e144d,
    DepthEnabledWord = 0x3e1445,
    MaximumFramesMargin = 0x9283,
    MinimumSpan = 0x928a,
    ForceDirection = 0x928b,
    Direction = 0x928c,
    ChannelWindow = 0x928d,
    StartDelay = 0x9281,
    LensStartDelay = 0x3e0fdd,
    SpeedRatio = 0x9617,
    AlternateSpeedRatio = 0x9618,
    SpeedFrames = 0x961a,
    Alternate = 0x94d2,
    AlternateWidth = 0x94d3,
    LensCount = 0x3e1213,
    GridSize = 0x94dd,
    GridFrameSize = 0x94de,
    GridThresholdRow = 0x94da,
    GridThresholdScale = 0x94db,
    DirectionAdjust = 0x940a,
    ReuseFlip = 0x9486,
    SideMargin = 0x3e0240,
    SideOverlap = 0x946f,
    StartOverlap = 0x92bd,
    StartDepth = 0x96de,
    ResultOverlap = 0x96c5,
    ResultLowOverlap = 0x96c6,
    ResultLowLevel = 0x96c7,
    TargetLowLevel = 0x96d3,
    TargetSpan = 0x92b8,
    TargetRetry = 0x92b5,
    TargetDirections = 0x92b4,
    TargetEnabled = 0x92b3,
    TargetChannels = 0x92c7,
    TargetContrast = 0x92c2,
    TargetPastChannel = 0x92c8,
    FirstFrameDepth = 0x92fe,
    TargetCurrentPosition = 0x94eb,
    FixedExposure = 0x9602,
    FirstWeight = 0x9607,
    SecondWeight = 0x9608,
    ThirdWeight = 0x9609,
    AlternateWeight = 0x960a,
    ContrastFloor = 0x92d9,
    FocusFloor = 0x942f,
    ListedScales = 0xa48b,
    Peak = 0x942a,
    PlainPeakLow = 0x946a,
    PlainPeakHigh = 0x9468,
    AreaPeakLow = 0x9448,
    AreaPeakHigh = 0x9446,
    BelowLevel = 0x92cd,
    TreatedChannelWindow = 0xa4d4,
    TreatedSettled = 0xa497,
    TreatedNext = 0xa498,
    TreatedDirection = 0xa499,
    TreatedFirst = 0xa488,
    Debug = 0x4907,
    Override = 0xa8,
    OverridePosition = 0x9611,
    CachedPending = 0x9278,
    Reuse = 0x9479,
    ReuseChannels = 0x9483,
    ReusePlain = 0x9481,
    ReuseLevel = 0x947f,
    ReuseLevelAbove = 0x947a,
    ReuseLevelBelow = 0x947b,
    ReuseStepAbove = 0x947c,
    ReuseStepBelow = 0x947d,
    ReuseMinimumLevel = 0x9480,
    NextPeak = 0x92cc,
    NextNear = 0x9470,
    NextTravel = 0x96c8,
    NextMissing = 0x96cb,
    NextArea = 0x94ca,
    SlowChoice = 0x96ca,
}

pub fn byte(p: &mut impl Properties, key: Key) -> u32 {
    p.property(Property(key as u32))
}
pub fn word(p: &mut impl Properties, key: Key) -> u16 {
    settings::word(p, Property(key as u32))
}
pub fn signed(p: &mut impl Properties, key: Key) -> i16 {
    i16::from(byte(p, key) as u8 as i8)
}

pub fn depth_values(
    p: &mut impl Properties,
    kind: u8,
    mode: u32,
    class: u8,
    selector: u8,
) -> [u16; 2] {
    let (ids, words) = match selector {
        2 => ([0x9664, 0x9666], true),
        0 => ([0x9668, 0x9669], false),
        _ => match kind {
            23 => ([0x94cd, 0x94ce], false),
            17 | 18 => ([0x9299; 2], true),
            8 => ([0x949c, 0x94a4], false),
            2 => ([0x949d, 0x94a5], false),
            5 => ([0x949e, 0x94a6], false),
            _ => (
                match (mode, class) {
                    (3, _) => [0x94a3, 0x94ab],
                    (_, 0) => [0x94a2, 0x94aa],
                    (1, _) => [0x94a0, 0x94a8],
                    (2, _) => [0x94a1, 0x94a9],
                    _ => [0x949f, 0x94a7],
                },
                false,
            ),
        },
    };
    ids.map(|id| {
        if words {
            settings::word(p, Property(id)).wrapping_mul(10)
        } else {
            (p.property(Property(id)) * 10) as u16
        }
    })
}

pub fn depth_periods(p: &mut impl Properties, selector: u8) -> [u32; 2] {
    let ids = match selector {
        0 => [0x966c, 0x966d],
        2 => [0x966a, 0x966b],
        _ => [0x94ac, 0x94ad],
    };
    ids.map(|id| p.property(Property(id)).wrapping_mul(1000))
}

pub enum Window {
    Confirmation,
    Request,
    Treated,
}
pub fn window(p: &mut impl Properties, window: Window) -> [u16; 4] {
    let ids = match window {
        Window::Confirmation => [0x9492, 0x9490, 0x9496, 0x9494],
        Window::Request => [0x9489, 0x9487, 0x948d, 0x948b],
        Window::Treated => [0xa49f, 0xa49d, 0xa4a3, 0xa4a1],
    };
    ids.map(|id| settings::word(p, Property(id)))
}

pub fn target_mode(p: &mut impl Properties, source: u32) -> u32 {
    p.property(Property(0x96cd + source))
}
pub fn target_fraction(p: &mut impl Properties, source: u32, low: bool) -> u32 {
    p.property(Property(
        (source.wrapping_add(0x4b6a))
            .wrapping_mul(2)
            .wrapping_add(u32::from(low)),
    ))
}
pub fn direction_percent(p: &mut impl Properties, channel: u8, alternate: bool) -> [u32; 2] {
    let ids = [
        [0x940b, 0x940e],
        [0x940c, 0x940f],
        [0x940d, 0x9410],
        [0x940b, 0x940e],
        [0x94d4, 0x94d6],
        [0x94d5, 0x94d7],
    ];
    ids[usize::from(alternate) * 3 + usize::from(channel.min(2))].map(|id| p.property(Property(id)))
}

pub fn weights(
    p: &mut impl Properties,
    program: usize,
    step: u8,
    gain: u32,
    alternate: bool,
    adjusted: bool,
) -> ([u32; 3], [u32; 3]) {
    let base = [0x9041, 0x9089, 0x90d1][program];
    let extra = [0x911a, 0x9132, 0x914a][program];
    let columns = [0, 1, 2 + u32::from(alternate)];
    (
        columns.map(|c| p.property(Property(base + u32::from(step.wrapping_mul(4)) + c))),
        if adjusted {
            columns.map(|c| p.property(Property(extra + (gain as u8).wrapping_mul(4) as u32 + c)))
        } else {
            [0; 3]
        },
    )
}

pub fn threshold_coefficients(p: &mut impl Properties, row: usize) -> [u16; 3] {
    [
        [0x92e0, 0x92e6, 0x92ec],
        [0x92de, 0x92e4, 0x92ea],
        [0x92f3, 0x92f7, 0x92fb],
        [0x92f1, 0x92f5, 0x92f9],
        [0x92e2, 0x92e8, 0x92ee],
        [0xa4a6, 0xa4a8, 0xa4aa],
    ][row]
        .map(|id| settings::word(p, Property(id)))
}

pub fn depth_settings(
    p: &mut impl Properties,
    high: bool,
) -> crate::adjust::motion::plan::DepthSettings {
    let (halves, bytes) = if high {
        ([0x943b, 0x9440, 0x9442], [0x943d, 0x943e, 0x943f])
    } else {
        ([0x942d, 0x9437, 0x9439], [0x942c, 0x942b, 0x9436])
    };
    crate::adjust::motion::plan::DepthSettings {
        halves: halves.map(|id| settings::word(p, Property(id))),
        bytes: bytes.map(|id| p.property(Property(id)) as u8),
    }
}
