"""Declared step4 endpoint sampling for the observed S2 mode2 preview.

The genuine request/upload has 426 visible rows within 432 programmed rows.
Preserve pitch padding and the final six rows. This is a named scene law,
not a recovered silicon resampling filter or decimation calibration.
"""
import struct

from .hardware import UnsupportedDevice


PROFILE = 'declared-still-mode2-preview-step4-endpoint-luma-unmeasured'


def plan(descriptor, capacity):
    from .still_resize_devices import _physical, MAX_BYTES
    d = struct.unpack('<20I', descriptor)
    if (d[:4] != (0, 2, 7839, 7854) or d[5:10] != (4944, 4896, 3264, 4892, 3260)
            or d[11:19] != (656, 0, 0, 640, 432, 639, 425, 0)
            or d[19] != 0x118060c or type(capacity) is not int
            or not 1312*432 <= capacity <= MAX_BYTES or (d[4] | d[10]) & 15):
        raise UnsupportedDevice('Still mode2 preview requires the observed step4/640x426-in-432 request')
    source_bytes, output_bytes = 9888*3264, 1312*432
    source, destination = _physical(d[4], source_bytes), _physical(d[10], output_bytes)
    if source < destination+output_bytes and destination < source+source_bytes:
        raise UnsupportedDevice('Still mode2 preview source/output physically overlap')
    expected = {4: 0, 0x10: source, 0x14: 4944, 0x18: 4896 | 3264 << 16,
        0x1c: destination, 0x20: 656, 0x24: 0, 0x28: 640 | 432 << 16,
        0x2c: 1, 0x30: 0, 0x34: 2, 0x38: 7839, 0x3c: 7854, 0x40: 4096,
        0x44: 4096, 0x48: 0, 0x4c: 0x10001, 0x50: 4892 | 3260 << 16,
        0x54: 639 | 425 << 16, 0x58: 4892 | 3260 << 16,
        **dict.fromkeys(range(0x60, 0xdc, 4), 0x10000)}
    return dict(source=d[4], source_bytes=source_bytes, source_width=4896, source_height=3264,
        source_stride_bytes=9888, destination=d[10], output_width=640, output_height=432,
        visible_output_height=426, output_stride_bytes=1312, output_span_bytes=output_bytes,
        output_capacity=capacity, expected=expected, sampling=PROFILE,
        assumptions='Declared step4 aligned endpoint-floor luma; neutral128 chroma; six padded rows and pitch padding untouched; no silicon filter identification')


def rows(source, geometry, *, backend='auto'):
    from .still_resize_devices import _numpy
    if (type(source) is not bytes or len(source) != geometry['source_bytes']
            or geometry.get('sampling') != PROFILE or backend not in ('auto', 'scalar', 'numpy')
            or backend == 'numpy' and _numpy is None):
        raise UnsupportedDevice('Still mode2 preview requires its exact declared source/backend')
    width, height, stride = 4896, 3264, 9888
    samples = [(x*1223//639)*4 for x in range(640)]
    if backend == 'numpy' or backend == 'auto' and _numpy is not None:
        view = _numpy.frombuffer(source, dtype=_numpy.uint8).reshape(height, stride)
        if _numpy.any(view.reshape(height, stride//4, 4)[:, :width//2, 2:] != 128):
            raise UnsupportedDevice('Still mode2 preview requires declared neutral-chroma input')
        origins = _numpy.asarray(samples, dtype=_numpy.int64)
        offsets = origins//2*4+origins%2
        result = []
        for y in range(426):
            row = _numpy.empty((320, 4), dtype=_numpy.uint8)
            row[:, :2] = view[(y*815//425)*4, offsets].reshape(320, 2)
            row[:, 2:] = 128
            result.append((geometry['destination']+y*1312, row.tobytes()))
        return result
    for y in range(height):
        for x in range(0, width, 2):
            if source[y*stride+2*x+2:y*stride+2*x+4] != b'\x80\x80':
                raise UnsupportedDevice('Still mode2 preview requires declared neutral-chroma input')
    result = []
    for y in range(426):
        origin = (y*815//425)*4*stride
        row = b''.join(bytes((source[origin+samples[x]*2],
                             source[origin+samples[x+1]*2], 128, 128)) for x in range(0, 640, 2))
        result.append((geometry['destination']+y*1312, row))
    return result
