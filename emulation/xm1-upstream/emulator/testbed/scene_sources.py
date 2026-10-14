"""Scene sources for the continuous sensor stream (see STREAM.md).

A ``SceneSource`` is asked once per delivered frame for the bytes the
statistics producer would DMA into the firmware's latched bank windows. It
returns data only; it never touches firmware state, callbacks or messages.
"""
from __future__ import annotations

from dataclasses import dataclass, field
import json
from pathlib import Path

import numpy as np


@dataclass(frozen=True)
class FrameContext:
    """Everything a scene may depend on for one frame."""
    index: int                 # 1-based frame number since the stream was enabled
    tick: int                  # virtual tick of the source0 edge
    sensor_mode: int           # applied sensor mode word at 0x173D308
    lens_position: int | None  # committed pose 0xCCDE64 (reference: changes only when a stock move ends)
    windows: dict = field(default_factory=dict)  # producer destination register -> latched base
    geometry: dict = field(default_factory=dict)  # producer geometry words 0x100/0x104/0x108/0x15c
    af: dict | None = None     # contrast-AF consumer configuration the firmware programmed (RAM), if valid
    # Physical lens position at this frame's exposure from the lens model's motion (pulses, committed-pose
    # frame; None when the lens model has none). With the stream's ``exposure_ticks`` flag it is the mean over
    # the exposure window and ``exposure_window`` is its (min, max).
    physical_position: float | None = None
    exposure_window: tuple | None = None

    @property
    def position(self):
        """The lens position a scene must look data up by: physical at exposure, else the committed pose."""
        return self.lens_position if self.physical_position is None else self.physical_position


class SceneSource:
    """Interface. Subclasses override ``statistics``; the rest are optional."""
    name = 'scene'

    def statistics(self, ctx: FrameContext) -> list[tuple[int, int, bytes]]:
        """Return ``(destination register, byte offset, data)`` spans."""
        raise NotImplementedError

    def preview_yuv(self, ctx: FrameContext, output: dict) -> bytes | None:
        """Packed YYCbCr for one latched image-work output (width/height/stride_bytes/
        source_width/source_height keys), or None to use the native analytic scene."""
        return None

    def histogram(self, ctx: FrameContext) -> bytes | None:
        """256 little-endian U32 channel-1 luma histogram, or None to reuse
        the first statistics histogram plane."""
        return None

    def fe80_values(self, ctx: FrameContext, channels: int) -> list[int] | None:
        """The FE80 contrast engine's ``channels`` U32 outputs for one preview job.

        Declared law (the engine's program and channel meanings are not
        recovered): larger means sharper. None lets the host derive them from
        the scene's contrast statistics."""
        return None

    def pd_rows(self, ctx: FrameContext):
        """Recorded phase-detection work-area bytes (a sweep's ``pd-buffer.bin``), if the scene has them."""
        return None

    def raw_pd(self, ctx: FrameContext, spec: dict):
        """Aperture planes for one RAW frame, or None when the scene has no phase-detection source.

        ``spec`` carries the firmware-programmed geometry (``width`` interleaved samples, ``plane_width``
        = width // 2, ``height``, ``pitch``, ``packed``, ``origin``). Return
        ``{'left': ndarray, 'right': ndarray}``, each uint16 of shape ``(height, plane_width)`` holding
        U14 intensities of the two apertures (see ``emulator/pdaf_stream.py`` for the declared layout).
        """
        return None

    def describe(self) -> dict:
        return dict(name=self.name)


class RecordedBankSource(SceneSource):
    """The camera's recorded idle-REC statistics bank, constant every frame."""
    name = 'recorded-bank'

    def __init__(self, fixture=None):
        from emulator import recorded_statistics
        self.fixture = fixture or recorded_statistics.load_fixture(recorded_statistics.PROFILE)

    def statistics(self, ctx):
        return list(self.fixture.spans)

    def describe(self):
        return dict(name=self.name, bank_sha256=self.fixture.sha256, spans=len(self.fixture.spans))


# ---------------------------------------------------------------------------
# Synthetic scene

def _synthetic_image(width, height, mean_luma, seed):
    """Deterministic packed YYCbCr (Y0 Y1 Cb Cr per pixel pair) textured target.

    Low-frequency gradient and 8x8 blocks give metering structure; per-pixel
    noise gives gradient energy that defocus blur visibly removes.
    """
    import numpy as np
    rng = np.random.default_rng(seed)
    block = 8
    cells = rng.integers(0, 160, size=((height + block - 1) // block, (width + block - 1) // block))
    luma = np.kron(cells, np.ones((block, block), dtype=np.int64))[:height, :width]
    yy, xx = np.mgrid[0:height, 0:width]
    luma = 30 + xx * 40 // width + yy * 24 // height + luma + rng.integers(-24, 25, size=(height, width))
    luma = np.clip(luma, 16, 235)
    luma = np.clip(luma + round(mean_luma - luma.mean()), 16, 235)
    packed = np.empty((height, width // 2, 4), dtype=np.uint8)
    packed[:, :, 0] = luma[:, 0::2]
    packed[:, :, 1] = luma[:, 1::2]
    packed[:, :, 2] = 128
    packed[:, :, 3] = 128
    return packed.tobytes()


class SyntheticSceneSource(SceneSource):
    """Lens-dependent contrast from the declared laws, realistic AE windows.

    One textured YYCbCr target is blurred by ``measurement_scene`` according
    to ``|target_position - lens_position|`` (its declared binomial law) and
    projected through ``afs_scene_fixture.consumer_payloads`` (three luma
    histograms, ten U64 gradient bands, four luma totals) and
    ``contrast_grid`` (the two contrast planes at the programmed grid
    dimensions). Windows with no declared law keep the camera's recorded
    idle-REC bytes, so unmodeled consumers see plausible values. All laws are
    declared simulation inputs, not ISP/optics calibration.
    """
    name = 'synthetic'

    def __init__(self, *, width=720, height=480, target_position=None, pulses_per_blur_pass=8,
                 mean_luma=97, seed=1, recorded=None, pulses_per_sample=8, flat_pd=False):
        from emulator import recorded_statistics
        self.width, self.height = width, height
        self.target_position = target_position   # None: latch the first frame's lens position
        self.pulses_per_blur_pass = pulses_per_blur_pass
        self.recorded = recorded or recorded_statistics.load_fixture(recorded_statistics.PROFILE)
        self.image = _synthetic_image(width, height, mean_luma, seed)
        self.mean_luma, self.seed = mean_luma, seed
        self.pulses_per_sample, self.flat_pd = pulses_per_sample, flat_pd   # declared PD law
        self._pd_cache = {}
        self.last_af = None
        self._cache = {}
        self.last = None

    @staticmethod
    def _grid_dimensions(ctx):
        columns = rows = 1
        for key, name in ((0x100, 'columns'), (0x104, 'rows')):
            word = ctx.geometry.get(0xfff18000 + key)
            if word is not None:
                value = ((word >> 12) & 15) + 1
                if 1 <= value <= 8:
                    if name == 'columns':
                        columns = value
                    else:
                        rows = value
        return columns, rows

    def _af_spans(self, ctx, lens):
        """Contrast-AF consumer windows (mask A3) from the declared squared-gradient law."""
        from emulator import afs_focus_scene, measurement_scene
        from emulator.hardware import UnsupportedDevice
        cfg = ctx.af
        passes = min(measurement_scene.MAX_BLUR_PASSES,
                     (abs(self.target_position - lens) + self.pulses_per_blur_pass // 2) // self.pulses_per_blur_pass)
        key = ('af', passes, cfg['dimensions'], tuple(sorted(cfg['grid'].items())),
               tuple(map(tuple, cfg['windows'])), cfg['band_count'], cfg['band_scale'], cfg['band_orientation'])
        spans = self._cache.get(key)
        if spans is None:
            try:
                result = afs_focus_scene.prepare(
                    self.image, width=self.width, height=self.height, stride_bytes=self.width * 2,
                    focus_position=lens, target_position=self.target_position, dimensions=cfg['dimensions'],
                    grid=cfg['grid'], windows=cfg['windows'], band_count=cfg['band_count'],
                    band_scale=cfg['band_scale'], band_orientation=cfg['band_orientation'])
            except UnsupportedDevice:
                spans = False
            else:
                spans = [(0xfff18500, offset, data) for offset, data in result['spans']]
                self.last_af = dict(passes=passes, coarse=result['contrast']['coarse'],
                                    fine=result['contrast']['fine'], band=result['band_raw'])
            self._cache[key] = spans
        return spans or None

    def statistics(self, ctx):
        from emulator import afs_scene_fixture, measurement_scene
        lens = 0 if ctx.position is None else round(ctx.position)
        if self.target_position is None:
            self.target_position = lens
        if ctx.af is not None:
            spans = self._af_spans(ctx, lens)
            if spans is not None:
                return spans
        columns, rows = self._grid_dimensions(ctx)
        passes = min(measurement_scene.MAX_BLUR_PASSES,
                     (abs(self.target_position - lens) + self.pulses_per_blur_pass // 2) // self.pulses_per_blur_pass)
        key = (passes, columns, rows)
        spans = self._cache.get(key)
        if spans is None:
            stride = self.width * 2
            blurred = (measurement_scene._blur_numpy if measurement_scene._numpy is not None
                       else measurement_scene._blur_scalar)(self.image, self.width, self.height, stride, passes) if passes else self.image
            payload = afs_scene_fixture.consumer_payloads(blurred, width=self.width, height=self.height, stride_bytes=stride)
            grid = afs_scene_fixture.contrast_grid(blurred, width=self.width, height=self.height,
                                                   stride_bytes=stride, columns=columns, rows=rows)
            modeled = {(r, o): d for r, o, d in payload['spans']
                       if (r, o) in ((0xfff18504, 0), (0xfff18508, 0), (0xfff18508, 0x400),
                                     (0xfff18508, 0x800), (0xfff18518, 0))}
            spans = []
            for register, offset, data in self.recorded.spans:
                if (register, offset) in modeled:
                    data = modeled[(register, offset)]
                if register == 0xfff18500:
                    continue  # replaced by the grid planes below
                spans.append((register, offset, data))
            for (offset, data) in grid['bank_spans']:
                spans.append((0xfff18500, offset, data))
            self._cache[key] = spans
            self.last = dict(passes=passes, columns=columns, rows=rows, coarse=grid['coarse'], fine=grid['fine'])
        return spans

    def _blurred(self, passes):
        from emulator import measurement_scene
        cache = self.__dict__.setdefault('_blur_cache', {})
        image = cache.get(passes)
        if image is None:
            stride = self.width * 2
            image = (measurement_scene._blur_numpy if measurement_scene._numpy is not None
                     else measurement_scene._blur_scalar)(self.image, self.width, self.height, stride, passes) if passes else self.image
            cache[passes] = image
        return image

    def fe80_values(self, ctx, channels):
        """Channel k: horizontal gradient energy at pixel distance k+1 of the lens-blurred luma.

        Declared law. Sharp scenes score high at every distance; defocus blur
        removes the short-distance energy first, so channels fall at different
        rates, peaking together at the target position."""
        import numpy as np
        from emulator import measurement_scene
        lens = 0 if ctx.position is None else round(ctx.position)
        if self.target_position is None:
            self.target_position = lens
        passes = min(measurement_scene.MAX_BLUR_PASSES,
                     (abs(self.target_position - lens) + self.pulses_per_blur_pass // 2) // self.pulses_per_blur_pass)
        key = ('fe80', passes, channels)
        values = self._cache.get(key)
        if values is None:
            image = self._blurred(passes)
            packed = np.frombuffer(image, dtype=np.uint8).reshape(self.height, self.width * 2)
            luma = packed.reshape(self.height, self.width // 2, 4)[:, :, :2].reshape(self.height, self.width).astype(np.int64)
            values = []
            for k in range(channels):
                d = k + 1
                diff = np.abs(luma[:, d:] - luma[:, :-d])
                values.append(int(diff.sum()) // 16)
            self._cache[key] = values
        return list(values)

    def preview_yuv(self, ctx, output):
        if (output['width'], output['height']) == (self.width, self.height) and output['stride_bytes'] == self.width * 2:
            return self.image
        return None

    def raw_pd(self, ctx, spec):
        """Textured PD frame (see ``pdaf_stream.scene_pd``): aperture separation round((target-lens)/pulses_per_sample)."""
        from emulator import pdaf_stream
        lens = 0 if ctx.position is None else round(ctx.position)
        if self.target_position is None:
            self.target_position = lens
        return pdaf_stream.scene_pd(spec, lens, self.target_position, pulses_per_sample=self.pulses_per_sample,
                                    flat=self.flat_pd, seed=self.seed, cache=self._pd_cache)

    def describe(self):
        return dict(name=self.name, width=self.width, height=self.height, target_position=self.target_position,
                    pulses_per_sample=self.pulses_per_sample, flat_pd=self.flat_pd,
                    pulses_per_blur_pass=self.pulses_per_blur_pass, mean_luma=self.mean_luma, seed=self.seed,
                    last=self.last)


# ---------------------------------------------------------------------------
# Camera focus-sweep captures

class SweepSceneSource(SceneSource):
    """Per-lens-position camera captures, interpolated by position (statistics, preview and phase detection).

    Layout contract (written by ``tools/x70_focus_sweep.py``): a directory holding ``session.json`` whose
    ``lease.samples[*]`` (older layouts: top-level ``samples``) carry ``index``, ``repeat`` and ``position`` (the
    pose estimate at 0xCCDE64), and per sample with repeat 0 the files ``pNNN-r0-statistics.bin`` (the
    0x29B0-byte bank at 0x1FECAA60), ``pNNN-r0-frame-720x480-yuv422.bin`` (720*480*2 packed bytes) and, for
    every repeat, ``pNNN-rR-paf-record.bin`` / ``pNNN-rR-pd-buffer.bin``.

    ``mode='nearest'`` returns the nearest position's bytes. ``mode='linear'`` blends the two bracketing banks
    lane-wise as unsigned 32-bit words (declared; the 64-bit gradient bands and opaque windows are not
    value-meaningful under blending and take the nearest sample).

    Phase detection (``pd='recorded'``).  The recorded PD buffer holds the stock paf task's *results*, not raw
    sensor rows (see ``research/af-sweeps/README.md``), so ``raw_pd`` synthesizes aperture planes from the decoded
    stock result of AF area ``pd_area`` (default 0, the whole AF frame) at the nearest recorded lens positions.
    Declared law: a position whose recorded readings were accepted (state valid/marginal in at least
    ``valid_fraction`` of the repeats) gets a textured plane pair whose disparity makes the stock extractor and
    correlation reproduce the recorded combined phase shift (8.8) through the linear map
    ``shift = shift_per_sample * disparity + shift_offset`` (calibrated on the bench, see
    ``research/af-sweeps/README.md``); between recorded positions the shift is interpolated linearly in lens
    position when both neighbours were accepted. A rejected position gets a featureless pair, which the stock
    code rejects (flag low-contrast).  The synthetic rows reproduce the shift and the accept/reject outcome of
    the camera, not its curve shapes, line statistics, noise, or the per-area differences; the stock decision
    code still runs on the emulator's own area configuration (one area).  ``pd=None`` disables ``raw_pd``.
    """
    name = 'sweep'
    BANK_BYTES = 0x29B0
    FRAME_BYTES = 720 * 480 * 2
    # Stock-shift law of ``pdaf_stream.aperture_planes_shifted`` measured on the bench (tools/calibrate_sweep_pd.py,
    # disparities -8..8 at lens 100: 8.8 shift = 170.75 * d + 26, fit error < 2 units): 512/3 per plane sample
    # (two passes of 256 per 3-plane-sample step) plus the stock phase bias 26 (cfg+8).
    SHIFT_PER_SAMPLE = 512.0 / 3.0
    SHIFT_OFFSET = 26.0

    def __init__(self, directory, *, mode='nearest', pd='recorded', pd_area=0, pd_table=None, valid_fraction=0.5,
                 shift_per_sample=None, shift_offset=None, max_disparity=48.0, seed=1):
        import json
        from pathlib import Path
        if mode not in ('nearest', 'linear'):
            raise ValueError("mode must be 'nearest' or 'linear'")
        if pd not in ('recorded', 'recorded-repeats', None):
            raise ValueError("pd must be 'recorded', 'recorded-repeats' or None")
        self.directory, self.mode = Path(directory), mode
        session = json.loads((self.directory / 'session.json').read_text())
        samples = session.get('samples') or session.get('lease', {}).get('samples', [])
        self._samples = samples
        self.entries = []
        for sample in samples:
            if sample.get('repeat') != 0 or sample.get('position') is None:
                continue
            prefix = f"p{sample['index']:03d}-r0"
            bank = self.directory / f'{prefix}-statistics.bin'
            if not bank.exists():
                continue
            self.entries.append(dict(position=sample['position'], prefix=prefix))
        if not self.entries:
            raise ValueError('Sweep directory has no sample with a statistics bank')
        self.entries.sort(key=lambda e: e['position'])
        self.positions = [e['position'] for e in self.entries]
        self._banks = {}
        self.pd, self.pd_area, self.valid_fraction = pd, pd_area, valid_fraction
        self.shift_per_sample = self.SHIFT_PER_SAMPLE if shift_per_sample is None else shift_per_sample
        self.shift_offset = self.SHIFT_OFFSET if shift_offset is None else shift_offset
        self.max_disparity, self.seed = max_disparity, seed
        self._pd_table = pd_table
        self._pd_cache = {}
        self.last_pd = None

    # ---------------------------------------------------------------- recorded phase-detection readings
    def pd_table(self):
        """``{position: [(accepted, shift_8_8), ...]}`` (one reading per recorded repeat), built lazily."""
        if self._pd_table is None:
            import json
            import sys
            from pathlib import Path
            root = Path(__file__).resolve().parents[2]
            cached = root / 'research/af-sweeps' / (self.directory.name + '.json')
            decoded_by_key = {}
            if cached.exists():
                decoded_by_key = {(d['index'], d['repeat']): d for d in json.loads(cached.read_text())['samples']}
            if str(root / 'tools') not in sys.path:
                sys.path.insert(0, str(root / 'tools'))
            import decode_af_sweep
            table = {}
            for sample in self._samples:
                if sample.get('position') is None:
                    continue
                try:
                    decoded = decoded_by_key.get((sample['index'], sample['repeat'])) or decode_af_sweep.decode_sample(
                        self.directory, sample)
                except FileNotFoundError:       # a sample without its saved PAF record (the daylight sweep has three)
                    continue
                areas = decoded['areas']
                if self.pd_area >= len(areas):
                    continue
                area = areas[self.pd_area]
                table.setdefault(sample['position'], []).append((area['state'] < 2, area['shift_8_8']))
            self._pd_table = table
        return self._pd_table

    def _pd_point(self, position):
        readings = self.pd_table()[position]
        accepted = [shift for ok, shift in readings if ok]
        if len(accepted) / len(readings) >= self.valid_fraction and accepted:
            return sum(accepted) / len(accepted)
        return None

    def pd_reading(self, lens_position):
        """``(accepted, shift_8_8)`` the declared law assigns to a lens position."""
        import bisect
        positions = sorted(self.pd_table())
        i = bisect.bisect_left(positions, lens_position)
        lo = positions[max(0, i - 1)]
        hi = positions[min(len(positions) - 1, i)]
        a, b = self._pd_point(lo), self._pd_point(hi)
        if lo == hi:
            return (a is not None), a
        t = (lens_position - lo) / (hi - lo) if i not in (0, len(positions)) else 0.0
        nearest = hi if t >= 0.5 else lo
        if a is not None and b is not None:
            return True, a + (b - a) * t
        value = self._pd_point(nearest)
        return value is not None, value

    def raw_pd(self, ctx, spec):
        if self.pd is None:
            return None
        from emulator import pdaf_stream
        lens = 0 if ctx.position is None else ctx.position
        accepted, shift = self.pd_reading(lens)
        disparity = 0.0
        if accepted:
            disparity = max(-self.max_disparity, min(self.max_disparity, (shift - self.shift_offset) / self.shift_per_sample))
        key = (spec['plane_width'], spec['height'], round(disparity * 8), accepted, self.seed)
        planes = self._pd_cache.get(key)
        if planes is None:
            planes = pdaf_stream.aperture_planes_shifted(spec['plane_width'], spec['height'], disparity=round(disparity * 8) / 8,
                                                         flat=not accepted, seed=self.seed)
            if len(self._pd_cache) >= 8:
                self._pd_cache.pop(next(iter(self._pd_cache)))
            self._pd_cache[key] = planes
        self.last_pd = dict(lens=lens, accepted=accepted, shift_8_8=shift, disparity=disparity)
        return planes

    # ---------------------------------------------------------------- statistics / preview
    def _bracket(self, position):
        import bisect
        i = bisect.bisect_left(self.positions, position)
        if i == 0:
            return self.entries[0], self.entries[0], 0.0
        if i == len(self.positions):
            return self.entries[-1], self.entries[-1], 0.0
        lo, hi = self.entries[i - 1], self.entries[i]
        span = hi['position'] - lo['position']
        return lo, hi, (position - lo['position']) / span if span else 0.0

    def nearest(self, position):
        lo, hi, t = self._bracket(position)
        return hi if t >= 0.5 else lo

    def _bank(self, entry):
        data = self._banks.get(entry['prefix'])
        if data is None:
            data = (self.directory / f"{entry['prefix']}-statistics.bin").read_bytes()
            if len(data) != self.BANK_BYTES:
                raise ValueError(f"{entry['prefix']} statistics bank is not 0x29B0 bytes")
            self._banks[entry['prefix']] = data
        return data

    def statistics(self, ctx):
        from emulator import recorded_statistics
        position = 0 if ctx.position is None else ctx.position
        lo, hi, t = self._bracket(position)
        near = hi if t >= 0.5 else lo
        bank_near = self._bank(near)
        bank = bank_near
        if self.mode == 'linear' and lo is not hi and 0.0 < t < 1.0:
            import struct
            a, b = self._bank(lo), self._bank(hi)
            words = struct.unpack('<%dI' % (self.BANK_BYTES // 4), a)
            other = struct.unpack('<%dI' % (self.BANK_BYTES // 4), b)
            blended = struct.pack('<%dI' % (self.BANK_BYTES // 4),
                                  *[min(0xffffffff, round(x + (y - x) * t)) for x, y in zip(words, other)])
            bank = blended
        offsets = dict(recorded_statistics.REGISTER_OFFSETS)
        spans = []
        for register, destination, source, length, scope in recorded_statistics.SPAN_LAYOUT:
            raw = bank_near if ('u64' in scope or 'opaque' in scope or 'maximum-ten' in scope) else bank
            spans.append((register, destination, raw[source:source + length]))
        return spans

    def preview_yuv(self, ctx, output):
        if (output['width'], output['height'], output['stride_bytes']) != (720, 480, 1440):
            return None
        entry = self.nearest(0 if ctx.position is None else ctx.position)
        path = self.directory / f"{entry['prefix']}-frame-720x480-yuv422.bin"
        if not path.exists():
            return None
        data = path.read_bytes()
        return data if len(data) == self.FRAME_BYTES else None

    def pd_rows(self, ctx):
        entry = self.nearest(0 if ctx.position is None else ctx.position)
        path = self.directory / f"{entry['prefix']}-pd-buffer.bin"
        return path.read_bytes() if path.exists() else None

    def pd_measurement(self, device, configuration):
        """Actual r0/r1 cost/scalar captures; stock still completes MMIO/IRQ.

        These captures contain no RAW rows. The declared repeat mode serves
        measured extractor outputs, preserving the stock correlation and AF
        state machine. Missing areas/groups fail rather than invent inputs.
        Driver request pointer 017ba680 is stored by stock 012129dc and read
        by callback 01212874; +2c is the destination curve. Buffer geometry
        is PafTask/Work in s5-4-hi-u/paf_algorithm.h (013480a0/0134a20c).
        """
        if self.pd != 'recorded-repeats':
            return None
        import struct
        from tools import decode_af_sweep as pd
        board = device.board
        request = struct.unpack('<I', board.read_memory(0x17ba680, 4))[0]
        curve = struct.unpack('<I', board.read_memory(request+44, 4))[0]
        buffers = struct.unpack('<I', board.read_memory(0x15f7c30, 4))[0]
        offset = (curve-buffers) & 0xffffffff
        slot, local = divmod(offset, pd.SLOT_BYTES)
        group = int(local >= 0x2f10)
        cost = local-(0x2f10 if group else 0x2310)-0x24
        which = int(cost >= 0x3e8)
        line, remainder = divmod(cost-which*0x3e8, 200)
        if slot >= 15 or remainder or not 0 <= line < 4:
            raise ValueError('Recorded repeat extraction is outside captured geometry')
        stream = board.pdaf_stream
        frame = stream.raw.last['frame']
        if not hasattr(self, '_repeat_events'):
            self._repeat_events, self._repeat_cache = [], {}
            self._repeat_frame, self._repeat_index = None, -1
        if frame != self._repeat_frame:
            self._repeat_index = (self._repeat_index+1) % 2
            self._repeat_frame = frame
        pose = board.sensor_stream.pose(board.virtual_ticks)[0]
        position = min(self.positions, key=lambda p: abs(p-pose))
        sample = next(s for s in self._samples if s['position'] == position and s['repeat'] == self._repeat_index)
        key = (sample['index'], self._repeat_index, slot, group)
        if key not in self._repeat_cache:
            data = (self.directory/f"p{sample['index']:03d}-r{self._repeat_index}-pd-buffer.bin").read_bytes()
            if slot >= struct.unpack_from('<I', data, 4)[0]:
                raise ValueError('Recorded repeat area was not captured: '+str(key))
            self._repeat_cache[key] = pd.Work(data, slot, group)
        work = self._repeat_cache[key]
        if work.samples != 41:
            raise ValueError('Recorded repeat group was not captured: '+str(key))
        values = work.line(line)['metrics']
        costs = work.curves(which)[line]
        totals, scalars = (values[4:6], values[6:8]) if which else ([values[1], values[0]], values[2:4])
        if not which: costs = costs[::-1]
        self._repeat_events.append(dict(frame=frame, repeat=self._repeat_index,
            pose=pose, recorded_position=position, index=sample['index'],
            slot=slot, group=group, which=which, line=line))
        return dict(configuration=configuration, sources=[], profile='recorded-camera-cost-metric-replay',
            result=dict(radius=20, curve=costs, totals=totals, scalars=scalars,
                samples=800, comparison_samples=600))

    def describe(self):
        return dict(name=self.name, directory=str(self.directory), mode=self.mode,
                    positions=self.positions, pd=self.pd, pd_area=self.pd_area,
                    shift_per_sample=self.shift_per_sample, shift_offset=self.shift_offset, last_pd=self.last_pd,
                    pd_repeat_events=getattr(self, '_repeat_events', []))


# ---------------------------------------------------------------------------
# Rendered / radiance-stack scenes through the calibrated sensor model

class RenderedSceneSource(SceneSource):
    """Per-lens-position radiance (pbrt renders, or any ``radiance_stacks.Stack``) through ``sensor_model.SensorModel``.

    ``source`` is a render directory (``manifest.json`` + per position EXRs, file contract in
    ``research/af-sensor/render-contract.md``) or a ``Stack``. For every delivered frame the scene is looked up by
    ``FrameContext.position`` (the physical lens position at exposure) and the sensor model, at light level ``ev`` (EV
    relative to the recorded *light* sweeps, ``0`` = light, ``-4.2`` = the recorded dark sweeps, fitted), produces with fresh
    per-frame noise (seeded by the frame index):

    * ``raw_pd``: the left/right U14 aperture planes (the stock extractor reads them; half-pupil radiance, Poisson +
      read noise, ADC, black level) with the ROI region of the frame filled from the stack;
    * ``preview_yuv``: the 720x480 live-view YYCbCr;
    * ``statistics``: bank windows from that preview (AE histograms, bands, totals, contrast grids / contrast-AF windows,
      the 0x25B0 histogram), unmodelled windows keep the camera's recorded idle bytes;
    * ``fe80_values``: the declared horizontal gradient energies of the preview luma.

    Ground truth (``truth``, ``depth_of_focus``) comes from the stack (manifest depth). Sensor parameters: see
    ``sensor_model.SensorParams`` (calibrated and declared values, ``research/af-sensor/calibration.json``).
    """
    name = 'rendered'
    PD_ZERO_BIAS_8_8 = -SweepSceneSource.SHIFT_OFFSET

    def __init__(self, source, *, ev=0.0, model=None, seed=1, mode=None, recorded=None, params=None):
        from .radiance_stacks import RenderedStack, TemporalRenderedStack, Stack
        from .sensor_model import SensorModel
        if isinstance(source, Stack):
            self.stack = source
        else:
            manifest = json.loads((Path(source) / 'manifest.json').read_text())
            self.stack = (TemporalRenderedStack(source) if isinstance(manifest.get('frame_sequence'), dict)
                          else RenderedStack(source))
        self.model = model or SensorModel(params, seed=seed)
        self.ev, self.mode = ev, mode
        self._recorded = recorded
        self._frame_cache = {}
        self.last = None

    # --------------------------------------------------------------------- ground truth
    @property
    def truth(self):
        return self.stack.truth

    @property
    def depth_of_focus(self):
        dof = getattr(self.stack, 'depth_of_focus_pulses', None)
        return dof() if callable(dof) else 7.0

    def set_light(self, ev):
        self.ev = ev
        self._frame_cache.clear()

    @property
    def recorded(self):
        if self._recorded is None:
            from emulator import recorded_statistics
            self._recorded = recorded_statistics.load_fixture(recorded_statistics.PROFILE)
        return self._recorded

    # --------------------------------------------------------------------- per-frame products
    def _frames(self, ctx):
        position = self.stack.positions[0] if ctx.position is None else ctx.position
        stack = self.stack.for_frame(ctx.index) if hasattr(self.stack, 'for_frame') else self.stack
        return stack.frames(position, self.mode) if self.mode else stack.frames(position)

    def _preview(self, ctx):
        key = (ctx.index, ctx.position, self.ev)
        hit = self._frame_cache.get(key)
        if hit is None:
            frames = self._frames(ctx)
            yuv = self.model.preview_yuv(frames.preview, self.ev, ctx.index,
                                         metering_mean=getattr(self.stack, 'metering_mean', None))
            hit = dict(yuv=yuv, frames=frames)
            if len(self._frame_cache) >= 4:
                self._frame_cache.pop(next(iter(self._frame_cache)))
            self._frame_cache[key] = hit
        return hit

    @staticmethod
    def _grid_dimensions(ctx):
        return SyntheticSceneSource._grid_dimensions(ctx)

    def statistics(self, ctx):
        yuv = self._preview(ctx)['yuv']
        columns, rows = self._grid_dimensions(ctx)
        spans, info = self.model.statistics_spans(yuv, template=self.recorded.spans, columns=columns, rows=rows, af=ctx.af)
        self.last = dict(info, position=ctx.position, ev=self.ev)
        return spans

    def preview_yuv(self, ctx, output):
        from .sensor_model import PREVIEW_H, PREVIEW_W
        if (output['width'], output['height'], output['stride_bytes']) != (PREVIEW_W, PREVIEW_H, PREVIEW_W * 2):
            return None
        return self._preview(ctx)['yuv']

    def fe80_values(self, ctx, channels):
        luma = self.model.luma(self._preview(ctx)['yuv']).astype(np.int64)
        return [int(np.abs(luma[:, k + 1:] - luma[:, :-(k + 1)]).sum()) // 16 for k in range(channels)]

    def raw_pd(self, ctx, spec):
        from .sensor_model import PD_ROI
        frames = self._frames(ctx)
        params = self.model.params
        scale = getattr(params, 'pd_defocus_scale', 1.0)
        if getattr(self.stack, 'ideal_optics', False) and scale != 1.0 and self.truth is not None and ctx.position is not None:
            # The camera's PD pairs behave as if the defocus were `pd_defocus_scale` times smaller (parallax and blur alike,
            # research/af-sensor/README.md section 5): the half-pupil views of the scaled lens position are served.
            stack = self.stack.for_frame(ctx.index) if hasattr(self.stack, 'for_frame') else self.stack
            frames = stack.frames(self.truth + scale * (ctx.position - self.truth), *([self.mode] if self.mode else []))
        roi = dict(PD_ROI)
        h, w = spec['height'], spec['plane_width']
        rows, cols = roi['row1'] - roi['row0'], roi['col1'] - roi['col0']
        if roi['row1'] > h or roi['col1'] > w:                       # geometry smaller than the captured one: centre the ROI
            roi.update(row0=max(0, (h - rows) // 2), col0=max(0, (w - cols) // 2))
            roi.update(row1=roi['row0'] + rows, col1=roi['col0'] + cols)
        left, right = frames.left, frames.right
        if getattr(self.stack, 'ideal_optics', False):
            # sensor-pixel sharp renders: the camera's effective PD baseline and the low-pass of the PD sampling (see sensor_model)
            from .radiance_stacks import gaussian_blur_x, shift_x, SHIFT_PER_SAMPLE
            params = self.model.params
            d = frames.meta.get('disparity')
            if params.pd_disparity_scale != 1.0 and d:
                k = 0.5 * (params.pd_disparity_scale - 1.0) * d
                left, right = shift_x(left, -k), shift_x(right, k)
            # Captured stock conversion adds +26 to the combined raw shift.
            # Camera sweeps already contain its compensating raw zero offset;
            # geometric renders have zero disparity at authored focus. Supply
            # that offset in the pupil pair, preserving blur and guest cfg.
            half_offset = 0.5 * self.PD_ZERO_BIAS_8_8 / SHIFT_PER_SAMPLE
            left, right = shift_x(left, -half_offset), shift_x(right, half_offset)
            if params.pd_prefilter_sigma:
                left, right = gaussian_blur_x(left, params.pd_prefilter_sigma), gaussian_blur_x(right, params.pd_prefilter_sigma)
        left, right = self.model.pd_frame(left, right, self.ev, ctx.index, frame=dict(height=h, plane_width=w), roi=roi)
        self.last_pd = dict(position=ctx.position, disparity=frames.meta.get('disparity'), ev=self.ev,
                            zero_bias_8_8=self.PD_ZERO_BIAS_8_8 if getattr(self.stack, 'ideal_optics', False) else 0)
        return dict(left=left, right=right)

    def describe(self):
        return dict(name=self.name, stack=getattr(self.stack, 'name', None), ev=self.ev, truth=self.truth,
                    depth_of_focus=self.depth_of_focus, positions=len(self.stack.positions), mode=self.mode,
                    sensor=self.model.describe(),
                    ideal_pd_zero_bias=dict(raw_shift_8_8=self.PD_ZERO_BIAS_8_8, status='declared',
                        evidence='Captured stock +26 phase bias; geometric renders have zero parallax at truth')
                        if getattr(self.stack, 'ideal_optics', False) else None,
                    temporal=self.stack.describe() if hasattr(self.stack, 'for_frame') else None)
