"""Explicit single-bank desired setters after the stock still quiet reset.

This qualifies the ten image stores made by10CBB44(mask800 or80C). It supplies no DMA,
status transition, pixels, IRQ or software completion for the next lifetime.
"""
from copy import copy, deepcopy
import struct

from .hardware import UnsupportedDevice

PROFILE = 'declared-still-image-bank0-desired-configuration-unmeasured'
KEY = 'configuration'
PARAMETERS = 0x160e504
MASKS = (0x800,0x80c)  # Image-only fixture; actual still dirtymask also selects Input.
WORDS = ((0x160e4e4,4,0x800),(0x15f6e50,4,1),(0x15f64a4,4,0),
         (0x15d502c,4,6),(0x15f63a8,4,1),(0x15f68a4,4,1),
         (0x15d5088,4,2),(0x15d5090,1,1),(0x15d5091,1,0),
         (0x1625f48,4,0),(0x1625f4c,4,0))
FIELDS = {'profile','armed_tick','prior_words','prior_reset_sha256','contract',
          'contract_sha256','accesses','state','completed_tick'}


def caller_words(mask):
    return [(address,size,mask if address==0x160e4e4 else value)
            for address,size,value in WORDS]


def owner(device):
    parent = getattr(device, 'still_reset', None)
    return parent.get(KEY) if type(parent) is dict else None


def historical(device):
    record = owner(device)
    if record is None:
        return device
    proxy = copy(device)
    proxy.still_reset = {k:v for k,v in device.still_reset.items() if k != KEY}
    proxy.registers = dict(record['prior_words'])
    return proxy


def _reset_hash(device):
    return device._afs_digest({k:v for k,v in device.still_reset.items() if k != KEY})


def words(device, parameters):
    """Pack only the selected bank, using the seven actual setter domains."""
    if type(parameters) is not str or len(parameters) != 256:
        raise UnsupportedDevice('Still image desired setters need the full128-byte caller block')
    try:
        data = bytes.fromhex(parameters)
    except ValueError as error:
        raise UnsupportedDevice('Still image caller block is malformed') from error
    if len(data) != 128 or data.hex() != parameters:
        raise UnsupportedDevice('Still image caller block is not canonical hex')
    u16 = lambda offset: struct.unpack_from('<H',data,8+offset)[0]
    u32 = lambda offset: struct.unpack_from('<I',data,8+offset)[0]
    if (not all(1 <= u16(o) <= 4096 for o in (0,2)) or u32(4) not in (0,1)
            or any(u32(o)>0x1fff for o in (12,16))
            or any(not 8<=u16(o)<=2048 or u16(o)%8 for o in (20,26))
            or not 1<=u16(22)<=2048 or u16(24)>2040 or u16(24)%8
            or u16(32)>=256 or u32(36)>3 or u32(40)>4
            or u32(44)>0x3ffffff0 or u32(44)%16
            or u16(48)>0x1ffc or u16(48)%8
            or data[60]>15 or data[61]>15):
        raise UnsupportedDevice('Still image caller fields exceed the real seven setter domains')
    result = [(u16(0)-1)|((u16(2)-1)<<16), (u32(4)==1)<<31|u16(8),
              u16(10), u32(12)|u32(16)<<16,
              (u16(20)-1)|((u16(22)-1)<<16), (u16(24)<<16)|(u16(26)-1),
              (u32(28)==1)<<24|u16(32)<<16|u32(36)<<4|u32(40),
              u32(44),u16(48),data[60]]
    if any(not device._config_valid(offset,value)
           for offset,value in zip(device.CONFIG_ORDER,result)):
        raise UnsupportedDevice('Still image packed desired fields exceed the hardware word domains')
    return result


def _clock(device):
    sources = [d for d in device.board.devices if type(d).__name__ == 'AfsFrameSource']
    producers = [d for d in device.board.devices if type(d).__name__ == 'ProducerControl']
    if len(sources)!=1 or len(producers)!=1:
        raise UnsupportedDevice('Still image configuration needs its unique readout and numeric owners')
    clock = sources[0].still_readout_clock_summary()
    anchor = producers[0].still_capture_source_anchor()
    if (clock is None or clock['state']!='acknowledged' or clock['eoi_tick'] is None
            or clock['generation']!=9 or clock['physical_irq']!='0x29'
            or clock['contract']['producer']!=anchor):
        raise UnsupportedDevice('Still image configuration lacks the real readout ACK/EOI and initializer')
    return dict(input_sha256=clock['input_sha256'],asserted_tick=clock['asserted_tick'],
                acknowledged_tick=clock['acknowledged_tick'],eoi_tick=clock['eoi_tick'],producer=anchor)


def arm(device, profile):
    from . import image_still_reset as reset
    if (profile!=PROFILE or owner(device) is not None or device.board.failure is not None
            or reset.owner(device) is None):
        raise UnsupportedDevice('Still image desired configuration requires its explicit one-shot profile')
    prior = reset.validate(device)
    read = lambda a,n: int.from_bytes(device.board.read_memory(a,n,cpu_id=0),'little')
    mask = read(0x160e4e4,4)
    if (prior['phase']!='completed' or prior['channel']!=2 or mask not in MASKS
            or any(read(a,n)!=v for a,n,v in caller_words(mask))
            or any(device.registers[base]!=1 for base in device.BASES)
            or any(b.enabled&((1<<44)|(1<<45)) or b.pending&((1<<44)|(1<<45))
                   or b.active in (44,45) or b.injected in (44,45)
                   for b in device.board.interrupts.banks)):
        raise UnsupportedDevice('Still desired configuration differs from the actual quiet mask800 owner')
    parameters = bytes(device.board.read_memory(PARAMETERS,128,cpu_id=0)).hex()
    contract = dict(mask=mask,channel=0,parameters_address=PARAMETERS,parameters=parameters,
                    expected=words(device,parameters),clock=_clock(device),words=[list(x) for x in caller_words(mask)])
    device.still_reset[KEY] = dict(profile=PROFILE,armed_tick=device.board.virtual_ticks,
        prior_words=dict(device.registers),prior_reset_sha256=_reset_hash(device),
        contract=contract,contract_sha256=device._afs_digest(contract),accesses=[],
        state='armed',completed_tick=None)
    return summary(device)


def validate(device, *, history=True):
    record = owner(device)
    if record is None:
        return None
    if 'prepare' in record:
        from . import image_still_prepare as prepare
        prepare.validate(device,history=history)
        return validate(prepare.historical(device),history=history)
    contract = record.get('contract')
    if (set(record)!=FIELDS or record['profile']!=PROFILE
            or record['prior_reset_sha256']!=_reset_hash(device)
            or type(contract) is not dict or set(contract)!=
               {'mask','channel','parameters_address','parameters','expected','clock','words'}
            or type(contract['mask']) is not int or contract['mask'] not in MASKS
            or (contract['channel'],contract['parameters_address'])!=(0,PARAMETERS)
            or contract['words']!=[list(x) for x in caller_words(contract['mask'])]
            or contract['expected']!=words(device,contract['parameters'])
            or record['contract_sha256']!=device._afs_digest(contract)
            or type(record['armed_tick']) is not int or not 0<=record['armed_tick']<=device.board.virtual_ticks
            or type(record['accesses']) is not list or len(record['accesses'])>10):
        raise UnsupportedDevice('Still image desired configuration identity or predecessor changed')
    if history:
        from . import image_still_reset as reset
        reset.validate(historical(device))
        if contract['clock']!=_clock(device):
            raise UnsupportedDevice('Still image desired configuration readout history changed')
    projected = dict(record['prior_words'])
    previous = record['armed_tick']
    for index,event in enumerate(record['accesses']):
        address=device.BASES[0]+device.CONFIG_ORDER[index]
        if (type(event) is not dict or set(event)!= {'cpu','address','word','old_word','tick'}
                or event['cpu']!=0 or event['address']!=address
                or event['word']!=contract['expected'][index] or event['old_word']!=projected[address]
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Still image desired stores lost their exact typed order')
        projected[address]=event['word'];previous=event['tick']
    count=len(record['accesses'])
    if (record['state']!=('complete' if count==10 else 'applying' if count else 'armed')
            or record['completed_tick']!=(previous if count==10 else None)
            or device.registers!=projected):
        raise UnsupportedDevice('Still image desired register or completion projection changed')
    return record


def access(device, operation, core, channel, offset, value=None):
    record=validate(device,history=False)
    if record is None:
        return False
    index=len(record['accesses'])
    if (type(core) is not int or core!=0 or index>=10
            or (operation,channel,offset,value)!=
               ('write',0,device.CONFIG_ORDER[index],record['contract']['expected'][index])):
        raise UnsupportedDevice('Still desired configuration requires its exact next bank0 setter')
    parameters=bytes(device.board.read_memory(PARAMETERS,128,cpu_id=0)).hex()
    if parameters!=record['contract']['parameters']:
        raise UnsupportedDevice('Still desired caller block changed during its setter transaction')
    address=device.BASES[0]+offset;old=device.registers[address];now=device.board.virtual_ticks
    record['accesses'].append(dict(cpu=core,address=address,word=value,old_word=old,tick=now))
    device.registers[address]=value
    device.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old),tick=now))
    record['state']='complete' if index==9 else 'applying'
    if index==9:record['completed_tick']=now
    return True


def summary(device):
    record=validate(device)
    return deepcopy(record) if record is not None else None
