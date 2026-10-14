"""A bounded, explicitly declared post-ISP channel1 experiment.

Mixed into ProducerControl so all guest accesses still use its single MMIO
owner. Only ordinary dictionary/list/integer state is retained. Sampling,
1024-byte length, scene law, DMA latency, latching and F4 clear responses are
experiment conventions. No guest callback, RAM flag or instruction selects
a result; the unchanged shared ISR invokes both of its stock clients.
"""
import hashlib
from copy import deepcopy

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from . import strict_audit
from .virtual_source import source_epochs, epoch_key, ram_span


HISTOGRAM_PROFILES = hp.PROFILES


class HistogramMixin:
    HISTOGRAM_WORDS = frozenset(hp.BASE+off for off in
        (0x700,0x704,0x98c,0x9f4,0x9a0,0x9f8,0x9c4,0x984,0x990))
    HISTOGRAM_PROJECTION = HISTOGRAM_WORDS | frozenset((hp.BASE+4,hp.BASE+8,
        hp.BASE+0x88c,hp.BASE+0x8f4))
    HISTOGRAM_ACCESS_LIMIT = 4096

    def _histogram_source_epochs(self):
        """Only the card fixture extends channel1 to the actual fifth edge.

        Ordinary ImageWork source projection remains2..4. No edge, callback,
        scene buffer or guest flag is created by this histogram-only view.
        """
        epochs=source_epochs(self.board)
        inputs=[d for d in self.board.devices if type(d).__name__=='InputProcessingControl']
        if len(inputs)!=1:raise UnsupportedDevice('Histogram requires one input-processing controller')
        if getattr(inputs[0],'event_profile',None) not in hp.CARD_HISTOGRAM_SOURCE_PROFILES:return epochs
        for event in inputs[0].event_history:
            if (event.get('state') not in ('asserted','acknowledged')
                    or event.get('profile')!=inputs[0].event_profile
                    or event.get('generation')!=3 or event.get('source')!=0 or event.get('ordinal')!=5):continue
            epoch={name:event.get(name) for name in ('profile','generation','ordinal','attempt',
                'cpu','source','physical_irq','asserted_tick')}|{'frame':4}
            hp.source_epoch(epoch)
            epochs.append(epoch)
        return epochs

    def _histogram_timerless_idle(self):
        """A complete delivery awaits only a new eligible external epoch.

        Time cannot change the latches in this case. Consuming MMIO and the
        next eligible epoch still reconstruct the complete retained state.
        """
        h=self.histogram
        if (self.histogram_profile not in HISTOGRAM_PROFILES or type(h) is not dict
                or h.get('profile')!=self.histogram_profile or h.get('state') not in ('enabled','released')
                or type(h.get('samples')) is not list or type(h.get('deliveries')) is not list
                or not 1<=len(h['samples'])==len(h['deliveries'])<=hp.MAX_SAMPLES
                or type(h.get('destination_updates')) is not list
                or any(type(d) is not dict or d.get('state')!='acknowledged'
                    or type(d.get('ack_channel')) is not int or d['ack_channel']!=2
                    or type(d.get('ack_step')) is not int or d['ack_step']!=0
                    or d.get('ack_due_tick') is not None or d.get('ack_requested_tick') is not None
                    for d in h['deliveries'])):return False
        if h['state']=='released':
            release=h.get('release')
            if (len(h['samples'])!=hp.MAX_SAMPLES or type(release) is not dict
                    or release.get('kind')!='release' or release.get('state')!='completed'
                    or type(release.get('step')) is not int
                    or release['step']!=len(hp.RELEASE_SEQUENCE)
                    or type(release.get('accesses')) is not list
                    or len(release['accesses'])!=len(hp.RELEASE_SEQUENCE)):return False
            # No timer or result remains. The kernel can temporarily mask this
            # exhausted route while unregistering another source in its bank.
            # Consuming MMIO still validates the complete state/route/history.
            owner=release['accesses'][0].get('cpu') if type(release['accesses'][0]) is dict else None
            return (type(owner) is int and owner in (0,1) and all(
                type(event) is dict and type(event.get('step')) is int and event['step']==index
                and type(event.get('cpu')) is int and event['cpu']==owner
                and type(event.get('word')) is int
                and (event.get('operation'),event.get('address'),event['word'])==(op,hex(address),word)
                for index,(event,(op,address,word)) in enumerate(zip(release['accesses'],hp.RELEASE_SEQUENCE))))
        if len(h['samples'])==hp.MAX_SAMPLES:return True
        epochs=self._histogram_source_epochs()
        return (not epochs or epoch_key(epochs[-1])==tuple(h['samples'][-1]['identity'])
            or len(h['destination_updates'])<=h['deliveries'][-1]['destination_update_count'])

    def _init_histogram(self,profile,data_ticks):
        if profile is not None and profile not in HISTOGRAM_PROFILES:
            raise ValueError('Unknown declared histogram profile')
        if type(data_ticks) is not int or not 1<=data_ticks<1<<64:
            raise ValueError('Histogram data delay must be a positive integer')
        self.histogram_profile,self.histogram_data_ticks=profile,data_ticks
        self.histogram=None
        self.dma_busy=False
        if profile is not None:
            # Initial idle words are an unmeasured fixture; first arm may only
            # begin after the independently validated main fresh request.
            self.registers.update({a:0 for a in self.HISTOGRAM_WORDS
                if a not in self.registers})

    def _histogram_seal(self):
        h=self.histogram
        h['state_sha256']=hp.digest({k:v for k,v in h.items() if k!='state_sha256'})
        self.dma_busy=any(e['state']!='acknowledged' for e in h['deliveries'])

    def _validate_histogram(self):
        h=self.histogram
        if h is None:return
        if (self.histogram_profile!=hp.PROFILE or not isinstance(h,dict)
                or h.get('profile')!=hp.PROFILE or h.get('data_ticks')!=self.histogram_data_ticks
                or h.get('ack_ticks')!=self.ack_ticks
                or h.get('state') not in ('setup','enabled','releasing','released')
                or hp.digest({k:v for k,v in h.items() if k!='state_sha256'})!=h.get('state_sha256')):
            raise UnsupportedDevice('Declared histogram state/profile/fingerprint changed')
        hp.validate_ledger(h['setup'])
        if h['release'] is not None:hp.validate_ledger(h['release'])
        initial={hex(a):0 for a in self.HISTOGRAM_PROJECTION}
        initial[hex(self.BASE+4)]=0x33173317
        initial[hex(self.BASE+0x8f4)]=1
        if h['setup']['prior_words']!=initial:
            raise UnsupportedDevice('Histogram initial projection differs from its independently armed fixture')
        projection=dict(initial);last_tick=h['setup']['started_tick']
        accesses=h.get('accesses')
        if not isinstance(accesses,list) or len(accesses)>self.HISTOGRAM_ACCESS_LIMIT:
            raise UnsupportedDevice('Histogram ordered access history is malformed/exhausted')
        for index,event in enumerate(accesses):
            if (not isinstance(event,dict) or set(event)!=set(('operation','cpu','address','word','tick','step'))
                    or type(event['step']) is not int or event['step']!=index
                    or event['address'] not in initial or type(event['word']) is not int
                    or not 0<=event['word']<=0xffffffff or type(event['tick']) is not int
                    or not last_tick<=event['tick']<1<<64):
                raise UnsupportedDevice('Histogram ordered access type/projection/tick changed')
            last_tick=event['tick'];operation=event['operation']
            if operation in ('write','read'):
                if type(event['cpu']) is not int or event['cpu'] not in (0,1):
                    raise UnsupportedDevice('Histogram access CPU changed')
                if operation=='write':projection[event['address']]=event['word']
                elif projection[event['address']]!=event['word']:
                    raise UnsupportedDevice('Histogram read differs from its retained word history')
            elif (operation=='response' and event['cpu'] is None and event['word']==0
                    and event['address'] in (hex(self.BASE+0x8f4),hex(self.BASE+0x9f4))
                    and projection[event['address']]==1):
                projection[event['address']]=0
            else:raise UnsupportedDevice('Histogram access has an unsupported effect')
        if projection!=h['current_words']:
            raise UnsupportedDevice('Histogram current projection is not derived from ordered history')
        for event,saved in zip(accesses,h['setup']['accesses']):
            if event!=saved:raise UnsupportedDevice('Histogram setup history changed outside its ordered ledger')
        if (h['setup']['state']=='partial')!=(h['state']=='setup'):
            raise UnsupportedDevice('Histogram setup/controller states disagree')
        updates=[e for e in accesses[1:] if e['operation']=='write' and e['address']==hex(self.BASE+0x700)]
        if updates!=h['destination_updates']:
            raise UnsupportedDevice('Histogram desired destination history changed')
        expected=self._fresh_shutdown_project({int(a,16):v for a,v in h['current_words'].items()})
        for a,v in h['current_words'].items():
            try:address=int(a,16) if isinstance(a,str) else None
            except ValueError:address=None
            if (address not in self.HISTOGRAM_PROJECTION or a!=hex(address)
                    or type(v) is not int or not 0<=v<=0xffffffff
                    or type(self.registers.get(address)) is not int
                    or self.registers[address]!=expected[address]):
                raise UnsupportedDevice('Declared histogram projected registers changed outside history')
        if set(h['current_words'])!={hex(a) for a in self.HISTOGRAM_PROJECTION}:
            raise UnsupportedDevice('Declared histogram projection domain changed')
        if (not isinstance(h['samples'],list) or not isinstance(h['deliveries'],list)
                or len(h['samples'])!=len(h['deliveries']) or len(h['samples'])>hp.MAX_SAMPLES):
            raise UnsupportedDevice('Declared histogram sample/delivery count changed')
        previous=[]
        for index,(request,delivery) in enumerate(zip(h['samples'],h['deliveries'])):
            hp.validate_sample(request)
            if request['ordinal']!=index+1 or request['identity'] in previous:
                raise UnsupportedDevice('Declared histogram source identity/ordinal changed')
            previous.append(request['identity'])
            if (not isinstance(delivery,dict)
                    or delivery.get('state') not in ('pending','data-before-irq','acknowledged')
                    or type(delivery.get('destination_update_count')) is not int
                    or not 0<=delivery['destination_update_count']<=len(h['destination_updates'])
                    or type(delivery.get('ack_channel')) is not int
                    or not 0<=delivery['ack_channel']<=2
                    or type(delivery.get('ack_step')) is not int or not 0<=delivery['ack_step']<=2
                    or delivery.get('state')=='acknowledged' and delivery['ack_channel']!=2):
                raise UnsupportedDevice('Declared histogram delivery/ACK state changed')
            if delivery['state']=='pending':
                if delivery['ack_channel']!=0 or delivery['ack_step']!=0:
                    raise UnsupportedDevice('Histogram ACK precedes its declared data')
            else:
                completed=delivery.get('completed_tick')
                if type(completed) is not int or not request['due_tick']<=completed<=self.board.virtual_ticks:
                    raise UnsupportedDevice('Histogram data completion precedes its declared input')
            if delivery['ack_step']==2:
                requested=delivery.get('ack_requested_tick')
                if (delivery['state']!='data-before-irq' or delivery['ack_channel']>=2
                        or type(requested) is not int or not delivery['completed_tick']<=requested<=self.board.virtual_ticks
                        or type(delivery.get('ack_due_tick')) is not int
                        or delivery['ack_due_tick']!=requested+self.ack_ticks):
                    raise UnsupportedDevice('Histogram ACK request/due interval changed')
            elif delivery.get('ack_due_tick') is not None or delivery.get('ack_requested_tick') is not None:
                raise UnsupportedDevice('Histogram ACK timer is not tied to a pending request')
            events=delivery.get('ack_events')
            if not isinstance(events,list) or len(events)!=delivery['ack_channel']:
                raise UnsupportedDevice('Histogram response/channel count changed')
            for channel,event in enumerate(events):
                if (not isinstance(event,dict) or set(event)!=set(('channel','completed_tick','response','access_step'))
                        or type(event['channel']) is not int or event['channel']!=channel
                        or type(event['access_step']) is not int or not 0<=event['access_step']<len(accesses)
                        or event['response']!=0 or type(event['response']) is not int):
                    raise UnsupportedDevice('Histogram response history changed')
                access=accesses[event['access_step']]
                if (access['operation']!='response' or access['address']!=hex(self.BASE+0x8f4+channel*0x100)
                        or access['tick']!=event['completed_tick']):
                    raise UnsupportedDevice('Histogram response is not bound to its actual retained access')
        if self.dma_busy is not any(e['state']!='acknowledged' for e in h['deliveries']):
            raise UnsupportedDevice('Declared histogram checkpoint busy marker changed')
        if h['setup']['state']=='completed' and not self._fresh_shutdown_route_matches(h['route']):
            raise UnsupportedDevice('Declared histogram shared IRQ2E route changed')

    def _histogram_expected(self,expected):
        """Bind current words while preserving the main arm's old ledger."""
        self._validate_histogram()
        if self.histogram is not None:
            for a,v in self.histogram['current_words'].items():
                address=int(a,16)
                if address in expected:expected[address]=v
        return expected

    def _histogram_event(self,operation,core,address,value):
        h=self.histogram
        if len(h['accesses'])>=self.HISTOGRAM_ACCESS_LIMIT:
            raise UnsupportedDevice('Declared histogram access experiment is exhausted')
        event=dict(operation=operation,cpu=core,address=hex(address),word=value,
            tick=self.board.virtual_ticks,step=len(h['accesses']))
        h['accesses'].append(event)
        if operation in ('write','response'):
            self.registers[address]=value
            h['current_words'][hex(address)]=value
        if operation=='write':
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(value)))
        return event

    def _histogram_access(self,operation,core,address,size,value=None):
        if self.histogram_profile is None:return False
        h=self.histogram
        is_histogram=address in self.HISTOGRAM_WORDS
        shared=address in (self.BASE+4,self.BASE+8)
        ack=address in self.ACK_REGISTERS
        if not (is_histogram or shared or ack):return False
        if size!=4 or type(core) is not int or core not in (0,1):
            raise UnsupportedDevice('Declared histogram requires typed CPU identity and WORD access')
        if operation=='write':hp.uint(value)
        if h is None:
            # The earlier independent recorded channel0 job already uses
            # paired ACK0/ACK1. Keep its complete original handling intact.
            if address!=self.BASE+0x700:return False
            if (operation!='write' or address!=self.BASE+0x700 or value!=hp.DESTINATION_A):
                raise UnsupportedDevice('Declared histogram requires the first selected700 destination store')
            self._validate_rearm()
            r=self.rearm
            if (r['state']!='enabled-awaiting-fresh-input'
                    or not isinstance(r.get('geometry'),dict) or r['geometry']['state']!='completed'
                    or not r['rotations'] or r['rotations'][-1]['state']!='completed'
                    or not isinstance(r.get('image_global_configuration'),dict)
                    or r['image_global_configuration']['state']!='completed'
                    or self.busy is not False or self.pending_data is not None or self.pending_ack is not None):
                raise UnsupportedDevice('Declared histogram requires a complete independently armed main request')
            prior={a:self.registers[a] for a in self.HISTOGRAM_PROJECTION}
            h=dict(profile=hp.PROFILE,data_ticks=self.histogram_data_ticks,ack_ticks=self.ack_ticks,
                state='setup',setup=hp.new_ledger('setup',prior,self.board.virtual_ticks),release=None,
                current_words={hex(a):v for a,v in prior.items()},accesses=[],samples=[],deliveries=[],
                destination_updates=[],route=None,
                assumptions='Declared post-ISP source/data law,1024B,latching,latency and paired F4 response; no fresh channel0 data')
            self.histogram=h
            self._histogram_seal()
        elif not (is_histogram or shared or ack):return False
        self._validate_rearm_access()
        if strict_audit.ENABLED:
            self._validate_histogram()
        if operation=='read':value=self.registers[address]
        if h['state']=='setup':
            updated=hp.advance_ledger(h['setup'],operation,address,value,core,self.board.virtual_ticks)
            self._histogram_event(operation,core,address,value)
            h['setup']=updated
            if updated['state']=='completed':
                h['state']='enabled';h['route']=self._dma_route()
            self._histogram_seal()
            self.board.record('Declared histogram setup retained',**h['accesses'][-1],state=h['state'])
            return True
        if h['state']=='released':
            raise UnsupportedDevice('Declared histogram released; repeat allocation/arm is unresolved')
        if ack:
            self._histogram_ack_access(operation,core,address,value)
            self._histogram_seal();return True
        if h['state']=='enabled' and operation=='write' and address==self.BASE+0x700:
            if value not in (hp.DESTINATION_A,hp.DESTINATION_B):
                raise UnsupportedDevice('Declared histogram next destination is outside the two captured banks')
            event=self._histogram_event(operation,core,address,value)
            h['destination_updates'].append(dict(event))
            self._histogram_seal()
            self.board.record('Declared histogram next desired destination retained',**event,
                effects='Pending payload retains its previously latched target')
            return True
        if (h['state']=='enabled' and operation=='read' and address==self.BASE+4
                and h['deliveries'] and all(e['state']=='acknowledged' for e in h['deliveries'])):
            h['release']=hp.new_ledger('release',
                {int(a,16):v for a,v in h['current_words'].items()},self.board.virtual_ticks)
            h['state']='releasing'
        if h['state']=='releasing':
            updated=hp.advance_ledger(h['release'],operation,address,value,core,self.board.virtual_ticks)
            self._histogram_event(operation,core,address,value);h['release']=updated
            if updated['state']=='completed':h['state']='released'
            self._histogram_seal()
            self.board.record('Declared histogram guest release retained',**h['accesses'][-1],state=h['state'])
            return True
        if operation=='read' and shared:
            # Retained values only, not release/status completion or a result.
            self._histogram_event(operation,core,address,value);self._histogram_seal();return True
        raise UnsupportedDevice('Unknown active histogram configuration/control/status transaction')

    def _histogram_ack_access(self,operation,core,address,value):
        h=self.histogram
        if (h['deliveries'] and h['deliveries'][-1]['state']=='pending' and operation=='read'
                and len(h['deliveries'])>1 and h['deliveries'][-2]['state']=='acknowledged'
                and value==0):
            # The next source may be sampled at the tick that clears the
            # preceding pair. Its actual ISR must still observe final F4=0
            # before running either callback. This read is not a new ACK.
            self._histogram_event(operation,core,address,value)
            return
        if not h['deliveries'] or h['deliveries'][-1]['state']=='pending':
            raise UnsupportedDevice('Histogram paired ACK requires declared data before IRQ')
        delivery=h['deliveries'][-1]
        channel=(address-self.BASE-0x88c)//0x100
        offset=address-(self.BASE+0x88c+channel*0x100)
        if operation=='read':
            if (channel not in (0,1) or channel>delivery['ack_channel']
                    or channel==delivery['ack_channel'] and delivery['ack_step']!=2):
                raise UnsupportedDevice('Histogram status poll precedes its paired request')
        else:
            expected_address=self.BASE+0x88c+delivery['ack_channel']*0x100
            if delivery['ack_step']==1:expected_address+=0x68
            expected_value=1 if delivery['ack_step']==1 else 0
            if (delivery['state']!='data-before-irq' or delivery['ack_channel']>=2
                    or delivery['ack_step']==2 or address!=expected_address or value!=expected_value):
                raise UnsupportedDevice('Histogram paired ACK order/value is unresolved')
            delivery['ack_step']+=1
            if delivery['ack_step']==2:
                delivery['ack_requested_tick']=self.board.virtual_ticks
                delivery['ack_due_tick']=self.board.virtual_ticks+self.ack_ticks
        self._histogram_event(operation,core,address,value)

    def _tick_histogram(self,now):
        if self.histogram_profile is None or self.histogram is None:return
        # A pending DMA/ACK cannot change before its declared deadline. Stock
        # 42E60 temporarily masks a whole IRQ bank while unregistering another
        # source; that mask is not a changed destination or completion route.
        # Consuming MMIO and the actual due tick retain the full validation.
        # Keep malformed pending records on the strict path below.
        h=self.histogram
        if type(h) is dict and type(h.get('deliveries')) is list and h['deliveries']:
            delivery=h['deliveries'][-1]
            if type(delivery) is dict:
                due=None
                if (delivery.get('state')=='pending' and type(h.get('samples')) is list
                        and h['samples'] and type(h['samples'][-1]) is dict):
                    due=h['samples'][-1].get('due_tick')
                elif delivery.get('state')=='data-before-irq' and delivery.get('ack_step')==2:
                    due=delivery.get('ack_due_tick')
                if type(due) is int and now<due:return
        self._validate_histogram()
        h=self.histogram
        if h['state'] not in ('enabled',):return
        if h['deliveries']:
            delivery=h['deliveries'][-1];request=h['samples'][-1]
            if delivery['state']=='pending' and now>=request['due_tick']:
                hp.validate_sample(request)
                if (self._dma_route()!=request['route']
                        or request['identity'] not in [list(epoch_key(e)) for e in self._histogram_source_epochs()]):
                    raise UnsupportedDevice('Histogram sampled source identity/IRQ route changed before data')
                address=int(request['latched_destination'],16)
                ram_span(address,hp.PAYLOAD_BYTES)
                data=bytes.fromhex(request['payload_hex'])
                if hashlib.sha256(data).hexdigest()!=request['payload_sha256']:
                    raise UnsupportedDevice('Histogram latched payload changed before bus write')
                self.board.write_memory(address,data)
                delivery.update(state='data-before-irq',completed_tick=now)
                self._histogram_seal()
                self.board.record('Declared histogram1024B delivered before shared IRQ2E',
                    request=request,delivery=deepcopy(delivery),channel0_data='No fresh channel0 bytes supplied')
                self.board.queue_irq(request['route'],0x2e)
            if delivery['state']=='data-before-irq' and delivery['ack_step']==2 and now>=delivery['ack_due_tick']:
                channel=delivery['ack_channel'];address=self.BASE+0x8f4+channel*0x100
                if type(self.registers[address]) is not int or self.registers[address]!=1:
                    raise UnsupportedDevice('Histogram pending F4 request changed before response')
                event=self._histogram_event('response',None,address,0)
                delivery['ack_events'].append(dict(channel=channel,completed_tick=now,response=0,
                    access_step=event['step']))
                delivery.update(ack_channel=channel+1,ack_step=0,ack_due_tick=None,ack_requested_tick=None)
                if delivery['ack_channel']==2:delivery.update(state='acknowledged',acknowledged_tick=now)
                self._histogram_seal()
                self.board.record('Declared histogram paired F4 response',channel=channel,tick=now,
                    inference='Unmeasured clear latency; controller pending/EOI and both real callbacks remain untouched')
        if (len(h['samples'])>=hp.MAX_SAMPLES
                or h['deliveries'] and h['deliveries'][-1]['state']!='acknowledged'):
            return
        epochs=self._histogram_source_epochs()
        if not epochs:return
        epoch=epochs[-1]
        if h['samples']:
            last=h['samples'][-1];delivery=h['deliveries'][-1]
            if epoch_key(epoch)==tuple(last['identity']):return
            if len(h['destination_updates'])<=delivery['destination_update_count']:return
        hp.source_epoch(epoch)
        destination=self.registers[self.BASE+0x700]
        if (self.registers[self.BASE+8]!=0x10001 or self.registers[self.BASE+0x984]!=1
                or self.registers[self.BASE+0x9f8]!=0x10000000):
            raise UnsupportedDevice('Histogram sampled descriptor is not independently enabled')
        crop=(self.registers[self.BASE+0x604],self.registers[self.BASE+0x608])
        if crop!=(0xc098f,0x200328):
            raise UnsupportedDevice('Histogram scene scope does not cover changed crop')
        ram_span(destination,hp.PAYLOAD_BYTES)
        from x70_native._native import virtual_scene_histogram
        payload=virtual_scene_histogram(2496,853,12,31,2448,809,epoch['frame'])
        provenance=dict(generator='Rust virtual_scene_histogram',source_width=2496,source_height=853,
            crop=[12,31,2448,809],law='BT601 limited Ybyte,local x step2,256 LE U32; same declared analytic scene',
            span_basis='Explicit1024B;704 count units unmeasured; no sensor/ISP fidelity claim')
        request=hp.sample_request(epoch=epoch,destination=destination,count_word=self.registers[self.BASE+0x704],
            crop_words=crop,payload=payload,provenance=provenance,now=now,
            data_ticks=self.histogram_data_ticks,route=self._dma_route(),previous=h['samples'])
        h['samples'].append(request)
        h['deliveries'].append(dict(state='pending',destination_update_count=len(h['destination_updates']),
            ack_channel=0,ack_step=0,ack_due_tick=None,ack_requested_tick=None,ack_events=[]))
        self._histogram_seal()
        self.board.record('Declared histogram tuple/source sampled',request=request,
            latching='Unmeasured current descriptor/source pairing; later700 stores cannot retarget pending bytes')

    def _histogram_next_tick(self):
        h=self.histogram
        if h is None or not h['deliveries']:return None
        delivery=h['deliveries'][-1]
        if delivery['state']=='pending':return h['samples'][-1]['due_tick']
        return delivery.get('ack_due_tick')
