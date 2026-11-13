#![forbid(unsafe_code)]

//! Initializing, starting, stopping and terminating the controller, and ending the operations
//! still running when it stops.

use super::changes::{Mask, Stage, Subscription, Work};
use super::{Lifecycle, Operation, OperationState, ProfileGeometry, Refusal, Status, Tables};
use crate::memory::{Block, Location, Memory};
use crate::ports::{HandlerUnit, PipelineChannel, Ports, Stages};
use crate::stock::layout::{self, cleared, controller as field, dump, policy, work};

/// The pipeline channel the second, third and indexed operations and the stage releases name.
const SHARED_CHANNEL: PipelineChannel = PipelineChannel(2);

/// What starting or stopping the controller reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleRefusal {
    Controller(Refusal),
    /// A handler unit refused to attach or detach, with its code.
    Unit(u32),
}

impl LifecycleRefusal {
    pub const fn code(self) -> u32 {
        match self {
            LifecycleRefusal::Controller(refusal) => refusal.code(),
            LifecycleRefusal::Unit(code) => code,
        }
    }
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xed87b0.
    pub fn initialize(&mut self) {
        self.set_lifecycle(Lifecycle::Closed);
        for (operation, bytes) in cleared::OPERATIONS {
            self.memory.fill(field::state(operation), bytes, 0);
        }
        for handle in field::UNIT_HANDLES {
            self.memory.write_u32(handle, 0);
        }
        self.memory.write_u32(field::PIPELINE_CHANNEL, 6);
        self.memory.write_u8(field::RECORD_KIND, 0);
        self.clear(Block::Parameters, cleared::PARAMETERS);
        self.memory.write_u16(layout::parameters::FIRST_DIVISOR, 3);
        self.memory.write_u16(layout::parameters::SECOND_DIVISOR, 4);
        self.clear(Block::Status, cleared::STATUS);
        self.clear(Block::Policy, cleared::POLICY);
        self.declare_dump_regions();
        self.clear(Block::Selection, cleared::SELECTION);
        self.clear(Block::Requests, cleared::REQUESTS);
        self.clear(Block::Changes, cleared::CHANGES);
        self.clear(Block::Work, cleared::WORK);
        self.clear(Block::Counters, cleared::COUNTERS);
        let gains = self.quad_gains();
        self.publish_parameter_diagnostics();
        self.set_diagnostic_rate(self.memory.read_u16(layout::status::RATE) as u32);
        self.set_diagnostic_gains(gains);
        self.memory.write_u32(field::INITIALIZED_CLEARED, 0);
    }

    fn clear(&mut self, block: Block, bytes: u16) { self.memory.fill(Location::new(block, 0), bytes, 0) }

    /// 0xedf558.
    fn declare_dump_regions(&mut self) {
        for (at, address) in dump::BUFFERS {
            self.memory.write_u32(at, address);
        }
        for (region, address) in dump::REGIONS {
            self.ports.reserve_region(region, address, dump::REGION_BYTES);
        }
        for (region, address) in dump::REGIONS {
            self.ports.register_region(region, address, dump::REGION_BYTES);
        }
    }

    /// 0xed89b0.
    pub fn terminate(&mut self) -> Status {
        match self.lifecycle() {
            Some(Lifecycle::Terminated) => Err(Refusal::AlreadyThere),
            Some(Lifecycle::Closed) => {
                self.set_lifecycle(Lifecycle::Terminated);
                self.clear(Block::Requests, cleared::REQUESTS);
                self.clear(Block::Changes, cleared::CHANGES);
                self.clear(Block::Work, cleared::WORK);
                for (region, _) in dump::REGIONS {
                    self.ports.release_region(region);
                }
                Ok(())
            }
            _ => Err(Refusal::WrongState),
        }
    }

    /// 0xedbcf0.
    pub fn start(&mut self) -> Result<(), LifecycleRefusal> {
        match self.lifecycle() {
            Some(Lifecycle::Started) => return Err(LifecycleRefusal::Controller(Refusal::AlreadyThere)),
            Some(Lifecycle::Open) => {}
            _ => return Err(LifecycleRefusal::Controller(Refusal::WrongState)),
        }
        self.clear(Block::Changes, cleared::CHANGES);
        for at in field::START_CLEARED {
            self.memory.write_u32(at, 0);
        }
        self.memory.write_u32(field::CHANNEL_HELD, 0);
        self.memory.write_u8(field::RECORD_KIND, 0);
        self.memory.write_u32(field::SCAN_OPTION, 0);
        self.clear(Block::Work, cleared::WORK);
        let [first, _] = field::UNIT_HANDLES;
        let attached = if self.memory.read_u32(first) == 0 {
            self.ports.attach(HandlerUnit::First).map(|handle| self.memory.write_u32(first, handle))
        } else {
            Ok(())
        };
        self.memory.write_u32(field::PRIMARY_COMPLETIONS, 1);
        self.set_lifecycle(Lifecycle::Started);
        attached.map_err(LifecycleRefusal::Unit)
    }

    /// 0xedbaf4.
    pub fn stop(&mut self) -> Result<(), LifecycleRefusal> {
        match self.lifecycle() {
            Some(Lifecycle::Open) => return Err(LifecycleRefusal::Controller(Refusal::AlreadyThere)),
            Some(Lifecycle::Started) => {}
            _ => return Err(LifecycleRefusal::Controller(Refusal::WrongState)),
        }
        let mut outcome = Ok(());
        for (unit, at) in [HandlerUnit::First, HandlerUnit::Second].into_iter().zip(field::UNIT_HANDLES) {
            let handle = self.memory.read_u32(at);
            if handle != 0 {
                let code = self.ports.detach(unit, handle);
                self.memory.write_u32(at, 0);
                outcome = if code == 0 { Ok(()) } else { Err(LifecycleRefusal::Unit(code)) };
            }
        }
        let refused = Err(LifecycleRefusal::Controller(Refusal::PipelineRefused));
        if !self.operation(Operation::Indexed).idle() && self.finish_indexed().is_err() {
            outcome = refused;
        }
        if !self.operation(Operation::Primary).idle() {
            if self.finish_primary().is_err() {
                outcome = refused;
            }
            self.memory.write_u32(field::PRIMARY_COMPLETIONS, 0);
        }
        if !self.operation(Operation::Auxiliary).idle() && self.finish_auxiliary().is_err() {
            outcome = refused;
        }
        if !self.operation(Operation::First).idle() && self.finish_first().is_err() {
            outcome = refused;
        }
        if !self.operation(Operation::Second).idle() && self.finish_second().is_err() {
            outcome = refused;
        }
        if !self.operation(Operation::Third).idle() && self.finish_third().is_err() {
            return refused;
        }
        if outcome.is_ok() {
            self.ports.settle();
            self.unsubscribe_fourth();
            self.unsubscribe_scans();
            self.unsubscribe(Subscription::Format);
            self.ports.release_capture_hardware();
            self.ports.release(SHARED_CHANNEL, Stages(0x1e));
            if self.primary_quiet() {
                self.unsubscribe(Subscription::Output);
                self.ports.release(SHARED_CHANNEL, Stages(0x1));
            }
            self.set_lifecycle(Lifecycle::Open);
            self.memory.write_u32(field::PRIMARY_FLAG, 0);
            self.clear(Block::Changes, cleared::CHANGES);
            self.clear(Block::Work, cleared::WORK);
        }
        outcome
    }

    fn stop_operation(&mut self, operation: Operation, channel: PipelineChannel, stages: Stages) -> Status {
        self.set_operation(operation, OperationState::STOPPING);
        if !self.ports.stop(channel, stages) {
            return Err(Refusal::PipelineRefused);
        }
        self.set_operation(operation, OperationState::IDLE);
        Ok(())
    }

    fn notify_stopped(&mut self, operation: Operation) {
        let callback = self.completion(operation);
        if callback.is_set() {
            self.ports.notify_stopped(operation, callback);
            self.clear_completion(operation);
        }
    }

    /// 0xee37dc.
    pub(crate) fn finish_primary(&mut self) -> Status {
        self.stop_operation(Operation::Primary, self.pipeline_channel(), Stages(0x80))?;
        self.notify_stopped(Operation::Primary);
        Ok(())
    }

    /// 0xee3860.
    pub(crate) fn finish_auxiliary(&mut self) -> Status {
        self.stop_operation(Operation::Auxiliary, self.pipeline_channel(), Stages(0x2000))?;
        self.notify_stopped(Operation::Auxiliary);
        Ok(())
    }

    /// 0xee38d0.
    pub(crate) fn finish_first(&mut self) -> Status {
        let stages = Stages(self.memory.read_u32(field::FIRST_STAGES));
        self.stop_operation(Operation::First, self.pipeline_channel(), stages)?;
        self.memory.write_u32(field::FIRST_STAGES, 0);
        self.memory.write_u32(field::SELECTED, 0);
        self.notify_stopped(Operation::First);
        Ok(())
    }

    /// 0xee394c.
    pub(crate) fn finish_second(&mut self) -> Status {
        self.stop_operation(Operation::Second, SHARED_CHANNEL, Stages(0xc00))?;
        self.memory.write_u32(field::SELECTED, 0);
        self.notify_stopped(Operation::Second);
        Ok(())
    }

    /// 0xee3d44.
    pub(crate) fn finish_third(&mut self) -> Status {
        self.set_operation(Operation::Third, OperationState::IDLE);
        let outcome = self.finish_channel();
        self.notify_stopped(Operation::Third);
        outcome
    }

    /// 0xee3c48.
    fn finish_channel(&mut self) -> Status {
        if !self.operation(Operation::Channel).running() {
            return Err(Refusal::PipelineRefused);
        }
        self.set_operation(Operation::Channel, OperationState::STOPPING);
        if !self.ports.stop(SHARED_CHANNEL, Stages(0x3f)) {
            return Err(Refusal::PipelineRefused);
        }
        if self.channel_quiet() {
            if self.all_quiet() {
                let _ = self.release(Stage::Base);
            }
            if self.format_quiet() {
                self.release_payloads();
                let _ = self.release_modes();
                let _ = self.release(Stage::QuadSelection);
                let _ = self.release(Stage::Tertiary);
                let _ = self.release(Stage::Format);
            } else {
                self.request(Work::QuadSelection);
            }
            self.release_scan_if_quiet();
            let _ = self.send_disable();
            self.unsubscribe_when_quiet();
        }
        self.set_operation(Operation::Channel, OperationState::IDLE);
        self.memory.write_u32(field::CHANNEL_TOGGLE, 0);
        Ok(())
    }

    /// 0xee4084.
    pub(crate) fn finish_indexed(&mut self) -> Status {
        self.set_operation(Operation::Indexed, OperationState::IDLE);
        let outcome = self.reset_selected();
        self.notify_stopped(Operation::Indexed);
        outcome
    }

    /// 0xee3fbc.
    fn reset_selected(&mut self) -> Status {
        if !self.ports.stop(SHARED_CHANNEL, Stages(0x40)) {
            return Err(Refusal::PipelineRefused);
        }
        if !self.channel_quiet() {
            return Ok(());
        }
        if self.all_quiet() {
            let _ = self.release(Stage::Base);
        }
        if self.format_quiet() {
            self.release_payloads();
            let _ = self.release_modes();
            let _ = self.release(Stage::QuadSelection);
            let _ = self.release(Stage::Format);
        } else {
            self.request(Work::QuadSelection);
        }
        self.release_scan_if_quiet();
        let _ = self.send_disable();
        self.unsubscribe_when_quiet();
        Ok(())
    }

    fn release_scan_if_quiet(&mut self) {
        if self.subscription_word(Subscription::ScanConfiguration) != 0
            && self.all_idle(&[Operation::First, Operation::Second, Operation::Third, Operation::Indexed])
        {
            self.release_scan_claim();
        }
    }

    fn unsubscribe_when_quiet(&mut self) {
        if self.primary_quiet() {
            self.unsubscribe(Subscription::Output);
        }
        if self.first_quiet() {
            self.unsubscribe(Subscription::Format);
        }
        if self.all_idle(&[Operation::Third, Operation::Indexed]) {
            self.unsubscribe_fourth();
        }
    }

    /// 0xee47ec.
    pub(crate) fn send_disable(&mut self) -> Status {
        let stages = self.mask(Mask::Disable);
        if stages == 0 {
            return Ok(());
        }
        let accepted = self.ports.disable(self.pipeline_channel(), Stages(stages));
        self.clear_mask(Mask::Disable);
        if accepted { Ok(()) } else { Err(Refusal::PipelineRefused) }
    }

    /// 0xee4790.
    pub(crate) fn send_enable(&mut self) -> Status {
        let stages = self.mask(Mask::Enable);
        if stages == 0 {
            return Ok(());
        }
        let accepted = self.ports.enable(self.pipeline_channel(), Stages(stages), work::PAYLOADS);
        self.clear_mask(Mask::Enable);
        if accepted { Ok(()) } else { Err(Refusal::PipelineRefused) }
    }

    /// 0xee345c.
    pub(crate) fn primary_quiet(&self) -> bool {
        self.all_quiet() && self.memory.read_u32(field::PRIMARY_HOLD) == 0
    }

    /// 0xee5a04.
    pub(crate) fn all_quiet(&self) -> bool {
        self.all_idle(&[
            Operation::Primary,
            Operation::Auxiliary,
            Operation::First,
            Operation::Second,
            Operation::Third,
            Operation::Indexed,
        ])
    }

    /// 0xee3514.
    pub(crate) fn first_quiet(&self) -> bool {
        self.all_idle(&[Operation::First, Operation::Auxiliary, Operation::Second, Operation::Third, Operation::Indexed])
            && (self.memory.read_u32(field::PRIMARY_FLAG) != 1 || self.operation(Operation::Primary).idle())
    }

    /// 0xee5aa4.
    pub(crate) fn format_quiet(&self) -> bool { self.subscription_word(Subscription::Format) != 0 && self.first_quiet() }

    /// 0xee3be0.
    fn channel_quiet(&self) -> bool {
        let stopping_or = |operation, other: OperationState| {
            let state = self.operation(operation);
            state == OperationState::STOPPING || state == other
        };
        stopping_or(Operation::Channel, OperationState::RUNNING)
            && stopping_or(Operation::Third, OperationState::IDLE)
            && stopping_or(Operation::Indexed, OperationState::IDLE)
    }

    /// Drops the configuration the pipeline holds for the policy block (0xedc628).
    pub(crate) fn release_policy_configuration(&mut self) {
        if self.memory.read_u32(policy::CONFIGURATION) == 0 {
            return;
        }
        self.ports.release_configuration_hardware();
        self.memory.fill(policy::CONFIGURATION, policy::CONFIGURATION_BYTES, 0);
    }
}
