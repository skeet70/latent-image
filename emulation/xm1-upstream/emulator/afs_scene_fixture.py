"""Declared paired-aperture image and raw PDAF arithmetic, without guest effects.

The X70 raw extractor exposes a 41-word comparison curve, two U14 scalars
and two U32 totals. Their physical computation is not recovered. This module
defines one experiment: little-endian U16 intensities, SAD comparison, and
eight declared lens pulses per image-sample disparity. It neither decides
focus nor writes camera flags. The stock PAF software must judge the curve.
"""
from __future__ import annotations

import hashlib
import struct

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

from .hardware import UnsupportedDevice


PROFILE = 'declared-pdaf-paired-u16-sad-unmeasured'
PROFILES = (PROFILE,)
MAX_SAMPLES = 65536
MAX_RADIUS = 20
MAX_INTENSITY = 0x3fff
CONSUMER_PROFILE = 'declared-frozen-yycbcr-luma-consumer-statistics-unmeasured'
CONTRAST_PROFILE = 'declared-yycbcr-coarse-fine-horizontal-gradient-grid-unmeasured'
PACKED_GRID_PROFILE = 'declared-opaque-high-low-u16-grid-contract'
GRID_ROW_BYTES = 0x40
CONTRAST_PLANE_OFFSETS = (0, 0x400)
PACKED_GRID_OFFSET = 0x1990


def _luma_array(image, width, height, stride_bytes):
    """A signed copy avoids U8 wrapping; padding/chroma never enter energy."""
    packed = _numpy.frombuffer(image, dtype=_numpy.uint8).reshape(height, stride_bytes)
    pairs = packed[:, :width * 2].reshape(height, width // 2, 4)
    return pairs[:, :, :2].reshape(height, width).astype(_numpy.int32)


def _contrast_cell(image, width, stride_bytes, x0, x1, y0, y1, luma=None):
    if luma is not None:
        cell = luma[y0:y1, x0:x1]
        fine = int(_numpy.abs(cell[:, 1:] - cell[:, :-1]).sum(dtype=_numpy.uint64))
        # Adjacent three-sample box sums differ by v[x+3]-v[x].
        # Keep the scalar law's integer division once per row.
        coarse = int((_numpy.abs(cell[:, 3:] - cell[:, :-3]).sum(
            axis=1, dtype=_numpy.uint64) // 3).sum(dtype=_numpy.uint64))
        return coarse, fine
    coarse = fine = 0
    for y in range(y0, y1):
        raster = image[y * stride_bytes:(y + 1) * stride_bytes]
        values = [raster[(x // 2) * 4 + (x & 1)] for x in range(x0, x1)]
        fine += sum(abs(b - a) for a, b in zip(values, values[1:]))
        coarse += sum(abs(values[x + 3] - values[x]) for x in range(len(values) - 3)) // 3
    return coarse, fine


def _integer(value, lo, hi, name):
    if type(value) is not int or not lo <= value <= hi:
        raise UnsupportedDevice('Declared AF scene: invalid ' + name)
    return value


def paired_raster(*, width, height, focus_position, target_position,
                  pulses_per_sample=8, flat=False):
    """Return two immutable U16 rasters from one deterministic textured target.

    Lens positions are external/model inputs. A positive target-minus-focus
    shifts the right-aperture texture to the right. Box blur increases with
    absolute defocus. This parallax, blur and texture are explicit test laws,
    not X70 optical calibration or a recovered sensor-pixel layout.
    """
    _integer(width, 48, 8192, 'width')
    _integer(height, 1, 8192, 'height')
    if width * height > MAX_SAMPLES:
        raise UnsupportedDevice('Declared AF scene: raster exceeds sample bound')
    _integer(focus_position, -32768, 32767, 'focus position')
    _integer(target_position, -32768, 32767, 'target position')
    _integer(pulses_per_sample, 1, 4096, 'parallax scale')
    if type(flat) is not bool:
        raise UnsupportedDevice('Declared AF scene: flat selector must be boolean')
    defocus = target_position - focus_position
    disparity = round(defocus / pulses_per_sample)
    if abs(disparity) > MAX_RADIUS:
        raise UnsupportedDevice('Declared AF scene: disparity exceeds comparison radius')
    blur = min(3, abs(disparity) // 4)

    def texture(x, y):
        if flat:
            return 8192
        # A nonperiodic deterministic edge target. Hashing signed positions
        # also supplies consistent guard pixels outside the visible raster.
        word = int.from_bytes(hashlib.sha256(struct.pack('<ii', x // 4, y // 4)).digest()[:2], 'little')
        return 2048 + (word % 12288)

    def sample(x, y):
        return sum(texture(x + delta, y) for delta in range(-blur, blur + 1)) // (2 * blur + 1)

    left, right = [], []
    for y in range(height):
        for x in range(width):
            left.append(sample(x, y))
            right.append(sample(x - disparity, y))
    return dict(profile=PROFILE, width=width, height=height,
                focus_position=focus_position, target_position=target_position,
                pulses_per_sample=pulses_per_sample, disparity=disparity,
                blur_radius=blur, flat=flat,
                left=struct.pack('<' + 'H' * len(left), *left),
                right=struct.pack('<' + 'H' * len(right), *right))


def raw_statistics(left_rows, right_rows, *, radius):
    """Compute raw comparison data; all shifts compare the same sample count.

    Both inputs have equal rectangular U14 sample values. A fixed guard of
    ``radius`` samples on each side avoids an edge-overlap bias. The two
    scalar values are intensity ranges and the totals are sample sums.
    Those meanings, and SAD rather than another distance, are declared.
    """
    _integer(radius, 0, MAX_RADIUS, 'comparison radius')
    if not isinstance(left_rows, (tuple, list)) or not isinstance(right_rows, (tuple, list)):
        raise UnsupportedDevice('Declared AF scene: row arrays required')
    if not left_rows or len(left_rows) != len(right_rows):
        raise UnsupportedDevice('Declared AF scene: unequal/empty row arrays')
    width = len(left_rows[0])
    if width <= 2 * radius or width * len(left_rows) > MAX_SAMPLES:
        raise UnsupportedDevice('Declared AF scene: insufficient guard or sample bound')
    left, right = [], []
    for a, b in zip(left_rows, right_rows):
        if not isinstance(a, (tuple, list)) or not isinstance(b, (tuple, list)) or len(a) != width or len(b) != width:
            raise UnsupportedDevice('Declared AF scene: nonrectangular samples')
        left.append(tuple(_integer(v, 0, MAX_INTENSITY, 'U14 intensity') for v in a))
        right.append(tuple(_integer(v, 0, MAX_INTENSITY, 'U14 intensity') for v in b))
    curves = []
    for shift in range(-radius, radius + 1):
        sad = sum(abs(a[x] - b[x + shift]) for a, b in zip(left, right)
                  for x in range(radius, width - radius))
        _integer(sad, 0, 0xffffffff, 'U32 comparison sum')
        curves.append(sad)
    all_left = [v for row in left for v in row]
    all_right = [v for row in right for v in row]
    return dict(radius=radius, curve=curves,
                scalars=[max(all_left) - min(all_left), max(all_right) - min(all_right)],
                totals=[sum(all_left), sum(all_right)],
                samples=width * len(left), comparison_samples=(width - 2 * radius) * len(left))


def consumer_payloads(image, *, width, height, stride_bytes):
    """Project one frozen YYCbCr scene into the known consumer byte windows.

    The three U32 histogram planes use neutral luma, the ten U64 windows
    use horizontal absolute gradients per image band, and the four U32
    totals use luma sums. These computations/scales are declared, not the
    recovered ISP law. Other windows explicitly contain zero experiment
    inputs; neither record semantics nor silicon transfer sizes are inferred.
    """
    from .recorded_statistics import SPAN_LAYOUT
    _integer(width, 2, 8192, 'YYCbCr width')
    _integer(height, 1, 8192, 'YYCbCr height')
    _integer(stride_bytes, 4, 32768, 'YYCbCr stride')
    if (width & 1 or stride_bytes < width * 2 or width * height > 1048576
            or type(image) is not bytes or len(image) != stride_bytes * height):
        raise UnsupportedDevice('Declared AF scene: invalid bounded YYCbCr raster')
    histogram = [0] * 256
    contrast = [0] * 10
    total = 0
    for y in range(height):
        row = image[y * stride_bytes:y * stride_bytes + width * 2]
        luma = [row[(x // 2) * 4 + (x & 1)] for x in range(width)]
        # Packed pairs are Y0,Y1,Cb,Cr; pair-relative luma offsets0/1.
        for value in luma:
            histogram[value] += 1
            total += value
        contrast[min(9, y * 10 // height)] += sum(abs(a - b) for a, b in zip(luma, luma[1:]))
    hist_bytes = struct.pack('<256I', *histogram)
    values = {(0xfff18504, 0): struct.pack('<10Q', *contrast),
              **{(0xfff18508, offset): hist_bytes for offset in (0, 0x400, 0x800)},
              (0xfff18518, 0): struct.pack('<4I', *([total] * 4))}
    spans = [(register, offset, values.get((register, offset), bytes(size)))
             for register, offset, _capture, size, _scope in SPAN_LAYOUT]
    return dict(profile=CONSUMER_PROFILE, spans=spans, pixels=width * height,
                image_sha256=hashlib.sha256(image).hexdigest(),
                luma_sum=total, histogram_samples=sum(histogram), contrast_bands=contrast,
                known_numeric_windows='3x256U32 histograms,10U64 windows,4U32 totals',
                assumptions='Frozen displayed YYCbCr scene; neutral luma histogram planes, absolute-gradient bands and luma sums; remaining consumer windows explicitly zero; no ISP calibration or AF decision')


def contrast_grid(image, *, width, height, stride_bytes, columns=1, rows=1):
    """Prepare the two U32 grids copied by stock 12F6310, without guest effects.

    The source row pitch (16 U32), two plane offsets and maximum 8x8
    consumer dimensions are recovered. The optical/ISP filters, gain,
    physical integration windows and their relation to sensor pixels are
    not. This explicit experiment divides the supplied luma raster into
    equal fractional cells. Plane0 sums absolute horizontal gradients of a
    three-sample box average; plane1 uses unsmoothed gradients. Comparisons
    stay inside each cell and no artificial border is introduced.

    Stock 12B7CE8 reads these into result words0/1. The subsequent CAF
    recorder calls their sum VAL_LO and word1 VAL_HI. The model supplies
    energies only, never a focus classification or a lens position.
    Returned spans describe consumer input bytes, not silicon DMA lengths.
    They are deliberately separate from the live zero-window experiment.
    """
    _integer(width, 2, 8192, 'YYCbCr width')
    _integer(height, 1, 8192, 'YYCbCr height')
    _integer(stride_bytes, 4, 32768, 'YYCbCr stride')
    _integer(columns, 1, 8, 'contrast columns')
    _integer(rows, 1, 8, 'contrast rows')
    if (width & 1 or stride_bytes < width * 2 or width * height > 1048576
            or type(image) is not bytes or len(image) != stride_bytes * height
            or width < columns * 4 or height < rows):
        raise UnsupportedDevice('Declared AF scene: invalid bounded contrast raster/grid')
    size = (rows - 1) * GRID_ROW_BYTES + columns * 4
    planes = [bytearray(size), bytearray(size)]
    energies = [[[] for _ in range(rows)] for _ in range(2)]
    luma = _luma_array(image, width, height, stride_bytes) if _numpy is not None else None
    for cy in range(rows):
        for cx in range(columns):
            x0, x1 = cx * width // columns, (cx + 1) * width // columns
            y0, y1 = cy * height // rows, (cy + 1) * height // rows
            coarse, fine = _contrast_cell(image, width, stride_bytes, x0, x1, y0, y1, luma)
            for index, value in enumerate((coarse, fine)):
                _integer(value, 0, 0xffffffff, 'U32 contrast energy')
                struct.pack_into('<I', planes[index], cy * GRID_ROW_BYTES + cx * 4, value)
                energies[index][cy].append(value)
    return dict(profile=CONTRAST_PROFILE, dimensions=bytes((columns, rows)),
                bank_spans=[(offset, bytes(raw)) for offset, raw in zip(CONTRAST_PLANE_OFFSETS, planes)],
                coarse=energies[0], fine=energies[1], pixels=width * height,
                image_sha256=hashlib.sha256(image).hexdigest(),
                assumptions='Declared equal fractional cells and three-sample/unsmoothed horizontal absolute gradients; no recovered ISP filter, sensor crop, gain, physical timing or focus decision')


def packed_u16_grid(high_rows, low_rows):
    """Encode the opaque +1990 grid's proven high16/low16 consumer contract.

    12F65A8 splits each U32 into two duplicated high16 and two duplicated
    low16 planes. 12B7DB0 consumes their signed difference. This function
    assigns no physical channel meaning and derives no values from a scene.
    """
    if (not isinstance(high_rows, (tuple, list)) or not isinstance(low_rows, (tuple, list))
            or not high_rows or len(high_rows) != len(low_rows)):
        raise UnsupportedDevice('Declared AF scene: unequal/empty packed grid rows')
    rows = _integer(len(high_rows), 1, 8, 'packed rows')
    if not isinstance(high_rows[0], (tuple, list)):
        raise UnsupportedDevice('Declared AF scene: packed row arrays required')
    columns = _integer(len(high_rows[0]), 1, 8, 'packed columns')
    raw = bytearray((rows - 1) * GRID_ROW_BYTES + columns * 4)
    for y, (high, low) in enumerate(zip(high_rows, low_rows)):
        if (not isinstance(high, (tuple, list)) or not isinstance(low, (tuple, list))
                or len(high) != columns or len(low) != columns):
            raise UnsupportedDevice('Declared AF scene: nonrectangular packed grid')
        for x, (hi, lo) in enumerate(zip(high, low)):
            hi = _integer(hi, 0, 0xffff, 'packed high16')
            lo = _integer(lo, 0, 0xffff, 'packed low16')
            struct.pack_into('<I', raw, y * GRID_ROW_BYTES + x * 4, (hi << 16) | lo)
    return dict(profile=PACKED_GRID_PROFILE, dimensions=bytes((columns, rows)),
                bank_spans=[(PACKED_GRID_OFFSET, bytes(raw))],
                assumptions='Only the recovered high16/low16 byte packing; physical channel meaning unknown')
