"""Explicit remaining still image initialization: passive reset then active arm.

The selected bank0 path is whole10CA754 (36 accesses) followed by unchanged
10CAEA0/92B978/92A9AC (D8, control read and enable). Prior image/AF lifetimes
remain immutable. This supplies no pixels, completion status or interrupt.
"""
from copy import copy,deepcopy
import struct

from .hardware import UnsupportedDevice

PROFILE='declared-still-image-bank0-reset-and-enable-unmeasured'
KEY='activation'
DESCRIPTORS=(0x15f66c4,)
CALLBACKS=(0x11f5c40,)
WORDS=((0x160e4e4,4,0),(0x160e4e8,4,0),(0x160e4f8,4,0x501),
       (0x15f6e50,4,1),(0x15d502c,4,6),(0x15d5091,1,0),
       (0x15f63a8,4,1),(0x15f68a4,4,1),(0x15f64a4,4,0),
       (0x160e570,4,0),(0x15f66a4,4,1),(0x15f66a8,4,0x131db24),
       (0x15f66e8,4,CALLBACKS[0]))
FIELDS={'profile','armed_tick','prior_words','prior_prepare_sha256','contract',
        'contract_sha256','accesses','arms','state','completed_tick'}


def _descriptors():
    data=bytearray(0x8c)
    struct.pack_into('<I',data,0x24,CALLBACKS[0])
    struct.pack_into('<I',data,0x64,1)
    return [data.hex()]


def owner(device):
    from . import image_still_prepare as prepare
    parent=prepare.owner(device)
    return parent.get(KEY) if type(parent) is dict else None


def historical(device):
    record=owner(device)
    if record is None:return device
    proxy=copy(device);proxy.still_reset=dict(device.still_reset)
    proxy.still_reset['configuration']=dict(device.still_reset['configuration'])
    proxy.still_reset['configuration']['prepare']={k:v for k,v in
        device.still_reset['configuration']['prepare'].items() if k!=KEY}
    proxy.registers=dict(record['prior_words'])
    return proxy


def _prior_hash(device):
    from . import image_still_prepare as prepare
    return device._afs_digest({k:v for k,v in prepare.owner(device).items() if k!=KEY})


def _read(device,a,n=4):
    return int.from_bytes(device.board.read_memory(a,n,cpu_id=0),'little')


def _route(device,enabled):
    device._afs_rearm_route(0,enabled)
    device._afs_rearm_route(1,False)


def _expected(device,record):
    before=record['prior_words'];base=device.BASES[0]
    reset=[]
    for op,offset in device.IDLE_RESET_SEQUENCE:
        value=(before[base+offset] if op=='read' else
               0x10001000 if offset==0x10 else 0 if offset==0x1c else
               before[base+0x20] if offset==0x20 else 0x10000)
        reset.append((op,base+offset,value))
    return reset+[('write',base+0xd8,1),('read',base,1),('write',base,0x10001)]


def arm(device,profile):
    from . import image_still_prepare as prepare
    if profile!=PROFILE or owner(device) is not None or device.board.failure is not None:
        raise UnsupportedDevice('Still image activation requires explicit one-shot admission')
    parent=prepare.validate(device)
    if (parent is None or parent['state']!='complete' or len(parent['accesses'])!=35
            or any(_read(device,a,n)!=v for a,n,v in WORDS)
            or any(device.registers[base]!=1 for base in device.BASES)
            or _read(device,0x160e538)==0 or _read(device,0x160e538)!=device.registers[device.BASES[0]+0xc0]
            or any(_read(device,0xccc1f4+16*c)!=0 for c in (0,1))
            or any(_read(device,0x15233ac+4*c)!=0 for c in (0,1))):
        raise UnsupportedDevice('Still image activation differs from its selected bank0 quiet initializer')
    _route(device,False)
    descriptors=[bytes(device.board.read_memory(a,0x8c,cpu_id=0)).hex() for a in DESCRIPTORS]
    expected_descriptors=_descriptors()
    if descriptors!=expected_descriptors:
        raise UnsupportedDevice('Still image activation requires the real mode10 still descriptor/callback shape')
    contract=dict(reset_mask=0x501,channels=[0],outputs=[_read(device,0x160e538),0],
                  descriptors=descriptors,descriptor_addresses=list(DESCRIPTORS),words=[list(x) for x in WORDS])
    parent[KEY]=dict(profile=PROFILE,armed_tick=device.board.virtual_ticks,prior_words=dict(device.registers),
        prior_prepare_sha256=_prior_hash(device),contract=contract,contract_sha256=device._afs_digest(contract),
        accesses=[],arms=[],state='armed',completed_tick=None)
    return summary(device)


def validate(device,*,history=True):
    record=owner(device)
    if record is None:return None
    if 'data' in record:
        from . import afs_still_image_delivery as delivery
        delivery.validate(device,history=history)
        return validate(delivery.historical(device),history=history)
    c=record.get('contract')
    if (set(record)!=FIELDS or record['profile']!=PROFILE
            or record['prior_prepare_sha256']!=_prior_hash(device)
            or type(c) is not dict or set(c)!={'reset_mask','channels','outputs','descriptors','descriptor_addresses','words'}
            or (c['reset_mask'],c['channels'],c['descriptor_addresses'],c['words'])!=
                (0x501,[0],list(DESCRIPTORS),[list(x) for x in WORDS])
            or c['outputs']!=[record['prior_words'][device.BASES[0]+0xc0],0] or c['outputs'][0]==0
            or c['descriptors']!=_descriptors()
            or record['contract_sha256']!=device._afs_digest(c)
            or type(record['armed_tick']) is not int or not 0<=record['armed_tick']<=device.board.virtual_ticks
            or type(record['accesses']) is not list or len(record['accesses'])>39):
        raise UnsupportedDevice('Still image reset/enable identity or predecessor changed')
    if history:
        from . import image_still_prepare as prepare
        prepare.validate(historical(device))
    words=dict(record['prior_words']);previous=record['armed_tick'];expected=_expected(device,record)
    for index,event in enumerate(record['accesses']):
        if (type(event) is not dict or set(event)!={'operation','cpu','address','word','old_word','tick'}
                or event['cpu']!=0 or (event['operation'],event['address'],event['word'])!=expected[index]
                or event['old_word']!=words[event['address']]
                or type(event['tick']) is not int or not previous<=event['tick']<=device.board.virtual_ticks):
            raise UnsupportedDevice('Still image reset/enable lost its whole39-access order')
        if event['operation']=='write':words[event['address']]=event['word']
        previous=event['tick']
    count=len(record['accesses']);arms=[]
    if count==39:
        if type(record['arms']) is not list or len(record['arms'])!=1:
            raise UnsupportedDevice('Still image activation lost its selected output arm')
        item=record['arms'][0]
        expected_arm=dict(channel=0,destination=c['outputs'][0],descriptor_address=DESCRIPTORS[0],
            descriptor_sha256=device._afs_digest(c['descriptors'][0]),callback=CALLBACKS[0],
            requested_tick=previous,logical_irq=0x2f,physical_irq=0x2c,isr=0x92a71c,
            priority=10,source_type=1,desired_control=0x10001)
        if (type(item) is not dict or set(item)!=set(expected_arm)|{'handle'}
                or any(item[k]!=v for k,v in expected_arm.items())
                or type(item['handle']) is not int or item['handle']==0):
            raise UnsupportedDevice('Still image activation changed its actual registrar/output ownership')
        arms=[item]
    if (record['arms']!=arms or record['state']!=('complete' if count==39 else 'applying' if count else 'armed')
            or record['completed_tick']!=(previous if count==39 else None) or device.registers!=words):
        raise UnsupportedDevice('Still image reset/enable register/arm projection changed')
    return record


def access(device,operation,core,channel,offset,value=None):
    record=validate(device,history=False)
    if record is None:return False
    index=len(record['accesses']);address=device.BASES[channel]+offset
    word=device.registers[address] if operation=='read' else value
    if (type(core) is not int or core!=0 or index>=39
            or (operation,address,word)!=_expected(device,record)[index]):
        raise UnsupportedDevice('Still image activation requires its next reset/registration/enable access')
    if index>=36:
        if bytes(device.board.read_memory(DESCRIPTORS[0],0x8c,cpu_id=0)).hex()!=record['contract']['descriptors'][0]:
            raise UnsupportedDevice('Still image descriptor changed before active registration')
        if index==36:_route(device,False)
        else:
            _route(device,True)
            if (_read(device,0xccc1f0)!=CALLBACKS[0] or _read(device,0xccc1f4)!=1
                    or _read(device,0x15233ac)==0):
                raise UnsupportedDevice('Still image active arm lacks actual stock registrar/callback')
    now=device.board.virtual_ticks;old=device.registers[address]
    record['accesses'].append(dict(operation=operation,cpu=core,address=address,word=word,old_word=old,tick=now))
    if operation=='write':
        device.registers[address]=word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old),tick=now))
    record['state']='complete' if index==38 else 'applying'
    if index==38:
        c=record['contract'];record['completed_tick']=now
        record['arms']=[dict(channel=0,destination=c['outputs'][0],descriptor_address=DESCRIPTORS[0],
            descriptor_sha256=device._afs_digest(c['descriptors'][0]),callback=CALLBACKS[0],
            requested_tick=now,logical_irq=0x2f,physical_irq=0x2c,isr=0x92a71c,priority=10,
            source_type=1,desired_control=0x10001,handle=_read(device,0x15233ac))]
    return True


def summary(device):
    record=validate(device)
    return deepcopy(record) if record is not None else None
