module Emulation.RootTaskPlan
  ( RootPlan(..), PlanError(..), derivePlan, rootLow, rootHigh, stackLow, stackHigh
  , taskLayout, callbackResources, sourceBodies, sourceWord, labelAgrees, physical, conditionHolds
  , SyncRecords(..), SyncCallback(..), irqNest, irqStack, irqAtcm, irqCallbackDepth, loaderDispatchReturn
  ) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR, testBit)
import qualified Data.ByteString as B
import Data.List (find, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32, Word64)
import Emulation.RootTaskWire
import Emulation.TaskMemoryWire
import qualified Emulation.TaskMemoryWitness as Memory
import qualified Emulation.LoaderCode as Loader
import qualified Emulation.StartupTaskWire as T
import qualified Emulation.PrimaryInitializerSource as Primary
import qualified Emulation.InterfaceInitializer as Secondary
import qualified Emulation.LoaderPrefixWitness as Prefix
import qualified Replacement.RootTask.Admission as Root
import qualified Replacement.KernelInstall.Wire as W
import qualified FujiAF.UpdateContainer as U
import StockInventory (GuestAddress(..), CodeFootprint(..), FootprintError, deriveRetainedFootprint)

data PlanError = SourceWordRejected | PublicationTableRejected | CallbackPublicationRejected GuestAddress
  | ServiceGraphLimitExceeded | ServiceSourceMissing GuestAddress
  | ServiceFootprintRejected GuestAddress FootprintError | UnresolvedServiceTransfer GuestAddress A.Instruction | SiteRejected
  | IrqCallbackSourceRejected GuestAddress deriving (Eq, Show)
data SyncCallback = SyncCallback { callbackCell :: W.Address, callbackTarget :: W.Address } deriving (Eq, Show)
data SyncRecords = SyncRecords
  { syncCore :: T.KernelCore, syncBase :: W.Address, syncCallbacks :: [SyncCallback] } deriving (Eq, Show)
data RootPlan = RootPlan
  { memoryPlan :: Memory.CapturePlan, tableA :: B.ByteString, tableB :: B.ByteString
  , serviceWords :: S.Set Word32, serviceInstructions :: S.Set Word32, unresolvedTransfers :: S.Set Word32
  , rootCallMap :: M.Map Word32 Root.CallSite, requiredSites :: S.Set Word32, syncRecords :: [SyncRecords] }
rootLow, rootHigh, stackLow, stackHigh :: Word32
rootLow = 0x69d160
rootHigh = 0x69d2b4
stackLow = 0xa10200
stackHigh = 0xa5a200
physical :: Word32 -> Word32
physical value = value .&. 0x3fffffff
sourceBodies :: SourceFiles -> B.ByteString -> [(Word32,B.ByteString)]
sourceBodies files image = [(0x40000,sourceA files),(0xd0000,sourceB files),(0x61c400,B.take (0x89ea8c-0x61c400) image)]
sourceWord :: [(Word32,B.ByteString)] -> Word32 -> Either PlanError Word32
sourceWord sources pc = do
  (base,body) <- maybe (Left SourceWordRejected) Right
    (find (\(base,body) -> pc >= base && toInteger pc+4 <= toInteger base+toInteger (B.length body)) sources)
  unless (pc `mod` 4 == 0) (Left SourceWordRejected)
  pure (U.word32At body (fromIntegral (pc-base)))
taskLayout :: T.KernelCore -> (Word32,Word32,Word32,Word32,Word32,Word32,T.TaskId)
taskLayout T.PrimaryKernel = (0x4005beac+40*103,0x4005ff0c+56*103,0x40054760,0x40054768,0xa10a00,0xa10e00,T.TaskId 104)
taskLayout T.SecondaryKernel = (0x400ebe6c+40*102,0x400efecc+56*102,0x400e4720,0x400e4728,0xa10200,0xa10600,T.TaskId 103)
callbackResources :: [(Word32,Int)]
callbackResources = [(0x96ca4c,4),(0x828094,4),(0x8e1f28,4),(0x9c3500,0x404),(0x89ea8c,12)
  ,(0x5d178+12*103,12),(0x624b4+16*103,16),(irqNest,4)]
-- | Loader A 0x400545a4 (literal 0x40418): 0x40240 increments it on IRQ entry and 0x4031c decrements it on exit;
-- boot 0x43848 and the scheduler idle window 0x436f0..0x43720 also hold it at 1.
irqNest :: Word32
irqNest = 0x545a4
irqAtcm :: Word32
irqAtcm = 0x70000011
-- | Core0 ATCM window of 'irqAtcm'; its top is the first-level IRQ stack that 0x40250 loads from 0x40370.
irqStack :: (Word32,Word32)
irqStack = (base,base+(512 `shiftL` fromIntegral ((irqAtcm `shiftR` 2) .&. 31)))
  where base = irqAtcm .&. 0xfffff000
-- | Callback frame plus the 24-byte 0x725b74 and 8-byte 0x412bc frames on its success path.
irqCallbackDepth :: Word32
irqCallbackDepth = 8+32
-- | Return address that dispatcher 0x72337c saves when loader A 0x4bf08 calls it.
loaderDispatchReturn :: Word32
loaderDispatchReturn = 0x4bf0c
-- | Loader A IRQ entry, handler call and exit with their literals, the I-clearing dispatch at 0x4bef0..0x4bf08,
-- dispatcher 0x72337c, the 0x725424 dispatcher selection and table stores, and the callback's frame push and pop.
irqCallbackSource :: [(Word32,[Word32])]
irqCallbackSource =
  [(0x40204,[0xe3a0d0d3,0xe129f00d,0xe92dd00f,0xe3a000d2,0xe129f000,0xe24e0004,0xe14f1000,0xe2012040,0xe3822093,0xe129f002
    ,0xe58d0018,0xe92d0002,0xe59f21dc,0xe5923000,0xe2830001,0xe5820000,0xe3530000,0x1b000002,0xe1a0200d,0xe59fd118,0xe92d0004])
  ,(0x402c0,[0xe59f0158,0xe12fff30,0xe10f2000,0xe3c22080,0xe129f002,0xe3500000,0x11a0e00f,0x11a0f000,0xe10f2000,0xe2022040
    ,0xe3822093,0xe129f002,0xe59f212c,0xe12fff32,0xe59f2118,0xe5921000,0xe2411001,0xe3510000,0x059f010c,0x0b00093a
    ,0xe59f2100,0xe5921000,0xe2413001,0xe5823000,0xe3530000,0x1a000008,0xe8bd0001,0xe1a0d000,0xe59f10f0,0xe5910000
    ,0xe3500000,0x0a000002,0xe3a00000,0xe5810000,0xea000d03,0xe8bd0002,0xe10f2000,0xe2022040,0xe3c11040,0xe1811002
    ,0xe169f001,0xe8fdd00f])
  ,(0x40370,[snd irqStack]),(0x40418,[0x40000000 .|. irqNest]),(0x40420,[0x00040d50,0x00040d60,0x400546d8])
  ,(0x4bef0,[0xe10f3000,0xe3c33080,0xe129f003,0xe5942000,0xe5923008,0xe5920004,0xe12fff33])
  ,(0x72337c,[0xe92d4800,0xe28db004,0xe3033500,0xe344309c,0xe7933100,0xe12fff33,0xe8bd8800])
  ,(0x725464,[0xe3580000,0x0303337c,0x03403072]),(0x725540,[0xe3a02001,0xe5892304])
  ,(0x7255b8,[0xe3033500,0xe344309c,0xe7837104,0xe5932000,0xe0831104,0xe5812100,0xe581a200]),(0x7255f4,[0xe5862000])
  ,(0x69cc18,[0xe92d4800]),(0x69cc68,[0xe8bd8800])]
checkIrqCallbackSource :: [(Word32,B.ByteString)] -> Either PlanError ()
checkIrqCallbackSource sources = do
  mapM_ (\(base,expected) -> mapM_ (\(location,word') -> do
    actual <- sourceWord sources location
    unless (actual == word') (Left (IrqCallbackSourceRejected (GuestAddress location)))) (zip [base,base+4..] expected)) irqCallbackSource
irqSites :: [(Word32,Boundary)]
irqSites = [(0x40204,IrqEntry),(0xd0204,IrqEntry),(0x40234,IrqFrame),(0xd0234,IrqFrame)
  ,(0x40364,IrqReturn),(0xd0364,IrqReturn),(0x437d8,IrqReturn),(0xd37d8,IrqReturn)]
cleanupSites :: [Word32]
cleanupSites = [0x72614c,0x726150,0x72615c,0x726160,0x726198,0x72619c,0x7261a4,0x7261a8
  ,0x7261b8,0x7261bc,0x7261c4,0x7261c8,0x7261cc,0x453f0,0x453f4,0x454e8,0x436e8]
spanOf :: (Word64,Word64) -> Memory.PhysicalSpan
spanOf (low,high) = Memory.PhysicalSpan (PhysicalBoundary low) (PhysicalBoundary high)
fetchSpans :: [Memory.PhysicalSpan]
fetchSpans = map spanOf ([(fromIntegral rootLow,fromIntegral rootHigh),(0x725dd8,0x725ddc),(0x69cc18,0x69cc6c)
  ,(0x7261c8,0x7261d0),(0x436e8,0x436ec),(0x453f0,0x453f8),(0x43668,0x43670),(0x43728,0x43730),(0x437a4,0x437ac)]
  ++ [(fromIntegral pc,fromIntegral pc+4) | (pc,_) <- irqSites])
writeSpans :: [(Word64,Word64)]
writeSpans = [(0x40000,0x4dae0),(0xd0000,0xddaa0),(0x61c400,0x89ea8c)
  ,(0x96bd20,0x96bd28),(0x4dae0,0x4f10c),(0xddaa0,0xdf0cc)
  ,(0x5beac,0x5d16c),(0x5ff0c,0x6194c),(0xebe6c,0xed12c),(0xefecc,0xf190c)
  ,(0x5d178,0x5d718),(0xed138,0xed6d8)
  ,(0x54760,0x54764),(0x54768,0x5476c),(0xe4720,0xe4724),(0xe4728,0xe472c)
  ,(fromIntegral stackLow,fromIntegral stackHigh),(0x93e6f4,0x93e894),(0x7cad9c,0x7caf3c),(0x89ea98,0x89ea9c)]
  ++ [(fromIntegral base,fromIntegral base+fromIntegral size) | (base,size) <- callbackResources]
loaderProtected :: [(Word64,Word64)]
loaderProtected = Loader.loaderSpans 0x40000 0xdae0 ++ Loader.loaderSpans 0xd0000 0xdaa0
baseImmutable :: [(Word64,Word64)]
baseImmutable = loaderProtected ++ [(fromIntegral rootLow,fromIntegral rootHigh),(0x96bd20,0x96bd28)
  ,(0x4dae0,0x4f10c),(0xddaa0,0xdf0cc),(0x7cad9c,0x7caf3c)]
wordSpans :: S.Set Word32 -> [Memory.PhysicalSpan]
wordSpans words' = [spanOf (low,high) | [low,high] <- Memory.unionRanges [[fromIntegral pc,fromIntegral pc+4] | pc <- S.toAscList words']]
publishedTables :: SourceFiles -> Either PlanError (B.ByteString,B.ByteString)
publishedTables files = do
  (a,_) <- either (const (Left PublicationTableRejected)) Right (Primary.publications (sourceA files))
  b <- either (const (Left PublicationTableRejected)) Right (Secondary.originalStores (sourceB files))
  primary <- table 0x4dae0 [(physical guest,value) | Primary.Publication (Primary.Guest guest) value _ <- a]
  secondary <- table 0xddaa0 [(physical guest,value) | Secondary.ExpectedStore (Prefix.Guest guest) value <- b]
  pure (primary,secondary)
  where
    table base stores = do
      unless (length stores == 1419 && map fst (sortOn fst stores) == [base,base+4..base+0x1628]) (Left PublicationTableRejected)
      pure (B.concat [U.putWord32 value | (_,value) <- sortOn fst stores])
sourceWide :: [(Word32,B.ByteString)] -> Word32 -> A.Reg -> Either PlanError Word32
sourceWide sources location register = do
  low <- A.decode location <$> sourceWord sources location
  high <- A.decode (location+4) <$> sourceWord sources (location+4)
  case (low,high) of
    (A.MoveWide (A.Cond 14) destination lower,A.MoveTop (A.Cond 14) upperDestination upper)
      | destination == register && upperDestination == register -> pure (lower .|. (upper `shiftL` 16))
    _ -> Left (CallbackPublicationRejected (GuestAddress location))
sourceSyncRecords :: [(Word32,B.ByteString)] -> Either PlanError [SyncRecords]
sourceSyncRecords sources = mapM derive [(T.PrimaryKernel,0x43ac0,1),(T.SecondaryKernel,0xd3a7c,2)]
  where
    derive (core,entry,owner) = do
      mapM_ (\(offset,expected) -> do
        actual <- sourceWord sources (entry+offset)
        unless (actual == expected) (Left (CallbackPublicationRejected (GuestAddress (entry+offset)))))
        [(8,0xe3a0c001),(0x38,0xe24c3001),(0x3c,0xe0631183),(0x44,0xe0830283),(0x48,0xe0853100)
        ,(0x4c,0xe35c0000 .|. owner),(0x50,0x1a000009),(0x60,0xe583a00c),(0x6c,0xe5831010)
        ,(0x70,0xe583a018),(0x74,0xe583101c),(0x88,0xe583700c),(0x8c,0xe5838010)
        ,(0x90,0xe5837018),(0x94,0xe583801c),(0x170,0xe28cc001),(0x174,0xe35c0003),(0x178,0x1affffae)]
      base <- sourceWide sources (entry+0x14) (A.Reg 5)
      unless (toInteger base+264 <= 0x100000000) (Left (CallbackPublicationRejected (GuestAddress (entry+0x14))))
      acquire <- sourceWide sources (entry+0x1c) (A.Reg 7)
      release <- sourceWide sources (entry+0x24) (A.Reg 8)
      localAcquire <- sourceWide sources (entry+0x30) (A.Reg 10)
      localRelease <- sourceWide sources (entry+0x64) (A.Reg 1)
      unless (acquire == localAcquire && release == localRelease) (Left (CallbackPublicationRejected (GuestAddress entry)))
      pure (SyncRecords core (W.Address base)
        [SyncCallback (W.Address (base+132*domain+offset)) (W.Address target)
        | domain <- [0,1],(offset,target) <- [(12,acquire),(16,release),(24,acquire),(28,release)]])
sourcePublishedEntries :: SourceFiles -> [(Word32,B.ByteString)] -> [SyncRecords] -> Either PlanError (S.Set Word32)
sourcePublishedEntries files sources records = do
  vectors <- mapM (sourceWord sources) [base+0xe0+4*index | base <- [0x40000,0xd0000],index <- [0..6]]
  literals <- mapM (sourceWord sources) [base+offset | base <- [0x40000,0xd0000],offset <- [0x410,0x414,0x420,0x424]]
  callbacks <- mapM (\(location,register) -> sourceWide sources location (A.Reg register))
    [(0x42884,4),(0x4290c,5),(0x40da4,3),(0x40b7c,3),(0x43f60,3),(0x4be50,2)
    ,(0xd2884,4),(0xd290c,5),(0xd0da4,3),(0xd0b7c,3),(0xd3f1c,3),(0xdbe10,2)]
  initialCallbacks <- mapM (sourceWord [(0x61c400,sourceImage files)]) [0x89ea8c,0x89ea94]
  let synchronization = [target | record <- records,SyncCallback _ (W.Address target) <- syncCallbacks record]
      entries = S.fromList (vectors++literals++callbacks++synchronization++initialCallbacks)
  mapM_ (\entry -> do
    unless (entry `mod` 4 == 0) (Left (CallbackPublicationRejected (GuestAddress entry)))
    _ <- sourceWord sources entry
    pure ()) (S.toList entries)
  pure entries
footprints :: SourceFiles -> S.Set Word32 -> Either PlanError (S.Set Word32,S.Set Word32,S.Set Word32)
footprints files initial = walk (S.toList initial) S.empty S.empty S.empty S.empty
  where
    sources = sourceBodies files (sourceImage files)
    walk [] _ words' instructions unresolved = Right (words',instructions,unresolved)
    walk (entry:pending) entries words' instructions unresolved
      | S.member entry entries || rootLow <= entry && entry < rootHigh = walk pending entries words' instructions unresolved
      | S.size entries >= 2048 = Left ServiceGraphLimitExceeded
      | otherwise = do
          (base,body) <- maybe (Left (ServiceSourceMissing (GuestAddress entry))) Right
            (find (\(base,body) -> entry >= base && toInteger entry+4 <= toInteger base+toInteger (B.length body)) sources)
          footprint <- either (Left . ServiceFootprintRejected (GuestAddress entry)) Right
            (deriveRetainedFootprint (GuestAddress base) body (GuestAddress entry))
          let code = S.fromList [pc | GuestAddress pc <- footprintInstructions footprint]
              literals = S.fromList [pc | GuestAddress pc <- footprintLiterals footprint]
              transfers = S.fromList [pc | GuestAddress pc <- footprintUnresolvedTransfers footprint]
          mapM_ checkTransfer (S.toList transfers)
          let next = [physical target | GuestAddress target <- footprintDependencies footprint]
          walk (next++pending) (S.insert entry entries) (S.unions [words',code,literals]) (S.union instructions code) (S.union unresolved transfers)
    checkTransfer pc = do
      raw <- sourceWord sources pc
      let instruction = A.decode pc raw
          dispatch = lookup pc [(0x436e8,(A.Reg 1,A.OffsetImmediate 28)),(0x43758,(A.Reg 2,A.OffsetImmediate 8))
            ,(0xd36e8,(A.Reg 1,A.OffsetImmediate 28)),(0xd3758,(A.Reg 2,A.OffsetImmediate 8))]
          irqReturn = pc `elem` [0x40364,0xd0364,0x437d8,0xd37d8]
          vectorReturn = lookup pc [(0x40124,0xe1b0f00e),(0x401b0,0xe8fd9000),(0x40200,0xe8fd9000),(0x40d38,0xe1b0f00e)
            ,(0xd0124,0xe1b0f00e),(0xd01b0,0xe8fd9000),(0xd0200,0xe8fd9000),(0xd0d38,0xe1b0f00e)] == Just raw
          admitted = case instruction of
            A.BranchExchange {} -> True
            A.DataProcessing _ A.MOV _ destination _ (A.ShiftedImmediate _ A.LSL 0) -> destination == A.pc
            A.Load _ A.Word False destination base offset A.OffsetOnly -> destination == A.pc && dispatch == Just (base,offset)
            A.LoadMultiple _ base _ _ registers -> base == A.sp && A.pc `elem` registers
            A.Rejected A.ExceptionReturn -> irqReturn || vectorReturn
            _ -> False
      unless admitted (Left (UnresolvedServiceTransfer (GuestAddress pc) instruction))

derivePlan :: SourceFiles -> Root.RootProof -> B.ByteString -> Either PlanError RootPlan
derivePlan files proof runtime = do
  (primary,secondary) <- publishedTables files
  let sources = sourceBodies files (sourceImage files)
  records <- sourceSyncRecords sources
  checkIrqCallbackSource sources
  publications <- sourcePublishedEntries files sources records
  canonicalCalls <- fmap concat $ mapM (\pc -> do
    raw <- sourceWord sources pc
    pure (case A.decode pc raw of A.Branch _ True destination -> [destination]; _ -> [])) [rootLow,rootLow+4..rootHigh-4]
  let tableTargets body = [U.word32At body offset | offset <- [0,4..92*4]++[0x1618,0x161c,0x1620,0x1624],offset+4 <= B.length body]
      staticSeeds = S.fromList (canonicalCalls++map Root.callTarget (Root.callSites proof)
        ++ [0x69c9f8,0x69cc00,0x69cc18,0x725b74,0x72835c,0x726478,0x725dd8,0x724924,0x72337c
           ,0x4d434,0x41140,0x4d1a4,0x453f0,0x43dd0,0x43688,0x412bc,0x4118c,0x43728]
        ++ filter (/=0) (tableTargets primary++tableTargets secondary) ++ S.toList publications)
  (guarded,instructions,unresolved) <- footprints files staticSeeds
  let mutableLoaderCell pc = any (\(base,lastByte) -> pc >= base && pc < lastByte && not (any (\(low,high) -> fromIntegral pc >= low && fromIntegral pc < high) loaderProtected))
        [(0x40000,0x4dae0),(0xd0000,0xddaa0)]
      callbackCells = [physical cell | record <- records,SyncCallback (W.Address cell) _ <- syncCallbacks record]
      callbackProtected = [(fromIntegral cell,fromIntegral cell+4) | cell <- callbackCells]
      recordWrites = [(fromIntegral (physical base),fromIntegral (physical base)+264) | SyncRecords _ (W.Address base) _ <- records]
      protected = map spanOf (baseImmutable++callbackProtected) ++ wordSpans (S.filter (not . mutableLoaderCell) guarded)
      writes = [W.WatchClass span' [W.CpuWrite,W.HostWrite,W.ModelBusWrite] | span' <- Memory.unionRanges [[low,high] | (low,high) <- writeSpans++recordWrites]]
      fetched = [W.WatchClass [low,high] [W.CpuFetch] | Memory.PhysicalSpan (PhysicalBoundary low) (PhysicalBoundary high) <- fetchSpans]
      irqCores = M.fromList [(W.Address pc,if pc < 0xd0000 then W.Primary else W.Secondary) | (pc,_) <- irqSites]
      attributed = map spanOf [(0x40000,0x4dae0),(0xd0000,0xddaa0),(0x61c400,0x89ea8c)]
      capture = Memory.CapturePlan (W.Count 80000000) (writes++fetched) attributed protected irqCores
        [Memory.CodeImage (W.Address base) (W.Bytes body) | (base,body) <- sourceBodies files runtime]
      calls = M.fromList [(Root.callPc site,site) | site <- Root.callSites proof]
      required = S.fromList ([rootLow,rootLow+4..rootHigh-4]++[0x725dd8,0x724924,0x725f08,0x725f0c,0x725fa8,0x725fac]
        ++[0x69cc18,0x69cc1c..0x69cc68]++cleanupSites++map fst irqSites)
  pure (RootPlan capture primary secondary guarded (S.union instructions (S.fromList (Root.executionPcs proof))) unresolved calls required records)

-- | The boundary class a PC alone determines: the task-submit wrapper and the loader IRQ entry, frame and return points.
pcBoundary :: Word32 -> Maybe Boundary
pcBoundary location
  | physical location == 0x725dd8 = Just TaskSubmit
  | otherwise = lookup (physical location) irqSites
-- | Whether a producer label is admitted at a PC, `first` marking sample 0. A PC-determined class must match exactly;
-- elsewhere the label names the producer's dynamic role assignment and only may not claim a PC-determined class.
labelAgrees :: Bool -> Word32 -> Boundary -> Bool
labelAgrees first location label
  | first = label == Start && physical location == 0x725dd8
  | otherwise = case pcBoundary location of
      Just expected -> label == expected
      Nothing -> label `notElem` [Start,TaskSubmit,IrqEntry,IrqFrame,IrqReturn]

conditionHolds :: A.Cond -> W.WordValue -> Bool
conditionHolds (A.Cond condition) (W.WordValue flags) = case condition of
  0 -> z; 1 -> not z; 2 -> c; 3 -> not c; 4 -> n; 5 -> not n; 6 -> v; 7 -> not v
  8 -> c && not z; 9 -> not c || z; 10 -> n == v; 11 -> n /= v; 12 -> not z && n == v
  13 -> z || n /= v; 14 -> True; _ -> False
  where n = testBit flags 31; z = testBit flags 30; c = testBit flags 29; v = testBit flags 28
