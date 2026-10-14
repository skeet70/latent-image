"""Declared sparse U16 correction; coordinate and tap choices are assumptions.

Packed record size and four six-bit coefficients are observed firmware ABI.
This experiment chooses low13 as column, high13 as row, subtracts the declared
sensor origin, and applies the first qtable entry to left/right/up/down values.
Flags and other qtable entries are retained/validated, not reverse engineered.
"""
import hashlib
import struct

PROFILE = 'declared-sparse-monochrome-four-neighbor-prefilter-unmeasured'
COEFFICIENT_MASK = 0xf7efdfbf


def coefficients(qtable):
    if type(qtable) is not bytes or len(qtable) != 512:
        raise ValueError('Prefilter requires the actual 128-WORD table')
    words = struct.unpack('<128I', qtable)
    weights = tuple(tuple((word >> shift) & 0x3f for shift in (0, 7, 14, 21)) for word in words)
    if any(sum(row) not in (0, 32) for row in weights) or sum(weights[0]) != 32:
        raise ValueError('Prefilter coefficient sum differs from the recovered domain')
    return weights[0], tuple(word & COEFFICIENT_MASK for word in words)


def render(source, *, width, height, pitch, origin, source_offset, auxiliary, count, qtable, profile, column_shift=0):
    if (profile != PROFILE or type(source) is not bytes or type(auxiliary) is not bytes
            or any(type(v) is not int for v in (width, height, pitch, source_offset, count))
            or not 2 < width < 8192 or not 2 < height < 8192 or pitch < width * 2 or pitch & 1
            or source_offset < 0 or source_offset & 7 or source_offset >= width * 2
            or type(column_shift) is not int or column_shift < 0 or column_shift >= width
            or not (height - 1) * pitch + width * 2 <= len(source) <= height * pitch
            or not 0 < count < 0x40000 or len(auxiliary) != (count * 34 + 7) // 8
            or type(origin) is not tuple or len(origin) != 2
            or any(type(v) is not int or v < 0 for v in origin)):
        raise ValueError('Prefilter declared source/calibration bounds differ')
    weights, uploads = coefficients(qtable)
    bits = int.from_bytes(auxiliary, 'little')
    if bits >> (count * 34):
        raise ValueError('Prefilter auxiliary padding is not a packed 34-bit list')
    patches = {}
    eligible = 0
    for index in range(count):
        record = bits >> (index * 34) & ((1 << 34) - 1)
        x = (record & 0x1fff) + column_shift - origin[0]
        y = (record >> 17 & 0x1fff) - origin[1]
        # The source pointer starts eight bytes into the owned RAW allocation.
        # No endpoint, padding byte or unowned suffix is read or written.
        if not 0 < x < width - 1 or not 0 < y < height - 1:
            continue
        offset = y * pitch + x * 2
        neighbours = (offset - 2, offset + 2, offset - pitch, offset + pitch)
        if min((*neighbours, offset)) < source_offset or max(neighbours) + 2 > len(source):
            continue
        eligible += 1
        samples = [int.from_bytes(source[where:where + 2], 'little') for where in neighbours]
        corrected = min(65535, (sum(w * v for w, v in zip(weights, samples)) + 16) // 32)
        patches[offset] = corrected.to_bytes(2, 'little')
    output = bytearray(source)
    for offset, data in patches.items():
        output[offset:offset + 2] = data
    return dict(profile=PROFILE, weights=weights, coefficient_uploads=uploads,
                patches=tuple(sorted(patches.items())), eligible_records=eligible,
                changed_pixels=sum(source[offset:offset + 2] != data for offset, data in patches.items()),
                source_sha256=hashlib.sha256(source).hexdigest(),
                output_sha256=hashlib.sha256(output).hexdigest())
