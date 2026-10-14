"""One stock10CB618(mask3D0) passive Producer/Image table transaction.

The optional owner retains current physical fields separately from all prior
reset, geometry, AF and still-DMA histories. Firmware alone uploads the
calibration words. No pixels, DMA, result, IRQ or callback are supplied.
"""
from copy import deepcopy
import hashlib
import struct

from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R1
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-post-prefilter-producer-image-passive-configuration-unmeasured'
ATTRIBUTE = 'processing_passive_configuration'
FIRST = (0x934ae8, 0x10cb73c)
CALLER_BYTES = 0x254
TABLES = ((0x54,56),(0x58,56),*((o,1024) for o in range(0x60,0x78,4)))
WORDS = {0x15e32d0:4,0x15d502c:0,0x15f63a4:3,0x15f63a8:0,
    0x15f64a4:0,0x15f68a4:0,0x15f68a8:0,0x15f6bf4:0,0x15f6e50:1,
    0x173d308:1,0x152b724:6,0x160e4e4:0,0x160e4e8:0x3d0,
    0x160e4f4:0xd0,0x160e4f8:0x701}


def owner(device):return getattr(device,ATTRIBUTE,None)


def producer_for_image(image):
    matches=[d for d in image.board.devices if type(d).__name__=='ProducerControl' and owner(d) is not None]
    if len(matches)>1:raise UnsupportedDevice('Joint passive configuration has multiple physical owners')
    return matches[0] if matches else None


def image_for_producer(producer):
    matches=[d for d in producer.board.devices if type(d).__name__=='ImageWorkControl']
    if len(matches)!=1:raise UnsupportedDevice('Joint passive configuration requires exactly one Image owner')
    return matches[0]


def _prior(device):
    from . import producer_processing_configuration as producer_config
    from . import image_processing_configuration as image_config
    p=producer_config.summary(device);image=image_for_producer(device);i=image_config.summary(image)
    if p is None or i is None or p['state']!='complete' or i['state']!='complete':
        raise UnsupportedDevice('Joint passive configuration precedes completed Producer and Image geometry')
    return dict(producer=digest(p),image=digest(i),producer_registers=digest(device.registers),
        image_registers=digest(image.registers),initial_global=device.processing_reset['word'])


def plan(device,block,tables,initial):
    if type(block) is not bytes or len(block)!=CALLER_BYTES or set(tables)!=set(o for o,_ in TABLES):
        raise UnsupportedDevice('Joint passive configuration requires its complete caller and eight tables')
    p=device.GLOBAL;image=image_for_producer(device);u16=lambda o:struct.unpack_from('<H',block,o)[0]
    u32=lambda o:struct.unpack_from('<I',block,o)[0];events=[];words=dict(initial)
    def write(address,value):events.append(('write',address,value));words[address]=value
    def read(address):events.append(('read',address,words[address]));return words[address]
    #9349F8: two13-bit coordinates, six-bit phase pair and13-bit count.
    if max(u16(0x44),u16(0x46),u16(0x52))>0x1fff or max(block[0x50:0x52])>0x3f:
        raise UnsupportedDevice('Joint passive coordinates leave the stock13/6-bit domains')
    write(p+0x78,u16(0x44)<<16|u16(0x46))
    write(p+0x84,block[0x50]<<24|block[0x51]<<16|u16(0x52))
    for offset,address in ((0x54,p+0x88),(0x58,p+0xc0)):
        data=tables[offset]
        if type(data) is not bytes or len(data)!=56:raise UnsupportedDevice('Joint14-word map changed size')
        for index,(value,) in enumerate(struct.iter_unpack('<I',data)):
            write(address+index*4,(value&0x3fff)<<16|(value&0x3fffffff)>>16)
    #934B10 and six256-word uploads: exactly the typed stock masks.
    if max(u16(0x48),u16(0x4a))>0x1fff or max(u16(0x4c),u16(0x4e))>0x7ff:
        raise UnsupportedDevice('Joint passive geometry leaves the stock13/11-bit domains')
    write(p+0x7c,u16(0x48)<<16|u16(0x4a));write(p+0x80,u16(0x4c)<<16|u16(0x4e))
    value=read(p+0x84);write(p+0x84,value&0xc0c0ffff|block[0x50]<<24|block[0x51]<<16)
    for offset,address,mask in ((0x60,p+0x1c00,0x3fffffff),(0x64,p+0x2000,0x3fffffff),
            (0x68,p+0x2400,0x3fffffff),(0x6c,p+0x2800,0x3fffffff),
            (0x70,p+0x2c00,0x3fffffff),(0x74,p+0x3000,0xfffff)):
        data=tables[offset]
        if type(data) is not bytes or len(data)!=1024:raise UnsupportedDevice('Joint256-word table changed size')
        for index,(value,) in enumerate(struct.iter_unpack('<I',data)):write(address+index*4,value&mask)
    control=read(p+0x74)
    before=read(p);read(p)
    if u32(0x5c)>1 or u32(0x7c)>1 or u32(0x80)>1 or u32(0x84)>1:
        raise UnsupportedDevice('Joint passive mode policy leaves its stock Boolean domains')
    write(p,before&0xfffbffff|u32(0x5c)<<18)
    enable=control&1
    if u32(0x84)!=1:enable=int(enable==0) if u32(0x7c)==0 else u32(0x80)
    write(p+0x74,enable|(block[0x78]&7)<<16)
    #933FB4/9340F0 scalar fields; the final validated inputs are unused by stock.
    if max(u16(o) for o in (0x8c,0x8e,0x90,0x92))>0x1fff or u32(0x94)>0xfffff:
        raise UnsupportedDevice('Joint scalar coordinates leave the stock13/20-bit domains')
    if (max(u16(o) for o in (0x98,0xa2,0xa4,0xa6,0xa8))>0x3fff
            or max(u16(o) for o in (0x9a,0x9c,0x9e,0xa0))>0x1fff):
        raise UnsupportedDevice('Joint scalar fields leave the stock14/13-bit domains')
    for offset,value in ((0x40,u16(0x8c)|u16(0x8e)<<16),(0x44,u16(0x90)|u16(0x92)<<16),
            (0x48,u32(0x94)),(0x4c,u16(0x98)),(0x50,u16(0x9a)|u16(0x9c)<<16),
            (0x54,u16(0x9e)|u16(0xa0)<<16)):write(p+offset,value)
    #Both Image banks:35 ordinary passive stores each, no active control.
    from .image_still_prepare import words as image_words
    for channel,start in enumerate((0x134,0x1c4)):
        values=image_words(image,block[start:start+0x90].hex())
        for offset,value in zip(image.PASSIVE_CONFIG_ORDER,values):write(image.BASES[channel]+offset,value)
    if len(events)!=1651:raise UnsupportedDevice('Joint passive transaction lost its complete1651-access shape')
    return tuple(events)


def _plan_hash(events):
    return hashlib.sha256(b''.join(struct.pack('<BII',op=='write',address,value) for op,address,value in events)).hexdigest()


def _events_hash(events):
    return hashlib.sha256(b''.join(struct.pack('<BBIIQ',cpu,op=='write',address,value,tick)
                                 for cpu,op,address,value,tick in events)).hexdigest()


def _seal(record):
    record['seal']=digest(dict(profile=record['profile'],state=record['state'],step=record['step'],
        armed_tick=record['armed_tick'],closed_tick=record['closed_tick'],contract=record['contract_sha256'],
        prior=record['prior_sha256'],events=_events_hash(record['accesses']),words=digest(record['words'])))


def arm(device,profile=PROFILE,*,caller_address=None):
    if (profile!=PROFILE or owner(device) is not None or device.board.failure is not None
            or device.busy or not device._idle()):
        raise UnsupportedDevice('Joint passive configuration requires explicit single-lifetime admission')
    image=image_for_producer(device)
    if image.busy or image.dma_busy or image.phase_busy or image.pending_data is not None:
        raise UnsupportedDevice('Joint passive configuration requires quiet Image banks')
    from . import producer_processing_reset,image_processing_reset
    producer_processing_reset._route(device);image_processing_reset._route(image)
    read=lambda a:int.from_bytes(device.board.read_memory(a,4,cpu_id=0),'little')
    if any(read(a)!=value for a,value in WORDS.items()):
        raise UnsupportedDevice('Joint passive configuration changed the actual mask3D0 stopped owner')
    prior=_prior(device)
    if caller_address is None:
        core=device.board.cpus[0]
        if (int(core.reg_read(UC_ARM_REG_PC)),int(core.reg_read(UC_ARM_REG_LR)))!=FIRST:
            raise UnsupportedDevice('Joint passive admission requires its first9349F8 store')
        caller_address=int(core.reg_read(UC_ARM_REG_R1))-0x44
    if type(caller_address) is not int or caller_address&3 or not 0<caller_address<=0x20000000-CALLER_BYTES:
        raise UnsupportedDevice('Joint passive caller is outside aligned RAM')
    block=bytes(device.board.read_memory(caller_address,CALLER_BYTES,cpu_id=0));tables={};caps={}
    for offset,size in TABLES:
        address=struct.unpack_from('<I',block,offset)[0]
        physical=address&~0x40000000
        if address&3 or address&~0x5fffffff or not 0<physical<=0x20000000-size:
            raise UnsupportedDevice('Joint passive table leaves aligned owned RAM')
        data=bytes(device.board.read_memory(address,size,cpu_id=0));tables[offset]=data
        caps[offset]=dict(address=address,size=size,sha256=hashlib.sha256(data).hexdigest())
    initial={device.GLOBAL:prior['initial_global'],device.GLOBAL+0x74:device.registers[device.GLOBAL+0x74]}
    expected=plan(device,block,tables,initial)
    contract=dict(caller_address=caller_address,caller_sha256=hashlib.sha256(block).hexdigest(),
        tables=caps,initial=initial,plan_sha256=_plan_hash(expected),mask=0x3d0)
    record=dict(profile=PROFILE,state='configuring',step=0,armed_tick=device.board.virtual_ticks,
        closed_tick=None,caller_block=block,table_bytes=tables,contract=contract,contract_sha256=digest(contract),
        plan=expected,prior=prior,prior_sha256=digest(prior),accesses=[],words=dict(initial),seal=None)
    _seal(record);setattr(device,ATTRIBUTE,record)
    return summary(device)


def validate(device,*,full=False):
    r=owner(device)
    if r is None:return None
    if (r.get('profile')!=PROFILE or r['contract_sha256']!=digest(r['contract'])
            or r['prior_sha256']!=digest(r['prior']) or type(r['step']) is not int or not 0<=r['step']<=1651
            or len(r['accesses'])!=r['step'] or r['state']!=('complete' if r['step']==1651 else 'configuring')
            or (r['closed_tick'] is not None)!=(r['step']==1651)
            or r['contract']['caller_sha256']!=hashlib.sha256(r['caller_block']).hexdigest()
            or any(r['contract']['tables'][o]['sha256']!=hashlib.sha256(data).hexdigest()
                   for o,data in r['table_bytes'].items())
            or r['plan']!=plan(device,r['caller_block'],r['table_bytes'],r['contract']['initial'])
            or r['contract']['plan_sha256']!=_plan_hash(r['plan'])):
        raise UnsupportedDevice('Joint passive configuration changed its whole-driver contract')
    old_seal=r['seal'];_seal(r)
    if r['seal']!=old_seal:
        r['seal']=old_seal
        raise UnsupportedDevice('Joint passive configuration changed its retained access/word seal')
    words=dict(r['contract']['initial']);previous=r['armed_tick']
    for index,(cpu,op,address,value,tick) in enumerate(r['accesses']):
        if cpu!=0 or (op,address,value)!=r['plan'][index] or not previous<=tick<=device.board.virtual_ticks:
            raise UnsupportedDevice('Joint passive configuration lost ordered CPU0 chronology')
        if op=='write':words[address]=value
        previous=tick
    if words!=r['words'] or r['closed_tick'] is not None and r['closed_tick']!=previous:
        raise UnsupportedDevice('Joint passive configuration physical words changed')
    from . import producer_processing_reset,image_processing_reset
    producer_processing_reset._route(device);image_processing_reset._route(image_for_producer(device))
    if full and _prior(device)!=r['prior']:
        raise UnsupportedDevice('Joint passive configuration changed its completed processing predecessors')
    return r


def access(device,operation,cpu,address,size,value=None):
    r=validate(device)
    if (r is None or r['state']=='complete' or type(cpu) is not int or cpu!=0
            or type(size) is not int or size!=4 or operation not in ('read','write')):
        raise UnsupportedDevice('Joint passive configuration excludes new activation, data and unknown accesses')
    expected=r['plan'][r['step']]
    word=expected[2] if operation=='read' else value
    if (type(word) is not int or (operation,address,word)!=expected):
        raise UnsupportedDevice('Joint passive configuration requires its next exact stock field')
    if not r['accesses']:
        c=device.board.cpus[0]
        if (int(c.reg_read(UC_ARM_REG_PC)),int(c.reg_read(UC_ARM_REG_LR)))!=FIRST:
            raise UnsupportedDevice('Joint passive configuration changed the whole10CB618 caller')
        validate(device,full=True)
    if (bytes(device.board.read_memory(r['contract']['caller_address'],CALLER_BYTES,cpu_id=0))!=r['caller_block']
            or any(bytes(device.board.read_memory(cap['address'],cap['size'],cpu_id=0))!=r['table_bytes'][offset]
                   for offset,cap in r['contract']['tables'].items())):
        raise UnsupportedDevice('Joint passive caller/table mutated while stock uploaded its fields')
    r['accesses'].append((cpu,operation,address,word,device.board.virtual_ticks))
    if operation=='write':r['words'][address]=word
    r['step']+=1
    if r['step']==1651:r.update(state='complete',closed_tick=device.board.virtual_ticks)
    _seal(r)
    return word if operation=='read' else None


def summary(device):
    r=validate(device,full=True)
    return deepcopy({k:v for k,v in r.items() if k not in ('caller_block','table_bytes','seal')}) if r is not None else None
