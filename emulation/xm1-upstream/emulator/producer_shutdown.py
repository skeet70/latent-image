"""Exact idle fresh-arm shutdown, preserving producer and histogram history.

The declared stop profile admits92CD04(maskFF,value0), then92F6EC(channel0)
only after histogram channel1 was already released. Retained890zero is an
unmeasured idle convention. No transfer, response, interrupt or guest event is
created;92F6EC unregisters the actual IRQ client and92F528 frees its handle.
After unregister, at most16 chained10CA7F8 D4-zero/FC-zero getter/setter
transactions retain desired GLOBAL fields only, including stock XOR disable.
After the observed D4→FC pair, one933258 reset may write zero and poll bit28.
The same explicitly declared stop delay supplies that bit alone; immutable
arm/data/histogram/shutdown history survives, with no new processing lifetime.
"""
from .hardware import UnsupportedDevice
from . import histogram_protocol as hp


class FreshProducerShutdownMixin:
    FRESH_SHUTDOWN_SEQUENCE = (
        ('read',0x004,0x33173317),('read',0x008,0x10000),
        ('write',0x004,0x33170000),('write',0x008,0x10000),
        ('read',0x884,1),('write',0x884,0),
        ('read',0x890,0),('write',0x8f8,0))
    FRESH_SHUTDOWN_PRIOR = {0x004:0x33173317,0x008:0x10000,
                           0x884:1,0x890:0,0x8f8:0x10000000}
    #10CB5C8 ->10CA7F8(maskD4,0,0).40/80 use XOR on the
    #9336A4 getter's two-bit field, then9335D0 retains it at bits16..17.
    POST_SHUTDOWN_D4 = (
        ('read',0x10030204),('write',0x10030200),
        ('read',0x10030200),('write',0x10030000),
        ('read',0x10030000),('read',0x10030000),('write',0x10020000),
        ('read',0x10020000),('read',0x10020000),('write',0x10000000))
    POST_SHUTDOWN_FAMILIES = ('D4-zero','FC-zero','GLOBAL-zero-stop')
    POST_SHUTDOWN_LIMIT = 16
    POST_SHUTDOWN_RESET_POLL_LIMIT = 65536

    @staticmethod
    def _post_shutdown_sequence(prior, family):
        """Actual10CA7F8 zero-disable getter/setter accesses, including XOR."""
        if family not in FreshProducerShutdownMixin.POST_SHUTDOWN_FAMILIES:
            raise UnsupportedDevice('Unknown post-shutdown desired control family')
        if family=='GLOBAL-zero-stop':return (('read',prior),('write',0))
        sequence=[];word=prior
        for mask in ((0x0c,0x30,0x300) if family=='FC-zero' else (0x0c,0x300)):
            sequence.append(('read',word));word&=~mask;sequence.append(('write',word))
        if family=='FC-zero':
            #933580 gets both bits12/13;9334E0 clears only param[0]/bit12.
            sequence.extend((('read',word),('read',word)))
            word&=~0x1000;sequence.append(('write',word))
        for bit in (0x10000,0x20000):
            #9336A4 getter plus9335D0 setter, preserving bit18 and other fields.
            sequence.extend((('read',word),('read',word)))
            word^=bit;sequence.append(('write',word))
        return tuple(sequence)

    def _post_shutdown_candidates(self, prior, accesses, previous_requests=()):
        candidates=[]
        for family in self.POST_SHUTDOWN_FAMILIES:
            if family=='GLOBAL-zero-stop':
                previous=previous_requests
                if (self.stop_profile!='stock-idle-stop-handshakes-unmeasured'
                        or prior!=0x10030000 or len(previous)!=2
                        or [r['family'] for r in previous[:2]]!=['D4-zero','FC-zero']
                        or any(r['family']=='GLOBAL-zero-stop' for r in previous)):
                    continue
                if (len(accesses)<=2 and all((e['operation'],e['word'])==
                        self._post_shutdown_sequence(prior,family)[i] for i,e in enumerate(accesses))
                        or 2<len(accesses)<=2+self.POST_SHUTDOWN_RESET_POLL_LIMIT
                        and [(e['operation'],e['word']) for e in accesses[:2]]==[('read',prior),('write',0)]
                        and all(e['operation']=='read' and e['word'] in (0,0x10000000) for e in accesses[2:])
                        and all(e['word']==0 for e in accesses[2:-1])):
                    candidates.append(family)
                continue
            sequence=self._post_shutdown_sequence(prior,family)
            if len(accesses)<=len(sequence) and all(
                    (event['operation'],event['word'])==sequence[index]
                    for index,event in enumerate(accesses)):
                candidates.append(family)
        return candidates

    def _post_shutdown_request_state(self,request,candidates):
        if candidates==['GLOBAL-zero-stop'] and request['step']>=2:
            if request.get('acknowledged_tick') is None:return 'waiting-ack'
            return ('completed' if request['step']>2 and request['accesses'][-1]['word']==0x10000000
                    else 'acknowledged-awaiting-poll')
        return ('completed' if any(request['step']==len(self._post_shutdown_sequence(request['prior_word'],f))
                for f in candidates if f!='GLOBAL-zero-stop') else 'in-progress')

    def _validate_post_shutdown_control(self):
        record=self.post_shutdown_control
        if record is None:return
        stop=self.fresh_shutdown
        self._validate_fresh_shutdown()
        if (type(record) is not dict or type(record.get('step')) is not int
                or not 1<=record['step']<=self.POST_SHUTDOWN_LIMIT*15+self.POST_SHUTDOWN_RESET_POLL_LIMIT
                or type(record.get('cpu')) is not int or record['cpu'] not in (0,1)
                or type(record.get('prior_word')) is not int or record['prior_word']!=0x10030204
                or stop is None or stop['state']!='completed'
                or record['cpu']!=stop['cpu']
                or record.get('shutdown_sha256')!=hp.digest(stop)
                or type(record.get('requests')) is not list
                or not 1<=len(record['requests'])<=self.POST_SHUTDOWN_LIMIT):
            raise UnsupportedDevice('Post-shutdown desired control ownership/history changed')
        interrupts=getattr(self.board,'interrupts',None)
        if interrupts is None or any(bank.enabled&(1<<0x2e) for bank in interrupts.banks):
            raise UnsupportedDevice('Post-shutdown desired control requires IRQ2E unregistered')
        word=record['prior_word'];steps=0
        for index,request in enumerate(record['requests']):
            if (type(request) is not dict or type(request.get('ordinal')) is not int
                    or request['ordinal']!=index+1 or type(request.get('step')) is not int
                    or not 1<=request['step']<=2+self.POST_SHUTDOWN_RESET_POLL_LIMIT
                    or type(request.get('accesses')) is not list
                    or len(request['accesses'])!=request['step']
                    or type(request.get('prior_word')) is not int or request['prior_word']!=word):
                raise UnsupportedDevice('Post-shutdown desired control request chain changed')
            reset=request.get('family')=='GLOBAL-zero-stop' and request['step']>=2
            if reset:
                requested=request.get('requested_tick');due=request.get('due_tick');ack=request.get('acknowledged_tick')
                ack_polls=request.get('acknowledged_poll_count')
                if (type(self.board.virtual_ticks) is not int or not 0<=self.board.virtual_ticks<1<<64
                        or type(self.stop_ticks) is not int or self.stop_ticks<1
                        or type(requested) is not int or not 0<=requested<=self.board.virtual_ticks
                        or type(due) is not int or due!=requested+self.stop_ticks or due>=1<<64
                        or ack is not None and (type(ack) is not int or not due<=ack<=self.board.virtual_ticks)
                        or request.get('configuration_sha256')!=self._config_hash()
                        or request.get('retained_configuration_sha256')!=self._retained_config_hash()
                        or type(request.get('poll_ticks')) is not list
                        or len(request['poll_ticks'])!=request['step']-2
                        or (ack_polls is not None if ack is None else
                            type(ack_polls) is not int or not 0<=ack_polls<=len(request['poll_ticks']))):
                    raise UnsupportedDevice('Post-shutdown global reset delay/configuration changed')
            for step,event in enumerate(request['accesses']):
                if (type(event) is not dict or set(event)!={'operation','address','word','cpu','step'}
                        or event['operation'] not in ('read','write') or event['address']!=hex(self.GLOBAL)
                        or type(event['word']) is not int or not self._global_valid(event['word'])
                        or type(event['step']) is not int or event['step']!=step
                        or type(event['cpu']) is not int or event['cpu']!=record['cpu']):
                    raise UnsupportedDevice('Post-shutdown desired control ordered access changed')
                if reset and step>=2:
                    tick=request['poll_ticks'][step-2]
                    after_ack=ack is not None and step-2>=ack_polls
                    if (type(tick) is not int or not requested<=tick<=self.board.virtual_ticks
                            or step>2 and tick<request['poll_ticks'][step-3]
                            or ack is not None and (tick<ack if after_ack else tick>ack)
                            or event['word']!=(0x10000000 if after_ack else 0)):
                        raise UnsupportedDevice('Post-shutdown global reset poll precedes its actual acknowledgment')
                    word=event['word']
                elif event['operation']=='write':word=event['word']
                elif word!=event['word']:
                    raise UnsupportedDevice('Post-shutdown desired control read differs from its projection')
            candidates=self._post_shutdown_candidates(request['prior_word'],request['accesses'],record['requests'][:index])
            state=self._post_shutdown_request_state(request,candidates)
            completed=state=='completed'
            if reset and request['acknowledged_tick'] is not None:word=0x10000000
            if (not candidates or request.get('candidates')!=candidates
                    or request.get('family')!=(candidates[0] if len(candidates)==1 else None)
                    or request.get('state')!=state or index+1<len(record['requests']) and not completed
                    or type(request.get('current_word')) is not int or request['current_word']!=word):
                raise UnsupportedDevice('Post-shutdown desired control family/prefix projection changed')
            steps+=request['step']
        if (type(record.get('current_word')) is not int or record['current_word']!=word
                or record['step']!=steps or record.get('state')!=record['requests'][-1]['state']
                or type(self.registers.get(self.GLOBAL)) is not int or self.registers[self.GLOBAL]!=word):
            raise UnsupportedDevice('Post-shutdown desired control current projection changed')

    def _post_shutdown_global_word(self, expected):
        """Project live GLOBAL without rewriting the preceding arm/config ledger."""
        self._validate_post_shutdown_control()
        record=self.post_shutdown_control
        if record is None:return expected
        if type(expected) is not int or expected!=record['prior_word']:
            raise UnsupportedDevice('Post-shutdown desired control historical GLOBAL changed')
        return record['current_word']

    def _post_shutdown_control_access(self, operation, core, address, size, value):
        record=self.post_shutdown_control
        if record is None:
            interrupts=getattr(self.board,'interrupts',None)
            if (interrupts is None or any(bank.enabled&(1<<0x2e) for bank in interrupts.banks)
                    or self.registers[self.GLOBAL]!=0x10030204):
                raise UnsupportedDevice('Post-shutdown modes require original GLOBAL and IRQ2E unregistered')
            record=dict(state='in-progress',step=0,cpu=self.fresh_shutdown['cpu'],requests=[],
                prior_word=0x10030204,current_word=0x10030204,shutdown_sha256=hp.digest(self.fresh_shutdown))
        else:
            self._validate_post_shutdown_control()
        if (address!=self.GLOBAL or size!=4 or type(core) is not int
                or core not in (0,1) or core!=record['cpu']):
            raise UnsupportedDevice('Post-shutdown modes require close-owner GLOBAL WORD accesses')
        beginning=not record['requests'] or record['state']=='completed'
        if beginning:
            if len(record['requests'])>=self.POST_SHUTDOWN_LIMIT:
                raise UnsupportedDevice('Post-shutdown desired mode request budget exhausted')
            request=dict(ordinal=len(record['requests'])+1,state='in-progress',step=0,
                family=None,candidates=list(self.POST_SHUTDOWN_FAMILIES),accesses=[],
                prior_word=record['current_word'],current_word=record['current_word'])
        else:request=record['requests'][-1]
        actual=self.registers[self.GLOBAL] if operation=='read' else value
        event=dict(operation=operation,address=hex(address),word=actual,cpu=core,step=request['step'])
        accesses=request['accesses']+[event]
        candidates=self._post_shutdown_candidates(request['prior_word'],accesses,
            record['requests'] if beginning else record['requests'][:-1])
        if type(actual) is not int or not candidates:
            raise UnsupportedDevice('Post-shutdown control differs from recovered mode/reset transactions')
        reset=candidates==['GLOBAL-zero-stop'] and len(accesses)>=2
        if reset and len(accesses)==2:
            tick=self.board.virtual_ticks
            if (type(tick) is not int or tick<0 or type(self.stop_ticks) is not int
                    or self.stop_ticks<1 or tick+self.stop_ticks>=1<<64):
                raise UnsupportedDevice('Post-shutdown global reset requires bounded typed delay')
            reset_fields=dict(requested_tick=tick,due_tick=tick+self.stop_ticks,
                acknowledged_tick=None,acknowledged_poll_count=None,poll_ticks=[],configuration_sha256=self._config_hash(),
                retained_configuration_sha256=self._retained_config_hash())
        elif reset:
            tick=self.board.virtual_ticks;ack=request.get('acknowledged_tick')
            if (type(tick) is not int or tick<request['requested_tick']
                    or actual!=(0x10000000 if ack is not None else 0)):
                raise UnsupportedDevice('Post-shutdown global reset poll differs from its acknowledged state')
        if operation=='write':
            old=self.registers[address];self.registers[address]=actual
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(actual),old=hex(old)))
        request['accesses'].append(event);request['step']+=1;request['candidates']=candidates
        request['family']=candidates[0] if len(candidates)==1 else None
        request['current_word']=self.registers[address]
        if reset:
            if len(accesses)==2:request.update(reset_fields)
            else:request['poll_ticks'].append(tick)
        request['state']=self._post_shutdown_request_state(request,candidates)
        if beginning:record['requests'].append(request)
        record['step']+=1;record['current_word']=request['current_word'];record['state']=request['state']
        self.post_shutdown_control=record
        self.board.record('Post-shutdown desired control retained',**event,state=record['state'],
            ordinal=request['ordinal'],family=request['family'],
            limits='Exact desired fields or one declared zero/bit28 stop; historical arm/histogram/shutdown unchanged, no data/IRQ/new lifetime')
        return True

    def _post_shutdown_reset_due(self):
        record=self.post_shutdown_control
        if record is None:return None
        request=record['requests'][-1]
        return request.get('due_tick') if request.get('state')=='waiting-ack' else None

    def _tick_post_shutdown_reset(self,board):
        record=self.post_shutdown_control
        if record is None:return False
        request=record['requests'][-1]
        active=('waiting-ack','acknowledged-awaiting-poll')
        if record.get('state') not in active and request.get('state') not in active:return False
        self._validate_rearm();self._validate_histogram()
        if request['state']=='acknowledged-awaiting-poll' or board.virtual_ticks<request['due_tick']:return True
        self.registers[self.GLOBAL]=0x10000000
        request.update(acknowledged_tick=board.virtual_ticks,current_word=0x10000000,
                       acknowledged_poll_count=len(request['poll_ticks']),state='acknowledged-awaiting-poll')
        record.update(current_word=0x10000000,state=request['state'])
        board.record('Post-shutdown global stop acknowledged (unmeasured)',
            cpu=record['cpu'],requested_tick=request['requested_tick'],due_tick=request['due_tick'],
            acknowledged_tick=board.virtual_ticks,
            acknowledged_poll_count=request['acknowledged_poll_count'],
            limits='Existing STOP_PROFILE bit28 only; historical requests/status/data/IRQ unchanged; no new lifetime')
        return True

    def _validate_fresh_shutdown(self):
        stop=self.fresh_shutdown
        if stop is None:return
        if (not isinstance(stop,dict) or type(stop.get('step')) is not int
                or not 1<=stop['step']<=len(self.FRESH_SHUTDOWN_SEQUENCE)
                or stop.get('state')!=('completed' if stop['step']==8 else 'in-progress')
                or type(stop.get('cpu')) is not int or stop['cpu'] not in (0,1)
                or stop.get('rearm_sha256')!=hp.digest(self.rearm)
                or stop.get('histogram_sha256')!=hp.digest(self.histogram)
                or stop.get('original_history_sha256')!=self._rearm_history_hash()
                or self.busy is not False or self.dma_busy is not False
                or any(v is not None for v in (self.pending_data,self.pending_ack,self.pending_stop))
                or not isinstance(stop.get('accesses'),list) or len(stop['accesses'])!=stop['step']):
            raise UnsupportedDevice('Fresh producer shutdown ownership/history/quiescence changed')
        words={hex(self.BASE+off):v for off,v in self.FRESH_SHUTDOWN_PRIOR.items()}
        if stop.get('prior_words')!=words:
            raise UnsupportedDevice('Fresh producer shutdown initial projection changed')
        for step,event in enumerate(stop['accesses']):
            operation,offset,value=self.FRESH_SHUTDOWN_SEQUENCE[step]
            if (not isinstance(event,dict) or set(event)!={'operation','address','word','cpu','step'}
                    or event['operation']!=operation or event['address']!=hex(self.BASE+offset)
                    or type(event['word']) is not int or event['word']!=value
                    or type(event['step']) is not int or event['step']!=step
                    or type(event['cpu']) is not int or event['cpu']!=stop['cpu']):
                raise UnsupportedDevice('Fresh producer shutdown ordered access changed')
            if operation=='write':words[event['address']]=value
            elif words[event['address']]!=value:
                raise UnsupportedDevice('Fresh producer shutdown read differs from its projection')
        if words!=stop.get('current_words') or any(
                self.registers.get(int(a,16))!=v or type(self.registers.get(int(a,16))) is not int
                for a,v in words.items()):
            raise UnsupportedDevice('Fresh producer shutdown current projection changed')

    def _fresh_shutdown_project(self, expected):
        """Overlay the abort's words without changing historical arm ledgers."""
        self._validate_fresh_shutdown()
        if self.fresh_shutdown is not None:
            for address,value in self.fresh_shutdown['current_words'].items():
                address=int(address,16)
                if address in expected:expected[address]=value
        return expected

    def _fresh_shutdown_route_matches(self, route):
        self._validate_fresh_shutdown()
        interrupts=getattr(self.board,'interrupts',None)
        targets=[] if interrupts is None else [cpu for cpu,bank in enumerate(interrupts.banks)
            if bank.enabled&(1<<0x2e)]
        return (targets==[route] or targets==[] and self.fresh_shutdown is not None
                and self.fresh_shutdown['state']=='completed')

    def _fresh_shutdown_access(self, operation, core, address, size, value=None):
        stop=self.fresh_shutdown
        if stop is not None and stop['state']=='completed':
            if self._preview_configuration_access(operation,core,address,size,value):return True
        if stop is None:
            h=self.histogram
            if (operation!='read' or address!=self.BASE+4 or h is None or h.get('state')!='released'
                    or self.stop_profile!='stock-idle-stop-handshakes-unmeasured'):return False
            self._validate_rearm()
            self._validate_histogram()
            if (self.rearm['state']!='enabled-awaiting-fresh-input'
                    or self.busy is not False or self.dma_busy is not False
                    or any(v is not None for v in (self.pending_data,self.pending_ack,self.pending_stop))
                    or len(h['samples'])!=hp.MAX_SAMPLES or len(h['deliveries'])!=hp.MAX_SAMPLES
                    or any(d['state']!='acknowledged' for d in h['deliveries'])
                    or any(self.registers[self.BASE+o]!=v for o,v in self.FRESH_SHUTDOWN_PRIOR.items())
                    or self.registers[self.BASE+0x984]!=0 or self.registers[self.BASE+0x990]!=0
                    or self.registers[self.BASE+0x9f8]!=0):
                raise UnsupportedDevice('Fresh producer shutdown requires released histogram and idle enabled channel0')
            prior={hex(self.BASE+o):v for o,v in self.FRESH_SHUTDOWN_PRIOR.items()}
            stop=dict(state='in-progress',step=0,cpu=core,accesses=[],prior_words=prior,
                current_words=dict(prior),rearm_sha256=hp.digest(self.rearm),
                histogram_sha256=hp.digest(h),original_history_sha256=self._rearm_history_hash())
        else:
            self._validate_rearm()
            self._validate_histogram()
            if stop['state']=='completed':
                if self._preview_idle_stop_access(operation,core,address,size,value):return True
                return self._post_shutdown_control_access(operation,core,address,size,value)
        if size!=4 or type(core) is not int or core not in (0,1) or core!=stop['cpu']:
            raise UnsupportedDevice('Fresh producer shutdown requires same-owner WORD accesses')
        expected=self.FRESH_SHUTDOWN_SEQUENCE[stop['step']]
        actual=self.registers.get(address) if operation=='read' else value
        if (type(actual) is not int or (operation,address-self.BASE,actual)!=expected):
            raise UnsupportedDevice('Fresh producer shutdown access differs from the recovered transaction')
        event=dict(operation=operation,address=hex(address),word=actual,cpu=core,step=stop['step'])
        if operation=='write':
            old=self.registers[address]
            self.registers[address]=actual
            stop['current_words'][hex(address)]=actual
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(actual),old=hex(old)))
        stop['accesses'].append(event);stop['step']+=1
        stop['state']='completed' if stop['step']==8 else 'in-progress'
        self.fresh_shutdown=stop
        self.board.record('Fresh producer idle shutdown retained',**event,state=stop['state'],
            limits='Retained idle controls only; original arm/data/histogram histories unchanged, no response/data/IRQ')
        return True
