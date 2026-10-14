"""Bounded mode0 idle reset and calibration storage, without an image result.

The bank extents come from the stock 97A748 descriptor tables exercised by
1180B74 (semantic kinds6/7). Their words are retained as software uploads; no
Fujifilm color/CFA transform is inferred from those calibration parameters.
"""
from __future__ import annotations

import struct

from .hardware import UnsupportedDevice


PROFILE = 'declared-mode0-still-processing-initialization-unmeasured'
BASE = 0xfff40000
RESET_OFFSETS = (8, 0xc, 0x10, *range(0x18, 0x78, 4), 0x84, 0x88, 0x8c,
                 0x90, 0xa0, 0xa4, 0xa8, 0xb0, 0xb4, 0xb8, 0xbc)
# Whole 1172628 upload order, including the second 44800 selector write.
CALIBRATION_RUNS = ((0xfff44000, 12), (0xfff44200, 12), (0xfff44300, 52),
    (0xfff41000, 32), (0xfff4462c, 6), (0xfff44500, 31),
    (0xfff44600, 11), (0xfff44700, 22), (0xfff44100, 29),
    (0xfff44800, 8), *((0xfff44c00 + i * 0x100, 41) for i in range(20)),
    (0xfff44800, 1), (0xfff44400, 5))
CALIBRATION_ADDRESSES = tuple(a + 4 * i for a, count in CALIBRATION_RUNS for i in range(count))
AUXILIARY = (0xfff38004, 0xfff38008, 0xfff3800c, 0xfff38010,
             0xfff38054, 0xfff38058)
# 935514 noise controls and 935C08 fourteen 256-word calibration planes.
# These keys are created only by explicit initialization admission.
AUX_CALIBRATION = (*range(0xfff300b4, 0xfff300d8, 4),
                   0xfff300f0, 0xfff300f4, 0xfff300f8,
                   *range(0xfff31400, 0xfff34c00, 4),
                   *range(0xfff38004, 0xfff38060, 4))
GEOMETRY_INDICES = (*range(6), *range(11, 18), 59)


def _geometry(device, address):
    from .board import RAM_SIZE
    words = struct.unpack('<60I', device.board.read_memory(address, 0xf0, cpu_id=0))
    def span(pointer, pitch, width, height):
        physical = pointer & 0x3fffffff
        return (pointer < 0x60000000 and physical > 0 and pointer % 8 == 0
                and 0 < width <= pitch and 0 < height
                and physical + (height - 1) * pitch + width <= RAM_SIZE)
    if (words[0] != 0 or not span(words[1], words[2], words[3], words[4])
            or not span(words[11], words[12], words[13], words[14]) or words[5] not in (1, 2)
            or words[17] != 2 or words[59] != 0x1180640
            or not all(words[i] for i in (2, 3, 4, 12, 13, 14))):
        raise UnsupportedDevice('Still initialization requires the real single mode0 caller geometry')
    return tuple(words[i] for i in GEOMETRY_INDICES)


def arm(device, profile):
    from .still_processing_devices import PROFILE as DATA_PROFILE
    if (profile != PROFILE or device.profile != DATA_PROFILE or device.state != 'idle'
            or device.request is not None or hasattr(device, 'initialization')
            or device.board.failure is not None):
        raise UnsupportedDevice('Still initialization requires explicit inactive one-shot admission')
    banks = device.board.interrupts.banks
    if any(b.enabled & (1 << 61) or b.pending & (1 << 61)
           or b.active == 61 or b.injected == 61 for b in banks):
        raise UnsupportedDevice('Still initialization requires an absent drained IRQ61 owner')
    # Deliberately created only by explicit admission; old inactive snapshots
    # keep exactly their original attribute and register shape.
    device.initialization = dict(profile=PROFILE, state='reset', step=0,
        geometry=None, requested_tick=device.board.virtual_ticks,
        calibrated_tick=None, first_upload=len(device.uploads),
        initial_control=device.registers[BASE])
    for address in (*[BASE + o for o in RESET_OFFSETS], *CALIBRATION_ADDRESSES,
                    *AUXILIARY, *AUX_CALIBRATION):
        device.registers.setdefault(address, 0)
    return summary(device)


def validate(device):
    record = getattr(device, 'initialization', None)
    if record is None:
        return None
    keys = {'profile', 'state', 'step', 'geometry',
            'requested_tick', 'calibrated_tick', 'first_upload', 'initial_control'}
    if (type(record) is not dict or set(record) not in (keys, keys | {'auxiliary'}) or record['profile'] != PROFILE
            or record['state'] not in ('reset', 'idle', 'status', 'calibration', 'calibrated')
            or type(record['step']) is not int or record['step'] < 0):
        raise UnsupportedDevice('Still initialization marker differs from its bounded contract')
    limit = {'reset': len(RESET_OFFSETS), 'idle': 6, 'status': 0,
             'calibration': len(CALIBRATION_ADDRESSES), 'calibrated': len(CALIBRATION_ADDRESSES)}
    if record['step'] > limit[record['state']]:
        raise UnsupportedDevice('Still initialization exceeds its whole reset/upload bounds')
    if record['state'] == 'calibrated' and record['step'] != len(CALIBRATION_ADDRESSES):
        raise UnsupportedDevice('Still initialization calibration is incomplete')
    if (type(record['first_upload']) is not int or not 0 <= record['first_upload'] <= len(device.uploads)
            or type(record['initial_control']) is not int or not 0 <= record['initial_control'] <= 0xffffffff
            or record['geometry'] is not None and (type(record['geometry']) is not tuple
                or len(record['geometry']) != len(GEOMETRY_INDICES))):
        raise UnsupportedDevice('Still initialization history anchor differs')
    uploads = device.uploads[record['first_upload']:]
    first_idle = record['initial_control'] & ~0x10000
    second_idle = (first_idle | 0x10001) & ~1
    prefix = [(0xfff30000, 0), (0xfff38000, 0)] + [(BASE + o, 0) for o in RESET_OFFSETS]
    prefix += [(BASE, first_idle), (BASE, second_idle), (BASE + 0x84, 0x703f000f)]
    stage, step = record['state'], record['step']
    count = {'reset': min(len(uploads), 2) + step,
             'idle': 40 + int(step >= 2) + int(step >= 5), 'status': 42,
             'calibration': 43 + step, 'calibrated': 43 + len(CALIBRATION_ADDRESSES)}[stage]
    if (len(uploads) < count or stage != 'calibrated' and len(uploads) != count
            or uploads[:min(count, 43)] != prefix[:min(count, 43)]):
        raise UnsupportedDevice('Still initialization reset/idle/W1C history differs')
    calibration = uploads[43:count] if count > 43 else []
    if tuple(a for a, _ in calibration) != CALIBRATION_ADDRESSES[:len(calibration)]:
        raise UnsupportedDevice('Still initialization calibration history differs')
    if stage == 'calibrated':
        retained = dict(calibration)
        if 'auxiliary' in record:
            retained.update({a: v for op, a, v in record['auxiliary']['plan'][:record['auxiliary']['step']]
                             if op == 'write' and a in retained})
        if any(device.registers.get(a) != value for a, value in retained.items()):
            raise UnsupportedDevice('Still retained calibration differs from its stock upload')
    if 'auxiliary' in record:
        _validate_auxiliary(device, record['auxiliary'])
    return record


def _auxiliary_plan(descriptor, noise, lut, secondary, control, second_control):
    """Exact 10CFC58 subset: null49..52/54, 935514 + 935C08 storage.

    LUT values are uploaded software parameters. This packing law supplies no
    physical image transform, status, work completion or interrupt.
    """
    if (type(descriptor) is not bytes or len(descriptor) != 0xf0
            or type(noise) is not bytes or len(noise) != 44
            or type(lut) is not bytes or len(lut) != 0x5424
            or type(secondary) is not tuple or len(secondary) != 3
            or any(type(p) is not bytes for p in secondary)
            or tuple(map(len, secondary)) != (32, 136, 16)
            or any(type(v) is not int or not 0 <= v <= 0xffffffff for v in (control, second_control))):
        raise UnsupportedDevice('Still auxiliary parameter spans/controls differ from the actual setter ABI')
    words = struct.unpack('<60I', descriptor)
    if (words[0] != 0 or any(words[i] for i in (49, 50, 51, 52, 54))
            or not all(words[i] for i in (53, 55, 56, 57, 58))):
        raise UnsupportedDevice('Still auxiliary requires the audited single noise/LUT mode0 pointers')
    n = lambda offset: int.from_bytes(noise[offset:offset + 2], 'little')
    if (n(4) > 0xfff or n(0x12) > 0xfff
            or any(n(o) > 0x3ff for o in (10, 12, 0x18, 0x1a))
            or any(noise[o] > 7 for o in (6, 7, 8, 9, 0x14, 0x15, 0x16, 0x17))
            or any(noise[o] > 15 for o in (0xf, 0x10, 0x11, 0x1d, 0x1e, 0x1f, 0x20, 0x23, 0x26))
            or struct.unpack_from('<I', noise)[0] > 3):
        raise UnsupportedDevice('Still auxiliary noise fields exceed their actual setter widths')
    plan = []
    def rmw(mask, value):
        nonlocal control
        plan.append(('read', 0xfff30000, control))
        control = (control & ~mask) | value
        plan.append(('write', 0xfff30000, control))
    # Null color/sharpen/aux pointers; the independent noise and LUT pointers
    # remain enabled only by their stock field values.
    for mask in (0x3f, 0x300, 0x400, 0x3000):
        rmw(mask, 0)
    rmw(0xc000, struct.unpack_from('<I', noise)[0] << 14)
    packed_noise = (
        noise[9] | noise[8] << 4 | noise[7] << 8 | noise[6] << 12 | n(4) << 16,
        n(12) | n(10) << 16,
        noise[0x17] | noise[0x16] << 4 | noise[0x15] << 8 | noise[0x14] << 12 | n(0x12) << 16,
        n(0x1a) | n(0x18) << 16,
        noise[0x11] | noise[0x10] << 8 | noise[0xf] << 16 | noise[0xe] << 24,
        noise[0x1f] | noise[0x1e] << 8 | noise[0x1d] << 16 | noise[0x1c] << 24,
        noise[0x22] | noise[0x21] << 16 | noise[0x20] << 24,
        noise[0x25] | noise[0x24] << 16 | noise[0x23] << 24,
        noise[0x29] | noise[0x28] << 8 | noise[0x27] << 16 | noise[0x26] << 24)
    plan += [('write', 0xfff300b4 + 4 * i, v) for i, v in enumerate(packed_noise)]
    rmw(0x30000, 0)  # Null54.
    # 10CFC58 replaces LUT header geometry from 10CF1B8's mode0 half-sizes.
    headers = struct.unpack_from('<8I', lut)
    if headers[1] > 1 or headers[4] > 0x7ff or headers[5] > 0x7ff or headers[6] > 0x3f or headers[7] > 0x3f:
        raise UnsupportedDevice('Still auxiliary LUT header exceeds its stock widths')
    source_width = words[3] * 2 // 3 if words[5] == 1 else words[3] // 2
    width, height = source_width // 2, words[4] // 2
    if width > 0x1fff or height > 0x1fff:
        raise UnsupportedDevice('Still auxiliary LUT geometry exceeds its stock fields')
    plan += [('write', 0xfff300f0, height | width << 16),
             ('write', 0xfff300f4, headers[5] | headers[4] << 16),
             ('write', 0xfff300f8, headers[7] << 16 | headers[6] << 24)]
    def sample(offset):
        value = struct.unpack_from('<H', lut, offset)[0]
        if value > 0x3ff:
            raise UnsupportedDevice('Still auxiliary LUT sample exceeds ten bits')
        return value
    for i in range(256):
        for plane in range(6):
            v = sum(sample(0x20 + (plane * 3 + lane) * 0x200 + i * 2) << (10 * lane) for lane in range(3))
            plan.append(('write', 0xfff31400 + plane * 0x400 + i * 4, v))
    for plane in range(8):
        for i in range(256):
            v = sum(sample(0x4420 - lane * 0x1000 + plane * 0x200 + i * 2) << (10 * lane) for lane in range(3))
            plan.append(('write', 0xfff32c00 + plane * 0x400 + i * 4, v))
    rmw(0x300000, 0x100000 | headers[1] << 21)
    # Immutable mode0 table13ADC90 selects bit24=0.
    rmw(0x1000000, 0)
    # 10CFE1C secondary calibrations: gains, fourteen paired coefficients,
    # two correction words, and the stock no-offset selector.
    gains, curve, correction = (struct.unpack('<' + str(len(p) // 4) + 'I', p) for p in secondary)
    if (any(v > 0x1fff for v in gains) or curve[2] > 0x3f or curve[3] > 0x3f
            or curve[4] > 0x1fff or any(v > 0x3fff for v in curve[5:33])
            or any(v > 0x7fff for v in correction[:3]) or correction[3] > 0xfff):
        raise UnsupportedDevice('Still secondary auxiliary fields exceed their actual widths')
    def second_rmw(shift):
        nonlocal second_control
        plan.append(('read', 0xfff38000, second_control))
        second_control = (second_control & ~(1 << shift)) | (1 << shift)
        plan.append(('write', 0xfff38000, second_control))
    second_rmw(0)
    plan += [('write', 0xfff38004 + i * 4, gains[i * 2 + 1] | gains[i * 2] << 16) for i in range(4)]
    second_rmw(4)
    plan += [('write', 0xfff38014, height | width << 16),
             ('write', 0xfff38018, curve[4] | curve[3] << 16 | curve[2] << 24)]
    plan += [('write', 0xfff3801c + i * 4, curve[6 + i * 2] | curve[5 + i * 2] << 16) for i in range(14)]
    second_rmw(8)
    plan += [('write', 0xfff38054, correction[1] | correction[0] << 16),
             ('write', 0xfff38058, correction[3] | correction[2] << 16),
             ('write', 0xfff3805c, 0x2000e)]
    if words[38] != 1 or words[48] != 0:
        raise UnsupportedDevice('Still auxiliary requires the audited single geometry calibration update')
    plan.append(('write', 0xfff41004, height | width << 16))
    return tuple(plan)


def _validate_auxiliary(device, data):
    keys = {'descriptor', 'noise', 'lut', 'secondary', 'initial_control', 'second_control', 'plan', 'step', 'first_upload'}
    if (type(data) is not dict or set(data) not in (keys, keys | {'control_clears'})
            or type(data['plan']) is not tuple or type(data['step']) is not int
            or not 0 <= data['step'] <= len(data['plan']) or type(data['first_upload']) is not int
            or not 0 <= data['first_upload'] <= len(device.uploads)
            or data['plan'] != _auxiliary_plan(data['descriptor'], data['noise'], data['lut'],
                data['secondary'], data['initial_control'], data['second_control'])):
        raise UnsupportedDevice('Still auxiliary retained parameter/transaction plan differs')
    writes = [(a, v) for op, a, v in data['plan'][:data['step']] if op == 'write']
    if device.uploads[data['first_upload']:data['first_upload'] + len(writes)] != writes:
        raise UnsupportedDevice('Still auxiliary complete upload history differs')
    clears = data.get('control_clears', ())
    if (type(clears) is not tuple or clears not in ((), (0xfff30000,), (0xfff30000, 0xfff38000))
            or clears and (data['step'] != len(data['plan']) or device.state != 'acknowledged')):
        raise UnsupportedDevice('Still auxiliary retirement differs from the whole drained stop')
    if clears:
        # The whole caller configures and starts the main engine between the
        # auxiliary upload and its drained closing stores. Keep those stores
        # terminal, without treating that genuine configuration as adjacency.
        suffix = device.uploads[data['first_upload'] + len(writes):]
        auxiliary_addresses = (0xfff30000, 0xfff38000, 0xfff41004, *AUX_CALIBRATION)
        if (len(suffix) < len(clears) or suffix[-len(clears):] != [(a, 0) for a in clears]
                or any(a in auxiliary_addresses for a, _ in suffix[:-len(clears)])):
            raise UnsupportedDevice('Still auxiliary control-clear history differs')
    expected = dict(writes)
    expected.update(dict.fromkeys(clears, 0))
    if any(device.registers.get(a) != v for a, v in expected.items()):
        raise UnsupportedDevice('Still auxiliary retained calibration differs')
    return data


def _auxiliary_access(device, record, operation, address, value):
    if address not in (0xfff30000, 0xfff38000, 0xfff41004, *AUX_CALIBRATION):
        return False, None
    data = record.get('auxiliary')
    if data is None:
        descriptor = bytes(device.board.read_memory(0x17d0b68, 0xf0, cpu_id=0))
        words = struct.unpack('<60I', descriptor)
        if not words[53] and not words[55]:
            if address in AUX_CALIBRATION and address not in (*AUXILIARY, 0xfff3805c):
                raise UnsupportedDevice('Still auxiliary storage has no actual parameter owner')
            return False, None  # Keep the qualified semantic7 null subset.
        validate(device)
        if _geometry(device, 0x17d0b68) != record['geometry']:
            raise UnsupportedDevice('Still auxiliary differs from initialized geometry')
        noise = bytes(device.board.read_memory(words[53], 44, cpu_id=0))
        lut = bytes(device.board.read_memory(words[55], 0x5424, cpu_id=0))
        secondary = tuple(bytes(device.board.read_memory(words[i], n, cpu_id=0))
                          for i, n in ((56, 32), (57, 136), (58, 16)))
        control = device.registers[0xfff30000]
        second_control = device.registers[0xfff38000]
        plan = _auxiliary_plan(descriptor, noise, lut, secondary, control, second_control)
        if (operation, address) != plan[0][:2]:
            raise UnsupportedDevice('Still auxiliary starts outside the whole actual setter sequence')
        data = dict(descriptor=descriptor, noise=noise, lut=lut, secondary=secondary,
                    initial_control=control, second_control=second_control,
                    plan=plan, step=0, first_upload=len(device.uploads))
        record['auxiliary'] = data
    if data['step'] == len(data['plan']):
        clears = data.get('control_clears', ())
        wanted = (0xfff30000, 0xfff38000)
        banks = device.board.interrupts.banks
        if (operation != 'write' or value != 0 or device.state != 'acknowledged'
                or len(clears) >= 2 or address != wanted[len(clears)]
                or any(b.pending & (1 << 61) or b.active == 61 or b.injected == 61 for b in banks)):
            raise UnsupportedDevice('Still auxiliary upload cannot be repeated without a new owner')
        data['control_clears'] = (*clears, address)
        device.registers[address] = 0
        device.uploads.append((address, 0))
        return True, None
    op, wanted_address, wanted = data['plan'][data['step']]
    if (operation, address) != (op, wanted_address) or op == 'write' and value != wanted:
        raise UnsupportedDevice('Still auxiliary differs from its parameter-derived whole transaction')
    if op == 'read' and device.registers[address] != wanted:
        raise UnsupportedDevice('Still auxiliary current control differs from the retained stock setter')
    data['step'] += 1
    if op == 'write':
        device.registers[address] = value
        device.uploads.append((address, value))
    return True, wanted if op == 'read' else None


def access(device, operation, address, value=None):
    record = getattr(device, 'initialization', None)
    if record is not None and record['state'] == 'calibrated':
        handled, result = _auxiliary_access(device, record, operation, address, value)
        if handled:
            return True, result
    if record is None or record['state'] == 'calibrated':
        if address in CALIBRATION_ADDRESSES:
            raise UnsupportedDevice('Still calibration upload requires its unconsumed initialization owner')
        return False, None
    if device.state != 'idle' or device.request is not None:
        raise UnsupportedDevice('Still initialization cannot coexist with an active image job')
    if record['geometry'] is None:
        record['geometry'] = _geometry(device, 0x15608a4)
    stage, step = record['state'], record['step']
    if stage == 'reset' and address in (0xfff30000, 0xfff38000):
        index = len(device.uploads) - record['first_upload']
        if operation != 'write' or value != 0 or index not in (0, 1) or address != (0xfff30000, 0xfff38000)[index]:
            raise UnsupportedDevice('Still initialization auxiliary reset differs')
    elif stage == 'reset':
        if (operation != 'write' or address != BASE + RESET_OFFSETS[step] or value != 0
                or len(device.uploads) - record['first_upload'] != 2 + step):
            raise UnsupportedDevice('Still initialization differs from the whole38-word reset')
        record['step'] += 1
        if record['step'] == len(RESET_OFFSETS):
            record['state'], record['step'] = 'idle', 0
    elif stage == 'idle':
        if address != BASE or operation != ('write' if step in (1, 4) else 'read'):
            raise UnsupportedDevice('Still initialization differs from bit0/16 idle polling')
        if operation == 'write':
            mask = 0x10000 if step == 1 else 1
            if value != device.registers[BASE] & ~mask:
                raise UnsupportedDevice('Still initialization idle setter changes another control bit')
            # Explicit immediate-idle law. It grants no work completion.
            device.registers[BASE] = value | 0x10001
        record['step'] += 1
        if record['step'] == 6:
            record['state'], record['step'] = 'status', 0
        if operation == 'read':
            return True, device.registers[BASE]
        device.uploads.append((address, value))
        return True, None
    elif stage == 'status':
        if operation != 'write' or address != BASE + 0x84 or value != 0x703f000f:
            raise UnsupportedDevice('Still initialization requires its actual idle W1C clear')
        device.registers[address] = 0
        device.uploads.append((address, value))
        record['state'], record['step'] = 'calibration', 0
        return True, None
    elif stage == 'calibration':
        if operation != 'write' or address != CALIBRATION_ADDRESSES[step]:
            raise UnsupportedDevice('Still initialization differs from its complete typed calibration upload')
        record['step'] += 1
        if record['step'] == len(CALIBRATION_ADDRESSES):
            record['state'] = 'calibrated'
            record['calibrated_tick'] = device.board.virtual_ticks
    else:
        raise UnsupportedDevice('Unknown still initialization phase')
    device.registers[address] = value
    device.uploads.append((address, value))
    return True, None


def require_descriptor(device):
    record = validate(device)
    if record is not None and (record['state'] != 'calibrated'
            or _geometry(device, 0x17d0b68) != record['geometry']):
        raise UnsupportedDevice('Still image start differs from the initialized real caller geometry')
    if record is not None and 'auxiliary' in record and record['auxiliary']['step'] != len(record['auxiliary']['plan']):
        raise UnsupportedDevice('Still image start precedes the complete auxiliary calibration upload')


def summary(device):
    record = validate(device)
    if record is None:
        return None
    return dict(profile=record['profile'], state=record['state'], step=record['step'],
        calibration_words=len(CALIBRATION_ADDRESSES), requested_tick=record['requested_tick'],
        calibrated_tick=record['calibrated_tick'],
        limits='Explicit idle reset and software calibration storage; no pixels, status result or IRQ')
