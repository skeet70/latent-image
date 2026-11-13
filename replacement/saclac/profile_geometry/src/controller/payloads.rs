#![forbid(unsafe_code)]

//! The primary and secondary payloads the format subscription keeps current (0xee5c3c).

use super::changes::{Hold, Mask, Stage, Work};
use super::{ProfileGeometry, Tables};
use crate::memory::Memory;
use crate::ports::{Ports, PrimaryPayload, PrimaryWindow, SecondaryPayload, SecondaryWindow};
use crate::profile::ProfileHandle;
use crate::stock::layout::{parameters, work::payloads};

/// The profile no payload is built for.
const UNBUILT_PROFILE: ProfileHandle = ProfileHandle(8);
/// The window offset of policy 0x40; policy 0x80 takes the extended pair's first word less the
/// bias.
const FIXED_WINDOW_OFFSET: u32 = 62;
const PARAMETER_WINDOW_BIAS: u32 = 24;

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xee5c3c.
    pub(crate) fn update_payloads(&mut self) {
        if self.requested(Work::PrimaryPayload) {
            if self.claimed(Stage::PrimaryPayload) {
                let built = if self.held(Hold::PrimaryPayload) { None } else { self.primary_payload() };
                match built {
                    Some(payload) => {
                        self.store_primary(payload);
                        if payload.count != 0 {
                            self.add_to_mask(Mask::Apply, Stage::PrimaryPayload.pipeline_bit());
                            self.memory.set_bits_u32(payloads::PRIMARY_UPDATED, 1);
                            self.memory.write_u32(payloads::PRIMARY_UPDATE_CLEARED, 0);
                            if !self.pending(Stage::PrimaryPayload) {
                                self.enable(Stage::PrimaryPayload);
                            }
                        } else {
                            self.finish_if_pending(Stage::PrimaryPayload);
                        }
                    }
                    None => self.finish_if_pending(Stage::PrimaryPayload),
                }
            }
            self.clear_request(Work::PrimaryPayload);
        }
        if self.requested(Work::SecondaryPayload) {
            if self.claimed(Stage::SecondaryPayload) {
                let built = if self.held(Hold::SecondaryPayload) { None } else { self.secondary_payload() };
                match built {
                    Some(payload) => {
                        self.store_secondary(payload);
                        if !self.pending(Stage::SecondaryPayload) {
                            self.enable(Stage::SecondaryPayload);
                            self.add_to_mask(Mask::Apply, Stage::SecondaryPayload.pipeline_bit());
                        }
                    }
                    None => self.finish_if_pending(Stage::SecondaryPayload),
                }
            }
            self.clear_request(Work::SecondaryPayload);
        }
    }

    fn windowed_scale(&mut self, profile: ProfileHandle) -> u32 {
        let class = self.profiles.class_of(profile);
        let origin = self.ports.configured_rectangle(profile).top;
        origin / if class.first_flag != 0 { 2 } else { 1 } / self.profiles.selection_multiplier(profile)
    }

    /// 0xee5244.
    fn primary_payload(&mut self) -> Option<PrimaryPayload> {
        let profile = self.current_profile();
        if profile == UNBUILT_PROFILE {
            return None;
        }
        let group = self.profiles.pipeline_group(profile);
        let offset = match self.profiles.policy(profile) {
            0x4 | 0x8 | 0x20 => None,
            0x40 if !self.ports.has_windowed_payload(group) => None,
            0x40 => Some(FIXED_WINDOW_OFFSET),
            0x80 => Some(self.memory.read_u32(parameters::EXTENDED_PAIR).wrapping_sub(PARAMETER_WINDOW_BIAS)),
            _ => return None,
        };
        let window = offset.map(|offset| PrimaryWindow {
            rectangle: self.ports.relative_rectangle(profile, true),
            offset,
            scale: self.windowed_scale(profile),
        });
        self.ports.primary_payload(group, window)
    }

    /// 0xee5410.
    fn secondary_payload(&mut self) -> Option<SecondaryPayload> {
        let profile = self.current_profile();
        let group = self.profiles.pipeline_group(profile);
        let window = match self.profiles.policy(profile) {
            0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x40 => None,
            0x80 => Some(SecondaryWindow {
                rectangle: self.ports.relative_rectangle(profile, true),
                origin: self.ports.configured_rectangle(profile).top,
            }),
            _ => return None,
        };
        self.ports.secondary_payload(group, window)
    }

    /// 0xedc6b4.
    fn store_primary(&mut self, payload: PrimaryPayload) {
        self.memory.write_u32(payloads::PRIMARY_ENTRIES, payload.entries);
        self.memory.write_u16(payloads::PRIMARY_COUNT, payload.count);
        self.memory.write_u16(payloads::PRIMARY_OFFSETS[0], payload.first_offset);
        self.memory.write_u16(payloads::PRIMARY_OFFSETS[1], payload.second_offset);
        self.memory.write_u32(payloads::PRIMARY_OPTION, payload.option);
    }

    /// 0xedc70c.
    fn store_secondary(&mut self, payload: SecondaryPayload) {
        self.memory.write_u32(payloads::SECONDARY_VALUE, payload.value);
        self.memory.write_u16(payloads::SECONDARY_PAIR[0], payload.first);
        self.memory.write_u16(payloads::SECONDARY_PAIR[1], payload.second);
        self.memory.write_u32(payloads::SECONDARY_EXTRA, payload.extra);
    }
}
