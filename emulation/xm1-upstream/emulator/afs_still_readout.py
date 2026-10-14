"""Explicit finite channel2 RAW transfer through the stock completion IRQ.

Pixels use the declared monochrome codec, not a recovered sensor/CFA model.
Admission requires the actual finite readout driver. Only active row spans
are written, then IRQ24 executes the stock ACK, retirement and callback.
No source clock, task packet or firmware completion flag is supplied.
"""
from copy import deepcopy
import hashlib

from . import afs_still_raw as raw
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-finite-still-raw-row-dma-irq24-unmeasured'
IRQ = 0x24
ACK = ((0x350, 1), (4, 0), (0x350, 7), (0x354, 0x300000))
# When kernel registration precedes setup, the stock callback also retains
# the bit28 mask once before clearing it. Both whole caller orders are known.
ACK_WITH_MASK = ACK[:3] + ((0x354, 0x10300000),) + ACK[3:]


def _ack_prefix(events):
    words = tuple((event['offset'],event['value']) for event in events)
    return any(words == sequence[:len(words)] for sequence in (ACK,ACK_WITH_MASK))


def _acked(events):
    return tuple((event['offset'],event['value']) for event in events) in (ACK,ACK_WITH_MASK)


def _seal(record):
    # Row bytes are pinned separately, avoiding repeated raster serialization.
    return digest({key: value for key, value in record.items() if key not in ('rows', 'seal')})


def _rows_hash(rows):
    h = hashlib.sha256()
    for address, data in rows:
        h.update(address.to_bytes(4, 'little'));h.update(data)
    return h.hexdigest()


def _record(device):
    return getattr(device, 'raw_readout', None)


def validate(device, *, pixels=False):
    record = _record(device)
    if record is None: return None
    fields = {'profile', 'codec', 'geometry', 'source', 'rows', 'rows_sha256', 'requested_tick',
              'due_tick', 'delay_ticks', 'asserted_tick', 'acknowledged_tick', 'state',
              'write_cursor', 'write_end', 'acknowledgments', 'seal'}
    if (type(record) is not dict or set(record) != fields or record['profile'] != PROFILE
            or record['codec'] != raw.PROFILE or record['state'] not in ('armed', 'irq-pending', 'complete')
            or record['seal'] != _seal(record)
            or any(type(record[key]) is not int for key in ('requested_tick', 'due_tick', 'delay_ticks', 'write_cursor'))
            or not 1 <= record['delay_ticks'] <= 400000
            or not 0 <= record['requested_tick'] < record['due_tick'] == record['requested_tick'] + record['delay_ticks'] < 1 << 64
            or not 0 <= record['write_cursor'] <= len(device.writes)
            or type(record['rows']) is not tuple or len(record['rows']) != record['geometry']['height']):
        raise UnsupportedDevice('Finite RAW transfer identity, timing or row schema changed')
    geometry = record['geometry']
    if any(type(address) is not int or type(data) is not bytes
           or address != geometry['destination'] + y * geometry['pitch']
           or len(data) != geometry['row_bytes']
           for y, (address, data) in enumerate(record['rows'])):
        raise UnsupportedDevice('Finite RAW transfer row addresses or lengths changed')
    if pixels and _rows_hash(record['rows']) != record['rows_sha256']:
        raise UnsupportedDevice('Finite RAW transfer declared row bytes changed')
    acknowledgments = record['acknowledgments']
    if (type(acknowledgments) is not list or len(acknowledgments) > len(ACK_WITH_MASK)
            or any(set(event) != {'cpu', 'offset', 'value', 'tick'} or event['cpu'] != 0
                   or type(event['tick']) is not int or not record['due_tick'] <= event['tick'] <= device.board.virtual_ticks
                   for event in acknowledgments) or not _ack_prefix(acknowledgments)):
        raise UnsupportedDevice('Finite RAW transfer lacks its ordered stock retirement stores')
    end = record['write_end']
    if end is not None and (type(end) is not int or record['state'] != 'complete'
                           or not record['write_cursor'] <= end <= len(device.writes)):
        raise UnsupportedDevice('Finite RAW transfer closed history bound changed')
    actual = [(int(event['address'],16)-device.BASE, int(event['value'],16), event['cpu'])
              for event in device.writes[record['write_cursor']:end]
              if int(event['address'],16)-device.BASE in (4, 0x350, 0x354)]
    if actual != [(event['offset'],event['value'],event['cpu']) for event in acknowledgments]:
        raise UnsupportedDevice('Finite RAW transfer retirement history changed')
    if record['state'] == 'armed':
        if record['asserted_tick'] is not None or acknowledgments or record['acknowledged_tick'] is not None:
            raise UnsupportedDevice('Finite RAW transfer completed before its DMA')
    elif (type(record['asserted_tick']) is not int
          or not record['due_tick'] <= record['asserted_tick'] <= device.board.virtual_ticks):
        raise UnsupportedDevice('Finite RAW transfer lacks its physical IRQ assertion')
    if record['state'] == 'complete':
        if (end is None or not _acked(acknowledgments) or type(record['acknowledged_tick']) is not int
                or not acknowledgments[-1]['tick'] <= record['acknowledged_tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Finite RAW transfer completed before real ACK and EOI')
    elif record['acknowledged_tick'] is not None:
        raise UnsupportedDevice('Finite RAW transfer retained an early completion')
    return record


def arm(device, profile, *, scene, codec_profile, delay_ticks=1000):
    if (profile != PROFILE or codec_profile != raw.PROFILE or _record(device) is not None
            or device.board.failure is not None or type(delay_ticks) is not int
            or not 1 <= delay_ticks <= 400000 or device.busy or device.phase_busy or device.dma_busy
            or device.pending_command is not None or device.command_armed is not None):
        raise UnsupportedDevice('Finite RAW transfer requires explicit profiles and idle one-shot admission')
    plan = raw.prepare(device.board, scene=scene, profile=codec_profile)
    now = device.board.virtual_ticks
    if type(now) is not int or not 0 <= now < (1 << 64) - delay_ticks:
        raise UnsupportedDevice('Finite RAW transfer delay escapes virtual time')
    record = dict(profile=profile, codec=codec_profile, geometry=plan['geometry'], source=plan['source'],
                  rows=plan['rows'], rows_sha256=_rows_hash(plan['rows']), requested_tick=now,
                  due_tick=now+delay_ticks, delay_ticks=delay_ticks, asserted_tick=None,
                  acknowledged_tick=None, state='armed', write_cursor=len(device.writes), write_end=None, acknowledgments=[])
    record['seal'] = _seal(record)
    device.raw_readout = record
    return summary(device)


def observe_write(device, core, offset, value):
    record = _record(device)
    if record is None or record['state'] == 'complete' or offset not in (4,0x350,0x354): return
    validate(device)
    candidate = dict(cpu=core,offset=offset,value=value,tick=device.board.virtual_ticks)
    if (record['state'] != 'irq-pending' or type(core) is not int or core != 0
            or _acked(record['acknowledgments']) or not _ack_prefix([*record['acknowledgments'],candidate])):
        raise UnsupportedDevice('Finite RAW transfer permits only stock ordered IRQ retirement')
    record['acknowledgments'].append(candidate)
    record['seal'] = _seal(record)


def tick(device, board):
    record = _record(device)
    if record is None or record['state'] == 'complete': return
    validate(device)
    if record['state'] == 'armed' and board.virtual_ticks >= record['due_tick']:
        if raw._geometry(board) != record['geometry']:
            raise UnsupportedDevice('Finite RAW transfer caller changed before DMA')
        validate(device,pixels=True)
        # Validate every row and live descriptor before the first external write.
        for address,data in record['rows']: board.write_memory(address,data,cpu_id=0)
        board.queue_irq(0,IRQ)
        record.update(state='irq-pending',asserted_tick=board.virtual_ticks)
        record['seal'] = _seal(record)
    elif record['state'] == 'irq-pending' and _acked(record['acknowledgments']):
        banks = board.interrupts.banks
        drained = all(not b.pending & (1<<IRQ) and b.active != IRQ and b.injected != IRQ
                      and all(e.get('line') != IRQ for e in b.active_stack) for b in banks)
        if drained and not banks[0].enabled & (1<<IRQ):
            record.update(state='complete',acknowledged_tick=board.virtual_ticks,write_end=len(device.writes))
            record['seal'] = _seal(record)


def due(device):
    record = _record(device)
    return record['due_tick'] if record is not None and record['state'] == 'armed' else None


def summary(device):
    record = validate(device,pixels=True)
    if record is None: return None
    return deepcopy({key:value for key,value in record.items() if key not in ('rows','seal')})
