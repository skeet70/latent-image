"""Unarmed still RAW admission and an explicitly declared scene codec.

The finite class2 driver/packing fields come from whole stock1310E98 and
11F9320 execution. Monochrome nearest-neighbor YYCbCr luma times16 is a
simulation fixture, not a recovered CFA, exposure or sensor calibration.
This module only reads state and returns row write plans. It never writes
RAM, schedules DMA, asserts an IRQ or completes a firmware operation.
"""
import hashlib
import struct

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

from .afs_raw_source import pack_u12_rows
from .hardware import UnsupportedDevice
from .input_processing_devices import InputProcessingControl


PROFILE = 'declared-monochrome-yycbcr-nearest-luma-times16-still-raw-unmeasured'
MAX_PIXELS, MAX_SOURCE_PIXELS, RAM_BYTES = 32 * 1024 * 1024, 1048576, 0x20000000


def _integer(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise UnsupportedDevice('Still RAW plan: invalid ' + name)
    return value


def _profile(profile):
    if profile != PROFILE:
        raise UnsupportedDevice('Still RAW plan requires the explicit declared monochrome profile')


def _physical(address, length, name):
    _integer(address, 0, 0x5fffffff, name + ' address')
    physical = address - 0x40000000 if address >= 0x40000000 else address
    _integer(length, 1, 64 * 1024 * 1024, name + ' length')
    if physical >= RAM_BYTES or physical + length > RAM_BYTES:
        raise UnsupportedDevice('Still RAW plan: ' + name + ' span exceeds DRAM')
    return physical


def _word(board, address):
    return struct.unpack('<I', board.read_memory(address, 4, cpu_id=0))[0]


def _geometry(board):
    """Inspect a real finite driver; this grants no source/IRQ permission."""
    descriptor = struct.unpack('<14I', board.read_memory(0x15f6458, 56, cpu_id=0))
    first, second, x, y, height, width, pitch = descriptor[:7]
    for value, name in ((width, 'width'), (height, 'height')):
        _integer(value, 2 if name == 'width' else 1, 8192, name)
    if width & 1 or width * height > MAX_PIXELS or descriptor[10] not in (0, 1):
        raise UnsupportedDevice('Still RAW plan: unsupported raster or packing')
    _integer(x, 0, 0x3fff, 'normalized origin X')
    _integer(y, 0, 0x3fff, 'normalized origin Y')
    row_bytes = width * (3 if descriptor[10] else 4) // 2
    _integer(pitch, row_bytes, 32768, 'pitch')
    gap = pitch - row_bytes
    if gap & 15 or gap > 0x7ffffff0:
        raise UnsupportedDevice('Still RAW plan: unaligned channel2 row gap')
    destination = _physical(first, pitch * height, 'destination')
    if first & 15:
        raise UnsupportedDevice('Still RAW plan: unaligned first buffer')
    alternate = None if not second else _physical(second, pitch * height, 'alternate')
    if alternate is not None and alternate != destination and (
            destination < alternate + pitch * height and alternate < destination + pitch * height):
        raise UnsupportedDevice('Still RAW plan: distinct alternate overlaps destination')
    expected = {0x15f63a4: 3, 0x15f63a8: 3, 0x15f6454: 2,
        0x15f6490: 1, 0x15f64a0: 1, 0xbe38b8: 0,
        0x15f63ac: 0x12f60a8, 0xc6ddac: 0x11f5ea0,
        0x15f63b8: height, 0x15f63bc: width,
        0x15f63e4: height * row_bytes, 0x15f63e8: gap}
    if any(_word(board, address) != value for address, value in expected.items()):
        raise UnsupportedDevice('Still RAW plan: finite readout driver is not ready or geometry changed')
    if any((_word(board, address) & 0x3fffffff) != destination
           for address in (0x15f63b0, 0x15f63e0, 0x15f63f4)):
        raise UnsupportedDevice('Still RAW plan: finite class2 active banks differ')
    inputs = [d for d in board.devices if isinstance(d, InputProcessingControl)]
    if len(inputs) != 1:
        raise UnsupportedDevice('Still RAW plan: missing or ambiguous input engine')
    registers = inputs[0].registers
    if (registers.get(4) != 1 | descriptor[10] << 16
            or registers.get(0x354) != 0x10300000
            or registers.get(0x358) != destination
            or registers.get(0x35c) != 1 or registers.get(0x360) != gap):
        raise UnsupportedDevice('Still RAW plan: channel2 hardware differs from retained descriptor')
    interrupts = getattr(board, 'interrupts', None)
    banks = interrupts.banks if interrupts is not None else ()
    bank = banks[0] if len(banks) == 2 else None
    inflight = any(b.pending & (1 << 0x24) or b.active == 0x24 or b.injected == 0x24
                   or any(entry.get('line') == 0x24 for entry in b.active_stack) for b in banks)
    if (bank is None or not bank.enabled & (1 << 0x24) or inflight
            or any(b.enabled & (1 << 0x24) for b in banks[1:])
            or bank.config.get(0x400 + 4*0x24) != 0x920c08
            or bank.config.get(0x800 + 4*0x24) != 8):
        raise UnsupportedDevice('Still RAW plan: physical IRQ24 route is not live and drained')
    return dict(width=width, height=height, pitch=pitch, row_bytes=row_bytes, row_gap=gap,
        packed=bool(descriptor[10]), origin=(x, y), destination=destination,
        alternate=alternate, span_bytes=pitch*height, transport_bytes=row_bytes*height,
        descriptor_sha256=hashlib.sha256(struct.pack('<14I', *descriptor)).hexdigest())


def admission(board, *, profile=None):
    """Pure driver readiness query; unspecified profile always refuses."""
    try:
        _profile(profile)
        geometry = _geometry(board)
    except UnsupportedDevice as error:
        return dict(ready=False, reason=str(error), state='unarmed')
    return dict(ready=True, reason='Finite RAW driver ready; no source or IRQ permission granted',
                state='unarmed', geometry=geometry)


def encode_rows(image, *, source_width, source_height, source_stride, width, height,
                packed, profile=None, numpy=True):
    """Declare full-field nearest luma×16; return active bytes only per row."""
    _profile(profile)
    for value, name in ((source_width, 'source width'), (source_height, 'source height'),
                        (width, 'width'), (height, 'height')):
        _integer(value, 2 if 'width' in name else 1, 8192, name)
    _integer(source_stride, source_width*2, 32768, 'source stride')
    if (type(image) is not bytes or source_width & 1 or width & 1
            or len(image) != source_stride * source_height
            or source_width * source_height > MAX_SOURCE_PIXELS or source_stride*source_height > 4*1024*1024
            or width*height > MAX_PIXELS
            or type(packed) is not bool or type(numpy) is not bool):
        raise UnsupportedDevice('Still RAW plan: invalid bounded YYCbCr/source raster')
    rows = []
    xx = [x * source_width // width for x in range(width)]
    if numpy and _numpy is not None:
        pairs = _numpy.frombuffer(image, dtype=_numpy.uint8).reshape(source_height, source_stride)
        pairs = pairs[:, :source_width*2].reshape(source_height, source_width//2, 4)
        luma = pairs[:, :, :2].reshape(source_height, source_width).astype(_numpy.uint16)
    else:
        luma = None
    for y in range(height):
        sy = y * source_height // height
        if luma is None:
            values = [image[sy*source_stride + (x//2)*4 + (x&1)] * 16 for x in xx]
            rows.append(pack_u12_rows([values]) if packed else struct.pack('<'+'H'*width, *values))
        else:
            values = luma[sy, xx] * 16
            if packed:
                a, b = values[::2], values[1::2]
                output = _numpy.empty((width//2, 3), dtype=_numpy.uint8)
                output[:, 0] = a & 255
                output[:, 1] = a >> 8 | (b & 15) << 4
                output[:, 2] = b >> 4
                rows.append(output.tobytes())
            else:
                rows.append(values.astype('<u2').tobytes())
    return tuple(rows)


def prepare(board, *, scene, profile=None, numpy=True):
    """Return an unarmed row-only write plan; never apply it to the board."""
    _profile(profile)
    geometry = _geometry(board)
    required = ('address', 'bytes', 'width', 'height', 'stride_bytes', 'payload_sha256')
    if type(scene) is not dict or any(key not in scene for key in required):
        raise UnsupportedDevice('Still RAW plan: frozen YYCbCr scene is required')
    try:
        address = int(scene['address'], 16) if type(scene['address']) is str else scene['address']
    except ValueError as error:
        raise UnsupportedDevice('Still RAW plan: invalid scene address') from error
    sw = _integer(scene['width'], 2, 8192, 'source width')
    sh = _integer(scene['height'], 1, 8192, 'source height')
    ss = _integer(scene['stride_bytes'], sw*2, 32768, 'source stride')
    if sw & 1 or sw*sh > MAX_SOURCE_PIXELS or ss*sh > 4*1024*1024 or scene['bytes'] != ss*sh:
        raise UnsupportedDevice('Still RAW plan: unbounded or inconsistent source scene')
    source = _physical(address, scene['bytes'], 'scene')
    if source < geometry['destination'] + geometry['span_bytes'] and geometry['destination'] < source + scene['bytes']:
        raise UnsupportedDevice('Still RAW plan: scene overlaps destination')
    image = bytes(board.read_memory(source, scene['bytes'], cpu_id=0))
    if hashlib.sha256(image).hexdigest() != scene['payload_sha256']:
        raise UnsupportedDevice('Still RAW plan: frozen scene bytes changed')
    rows = encode_rows(image, source_width=scene['width'], source_height=scene['height'],
        source_stride=scene['stride_bytes'], width=geometry['width'], height=geometry['height'],
        packed=geometry['packed'], profile=profile, numpy=numpy)
    return dict(profile=profile, state='prepared-unarmed', geometry=geometry,
        source={key: scene[key] for key in required},
        rows=tuple((geometry['destination']+y*geometry['pitch'], row) for y, row in enumerate(rows)),
        limits='Declared monochrome nearest luma times16, without CFA/exposure calibration; row spans only; no DMA, source epoch, IRQ, firmware flag or operation completion')
