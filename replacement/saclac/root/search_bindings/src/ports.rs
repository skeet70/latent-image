#![forbid(unsafe_code)]

use super::{
    calc::{self, Tables},
    stock::{self, buffer, call, out, signed, w, Area},
    Session,
};
use xm1_saclac_calc::adjust::configuration::FrameInterval;
use xm1_saclac_calc::adjust::{
    drive_step,
    motion::drive::DriveKind,
    peak::{self, PeakCurve},
    scan_drive, search_targets,
};
use xm1_saclac_calc::profile::selection_code;
use xm1_saclac_config::{
    channel, limits,
    ports::RangeQuery,
    table::{self, Row, Step as TableStep},
};
use xm1_saclac_controllers::search::{Direction, LensPosition};
use xm1_saclac_focus_search::{
    contrast::settings::{Setting, StepKind},
    contrast::*,
    intake::{FrameSnapshot, IntakePorts, IntakeRecord, ProgramInputs, RecordReads, Zone},
    live,
    requests::{BridgeMotion, Request},
    s1::{S1Ports, S1Record, S1Snapshot},
};
use xm1_saclac_interface::search::{BridgeRequest, ResultAddress, S1DriveRequest, SearchRecord};
use xm1_saclac_interface::stock::{self as interface, record_handle_word};
use xm1_saclac_interface::{
    Calc, CalcRequest, Completion, ImgDc, ImgDcOperation, ImgDcRequest, RecordHandle,
};
use xm1_saclac_profile_records::field::{
    RecordField, RecordRead, SharedField, SharedRead, SharedWrite, Value,
};
use xm1_saclac_profile_records::stock::{
    record::{group_00, group_13},
    shared::{
        group_15, group_16, group_19, group_20, group_26, group_27, group_33, group_37, group_41,
    },
};
use xm1_saclac_profile_records::{Memory, SharedHeaderByte, StatisticsInUse, StockEntries};
use xm1_saclac_shared::settings::Channel;
use xm1_saclac_shared::stock::Machine;

impl<M: Machine, R: Memory, E: StockEntries, P> Session<'_, '_, M, R, E, P> {
    fn handle(&self) -> RecordHandle {
        self.record.0.get()
    }
    fn header(&self, byte: SharedHeaderByte) -> u8 {
        self.bindings
            .borrow_mut()
            .entries
            .header_byte(self.handle(), byte)
    }
    fn field<T: Value + Default, W>(&self, field: &RecordField<T, RecordRead, W>) -> T {
        self.bindings
            .borrow()
            .records
            .read(self.handle(), field)
            .unwrap_or_default()
    }
    fn shared<T: Value + Default, W>(&self, field: &SharedField<T, SharedRead, W>) -> T {
        self.bindings
            .borrow()
            .records
            .read_shared(self.handle(), field)
            .unwrap_or_default()
    }
    fn bytes<const N: usize, W>(&self, field: &SharedField<[u8; N], SharedRead, W>) -> [u8; N] {
        self.bindings
            .borrow()
            .records
            .read_shared(self.handle(), field)
            .unwrap_or([0; N])
    }
    fn store<T: Value, A>(&self, field: &SharedField<T, A, SharedWrite>, value: T) {
        let _ = self
            .bindings
            .borrow_mut()
            .records
            .write_shared(self.handle(), field, value);
    }
    fn enter(&self, entry: stock::Entry, args: &[u32]) -> u32 {
        call(&*self.bindings.borrow().machine, entry, args)
    }
    fn snapshot(&self) -> [u8; xm1_saclac_focus_search::stock::intake::FRAME_SNAPSHOT_BYTES] {
        let mut bytes = [0; xm1_saclac_focus_search::stock::intake::FRAME_SNAPSHOT_BYTES];
        buffer(
            &*self.bindings.borrow().machine,
            stock::SNAPSHOT,
            &[w(1), out(0)],
            &mut bytes,
        );
        bytes
    }
    fn tracing_bit(&self, mask: u16) -> bool {
        stock::trace_flags(&Area(&*self.bindings.borrow().machine)) & mask == mask
    }
}

impl<M: Machine, R: Memory, E: StockEntries, P> Session<'_, '_, M, R, E, P> {
    /// Preserve typed terminal outcomes for the root router.
    pub fn handle_live<S: live::LiveStrategy>(
        &mut self,
        search: &mut live::LiveSearch<S>,
        event: live::LaneEvent<S::Evaluation>,
    ) -> live::LaneStatus
    where
        Self: live::Ports<S::Record, S::Drive>,
    {
        search.handle(event, self)
    }

    /// Decode the two completions the round forwards to the live search.
    pub fn live_completion(
        &self,
        completed: xm1_saclac_interface::Completed<'_>,
    ) -> Option<live::LaneEvent<xm1_saclac_interface::search::Evaluation>> {
        let bytes = completed.parameters.bytes();
        match completed.completion {
            Completion::Calculation89 => {
                xm1_saclac_focus_search::stock::evaluation::evaluated(bytes, |address| {
                    stock::read(&*self.bindings.borrow().machine, address.0)
                })
                .map(live::LaneEvent::Evaluated)
            }
            Completion::Image46 => {
                interface::search::bridge_report(bytes).map(live::LaneEvent::LensReported)
            }
            _ => None,
        }
    }
}

impl<M: Machine, R: Memory, E: StockEntries, P> Platform for Session<'_, '_, M, R, E, P> {
    fn lens_position(&mut self) -> LensPosition {
        LensPosition(self.enter(stock::LENS_POSITION, &[]) as i16)
    }
    fn lens_identity(&mut self) -> LensIdentity {
        LensIdentity(self.enter(stock::LENS_IDENTITY, &[]) as i32)
    }
    fn table_count(&mut self) -> u32 {
        stock::table_count(&Area(&*self.bindings.borrow().machine))
    }
    fn limit_kind(&mut self) -> LimitKind {
        LimitKind(u32::from(self.header(SharedHeaderByte::At46)))
    }
    fn pinned_table(&mut self) -> bool {
        self.header(SharedHeaderByte::At63) == 1
    }
    fn table_position(&mut self) -> u16 {
        if self.header(SharedHeaderByte::At63) != 1 {
            return u16::from(self.header(SharedHeaderByte::At31));
        }
        if Platform::measurement_selection(self) == 1 {
            self.shared(&group_27::AT_073C) as u16
        } else {
            0
        }
    }
    fn lens_flags(&mut self) -> u32 {
        self.enter(stock::LENS_FLAGS, &[])
    }
    fn record_auxiliary(&mut self) -> u32 {
        u32::from(self.header(SharedHeaderByte::At61))
    }
    fn lens_state(&mut self) -> u32 {
        self.enter(stock::LENS_STATE, &[])
    }
    fn lens_bias(&mut self, lens: LensIdentity) -> u16 {
        channel::bias(&Area(&*self.bindings.borrow().machine), Channel(lens.0), 0) as u16
    }
    fn outer_limits(&mut self, query: LimitQuery) -> (LensPosition, LensPosition) {
        let mut bytes = [0; 4];
        let (kind, class) = query.table.map_or((0, 0), |kind| (kind.0, 6));
        buffer(
            &*self.bindings.borrow().machine,
            stock::OUTER_LIMITS,
            &[
                w(query.lens.0 as u32),
                w(0),
                w(u32::from(query.position)),
                w(kind),
                w(class),
                out(0),
                out(2),
                w(1),
                w(0),
                w(query.flags),
                w(query.auxiliary),
                w(query.state),
            ],
            &mut bytes,
        );
        (
            LensPosition(stock::half(&bytes, 0) as i16),
            LensPosition(stock::half(&bytes, 2) as i16),
        )
    }
    fn inner_limits(&mut self, query: LimitQuery) -> (LensPosition, LensPosition) {
        let mut bytes = [0; 4];
        let b = self.bindings.borrow();
        let (kind, class) = query.table.map_or((0, 0), |kind| (kind.0, 6));
        let reference = call(&*b.machine, stock::LENS_REFERENCE, &[]);
        let mut ports = calc::Derivations {
            machine: &*b.machine,
            range: Some(&mut bytes),
        };
        limits::lens_range(
            &mut ports,
            RangeQuery([
                query.lens.0 as u32,
                0,
                u32::from(query.position),
                kind,
                class,
                reference,
                0,
                0,
                1,
                0,
                query.flags,
                0,
                query.auxiliary,
                query.state,
            ]),
        );
        (
            LensPosition(stock::half(&bytes, 0) as i16),
            LensPosition(stock::half(&bytes, 2) as i16),
        )
    }
    fn table_offset(&mut self, lens: LensIdentity, kind: TableOffset) -> u16 {
        stock::table_offset(&Area(&*self.bindings.borrow().machine), lens.0, kind)
    }
    fn focus_offer(&mut self) -> FocusOffer {
        FocusOffer(self.shared(&group_20::AT_0140))
    }
    fn claim_focus_offer(&mut self) {
        self.store(&group_20::AT_0140, 2);
    }
    fn retained_focus(&mut self) -> RetainedFocus {
        xm1_saclac_focus_search::stock::retained_focus(&self.bytes(&group_19::AT_0120))
    }
    fn measurement_selection(&mut self) -> u8 {
        selection_code(
            self.bindings
                .borrow_mut()
                .entries
                .measurement_selection(self.handle()),
        )
    }
    fn record_selected(&mut self) -> u32 {
        u32::from(self.field(&group_00::AT_0051))
    }
    fn record_profile(&mut self) -> u32 {
        self.shared(&group_27::AT_0708)
    }
    fn publish_status(&mut self, status: FocusStatus) {
        self.store(
            &group_15::FOCUS_CONFIRMATION,
            xm1_saclac_focus_search::stock::focus_status(status),
        );
    }
    fn store_stage(&mut self, stage: StageRecord) {
        self.store(
            &group_15::AT_074C,
            xm1_saclac_focus_search::stock::stage_record(stage),
        );
    }
    fn claim_image_statistics(&mut self) {
        let mut in_use: StatisticsInUse = self.shared(&group_20::STATISTICS_IN_USE);
        in_use.search = true;
        self.store(&group_20::STATISTICS_IN_USE, in_use);
    }
    fn reset_focus_log(&mut self) {
        self.enter(stock::RESET_LOG, &[]);
    }
    fn tracing(&mut self) -> bool {
        self.tracing_bit(0x10)
    }
    fn trace_begin(&mut self, retained: LensPosition, focus: LensPosition) {
        stock::trace_values(
            &*self.bindings.borrow().machine,
            &[i32::from(retained.0), i32::from(focus.0)],
        );
    }
    fn setting(&mut self, setting: Setting) -> u32 {
        stock::setting(&Area(&*self.bindings.borrow().machine), setting)
    }
    fn lens_mode(&mut self) -> u32 {
        self.enter(stock::LENS_MODE, &[])
    }
    fn lens_busy(&mut self) -> bool {
        self.enter(stock::LENS_BUSY, &[]) == 1
    }
    fn lens_ready(&mut self) -> bool {
        self.enter(stock::LENS_READY, &[]) == 1
    }
    fn area_levels(&mut self) -> [u16; 2] {
        let b = self.bindings.borrow();
        if let Some(Some(levels)) = b.history.map(|entries| entries[0]) {
            return levels;
        }
        stock::area_levels(&*b.machine, &self.bytes(&group_16::AREA_STATISTICS))
    }
    fn focus_correction(&mut self, q: CorrectionQuery) -> i16 {
        let b = self.bindings.borrow();
        let area = Area(&*b.machine);
        if stock::setting(&area, Setting::FocusCorrection) == 0 {
            return 0;
        }
        let flags = call(&*b.machine, stock::LENS_FLAGS, &[]);
        let adjusted = limits::channel_step_word(
            &area,
            &mut calc::Derivations::new(&*b.machine),
            limits::WordQuery {
                level: q.position.0,
                channel: Channel(q.lens.0),
                step: TableStep(q.parameter),
                export_first: flags,
                export_second: 0,
                direction_mode: q.class,
            },
        );
        let rounded = adjusted / 10
            + if adjusted % 10 > 4 {
                1
            } else if adjusted % 10 < -4 {
                -1
            } else {
                0
            };
        (rounded as i16)
            .wrapping_sub(table::rounded_word(
                &area,
                TableStep(q.parameter),
                Row(q.lens.0 as u32),
            ))
            .wrapping_sub(table::compensated_word(
                &area,
                TableStep(q.parameter),
                Row(q.lens.0 as u32),
            ))
    }
    fn profile_period(&mut self, class: u32) -> u32 {
        FrameInterval::frame_interval(&mut Tables(&*self.bindings.borrow().machine), class)
            .unwrap_or(0)
    }
    fn record_class(&mut self) -> u32 {
        self.shared(&group_27::AT_0724)
    }
    fn step(&mut self, query: StepQuery) -> Step {
        let mut b = self.bindings.borrow_mut();
        let super::Bindings {
            machine,
            records,
            entries,
            state,
            ..
        } = &mut *b;
        let mut ports = calc::Ports {
            machine: &**machine,
            records: &**records,
            entries: &mut **entries,
        };
        let kind = match query.kind {
            StepKind::Direction => drive_step::StepKind::Direction,
            StepKind::Scan => drive_step::StepKind::Scan,
            StepKind::Return => drive_step::StepKind::Return,
            StepKind::Climb => drive_step::StepKind::ContinuousStart,
            StepKind::Sweep => drive_step::StepKind::ContinuousPeak,
            StepKind::Track => drive_step::StepKind::ContinuousTrack,
        };
        let step = drive_step::step(
            &mut ports,
            &state.configuration,
            self.handle(),
            drive_step::StepQuery {
                kind,
                channel: query.parameter,
                contrast: query.contrast,
                timing: query.profile_class,
                drive_kind: DriveKind(query.lens_mode),
            },
        );
        Step {
            size: step.size,
            rate: step.rate,
        }
    }
    fn monitor_step(&mut self, q: MonitorStepQuery) -> u16 {
        drive_step::monitored_step(
            &mut Tables(&*self.bindings.borrow().machine),
            q.lens.0 as u32,
            drive_step::Monitor {
                moved: q.moved == Some(LensMove::Ascending),
                mark: q.mark,
            },
            q.step as i16,
        ) as u16
    }
    fn request_scan_refresh(&mut self, requested: bool) {
        let mut b = self.bindings.borrow_mut();
        let super::Bindings { machine, state, .. } = &mut *b;
        state
            .configuration
            .request_refresh(&mut Tables(&**machine), requested);
    }
    fn scan_refresh(&mut self) -> bool {
        self.bindings.borrow().state.configuration.refresh()
    }
    fn plan_scan(&mut self, q: ScanQuery) -> ScanPlan {
        let mut b = self.bindings.borrow_mut();
        let super::Bindings {
            machine,
            records,
            entries,
            state,
            ..
        } = &mut *b;
        let mut ports = calc::Ports {
            machine: &**machine,
            records: &**records,
            entries: &mut **entries,
        };
        let query = scan_drive::ScanQuery {
            frame: scan_drive::ScanFrame {
                index: q.frame.parameter_index,
                level: q.frame.stage_halves[0] as i16,
                resource: q.frame.current_lens,
                scale_group: q.frame.stage_halves[1],
                scale_entry: q.frame.stage_last,
            },
            monitor: q.watching.then_some(drive_step::Monitor {
                moved: q.moved == Some(LensMove::Ascending),
                mark: q.mark,
            }),
            contrast: q.contrast,
            drive_kind: DriveKind(q.lens_mode),
            toward_second: q.direction > 0,
            limits: [q.limits.low.0, q.limits.high.0],
        };
        let plan =
            state
                .scan_drive
                .plan_scan(&mut ports, &state.configuration, self.handle(), query);
        ScanPlan {
            limits: plan.limits.map(LensPosition),
            window: plan.window.map(|v| v as u16),
            lead: plan.step.step.size,
            rate: plan.step.step.rate,
        }
    }
    fn corrected_position(&mut self, frame_mark: u16) -> CorrectedPosition {
        let b = self.bindings.borrow();
        let p = b
            .state
            .scan_drive
            .corrected_position(&mut Tables(&*b.machine), frame_mark);
        CorrectedPosition {
            frame: LensPosition(p.target),
            drive: LensPosition(p.position),
        }
    }
    fn drive_target(
        &mut self,
        lens: LensIdentity,
        amount: u16,
        low: LensPosition,
        high: LensPosition,
    ) -> LensPosition {
        let b = self.bindings.borrow();
        LensPosition(search_targets::drive_target(
            &mut Tables(&*b.machine),
            &mut Tables(&*b.machine),
            &mut Tables(&*b.machine),
            lens.0,
            u32::from(amount),
            i32::from(low.0),
            high.0,
        ))
    }
    fn below_threshold(&mut self, value: u32, high: u32, low: u32, percent: u32) -> bool {
        search_targets::below_threshold(
            &mut Tables(&*self.bindings.borrow().machine),
            value,
            high,
            low,
            percent,
        )
    }
    fn passed_peak(
        &mut self,
        series: &[u32],
        percent: u32,
        recent_peak: bool,
    ) -> (bool, Option<u32>) {
        search_targets::passed_peak(
            &mut Tables(&*self.bindings.borrow().machine),
            series,
            percent,
            recent_peak,
        )
    }
    fn fell_from_peak(&mut self, series: &[u32]) -> bool {
        search_targets::fell_from_peak(&mut Tables(&*self.bindings.borrow().machine), series)
    }
    fn contrast_log(&mut self, value: u32) -> u32 {
        self.enter(stock::CONTRAST_LOG, &[value])
    }
    fn fit_peak(&mut self, points: [FitPoint; 3]) -> LensPosition {
        let b = self.bindings.borrow();
        let curve = PeakCurve::from_three(
            points.map(|p| p.contrast as i32),
            points.map(|p| p.position),
        );
        LensPosition(peak::peak_position(&mut Tables(&*b.machine), &curve))
    }
    fn measurement_history(&mut self) -> Option<[Option<[u16; 2]>; 5]> {
        self.bindings.borrow().history
    }
    fn profile_focus(&mut self) -> i16 {
        self.shared(&group_27::AT_0244)
    }
    fn detailed_tracing(&mut self) -> bool {
        self.tracing_bit(0x30)
    }
    fn trace_peak_search(&mut self, t: PeakSearchTrace) {
        stock::trace_values(
            &*self.bindings.borrow().machine,
            &[
                i32::from(t.samples),
                t.recent_peak as i32,
                t.scan as i32,
                t.peak_contrast as i32,
                i32::from(t.focus.0),
                t.focus_speed as i32,
                i32::from(t.hold_mode),
                i32::from(t.limits[0].0),
                i32::from(t.limits[1].0),
                i32::from(t.frame_mark),
                i32::from(t.position.0),
                i32::from(t.correction_source),
                i32::from(t.corrected_position.0),
                t.correction_class as i32,
                i32::from(t.best.0),
                t.best_class as i32,
                t.contrast as i32,
                t.second as i32,
                t.combined as i32,
                t.reference as i32,
                i32::from(t.series_len),
                t.channel as i32,
                i32::from(t.channels),
                i32::from(t.rose),
                t.rise_percent as i32,
                t.highest[0] as i32,
                t.highest[1] as i32,
                t.lowest[0] as i32,
                t.lowest[1] as i32,
                i32::from(t.passed),
                t.fall_percent as i32,
                t.peaks[0] as i32,
                t.peaks[1] as i32,
                i32::from(t.limit_marks[0]),
                i32::from(t.limit_marks[1]),
            ],
        );
    }
    fn focus_tracing(&mut self) -> bool {
        self.tracing_bit(0x50)
    }
    fn trace_focus(&mut self, t: FocusTrace) {
        stock::trace_values(
            &*self.bindings.borrow().machine,
            &[
                i32::from(t.samples),
                t.movement[0] as i32,
                t.movement[1] as i32,
                i32::from(t.motion[0]),
                i32::from(t.motion[1]),
                t.focus_reported as i32,
                i32::from(t.hold_mode),
                t.restarted as i32,
                t.selected as i32,
                i32::from(t.count),
                t.contrast as i32,
                t.peak_contrast as i32,
                t.fall_percent as i32,
                t.rise_percent as i32,
                i32::from(t.step_base),
                i32::from(t.watch),
                i32::from(t.watch_reference),
                i32::from(t.watch_tens),
                i32::from(t.rises),
                i32::from(t.falls),
                i32::from(t.strikes),
                i32::from(t.strike_limit),
                i32::from(t.probed),
            ],
        );
    }
    fn minimum_step(&mut self, minimum: u32) -> u32 {
        search_targets::minimum_step(&mut Tables(&*self.bindings.borrow().machine), minimum)
    }
    fn profile_mode(&mut self) -> u32 {
        self.shared(&group_20::AT_014C)
    }
    fn parameter_label(&mut self, parameter: u32) -> u16 {
        let b = self.bindings.borrow();
        let area = Area(&*b.machine);
        table::primary_rounded_word(&area, TableStep(parameter % table::bank_size(&area))) as u16
    }
    fn ticks(&mut self) -> u32 {
        let b = self.bindings.borrow();
        let mut bytes = [0; 4];
        buffer(&*b.machine, stock::TICKS, &[out(0)], &mut bytes);
        stock::word(&bytes, 0)
    }
    fn append_focus_log(&mut self, log: &FocusLog) {
        stock::append_log(&mut *self.bindings.borrow_mut().machine, log);
    }
    fn newest_focus_log(&mut self) -> FocusLogEntry {
        stock::newest_log(&*self.bindings.borrow().machine)
    }
    fn show_debug_text(&mut self, text: &[u8], column: u8, row: u8) {
        stock::debug_text(&*self.bindings.borrow().machine, text, column, row);
    }
    fn angular_rate(&mut self) -> u32 {
        self.enter(stock::ANGULAR_RATE, &[])
    }
    fn request_angular_rate(&mut self) {
        self.enter(stock::REQUEST_ANGULAR_RATE, &[]);
    }
    fn stage(&mut self) -> StageRecord {
        stock::stage(&self.bytes(&group_15::AT_074C))
    }
    fn status_snapshot(&mut self) -> [u32; 2] {
        let bytes = self.snapshot();
        [stock::word(&bytes, 0x110), stock::word(&bytes, 0xf8)]
    }
    fn publish_pinned_status(&mut self, status: PinnedStatus) {
        let mut bytes = self.bytes(&group_33::AT_07A0);
        stock::pinned_status(&mut bytes, status);
        self.store(&group_33::AT_07A0, bytes);
        stock::report_position(&mut *self.bindings.borrow_mut().machine, status.reported.0);
    }
    fn trace_transition(&mut self, t: TransitionTrace) {
        stock::trace_values(
            &*self.bindings.borrow().machine,
            &[
                phase_code(t.phase),
                i32::from(t.limits[0].0),
                i32::from(t.limits[1].0),
                i32::from(t.marks[0]),
                i32::from(t.marks[1]),
                i32::from(t.lens.0),
                i32::from(t.position.0),
                i32::from(t.fit),
                t.level as i32,
                i32::from(t.direction),
                t.speed as i32,
                i32::from(t.focus.0),
            ],
        );
    }
    fn lens_move(&mut self) -> Option<LensMove> {
        (self.enter(stock::LENS_MOVING, &[]) == 1).then(|| {
            if self.enter(stock::LENS_MOVE, &[]) == 1 {
                LensMove::Ascending
            } else {
                LensMove::Descending
            }
        })
    }
    fn interpolated_bias(
        &mut self,
        lens: LensIdentity,
        moved: LensMove,
        position: LensPosition,
    ) -> i16 {
        let b = self.bindings.borrow();
        search_targets::interpolated_bias(
            &mut Tables(&*b.machine),
            &mut Tables(&*b.machine),
            lens.0 as u32,
            moved == LensMove::Ascending,
            i32::from(position.0),
        )
    }
}
fn phase_code(phase: Phase) -> i32 {
    match phase {
        Phase::Direction => 13,
        Phase::PeakSearch => 14,
        Phase::Peak => 15,
        Phase::Return => 16,
        Phase::Focus => 17,
        Phase::FocusDrive => 18,
        Phase::Approach => 19,
        Phase::Monitor => 20,
    }
}

impl<M: Machine, R: Memory, E: StockEntries, P> RecordReads for Session<'_, '_, M, R, E, P> {
    fn selection_ratio(&mut self) -> [u32; 2] {
        let mut bytes = [0; 8];
        buffer(
            &*self.bindings.borrow().machine,
            stock::SELECTION_RATIO,
            &[out(0), w(record_handle_word(self.handle()))],
            &mut bytes,
        );
        [stock::word(&bytes, 0), stock::word(&bytes, 4)]
    }
    fn converted_decade(&mut self) -> u16 {
        self.enter(stock::CONVERTED_DECADE, &[]) as u16
    }
    fn zone_limit(&mut self, zone: Zone) -> i16 {
        self.enter(stock::ZONE_LIMIT, &[zone.0]) as i16
    }
    fn lens_level(&mut self, step: u32, lens: LensIdentity) -> i16 {
        table::combined_word(
            &Area(&*self.bindings.borrow().machine),
            TableStep(step),
            Row(lens.0 as u32),
        )
    }
}
impl<M: Machine, R: Memory, E: StockEntries, P> IntakePorts for Session<'_, '_, M, R, E, P> {
    fn frame_snapshot(&mut self) -> FrameSnapshot {
        xm1_saclac_focus_search::stock::intake::frame_snapshot(&self.snapshot())
    }
    fn record(&mut self) -> IntakeRecord {
        IntakeRecord {
            scale: self.field(&group_00::AT_0000),
            selection: u32::from(Platform::measurement_selection(self)),
            mode: u32::from(self.header(SharedHeaderByte::At47)),
            profile_class: u32::from(self.header(SharedHeaderByte::At31)),
            resource: stock::word(&self.bytes(&group_37::AT_085C), 0),
            range: [
                self.shared(&group_27::AT_0244),
                self.shared(&group_27::AT_0248),
            ],
            range_shift: [
                self.shared(&group_27::AT_0710),
                self.shared(&group_27::AT_0712),
            ],
            window_status: self.shared(&group_27::WINDOW_STATUS).0,
            second_result: self.shared(&group_20::AT_014C),
            record_20b0: self.field(&group_13::AT_20B0_4),
            program_source: self.shared(&group_27::AT_0748),
            record_step: self.field(&group_00::AT_0044),
            lens_kind: self.shared(&group_27::AT_0742),
            program_word: self.shared(&group_27::AT_0720),
            stage: Platform::stage(self),
        }
    }
    fn imaging_profile_class(&mut self) -> u32 {
        let mut bytes = [0; 4];
        buffer(
            &*self.bindings.borrow().machine,
            stock::PROFILE_CLASS,
            &[out(0)],
            &mut bytes,
        );
        stock::word(&bytes, 0)
    }
    fn parameter_index(&mut self, parameter: u32) -> u32 {
        table::column(
            &Area(&*self.bindings.borrow().machine),
            TableStep(parameter),
        )
    }
    fn lens_identity(&mut self) -> LensIdentity {
        Platform::lens_identity(self)
    }
    fn lens_step(&mut self, kind: u8, lens: LensIdentity) -> u32 {
        self.enter(stock::LENS_STEP, &[u32::from(kind), lens.0 as u32])
    }
    fn program_level_word(&mut self, kind: u8, far: i16, program_word: u32, source: u32) -> u16 {
        self.enter(
            stock::PROGRAM_LEVEL_WORD,
            &[u32::from(kind), signed(far), program_word, source],
        ) as u16
    }
    fn planner_word(&mut self) -> u32 {
        u32::from(self.bindings.borrow().state.configuration.refresh())
    }
    fn selected_program(&mut self, p: ProgramInputs) -> u32 {
        let mut bytes = [0; 44];
        for (at, value) in [
            (4, p.selection),
            (8, p.second_result),
            (12, p.window_status),
            (16, p.profile_class),
            (20, p.resource),
            (28, p.record_20b0),
            (32, p.planner),
        ] {
            stock::put_word(&mut bytes, at, value);
        }
        stock::put_half(&mut bytes, 40, p.level_word);
        bytes[43] = 2;
        buffer(
            &*self.bindings.borrow().machine,
            stock::SELECT_PROGRAM,
            &[out(4), out(0)],
            &mut bytes,
        );
        stock::word(&bytes, 0)
    }
    fn program_level(&mut self, program: u32, base: i16, lens: LensIdentity) -> i32 {
        self.enter(
            stock::PROGRAM_LEVEL,
            &[program, signed(base), lens.0 as u32],
        ) as i32
    }
    fn peak_threshold_reached(&mut self) -> bool {
        if Platform::setting(self, Setting::RecentPeak) & 1 == 0 {
            return false;
        }
        let threshold = Platform::setting(self, Setting::RecentPeakLevel) as u16;
        self.bindings
            .borrow()
            .history
            .and_then(|entries| entries[0])
            .is_some_and(|levels| levels.into_iter().any(|level| level >= threshold))
    }
    fn scan_thresholds(&mut self) -> [i8; 2] {
        stock::thresholds(&Area(&*self.bindings.borrow().machine))
    }
}

impl<M: Machine, R: Memory, E: StockEntries, P: Calc + ImgDc> Session<'_, '_, M, R, E, P> {
    fn evaluate(&mut self, record: SearchRecord, callback: u32, completion: Completion) {
        let payload = interface::calc_payload(&xm1_saclac_interface::calc::CalcPayload::Search {
            record: xm1_saclac_interface::calc::Record(interface::search::search_record(&record)),
            callback: interface::StockCallbackWord(callback),
        });
        let mut b = self.bindings.borrow_mut();
        if Calc::request(
            &mut *b.peers,
            CalcRequest {
                operation: payload.0,
                parameters: interface::parameters(payload.1.bytes()),
                completions: &[completion],
            },
        )
        .is_err()
        {
            call(&*b.machine, stock::FAULT, &[]);
        }
    }
    fn image(
        &mut self,
        operation: ImgDcOperation,
        callback: Option<u32>,
        record: &[u8],
        completions: &[Completion],
    ) {
        let mut bytes = [0; 44];
        let prefix = usize::from(callback.is_some()) * 4;
        if let Some(callback) = callback {
            stock::put_word(&mut bytes, 0, callback);
        }
        bytes[prefix..prefix + record.len()].copy_from_slice(record);
        let mut b = self.bindings.borrow_mut();
        if ImgDc::request(
            &mut *b.peers,
            ImgDcRequest {
                operation,
                parameters: interface::parameters(&bytes[..prefix + record.len()]),
                completions,
            },
        )
        .is_err()
        {
            call(&*b.machine, stock::FAULT, &[]);
        }
    }
}
impl<M: Machine, R: Memory, E: StockEntries, P: Calc + ImgDc> live::Ports<SearchRecord, BridgeDrive>
    for Session<'_, '_, M, R, E, P>
{
    fn fault(&mut self) {
        self.enter(stock::FAULT, &[]);
    }
    fn request(&mut self, request: Request<SearchRecord, BridgeDrive>) {
        match request {
            Request::Evaluate(record) => {
                self.evaluate(record, stock::LIVE_EVALUATED, Completion::Calculation89)
            }
            Request::BridgeStop => self.image(ImgDcOperation::Request2f, None, &[], &[]),
            Request::Bridge { motion, drive } => {
                let travel = if !drive.moves {
                    0
                } else {
                    match motion {
                        BridgeMotion::Release => 0,
                        BridgeMotion::Travel(t) => match t.direction {
                            Direction::Increasing => t.steps as i16,
                            Direction::Decreasing => (t.steps as i16).wrapping_neg(),
                        },
                    }
                };
                let request = BridgeRequest {
                    phase: drive.phase,
                    scanning: drive.scanning,
                    window: drive.window,
                    travel,
                    rate: drive.rate,
                    moves: drive.moves && !matches!(motion, BridgeMotion::Release),
                    speed: drive.speed,
                    lens_mode: drive.lens_mode,
                };
                self.image(
                    ImgDcOperation::Request2e,
                    Some(stock::LIVE_REPORTED),
                    &interface::search::bridge_request(&request),
                    &[Completion::Image46],
                );
            }
        }
    }
}
impl<M: Machine, R: Memory, E: StockEntries, P: Calc + ImgDc> S1Ports
    for Session<'_, '_, M, R, E, P>
{
    fn fault(&mut self) {
        self.enter(stock::FAULT, &[]);
    }
    fn evaluate(&mut self, record: SearchRecord) {
        self.evaluate(record, stock::S1_EVALUATED, Completion::Calculation9d);
    }
    fn drive(&mut self, request: S1DriveRequest) {
        self.image(
            ImgDcOperation::Request44,
            Some(stock::S1_REPORTED),
            &interface::search::s1_drive_request(&request),
            &[Completion::Image5a],
        );
    }
    fn evaluation_result(
        &mut self,
        address: ResultAddress,
    ) -> [u8; interface::search::EVALUATION_BYTES] {
        stock::read(&*self.bindings.borrow().machine, address.0)
    }
    fn record(&mut self) -> S1Record {
        S1Record {
            scale: self.field(&group_00::AT_0000),
            selection: u32::from(Platform::measurement_selection(self)),
            mode: u32::from(self.header(SharedHeaderByte::At47)),
            profile_class: u32::from(self.header(SharedHeaderByte::At31)),
            resource: stock::word(&self.bytes(&group_37::AT_085C), 0),
            near: self.shared(&group_26::AT_0206),
            far: self.shared(&group_26::AT_0208),
            step: self.shared(&group_26::AT_0200),
            timing: self.shared(&group_26::AT_0230),
        }
    }
    fn stage_statistics(&mut self) {
        self.enter(stock::S1_STAGE, &[1]);
    }
    fn frame_snapshot(&mut self) -> S1Snapshot {
        let mut bytes = [0; xm1_saclac_focus_search::stock::s1::S1_SNAPSHOT_BYTES];
        buffer(
            &*self.bindings.borrow().machine,
            stock::S1_SNAPSHOT,
            &[w(1), out(0)],
            &mut bytes,
        );
        xm1_saclac_focus_search::stock::s1::s1_snapshot(&bytes)
    }
    fn near_position(&mut self) -> i16 {
        self.shared(&group_41::AT_0970)
    }
    fn interval_tracking(&mut self) -> u32 {
        u32::from(self.field(&group_00::INTERVAL_TRACKING).0)
    }
}
