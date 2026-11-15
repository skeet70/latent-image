use super::*;
use core::{cell::{RefCell, RefMut}, ops::{Deref, DerefMut}};
use xm1_saclac_calc::{adjust::configuration::{Centroid, FrameInterval as MotionTiming, GridPair, LibraryDiagnostics, MotionSource, ProfileStore}, exposure::ports::ProfileAccess, profile::Profile, scene_recognition::SceneRecognition};
use xm1_saclac_profile_records::ProfileRecords;
use xm1_focus_measurement_round::{lanes::Selection, profile::{ProfileRecords as RoundRecords, StatisticsInUse, PublishedResult, Profile as RoundProfile, ProfileGeometry as RoundGeometry, WindowRecord, WindowStatus as RoundWindowStatus, WindowPosition, IntervalHistory}};
use xm1_shot_sequence::{grid_boundary::GridLayers, shot_ports::ShotPorts, state::{ShotState, StateMemory}};

pub(crate) struct ServicesLoan<'a, M, R, E, P> {
    source: &'a RefCell<Services<M, R, E, P>>,
    active: Option<RefMut<'a, Services<M, R, E, P>>>,
}

impl<M, R, E, P> Deref for ServicesLoan<'_, M, R, E, P> {
    type Target = Services<M, R, E, P>;
    fn deref(&self) -> &Self::Target { self.active.as_deref().expect("acquisition service loan") }
}
impl<M, R, E, P> DerefMut for ServicesLoan<'_, M, R, E, P> {
    fn deref_mut(&mut self) -> &mut Self::Target { self.active.as_deref_mut().expect("acquisition service loan") }
}

/// A dispatch borrows the canonical services and scene state together.
pub struct Acquisition<'a, M, R, E, P, S, X> {
    pub(crate) services: ServicesLoan<'a, M, R, E, P>,
    recognition: &'a RefCell<SceneRecognition>,
    shot: &'a RefCell<ShotPorts<S, X>>,
}

impl<'a, M, R, E, P, S, X> Acquisition<'a, M, R, E, P, S, X> {
    pub fn new(services: &'a RefCell<Services<M, R, E, P>>, recognition: &'a RefCell<SceneRecognition>, shot: &'a RefCell<ShotPorts<S, X>>) -> Self {
        Self { services: ServicesLoan { source: services, active: Some(services.borrow_mut()) }, recognition, shot }
    }
}
impl<M, R, E, P, S: StateMemory, X> Acquisition<'_, M, R, E, P, S, X> {
    fn with_shot<V>(&mut self, operation: impl FnOnce(&mut ShotState<S>, &mut X) -> V) -> V {
        // Libraries' grid settings borrow these same Services.
        drop(self.services.active.take());
        let value = self.shot.borrow_mut().with_parts(operation);
        self.services.active = Some(self.services.source.borrow_mut());
        value
    }
}

impl<M, R, E, P, S, X> Deref for Acquisition<'_, M, R, E, P, S, X> {
    type Target = Services<M, R, E, P>;
    fn deref(&self) -> &Self::Target { &self.services }
}
impl<M, R, E, P, S, X> DerefMut for Acquisition<'_, M, R, E, P, S, X> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.services }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, S, X> ProfileAccess for Acquisition<'_, M, R, E, P, S, X> {
    type Records = R;
    type Entries = E;
    fn profile(&mut self) -> Profile<'_, R, E> { ProfileAccess::profile(&mut *self.services) }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P, S, X> Properties for Acquisition<'_, M, R, E, P, S, X> {
    fn property(&mut self, property: Property) -> u32 { self.services.property(property) }
}

macro_rules! forward {
    ($contract:path { $(fn $name:ident(&mut self $(, $argument:ident: $type:ty)*) $(-> $result:ty)?;)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + Calc1 + ImgDc, S, X>
            $contract for Acquisition<'_, M, R, E, P, S, X> {
            $(fn $name(&mut self $(, $argument: $type)*) $(-> $result)? {
                <Services<M, R, E, P> as $contract>::$name(&mut *self.services $(, $argument)*)
            })*
        }
    };
}
forward!(RoundRecords {
    fn offered(&mut self) -> Option<Selection>;
    fn statistics_in_use(&mut self) -> StatisticsInUse;
    fn set_statistics_in_use(&mut self, record: RecordHandle, in_use: StatisticsInUse);
    fn detach_capture_buffer(&mut self, record: RecordHandle);
    fn invalidate(&mut self, record: RecordHandle, result: PublishedResult);
    fn invalidate_focus_result(&mut self);
    fn grants_measurement_history(&mut self, record: RecordHandle) -> bool;
    fn acquisition_level(&mut self) -> i16;
    fn interval_history(&mut self) -> IntervalHistory;
    fn set_interval_history(&mut self, record: RecordHandle, history: IntervalHistory);
    fn profile(&mut self, record: RecordHandle) -> RoundProfile;
    fn geometry(&mut self, record: RecordHandle) -> RoundGeometry;
    fn window(&mut self) -> WindowRecord;
    fn store_window_status(&mut self, record: RecordHandle, status: RoundWindowStatus);
    fn store_window_position(&mut self, record: RecordHandle, position: WindowPosition);
});
forward!(Pipeline {
    fn capture(&mut self, statistics: Statistics);
    fn snapshot(&mut self, statistics: Statistics) -> PipelineSnapshot;
    fn timing(&mut self) -> TimingWord;
    fn frame_interval(&mut self, timing: TimingWord) -> Option<FrameInterval>;
    fn request_mode_six_statistics(&mut self);
    fn take_window_refresh(&mut self) -> bool;
    fn history_statistics(&mut self, cell: GridCell) -> HistoryStatistics;
});
forward!(Exposure {
    fn gain_offset(&mut self, step: GainStep) -> u32;
    fn exposure_value(&mut self, exposure: [PipelineWord; 2]) -> i16;
    fn timing_offset(&mut self, timing: TimingWord) -> u16;
    fn relative_exposure(&mut self, exposure: PipelineWord) -> i16;
});
forward!(Adjustment { fn adjustment_words(&mut self) -> AdjustmentWords; });
forward!(Trace { fn trace(&mut self, mark: TraceMark); });
forward!(rate_policy::ProfileCurves { fn profile_curve(&mut self, profile: i32, class: u8) -> u32; });

struct MotionProfiles<'a, M, R, E> {
    records: &'a mut ProfileRecords<R>,
    entries: &'a mut E,
    stock: &'a Stock<M>,
}
impl<M, R: Memory, E: StockEntries> ProfileAccess for MotionProfiles<'_, M, R, E> {
    type Records = R;
    type Entries = E;
    fn profile(&mut self) -> Profile<'_, R, E> { Profile::new(self.records, self.entries) }
}
impl<M, R: Memory, E: StockEntries> ProfileStore for MotionProfiles<'_, M, R, E> {
    fn records(&mut self) -> &mut ProfileRecords<R> { self.records }
}
impl<M: Machine, R, E> Properties for MotionProfiles<'_, M, R, E> {
    fn property(&mut self, property: Property) -> u32 { u32::from(self.stock.byte(AdjustmentKey(property.key()))) }
}

struct Timing<'a, M>(&'a M);
impl<M: Machine> MotionTiming for Timing<'_, M> {
    fn frame_interval(&mut self, timing: u32) -> Option<u32> { boundary::frame_interval(self.0, TimingWord(timing)) }
}

fn motion_source(flag: u32) -> MotionSource { if flag == 1 { MotionSource::HalfPress } else { MotionSource::Live } }
fn motion_grids(machine: &impl Machine, result: SharedAddress) -> GridPair {
    let current = machine.read_u32(result.0 + 0x18);
    let previous = machine.read_u32(result.0 + 0x14);
    GridPair {
        current: core::array::from_fn(|index| machine.read_u16(current + index as u32 * 2) as i16),
        previous: core::array::from_fn(|index| machine.read_u16(previous + index as u32 * 2) as i16),
    }
}
macro_rules! store {
    ($this:ident, $record:expr, $field:path, $value:expr) => { let _refused = $this.services.records.write_shared($record, &$field, $value); };
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + Calc1 + ImgDc, S: StateMemory, X: GridLayers> Ports for Acquisition<'_, M, R, E, P, S, X> {
    fn with_selection<V>(&mut self, operation: impl FnOnce(&mut SelectionMemory) -> V) -> V { operation(&mut self.services.acquisition.selection) }
    fn acquisition_settings(&mut self) -> AcquisitionSettings { boundary::acquisition_settings(|key| self.services.stock.byte(AdjustmentKey(key))) }

    fn record_view(&mut self, record: RecordHandle) -> RecordView {
        use record::group_00 as h;
        use shared::{group_20 as c, group_27 as x};
        let profile_index = {
            let services = &mut *self.services;
            u32::from(xm1_saclac_calc::profile::Profile::new(&services.records, &mut services.entries).grid_layout(record))
        };
        let entries = boundary::record_entries(&self.services.machine, record, profile_index);
        let category = self.services.entries.header_byte(record, SharedHeaderByte::At47);
        let cell = self.focus_cell(record, category);
        let (block_reference, block_word) = boundary::lens_snapshot_reference(&self.shared_bytes(record, &shared::group_17::LENS_SNAPSHOT));
        let selected = xm1_saclac_calc::profile::selection_code(self.services.entries.measurement_selection(record));
        RecordView {
            start: Start::of(self.shared(record, &c::AT_0140)), selection: profile_selection(u32::from(selected)),
            s1_mode: AcquisitionMode(u32::from(self.services.entries.header_byte(record, SharedHeaderByte::At31))),
            halfword: self.field(record, &h::AT_002E), mode_word: u32::from(self.field(record, &h::AT_0032)),
            byte: self.field(record, &h::AT_0033), second_halfword: self.field(record, &h::AT_0026),
            setting_word: u32::from(self.field(record, &h::AT_005B)), grid_kind: u32::from(self.field(record, &h::AT_005D)),
            focus_word: u32::from(self.field(record, &h::AT_0034)), window: self.field(record, &h::AT_0000),
            seventh_word: RetainedWord(u32::from(self.field(record, &h::AT_0036))), fourth_word: RetainedWord(u32::from(self.field(record, &h::AT_003D))),
            third_word: RetainedWord(u32::from(self.field(record, &h::AT_003F))), request: u32::from(self.field(record, &h::AT_003E)),
            detail_flag: u32::from(self.field(record, &h::AT_0041)), word: u32::from(self.field(record, &h::AT_0043)),
            reference: self.field(record, &h::AT_001C) as u16, selection_word: self.field(record, &h::AT_0014),
            lens_halfword: self.field(record, &h::AT_001E) as u16, lens_reference: u32::from(self.field(record, &h::AT_0044)),
            halfwords: [self.field(record, &h::AT_0020) as u16, self.field(record, &h::AT_0024) as u16],
            zero_word: RetainedWord(u32::from(self.services.entries.header_byte(record, SharedHeaderByte::At46))), category: u32::from(category),
            pair: boundary::words(&self.field(record, &h::AT_0004)), tenth_word: RetainedWord(u32::from(self.field(record, &h::AT_005F))),
            change_category: u32::from(self.services.entries.header_byte(record, SharedHeaderByte::At61)),
            signed_word: if selected == 1 { self.services.stock.zone_limit(2) as u16 } else { self.field(record, &h::AT_0022) as u16 },
            eighth_word: RetainedWord(u32::from(self.field(record, &h::AT_0045))), second_word: RetainedWord(u32::from(self.field(record, &h::AT_004B))),
            mode_flag: u32::from(self.services.entries.header_byte(record, SharedHeaderByte::At63)),
            rate_enabled: u32::from(self.field(record, &h::INTERVAL_TRACKING).0), fifth_word: RetainedWord(u32::from(self.field(record, &h::AT_0051))),
            forced: u32::from(self.field(record, &h::AT_006B)), ninth_word: RetainedWord(u32::from(self.field(record, &h::AT_0066))),
            profile_flags: [u32::from(self.field(record, &h::AT_0067)), u32::from(self.field(record, &h::AT_0068)), u32::from(self.field(record, &h::AT_0069)),
                self.field(record, &record::group_13::AT_20C8), self.field(record, &record::group_13::AT_20CC), self.field(record, &record::group_31::AT_2110),
                self.field(record, &record::group_13::AT_20D4), self.field(record, &record::group_13::AT_20B0_4)],
            lens_word_index: self.shared(record, &c::OFFERED_MEASUREMENTS).0, adjustment: self.shared(record, &c::AT_014C),
            kind: self.shared(record, &x::WINDOW_STATUS).0, kind_override: self.shared(record, &x::AT_0738), pair_word: boundary::position_word(self.shared(record, &x::WINDOW_POSITION)),
            reuse_blocked: self.shared(record, &x::AT_0744), restored: u32::from(self.field(record, &h::AT_0070)),
            program_word: boundary::words(&self.shared_bytes(record, &shared::group_37::AT_085C)), restore_position: u32::from(self.field(record, &h::AT_006F)),
            s1_byte: self.shared(record, &shared::group_26::AT_0236), s1_halfword: self.shared(record, &shared::group_26::AT_0238) as u16,
            release_halfword: self.shared(record, &x::AT_0246) as u16, area: boundary::words(&self.shared_bytes(record, &shared::group_22::AT_0180)),
            mode: AcquisitionMode(entries.mode), updated_position: entries.updated_position, position_halfword: entries.position_halfword,
            converted_level: entries.converted_level, restore_adjustment: entries.restore_adjustment, decade: entries.decade,
            profile_index, ratio: [cell.column, cell.row], lens_limit: entries.lens_limit, block_reference, block_word,
            s1_exposure: PipelineWord(self.shared(record, &shared::group_26::AT_0200)), s1_level: self.shared(record, &shared::group_26::AT_0206) as u16,
            s1_second: self.shared(record, &shared::group_26::AT_0208) as u16, s1_timing: TimingWord(self.shared(record, &shared::group_26::AT_0230)),
        }
    }
    fn s1_rate_enabled(&mut self, record: RecordHandle) -> bool { self.field(record, &record::group_00::INTERVAL_TRACKING).0 == 1 }

    fn published_exposure(&mut self, record: RecordHandle) -> PublishedExposure {
        use shared::group_27 as x;
        PublishedExposure {
            combined: self.shared(record, &x::AT_0248) as u16, reference_level: self.shared(record, &x::AT_024A) as u16,
            exposure: PipelineWord(self.shared(record, &x::AT_0708)), level: self.shared(record, &x::AT_070C) as u16,
            second: self.shared(record, &x::AT_070E) as u16, correction: self.shared(record, &x::AT_0710) as u16,
            aspect: self.shared(record, &x::AT_0712) as u16, reference: self.shared(record, &x::AT_0718) as u16,
            offset: self.shared(record, &x::AT_0716) as u16, gain_step: GainStep(self.shared(record, &x::AT_071C)),
            lens: ResourceIndex(self.shared(record, &x::AT_0720)), timing: TimingWord(self.shared(record, &x::AT_0724)),
            code: self.shared(record, &x::AT_0728), selected: self.shared(record, &x::AT_072A), adjustment: self.shared(record, &x::AT_072C),
            selection_reference: self.shared(record, &x::AT_024C) as u16,
            primary_levels: [self.shared(record, &x::AT_024E) as u16, self.shared(record, &x::AT_0250) as u16, self.shared(record, &x::AT_0252) as u16, self.shared(record, &x::AT_0254) as u16],
        }
    }

    fn publish_exposure(&mut self, record: RecordHandle, p: &ExposurePublication) {
        use shared::group_27 as x;
        store!(self, record, x::AT_0708, p.exposure.0); store!(self, record, x::AT_070C, p.level as i16);
        store!(self, record, x::AT_070E, p.second as i16); store!(self, record, x::AT_0718, p.reference as i16);
        store!(self, record, x::AT_0714, p.decade as i16); store!(self, record, x::AT_0716, p.offset as i16);
        store!(self, record, x::AT_0710, p.correction as i16); store!(self, record, x::AT_071C, p.gain_step.0);
        store!(self, record, x::AT_0724, p.timing.0); store!(self, record, x::AT_0728, p.code);
        store!(self, record, x::AT_072A, p.selected); store!(self, record, x::AT_072C, p.adjustment);
        store!(self, record, x::AT_0248, p.combined as i16); store!(self, record, x::AT_024A, p.reference_level as i16);
        store!(self, record, x::AT_0712, p.aspect as i16);
    }

    fn publish_primary_levels(&mut self, record: RecordHandle, p: &PrimaryLevels) {
        use shared::group_27 as x;
        store!(self, record, x::AT_0246, p.base as i16); store!(self, record, x::AT_0258, p.word);
        store!(self, record, x::AT_024E, p.levels[0] as i16); store!(self, record, x::AT_0250, p.levels[1] as i16);
        store!(self, record, x::AT_0252, p.levels[2] as i16); store!(self, record, x::AT_0254, p.levels[3] as i16);
        store!(self, record, x::AT_025E, p.levels[4]); store!(self, record, x::AT_0260, p.levels[5]); store!(self, record, x::AT_0266, p.levels[6]);
    }

    fn publish_grid(&mut self, record: RecordHandle, branch: Branch) {
        let (cells, window, entries) = boundary::grid_records(&self.services.acquisition.grids[branch_index(branch)]);
        store!(self, record, shared::group_27::AT_026C, cells); store!(self, record, shared::group_27::AT_0680, window);
        store!(self, record, shared::group_27::AT_06A8, entries);
    }

    fn publish_selection_levels(&mut self, record: RecordHandle, p: &SelectionLevels) {
        use shared::group_27 as x;
        store!(self, record, x::AT_0244, p.base as i16);
        store!(self, record, x::AT_0262, p.levels[0] as i16); store!(self, record, x::AT_0264, p.levels[1] as i16); store!(self, record, x::AT_0268, p.levels[2] as i16);
        store!(self, record, x::AT_026A, p.zeroed[0] as i16); store!(self, record, x::AT_06F8, p.zeroed[1] as i16);
        store!(self, record, x::AT_06FA, p.zeroed[2] as i16); store!(self, record, x::AT_06FC, p.zeroed[3] as i16);
        store!(self, record, x::AT_06FE, p.zeroed[4] as i16); store!(self, record, x::AT_0700, p.zeroed[5] as i16);
        store!(self, record, x::AT_0702, p.unsigned); store!(self, record, x::AT_0704, p.word); store!(self, record, x::AT_024C, p.reference as i16);
    }

    fn update_exposure_record(&mut self, record: RecordHandle, update: ExposureRecordUpdate) {
        if self.services.entries.header_byte(record, SharedHeaderByte::At63) != 1 { return; }
        let mut bytes = self.shared_bytes(record, &shared::group_33::AT_07A0);
        boundary::update_exposure_record(&mut bytes, update);
        store!(self, record, shared::group_33::AT_07A0, bytes);
    }
    fn store_lens(&mut self, record: RecordHandle, lens: ResourceIndex) { store!(self, record, shared::group_27::AT_0720, lens.0); }
    fn store_reference(&mut self, record: RecordHandle, reference: u16) { store!(self, record, shared::group_27::AT_0718, reference as i16); }
    fn store_selection_reference(&mut self, record: RecordHandle, reference: u16) { store!(self, record, shared::group_27::AT_024C, reference as i16); }
    fn program_kind(&mut self, record: RecordHandle) -> u32 { self.shared(record, &shared::group_27::AT_0748) }
    fn store_program_kind(&mut self, record: RecordHandle, kind: u32) { store!(self, record, shared::group_27::AT_0748, kind); }
    fn stored_program(&mut self, record: RecordHandle) -> Program { Program(u32::from(self.shared(record, &shared::group_27::AT_0742))) }
    fn store_program(&mut self, record: RecordHandle, program: Program) { store!(self, record, shared::group_27::AT_0742, program.0 as u8); }
    fn store_program_word(&mut self, record: RecordHandle, word: [u32; 2]) {
        let mut bytes = [0; 8]; boundary::put_words(&mut bytes, word); store!(self, record, shared::group_37::AT_085C, bytes);
    }
    fn store_program_choice(&mut self, record: RecordHandle, choice: &ProgramChoice) {
        let mut bytes = self.shared_bytes(record, &shared::group_15::AT_074C); boundary::put_program_choice(&mut bytes, choice);
        store!(self, record, shared::group_15::AT_074C, bytes);
    }
    fn acquisition_block(&mut self, record: RecordHandle) -> AcquisitionBlock { boundary::acquisition_block(&self.shared_bytes(record, &shared::group_14::FOCUS_RESULT)) }
    fn store_acquisition_block(&mut self, record: RecordHandle, block: &AcquisitionBlock) {
        let mut bytes = self.shared_bytes(record, &shared::group_14::FOCUS_RESULT); boundary::put_acquisition_block(&mut bytes, block);
        store!(self, record, shared::group_14::FOCUS_RESULT, bytes);
    }
    fn selection_record(&mut self, record: RecordHandle) -> SelectionRecord { boundary::selection_record(&self.shared_bytes(record, &shared::group_41::AT_099C)) }
    fn store_selection_record(&mut self, record: RecordHandle, selection: &SelectionRecord) {
        let mut bytes = self.shared_bytes(record, &shared::group_41::AT_099C); boundary::put_selection_record(&mut bytes, selection);
        store!(self, record, shared::group_41::AT_099C, bytes);
    }
    fn selection_slot(&mut self, record: RecordHandle) -> SelectionSlot {
        use record::{group_05 as a, group_06 as b};
        let i = Index::new(0).unwrap();
        SelectionSlot { word: self.field(record, &a::AT_0170.at(i)), level: self.field(record, &a::AT_0176.at(i)) as u16,
            kind: self.field(record, &b::AT_01A0.at(i)), limit_level: self.field(record, &b::AT_01A4.at(i)) as u16,
            flags: [self.field(record, &b::AT_01A8.at(i)), self.field(record, &b::AT_01AC.at(i))],
            levels: [self.field(record, &a::AT_0178.at(i)) as u16, self.field(record, &a::AT_017A.at(i)) as u16, self.field(record, &a::AT_017C.at(i)) as u16, self.field(record, &a::AT_0186.at(i)) as u16] }
    }
    fn publish_selection_slot(&mut self, record: RecordHandle, slot: &SelectionSlot) {
        use record::{group_05 as a, group_06 as b};
        let i = Index::new(0).unwrap();
        let _ = self.services.records.write(record, &a::AT_0170.at(i), slot.word);
        let _ = self.services.records.write(record, &a::AT_0176.at(i), slot.level as i16);
        let _ = self.services.records.write(record, &b::AT_01A0.at(i), slot.kind);
        let _ = self.services.records.write(record, &b::AT_01A4.at(i), slot.limit_level as i16);
        let _ = self.services.records.write(record, &b::AT_01A8.at(i), slot.flags[0]); let _ = self.services.records.write(record, &b::AT_01AC.at(i), slot.flags[1]);
        let _ = self.services.records.write(record, &a::AT_0178.at(i), slot.levels[0] as i16); let _ = self.services.records.write(record, &a::AT_017A.at(i), slot.levels[1] as i16);
        let _ = self.services.records.write(record, &a::AT_017C.at(i), slot.levels[2] as i16); let _ = self.services.records.write(record, &a::AT_0186.at(i), slot.levels[3] as i16);
    }
    fn store_slot_word(&mut self, record: RecordHandle, word: u32) { let _ = self.services.records.write(record, &record::group_05::AT_0170.at(Index::new(0).unwrap()), word); }
    fn position_pair(&mut self, record: RecordHandle, slot: PositionSlot) -> [u32; 2] { boundary::words(&self.shared_bytes(record, &shared::group_41::AT_0974.at(slot_index(slot)))) }
    fn store_position_pair(&mut self, record: RecordHandle, slot: PositionSlot, pair: [u32; 2]) {
        let mut bytes = [0; 8]; boundary::put_words(&mut bytes, pair); let _ = self.services.records.write_shared(record, &shared::group_41::AT_0974.at(slot_index(slot)), bytes);
    }
    fn store_restore_levels(&mut self, record: RecordHandle, reference: u16, levels: [u16; 2]) {
        store!(self, record, shared::group_41::AT_0972, reference as i16); store!(self, record, shared::group_41::AT_096C, levels[0] as i16); store!(self, record, shared::group_41::AT_096E, levels[1] as i16);
    }
    fn store_s1_level(&mut self, record: RecordHandle, level: i16) { store!(self, record, shared::group_41::AT_0970, level); }
    fn clear_grid_selection(&mut self, record: RecordHandle) { store!(self, record, shared::group_39::AT_0898, [0; 48]); }
    fn rate_record(&mut self, record: RecordHandle) -> RateRecord { boundary::rate_record(&self.shared_bytes(record, &shared::group_27::AT_07EC)) }
    fn store_rate_selection(&mut self, record: RecordHandle, selected: u32) {
        let mut bytes = self.shared_bytes(record, &shared::group_27::AT_07EC); boundary::put_rate_selection(&mut bytes, selected); store!(self, record, shared::group_27::AT_07EC, bytes);
    }
    fn clear_selection_reuse(&mut self, record: RecordHandle) { store!(self, record, shared::group_27::AT_0744, 0); }
    fn clear_controller_reuse(&mut self) { let record = boundary::controller_record(&self.services.machine); self.clear_selection_reuse(record); }
    fn notify_selection(&mut self, record: RecordHandle, notice: &SelectionNotice) { boundary::enter_selection_hook(&self.services.machine, wire::StockCallbackWord(self.shared(record, &shared::group_20::AT_0158)), notice) }
    fn notify_rate(&mut self, record: RecordHandle, notice: &RateNotice) { boundary::enter_rate_hook(&self.services.machine, wire::StockCallbackWord(self.shared(record, &shared::group_20::AT_016C)), notice) }

    fn current_lens(&mut self) -> ResourceIndex { ResourceIndex(boundary::current_lens(&self.services.machine)) }
    fn lens_word(&mut self, record: RecordHandle, lens: Option<ResourceIndex>) -> u32 { boundary::lens_word(&self.services.machine, record, lens) }
    fn collapse_index(&mut self, exposure: PipelineWord) -> u32 { table::column(&self.services.stock, Step(exposure.0)) }
    fn mapped_value(&mut self, first: u32, second: u32) -> u32 { u32::from(table::value(&self.services.stock, Step(first), Row(second))) }
    fn lens_position(&mut self, pair: [u32; 2], position: u32) -> u32 { table::nearest_step(&self.services.stock, Row(pair[0]), Step(pair[1]), Row(position)).0 }
    fn gain_step_for(&mut self, offset: i32) -> GainStep { GainStep(u32::from(lens::zone(offset, &mut self.services.stock).get())) }
    fn acquisition_gain_step(&mut self, adjustment: u32) -> GainStep { GainStep(boundary::acquisition_gain_step(&self.services.machine, adjustment)) }
    fn kind_decade(&mut self, kind: u32) -> u32 { boundary::kind_decade(&self.services.machine, kind) }
    fn change_positions(&mut self, record: RecordHandle) -> [u32; 2] { [self.field(record, &record::group_04::AT_0164), self.field(record, &record::group_04::AT_0168)] }

    fn select_program(&mut self, inputs: &ProgramInputs) -> Option<Program> {
        let choice = programs::ProfileChoice { selection: selection(selection_word(inputs.selection)), result: inputs.adjustment, window_status: WindowStatus(inputs.kind), mode: inputs.mode.0,
            flagged: inputs.word == 1, banded: inputs.option == 1, percent: inputs.percent, held: inputs.bytes[0] != 0, held_variant: inputs.bytes[1] };
        programs::choose(&mut *self.services, &choice).map(|profile| Program(profile.id()))
    }
    fn program_adjustment(&mut self, p: Program) -> u32 { program(p).record().adjustment }
    fn program_range_limit(&mut self, p: Program) -> u32 { program(p).record().range_limit }
    fn program_policy(&mut self, p: Program) -> u32 { program(p).record().policy }
    fn program_secondary_gate(&mut self, p: Program) -> u32 { u32::from(program(p).record().secondary_gate) }
    fn program_scale(&mut self, p: Program) -> u32 { program(p).record().scale }
    fn program_percent(&mut self, p: Program) -> u16 { programs::selection_percent(&mut *self.services, program(p)) as u16 }
    fn program_target(&mut self, inputs: &TargetInputs) -> ProgramTarget {
        let parameters = programs::SettlementInputs {
            percent: i32::from(inputs.reference as i16), reference: inputs.lens.0,
            start: inputs.level, deduction: inputs.gain_offset, preset: inputs.gain_level as i16,
        };
        let target = match inputs.lens_reference {
            Some(lens) => programs::settle_at_lens(&mut *self.services, program(inputs.program), &parameters, lens),
            None => programs::settle_by_steps(&mut *self.services, program(inputs.program), &parameters),
        };
        ProgramTarget {
            adjustment_flag: u32::from(target.raised), base: target.level as u16, level: target.settled as u16,
            exposure: PipelineWord(target.lens), gain_level: inputs.gain_level, second: target.split.held as u16,
            adjustment: target.offset as u16, offset: target.split.rest as u16,
        }
    }
    fn program_range(&mut self, inputs: &RangeInputs) -> Range {
        let parameters = programs::ResolutionInputs {
            lens: inputs.exposure.0, reference: inputs.lens, adjustment: inputs.adjustment,
            mode: inputs.mode, forced: inputs.forced == 1, primary: inputs.primary == 1,
            alternate: inputs.alternate == 1, request: inputs.request,
        };
        let mut level = inputs.level as i16;
        let mut split = programs::Split { held: inputs.first as i16, rest: inputs.second as i16 };
        let range = programs::resolve(&mut *self.services, program(inputs.program), &parameters, &mut level, &mut split);
        Range { timing: TimingWord(range.capture_profile), level: level as u16, first: split.held as u16,
            second: split.rest as u16, code: range.code, selected: range.selector }
    }
    fn program_lower_bound(&mut self, p: Program, lens: u32) -> u32 { programs::lens_limits(&mut *self.services, program(p), lens).near }
    fn profile_percent(&mut self, p: Program, combined: u16, lens: ResourceIndex, bound: u32) -> u16 { programs::choose_percent(&mut *self.services, program(p), i32::from(combined as i16), lens.0, bound) }

    fn grid_count(&mut self, selection: ProfileSelection, mode: AcquisitionMode, _: u32, grid_kind: u32) -> u32 {
        u32::from(xm1_saclac_calc::adjust::search_targets::scene_admitted(&mut *self.services, selection_word(selection) == 1, mode.0, grid_kind == 1))
    }
    fn indexed_configuration(&mut self, selection: ProfileSelection, adjustment: u32, index: u32) -> u8 { rate_policy::policy_setting(&mut *self.services, selection == ProfileSelection::Record, adjustment, index == 1) as u8 }
    fn level_category(&mut self, level: i16, selection: ProfileSelection, adjustment: u32) -> u32 { rate_policy::profile_curve(self, i32::from(level), selection == ProfileSelection::Record, adjustment) }
    fn selection_limits(&mut self, selection: ProfileSelection, adjustment: u32, grid_count: u32, request: u32) -> SelectionLimits {
        let limits = rate_policy::rate_settings(&mut *self.services, selection == ProfileSelection::Record, adjustment, grid_count != 0, request == 2);
        SelectionLimits { step: limits.base, lower: limits.second as u16, upper: limits.first as u16, byte: limits.byte }
    }
    fn grid_lead(&mut self, branch: Branch) -> codec::GridLead { codec::grid_lead(&self.services.acquisition.grids[branch_index(branch)]) }
    fn set_grid_count(&mut self, branch: Branch, count: u32) { codec::set_grid_count(&mut self.services.acquisition.grids[branch_index(branch)], count); }
    fn clear_grid_lead(&mut self, branch: Branch) { codec::clear_grid_lead(&mut self.services.acquisition.grids[branch_index(branch)]); }
    fn set_grid_configuration(&mut self, branch: Branch, configuration: u8) { codec::set_grid_configuration(&mut self.services.acquisition.grids[branch_index(branch)], configuration); }
    fn set_grid_inputs(&mut self, branch: Branch, inputs: &GridInputs) { codec::set_grid_inputs(&mut self.services.acquisition.grids[branch_index(branch)], inputs.timing.0, inputs.lens_word, inputs.positions, inputs.window); }
    fn fill_grid(&mut self, branch: Branch, inputs: &GridInputs) {
        let services = &mut *self.services;
        boundary::fill_grid(&services.machine, inputs, &mut services.acquisition.grids[branch_index(branch)]);
    }
    fn snapshot_grid(&mut self, branch: Branch, statistics: Statistics) {
        let services = &mut *self.services;
        codec::grid_from_snapshot(&mut services.acquisition.grids[branch_index(branch)], &boundary::snapshot(&services.machine, statistics));
    }
    fn adjustment_class(&mut self, adjustment: u32, reference: i16) -> u8 { rate_policy::level_selection(adjustment, reference) }
    fn adjustment_option(&mut self) -> u32 { u32::from(self.services.search.configuration.refresh()) }
    fn refreshed_position(&mut self) -> u32 { boundary::refreshed_position(&self.services.machine) }
    fn mode_one(&mut self, inputs: &ModeInputs) -> u32 { u32::from(treatments(inputs).admits(xm1_saclac_calc::adjust::scene_policy::Treatment::First)) }
    fn both_modes(&mut self, inputs: &ModeInputs) -> u32 { u32::from(treatments(inputs).admits_any()) }
    fn five_parameter_adjustment(&mut self, selection: ProfileSelection, position: u32, mode_flag: u32, modes: u32, grid_word: u32) -> u32 { boundary::five_parameter_adjustment(&self.services.machine, [selection_word(selection), position, mode_flag, modes, grid_word]) }
    fn rate_divisor(&mut self, position: u32, category: u32, lens: ResourceIndex, halfword: u16) -> u32 { boundary::rate_divisor(&self.services.machine, position, category, lens, halfword) }
    fn rate_limit(&mut self, selection: ProfileSelection, position: u32, halfword: u16) -> u32 { boundary::rate_limit(&self.services.machine, selection_word(selection), position, halfword) }
    fn rate_mode(&mut self, position: u32, halfword: u16, word: u32, mode_flag: u32) -> u16 { boundary::rate_mode(&self.services.machine, [position, u32::from(halfword), word, mode_flag]) }
    fn selection_mode_category(&mut self, mode_word: u32) -> u32 { u32::from(xm1_saclac_calc::policy::SelectionMode(mode_word).category()) }
    fn offset_position(&mut self, level: i16, reference: u16, flag: u32) -> u16 { xm1_saclac_calc::policy::offset_position(level, i32::from(reference), flag == 1) as u16 }
    fn clamp_selection_difference(&mut self, mode: AcquisitionMode, lens: ResourceIndex, level: i16, halfword: u16, reference: u32, target: i16) -> i16 { boundary::clamp_selection_difference(&self.services.machine, mode.0, lens, level, halfword, reference, target) }
    fn has_valid_offset(&mut self, selection: ProfileSelection, mode: AcquisitionMode, halfword: i16) -> u32 { u32::from(sample_grid::scene_offset_valid(selection_word(selection), mode.0, halfword)) }
    fn allows_unflagged_selection(&mut self, position: u32, halfword: u16) -> bool { boundary::allows_unflagged_selection(&self.services.machine, position, halfword) }
    fn capture_selection_position(&mut self, branch: Branch, inputs: &PositionInputs) -> SelectionPosition { boundary::capture_selection_position(&self.services.machine, boundary::region(branch).setting, branch, inputs) }
    fn copy_rate_grid_source(&mut self, buffer: StatisticsBuffer) {
        self.recognition.borrow_mut().set_source(&mut *self.services, SharedAddress(buffer.0));
    }
    fn rate_pair(&mut self, record: RecordHandle, result: SharedAddress, flag: u32, pair: &mut [u32; 2]) {
        let services = &mut *self.services;
        let grids = motion_grids(&services.machine, result);
        let mut previous = (pair[0] != u32::MAX).then_some(Centroid { x: pair[0], y: pair[1] });
        let mut profiles = MotionProfiles { records: &mut services.records, entries: &mut services.entries, stock: &services.stock };
        services.search.configuration.measure_motion(&mut profiles, &mut Timing(&services.machine), record, &grids, motion_source(flag), &mut previous);
        *pair = previous.map_or([u32::MAX; 2], |point| [point.x, point.y]);
    }
    fn clear_rate_configuration(&mut self, record: RecordHandle) {
        xm1_focus_measurement_round::profile::ProfileRecords::set_interval_history(&mut *self.services, record, Default::default());
    }
    fn rate_flag(&mut self, record: RecordHandle, flag: u32) {
        let services = &mut *self.services;
        let mut profiles = MotionProfiles { records: &mut services.records, entries: &mut services.entries, stock: &services.stock };
        let machine = &mut services.machine;
        let selection = &mut services.selection_publication;
        services.search.configuration.classify_motion(&mut profiles, |profiles, record| {
            crate::controller_services::Library { machine, records: profiles.records, selection }.publish_selection(record);
        }, record, motion_source(flag));
    }
    fn draw_rate(&mut self, result: SharedAddress, _: u32) {
        let services = &mut *self.services;
        let mut memory = crate::calculation_memory::MachineMemory(&mut services.machine);
        let records = &mut services.records;
        let selection = &mut services.selection_publication;
        services.search.configuration.export_diagnostics(&mut memory,
            |memory| crate::controller_services::Library { machine: memory.0, records, selection }.snapshot_diagnostics(),
            |memory| motion_grids(memory.0, result));
    }
    fn draw_rate_status(&mut self, level: u16, word: u32, flag: u32) { boundary::draw_rate_status(&self.services.machine, level, word, flag) }
    fn rate_position(&mut self, record: RecordHandle) -> (u32, u16) {
        let bytes = self.shared_bytes(record, &shared::group_27::AT_07EC);
        let enabled = u32::from_le_bytes(bytes[..4].try_into().expect("motion found word")) == 1;
        xm1_saclac_calc::adjust::search_targets::scene_and_follow(&mut *self.services, record, enabled)
    }

    fn statistics_buffer(&mut self, buffer: StatisticsBuffer) -> [u16; 256] { boundary::statistics_values(&self.services.machine, buffer) }
    fn extra_statistics(&mut self, values: &mut [u16]) { boundary::extra_statistics(&self.services.machine, values) }
    fn store_release(&mut self, release: &Release<'_>) { boundary::store_release(&mut self.services.machine, release) }
    fn set_pipeline_reference(&mut self, level: i16) { boundary::set_pipeline_reference(&self.services.machine, level) }
    fn prepare_s1_sample(&mut self) { boundary::prepare_s1_sample(&self.services.machine) }
    fn s1_sample(&mut self) -> PipelineSnapshot { xm1_focus_measurement_round::stock::s1_sample(&boundary::s1_sample(&self.services.machine)) }
    fn post_change_notice(&mut self, notice: &ChangeNotice) {
        let bytes = codec::change_notice(notice);
        if ImgDc::request(&mut self.services.peers, ImgDcRequest { operation: ImgDcOperation::Request2d, parameters: wire::parameters(&bytes), completions: &[] }).is_err() { boundary::fault(&self.services.machine); }
    }
    fn post_selection_table(&mut self, table: &SelectionTable) {
        boundary::place(&mut self.services.machine, boundary::SELECTION_TABLE, &codec::selection_table(table));
        if ImgDc::request(&mut self.services.peers, ImgDcRequest { operation: ImgDcOperation::Request2c, parameters: wire::parameters(&[]), completions: &[] }).is_err() { boundary::fault(&self.services.machine); }
    }
    fn post_focus_table(&mut self, table: &FocusTable) {
        boundary::place(&mut self.services.machine, boundary::FOCUS_TABLE, &codec::focus_table(table));
        if ImgDc::request(&mut self.services.peers, ImgDcRequest { operation: ImgDcOperation::Request45, parameters: wire::parameters(&[]), completions: &[] }).is_err() { boundary::fault(&self.services.machine); }
    }

    fn shot_reference_level(&mut self) -> u16 { boundary::shot_reference_level(&self.services.machine) }
    fn shot_selection_word(&mut self) -> u32 { boundary::shot_selection_word(&self.services.machine) }
    fn prepare_selection_defaults(&mut self, record: RecordHandle) -> ShotContext {
        let list = self.with_shot(|state, layers| xm1_shot_sequence::selection::record_list(record, xm1_shot_sequence::selection::ListSource::Record, state, layers));
        let bytes = list.bytes();
        boundary::place(&mut self.services.machine, 0x16b_fc30, bytes);
        ShotContext {
            word: u32::from_le_bytes(bytes[..4].try_into().expect("selection fixed word")),
            words: [8, 12, 16].map(|at| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("selection class"))),
            byte: bytes[0x216],
        }
    }
    fn restore_selection_setting(&mut self, branch: Branch, lens: ResourceIndex) {
        let request = self.with_shot(|state, layers| xm1_shot_sequence::selection_operations::reference_request(lens.0, state, layers));
        boundary::place(&mut self.services.machine, boundary::region(branch).setting.0, request.bytes());
    }
    fn copy_reference_backup(&mut self, branch: Branch) {
        let list = self.with_shot(|state, _| state.saved_selection());
        boundary::place(&mut self.services.machine, boundary::region(branch).shot_copy.0, list.bytes());
    }
    fn write_saved_settings(&mut self, record: SharedAddress) { boundary::write_saved_settings(&self.services.machine, record) }
    fn copy_selection_to_saved(&mut self, record: SharedAddress) {
        self.with_shot(|state, _| state.retain_result_from(xm1_saclac_interface::selection::ResultBuffer(record.0)));
    }
    fn restore_selection_if_enabled(&mut self, record: RecordHandle, position: u32) {
        if self.field(record, &record::group_00::INTERVAL_TRACKING).0 == 0 { return; }
        self.with_shot(|state, layers| {
            state.restore_result();
            xm1_shot_sequence::grid::update_grid(record, true, false, position, state, layers);
        });
    }
    fn apply_selection_value_change(&mut self) { self.with_shot(|state, _| state.set_auxiliary_retained(true)); }
    fn save_selection(&mut self, position: u32, saved: &SavedSelection, lens_position: u32) {
        use xm1_saclac_interface::{calculation::{AreaAt, AreaIndex, GridCell, GridPass, ResultPass}, selection::RetainedReference};
        let untracked = (saved.word == 7).then(|| AreaAt::Cell {
            pass: GridPass::new(saved.index).map_or(ResultPass::Plain, ResultPass::Grid),
            cell: GridCell { column: saved.bytes[0], row: saved.bytes[1] },
            index: AreaIndex::new(saved.flag).expect("retained selection area"),
        });
        self.with_shot(|state, _| xm1_shot_sequence::selection_operations::retain_reference(
            RetainedReference { source: position, reference: lens_position, position: saved.level, untracked }, state));
    }
    fn selected_grid_reference(&mut self, record: RecordHandle, _: GridReference) -> GridReference {
        use xm1_shot_sequence::{absorb::CalculationResults, classification::classify_subjects};
        use xm1_saclac_interface::{calculation::{AreaAt, AreaHalf, AreaIndex, AreaWord, TrackedKind}, selection::{ClassifyStage, SelectionSlot, SubjectQuery, SubjectShare}};
        use xm1_saclac_shared::settings::{AdjustmentKey, Adjustments};
        let mode = self.field(record, &record::group_00::AT_0051);
        let rate = self.shared_bytes(record, &shared::group_27::AT_07EC);
        let services = self.services.source;
        let (mut answer, active, class) = self.with_shot(|state, layers| {
            let control = || services.borrow().stock.byte(AdjustmentKey(0x5257)) & 3;
            let stage = if state.tracking() && control() != 0 {
                let (source, area) = {
                    let services = &mut *services.borrow_mut();
                    (services.entries.header_byte(record, SharedHeaderByte::At31), services.entries.header_byte(record, SharedHeaderByte::At63))
                };
                classify_subjects(ClassifyStage::Round,
                    SubjectShare { mode: u32::from_le_bytes(rate[16..20].try_into().expect("subject mode")), percent: u16::from_le_bytes([rate[20], rate[21]]) },
                    SubjectQuery { kind: u32::from(mode), source: u32::from(source), area: u32::from(area) }, state, layers);
                Some(ClassifyStage::Round)
            } else if control() == 0 { Some(ClassifyStage::Absorb) } else { None };
            let index = AreaIndex::FIRST;
            let pass = state.selected_pass();
            let cell = state.selection_cell(SelectionSlot { kind: None, pass, index }).cell();
            let kind = stage.and_then(|stage| TrackedKind::new(state.area_kind(stage, index)));
            let area = state.area(kind.map_or(AreaAt::Cell { pass, cell, index }, |kind| AreaAt::Tracked { pass, kind, index }));
            (GridReference {
                level: area.half(AreaHalf::Level) as u16, second: area.total() as u16, word: u32::from(mode),
                levels: [AreaHalf::At0c, AreaHalf::At0e, AreaHalf::At10, AreaHalf::At1a].map(|half| area.half(half) as u16),
            }, area.word(AreaWord::At24), area.word(AreaWord::ClassIndex))
        });
        if mode == 1 {
            let mut context = xm1_power_lifecycle::record_address_space::RecordContext::new(
                crate::calculation_memory::MachineMemory(&mut self.services.machine), ());
            answer.word = xm1_power_lifecycle::record_numeric::helper_fa6904(&mut context, active, class).expect("selected-area class address");
        }
        answer
    }

    fn refresh_hardware_state(&mut self) { boundary::refresh_hardware_state(&self.services.machine) }
    fn hardware_ready(&mut self) -> bool { boundary::hardware_ready(&self.services.machine) }
    fn clock_word(&mut self) -> u32 { boundary::clock_word(&self.services.machine) }
    fn store_selection_window(&mut self, branch: Branch, window: &SelectionWindow) {
        let services = &mut *self.services;
        boundary::place(&mut services.machine, boundary::grid_address(branch).0, &services.acquisition.grids[branch_index(branch)].0);
        codec::selection_window(window, boundary::grid_address(branch), |at, bytes| boundary::write_record(&mut services.machine, boundary::region(branch).window, at, bytes));
    }
    fn store_selection_setting(&mut self, branch: Branch, setting: &SelectionSetting) {
        let address = boundary::region(branch).setting;
        codec::selection_setting(setting, address, |at, bytes| boundary::write_record(&mut self.services.machine, address, at, bytes));
    }
    fn store_restored_setting(&mut self, branch: Branch, setting: &RestoredSetting) {
        codec::restored_setting(setting, |at, bytes| boundary::write_record(&mut self.services.machine, boundary::region(branch).setting, at, bytes));
    }
    fn setting_echo(&mut self, branch: Branch) -> SettingEcho {
        let (ready, updated_position, position_halfword, shot_word) = codec::setting_echo(&boundary::read_record(&self.services.machine, boundary::region(branch).setting));
        SettingEcho { ready, updated_position, position_halfword, shot_word }
    }
    fn request_acquisition(&mut self, branch: Branch, request: &AcquisitionRequest, grid: EvaluationGrid) {
        let services = &mut *self.services;
        let grid = match grid { EvaluationGrid::Current => &services.acquisition.grids[branch_index(branch)], EvaluationGrid::Empty => &EMPTY_GRID };
        let mut peers = CalculationPeers { machine: &mut services.machine, peers: &mut services.peers };
        if xm1_saclac_calc::requests::request_acquisition(&mut peers, grid, *request, boundary::acquisition_callback(branch).0).is_err() { boundary::fault(&services.machine); }
    }
    fn request_selection(&mut self, branch: Branch, request: &SelectionRequest) {
        let payload = Calc1Payload::Selection { request: *request, region: boundary::region(branch), callback: boundary::selection_callback(branch).0 };
        if xm1_saclac_calc::requests::Peers::calc1(&mut *self.services, &payload).is_err() { boundary::fault(&self.services.machine); }
    }
    fn acquisition_result(&mut self, completed: Completed<'_>) -> Option<AcquisitionResult> {
        let address = boundary::result_address(completed.parameters.bytes())?;
        Some(codec::acquisition_result(address, &boundary::read_record(&self.services.machine, address)))
    }
    fn selection_result(&mut self, completed: Completed<'_>) -> Option<SelectionResult> {
        let address = boundary::result_address(completed.parameters.bytes())?;
        Some(codec::selection_result(address, &boundary::read_record(&self.services.machine, address)))
    }
    fn shot_record_levels(&mut self, record: SharedAddress) -> ShotRecordLevels { codec::shot_record_levels(&boundary::read_record(&self.services.machine, record)) }
    fn saved_exposure(&mut self, record: SharedAddress) -> SavedExposure { codec::saved_exposure(&boundary::read_record(&self.services.machine, record)) }
    fn publish_saved_exposure(&mut self, record: RecordHandle, exposure: &SavedExposure) {
        store!(self, record, shared::group_27::AT_0708, exposure.exposure); store!(self, record, shared::group_27::AT_070C, exposure.level as i16);
        store!(self, record, shared::group_27::AT_070E, exposure.second as i16); store!(self, record, shared::group_27::AT_0712, exposure.aspect as i16);
        store!(self, record, shared::group_27::AT_0716, exposure.offset as i16);
    }
    fn release_result(&mut self, released: ReleasedResult) {
        match released {
            ReleasedResult::Acquisition(address) => boundary::release(&self.services.machine, address, codec::ACQUISITION_RESULT_BYTES),
            ReleasedResult::Selection { result, shot } => {
                boundary::release(&self.services.machine, result, codec::SELECTION_RESULT_BYTES);
                if let Some((shot, saved)) = shot { boundary::release(&self.services.machine, shot, boundary::SHOT_RECORD_BYTES); boundary::release(&self.services.machine, saved, boundary::SAVED_RECORD_BYTES); }
            }
        }
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P: Calc + Calc1 + ImgDc, S: StateMemory, X: GridLayers> xm1_focus_measurement_round::children::Ports for Acquisition<'_, M, R, E, P, S, X> {
    fn fault(&mut self) { boundary::fault(&self.services.machine) }
}
