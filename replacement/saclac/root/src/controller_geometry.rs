#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_controllers::layers::{CorrectedWindow, DisplayRecords, RecordChannel, RecordMode, StatisticsGeometry, WindowInputs};
use xm1_saclac_imaging::{capture::gains::{self, GainClass, GainRequest, GainSources}, driver::Isp};
use xm1_saclac_interface::ImageProcessor;
use xm1_saclac_profile_geometry::{controller::{ProfileGeometry, Tables}, ports::DumpBuffer, record_mode, services::OperationConditions, profile::ProfileHandle, stock::records::{Parameters, StatusSnapshot}};
use xm1_saclac_shared::stock::Machine;
use crate::{capture_gains::Sources as GainPorts, geometry::GeometryBindings, stock::geometry::Memory};

type Layer<M, P, I, T> = ProfileGeometry<Memory<M>, GeometryBindings<M, P, I>, T>;

pub struct GeometryServices<'a, M, P, I, T> {
    pub geometry: &'a RefCell<Layer<M, P, I, T>>,
}

impl<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables> DisplayRecords for GeometryServices<'_, M, P, I, T> {
    fn reset(&mut self) { self.geometry.borrow_mut().ports().stages.machine.enter(0xee_cd94, &[]); }
    fn parameters(&mut self) -> Parameters { self.geometry.borrow().parameters() }
    fn status(&mut self) -> StatusSnapshot { self.geometry.borrow().status_snapshot() }
    fn apply_parameters(&mut self, parameters: &Parameters) { let _ = self.geometry.borrow_mut().apply_parameters(parameters); }
    fn apply_status(&mut self, status: &StatusSnapshot) { let _ = self.geometry.borrow_mut().apply_status(status); }
}

fn record_channel(channel: RecordChannel) -> record_mode::RecordChannel {
    match channel { RecordChannel::LiveView => record_mode::RecordChannel::SINGLE, RecordChannel::S1 => record_mode::RecordChannel::STATISTICS }
}
impl<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables> RecordMode for GeometryServices<'_, M, P, I, T> {
    fn initialize(&mut self) -> bool { self.geometry.borrow_mut().initialize_record_mode().is_ok() }
    fn available(&mut self, channel: RecordChannel) -> bool { self.geometry.borrow_mut().record_mode_available(record_channel(channel)) }
    fn begin(&mut self, channel: RecordChannel) { let _ = self.geometry.borrow_mut().begin_record_mode(record_channel(channel)); }
    fn finish(&mut self) { let _ = self.geometry.borrow_mut().finish_record_mode(u16::MAX); }
}
impl<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables> StatisticsGeometry for GeometryServices<'_, M, P, I, T> {
    fn correct_window(&mut self, window: [u16; 3], first: u32, second: u32, class: u32) -> [u16; 3] {
        self.geometry.borrow_mut().apply_profile_factors(window, first, second, class)
    }
    fn linearize(&mut self, value: u32) -> u32 { self.geometry.borrow_mut().ports().stages.machine.enter(0xee_9ab0, &[value as usize]) }
    fn dump_pipeline(&mut self, buffer: u32) { self.geometry.borrow_mut().dump_pipeline((buffer != 0).then_some(DumpBuffer(buffer))); }
}


impl<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables> GeometryServices<'_, M, P, I, T> {
    pub fn corrected_window(&mut self, input: &WindowInputs) -> CorrectedWindow {
        corrected_window(&mut *self.geometry.borrow_mut(), input)
    }

    pub fn configured_operation(&mut self, values: [u32; 11]) -> u32 {
        let conditions = operation_conditions(values);
        u32::from(self.geometry.borrow_mut().configured_operation(&conditions))
    }

    pub fn operation_available(&mut self, record: u32) -> u32 { u32::from(self.geometry.borrow_mut().configured_operation_available(record)) }
}

pub(crate) fn operation_conditions(values: [u32; 11]) -> OperationConditions {
    OperationConditions {
        requested: values[0], alternate: values[1], held: values[2], vetoes: [values[3], values[5], values[8]],
        mode: values[4], profile: ProfileHandle(values[7]), busy: values[9], stage: values[10],
    }
}

pub(crate) fn corrected_window<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables>(geometry: &mut Layer<M, P, I, T>, input: &WindowInputs) -> CorrectedWindow {
    let (window, class) = {
        let machine = &geometry.ports().stages.machine;
        let mut ports = GainPorts(machine);
        let custom = input.measured.unwrap_or_else(|| ports.class_gains(GainClass(5), 0));
        let request = GainRequest {
            mode: input.scene,
            variant: input.words[0],
            custom_source: input.words[1],
            bank: input.words[2],
            fixed: core::array::from_fn(|index| u16::from_le_bytes([input.bytes[2 * index], input.bytes[2 * index + 1]])),
            preset: 0,
            temperature: input.mode,
            column: 500,
            level: 600,
            custom,
            variant_set: 0,
            forced: true,
            variant_subset: 0,
        };
        gains::gains(&mut ports, &request)
    };
    CorrectedWindow { window: geometry.apply_profile_factors(window, input.factors[0], input.factors[1], input.class), extent: class.0 }
}
