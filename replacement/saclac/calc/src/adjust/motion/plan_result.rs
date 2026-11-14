use crate::adjust::motion::plan_geometry::treated_window;
use crate::adjust::motion::plan_ports::PlanPorts;
use crate::adjust::motion::planning::area;
use crate::adjust::motion::preparation::PreviousFocus;
use crate::adjust::motion::search::SearchMode;
use crate::adjust::motion::state::MotionState;
use crate::stock::plan_config::{Key::*, byte as b, word as w};
use crate::stock::plan_trace::{self as trace, Trace};
use xm1_saclac_profile_records::stock::shared::group_17;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusPublication {
    pub enabled: u32,
    pub status: u32,
    pub snapshot: [u32; 4],
    pub pending: u32,
}

fn set_position(snapshot: &mut [u32; 4], position: u16) {
    snapshot[0] = (snapshot[0] & 0xffff0000) | u32::from(position);
}

impl MotionState {
    /// Publishes the admitted confirmation or designation without a new search.
    pub fn publish_existing(&mut self, p: &mut impl PlanPorts, output: &mut FocusPublication) {
        self.plan.result.pending = 0;
        self.plan.result.area = area(self);
        self.plan.result.diagnostic_status = [3, 0];
        self.plan.result.treated_pending = 0;
        self.plan.flags.side = 2;
        output.pending = 0;
        output.enabled = 1;
        let (reason, kind) = if b(p, NoSearch) == 1 || p.control_state() == 2 {
            self.plan.result.kind = 2;
            self.plan.result.position = 0;
            self.plan.flags.found = false;
            self.plan.flags.searching = true;
            output.status = 2;
            set_position(&mut output.snapshot, 0);
            output.snapshot[1] = 0;
            output.snapshot[2] = 0x80;
            output.snapshot[3] = 2;
            (61, 6)
        } else if self.readings.scene == 24 {
            self.plan.result.kind = 0;
            self.plan.flags.found = true;
            self.plan.flags.searching = false;
            let mode = p.snapshot_mode();
            let position =
                p.channel_positions(self.readings.lens, 0, self.readings.reference, mode)[0] as u16;
            self.plan.result.position = position;
            output.status = 0;
            set_position(&mut output.snapshot, position);
            output.snapshot[1] = self.readings.lens;
            output.snapshot[2] = 0;
            output.snapshot[3] = 1;
            self.plan.flags.side = 1;
            (59, 6)
        } else if self.readings.designation[0] == 1 {
            let designation = self.readings.designation;
            self.plan.result.kind = 0;
            self.plan.result.position = designation[3] as u16;
            self.plan.result.area = designation[2].wrapping_mul(7).wrapping_add(designation[1]);
            self.plan.flags.found = true;
            self.plan.flags.searching = false;
            output.status = 0;
            output.snapshot.copy_from_slice(&designation[3..7]);
            (60, 6)
        } else if let Some(snapshot) = p.cached_focus() {
            self.plan.result.kind = 0;
            self.plan.result.position = self.readings.confirmation[2] as u16;
            self.plan.flags.found = true;
            self.plan.flags.searching = false;
            output.status = 0;
            output.pending = u32::from(b(p, CachedPending) == 1);
            output.snapshot = snapshot;
            self.plan.flags.side = snapshot[3];
            (58, 4)
        } else {
            let found = self.previous == PreviousFocus::Reported;
            self.plan.result.kind = if found { 0 } else { 2 };
            self.plan.result.position = self.readings.confirmation[2] as u16;
            self.plan.flags.found = found;
            self.plan.flags.searching = !found;
            output.status = if found { 0 } else { 2 };
            output
                .snapshot
                .copy_from_slice(&self.readings.confirmation[2..6]);
            output.snapshot[3] = 2;
            (58, 4)
        };
        trace::store(p, self.section, Trace::Area, reason);
        trace::store(p, self.section, Trace::Kind, kind);
        trace::store(
            p,
            self.section,
            Trace::Found,
            u32::from(self.plan.flags.found),
        );
        trace::store(
            p,
            self.section,
            Trace::Position,
            self.plan.result.position as i16 as i32 as u32,
        );
        for (field, value) in [
            (Trace::FocusStatus, 3),
            (Trace::MotionStatus, 4),
            (Trace::Class, 0),
            (Trace::Tone, 3),
        ] {
            trace::store(p, self.section, field, value);
        }
    }

    pub fn publish_fallback(&mut self, p: &mut impl PlanPorts, output: &mut FocusPublication) {
        let result = &mut self.plan.result;
        result.kind = 2;
        result.position = 0;
        result.diagnostic_status = [4, 3];
        result.pending = 0;
        result.treated_pending = 0;
        self.plan.result.area = area(self);
        self.plan.flags.found = false;
        self.plan.flags.searching = true;
        self.plan.flags.side = 2;
        output.enabled = 1;
        output.status = 2;
        output.pending = 0;
        let snapshot: [u8; 48] = p
            .profile()
            .read_shared(self.readings.record, &group_17::LENS_SNAPSHOT);
        output.snapshot = core::array::from_fn(|i| {
            u32::from_le_bytes([
                snapshot[i * 4],
                snapshot[i * 4 + 1],
                snapshot[i * 4 + 2],
                snapshot[i * 4 + 3],
            ])
        });
        output.snapshot[2] = 0x80;
    }
}

pub(crate) fn settle(s: &mut MotionState, p: &mut impl PlanPorts) {
    let old_kind = s.plan.kind;
    let debug = b(p, Debug) == 1;
    let mut text = [*b"--"; 3];
    s.plan.flags.side = u32::from(s.plan.profile.reverse);
    trace::store(p, s.section, Trace::Area, s.plan.result.area);
    if p.control_state() == 2 {
        s.plan.result.kind = 2;
    }
    if s.plan.result.kind >= 2 {
        s.plan.result.area = area(s);
        s.plan.result.kind = 2;
        s.plan.result.pending = 0;
        s.plan.result.treated_pending = 0;
        s.plan.flags.found = false;
        s.plan.flags.searching = true;
        s.plan.flags.side = 1;
        s.plan.result.position = s.layout.treated_position as u16;
    } else {
        if s.readings.load_test || s.search.mode != SearchMode::Channel || s.plan.result.area > 48 {
            s.plan.result.area = area(s);
        }
        let treated_wait = s.search.mode == SearchMode::Treated
            && trace::load(p, s.section, Trace::Area) != 80
            && !treated_window(s, b(p, TreatedSettled));
        let pending = treated_wait
            || s.plan.result.pending != 0
            || (s.search.mode == SearchMode::Treated && s.plan.result.treated_pending != 0);
        if pending {
            if s.readings.scene == 50 {
                s.plan.result.area_count = 1;
            }
            s.plan.flags.searching = true;
            s.plan.flags.found = false;
            if s.search.mode == SearchMode::Treated && s.plan.result.treated_pending != 0 {
                s.plan.flags.side = 1;
                s.plan.result.position = s.layout.treated_position as u16;
            }
        } else {
            s.plan.flags.searching = false;
            s.plan.flags.found = s.plan.result.kind != 1 || s.plan.result.class == 9;
        }
        if s.search.mode == SearchMode::Area {
            trace::store(p, s.section, Trace::Area, s.plan.result.area);
        }
        if debug && s.readings.scene == 50 {
            for (i, slot) in text.iter_mut().enumerate() {
                if i == 0 || usize::from(s.plan.result.area_count) > i {
                    let value = s.plan.result.areas[i].index;
                    if value < 100 {
                        *slot = [b'0' + (value / 10) as u8, b'0' + (value % 10) as u8];
                    }
                }
            }
        }
    }
    if debug {
        if s.readings.scene == 50 {
            for (i, line) in text.iter().enumerate() {
                p.text(line, 5, 7 + i as u8, 1, 7);
            }
        } else {
            p.release_aux_text();
        }
    }
    if b(p, Override) & 2 != 0 {
        s.plan.result.position = w(p, OverridePosition);
        s.plan.flags.side = u32::from(s.plan.profile.reverse);
    }
    let pass = &s.plan;
    let values = [
        (Trace::Class, pass.result.class),
        (Trace::Found, u32::from(pass.flags.found)),
        (Trace::Searching, u32::from(pass.flags.searching)),
        (Trace::Kind, u32::from(old_kind.0)),
        (Trace::FrameDelay, u32::from(s.search.frame_delay)),
        (Trace::Position, pass.result.position as i16 as i32 as u32),
        (Trace::FocusStatus, pass.result.diagnostic_status[1]),
        (Trace::MotionStatus, pass.result.diagnostic_status[0]),
        (Trace::ToneScanned, pass.tone.words[1]),
        (Trace::Tone, pass.tone.words[0]),
        (Trace::ToneLow, pass.tone_limits[0]),
        (Trace::ToneHigh, pass.tone_limits[1]),
        (Trace::ToneFirst, pass.tone.words[11]),
        (Trace::ToneSecond, pass.tone.words[12]),
        (Trace::ToneDriven, pass.tone.words[13]),
        (Trace::ToneHeld, pass.tone.words[14]),
        (Trace::ResultDecision, pass.result.decision),
        (Trace::Fast, pass.result.fast),
        (Trace::Decision, pass.decision),
    ];
    for (field, value) in values {
        trace::store(p, s.section, field, value);
    }
}
