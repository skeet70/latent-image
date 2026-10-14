"""Explicit full-field mode0 resize experiment, separate from silicon filtering.

Only a copied C9BA40 no-distortion request is supported. Halfword pitches
become byte pitches; the supplied monochrome YYCbCr rows use a declared
floor-nearest law. ARM owns IRQ74 ACK, idle polling, unregister and callback.
"""
from __future__ import annotations

import hashlib
import json
import struct

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

from .hardware import UnsupportedDevice
from . import still_resize_successor as successor
from . import still_resize_preview as preview
from . import still_resize_thumbnail as thumbnail
from . import still_resize_jpeg_stream as jpeg_stream


PROFILE='declared-mode0-resize-monochrome-yycbcr-irq74-unmeasured'
MAX_BYTES=64*1024*1024


def _seal(request):
    payload={k:request[k] for k in ('plan','source_sha256','row_identity','completion_ticks')}
    if 'scene_profile' in request:payload['scene_profile']=request['scene_profile']
    return hashlib.sha256(request['descriptor']+json.dumps(payload,sort_keys=True,
        separators=(',',':')).encode()).hexdigest()


def _request_plan(request):
    profile=request.get('scene_profile',PROFILE)
    if profile==preview.PROFILE:return preview.plan(request['descriptor'],request['plan']['output_capacity'])
    if profile==thumbnail.PROFILE:return thumbnail.plan(request['descriptor'],request['plan']['output_capacity'])
    if profile!=PROFILE:raise UnsupportedDevice('Still resize retained scene profile changed')
    return _plan(request['descriptor'],request['plan']['output_capacity'])


def _physical(address,count):
    if type(address) is not int or not 0<address<0x60000000:
        raise UnsupportedDevice('Still resize buffer address escapes DRAM')
    result=address-0x40000000 if address>=0x40000000 else address
    if result+count>0x20000000:
        raise UnsupportedDevice('Still resize buffer span escapes DRAM')
    return result


def _plan(descriptor,capacity):
    d=struct.unpack('<20I',descriptor)
    sw,sh,dw,dh=d[6],d[7],d[14],d[15]
    if (d[0:2]!=(0,0) or d[8:10] not in ((0,0),(sw-1,sh-1)) or any(d[i] for i in (12,13,18))
            or d[19] not in (0,0x118060c) or d[16:18]!=(dw-1,dh-1)
            or not 16<=sw<=8192 or sw&7 or not 2<=sh<=8192
            or not 16<=dw<=8191 or dw&15 or not 8<=dh<=8192 or dh&7
            or not sw<=d[5]<=32760 or d[5]&7
            or not dw<=d[11]<=32752 or d[11]&15
            or type(capacity) is not int or not 1<=capacity<=MAX_BYTES):
        raise UnsupportedDevice('Still resize requires bounded mode0/full-field/no-distortion geometry')
    rx=(sw-1)*4096//(dw-1);ry=(sh-1)*4096//(dh-1)
    if d[2:4] not in ((0,0),(rx,ry)) or max(rx,ry)>65535:
        raise UnsupportedDevice('Still resize lacks its calculated full-field ratios')
    rx=4095 if rx==4096 else rx;ry=4095 if ry==4096 else ry
    selector=next((s for s in range(3) if rx*(64>>s)>>12<=160 and ry<<3<0x21000),None)
    if selector is None:raise UnsupportedDevice('Still resize ratios exceed the audited driver selector')
    sp,dp=2*d[5],2*d[11];source_bytes=sp*sh;output_bytes=dp*dh
    if source_bytes>MAX_BYTES or output_bytes>capacity:
        raise UnsupportedDevice('Still resize source/output capacity escapes its owned allocation')
    src=_physical(d[4],source_bytes);dst=_physical(d[10],output_bytes)
    if (d[4]|d[10])&15 or src<dst+output_bytes and dst<src+source_bytes:
        raise UnsupportedDevice('Still resize buffers are unaligned or physically overlap')
    expected={4:0,0x10:src,0x14:d[5],0x18:sw|sh<<16,0x1c:dst,
        0x20:d[11],0x24:0,0x28:dw|dh<<16,0x2c:selector,0x30:0,0x34:0,
        0x38:rx,0x3c:ry,0x40:4096,0x44:4096,0x48:0,0x4c:0x10001,
        0x50:sw-1|(sh-1)<<16,0x54:dw-1|(dh-1)<<16,0x58:sw-1|(sh-1)<<16,
        **dict.fromkeys(range(0x60,0xdc,4),0x10000)}
    return dict(source=d[4],source_bytes=source_bytes,source_width=sw,source_height=sh,
        source_stride_bytes=sp,destination=d[10],output_width=dw,output_height=dh,
        output_stride_bytes=dp,output_span_bytes=output_bytes,output_capacity=capacity,
        expected=expected,assumptions='Declared full-field floor nearest x*width/output_width; neutral128 chroma; no silicon coefficient/filter or S2 packing identification')


def _rows(source,plan,*,backend='auto'):
    sw,sh,dw,dh=(plan[k] for k in ('source_width','source_height','output_width','output_height'))
    sp,dp=plan['source_stride_bytes'],plan['output_stride_bytes']
    if (type(source) is not bytes or len(source) != sp*sh
            or backend not in ('auto','scalar','numpy') or backend=='numpy' and _numpy is None):
        raise UnsupportedDevice('Still resize requires its exact source span and available arithmetic backend')
    if backend=='numpy' or backend=='auto' and _numpy is not None:
        return _vector_rows(source,plan)
    for y in range(sh):
        for x in range(0,sw,2):
            p=y*sp+2*x
            if source[p+2:p+4]!=b'\x80\x80':
                raise UnsupportedDevice('Still resize supplied scene is not declared neutral-chroma YYCbCr')
    samples=[(x*sw//dw)//2*4+(x*sw//dw)%2 for x in range(dw)]
    rows=[]
    for y in range(dh):
        origin=(y*sh//dh)*sp;row=bytearray(dw*2)
        for x in range(0,dw,2):
            row[x*2:x*2+4]=bytes((source[origin+samples[x]],source[origin+samples[x+1]],128,128))
        rows.append((plan['destination']+y*dp,bytes(row)))
    return rows


def _vector_rows(source,plan):
    """The same floor-nearest law on read-only pitched source views."""
    sw,sh,dw,dh=(plan[k] for k in ('source_width','source_height','output_width','output_height'))
    sp,dp=plan['source_stride_bytes'],plan['output_stride_bytes']
    view=_numpy.frombuffer(source,dtype=_numpy.uint8).reshape(sh,sp)
    if _numpy.any(view.reshape(sh,sp//4,4)[:,:sw//2,2:] != 128):
        raise UnsupportedDevice('Still resize supplied scene is not declared neutral-chroma YYCbCr')
    x=_numpy.arange(dw,dtype=_numpy.int64)*sw//dw
    samples=(x//2)*4+x%2
    rows=[]
    for y in range(dh):
        row=_numpy.empty((dw//2,4),dtype=_numpy.uint8)
        row[:,:2]=view[y*sh//dh,samples].reshape(dw//2,2)
        row[:,2:]=128
        rows.append((plan['destination']+y*dp,row.tobytes()))
    return rows


class StillResizeControl:
    BASE=0xfff65000
    OFFSETS=(0,4,*range(0x10,0x5c,4),*range(0x60,0xdc,4),0x140,0x144,0x148)

    def __init__(self,board,*,profile=None,idle_ticks=2):
        if profile not in (None,PROFILE) or type(idle_ticks) is not int or not 1<=idle_ticks<=400000:
            raise ValueError('Still resize requires a named profile and bounded idle delay')
        self.board,self.profile,self.idle_ticks=board,profile,idle_ticks
        self.registers=dict.fromkeys(self.OFFSETS,0);self.uploads=[]
        self.state='idle';self.request=None;self.pending_action=None

    def contains(self,address,size):
        return any(self.BASE+o<=address<self.BASE+o+4 for o in self.OFFSETS)

    def _typed(self,cpu,address,size):
        if self.profile!=PROFILE or type(cpu) is not int or cpu!=0 or size!=4 or address-self.BASE not in self.registers:
            raise UnsupportedDevice('Still resize requires its explicit CPU0 WORD profile/register')
        return address-self.BASE

    def _route(self,*,drained):
        banks=self.board.interrupts.banks;b=banks[0]
        if (not b.enabled&(1<<74) or b.config.get(0x400+4*74)!=0x97abfc
                or b.config.get(0x800+4*74)!=13 or b.source_config&(1<<74)
                or any((n!=0 and x.enabled&(1<<74)) or drained and
                    (x.pending&(1<<74) or x.active==74 or x.injected==74
                     or any(entry.get('line')==74 for entry in getattr(x,'active_stack',[])))
                     for n,x in enumerate(banks))):
            raise UnsupportedDevice('Still resize lacks the stock IRQ74 route/owner')

    def _validate(self,*,source=False):
        r=self.request
        if r is None:return
        if jpeg_stream.validate(self,source=source):return
        if successor.validate(self):return
        successor.history(self)
        if _seal(r)!=r['seal_sha256']:
            raise UnsupportedDevice('Still resize retained input/geometry seal changed')
        actual=bytes(self.board.read_memory(0xc9ba40,80,cpu_id=0))
        # The whole ISR clears only its saved callback after disabling and
        # unregistering IRQ74. Keep the original caller descriptor sealed.
        cleared=(r['closed_tick'] is not None and
            bytes(self.board.read_memory(0xc9ba3c,4,cpu_id=0))==bytes(4))
        if actual!=r['descriptor'] and not (cleared and actual==r['descriptor'][:76]+bytes(4)):
            raise UnsupportedDevice('Still resize copied request changed')
        if _request_plan(r)!=r['plan']:
            raise UnsupportedDevice('Still resize retained geometry changed')
        if any(self.registers[o]!=w for o,w in r['plan']['expected'].items()):
            raise UnsupportedDevice('Still resize retained register projection changed')
        if tuple((a,hashlib.sha256(row).hexdigest()) for a,row in r['rows'])!=r['row_identity']:
            raise UnsupportedDevice('Still resize retained scene rows changed')
        if source and hashlib.sha256(self.board.read_memory(r['plan']['source'],r['plan']['source_bytes'],cpu_id=0)).hexdigest()!=r['source_sha256']:
            raise UnsupportedDevice('Still resize owned input changed')

    def arm_scene(self,*,profile,source_sha256,output_capacity,completion_ticks=1000):
        if (profile!=PROFILE or self.profile!=PROFILE or self.state!='idle' or self.request is not None
                or self.pending_action is not None or self.board.failure is not None
                or type(completion_ticks) is not int or not 1<=completion_ticks<=400000):
            raise UnsupportedDevice('Still resize requires explicit one-shot scene admission')
        self.request=self._prepare_scene(source_sha256,output_capacity,completion_ticks)
        self.state='armed'
        return self.snapshot()

    def _prepare_scene(self,source_sha256,output_capacity,completion_ticks,scene_profile=PROFILE):
        descriptor=bytes(self.board.read_memory(0xc9ba40,80,cpu_id=0))
        if scene_profile==preview.PROFILE:plan=preview.plan(descriptor,output_capacity)
        elif scene_profile==thumbnail.PROFILE:plan=thumbnail.plan(descriptor,output_capacity)
        elif scene_profile==PROFILE:plan=_plan(descriptor,output_capacity)
        else:raise UnsupportedDevice('Still resize scene requires a named bounded geometry')
        if any(self.registers[o]!=w for o,w in plan['expected'].items()):
            raise UnsupportedDevice('Still resize upload differs from its actual copied request')
        source=bytes(self.board.read_memory(plan['source'],plan['source_bytes'],cpu_id=0))
        if type(source_sha256) is not str or hashlib.sha256(source).hexdigest()!=source_sha256:
            raise UnsupportedDevice('Still resize requires its explicitly owned source hash')
        if scene_profile==preview.PROFILE:rows=preview.rows(source,plan)
        elif scene_profile==thumbnail.PROFILE:rows=thumbnail.rows(source,plan)
        else:rows=_rows(source,plan)
        request=dict(descriptor=descriptor,plan=plan,source_sha256=source_sha256,rows=rows,
            row_identity=tuple((a,hashlib.sha256(row).hexdigest()) for a,row in rows),
            completion_ticks=completion_ticks,start_step=0,started_tick=None,completed_tick=None,
            acknowledged_tick=None,idle_tick=None,closed_tick=None)
        if scene_profile!=PROFILE:request['scene_profile']=scene_profile
        request['seal_sha256']=_seal(request)
        return request

    def arm_preview_scene(self,*,profile=preview.PROFILE,source_sha256,output_capacity,completion_ticks=1000):
        if (profile!=preview.PROFILE or self.profile!=PROFILE or self.state!='idle' or self.request is not None
                or self.pending_action is not None or self.board.failure is not None
                or type(completion_ticks) is not int or not 1<=completion_ticks<=400000):
            raise UnsupportedDevice('Still mode2 preview requires explicit one-shot scene admission')
        self.request=self._prepare_scene(source_sha256,output_capacity,completion_ticks,profile)
        self.state='armed'
        return self.snapshot()

    def arm_next_initialization(self,profile=successor.PROFILE):
        return successor.arm(self,profile)

    def arm_next_scene(self,*,profile,source_sha256,output_capacity,completion_ticks=1000):
        return successor.scene(self,profile=profile,source_sha256=source_sha256,
            output_capacity=output_capacity,completion_ticks=completion_ticks)

    def arm_next_thumbnail_scene(self,*,profile=thumbnail.PROFILE,source_sha256,output_capacity,completion_ticks=1000):
        if profile!=thumbnail.PROFILE:
            raise UnsupportedDevice('Still thumbnail successor requires its exact named profile')
        return successor.scene(self,profile=profile,source_sha256=source_sha256,
            output_capacity=output_capacity,completion_ticks=completion_ticks)

    def arm_thumbnail_scene(self,*,profile=thumbnail.PROFILE,source_sha256,output_capacity,completion_ticks=1000):
        return self.arm_next_thumbnail_scene(profile=profile,source_sha256=source_sha256,
            output_capacity=output_capacity,completion_ticks=completion_ticks)

    def arm_jpeg_stream_initialization(self,profile=jpeg_stream.PROFILE):
        return jpeg_stream.arm_initialization(self,profile)

    def arm_jpeg_stream_scene(self,*,profile=jpeg_stream.PROFILE,source_sha256,source_capacity):
        return jpeg_stream.arm_scene(self,profile,source_sha256=source_sha256,source_capacity=source_capacity)

    def arm_jpeg_main_stream_initialization(self,profile=None):
        from . import still_resize_jpeg_main_stream as main_stream
        return main_stream.arm_initialization(self,main_stream.PROFILE if profile is None else profile)

    def arm_jpeg_main_stream_scene(self,*,profile=None,source_sha256,source_capacity):
        from . import still_resize_jpeg_main_stream as main_stream
        return main_stream.arm_scene(self,main_stream.PROFILE if profile is None else profile,
            source_sha256=source_sha256,source_capacity=source_capacity)

    def jpeg_stream_input(self,*,require_started=False):
        return jpeg_stream.input_view(self,require_started=require_started)

    def complete_jpeg_stream(self,expected_sha256):
        return jpeg_stream.complete(self,expected_sha256)

    def jpeg_stream_summary(self):
        return jpeg_stream.summary(self)

    def successor_summary(self):
        return successor.summary(self)

    def read(self,cpu,address,size):
        offset=self._typed(cpu,address,size)
        if offset not in (0,0x144):raise UnsupportedDevice('Still resize read lacks an audited control/status consumer')
        return self.registers[offset]

    def write(self,cpu,address,size,value):
        offset=self._typed(cpu,address,size)
        if type(value) is not int or not 0<=value<=0xffffffff:
            raise UnsupportedDevice('Still resize write is not a WORD')
        if jpeg_stream.before_write(self,offset,value):return
        if successor.before_write(self,offset,value):return
        if self.state=='armed':
            step=self.request['start_step'];expected=((0x140,0),(0x148,0x21),(0,self.registers[0]|1))[step]
            if (offset,value)!=expected:raise UnsupportedDevice('Still resize requires the exact next stock start store')
            if step==2:
                self._validate(source=True);self._route(drained=True)
                due=self.board.virtual_ticks+self.request['completion_ticks']
                if due>=1<<64:raise UnsupportedDevice('Still resize completion clock overflows')
                self.pending_action=dict(kind='resize-data-before-irq74',due_tick=due)
                self.request['started_tick']=self.board.virtual_ticks;self.state='running'
            self.request['start_step']+=1;self.registers[offset]=value;return
        if offset==0x144:
            if self.state=='idle' and self.request is None and self.pending_action is None and value==0x3f:
                # Whole 97ACD4/9368AC clears all six stale status bits after
                # its quiet opening idle poll. This supplies no completion.
                self.registers[offset]&=~value;return
            if self.state!='irq-pending' or value!=0x20 or self.registers[offset]!=0x20:
                raise UnsupportedDevice('Still resize status W1C precedes its actual completion')
            self._validate();self._route(drained=False)
            self.registers[offset]=0;self.request['acknowledged_tick']=self.board.virtual_ticks
            self.state='acknowledged';return
        if offset==0:
            if (self.state not in ('idle','closed','acknowledged') or self.pending_action is not None
                    or value!=self.registers[0]&~0x10000 or value&1):
                raise UnsupportedDevice('Still resize idle handshake precedes drained completion or is an unarmed start')
            self._validate();due=self.board.virtual_ticks+self.idle_ticks
            if due>=1<<64:raise UnsupportedDevice('Still resize idle clock overflows')
            self.registers[0]=value;self.pending_action=dict(kind='resize-idle-ack',due_tick=due,
                final=self.state=='acknowledged');self.state='stopping';return
        if offset==0x148:
            if value!=0 or self.state not in ('idle','idle-ready','closed'):
                raise UnsupportedDevice('Still resize interrupt disable precedes its idle acknowledgment')
            if self.state=='idle-ready':self.state='closed';self.request['closed_tick']=self.board.virtual_ticks
            self.registers[offset]=0;return
        if (self.state!='idle' or self.request is not None and not (successor.setup_allowed(self) or jpeg_stream.setup_allowed(self))
                or offset in (0x140,)):
            raise UnsupportedDevice('Still resize configuration changed outside its idle initializer')
        masks={4:0x40,0x10:0x3fffffff,0x14:0x7fff,0x18:0x7fff7fff,0x1c:0x3fffffff,
            0x20:0x7fff,0x24:0x1fff1fff,0x28:0x7fff7fff,0x2c:3,0x30:0x000f000f,
            0x34:3,0x38:0xffff,0x3c:0xffff,0x40:0x1fff,0x44:0x1fff,
            0x48:0x7fff7fff,0x4c:0x00070007,0x50:0xffffffff,0x54:0xffffffff,0x58:0xffffffff,
            **dict.fromkeys(range(0x60,0xdc,4),0x1ffff)}
        if offset not in masks or value&~masks[offset] or len(self.uploads)>=65536:
            raise UnsupportedDevice('Still resize upload escapes the audited register domain')
        self.registers[offset]=value;self.uploads.append((offset,value))

    def tick(self,board):
        if board is not self.board:raise UnsupportedDevice('Still resize timer requires its owning board')
        if jpeg_stream.tick(self):return True
        action=self.pending_action
        if action is None or board.virtual_ticks<action['due_tick']:return False
        self._validate(source=action['kind']=='resize-data-before-irq74')
        if action['kind']=='resize-data-before-irq74':
            if (set(action)!= {'kind','due_tick'} or self.state!='running'
                    or self.registers[0x148]!=0x21 or self.registers[0]&1!=1
                    or action['due_tick']!=self.request['started_tick']+self.request['completion_ticks']):
                raise UnsupportedDevice('Still resize completion lost its live request')
            self._route(drained=True)
            for address,row in self.request['rows']:board.write_memory(address,row,cpu_id=0)
            self.registers[0]&=~1;self.registers[0x144]=0x20;self.state='irq-pending'
            self.request['completed_tick']=board.virtual_ticks;board.queue_irq(0,74)
        elif action['kind']=='resize-idle-ack':
            if self.state!='stopping' or self.registers[0]&0x10001:
                raise UnsupportedDevice('Still resize idle response lost its control request')
            self.registers[0]|=0x10000;self.state='idle-ready' if action['final'] else 'idle'
            if action['final']:self.request['idle_tick']=board.virtual_ticks
        else:raise UnsupportedDevice('Still resize timer action is unknown')
        self.pending_action=None
        return True

    def next_event_tick(self,now):
        return max(now,self.pending_action['due_tick']) if self.pending_action is not None else None

    def snapshot(self):
        self._validate()
        stream=jpeg_stream.summary(self)
        result=dict(device=type(self).__name__,profile=self.profile,state=self.state,
            registers=dict(self.registers),uploads=len(self.uploads),pending_action=self.pending_action,
            geometry=self.request['plan'] if self.request is not None else None,
            successor=None if stream is not None else successor.summary(self),
            limits='At most two explicitly owned monochrome YYCbCr requests under named mode0, observed mode2-preview or mode1-thumbnail scene laws; padding untouched; bytes before physical IRQ74; ARM owns W1C, idle, unregister and callback; no physical filter claim')
        if stream is not None:result['jpeg_stream']=stream
        return result
