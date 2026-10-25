"""Stock face configuration and one complete, retained processing request.

145DC7C programs this bank before any face processing start. Table/cache
stores retain guest words; they are not physical implemented-bit masks or
predicted results. Four local arms and the central kick retain an awaiting
result request. They do not produce detections, transfers or an interrupt.
"""
from copy import deepcopy
import hashlib
import json
from x70_native._native import face_mode_write, face_mode_read

from .hardware import UnsupportedDevice

EMPTY_RESULT_PROFILE = 'declared-empty-face-results-for-virtual-scene-unmeasured'
RESULT_PROFILES = (EMPTY_RESULT_PROFILE,)
STOP_PROFILE = 'declared-armed-face-dma-idle-abort-unmeasured'
STOP_PROFILES = (STOP_PROFILE,)


class FaceAnalysisControl:
    BASE = 0xfff14000
    # Opt-in stream mode (emulator/testbed/STREAM.md): a permissive retained-word bank that
    # produces no detections, DMA or IRQ. Class default keeps older checkpoints loadable.
    stream = None
    mode_profile = None

    def enable_stream(self):
        if self.stream is not None:
            raise UnsupportedDevice('Face stream is already enabled')
        self.stream = dict(profile='declared-testbed-face-retained-words-no-results-unmeasured',
                           words={}, writes=0)
        self.board.record('Continuous-stream face bank enabled', profile=self.stream['profile'],
                          limits='Retained guest words only; DMA status reads idle; no detections or IRQ')
    STOP_PROFILE = STOP_PROFILE
    FIXED_WORDS = {0x8:0x01000000,0x564:0xffff,0x57c:0}
    # Table and workspace bus addresses follow each firmware's layout. X-M1 1.50 writes 0x0106f3e8/0x01070408
    # to 0x20/0x24 and 0x421a1800/0x421ab000/0x421afc00 to 0x4c/0x50/0x560.
    ADDRESS_WORDS = frozenset((0x20,0x24,0x4c,0x50,0x560))
    MODE_WORDS = {0x54:frozenset((0x00800100,0x006800d0)),
                  0x60:frozenset((0x003800bc,0x002800bc,0x00880006)),
                  0x68:frozenset((0x00620004,0x00690004,0x002b0097)),
                  0x70:frozenset((0x00010000,0x00200000,0x00500000)),
                  0x78:frozenset((0,)),
                  0x64:frozenset((0x01930191,0x00c90191,0x01050105)),
                  0x6c:frozenset((0x01fc01fc,0x00fd01fc,0x014a0149)),
                  0x74:frozenset((0x0282027f,0x0140027f,0x01a0019f)),
                  0x7c:frozenset((0,))}
    STRIDES = frozenset(0x84+12*i for i in range(16))
    BUFFERS = frozenset(0x88+12*i for i in range(16))
    RAW_CONFIG = frozenset((0x40,0x44,
        *(0x80+12*i for i in range(16)),
        *range(0x280,0x3e8,4),*range(0x470,0x4c0,4),*range(0x4c0,0x4d8,4),
        *range(0x4e0,0x4f8,4),*range(0x500,0x518,4),*range(0x520,0x538,4)))
    CONFIG = frozenset((*FIXED_WORDS,*ADDRESS_WORDS,*MODE_WORDS,*STRIDES,*BUFFERS,*RAW_CONFIG,0x10,0x570))
    DMA_WORDS = frozenset(0x800+0x80*channel+offset for channel in range(4)
                          for offset in (0,4,0x10,0x20,0x28,0x44))
    JOB_WORDS = frozenset((0x14,0x48,0x200,0x204,0x208,0x210,0x230,0x250,
                          0x274,0x278,0x574,0x578,4,0x8f8,
                          *(0x80c+0x80*i for i in range(4))))
    JOB_PREFIX = (0x274,0x278)
    JOB_TAIL = (0x210,0x230,0x250,
                *(0x280+24*i+j for i in range(10) for j in (0,4,8,16,20)),
                0x574,0x200,0x578,0x48)
    RECONFIG_ORDER = (0,2,3,1)
    ARM_ORDER = (1,3,0,2)

    def __init__(self, board, result_profile=None, result_ticks=400000, *, stop_profile=None, mode_profile=None):
        if result_profile is not None:
            raise ValueError('Face result production is not implemented; use result_profile=None')
        if type(result_ticks) is not int or result_ticks <= 0:
            raise ValueError('Face result delay must be a positive integer')
        if stop_profile not in (None,STOP_PROFILE):
            raise ValueError('Unknown face stop profile')
        self.board = board
        self.registers = {offset:0 for offset in self.CONFIG|self.DMA_WORDS|self.JOB_WORDS}
        self.writes = []
        self.dma_states = ['initial']*4
        self.dma_events = []
        self.last_rejected = None
        self.result_profile = result_profile
        self.result_ticks = result_ticks
        self.request = None
        self.stop_profile,self.stop=stop_profile,None
        self.mode_profile=mode_profile
        board.record('Face analysis passive configuration fixture',base=hex(self.BASE),
            words=len(self.CONFIG),evidence='145DC7C configuration;1464330/14647FC already-inactive DMA init',
            limits='Initial zeros and physical masks unmeasured; request retention supplies no results, transfers, completion, IRQ or pixels')

    def contains(self, address, size):
        return self.BASE <= address < self.BASE+0x1000

    def _reject(self, operation, core, address, size, value=None, reason='Unknown face register/width/state'):
        self.last_rejected = dict(operation=operation,cpu=core,address=hex(address),size=size,
            value=hex(value) if isinstance(value,int) else repr(value),reason=reason)
        self.board.record('Face analysis request rejected',**self.last_rejected)
        raise UnsupportedDevice(f'{reason}: {address:#x}/{size}')

    @staticmethod
    def _word(value):
        return isinstance(value,int) and not isinstance(value,bool) and 0<=value<=0xffffffff

    def _config_valid(self, offset, value):
        if not self._word(value):return False
        if offset in self.FIXED_WORDS:return value==self.FIXED_WORDS[offset]
        if offset in self.ADDRESS_WORDS:return not value&3
        if offset in self.MODE_WORDS:return value in self.MODE_WORDS[offset]
        if offset==0x10:
            return not value&~0x07000777 and all((value>>bit)&7<=4 for bit in (0,4,8,24))
        if offset==0x570:return value<=0xff
        if offset in self.STRIDES:return not value&~0x03ffffff
        if offset in self.BUFFERS:return not value&3 and 0x40000000<=value<0x60000000
        return offset in self.RAW_CONFIG

    @staticmethod
    def _digest(value):
        return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()

    def _request_valid(self, registers=None):
        """Bind retained words to the plain request anchor and accepted accesses."""
        request = self.request
        if (not isinstance(request,dict) or
                type(request.get('step')) is not int or type(request.get('channel_index')) is not int or
                not 0<=request['channel_index']<4 or
                request.get('phase') not in ('parameter-byte','programming','reconfiguring','arming',
                    'interrupt-selection','central-start','armed-awaiting-result') or
                not 0<=request['step']<64 or
                request.get('protocol_sha256') != self._protocol_digest(request) or
                not isinstance(request.get('configured_channels'),list) or
                any(type(c) is not int or not 0<=c<4 for c in request['configured_channels']) or
                (request.get('selected_channel') is not None and
                    (type(request['selected_channel']) is not int or not 0<=request['selected_channel']<4)) or
                self._digest(request['initial_registers']) != request['initial_sha256'] or
                self._digest(request['accesses']) != request['accesses_sha256']):
            return False
        if not self._progress_valid(request): return False
        registers=self.registers if registers is None else registers
        words = request['initial_registers'].copy()
        if set(words) != set(registers) or any(not self._word(v) for v in words.values()):
            return False
        for event in request['accesses']:
            if (event['operation'] not in ('read','write') or
                    type(event['offset']) is not int or event['offset'] not in words or
                    not self._word(event['value'])):
                return False
            if event['operation'] == 'read':
                if words[event['offset']] != event['value']: return False
            else:
                words[event['offset']] = event['value']
        return words == registers and all(self._word(v) for v in registers.values())

    def _protocol_digest(self, request):
        return self._digest({name:request.get(name) for name in ('phase','step','channel_index',
            'optional_mask_words','input_pointer','requested_tick','result_state',
            'configured_channels','selected_channel','initial_sha256','accesses_sha256')})

    def _progress_valid(self, request):
        """Protocol state must describe the actual access prefix, not just a digest."""
        phase,step,index=request['phase'],request['step'],request['channel_index']
        phases=('parameter-byte','programming','reconfiguring','arming','interrupt-selection',
                'central-start','armed-awaiting-result')
        level=phases.index(phase)
        if (phase in ('parameter-byte','interrupt-selection','central-start','armed-awaiting-result') and step!=0
                or phase in ('reconfiguring','arming') and step>=5): return False
        events=request['accesses']
        expected=[('write',0x14,'parameter-byte',0)]
        if level>=1: expected.append(('write',0x570,'parameter-byte',0))
        optional=request['optional_mask_words']
        if optional is not None and type(optional) is not bool: return False
        sequence=self.JOB_PREFIX+((0x204,0x208) if optional is not False else ())+self.JOB_TAIL
        amount=step if phase=='programming' else len(sequence) if level>=2 else 0
        if amount>len(sequence): return False
        expected.extend(('write',o,'programming',i) for i,o in enumerate(sequence[:amount]))
        configured=[];selected=None
        amount=index*5+step if phase=='reconfiguring' else 20 if level>=3 else 0
        for start in range(0,amount,5):
            if len(events)<=len(expected): return False
            first=events[len(expected)]
            offset=first.get('offset')
            if type(offset) is not int or offset not in tuple(0x820+0x80*c for c in range(4)): return False
            channel=(offset-0x820)//0x80;base=0x800+0x80*channel
            count=min(5,amount-start)
            slots=(('read',0x20),('write',0x20),('write',4),('write',0x28),('write',0x44))
            expected.extend((op,base+o,'reconfiguring',i) for i,(op,o) in enumerate(slots[:count]))
            if count==5: configured.append(channel)
            else: selected=channel
        if request.get('configured_channels')!=configured or request.get('selected_channel')!=selected: return False
        amount=index*5+step if phase=='arming' else 20 if level>=4 else 0
        for i in range(amount):
            channel=self.ARM_ORDER[i//5];base=0x800+0x80*channel
            op,o=(('write',0xc),('read',0x20),('write',0x20),('read',4),('write',4))[i%5]
            expected.append((op,base+o,'arming',i%5))
        if level>=5: expected.append(('write',0x8f8,'interrupt-selection',0))
        if level>=6: expected.append(('write',4,'central-start',0))
        return len(events)==len(expected) and all(
            type(event.get('offset')) is int and type(event.get('step')) is int and
            (event.get('operation'),event.get('offset'),event.get('phase'),event.get('step'))==item
            for event,item in zip(events,expected))

    def _begin_request(self, core, address, size, value, *, inactive_mode=0):
        written = {int(event['address'],16)-self.BASE for event in self.writes}
        if (type(inactive_mode) is not int or inactive_mode not in (0,2) or
                self.request is not None or not self._word(value) or value >= 0x1000 or
                not self.CONFIG <= written or self.dma_states != ['configured']*4 or
                any(not self._word(v) for v in self.registers.values()) or
                any(not self._config_valid(o,self.registers[o]) for o in self.CONFIG) or
                any(self.registers[0x804+0x80*i] != 0 or
                    self.registers[0x800+0x80*i] != 0 or
                    self.registers[0x810+0x80*i] != 0 or
                    self.registers[0x820+0x80*i]&~0x00700000 != 0x3000 or
                    (self.registers[0x820+0x80*i]>>20)&7 > 4 or
                    self.registers[0x828+0x80*i] != 0x100 or
                    self.registers[0x844+0x80*i] != inactive_mode for i in range(4))):
            self._reject('write',core,address,size,value,
                reason='Face request requires complete passive initialization and four configured inactive channels')
        anchor = deepcopy(self.registers)
        self.request = dict(phase='parameter-byte',step=0,channel_index=0,
            optional_mask_words=None,configured_channels=[],selected_channel=None,initial_registers=anchor,
            initial_sha256=self._digest(anchor),accesses=[],accesses_sha256=self._digest([]),
            input_pointer=None,requested_tick=None,
            result_state='unproduced',effects='Desired request only; no data, result or IRQ')
        self._request_record('write',core,0x14,value)
        self.request['protocol_sha256']=self._protocol_digest(self.request)

    def _request_record(self, operation, core, offset, value):
        request = self.request
        event = dict(operation=operation,cpu=core,offset=offset,value=value,
                     phase=request['phase'],step=request['step'])
        if operation == 'write':
            old = self.registers[offset]
            self.registers[offset] = value
            self.writes.append(dict(cpu=core,address=hex(self.BASE+offset),old=hex(old),value=hex(value)))
        request['accesses'].append(event)
        request['accesses_sha256'] = self._digest(request['accesses'])
        self.board.record('Face processing request retained',**event,
            effects='No processing, DMA payload, detections, completion or IRQ')
        return value

    def _request_access(self, operation, core, address, size, value=None):
        offset = address-self.BASE
        request = self.request
        if not self._request_valid():
            self._reject(operation,core,address,size,value,reason='Face retained request/configuration was changed outside its access ledger')
        phase,step = request['phase'],request['step']
        next_phase,next_step,next_channel = phase,step,request['channel_index']
        accepted = False
        if phase == 'parameter-byte':
            accepted = operation=='write' and offset==0x570 and self._word(value) and value<=0x1f
            next_phase,next_step = 'programming',0
        elif phase == 'programming':
            # The software may skip both mask words; partial optional pairs reject.
            sequence = self.JOB_PREFIX+(0x204,0x208)+self.JOB_TAIL
            if step==2 and operation=='write' and offset==0x210:
                sequence = self.JOB_PREFIX+self.JOB_TAIL
                chosen_optional = False
            else:
                chosen_optional = True if step==2 else request['optional_mask_words']
                if chosen_optional is False: sequence = self.JOB_PREFIX+self.JOB_TAIL
            accepted = (operation=='write' and step<len(sequence) and offset==sequence[step]
                        and self._word(value))
            if offset==0x48:
                accepted = accepted and value%4==0 and 0x40000000<=value<0x60000000
            if offset==0x578: accepted = accepted and value==0x000b000b
            next_step=step+1
            if next_step==len(sequence): next_phase,next_step,next_channel='reconfiguring',0,0
        elif phase in ('reconfiguring','arming'):
            if phase=='reconfiguring' and step==0:
                channel=(offset-0x820)//0x80 if offset in tuple(0x820+0x80*c for c in range(4)) else None
                if (operation!='read' or channel is None or self.dma_states[channel]!='configured' or
                        self.registers[0x804+0x80*channel]!=0):
                    self._reject(operation,core,address,size,value,
                        reason='Face reconfiguration must first select an initialized inactive physical channel by its control read')
            else:
                channel=request['selected_channel'] if phase=='reconfiguring' else self.ARM_ORDER[request['channel_index']]
            base=0x800+0x80*channel
            old20=self.registers[base+0x20]
            if phase=='reconfiguring':
                expected=(('read',0x20),('write',0x20),('write',4),('write',0x28),('write',0x44))[step]
                accepted = operation==expected[0] and offset==base+expected[1]
                if operation=='write':
                    if step==1:
                        accepted = (accepted and self._word(value) and
                            value&~0x00700000 == ((old20|0x3000)&0x8f8fffff)&~0x00700000 and
                            (value>>20)&7<=4)
                    else: accepted = accepted and value==(0,0x100,2)[step-2] and self._word(value)
            else:
                expected=(('write',0xc),('read',0x20),('write',0x20),('read',4),('write',4))[step]
                accepted = operation==expected[0] and offset==base+expected[1]
                if operation=='write':
                    wanted = 1 if step in (0,4) else old20&0xefffffff
                    accepted = accepted and self._word(value) and value==wanted
            if operation=='read' and accepted: value=self.registers[offset]
            next_step=step+1
            if next_step==5:
                next_step=0;next_channel+=1
                if next_channel==4:
                    next_channel=0
                    next_phase='arming' if phase=='reconfiguring' else 'interrupt-selection'
        elif phase=='interrupt-selection':
            accepted=operation=='write' and offset==0x8f8 and self._word(value) and value==0x10000000
            next_phase='central-start'
        elif phase=='central-start':
            accepted=operation=='write' and offset==4 and self._word(value) and value==1
            next_phase='armed-awaiting-result'
        if not accepted:
            self._reject(operation,core,address,size,value,
                reason='Face request access is outside the complete recovered submission; results/status/completion remain unsupported')
        result=self._request_record(operation,core,offset,value)
        if phase=='reconfiguring':
            if step==0: request['selected_channel']=channel
            if step==4:
                request['configured_channels'].append(channel)
                request['selected_channel']=None
        if phase=='programming' and step==2: request['optional_mask_words']=chosen_optional
        if offset==0x48 and operation=='write': request['input_pointer']=hex(value)
        request['phase'],request['step'],request['channel_index']=next_phase,next_step,next_channel
        if next_phase=='armed-awaiting-result':
            request['requested_tick']=getattr(self.board,'virtual_ticks',None)
            self.dma_states=['armed-awaiting-result']*4
            self.board.record('Face processing request armed; awaiting unmodeled result',
                input=request['input_pointer'],result_profile=self.result_profile,
                limits='No inferred data/status/count/completion/IRQ response from a desired arm')
        request['protocol_sha256']=self._protocol_digest(request)
        return result

    def _dma(self, operation, core, address, size, value=None):
        offset=address-self.BASE
        channel=(offset-0x800)//0x80
        local=(offset-0x800)%0x80
        state=self.dma_states[channel]
        read_steps={'initial':(4,'clear-enable'),'poll-idle':(0x10,'poll-enable'),
                    'poll-enable':(4,'clear-command'),'config-read':(0x20,'config-write')}
        write_steps={'clear-enable':(4,0,'poll-idle'),
                     'clear-command':(0,0,'config-read'),
                     'config-control':(4,0,'config-count'),
                     'config-count':(0x28,0x100,'config-extra'),
                     'config-extra':(0x44,0,'configured')}
        stored=self.registers[offset]
        next_state=None
        if operation=='read':
            step=read_steps.get(state)
            if step is not None and local==step[0] and self._word(stored) and stored==0:
                next_state=step[1]
                value=stored
        elif self._word(value) and self._word(stored) and stored==0:
            step=write_steps.get(state)
            if step is not None and (local,value)==step[:2]:next_state=step[2]
            elif (state=='config-write' and local==0x20 and value&~0x00703000==0
                    and value&0x000fffff==0x3000 and (value>>20)&7<=4):
                next_state='config-control'
        if next_state is None:
            self._reject(operation,core,address,size,value,
                reason='Only exact guest clear/poll/configure of an initially inactive face DMA channel is modeled')
        self.dma_states[channel]=next_state
        event=dict(operation=operation,cpu=core,address=hex(address),value=hex(value),
                   channel=channel,old_state=state,state=next_state)
        self.dma_events.append(event)
        self.board.record('Face DMA inactive initialization',**event)
        return value

    @classmethod
    def _stop_slot(cls, step):
        #145987C central reset, then145FFF4 /1464684 channel shutdown.
        if step<2:return 'write',0,step,None
        if step in (26,27):return 'write',0,step-26,None
        if step>=28:
            channel=cls.ARM_ORDER[(step-28)//2]
            return ('read' if step%2==0 else 'write'),0x804+0x80*channel,None,channel
        channel=cls.ARM_ORDER[(step-2)//6]
        operation,local=(('write',0xc),('read',4),('write',4),
                         ('read',0x10),('read',0x20),('write',0x20))[(step-2)%6]
        return operation,0x800+0x80*channel+local,None,channel

    @staticmethod
    def _stop_phase(step):
        return ('central-reset' if step<2 else 'stopping-dma' if step<26 else
                'complete' if step==26 else 'closing-reset' if step<28 else
                'closing-dma' if step<36 else 'closed')

    def _stop_valid(self):
        if isinstance(self.stop,dict) and 'reconfiguration' in self.stop:
            from . import face_still_reconfiguration
            try:
                face_still_reconfiguration.validate(self)
                return True
            except UnsupportedDevice:
                return False
        stop=self.stop
        if (self.stop_profile!=STOP_PROFILE or not isinstance(stop,dict)
                or type(stop.get('step')) is not int or not 0<=stop['step']<=36
                or type(stop.get('cpu')) is not int or stop['cpu'] not in (0,1)
                or stop.get('profile')!=STOP_PROFILE or stop.get('result_state')!='unproduced'
                or stop.get('request_sha256')!=self._digest(self.request)
                or not isinstance(stop.get('initial_registers'),dict)
                or not self._request_valid(stop['initial_registers'])
                or not isinstance(stop.get('accesses'),list)
                or len(stop['accesses'])!=stop['step']):return False
        words=stop['initial_registers'].copy();control=None
        states=['armed-awaiting-result']*4
        for step,event in enumerate(stop['accesses']):
            operation,offset,fixed,channel=self._stop_slot(step)
            if (not isinstance(event,dict) or set(event)!= {'operation','offset','cpu','value'}
                    or event['operation']!=operation or type(event['offset']) is not int
                    or event['offset']!=offset or event['cpu']!=stop['cpu']
                    or not self._word(event['value'])):return False
            value=event['value']
            if offset==0:
                if value!=fixed:return False
                control=value
            elif operation=='read':
                if value!=words[offset] or offset%0x80==0x10 and value!=0:return False
            else:
                wanted=1 if offset%0x80==0xc else words[offset]&~(1 if offset%0x80==4 else 0x10000000)
                if value!=wanted:return False
                words[offset]=value
                if offset%0x80==0x20:states[channel]='stopped'
                if step>=28:states[channel]='closed'
        return (words==self.registers and states==self.dma_states
                and stop.get('control')==control and stop.get('phase')==self._stop_phase(stop['step']))

    def _stop_access(self, operation, core, address, size, value=None):
        offset=address-self.BASE
        if self.stop is None:
            if (operation!='write' or size!=4 or offset!=0 or not self._word(value) or value!=0
                    or type(core) is not int or core not in (0,1)
                    or not self._request_valid() or self.request['phase']!='armed-awaiting-result'
                    or self.request['accesses'][-1]['cpu']!=core
                    or self.dma_states!=['armed-awaiting-result']*4
                    or any(self.registers[0x810+0x80*i]!=0 for i in range(4))):
                self._reject(operation,core,address,size,value,
                    reason='Face abort requires a complete armed request with four retained idle DMA status words')
            self.stop=dict(step=0,phase='central-reset',cpu=core,control=None,accesses=[],
                initial_registers=deepcopy(self.registers),request_sha256=self._digest(self.request),
                result_state='unproduced',profile=self.stop_profile)
        if not self._stop_valid():
            self._reject(operation,core,address,size,value,
                reason='Face abort request/registers/order changed outside its stop ledger')
        stop=self.stop
        if stop['step']==36:
            self._reject(operation,core,address,size,value,reason='Face completed abort has no further proven access')
        wanted_operation,wanted_offset,fixed,channel=self._stop_slot(stop['step'])
        accepted=(size==4 and type(core) is int and core==stop['cpu']
                  and operation==wanted_operation and offset==wanted_offset)
        if operation=='read' and accepted:
            value=self.registers[offset]
            accepted=self._word(value) and (offset%0x80!=0x10 or value==0)
        elif operation=='write' and accepted:
            wanted=(fixed if offset==0 else 1 if offset%0x80==0xc else
                    self.registers[offset]&~(1 if offset%0x80==4 else 0x10000000))
            accepted=self._word(value) and value==wanted
        if not accepted:
            self._reject(operation,core,address,size,value,
                reason='Face abort access is outside the recovered central-reset/four-channel idle-stop transaction')
        event=dict(operation=operation,offset=offset,cpu=core,value=value)
        stop['accesses'].append(event)
        if operation=='write':
            if offset==0:stop['control']=value
            else:self.registers[offset]=value
            if offset%0x80==0x20:self.dma_states[channel]='stopped'
            if stop['step']>=28:self.dma_states[channel]='closed'
        stop['step']+=1
        stop['phase']=self._stop_phase(stop['step'])
        self.board.record('Face armed request abort retained',**event,step=stop['step'],
            profile=self.stop_profile,
            limits='Literal reset and retained zero DMA10 idle convention only; no face result, payload, completion IRQ or callback')
        return value

    def read(self, core, address, size):
        offset=address-self.BASE
        value=face_mode_read(self,core,address,size)
        if value is not None:
            return value
        if isinstance(self.stop,dict) and 'reconfiguration' in self.stop:
            from . import face_still_reconfiguration
            return face_still_reconfiguration.access(self,'read',core,address,size)
        if self.stream is not None:
            if size!=4 or address&3:self._reject('read',core,address,size,reason='Face stream requires aligned WORDs')
            return self.stream['words'].get(offset,self.registers.get(offset,0))
        if self.stop is not None:
            return self._stop_access('read',core,address,size)
        if size==4 and self.request is not None:
            return self._request_access('read',core,address,size)
        if size!=4 or offset not in self.DMA_WORDS:
            self._reject('read',core,address,size,reason='Face configuration/output read semantics are unproven')
        return self._dma('read',core,address,size)

    def write(self, core, address, size, value):
        offset=address-self.BASE
        if face_mode_write(self,core,address,size,value):
            return
        if isinstance(self.stop,dict) and 'reconfiguration' in self.stop:
            from . import face_still_reconfiguration
            return face_still_reconfiguration.access(self,'write',core,address,size,value)
        if self.stream is not None:
            if size!=4 or address&3:self._reject('write',core,address,size,value,reason='Face stream requires aligned WORDs')
            self.stream['words'][offset]=value;self.stream['writes']+=1
            return
        if self.stop is not None or (self.stop_profile is not None and offset==0):
            return self._stop_access('write',core,address,size,value)
        if size==4 and self.request is not None:
            return self._request_access('write',core,address,size,value)
        if size==4 and offset==0x14:
            self._begin_request(core,address,size,value)
            return
        if size!=4 or offset not in self.CONFIG|self.DMA_WORDS:
            self._reject('write',core,address,size,value)
        if offset in self.DMA_WORDS:
            self._dma('write',core,address,size,value)
        elif not self._config_valid(offset,value):
            self._reject('write',core,address,size,value,
                reason='Face configuration word violates the recovered software setter domain')
        old=self.registers[offset]
        self.registers[offset]=value
        event=dict(cpu=core,address=hex(address),old=hex(old),value=hex(value))
        self.writes.append(event)
        self.board.record('Face analysis configuration retained',**event,
            effects='Passive guest words only; no processing, data, completion or IRQ')

    def arm_still_reconfiguration(self, profile):
        from . import face_still_reconfiguration
        return face_still_reconfiguration.arm(self,profile)

    def still_reconfiguration_summary(self):
        from . import face_still_reconfiguration
        return face_still_reconfiguration.summary(self)

    def arm_still_cancellation(self, profile):
        from . import face_still_cancellation
        return face_still_cancellation.arm(self,profile)

    def still_cancellation_summary(self):
        from . import face_still_cancellation
        return face_still_cancellation.summary(self)

    def snapshot(self):
        self.still_reconfiguration_summary()
        return dict(name=type(self).__name__,base=hex(self.BASE),
            registers={hex(self.BASE+o):hex(v) for o,v in self.registers.items()},
            writes=self.writes,dma_states=self.dma_states,dma_events=self.dma_events,
            last_rejected=self.last_rejected,
            result_profile=self.result_profile,result_ticks=self.result_ticks,
            mode_profile=self.mode_profile,mode_resets=getattr(self,'mode_resets',0),
            stop_profile=self.stop_profile,stop=deepcopy(self.stop),
            request=deepcopy(self.request),awaiting_result=(self.stop is None and self.request is not None and self.request['phase']=='armed-awaiting-result'),
            limits=['203 recovered configuration offsets retain guest words, not face detections.',
                'Mutable parameter/size/table stores are raw32-bit configuration; physical masks remain unknown.',
                'Stride fields and selector domains follow software construction, not measured silicon masks.',
                'Initial configuration zeros and already-inactive DMA04/10/20 zeros are unmeasured fixtures.',
                'The initialized four-channel clear/poll sequence and one complete job reconfiguration/arm are retained; no live DMA quiescence is inferred.',
                'Mode table values and literal setters are bounded; high DRAM addresses are retained without reading or writing their payloads.',
                'Central kick retains an armed-awaiting-result request. It is capturable plain desired state, without hidden timers or a result response.',
                'The optional result profile name is reserved; non-None constructor profiles reject because result production is not implemented.',
                'Default output/result/status, completion strobes, active stop and IRQ2F remain unsupported.',
                'Explicit stop profile accepts only145987C reset0/1 and1464684 channel order1,3,0,2; retained DMA10 zero supplies an unmeasured idle convention, without a face result or interrupt.',
                'The original submission remains immutable; a separate ordered abort ledger owns only the central reset and DMA control clears. One later1460110/1464444 reset pair and four read/clear pairs may close the canceled channels.',
                'An explicitly armed still reconfiguration may retain the whole14601C0 mode-zero size-change, idle channel release/reinitialization and one later desired submission; it supplies no face result or IRQ.',
                'No host callback, task, queue, result, face output or LCD/HDMI pixel effect is supplied.'])
