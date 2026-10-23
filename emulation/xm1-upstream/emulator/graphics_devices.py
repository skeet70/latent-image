"""Bounded stock graphics-accelerator initialization and pattern fill.

The MMIO helper at0xbcdbfc doubles its logical offsets. This device operates
on physical addresses and performs the independently evidenced packet4 RAM
fill. An explicit optional copy profile reproduces one stock32bpp CPU fallback;
it does not claim independently observed GPU pixel semantics. A second opt-in
profile accepts an exact initial rectangle clear and matched state-only
initializers, which write zero pixels. A separate declared UI profile parses
bounded driver-generated path/fill batches and renders affine solid/image/linear/centered-radial
primitives with explicitly unmeasured software pixel/AA/stroke/timing laws.
Other rasterization and copy commands stop explicitly.
See research/emulator-graphics/README.md for driver proof and model limits.
"""
from collections import OrderedDict
import functools
import hashlib
import math
import pickle
import struct

from .hardware import UnsupportedDevice
from .graphics_clear_profile import (PROFILE as CLEAR_PROFILE, path_descriptor, parse_clear, render_clear,
                                     PATH_TEMPLATE, FILL_PREFIX, FILL_SUFFIX, GAMMA_BYTES, FOOTER)
from x70_native._native import ui_gradient as _ui_gradient, ui_gradient_colors as _ui_gradient_colors


# B6BB44/B6B9D4/B83F3C establish the list grammar. These sets describe the
# bounded UI register families, not all SMAPH-H commands. Register addresses
# are command indices, independent of the doubled host-MMIO addressing.
UI_PROFILE = 'declared-vg-ui-primitives-unmeasured'
_UI_PATH_REGISTERS = ({0x10, 0x20, 0x30, 0x60, 0x69, 0x6a, 0x7b, 0x7c,
                       0x80, 0x81, 0x8b, 0x8c, 0x8d, 0x8e} |
                      set(range(0x40, 0x49)) | set(range(0x61, 0x69)) |
                      set(range(0x84, 0x89)))
_UI_FILL_REGISTERS = ({0x18, 0x28, 0x31, 0xa0, 0xa2, 0xad, 0x100, 0x115,
                       0x117, 0x118, 0x11b, 0x140, 0x143, 0x144, 0x147, 0x14d, 0x14e,
                       0x152, 0x153, 0x154, 0x155, 0x156, 0x157, 0x16c,
                       0x168, 0x169, 0x16a, 0x16b, 0x16d, 0x178, 0x184, 0x185, 0x187, 0x188, 0x190, 0x1c0} |
                      set(range(0xa4, 0xa9)) | set(range(0xc0, 0xe0)) |
                      set(range(0x106, 0x115)) | set(range(0x180, 0x184)) |
                      {0x1a0, 0x1a1, 0x1a2, 0x1a3, 0x1a4, 0x1a5, 0x1a7, 0x1a9, 0x1aa})
_UI_GEOMETRY_WORDS = {0x28: 2, 0x29: 2, 0x2a: 2, 0x2b: 2, 0x2c: 2,
                      0x30: 4, 0x31: 6, 0x32: 8, 0x33: 8}
# Internal integer-only bank metadata, outside the hardware register namespace:
# 16B selects one of two128-word ramp banks;16C writes/increments its index.
# Keep each slot's byte-lane mask so a truncated first upload cannot draw.
_UI_RAMP_VALUES, _UI_RAMP_LANES, _UI_RAMP_CURSOR = 0x10000, 0x10100, 0x10200


def _ui_ram(address, length, ram_bytes, name):
    alias = address & 0xc0000000
    physical = address - alias
    if (alias not in (0, 0x40000000) or address & 3 or length <= 0 or
            physical + length > ram_bytes):
        raise UnsupportedDevice(f'Graphics UI {name} has unresolved RAM range/alias/alignment')
    return physical


def _ui_float(word):
    """Decode B90EF4/B6DF2C's sign1/exponent7/mantissa16 number."""
    if word & 0xff000000 or (word >> 16 & 0x7f) == 0x7f:
        raise UnsupportedDevice('Graphics UI matrix/radius has unsupported float24 encoding')
    exponent = word >> 16 & 0x7f
    if exponent == 0:
        if word & 0x7fffff:
            raise UnsupportedDevice('Graphics UI float24 denormal is unproved')
        return 0.0
    value = math.ldexp(1 + (word & 0xffff) / 65536, exponent - 63)
    value = -value if word & 0x800000 else value
    if abs(value) > 2**24:
        raise UnsupportedDevice('Graphics UI float24 exceeds the declared coordinate bound')
    return value


_UI_STREAM_CACHE = {}
# Byte-lane write masks by the header's 4-bit lane field.
_UI_LANE_MASKS = tuple(sum(255 << (8 * lane) for lane in range(4) if lanes >> lane & 1)
                       for lanes in range(16))
_UI_PATH_MARKERS = {0x10: 0x12345678, 0x20: 0x12345678, 0x30: 0xf0}
_UI_FILL_MARKERS = {0x18: 0x12345678, 0x28: 0x12345678, 0x31: 0xf0}


def _ui_stream(words, bank_name, initial=None):
    """_ui_stream_parse, memoized on its complete inputs.

    A submission parses each list up to three times and frames repeat lists;
    the parse is a pure function of the words, bank and initial registers.
    Callers get fresh dicts; rejections are not cached and raise every time.
    """
    key = (bank_name, tuple(words), tuple(sorted(initial.items())) if initial else ())
    hit = _UI_STREAM_CACHE.get(key)
    if hit is None:
        hit = _ui_stream_parse(words, bank_name, initial)
        if len(_UI_STREAM_CACHE) >= 1024:
            _UI_STREAM_CACHE.clear()
        _UI_STREAM_CACHE[key] = hit
    draws, registers = hit
    return [dict(draw, registers=dict(draw['registers'])) for draw in draws], dict(registers)


_NATIVE_STREAM = False


def _ui_stream_parse(words, bank_name, initial=None):
    """The native parser (x70_native ui_stream_parse) if built, else the reference."""
    global _NATIVE_STREAM
    if _NATIVE_STREAM is False:
        try:
            from x70_native._native import ui_stream_parse as _NATIVE_STREAM
        except (ImportError, AttributeError):
            _NATIVE_STREAM = None
    if _NATIVE_STREAM is None:
        return _ui_stream_reference(words, bank_name, initial)
    try:
        return _NATIVE_STREAM(words, bank_name == 'path', initial or None)
    except ValueError as error:
        raise UnsupportedDevice(str(error)) from None


def _ui_stream_reference(words, bank_name, initial=None):
    """Parse byte-lane writes and geometry; never treat an unknown word as NOP.

    Draw state is sampled at the real 8D fence / AD+1A9 pair. Sequence numbers
    may start anywhere (new client contexts reset them), but within one list
    they must increment with the driver's FFFF -> 1 wrap. Zero padding is
    allowed only at the end and is bounded by the driver's 16-word alignment.
    """
    if not words or len(words) % 16:
        raise UnsupportedDevice('Graphics UI requires complete16-word-aligned lists')
    registers = dict(initial or {})
    known = _UI_PATH_REGISTERS if bank_name == 'path' else _UI_FILL_REGISTERS
    draws, geometry, position, previous, pending, geometry_state = [], [], 0, None, None, None
    while position < len(words):
        header = words[position]
        if header == 0:
            if len(words) - position > 15 or any(words[position:]):
                raise UnsupportedDevice('Graphics UI has interior/excessive zero padding')
            break
        position += 1
        if header & 0xfff00000 == 0x40000000:
            index, lanes = header & 0xffff, header >> 16 & 15
            if not lanes or index not in known or position == len(words):
                raise UnsupportedDevice(f'Graphics UI unknown/incomplete {bank_name} register {header:#x}')
            value = words[position]
            position += 1
            mask = _UI_LANE_MASKS[lanes]
            registers[index] = registers.get(index, 0) & ~mask | value & mask
            if bank_name == 'fill' and index in (0x16b, 0x16c):
                if registers.get(0x140, 0) & 0x400:
                    if index == 0x16b:
                        selector = registers[index]
                        if lanes != 15 or selector & ~0x1007f:
                            raise UnsupportedDevice('Graphics UI gradient ramp selector is unsupported')
                        registers[_UI_RAMP_CURSOR] = selector
                    else:
                        cursor = registers.get(_UI_RAMP_CURSOR)
                        if cursor is None or cursor & ~0x1007f or lanes not in (3, 12, 15):
                            raise UnsupportedDevice('Graphics UI gradient ramp upload has no bounded index/lanes')
                        ramp_index = (cursor >> 16)*128 + (cursor & 127)
                        slot = _UI_RAMP_VALUES + ramp_index
                        registers[slot] = registers.get(slot, 0) & ~mask | value & mask
                        lane_slot = _UI_RAMP_LANES + ramp_index
                        registers[lane_slot] = registers.get(lane_slot, 0) | lanes
                        registers[_UI_RAMP_CURSOR] = cursor + 1
                elif index == 0x16c and registers[index] != 0x40001:
                    raise UnsupportedDevice('Graphics UI non-gradient ramp/filter write is unsupported')
            markers = _UI_PATH_MARKERS if bank_name == 'path' else _UI_FILL_MARKERS
            if index in markers and registers[index] != markers[index]:
                raise UnsupportedDevice('Graphics UI unknown stream flush marker')
            if bank_name == 'path' and index == 0x8d or bank_name == 'fill' and index == 0xad:
                sequence = registers[index]
                if sequence >> 16 != 1 or sequence & 0xffff == 0:
                    raise UnsupportedDevice('Graphics UI unknown synchronization word')
                sequence &= 0xffff
                if previous is not None and sequence != (1 if previous == 0xffff else previous + 1):
                    raise UnsupportedDevice('Graphics UI nonconsecutive draw fences')
                previous = sequence
                if bank_name == 'path':
                    if not geometry:
                        raise UnsupportedDevice('Graphics UI geometry-free draw needs the exact initializer profile')
                    if any(registers.get(key) != value for key, value in geometry_state.items()):
                        raise UnsupportedDevice('Graphics UI path transform/style changed during geometry')
                    draws.append(dict(sequence=sequence, registers=dict(registers), geometry=geometry))
                    geometry, geometry_state = [], None
                else:
                    if pending is not None:
                        raise UnsupportedDevice('Graphics UI fill fence is missing its draw')
                    pending = sequence
            elif bank_name == 'fill' and index == 0x1a9:
                if pending is None or registers[index] != 3:
                    raise UnsupportedDevice('Graphics UI draw requires one preceding fill fence and control3')
                draws.append(dict(sequence=pending, registers=dict(registers)))
                pending = None
        elif bank_name == 'path' and header >> 24 in _UI_GEOMETRY_WORDS:
            code, flags = header >> 24, header >> 16 & 255
            count = _UI_GEOMETRY_WORDS[code]
            if header & 0xffff or flags & ~0x3f or position + count > len(words):
                raise UnsupportedDevice('Graphics UI unknown/incomplete geometry flags or operands')
            values = struct.unpack('<' + 'f' * count, struct.pack('<' + 'I' * count, *words[position:position+count]))
            if any(not math.isfinite(value) or abs(value) > 2**24 for value in values):
                raise UnsupportedDevice('Graphics UI geometry is nonfinite or outside the declared bound')
            if geometry_state is None:
                geometry_state = {key: registers.get(key) for key in
                                  tuple(range(0x40, 0x49)) + (0x60, 0x69, 0x6a)}
            geometry.append((code, flags, values))
            position += count
        else:
            raise UnsupportedDevice(f'Graphics UI unknown {bank_name} command {header:#x}')
    if geometry or pending is not None:
        raise UnsupportedDevice('Graphics UI list ends before its draw fence')
    if bank_name == 'fill':
        if tuple(words[:position][-6:]) != (0x400f0028, 0x12345678, 0x40010031, 0xf0, 0x400f0018, 0x12345678):
            raise UnsupportedDevice('Graphics UI requires the complete B90600 fill footer')
    return draws, registers


def _ui_affine(registers, first, *, row_major=False, allow_singular=False):
    if any(first + index not in registers for index in range(9)):
        raise UnsupportedDevice('Graphics UI requires a complete matrix state')
    # B6DF2C sets path40's separate high-byte image-transform control. It
    # does not belong to the float24 payload in the three written low lanes.
    if not row_major and registers[first] >> 24 not in (0, 1):
        raise UnsupportedDevice('Graphics UI unknown path matrix control byte')
    values = [_ui_float(registers[first + index] & 0xffffff if not row_major and index == 0
                        else registers[first + index]) for index in range(9)]
    if row_major:
        a, b, c, d, e, f, g, h, i = values
    else:
        a, d, g, b, e, h, c, f, i = values
    if (g, h, i) != (0.0, 0.0, 1.0):
        raise UnsupportedDevice('Graphics UI requires an affine matrix')
    if not allow_singular and a * e - b * d == 0:
        raise UnsupportedDevice('Graphics UI requires a nonsingular affine matrix')
    return (a, b, c, d, e, f)




def _ui_initialize(path_words, fill_words, *, initial_banks, max_bytes):
    """B6CEA0 repeated state + B90EF4 dirty matrix/mode/scissor branches.

    This validates parameterized writer branches, rather than accepting any
    empty list. Every upload, lane, order, footer and padding must match; there
    are no geometry records or draw fences and therefore zero pixel writes.
    B90EF4 emits each scissor through B6C030 (lowX,lowY,highX,highY), then count
    and optional enable. The two dirty bits are independent.
    """
    path_jobs, path_bank = _ui_stream(path_words, 'path', initial_banks.get('path'))
    fill_jobs, fill_bank = _ui_stream(fill_words, 'fill', initial_banks.get('fill'))
    if path_jobs or fill_jobs or len(path_words) not in (16, 32):
        raise UnsupportedDevice('Graphics UI state-only initialization cannot contain draws or unknown path updates')
    dimensions = path_bank.get(0x7b, 0)
    width, height = dimensions >> 16, dimensions & 0xffff
    if (not width or not height or width & 7 or height & 7 or width*height > 0x100000 or
            width*height*4 > max_bytes):
        raise UnsupportedDevice('Graphics UI initializer has unsupported framebuffer dimensions')
    mode = path_words[5]
    if mode not in (0, 1, 2):
        raise UnsupportedDevice('Graphics UI initializer sample mode is unsupported')
    expected_path = [*PATH_TEMPLATE[:4], 0x40010060, mode, 0x40010081, 1]
    expected_path[3] = dimensions
    position = len(expected_path)
    matrix_refresh = path_words[position] == 0x40070040
    if matrix_refresh:
        if position + 18 > len(path_words):
            raise UnsupportedDevice('Graphics UI initializer matrix refresh is truncated')
        for index in range(9):
            if path_words[position] != 0x40070040 + index:
                raise UnsupportedDevice('Graphics UI initializer matrix refresh is incomplete/reordered')
            _ui_float(path_words[position+1])
            expected_path += path_words[position:position+2]
            position += 2
        _ui_affine(path_bank, 0x40, allow_singular=True)
    if path_words[position] == 0x40010060:
        expected_path += [0x40010060, mode]
    if list(path_words) != expected_path + [0]*(-len(expected_path) % 16):
        raise UnsupportedDevice('Graphics UI initializer requires the complete repeated B6CEA0 path branch')
    expected_fill = list(FILL_PREFIX)
    for value in GAMMA_BYTES:
        expected_fill += [0x40070188, value*0x010101]
    expected_fill += FILL_SUFFIX[:22]
    if len(fill_words) < len(expected_fill) + len(FOOTER):
        raise UnsupportedDevice('Graphics UI initializer fill state is incomplete')
    order = fill_words[559]
    if order not in (0, 1):
        raise UnsupportedDevice('Graphics UI initializer order is unsupported')
    expected_fill[31], expected_fill[559] = height << 16 | width, order
    if list(fill_words[:len(expected_fill)]) != expected_fill:
        raise UnsupportedDevice('Graphics UI initializer requires the complete known state/gamma upload')
    position, clips = len(expected_fill), []
    while position < len(fill_words) and fill_words[position] == 0x400300c0 + len(clips)*2:
        if len(clips) == 16 or position + 8 > len(fill_words):
            raise UnsupportedDevice('Graphics UI initializer scissor count exceeds its bound')
        index = 0xc0 + len(clips)*2
        headers = [0x40030000 | index, 0x40030000 | index+1,
                   0x400c0000 | index, 0x400c0000 | index+1]
        words = fill_words[position:position+8]
        if list(words[::2]) != headers or any(word & 0xffff0000 for word in words[1:4:2]) or any(word & 0xffff for word in words[5:8:2]):
            raise UnsupportedDevice('Graphics UI initializer scissor lanes/order are unsupported')
        x0, y0, x1, y1 = words[1], words[3], (words[5] >> 16)+1, (words[7] >> 16)+1
        if x0 >= x1 or y0 >= y1:
            raise UnsupportedDevice('Graphics UI initializer scissor has reversed endpoints')
        clips.append((x0, y0, x1, y1))
        expected_fill += words
        position += 8
    count_refresh = fill_words[position] == 0x40010100
    if count_refresh:
        if fill_words[position+1] != len(clips):
            raise UnsupportedDevice('Graphics UI initializer scissor records/count disagree')
        expected_fill += fill_words[position:position+2]
        position += 2
    elif clips:
        raise UnsupportedDevice('Graphics UI initializer scissor records require the real count write')
    mode_refresh = fill_words[position] == 0x40010115
    if mode_refresh:
        if fill_words[position+1] not in (0, 1):
            raise UnsupportedDevice('Graphics UI initializer scissor enable is unsupported')
        expected_fill += fill_words[position:position+2]
    expected_fill += FOOTER
    if list(fill_words) != expected_fill + [0]*(-len(expected_fill) % 16):
        raise UnsupportedDevice('Graphics UI initializer has unknown trailing state or incomplete footer')
    return dict(operation='initialize', first_global=False, width=width, height=height,
                frame_bytes=width*height*4, path_mode=mode, order=order,
                matrix_refresh=matrix_refresh, scissor_count_refresh=count_refresh,
                scissor_enable_refresh=mode_refresh, refreshed_scissors=clips,
                pixel_effect='None: validated repeated initializer/dirty state branches contain no geometry or draw'), {
                    'path': path_bank, 'fill': fill_bank}


def _ui_batch(path_words, fill_words, *, initial_banks, ram_bytes, max_bytes):
    path_draws, path_bank = _ui_stream(path_words, 'path', initial_banks.get('path'))
    fill_draws, fill_bank = _ui_stream(fill_words, 'fill', initial_banks.get('fill'))
    if not path_draws or len(path_draws) != len(fill_draws) or len(path_draws) > 1024:
        raise UnsupportedDevice('Graphics UI requires a bounded paired path/fill draw batch')
    draws, targets, texture_ranges, scratch_ranges, texture_shapes = [], {}, [], [], set()
    for path, fill in zip(path_draws, fill_draws):
        p, f = path['registers'], fill['registers']
        if path['sequence'] != fill['sequence']:
            raise UnsupportedDevice('Graphics UI path/fill synchronization does not match')
        required_p = {0x60, 0x7b, 0x7c, 0x80, 0x81, 0x8b, 0x8c, 0x8e} | set(range(0x84, 0x89))
        required_f = {0xa0, 0xa2, 0x115, 0x140, 0x184, 0x185, 0x1a0, 0x1a1,
                      0x1a2, 0x1a3, 0x1a4, 0x1a5, 0x1a7, 0x1aa} | set(range(0xa4, 0xa9))
        if not required_p.issubset(p) or not required_f.issubset(f):
            raise UnsupportedDevice('Graphics UI draw has incomplete framebuffer/path state')
        width, height = f[0x1a3] & 0xffff, f[0x1a3] >> 16
        frame_bytes = width * height * 4
        if (not width or not height or width & 7 or height & 7 or
                width * height > 0x100000 or frame_bytes > max_bytes or
                p[0x7b] != width << 16 | height):
            raise UnsupportedDevice('Graphics UI framebuffer dimensions exceed supported tiled bounds')
        if f[0x1a2] != 0 or f[0x1a1] != 0 or f[0x1aa] != 0 or f[0x1a7] not in (0x100, 0x101):
            raise UnsupportedDevice('Graphics UI supports only unmasked format0 RGBA targets')
        destination = _ui_ram(f[0x1a4], frame_bytes, ram_bytes, 'framebuffer')
        target = (destination, width, height, f[0x1a7] & 255)
        if destination in targets and targets[destination] != target:
            raise UnsupportedDevice('Graphics UI changes dimensions/order of a batch target')
        targets[destination] = target
        style, sample_mode = p[0x60], p[0x60] & 255
        if sample_mode > 2 or style >> 8 & 255 not in (0, 1) or style >> 24 not in (0, 1):
            raise UnsupportedDevice('Graphics UI unknown path/sample mode')
        samples = 1 << sample_mode
        padded_height = height + ((height & 8) if samples == 1 else 0)
        if (p[0x80] != padded_height * samples << 16 or p[0x8e] != padded_height * samples << 16 or
                p[0x7c] != 1 or p[0x8c] != 1 or p[0x81] != 0x1000101 or
                f[0xa0] & 0xffff not in (sample_mode, sample_mode | 0x10) or
                f[0xa0] >> 16 != padded_height * samples or f[0xa2] != width * samples - 1):
            raise UnsupportedDevice('Graphics UI raster/scratch dimensions or draw controls disagree')
        stride = padded_height * samples * 16
        scratch = p[0x84]
        if (any(p[0x84 + index] != scratch + index * stride or
                f[0xa4 + index] != p[0x84 + index] for index in range(5)) or
                p[0x8b] != scratch + stride * 5 - 8):
            raise UnsupportedDevice('Graphics UI path/fill scratch banks do not match')
        scratch_ranges.append((_ui_ram(scratch, stride * 5, ram_bytes, 'scratch'), stride * 5))
        matrix = _ui_affine(p, 0x40, allow_singular=bool(style & 0x100))
        paint, gradient = f[0x140] & 255, bool(f[0x140] & 0x400)
        if (f[0x140] & ~0x400) >> 8 not in (0x500, 0x2100) or paint not in (0, 2) or paint and gradient:
            raise UnsupportedDevice('Graphics UI unsupported paint/mask controls')
        expected_flags = 0 if gradient else (8 if paint == 0 else 10)
        if f[0x184] not in (expected_flags, expected_flags | 0x1000000):
            raise UnsupportedDevice('Graphics UI unsupported gamma/mask/filter paint controls')
        if f[0x184] & 0x1000000 and any(f.get(index) != value for index, value in
                ((0x180, 0x800080), (0x181, 0x800080), (0x182, 0), (0x183, 0))):
            raise UnsupportedDevice('Graphics UI nonidentity color transform is not supported')
        if any(f.get(index) != value for index, value in ((0x190, 0), (0x1c0, 0),
                (0x14d, 0), (0x144, 0), (0x154, 0), (0x187, 0), (0x178, 1),
                (0x147, 1), (0x157, 1))):
            raise UnsupportedDevice('Graphics UI requires known inactive mask/gamma and sampler controls')
        if not gradient and _UI_RAMP_CURSOR not in f and f.get(0x16c) != 0x40001:
            raise UnsupportedDevice('Graphics UI requires the known inactive ramp/filter initialization')
        blend = f[0x1a0] >> 16 & 255
        if f[0x1a0] != blend << 16 or blend not in (0, 1) or f[0x1a5] != int(blend != 0):
            raise UnsupportedDevice('Graphics UI supports only SRC and SRC_OVER blend controls')
        if (f[0x115] & ~1 not in (0, 0x10000, 0x1000000) or
                bool(f[0x115] & 0x1000000) != bool(paint) or bool(f[0x115] & 0x10000) != gradient):
            raise UnsupportedDevice('Graphics UI unknown scissor/image mode')
        clips = []
        if f[0x115] & 1:
            count = f.get(0x100)
            if count is None or not 0 <= count <= 16:
                raise UnsupportedDevice('Graphics UI scissor count exceeds the declared bound')
            for index in range(count):
                if not {0xc0 + 2*index, 0xc1 + 2*index}.issubset(f):
                    raise UnsupportedDevice('Graphics UI scissor record is incomplete')
                horizontal, vertical = f[0xc0 + 2*index], f[0xc1 + 2*index]
                x0, y0 = horizontal & 0xffff, vertical & 0xffff
                x1, y1 = (horizontal >> 16) + 1, (vertical >> 16) + 1
                if x0 >= x1 or y0 >= y1:
                    raise UnsupportedDevice('Graphics UI scissor has reversed endpoints')
                clips.append((x0, y0, min(x1, width), min(y1, height)))
        else:
            clips = [(0, 0, width, height)]
        draw = dict(target=target, sequence=path['sequence'], matrix=matrix,
                    geometry=path['geometry'], samples=samples, clips=clips,
                    nonzero=bool(f[0xa0] & 0x10), blend=blend,
                    color=tuple((f[0x185] >> shift) & 255 for shift in (0, 8, 16, 24)),
                    stroke=not bool(style & 0x100), texture=None,
                    gradient=_ui_gradient(f) if gradient else None)
        if draw['stroke']:
            radius = _ui_float(p.get(0x69, 0))
            cap, join = style >> 18 & 3, style >> 16 & 3
            if not 0 < radius <= 256 or cap > 2 or join > 2 or p.get(0x6a, 0) or style & 0xf00000:
                raise UnsupportedDevice('Graphics UI unsupported dashed/stroke cap/join/radius state')
            draw.update(radius=radius, cap=cap, join=join, device_radius=bool(style & 0x1000000))
            if radius * (1 if draw['device_radius'] else
                         max(math.hypot(matrix[0], matrix[3]), math.hypot(matrix[1], matrix[4]))) > 256:
                raise UnsupportedDevice('Graphics UI transformed stroke radius exceeds the declared bound')
        if paint:
            needed = {0x143, 0x153, 0x152, 0x155, 0x156, 0x117, 0x118}
            if not needed.issubset(f) or f[0x156] != 0 or f[0x143] != 6 or f[0x153] != 6 or draw['stroke']:
                raise UnsupportedDevice('Graphics UI supports only the bounded format0 image family')
            texture_width, texture_height = f[0x152] >> 16, f[0x152] & 0xffff
            if (not texture_width or not texture_height or texture_width & 7 or texture_height & 7 or
                    texture_width * texture_height > 0x100000 or
                    f[0x117] >> 24 > 20 or f[0x118] >> 24 > 20 or
                    1 << (f[0x117] >> 24) != texture_width or
                    1 << (f[0x118] >> 24) != texture_height):
                raise UnsupportedDevice('Graphics UI texture dimensions/stride/exponents disagree')
            valid = (_ui_float(f[0x117] & 0xffffff) * texture_width,
                     _ui_float(f[0x118] & 0xffffff) * texture_height)
            if any(int(value) != value or value <= 0 for value in valid) or valid[0] > texture_width or valid[1] > texture_height:
                raise UnsupportedDevice('Graphics UI texture extent is unresolved')
            source = _ui_ram(f[0x155], texture_width * texture_height * 4, ram_bytes, 'texture')
            texture_ranges.append((source, texture_width * texture_height * 4))
            texture_shapes.add((source, texture_width, texture_height))
            draw['texture'] = dict(source=source, width=texture_width, height=texture_height,
                                   valid_width=int(valid[0]), valid_height=int(valid[1]),
                                   matrix=_ui_affine(f, 0x10c, row_major=True))
        draws.append(draw)
    target_ranges = [(key, target[1] * target[2] * 4) for key, target in targets.items()]
    def overlap(a, b):
        return a[0] < b[0] + b[1] and b[0] < a[0] + a[1]
    if any(overlap(a, b) for a in target_ranges for b in scratch_ranges + texture_ranges):
        raise UnsupportedDevice('Graphics UI framebuffer overlaps scratch or sampled texture')
    if any(overlap(a, b) for index, a in enumerate(target_ranges) for b in target_ranges[index+1:]):
        raise UnsupportedDevice('Graphics UI partially overlapping batch targets are unproved')
    if sum(length for _, length in target_ranges) + sum(w*h*4 for _, w, h in texture_shapes) > max_bytes:
        raise UnsupportedDevice('Graphics UI aggregate destination/texture snapshot exceeds the byte bound')
    return draws, {'path': path_bank, 'fill': fill_bank}


def _ui_transform(point, matrix):
    x, y = point
    a, b, c, d, e, f = matrix
    result = (a*x + b*y + c, d*x + e*y + f)
    if any(not math.isfinite(value) or abs(value) > 2**24 for value in result):
        raise UnsupportedDevice('Graphics UI transformed geometry exceeds the declared bound')
    return result


def _ui_curve(points, tolerance):
    """Bounded adaptive de Casteljau subdivision (declared software law)."""
    output, pending = [], [(points, 0)]
    while pending:
        points, depth = pending.pop()
        start, end = points[0], points[-1]
        dx, dy = end[0] - start[0], end[1] - start[1]
        denominator = math.hypot(dx, dy)
        if denominator:
            def distance(point):
                t = min(1, max(0, ((point[0]-start[0])*dx + (point[1]-start[1])*dy) / denominator**2))
                return math.dist(point, (start[0]+t*dx, start[1]+t*dy))
            error = max(distance(point) for point in points[1:-1])
        else:
            error = max(math.dist(start, p) for p in points[1:-1])
        if error <= tolerance:
            output.append(end)
        elif depth == 14:
            raise UnsupportedDevice('Graphics UI curve subdivision exceeds the declared depth')
        else:
            layers = [list(points)]
            while len(layers[-1]) > 1:
                layers.append([((a[0]+b[0])/2, (a[1]+b[1])/2)
                               for a, b in zip(layers[-1], layers[-1][1:])])
            left = tuple(layer[0] for layer in layers)
            right = tuple(layer[-1] for layer in reversed(layers))
            pending.extend(((right, depth + 1), (left, depth + 1)))
        if len(output) + len(pending) > 65536:
            raise UnsupportedDevice('Graphics UI curve flattening exceeds the declared edge bound')
    return output


def _ui_paths(draw):
    """Recover the path's centerline, keeping extrusion records separate.

    2A/2C/32/33 are the driver's tangent/cap/join records, not ordinary path
    vertices. The explicit profile reconstructs their stroke from the original
    28/29/30/31 centerline and style state. That pixel law is unmeasured; it
    does not claim to execute the vendor's extrusion engine exactly.
    """
    matrix = draw['matrix']
    object_stroke = draw['stroke'] and not draw['device_radius']
    scale = max(math.hypot(matrix[0], matrix[3]), math.hypot(matrix[1], matrix[4]), 1)
    tolerance = 0.125 / scale if object_stroke else 0.125
    transform = (lambda point: point) if object_stroke else lambda point: _ui_transform(point, matrix)
    paths, current, closed, edges = [], [], False, 0
    def flush():
        nonlocal current, closed
        if len(current) > 1:
            paths.append((current, closed or current[0] == current[-1]))
        current, closed = [], False
    for code, flags, values in draw['geometry']:
        points = [transform(pair) for pair in zip(values[::2], values[1::2])]
        if code == 0x28:
            flush()
            current = [points[0]]
        elif code in (0x29, 0x30, 0x31):
            if not current:
                raise UnsupportedDevice('Graphics UI line/curve has no preceding move')
            if code == 0x29:
                added = points
            else:
                added = _ui_curve(tuple([current[-1]] + points), tolerance)
            current += added
            edges += len(added)
        elif not draw['stroke']:
            raise UnsupportedDevice('Graphics UI extrusion metadata is invalid in a fill path')
        elif code == 0x2b:
            if not current:
                raise UnsupportedDevice('Graphics UI stroke close has no centerline')
            closed = True
            if current[-1] != current[0]:
                current.append(current[0])
            flush()
        elif code == 0x2a and flags & 2:
            flush()
        if edges > 65536:
            raise UnsupportedDevice('Graphics UI batch path exceeds the declared flattened edge bound')
    flush()
    if not paths:
        raise UnsupportedDevice('Graphics UI draw has no supported fill edges/stroke centerline')
    return paths


def _ui_stroke(paths, draw):
    """Segment rectangles with declared round/bevel/miter joins and caps.

    Radius69 is already in device units when path60 byte3 is1. Otherwise the
    affine transform acts on an object-space circular pen. Miter limit4,
    0.125px curve tolerance, and polygonal circles are software assumptions.
    Dashed, projective and nonidentity color-transform jobs reject above.
    """
    polygons, radius = [], draw['radius']
    transform = (lambda p: p) if draw['device_radius'] else lambda p: _ui_transform(p, draw['matrix'])
    def polygon(points):
        points = [transform(p) for p in points]
        area = sum(a[0]*b[1] - b[0]*a[1] for a, b in zip(points, points[1:] + points[:1]))
        if area < 0:
            points.reverse()
        polygons.append(points + points[:1])
    def circle(center):
        scale = 1 if draw['device_radius'] else max(math.hypot(draw['matrix'][0], draw['matrix'][3]),
                                                   math.hypot(draw['matrix'][1], draw['matrix'][4]))
        count = max(12, min(256, math.ceil(math.pi * math.sqrt(max(radius * scale, 1) / 0.125))))
        polygon([(center[0] + radius*math.cos(2*math.pi*i/count),
                  center[1] + radius*math.sin(2*math.pi*i/count)) for i in range(count)])
    for points, closed in paths:
        points = [p for index, p in enumerate(points) if index == 0 or p != points[index-1]]
        if len(points) < 2:
            continue
        if closed and points[-1] != points[0]:
            points.append(points[0])
        tangents = []
        for start, end in zip(points, points[1:]):
            length = math.dist(start, end)
            if length == 0:
                continue
            tx, ty = (end[0]-start[0])/length, (end[1]-start[1])/length
            nx, ny = -ty*radius, tx*radius
            tangents.append((tx, ty))
            polygon([(start[0]+nx, start[1]+ny), (end[0]+nx, end[1]+ny),
                     (end[0]-nx, end[1]-ny), (start[0]-nx, start[1]-ny)])
        vertices = range(len(points)-1) if closed else range(1, len(points)-1)
        for index in vertices:
            vertex = points[index]
            if draw['join'] == 1:
                circle(vertex)
                continue
            before, after = tangents[index-1], tangents[index % len(tangents)]
            cross = before[0]*after[1] - before[1]*after[0]
            if abs(cross) < 1e-12:
                continue
            sign = -1 if cross > 0 else 1
            first = (vertex[0]-before[1]*radius*sign, vertex[1]+before[0]*radius*sign)
            last = (vertex[0]-after[1]*radius*sign, vertex[1]+after[0]*radius*sign)
            join = [vertex, first, last]
            if draw['join'] == 0:
                delta = (last[0]-first[0], last[1]-first[1])
                amount = (delta[0]*after[1]-delta[1]*after[0])/cross
                miter = (first[0]+amount*before[0], first[1]+amount*before[1])
                if math.dist(vertex, miter) <= 4*radius:
                    join = [vertex, first, miter, last]
            polygon(join)
        if not closed:
            for point, tangent, direction in ((points[0], tangents[0], -1), (points[-1], tangents[-1], 1)):
                if draw['cap'] == 1:
                    circle(point)
                elif draw['cap'] == 2:
                    tx, ty = tangent[0]*radius*direction, tangent[1]*radius*direction
                    nx, ny = -tangent[1]*radius, tangent[0]*radius
                    polygon([(point[0]+nx, point[1]+ny), (point[0]+nx+tx, point[1]+ny+ty),
                             (point[0]-nx+tx, point[1]-ny+ty), (point[0]-nx, point[1]-ny)])
    return polygons


def _ui_coverage(paths, draw):
    """Declared regular1/2/4 squared sample-grid winding coverage."""
    import numpy as np
    width, height = draw['target'][1:3]
    a, b, _, d, e, _ = draw['matrix']
    if not draw['stroke'] and a*e - b*d == 0:
        # B90EF4/B6DF2C emit singular matrices without a determinant gate.
        # The observed menu batch scales two filled rectangles to height0.
        # A finite affine collapse maps all fill geometry to a line/point;
        # validate geometry normally but give it exactly zero area coverage.
        return (0, 0, np.zeros((0, 0), dtype=np.float64))
    if draw['stroke']:
        paths = [(polygon, True) for polygon in _ui_stroke(paths, draw)]
    # Stock fill construction emits its own close edges. Move commands split
    # edge runs; adding a synthetic closing edge would alter emitted geometry.
    edges = [(a, b) for points, _ in paths for a, b in zip(points, points[1:]) if a != b and a[1] != b[1]]
    if not edges:
        return (0, 0, np.zeros((0, 0), dtype=np.float64))
    nonzero = draw['stroke'] or draw['nonzero']
    sweep = {}
    for a, b in edges:
        direction = 1 if b[1] > a[1] else -1
        for y, sign in ((min(a[1], b[1]), 1), (max(a[1], b[1]), -1)):
            count, winding = sweep.get(y, (0, 0))
            sweep[y] = count + sign, winding + direction*sign
    count = winding = 0
    for y in sorted(sweep):
        extra_count, extra_winding = sweep[y]
        count, winding = count + extra_count, winding + extra_winding
        if winding != 0 if nonzero else count & 1:
            raise UnsupportedDevice('Graphics UI emitted fill edges have an unbalanced scanline')
    if not draw['clips']:
        return (0, 0, np.zeros((0, 0), dtype=np.float64))
    points = [p for edge in edges for p in edge]
    x0, y0 = max(0, math.floor(min(p[0] for p in points))), max(0, math.floor(min(p[1] for p in points)))
    x1, y1 = min(width, math.ceil(max(p[0] for p in points))), min(height, math.ceil(max(p[1] for p in points)))
    if x0 >= x1 or y0 >= y1:
        return (0, 0, np.zeros((0, 0), dtype=np.float64))
    scale = draw['samples']
    sample_width, sample_height = (x1-x0)*scale, (y1-y0)*scale
    fill = _native_fill_counts()
    if fill is not None:
        # Same rows and arithmetic natively: inside-sample counts per pixel;
        # count/scale**2 is the reference raster's mean exactly.
        try:
            counts = fill([float(value) for edge in edges for point in edge for value in point],
                          bool(nonzero), x0, y0, scale, x1-x0, y1-y0)
        except ValueError as error:
            raise UnsupportedDevice(str(error)) from None
        coverage = np.frombuffer(counts, dtype=np.uint8).reshape(y1-y0, x1-x0) / (scale*scale)
    else:
        coverage = _numpy_fill(edges, nonzero, x0, y0, scale, sample_width, sample_height)
    clipped = np.zeros_like(coverage, dtype=np.bool_)
    for left, bottom, right, top in draw['clips']:
        left, bottom, right, top = max(left, x0), max(bottom, y0), min(right, x1), min(top, y1)
        if left < right and bottom < top:
            clipped[bottom-y0:top-y0, left-x0:right-x0] = True
    coverage *= clipped
    return x0, y0, coverage


_NATIVE_FILL = False


def _native_fill_counts():
    """The native sample-row fill (x70_native ui_fill_counts), or None."""
    global _NATIVE_FILL
    if _NATIVE_FILL is False:
        try:
            from x70_native._native import ui_fill_counts as _NATIVE_FILL
        except (ImportError, AttributeError):
            _NATIVE_FILL = None
    return _NATIVE_FILL


def _numpy_fill(edges, nonzero, x0, y0, scale, sample_width, sample_height):
    """NumPy form of the reference scanline fill: per-pixel sample coverage."""
    import numpy as np
    x1, y1 = x0 + sample_width//scale, y0 + sample_height//scale
    # All sample rows at once, with the per-row arithmetic in the same order:
    # each row sorts its crossings by (x, direction) and fills between
    # consecutive crossings whose running winding/parity is inside.
    ax, ay, bx, by = (np.array([edge[i][j] for edge in edges], dtype=np.float64)
                      for i, j in ((0, 0), (0, 1), (1, 0), (1, 1)))
    y = (y0 + (np.arange(sample_height) + .5)/scale)[:, None]
    active = (np.minimum(ay, by) <= y) & (y < np.maximum(ay, by))
    xs = np.where(active, ax + (y-ay)*(bx-ax)/(by-ay), np.inf)
    directions = np.where(active, np.where(by > ay, 1, -1), 0)
    order = np.lexsort((directions, xs), axis=1)
    xs = np.take_along_axis(xs, order, axis=1)
    directions = np.take_along_axis(directions, order, axis=1)
    active = np.take_along_axis(active, order, axis=1)
    levels = np.cumsum(directions if nonzero else active, axis=1)
    inside = levels != 0 if nonzero else levels & 1 == 1
    if np.any(inside[np.arange(sample_height), active.sum(axis=1)-1] & active.any(axis=1)):
        raise UnsupportedDevice('Graphics UI emitted fill edges have an unbalanced scanline')
    spans = inside[:, :-1] & active[:, 1:]
    rows, columns = np.nonzero(spans)
    lefts = np.maximum(0, np.ceil((xs[rows, columns]-x0)*scale-.5).astype(np.int64))
    rights = np.minimum(sample_width, np.ceil((xs[rows, columns+1]-x0)*scale-.5).astype(np.int64))
    keep = lefts < rights
    marks = np.zeros((sample_height, sample_width+1), dtype=np.int64)
    np.add.at(marks, (rows[keep], lefts[keep]), 1)
    np.add.at(marks, (rows[keep], rights[keep]), -1)
    raster = np.cumsum(marks, axis=1)[:, :sample_width] > 0
    return raster.reshape(y1-y0, scale, x1-x0, scale).mean(axis=(1, 3))


def _numpy_composite(region, coverage, source, blend):
    """Reference blend of one draw over its region (in place)."""
    import numpy as np
    destination = region.astype(np.float64)/255
    alpha = source[..., 3] * coverage
    retained = 1-alpha if blend else 1-coverage
    out_alpha = alpha + destination[..., 3]*retained
    numerator = source[..., :3]*alpha[..., None] + destination[..., :3]*destination[..., 3:4]*retained[..., None]
    out_rgb = np.divide(numerator, out_alpha[..., None], out=np.zeros_like(numerator), where=out_alpha[..., None] != 0)
    result = np.concatenate((out_rgb, out_alpha[..., None]), axis=-1)
    converted = np.floor(np.clip(result, 0, 1)*255+.5).astype(np.uint8)
    # Zero-coverage pixels preserve all bytes, including RGB under alpha0.
    active = (coverage != 0) & (alpha != 0 if blend else True)
    region[active] = converted[active]


_NATIVE_COMPOSITE = False


def _native_composite():
    """The native blend (x70_native ui_composite), or None."""
    global _NATIVE_COMPOSITE
    if _NATIVE_COMPOSITE is False:
        try:
            from x70_native._native import ui_composite as _NATIVE_COMPOSITE
        except (ImportError, AttributeError):
            _NATIVE_COMPOSITE = None
    return _NATIVE_COMPOSITE


# Coverage of recent draws, keyed on every draw field except the paint ones
# (_UI_PAINT_FIELDS): _ui_paths and _ui_coverage read only those fields.
_NATIVE_COMPOSITE_TILED = False


def _native_composite_tiled():
    """The native in-place tiled blend (x70_native ui_composite_tiled), or None."""
    global _NATIVE_COMPOSITE_TILED
    if _NATIVE_COMPOSITE_TILED is False:
        try:
            from x70_native._native import ui_composite_tiled as _NATIVE_COMPOSITE_TILED
        except (ImportError, AttributeError):
            _NATIVE_COMPOSITE_TILED = None
    return _NATIVE_COMPOSITE_TILED


_ui_coverage_memo = OrderedDict()
_ui_coverage_bytes = [0]
_UI_COVERAGE_BUDGET = 256 << 20
_UI_PAINT_FIELDS = frozenset({'color', 'gradient', 'texture', 'blend', 'sequence'})


def _ui_draw_coverage(draw):
    """(x0, y0, coverage) of one draw, reused for an identical draw geometry."""
    key = pickle.dumps(sorted((name, value) for name, value in draw.items() if name not in _UI_PAINT_FIELDS), 5)
    cached = _ui_coverage_memo.get(key)
    if cached is not None:
        _ui_coverage_memo.move_to_end(key)
        return cached
    x0, y0, coverage = _ui_coverage(_ui_paths(draw), draw)
    coverage.flags.writeable = False
    result = _ui_coverage_memo[key] = (x0, y0, coverage)
    _ui_coverage_bytes[0] += coverage.nbytes
    while _ui_coverage_bytes[0] > _UI_COVERAGE_BUDGET and len(_ui_coverage_memo) > 1:
        _, (_, _, evicted) = _ui_coverage_memo.popitem(last=False)
        _ui_coverage_bytes[0] -= evicted.nbytes
    return result


# Recent _ui_render batches: (draw repr, target hashes, texture hashes) ->
# (output, touched). Outputs are immutable bytes; process-local, not device state.
_ui_render_memo = OrderedDict()


@functools.lru_cache(maxsize=16)
def _ui_morton(width, height):
    import numpy as np
    y, x = np.indices((height, width), dtype=np.int64)
    local = ((x & 1) | (y & 1) << 1 | (x & 2) << 1 | (y & 2) << 2 |
             (x & 4) << 2 | (y & 4) << 3)
    result = (((y >> 3)*(width >> 3) + (x >> 3))*64 + local)
    result.flags.writeable = False  # shared by the cache
    return result





def _ui_render(draws, *, read_bytes, cpu_id):
    """Snapshot all inputs, render in list order, then return atomic DMA bytes.

    Explicit assumptions: straight RGBA8 SRC/SRC_OVER, nearest image lookup,
    transparent image border, regular-grid AA, software centerline stroking and
    atomic whole-batch completion. No physical GPU pixel or timing claim.
    """
    import numpy as np
    targets, textures, source_hashes, touched = {}, {}, {}, 0
    texture_hashes = []
    def read(address, length):
        payload = bytes(read_bytes(address, length, cpu_id=cpu_id))
        if len(payload) != length:
            raise UnsupportedDevice('Graphics UI destination/texture snapshot is incomplete')
        return payload
    # The batch is a pure function of the draw list and the snapshotted
    # target/texture bytes: an identical batch reuses its earlier output.
    payloads = {}
    for draw in draws:
        address, width, height, order = draw['target']
        if address not in payloads:
            payloads[address] = read(address, width*height*4)
            source_hashes[hex(address)] = hashlib.sha256(payloads[address]).hexdigest()
        texture = draw['texture']
        if texture:
            key = (texture['source'], texture['width'], texture['height'])
            if key not in payloads:
                payloads[key] = read(key[0], key[1]*key[2]*4)
                texture_hashes.append((key, hashlib.sha256(payloads[key]).hexdigest()))
    memo_key = (pickle.dumps(draws, 5), tuple(source_hashes.items()), tuple(texture_hashes))
    cached = _ui_render_memo.get(memo_key)
    if cached is not None:
        _ui_render_memo.move_to_end(memo_key)
        output, touched = cached
        return list(output), touched, dict(source_hashes)
    # Targets stay in their tiled memory layout: each draw gathers only the
    # pixels it covers (row-major via the Morton index grid, RGBA channel
    # order) and scatters them back, the same bytes as detiling the whole
    # target, drawing and retiling it.
    for draw in draws:
        address, width, height, order = draw['target']
        if address not in targets:
            payload = payloads[address]
            targets[address] = (bytearray(payload), width, height, order)
            source_hashes[hex(address)] = hashlib.sha256(payload).hexdigest()
        texture = draw['texture']
        if texture:
            key = (texture['source'], texture['width'], texture['height'])
            if key not in textures:
                payload = payloads[key]
                pixels = np.frombuffer(payload, dtype=np.uint8).reshape(-1, 4)[_ui_morton(key[1], key[2])]
                textures[key] = pixels[..., [3, 2, 1, 0]].copy()
    for draw in draws:
        x0, y0, coverage = _ui_draw_coverage(draw)
        if not coverage.size or not np.any(coverage):
            continue
        tiled, width, height, order = targets[draw['target'][0]]
        source = np.asarray(draw['color'], dtype=np.float64)/255
        if draw.get('gradient'):
            source = _ui_gradient_colors(draw['gradient'], coverage.shape, x0, y0)
        texture = draw['texture']
        if texture:
            yy, xx = np.indices(coverage.shape, dtype=np.float64)
            xx += x0 + .5
            yy += y0 + .5
            a, b, c, d, e, f = texture['matrix']
            u, v = np.floor(a*xx+b*yy+c).astype(np.int64), np.floor(d*xx+e*yy+f).astype(np.int64)
            valid = (u >= 0) & (v >= 0) & (u < texture['valid_width']) & (v < texture['valid_height'])
            image = textures[(texture['source'], texture['width'], texture['height'])]
            source = image[np.clip(v, 0, texture['height']-1), np.clip(u, 0, texture['width']-1)].astype(np.float64)/255
            source *= valid[..., None]
        composite = _native_composite_tiled()
        if composite is not None:
            # Same per-pixel f64 arithmetic natively, in the tiled target
            # (x70_native ui_composite_tiled).
            composite(tiled, width, x0, y0, coverage.shape[1], coverage.shape[0], order,
                      np.ascontiguousarray(coverage, dtype=np.float64).tobytes(),
                      np.ascontiguousarray(source, dtype=np.float64).tobytes(), bool(draw['blend']))
        else:
            cells = _ui_morton(width, height)[y0:y0+coverage.shape[0], x0:x0+coverage.shape[1]]
            channels = [3, 2, 1, 0] if order == 0 else [2, 1, 0, 3]
            pixels = np.frombuffer(tiled, dtype=np.uint8).reshape(-1, 4)
            region = pixels[cells][..., channels]
            _numpy_composite(region, coverage, source, draw['blend'])
            pixels[cells] = region[..., channels]
        touched += int(np.count_nonzero(coverage))*4
    output = [(address, bytes(tiled)) for address, (tiled, _, _, _) in targets.items()]
    _ui_render_memo[memo_key] = (tuple(output), touched)
    while len(_ui_render_memo) > 16:
        _ui_render_memo.popitem(last=False)
    return output, touched, source_hashes


# Integer words of the latest capture's streams, by stream identity, kept off
# the device (its __dict__ is checkpointed): see _capture_command.
_captured_words = {}


def _capture_hex_words(capture):
    """Give deferred streams their hex 'words' copy (a rejection is recorded)."""
    for stream in capture.get('streams', {}).values():
        captured = _captured_words.get(id(stream))
        if (captured is not None and captured[0] is stream and 'words' not in stream
                and 'word_count' not in stream):
            stream['words'] = [hex(word) for word in captured[1]]


class GraphicsAccelerator:
    BASE = 0xfff78000
    SIZE = 0x5000
    IRQ = 0x3c
    FILL_STATUS = 1 << 26
    FILL_CONTROL = 0x0f100201
    COPY_STATUS = 1 << 30
    COPY_PROFILE = 'stock-vg-32bpp-fallback'
    CLEAR_STATUS = 1 << 31
    CLEAR_PROFILE = CLEAR_PROFILE
    UI_PROFILE = UI_PROFILE
    UI_CAPTURE_BYTES = 0x80000
    # Values actually written by the unchanged initializer0xbcdcf8.
    INIT_VALUES = {
        0x1800: {0}, 0x1880: {0x12345678},
        0x18c0: {0xfffffff0, 0xffffffff}, 0xc1c: {0x8000},
        0x10c0: {0xffffffff}, 0x10c4: {0xffffffff, 0xfffffff0},
        0x1000: {0}, 0x1020: {0}, 0x10a0: {0x12345678},
        0x1248: {1},
    }
    # Retain actual descriptors until their unsupported engine is triggered.
    COMMAND_REGISTERS = {0x10d4, 0x10dc, 0x10e4, 0x10ec, 0x20e0, 0x20e8}
    COPY_REGISTERS = {0xc00, 0xc04, 0xc08, 0xc0c, 0xc10, 0xc20, 0xc24, 0xc28}
    COMMAND_SETUP = {0x1204: {0, 0x10000}, 0x12b4: {0},
                     0x1234: {0xffff}, 0x128c: {0x10000}}
    TRIGGERS = {0x10d0, 0x10e0, 0x20f0}
    OFFSETS = (set(INIT_VALUES) | COMMAND_REGISTERS | COPY_REGISTERS |
               set(COMMAND_SETUP) | TRIGGERS | {0x10, 0x14, 0x18, 0x1c, 0x34, 0xc18, 0x105c})

    def __init__(self, *, write_bytes, schedule, raise_irq=None, irq_targets=None, record=None,
                 read_bytes=None, ram_bytes=0x08000000,
                 max_fill_bytes=0x04000000, max_copy_bytes=None,
                 copy_profile=None, clear_profile=None, max_clear_bytes=None,
                 max_command_capture_bytes=None, bytes_per_tick=16):
        if not 0 < max_fill_bytes <= ram_bytes <= 0x40000000 or bytes_per_tick < 1:
            raise ValueError("Invalid graphics memory/throughput bounds")
        if max_copy_bytes is None:
            max_copy_bytes = min(0x04000000, ram_bytes)
        if not 0 < max_copy_bytes <= ram_bytes or copy_profile not in (None, self.COPY_PROFILE):
            raise ValueError('Invalid graphics copy profile/bounds')
        if max_command_capture_bytes is None:
            max_command_capture_bytes = self.UI_CAPTURE_BYTES if clear_profile == self.UI_PROFILE else 0x10000
        capture_limit = self.UI_CAPTURE_BYTES if clear_profile == self.UI_PROFILE else 0x10000
        if not 8 <= max_command_capture_bytes <= capture_limit or max_command_capture_bytes & 7:
            raise ValueError('Invalid graphics command capture bound')
        if max_clear_bytes is None:
            max_clear_bytes = min(0x04000000, ram_bytes)
        if not 0 < max_clear_bytes <= ram_bytes or clear_profile not in (None, self.CLEAR_PROFILE, self.UI_PROFILE):
            raise ValueError('Invalid graphics clear profile/bounds')
        self.write_bytes, self.schedule, self.raise_irq = write_bytes, schedule, raise_irq
        self.read_bytes = read_bytes
        self.irq_targets = irq_targets
        self.record = record if record is not None else lambda *args, **kwargs: None
        self.ram_bytes, self.max_fill_bytes = ram_bytes, max_fill_bytes
        self.bytes_per_tick = bytes_per_tick
        self.copy_profile, self.max_copy_bytes = copy_profile, max_copy_bytes
        self.max_command_capture_bytes = max_command_capture_bytes
        self.clear_profile, self.max_clear_bytes = clear_profile, max_clear_bytes
        self._command_pending = None
        self.command_banks = {'path': {}, 'fill': {}}
        self._clear_ack_offsets = set()
        self.regs, self.written = {}, set()
        self.busy = False
        self.jobs_started = self.jobs_completed = self.bytes_completed = 0
        self.irqs_raised = self.acknowledgements = 0
        self.last_job = None

    def contains(self, address, size):
        return self.BASE <= address and address + size <= self.BASE + self.SIZE

    def _offset(self, address, size):
        physical_offset = address - self.BASE
        logical = physical_offset // 2
        if (not self.contains(address, size) or size != 4 or
                physical_offset & 7 or logical not in self.OFFSETS):
            raise UnsupportedDevice(f"Unknown graphics-accelerator register/access {address:#x}/{size}")
        return logical

    def read(self, cpu_id, address, size):
        offset = self._offset(address, size)
        if offset == 0x105c and self.clear_profile is None:
            raise UnsupportedDevice('Graphics command-list post-wait status is unresolved without clear profile')
        return self.regs.get(offset, 0)

    def write(self, cpu_id, address, size, value):
        offset = self._offset(address, size)
        value &= 0xffffffff
        if offset in (0x34, 0x105c):
            raise UnsupportedDevice("Graphics completion status is read-only")
        if offset in self.TRIGGERS:
            self.regs[offset] = value
            self.written.add(offset)
            capture = self._capture_command(cpu_id, offset, value)
            if self.clear_profile is None or offset == 0x20f0:
                raise UnsupportedDevice("Graphics command-list rasterization is not modeled")
            try:
                self._start_commands(cpu_id, offset, value, capture)
            except UnsupportedDevice as error:
                _capture_hex_words(capture)
                capture['rejection'] = str(error)
                self.record('Graphics UI command rejected' if self.clear_profile == self.UI_PROFILE
                            else 'Graphics clear-only command rejected', **capture)
                raise
            return
        if offset == 0xc18:
            if value:
                if self.copy_profile is None:
                    self.regs[offset] = value
                    self.written.add(offset)
                    raise UnsupportedDevice("Graphics image copy/untile is not modeled")
                self._start_copy(cpu_id, value)
                return
            if self.busy:
                raise UnsupportedDevice('Graphics copy abort semantics are not modeled')
            if self.regs.get(0x34, 0) & self.COPY_STATUS:
                self.regs[0x34] &= ~self.COPY_STATUS
                self.acknowledgements += 1
                self.record('Graphics copy status acknowledged', cpu=cpu_id)
        elif offset == 0x1c:
            if value:
                self._start_fill(cpu_id, value)
                return
            if self.busy:
                raise UnsupportedDevice("Graphics fill abort semantics are not modeled")
            if self.regs.get(0x34, 0) & self.FILL_STATUS:
                self.regs[0x34] &= ~self.FILL_STATUS
                self.acknowledgements += 1
                self.record("Graphics fill status acknowledged", cpu=cpu_id)
        elif offset in self.INIT_VALUES:
            if value not in self.INIT_VALUES[offset]:
                raise UnsupportedDevice(f"Unknown graphics initialization {offset:#x}={value:#x}")
            if offset in (0x1800, 0x1020) and self.regs.get(0x34, 0) & self.CLEAR_STATUS:
                if self.busy:
                    raise UnsupportedDevice('Graphics clear abort semantics are not modeled')
                self._clear_ack_offsets.add(offset)
                if self._clear_ack_offsets == {0x1800, 0x1020}:
                    self.regs[0x34] &= ~self.CLEAR_STATUS
                    self.acknowledgements += 1
                    self.record('Graphics clear bit31 acknowledged by both actual ISR stores', cpu=cpu_id)
        elif offset in self.COMMAND_SETUP:
            if value not in self.COMMAND_SETUP[offset]:
                raise UnsupportedDevice(f"Unknown graphics command-list setup {offset:#x}={value:#x}")
        elif offset in {0x10, 0x14, 0x18} and self.busy:
            raise UnsupportedDevice("Graphics fill descriptor changed while busy")
        elif offset in self.COPY_REGISTERS and self.busy:
            raise UnsupportedDevice('Graphics copy descriptor changed while busy')
        elif offset in self.COMMAND_REGISTERS and (self.busy or
                self._command_pending is not None and offset in (0x10d4, 0x10dc)):
            raise UnsupportedDevice('Graphics command-list descriptor changed while pending/busy')
        self.regs[offset] = value
        self.written.add(offset)

    def _capture_command(self, cpu_id, offset, trigger):
        """Retain bounded raw evidence even when the command remains unsupported.

        BCE024 writes word_count>>1 to the two length registers, hence each
        retained count represents eight bytes. The alternate engine's shifts
        are recorded without inventing a byte length or fetching its payload.
        """
        # The UI profile's fill trigger validates or rejects within this call:
        # its streams keep integer words (_captured_words) and get the hex
        # 'words' copy only if rejected (_capture_hex_words); on acceptance
        # the copy was always replaced by 'word_count' before any observer.
        deferred = self.clear_profile == self.UI_PROFILE and offset == 0x10e0
        _captured_words.clear()
        capture = {'operation': 'command-list', 'cpu': cpu_id,
                   'trigger_logical_offset': hex(offset), 'trigger': hex(trigger),
                   'completed': False, 'accepted': False, 'streams': {},
                   'capture_bound_bytes_each': self.max_command_capture_bytes}
        for bank, address_offset, count_offset in (
                ('path', 0x10d4, 0x10dc), ('fill', 0x10e4, 0x10ec)):
            if offset == 0x20f0:
                continue
            stream = {'address_register_logical_offset': hex(address_offset),
                      'count_register_logical_offset': hex(count_offset)}
            capture['streams'][bank] = stream
            if not {address_offset, count_offset}.issubset(self.written):
                stream['capture_error'] = 'descriptor not yet written'
                continue
            address, count = self.regs[address_offset], self.regs[count_offset]
            declared = count * 8
            stream.update(address_register=hex(address), count_register=hex(count),
                          declared_bytes=declared)
            alias = address & 0xc0000000
            physical = address - alias
            if (alias not in (0, 0x40000000) or address & 3 or
                    physical + declared > self.ram_bytes):
                stream['capture_error'] = 'unresolved RAM range/alias/alignment'
                continue
            if self.read_bytes is None:
                stream['capture_error'] = 'no memory reader configured'
                continue
            length = min(declared, self.max_command_capture_bytes)
            try:
                payload = bytes(self.read_bytes(physical, length, cpu_id=cpu_id))
            except Exception as error:
                stream['capture_error'] = type(error).__name__ + ': ' + str(error)
                continue
            words = struct.unpack(f'<{len(payload)//4}I', payload[:len(payload)//4*4])
            stream.update(physical_address=hex(physical), captured_bytes=len(payload),
                          truncated=len(payload) != declared,
                          sha256_captured=hashlib.sha256(payload).hexdigest())
            _captured_words[id(stream)] = (stream, words)
            if not deferred:
                stream['words'] = [hex(word) for word in words]
            if len(payload) != length:
                stream['capture_error'] = 'short memory read'
        if offset == 0x20f0:
            capture['alternate_descriptor_registers'] = {
                hex(n): hex(self.regs[n]) for n in (0x20e0, 0x20e8) if n in self.written}
            capture['capture_error'] = 'alternate address/length semantics are unresolved'
        self.last_job = capture
        # The explicit UI profile retains raw words locally for a rejection,
        # while routine board event history stays compact. This is not a new
        # evidence receipt and avoids duplicating a large list per trigger.
        observed = capture
        if self.clear_profile == self.UI_PROFILE:
            observed = {**capture, 'streams': {name: {key: value for key, value in stream.items()
                                                    if key != 'words'}
                                               for name, stream in capture['streams'].items()}}
        self.record('Graphics command-list payload captured before validation', **observed)
        return capture

    def _command_words(self, capture, bank):
        stream = capture['streams'].get(bank, {})
        captured = _captured_words.get(id(stream))
        if captured is not None and captured[0] is not stream:
            captured = None
        if (stream.get('capture_error') or stream.get('truncated')
                or ('words' not in stream and captured is None)):
            raise UnsupportedDevice(f'Graphics clear-only {bank} requires a complete bounded payload capture')
        return list(captured[1]) if captured is not None else [int(word, 16) for word in stream['words']]

    def _start_commands(self, cpu_id, offset, trigger, capture):
        if self.busy or self.regs.get(0x34, 0) & (self.FILL_STATUS | self.COPY_STATUS | self.CLEAR_STATUS):
            raise UnsupportedDevice('Graphics clear requires idle and acknowledged previous completion')
        if self.read_bytes is None or self.raise_irq is None:
            raise UnsupportedDevice('Graphics clear-only requires memory and IRQ routing')
        if offset == 0x10d0:
            if trigger != 0x10001 or self._command_pending is not None:
                raise UnsupportedDevice('Graphics clear-only path trigger/order is unsupported')
            words = self._command_words(capture, 'path')
            if self.clear_profile == self.UI_PROFILE:
                _ui_stream(words, 'path', self.command_banks['path'])
            else:
                path_descriptor(words, ram_bytes=self.ram_bytes, max_clear_bytes=self.max_clear_bytes)
            self._command_pending = {'cpu': cpu_id, 'path_words': words}
            capture.update(stage='path validated; awaiting separate fill stream', accepted=True)
            return
        if offset != 0x10e0 or trigger != 1 or self._command_pending is None:
            raise UnsupportedDevice('Graphics clear-only fill trigger/order is unsupported')
        if self._command_pending['cpu'] != cpu_id:
            raise UnsupportedDevice('Graphics clear-only cross-CPU stream submission is unproved')
        path_words, fill_words = self._command_words(capture, 'path'), self._command_words(capture, 'fill')
        if path_words != self._command_pending['path_words']:
            raise UnsupportedDevice('Graphics clear-only path memory changed before fill submission')
        descriptor = None
        if self.clear_profile != self.UI_PROFILE or len(path_words) <= 80:
            try:
                descriptor, banks = parse_clear(path_words, fill_words,
                    ram_bytes=self.ram_bytes, max_clear_bytes=self.max_clear_bytes,
                    initial_banks=self.command_banks)
            except UnsupportedDevice:
                if self.clear_profile != self.UI_PROFILE:
                    raise
        if descriptor is None:
            path_jobs, _ = _ui_stream(path_words, 'path', self.command_banks['path'])
            fill_jobs, _ = _ui_stream(fill_words, 'fill', self.command_banks['fill'])
            if not path_jobs and not fill_jobs:
                descriptor, banks = _ui_initialize(path_words, fill_words,
                    initial_banks=self.command_banks, max_bytes=self.max_clear_bytes)
            else:
                draws, banks = _ui_batch(path_words, fill_words,
                    initial_banks=self.command_banks, ram_bytes=self.ram_bytes,
                    max_bytes=self.max_clear_bytes)
        irq_cpu = cpu_id
        if self.irq_targets is not None:
            targets = list(self.irq_targets(self.IRQ))
            if len(targets) != 1 or targets[0] not in (0, 1):
                raise UnsupportedDevice('Graphics clear IRQ has missing or ambiguous firmware-enabled bank')
            irq_cpu = targets[0]
        if descriptor is None:
            self._start_ui_draws(cpu_id, irq_cpu, capture, draws, banks)
            return
        initialization_only = descriptor['operation'] == 'initialize'
        if initialization_only:
            source, payload, changed_bytes = None, None, 0
        else:
            source = bytes(self.read_bytes(descriptor['destination'], descriptor['frame_bytes'], cpu_id=cpu_id))
            if len(source) != descriptor['frame_bytes']:
                raise UnsupportedDevice('Graphics clear-only destination read is incomplete')
            payload = render_clear(source, descriptor)
            x0, y0, x1, y1 = descriptor['rectangle']
            changed_bytes = (x1-x0) * (y1-y0) * 4
        delay = max(2, (changed_bytes + self.bytes_per_tick-1) // self.bytes_per_tick)
        self._command_pending = None
        for bank, registers in banks.items():
            self.command_banks[bank].update(registers)
        self._clear_ack_offsets.clear()
        self.busy = True
        self.jobs_started += 1
        if self.clear_profile == self.UI_PROFILE:
            for stream in capture['streams'].values():
                if 'words' in stream:
                    stream['word_count'] = len(stream.pop('words'))
                elif _captured_words.get(id(stream), (None,))[0] is stream:
                    stream['word_count'] = len(_captured_words[id(stream)][1])
        capture.update(operation=descriptor['operation'], profile=self.clear_profile, accepted=True,
            descriptor={key: value for key, value in descriptor.items() if key != 'pixel_bytes'},
            changed_bytes=changed_bytes, irq_cpu=irq_cpu, latency_ticks=delay,
            source_sha256=None if source is None else hashlib.sha256(source).hexdigest(),
            pixel_semantics='No pixel writes: exact initializer/state/flush without geometry' if initialization_only else 'inferred GPU profile matching stock writer/software layout; no hardware draw validation',
            completion_semantics='cross-stream fence/draw and bit31 completion are explicit software-derived assumptions')
        self.record('Graphics clear-only started', **capture)

        def complete():
            if payload is not None:
                self.write_bytes(descriptor['destination'], payload, cpu_id=cpu_id)
            self.busy = False
            self.jobs_completed += 1
            self.bytes_completed += changed_bytes
            capture.update(completed=True, sha256=None if payload is None else hashlib.sha256(payload).hexdigest())
            self.regs[0x34] = self.regs.get(0x34, 0) | self.CLEAR_STATUS
            self.record('Graphics clear-only completed', **capture)
            self.irqs_raised += 1
            self.raise_irq(irq_cpu, self.IRQ)

        self.schedule(delay, complete)

    def _start_ui_draws(self, cpu_id, irq_cpu, capture, draws, banks):
        # Render/preflight can fail; no RAM write, register-bank mutation,
        # completion, IRQ or scheduled callback occurs until it all succeeds.
        output, touched, source_hashes = _ui_render(draws, read_bytes=self.read_bytes, cpu_id=cpu_id)
        dma_bytes = sum(len(payload) for _, payload in output)
        delay = max(2, (dma_bytes + self.bytes_per_tick-1) // self.bytes_per_tick)
        self._command_pending = None
        self.command_banks = banks
        self._clear_ack_offsets.clear()
        self.busy = True
        self.jobs_started += 1
        for stream in capture['streams'].values():
            if 'words' in stream:
                stream['word_count'] = len(stream.pop('words'))
            else:
                stream['word_count'] = len(_captured_words[id(stream)][1])
        capture.update(operation='ui-primitives', profile=self.clear_profile, accepted=True,
            draw_count=len(draws), filled_paths=sum(not draw['stroke'] and draw['texture'] is None for draw in draws),
            stroked_paths=sum(draw['stroke'] for draw in draws), sampled_images=sum(draw['texture'] is not None for draw in draws),
            gradients=sum(draw.get('gradient') is not None for draw in draws),
            destinations=[dict(address=hex(address), bytes=len(payload)) for address, payload in output],
            changed_bytes=dma_bytes, covered_bytes_in_draw_order=touched, irq_cpu=irq_cpu,
            latency_ticks=delay, source_sha256=source_hashes,
            pixel_semantics='declared software RGBA8 SRC/SRC_OVER, affine paths, regular-grid AA, centerline strokes, nearest Morton images, linear ramp interpolation; hardware pixels unmeasured',
            completion_semantics='atomic validated whole batch before bit31/physicalIRQ3c; throughput timing unmeasured')
        self.record('Graphics declared UI batch started', **capture)

        def complete():
            for address, payload in output:
                self.write_bytes(address, payload, cpu_id=cpu_id)
            self.busy = False
            self.jobs_completed += 1
            self.bytes_completed += dma_bytes
            capture.update(completed=True, output_sha256={hex(address): hashlib.sha256(payload).hexdigest()
                                                         for address, payload in output})
            self.regs[0x34] = self.regs.get(0x34, 0) | self.CLEAR_STATUS
            self.record('Graphics declared UI batch completed', **capture)
            self.irqs_raised += 1
            self.raise_irq(irq_cpu, self.IRQ)

        self.schedule(delay, complete)

    def _start_fill(self, cpu_id, control):
        if control != self.FILL_CONTROL:
            raise UnsupportedDevice(f"Unknown graphics fill control {control:#x}")
        if self.busy or self._command_pending is not None or self.regs.get(0x34, 0) & (self.FILL_STATUS | self.COPY_STATUS | self.CLEAR_STATUS):
            raise UnsupportedDevice("Graphics fill requires idle and acknowledged previous completion")
        if not {0x10, 0x14, 0x18}.issubset(self.written):
            raise UnsupportedDevice("Graphics fill descriptor is incomplete")
        if self.raise_irq is None:
            raise UnsupportedDevice("Graphics fill completion IRQ routing is not configured")
        irq_cpu = cpu_id
        if self.irq_targets is not None:
            targets = list(self.irq_targets(self.IRQ))
            if len(targets) != 1 or targets[0] not in (0, 1):
                raise UnsupportedDevice("Graphics fill IRQ has missing or ambiguous firmware-enabled bank")
            irq_cpu = targets[0]
        start, end, word = (self.regs[offset] for offset in (0x10, 0x14, 0x18))
        # Only the real Board DRAM aliases are supported; no truncation of
        # unknown physical address/control bits is applied.
        alias = start & 0xc0000000
        if alias not in (0, 0x40000000) or (end & 0xc0000000) != alias:
            raise UnsupportedDevice("Graphics fill uses an unresolved RAM address/alias")
        destination, endpoint = start - alias, end - alias
        count = endpoint - destination
        if (start & 0x7f or count <= 0 or count & 0xf or
                count > self.max_fill_bytes or endpoint > self.ram_bytes):
            raise UnsupportedDevice("Graphics fill range/alignment exceeds evidenced bounds")
        pattern = word.to_bytes(4, "little")
        self.busy = True
        self.regs[0x1c] = control
        self.written.add(0x1c)
        self.jobs_started += 1
        delay = max(2, (count + self.bytes_per_tick - 1) // self.bytes_per_tick)
        self.last_job = {"cpu": cpu_id, "start_register": hex(start),
                         "end_exclusive_register": hex(end), "destination": hex(destination),
                         "bytes": count, "pattern_word": hex(word),
                         "pattern_little_endian": pattern.hex(), "control": hex(control),
                         "irq_cpu": irq_cpu,
                         "irq_routing": "firmware-enabled bank" if self.irq_targets is not None else "submitting CPU fixture",
                         "latency_ticks": delay, "completed": False}
        job = self.last_job
        self.record("Graphics fill started", **job)

        def complete():
            payload = pattern * (count // 4)
            # DMA/bus memory writes complete before the visible status/IRQ.
            self.write_bytes(destination, payload, cpu_id=cpu_id)
            self.busy = False
            self.jobs_completed += 1
            self.bytes_completed += count
            job["completed"] = True
            job["sha256"] = hashlib.sha256(payload).hexdigest()
            self.regs[0x34] = self.regs.get(0x34, 0) | self.FILL_STATUS
            self.record("Graphics fill completed", **job)
            self.irqs_raised += 1
            self.raise_irq(irq_cpu, self.IRQ)

        self.schedule(delay, complete)

    def _start_copy(self, cpu_id, trigger):
        """Explicit software-derived profile, never a generic copy completion."""
        if trigger != 1:
            raise UnsupportedDevice(f'Unknown graphics copy trigger {trigger:#x}')
        if self.busy or self._command_pending is not None or self.regs.get(0x34, 0) & (self.FILL_STATUS | self.COPY_STATUS | self.CLEAR_STATUS):
            raise UnsupportedDevice('Graphics copy requires idle and acknowledged previous completion')
        if not {0xc00, 0xc04, 0xc08, 0xc0c, 0xc10}.issubset(self.written):
            raise UnsupportedDevice('Graphics copy descriptor is incomplete')
        control = self.regs[0xc10]
        # The valid native-pixmap VG path supplies caller flag1 and internal
        # format0 maps to hardware format0 for source/destination. All other
        # controls, including alternate bit3 descriptors, remain unsupported.
        if control != 1:
            raise UnsupportedDevice(f'Unknown graphics software-profile copy control {control:#x}')
        source_dimensions = self.regs[0xc08]
        destination_dimensions = self.regs[0xc0c]
        width, height = source_dimensions & 0xffff, source_dimensions >> 16
        if (source_dimensions != destination_dimensions or width == 0 or height == 0 or
                width & 7 or height & 7):
            raise UnsupportedDevice('Graphics software-profile copy requires identical positive8-aligned dimensions')
        count = width * height * 4
        if count > self.max_copy_bytes:
            raise UnsupportedDevice('Graphics software-profile copy exceeds byte bound')
        source_register, destination_register = self.regs[0xc00], self.regs[0xc04]
        addresses = []
        for address in (source_register, destination_register):
            alias = address & 0xc0000000
            if alias not in (0, 0x40000000):
                raise UnsupportedDevice('Graphics software-profile copy has unresolved RAM alias')
            physical = address - alias
            if physical & 3 or physical + count > self.ram_bytes:
                raise UnsupportedDevice('Graphics software-profile copy range/alignment exceeds bounds')
            addresses.append(physical)
        source, destination = addresses
        if source < destination + count and destination < source + count:
            raise UnsupportedDevice('Graphics software-profile overlapping copy is unproved')
        if self.read_bytes is None or self.raise_irq is None:
            raise UnsupportedDevice('Graphics software-profile copy requires memory and IRQ routing')
        irq_cpu = cpu_id
        if self.irq_targets is not None:
            targets = list(self.irq_targets(self.IRQ))
            if len(targets) != 1 or targets[0] not in (0, 1):
                raise UnsupportedDevice('Graphics copy IRQ has missing or ambiguous firmware-enabled bank')
            irq_cpu = targets[0]
        # Snapshot source at the trigger, then produce the exact Morton8x8,
        # inverted-y ROR8 result of the stock0xb6810c CPU fallback. Atomic
        # completion timing/source snapshot are explicit model assumptions.
        input_bytes = bytes(self.read_bytes(source, count, cpu_id=cpu_id))
        if len(input_bytes) != count:
            raise UnsupportedDevice('Graphics software-profile copy source read is incomplete')
        import numpy as np
        words = np.frombuffer(input_bytes, dtype='<u4')[_ui_morton(width, height)]
        payload = ((words >> 8) | ((words & 0xff) << 24))[::-1].astype('<u4').tobytes()
        delay = max(2, (count + self.bytes_per_tick - 1) // self.bytes_per_tick)
        self.busy = True
        self.regs[0xc18] = trigger
        self.written.add(0xc18)
        self.jobs_started += 1
        self.last_job = {'operation': 'copy', 'profile': self.copy_profile,
                         'cpu': cpu_id, 'source_register': hex(source_register),
                         'destination_register': hex(destination_register),
                         'source': hex(source), 'destination': hex(destination),
                         'width': width, 'height': height, 'bytes': count,
                         'control': hex(control), 'trigger': trigger,
                         'source_sha256': hashlib.sha256(input_bytes).hexdigest(),
                         'irq_cpu': irq_cpu,
                         'irq_routing': 'firmware-enabled bank' if self.irq_targets is not None else 'submitting CPU fixture',
                         'latency_ticks': delay, 'completed': False,
                         'pixel_semantics': 'inferred GPU profile matching actual stock CPU fallback, not hardware-observed'}
        job = self.last_job
        self.record('Graphics software-profile copy started', **job)

        def complete():
            self.write_bytes(destination, payload, cpu_id=cpu_id)
            self.busy = False
            self.jobs_completed += 1
            self.bytes_completed += count
            job['completed'] = True
            job['sha256'] = hashlib.sha256(payload).hexdigest()
            self.regs[0x34] = self.regs.get(0x34, 0) | self.COPY_STATUS
            self.record('Graphics software-profile copy completed', **job)
            self.irqs_raised += 1
            self.raise_irq(irq_cpu, self.IRQ)

        self.schedule(delay, complete)

    def snapshot(self):
        return {"device": type(self).__name__, "base": hex(self.BASE),
                "logical_offset_multiplier": 2,
                "registers": {hex(self.BASE + offset * 2): hex(value)
                              for offset, value in sorted(self.regs.items())},
                "written_logical_offsets": [hex(n) for n in sorted(self.written)],
                "busy": self.busy, "jobs_started": self.jobs_started,
                "jobs_completed": self.jobs_completed, "bytes_completed": self.bytes_completed,
                "irqs_raised": self.irqs_raised, "acknowledgements": self.acknowledgements,
                "last_job": None if self.last_job is None else dict(self.last_job),
                "bytes_per_tick": self.bytes_per_tick,
                'copy_profile': self.copy_profile, 'max_copy_bytes': self.max_copy_bytes,
                'max_command_capture_bytes': self.max_command_capture_bytes,
                'clear_profile': self.clear_profile, 'max_clear_bytes': self.max_clear_bytes,
                'command_pending': self._command_pending is not None,
                'command_register_banks': {bank: {hex(offset): hex(value) for offset, value in sorted(registers.items())}
                                           for bank, registers in self.command_banks.items()},
                "irq_routing": "exactly one firmware-enabled bank at trigger" if self.irq_targets is not None else "submitting CPU fixture",
                "limits": ["Only packet4 exact control0x0f100201 RAM pattern fill is supported",
                           "Initialization register values come from0xbcdcf8; physical reset values are uncaptured",
                           "Command-list rasterization and unknown copy controls/formats stop explicitly",
                           "Optional stock-vg-32bpp-fallback profile infers GPU pixels from one stock CPU fallback; no hardware-copy validation",
                           "Copy source snapshot/atomic completion timing and dimension/alignment bounds are explicit model assumptions",
                           "Optional clear-only profile accepts exact initial format0/no-source/no-scissor rectangle or matched first/repeated state-only initialization, including exact identity-matrix refresh",
                           "State-only initializers contain no geometry and perform zero pixel writes; completion semantics remain inferred",
                           "Clear pixel packing/Morton layout derive from software; cross-stream fence/draw, bit31 completion and atomic destination snapshot are inferred",
                           "Optional declared UI profile parses byte-lane registers, fixed-arity geometry and paired draw fences; unknown commands, paints, formats or state reject before DMA/completion",
                           "UI profile supports bounded affine format0 solid/image/linear/centered-radial fills and nondashed centerline strokes; gamma, off-center radial gradients, projective, masks and nonidentity color transforms remain unsupported",
                           "UI straight RGBA SRC/SRC_OVER, regular1/2/4 squared AA,0.125px curve flattening, reconstructed cap/join strokes with miter limit4, nearest image sampling and transparent image border are declared unmeasured laws",
                           "UI whole-batch destination/input snapshots and bit31/IRQ completion timing are model assumptions; actual driver progress does not validate hardware pixel fidelity",
                           "Logical105c retains explicit zero reset fixture; its physical post-wait value is unmeasured, never forced ready",
                           "Pattern fill timing/atomic completion and IRQ bank-routing rule are model assumptions",
                           "Real kernel IRQ3c registration/masking and task scheduling remain the Board's responsibility"]}
