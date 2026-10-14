"""Sensor model: scene radiance -> what the stock AE/AF code reads from the sensor path.

Input is *linear scene radiance*, normalised so that the metering reference (the mean of the full-pupil radiance over the
frame, what auto-exposure meters) is 1.0. The model turns it, at a chosen light level ``ev``, into

* PD aperture-plane rows (``pd_planes``): the two U14 planes the stock extractor reads, from the left/right half-pupil
  radiance (electrons: Poisson shot noise + Gaussian read noise, ADC gain, black level, quantization, clipping);
* the live-view YYCbCr preview (``preview_yuv``): 720x480 packed Y0 Y1 Cb Cr, with the preview's own exposure program,
  noise and tone curve;
* the statistics-bank windows the stock AE/AF consumers read (``statistics_spans``): AE histogram windows and the
  contrast grids, computed from the (noisy) preview with the declared laws of ``afs_scene_fixture`` /
  ``afs_focus_scene``; one window (the 0x25B0 histogram) uses a law calibrated against the recorded banks.

Light level: ``ev`` is the scene luminance relative to the recorded *light* sweeps (``ev = 0``; ``-4.2`` is the effective
level at which the model reproduces the recorded dark sweeps' PD acceptance and spread, see research/af-sensor/README.md). Camera exposure programs follow it per stream: the PD stream keeps its exposure
(declared: no AE headroom, ``pd_ae_max_ev = 0``), the preview stream's AE compensates up to ``preview_ae_max_ev``.

Every parameter carries a status in ``SensorParams.STATUS``: ``calibrated`` (fitted against the recorded sweeps by
``tools/af_sensor_calibrate.py``, values in ``research/af-sensor/calibration.json``) or ``declared`` (a stated assumption
that no recorded data constrains). ``SensorParams.load()`` reads the calibration file when present.
"""
from __future__ import annotations

from dataclasses import dataclass, field, fields, asdict
import json
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
CALIBRATION = ROOT / 'research/af-sensor/calibration.json'

# Frame geometry of the continuous PD stream (captured: AFBench half-press, emulator/testbed/paf_probe.py).
PD_FRAME = dict(width=5120, height=329, plane_width=2560, packed=True, pitch=7680)
PD_ROI = dict(col0=1115, col1=1354, row0=99, row1=218)          # plane columns / raster rows the stock jobs read
PREVIEW_W, PREVIEW_H = 720, 480
STAT_W, STAT_H = 1200, 800           # statistics raster of the 0x25B0 histogram window (960,000 counts in the recordings)


def resize(image, height, width):
    """Separable bilinear resize (box pre-average when shrinking by 2x or more); float32 in, float32 out."""
    a = np.asarray(image, dtype=np.float32)
    h, w = a.shape[:2]
    if (h, w) == (height, width):
        return a
    for axis, size, new in ((0, h, height), (1, w, width)):
        factor = size // new
        if factor >= 2:
            cut = (a.shape[axis] // factor) * factor
            a = np.take(a, np.arange(cut), axis=axis)
            shape = list(a.shape)
            shape[axis:axis + 1] = [cut // factor, factor]
            a = a.reshape(shape).mean(axis=axis + 1)
        n = a.shape[axis]
        if n != new:
            src = (np.arange(new, dtype=np.float64) + 0.5) * n / new - 0.5
            lo = np.clip(np.floor(src).astype(np.int64), 0, n - 1)
            hi = np.clip(lo + 1, 0, n - 1)
            t = (np.clip(src, 0, n - 1) - lo).astype(np.float32)
            shape = [1] * a.ndim
            shape[axis] = new
            t = t.reshape(shape)
            a = np.take(a, lo, axis=axis) * (1 - t) + np.take(a, hi, axis=axis) * t
    return a.astype(np.float32)


def expand_luma(luma, gamma):
    """Gamma-expanded luma ``1000 * ((Y - 16) / 219) ** gamma`` (float64) of limited-range preview luma."""
    return 1000.0 * np.clip((np.asarray(luma, dtype=np.float64) - 16.0) / 219.0, 0.0, 1.0) ** gamma


def gauss_x(image, sigma):
    """Horizontal Gaussian (edge padded, same width)."""
    radius = int(np.ceil(4 * sigma))
    k = np.exp(-0.5 * (np.arange(-radius, radius + 1) / sigma) ** 2)
    k /= k.sum()
    padded = np.pad(image, ((0, 0), (radius, radius)), mode='edge')
    return sum(w * padded[:, i:i + image.shape[1]] for i, w in enumerate(k))


def box_x(image, n):
    """Horizontal box average of ``n`` pixels (valid width ``W - n + 1``)."""
    if n <= 1:
        return image
    csum = np.cumsum(np.pad(image, ((0, 0), (1, 0))), axis=1)
    return (csum[:, n:] - csum[:, :-n]) / n


@dataclass
class SensorParams:
    """All sensor-model parameters. Defaults are the declared starting values; ``load()`` applies the calibration."""
    # --- phase-detection pixels
    pd_e_ref: float = 22400.0          # electrons per full-pupil PD sample at reference radiance (1.0), ev = 0
    pd_sensitivity: float = 0.5        # light fraction of a half-pupil PD sample (left/right views are full-pupil normalised)
    pd_read_noise_e: float = 4.0       # RMS read noise of one PD sample, electrons
    pd_gain_dn_per_e: float = 0.5      # ADC gain, DN per electron (ISO gain)
    black_level_dn: float = 1024.0     # optical black pedestal, U14 DN
    adc_bits: int = 14                 # quantization / clipping depth of the PD planes
    pd_ae_max_ev: float = 0.0          # exposure headroom the PD stream's AE adds as light falls (EV)
    pd_row_gain_sd: float = 0.0        # per-row fixed gain pattern (fractional sd), declared off
    pd_disparity_scale: float = 1.0    # parallax-only baseline scale of rendered scenes (superseded by pd_defocus_scale; 1 = off)
    pd_defocus_scale: float = 1.0      # effective PD defocus relative to the true one (rendered scenes): disparity and blur alike
    pd_prefilter_sigma: float = 5.0    # horizontal Gaussian of the PD samples, plane samples (renders are sensor-pixel sharp)
    # --- live-view preview
    preview_mean_linear: float = 0.17  # linear mean of the preview at ev = 0 (AE target; recorded light sweeps)
    preview_gamma: float = 2.2         # tone curve Y = 16 + 219 * lin^(1/gamma)
    preview_e_full: float = 150000.0   # electrons per preview pixel at linear 1.0 (shot noise scale)
    preview_noise_lin: float = 0.011   # additive Gaussian noise, linear units (read + ISP), signal independent
    preview_ae_max_ev: float = 4.1     # AE exposure compensation range of the live view (EV)
    preview_iso_fraction: float = 0.0  # share of that compensation applied as gain after the read noise (noise x 2^(fraction*EV))
    preview_black: int = 16            # limited-range YCbCr
    preview_white: int = 235
    # --- statistics bank
    stat_hist_total: int = STAT_W * STAT_H   # counts of the 0x25B0 histogram window (recordings: 960,000)
    contrast_smooth: int = 15          # box width (preview px) ahead of the contrast grid's plane-1 gradient (0: declared law)
    contrast_window: tuple = (240, 160)  # centred preview window the contrast grid cells cover ((): whole frame, declared)
    contrast_scale: tuple = (1.0, 1.0)  # multipliers on contrast-grid plane 0 and plane 1 (declared 1)
    contrast_gamma: float = 0.0        # >0: planes are computed on gamma-expanded luma 1000*((Y-16)/219)^gamma (0: gamma-coded Y, legacy laws)
    contrast_core: float = 0.0         # plane 0 (gamma > 0): |gradient| below this (gamma-expanded units) is coring noise and counts nothing
    contrast_sigma: float = 1.2        # plane 0 (gamma > 0): horizontal Gaussian sigma (preview px) ahead of the gradient
    contrast_window0: tuple = (160, 100)   # plane 0 (gamma > 0): centred preview window (plane 1 keeps `contrast_window`)

    STATUS = {
        'pd_e_ref': 'calibrated', 'pd_sensitivity': 'declared', 'pd_read_noise_e': 'calibrated',
        'pd_gain_dn_per_e': 'declared', 'black_level_dn': 'declared', 'adc_bits': 'declared',
        'pd_ae_max_ev': 'declared', 'pd_row_gain_sd': 'declared', 'pd_disparity_scale': 'declared', 'pd_defocus_scale': 'calibrated', 'pd_prefilter_sigma': 'declared',
        'preview_mean_linear': 'calibrated', 'preview_gamma': 'declared', 'preview_e_full': 'declared',
        'preview_noise_lin': 'calibrated', 'preview_ae_max_ev': 'calibrated', 'preview_iso_fraction': 'calibrated', 'preview_black': 'declared',
        'preview_white': 'declared', 'stat_hist_total': 'calibrated', 'contrast_smooth': 'calibrated', 'contrast_window': 'calibrated', 'contrast_scale': 'calibrated',
        'contrast_gamma': 'calibrated', 'contrast_core': 'calibrated', 'contrast_sigma': 'calibrated', 'contrast_window0': 'calibrated',
    }

    @classmethod
    def load(cls, path=CALIBRATION):
        path = Path(path)
        params = cls()
        if path.exists():
            values = json.loads(path.read_text()).get('parameters', {})
            known = {f.name for f in fields(cls)}
            params = cls(**{k: v for k, v in values.items() if k in known})
        return params

    def to_dict(self):
        return asdict(self)

    def describe(self):
        return {f.name: dict(value=getattr(self, f.name), status=self.STATUS.get(f.name, 'declared'))
                for f in fields(self)}


# ---------------------------------------------------------------------------------------------------------------------

def _rng(seed, *keys):
    return np.random.default_rng([int(seed) & 0xffffffff, *[int(k) & 0xffffffff for k in keys]])


class SensorModel:
    def __init__(self, params: SensorParams | None = None, seed: int = 1, noise: bool = True):
        self.params = params or SensorParams.load()
        self.seed = seed
        self.noise = noise          # False: expected values only (no shot/read noise), for calibration of the signal path

    # ----------------------------------------------------------------------------------------------- exposure
    def exposure(self, ev):
        """Per-stream exposure at scene light level ``ev``: total EV relative to the recorded light level."""
        p = self.params
        pd = ev + min(max(-ev, 0.0), p.pd_ae_max_ev)
        preview = ev + min(max(-ev, 0.0), p.preview_ae_max_ev)
        return dict(ev=ev, pd_ev=pd, preview_ev=preview, pd_electrons_ref=p.pd_e_ref * 2.0 ** pd,
                    preview_mean_linear=min(1.0, p.preview_mean_linear * 2.0 ** preview),
                    preview_noise_lin=p.preview_noise_lin * 2.0 ** (p.preview_iso_fraction * (preview - ev)))

    # ----------------------------------------------------------------------------------------------- PD planes
    def adc(self, electrons, rng):
        """Electrons (expected) -> noisy quantized U14 DN: Poisson + Gaussian read noise, gain, black level, clip."""
        p = self.params
        lam = np.maximum(electrons, 0.0)
        if self.noise:
            collected = rng.poisson(lam).astype(np.float64) if lam.size else lam
            collected += rng.normal(0.0, p.pd_read_noise_e, size=lam.shape)
        else:
            collected = lam
        top = float((1 << p.adc_bits) - 1)
        dn = np.rint(p.black_level_dn + p.pd_gain_dn_per_e * collected)
        return np.clip(dn, 0.0, top).astype(np.uint16)

    def pd_planes(self, left_radiance, right_radiance, ev, frame_index=0, *, normalised_to_full_pupil=True):
        """Left/right U14 planes (same shape as the radiance arrays) for one RAW frame.

        ``*_radiance``: linear radiance of the two half-pupil views, reference 1.0 = scene mean at full pupil. When the views
        are normalised to full-pupil exposure (the renderer's convention) the physical half flux is applied here
        (``pd_sensitivity``)."""
        e_ref = self.exposure(ev)['pd_electrons_ref'] * (self.params.pd_sensitivity if normalised_to_full_pupil else 1.0)
        out = []
        for index, radiance in enumerate((left_radiance, right_radiance)):
            rng = _rng(self.seed, frame_index, 0x50440 + index)
            out.append(self.adc(np.asarray(radiance, dtype=np.float64) * e_ref, rng))
        return out[0], out[1]

    def pd_frame(self, region_left, region_right, ev, frame_index=0, *, frame=PD_FRAME, roi=PD_ROI, background=1.0,
                 normalised_to_full_pupil=True):
        """Full-frame planes ``(height, plane_width)`` with the ROI region filled and a flat background elsewhere.

        The stock extractor only reads the ROI; the rest of the delivered frame is a flat field at ``background``
        times the reference level (with the same noise model, so totals/saturation of other readers stay plausible)."""
        left, right = self.pd_planes(region_left, region_right, ev, frame_index, normalised_to_full_pupil=normalised_to_full_pupil)
        h, w = frame['height'], frame['plane_width']
        base = self.adc(np.full((1, w), background * self.exposure(ev)['pd_electrons_ref']
                                * (self.params.pd_sensitivity if normalised_to_full_pupil else 1.0)),
                        _rng(self.seed, frame_index, 0x50450))
        full_l = np.repeat(base, h, axis=0)
        full_r = full_l.copy()
        r0, r1, c0, c1 = roi['row0'], roi['row1'], roi['col0'], roi['col1']
        full_l[r0:r1, c0:c1] = left
        full_r[r0:r1, c0:c1] = right
        return full_l, full_r

    # ----------------------------------------------------------------------------------------------- preview
    def preview_yuv(self, radiance_rgb, ev, frame_index=0, *, metering_mean=None):
        """Packed YYCbCr (``PREVIEW_H`` x ``PREVIEW_W`` x 2 bytes) of a linear radiance image, ``(H, W)`` luma or ``(H, W, 3)`` RGB.

        Radiance reference: scene mean luminance = 1.0 (AE metering). The preview's exposure program lifts the signal up to
        ``preview_ae_max_ev`` as the light falls; noise: Poisson in electrons plus additive ``preview_noise_lin``."""
        # Temporal scenes hold this reference across lighting states; ordinary
        # scenes retain the existing per-frame preview AE declaration.
        p = self.params
        img = np.asarray(radiance_rgb, dtype=np.float32)
        gray = img.ndim == 2
        img = resize(img, PREVIEW_H, PREVIEW_W)
        mean = metering_mean if metering_mean is not None else (float(img.mean()) if img.size else 1.0)
        scale = self.exposure(ev)['preview_mean_linear'] / max(mean, 1e-12)
        lin = img * scale
        rng = _rng(self.seed, frame_index, 0x5052)
        if self.noise:
            electrons = np.maximum(lin, 0.0) * p.preview_e_full
            lin = rng.poisson(electrons).astype(np.float32) / p.preview_e_full
            lin = lin + rng.normal(0.0, self.exposure(ev)['preview_noise_lin'], size=lin.shape).astype(np.float32)
        nonlinear = np.clip(lin, 0.0, 1.0) ** (1.0 / p.preview_gamma)
        span = p.preview_white - p.preview_black
        if gray:
            y = np.rint(p.preview_black + span * nonlinear)
            cb = cr = np.full(y.shape, 128.0, dtype=np.float32)
        else:
            r, g, b = nonlinear[..., 0], nonlinear[..., 1], nonlinear[..., 2]
            yy = 0.299 * r + 0.587 * g + 0.114 * b
            y = np.rint(p.preview_black + span * yy)
            cb = np.rint(128 + 224 * (b - yy) / 1.772)
            cr = np.rint(128 + 224 * (r - yy) / 1.402)
        y = np.clip(y, 0, 255).astype(np.uint8)
        packed = np.empty((PREVIEW_H, PREVIEW_W // 2, 4), dtype=np.uint8)
        packed[:, :, 0], packed[:, :, 1] = y[:, 0::2], y[:, 1::2]
        packed[:, :, 2] = np.clip(0.5 * (cb[:, 0::2] + cb[:, 1::2]), 0, 255).astype(np.uint8)
        packed[:, :, 3] = np.clip(0.5 * (cr[:, 0::2] + cr[:, 1::2]), 0, 255).astype(np.uint8)
        return packed.tobytes()

    # ----------------------------------------------------------------------------------------------- statistics
    @staticmethod
    def luma(yuv_bytes, width=PREVIEW_W, height=PREVIEW_H):
        return np.frombuffer(yuv_bytes, dtype=np.uint8).reshape(height, width // 2, 4)[:, :, :2].reshape(height, width)

    def consumer_payloads(self, yuv_bytes):
        """Numpy form of ``afs_scene_fixture.consumer_payloads`` (declared law, byte-identical result; the original is a Python
        loop over every pixel): three luma histograms (256 U32), ten U64 horizontal absolute-gradient bands, four luma totals."""
        y = self.luma(yuv_bytes).astype(np.int64)
        hist = np.bincount(y.ravel(), minlength=256).astype('<u4').tobytes()
        grad = np.abs(y[:, 1:] - y[:, :-1]).sum(axis=1)
        bands = np.zeros(10, dtype=np.uint64)
        np.add.at(bands, np.minimum(9, np.arange(PREVIEW_H) * 10 // PREVIEW_H), grad.astype(np.uint64))
        total = int(y.sum())
        return {(0xfff18504, 0): bands.astype('<u8').tobytes(),
                **{(0xfff18508, offset): hist for offset in (0, 0x400, 0x800)},
                (0xfff18518, 0): np.array([total] * 4, dtype='<u4').tobytes(),
                (0xfff18520, 0x200): self.histogram_window(yuv_bytes)}

    def smoothed_fine_plane(self, yuv_bytes, columns, rows, width=PREVIEW_W, height=PREVIEW_H):
        """Contrast-grid plane 1 with the calibrated law: squared adjacent differences of the box-smoothed luma, per cell, /256.

        The declared plane-1 law (unsmoothed adjacent differences) is noise dominated in the dark, whereas the recorded plane 1
        correlates 0.99-1.00 (log, 85 positions) with a 15-pixel box-smoothed gradient in both the light and dark sweeps
        (research/af-sensor/README.md); ``contrast_smooth`` is that box width. Same byte layout as the declared planes."""
        import struct
        n = int(self.params.contrast_smooth)
        luma = self.luma(yuv_bytes, width, height).astype(np.float64)
        csum = np.cumsum(np.pad(luma, ((0, 0), (1, 0))), axis=1)
        smooth = (csum[:, n:] - csum[:, :-n]) / n
        width = smooth.shape[1]
        size = (rows - 1) * 64 + columns * 4
        plane = bytearray(size)
        for cy in range(rows):
            for cx in range(columns):
                x0, x1 = cx * width // columns, (cx + 1) * width // columns
                y0, y1 = cy * height // rows, (cy + 1) * height // rows
                cell = smooth[y0:y1, x0:x1]
                diff = cell[:, 1:] - cell[:, :-1]
                value = int(np.floor((diff * diff).sum(axis=1) / 256.0).sum())
                struct.pack_into('<I', plane, cy * 64 + cx * 4, min(value, 0xffffffff))
        return bytes(plane)

    def linear_planes(self, yuv_bytes, columns, rows, *, width=PREVIEW_W, height=PREVIEW_H, plane=1):
        """Contrast-grid plane on gamma-expanded luma (``contrast_gamma`` > 0), same byte layout as the legacy planes.

        Calibrated on the recorded sweeps (light, daylight, dark 1 and 2; research/af-sensor/README.md section 6): the recorded cells
        follow the *linear* gradient energy of the live view, independent of the exposure program, which the gamma-coded luma
        laws (error 1.6x plane 1 and 5-8x plane 0 between light and dark) did not.

        plane 1: squared adjacent differences of the ``contrast_smooth``-pixel box of the expanded luma, per cell, times
        ``contrast_scale[1]``. plane 0: absolute adjacent differences of the ``contrast_sigma`` Gaussian of the expanded luma
        in the ``contrast_window0`` window, minus the coring threshold ``contrast_core`` (negative parts dropped), times
        ``contrast_scale[0]``."""
        import struct
        p = self.params
        lin = expand_luma(self.luma(yuv_bytes, width, height), p.contrast_gamma)
        if plane == 1:
            smooth = box_x(lin, max(1, int(p.contrast_smooth)))
            energy = lambda diff: (diff * diff)
            scale = p.contrast_scale[1]
        else:
            smooth = gauss_x(lin, p.contrast_sigma)
            energy = lambda diff: np.maximum(np.abs(diff) - p.contrast_core, 0.0)
            scale = p.contrast_scale[0]
        width_s = smooth.shape[1]
        size = (rows - 1) * 64 + columns * 4
        out = bytearray(size)
        for cy in range(rows):
            for cx in range(columns):
                x0, x1 = cx * width_s // columns, (cx + 1) * width_s // columns
                y0, y1 = cy * height // rows, (cy + 1) * height // rows
                cell = smooth[y0:y1, x0:x1]
                value = float(energy(cell[:, 1:] - cell[:, :-1]).sum()) * scale
                struct.pack_into('<I', out, cy * 64 + cx * 4, int(min(value, 0xffffffff)))
        return bytes(out)

    def histogram_window(self, yuv_bytes):
        """The 0x25B0 histogram (256 U32, calibrated law): ``Y >> 1`` of the statistics raster, ``stat_hist_total`` counts.

        Law (calibrated by quantile matching against the recorded light-sweep banks, see research/af-sensor/README.md):
        bin = Y / 2 on a 1200x800 raster (the preview resampled), so the total is 960,000."""
        y = self.luma(yuv_bytes).astype(np.float32)
        stat = resize(y, STAT_H, STAT_W)
        bins = np.clip(np.rint(stat * 0.5 - 0.0).astype(np.int64), 0, 255)
        return np.bincount(bins.ravel(), minlength=256).astype('<u4').tobytes()

    @staticmethod
    def crop_yuv(yuv_bytes, width, height):
        """Centred ``width`` x ``height`` window (even width) of a packed preview as packed YYCbCr bytes."""
        width -= width % 2
        packed = np.frombuffer(yuv_bytes, dtype=np.uint8).reshape(PREVIEW_H, PREVIEW_W // 2, 4)
        x0, y0 = (PREVIEW_W - width) // 2 // 2, (PREVIEW_H - height) // 2
        return packed[y0:y0 + height, x0:x0 + width // 2].tobytes()

    @staticmethod
    def _scale_words(data, scale):
        if scale == 1.0:
            return data
        words = np.frombuffer(data, dtype='<u4').astype(np.float64) * scale
        return np.minimum(words, 0xffffffff).astype('<u4').tobytes()

    def statistics_spans(self, yuv_bytes, *, template, columns=1, rows=1, af=None):
        """Bank spans ``(register, offset, bytes)`` for one preview frame.

        ``template``: the recorded idle bank spans for windows with no declared law. Replaced by scene laws: three luma
        histogram planes, ten gradient bands and four luma totals (``afs_scene_fixture.consumer_payloads``, declared), the
        two contrast-grid planes at the programmed grid (``contrast_grid``, declared; with ``af``, the contrast-AF consumer
        windows via ``afs_focus_scene``, declared) and the 0x25B0 histogram (calibrated). The contrast grids look at a
        centred ``contrast_window`` of the preview (calibrated: the recorded cells track a 240x160 window, not the whole
        frame) and plane 1 uses the ``contrast_smooth`` box (calibrated), scaled by ``contrast_scale`` (calibrated at the light
        level; the recorded scale differs by exposure, see README)."""
        from emulator import afs_scene_fixture, afs_focus_scene
        from emulator.hardware import UnsupportedDevice
        p = self.params
        modeled = self.consumer_payloads(yuv_bytes)
        cw, ch = (int(p.contrast_window[0]), int(p.contrast_window[1])) if p.contrast_window else (PREVIEW_W, PREVIEW_H)
        crop = yuv_bytes if (cw, ch) == (PREVIEW_W, PREVIEW_H) else self.crop_yuv(yuv_bytes, cw, ch)
        stride = cw * 2
        info, grid_spans = {}, None
        if af is not None:
            try:
                result = afs_focus_scene.prepare(
                    crop, width=cw, height=ch, stride_bytes=stride, focus_position=0, target_position=0,
                    dimensions=af['dimensions'], grid=af['grid'], windows=af['windows'], band_count=af['band_count'],
                    band_scale=af['band_scale'], band_orientation=af['band_orientation'])
                grid_spans = [(0xfff18500, off, data) for off, data in result['spans']]
                info = dict(coarse=result['contrast']['coarse'], fine=result['contrast']['fine'], band=result['band_raw'])
                columns, rows = af['dimensions'][0], af['dimensions'][1]
            except UnsupportedDevice:
                grid_spans = None
        if grid_spans is None:
            grid = afs_scene_fixture.contrast_grid(crop, width=cw, height=ch, stride_bytes=stride, columns=columns, rows=rows)
            grid_spans = [(0xfff18500, off, data) for off, data in grid['bank_spans']]
            info = dict(coarse=grid['coarse'], fine=grid['fine'])
        out = []
        for r, o, d in grid_spans:
            if r == 0xfff18500 and o == 0x800 and p.contrast_gamma > 0:
                # The contiguous AF band must use the same luma/filter law
                # as the fine grid. The legacy gamma-coded adjacent-gradient
                # band otherwise survives even when both grids are replaced.
                # This projection is declared; the recorded band has not
                # been calibrated. One cell includes grid-boundary gradients.
                import struct
                expanded = expand_luma(self.luma(crop, cw, ch), p.contrast_gamma)
                smooth = box_x(expanded, max(1, int(p.contrast_smooth)))
                value = int(np.minimum((np.diff(smooth, axis=1) ** 2).sum() *
                                       p.contrast_scale[1], 0xffffffffffffffff))
                out.append((r, o, struct.pack('<Q', value)))
                info['band'] = value
                continue
            if r == 0xfff18500 and o in (0, 0x400) and p.contrast_gamma > 0:
                if o == 0:
                    w0, h0 = int(p.contrast_window0[0]), int(p.contrast_window0[1])
                    crop0 = self.crop_yuv(yuv_bytes, w0, h0)
                    d = self.linear_planes(crop0, columns, rows, width=w0, height=h0, plane=0)
                else:
                    d = self.linear_planes(crop, columns, rows, width=cw, height=ch, plane=1)
                import struct
                info['coarse' if o == 0 else 'fine'] = [
                    [struct.unpack_from('<I', d, y * 64 + x * 4)[0] for x in range(columns)]
                    for y in range(rows)]
                out.append((r, o, d))
                continue
            if r == 0xfff18500 and o == 0x400 and p.contrast_smooth:
                d = self.smoothed_fine_plane(crop, columns, rows, width=cw, height=ch)
            if r == 0xfff18500 and o in (0, 0x400):
                d = self._scale_words(d, p.contrast_scale[0 if o == 0 else 1])
            out.append((r, o, d))
        if af is not None and info.get('band') is not None:
            # the programmed contrast-AF windows (band + auxiliary windows) overlay the other registers' windows in the bank:
            # like SyntheticSceneSource, only these spans are served while the contrast-AF consumer is configured
            return out, info
        spans = [(r, o, modeled.get((r, o), d)) for r, o, d in template if r != 0xfff18500]
        return spans + out, info

    def describe(self):
        return dict(parameters=self.params.describe(), seed=self.seed)
