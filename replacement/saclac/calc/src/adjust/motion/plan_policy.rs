use crate::adjust::motion::plan::{AreaLayout, Plan, PlanKind, StartTarget};
use crate::adjust::motion::plan_geometry::{
    self as geometry, current, depth, symmetric, target_bounds, treated_window,
};
use crate::adjust::motion::plan_ports::{PlanPorts, PositionRemap};
use crate::adjust::motion::preparation::PreviousFocus;
use crate::adjust::motion::prepare::channel_bounds;
use crate::adjust::motion::search::{ChannelClass, SearchMode, contrast_reaches};
use crate::adjust::motion::state::MotionState;
use crate::exposure::settings;
use crate::stock::plan;
use crate::stock::plan_config::{self as cfg, Key::*, byte as b, word as w};
use crate::stock::plan_trace::{self as trace, Trace};

fn reuse_reason(s: &MotionState, p: &mut impl PlanPorts) -> u32 {
    let r = &s.readings;
    if b(p, Reuse) != 1 {
        return 15;
    }
    if r.request.at_04 == 1 {
        return 11;
    }
    if r.load_test || r.scene == 50 {
        return 1;
    }
    if r.class <= 6 && (1u32 << r.class) & 0x71 != 0 {
        return 1;
    }
    let flags = b(
        p,
        if r.channel == 0 {
            ReusePlain
        } else {
            ReuseChannels
        },
    );
    if (s.search.mode == SearchMode::Plain && flags & 1 == 0)
        || (s.search.mode == SearchMode::Area && flags & 2 == 0)
        || !matches!(s.search.mode, SearchMode::Plain | SearchMode::Area)
    {
        return 2;
    }
    if (r.measurement_selection == xm1_saclac_profile_records::MeasurementSelection::Record
        && flags & 4 == 0)
        || (r.at_005b == 1 && flags & 8 == 0)
        || (r.request.at_00 & 1 != 0 && flags & 32 == 0)
    {
        return 1;
    }
    if r.request.at_00 & 2 != 0 {
        return 3;
    }
    if r.request.at_00 & 8 != 0 {
        return 5;
    }
    let choice = b(p, ReuseLevel);
    let (level, reference) = match choice {
        1 => (r.levels[2], r.request.positions[1]),
        2 => (r.levels[0], r.request.positions[2]),
        3 => (r.levels[3], r.request.positions[3]),
        _ => (r.levels[1], r.request.positions[0]),
    };
    let level_offset = settings::level_step(p, 3);
    let reference_offset = settings::level_step(p, 3);
    let reference = reference.wrapping_add(reference_offset);
    let below = b(p, ReuseLevelBelow) as i16;
    let above = b(p, ReuseLevelAbove) as i16;
    trace::store(p, s.section, Trace::ReuseLevelKind, choice);
    trace::store(p, s.section, Trace::ReuseReference, reference as i32 as u32);
    let level = level.wrapping_add(level_offset);
    trace::store(p, s.section, Trace::ReuseLevel, level as i32 as u32);
    if level >= reference.wrapping_add(above.wrapping_mul(10))
        || level <= reference.wrapping_sub(below.wrapping_mul(10))
    {
        return 6;
    }
    let reference = r.request.positions[0].wrapping_add(settings::level_step(p, 2));
    let level = r.levels[1].wrapping_add(settings::level_step(p, 2));
    let below = b(p, ReuseStepBelow) as i16;
    let above = b(p, ReuseStepAbove) as i16;
    trace::store(p, s.section, Trace::ReuseStep, reference as i32 as u32);
    if level >= reference.wrapping_add(above.wrapping_mul(10))
        || level <= reference.wrapping_sub(below.wrapping_mul(10))
    {
        return 7;
    }
    let level = s
        .exposures
        .second
        .base_level
        .wrapping_add(settings::level_step(p, 2));
    if i32::from(level) <= (b(p, ReuseMinimumLevel) * 10) as i32 {
        return 8;
    }
    if r.request.at_00 & 16 != 0 {
        return 9;
    }
    let position = p.remap_position(PositionRemap {
        position: r.request.position,
        lens: r.lens,
        calibration: r.request.at_10,
        step: s.exposures.second.lens_step,
        side: r.request.side,
    });
    if position < s.bounds.low
        || position > s.bounds.high
        || ((position == s.bounds.low || position == s.bounds.high)
            && position != s.layout.start_position)
    {
        return 14;
    }
    if s.search.channel_status {
        return 12;
    }
    if r.request.at_04 == 2 {
        return 13;
    }
    0
}

pub(crate) fn first(s: &MotionState, p: &mut impl PlanPorts, pass: &mut Plan) {
    let r = &s.readings;
    let plain = b(p, Plain) & 1 != 0;
    let base = if plain || r.load_test {
        1
    } else if s.previous == PreviousFocus::Admitted {
        2
    } else if s.search.mode == SearchMode::Treated {
        if r.scene == 50 || !treated_window(s, b(p, TreatedFirst)) {
            7
        } else {
            let limits = p.search_bounds(&geometry::query(s, r.channel, r.lens_state));
            let depth = depth(s, p, PlanKind(8), 1, r.load_test);
            let travel = p
                .depth_positions(r.lens, s.exposures.second.lens_step, depth)
                .wrapping_mul(10) as u16
                & 0xfffe;
            let window = symmetric(
                s,
                p,
                s.layout.treated_position,
                travel,
                cfg::Window::Treated,
            );
            if limits.low < window.high
                && window.low < limits.high
                && (limits.low < window.low || window.high < limits.high)
            {
                8
            } else {
                7
            }
        }
    } else {
        1
    };
    pass.decided = PlanKind(base);
    pass.kind = PlanKind(base);
    if !plain {
        let reason = reuse_reason(s, p);
        if reason == 0 {
            pass.kind = PlanKind(5);
        } else {
            trace::store(p, s.section, Trace::ReuseReason, reason);
        }
    }
    trace::target(p, s.results, 5);
    let mut source = match pass.kind.0 {
        5 => 0,
        2 if r.confirmation[1] == 0 => 2,
        8 => 4,
        _ => 5,
    };
    if source < 4 {
        if r.channel != 0 {
            source += 1;
        } else {
            let bounds = channel_bounds(p, r, s.exposures.second.lens_step);
            if current(s, p) > bounds.high {
                source += 1;
            }
        }
    }
    let mode = if source > 4 {
        0
    } else {
        cfg::target_mode(p, source).min(if source == 4 { 3 } else { 2 })
    };
    let position = if mode == 2 {
        match source {
            0 | 1 => p.remap_position(PositionRemap {
                position: r.request.position,
                lens: r.lens,
                calibration: r.request.at_10,
                step: s.exposures.second.lens_step,
                side: r.request.side,
            }),
            2 | 3 => p.remap_position(PositionRemap {
                position: r.confirmation[2] as i16,
                lens: r.lens,
                calibration: r.confirmation[4],
                step: s.exposures.second.lens_step,
                side: r.confirmation[5],
            }),
            4 => s.layout.treated_position,
            _ => current(s, p),
        }
    } else {
        current(s, p)
    };
    let direction = if mode == 3 {
        u32::from(current(s, p) >= s.layout.treated_position)
    } else if source < 6 {
        let configured = (b(p, TargetDirections) >> source) & 1;
        if (4..=6).contains(&r.class)
            && r.confirmation[0] != 0
            && r.confirmation[1] != 0
            && r.confirmation[7] < 2
        {
            r.confirmation[7]
        } else {
            configured
        }
    } else {
        0
    };
    pass.target = StartTarget {
        source,
        position,
        direction,
    };
    trace::target_details(
        p,
        s.results,
        [source, mode, position as i32 as u32, direction],
    );
    if mode == 1 {
        trace::target(p, s.results, 1);
        return;
    }
    if b(p, TargetEnabled) == 0 {
        trace::target(p, s.results, 0);
        pass.kind = PlanKind(1);
        return;
    }
    let channel_allowed = r.channel == 0 || source > 5 || b(p, TargetChannels) >> source & 1 != 0;
    if !channel_allowed || s.search.mode == SearchMode::Channel || r.scene == 50 || r.load_test {
        trace::target(p, s.results, 1);
        pass.kind = PlanKind(1);
        return;
    }
    if s.search.mode != SearchMode::Treated && r.class != 0 {
        let scale = u32::from(w(p, TargetContrast));
        if contrast_reaches(p, s.results, s.search.area, s.search.mode, scale) != Some(true) {
            trace::target(p, s.results, 2);
            pass.kind = PlanKind(1);
            return;
        }
    }
    if b(p, TargetPastChannel) == 0 {
        let bounds = channel_bounds(p, r, s.exposures.second.lens_step);
        if bounds.high < current(s, p) {
            trace::target(p, s.results, 3);
            pass.kind = PlanKind(15);
            return;
        }
    }
    if target_bounds(s, p, pass.target).0 {
        pass.kind = PlanKind(11);
    } else {
        trace::target(p, s.results, 4);
        let bounds = channel_bounds(p, r, s.exposures.second.lens_step);
        pass.kind = PlanKind(if bounds.high < current(s, p) { 15 } else { 16 });
    }
}

fn wide_status(s: &MotionState, p: &mut impl PlanPorts) -> u32 {
    let pass = &s.plan;
    let result = &pass.result;
    if pass.decision != 0 {
        return 6;
    }
    if s.search.channel_class != ChannelClass::Wide {
        return s.search.channel_class.code();
    }
    if result.kind == 0 && result.pending == 0 {
        return 6;
    }
    if !pass.profile.reverse {
        return 4;
    }
    let scanned = pass.tone.words[1];
    let fallback = |s: &MotionState| {
        if result.kind == 1 && result.pending == 0 {
            9
        } else if s.readings.armed == 1
            || (scanned == 1 && result.pending == 1)
            || (scanned != 1 && pass.tone.words[9] == 1 && pass.tone.words[10] == 1)
        {
            12
        } else {
            7
        }
    };
    if scanned == 0 && !s.peak_reached(p) {
        return fallback(s);
    }
    if b(p, NextPeak) != 0 && pass.tone.words[12] == 1 {
        return 3;
    }
    if scanned == 1 {
        if result.retry_requested == 1 {
            return 12;
        }
        if b(p, NextNear) & 2 == 0 {
            return 8;
        }
    }
    fallback(s)
}

pub(crate) fn next(s: &MotionState, p: &mut impl PlanPorts) -> PlanKind {
    if p.control_state() == 2 {
        return PlanKind::SETTLED;
    }
    let pass = &s.plan;
    let r = &pass.result;
    let success = r.kind < 2 && r.pending == 0;
    let mut next = match pass.kind.0 {
        2 => {
            if success {
                3
            } else {
                1
            }
        }
        5 => {
            if success {
                3
            } else {
                pass.decided.0
            }
        }
        8 => {
            if !treated_window(s, b(p, TreatedNext))
                || success
                || (r.kind < 2 && r.treated_pending != 0)
            {
                3
            } else {
                7
            }
        }
        9 => {
            if success {
                3
            } else {
                27
            }
        }
        10 | 12 | 13 | 15 | 19 | 20 | 22..=27 => 3,
        11 => {
            if r.retry_requested == 1 {
                12
            } else if success {
                3
            } else if pass.tone.words[1] == 1
                || pass.tone_limits[1] != 1
                || pass.tone.words[10] != 1
            {
                let travel = if pass.profile.reverse {
                    s.bounds.high.wrapping_sub(pass.profile.start)
                } else {
                    pass.profile.start.wrapping_sub(s.bounds.low)
                };
                let depth = w(p, NextTravel).wrapping_mul(10) & 0xfffe;
                if travel < p.depth_positions(s.readings.lens, s.exposures.second.lens_step, depth)
                {
                    3
                } else {
                    13
                }
            } else {
                12
            }
        }
        17 | 18 => {
            let kind = if r.kind == 0 {
                pass.kind.0 + 2
            } else if r.kind == 1 || b(p, ScanMissing) != 0 {
                pass.kind.0 + 4
            } else {
                3
            };
            trace::store(p, s.section, Trace::ScanFollow, 1);
            trace::store(p, s.section, Trace::ScanResult, r.kind);
            kind
        }
        _ => {
            let wide = wide_status(s, p);
            trace::store(p, s.section, Trace::NextWide, wide);
            if trace::load(p, s.section, Trace::NextWide) == 12 {
                if s.readings.scene == 50 { 10 } else { 9 }
            } else {
                3
            }
        }
    };
    if pass.checks_speed && r.kind < 2 && r.pending != 1 && next == 3 && r.fast == 1 {
        next = 24;
    } else if b(p, NextMissing) != 0
        && pass.checks_speed
        && r.kind >= 2
        && next == 3
        && s.search.retry_missing
    {
        next = 26;
    } else if b(p, NextArea) != 0
        && r.kind <= 1
        && r.decision == 1
        && next == 3
        && !s.readings.load_test
        && s.readings.scene != 50
        && s.search.mode != SearchMode::Treated
        && r.class != 10
        && (pass.checks_speed || pass.speed_choice != 0)
        && !(pass.layout == AreaLayout::Grid && (62..71).contains(&r.area))
        && plan::kind_row(pass.kind)[1] != 0
    {
        next = 23;
    } else if next == 3 {
        if !s.readings.load_test && pass.kind.0 != 25 && r.kind < 2 && r.area_status == 1 {
            next = 25;
        } else {
            return PlanKind::SETTLED;
        }
    }
    trace::history(p, s);
    PlanKind(next)
}
