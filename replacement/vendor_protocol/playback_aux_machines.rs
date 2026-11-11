//! Event decoding for consumers at 0x0120c268 and 0x0120c650.
//! Evidence: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, authenticated overlay47 extents.

use super::playback_consumer::PlaybackMessageCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationSourceKind(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationControlKind(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum NavigationEvent {
    Request143 = 0,
    Cancel = 1,
    Request102 = 2,
    Movement = 3,
    Source54To57 = 4,
    OtherSource = 5,
    Request119 = 6,
    Request10aSource54To57 = 7,
    Request10aOtherSource = 8,
    Control39Or4aOr56 = 9,
    Control3bTo3d = 10,
    Request107 = 11,
    Request115 = 12,
    Request116 = 13,
    Request108 = 14,
    Request109 = 15,
    Request112 = 16,
    Request117 = 17,
    OtherControl4a = 18,
    OtherControl = 19,
    Completion1407 = 20,
    Completion708 = 21,
    OtherMessage = 22,
}

impl NavigationEvent {
    pub const fn from_message(
        message: PlaybackMessageCode,
        source: NavigationSourceKind,
        control: NavigationControlKind,
    ) -> Option<Self> {
        let code = message.wire();
        if code == 0x0143 {
            return Some(Self::Request143);
        }
        if code & 0x7f00 == 0x0100 {
            let source_is_54_to_57 = source.0.wrapping_sub(0x54) <= 3;
            return match code {
                0x0101 => Some(Self::Cancel),
                0x0102 => Some(Self::Request102),
                0x0103..=0x0105 | 0x010c | 0x010e => Some(Self::Movement),
                0x010b | 0x010d | 0x010f => Some(if source_is_54_to_57 {
                    Self::Source54To57
                } else {
                    Self::OtherSource
                }),
                0x0119 => Some(Self::Request119),
                0x010a => Some(if source_is_54_to_57 {
                    Self::Request10aSource54To57
                } else {
                    Self::Request10aOtherSource
                }),
                0x0106 => match control.0 {
                    0x39 | 0x4a | 0x56 => Some(Self::Control39Or4aOr56),
                    0x3b..=0x3d => Some(Self::Control3bTo3d),
                    _ => None,
                },
                0x0107 => Some(Self::Request107),
                0x0115 => Some(Self::Request115),
                0x0116 => Some(Self::Request116),
                0x0108 => Some(Self::Request108),
                0x0109 => Some(Self::Request109),
                0x0112 => Some(Self::Request112),
                0x0117 => Some(Self::Request117),
                _ => Some(if control.0 == 0x4a {
                    Self::OtherControl4a
                } else {
                    Self::OtherControl
                }),
            };
        }
        if code == 0x1407 {
            Some(Self::Completion1407)
        } else if code & 0x7f00 == 0x0700 {
            if code == 0x0708 { Some(Self::Completion708) } else { None }
        } else {
            Some(Self::OtherMessage)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestedSourceMode(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum RequestedSourceEvent {
    Mode1 = 0,
    Mode2 = 1,
    Mode3 = 2,
    Mode4 = 3,
    Mode5 = 4,
    Mode6 = 5,
    Mode8 = 6,
    Mode9 = 7,
    Mode11 = 8,
    Other = 9,
}

impl RequestedSourceMode {
    pub const fn event(self) -> RequestedSourceEvent {
        match self.0 {
            1 => RequestedSourceEvent::Mode1,
            2 => RequestedSourceEvent::Mode2,
            3 => RequestedSourceEvent::Mode3,
            4 => RequestedSourceEvent::Mode4,
            5 => RequestedSourceEvent::Mode5,
            6 => RequestedSourceEvent::Mode6,
            8 => RequestedSourceEvent::Mode8,
            9 => RequestedSourceEvent::Mode9,
            11 => RequestedSourceEvent::Mode11,
            _ => RequestedSourceEvent::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NavigationControlKind, NavigationEvent, NavigationSourceKind, RequestedSourceEvent, RequestedSourceMode};
    use crate::playback_consumer::PlaybackMessageCode;

    #[test]
    fn navigation_source_range_includes_55_and_57_but_not_58() {
        for code in [0x010b, 0x010d, 0x010f] {
            for source in [0x54, 0x55, 0x56, 0x57] {
                assert_eq!(NavigationEvent::from_message(PlaybackMessageCode::from_wire(code), NavigationSourceKind(source), NavigationControlKind(0)), Some(NavigationEvent::Source54To57));
            }
            for source in [0, 0x53, 0x58, u32::MAX] {
                assert_eq!(NavigationEvent::from_message(PlaybackMessageCode::from_wire(code), NavigationSourceKind(source), NavigationControlKind(0)), Some(NavigationEvent::OtherSource));
            }
        }
    }

    #[test]
    fn navigation_143_is_distinct_from_144() {
        assert_eq!(NavigationEvent::from_message(PlaybackMessageCode::from_wire(0x143), NavigationSourceKind(0), NavigationControlKind(0)), Some(NavigationEvent::Request143));
        assert_eq!(NavigationEvent::from_message(PlaybackMessageCode::from_wire(0x144), NavigationSourceKind(0), NavigationControlKind(0)), Some(NavigationEvent::OtherControl));
    }

    #[test]
    fn unsupported_controls_and_late_700_messages_do_not_dispatch() {
        for code in [0x106, 0x700, 0x701, 0x709, 0x8708] {
            assert_eq!(NavigationEvent::from_message(PlaybackMessageCode::from_wire(code), NavigationSourceKind(0), NavigationControlKind(0)), None);
        }
        assert_eq!(NavigationEvent::from_message(PlaybackMessageCode::from_wire(0x708), NavigationSourceKind(0), NavigationControlKind(0)), Some(NavigationEvent::Completion708));
    }

    #[test]
    fn requested_source_mode_eleven_uses_its_own_event() {
        assert_eq!(RequestedSourceMode(11).event(), RequestedSourceEvent::Mode11);
        for mode in [0, 7, 10, 12, u32::MAX] {
            assert_eq!(RequestedSourceMode(mode).event(), RequestedSourceEvent::Other);
        }
    }
}
