{-# LANGUAGE GHC2021 #-}
module RedEyeMaskScenarios where

import Data.Array (Array, (!), (//), listArray)
import Data.Bits
import qualified Data.ByteString as B
import Data.Int (Int16,Int32)
import Data.List (foldl')
import qualified Data.Map.Strict as M
import Data.Word
import PipelineModel
import Scenarios (Address(..),Patch(..),wordsAt,readWord)

data Raster = Raster { rasterWidth :: Int, rasterHeight :: Int, rasterBytes :: Array Int Word8 } deriving (Eq,Show)
data Shape = Cross | Square deriving (Eq,Show)
data Operation = Erode | Dilate deriving (Eq,Show)

raster :: Int -> Int -> [Word8] -> Raster
raster width height bytes = Raster width height $ listArray (-2*width,storage-1) (replicate (2*width) 0 ++ take storage (bytes++repeat 0))
 where storage=max ((height+3)*width) (length bytes)
pixel :: Raster -> Int -> Word8
pixel image offset = rasterBytes image ! offset
pixels :: Raster -> [Word8]
pixels image = [pixel image offset | offset <- [0..rasterWidth image*rasterHeight image-1]]
replacePixels :: Raster -> [(Int,Word8)] -> Raster
replacePixels image changes = image { rasterBytes=rasterBytes image // changes }

maskImageInitial :: Word32 -> Word32 -> Int -> Int -> [Word8] -> [Patch]
maskImageInitial descriptor address width height bytes =
  [wordsAt descriptor [address,fromIntegral height,fromIntegral width],bytePatch (address-fromIntegral (2*width)) (replicate (2*width) 0 ++ bytes ++ replicate (max 0 ((height+3)*width-length bytes)) 0)]
maskConfigurationInitial :: [Word8] -> [Patch]
maskConfigurationInitial settings = [bytePatch 0x14e213e (take 10 (settings++repeat 0))]
bytePatch :: Word32 -> [Word8] -> Patch
bytePatch address bytes = Patch (Address address) (B.pack bytes)
configuration :: [Word8] -> Int -> Int32
configuration settings index = fromIntegral ((settings++repeat 0) !! index)
i32 :: Integral a => a -> Int32
i32 = fromIntegral

morphology :: Operation -> Shape -> Raster -> Raster
morphology operation shape image = replacePixels initial
  [(row*stride+column,combine [pixel image (row*stride+column+dy*stride+dx) | (dy,dx)<-kernel])
  | row<-[1..height-2],column<-[1..width-2]]
 where
  width=rasterWidth image
  height=rasterHeight image
  stride=max 2 width
  kernel=[(dy,dx) | dy<-[-1..1],dx<-[-1..1],shape==Square || dx==0 || dy==0]
  combine=foldl1 (case operation of Erode -> (.&.); Dilate -> (.|.))
  initial=if operation==Erode && shape==Cross then raster width height [] else image

chromaScore :: Raster -> Raster
chromaScore source = raster width height [score row column | row<-[0..height-1],column<-[0..width-1]]
 where
  width=rasterWidth source
  height=rasterHeight source
  score row column = fromIntegral (max 0 (min 255 ((((198*v-98*y-43*u) :: Int) `shiftR` 8)+99)))
   where
    offset=(row*(width `shiftR` 1)+(column `shiftR` 1))*4
    y=fromIntegral (pixel source (offset+(column .&. 1)))
    u=fromIntegral (pixel source (offset+2))
    v=fromIntegral (pixel source (offset+3))

angleIndex :: Int32 -> Int
angleIndex angle = fromIntegral (normalize angle `quot` 15)
 where
  signed value=fromIntegral (fromIntegral value :: Int16) :: Int32
  unsigned value=fromIntegral (fromIntegral value :: Word16) :: Int32
  normalize value | angle>179 = let next=unsigned (value-360) in if signed next>179 then normalize next else signed next
                  | angle< -180 = let next=unsigned (value+360) in if signed next< -180 then normalize next else signed next
                  | otherwise = signed value
samplerSine :: B.ByteString -> Int32 -> Int32
samplerSine image angle = tableHalf image (0x10873c4+2*angleIndex angle)
samplerCosine :: B.ByteString -> Int32 -> Int32
samplerCosine image angle = tableHalf image (0x1086d84+2*angleIndex angle)
tableHalf :: B.ByteString -> Int -> Int32
tableHalf image address = fromIntegral (fromIntegral (readWord image (fromIntegral address) .&. 0xffff) :: Int16)

sampleYuv :: B.ByteString -> Raster -> Int32 -> Int32 -> Int32 -> Int32 -> Int -> Int -> Raster
sampleYuv image source angle scale centerX centerY width height = raster width height $ concat
  [[channel 0 row column,channel 0 row (column+1),channel 1 row column,channel 2 row column] | row<-[0..height-1],column<-[0,2..width-1]]
 where
  effectiveScale=if angle==0 then scale else fromIntegral (fromIntegral scale :: Int16)
  coordinate row column = (project x centerX,project y centerY)
   where
    dx=i32 column*16-((i32 width-1)*16 `shiftR` 1)
    dy=i32 row*16-((i32 height-1)*16 `shiftR` 1)
    (x,y) | angle==0=(dx,dy)
          | otherwise=((dx*samplerCosine image angle-dy*samplerSine image angle) `quot` 4096,(dx*samplerSine image angle+dy*samplerCosine image angle) `quot` 4096)
    project value center=(value*1024 `quot` effectiveScale)+center
  channel kind row column
   | xq<0 || yq<0 || xq>=(i32 (rasterWidth source)-1)*16 || yq>=(i32 (rasterHeight source)-1)*16 = if kind==0 then 0 else 128
   | otherwise=fromIntegral ((upper*(1024-fy)+lower*fy) `quot` 1048576)
   where
    (xq,yq)=coordinate row column
    sourceY=yq `quot` 16
    fy=(yq*1024 `quot` 16)-sourceY*1024
    sourceX=if kind==0 then xq `quot` 16 else xq `quot` 32
    fx=if kind==0 then xq*1024 `quot` 16-sourceX*1024 else (xq*1024 `quot` 16-sourceX*2048) `quot` 2
    sample dy dx=i32 (pixel source (fromIntegral ((sourceY+i32 dy)*i32 (rasterWidth source `shiftR` 1)+(if kind==0 then (sourceX+i32 dx) `shiftR` 1 else sourceX+i32 dx))*4+(if kind==0 then fromIntegral ((sourceX+i32 dx) .&. 1) else kind+1)))
    horizontal dy=(1024-fx)*sample dy 0+fx*sample dy 1
    upper=horizontal 0
    lower=horizontal 1

imageHistogram :: Raster -> [Int32]
imageHistogram image = [M.findWithDefault 0 value bins | value<-[0..255]]
 where bins=M.fromListWith (+) [(value,1) | value<-pixels image]
maskThreshold :: Raster -> Word8
maskThreshold image = fromIntegral $ choose 0 (reverse (zip [0::Int ..255] (imageHistogram image)))
 where
  quota=i32 (rasterWidth image*rasterHeight image)*20 `quot` 100
  choose _ []=0
  choose total ((value,count):remaining) | total+count>quota=value
                                         | otherwise=choose (total+count) remaining
binaryMask :: Raster -> Raster
binaryMask image = raster width height
  [if radius row column<=225 || pixel image (row*width+column)>=threshold then 255 else 0 | row<-[0..height-1],column<-[0..width-1]]
 where
  width=rasterWidth image
  height=rasterHeight image
  threshold=maskThreshold image
  radius row column=(row-(height-1) `quot` 2)^2+(column-(width-1) `quot` 2)^2

fillMaskRows :: Raster -> Raster
fillMaskRows image = raster width height (concat [snd (foldl' (step row) (False,[]) [0..width-1]) | row<-[0..height-1]])
 where
  width=rasterWidth image
  height=rasterHeight image
  step row (active,output) column = (next,output++[if keep then 255 else 0])
   where
    offset=row*width+column
    present=pixel image offset/=0
    next | row<14 || row>50=active
         | not active=present
         | present=True
         | otherwise=any ((/=0).pixel image.(offset+)) [1..5]
    support=any ((/=0).pixel image.(offset+)) [3-3*height,3-4*height,3-5*height,3-6*height,-height,height]
    keep | row<14 || row>50=present
         | otherwise=next && (present || support)
fillMaskColumns :: Raster -> Raster
fillMaskColumns image = replacePixels (raster width height []) $ concat [snd (foldl' (step column) (False,[]) [0..height-1]) | column<-[0..width-1]]
 where
  width=rasterWidth image
  height=rasterHeight image
  step column (active,output) row=(next,output++[(offset,if keep then 255 else 0)])
   where
    offset=row*width+column
    present=pixel image offset/=0
    next | column<14 || column>50=active
         | not active=present
         | present=True
         | otherwise=any ((/=0).pixel image.(offset+).(*width)) [1..5]
    support=any ((/=0).pixel image.(offset+)) [3*width-3,3*width-4,3*width-5,3*width-6,-1,1]
    keep | column<14 || column>50=pixel image (offset+width)/=0
         | otherwise=next && (present || (active && support))

rowRuns :: Raster -> Int -> Int -> Int -> [(Int,Int)]
rowRuns image row first lastColumn = reverse (third (foldl' step (False,first,[]) [first..lastColumn-1]))
 where
  third (_,_,value)=value
  step (inside,start,complete) column
    | not inside && present=(True,column,complete)
    | inside && not present=(False,start,(start,column):complete)
    | otherwise=(inside,start,complete)
   where present=pixel image (row*rasterWidth image+column)/=0

touching :: (Int,Int) -> (Int,Int) -> Bool
touching (a,b) (c,d) = c-1<=b && b<=d || a-1<=d && d<=b
within :: (Int,Int) -> (Int,Int) -> Bool
within (a,b) (c,d) = c-1<=a && a<=d+1 && c-1<=b && b<=d+1

fillForkGaps :: Raster -> Raster
fillForkGaps image = replacePixels image (concatMap gaps (M.elems records))
 where
  records=foldl' process M.empty [8..55]
  get record offset=M.findWithDefault 0 offset record
  put offset value=M.insert offset value
  pair record offset=(get record offset,get record (offset+1))
  setPair offset (a,b)=put offset a . put (offset+1) b
  process allRecords row
    | null runs=allRecords
    | otherwise=foldl' advance seeded [0..9]
   where
    runs=take 11 [(a,b-1) | (a,b)<-rowRuns image row 8 56]
    seeded=foldl' seed allRecords runs
    seed allRecords' run=case [index | index<-[0..9],get (M.findWithDefault M.empty index allRecords') 0==0] of
      [] -> allRecords'
      index:_ -> M.insert index (put 0 1 (setPair 3 run (M.findWithDefault M.empty index allRecords'))) allRecords'
    advance allRecords' index=M.insert index next (foldl' (\current (other,changed) -> M.insert other changed current) allRecords' updates)
     where
      record=M.findWithDefault M.empty index allRecords'
      state=get record 0
      overlaps=filter (touching (pair record 3)) runs
      (next,updates) | state==1=(put 0 2 record,[])
                     | state==2 && length overlaps>1=(put 0 3 (put 1 row (put 2 1 (setPair 7 (head overlaps) (setPair 9 (fst (overlaps!!1),snd (last overlaps)) record)))),[])
                     | state==2=(put 0 0 record,[])
                     | state/=3=(record,[])
                     | otherwise=advanceFork allRecords' index record
    advanceFork allRecords' index record
      | matches>1 || get scanned 0==5=(scanned,[])
      | otherwise=case [merge otherIndex other | (otherIndex,other)<-M.toAscList allRecords',otherIndex/=index,get other 0==3,canMerge other] of
          next:_ -> next
          [] -> (put 0 0 scanned,[])
     where
      (matches,scanned,lastLength)=foldl' follow (0,record,get record 2) (zip [0..] runs)
      follow (count,record',_) (runIndex,run)
        | within (pair record' (base+4)) run=(count,put 0 5 record',len)
        | count==0=case filter (touching (pair record' (base+3))) (drop runIndex runs) of
            [] -> (0,record',len)
            candidates -> (1,setPair (base+7) (last candidates) record',len)
        | touching (pair record' (base+5)) run=(count+1,put 2 (len+1) (setPair (base+9) run record'),len)
        | otherwise=(count,record',len)
       where len=get record' 2; base=4*len
      base=4*lastLength
      canMerge other | matches==1=pair scanned (base+5)==pair other (4*get other 2+3)
                     | otherwise=touching (pair scanned (base+3)) (pair other (4*get other 2+5))
      merge otherIndex other = (put 2 (get scanned 2+1) (foldl' (\record' offset -> put (base+4+offset) (get changed (base+offset)) record') changed [3..6]),[(otherIndex,changedOther)])
       where
        side=if matches==1 then 5 else 3
        opposite=if matches==1 then 3 else 5
        changed=setPair (base+side) (pair other (4*get other 2+side)) scanned
        changedOther=setPair (4*get other 2+opposite) (pair changed (base+opposite)) other
  gaps record | get record 0/=5=[]
              | otherwise=[((get record 1+row)*rasterWidth image+column,255) | row<-[0..get record 2-1],column<-[get record (4*row+8)..get record (4*row+9)-1]]

data CenterScan = CenterScan
  { runActive :: Bool
  , runSource :: Int
  , runDestination :: Int
  , previousSource :: Int
  , previousLeft :: Int
  , currentLeft :: Int
  , seedLeft :: Int
  , centerFound :: Bool
  , centerOutput :: Raster
  } deriving (Eq,Show)

retainCenter :: [Word8] -> Raster -> Raster
retainCenter settings initial = loop 0 0 initial
 where
  width=rasterWidth initial
  height=rasterHeight initial
  middle=height `quot` 2
  drift=fromIntegral (configuration settings 3)
  empty=raster width height []
  copySource source end scan
    | end-runSource scan<4 || runSource scan>=end=scan
    | otherwise=scan {runDestination=runDestination scan+end-runSource scan,centerOutput=replacePixels (centerOutput scan)
        [(runDestination scan+offset-runSource scan,pixel source offset) | offset<-[runSource scan..end-1]]}
  linked adjacent end scan=runDestination scan<end && any (\offset -> pixel (centerOutput scan) (offset+adjacent)/=0) [runDestination scan..end-1]
  adjust row scan
    | drift+currentLeft scan<previousLeft scan && width `quot` 2+2<row && previousLeft scan<width `quot` 2=
        scan {runSource=previousSource scan+height,runDestination=runDestination scan+previousLeft scan-currentLeft scan}
    | otherwise=scan
  propagate source row adjacent end scan
    | not (linked adjacent end scan)=scan
    | otherwise=copySource source end (adjusted {previousSource=runSource adjusted,previousLeft=currentLeft adjusted})
   where adjusted=adjust row scan
  scanDown source scan row=foldl' step (scan {runActive=False}) [0..width-1]
   where
    step current column
      | not (runActive current)=if present then current {runActive=True,runDestination=offset,currentLeft=column,
          runSource=if row==middle then runSource current else offset,seedLeft=if row==middle then column else seedLeft current} else current
      | present=current
      | row==middle=
          if seedLeft current<width `quot` 2 && width `quot` 2<column
          then current {runActive=False,centerFound=True,runDestination=runDestination current+column-seedLeft current,
            centerOutput=replacePixels (centerOutput current) [(runDestination current+x-seedLeft current,255) | x<-[seedLeft current..column-1]]}
          else current {runActive=False}
      | otherwise=(propagate source row (-width) offset current) {runActive=False}
     where offset=row*width+column; present=pixel source offset/=0
  scanUp source scan row=foldl' step (scan {runActive=False,runSource=row*width}) [0..width-1]
   where
    step current column
      | not (runActive current)=if present then current {runActive=True,runSource=offset,runDestination=offset,currentLeft=column} else current
      | present=current
      | otherwise=propagated {runActive=False,centerFound=centerFound propagated || (linked width offset current && row==height `quot` 2 && currentLeft current<=width `quot` 2 && width `quot` 2<=column)}
     where
      offset=row*width+column
      present=pixel source offset/=0
      propagated=propagate source row width offset current
  scanLinear source adjacent current offset
    | not (runActive current)=if pixel source offset/=0 then current {runActive=True,runSource=offset,runDestination=offset} else current
    | pixel source offset/=0=current
    | otherwise=(if linked adjacent offset current then copySource source offset current else current) {runActive=False}
  scanReverseRow source scan row=foldl' (scanLinear source width) (scan {runSource=row*width,runDestination=row*width}) [row*width..(row+1)*width-1]
  loop iteration oldSeed source
    | centerFound final || iteration==9=centerOutput final
    | otherwise=loop (iteration+1) (seedLeft final) (replacePixels source [(offset,0) | offset<-[0..width*height-1],pixel (centerOutput final) offset/=0])
   where
    start=CenterScan False (middle*width) (middle*width) (middle*width) 0 0 oldSeed False empty
    down=foldl' (scanDown source) start [middle..height-1]
    up=foldl' (scanUp source) (down {previousLeft=0,currentLeft=0}) [middle-1,middle-2..1]
    forward=foldl' (scanLinear source (-width)) (up {runSource=0,runDestination=0}) [0..width*height-1]
    final=foldl' (scanReverseRow source) forward [height,height-1..1]

basisMask :: [Word8] -> Raster -> Raster
basisMask settings color = morphology Dilate Cross (retainCenter settings (fillForkGaps (fillMaskColumns (fillMaskRows binary))))
 where
  width=rasterWidth color
  height=rasterHeight color
  threshold=maskThreshold color
  inner=fromIntegral (configuration settings 9)
  outer=fromIntegral (configuration settings 0+configuration settings 2*256)
  binary=raster width height [if ((pixel color (row*width+column)>=threshold || radius<=inner) && radius<=outer) then 255 else 0
    | row<-[0..height-1],column<-[0..width-1],let radius=(row-(height-1) `quot` 2)^2+(column-(width-1) `quot` 2)^2]

refineMask :: Raster -> Raster
refineMask color = iterate pairDilate core !! (depth+3)
 where
  pairErode=morphology Erode Square . morphology Erode Cross
  pairDilate=morphology Dilate Square . morphology Dilate Cross
  initial=iterate pairErode (iterate pairDilate (iterate pairErode (binaryMask color)!!2)!!5)!!3
  layers=takeWhile (any (/=0).pixels) (iterate pairErode initial)
  depth=max 0 (length layers-1)
  core=if null layers then initial else last layers
filterMask :: [Word8] -> Int32 -> Raster -> Raster -> Raster
filterMask settings distance basis mask
  | total==0=mask
  | otherwise=replacePixels mask [(offset,filtered row column) | row<-[0..height-1],column<-[0..width-1],let offset=row*width+column]
 where
  width=rasterWidth mask
  height=rasterHeight mask
  total=sum (map i32 (pixels mask))
  centerX=sum [i32 column*i32 (pixel mask (row*width+column)) | row<-[0..height-1],column<-[0..width-1]] `quot` total
  centerY=sum [i32 row*i32 (pixel mask (row*width+column)) | row<-[0..height-1],column<-[0..width-1]] `quot` total
  outer=configuration settings 5
  inner=configuration settings 6
  gain=configuration settings 4
  filtered row column
    | configuration settings 8/=0 && pixel basis offset==0=0
    | radius>outer=0
    | otherwise=fromIntegral (max 0 (min 255 ((factor*i32 (pixel mask offset)) `shiftR` 8)))
   where
    offset=row*width+column
    radius=(((i32 column-centerX)^2+(i32 row-centerY)^2)*10000) `quot` distance
    factor=if radius>inner then (outer-radius)*gain `quot` 119 else gain

maskGains :: [Word8] -> Raster -> Raster -> Maybe [Int32]
maskGains settings sampled mask
  | null selected=Nothing
  | otherwise=Just [gain yMean yCutoff,gain chromaMean chromaCutoff,gain chromaMean chromaCutoff]
 where
  width=rasterWidth mask
  height=rasterHeight mask
  channels=[(y,chroma,pixel mask (row*width+column)/=0) | row<-[0..height-1],column<-[0..width-1],let pair=(row*(rasterWidth sampled `shiftR` 1)+(column `shiftR` 1))*4,let y=i32 (pixel sampled (pair+(column .&. 1))),let u=i32 (pixel sampled (pair+2)),let v=i32 (pixel sampled (pair+3)),let chroma=max (abs (u-128)) (abs (v-128))]
  selected=[(y,chroma) | (y,chroma,True)<-channels]
  yMean=sum (map fst selected) `quot` i32 (length selected)
  chromaMean=sum (map snd selected) `quot` i32 (length selected)
  cutoff multiplier values=case [bin | bin<-[0..254],i32 (length (filter (<=bin) values))*multiplier>i32 (width*height)] of bin:_ -> bin; [] -> 0
  yCutoff=cutoff (configuration settings 1) [y | (y,_,_)<-channels]
  chromaCutoff=cutoff (configuration settings 7) [chroma | (_,chroma,_)<-channels]
  gain mean cutoffValue=if mean==0 then 1 else (cutoffValue `shiftL` 8) `quot` mean

prepareMaskPixels :: B.ByteString -> [Word8] -> Raster -> Int32 -> Int32 -> Int32 -> Int -> Int -> Int32 -> (Raster,Maybe [Int32])
prepareMaskPixels image settings source scale centerX centerY width height distance = (result,if scaled>0 then maskGains settings sampled result else Nothing)
 where
  sampled=sampleYuv image source 0 scale centerX centerY width height
  color=chromaScore sampled
  refined=refineMask color
  basis=if configuration settings 8==0 then raster width height [] else basisMask settings color
  scaled=(distance*scale `quot` 1024)*scale `quot` 1024
  result=if scaled>0 then filterMask settings scaled basis refined else refined

redeyeMaskPublications :: [StagePublication]
redeyeMaskPublications = [StagePublication (Address address) (StageSymbol ("xm1_pipeline_image_redeye_"++symbol)) | (address,symbol)<-
  [(0x1072ad8,"sample_rotated"),(0x1072ff0,"sample_scaled"),(0x1073404,"dispatch_sample"),(0x1083954,"chroma_score"),(0x1083a2c,"dilate_square"),(0x1083b60,"erode_square"),(0x1083c94,"dilate_cross"),(0x1083dc0,"erode_cross"),(0x1083eec,"intervals_touch"),(0x1083f3c,"endpoints_within"),(0x1083f88,"fill_fork_gaps"),(0x1084710,"fill_mask_rows"),(0x10848f0,"fill_mask_columns"),(0x1084ac8,"retain_center_component"),(0x10852cc,"build_masks"),(0x108566c,"count_mask_pixels"),(0x10856dc,"refine_mask"),(0x1085854,"filter_mask"),(0x1085ab0,"mask_gains"),(0x1085cf0,"prepare_edit_mask"),(0x1086c54,"sine"),(0x1086ce0,"cosine")]]

redeyeMaskScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
redeyeMaskScenarios image _resolve = morphologyCases ++ scoreCases ++ intervalCases ++ samplingCases ++ maskCases ++ trigCases ++ closureCases ++ rootCases
 where
  publication address=head [value | value<-redeyeMaskPublications,stageStockEntry value==Address address]
  scenario address name arguments initial final result=StageScenario (StageCaseName ("redeye-mask/"++name)) (publication address) arguments initial final result [] []
  descriptor=0x960000
  workDescriptor=0x960020
  input=0x970000
  output=0x980000
  bytes image'=bytePatch input (pixels image')
  seed=raster 7 7 [if row==3 && column==3 then 0x5a else 0 | row<-[0..6],column<-[0..6]]
  dense=raster 7 7 [if row==3 && column==3 then 0x3c else 0xff | row<-[0..6],column<-[0..6]]
  morphologyCases=[scenario address (name++suffix) [descriptor,workDescriptor]
    (maskImageInitial descriptor input 7 7 (pixels source)++maskImageInitial workDescriptor output 7 7 (replicate 49 0xa5)) [bytes (morphology operation shape source)] Nothing
    | (address,name,operation,shape)<-[(0x1083a2c,"dilate-square",Dilate,Square),(0x1083b60,"erode-square",Erode,Square),(0x1083c94,"dilate-cross",Dilate,Cross),(0x1083dc0,"erode-cross",Erode,Cross)],(suffix,source)<-[("-nonzero",seed),("-dense",dense)]]
  scoreCases=[scenario 0x1083954 "chroma-score-clamp-and-parity" [descriptor,workDescriptor]
    (maskImageInitial descriptor input 4 1 [0,255,0,255,255,0,255,0]++maskImageInitial workDescriptor output 4 1 (replicate 4 0xa5))
    [bytePatch output [255,198,0,56]] Nothing]
  intervalCases=[scenario address ("interval-"++show address++"-"++show arguments) arguments [] [] (Just expected)
    | (address,arguments,expected)<-[(0x1083eec,[2,4,5,8],1),(0x1083eec,[2,4,6,8],0),(0x1083f3c,[4,8,5,7],1),(0x1083f3c,[3,8,5,7],0)]]
  source=raster 8 8 (concat [[fromIntegral (row*20+column),fromIntegral (row*20+column+1),100,200] | row<-[0..7],column<-[0,2,4,6]])
  samplingCases=[scenario 0x1072ff0 "sample-scaled-fractional-nonzero" [input,8,8,1024,52,52,4,4,output]
    [bytePatch input (pixelsPacked source),bytePatch output (replicate 32 0xa5)] [bytePatch output (pixelsPacked (sampleYuv image source 0 1024 52 52 4 4))] Nothing,
    scenario 0x1072ff0 "sample-scaled-outside" [input,8,8,1024,0xfffffff0,0xfffffff0,2,2,output]
    [bytePatch input (pixelsPacked source),bytePatch output (replicate 8 0xa5)] [bytePatch output [0,0,128,128,0,0,128,128]] Nothing,
    scenario 0x1072ff0 "sample-scaled-odd-width-control" [0,0,0,0,0,0,3,9,output] [bytePatch output [1,2,3,4]] [bytePatch output [1,2,3,4]] Nothing,
    scenario 0x1073404 "dispatcher-null-configuration-control" [0,1,0,0,0,64,64,0] [] [] Nothing]
  pixelsPacked sample=[pixel sample offset | offset<-[0..rasterWidth sample*rasterHeight sample*2-1]]
  maskCases=[scenario 0x108566c "count-nonzero-not-sum" [descriptor] (maskImageInitial descriptor input 7 7 (pixels seed)) [] (Just 1),
    scenario 0x108566c "count-empty-height-control" [descriptor] [wordsAt descriptor [0,0,7]] [] (Just 0),
    scenario 0x1085854 "filter-zero-weight-preserves-mask" [descriptor,0,0,0] (maskImageInitial descriptor input 7 7 (replicate 49 0)) [bytePatch input (replicate 49 0)] Nothing,
    scenario 0x1085854 "filter-nonzero-centroid" [descriptor,0,workDescriptor,10000]
      (maskImageInitial descriptor input 7 7 (pixels dense)++maskImageInitial workDescriptor output 7 7 (replicate 49 255)++maskConfigurationInitial [0,2,0,0,200,255,255,2,0,0])
      [bytes (filterMask [0,2,0,0,200,255,255,2,0,0] 10000 (raster 7 7 (replicate 49 255)) dense)] Nothing]
  trigCases=[scenario address ("trig-"++show address++"-"++show angle) [fromIntegral angle] [] [] (Just (fromIntegral (function image angle)))
    | (address,function)<-[(0x1086c54,samplerSine),(0x1086ce0,samplerCosine)],angle<-[-180,-91,-1,0,14,15,90,179,180,359]]
  componentWorkspace=0x990000
  editAddress=0xa10000
  contextAddress=0x9a0000
  sampleAddress=0x9b0000
  colorAddress=0x9c0000
  temporaryAddress=0x9d0000
  workAddress=0x9e0000
  basisAddress=0x9f0000
  maskAddress=0xa00000
  flags=[100,2,0,3,255,255,255,2,0,25]
  fork=raster 64 64 [if (row==9 && column>=24 && column<=40) || (row==10 && (column>=24 && column<=28 || column>=36 && column<=40)) || (row==11 && column>=24 && column<=40) then 255 else 0 | row<-[0..63],column<-[0..63]]
  forkExpected=replacePixels fork [(10*64+column,255) | column<-[29..35]]
  circle=raster 64 64 [if (row-31)^2+(column-31)^2<=100 then 255 else 0 | row<-[0..63],column<-[0..63]]
  closureCases=
    [scenario 0x1083f88 "fork-split-rejoin-fills-nonzero-gap" [descriptor,workDescriptor,componentWorkspace]
      (maskImageInitial descriptor input 64 64 (pixels fork)++maskImageInitial workDescriptor output 64 64 (replicate 4096 0xa5)++[bytePatch componentWorkspace (replicate 1950 0xa5)])
      [bytes forkExpected] Nothing,
     scenario 0x1083f88 "fork-no-closed-runs-control" [descriptor,workDescriptor,componentWorkspace]
      (maskImageInitial descriptor input 64 64 (replicate 4096 255)++maskImageInitial workDescriptor output 64 64 (replicate 4096 0xa5)++[bytePatch componentWorkspace (replicate 1950 0xa5)])
      [bytePatch input (replicate 4096 255)] Nothing,
     scenario 0x1084710 "row-fill-retains-supported-run" [descriptor,workDescriptor]
      (maskImageInitial descriptor input 64 64 (pixels circle)++maskImageInitial workDescriptor output 64 64 (replicate 4096 0xa5))
      [bytes (fillMaskRows circle)] Nothing,
     scenario 0x10848f0 "column-fill-retains-supported-run" [descriptor,workDescriptor]
      (maskImageInitial descriptor input 64 64 (pixels circle)++maskImageInitial workDescriptor output 64 64 (replicate 4096 0xa5))
      [bytes (fillMaskColumns circle)] Nothing,
     scenario 0x1084ac8 "center-retention-connected-nonzero" [descriptor,workDescriptor]
      (maskImageInitial descriptor input 64 64 (pixels circle)++maskImageInitial workDescriptor output 64 64 (replicate 4096 0xa5)++maskConfigurationInitial flags)
      [bytes (retainCenter flags circle)] Nothing,
     scenario 0x1084ac8 "center-retention-unclosed-run-control" [descriptor,workDescriptor]
      (maskImageInitial descriptor input 64 64 (replicate 4096 255)++maskImageInitial workDescriptor output 64 64 (replicate 4096 0xa5)++maskConfigurationInitial flags)
      [bytePatch input (replicate 4096 0)] Nothing,
     scenario 0x10852cc "binary-mask-histogram-and-forced-center" [descriptor,workDescriptor,contextAddress+60,contextAddress+44,contextAddress+28,contextAddress+76,contextAddress+0x84c]
      (maskImageInitial descriptor input 64 64 (replicate 4096 0xa5)++maskImageInitial workDescriptor output 64 64 [fromIntegral (column*4) | _row<-[0..63],column<-[0..63]]++maskConfigurationInitial flags++workspace)
      [bytes (binaryMask (raster 64 64 [fromIntegral (column*4) | _row<-[0..63],column<-[0..63]]))] Nothing,
     scenario 0x10856dc "refine-nonzero-binary-mask" [descriptor,workDescriptor,contextAddress+28,contextAddress+44,contextAddress+60,contextAddress+76,contextAddress+0x84c]
      (maskImageInitial descriptor input 64 64 (replicate 4096 0xa5)++maskImageInitial workDescriptor output 64 64 (replicate 4096 200)++maskConfigurationInitial flags++workspace)
      [bytes (refineMask (raster 64 64 (replicate 4096 200)))] Nothing,
     scenario 0x1085ab0 "gain-zero-mask-preserves-prior-values" [descriptor,editAddress,contextAddress+76,contextAddress+0x44c]
      (maskImageInitial descriptor input 8 8 (pixelsPacked source)++[wordsAt editAddress [11,22,33,0,0,0,0,1024,output,8,8,0,0,0],bytePatch output (replicate 64 0)]++workspace++maskConfigurationInitial flags)
      [wordsAt editAddress [11,22,33]] Nothing,
     scenario 0x1072ad8 "rotated-sampling-nonzero-and-border" [input,8,8,90,1024,52,52,4,4,output]
      [bytePatch input (pixelsPacked source),bytePatch output (replicate 32 0xa5)]
      [bytePatch output (pixelsPacked (sampleYuv image source 90 1024 52 52 4 4))] Nothing,
     scenario 0x1073404 "dispatcher-active-scaled-sampling" [descriptor,0,1024,52,52,4,4,workDescriptor]
      (maskImageInitial descriptor input 8 8 (pixelsPacked source)++[wordsAt workDescriptor [output],bytePatch output (replicate 32 0xa5)])
      [bytePatch output (pixelsPacked (sampleYuv image source 0 1024 52 52 4 4))] Nothing]
  workspace=maskImageInitial contextAddress sampleAddress 64 64 (replicate 8192 0)
    ++maskImageInitial (contextAddress+12) colorAddress 64 64 (replicate 4096 0)
    ++maskImageInitial (contextAddress+28) temporaryAddress 64 64 (replicate 4096 0)
    ++maskImageInitial (contextAddress+44) workAddress 64 64 (replicate 4096 0)
    ++maskImageInitial (contextAddress+60) basisAddress 64 64 (replicate 4096 0)
    ++[bytePatch (contextAddress+76) (replicate 2048 0),bytePatch (contextAddress+0x84c) (replicate 1950 0)]
  largeSource=raster 128 128 (concat (replicate (128*64) [100,120,100,220]))
  rootCases=[scenario 0x1085cf0 ("prepare-edit-mask-"++name) [descriptor,editAddress,fromIntegral distance,contextAddress]
    (maskImageInitial descriptor input 128 128 (pixelsPacked largeSource)++workspace++maskConfigurationInitial settings
      ++[wordsAt editAddress [11,22,33,1016,64,1016,64,1024,maskAddress,64,64,0,0,0],bytePatch (maskAddress-128) (replicate (4096+320) 0)])
    [bytePatch maskAddress (pixels result),wordsAt editAddress (maybe [11,22,33] (map fromIntegral) gains)] Nothing
    | (name,distance,settings)<-[("positive-nonzero-gains",1000000,flags),("negative-distance-control",-1,flags),("zero-distance-control",0,flags),("basis-enabled-nonzero",1000000,take 8 flags++[1,25])],
      let (result,gains)=prepareMaskPixels image settings largeSource 1024 1016 1016 64 64 distance]
