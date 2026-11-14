use crate::adjust::motion::plan::{AreaLayout, Plan, PlanKind, StartTarget};
use crate::adjust::motion::plan_ports::{PlanPorts, PlanSpeedRequest};
use crate::adjust::motion::ports::{AreaGrid, BoundsQuery, SearchBounds};
use crate::adjust::motion::prepare::channel_bounds;
use crate::adjust::motion::queries::round_tenths;
use crate::adjust::motion::state::MotionState;
use crate::stock::plan;
use crate::stock::plan_config::{self as cfg, Key::*, byte as b, signed as sb, word as w};

pub(crate) fn div(value: i32, divisor: i32) -> i32 {
    value.checked_div(divisor).unwrap_or(0)
}
pub(crate) fn ceil_div(value: i32, divisor: i32) -> i32 {
    let q = div(value, divisor);
    if divisor == 0 || value.checked_rem(divisor).unwrap_or(0) == 0 {
        q
    } else if (value < 0) != (divisor < 0) {
        q - 1
    } else {
        q + 1
    }
}

pub(crate) fn depth(
    s: &MotionState,
    p: &mut impl PlanPorts,
    kind: PlanKind,
    selector: u8,
    load: bool,
) -> u16 {
    if b(p, Plain) & 1 != 0 || load {
        return 150;
    }
    let [low, high] =
        cfg::depth_values(p, kind.0, s.search.mode.code(), s.readings.class, selector);
    if selector != 2 && b(p, DepthEnabled) == 0 && w(p, DepthEnabledWord) == 0 {
        return (b(p, FixedDepth) * 10) as u16;
    }
    let high = high.max(low);
    let [first, last] = cfg::depth_periods(p, selector);
    let period = s.frame_period();
    if period <= first {
        low
    } else if period >= last {
        high
    } else {
        u32::from(low).wrapping_add(
            period
                .wrapping_sub(first)
                .wrapping_mul(u32::from(high - low))
                / (last - first),
        ) as u16
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Speed {
    pub depth: u16,
    pub step: u16,
    pub frame_depth: u16,
    pub travel: u16,
    pub speed: u32,
}

pub(crate) fn speed(
    s: &MotionState,
    p: &mut impl PlanPorts,
    kind: PlanKind,
    selector: u8,
    load: bool,
) -> Speed {
    let depth = depth(s, p, kind, selector, load);
    let distance =
        (i32::from(s.bounds.high) - i32::from(s.bounds.low)).unsigned_abs() as u16 as i16;
    let maximum_step = div(i32::from(distance), b(p, MinimumSpan) as i32) as i16;
    let minimum_step = ceil_div(i32::from(distance), 251 - b(p, MaximumFramesMargin) as i32) as i16;
    let r = &s.readings;
    let step = p.plan_speed(PlanSpeedRequest {
        lens: r.lens,
        step: s.exposures.second.lens_step,
        depth,
        period: s.frame_period() / 100,
        maximum_step,
        minimum_step,
        drive_kind: r.drive_kind,
        selector,
    });
    let travel = (u64::from(s.frame_period()) * u64::from(step.rate) / 100_000) as u16;
    Speed {
        depth,
        step: step.size,
        frame_depth: p.travel_distance(r.lens, s.exposures.second.lens_step, travel as i16),
        travel,
        speed: step.rate,
    }
}

pub(crate) fn current(s: &MotionState, p: &mut impl PlanPorts) -> i16 {
    let position = p.position() as i16;
    let side = p.current_side();
    let _ = s;
    p.side_adjusted(position, side, 1)
}

pub(crate) fn treated_window(s: &MotionState, flags: u32) -> bool {
    let bit = if s.readings.auxiliary != 0 {
        16
    } else if s.readings.scene == 51 {
        4
    } else if s.readings.scene == 52 {
        8
    } else if s.layout.tracking.map_or(false, |t| t.area.at_2c() != 0) {
        2
    } else if s.layout.tracking.map_or(false, |t| t.area.at_26() == 1) {
        32
    } else {
        1
    };
    flags & bit != 0
}

pub(crate) fn interpolated(
    s: &MotionState,
    p: &mut impl PlanPorts,
    value: u16,
    window: cfg::Window,
) -> u16 {
    let [upper, lower, maximum, minimum] = cfg::window(p, window).map(i32::from);
    if value == 0 || upper <= lower || minimum >= maximum {
        return minimum as u16;
    }
    let bounds = channel_bounds(p, &s.readings, s.exposures.second.lens_step);
    let distance = (i32::from(bounds.high) - i32::from(bounds.low)).abs();
    let percent = distance * 100 / i32::from(value);
    if percent <= lower * 10 {
        minimum as u16
    } else if percent >= upper * 10 {
        maximum as u16
    } else {
        round_tenths(
            div(
                (percent - lower * 10).wrapping_mul(maximum - minimum),
                upper - lower,
            )
            .wrapping_add(minimum * 10),
        ) as u16
    }
}

pub(crate) fn symmetric(
    s: &MotionState,
    p: &mut impl PlanPorts,
    centre: i16,
    value: u16,
    window: cfg::Window,
) -> SearchBounds {
    let coefficient = interpolated(s, p, value, window);
    let delta = div(i32::from(value).wrapping_mul(i32::from(coefficient)), 100) as i16;
    SearchBounds {
        low: centre.wrapping_sub(delta),
        high: centre.wrapping_add(delta),
    }
}

fn target_side(
    s: &MotionState,
    p: &mut impl PlanPorts,
    target: StartTarget,
    direction: u32,
) -> (u8, SearchBounds) {
    let speed = speed(s, p, PlanKind(11), 1, s.readings.load_test);
    let low = i32::from(s.exposures.second.level)
        < i32::from(s.layout.lowest_level) - i32::from(sb(p, TargetLowLevel).wrapping_mul(10));
    let mut delta = div(
        i32::from(speed.travel).wrapping_mul(cfg::target_fraction(p, target.source, low) as i32),
        100,
    ) as i16;
    let span = div(
        (b(p, TargetSpan) as i32).wrapping_mul(i32::from(speed.travel)),
        10,
    ) as i16;
    if direction != p.current_side() {
        delta = delta.wrapping_add(w(p, SideMargin) as i16);
    }
    let mut bounds = s.bounds;
    let too_short = match direction {
        0 => {
            bounds.low = target.position.wrapping_sub(delta);
            if bounds.low < s.bounds.low {
                return (16, s.bounds);
            }
            s.bounds.high < bounds.low.wrapping_add(span)
        }
        1 => {
            bounds.high = target.position.wrapping_add(delta);
            if bounds.high > s.bounds.high {
                return (16, s.bounds);
            }
            bounds.high.wrapping_sub(span) < s.bounds.low
        }
        _ => return (16, s.bounds),
    };
    if too_short {
        (if b(p, TargetRetry) != 0 { 0 } else { 16 }, s.bounds)
    } else {
        (11, bounds)
    }
}

pub(crate) fn target_bounds(
    s: &MotionState,
    p: &mut impl PlanPorts,
    target: StartTarget,
) -> (bool, SearchBounds, bool) {
    let mut direction = target.direction;
    let (mut status, mut bounds) = target_side(s, p, target, direction);
    if status == 0 {
        direction = u32::from(direction == 0);
        (status, bounds) = target_side(s, p, target, direction);
    }
    (status == 11, bounds, direction == 1)
}

pub(crate) fn query(s: &MotionState, channel: u8, lens_state: u32) -> BoundsQuery {
    BoundsQuery {
        lens: s.readings.lens,
        scene: s.readings.scene,
        channel,
        class: s.readings.class,
        step: s.exposures.second.lens_step,
        reference: s.readings.reference,
        held: s.readings.auxiliary,
        lens_state,
    }
}

pub(crate) fn secondary_grid(
    p: &mut impl PlanPorts,
    mode: AreaLayout,
    profile: u32,
    primary: AreaGrid,
    mut secondary: AreaGrid,
) -> AreaGrid {
    match mode {
        AreaLayout::Grid => {
            let [height_step, width_step] = p.cell_steps(profile);
            let frame = p.frame_size(profile);
            let width_scale = b(p, GridSize);
            let height_scale = b(p, GridSize);
            let width_frame_scale = u32::from(w(p, GridFrameSize));
            let height_frame_scale = u32::from(w(p, GridFrameSize));
            let width = (primary.cell_width.wrapping_mul(width_scale) / 100)
                .max(frame.width.wrapping_mul(width_frame_scale) / 1000)
                .min(primary.cell_width)
                / 3;
            let height = (primary.cell_height.wrapping_mul(height_scale) / 100)
                .max(frame.height.wrapping_mul(height_frame_scale) / 1000)
                .min(primary.cell_height)
                / 3;
            secondary.cell_width = width
                .wrapping_sub(width.checked_rem(width_step).unwrap_or(0))
                .max(width_step);
            secondary.cell_height = height
                .wrapping_sub(height.checked_rem(height_step).unwrap_or(0))
                .max(height_step);
            secondary.columns = 3;
            secondary.rows = 3;
        }
        AreaLayout::GridAlternate => {
            secondary.cell_width = primary.cell_width;
            secondary.cell_height = primary.cell_height;
            secondary.columns = 1;
            secondary.rows = 1;
        }
        _ => return secondary,
    }
    secondary.left = primary.left.wrapping_add(
        u32::from(primary.columns >> 1).wrapping_mul(
            primary
                .cell_width
                .wrapping_add(u32::from(primary.gap_horizontal)),
        ),
    );
    secondary.top = primary.top.wrapping_add(
        u32::from(primary.rows >> 1).wrapping_mul(
            primary
                .cell_height
                .wrapping_add(u32::from(primary.gap_vertical)),
        ),
    );
    if mode == AreaLayout::Grid {
        secondary.left = secondary.left.wrapping_add(
            primary
                .cell_width
                .wrapping_sub(secondary.cell_width.wrapping_mul(3))
                >> 1,
        );
        secondary.top = secondary.top.wrapping_add(
            primary
                .cell_height
                .wrapping_sub(secondary.cell_height.wrapping_mul(3))
                >> 1,
        );
    }
    secondary.gap_horizontal = 0;
    secondary.gap_vertical = 0;
    secondary
}

pub(crate) fn alternate(s: &MotionState, p: &mut impl PlanPorts, pass: &Plan) -> bool {
    if b(p, Alternate) == 0
        || s.readings.load_test
        || s.readings.channel == 0
        || (b(p, LensCount) as i32).wrapping_sub(1) < s.readings.lens as i32
        || plan::kind_row(pass.kind)[5] == 0
    {
        return false;
    }
    if pass.checks_speed
        && pass.speeds[1] != 0
        && pass.speeds[2].wrapping_mul(100) / pass.speeds[1] >= b(p, AlternateSpeedRatio) * 10
    {
        return false;
    }
    let sample = secondary_grid(
        p,
        AreaLayout::Grid,
        s.exposures.second.profile,
        s.layout.grids[0],
        AreaGrid::default(),
    );
    let search = secondary_grid(
        p,
        AreaLayout::Grid,
        crate::stock::motion::SEARCH_PROFILE,
        s.search.frames[0],
        AreaGrid::default(),
    );
    if sample
        .cell_width
        .wrapping_mul(100)
        .checked_div(search.cell_width)
        .unwrap_or(0)
        < b(p, AlternateWidth)
        || sample.cell_height.wrapping_mul(u32::from(search.rows)) > s.layout.grids[0].cell_height
    {
        return false;
    }
    let mut q = query(s, 0, 0);
    q.scene = 0;
    q.class = 3;
    q.held = 0;
    let first = p.search_bounds(&q);
    q.channel = 1;
    first.high < p.search_bounds(&q).high
}

pub(crate) fn adjust_direction(
    s: &MotionState,
    p: &mut impl PlanPorts,
    kind: PlanKind,
    alternate: bool,
    mut bounds: SearchBounds,
    reverse: &mut bool,
) {
    let flags = b(p, DirectionAdjust);
    if !(alternate || s.readings.channel == 0 || flags & 16 != 0) || plan::kind_row(kind)[3] == 0 {
        return;
    }
    let mask = if kind.0 == 16 {
        2
    } else {
        match s.readings.class {
            1 => 4,
            0 => 8,
            _ => 1,
        }
    };
    if flags & mask == 0 {
        return;
    }
    if kind.0 == 5 {
        bounds = channel_bounds(p, &s.readings, s.exposures.second.lens_step);
    }
    let position = current(s, p);
    let [low_percent, high_percent] = cfg::direction_percent(
        p,
        if s.readings.channel > 2 {
            0
        } else {
            s.readings.channel
        },
        alternate,
    );
    let distance = i32::from(bounds.high) - i32::from(bounds.low);
    let low = bounds
        .low
        .wrapping_add(div((low_percent as i32).wrapping_mul(distance), 100) as i16);
    let high = bounds
        .high
        .wrapping_sub(div((high_percent as i32).wrapping_mul(distance), 100) as i16);
    if position > low && position <= high {
        return;
    }
    *reverse = position > low;
    if kind.0 == 5 {
        *reverse = !*reverse;
    }
}
