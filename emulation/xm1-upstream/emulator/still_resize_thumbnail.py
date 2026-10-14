"""Declared step2 endpoint sampling for the observed S2 mode1 thumbnail.

The genuine request has 213 visible rows within 216 programmed rows. Keep
the last three rows untouched. This is a named monochrome scene experiment,
not a recovered silicon filter or decimation calibration.
"""
import struct

from .hardware import UnsupportedDevice


PROFILE = 'declared-still-mode1-thumbnail-step2-endpoint-luma-unmeasured'


def plan(descriptor, capacity):
    from .still_resize_devices import _physical, MAX_BYTES
    d = struct.unpack('<20I', descriptor)
    if (d[:4] != (0, 1, 4096, 4096) or d[5:10] != (656, 640, 426, 638, 424)
            or d[11:19] != (320, 0, 0, 320, 216, 319, 212, 0)
            or d[19] != 0x118060c or type(capacity) is not int
            or not 640*216 <= capacity <= MAX_BYTES or (d[4] | d[10]) & 15):
        raise UnsupportedDevice('Still mode1 thumbnail requires the observed step2/320x213-in-216 request')
    source_bytes, output_bytes = 1312*426, 640*216
    source, destination = _physical(d[4], source_bytes), _physical(d[10], output_bytes)
    if source < destination+output_bytes and destination < source+source_bytes:
        raise UnsupportedDevice('Still mode1 thumbnail source/output physically overlap')
    expected = {4: 0, 0x10: source, 0x14: 656, 0x18: 640 | 426 << 16,
        0x1c: destination, 0x20: 320, 0x24: 0, 0x28: 320 | 216 << 16,
        0x2c: 0, 0x30: 0, 0x34: 1, 0x38: 4095, 0x3c: 4095, 0x40: 4096,
        0x44: 4096, 0x48: 0, 0x4c: 0x10001, 0x50: 638 | 424 << 16,
        0x54: 319 | 212 << 16, 0x58: 638 | 424 << 16,
        **dict.fromkeys(range(0x60, 0xdc, 4), 0x10000)}
    return dict(source=d[4], source_bytes=source_bytes, source_width=640, source_height=426,
        source_stride_bytes=1312, destination=d[10], output_width=320, output_height=216,
        visible_output_height=213, output_stride_bytes=640, output_span_bytes=output_bytes,
        output_capacity=capacity, expected=expected, sampling=PROFILE,
        assumptions='Declared step2 aligned endpoint luma; neutral128 chroma; three padded rows untouched; no silicon filter identification')


def rows(source, geometry, *, backend='auto'):
    from .still_resize_devices import _numpy
    if (type(source) is not bytes or len(source) != geometry['source_bytes']
            or geometry.get('sampling') != PROFILE or backend not in ('auto', 'scalar', 'numpy')
            or backend == 'numpy' and _numpy is None):
        raise UnsupportedDevice('Still mode1 thumbnail requires its exact declared source/backend')
    if backend == 'numpy' or backend == 'auto' and _numpy is not None:
        view = _numpy.frombuffer(source, dtype=_numpy.uint8).reshape(426, 1312)
        if _numpy.any(view.reshape(426, 328, 4)[:, :320, 2:] != 128):
            raise UnsupportedDevice('Still mode1 thumbnail requires declared neutral-chroma input')
        offsets = _numpy.arange(320, dtype=_numpy.int64)*4
        result = []
        for y in range(213):
            row = _numpy.empty((160, 4), dtype=_numpy.uint8)
            row[:, :2] = view[y*2, offsets].reshape(160, 2)
            row[:, 2:] = 128
            result.append((geometry['destination']+y*640, row.tobytes()))
        return result
    for y in range(426):
        for x in range(0, 640, 2):
            if source[y*1312+2*x+2:y*1312+2*x+4] != b'\x80\x80':
                raise UnsupportedDevice('Still mode1 thumbnail requires declared neutral-chroma input')
    return [(geometry['destination']+y*640,
        b''.join(bytes((source[y*2*1312+x*4], source[y*2*1312+(x+1)*4], 128, 128))
            for x in range(0, 320, 2))) for y in range(213)]
