"""Declared ROI-only mono12 phase gain, not recovered F1C filter math."""
import hashlib
import struct

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

PROFILE = 'declared-f1c-mode3-roi-phase-q8-nearest-gain-unmeasured'
FIELDS = {'address', 'capacity', 'stride_samples', 'x0', 'y0', 'x1', 'y1',
          'center_x', 'center_y', 'packing'}
CPU_BORDER_PROFILE = 'recovered-u16-border-clear-1168694'


def prepare_cpu_border(source, *, descriptor, border):
    """Predict the recovered116BE88→1168694(source,0,0,border,border).

    This is CPU code, not an additional simulated DMA. The admitted subset is
    the observed unpacked U16 descriptor; packed12 and byte formats refuse.
    """
    d = tuple(descriptor)
    if (type(source) is not bytes or len(d) != 7 or any(type(v) is not int for v in d)
            or type(border) is not int or not 1 <= border <= 64
            or d[0] != 0 or d[6] != 1 or d[1] & 7 or not 0 < d[3] <= 16384
            or d[3] & 7 or d[4] & 7):
        raise ValueError('CPU border clear requires the bounded unpacked U16 caller')
    physical = d[1] & 0x3fffffff
    if (not 0 < d[5] <= len(source) // d[3] or not 0 < d[4] <= d[3]
            or d[2] != d[3] * d[5] or d[2] > len(source)
            or len(source) % d[3] or physical + len(source) > 0x20000000
            or 4 * border > d[4]):
        raise ValueError('CPU border clear leaves its independently owned pitched span')
    output = bytearray(source)
    width = 2 * border
    for y in range(d[5]):
        at = y * d[3]
        output[at:at + width] = bytes(width)
        output[at + d[4] - width:at + d[4]] = bytes(width)
    output = bytes(output)
    return dict(output=output, identity=dict(profile=CPU_BORDER_PROFILE,
        descriptor=d, arguments=(0, 0, border, border),
        input_sha256=hashlib.sha256(source).hexdigest(),
        output_sha256=hashlib.sha256(output).hexdigest(),
        zeroed_bytes=4 * border * d[5],
        limits='Recovered U16 left/right CPU zeroing; padding and unused rows retained'))


def prepare(source, *, source_metadata, gain_map, black_levels, profile, backend='auto'):
    """Return immutable output/identity; preserve outside ROI and row padding.

    Four CPU row-major8x8 gain planes are nearest sampled across the ROI's
    relative2x2 phases. Positive post-black signal is multiplied by Q8 gain,
    rounded down and clipped to mono12. Samples at/below black are retained.
    Coefficients, filter controls and hardware map-coordinate meanings remain
    outside this explicitly selected simulation law.
    """
    m = source_metadata
    if (profile != PROFILE or backend not in ('auto', 'scalar', 'numpy')
            or backend == 'numpy' and _numpy is None
            or type(source) is not bytes or type(m) is not dict or set(m) != FIELDS
            or m['packing'] != 'u16le-mono12' or type(gain_map) is not bytes or len(gain_map) != 512
            or type(black_levels) not in (tuple, list) or len(black_levels) != 4
            or any(type(v) is not int or not 0 <= v <= 4095 for v in black_levels)):
        raise ValueError('F1C gain requires explicit profile/backend and exact mono12 mesh/ownership')
    for name in FIELDS - {'packing'}:
        if type(m[name]) is not int:
            raise ValueError('F1C gain metadata must contain exact integer fields')
    stride, capacity = m['stride_samples'], m['capacity']
    physical = m['address'] - 0x40000000 if m['address'] >= 0x40000000 else m['address']
    if (not 12 <= stride <= 8192 or not 0 < capacity <= 64 * 1024 * 1024
            or len(source) != capacity or capacity % (2 * stride) or m['address'] & 1
            or not 0 <= physical < physical + capacity <= 0x20000000):
        raise ValueError('F1C gain source exceeds its independently owned pitched allocation')
    height = capacity // (2 * stride)
    x0, y0, x1, y1 = (m[k] for k in ('x0', 'y0', 'x1', 'y1'))
    if (not 0 <= x0 < x0 + 7 <= x1 < stride or not 0 <= y0 < y0 + 7 <= y1 < height
            or not x0 <= m['center_x'] <= x1 or not y0 <= m['center_y'] <= y1):
        raise ValueError('F1C gain requires a bounded ROI large enough for its8x8 mesh')
    gains = struct.unpack('<256H', gain_map)
    if any(v > 2047 for v in gains):
        raise ValueError('F1C gain map leaves the recovered U11 domain')
    output = bytearray(source)
    changed = 0
    width, rows = x1 - x0 + 1, y1 - y0 + 1
    vector = backend == 'numpy' or backend == 'auto' and _numpy is not None
    if vector:
        image = _numpy.frombuffer(source, dtype='<u2').reshape(height, stride)
        if int(image.max()) > 4095:
            raise ValueError('F1C gain source leaves mono12')
        result = _numpy.frombuffer(output, dtype='<u2').reshape(height, stride)
        mesh = _numpy.array(gains, dtype=_numpy.int32).reshape(4, 8, 8)
        for phase in range(4):
            dx, dy = phase & 1, phase >> 1
            xs = _numpy.arange(x0 + dx, x1 + 1, 2)
            ys = _numpy.arange(y0 + dy, y1 + 1, 2)
            values = image[_numpy.ix_(ys, xs)].astype(_numpy.int32)
            table = mesh[phase][_numpy.ix_((ys - y0) * 8 // rows, (xs - x0) * 8 // width)]
            black = black_levels[phase]
            transformed = _numpy.where(values > black,
                _numpy.minimum(4095, black + (((values - black) * table) >> 8)), values)
            changed += int(_numpy.count_nonzero(transformed != values))
            result[_numpy.ix_(ys, xs)] = transformed
    else:
        if any(v[0] > 4095 for v in struct.iter_unpack('<H', source)):
            raise ValueError('F1C gain source leaves mono12')
        for y in range(y0, y1 + 1):
            gy = (y - y0) * 8 // rows
            for x in range(x0, x1 + 1):
                phase = ((y - y0) & 1) * 2 + ((x - x0) & 1)
                at = 2 * (y * stride + x)
                value = struct.unpack_from('<H', source, at)[0]
                black = black_levels[phase]
                gain = gains[phase * 64 + gy * 8 + (x - x0) * 8 // width]
                transformed = min(4095, black + ((value - black) * gain >> 8)) if value > black else value
                changed += int(transformed != value)
                struct.pack_into('<H', output, at, transformed)
    output = bytes(output)
    identity = dict(profile=PROFILE, source_metadata=dict(m),
        input_sha256=hashlib.sha256(source).hexdigest(), output_sha256=hashlib.sha256(output).hexdigest(),
        gain_sha256=hashlib.sha256(gain_map).hexdigest(), black_levels=tuple(black_levels),
        gain_range=(min(gains), max(gains)), changed_samples=changed,
        phase_origin=(x0, y0), interpolation='Declared floor-nearest ROI8x8 mesh',
        signal='Preserve at/below-black samples; floor Q8 gain on positive signal; clip12bit',
        physical_filter_math='Unqualified; coefficients and hardware coordinates are not interpreted')
    return dict(output=output, identity=identity)
