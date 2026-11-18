#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::motion::queries, exposure::ports::ProfileAccess, finish::{self, TargetLens, TargetQuery}, lens_checks};
use xm1_saclac_config::{channel, table::{self, Row, Step}};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, Calc, Calc1, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{settings::Channel, stock::Machine};
use xm1_shot_sequence::{boundary::{LensPoint, LensReference}, finish_work::{FinishLibrary, PositionAdjustment}};
use crate::{round_ports::RoundPorts, services::Services, shot_services::Libraries};

impl<M: Machine + Clone, R, E, P> TargetLens for Services<M, R, E, P> {
    fn resource_value(&mut self, resource: u32) -> u32 { channel::base(&self.stock, Channel(resource as i32)) }
    fn geometry_value(&mut self, position: i16, reference: u32, control: u32, lens_control: u32, kind: u32) -> u32 {
        self.machine.enter(0x749be4, &[position as i32 as u32 as usize, reference as usize, control as usize, lens_control as usize, kind as usize])
    }
    fn mapped_value(&mut self, value: u32, bank: u32) -> u32 { u32::from(table::value(&self.stock, Step(value), Row(bank))) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables, A, Q>
    FinishLibrary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn point_selection(&mut self, point: &LensPoint, lens_reference: LensReference, state: u32) -> u32 {
        let mut services = self.services.borrow_mut();
        let mut geometry = self.geometry.borrow_mut();
        let mut ports = RoundPorts::new(&mut services, &mut geometry, self.recognition, self.live_search_record.get());
        lens_checks::point_zone(&mut ports, &lens_checks::LensPoint { position: point.position, side: point.kind }, lens_reference.0, state).code()
    }
    fn target_selection(&mut self, record: RecordHandle, lens_reference: LensReference, position: i16, control: u32, kind: u32, lens_control: u32, frame: u32) -> u32 {
        finish::target_selection(&mut *self.services.borrow_mut(), TargetQuery {
            record, lens_reference: lens_reference.0, position, control, kind, lens_control,
            frame: (frame != 0).then_some(SharedAddress(frame)),
        }).code()
    }
    fn mode_selection(&mut self, record: RecordHandle, mode: u32) -> u32 {
        u32::from(self.services.borrow_mut().profile().style_option(record, mode))
    }
    fn correction(&mut self, point: &LensPoint, drive: u32, frame_drive: u32) -> u32 {
        // f18308..18 passes point reference/state/position/kind, then drive and frame drive.
        // fa3640 sign-extends the resulting halfword.
        queries::step_offset(&mut *self.services.borrow_mut(), point.reference.0, point.state, point.position, point.kind, drive, frame_drive) as i32 as u32
    }
    fn adjust_position(&mut self, adjustment: PositionAdjustment) -> i16 {
        // f47300 forwards seven words; the caller's eighth record argument is not consumed.
        xm1_power_lifecycle::record_profile_mapping::helper_74947c(
            &mut crate::calibration_inputs::runtime(&self.services.borrow().machine),
            adjustment.position as i32 as u32, adjustment.reference.0, adjustment.state, adjustment.control,
            u32::from(adjustment.first), u32::from(adjustment.second), adjustment.kind,
        ).expect("finish position remap") as i16
    }
}

impl<M: crate::command_source::CommandSource + Clone, R: Memory, E: StockEntries, P, I, T, A, Q>
    xm1_shot_sequence::finish_work::FinishSources for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn target_source(&mut self) -> xm1_shot_sequence::finish_work::TargetSource {
        let waiting = self.services.borrow().s1_waiting;
        xm1_shot_sequence::finish_work::TargetSource { capture: waiting.capture, lens_control: waiting.bridge, live: waiting.release }
    }
    fn capture_slots(&mut self) -> (u8, u8) {
        let parameters = xm1_shot_sequence::shot_ports::ShotCommands::capture_parameters(self);
        let services = self.services.borrow();
        (services.machine.read_u8(parameters.selection.0), services.machine.read_u8(parameters.selection.0.wrapping_add(1)))
    }
    fn capture_target(&mut self, index: usize) -> u32 {
        let parameters = xm1_shot_sequence::shot_ports::ShotCommands::capture_parameters(self);
        let services = self.services.borrow();
        let slot = services.machine.read_u32(parameters.slots.0.wrapping_add((index as u32).wrapping_mul(4)));
        services.machine.read_u32(slot.wrapping_add(4))
    }
    fn frame_drive(&mut self, frame: xm1_shot_sequence::boundary::Frame) -> u32 {
        self.services.borrow().machine.read_u32(frame.0.wrapping_add(xm1_shot_sequence::stock::finish::FRAME_DRIVE))
    }
    fn store_frame_correction(&mut self, frame: xm1_shot_sequence::boundary::Frame, correction: u32) {
        self.services.borrow().machine.clone().write_u32(frame.0.wrapping_add(xm1_shot_sequence::stock::finish::FRAME_CORRECTION), correction);
    }
    fn capture_status(&mut self) -> u32 {
        self.services.borrow().machine.read_u32(xm1_shot_sequence::stock::finish::CAPTURE_STATUS)
    }
}

use xm1_saclac_profile_records::{field::Index, stock::{record as r, shared as s}, SharedHeaderByte};
use xm1_shot_sequence::finish_work::{AreaPositions, CorrectionRecord, FinishConfiguration, FinishLayers, FinishLensValue, FinishProfile, FinishTrace};
use xm1_shot_sequence::stock::finish as wire;
use xm1_saclac_config::{adjustment, limits, ports::Derivations};
use xm1_saclac_shared::lens::{self, LensBounds};
use xm1_power_lifecycle::record_profile_globals;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q>
    FinishLayers for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn profile(&mut self, record: RecordHandle, value: FinishProfile) -> u32 {
        let mut services = self.services.borrow_mut();
        match value {
            FinishProfile::Override => u32::from(services.entries.header_byte(record, SharedHeaderByte::At47)),
            FinishProfile::SelectionClass => u32::from(xm1_saclac_calc::profile::selection_code(services.entries.measurement_selection(record))),
            FinishProfile::LensMode => u32::from(services.field(record, &r::group_00::AT_0066)),
            FinishProfile::LensKind => services.field(record, &r::group_13::AT_20B0_4),
            FinishProfile::AreaFlag => u32::from(services.field(record, &r::group_00::AT_0064)),
            FinishProfile::LensControl => services.records.read_element(record, &r::group_05::AT_0170, Index::new(0).expect("first lens control")).unwrap_or(0),
            FinishProfile::LiveTarget => services.shared(record, &s::group_27::AT_0708),
            FinishProfile::DefaultTarget => services.shared(record, &s::group_26::AT_0200),
        }
    }
    fn lens(&mut self, value: FinishLensValue) -> u32 {
        let services = self.services.borrow();
        let mut context = crate::calibration_inputs::runtime(&services.machine);
        match value {
            FinishLensValue::Position => record_profile_globals::helper_76da84(&mut context).expect("lens position"),
            FinishLensValue::PrefersRelative => record_profile_globals::helper_76dac4(&mut context).expect("lens relative preference"),
            FinishLensValue::Drive => record_profile_globals::helper_76f340(&mut context).expect("lens drive"),
            FinishLensValue::Direction => record_profile_globals::helper_76dad4(&mut context).expect("lens direction"),
            FinishLensValue::Distance => record_profile_globals::helper_76d9c8(&mut context).expect("lens distance"),
            FinishLensValue::ReferenceLimit => channel::count(&services.stock),
            FinishLensValue::MotorState => record_profile_globals::helper_76da38(&mut context).expect("lens motor state"),
            FinishLensValue::FocusState => record_profile_globals::helper_76d9a8(&mut context).expect("lens focus state"),
            FinishLensValue::Step => services.machine.read_u32(0x00942ab0 + 0x98),
            FinishLensValue::Busy | FinishLensValue::Manual => services.machine.enter(wire::lens_accessor(value), &[]),
        }
    }
    fn lens_pair(&mut self) -> (u32, u16) {
        let services = self.services.borrow();
        let value = record_profile_globals::helper_76d9c8(&mut crate::calibration_inputs::runtime(&services.machine)).expect("lens pair reference");
        if (value as i32) < 0 || value >= channel::count(&services.stock) { return (0, 0); }
        let mut output = crate::stock::acquisition::Aligned([0; 6]);
        output[..4].copy_from_slice(&value.to_le_bytes());
        services.machine.enter_with(0x768b20, &[xm1_saclac_shared::stock::Argument::Buffer(0), xm1_saclac_shared::stock::Argument::Buffer(4)], &mut output.0);
        (u32::from_le_bytes(output[..4].try_into().expect("lens pair word")), u16::from_le_bytes(output[4..6].try_into().expect("lens pair fraction")))
    }
    fn configuration(&mut self, value: FinishConfiguration) -> u32 {
        let services = self.services.borrow();
        let key = wire::configuration_entry(value);
        if value == FinishConfiguration::PositionUnit { u32::from(adjustment::half(&services.stock, key)) } else { u32::from(adjustment::byte(&services.stock, key)) }
    }
    fn load_test(&mut self) -> u32 { self.services.borrow().machine.enter(wire::LOAD_TEST, &[]) }
    fn trace(&mut self, trace: FinishTrace) { self.services.borrow().machine.enter(0x72872c, &[wire::trace_word(trace) as usize]); }
    fn designated_point(&mut self, record: RecordHandle) -> LensPoint {
        let bytes = self.services.borrow().field(record, &r::group_02::AT_00D4);
        let words: [u32; 4] = core::array::from_fn(|i| u32::from_le_bytes(bytes[4*i..4*i+4].try_into().expect("lens point word")));
        LensPoint { position: words[0] as i16, detail: (words[0] >> 16) as u16, reference: LensReference(words[1]), state: words[2], kind: words[3] }
    }
    fn area_positions(&mut self, record: RecordHandle) -> AreaPositions { wire::area_positions(&self.services.borrow().shared_bytes(record, &s::group_34::AT_0814)) }
    fn store_area_positions(&mut self, record: RecordHandle, positions: &AreaPositions) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_34::AT_0814, wire::area_positions_bytes(positions));
    }
    fn correction_record(&mut self, record: RecordHandle) -> CorrectionRecord {
        let body = self.services.borrow().shared_bytes(record, &s::group_19::AT_0120);
        let mut bytes = [0; 64];
        bytes[..28].copy_from_slice(&body);
        CorrectionRecord { enabled: u32::from_le_bytes(body[..4].try_into().expect("correction enabled")), pending: u32::from_le_bytes(body[24..28].try_into().expect("correction pending")), bytes }
    }
    fn store_correction_record(&mut self, record: RecordHandle, correction: &CorrectionRecord) {
        let mut bytes: [u8; 28] = correction.bytes[..28].try_into().expect("correction body");
        bytes[..4].copy_from_slice(&correction.enabled.to_le_bytes());
        bytes[24..28].copy_from_slice(&correction.pending.to_le_bytes());
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_19::AT_0120, bytes);
    }
    fn clamp_step(&mut self, position: i16, kind: u32, direction: u32) -> i16 {
        let services = self.services.borrow();
        crate::calibration_inputs::Native(&services.machine, &services.stock).adjust_direction(position, kind, direction)
    }
    fn adjust(&mut self, position: i16, point: &LensPoint, area_flag: bool) -> i16 {
        xm1_power_lifecycle::record_profile_mapping::helper_74947c(&mut crate::calibration_inputs::runtime(&self.services.borrow().machine),
            position as i32 as u32, point.reference.0, point.state, 0, u32::from(area_flag), 0, point.kind).expect("finish point remap") as i16
    }
    fn near_bounds(&mut self, position: i16, distance: u32, target: u32, drive: u32, direction: u32, step: u32) -> [u32; 3] {
        let services = self.services.borrow();
        let answer = limits::curve_positions(&services.stock, &mut crate::calibration_inputs::Native(&services.machine, &services.stock), position, Channel(distance as i32),
            limits::PositionQuery { first: target, export_first: drive, direction_mode: direction, export_second: step });
        [answer.centre, answer.lower, answer.upper]
    }
    fn far_bounds(&mut self, position: i16, pair: (u32, u16), target: u32, drive: u32, direction: u32, step: u32) -> [u32; 3] {
        let services = self.services.borrow();
        limits::interpolated_curve_positions(&services.stock, &mut crate::calibration_inputs::Native(&services.machine, &services.stock), position, Channel(pair.0 as i32), u32::from(pair.1),
            limits::PositionQuery { first: target, export_first: drive, direction_mode: direction, export_second: step })
            .map_or([u32::MAX; 3], |answer| [answer.centre, answer.lower, answer.upper])
    }
    fn set_lens_target(&mut self, position: i16, reference: LensReference, drive: u32, kind: u32) {
        let services = self.services.borrow();
        if channel::is_valid(&services.stock, Channel(reference.0 as i32)) {
            // f18678..88 supplies zero as geometry's third argument.
            let value = services.machine.enter(0x749be4, &[position as i32 as u32 as usize, reference.0 as usize, 0, drive as usize, kind as usize]);
            services.machine.enter(0x740384, &[value as usize]);
        }
    }
    fn store_area_selection(&mut self, record: RecordHandle, index: u8, selection: u32) {
        if let Some(index) = Index::new(usize::from(index)) { let _ = self.services.borrow_mut().records.write_element(record, &r::group_08::AT_01F8, index, selection); }
    }
    fn store_area_mode(&mut self, record: RecordHandle, index: u8, mode: u32) {
        if let Some(index) = Index::new(usize::from(index)) { let _ = self.services.borrow_mut().records.write_element(record, &r::group_08::AT_01F4, index, mode); }
    }
    fn notify_bounds(&mut self, bounds: [u32; 3]) { lens::notify_bounds(0, LensBounds(bounds), &mut self.services.borrow_mut().stock); }
}
