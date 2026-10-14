"""Pure, unarmed monochrome RAW-to-YYCbCr experiment.

The companion still RAW scene law writes luma times16, in U16LE or the
stock scalar packed12 byte contract. This law divides samples by16, clips
to8-bit, applies an explicit nearest-neighbor crop/resize, and returns two
luma bytes followed by neutral Cb/Cr. It is not a recovered CFA, demosaic,
filter, color transform, ISP/JPEG packing or hardware completion law.
Only active row spans are returned: no RAM, padding, status or IRQ is changed.
"""
from __future__ import annotations

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

from .hardware import UnsupportedDevice


PROFILE = 'declared-monochrome-raw-div16-nearest-yycbcr-unmeasured'
MAX_PIXELS, MAX_BYTES, RAM_BYTES = 32 * 1024 * 1024, 64 * 1024 * 1024, 0x20000000


def _integer(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise UnsupportedDevice('Still processing scene: invalid ' + name)
    return value


def _physical(address, length, name):
    _integer(address, 0, 0x5fffffff, name + ' address')
    physical = address - 0x40000000 if address >= 0x40000000 else address
    if physical >= RAM_BYTES or physical + length > RAM_BYTES:
        raise UnsupportedDevice('Still processing scene: ' + name + ' span exceeds DRAM')
    return physical


def geometry_plan(*, source_address, width, height, stride_bytes, packed,
                  destination_address, output_width, output_height,
                  output_stride_bytes, output_capacity, crop=None, profile=None):
    """Validate declared physical spans and return an unarmed geometry plan.

    Addresses may use the40000000 DRAM alias; overlap is checked physically.
    Pitches are bytes. The caller must obtain the real buffer/geometry first;
    this helper does not select an engine mode or infer an MMIO encoding.
    """
    if type(profile) is not str or profile != PROFILE:
        raise UnsupportedDevice('Still processing scene requires the explicit monochrome profile')
    if type(packed) is not bool:
        raise UnsupportedDevice('Still processing scene requires explicit packed12 or U16LE input')
    for value, name in ((width, 'width'), (height, 'height'),
                        (output_width, 'output width'), (output_height, 'output height')):
        _integer(value, 2 if 'width' in name else 1, 8192, name)
    if width & 1 or output_width & 1 or max(width * height, output_width * output_height) > MAX_PIXELS:
        raise UnsupportedDevice('Still processing scene: bounded even-width rasters required')
    active_bytes = width * (3 if packed else 4) // 2
    _integer(stride_bytes, active_bytes, 32768, 'source stride')
    _integer(output_stride_bytes, output_width * 2, 32768, 'output stride')
    _integer(output_capacity, 1, MAX_BYTES, 'output capacity')
    source_bytes, destination_bytes = stride_bytes * height, output_stride_bytes * output_height
    if source_bytes > MAX_BYTES or destination_bytes > output_capacity:
        raise UnsupportedDevice('Still processing scene: source bound or output capacity exceeded')
    source = _physical(source_address, source_bytes, 'source')
    destination = _physical(destination_address, output_capacity, 'destination')
    if source < destination + output_capacity and destination < source + source_bytes:
        raise UnsupportedDevice('Still processing scene: aliased source and destination overlap')
    if crop is None:
        crop = (0, 0, width, height)
    if type(crop) not in (tuple, list) or len(crop) != 4:
        raise UnsupportedDevice('Still processing scene: explicit four-field crop required')
    x, y, cw, ch = crop
    _integer(x, 0, width - 1, 'crop X'); _integer(y, 0, height - 1, 'crop Y')
    _integer(cw, 1, width - x, 'crop width'); _integer(ch, 1, height - y, 'crop height')
    return dict(profile=profile, state='prepared-unarmed',
        source=dict(address=source, width=width, height=height, stride_bytes=stride_bytes,
                    span_bytes=source_bytes, active_row_bytes=active_bytes, packed=packed),
        output=dict(address=destination, width=output_width, height=output_height,
                    stride_bytes=output_stride_bytes, capacity=output_capacity,
                    span_bytes=destination_bytes, active_row_bytes=output_width * 2),
        crop=(x, y, cw, ch),
        assumptions='Declared RAW luma times16 inverse; floor nearest crop/resize; row-major YYCbCr with neutral128 chroma; no CFA/color/filter calibration, packing identification or hardware effects')


def render_rows(source, *, numpy=True, **geometry):
    """Return active (physical address, bytes) rows; leave row padding intact."""
    plan = geometry_plan(**geometry)
    src, out = plan['source'], plan['output']
    if type(source) is not bytes or len(source) != src['span_bytes'] or type(numpy) is not bool:
        raise UnsupportedDevice('Still processing scene: complete immutable source bytes required')
    x, y, cw, ch = plan['crop']
    xs = [x + column * cw // out['width'] for column in range(out['width'])]
    ys = [y + row * ch // out['height'] for row in range(out['height'])]
    rows = []
    if numpy and _numpy is not None:
        raster = _numpy.frombuffer(source, dtype=_numpy.uint8).reshape(src['height'], src['stride_bytes'])
        active = raster[:, :src['active_row_bytes']]
        if src['packed']:
            pairs = active.reshape(src['height'], src['width'] // 2, 3).astype(_numpy.uint16)
            samples = _numpy.empty((src['height'], src['width']), dtype=_numpy.uint16)
            samples[:, 0::2] = pairs[:, :, 0] | (pairs[:, :, 1] & 15) << 8
            samples[:, 1::2] = pairs[:, :, 1] >> 4 | pairs[:, :, 2] << 4
        else:
            pairs = active.reshape(src['height'], src['width'], 2).astype(_numpy.uint16)
            samples = pairs[:, :, 0] | pairs[:, :, 1] << 8
        for row, sy in enumerate(ys):
            packed_row = _numpy.full((out['width'] // 2, 4), 128, dtype=_numpy.uint8)
            packed_row[:, :2] = _numpy.minimum(samples[sy, xs] >> 4, 255).reshape(-1, 2)
            rows.append((out['address'] + row * out['stride_bytes'], packed_row.tobytes()))
    else:
        for row, sy in enumerate(ys):
            luma = []
            for sx in xs:
                offset = sy * src['stride_bytes'] + sx * (3 if src['packed'] else 4) // 2
                word = source[offset] | source[offset + 1] << 8
                sample = (word >> 4 if sx & 1 else word & 0xfff) if src['packed'] else word
                luma.append(min(sample >> 4, 255))
            packed_row = bytes(value for column in range(0, out['width'], 2)
                               for value in (luma[column], luma[column + 1], 128, 128))
            rows.append((out['address'] + row * out['stride_bytes'], packed_row))
    return dict(**plan, rows=tuple(rows))
