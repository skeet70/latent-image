#![forbid(unsafe_code)]

//! The operations refresh: for every running operation and subscription, the payload and
//! geometry work a change left, then the four stage masks handed to the pipeline.

use super::changes::{Mask, Subscription, Work};
use super::{Operation, ProfileGeometry, Refusal, Status, Tables};
use crate::memory::Memory;
use crate::ports::{Ports, Stages};
use crate::profile::ProfileKind;
use crate::stock::layout::{controller as field, policy, work};

/// Alternate geometry moves these geometry records by the profile's offset pair, in pixels.
const OFFSET_RECORDS: [(u32, &[(u16, u16)]); 2] = [
    (0x4000, &[(0x198, 0x19c)]),
    (0x2000, &[(0x1a0, 0x1a2), (0x1a4, 0x1a6)]),
];
/// Alternate geometry moves these geometry records by twice the offset pair.
const DOUBLED_OFFSET_RECORDS: [(u32, (u16, u16)); 2] = [(0x800, (0x14, 0x18)), (0x1000, (0x4c, 0x50))];

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xee364c.
    pub(crate) fn refresh_operations(&mut self) -> Status {
        if self.operation(Operation::Primary).running() {
            let mode = self.memory.read_u32(field::CROP_MODE);
            let _ = self.prepare_crop(mode);
            self.reset_pending_pair();
            let _ = self.rebuild_scan_configuration();
        }
        if self.operation(Operation::First).running() {
            self.refresh_scan_operation();
            self.update_primary_scan_geometry();
            self.update_secondary_scan_geometry();
        }
        if self.operation(Operation::Second).running() {
            self.refresh_scan_operation();
        }
        if self.operation(Operation::Third).running() {
            self.reset_pending_pair();
            self.prepare_compact_rectangles();
            self.prepare_extended_rectangles();
            self.prepare_rectangle_set();
            self.prepare_record_rectangles();
            self.select_quad();
            self.update_modes();
            self.update_tertiary();
            let _ = self.rebuild_scan_configuration();
        }
        if self.operation(Operation::Indexed).running() {
            self.prepare_indexed_pair();
            let _ = self.rebuild_scan_configuration();
        }
        if self.subscribed(Subscription::Output) {
            self.clear_request(Work::Output);
        }
        if self.subscribed(Subscription::Format) {
            let _ = self.send_format();
            self.update_payloads();
        }
        self.send_geometry()?;
        self.send_apply()?;
        self.restore_policy_stages();
        self.send_enable()?;
        self.send_disable()
    }

    fn refresh_scan_operation(&mut self) {
        let _ = self.rebuild_scan_configuration();
        self.prepare_scan_rectangles(super::windows::ScanSide::Primary);
        self.prepare_scan_rectangles(super::windows::ScanSide::Secondary);
        self.reset_pending_pair();
        self.select_quad();
        self.update_modes();
        self.update_tertiary();
    }

    /// 0xee7528.
    fn reset_pending_pair(&mut self) {
        if self.requested(Work::PendingPair) {
            for at in work::PENDING_PAIR {
                self.memory.write_u16(at, 0);
            }
            self.add_to_mask(Mask::Geometry, 0x4);
            self.clear_request(Work::PendingPair);
        }
    }

    /// Whether a configured profile kind of zero takes its geometry from the class remainders.
    pub(crate) fn remainder_geometry(&self) -> bool {
        self.memory.read_u32(field::REMAINDER_GEOMETRY) != 0
            && self.profiles.entry(self.current_profile()).kind == ProfileKind(0)
    }

    /// 0xee6fd4.
    pub(crate) fn alternate_offsets(&mut self) -> (u32, u32) {
        let profile = self.current_profile();
        let expanded = self.ports.expanded_rectangle(profile);
        let configured = self.ports.configured_rectangle(profile);
        let class = self.profiles.class_of(profile);
        let mut first = expanded.top.wrapping_sub(configured.top);
        let mut second = expanded.left.wrapping_sub(configured.left);
        if class.first_flag != 0 {
            first >>= 1;
        }
        if class.second_flag != 0 {
            second >>= 1;
        }
        (first / self.profiles.selection_multiplier(profile), second)
    }

    /// Moves the geometry records `stages` names by the alternate geometry's offsets (0xee7088).
    fn offset_geometry_records(&mut self, stages: u32) {
        if !self.remainder_geometry() {
            return;
        }
        let (first, second) = self.alternate_offsets();
        for (stage, records) in OFFSET_RECORDS {
            if stages & stage != 0 {
                for &(first_at, second_at) in records {
                    self.subtract_u16(work::GEOMETRY_RECORDS.plus(first_at), first);
                    self.subtract_u16(work::GEOMETRY_RECORDS.plus(second_at), second);
                }
            }
        }
        for (stage, (first_at, second_at)) in DOUBLED_OFFSET_RECORDS {
            if stages & stage != 0 {
                self.subtract_u32(work::GEOMETRY_RECORDS.plus(first_at), first << 1);
                self.subtract_u32(work::GEOMETRY_RECORDS.plus(second_at), second << 1);
            }
        }
    }

    /// Moves the payloads `stages` names by the alternate geometry's offsets (0xee458c).
    fn offset_payloads(&mut self, stages: u32) {
        if !self.remainder_geometry() {
            return;
        }
        let (first, second) = self.alternate_offsets();
        if stages & 0x8 != 0 {
            self.memory.write_u16(work::SECONDARY_PAYLOAD_OFFSET, 0);
        }
        for (stage, (first_at, second_at)) in work::OFFSET_PAYLOADS {
            if stages & stage != 0 {
                self.subtract_u16(first_at, first);
                self.subtract_u16(second_at, second);
            }
        }
        for (stage, (first_at, second_at)) in work::DOUBLED_OFFSET_PAYLOADS {
            if stages & stage != 0 {
                self.subtract_u32(first_at, first << 1);
                self.subtract_u32(second_at, second << 1);
            }
        }
    }

    fn subtract_u16(&mut self, at: crate::memory::Location, value: u32) {
        let current = self.memory.read_u16(at);
        self.memory.write_u16(at, current.wrapping_sub(value as u16));
    }

    fn subtract_u32(&mut self, at: crate::memory::Location, value: u32) {
        let current = self.memory.read_u32(at);
        self.memory.write_u32(at, current.wrapping_sub(value));
    }

    /// Sends the geometry records the mask names (0xee71a0).
    fn send_geometry(&mut self) -> Status {
        let stages = self.mask(Mask::Geometry);
        if stages == 0 {
            return Ok(());
        }
        self.offset_geometry_records(stages);
        let accepted = self.ports.configure(self.pipeline_channel(), Stages(stages), work::GEOMETRY_RECORDS);
        self.clear_mask(Mask::Geometry);
        if accepted { Ok(()) } else { Err(Refusal::PipelineRefused) }
    }

    /// Applies the payloads the mask names and marks the ones the next frame completes
    /// (0xee458c).
    fn send_apply(&mut self) -> Status {
        let stages = self.mask(Mask::Apply);
        if stages == 0 {
            return Ok(());
        }
        self.offset_payloads(stages);
        let accepted = self.ports.apply(self.pipeline_channel(), Stages(stages), work::PAYLOADS);
        if accepted {
            for (bit, at) in work::APPLIED {
                if stages & bit != 0 {
                    self.memory.write_u32(at, 1);
                }
            }
        }
        self.clear_mask(Mask::Apply);
        if accepted { Ok(()) } else { Err(Refusal::PipelineRefused) }
    }

    /// 0xee5714, 0xedc5fc.
    fn restore_policy_stages(&mut self) {
        if self.memory.read_u32(work::SCAN_REBUILT) != 1 {
            return;
        }
        let stages = self.memory.read_u32(policy::RESTORED_STAGES);
        self.ports.restore_stages(policy::CONFIGURATION, Stages(stages));
        self.memory.write_u32(policy::RESTORED_STAGES, 0);
        self.memory.write_u32(work::SCAN_REBUILT, 0);
    }
}
