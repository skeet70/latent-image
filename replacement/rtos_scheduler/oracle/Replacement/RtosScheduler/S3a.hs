{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosScheduler.S3a (Service (..), services, serviceName, serviceExtent, Scenario (..), scenarios, Linked (..), makeSpec, difference, expected, contractHolds, controlSpec) where

import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (sortOn)
import Data.Word (Word32)
import qualified Emulation.NativeCase as C
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Layout
import Replacement.RtosScheduler.Oracle (Env (..), Row (..), Locality (..), Body (..), caseGrid, caseSpec, taskAddress, recordAddress)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.RtosScheduler.Regions (Helper (Rotate))

data Service = Disable | Enable | Exit | RotateTask | RotateInterrupt | RotateCore | RotateInterruptCore deriving (Eq, Show, Enum, Bounded)
services :: [Service]
services = [minBound .. maxBound]
serviceName :: Service -> String
serviceName s = case s of
  Disable -> "disable-dispatch"
  Enable -> "enable-dispatch"
  Exit -> "exit-task"
  RotateTask -> "rotate-ready"
  RotateInterrupt -> "interrupt-rotate-ready"
  RotateCore -> "core-rotate-ready"
  RotateInterruptCore -> "interrupt-core-rotate-ready"
serviceExtent :: Domain -> Service -> (Word32, Word32)
serviceExtent domain s = (start + delta, size)
  where
    delta = if domain == DomainA then 0 else 0x8ffc0
    (start, size) = case s of
      Disable -> (0x4c9dc,228)
      Enable -> (0x4cac0,280)
      Exit -> (0x453f0,256)
      RotateTask -> (0x4c28c,224)
      RotateInterrupt -> (0x4c36c,200)
      RotateCore -> (0x4c434,252)
      RotateInterruptCore -> (0x4c530,240)

data Scenario = Scenario
  { context :: Word32, irqMasked :: Bool, selector :: Word32, priority :: Word32
  , members :: Int, selectedQueue :: Bool, dispatchWord :: Word32, rescheduleWord :: Word32
  , discarded :: Word32, flags :: Word32, ready :: Bool, held :: Bool, retryAcquire :: Bool, entryControl :: Word32
  } deriving (Eq, Show)

interruptService :: Service -> Bool
interruptService s = s `elem` [RotateInterrupt, RotateInterruptCore]
coreService :: Service -> Bool
coreService s = s `elem` [RotateCore, RotateInterruptCore]
rotation :: Service -> Bool
rotation s = s `elem` [RotateTask, RotateInterrupt, RotateCore, RotateInterruptCore]

scenarios :: Service -> [Scenario]
scenarios s = baseRows ++ refusalRows ++ specialRows
  where
    base = Scenario (if interruptService s then 1 else 0) False 0 4 2 True 1 0 0 0 True False False 0x53
    baseRows
      | rotation s = [base {selector=c, priority=p, members=n, selectedQueue=q, dispatchWord=d, rescheduleWord=r} | c <- if coreService s then [0,1,2] else [0], p <- if s==RotateTask then [0,1,4,16] else [1,4,16], n <- [0,1,2,3], q <- [False,True], d <- [0,1,2,0x80000000], r <- [0,1,0x80000000]]
      | s==Exit = [base {dispatchWord=d, discarded=x, flags=f, context=c, irqMasked=i, selectedQueue=q} | d <- [0,1,2,0x80000000], x <- [0,1,2], f <- [0,1,6,7,0xff], c <- [0,1], i <- [False,True], q <- [False,True]]
      | s==Enable = [base {dispatchWord=d, discarded=x, selectedQueue=q} | d <- [0,1,2,0x80000000], x <- [0,1,2], q <- [False,True]]
      | otherwise = [base {dispatchWord=d} | d <- [0,1,2,0x80000000]]
    refusalRows = [base {context=c, irqMasked=i, priority=p, selector=k} | c <- [0,1,2,0x80000000], i <- [False,True], p <- if rotation s then [0,1,16,17,0xffffffff] else [4], k <- if s==RotateCore then [0,3,0xffffffff] else [0]]
    specialRows = [base {held=True},base {retryAcquire=True}] ++ [base {ready=False} | s==Disable] ++ [base {entryControl=control,irqMasked=masked} | control<-[0x13,0x53,0x1f,0x5f,0xa0000053,0x080f005f],masked<-[False,True]]

handlerAddress :: Domain -> Word32
handlerAddress d = if d==DomainA then 0x400545a4 else 0x400e4564

poke :: Word32 -> B.ByteString -> CaseRegion -> CaseRegion
poke at value region@(CaseRegion raw permission)
  | at >= base && at + fromIntegral (B.length value) <= base + fromIntegral (B.length bytes) = CaseRegion raw { C.regionBytes = B.take offset bytes <> value <> B.drop (offset+B.length value) bytes } permission
  | otherwise = region
  where
    base = fromIntegral (C.baseAddress raw) :: Word32
    offset = fromIntegral (at-base)
    bytes = C.regionBytes raw

data Linked = Standalone B.ByteString | Aggregate B.ByteString [Agg.AggregateSymbol] [Agg.SectionPlacement]

makeSpec :: Env -> Domain -> B.ByteString -> Maybe Linked -> Service -> Scenario -> String -> CaseSpec
makeSpec env domain loader owned service scenario label = spec
  { caseId=label, caseEntry=address entry, caseRegions=regions, caseFunctions=functions
  , caseInitial=initial, caseExpectedStatus=endStatus, caseKernel=Just (KernelCase (InstructionLimit 1600) stop Nothing Nothing Nothing excluded [])
  , caseExpectedRegisters=[(R0,expectedR0 service scenario endStatus)], caseProvenance="X-M1 1.50 exact S3a contracts, x70view 55436fc85c", caseExclusion="One service call; retained helpers execute at stock addresses; dispatcher entry is a stop boundary."
  }
  where
    layout=envLayout env
    own=domainWords layout domain
    target = if coreService service && selector scenario /= 0 then (if selector scenario==1 then DomainA else DomainB) else domain
    targetWords=domainWords layout target
    row0=case caseGrid layout Rotate of first:_ -> first; [] -> error "empty rotate grid"
    lists=M.fromList ([(3,take (members scenario) [2,3,4])] ++ [(7,[5]) | not (selectedQueue scenario)])
    chosen=if selectedQueue scenario then (if members scenario==0 then Nothing else Just 2) else Just 5
    row=row0 {rowLocality=if target==domain then Local else Remote, rowLists=lists, rowSelected=chosen, rowStale=Nothing, rowDispatch=dispatchWord scenario, rowTaskPriority=3, rowState=1, rowFlags=0, rowArgument=3, rowLockWord=if held scenario then 1 else 0}
    entry=fst (serviceExtent domain service)
    loaderBase=if domain==DomainA then 0x40000 else 0xd0000
    selected = case owned of
      Just (Aggregate _ symbols _) -> symbols
      _ -> []
    placements = case owned of
      Just (Aggregate _ _ rows) -> rows
      _ -> []
    placementSpans = [(Agg.placementAddress placement, Agg.placementBytes placement) | placement <- placements]
    aggregateImage = case owned of
      Just (Aggregate bin symbols _) -> Just (Agg.AggregateImage (minimum (map Agg.symbolAddress symbols)) bin)
      _ -> Nothing
    bytes=case owned of
      Nothing -> loader
      Just (Standalone bin) -> foldl' replace loader services
        where
          ownBase=fst (serviceExtent domain Exit)
          replace old s=let (at,size)=serviceExtent domain s; offset=fromIntegral (at-loaderBase); source=fromIntegral (at-ownBase) in B.take offset old <> B.take (fromIntegral size) (B.drop source bin) <> B.drop (offset+fromIntegral size) old
      Just (Aggregate _ _ rows) -> case aggregateImage of
        Just image ->
          let withSymbols = foldl' installSymbol loader selected
              installSymbol old symbol = either error id (Agg.installSymbol loaderBase old image symbol)
           in either error id (Agg.installPlacements loaderBase withSymbols image rows)
        Nothing -> loader
    slice :: Word32 -> Word32 -> B.ByteString
    slice at size=B.take (fromIntegral size) (B.drop (fromIntegral (at-loaderBase)) bytes)
    tableAt=if domain==DomainA then 0x4daa0 else 0xdda64
    helperBase=minimum (map fst helpers)
    helperEnd=maximum [at+size | (at,size)<-helpers]
    symbolSpans = [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- selected]
    allSymbolSpans = case owned of
      Just (Aggregate _ symbols _) -> [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- symbols, Agg.symbolSize symbol > 0]
      _ -> []
    aggregateBase = minimum (map fst (symbolSpans ++ allSymbolSpans ++ helpers ++ placementSpans))
    aggregateTop = maximum [at+size | (at,size)<-symbolSpans ++ allSymbolSpans ++ helpers ++ placementSpans]
    body = case owned of
      Just (Aggregate _ _ _) -> Body entry [(aggregateBase, slice aggregateBase (aggregateTop-aggregateBase))] [] [] [(tableAt,slice tableAt 16)]
      _ -> Body entry [(entry,slice entry (snd (serviceExtent domain service))),(helperBase,slice helperBase (helperEnd-helperBase))] [] [] [(tableAt,slice tableAt 16)]
    spec=caseSpec env domain body label Rotate row
    rawRegions=map extendContext (caseRegions spec)
    extendContext region@(CaseRegion raw permission)
      | C.baseAddress raw==fromIntegral (wordsSchedulerLock own) =
          let prefix=fromIntegral (wordsSchedulerLock own-handlerAddress domain)
              initialBytes=B.replicate prefix 0<>C.regionBytes raw
          in mkRegion (regionIdOf raw) (address (handlerAddress domain)) (B.length initialBytes) permission initialBytes
      | otherwise = region
    localTask=taskAddress layout domain 2
    targetTask=taskAddress layout target 2
    w at value=(at,word32le value)
    patches=
      [ w (wordsContexts own-4) (recordAddress layout domain domain)
      , w (handlerAddress domain) (context scenario)
      , w (wordsRunning own) localTask
      , w (wordsReadyHeads own-4) (rescheduleWord scenario)
      , w (localTask+0x30) (discarded scenario)
      , (localTask+0x0e,B.singleton (fromIntegral (flags scenario)))
      , (localTask+0x0c,B.singleton (if ready scenario then 1 else 2))
      , (localTask+0x0d,B.singleton 3)
      ] ++ [w (wordsDispatchEnabled own) (dispatchWord scenario) | target /= domain]
        ++ [w (wordsSelected own) (if selectedQueue scenario then localTask else taskAddress layout domain 5) | service `elem` [Disable,Enable]]
        ++ [w (wordsRunning targetWords) targetTask | target==domain]
        ++ [w (recordAddress layout domain target+0xc) 0x00101000 | retryAcquire scenario]
        ++ [w (recordAddress layout domain target+0x10) 0x00102000 | not (ready scenario)]
    regions=foldl' (\rs (at,value)->map (poke at value) rs) (rawRegions++fixtureRegions) patches
    helpers=if domain==DomainA then [(0x439d8,0x3c),(0x43a14,0x28),(0x43c48,0x5c),(0x43ca4,0x7c),(0x43d20,0xb0),(0x43dd0,0x48),(0x43f70,0x2c),(0x44018,0x2c),(0x44134,0x80),(0x433e4,0x60)] else [(0xd398c,0x3c),(0xd39c8,0x28),(0xd3c04,0x5c),(0xd3c60,0x7c),(0xd3cdc,0xb0),(0xd3d8c,0x48),(0xd3f2c,0x2c),(0xd3fd4,0x30),(0xd40f4,0x80),(0xd33e4,0x60)]
    functions=(case owned of
      Just (Aggregate _ _ _) -> [(address at,size) | (at,size)<-mergeSpans (symbolSpans ++ allSymbolSpans ++ helpers)]
      _ -> [(address at,size) | (at,size)<-serviceExtent domain service:helpers]) ++ fixtureFunctions
    (endStatus,stop)=expected env domain service scenario
    excluded=Cpsr : (if endStatus==InstructionBudget then [R0,R1,R2,R3,R4,R5,R6,R7,R8,R9,R10,R11,R12,Sp,Lr,Pc] else if stop/=Nothing then [R1,R2,R3,R4,R5,R6,R7,R8,R9,R10,R11,R12,Sp,Lr] else [R1,R2,R3,R12,Lr])
    initial=map (\(reg,value)->(reg,case reg of R0 -> if coreService service then selector scenario else priority scenario; R1 -> priority scenario; Cpsr -> entryControl scenario .|. (if irqMasked scenario then 0x80 else 0); _ -> value)) (caseInitial spec)
    acquireAt=if domain==DomainA then 0x439d8 else 0xd398c
    releaseAt=if domain==DomainA then 0x43a14 else 0xd39c8
    fixtureCode at words' = mkRegion (RegionId ("code-fixture-"++show at)) (address at) (4*length words') ReadExecute (assemble words')
    fixtureRegions=
      [fixtureCode 0x00101000 [0xe59f2018,0xe5921000,0xe3510000,0x12411001,0x15821000,0x13a00001,0x112fff1e,branchWord (0x00101000+28) acquireAt,0x0096d000] | retryAcquire scenario]
      ++ [mkRegion (RegionId "retry-state") (address 0x0096d000) 4 ReadWrite (word32le 1) | retryAcquire scenario]
      ++ [fixtureCode 0x00102000 [0xe59f1008,0xe3a02001,0xe5c12000,branchWord (0x00102000+12) releaseAt,localTask+0xc] | not (ready scenario)]
    fixtureFunctions=[(address 0x00101000,32) | retryAcquire scenario] ++ [(address 0x00102000,16) | not (ready scenario)]

mergeSpans :: [(Word32, Word32)] -> [(Word32, Word32)]
mergeSpans spans = go (sortOn fst spans)
  where
    go []=[]
    go ((start,size):rest)=extend start (start+size) rest
    extend start end []=[(start,end-start)]
    extend start end ((lo,size):rest)
      | lo<end=extend start (max end (lo+size)) rest
      | otherwise=(start,end-start):extend lo (lo+size) rest

address :: Word32 -> GuestAddress
address=GuestAddress . fromIntegral

valid :: Service -> Scenario -> Bool
valid s c = s==Exit || ((context c/=0)==interruptService s && not (irqMasked c) && (not (rotation s) || (if s==RotateTask then priority c<=16 else priority c-1<16)) && (s/=RotateCore || selector c<3))

expected :: Env -> Domain -> Service -> Scenario -> (RunnerStatus,Maybe GuestAddress)
expected _ domain s c
  | not (valid s c) = (Returned,Nothing)
  | held c = (InstructionBudget,Nothing)
  | s==Exit || (s==Enable && discarded c/=0) = (StopAddress,Just (address (0x43688+delta)))
  | s==Enable && not (selectedQueue c) = (StopAddress,Just (address (0x43640+delta)))
  | rotation s && not (interruptService s) && changes && local = (StopAddress,Just (address (0x43640+delta)))
  | otherwise = (Returned,Nothing)
  where
    delta=if domain==DomainA then 0 else 0x90000
    p=if priority c==0 then 3 else priority c-1
    changes=p==3 && members c>=2 && selectedQueue c && dispatchWord c/=0
    local=not (coreService s) || selector c==0 || selector c==(if domain==DomainA then 1 else 2)

difference :: CaseSpec -> NativeObservation -> NativeObservation -> [KernelDifference]
difference spec a b=compareKernelCase spec {caseRegions=filter compared (caseRegions spec)} a b
  where
    compared (CaseRegion r _) = regionName (regionIdOf r) /= "stack" && take 5 (regionName (regionIdOf r))/="code-"

controlSpec :: Bool -> CaseSpec -> CaseSpec
controlSpec equivalent spec
  | equivalent = spec
      { caseEntry=address wrapper
      , caseFunctions=(address wrapper,4):caseFunctions spec
      , caseRegions=mkRegion (RegionId "code-equivalent") (address wrapper) 4 ReadExecute (word32le branch):caseRegions spec
      }
  | otherwise = spec {caseRegions=map (poke entry (assemble [0xe3a00001,0xe12fff1e])) (caseRegions spec)}
  where
    wrapper=0x00100000
    entry=guestWord (caseEntry spec)
    branch=0xea000000 + ((entry-wrapper-8) `div` 4 `mod` 0x1000000)

contractHolds :: Env -> Domain -> Service -> Scenario -> CaseSpec -> NativeObservation -> Bool
contractHolds env domain service scenario spec observation =
  nativeStatus observation == status && statusResult && effects
  where
    (status,_) = expected env domain service scenario
    reg register = lookupRegister register observation
    result = expectedR0 service scenario status
    statusResult
      | status==Returned = reg R0==Just result
      | status==StopAddress = reg R0==Just result
      | otherwise = True
    words'=domainWords (envLayout env) domain
    readOut :: Word32 -> Maybe Word32
    readOut at = case [(C.baseAddress raw,bytes) | CaseRegion raw _ <- caseRegions spec, (ident,bytes)<-nativeMemory observation, ident==regionIdOf raw, fromIntegral at>=C.baseAddress raw, fromIntegral at+4<=C.baseAddress raw+fromIntegral (B.length bytes)] of
      (base,bytes):_ -> Just (readWordAt bytes (fromIntegral (fromIntegral at-base :: Integer)))
      [] -> Nothing
    effects
      | not (valid service scenario) || status==InstructionBudget = True
      | service==Disable = readOut (wordsDispatchEnabled words')==Just 0
      | service==Enable = readOut (wordsDispatchEnabled words')==Just 1
      | service==Exit = readOut (wordsDispatchEnabled words')==Just (if dispatchWord scenario==0 then 1 else dispatchWord scenario) && exitEffects
      | otherwise = rotationEffects
    layout=envLayout env
    localTask=taskAddress layout domain 2
    byteOut at = (.&. 0xff) <$> readOut at
    exitEffects
      | discarded scenario/=0 = byteOut (localTask+0xc)==Just 1 && byteOut (localTask+0xe)==Just (flags scenario)
      | otherwise = byteOut (localTask+0xc)==Just (if flags scenario .&. 1/=0 then 1 else 0)
          && byteOut (localTask+0xd)==Just 2
          && byteOut (localTask+0xe)==Just (flags scenario .&. (if flags scenario .&. 1/=0 then 0xf8 else 0xf9))
    target = if coreService service && selector scenario/=0 then (if selector scenario==1 then DomainA else DomainB) else domain
    targetWords=domainWords layout target
    task=taskAddress layout target
    n=members scenario
    p=if priority scenario==0 then 3 else priority scenario-1
    rotates=p==3 && n>=2
    changes=rotates && selectedQueue scenario
    requests=changes && dispatchWord scenario/=0
    local=target==domain
    oldSelected=if selectedQueue scenario then (if n==0 then 0 else task 2) else task 5
    newSelected=if changes then task 3 else oldSelected
    queue=if rotates then take (n-1) [3,4]++[2] else take n [2,3,4]
    headAddress=wordsReadyHeads targetWords+3*8
    links=zip3 queue (headAddress:map task queue) (drop 1 (map task queue)++[headAddress])
    rotationEffects =
      readOut (wordsSelected targetWords)==Just newSelected
      && readOut headAddress==Just (case queue of first:_ -> task first; [] -> headAddress)
      && readOut (headAddress+4)==Just (case reverse queue of lastTask:_ -> task lastTask; [] -> headAddress)
      && all (\(slot,previous,next)->readOut (task slot)==Just next && readOut (task slot+4)==Just previous) links
      && readOut (wordsReadyHeads words'-4)==Just (if interruptService service && requests && local then 1 else rescheduleWord scenario)
      && all (\(bank,targetDomain)->readOut bank==Just (if coreService service && requests && not local && target==targetDomain then (1 `shiftL` 24) else 0)) [(0xfffb30cc,DomainA),(0xfffb40cc,DomainB)]

expectedR0 :: Service -> Scenario -> RunnerStatus -> Word32
expectedR0 service scenario status
  | status==StopAddress = fromIntegral (case service of Exit -> -10; Enable -> -92; RotateTask -> -85; _ -> -4 :: Int)
  | valid service scenario || service==RotateCore = 0
  | ((context scenario/=0)/=interruptService service) || irqMasked scenario = fromIntegral (-25 :: Int)
  | otherwise = fromIntegral (-17 :: Int)

branchWord :: Word32 -> Word32 -> Word32
branchWord from to = 0xea000000 + ((to-from-8) `div` 4 `mod` 0x1000000)
