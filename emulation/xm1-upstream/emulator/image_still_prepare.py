"""One explicit bank0 passive mode preparation after still desired setters.

Stock10CB618(mask100) packs the caller block through92ABF8/92ADB4/92AE94/
92AFEC. This records35 stores and preserves the prior image lifetimes; no
pixels, active arm, status transition, IRQ or software completion is supplied.
"""
from copy import copy, deepcopy
import struct

from .hardware import UnsupportedDevice

PROFILE = 'declared-still-image-bank0-passive-prepare-unmeasured'
KEY = 'prepare'
PARAMETERS = 0x160e988
SIZE = 0x90
WORDS = ((0x160e4e4,4,0),(0x160e4e8,4,0x100),(0x160e4f8,4,0x501),
         (0x15f6e50,4,1),(0x15d502c,4,6),(0x15d5091,1,0),
         (0x15f63a8,4,1),(0x15f68a4,4,1),(0x15f64a4,4,0))
FIELDS = {'profile','armed_tick','prior_words','prior_configuration_sha256','contract',
          'contract_sha256','accesses','state','completed_tick'}


def owner(device):
    from . import image_still_configuration as configuration
    parent=configuration.owner(device)
    return parent.get(KEY) if type(parent) is dict else None


def historical(device):
    record=owner(device)
    if record is None:return device
    proxy=copy(device)
    proxy.still_reset=dict(device.still_reset)
    proxy.still_reset['configuration']={k:v for k,v in device.still_reset['configuration'].items() if k!=KEY}
    proxy.registers=dict(record['prior_words'])
    return proxy


def _prior_hash(device):
    return device._afs_digest({k:v for k,v in device.still_reset['configuration'].items() if k!=KEY})


def words(device,parameters):
    if type(parameters) is not str or len(parameters)!=2*SIZE:
        raise UnsupportedDevice('Still passive preparation requires its whole144-byte caller block')
    try:data=bytes.fromhex(parameters)
    except ValueError as error:raise UnsupportedDevice('Still passive caller block is malformed') from error
    if len(data)!=SIZE or data.hex()!=parameters:
        raise UnsupportedDevice('Still passive caller block is not canonical hex')
    u16=lambda o:struct.unpack_from('<H',data,o)[0]
    u32=lambda o:struct.unpack_from('<I',data,o)[0]
    if (any(u16(o)>0x1fff for o in (0,2)) or any(u32(o)>0x1fff for o in (4,8))
            or any(u16(o)>0x7fff for o in (12,14)) or data[16]>8 or data[17]>0x20):
        raise UnsupportedDevice('Still passive caller exceeds the unchanged setter domains')
    result=[u16(0)|u16(2)<<16,u32(4)|u32(8)<<16,u16(12)|u16(14)<<16,
            data[16]|data[17]<<16]+[u32(o)&0x1ffff for o in range(20,SIZE,4)]
    if any(not device._config_valid(offset,value)
           for offset,value in zip(device.PASSIVE_CONFIG_ORDER,result)):
        raise UnsupportedDevice('Still passive packed fields exceed known register domains')
    return result


def arm(device,profile):
    from . import image_still_configuration as configuration
    if profile!=PROFILE or owner(device) is not None or device.board.failure is not None:
        raise UnsupportedDevice('Still passive preparation requires explicit one-shot admission')
    parent=configuration.validate(device)
    read=lambda a,n:int.from_bytes(device.board.read_memory(a,n,cpu_id=0),'little')
    if (parent is None or parent['state']!='complete' or len(parent['accesses'])!=10
            or any(read(a,n)!=v for a,n,v in WORDS)
            or any(device.registers[base]!=1 for base in device.BASES)
            or any(b.enabled&((1<<44)|(1<<45)) or b.pending&((1<<44)|(1<<45))
                   or b.active in (44,45) or b.injected in (44,45)
                   for b in device.board.interrupts.banks)):
        raise UnsupportedDevice('Still passive preparation differs from the exact mask100 quiet owner')
    parameters=bytes(device.board.read_memory(PARAMETERS,SIZE,cpu_id=0)).hex()
    contract=dict(mask=0x100,channel=0,parameters_address=PARAMETERS,parameters=parameters,
                  expected=words(device,parameters),words=[list(x) for x in WORDS])
    parent[KEY]=dict(profile=PROFILE,armed_tick=device.board.virtual_ticks,prior_words=dict(device.registers),
        prior_configuration_sha256=_prior_hash(device),contract=contract,
        contract_sha256=device._afs_digest(contract),accesses=[],state='armed',completed_tick=None)
    return summary(device)


def validate(device,*,history=True):
    record=owner(device)
    if record is None:return None
    if 'activation' in record:
        from . import image_still_activation as activation
        activation.validate(device,history=history)
        return validate(activation.historical(device),history=history)
    contract=record.get('contract')
    if (set(record)!=FIELDS or record['profile']!=PROFILE
            or record['prior_configuration_sha256']!=_prior_hash(device)
            or type(contract) is not dict or set(contract)!=
               {'mask','channel','parameters_address','parameters','expected','words'}
            or (contract['mask'],contract['channel'],contract['parameters_address'])!=(0x100,0,PARAMETERS)
            or contract['words']!=[list(x) for x in WORDS]
            or contract['expected']!=words(device,contract['parameters'])
            or record['contract_sha256']!=device._afs_digest(contract)
            or type(record['armed_tick']) is not int or not 0<=record['armed_tick']<=device.board.virtual_ticks
            or type(record['accesses']) is not list or len(record['accesses'])>35):
        raise UnsupportedDevice('Still passive preparation identity or predecessor changed')
    if history:
        from . import image_still_configuration as configuration
        configuration.validate(historical(device))
    projected=dict(record['prior_words']);previous=record['armed_tick']
    for index,event in enumerate(record['accesses']):
        address=device.BASES[0]+device.PASSIVE_CONFIG_ORDER[index]
        if (type(event) is not dict or set(event)!={'cpu','address','word','old_word','tick'}
                or event['cpu']!=0 or event['address']!=address
                or event['word']!=contract['expected'][index] or event['old_word']!=projected[address]
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Still passive preparation lost its exact35-store typed order')
        projected[address]=event['word'];previous=event['tick']
    count=len(record['accesses'])
    if (record['state']!=('complete' if count==35 else 'applying' if count else 'armed')
            or record['completed_tick']!=(previous if count==35 else None) or device.registers!=projected):
        raise UnsupportedDevice('Still passive preparation register/completion projection changed')
    return record


def access(device,operation,core,channel,offset,value=None):
    record=validate(device,history=False)
    if record is None:return False
    index=len(record['accesses'])
    if (type(core) is not int or core!=0 or index>=35
            or (operation,channel,offset,value)!=
               ('write',0,device.PASSIVE_CONFIG_ORDER[index],record['contract']['expected'][index])):
        raise UnsupportedDevice('Still passive preparation requires its exact next bank0 setter')
    if bytes(device.board.read_memory(PARAMETERS,SIZE,cpu_id=0)).hex()!=record['contract']['parameters']:
        raise UnsupportedDevice('Still passive caller changed during its whole setter transaction')
    address=device.BASES[0]+offset;old=device.registers[address];now=device.board.virtual_ticks
    record['accesses'].append(dict(cpu=core,address=address,word=value,old_word=old,tick=now))
    device.registers[address]=value
    device.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old),tick=now))
    record['state']='complete' if index==34 else 'applying'
    if index==34:record['completed_tick']=now
    return True


def summary(device):
    record=validate(device)
    return deepcopy(record) if record is not None else None
