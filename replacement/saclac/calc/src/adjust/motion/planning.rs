//! Pass planning over a prepared motion run.

use crate::adjust::motion::drive::{DriveMode, quotient};
use crate::adjust::motion::plan::{AreaLayout, ContrastThresholds, Motion, Plan, PlanKind};
use crate::adjust::motion::plan_geometry::{
    self as geometry, Speed, adjust_direction, alternate, div, secondary_grid, speed, symmetric,
    target_bounds, treated_window,
};
use crate::adjust::motion::plan_policy;
use crate::adjust::motion::plan_ports::{PlanPorts, PositionRemap};
use crate::adjust::motion::plan_result;
use crate::adjust::motion::ports::SearchBounds;
use crate::adjust::motion::prepare::channel_bounds;
use crate::adjust::motion::search::{ChannelClass, SearchMode};
use crate::adjust::motion::state::MotionState;
use crate::stock::plan;
use crate::stock::plan_config::{self as cfg, Key::*, byte as b, signed as sb, word as w};
use crate::stock::plan_trace::{self as trace, Trace};

fn channel_bit(channel: u8) -> u32 {
    match channel {
        1 => 2,
        2 => 4,
        _ => 1,
    }
}
fn signed_area(s: &MotionState) -> u32 {
    (i32::from(s.search.area.row as i8) * 7 + i32::from(s.search.area.column as i8)) as u32
}

fn scan_kind(s: &MotionState, p: &mut impl PlanPorts, pass: &mut Plan) {
    let kind = pass.kind;
    let candidate = if plan::kind_row(kind)[2] != 0 {
        if kind.0 == 15 { 18 } else { 17 }
    } else if matches!(kind.0, 17 | 18) {
        kind.0
    } else {
        return;
    };
    let r = &s.readings;
    let admitted = !r.load_test
        && r.scene != 50
        && (r.class != 0 || b(p, ScanModes) & 8 != 0)
        && (r.measurement_selection != xm1_saclac_profile_records::MeasurementSelection::Record
            || b(p, ScanModes) & 4 != 0)
        && b(p, ScanChannels) & channel_bit(r.channel) != 0
        && match s.search.mode {
            SearchMode::Channel => false,
            SearchMode::Treated => b(p, ScanModes) & 16 != 0,
            SearchMode::Area => r.class == 0 || b(p, ScanModes) & 2 != 0,
            SearchMode::Plain => b(p, ScanModes) & 1 != 0,
        };
    if !admitted {
        return;
    }
    let original = speed(s, p, kind, 1, r.load_test);
    let bounds = if kind.0 == 15 {
        s.bounds
    } else {
        channel_bounds(p, r, s.exposures.second.lens_step)
    };
    let distance = (i32::from(bounds.high) - i32::from(bounds.low)).unsigned_abs();
    let duration = quotient(
        u64::from(s.frame_period()) * 10 * u64::from(distance),
        u64::from(original.travel),
    ) as u32;
    if duration < u32::from(w(p, ScanDuration)) * 1000 {
        return;
    }
    let scan = speed(s, p, PlanKind(17), 1, r.load_test);
    if u64::from(original.speed) * u64::from(b(p, ScanSpeedRatio)) < u64::from(scan.speed) * 10 {
        pass.decided = kind;
        pass.kind = PlanKind(candidate);
    }
}

fn register_word(p: &mut impl PlanPorts, register: u8) -> i16 {
    i16::from_be_bytes([p.load_register(register), p.load_register(register + 1)])
}

fn load_speed(s: &MotionState, p: &mut impl PlanPorts) -> Speed {
    let magnitude = register_word(p, 0x86).wrapping_abs() as u16;
    let period = s.frame_period().max(1);
    let depth = p.travel_distance(
        s.readings.lens,
        s.exposures.second.lens_step,
        magnitude.wrapping_mul(10) as i16,
    );
    let requested = quotient(
        (i64::from(magnitude as i16) as u64).wrapping_mul(1_000_000),
        u64::from(period),
    ) as u32;
    let mut rate = requested;
    if rate < p.minimum_speed() {
        rate = p.minimum_speed();
    }
    let kind = p.drive_kind();
    if rate > p.top_speed(kind, DriveMode::MEASUREMENT) {
        let kind = p.drive_kind();
        rate = p.top_speed(kind, DriveMode::MEASUREMENT);
    }
    let travel = (u64::from(period) * u64::from(rate) / 100_000) as u16;
    Speed {
        depth,
        step: magnitude,
        frame_depth: p.travel_distance(
            s.readings.lens,
            s.exposures.second.lens_step,
            travel as i16,
        ),
        travel,
        speed: rate,
    }
}

fn speeds(s: &MotionState, p: &mut impl PlanPorts, pass: &mut Plan) {
    let middle = if s.readings.load_test && pass.motion == Motion::Continuous {
        load_speed(s, p)
    } else {
        speed(s, p, pass.kind, 1, s.readings.load_test)
    };
    trace::speed(
        p,
        s.section,
        1,
        [middle.depth, middle.step, middle.frame_depth, middle.travel],
    );
    let fast = speed(s, p, pass.kind, 2, s.readings.load_test);
    trace::speed(
        p,
        s.section,
        2,
        [fast.depth, fast.step, fast.frame_depth, fast.travel],
    );
    let slow = speed(s, p, pass.kind, 0, s.readings.load_test);
    trace::speed(
        p,
        s.section,
        0,
        [slow.depth, slow.step, slow.frame_depth, slow.travel],
    );
    pass.speeds = [
        slow.speed.min(middle.speed),
        middle.speed,
        fast.speed.max(middle.speed),
    ];
    pass.speed_choice = match pass.kind.0 {
        25 => 0,
        26 => u8::from(b(p, SlowChoice) == 1),
        _ => 1,
    };
    pass.speed = pass.speeds[usize::from(pass.speed_choice)];
    pass.frame_travel = (u64::from(s.frame_period()) * u64::from(pass.speed) / 100_000) as u16;
    pass.frame_depth = p.travel_distance(
        s.readings.lens,
        s.exposures.second.lens_step,
        pass.frame_travel as i16,
    );
    pass.checks_speed = if plan::kind_row(pass.kind)[6] == 1
        && p.checks_drive_speed()
        && !s.readings.load_test
        && s.search.mode != SearchMode::Treated
        && pass.motion != Motion::Stepped
        && s.search.mode != SearchMode::Channel
        && s.readings.scene != 50
        && pass.speeds[2] != 0
        && pass.speeds[1].wrapping_mul(100) / pass.speeds[2] <= b(p, SpeedRatio)
    {
        let bounds = channel_bounds(p, &s.readings, s.exposures.second.lens_step);
        div(
            (i32::from(bounds.high) - i32::from(bounds.low)).abs() * 10,
            i32::from(pass.frame_travel as i16),
        ) >= b(p, SpeedFrames) as i32
    } else {
        false
    };
}

fn overlap_bounds(s: &MotionState, centre: i16, margin: i16) -> SearchBounds {
    if s.plan.profile.reverse {
        SearchBounds {
            low: centre.wrapping_sub(margin),
            high: s.bounds.high,
        }
    } else {
        SearchBounds {
            low: s.bounds.low,
            high: centre.wrapping_add(margin),
        }
    }
}

fn pass_bounds(
    s: &MotionState,
    p: &mut impl PlanPorts,
    pass: &Plan,
    reverse: &mut bool,
) -> SearchBounds {
    let old = &s.plan;
    let r = &s.readings;
    let step = s.exposures.second.lens_step;
    let travel = pass.frame_travel;
    let mut bounds = match pass.kind.0 {
        2 | 5 => {
            let remap = if pass.kind.0 == 2 {
                PositionRemap {
                    position: r.confirmation[2] as i16,
                    lens: r.lens,
                    calibration: r.confirmation[4],
                    step,
                    side: r.confirmation[5],
                }
            } else {
                PositionRemap {
                    position: r.request.position,
                    lens: r.lens,
                    calibration: r.request.at_10,
                    step,
                    side: r.request.side,
                }
            };
            let centre = p.remap_position(remap);
            let result = symmetric(
                s,
                p,
                centre,
                travel,
                if pass.kind.0 == 2 {
                    cfg::Window::Confirmation
                } else {
                    cfg::Window::Request
                },
            );
            if pass.kind.0 == 2 {
                let half = result.high.wrapping_sub(centre);
                trace::store(p, s.section, Trace::ConfirmationHigh, half as i32 as u32);
                trace::store(p, s.section, Trace::ConfirmationLow, half as i32 as u32);
            }
            result
        }
        8 => symmetric(
            s,
            p,
            s.layout.treated_position,
            travel,
            cfg::Window::Treated,
        ),
        9 | 10 => {
            let first = p.search_bounds(&geometry::query(s, 0, 0));
            let second = p.search_bounds(&geometry::query(s, 1, 0));
            let low = if pass.kind.0 == 9 {
                first.high.wrapping_sub(div(
                    (b(p, SideOverlap) as i32).wrapping_mul(i32::from(travel)),
                    10,
                ) as i16)
            } else {
                first.low
            };
            SearchBounds {
                low,
                high: second.high,
            }
        }
        11 => {
            let (_, bounds, direction) = target_bounds(s, p, pass.target);
            *reverse = direction;
            bounds
        }
        12 | 13 => {
            let margin = if pass.kind.0 == 12 {
                div(
                    (b(p, StartOverlap) as i32).wrapping_mul(i32::from(travel)),
                    100,
                ) as i16
            } else {
                let depth = (b(p, StartDepth) * 10) as u16 & 0xfffe;
                p.depth_positions(r.lens, step, depth)
            };
            let margin = margin.wrapping_add(w(p, SideMargin) as i16);
            let centre = p.side_adjusted(old.profile.start, u32::from(old.profile.reverse), 1);
            overlap_bounds(s, centre, margin)
        }
        15 | 18 | 20 | 22 => s.bounds,
        23 => {
            let centre = p.side_adjusted(
                old.result.position as i16,
                u32::from(old.profile.reverse),
                1,
            );
            symmetric(s, p, centre, travel, cfg::Window::Request)
        }
        24 | 25 => {
            let low = i32::from(s.exposures.second.level)
                < i32::from(s.layout.lowest_level)
                    - i32::from(sb(p, ResultLowLevel).wrapping_mul(10));
            let margin = div(
                (b(p, if low { ResultLowOverlap } else { ResultOverlap }) as i32)
                    .wrapping_mul(i32::from(travel)),
                100,
            ) as i16;
            let margin = margin.wrapping_add(w(p, SideMargin) as i16);
            let centre = p.side_adjusted(
                old.result.position as i16,
                u32::from(old.profile.reverse),
                1,
            );
            overlap_bounds(s, centre, margin)
        }
        _ => channel_bounds(p, r, step),
    };
    if matches!(pass.kind.0, 19 | 20) {
        let centre = p.side_adjusted(
            old.result.position as i16,
            u32::from(old.profile.reverse),
            1,
        );
        let depth = w(p, ScanFollowDepth).wrapping_mul(10) & 0xfffe;
        let margin = p.depth_positions(r.lens, step, depth);
        if old.profile.reverse {
            bounds.low = centre.wrapping_sub(margin);
        } else {
            bounds.high = centre.wrapping_add(margin);
        }
    }
    bounds.low = bounds.low.max(s.bounds.low);
    bounds.high = bounds.high.min(s.bounds.high);
    let minimum = div(
        i32::from((travel as i16).wrapping_mul(b(p, MinimumSpan) as i16)),
        10,
    ) as i16;
    if ((i32::from(bounds.high) - i32::from(bounds.low)).abs() as i16) < minimum {
        let centre = ((i32::from(bounds.high) + i32::from(bounds.low)) >> 1) as i16;
        bounds = SearchBounds {
            low: centre.wrapping_sub(minimum >> 1),
            high: centre.wrapping_add(minimum >> 1),
        };
        if bounds.low < s.bounds.low {
            bounds.high = bounds
                .high
                .wrapping_add(s.bounds.low.wrapping_sub(bounds.low));
            bounds.low = s.bounds.low;
        } else if bounds.high > s.bounds.high {
            bounds.low = bounds
                .low
                .wrapping_sub(bounds.high.wrapping_sub(s.bounds.high));
            bounds.high = s.bounds.high;
        }
    }
    bounds
}

fn lead(s: &MotionState, speed: u32) -> u16 {
    let mut latest = s
        .sampling
        .primary()
        .iter()
        .copied()
        .max()
        .unwrap_or(i32::MIN);
    if s.search.mode == SearchMode::Treated {
        let count = usize::from(
            s.layout.grids[1]
                .columns
                .wrapping_mul(s.layout.grids[1].rows),
        );
        for &offset in s.sampling.secondary_cells().iter().take(count) {
            latest = latest.max(offset);
        }
    }
    let first = (i64::from(speed) * i64::from(latest) / 1_000_000) as i16;
    let last =
        (i64::from(speed) * i64::from(latest.wrapping_add(s.sampling.exposure)) / 1_000_000) as i16;
    ((i32::from(first) + i32::from(last)) / 2).max(0) as u16
}

fn drive_bounds(s: &MotionState, p: &mut impl PlanPorts, pass: &mut Plan) -> u16 {
    if s.readings.load_test {
        pass.profile.start = register_word(p, 0x82);
        pass.profile.end = register_word(p, 0x84);
        pass.step = register_word(p, 0x86);
        if (pass.profile.start < pass.profile.end && pass.step < 0)
            || (pass.profile.end < pass.profile.start && pass.step > 0)
        {
            pass.step = pass.step.wrapping_neg();
        }
        pass.profile.reverse = pass.profile.start >= pass.profile.end;
        return 0;
    }
    if b(p, Plain) & 1 != 0 {
        let [low, high] = p.position_limits(s.readings.lens);
        pass.profile.reverse = b(p, Plain) & 2 != 0;
        let configured = (b(p, Plain) & 0xfff) >> 4;
        pass.step = if configured == 0 {
            8
        } else {
            (configured * 2) as i16
        };
        if pass.profile.reverse {
            pass.profile.start = high;
            pass.profile.end = low;
            pass.step = pass.step.wrapping_neg();
        } else {
            pass.profile.start = low;
            pass.profile.end = high;
        }
        return 0;
    }
    let mask = if s.readings.channel != 0 {
        2
    } else {
        plan::direction_mask(s.readings.scene, s.readings.style)
    };
    let mut reverse = b(p, Direction) & mask != 0;
    let effective = if matches!(pass.kind.0, 5 | 17 | 18) {
        pass.decided
    } else {
        pass.kind
    };
    match effective.0 {
        7 => reverse = !(s.plan.kind.0 == 8 && treated_window(s, b(p, TreatedDirection))),
        8 | 15 | 27 => reverse = true,
        9 => reverse = false,
        10 => reverse = s.plan.profile.reverse,
        12..=14 | 19..=26 => reverse = !s.plan.profile.reverse,
        16 => reverse = target_bounds(s, p, pass.target).2,
        _ => {}
    }
    if pass.kind.0 == 5 && b(p, ReuseFlip) & channel_bit(s.readings.channel) != 0 {
        reverse = !reverse;
    }
    if matches!(pass.kind.0, 17 | 18) {
        let bounds = if effective.0 == 15 {
            s.bounds
        } else {
            channel_bounds(p, &s.readings, s.exposures.second.lens_step)
        };
        let mut temporary = Plan {
            kind: effective,
            ..Plan::default()
        };
        temporary.profile.reverse = true;
        temporary.profile.start = bounds.high;
        temporary.profile.end = bounds.low;
        let grid = alternate(s, p, &temporary);
        adjust_direction(s, p, effective, grid, bounds, &mut reverse);
        if b(p, ScanFlip) & channel_bit(s.readings.channel) != 0 {
            reverse = !reverse;
        }
    }
    pass.step = (pass.frame_travel / 10) as i16;
    let bounds = pass_bounds(s, p, pass, &mut reverse);
    let grid = alternate(s, p, pass);
    adjust_direction(s, p, pass.kind, grid, bounds, &mut reverse);
    match b(p, ForceDirection) {
        1 => reverse = true,
        2 => reverse = false,
        _ => {}
    }
    let high = p.side_adjusted(bounds.high, 1, u32::from(reverse));
    let low = p.side_adjusted(bounds.low, 1, u32::from(reverse));
    pass.profile.reverse = reverse;
    if reverse {
        pass.profile.start = high;
        pass.profile.end = low;
        pass.step = pass.step.wrapping_neg();
    } else {
        pass.profile.start = low;
        pass.profile.end = high;
    }
    let lead = lead(s, pass.speed);
    pass.profile.start = pass
        .profile
        .start
        .wrapping_add(if reverse {
            lead as i16
        } else {
            (lead as i16).wrapping_neg()
        })
        .wrapping_sub(pass.step);
    pass.profile.end = pass.profile.end.wrapping_add(pass.step);
    if pass.kind.0 == 11
        && b(p, TargetCurrentPosition) != 0
        && s.search.mode != SearchMode::Treated
        && (4..=6).contains(&s.readings.class)
        && s.readings.confirmation[0] == 1
        && s.readings.confirmation[1] != 0
    {
        pass.profile.start = p.position() as i16;
    }
    let margin = p.travel_margin() as i16;
    let [mut low, mut high] = p.position_limits(s.readings.lens);
    if pass.profile.start > pass.profile.end {
        high = high.wrapping_sub(margin);
    } else {
        low = low.wrapping_add(margin);
    }
    pass.profile.start = pass.profile.start.min(high).max(low);
    pass.profile.end = pass.profile.end.min(high).max(low);
    if !reverse && s.search.channel_class == ChannelClass::Wide {
        let bound = channel_bounds(p, &s.readings, s.exposures.second.lens_step).high;
        let bound = p.side_adjusted(bound, 1, 0);
        let [low, high] = p.position_limits(s.readings.lens);
        pass.near_bound = bound.min(high).max(low);
        if plan::kind_row(pass.kind)[5] == 1 {
            pass.profile.end = p
                .side_adjusted(s.bounds.high, 1, 0)
                .wrapping_add(pass.step)
                .min(high)
                .max(low);
            pass.decision = 1;
        } else if pass.kind.0 < 23 && (1u32 << pass.kind.0) & 0x40a800 != 0 {
            pass.decision = 1;
        }
    }
    lead
}

fn frame_range(s: &MotionState, p: &mut impl PlanPorts, pass: &mut Plan) {
    pass.last_frame = if pass.motion == Motion::Continuous || pass.step == 0 {
        250
    } else {
        let distance = (i32::from(pass.profile.end) - i32::from(pass.profile.start)).abs() as i16;
        let step = pass.step.wrapping_abs();
        let frames = (div(i32::from(step) + i32::from(distance) - 1, i32::from(step)) + 1) as i16;
        let frames = frames.min(251) as u16;
        if frames == 0 { 0 } else { frames - 1 }
    };
    let depth = (b(p, FirstFrameDepth) * 10) as u16 & 0xfffe;
    let minimum = p.depth_positions(s.readings.lens, s.exposures.second.lens_step, depth);
    for frame in 0..252u32 {
        let time = s
            .frame_period()
            .wrapping_mul(frame + 1)
            .wrapping_sub(pass.start_time as u32) as i32;
        let position = pass.profile.position_at(time);
        let travelled = (i32::from(pass.profile.start) - i32::from(position)).abs() as i16;
        if minimum < travelled {
            pass.first_frame = frame as u16;
            break;
        }
    }
}

fn weights(s: &MotionState, p: &mut impl PlanPorts, pass: &Plan) -> [u8; 3] {
    if b(p, FixedExposure) != 0 {
        return [
            b(p, FirstWeight),
            b(p, SecondWeight),
            b(
                p,
                if pass.layout == AreaLayout::GridAlternate {
                    AlternateWeight
                } else {
                    ThirdWeight
                },
            ),
        ]
        .map(|v| v as u8);
    }
    let second = &s.exposures.second;
    // The retained block's program table stays preparation-zeroed.
    let quarter = p.log2_difference(u32::from(second.frames) * 25, 100);
    let level = second.first_level.wrapping_add(quarter).max(0) as u32;
    let (weights, extra) = cfg::weights(
        p,
        second.program.index(),
        second.step,
        second.gain,
        pass.layout == AreaLayout::GridAlternate,
        level != 0,
    );
    core::array::from_fn(|i| {
        weights[i].wrapping_add(if level == 0 {
            0
        } else {
            level.wrapping_mul(extra[i]).wrapping_add(500) / 1000
        }) as u8
    })
}

pub(crate) fn scaled_threshold(
    coefficient: u32,
    change: u16,
    gain: u16,
    share: u32,
    depth: u32,
    floor: u32,
) -> u32 {
    let base = coefficient.wrapping_mul(depth) / 100;
    let base = (quotient(u64::from(base) * u64::from(share), 10_000) as u32).max(floor);
    u32::from(gain).wrapping_mul(u32::from(change).wrapping_mul(base) / 10) / 10
}

fn thresholds(s: &MotionState, p: &mut impl PlanPorts, pass: &Plan) -> ContrastThresholds {
    let mut out = ContrastThresholds::default();
    let depth = u32::from(pass.frame_depth).min(b(p, DepthCeiling) * 10);
    let depths = [depth, depth * 2, depth * 3];
    let change = s.exposures.second.change_scale;
    let gain = s.exposures.second.gain_scale;
    for row in 0..4 {
        let floor = b(p, if row < 2 { ContrastFloor } else { FocusFloor });
        let coefficients = cfg::threshold_coefficients(p, row);
        out.rows[row] = core::array::from_fn(|i| {
            scaled_threshold(
                u32::from(coefficients[i]),
                change,
                gain,
                pass.primary_share,
                depths[i],
                floor,
            )
        });
    }
    let floor = b(p, ContrastFloor);
    out.rows[4] = match pass.layout {
        AreaLayout::GridAlternate => {
            let c = cfg::threshold_coefficients(p, 4);
            core::array::from_fn(|i| {
                scaled_threshold(
                    u32::from(c[i]),
                    change,
                    gain,
                    pass.secondary_share,
                    depths[i],
                    floor,
                )
            })
        }
        AreaLayout::Grid => {
            let scale = b(p, GridThresholdScale) as u16;
            let row = usize::from(b(p, GridThresholdRow) == 1);
            core::array::from_fn(|i| {
                (quotient(
                    u64::from(out.rows[row][i])
                        .wrapping_mul(u64::from(pass.secondary_share))
                        .wrapping_mul(u64::from(scale)),
                    u64::from(pass.primary_share.wrapping_mul(10)),
                ) as u32)
                    .max(floor)
            })
        }
        _ => {
            let change = if b(p, ListedScales) & 4 == 0 {
                10
            } else {
                change
            };
            let gain = if b(p, ListedScales) & 2 == 0 {
                10
            } else {
                gain
            };
            let share = if b(p, ListedScales) & 1 == 0 {
                10_000
            } else {
                pass.secondary_share
            };
            let c = cfg::threshold_coefficients(p, 5);
            core::array::from_fn(|i| {
                scaled_threshold(u32::from(c[i]), change, gain, share, depths[i], floor)
            })
        }
    };
    out
}

fn window_flags(s: &MotionState, p: &mut impl PlanPorts, pass: &mut Plan) {
    pass.checks_peak = if !s.readings.load_test
        && b(p, Peak) != 0
        && matches!(s.search.mode, SearchMode::Plain | SearchMode::Area)
    {
        let (upper, lower) = if s.search.mode == SearchMode::Plain {
            (PlainPeakHigh, PlainPeakLow)
        } else {
            (AreaPeakHigh, AreaPeakLow)
        };
        let high = w(p, upper) as i16;
        let low = w(p, lower) as i16;
        s.exposures.second.level <= high && s.exposures.second.level >= low
    } else {
        false
    };
    pass.channel_window = b(p, Plain) & 1 == 0
        && !s.readings.load_test
        && if s.readings.scene == 50 {
            true
        } else if matches!(pass.kind.0, 17 | 18) && pass.layout != AreaLayout::Grid {
            false
        } else if s.search.mode == SearchMode::Treated {
            treated_window(s, b(p, TreatedChannelWindow))
        } else {
            b(p, ChannelWindow) & if s.readings.channel == 0 { 1 } else { 2 } != 0
        };
    pass.window_enabled = true;
    pass.below_level = s.search.mode != SearchMode::Treated
        && i32::from(s.exposures.second.level) <= ((b(p, BelowLevel) * 5) as i16 as i32) * 2;
    pass.untreated_window = s.search.mode != SearchMode::Treated;
    pass.single_cell_window = pass.layout == AreaLayout::GridAlternate;
    pass.checks_speed &= pass.single_cell_window;
}

impl MotionState {
    /// Keeps the installed pass when settled; otherwise replaces it with the next pass.
    pub fn plan_next(&mut self, p: &mut impl PlanPorts) -> PlanKind {
        let mut pass = Plan::default();
        if self.plan.kind == PlanKind::FIRST {
            plan_policy::first(self, p, &mut pass);
        } else {
            pass.kind = plan_policy::next(self, p);
            pass.decided = pass.kind;
        }
        scan_kind(self, p, &mut pass);
        if pass.kind == PlanKind::SETTLED {
            plan_result::settle(self, p);
            return PlanKind::SETTLED;
        }
        let mask = if self.readings.load_test { 2 } else { 1 };
        pass.motion = if b(p, DriveMotion) & mask != 0 {
            Motion::Stepped
        } else {
            Motion::Continuous
        };
        pass.reset_results();
        speeds(self, p, &mut pass);
        self.depth_settings = cfg::depth_settings(p, self.plan.frame_depth > 150);
        trace::depth(p, self.section, self.depth_settings);
        let lead = drive_bounds(self, p, &mut pass);
        trace::store(
            p,
            self.section,
            Trace::Start,
            pass.profile.start as i32 as u32,
        );
        trace::store(p, self.section, Trace::End, pass.profile.end as i32 as u32);
        trace::store(p, self.section, Trace::Lead, u32::from(lead));
        pass.start_time = u32::from(w(p, LensStartDelay))
            .wrapping_add(b(p, StartDelay))
            .wrapping_mul(100) as i32;
        let planned = pass.profile.plan(pass.speed, self.readings.drive_kind, p);
        trace::drive(p, self.section, pass.start_time, planned);
        frame_range(self, p, &mut pass);
        pass.layout = if self.search.mode == SearchMode::Treated {
            AreaLayout::Listed
        } else if pass.kind.0 == 11
            || (plan::kind_row(pass.kind)[4] == 1
                && self.search.channel_class == ChannelClass::Wide)
            || pass.profile.reverse
            || !alternate(self, p, &pass)
        {
            AreaLayout::GridAlternate
        } else {
            AreaLayout::Grid
        };
        trace::store(p, self.section, Trace::Layout, pass.layout.code());
        self.layout.grids[1] = secondary_grid(
            p,
            pass.layout,
            self.exposures.second.profile,
            self.layout.grids[0],
            self.layout.grids[1],
        );
        let reference = self.search.frames[0]
            .cell_height
            .wrapping_mul(self.search.frames[0].cell_width);
        let share = |grid: crate::adjust::motion::ports::AreaGrid| {
            quotient(
                u64::from(grid.cell_height)
                    .wrapping_mul(u64::from(grid.cell_width))
                    .wrapping_mul(10_000),
                u64::from(reference),
            ) as u32
        };
        pass.primary_share = share(self.layout.grids[0]);
        pass.secondary_share = if pass.layout == AreaLayout::Single {
            10_000
        } else {
            share(self.layout.grids[1])
        };
        trace::store(
            p,
            self.section,
            Trace::SecondaryAreaShare,
            pass.secondary_share,
        );
        trace::store(p, self.section, Trace::PrimaryShare, pass.primary_share);
        trace::store(p, self.section, Trace::SecondaryShare, pass.secondary_share);
        pass.weights = weights(self, p, &pass);
        trace::store(
            p,
            self.section,
            Trace::Weights,
            u32::from(pass.weights[0])
                | (u32::from(pass.weights[1]) << 8)
                | (u32::from(pass.weights[2]) << 16),
        );
        self.thresholds = thresholds(self, p, &pass);
        window_flags(self, p, &mut pass);
        pass.selection = u32::from(pass.speed_choice);
        let kind = pass.kind;
        self.plan = pass;
        kind
    }
}

pub(crate) fn area(s: &MotionState) -> u32 {
    signed_area(s)
}
