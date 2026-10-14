"""Per-lens-position radiance stacks: the data a ``RenderedSceneSource`` turns into sensor output.

A *stack* maps a lens position (committed pose, pulses) to ``Frames``: linear radiance of the two half-pupil PD views over
the stock PD region of interest, a full-frame preview radiance and, optionally, depth. Radiance is normalised so that the
reference full-pupil radiance is 1.0 (rendered AF patch; recorded proxy frame mean); exposure and light level are the
sensor model's job, never the stack's. Preview exposure meters independently unless a temporal stack holds its base meter.

``SweepProxyStack`` builds a stack from a recorded camera focus sweep (the preview frames are the radiance proxy; PD views are
made from the preview crop displaced by the sweep's recorded PD shift). ``RenderedStack`` (file contract in
``research/af-sensor/render-contract.md``) loads the pbrt pipeline's EXR directory.
"""
from __future__ import annotations

from dataclasses import dataclass, field
import bisect
import json
from pathlib import Path

import numpy as np

from .sensor_model import PD_ROI, PREVIEW_H, PREVIEW_W, resize

ROI_COLS = PD_ROI['col1'] - PD_ROI['col0']      # plane samples
ROI_ROWS = PD_ROI['row1'] - PD_ROI['row0']      # raster rows
SHIFT_PER_SAMPLE, SHIFT_OFFSET = 512.0 / 3.0, 26.0      # stock shift (8.8) = 170.67 * disparity + 26 (research/af-sweeps)


@dataclass
class Frames:
    """One lens position. ``left``/``right``: ``(ROI_ROWS, ROI_COLS)`` float32 half-pupil radiance (full-pupil normalised),
    ``preview``: ``(H, W)`` luma or ``(H, W, 3)`` RGB, ``meta``: free-form (disparity, depth, ...)."""
    left: np.ndarray
    right: np.ndarray
    preview: np.ndarray
    meta: dict = field(default_factory=dict)


def blend(a: Frames, b: Frames, t: float) -> Frames:
    mix = lambda x, y: (x * (1.0 - t) + y * t).astype(np.float32)
    meta = dict(a.meta if t < 0.5 else b.meta)
    return Frames(mix(a.left, b.left), mix(a.right, b.right), mix(a.preview, b.preview), meta)


class Stack:
    """Interface: ``positions`` (sorted, committed-pose pulses), ``load(position)`` -> Frames, ``truth`` (in-focus position)."""
    positions: list
    truth: float | None = None
    name = 'stack'
    ideal_optics = False     # True: PD views carry the ideal half-pupil parallax (the sensor model applies its PD baseline scale)

    def load(self, position) -> Frames:
        raise NotImplementedError

    def frames(self, position, mode='linear') -> Frames:
        """Frames at an arbitrary lens position: nearest recorded/rendered position, or a linear blend of the neighbours."""
        pos = self.positions
        i = bisect.bisect_left(pos, position)
        if i == 0:
            return self.cached(pos[0])
        if i == len(pos):
            return self.cached(pos[-1])
        lo, hi = pos[i - 1], pos[i]
        t = (position - lo) / (hi - lo) if hi != lo else 0.0
        if mode == 'nearest' or t <= 0.0 or t >= 1.0:
            return self.cached(hi if t >= 0.5 else lo)
        return blend(self.cached(lo), self.cached(hi), t)

    def cached(self, position) -> Frames:
        cache = self.__dict__.setdefault('_cache', {})
        if position not in cache:
            if len(cache) >= 6:
                cache.pop(next(iter(cache)))
            cache[position] = self.load(position)
        return cache[position]


def gaussian_blur_x(a, sigma):
    """Gaussian blur along the last axis (edge replicate), float32."""
    if sigma < 0.05:
        return a
    r = max(1, int(np.ceil(3 * sigma)))
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    pad = np.pad(a, ((0, 0), (r, r)), mode='edge')
    out = np.zeros(a.shape, dtype=np.float64)
    for i, w in enumerate(k):
        out += w * pad[:, i:i + a.shape[1]]
    return out.astype(np.float32)


def _bilinear_sample(image, ys, xs):
    """Bilinear lookup of ``image[y, x]`` at float coordinates (clamped), broadcasting ``ys``/``xs``."""
    h, w = image.shape[:2]
    ys = np.clip(ys, 0, h - 1)
    xs = np.clip(xs, 0, w - 1)
    y0, x0 = np.floor(ys).astype(np.int64), np.floor(xs).astype(np.int64)
    y1, x1 = np.minimum(y0 + 1, h - 1), np.minimum(x0 + 1, w - 1)
    fy, fx = (ys - y0).astype(np.float32), (xs - x0).astype(np.float32)
    top = image[y0, x0] * (1 - fx) + image[y0, x1] * fx
    bottom = image[y1, x0] * (1 - fx) + image[y1, x1] * fx
    return (top * (1 - fy) + bottom * fy).astype(np.float32)


class SweepProxyStack(Stack):
    """A recorded camera focus sweep as a radiance stack (preview frames = radiance proxy).

    Declared mapping (the real mapping of the PD area onto live-view pixels is not recovered): the PD region of interest
    (``ROI_COLS`` plane samples x ``ROI_ROWS`` raster rows) covers a ``window`` of preview pixels (default 70 x 24, the crop
    that best tracks the PD truth in ``research/af-sweeps``) centred at ``center`` (default 346, 240). Preview luma is
    linearised with ``gamma`` (declared 2.2) and normalised by its frame mean. The two half-pupil views are the preview crop
    displaced by +-d/2 plane samples, ``d = (shift - 26) / 170.67`` with ``shift`` the sweep's recorded stock phase shift at
    that position (area 0, mean of the accepted repeats; interpolated across rejected positions)."""
    name = 'sweep-proxy'

    def __init__(self, directory, *, gamma=2.2, center=(346.0, 240.0), window=(70.0, 24.0), truth=81.6, pd_area=0,
                 modulation=None, texture=None):
        self.directory = Path(directory)
        self.texture = texture      # dict(mix, amplitude, sigma0, alpha, octaves, blur_per_sample, blur_floor, seed) or None
        self.modulation = modulation        # {position: gain} on the PD views' deviation from their mean (calibration of the proxy)
        self.gamma, self.center, self.window, self.truth = gamma, center, window, truth
        session = json.loads((self.directory / 'session.json').read_text())
        samples = session.get('samples') or session['lease']['samples']
        self.entries = {}
        for s in samples:
            if s.get('repeat') == 0 and s.get('position') is not None:
                p = f"p{s['index']:03d}-r0"
                if (self.directory / f'{p}-frame-720x480-yuv422.bin').exists():
                    self.entries[s['position']] = p
        self.positions = sorted(self.entries)
        self.shift, self.shift_known = self._recorded_shifts(samples, pd_area)

    def _recorded_shifts(self, samples, pd_area):
        import sys
        root = Path(__file__).resolve().parents[2]
        cached = root / 'research/af-sweeps' / (self.directory.name + '.json')
        if cached.exists():
            decoded = {(d['index'], d['repeat']): d for d in json.loads(cached.read_text())['samples']}
        else:
            if str(root / 'tools') not in sys.path:
                sys.path.insert(0, str(root / 'tools'))
            import decode_af_sweep
            decoded = {(s['index'], s['repeat']): decode_af_sweep.decode_sample(self.directory, s) for s in samples}
        readings = {}
        for s in samples:
            d = decoded.get((s['index'], s['repeat']))
            if d is None or s.get('position') is None or pd_area >= len(d['areas']):
                continue
            area = d['areas'][pd_area]
            readings.setdefault(s['position'], []).append((area['state'] < 2, area['shift_8_8']))
        known = {}
        for position, items in readings.items():
            ok = [v for good, v in items if good]
            if ok:
                known[position] = float(np.mean(ok))
        xs = sorted(known)
        if not xs:
            raise ValueError('sweep has no accepted PD reading')
        return {p: float(np.interp(p, xs, [known[x] for x in xs])) for p in self.positions}, known

    def _texture_field(self):
        """Static fine texture (relative deviation from 1, unit std) on a margin-extended ROI grid, cached."""
        t = self.texture
        key = ('texture', t.get('seed', 1), t['sigma0'], t['alpha'], t.get('octaves', 3))
        field = self.__dict__.get('_field')
        if field is None or field[0] != key:
            margin = 48
            rng = np.random.default_rng(int(t.get('seed', 1)))
            total = np.zeros((ROI_ROWS, ROI_COLS + 2 * margin), dtype=np.float64)
            for octave in range(int(t.get('octaves', 3))):
                noise = rng.normal(size=total.shape)
                sigma = t['sigma0'] * 2.0 ** octave
                layer = gaussian_blur_x(noise, sigma)
                layer /= max(layer.std(), 1e-9)
                total += layer * 2.0 ** (-octave * t['alpha'])
            total = (total - total.mean()) / max(total.std(), 1e-9)
            self._field = (key, total.astype(np.float32), margin)
        return self._field[1], self._field[2]

    def _textured(self, left, right, disparity):
        """Replace (``mix`` = 1) or add to the preview-derived views a fine texture, defocus-blurred and displaced like them."""
        t = self.texture
        field, margin = self._texture_field()
        table = t.get('blur_table')
        if table is not None:
            sigma = float(np.interp(self._current_position, sorted(table), [table[k] for k in sorted(table)]))
        else:
            sigma = float(np.hypot(t['blur_per_sample'] * abs(disparity) ** t.get('blur_power', 1.0), t['blur_floor']))
        views = []
        for sign, base in ((+0.5, left), (-0.5, right)):
            shift = sign * disparity
            xs = np.arange(ROI_COLS, dtype=np.float32) + margin + shift
            ys = np.arange(ROI_ROWS, dtype=np.float32)
            yy, xx = np.meshgrid(ys, xs, indexing='ij')
            view = gaussian_blur_x(_bilinear_sample(field, yy, xx), sigma) if sigma > 0.05 else _bilinear_sample(field, yy, xx)
            view = 1.0 + t['amplitude'] * view
            views.append(((1.0 - t['mix']) * base + t['mix'] * view).astype(np.float32))
        return views[0], views[1]

    def preview_luma(self, position):
        raw = (self.directory / f'{self.entries[position]}-frame-720x480-yuv422.bin').read_bytes()
        packed = np.frombuffer(raw, dtype=np.uint8).reshape(PREVIEW_H, PREVIEW_W // 2, 4)
        return packed[:, :, :2].reshape(PREVIEW_H, PREVIEW_W).astype(np.float32)

    def load(self, position):
        self._current_position = position
        y = self.preview_luma(position)
        lin = np.clip((y - 16.0) / 219.0, 0.0, 1.0) ** self.gamma
        lin = lin / max(float(lin.mean()), 1e-9)
        shift = self.shift[position]
        disparity = (shift - SHIFT_OFFSET) / SHIFT_PER_SAMPLE            # plane samples
        wx, wy = self.window
        cx, cy = self.center
        cols = (np.arange(ROI_COLS, dtype=np.float32) - (ROI_COLS - 1) / 2.0) * (wx / ROI_COLS)
        rows = (np.arange(ROI_ROWS, dtype=np.float32) - (ROI_ROWS - 1) / 2.0) * (wy / ROI_ROWS)
        px_per_sample = wx / ROI_COLS
        yy, xx = np.meshgrid(cy + rows, cx + cols, indexing='ij')
        left = _bilinear_sample(lin, yy, xx + 0.5 * disparity * px_per_sample)
        right = _bilinear_sample(lin, yy, xx - 0.5 * disparity * px_per_sample)
        if self.texture is not None:
            left, right = self._textured(left, right, disparity)
        gain = None if self.modulation is None else self.modulation.get(position)
        if gain is not None:
            mean = 0.5 * (left.mean() + right.mean())
            left, right = (mean + gain * (left - mean)).astype(np.float32), (mean + gain * (right - mean)).astype(np.float32)
        return Frames(left, right, lin, dict(disparity=float(disparity), recorded_shift=float(shift), position=position))


# ---------------------------------------------------------------------------------------------------------------------
# Rendered scenes (pbrt pipeline, file contract: research/af-sensor/render-contract.md)

def _exr_site_packages():
    """``<repo>/.tools/af-render-venv`` site-packages (the render pipeline's venv; also found from a worktree)."""
    import os
    import sys
    candidates = [os.environ.get('X70_EXR_SITE_PACKAGES')]
    for ancestor in Path(__file__).resolve().parents:
        for sub in ('.tools/af-render-venv', 'af-render-venv'):
            candidates.append(ancestor / sub / 'lib' / f'python{sys.version_info.major}.{sys.version_info.minor}' / 'site-packages')
    return next((Path(c) for c in candidates if c and Path(c).exists()), None)


AF_RENDER_VENV = _exr_site_packages()
PULSE_MM = 0.0037453           # lens extension per pulse of the X70-like render lens (af-render manifest: 15 pulses = 0.056179 mm)
ORIGIN_PULSE = -119.0          # committed pulse at infinity focus in the same lens model


def _openexr():
    try:
        import OpenEXR
        return OpenEXR
    except ImportError:
        import sys
        if AF_RENDER_VENV is not None and str(AF_RENDER_VENV) not in sys.path:
            sys.path.append(str(AF_RENDER_VENV))      # same Python/ABI, only used when OpenEXR is not installed
        import OpenEXR
        return OpenEXR


def read_image(path):
    """``(H, W, C)`` float32 and the data-window origin ``(x0, y0)``; ``.exr`` (RGB or Y channels) or ``.npy``."""
    path = Path(path)
    if path.suffix == '.npy':
        a = np.load(path).astype(np.float32)
        return (a[..., None] if a.ndim == 2 else a), (0, 0)
    exr = _openexr().File(str(path))
    channels = exr.channels()
    key = 'RGB' if 'RGB' in channels else 'Y' if 'Y' in channels else next(iter(channels))
    a = np.asarray(channels[key].pixels, dtype=np.float32)
    window = exr.header()['dataWindow']
    return (a[..., None] if a.ndim == 2 else a), (int(window[0][0]), int(window[0][1]))


def luminance(image):
    a = np.asarray(image, dtype=np.float32)
    if a.ndim == 2 or a.shape[-1] == 1:
        return a.reshape(a.shape[0], a.shape[1])
    return (0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]).astype(np.float32)


def measure_disparity(left, right, radius=40):
    """Plane-sample disparity ``d`` with ``right(x) = left(x - d)`` (the stock SAD convention: ``argmin_s sum |L(x) - R(x+s)|``)."""
    l, r = np.asarray(left, dtype=np.float64), np.asarray(right, dtype=np.float64)
    width = l.shape[1]
    radius = min(radius, width // 3)
    costs = np.array([np.abs(l[:, radius:width - radius] - r[:, radius + s:width - radius + s]).sum()
                      for s in range(-radius, radius + 1)])
    i = int(costs.argmin())
    if 0 < i < len(costs) - 1:
        a, b, c = costs[i - 1], costs[i], costs[i + 1]
        den = a - 2 * b + c
        frac = 0.5 * (a - c) / den if den > 0 else 0.0
    else:
        frac = 0.0
    return float(i - radius + frac)


def shift_x(image, shift):
    """Resample along x by ``shift`` samples (positive moves content toward larger x), linear, edge replicate."""
    if abs(shift) < 1e-6:
        return image
    h, w = image.shape
    xs = np.arange(w, dtype=np.float32) - shift
    yy, xx = np.meshgrid(np.arange(h, dtype=np.float32), xs, indexing='ij')
    return _bilinear_sample(image, yy, xx)


class RenderedStack(Stack):
    """The pbrt renders of one scene/placement (``manifest.json`` + per position ``full/left/right[/preview]`` images).

    * the half-pupil views cover ``pd_region`` (sensor pixels). They are resampled to the stock PD region of interest:
      ``px_per_sample`` sensor pixels per PD plane sample horizontally (declared 2: left/right pixels interleave), centre
      cropped to the ROI width, rows stretched to the ROI rows (the ROI spans more raster rows than the render region);
    * radiance is divided by the median full-pupil AF-region mean over sampled positions. ``pd_e_ref`` was calibrated
      on that patch, not the whole-frame preview; preview exposure meters its own image independently;
    * between rendered positions the views of the nearest position are re-registered to the linearly interpolated disparity
      (``mode='shift'``, preview radiance blends neighbours), blended (``'blend'``) or taken as is (``'nearest'``);
    * ``truth``: ``manifest['truth_pulse']`` when present, else the pulse at which the lens focus distance equals the centre
      ray depth (interpolated in diopters over the rendered focus distances), else ``nominal_pulse``.
    """
    name = 'rendered'
    ideal_optics = True

    def __init__(self, directory, *, px_per_sample=2.0, mode='shift'):
        self.directory = Path(directory)
        self.manifest = json.loads((self.directory / 'manifest.json').read_text())
        self.px_per_sample, self.mode = px_per_sample, mode
        entries = self.manifest['positions']
        self.entries = {float(e['pulse']): e for e in entries.values()}
        self.positions = sorted(self.entries)
        self._reference = None
        self._disparity = {}
        self.truth = self._truth()

    # ------------------------------------------------------------------ manifest facts
    def _truth(self):
        m = self.manifest
        if m.get('truth_pulse') is not None:
            return float(m['truth_pulse'])
        depth = (m.get('depth') or {}).get('center_ray_depth_film_plane_mm')
        pts = []
        for e in self.entries.values():
            dist = (e.get('focus') or {}).get('focus_distance_from_film_plane_mm')
            if dist is not None:
                pts.append((float(e['pulse']), 0.0 if dist in (None, float('inf')) else 1.0 / float(dist)))
        if depth and len(pts) >= 2:
            pts.sort()
            pulses, diopters = zip(*pts)
            target = 1.0 / float(depth)
            if min(diopters) <= target <= max(diopters):
                order = np.argsort(diopters)
                return float(np.interp(target, np.asarray(diopters)[order], np.asarray(pulses)[order]))
        return float(m['nominal_pulse']) if m.get('nominal_pulse') is not None else None

    def depth_of_focus_pulses(self, blur_px=2.0):
        """Half depth of focus in pulses: defocus whose blur circle is ``blur_px`` pixels (manifest optics)."""
        m = self.manifest
        pitch_mm = m['pixel_pitch_um'] * 1e-3
        v = m['focal_mm'] + (self.truth - ORIGIN_PULSE) * PULSE_MM
        return blur_px * pitch_mm * v / m['aperture_diameter_mm'] / PULSE_MM

    # ------------------------------------------------------------------ images
    def _path(self, entry, view):
        item = (entry.get('renders') or {}).get(view)
        return None if item is None else self.directory / item['file']

    def _roi(self, path):
        image, _ = read_image(path)
        lum = luminance(image)
        samples = lum.shape[1] / self.px_per_sample
        lum = resize(lum, ROI_ROWS, int(round(samples)))
        if lum.shape[1] >= ROI_COLS:
            c0 = (lum.shape[1] - ROI_COLS) // 2
            lum = lum[:, c0:c0 + ROI_COLS]
        else:
            pad = ROI_COLS - lum.shape[1]
            lum = np.pad(lum, ((0, 0), (pad // 2, pad - pad // 2)), mode='edge')
        return lum

    def reference(self):
        if self._reference is None:
            means = []
            for position in self.positions[::max(1, len(self.positions) // 5)]:
                entry = self.entries[position]
                path = self._path(entry, 'full')
                if path is None:
                    raise ValueError('Rendered PD reference requires a full-pupil AF-region image')
                means.append(float(luminance(read_image(path)[0]).mean()))
            self._reference = float(np.median(means))
            if not np.isfinite(self._reference) or self._reference <= 0:
                raise ValueError('Rendered PD reference must be finite and positive')
        return self._reference

    def preview_for(self, entry):
        path = self._path(entry, 'preview')
        if path is not None:
            return read_image(path)[0] / self.reference()
        # fallback (declared; render a preview!): the full-pupil region image, reduced to preview scale and mirror-tiled over the
        # frame, as for a fronto-parallel textured plane. A flat field here makes the stock S1 gate choose contrast AF.
        full = luminance(read_image(self._path(entry, 'full'))[0]) / self.reference()
        scale = max(1.0, self.manifest.get('preview', {}).get('scale', 8) * 0.85)
        small = resize(full, max(2, int(round(full.shape[0] / scale))), max(4, int(round(full.shape[1] / scale))))
        tile = np.block([[small, small[:, ::-1]], [small[::-1], small[::-1, ::-1]]])
        reps = (PREVIEW_H // tile.shape[0] + 1, PREVIEW_W // tile.shape[1] + 1)
        return np.tile(tile, reps)[:PREVIEW_H, :PREVIEW_W].astype(np.float32)

    def disparity(self, position):
        """Plane-sample disparity of a rendered position (measured on the resampled views, stock SAD sign)."""
        if position not in self._disparity:
            entry = self.entries[position]
            left = self._roi(self._path(entry, 'left'))
            right = self._roi(self._path(entry, 'right'))
            self._disparity[position] = measure_disparity(left, right)
        return self._disparity[position]

    def load(self, position):
        entry = self.entries[position]
        ref = self.reference()
        left = self._roi(self._path(entry, 'left')) / ref
        right = self._roi(self._path(entry, 'right')) / ref
        return Frames(left.astype(np.float32), right.astype(np.float32), self.preview_for(entry).astype(np.float32),
                      dict(position=position, disparity=self.disparity(position), entry=entry.get('tag')))

    def frames(self, position, mode=None):
        mode = mode or self.mode
        if mode != 'shift':
            return super().frames(position, mode)
        pos = self.positions
        i = bisect.bisect_left(pos, position)
        near = pos[0] if i == 0 else pos[-1] if i == len(pos) else (pos[i] if pos[i] - position < position - pos[i - 1] else pos[i - 1])
        base = self.cached(near)
        # PDAF re-registers the nearest half-pupil pair, but the preview must
        # follow the sampled physical pose too. Reusing base.preview made a
        # continuous stock contrast scan see plateaus and abrupt focus jumps.
        preview = base.preview
        if 0 < i < len(pos):
            lo, hi = pos[i - 1], pos[i]
            t = (position - lo) / (hi - lo)
            preview = (self.cached(lo).preview * (1 - t) + self.cached(hi).preview * t).astype(np.float32)
        disp = [self.disparity(p) for p in pos]
        d_here = float(np.interp(position, pos, disp))
        if len(pos) >= 2 and position < pos[0]:            # beyond the rendered range: extrapolate the parallax linearly
            d_here = disp[0] + (position - pos[0]) * (disp[1] - disp[0]) / (pos[1] - pos[0])
        elif len(pos) >= 2 and position > pos[-1]:
            d_here = disp[-1] + (position - pos[-1]) * (disp[-1] - disp[-2]) / (pos[-1] - pos[-2])
        delta = d_here - base.meta['disparity']
        if abs(delta) < 1e-3 and preview is base.preview:
            return base
        left = shift_x(base.left, -0.5 * delta)       # right(x) = left(x - d): raise d by moving right toward +x, left toward -x
        right = shift_x(base.right, 0.5 * delta)
        return Frames(left, right, preview, dict(base.meta, disparity=d_here, sampled_position=position))


class TemporalRenderedStack(Stack):
    """Frozen lighting states selected by absolute sensor frame index.

    All states share base radiance and preview metering references. Geometry
    and truth remain the base scene's; lighting variants may add dense poses.
    """
    name = 'temporal-rendered'
    ideal_optics = True

    def __init__(self, directory):
        import hashlib
        directory = Path(directory)
        manifest = json.loads((directory / 'manifest.json').read_text())
        self.sequence = manifest['frame_sequence']
        names, self.cycle = self.sequence['directories'], self.sequence['cycle']
        if (not names or not self.cycle or any(Path(n).name != n for n in names)
                or any(type(i) is not int or not 0 <= i < len(names) for i in self.cycle)):
            raise ValueError('Invalid temporal directory/cycle mapping')
        self.stacks, self.manifest_hashes = [], {}
        for name in names:
            path = directory.parent / name
            raw = (path / 'manifest.json').read_bytes()
            m = json.loads(raw)
            if not m.get('preparation', {}).get('denoise'):
                raise ValueError(f'Temporal state {name} is not prepared')
            entries = m.get('positions', {}).values()
            if not m.get('positions') or any(any(v not in e.get('renders', {}) or
                    not (path / e['renders'][v]['file']).is_file()
                    for v in ('full', 'left', 'right', 'preview')) for e in entries):
                raise ValueError(f'Temporal state {name} is incomplete')
            self.stacks.append(RenderedStack(path))
            self.manifest_hashes[name] = hashlib.sha256(raw).hexdigest()
        base = self.stacks[0]
        self.positions, self.truth = base.positions, base.truth
        ref = base.reference()
        if not np.isfinite(ref) or ref <= 0:
            raise ValueError('Temporal base metering reference must be positive')
        for stack in self.stacks:
            stack._reference = ref
            stack.truth = self.truth
        self.metering_mean = float(base.frames(self.truth).preview.mean())
        self.reference_radiance = ref

    def for_frame(self, index):
        return self.stacks[self.cycle[index % len(self.cycle)]]

    def depth_of_focus_pulses(self):
        return self.stacks[0].depth_of_focus_pulses()

    def describe(self):
        return dict(sequence=self.sequence, manifest_sha256=self.manifest_hashes,
                    reference_radiance=self.reference_radiance,
                    preview_metering_mean=self.metering_mean,
                    assumption='Frozen lighting per frame; base exposure held; no rolling shutter/motion blur')
