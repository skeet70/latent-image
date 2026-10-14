"""Declared Resize-to-JPEG virtual raster, never a guest RAM scratch buffer.

The separate Resize owner qualifies the actual class1 upload/start/EOF/close.
This helper binds its virtual rows to JPEG's real descriptor and source hash.
"""
from copy import deepcopy
import hashlib
import json
import struct

from .hardware import UnsupportedDevice


PROFILE = 'declared-channel8-resize-stream-monochrome-jpeg-unmeasured'
MAIN_PROFILE = 'declared-channel8-resize-main-stream-monochrome-jpeg-unmeasured'
PROFILES = frozenset((PROFILE, MAIN_PROFILE))
RESIZE_PROFILES = {
    PROFILE: 'declared-still-class1-mode1-jpeg-thumbnail-stream-unmeasured',
    MAIN_PROFILE: 'declared-still-class1-mode0-main-jpeg-stream-unmeasured',
}
KEYS = frozenset(('profile', 'anchor_sha256', 'descriptor_hex', 'descriptor_sha256', 'source',
    'source_bytes', 'source_sha256', 'width', 'height', 'stride_bytes', 'packed_sha256',
    'source_stride_bytes', 'source_capacity', 'source_width', 'source_height', 'expected', 'history_sha256'))


def _reject(reason):
    raise UnsupportedDevice('Streamed JPEG: ' + reason)


def _owner(device):
    candidates = [item for item in getattr(device.board, 'devices', ()) if callable(getattr(item, 'jpeg_stream_input', None))
                  and callable(getattr(item, 'complete_jpeg_stream', None))]
    if len(candidates) != 1 or candidates[0].board is not device.board:
        _reject('requires one same-board explicitly selected Resize stream owner')
    return candidates[0]


def validate_metadata(value, *, profile=None):
    if not isinstance(value, dict) or set(value) != KEYS:
        _reject('retained Resize stream metadata schema changed')
    for key in ('anchor_sha256', 'descriptor_sha256', 'source_sha256', 'packed_sha256'):
        if type(value[key]) is not str or len(value[key]) != 64:
            _reject('retained Resize stream identity is malformed')
    try:
        descriptor = bytes.fromhex(value['descriptor_hex'])
        d = struct.unpack('<20I', descriptor)
    except (TypeError, ValueError, struct.error):
        _reject('retained Resize stream descriptor is malformed')
    selected = next((name for name, resize in RESIZE_PROFILES.items()
                     if value['profile'] == resize), None)
    if selected is None or profile is not None and profile != selected:
        _reject('JPEG profile differs from its explicitly selected Resize input law')
    if selected == PROFILE:
        configuration = ((1, 1, 4096, 4096), (320, 320, 240, 318, 238),
            (0, 160, 0, 0, 160, 120, 159, 119, 0), 153600, (153600, 320, 240), (160, 120, 320, 640))
    else:
        configuration = ((1, 0, 4096, 4096), (4944, 4896, 3264, 4895, 3263),
            (0, 4896, 0, 0, 4896, 3264, 4895, 3263, 0), 32274432,
            (32274432, 4896, 3264), (4896, 3264, 9792, 9888))
    header, geometry, output, source_bytes, source_geometry, packed_geometry = configuration
    if (hashlib.sha256(descriptor).hexdigest() != value['descriptor_sha256']
            or d[:4] != header or d[5:10] != geometry
            or d[10:19] != output or d[19] != 0x1cd52b8
            or value['source'] != d[4] or not 0 < d[4] < 0x60000000 or d[4] & 15
            or (d[4] & 0x3fffffff) + source_bytes > 0x20000000
            or value['source_bytes'] != source_bytes
            or (value['source_capacity'], value['source_width'], value['source_height']) != source_geometry
            or (value['width'], value['height'], value['stride_bytes'], value['source_stride_bytes'])
                != packed_geometry
            or any(type(value[key]) is not int for key in
                   ('source', 'source_bytes', 'width', 'height', 'stride_bytes', 'source_stride_bytes',
                    'source_capacity', 'source_width', 'source_height'))
            or not isinstance(value['expected'], dict)
            or any(type(key) is not int or type(word) is not int for key, word in value['expected'].items())
            or value['history_sha256'] is not None and
                (type(value['history_sha256']) is not str or len(value['history_sha256']) != 64)):
        _reject('requires the actual bounded class1 thumbnail or full-field main stream geometry')
    anchor = {key: item for key, item in value.items() if key not in ('anchor_sha256', 'descriptor_hex')}
    if hashlib.sha256(json.dumps(anchor, sort_keys=True, separators=(',', ':')).encode()).hexdigest() != value['anchor_sha256']:
        _reject('retained Resize stream anchor no longer seals its complete input')
    return value


def _input(device, *, state, profile):
    value = _owner(device).jpeg_stream_input()
    if not isinstance(value, dict) or value.get('state') != state:
        _reject('Resize stream is not in its actual ' + state + ' phase')
    try:
        descriptor = value['descriptor']
        metadata = {key: deepcopy(value[key]) for key in KEYS if key != 'descriptor_hex'}
        metadata['descriptor_hex'] = descriptor.hex()
        packed = value['packed_bytes']
    except (KeyError, AttributeError):
        _reject('Resize stream facade is incomplete')
    validate_metadata(metadata, profile=profile)
    if (type(packed) is not bytes or len(packed) != metadata['stride_bytes'] * metadata['height']
            or hashlib.sha256(packed).hexdigest() != metadata['packed_sha256']):
        _reject('Resize virtual raster differs from its sealed packed identity')
    return metadata, packed


def arm_input(device, descriptor, stride_bytes, source_sha256, *, profile=PROFILE):
    metadata, _ = _input(device, state='ready', profile=profile)
    d = struct.unpack('<15I', descriptor)
    if (d[:3] != (metadata['width'], metadata['height'], metadata['source'])
            or stride_bytes != metadata['source_stride_bytes'] or source_sha256 != metadata['source_sha256']):
        _reject('JPEG descriptor/pitch/source differs from its explicitly owned Resize stream')
    return metadata


def codec_input(device):
    expected = device.declared['latch']['resize_stream']
    profile = device.declared['latch']['profile']
    validate_metadata(expected, profile=profile)
    metadata, packed = _input(device, state='streaming', profile=profile)
    if metadata != expected:
        _reject('live Resize stream changed before JPEG data delivery')
    return packed, metadata['stride_bytes']


def complete(device):
    """Only due JPEG hardware delivery may signal the selected stream EOF."""
    _owner(device).complete_jpeg_stream(device.declared['latch']['resize_stream']['anchor_sha256'])
