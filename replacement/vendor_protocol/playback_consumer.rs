//! Queue-4 playback source classification for X-M1 1.50.
//!
//! Evidence: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3,
//! indexed router 0x0120b778 (616 bytes, SHA-256
//! fb979e69592f1d6c79dd71497fbb2157e96f570c6219b139ccee8a9e90fe2c11).
//! The NONMATCHING reconstruction is confirmed against the authenticated
//! overlay47 image, SHA-256
//! 0cf67b7221c3b0755092da0ae4c6d6fb817f2b32771ee53dff691fc59b28770c.
//!
//! This module classifies messages. It does not execute the playback actor.
//! The actor's machine-1 action bank is 0x01218d90, its transition bank is
//! 0x01219ba4, its followup bank is 0x012173f4, and its stride is four.
//! The CPU work below those actions, including mode selection at 0x0120cab0,
//! source processing at 0x0120b40c, rendering, storage and cancellation,
//! remains outside this module. Queue-3 commands 0x0200..=0x0202 are not
//! playback messages.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackMessageCode(u16);

impl PlaybackMessageCode {
    pub const fn from_wire(value: u16) -> Self {
        Self(value)
    }

    pub const fn wire(self) -> u16 {
        self.0
    }

    pub const fn source_selector(self) -> SourceSelector {
        let code = self.0;
        if code >= 0x0142 && code <= 0x0144 {
            SourceSelector::ModeSelection
        } else if code & 0x7f00 == 0x0700 {
            if code == 0x0701 {
                SourceSelector::PrimaryControl
            } else {
                SourceSelector::SecondaryControl
            }
        } else if code >= 0xba00 && code <= 0xba07 {
            SourceSelector::ControlCompletion
        } else if code & 0x7f00 == 0x0800 {
            if code == 0x0801 {
                SourceSelector::PrimarySource
            } else {
                SourceSelector::SecondarySource
            }
        } else if code >= 0xe600 && code <= 0xe61f {
            SourceSelector::SourceCompletion
        } else if code & 0x7f00 == 0x0100
            || (code >= 0x1400 && code <= 0x141c)
            || (code >= 0x2a09 && code <= 0x2a0a)
            || (code >= 0xaa00 && code <= 0xaa08)
            || (code >= 0xaf00 && code <= 0xaf0f)
            || (code >= 0xcf00 && code <= 0xcf07)
            || code == 0x4f08
            || (code >= 0x560c && code <= 0x560d)
            || (code >= 0xd600 && code <= 0xd60b)
            || code == 0x5f03
            || code == 0xda00
            || code == 0xda01
            || code == 0xdb00
            || code == 0xda03
            || code == 0xdb01
            || code == 0xdc01
            || code == 0xdd01
            || code == 0xde01
            || code == 0xdf01
            || code == 0xdf04
            || code == 0xd505
        {
            SourceSelector::SnapshotSource
        } else {
            SourceSelector::Other
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum SourceSelector {
    ModeSelection = 0,
    PrimaryControl = 1,
    SecondaryControl = 2,
    ControlCompletion = 3,
    PrimarySource = 4,
    SecondarySource = 5,
    SourceCompletion = 6,
    SnapshotSource = 7,
    Other = 8,
}

impl SourceSelector {
    pub const fn machine_event(self) -> u16 {
        self as u16
    }

    // The router calls 0x01249884 then 0x0121ebfc before machine 1.
    pub const fn snapshots_source(self) -> bool {
        matches!(self, Self::SnapshotSource)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackModeCommand(u32);

impl PlaybackModeCommand {
    pub const fn from_wire(value: u32) -> Self {
        Self(value)
    }

    pub const fn wire(self) -> u32 {
        self.0
    }

    // Actual query 0x0121cb28.
    pub const fn selection(self) -> ModeSelection {
        match self.0 {
            0xd1 => ModeSelection::One,
            0xd2 => ModeSelection::Four,
            0xd3 => ModeSelection::Five,
            0xd4 => ModeSelection::Two,
            0xd5 => ModeSelection::Six,
            0xd6 => ModeSelection::Seven,
            0xd7 => ModeSelection::Eight,
            0xd8 => ModeSelection::Nine,
            0xd9 => ModeSelection::Ten,
            _ => ModeSelection::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ModeSelection {
    None = 0,
    One = 1,
    Two = 2,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackSourceKind {
    Cb,
    CdOrCe,
    Cf,
    Other,
}

impl PlaybackSourceKind {
    pub const fn from_wire(value: u32) -> Self {
        match value {
            0xcb => Self::Cb,
            0xcd | 0xce => Self::CdOrCe,
            0xcf => Self::Cf,
            _ => Self::Other,
        }
    }

    // Actual selector 0x0121e698.
    pub const fn work_mode(self, command: PlaybackModeCommand) -> PlaybackWorkMode {
        match self {
            Self::Cb => PlaybackWorkMode::Two,
            Self::CdOrCe => PlaybackWorkMode::Five,
            Self::Cf => match command.0 {
                0xd2 | 0xd3 => PlaybackWorkMode::Nine,
                0xd5..=0xd9 => PlaybackWorkMode::Six,
                _ => PlaybackWorkMode::Seven,
            },
            Self::Other => PlaybackWorkMode::One,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PlaybackWorkMode {
    One = 1,
    Two = 2,
    Five = 5,
    Six = 6,
    Seven = 7,
    Nine = 9,
}

#[cfg(test)]
mod tests {
    use super::{
        PlaybackMessageCode, PlaybackModeCommand, PlaybackSourceKind, PlaybackWorkMode,
        ModeSelection, SourceSelector,
    };

    #[test]
    fn playback_producers_route_through_source_snapshot() {
        for code in [
            0x100, 0x101, 0x103, 0x119, 0x128, 0x130, 0x131, 0x132, 0x133,
            0x134, 0x135, 0x136, 0x137, 0x138, 0x139, 0x13a, 0x13b, 0x13c,
            0x13d, 0x13e, 0x13f, 0x140, 0x141,
        ] {
            let selector = PlaybackMessageCode::from_wire(code).source_selector();
            assert_eq!(selector, SourceSelector::SnapshotSource);
            assert!(selector.snapshots_source());
        }
    }

    #[test]
    fn explicit_mode_requests_precede_the_command_family() {
        for code in 0x142..=0x144 {
            let selector = PlaybackMessageCode::from_wire(code).source_selector();
            assert_eq!(selector, SourceSelector::ModeSelection);
            assert!(!selector.snapshots_source());
        }
    }

    #[test]
    fn sign_bit_is_masked_only_for_command_families() {
        for (code, expected) in [
            (0x8701, SourceSelector::SecondaryControl),
            (0x8801, SourceSelector::SecondarySource),
            (0x8100, SourceSelector::SnapshotSource),
            (0xfa00, SourceSelector::Other),
            (0x0701, SourceSelector::PrimaryControl),
            (0x0801, SourceSelector::PrimarySource),
        ] {
            assert_eq!(PlaybackMessageCode::from_wire(code).source_selector(), expected);
        }
    }

    #[test]
    fn completion_range_endpoints_and_neighbors_remain_distinct() {
        for (first, last, expected) in [
            (0xba00, 0xba07, SourceSelector::ControlCompletion),
            (0xe600, 0xe61f, SourceSelector::SourceCompletion),
            (0xaa00, 0xaa08, SourceSelector::SnapshotSource),
            (0xaf00, 0xaf0f, SourceSelector::SnapshotSource),
            (0xcf00, 0xcf07, SourceSelector::SnapshotSource),
            (0xd600, 0xd60b, SourceSelector::SnapshotSource),
        ] {
            for code in first..=last {
                assert_eq!(PlaybackMessageCode::from_wire(code).source_selector(), expected);
            }
            assert_eq!(PlaybackMessageCode::from_wire(first - 1).source_selector(), SourceSelector::Other);
            assert_eq!(PlaybackMessageCode::from_wire(last + 1).source_selector(), SourceSelector::Other);
        }
    }

    #[test]
    fn unrelated_record_queue_commands_do_not_enter_playback_processing() {
        for code in [0x0200, 0x0201, 0x0202, 0xda02, 0xdf02, 0xdf03, 0xffff] {
            assert_eq!(PlaybackMessageCode::from_wire(code).source_selector(), SourceSelector::Other);
        }
    }

    #[test]
    fn cf_source_uses_command_specific_work_modes() {
        let source = PlaybackSourceKind::from_wire(0xcf);
        for command in [0xd2, 0xd3] {
            assert_eq!(source.work_mode(PlaybackModeCommand::from_wire(command)), PlaybackWorkMode::Nine);
        }
        for command in 0xd5..=0xd9 {
            assert_eq!(source.work_mode(PlaybackModeCommand::from_wire(command)), PlaybackWorkMode::Six);
        }
        for command in [0, 0xd1, 0xd4, 0xda, u32::MAX] {
            assert_eq!(source.work_mode(PlaybackModeCommand::from_wire(command)), PlaybackWorkMode::Seven);
        }
    }

    #[test]
    fn other_source_kinds_do_not_depend_on_the_mode_command() {
        for command in [0, 0xd2, 0xd5, u32::MAX] {
            let command = PlaybackModeCommand::from_wire(command);
            for (source, expected) in [
                (0xcb, PlaybackWorkMode::Two),
                (0xcd, PlaybackWorkMode::Five),
                (0xce, PlaybackWorkMode::Five),
                (0xcc, PlaybackWorkMode::One),
                (0, PlaybackWorkMode::One),
            ] {
                assert_eq!(PlaybackSourceKind::from_wire(source).work_mode(command), expected);
            }
        }
    }

    #[test]
    fn d4_command_selects_mode_two_instead_of_a_sequential_index() {
        assert_eq!(PlaybackModeCommand::from_wire(0xd4).selection(), ModeSelection::Two);
        assert_eq!(PlaybackModeCommand::from_wire(0xd2).selection(), ModeSelection::Four);
        assert_eq!(PlaybackModeCommand::from_wire(0xda).selection(), ModeSelection::None);
    }
}
