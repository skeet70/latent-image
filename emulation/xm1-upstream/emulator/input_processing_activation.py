"""Declared channel4 setup/enable, with no input DMA or completion.

Whole10CAEA0(mask2000) programs923F48 then923E14 registers logical2A /
physical38. A new private register ledger retains that entire transaction;
the preceding RAW transfer, Input events and paired Image remain unchanged.
The idle3AC zero is a declared simulation input, not a measured register.
"""
from copy import deepcopy
import hashlib
import struct

from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R4

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import image_processing_activation as image_activation
from . import afs_still_readout

PROFILE = 'declared-postprocessing-input-channel4-activation-unmeasured'
ATTRIBUTE = 'processing_activation'
FIRST = (0x924060, 0x923f7c)
CALLER = 0x15f672c
CALLBACK = 0x11f5b9c
IRQ = 38
VECTOR = 0x920c90
PRIORITY = 8
REGISTERS = (0x390,0x394,0x398,0x39c,0x3a0,0x3a4,0x3a8,0x3ac,0x3b4,0x3b8)
PCS = (0x924060,0x9240a8,0x9240e8,0x92415c,0x92416c,
       0x924174,0x924178,0x924200,0x924244,0x920acc,
       0x920b68,0x920b98,0x923d04,0x923d0c,0x923d14)


def owner(device): return getattr(device, ATTRIBUTE, None)


def contains(device, address, size):
    return address - device.BASE in REGISTERS


def plan(block):
    if (type(block) is not bytes or len(block) != 0x24
            or struct.unpack_from('<3I',block) != (1,0,0)
            or block[0xc] != 0 or block[0xd] != 0
            or struct.unpack_from('<5H',block,0xe) != (4992,3296,4,0,9984)
            or struct.unpack_from('<3I',block,0x18) != (0x4e73bc00,3,0)):
        raise UnsupportedDevice('Processing Input activation changed its observed RAW geometry/mode request')
    # Initial3AC zero is an explicit idle input. The only subsequent reads
    # return words written by this same whole setup/enable transaction.
    return (('write',0xfff10394,0x1000000),('write',0xfff10398,0x13800000),
        ('write',0xfff103a0,0xce0),('write',0xfff1039c,0x40000),
        ('write',0xfff103a4,0x2700),('write',0xfff103a8,0x4e73bc00),
        ('read',0xfff103ac,0),('write',0xfff103ac,3),
        ('write',0xfff103b8,0xfb000000),('write',0xfff103b4,0x10000003),
        ('read',0xfff103ac,3),('write',0xfff103ac,0x8003),
        ('read',0xfff103ac,0x8003),('write',0xfff103ac,0xc003),
        ('write',0xfff10390,1))


def _source_hash(device):
    # Seal the complete owned allocation. The exact hardware interpretation
    # of the offset/count fields is not yet a recovered sampling contract.
    return hashlib.sha256(device.board.read_memory(0x4e73bc00,32947200,cpu_id=0)).hexdigest()


def _raw_anchor(device):
    r = afs_still_readout.validate(device)
    geometry = r['geometry'] if r is not None else {}
    if (r is None or r['state'] != 'complete'
            or any(geometry.get(k) != v for k,v in dict(width=4992,height=3300,pitch=9984,
                  row_bytes=9984,row_gap=0,packed=False,destination=0x0e73bc00,
                  span_bytes=32947200,transport_bytes=32947200).items())):
        raise UnsupportedDevice('Processing Input activation lacks its closed owned U16 RAW source')
    return dict(geometry=deepcopy(geometry),
                sha256=digest({k:v for k,v in r.items() if k not in ('rows','seal')}))


def _prior(device):
    images = [d for d in device.board.devices if type(d).__name__ == 'ImageWorkControl']
    if len(images) != 1:
        raise UnsupportedDevice('Processing Input activation requires its paired Image owner')
    image = image_activation.summary(images[0])
    if (image is None or image['state'] != 'enabled-awaiting-input'
            or image.get('channel1',{}).get('state') != 'enabled-awaiting-input'):
        raise UnsupportedDevice('Processing Input activation precedes the actual two Image channels')
    # Each copied caller byte block is already validated and hash-pinned by
    # its Image owner. Keep only its plain digest in the predecessor capsule.
    image.pop('caller_block')
    image['channel1'].pop('caller_block')
    return dict(image_sha256=digest(image),raw=_raw_anchor(device))


def _route_words(device):
    return [dict(vector=b.config.get(0x400+IRQ*4),priority=b.config.get(0x800+IRQ*4),
                 source_type=b.source_config>>IRQ&1,
                 configuration_140=b.config.get(0x140+IRQ//16*4,0)>>(IRQ%16*2)&3,
                 configuration_180=b.config.get(0x180+IRQ//16*4,0)>>(IRQ%16*2)&3)
            for b in device.board.interrupts.banks]


def _route(device, record):
    banks = device.board.interrupts.banks
    for cpu,bank in enumerate(banks):
        if (bank.pending & (1<<IRQ) or bank.active == IRQ or bank.injected == IRQ
                or any(e.get('line') == IRQ for e in bank.active_stack)
                or cpu == 1 and bank.enabled & (1<<IRQ)):
            raise UnsupportedDevice('Processing Input activation excludes a completion IRQ')
    words = _route_words(device)
    if words[1] != record['route'][1]:
        raise UnsupportedDevice('Processing Input activation changed CPU1 registration')
    read = lambda a: int.from_bytes(device.board.read_memory(a,4,cpu_id=0),'little')
    enabled = bool(banks[0].enabled & (1<<IRQ))
    target = dict(vector=VECTOR,priority=PRIORITY,source_type=1,
                  configuration_140=1,configuration_180=0)
    if enabled:
        if (record['step'] < 10 or words[0] != target or read(0xc6ddb4) != CALLBACK
                or read(0xc6ddc8) != 1 or read(0xc6dda0) == 0):
            raise UnsupportedDevice('Processing Input activation changed its stock physical38 route/callback')
    elif record['step'] >= 11:
        raise UnsupportedDevice('Processing Input activation lost its actual physical38 registration')
    elif record['step'] < 10:
        # Whole923E14 stores its software active lease before920A3C ACK.
        # Between the setup return and that store, step9 can still be idle.
        if (words != record['route'] or read(0xc6ddb4) != 0
                or read(0xc6ddc8) not in ((0,1) if record['step']==9 else (0,))):
            raise UnsupportedDevice('Processing Input activation changed its unregistered prefix')
    elif (any(words[0][k] not in (record['route'][0][k],target[k]) for k in target)
            or read(0xc6ddb4) not in (0,CALLBACK) or read(0xc6ddc8) != 1):
        raise UnsupportedDevice('Processing Input activation changed partial stock registration')


def _seal(record):
    record['seal'] = digest({k:v for k,v in record.items() if k not in ('seal','caller_block','data')})


def arm(device, profile=PROFILE, *, caller_address=None):
    if (profile != PROFILE or owner(device) is not None or device.board.failure is not None
            or device.busy or device.dma_busy or device.phase_busy or device.pending_command is not None
            or device.command_armed is not None or device.event_pending or device.event_due is not None):
        raise UnsupportedDevice('Processing Input activation requires its explicit quiet single lifetime')
    read = lambda a: int.from_bytes(device.board.read_memory(a,4,cpu_id=0),'little')
    if (read(0x15f66a4),read(0x15f66a8),read(0x160e4e4),read(0x160e4f8)) != (1,0x131db24,0,0):
        raise UnsupportedDevice('Processing Input activation changed its cleared-mask2000 caller')
    if caller_address is None:
        core = device.board.cpus[0]
        if (int(core.reg_read(UC_ARM_REG_PC)),int(core.reg_read(UC_ARM_REG_LR))) != FIRST:
            raise UnsupportedDevice('Processing Input activation requires first whole923F48 store')
        caller_address = int(core.reg_read(UC_ARM_REG_R4))
    if type(caller_address) is not int or caller_address&3 or not 0<caller_address<0x20000000-0x24:
        raise UnsupportedDevice('Processing Input activation caller leaves aligned RAM')
    block = bytes(device.board.read_memory(caller_address,0x24,cpu_id=0))
    sequence = plan(block)
    prior = _prior(device)
    record = dict(profile=PROFILE,state='activating',step=0,armed_tick=device.board.virtual_ticks,
        closed_tick=None,caller_address=caller_address,caller_block=block,
        caller_sha256=hashlib.sha256(block).hexdigest(),plan=sequence,prior=prior,
        prior_sha256=digest(prior),source_sha256=_source_hash(device),route=_route_words(device),
        words={0xfff103ac:0},accesses=[])
    _seal(record);_route(device,record);setattr(device,ATTRIBUTE,record)
    return summary(device)


def validate(device, *, full=False):
    r = owner(device)
    if r is None:return None
    if 'data' in r:
        from . import processing_image_delivery
        processing_image_delivery.validate(device,history=full)
        return validate(processing_image_delivery.historical(device),full=full)
    if (type(r) is not dict or r.get('profile') != PROFILE
            or r.get('seal') != digest({k:v for k,v in r.items() if k not in ('seal','caller_block')})
            or r['caller_sha256'] != hashlib.sha256(r['caller_block']).hexdigest()
            or r['plan'] != plan(r['caller_block']) or r['prior_sha256'] != digest(r['prior'])
            or type(r['step']) is not int or not 0<=r['step']<=15 or len(r['accesses']) != r['step']
            or r['state'] != ('enabled-awaiting-input' if r['step']==15 else 'activating')
            or (r['closed_tick'] is not None) != (r['step']==15)):
        raise UnsupportedDevice('Processing Input activation lost its sealed whole-driver contract')
    words = {0xfff103ac:0};previous=r['armed_tick']
    for index,(cpu,op,address,value,tick) in enumerate(r['accesses']):
        if (cpu!=0 or (op,address,value)!=r['plan'][index]
                or type(tick) is not int or not previous<=tick<=device.board.virtual_ticks):
            raise UnsupportedDevice('Processing Input activation lost ordered CPU0 chronology')
        if op=='write':words[address]=value
        previous=tick
    if r['words']!=words or r['closed_tick'] is not None and r['closed_tick']!=previous:
        raise UnsupportedDevice('Processing Input activation changed its physical register history')
    _route(device,r)
    if full and (r['prior']!=_prior(device) or r['source_sha256']!=_source_hash(device)):
        raise UnsupportedDevice('Processing Input activation changed its paired Image/owned RAW predecessor')
    return r


def access(device,operation,cpu,address,size,value=None):
    r = validate(device)
    if (r is None or r['step']==15 or type(cpu) is not int or cpu!=0
            or type(size) is not int or size!=4):
        raise UnsupportedDevice('Processing Input activation excludes data/status/completion or another lifetime')
    expected = r['plan'][r['step']]
    word = expected[2] if operation=='read' else value
    core = device.board.cpus[0]
    if (type(word) is not int or (operation,address,word)!=expected
            or int(core.reg_read(UC_ARM_REG_PC))!=PCS[r['step']]
            or bytes(device.board.read_memory(r['caller_address'],0x24,cpu_id=0))!=r['caller_block']):
        raise UnsupportedDevice('Processing Input activation requires its next exact whole-driver access')
    if r['step']==0:
        if (int(core.reg_read(UC_ARM_REG_LR))!=FIRST[1]
                or int(core.reg_read(UC_ARM_REG_R4))!=r['caller_address']):
            raise UnsupportedDevice('Processing Input activation changed whole923F48 caller ABI')
        validate(device,full=True)
    r['accesses'].append((cpu,operation,address,word,device.board.virtual_ticks))
    if operation=='write':r['words'][address]=word
    r['step']+=1
    if r['step']==15:r.update(state='enabled-awaiting-input',closed_tick=device.board.virtual_ticks)
    _seal(r)
    return word if operation=='read' else None


def summary(device):
    r = validate(device,full=True)
    result=deepcopy({k:v for k,v in r.items() if k not in ('seal','caller_block')}) if r is not None else None
    if result is not None and 'data' in owner(device):
        from . import processing_image_delivery
        result['data']=processing_image_delivery.summary(device)
    return result
