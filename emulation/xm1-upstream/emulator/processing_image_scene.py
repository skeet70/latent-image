"""Pure declared U16 monochrome processing into paired YYCbCr outputs.

This is a simulation law, not a recovered Fuji ISP: the Input offset fields
are interpreted as row/column offsets, U16 values are clamped to twelve bits,
then floor-nearest sampled and shifted four bits into luma. Chroma is128.
The B8 field is retained without treating it as a color-format enum.
"""
import hashlib
import struct

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from .virtual_source import ram_span

PROFILE = 'declared-postprocessing-u16-mono-to-paired-yycbcr-nearest-unmeasured'
LIMITS = ('Declared Input offsets as row/column; clamp U16 to12bits then shift4; '
    'floor-nearest sampling and neutral128 YYCbCr; byte pitch=2*C4. '
    'Physical crop, CFA, ISP/filter/gamma/color arithmetic and frame cadence are unmeasured.')


def _int(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise UnsupportedDevice('Processing image scene: invalid ' + name)
    return value


def geometry(desired, allocation, *, channel):
    if (type(desired) is not dict or set(desired) != {4,8,12,20,176,180,184,192,196,244}
            or type(allocation) is not dict or set(allocation) != {'address','bytes'}):
        raise UnsupportedDevice('Processing image scene requires the whole desired tuple and owned allocation')
    _int(channel,0,1,'channel')
    width, height = (desired[0xb0]&0x7ff)+1, ((desired[0xb0]>>16)&0x7ff)+1
    pitch, destination = 2*_int(desired[0xc4],1,0x1ffc,'halfword pitch'), desired[0xc0]
    if width&1 or width*height>4*1024*1024 or pitch<width*2:
        raise UnsupportedDevice('Processing image scene requires bounded paired luma dimensions/pitch')
    begin,end=ram_span(destination,(height-1)*pitch+width*2)
    low,high=ram_span(_int(allocation['address'],0,0x5fffffff,'allocation address'),
        _int(allocation['bytes'],1,64*1024*1024,'allocation capacity'))
    if not low<=begin<end<=high:
        raise UnsupportedDevice('Processing image output exceeds its explicitly owned allocation')
    return dict(channel=channel,width=width,height=height,pitch=pitch,row_bytes=width*2,
        destination=destination,physical_begin=begin,physical_end=end,
        allocation=dict(allocation),desired=dict(desired),packing='declared-YYCbCr-neutral128')


def encode_rows(source, *, source_width, source_height, source_pitch,
                sample_width, sample_height, row_offset, column_offset,
                width, height, profile=PROFILE, numpy=True):
    if profile != PROFILE or type(numpy) is not bool:
        raise UnsupportedDevice('Processing image scene requires its explicit monochrome profile')
    for value, low, high, name in ((source_width,1,8192,'source width'),
            (source_height,1,8192,'source height'),(source_pitch,2*source_width,32768,'source pitch'),
            (sample_width,1,source_width,'sample width'),(sample_height,1,source_height,'sample height'),
            (row_offset,0,source_height-1,'declared row offset'),
            (column_offset,0,source_width-1,'declared column offset'),
            (width,2,2048,'output width'),(height,1,2048,'output height')):
        _int(value,low,high,name)
    if (type(source) is not bytes or len(source)!=source_pitch*source_height
            or len(source)>64*1024*1024 or source_pitch&1 or width&1
            or row_offset+sample_height>source_height or column_offset+sample_width>source_width):
        raise UnsupportedDevice('Processing image scene leaves its complete immutable U16 source')
    if numpy and _numpy is not None:
        raster=_numpy.frombuffer(source,dtype='<u2').reshape(source_height,source_pitch//2)
        xx=column_offset+_numpy.arange(width)*sample_width//width
        rows=[]
        for y in range(height):
            yy=row_offset+y*sample_height//height
            luma=(_numpy.minimum(raster[yy,xx],4095)>>4).astype(_numpy.uint8)
            row=_numpy.full((width//2,4),128,dtype=_numpy.uint8)
            row[:,:2]=luma.reshape(width//2,2); rows.append(row.tobytes())
        return tuple(rows)
    rows=[]
    for y in range(height):
        start=(row_offset+y*sample_height//height)*source_pitch
        row=bytearray()
        for x in range(0,width,2):
            for q in (x,x+1):
                position=start+2*(column_offset+q*sample_width//width)
                row.append(min(struct.unpack_from('<H',source,position)[0],4095)>>4)
            row.extend((128,128))
        rows.append(bytes(row))
    return tuple(rows)


def prepare(source, source_geometry, outputs, *, profile=PROFILE, numpy=True):
    if profile!=PROFILE or type(source_geometry) is not dict or set(source_geometry)!={
            'address','width','height','pitch','sample_width','sample_height','row_offset','column_offset'}:
        raise UnsupportedDevice('Processing image scene requires the explicit whole U16 source geometry')
    if type(outputs) is not tuple or len(outputs)!=2:
        raise UnsupportedDevice('Processing image scene requires exactly two owned outputs')
    source_span=ram_span(source_geometry['address'],len(source))
    plans=[]
    for channel,output in enumerate(outputs):
        if type(output) is not dict or set(output)!={'desired','allocation'}:
            raise UnsupportedDevice('Processing image scene requires desired/output allocation pairs')
        g=geometry(output['desired'],output['allocation'],channel=channel)
        if source_span[0]<g['physical_end'] and g['physical_begin']<source_span[1]:
            raise UnsupportedDevice('Processing image output aliases its immutable RAW source')
        rows=encode_rows(source,source_width=source_geometry['width'],
            source_height=source_geometry['height'],source_pitch=source_geometry['pitch'],
            sample_width=source_geometry['sample_width'],sample_height=source_geometry['sample_height'],
            row_offset=source_geometry['row_offset'],column_offset=source_geometry['column_offset'],
            width=g['width'],height=g['height'],profile=profile,numpy=numpy)
        plans.append(dict(geometry=g,rows=tuple((g['destination']+y*g['pitch'],row) for y,row in enumerate(rows)),
            payload_sha256=hashlib.sha256(b''.join(rows)).hexdigest()))
    if plans[0]['geometry']['physical_begin']<plans[1]['geometry']['physical_end'] and \
            plans[1]['geometry']['physical_begin']<plans[0]['geometry']['physical_end']:
        raise UnsupportedDevice('Processing image outputs overlap')
    capsule=dict(profile=profile,source_geometry=dict(source_geometry),
        source_sha256=hashlib.sha256(source).hexdigest(),
        outputs=[dict(geometry=p['geometry'],payload_sha256=p['payload_sha256']) for p in plans],limits=LIMITS)
    return dict(**capsule,input_sha256=digest(capsule),plans=tuple(plans))
