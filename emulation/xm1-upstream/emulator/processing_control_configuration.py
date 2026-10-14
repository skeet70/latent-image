"""One stock10CB5B0(maskD0) control transaction after passive uploads.

Firmware alone selects the preprocessing mode and enables its two GLOBAL
fields. This optional owner supplies no source clock, data, result or IRQ.
The subsequent mask701 Input/Image reset remains a distinct strict boundary.
"""
from copy import deepcopy
import hashlib
import struct

from unicorn.arm_const import (UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R4,
                              UC_ARM_REG_R5, UC_ARM_REG_R10)
from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import processing_passive_configuration as passive

PROFILE='declared-post-prefilter-preprocessing-control-enable-unmeasured'
ATTRIBUTE='processing_control_configuration'
FIRST=(0x933478,0x10ca934)
WORDS=dict(passive.WORDS)
WORDS[0x160e4e8]=0


def owner(device):return getattr(device,ATTRIBUTE,None)


def plan(device,block,initial):
    if type(block) is not bytes or len(block)!=passive.CALLER_BYTES:
        raise UnsupportedDevice('Processing control requires its complete passive caller')
    mode,policy=struct.unpack_from('<I',block,0x88)[0],struct.unpack_from('<I',block,0x5c)[0]
    if mode not in (1,2) or policy not in (0,1):
        raise UnsupportedDevice('Processing control leaves the declared stock mode/policy class')
    address=device.GLOBAL;events=[];word=initial
    def read():events.append(('read',address,word))
    def write(value):
        nonlocal word
        word=value;events.append(('write',address,value))
    read();write(word&~0x300|mode<<8)
    for bit in (1,2):
        read();read()
        enabled=(word>>16&3)|bit
        write(word&~0x70000|enabled<<16|policy<<18)
    return tuple(events)


def _prior(device):
    record=passive.summary(device)
    if record is None or record['state']!='complete' or record['step']!=1651:
        raise UnsupportedDevice('Processing control precedes the whole passive table upload')
    return dict(passive_sha256=digest(record),initial_word=record['words'][device.GLOBAL])


def _seal(record):
    record['seal']=digest({k:v for k,v in record.items() if k not in ('seal','caller_block')})


def arm(device,profile=PROFILE,*,caller_address=None):
    if (profile!=PROFILE or owner(device) is not None or device.board.failure is not None
            or not device._idle() or device.busy):
        raise UnsupportedDevice('Processing control requires explicit single-lifetime admission')
    from .producer_processing_reset import _route
    _route(device)
    read=lambda a:int.from_bytes(device.board.read_memory(a,4,cpu_id=0),'little')
    if any(read(a)!=v for a,v in WORDS.items()):
        raise UnsupportedDevice('Processing control changed its actual stopped maskD0 owner')
    prior=_prior(device)
    if caller_address is None:
        cpu=device.board.cpus[0]
        if (int(cpu.reg_read(UC_ARM_REG_PC)),int(cpu.reg_read(UC_ARM_REG_LR)))!=FIRST:
            raise UnsupportedDevice('Processing control requires its first stock933478 read')
        caller_address=int(cpu.reg_read(UC_ARM_REG_R10))
    if (type(caller_address) is not int or caller_address&3
            or not 0<caller_address<=0x20000000-passive.CALLER_BYTES):
        raise UnsupportedDevice('Processing control caller leaves aligned RAM')
    previous=passive.owner(device)
    block=bytes(device.board.read_memory(caller_address,passive.CALLER_BYTES,cpu_id=0))
    successor=bytearray(previous['caller_block'])
    # Stock12031D0 marks the completed bit80 upload at1203374 before
    # entering12034E4's enable wrapper. Retain that single observed change
    # in the new caller capsule; the previous upload remains byte-exact.
    struct.pack_into('<I',successor,0x84,1)
    if (caller_address!=previous['contract']['caller_address']
            or block!=bytes(successor)):
        raise UnsupportedDevice('Processing control changed the completed table caller')
    contract=dict(caller_address=caller_address,caller_sha256=hashlib.sha256(block).hexdigest(),
                  mask=0xd0,enabled=1,bookkeeping=dict(address=caller_address+0x84,
                    previous=struct.unpack_from('<I',previous['caller_block'],0x84)[0],current=1,pc=0x1203374))
    record=dict(profile=PROFILE,state='configuring',step=0,armed_tick=device.board.virtual_ticks,
        closed_tick=None,caller_block=block,contract=contract,contract_sha256=digest(contract),
        prior=prior,prior_sha256=digest(prior),plan=plan(device,block,prior['initial_word']),
        accesses=[],word=prior['initial_word'],seal=None)
    _seal(record);setattr(device,ATTRIBUTE,record)
    return summary(device)


def validate(device,*,full=False):
    r=owner(device)
    if r is None:return None
    if (type(r) is not dict or r.get('profile')!=PROFILE
            or r['seal']!=digest({k:v for k,v in r.items() if k not in ('seal','caller_block')})
            or r['contract_sha256']!=digest(r['contract']) or r['prior_sha256']!=digest(r['prior'])
            or r['contract']['caller_sha256']!=hashlib.sha256(r['caller_block']).hexdigest()
            or r['plan']!=plan(device,r['caller_block'],r['prior']['initial_word'])
            or type(r['step']) is not int or not 0<=r['step']<=8 or len(r['accesses'])!=r['step']
            or r['state']!=('complete' if r['step']==8 else 'configuring')
            or (r['closed_tick'] is not None)!=(r['step']==8)):
        raise UnsupportedDevice('Processing control changed its sealed whole-driver contract')
    word=r['prior']['initial_word'];previous=r['armed_tick']
    for index,(cpu,operation,address,value,tick) in enumerate(r['accesses']):
        if (cpu!=0 or (operation,address,value)!=r['plan'][index]
                or not previous<=tick<=device.board.virtual_ticks):
            raise UnsupportedDevice('Processing control changed ordered CPU0 chronology')
        if operation=='write':word=value
        previous=tick
    if word!=r['word'] or r['closed_tick'] is not None and r['closed_tick']!=previous:
        raise UnsupportedDevice('Processing control changed its physical GLOBAL field')
    from .producer_processing_reset import _route
    _route(device)
    if full and _prior(device)!=r['prior']:
        raise UnsupportedDevice('Processing control changed its passive predecessor')
    return r


def access(device,operation,cpu,address,size,value=None):
    r=validate(device)
    if (r is None or r['state']=='complete' or cpu!=0 or type(cpu) is not int
            or type(size) is not int or size!=4 or operation not in ('read','write')):
        raise UnsupportedDevice('Processing control excludes additional reset, activation and data lifetimes')
    expected=r['plan'][r['step']]
    word=expected[2] if operation=='read' else value
    if type(word) is not int or (operation,address,word)!=expected:
        raise UnsupportedDevice('Processing control requires its next exact GLOBAL access')
    if not r['accesses']:
        core=device.board.cpus[0]
        if ((int(core.reg_read(UC_ARM_REG_PC)),int(core.reg_read(UC_ARM_REG_LR)))!=FIRST
                or int(core.reg_read(UC_ARM_REG_R4))!=0xd0 or int(core.reg_read(UC_ARM_REG_R5))!=1
                or int(core.reg_read(UC_ARM_REG_R10))!=r['contract']['caller_address']):
            raise UnsupportedDevice('Processing control changed the whole10CB5B0 caller')
        validate(device,full=True)
    if bytes(device.board.read_memory(r['contract']['caller_address'],passive.CALLER_BYTES,cpu_id=0))!=r['caller_block']:
        raise UnsupportedDevice('Processing control caller changed during the stock transaction')
    r['accesses'].append((cpu,operation,address,word,device.board.virtual_ticks))
    if operation=='write':r['word']=word
    r['step']+=1
    if r['step']==8:r.update(state='complete',closed_tick=device.board.virtual_ticks)
    _seal(r)
    return word if operation=='read' else None


def summary(device):
    r=validate(device,full=True)
    return deepcopy({k:v for k,v in r.items() if k not in ('seal','caller_block')}) if r is not None else None
