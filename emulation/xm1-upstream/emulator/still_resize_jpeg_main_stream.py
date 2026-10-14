"""One observed unity main-image stream after the closed JPEG thumbnail.

The prior thumbnail remains a sealed owner. The virtual raster removes
declared row padding without writing a guest buffer or supplying IRQ74.
"""
from copy import copy, deepcopy
import struct

from .hardware import UnsupportedDevice


PROFILE = 'declared-still-class1-mode0-main-jpeg-stream-unmeasured'
SOURCE_BYTES = 9888*3264


def _reject(reason):
    raise UnsupportedDevice('Main Resize JPEG stream: '+reason)


def plan(descriptor):
    from .still_resize_devices import _physical
    d = struct.unpack('<20I', descriptor)
    if (d[:4] != (1, 0, 4096, 4096) or d[5:10] != (4944, 4896, 3264, 4895, 3263)
            or d[10:19] != (0, 4896, 0, 0, 4896, 3264, 4895, 3263, 0)
            or d[19] != 0x1cd52b8 or d[4] & 15):
        _reject('requires the observed class1/mode0/unity full-field descriptor')
    source = _physical(d[4], SOURCE_BYTES)
    expected = {4: 0x40, 0x10: source, 0x14: 4944, 0x18: 4896 | 3264 << 16,
        0x1c: 0, 0x20: 4896, 0x24: 0, 0x28: 4896 | 3264 << 16,
        0x2c: 0, 0x30: 0, 0x34: 0, 0x38: 4095, 0x3c: 4095, 0x40: 4096,
        0x44: 4096, 0x48: 0, 0x4c: 0x10001, 0x50: 4895 | 3263 << 16,
        0x54: 4895 | 3263 << 16, 0x58: 4895 | 3263 << 16,
        **dict.fromkeys(range(0x60, 0xdc, 4), 0x10000)}
    return dict(source=d[4], source_bytes=SOURCE_BYTES, source_width=4896, source_height=3264,
        source_stride_bytes=9888, width=4896, height=3264, stride_bytes=9792, expected=expected)


def pixels(source):
    if type(source) is not bytes or len(source) != SOURCE_BYTES:
        _reject('requires the exact original pitched source span')
    # Explicit packed YYCbCr unity law: preserve active samples, omit padding.
    return b''.join(source[y*9888:y*9888+9792] for y in range(3264))


def _digest(record):
    from .still_resize_jpeg_stream import _hash, _seal
    marker = record['marker']
    plain = dict(marker={k: v for k, v in marker.items() if k not in ('history', 'input')},
        input={k: v for k, v in marker['input'].items() if k not in ('descriptor', 'packed_bytes')},
        history=[item['sha256'] for item in marker['history']],
        registers=record['registers'], uploads=record['uploads'],
        ram_descriptor_sha256=_hash(record['ram_descriptor']))
    return _seal(plain)


def validate_previous(device, marker):
    from . import still_resize_jpeg_stream as stream
    record = marker.get('previous')
    if (not isinstance(record, dict)
            or set(record) != {'marker', 'registers', 'uploads', 'ram_descriptor', 'sha256'}
            or record['marker'].get('profile') != stream.PROFILE
            or record['marker'].get('phase') != 'idle-acknowledged'
            or _digest(record) != record['sha256']
            or device.request['jpeg_stream'] != record['marker']
            or record['ram_descriptor'] != record['marker']['input']['descriptor'][:76]+bytes(4)
            or marker['entry_words'] != record['registers']
            or marker['entry_uploads'] != record['uploads']
            or marker['history'] != record['marker']['history']):
        _reject('closed thumbnail capsule changed')
    view = copy(device)
    view.request = {k: v for k, v in device.request.items() if k != 'jpeg_main_stream'}
    view.registers, view.uploads = dict(record['registers']), list(record['uploads'])
    view.state, view.pending_action = 'idle', None
    # Only the sealed closed predecessor is inspected. Current main RAM and
    # IRQ ownership belong to the new, independently admitted stream.
    stream.validate(view, historical=True)


def arm_initialization(device, profile):
    from . import still_resize_jpeg_stream as stream
    from .still_resize_devices import PROFILE as DEVICE_PROFILE
    marker = device.request.get('jpeg_stream') if isinstance(device.request, dict) else None
    if (profile != PROFILE or device.profile != DEVICE_PROFILE or not isinstance(marker, dict)
            or marker.get('profile') != stream.PROFILE or marker.get('phase') != 'idle-acknowledged'
            or 'jpeg_main_stream' in device.request or device.state != 'idle'
            or device.pending_action is not None or device.board.failure is not None):
        _reject('requires one explicit genuinely closed thumbnail stream')
    stream.validate(device)
    if not stream._closed(device):
        _reject('thumbnail kernel registration is not retired and drained')
    record = dict(marker=deepcopy(marker), registers=dict(device.registers), uploads=tuple(device.uploads),
        ram_descriptor=bytes(device.board.read_memory(0xc9ba40, 80, cpu_id=0)))
    record['sha256'] = _digest(record)
    device.request['jpeg_main_stream'] = dict(profile=PROFILE, phase='awaiting-open',
        armed_tick=device.board.virtual_ticks, history=deepcopy(marker['history']),
        history_sha256=stream._seal([item['sha256'] for item in marker['history']]+[record['sha256']]),
        entry_uploads=tuple(device.uploads),
        entry_words=dict(device.registers), input=None, start_step=0, started_tick=None,
        finished_tick=None, stop_tick=None, quiet_tick=None, previous=record)
    return stream.summary(device)


def arm_scene(device, profile, *, source_sha256, source_capacity):
    from . import still_resize_jpeg_stream as stream
    if profile != PROFILE:
        _reject('requires the exact named main streaming law')
    return stream.arm_scene(device, profile, source_sha256=source_sha256, source_capacity=source_capacity)
