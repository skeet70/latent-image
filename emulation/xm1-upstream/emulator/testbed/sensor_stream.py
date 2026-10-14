"""Host side of the continuous sensor stream (design: STREAM.md).

``enable(board, scene=..., fps=...)`` switches a loaded REC snapshot from the
strict, finite external-frame ledgers to a periodic frame clock. The devices
keep ownership of their registers and ACK contracts:

* ``InputProcessingControl`` asserts source0 (IRQ 0x27) / source1 (IRQ 0x29)
  every frame, only while the firmware's own enable/mask/route allow it, and
  clears it only on the stock selector ACK;
* ``ProducerControl`` (``producer_stream``) latches the firmware-programmed
  bank tuple at each source0 edge, writes this module's scene bytes into it
  and asserts IRQ 0x2E, then accepts the stock paired ACK.

Nothing else is written into guest state.
"""
from __future__ import annotations

import struct

from emulator import producer_stream
from emulator.testbed.scene_sources import FrameContext, SceneSource

PROFILE = 'declared-testbed-continuous-sensor-stream-unmeasured'
# Inference (research/emulator-kernel/interrupts.md): the 12 MHz timer input
# counts one per virtual tick at prescaler index 0.
TICKS_PER_SECOND = 12_000_000
SENSOR_MODE_ADDRESS = 0x173D308
LENS_POSE_ADDRESS = 0xCCDE64
GEOMETRY_OFFSETS = (0x100, 0x104, 0x108, 0x15C)

# Frame rates observed on the camera (research/af-iteration-loop.md): idle
# live view in drive mode MV is ~54.54 fps, ~60 fps at the start of a
# half-press and up to ~200 fps while the lens moves during AF; 60-120 fps in
# low light. Keyed by named condition; ``FPS_BY_SENSOR_MODE`` maps the applied
# sensor mode word to a default condition.
FPS_TABLE = {'idle': 54.54, 'half-press': 60.0, 'af-lens-moving': 200.0,
             'low-light-min': 60.0, 'low-light-max': 120.0}
FPS_BY_SENSOR_MODE = {0x28: 'idle', 0x50: 'half-press'}


def _device(board, name):
    matches = [d for d in board.devices if type(d).__name__ == name]
    if len(matches) != 1:
        raise ValueError(f'Stream requires exactly one {name}')
    return matches[0]


class SensorStream:
    def __init__(self, board, scene: SceneSource, *, fps, time_scale=1.0, data_delay_fraction=0.1,
                 source1=False, companion_source1=True, exposure_ticks=0, exposure_samples=8):
        self.board, self.scene = board, scene
        # Optional exposure-time motion blur: scenes get the mean physical position over the
        # ``exposure_ticks`` before each frame's edge (0, the default, uses the position at the edge).
        self.exposure_ticks, self.exposure_samples = int(exposure_ticks), int(exposure_samples)
        lenses = [d for d in board.devices if type(d).__name__ == 'LensEndpoint']
        self.lens = lenses[0] if len(lenses) == 1 else None
        self.pose_log = []        # bounded diagnostic ring: (frame index, tick, committed, physical)
        self.served = []          # statistics frames served: (frame index, tick, committed, physical, af configured)
        self.time_scale = time_scale
        self.fps = fps
        self.input = _device(board, 'InputProcessingControl')
        self.producer = _device(board, 'ProducerControl')
        self.image_dev = _device(board, 'ImageWorkControl')
        self.fe80 = _device(board, 'FE80Control')
        self.edges = []          # bounded diagnostic ring
        self.contexts = {}
        period = self.period_ticks(fps)
        self.input.enable_stream(period_ticks=period, source1=source1, companion_source1=companion_source1)
        self.producer.enable_stream(data_delay_ticks=max(1, int(period * data_delay_fraction)))
        self.image_dev.enable_stream(data_delay_ticks=max(1, int(period * 2.5 * data_delay_fraction)))
        self.fe80.enable_stream(job_delay_ticks=max(1, int(period * 0.15)))
        _device(board, 'FaceAnalysisControl').enable_stream()
        board.sensor_stream = self

    # -- timing ---------------------------------------------------------------
    def period_ticks(self, fps):
        if isinstance(fps, str):
            fps = FPS_TABLE[fps]
        return round(TICKS_PER_SECOND * self.time_scale / fps)

    def set_fps(self, fps):
        self.fps = fps
        self.input.set_stream_period(self.period_ticks(fps))

    # -- device callbacks -----------------------------------------------------
    def frame_ready(self, source, tick):
        """Gate an external source0 edge while the declared sensor mode settles.

        The PDAF timing profile owns the number of invalid frames. Gate before
        the source IRQ, so stock frame readiness and RAW DMA remain separate.
        No guest state is written and the admitted edge still needs stock ACK.
        """
        pdaf = getattr(self.board, 'pdaf_stream', None)
        return source != 0 or pdaf is None or pdaf.raw.sensor_frame_ready(self._word(SENSOR_MODE_ADDRESS))

    def _word(self, address, size=4, signed=False):
        return int.from_bytes(bytes(self.board.read_memory(address, size, cpu_id=0)), 'little', signed=signed)

    def af_configuration(self):
        """The contrast-AF consumer packet the firmware programmed, or None while it is not valid.

        Same RAM words as the strict ``afs_focus_data._configuration``: grid packet 0x16224D8,
        consumer dimensions 0x16227A4..A9, ten-region descriptor 0x16228C0, band count/scale/orientation."""
        def read(address, size):
            return bytes(self.board.read_memory(address, size, cpu_id=0))
        width, height, x, y, gaps, counts = struct.unpack('<6I', read(0x16224d8, 24))
        dimensions = read(0x16227a4, 6)
        windows = read(0x16228c0, 164)
        if (counts & ~0xffff or not counts or dimensions != bytes((counts & 255, counts >> 8)) * 3
                or struct.unpack_from('<I', windows)[0] != 10):
            return None
        return dict(dimensions=dimensions, band_count=read(0x16227aa, 1)[0],
                    band_scale=int.from_bytes(read(0x1622508, 4), 'little'),
                    band_orientation=int.from_bytes(read(0x1622888, 4), 'little'),
                    grid=dict(cell_width=width, cell_height=height, x=x, y=y, gap_x=gaps & 65535,
                              gap_y=gaps >> 16, columns=counts & 255, rows=counts >> 8),
                    windows=[list(struct.unpack_from('<4I', windows, 4 + i * 16)) for i in range(10)])

    def pose(self, tick):
        """``(committed, physical, exposure_window)`` of the lens for a frame whose edge is at ``tick``.

        ``committed`` is the firmware pose word 0xCCDE64 (it changes only when a stock move ends). ``physical``
        is the lens model's declared physical position (``LensEndpoint.focus_pose``: start plus the pulses the
        declared ramp law has delivered), or None when the lens model has none. With ``exposure_ticks`` it is
        the mean over the exposure window ending at ``tick`` and the window is the (min, max) seen."""
        committed = self._word(LENS_POSE_ADDRESS, 2, signed=True)
        if self.lens is None:
            return committed, None, None
        physical = self.lens.focus_pose(tick)
        if physical is None or self.exposure_ticks <= 0:
            return committed, physical, None
        n = max(2, self.exposure_samples)
        samples = [self.lens.focus_pose(tick - self.exposure_ticks * (n - 1 - i) // (n - 1)) for i in range(n)]
        return committed, sum(samples) / n, (min(samples), max(samples))

    def on_edge(self, pending, tick):
        if pending['source'] != 0:
            return
        p = self.producer
        committed, physical, window = self.pose(tick)
        ctx = FrameContext(index=pending['frame'], tick=tick,
                           sensor_mode=self._word(SENSOR_MODE_ADDRESS),
                           lens_position=committed, physical_position=physical, exposure_window=window,
                           windows={a: p.registers[a] for a in p.DESTINATIONS},
                           geometry={p.BASE + o: p.registers[p.BASE + o] for o in GEOMETRY_OFFSETS},
                           af=self.af_configuration() if self._word(SENSOR_MODE_ADDRESS) != 0x28 else None)
        frame = dict(index=ctx.index, tick=tick, sensor_mode=ctx.sensor_mode, lens_position=ctx.lens_position,
                     physical_position=ctx.physical_position)
        self.pose_log.append((ctx.index, tick, committed, physical))
        del self.pose_log[:-4096]
        from emulator import image_stream
        image_stream.latch_frame(self.image_dev, frame, tick)
        if producer_stream.latch_frame(p, frame, tick):
            self.contexts[ctx.index] = ctx
            for stale in [k for k in self.contexts if k < ctx.index - 8]:
                del self.contexts[stale]

    def on_ack(self, pending, tick):
        pass

    def image(self, frame, output):
        """Preview YYCbCr for one latched image output, or None for the native scene."""
        ctx = self.contexts.get(frame['index'])
        return None if ctx is None else self.scene.preview_yuv(ctx, output)

    def histogram(self, frame, spans):
        """The 1024-byte channel-1 luma histogram: scene's own, else statistics plane 0."""
        ctx = self.contexts.get(frame['index'])
        data = None if ctx is None else self.scene.histogram(ctx)
        if data is None:
            data = next(d for r, o, d in spans if (r, o) == (0xfff18508, 0))
        return data

    def fe80_values(self, channels):
        """The 14 FE80 contrast outputs for a job completing now (scene law, else statistics-derived)."""
        p = self.producer
        latest = self.contexts.get(self.input.stream['frame'])
        committed, physical, window = (self.pose(self.board.virtual_ticks) if latest is None else
                                       (latest.lens_position, latest.physical_position, latest.exposure_window))
        ctx = FrameContext(index=self.input.stream['frame'], tick=self.board.virtual_ticks,
                           sensor_mode=self._word(SENSOR_MODE_ADDRESS),
                           lens_position=committed, physical_position=physical, exposure_window=window,
                           windows={a: p.registers[a] for a in p.DESTINATIONS},
                           geometry={p.BASE + o: p.registers[p.BASE + o] for o in GEOMETRY_OFFSETS})
        values = self.scene.fe80_values(ctx, channels)
        if values is None:
            import struct
            fine = next((struct.unpack_from('<I', d)[0] for r, o, d in self.scene.statistics(ctx)
                         if (r, o) == (0xfff18500, 0x400)), 0)
            values = [fine] * channels
        return values

    def statistics(self, frame):
        ctx = self.contexts[frame['index']]
        self.served.append((ctx.index, ctx.tick, ctx.lens_position, ctx.physical_position, ctx.af is not None))
        del self.served[:-4096]
        return self.scene.statistics(ctx)

    def summary(self):
        return dict(profile=PROFILE, fps=self.fps, scene=self.scene.describe(),
                    input={k: v for k, v in self.input.stream.items() if k != 'history'},
                    producer={k: v for k, v in self.producer.stream.items() if k not in ('log', 'tuple', 'frame')},
                    image={k: v for k, v in self.image_dev.stream.items() if k not in ('frame', 'outputs')},
                    fe80={k: v for k, v in self.fe80.stream.items() if k != 'last'})


def enable_lens_motion(board):
    """Select the existing declared AF lens response law, as the manual feed does.

    The stock aperture/focus drivers then complete through the real lens
    IRQs. This is the one lens-side opt-in the half-press needs; the lens
    model itself is not changed here.
    """
    lenses = [d for d in board.devices if type(d).__name__ == 'LensEndpoint']
    if len(lenses) != 1 or not hasattr(lenses[0], 'enable_afs_motion'):
        return None
    from emulator.lens_af_motion import PROFILE
    if lenses[0].requests['focus'] is None:
        lenses[0].enable_afs_motion(PROFILE)
    return PROFILE


def enable(board, scene: SceneSource, fps='idle', *, lens_motion=True, instructions_per_tick=None,
           **options) -> SensorStream:
    """Start the stream. ``instructions_per_tick`` sets the board's declared CPU-speed
    ratio first (see STREAM.md; default: leave it unchanged)."""
    if getattr(board, 'sensor_stream', None) is not None:
        raise ValueError('Sensor stream is already enabled')
    if instructions_per_tick is not None:
        board.set_instructions_per_tick(instructions_per_tick)
    stream = SensorStream(board, scene, fps=fps, **options)
    if lens_motion:
        stream.lens_motion_profile = enable_lens_motion(board)
    return stream


def load_snapshot(directory, *, instructions_per_tick=None):
    """Development load of a portable snapshot, accepting every drifted model module.

    The shared worktree's model sources move on after a snapshot is saved
    (this package itself changes the producer/input/image models), so the
    strict ``tools.afs_load.load_dev`` allow-list is replaced by "all changed
    model modules". Identity outside models must still match. The accepted
    names are returned on ``board.stream_changed_models``.
    """
    import json
    from pathlib import Path
    from emulator.boot import make_board
    from x70_native import portable_checkpoint as pc
    directory = Path(directory)
    manifest = json.loads((directory / 'manifest.json').read_text())
    dummy = make_board(**manifest['factory_options'])
    old, cur = manifest['identity'], pc._identity(dummy)
    del dummy
    revisions = {}
    for key in old:
        if key == 'models':
            for name in set(old[key]) | set(cur[key]):
                if old[key].get(name) != cur[key].get(name):
                    revisions['models.' + name] = {'saved': old[key].get(name), 'current': cur[key].get(name)}
        elif old[key] != cur[key]:
            raise RuntimeError('Snapshot identity differs outside models: ' + key)
    options = dict(instructions_per_tick=instructions_per_tick, override_ratio=instructions_per_tick is not None)
    board = (pc.load_for_development(directory, revisions, **options) if revisions
             else pc.load(directory, **options))
    board.stream_changed_models = sorted(revisions)
    return board
