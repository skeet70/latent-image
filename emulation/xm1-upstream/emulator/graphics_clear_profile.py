"""Explicitly opted-in stock OpenVG initial clear and state-only grammars.

This is a bounded software inference of GPU behavior, not captured hardware.
Templates are the full B6CEA0/B85010 writer output; the B90600 footer and zero
padding are also mandatory. Unknown state, extra geometry and other lists fail.
The template source report SHA256 is
bb059d343ed60830109c70d23ef23d9acddd57d88c38f4fb9344620b184e0ab5.
"""
import math
import struct

from .hardware import UnsupportedDevice

PROFILE = 'stock-vg-clear-only'
PATH_TEMPLATE = (
    0x40080081, 0x1000000, 0x400f007b, 0x400020, 0x40010060, 0x0, 0x40010081, 0x1,
    0x40080040, 0x0, 0x40070040, 0x3f0000, 0x40070041, 0x0, 0x40070042, 0x0,
    0x40070043, 0x0, 0x40070044, 0x3f0000, 0x40070045, 0x0, 0x40070046, 0x0,
    0x40070047, 0x0, 0x40070048, 0x3f0000, 0x40010060, 0x0, 0x40020060, 0x100,
    0x400c0080, 0x200000, 0x400c008e, 0x200000, 0x400f0084, 0x2200000, 0x400f0085, 0x2200200,
    0x400f0086, 0x2200400, 0x400f0087, 0x2200600, 0x400f0088, 0x2200800, 0x28000000, 0x40400000,
    0x40a00000, 0x29000000, 0x40400000, 0x41800000, 0x28000000, 0x41800000, 0x41800000, 0x29000000,
    0x41800000, 0x40a00000, 0x4001007c, 0x1, 0x40020081, 0x100, 0x400f008b, 0x22009f8,
    0x4001008c, 0x1, 0x4007008d, 0x10001,
)
FILL_PREFIX = (
    0x40080140, 0x0, 0x40010190, 0x0, 0x400101c0, 0x0, 0x4001014d, 0x1,
    0x40010140, 0x3, 0x4001014d, 0x0, 0x400f014d, 0x0, 0x400f014d, 0x0,
    0x40040140, 0x50000, 0x40010140, 0x0, 0x400f0143, 0x0, 0x400f0153, 0x0,
    0x400f0144, 0x0, 0x400f0154, 0x0, 0x400101a2, 0x0, 0x400f01a3, 0x200040,
    0x40040184, 0x0, 0x40010187, 0x0,
)
GAMMA_BYTES = bytes.fromhex(
    '000c151c21262a2e3134373a3d3f424446484b4d4f5052545658595b5d5e606163646667686a6b6c6e6f707273747576'
    '78797a7b7c7d7e7f8082838485868788898a8b8c8d8d8e8f90919293949596979798999a9b9c9c9d9e9fa0a1a1a2a3a4'
    'a4a5a6a7a8a8a9aaababacadaeaeafb0b0b1b2b2b3b4b5b5b6b7b7b8b9b9babbbbbcbdbdbebfbfc0c0c1c2c2c3c4c4c5'
    'c5c6c7c7c8c9c9cacacbcbcccdcdcececfd0d0d1d1d2d2d3d4d4d5d5d6d6d7d7d8d9d9dadadbdbdcdcdddddededfdfe0'
    'e0e1e2e2e3e3e4e4e5e5e6e6e7e7e8e8e9e9eaeaebebececedededeeeeefeff0f0f1f1f2f2f3f3f4f4f5f5f5f6f6f7f7'
    'f8f8f9f9fafafafbfbfcfcfdfdfefeff'
)
FILL_SUFFIX = (
    0x40020140, 0x0, 0x400f014d, 0x0, 0x400f014d, 0x0, 0x40010178, 0x1,
    0x400f016c, 0x40001, 0x400101a7, 0x0, 0x400201a7, 0x100, 0x40010147, 0x1,
    0x40010157, 0x1, 0x400801a0, 0x0, 0x400101a1, 0x0, 0x40080115, 0x0,
    0x40040115, 0x0, 0x40010140, 0x0, 0x400f014d, 0x0, 0x400f014d, 0x0,
    0x40010140, 0x0, 0x400f014d, 0x0, 0x400f014d, 0x0, 0x40020140, 0x0,
    0x400f014d, 0x0, 0x400f014d, 0x0, 0x40010184, 0x0, 0x40020184, 0x0,
    0x400201a0, 0x0, 0x400101a2, 0x0, 0x400f01a4, 0x2100000, 0x40020184, 0x0,
    0x400401a7, 0x0, 0x400f0185, 0xffbf8040, 0x40010184, 0x8, 0x40080184, 0x0,
    0x400401a0, 0x0, 0x400101a5, 0x0, 0x400101aa, 0x0, 0x400f00a4, 0x2200000,
    0x400f00a5, 0x2200200, 0x400f00a6, 0x2200400, 0x400f00a7, 0x2200600, 0x400f00a8, 0x2200800,
    0x400f00a0, 0x200010, 0x400300a2, 0x3f, 0x400c0190, 0x0, 0x400c0190, 0x0,
    0x400c0190, 0x0, 0x400700ad, 0x10001, 0x400101a9, 0x3,
)
FOOTER = (0x400f0028, 0x12345678, 0x40010031, 0xf0, 0x400f0018, 0x12345678)


def _bounded_ram(address, count, ram_bytes, name):
    alias = address & 0xc0000000
    physical = address - alias
    if alias not in (0, 0x40000000) or address & 3 or physical + count > ram_bytes:
        raise UnsupportedDevice(f'Graphics clear {name} has unresolved RAM range/alias/alignment')
    return physical


def _exact_padded(words, expected, bank):
    padded = expected + [0] * (-len(expected) % 16)
    if len(words) != len(padded):
        raise UnsupportedDevice(f'Graphics clear-only {bank} word count {len(words)} != {len(padded)}')
    for index, (actual, known) in enumerate(zip(words, padded)):
        if actual != known:
            raise UnsupportedDevice(f'Graphics clear-only unknown {bank} word[{index}]={actual:#x}, expected {known:#x}')


def path_descriptor(words, *, ram_bytes, max_clear_bytes):
    if len(words) == 16:
        # B6CEA0 with the process-global initializer already executed. Exact
        # state/flush-only stream observed in the next stock boot submission.
        width, height = words[3] >> 16, words[3] & 0xffff
        frame_bytes = width * height * 4
        if width == 0 or height == 0 or width & 7 or height & 7 or frame_bytes > max_clear_bytes:
            raise UnsupportedDevice('Graphics initializer requires positive8-aligned bounded format0 dimensions')
        expected = list(PATH_TEMPLATE[:4]) + [0x40010060, 2, 0x40010081, 1]
        expected[3] = width << 16 | height
        _exact_padded(words, expected, 'repeated initializer path')
        return {'operation': 'initialize', 'first_global': False,
                'width': width, 'height': height, 'frame_bytes': frame_bytes, 'path_mode': 2,
                'pixel_effect': 'None: exact initialization/state/flush commands contain no geometry'}
    if len(words) == 32 and words[4] == 0x40010060:
        # Repeated B6CEA0 plus B90EF4 dirty bits1/2. The latter refreshes
        # exactly nine identity-matrix words and mode2; neither emits geometry
        # or any fill command. The paired fill remains the exact repeated
        # initializer grammar below. Do not treat the prior fill descriptor
        # captured at a path trigger as a new submitted fill stream.
        width, height = words[3] >> 16, words[3] & 0xffff
        frame_bytes = width * height * 4
        if width == 0 or height == 0 or width & 7 or height & 7 or frame_bytes > max_clear_bytes:
            raise UnsupportedDevice('Graphics initializer requires positive8-aligned bounded format0 dimensions')
        expected = list(PATH_TEMPLATE[:4]) + [0x40010060, 2, 0x40010081, 1]
        expected += list(PATH_TEMPLATE[10:28]) + [0x40010060, 2]
        expected[3] = width << 16 | height
        _exact_padded(words, expected, 'repeated identity-matrix initializer path')
        return {'operation': 'initialize', 'first_global': False,
                'width': width, 'height': height, 'frame_bytes': frame_bytes, 'path_mode': 2,
                'identity_matrix_refresh': True,
                'pixel_effect': 'None: exact initialization/matrix/mode commands contain no geometry'}
    if len(words) == 32:
        # First process-global B6CEA0 initialization followed by the exact
        # B90EF4 dirty-bit2 update seen in the real ROM-boot command stream.
        # No geometry is present and this case must never write pixels.
        width, height = words[3] >> 16, words[3] & 0xffff
        frame_bytes = width * height * 4
        if width == 0 or height == 0 or width & 7 or height & 7 or frame_bytes > max_clear_bytes:
            raise UnsupportedDevice('Graphics initializer requires positive8-aligned bounded format0 dimensions')
        expected = list(PATH_TEMPLATE[:4]) + list(PATH_TEMPLATE[8:28]) + [
            0x40020080, 0, 0x40010060, 2, 0x40010081, 1, 0x40010060, 2]
        expected[3] = width << 16 | height
        _exact_padded(words, expected, 'initializer path')
        return {'operation': 'initialize', 'first_global': True, 'width': width, 'height': height,
                'frame_bytes': frame_bytes, 'path_mode': 2,
                'pixel_effect': 'None: exact initialization/state/flush commands contain no geometry'}
    if len(words) != 80:
        raise UnsupportedDevice('Graphics clear-only requires one complete initial rectangle path')
    width, height = words[3] >> 16, words[3] & 0xffff
    frame_bytes = width * height * 4
    if width == 0 or height == 0 or width & 7 or height & 7 or frame_bytes > max_clear_bytes:
        raise UnsupportedDevice('Graphics clear-only requires positive8-aligned bounded format0 dimensions')
    padded_height = height + (height & 8)
    stride = padded_height * 16
    scratch = words[37]
    _bounded_ram(scratch, stride * 5, ram_bytes, 'scratch')
    corners = []
    for index in (47, 50, 53, 56):
        xy = struct.unpack('<2f', struct.pack('<2I', *words[index:index+2]))
        if any(not math.isfinite(value) or int(value) != value for value in xy):
            raise UnsupportedDevice('Graphics clear-only requires integer finite rectangle vertices')
        corners.append(tuple(int(value) for value in xy))
    (x0, y0), (left, y1), (x1, bottom), (right, top) = corners
    if (left != x0 or bottom != y1 or right != x1 or top != y0 or
            not 0 <= x0 < x1 <= width or not 0 <= y0 < y1 <= height):
        raise UnsupportedDevice('Graphics clear-only requires one clipped axis-aligned rectangle')
    sequence = words[67]
    if sequence >> 16 != 1 or sequence & 0xffff == 0:
        raise UnsupportedDevice('Graphics clear-only has unknown path synchronization word')
    expected = list(PATH_TEMPLATE)
    expected[3] = width << 16 | height
    expected[33], expected[35] = padded_height << 16, height << 16
    for index in range(5):
        expected[37 + index*2] = scratch + index*stride
    for index in (47, 48, 50, 51, 53, 54, 56, 57):
        expected[index] = words[index]
    expected[63], expected[67] = scratch + stride*5 - 8, sequence
    _exact_padded(words, expected, 'path')
    return {'operation': 'clear', 'width': width, 'height': height, 'frame_bytes': frame_bytes,
            'rectangle': [x0, y0, x1, y1], 'scratch': scratch,
            'scratch_stride': stride, 'padded_height': padded_height,
            'path_sequence': sequence & 0xffff}


def _register_bank(words, initial=None):
    bank, index = dict(initial or {}), 0
    while index < len(words):
        header = words[index]
        if header == 0:
            break
        if header in (0x28000000, 0x29000000):
            index += 3
            continue
        register, lanes = header & 0xffff, header >> 16 & 0xf
        mask = sum(0xff << (lane*8) for lane in range(4) if lanes >> lane & 1)
        bank[register] = bank.get(register, 0) & ~mask | words[index+1] & mask
        index += 2
    return bank


def parse_clear(path_words, fill_words, *, ram_bytes, max_clear_bytes, initial_banks=None):
    initial_banks = initial_banks or {}
    def banks():
        return {'path': _register_bank(path_words, initial_banks.get('path')),
                'fill': _register_bank(fill_words, initial_banks.get('fill'))}
    result = path_descriptor(path_words, ram_bytes=ram_bytes, max_clear_bytes=max_clear_bytes)
    if result['operation'] == 'initialize':
        if not result['first_global']:
            matrix_state_update = result.get('identity_matrix_refresh', False) and len(fill_words) == 592
            if len(fill_words) != 576 and not matrix_state_update:
                raise UnsupportedDevice('Graphics initializer requires exact repeated-global fill state and footer')
            order = fill_words[559]
            if order not in (0, 1):
                raise UnsupportedDevice('Graphics initializer order is unsupported')
            expected = list(FILL_PREFIX)
            for value in GAMMA_BYTES:
                expected += [0x40070188, value * 0x010101]
            expected += list(FILL_SUFFIX[:22])
            if matrix_state_update:
                # B90EF4 dirty bits4/8: zero clip-record count and mode0.
                # This pair was captured only at the subsequent actual fill
                # trigger; the prior576-word descriptor is not its evidence.
                expected += [0x40010100, 0, 0x40010115, 0]
            expected[31] = result['height'] << 16 | result['width']
            expected[559] = order
            _exact_padded(fill_words, expected + list(FOOTER), 'repeated initializer fill')
            result.update(order=order)
            if matrix_state_update:
                result.update(zero_clip_mode_refresh=True)
            return result, banks()
        if len(fill_words) != 592:
            raise UnsupportedDevice('Graphics initializer requires exact first-global fill state and footer')
        order, target = fill_words[559], fill_words[571]
        if order not in (0, 1):
            raise UnsupportedDevice('Graphics initializer order is unsupported')
        physical = _bounded_ram(target, result['frame_bytes'], ram_bytes, 'initializer target')
        expected = list(FILL_PREFIX)
        for value in GAMMA_BYTES:
            expected += [0x40070188, value * 0x010101]
        expected += list(FILL_SUFFIX[:20]) + [0x400101b0, 0xb, 0x400f01a4, target,
            0x400f01a8, target + result['width'] * result['height'] - 8, 0x400101a1, 0]
        if fill_words[576] == 0x40010100:
            # Exact B90EF4 dirty bits4/8: zero scissor-record count and mode0.
            # Nonzero records/mode and every other update remain unsupported.
            expected += [0x40010100, 0, 0x40010115, 0]
        expected[31] = result['height'] << 16 | result['width']
        expected[559] = order
        _exact_padded(fill_words, expected + list(FOOTER), 'initializer fill')
        result.update(target=physical, target_register=hex(target), order=order)
        return result, banks()
    if len(fill_words) != 656:
        raise UnsupportedDevice('Graphics clear-only requires complete initializer, one clear and exact footer')
    width, height = result['width'], result['height']
    order, destination, canonical = fill_words[559], fill_words[601], fill_words[607]
    if order not in (0, 1):
        raise UnsupportedDevice('Graphics clear-only order is unsupported')
    physical = _bounded_ram(destination, result['frame_bytes'], ram_bytes, 'destination')
    scratch_physical = _bounded_ram(result['scratch'], result['scratch_stride']*5, ram_bytes, 'scratch')
    if physical < scratch_physical + result['scratch_stride']*5 and scratch_physical < physical + result['frame_bytes']:
        raise UnsupportedDevice('Graphics clear-only overlapping framebuffer/scratch is unproved')
    sequence = fill_words[639]
    if sequence >> 16 != 1 or sequence & 0xffff == 0:
        raise UnsupportedDevice('Graphics clear-only has unknown fill synchronization word')
    expected = list(FILL_PREFIX)
    for value in GAMMA_BYTES:
        expected += [0x40070188, value * 0x010101]
    expected += FILL_SUFFIX
    expected[31] = height << 16 | width
    expected[559], expected[601], expected[607] = order, destination, canonical
    for index in range(5):
        expected[619 + index*2] = result['scratch'] + index*result['scratch_stride']
    expected[629] = result['padded_height'] << 16 | 0x10
    expected[631], expected[639] = width - 1, sequence
    _exact_padded(fill_words, expected + list(FOOTER), 'fill')
    red, green, blue, alpha = ((canonical >> shift) & 0xff for shift in (0, 8, 16, 24))
    pixel = red << 24 | green << 16 | blue << 8 | alpha if order == 0 else alpha << 24 | red << 16 | green << 8 | blue
    result.update(destination=physical, destination_register=hex(destination),
                  canonical_rgba_word=hex(canonical), pixel_word=hex(pixel), order=order,
                  fill_sequence=sequence & 0xffff, pixel_bytes=pixel.to_bytes(4, 'little'))
    return result, banks()


def render_clear(source_bytes, descriptor):
    """Atomic software-derived Morton update; preserve every other pixel byte."""
    output = bytearray(source_bytes)
    width = descriptor['width']
    x0, y0, x1, y1 = descriptor['rectangle']
    pixel = descriptor['pixel_bytes']
    morton = tuple(sum(((x >> bit & 1) << (2*bit)) | ((y >> bit & 1) << (2*bit+1))
                       for bit in range(3)) for y in range(8) for x in range(8))
    for y in range(y0, y1):
        row = y // 8 * (width // 8)
        for x in range(x0, x1):
            offset = ((row + x // 8) * 64 + morton[(y & 7)*8 + (x & 7)]) * 4
            output[offset:offset+4] = pixel
    return bytes(output)
