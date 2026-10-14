"""Quick-path pure host arithmetic; guest clocks, bytes and devices stay intact.

The scanline implementation follows emulator.graphics_devices._ui_coverage.
It vectorizes the same float64 expressions, ordering, winding and AA grid;
unsupported/oversized cases use the original. Install only AFTER snapshot
identity checks, so the saved model identity remains the frozen reference.
"""
import math
import numpy as np


def coverage(paths,draw,reference,stroke,unsupported):
    reference_paths=paths
    width,height=draw['target'][1:3]
    a,b,_,d,e,_=draw['matrix']
    empty=lambda:(0,0,np.zeros((0,0),dtype=np.float64))
    if not draw['stroke'] and a*e-b*d==0:return empty()
    if draw['stroke']: paths=[(polygon,True) for polygon in stroke(paths,draw)]
    edges=[(a,b) for points,_ in paths for a,b in zip(points,points[1:]) if a!=b and a[1]!=b[1]]
    if not edges:return empty()
    nonzero=draw['stroke'] or draw['nonzero']
    sweep={}
    for a,b in edges:
        direction=1 if b[1]>a[1] else -1
        for y,sign in ((min(a[1],b[1]),1),(max(a[1],b[1]),-1)):
            count,winding=sweep.get(y,(0,0));sweep[y]=count+sign,winding+direction*sign
    count=winding=0
    for y in sorted(sweep):
        c,w=sweep[y];count+=c;winding+=w
        if winding!=0 if nonzero else count&1:raise unsupported('Graphics UI emitted fill edges have an unbalanced scanline')
    if not draw['clips']:return empty()
    points=[p for edge in edges for p in edge]
    x0,y0=max(0,math.floor(min(p[0] for p in points))),max(0,math.floor(min(p[1] for p in points)))
    x1,y1=min(width,math.ceil(max(p[0] for p in points))),min(height,math.ceil(max(p[1] for p in points)))
    if x0>=x1 or y0>=y1:return empty()
    scale=draw['samples'];sw,sh=(x1-x0)*scale,(y1-y0)*scale
    if sh*len(edges)>4_000_000:return reference(reference_paths,draw)
    endpoints=np.asarray(edges,dtype=np.float64)
    ax,ay,bx,by=(endpoints[:,0,0],endpoints[:,0,1],endpoints[:,1,0],endpoints[:,1,1])
    y=y0+(np.arange(sh,dtype=np.float64)+.5)/scale
    active=(np.minimum(ay,by)<=y[:,None]) & (y[:,None]<np.maximum(ay,by))
    cross=ax+(y[:,None]-ay)*(bx-ax)/(by-ay)
    if not np.isfinite(cross[active]).all():return reference(reference_paths,draw)
    cross=np.where(active,cross,np.inf)
    directions=np.broadcast_to(np.where(by>ay,1,-1),cross.shape)
    order=np.lexsort((directions,cross),axis=1)
    xs=np.take_along_axis(cross,order,axis=1)
    dirs=np.take_along_axis(np.where(active,directions,0),order,axis=1)
    levels=np.cumsum(dirs if nonzero else np.take_along_axis(active,order,axis=1),axis=1)
    if np.any(levels[:,-1]!=0 if nonzero else levels[:,-1]&1):raise unsupported('Graphics UI emitted fill edges have an unbalanced scanline')
    inside=(levels[:,:-1]!=0 if nonzero else (levels[:,:-1]&1)!=0) & np.isfinite(xs[:,1:])
    rows,columns=np.nonzero(inside)
    left=np.maximum(0,np.ceil((xs[rows,columns]-x0)*scale-.5)).astype(np.int64)
    right=np.minimum(sw,np.ceil((xs[rows,columns+1]-x0)*scale-.5)).astype(np.int64)
    valid=left<right;rows,left,right=rows[valid],left[valid],right[valid]
    diff=np.zeros((sh,sw+1),dtype=np.int32)
    np.add.at(diff,(rows,left),1);np.add.at(diff,(rows,right),-1)
    raster=np.cumsum(diff[:,:sw],axis=1,dtype=np.int32)>0
    result=raster.reshape(y1-y0,scale,x1-x0,scale).mean(axis=(1,3))
    clipped=np.zeros_like(result,dtype=np.bool_)
    for left,bottom,right,top in draw['clips']:
        left,bottom,right,top=max(left,x0),max(bottom,y0),min(right,x1),min(top,y1)
        if left<right and bottom<top:clipped[bottom-y0:top-y0,left-x0:right-x0]=True
    result*=clipped
    return x0,y0,result


def install():
    from emulator import graphics_devices as gpu
    original=gpu._ui_coverage
    if getattr(original,'_quick_vectorized',False):return lambda:None
    def fast(paths,draw):return coverage(paths,draw,original,gpu._ui_stroke,gpu.UnsupportedDevice)
    fast._quick_vectorized=True
    gpu._ui_coverage=fast
    return lambda:setattr(gpu,'_ui_coverage',original)
