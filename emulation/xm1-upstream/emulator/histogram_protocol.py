"""Plain, CPU-independent helpers for the declared channel1 experiment.

These functions perform no RAM, MMIO, IRQ, timer or guest operation. The
integrating device must preflight/apply their intents and run the real ISR.
They describe software construction and explicitly assumed latching, not a
recovered histogram law or hardware DMA count.
"""
from copy import deepcopy
import hashlib
import json
import struct

from .hardware import UnsupportedDevice


PROFILE = 'declared-postisp-histogram256-u32-unmeasured'
PROFILES = (PROFILE,)
BASE = 0xfff18000
DESTINATION_A = 0x5fecd010
DESTINATION_B = 0x5fecf9c0
PAYLOAD_BYTES = 1024
MAX_SAMPLES = 2
SOURCE_PROFILE = 'stock-input0-three-startup-plus-three-source0-edges-unmeasured'
DISPLAY_SOURCE_PROFILE = 'stock-input0-three-startup-plus-four-source0-edges-long-final-gap-unmeasured'
DUE_ROUTE_DISPLAY_SOURCE_PROFILE='declared-rec-display-input-edges-due-route-unmeasured'
MENU_CLOSE_SOURCE_PROFILE='declared-rec-display-plus-one-close-source1-edge-unmeasured'
PREVIEW_REENTRY_SOURCE_PROFILE='declared-rec-display-close-plus-one-reentry-source1-edge-unmeasured'
PREVIEW_READY_SOURCE_PROFILE='declared-rec-display-close-reentry-plus-one-ready-source0-edge-unmeasured'
CARD_HISTOGRAM_SOURCE_PROFILE='declared-rec-display-preview-ready-card-late-histogram-epoch-unmeasured'
CARD_STAGING_SOURCE_PROFILE='declared-rec-display-card-late-source-staging-unmeasured'
CARD_HISTOGRAM_SOURCE_PROFILES=(CARD_HISTOGRAM_SOURCE_PROFILE,CARD_STAGING_SOURCE_PROFILE)
SOURCE_PROFILES = (SOURCE_PROFILE,DISPLAY_SOURCE_PROFILE,DUE_ROUTE_DISPLAY_SOURCE_PROFILE,
                   MENU_CLOSE_SOURCE_PROFILE,PREVIEW_REENTRY_SOURCE_PROFILE,PREVIEW_READY_SOURCE_PROFILE,
                   *CARD_HISTOGRAM_SOURCE_PROFILES)

#92F360/404,92F584,10CA630(mask40),92CD04. The three RMW reads are real.
SETUP_SEQUENCE = (
    ('write',BASE+0x700,DESTINATION_A),('write',BASE+0x704,0x200),
    ('write',BASE+0x98c,0),('write',BASE+0x9f4,1),
    ('write',BASE+0x9a0,0x80403000),('write',BASE+0x9f8,0),
    ('write',BASE+0x9c4,2),('write',BASE+0x9f8,0x10000000),
    ('read',BASE+0x984,0),('write',BASE+0x984,1),
    ('read',BASE+4,0x33173317),('read',BASE+8,0),
    ('write',BASE+4,0x33173317),('write',BASE+8,0x10001))

#10CB2F4(mask40),92CD04 and92F6EC: no whole-engine stop or IRQ close.
RELEASE_SEQUENCE = (
    ('read',BASE+4,0x33173317),('read',BASE+8,0x10001),
    ('write',BASE+4,0x33173317),('write',BASE+8,0x10000),
    ('read',BASE+0x984,1),('write',BASE+0x984,0),
    ('read',BASE+0x990,0),('write',BASE+0x9f8,0))


def _reject(message):
    raise UnsupportedDevice('Histogram channel1: '+message)


def uint(value, bits=32):
    if type(value) is not int or not 0 <= value < 1 << bits:
        _reject('unknown unsigned value/type')
    return value


def positive_ticks(value):
    if type(value) is not int or not 1 <= value < 1 << 64:
        _reject('delay must be a positive integer')
    return value


def digest(value):
    """Deterministic plain-state fingerprint, without hidden cached state."""
    try:
        encoded=json.dumps(value,sort_keys=True,separators=(',',':'),allow_nan=False).encode()
    except (TypeError,ValueError,OverflowError):
        _reject('unknown nonserializable plain state')
    return hashlib.sha256(encoded).hexdigest()


def source_epoch(event):
    """Validate the shared projection supplied from genuine device history.

    The caller must select an asserted/acknowledged InputProcessingControl
    entry. No guest RAM flag, callback or PC is consulted here.
    """
    fields=('profile','generation','ordinal','attempt','cpu','source',
            'physical_irq','asserted_tick','frame')
    if not isinstance(event,dict) or set(event)!=set(fields):
        _reject('unknown external epoch schema')
    if (event['profile'] not in SOURCE_PROFILES or event['physical_irq']!='0x27'
            or event['generation']!=3 or event['cpu']!=0 or event['source']!=0):
        _reject('external epoch is outside the declared REC source0 scope')
    for field in ('generation','ordinal','attempt','cpu','source','asserted_tick','frame'):
        uint(event[field],64)
    limit=5 if event['profile'] in CARD_HISTOGRAM_SOURCE_PROFILES else 4
    if (not 2<=event['ordinal']<=limit or event['attempt']!=event['ordinal']+2
            or event['frame']!=event['ordinal']-1):
        _reject('external epoch ordinal/attempt/declared scene frame disagree')
    return dict(event)


def epoch_identity(event):
    event=source_epoch(event)
    return tuple(event[k] for k in ('profile','generation','ordinal','attempt','asserted_tick'))


def payload_words(words):
    """Exactly256 LE U32 values, unrelated to the firmware704 count units."""
    if not isinstance(words,(list,tuple)) or len(words)!=256:
        _reject('declared payload requires exactly256 unsigned WORDs')
    return struct.pack('<256I',*(uint(v) for v in words))


def sample_request(*,epoch,destination,count_word,crop_words,payload,
                   provenance,now,data_ticks,route,previous=()):
    """Build a plain, explicitly latched data intent; it applies no effects."""
    epoch=source_epoch(epoch);uint(now,64);positive_ticks(data_ticks)
    uint(destination);uint(count_word)
    if destination not in (DESTINATION_A,DESTINATION_B) or count_word!=0x200:
        _reject('unknown destination/count contract')
    if route not in (0,1) or type(route) is not int:
        _reject('unknown or ambiguous sharedIRQ2E route')
    if (not isinstance(crop_words,(tuple,list)) or len(crop_words)!=2
            or any(type(v) is not int for v in crop_words)
            or tuple(crop_words)!=(0x000c098f,0x00200328)):
        _reject('crop differs from the current12/2448/31/809 request')
    if type(payload) is not bytes or len(payload)!=PAYLOAD_BYTES:
        _reject('declared payload is not exactly1024 immutable bytes')
    if not isinstance(provenance,dict) or not provenance:
        _reject('declared post-ISP law/input provenance is missing')
    if not isinstance(previous,(list,tuple)) or len(previous)>=MAX_SAMPLES:
        _reject('two-sample experiment is exhausted')
    if now < epoch['asserted_tick'] or now+data_ticks >= 1 << 64:
        _reject('source assertion/due tick is outside the request interval')
    identities=[]
    for index,record in enumerate(previous):
        validate_sample(record)
        if record['ordinal']!=index+1:_reject('prior sample ordinal is inconsistent')
        identities.append(epoch_identity(record.get('epoch')))
    identity=epoch_identity(epoch)
    if identity in identities or any(identity[2]<=x[2] or identity[4]<=x[4] for x in identities):
        _reject('duplicate, older or nonmonotonic external epoch')
    #Round-trip proves this remains ordinary serializable experiment data.
    digest(provenance)
    provenance=json.loads(json.dumps(provenance,sort_keys=True,allow_nan=False))
    result=dict(profile=PROFILE,ordinal=len(previous)+1,epoch=epoch,
        identity=list(identity),state='pending-declared-postisp-input',
        latched_destination=hex(destination),count_word=count_word,
        crop_words=list(crop_words),payload_bytes=PAYLOAD_BYTES,
        payload_hex=payload.hex(),payload_sha256=hashlib.sha256(payload).hexdigest(),
        provenance=provenance,provenance_sha256=digest(provenance),
        route=route,requested_tick=now,due_tick=now+data_ticks,
        data_ticks=data_ticks,
        limits='Explicit post-ISP/latching/source delay; no704-derived length or fresh channel0 result')
    result['request_sha256']=digest(result)
    return result


def validate_sample(record):
    """Validate an immutable data intent, independently of delivery history."""
    fields=('profile','ordinal','epoch','identity','state','latched_destination',
        'count_word','crop_words','payload_bytes','payload_hex','payload_sha256',
        'provenance','provenance_sha256','route','requested_tick','due_tick',
        'data_ticks','limits','request_sha256')
    if not isinstance(record,dict) or set(record)!=set(fields):
        _reject('unknown sample intent schema')
    if (record['profile']!=PROFILE or type(record['ordinal']) is not int
            or not 1<=record['ordinal']<=MAX_SAMPLES
            or record['state']!='pending-declared-postisp-input'):
        _reject('unknown sample intent profile/ordinal/state')
    epoch=source_epoch(record['epoch'])
    if record['identity']!=list(epoch_identity(epoch)):
        _reject('sample identity changed outside its source epoch')
    if (record['latched_destination'] not in (hex(DESTINATION_A),hex(DESTINATION_B))
            or type(record['count_word']) is not int or record['count_word']!=0x200
            or record['crop_words']!=[0x000c098f,0x00200328]
            or any(type(v) is not int for v in record['crop_words'])
            or type(record['payload_bytes']) is not int or record['payload_bytes']!=PAYLOAD_BYTES):
        _reject('sample destination/count/crop/span changed')
    if not isinstance(record['payload_hex'],str):_reject('unknown payload serialization')
    try:payload=bytes.fromhex(record['payload_hex'])
    except ValueError:_reject('unknown payload serialization')
    if (len(payload)!=PAYLOAD_BYTES or payload.hex()!=record['payload_hex']
            or hashlib.sha256(payload).hexdigest()!=record['payload_sha256']):
        _reject('sample payload bytes/hash changed')
    if (not isinstance(record['provenance'],dict) or not record['provenance']
            or digest(record['provenance'])!=record['provenance_sha256']):
        _reject('sample source law/provenance changed')
    if type(record['route']) is not int or record['route'] not in (0,1):
        _reject('sample route is ambiguous')
    now=uint(record['requested_tick'],64);due=uint(record['due_tick'],64)
    delay=positive_ticks(record['data_ticks'])
    if now<epoch['asserted_tick'] or due!=now+delay:
        _reject('sample due/source interval changed')
    projected={k:v for k,v in record.items() if k!='request_sha256'}
    if digest(projected)!=record['request_sha256']:
        _reject('sample intent changed outside its request fingerprint')
    return True


def new_ledger(sequence, initial_words, now):
    """Inactive ordered state suitable for exact checkpoint rollback."""
    uint(now,64)
    if sequence not in ('setup','release'):_reject('unknown ledger kind')
    if not isinstance(initial_words,dict):_reject('unknown initial word projection')
    prior={hex(uint(a)):uint(v) for a,v in initial_words.items()}
    return dict(kind=sequence,step=0,state='partial',prior_words=prior,
        current_words=dict(prior),accesses=[],started_tick=now,
        ledger_sha256=digest([]))


def advance_ledger(record,operation,address,value,cpu,now):
    """Return a new validated ledger; the integrating device commits MMIO."""
    validate_ledger(record);uint(address);uint(value);uint(now,64)
    if type(cpu) is not int or cpu not in (0,1):_reject('unknown CPU identity')
    sequence=SETUP_SEQUENCE if record['kind']=='setup' else RELEASE_SEQUENCE
    step=record['step']
    if step>=len(sequence) or (operation,address,value)!=sequence[step]:
        _reject('ordered transaction differs from the original selected request')
    if now<record['started_tick'] or record['accesses'] and now<record['accesses'][-1]['tick']:
        _reject('transaction tick moved backward')
    result=deepcopy(record)
    result['accesses'].append(dict(step=step,cpu=cpu,operation=operation,
        address=hex(address),word=value,tick=now))
    if operation=='write':result['current_words'][hex(address)]=value
    elif result['current_words'].get(hex(address))!=value:
        _reject('readback differs from retained request')
    result['step']+=1
    result['state']='completed' if result['step']==len(sequence) else 'partial'
    result['ledger_sha256']=digest(result['accesses'])
    return result


def validate_ledger(record):
    if not isinstance(record,dict) or record.get('kind') not in ('setup','release'):
        _reject('unknown plain ledger')
    sequence=SETUP_SEQUENCE if record['kind']=='setup' else RELEASE_SEQUENCE
    if (type(record.get('step')) is not int or not 0<=record['step']<=len(sequence)
            or not isinstance(record.get('accesses'),list)
            or len(record['accesses'])!=record['step']
            or record.get('state')!=('completed' if record['step']==len(sequence) else 'partial')):
        _reject('ledger count/state is inconsistent')
    uint(record.get('started_tick'),64)
    for key in ('prior_words','current_words'):
        if not isinstance(record.get(key),dict):_reject('unknown word projection')
        for a,v in record[key].items():
            try:address=int(a,16) if isinstance(a,str) else None
            except ValueError:_reject('unknown projected address')
            if not isinstance(a,str) or a!=hex(uint(address)):_reject('unknown projected address')
            uint(v)
    current=dict(record['prior_words']);last=record['started_tick']
    for index,event in enumerate(record['accesses']):
        if not isinstance(event,dict) or set(event)!=set(('step','cpu','operation','address','word','tick')):
            _reject('unknown access schema')
        uint(event['step']);uint(event['word']);uint(event['tick'],64)
        op,address,word=sequence[index]
        if (event['step']!=index or type(event['cpu']) is not int or event['cpu'] not in (0,1)
                or (event['operation'],event['address'],event['word'])!=(op,hex(address),word)
                or event['tick']<last):_reject('access type/order/value/tick is inconsistent')
        last=event['tick']
        if op=='write':current[hex(address)]=word
        elif current.get(hex(address))!=word:_reject('historical read differs from retained word')
    if current!=record['current_words'] or digest(record['accesses'])!=record.get('ledger_sha256'):
        _reject('ledger words/hash changed outside its ordered history')
    return True
