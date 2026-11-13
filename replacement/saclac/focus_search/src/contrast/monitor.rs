#![forbid(unsafe_code)]

//! The monitor after a suspension: it keeps the lens until the lens identity changes, then
//! follows the move with a return for a small lens bias or a peak search for a large one
//! (0xf467cc).

use super::sample::{ScanState, Watch};
use super::settings::Setting;
use super::{Context, LensMove, Phase, Platform};

impl Context {
    /// 0xf467cc. `bridge_busy`: a bridge request is in flight.
    pub(crate) fn monitor_step(&mut self, platform: &mut impl Platform, bridge_busy: bool) -> Phase {
        let moved = match self.monitor.moved {
            Some(moved) => moved,
            None => {
                let Some(moved) = (if self.lens != self.previous_lens { platform.lens_move() } else { None }) else {
                    self.samples = 0;
                    return Phase::Monitor;
                };
                self.monitor.moved = Some(moved);
                let direction = match moved {
                    LensMove::Ascending => 1,
                    LensMove::Descending => -1,
                };
                self.direction = direction;
                self.previous_direction = direction;
                moved
            }
        };
        let position = platform.lens_position();
        self.monitor.mark = platform.interpolated_bias(self.lens, moved, position);
        let tens = platform.setting(match moved {
            LensMove::Ascending => Setting::MonitorRiseTens,
            LensMove::Descending => Setting::MonitorFallTens,
        });
        let threshold = i32::from(tens.wrapping_mul(10) as u16);
        let watch = if i32::from(self.monitor.mark).abs() < threshold { Watch::Return } else { Watch::PeakSearch };
        if self.monitor.watch != watch {
            self.samples = 1;
            self.monitor.settled = false;
            self.head_for_mark();
            if bridge_busy {
                self.stop_scan(platform);
                return Phase::Monitor;
            }
        }
        match watch {
            Watch::PeakSearch if self.lens != self.previous_lens => {
                self.head_for_mark();
                if self.monitor.settled {
                    self.samples = 1;
                }
                self.monitor.settled = false;
                self.monitor_peak_search(platform);
            }
            Watch::PeakSearch if self.monitor.settled => self.bridge.scan = ScanState::Running,
            Watch::PeakSearch => self.monitor_peak_search(platform),
            Watch::Return | Watch::Scene => {
                self.return_step(platform);
            }
        }
        self.monitor.watch = watch;
        Phase::Monitor
    }

    /// The direction the lens bias points.
    fn head_for_mark(&mut self) {
        let direction = if self.monitor.mark < 0 { -1 } else { 1 };
        self.direction = direction;
        self.previous_direction = direction;
    }

    /// A peak search that reaches its peak settles the monitor until the lens moves again.
    fn monitor_peak_search(&mut self, platform: &mut impl Platform) {
        if self.peak_search_step(platform) == Phase::Peak {
            self.monitor.settled = true;
        }
    }
}
