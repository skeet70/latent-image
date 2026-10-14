"""Proven packed12 bytes and PDAF selector fields, without guest effects.

Stock scalar readers 109FB50/10E7E6C use contiguous little-endian12-bit
pairs. This does not establish the FFF24000 aperture pixel coordinates or
its filtering. S1 flag1 is likely this packed format:1347F6C passes the
opposite encoding to1212FE4, whose flag0 uses3bytes/2samples. That format
relationship remains an inference until the active source/driver is seen.
No helper in this module supplies phase results, IRQs or focus decisions.
"""
from __future__ import annotations

from .hardware import UnsupportedDevice


PACKING_PROFILE = 'stock-scalar-contiguous-little-endian-packed12-byte-contract'
MAX_SAMPLES = 65536
MAX_BYTES = 4 * MAX_SAMPLES


def _integer(value, minimum, maximum, name):
    if type(value) is not int or not minimum <= value <= maximum:
        raise UnsupportedDevice('AF RAW bytes: invalid ' + name)
    return value


def pack_u12_rows(rows, *, stride_bytes=None):
    """Encode equal even-width rows; optional row padding is explicitly zero.

    Two samples A/B become A.low8, A.high4|B.low4<<4, B.high8.
    Row pitch belongs to the caller. Hardware alignment and phase sampling
    are deliberately not inferred from this scalar-reader contract.
    """
    if not isinstance(rows, (tuple, list)) or not rows or not isinstance(rows[0], (tuple, list)):
        raise UnsupportedDevice('AF RAW bytes: nonempty row arrays required')
    width = _integer(len(rows[0]), 2, 8192, 'row width')
    if width & 1 or width * len(rows) > MAX_SAMPLES:
        raise UnsupportedDevice('AF RAW bytes: even width/sample bound required')
    packed_bytes = width * 3 // 2
    if stride_bytes is None:
        stride_bytes = packed_bytes
    _integer(stride_bytes, packed_bytes, 32768, 'row stride')
    if len(rows) * stride_bytes > MAX_BYTES:
        raise UnsupportedDevice('AF RAW bytes: padded raster exceeds byte bound')
    result = bytearray(len(rows) * stride_bytes)
    for y, row in enumerate(rows):
        if not isinstance(row, (tuple, list)) or len(row) != width:
            raise UnsupportedDevice('AF RAW bytes: nonrectangular row arrays')
        for x in range(0, width, 2):
            a = _integer(row[x], 0, 0xfff, 'U12 sample')
            b = _integer(row[x + 1], 0, 0xfff, 'U12 sample')
            offset = y * stride_bytes + x * 3 // 2
            result[offset:offset + 3] = bytes((a & 255, a >> 8 | (b & 15) << 4, b >> 4))
    return bytes(result)


def read_u12_sample(raw, index):
    """Mirror the scalar byte contract for one base-relative pixel index.

    Use a row slice when the raster has padding, just as the whole ARM
    reader needs a row base or an appropriate linear index from its caller.
    This does not select an aperture or reinterpret sample intensity gain.
    """
    if type(raw) is not bytes or not 3 <= len(raw) <= MAX_BYTES:
        raise UnsupportedDevice('AF RAW bytes: bounded immutable bytes required')
    _integer(index, 0, MAX_SAMPLES - 1, 'sample index')
    offset = index * 3 // 2
    if offset + 2 > len(raw):
        raise UnsupportedDevice('AF RAW bytes: sample read exceeds supplied bytes')
    word = raw[offset] | raw[offset + 1] << 8
    return word & 0xfff if index % 2 == 0 else word >> 4


def phase_pattern_contract(left_pattern, right_pattern):
    """Return only the table-derived fields admitted by stock12129DC.

    Patterns0..3 have class0,4..7 class1. The classes must match.
    Class1 reverses source order. The left/right selector widths and
    horizontal/vertical result-count halving are recovered; none of these
    facts establish which sensor pixels or physical aperture they select.
    """
    _integer(left_pattern, 0, 7, 'left pattern')
    _integer(right_pattern, 0, 7, 'right pattern')
    group = left_pattern // 4
    if group != right_pattern // 4:
        raise UnsupportedDevice('AF RAW bytes: stock12129DC refuses mismatched pattern classes')
    return dict(left_pattern=left_pattern, right_pattern=right_pattern,
                classification=group, source_order=(1, 0) if group else (0, 1),
                left_selector=left_pattern & 1, right_selector=right_pattern & 3,
                selector_register_bits=(left_pattern & 1) << 16 | (right_pattern & 3) << 20,
                halve_horizontal_result_count=left_pattern not in (0, 4),
                halve_vertical_result_count=right_pattern not in (0, 4),
                limits='Recovered pattern/selector/count fields; aperture coordinates and hardware filtering unresolved')
