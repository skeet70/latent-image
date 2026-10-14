"""Explicit channel8 row-major JPEG transaction; no inferred sensor packing.

All mutable state is plain data under JPEGEncodeControl.declared. Codec input
and restart interval are declared independently of opaque62048/62080 fields.
ARM owns descriptor results, callbacks, mailbox posts and IRQ unregister.
"""
from __future__ import annotations

import hashlib
import json
import struct

from .jpeg_codec import PACKED_422_PROFILE, MAX_BYTES, MAX_PIXELS, encode_packed_yycbcr, natural_tables
from .hardware import UnsupportedDevice
from . import jpeg_streamed


PROFILE = 'declared-channel8-packed-yycbcr-jpeg-unmeasured'
OFFSETS = (0, 0x24, 0x28, 0x40, 0x44, 0x48, 0x4c, 0x50, 0x80,
           0x84, 0x8c, 0x94, 0x100, 0x10c, 0x110, 0x118, 0x11c, 0x120, 0x124, 0x128)


def _reject(reason):
    raise UnsupportedDevice('Declared JPEG: ' + reason)


def _hash(data):
    return hashlib.sha256(data).hexdigest()


def _seal(value):
    return _hash(json.dumps(value, sort_keys=True, separators=(',', ':')).encode())


def _physical(address, count):
    if type(address) is not int or not 0 < address < 0x60000000:
        _reject('buffer address is outside DRAM')
    physical = address - 0x40000000 if address >= 0x40000000 else address
    if physical + count > 0x20000000:
        _reject('buffer span escapes DRAM')
    return physical


def arm(device, profile, *, descriptor_address, stride_bytes, source_sha256,
        completion_ticks, restart_interval, successor=False):
    b = device.board
    history = None
    initialization = None
    if successor:
        from . import jpeg_successor
        history, initialization = jpeg_successor.prepare_upload(device, descriptor_address)
    if (profile not in {PROFILE} | jpeg_streamed.PROFILES or device.profile != 'declared-channel8-idle-reset-unmeasured'
            or not successor and device.declared is not None
            or device.state != 'idle' or device.pending is not None
            or device.resets < 1 or device.registers != (dict.fromkeys(OFFSETS, 0)
                if successor else {0: 0, 0x24: 0, 0x118: 0})):
        _reject('requires explicit selection at the post-reset pre-upload boundary')
    if (type(completion_ticks) is not int or not 1 <= completion_ticks <= 10000000
            or type(stride_bytes) is not int or type(restart_interval) is not int
            or not 0 <= restart_interval <= 65535 or type(source_sha256) is not str
            or len(source_sha256) != 64):
        _reject('delay, row pitch, restart or source identity is not explicitly bounded')
    _physical(descriptor_address, 0x3c)
    if descriptor_address & 3:
        _reject('descriptor is not WORD aligned')
    if type(b.virtual_ticks) is not int or not 0 <= b.virtual_ticks < (1 << 64) - completion_ticks:
        _reject('completion due tick overflows')
    read = lambda address, count: bytes(b.read_memory(address, count, cpu_id=0))
    descriptor = read(descriptor_address, 0x3c)
    d = struct.unpack('<15I', descriptor)
    width, height, source, destination, capacity, budget = d[:6]
    if (not 2 <= width <= 65535 or width & 1 or not 1 <= height <= 65535
            or width * height > MAX_PIXELS or not width * 2 <= stride_bytes <= MAX_BYTES
            or stride_bytes * height > MAX_BYTES or not 1 <= budget <= capacity <= MAX_BYTES
            or (source | destination | capacity) & 7 or d[6:8] != (0, 0)
            or d[8] not in (0, 1) or d[9] != 1 or d[12] != 0 or d[13] > 65535
            or d[14] not in (0, 0x1cd52c4)):
        _reject('descriptor differs from bounded quantized main row-major class')
    src = _physical(source, stride_bytes * height)
    stream = None
    source_bytes = stride_bytes * height
    if profile in jpeg_streamed.PROFILES:
        stream = jpeg_streamed.arm_input(device, descriptor, stride_bytes, source_sha256, profile=profile)
        source_bytes = stream['source_bytes']
        src = _physical(source, source_bytes)
    dst = _physical(destination, capacity)
    if history is not None:
        jpeg_successor.check_destination(history, destination, capacity)
    desc = _physical(descriptor_address, 0x3c)
    if (src < dst + capacity and dst < src + source_bytes
            or desc < dst + capacity and dst < desc + 0x3c):
        _reject('source/descriptor and destination physically overlap')
    for pointer in d[10:12]:
        table = _physical(pointer, 64)
        if table < dst + capacity and dst < table + 64:
            _reject('quantization and destination physically overlap')
    fifo = read(d[10], 64) + read(d[11], 64)
    natural_tables(fifo)
    if _hash(read(source, source_bytes)) != source_sha256:
        _reject('scene source bytes differ from their explicit identity')
    latch = dict(profile=profile, descriptor_address=descriptor_address,
        descriptor_hex=descriptor.hex(), width=width, height=height, source=source,
        destination=destination, capacity=capacity, budget=budget, stride_bytes=stride_bytes,
        source_sha256=source_sha256, qtable_fifo_hex=fifo.hex(), armed_tick=b.virtual_ticks,
        completion_ticks=completion_ticks, restart_interval=restart_interval)
    if stream is not None:
        latch['resize_stream'] = stream
    device.declared = dict(latch=latch, latch_sha256=_seal(latch), state='uploading',
        events=[], pending_action=None, started_tick=None, delivered_tick=None,
        acknowledged_tick=None, count_read_tick=None, payload_sha256=None, output_bytes=None,
        route=None, close_requested_tick=None)
    if history is not None:
        device.declared['history'] = history
        device.declared['initialization'] = initialization
    for offset in OFFSETS:
        device.registers.setdefault(offset, 0)
    return dict(latch)


def sequence(latch):
    width, height = latch['width'], latch['height']
    descriptor = struct.unpack('<15I', bytes.fromhex(latch['descriptor_hex']))
    opaque80 = descriptor[13]
    result = [('write', 0, 4), ('write', 0x28, 0xfd0000), ('write', 0x40, 1),
        ('write', 0x44, width | height << 16), ('write', 0x48, 0),
        ('write', 0x4c, 0), ('write', 0x50, 0), ('write', 0x80, opaque80),
        ('write', 0x100, latch['source']), ('write', 0x10c, latch['destination']),
        ('write', 0x110, latch['capacity']), ('write', 0x11c, 0), ('write', 0x120, int(descriptor[8] != 0) << 1),
        ('write', 0x124, 0), ('write', 0x128, 0), ('write', 0x84, 0x100)]
    fifo = bytes.fromhex(latch['qtable_fifo_hex'])
    result += [('write', 0x8c, int.from_bytes(fifo[n:n + 4], 'big')) for n in range(0, 128, 4)]
    return result + [('write', 0x84, 1), ('read', 0, 4), ('write', 0, 5)]


def _route(device, *, drained):
    irq = getattr(device.board, 'interrupts', None)
    if irq is None or len(irq.banks) != 2:
        _reject('requires the real two-bank interrupt controller')
    bank, other = irq.banks
    line = 77
    read = lambda address: int.from_bytes(device.board.read_memory(address, 4, cpu_id=0), 'little')
    handle = read(0x15231a4)
    if (not handle or handle != read(0xc96f20 + line * 4)
            or bank.config.get(0x400 + 4 * line) != 0x10c50a0
            or bank.config.get(0x800 + 4 * line) != 7
            or (bank.config.get(0x180 + 4 * (line // 16), 0) >> (2 * (line % 16))) & 3 != 1
            or bank.source_config & (1 << line) or not bank.enabled & (1 << line)
            or other.enabled & (1 << line) or other.pending & (1 << line)):
        _reject('IRQ77 differs from the stock CPU0 registration')
    if drained and any(b.pending & (1 << line) or b.active == line or b.injected == line
            or any(i['line'] == line for i in b.active_stack) for b in irq.banks):
        _reject('IRQ77 is not drained')
    return dict(cpu=0, line=line, vector=0x10c50a0, priority=7, handle=handle)


def _closed_unregistered(device):
    """Only stock reset followed by IRQ77 teardown ends live RAM ownership."""
    if device.state != 'idle' or device.pending is not None:
        return False
    irq = getattr(device.board, 'interrupts', None)
    if irq is None or len(irq.banks) != 2:
        return False
    line = 77
    if any(bank.enabled & (1 << line) or bank.pending & (1 << line)
            or bank.active == line or bank.injected == line
            or any(item['line'] == line for item in bank.active_stack) for bank in irq.banks):
        return False
    read = lambda address: int.from_bytes(device.board.read_memory(address, 4, cpu_id=0), 'little')
    return not read(0x15231a4) and not read(0xc96f20 + line * 4)


def validate(device, *, source=False, historical=False):
    record = device.declared
    if isinstance(record, dict) and ('history' in record or 'successor' in record):
        from . import jpeg_successor
        jpeg_successor.validate(device)
        if 'successor' in record:
            from copy import copy
            view = copy(device)
            view.declared = jpeg_successor._base(record)
            view.registers = record['successor']['previous']['registers']
            validate(view, historical=True)
            return record
    if (not isinstance(record, dict) or set(record) - {'history', 'initialization'} != {'latch', 'latch_sha256', 'state', 'events',
            'pending_action', 'started_tick', 'delivered_tick', 'acknowledged_tick', 'count_read_tick',
            'payload_sha256', 'output_bytes', 'route', 'close_requested_tick'}
            or not isinstance(record['latch'], dict) or _seal(record['latch']) != record['latch_sha256']):
        _reject('retained input latch is malformed or changed')
    if historical and record['state'] != 'closed':
        _reject('only a closed JPEG can use its immutable historical projection')
    if (not historical and ('history' in record) != ('initialization' in record)) or (
            'history' in record and not record['history']):
        _reject('JPEG successor history and fresh initialization must be retained together')
    if 'initialization' in record:
        from . import jpeg_successor
        jpeg_successor.validate_initialization(device, record)
    latch, events = record['latch'], record['events']
    stream = latch.get('profile') in jpeg_streamed.PROFILES
    if (set(latch) != {'profile', 'descriptor_address', 'descriptor_hex', 'width', 'height',
            'source', 'destination', 'capacity', 'budget', 'stride_bytes', 'source_sha256',
            'qtable_fifo_hex', 'armed_tick', 'completion_ticks', 'restart_interval'} | ({'resize_stream'} if stream else set())
            or latch['profile'] not in {PROFILE} | jpeg_streamed.PROFILES):
        _reject('retained input latch schema/profile differs')
    if stream:
        metadata = jpeg_streamed.validate_metadata(latch['resize_stream'], profile=latch['profile'])
        if (latch['source'] != metadata['source'] or latch['source_sha256'] != metadata['source_sha256']
                or latch['stride_bytes'] != metadata['source_stride_bytes']
                or (latch['width'], latch['height']) != (metadata['width'], metadata['height'])):
            _reject('JPEG latch no longer matches its sealed Resize stream')
    expected = sequence(latch)
    if not isinstance(events, list) or len(events) > len(expected):
        _reject('upload transaction escapes its bounded sequence')
    previous = latch['armed_tick']
    for item, access in zip(events, expected):
        if (set(item) != {'operation', 'offset', 'value', 'tick'}
                or (item['operation'], item['offset'], item['value']) != access
                or type(item['tick']) is not int or not previous <= item['tick'] <= device.board.virtual_ticks):
            _reject('retained programming transaction differs from stock')
        previous = item['tick']
    projection = dict.fromkeys(OFFSETS, 0)
    for item in events:
        if item['operation'] == 'write':
            projection[item['offset']] = item['value']
    state = record['state']
    if state == 'uploading':
        if (len(events) >= len(expected) or record['pending_action'] is not None
                or any(record[key] is not None for key in ('started_tick', 'delivered_tick',
                    'acknowledged_tick', 'count_read_tick', 'payload_sha256', 'output_bytes',
                    'route', 'close_requested_tick'))):
            _reject('upload state/action is inconsistent')
    elif state in ('encoding', 'delivered', 'acknowledged', 'closing', 'closed'):
        if len(events) != len(expected) or record['started_tick'] != events[-1]['tick']:
            _reject('active state lacks a complete stock programming/start transaction')
        if state == 'encoding':
            action = record['pending_action']
            if (not isinstance(action, dict) or set(action) != {'kind', 'cpu', 'due_tick', 'payload'}
                    or action['kind'] != 'jpeg-data-before-irq77' or action['cpu'] != 0
                    or action['due_tick'] != record['started_tick'] + latch['completion_ticks']
                    or type(action['payload']) is not bytes or not action['payload'].startswith(b'\xff\xd8')
                    or not action['payload'].endswith(b'\xff\xd9')
                    or len(action['payload']) != record['output_bytes']
                    or _hash(action['payload']) != record['payload_sha256']):
                _reject('plain pending JPEG action is malformed or changed')
        elif record['pending_action'] is not None:
            _reject('terminal JPEG state retains a future action')
    else:
        _reject('unknown retained JPEG state')
    if state != 'uploading':
        if (type(record['output_bytes']) is not int or not 1 <= record['output_bytes'] <= latch['capacity']
                or type(record['payload_sha256']) is not str or len(record['payload_sha256']) != 64
                or not isinstance(record['route'], dict)):
            _reject('output count/hash/route are malformed')
        delivered = record['delivered_tick']
        if state == 'encoding':
            if any(record[key] is not None for key in ('delivered_tick', 'acknowledged_tick',
                    'count_read_tick', 'close_requested_tick')):
                _reject('future completion has terminal acknowledgements')
        elif (type(delivered) is not int or not record['started_tick'] + latch['completion_ticks']
                <= delivered <= device.board.virtual_ticks):
            _reject('delivered JPEG chronology differs')
        else:
            projection[0x24], projection[0x94] = 0x10, record['output_bytes']
        acknowledged = record['acknowledged_tick']
        if state in ('acknowledged', 'closing', 'closed'):
            if type(acknowledged) is not int or not delivered <= acknowledged <= device.board.virtual_ticks:
                _reject('completion ACK chronology differs')
            projection[0x24] = 0
        elif acknowledged is not None or record['count_read_tick'] is not None:
            _reject('JPEG has acknowledgements before the real ACK')
        if record['count_read_tick'] is not None and (type(record['count_read_tick']) is not int
                or not acknowledged <= record['count_read_tick'] <= device.board.virtual_ticks):
            _reject('hardware count-read chronology differs')
        if state in ('closing', 'closed'):
            if (type(record['close_requested_tick']) is not int or record['count_read_tick'] is None
                    or not record['count_read_tick'] <= record['close_requested_tick'] <= device.board.virtual_ticks):
                _reject('reset/close chronology differs')
            # Existing91F620 owns only these three registers while closing.
            for offset in (0, 0x24, 0x118):
                projection[offset] = device.registers[offset]
            if state == 'closed' and any(projection[offset] for offset in (0, 0x24, 0x118)):
                _reject('closed JPEG reset words are not cleared')
        elif record['close_requested_tick'] is not None:
            _reject('JPEG has a reset request before close')
    if device.registers != projection:
        _reject('live programming/result words differ from the accepted transaction')
    # State 'closed' records the completed91F620 reset, which precedes919634
    # unregister in10C51FC. Keep bytes owned through that final teardown.
    # Sealed successor entries are admitted only after the same unregister;
    # their latch/count/hash remain evidence after stock Exif/buffer reuse.
    retired = historical or state == 'closed' and _closed_unregistered(device)
    read = lambda address, count: bytes(device.board.read_memory(address, count, cpu_id=0))
    descriptor = bytearray(read(latch['descriptor_address'], 0x3c)) if not retired else None
    # Count/result are only written by the real ISR, after hardware delivery.
    if not retired and state in ('delivered', 'acknowledged', 'closing', 'closed'):
        descriptor[0x18:0x20] = bytes(8)
    if not retired and descriptor.hex() != latch['descriptor_hex']:
        _reject('live descriptor changed outside the real ISR result fields')
    d = struct.unpack('<15I', bytes.fromhex(latch['descriptor_hex']))
    if not retired and read(d[10], 64) + read(d[11], 64) != bytes.fromhex(latch['qtable_fifo_hex']):
        _reject('live quantization source differs from the latched tables')
    if source and not retired and stream:
        jpeg_streamed.codec_input(device)
    if source and not retired and not stream and _hash(read(latch['source'], latch['stride_bytes'] * latch['height'])) != latch['source_sha256']:
        _reject('scene source changed before its DMA completion')
    if not retired and state in ('delivered', 'acknowledged', 'closing', 'closed') and _hash(read(latch['destination'],
            record['output_bytes'])) != record['payload_sha256']:
        _reject('JPEG output bytes changed before consuming completion/cleanup')
    return record


def access(device, cpu, operation, offset, value=None):
    record = device.declared
    if record['state'] == 'uploading':
        expected = sequence(record['latch'])[len(record['events'])]
        if cpu != 0 or (operation, offset) != expected[:2] or operation == 'write' and value != expected[2]:
            _reject('parameter/FIFO/start transaction differs from stock')
        if operation == 'write' and offset == 0 and value == 5:
            validate(device, source=True)
            latch = record['latch']
            route = _route(device, drained=True)
            if not 0 <= device.board.virtual_ticks < (1 << 64) - latch['completion_ticks']:
                _reject('started completion due tick overflows')
            if int.from_bytes(device.board.read_memory(0x15231a8, 4, cpu_id=0), 'little') != latch['descriptor_address']:
                _reject('stock active descriptor pointer differs')
            if latch['profile'] in jpeg_streamed.PROFILES:
                pixels, codec_stride = jpeg_streamed.codec_input(device)
            else:
                pixels = bytes(device.board.read_memory(latch['source'], latch['stride_bytes'] * latch['height'], cpu_id=0))
                codec_stride = latch['stride_bytes']
            payload = encode_packed_yycbcr(pixels, latch['width'], latch['height'],
                stride_bytes=codec_stride, qtable_fifo=bytes.fromhex(latch['qtable_fifo_hex']),
                output_capacity=latch['capacity'], profile=PACKED_422_PROFILE,
                restart_interval=latch['restart_interval'])
            record.update(state='encoding', started_tick=device.board.virtual_ticks, route=route,
                output_bytes=len(payload), payload_sha256=_hash(payload),
                pending_action=dict(kind='jpeg-data-before-irq77', cpu=0,
                    due_tick=device.board.virtual_ticks + latch['completion_ticks'], payload=payload))
        actual = expected[2]
        record['events'].append(dict(operation=operation, offset=offset, value=actual,
                                     tick=device.board.virtual_ticks))
        if operation == 'write':
            device.registers[offset] = actual
        return actual
    if record['state'] in ('delivered', 'acknowledged') and cpu == 0:
        if operation == 'read' and offset == 0x24:
            validate(device)
            return device.registers[offset]
        if operation == 'write' and offset == 0x24 and value == 0x10 and record['state'] == 'delivered':
            validate(device)
            if _route(device, drained=False) != record['route'] or device.registers[offset] != 0x10:
                _reject('IRQ77 ACK ownership/status differs')
            device.registers[offset] = 0
            record.update(state='acknowledged', acknowledged_tick=device.board.virtual_ticks)
            return value
        if operation == 'read' and offset == 0x94 and record['state'] == 'acknowledged':
            validate(device)
            record['count_read_tick'] = device.board.virtual_ticks
            return device.registers[offset]
    _reject('unknown active read/write or duplicate start/ACK')


def begin_close(device):
    record = validate(device)
    if (record['state'] != 'acknowledged' or record['count_read_tick'] is None
            or device.state != 'idle' or device.pending is not None):
        _reject('idle/reset requested before the real completion ACK/count read')
    if _route(device, drained=True) != record['route']:
        _reject('cleanup does not own the original drained IRQ77 registration')
    record.update(state='closing', close_requested_tick=device.board.virtual_ticks)


def tick(device):
    record = device.declared
    action = record['pending_action'] if record is not None else None
    if action is None:
        return False
    if not isinstance(action, dict) or type(action.get('due_tick')) is not int:
        _reject('pending action has no plain integer due tick')
    if device.board.virtual_ticks < action['due_tick']:
        return False
    validate(device, source=True)
    if _route(device, drained=True) != record['route']:
        _reject('completion no longer owns its original IRQ77 route')
    payload = action['payload']
    if record['latch']['profile'] in jpeg_streamed.PROFILES:
        jpeg_streamed.complete(device)
    device.board.write_memory(record['latch']['destination'], payload, cpu_id=0)
    device.registers[0x94], device.registers[0x24] = len(payload), 0x10
    record.update(state='delivered', delivered_tick=device.board.virtual_ticks, pending_action=None)
    device.board.queue_irq(0, 77)
    return True


def summary(device):
    if device.declared is None:
        return None
    record = validate(device)
    result = dict(profile=record['latch']['profile'], state=record['state'], latch=dict(record['latch']),
        access_count=len(record['events']), started_tick=record['started_tick'],
        delivered_tick=record['delivered_tick'], acknowledged_tick=record['acknowledged_tick'],
        count_read_tick=record['count_read_tick'], output_bytes=record['output_bytes'],
        payload_sha256=record['payload_sha256'],
        due_tick=record['pending_action']['due_tick'] if record['pending_action'] is not None else None,
        limit='Explicit row-major YYCbCr/4:2:2/libjpeg surrogate; unknown sampling/field80 are not decoded; no capture/file result')
    if 'history' in record:
        result['initialization_access_count'] = len(record['initialization']['events'])
        result['closed_lifetimes'] = [dict(descriptor_address=entry['record']['latch']['descriptor_address'],
            destination=entry['record']['latch']['destination'], output_bytes=entry['record']['output_bytes'],
            payload_sha256=entry['record']['payload_sha256'], closed_tick=entry['closed_tick'])
            for entry in record['history']]
    if 'successor' in record:
        result['successor_initialization'] = dict(profile=record['successor']['profile'],
            state=record['successor']['state'], access_count=len(record['successor']['events']))
    return result
