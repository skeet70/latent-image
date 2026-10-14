"""Declared timing profiles of the AF bench: how long the emulated devices take compared with the camera.

The boot profiles (`emulator/profiles/afs-shot.json`) give every modeled device the same placeholder latency of
400,000 ticks (33.3 ms), and the CPU runs at a declared 8 instructions per tick. Against the camera that made a stock
AF-S half-press four to nine times too slow. `CAMERA_AFS_1` replaces the latencies on the S1 path by values fitted to
camera evidence (see `research/af-timing/README.md` for each number, its source and its limits). Every value is a
declared, unmeasured model parameter. The source0 readiness gate places sensor settling before the RAW wait completes.
Fresh five-start replays preserve the earlier decisions and finals while bringing elapsed times closer to the camera.
`DECLARED` keeps the earlier
AFBench defaults (reproduces older result tables).

    AFBench.create(scene=scene)                      # the default: CAMERA_AFS_1
    AFBench.create(scene=scene, timing='camera')     # the camera-fitted profile
    AFBench.create(scene=scene, instructions_per_tick=8, lens_options=dict(ticks_per_second=5_000_000))   # per-knob override
"""
from dataclasses import dataclass

TICKS_PER_SECOND = 12_000_000


@dataclass(frozen=True)
class TimingProfile:
    name: str
    instructions_per_tick: int
    lens: tuple = ()            # generic focus-move parameters (`lens_focus_motion.generic_parameters`)
    pdaf: tuple = ()            # `pdaf_stream.PdafStream` options
    devices: tuple = ()         # (device class name, attribute, value) set on the loaded board
    evidence: str = ''

    def lens_options(self):
        return dict(self.lens)

    def pdaf_options(self):
        return dict(self.pdaf)

    def apply_devices(self, board):
        """Set the declared device latencies (a run boundary; nothing here is saved in portable snapshots)."""
        for device_type, attribute, value in self.devices:
            matches = [d for d in board.devices if type(d).__name__ == device_type and hasattr(d, attribute)]
            if len(matches) != 1:
                raise ValueError(f'Timing profile {self.name}: {device_type}.{attribute} matches {len(matches)} devices')
            setattr(matches[0], attribute, value)


DECLARED = TimingProfile(
    'declared', 8, lens=(), pdaf=(), devices=(),
    evidence='The pre-calibration AFBench defaults: 8 instructions per tick, lens clock 5,000,000 ticks per second, '
             '400,000-tick placeholder latencies for MOVRSZ and the AF extractor, 20,000-tick RAW DMA.')

CAMERA_AFS_1 = TimingProfile(
    'camera-afs-1', 24,
    # Lens: the stock pulse-period tables (flat 3,500 pulses/s for long moves, a 15-pulse-per-stage ramp from 1,240 pulses/s
    # for short ones) in true seconds, plus 26.7 ms between a main-move command and the end of the move that the stock
    # tables do not explain. Fitted to the motor word and pose commits of ff80-focus-trials-20261006T185535Z-687e4d.
    lens=(('ticks_per_second', TICKS_PER_SECOND), ('start_ticks', 320_000)),
    # PDAF hardware: RAW frame DMA 12.8 ms, one extraction block 0.15 ms, and two skipped source0 frames after the sensor mode
    # change (first PDAF frame wait about three frame periods). Fitted to the stock performance trace of
    # ff80-af-readonly-20261004T224448Z-03925a7d.
    pdaf=(('extract_ticks', 1_800), ('dma_ticks', 153_600), ('mode_change_frames', 2)),
    # MOVRSZ (image resizer) job 1 ms: four of them gate the S1 display sequence; the camera's S1 reaches AF start in 9..22 ms.
    devices=(('FujiDecompressor', 'movrsz_ticks', 12_000),),
    evidence='CPU 24 instructions per tick: the stock AF-S begin to PDAF task start is 8.3 ms on the camera (five episodes, '
             '8.1..8.3) and 24.4 ms at 8 instructions per tick. See research/af-timing/README.md.')

PROFILES = {p.name: p for p in (DECLARED, CAMERA_AFS_1)}
PROFILES['camera'] = CAMERA_AFS_1          # the newest camera-calibrated profile
DEFAULT = 'camera'          # five-start decision/final and phase validation: research/af-timing/README.md


def resolve(timing):
    if isinstance(timing, TimingProfile):
        return timing
    try:
        return PROFILES[DEFAULT if timing is None else timing]
    except KeyError:
        raise ValueError(f'Unknown timing profile {timing!r}; known: {sorted(PROFILES)}') from None
