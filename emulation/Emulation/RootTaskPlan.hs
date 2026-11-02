module Emulation.RootTaskPlan
  ( RootPlan(..), PlanError(..), derivePlan, rootLow, rootHigh, stackLow, stackHigh
  , taskLayout, callbackResources, sourceBodies, sourceWord, declaredBoundary, physical, conditionHolds
  ) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Bits ((.&.), testBit)
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

data PlanError = SourceWordRejected | PublicationTableRejected | ServiceGraphLimitExceeded | ServiceSourceMissing GuestAddress
  | ServiceFootprintRejected GuestAddress FootprintError | UnresolvedServiceTransfer GuestAddress A.Instruction | SiteRejected deriving (Eq, Show)
data RootPlan = RootPlan
  { memoryPlan :: Memory.CapturePlan, tableA :: B.ByteString, tableB :: B.ByteString
  , serviceWords :: S.Set Word32, serviceInstructions :: S.Set Word32, unresolvedTransfers :: S.Set Word32, serviceEntries :: S.Set Word32
  , rootCallMap :: M.Map Word32 Root.CallSite, requiredSites :: S.Set Word32 }
rootLow, rootHigh, stackLow, stackHigh :: Word32
rootLow = 0x69d160
rootHigh = 0x69d2b4
stackLow = 0xa10200
stackHigh = 0xa5b200
physical :: Word32 -> Word32
physical value = value .&. 0x3fffffff
address :: W.Address -> Word32
address (W.Address value) = value
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
  ,(0x5d178+12*103,12),(0x624b4+16*103,16)]
irqSites :: [(Word32,Boundary)]
irqSites = [(0x40204,IrqEntry),(0xd0204,IrqEntry),(0x40234,IrqFrame),(0xd0234,IrqFrame)
  ,(0x40364,IrqReturn),(0xd0364,IrqReturn),(0x437d8,IrqReturn),(0xd37d8,IrqReturn)]
cleanupSites :: [Word32]
cleanupSites = [0x72614c,0x726150,0x72615c,0x726160,0x726198,0x72619c,0x7261a4,0x7261a8
  ,0x7261b8,0x7261bc,0x7261c4,0x7261c8,0x7261cc,0x453f0,0x453f4,0x454e8,0x436e8]
continuations :: [Word32]
continuations = [0x43668,0x4366c,0x43728,0x4372c,0x437a4,0x437a8]
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
footprints :: SourceFiles -> S.Set Word32 -> Either PlanError (S.Set Word32,S.Set Word32,S.Set Word32,S.Set Word32)
footprints files initial = walk (S.toList initial) S.empty S.empty S.empty S.empty
  where
    sources = sourceBodies files (sourceImage files)
    walk [] entries words' instructions unresolved = Right (entries,words',instructions,unresolved)
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
          scheduler = pc `elem` [0x436e8,0x43758,0xd36e8,0xd3758]
          irqReturn = pc `elem` [0x40364,0xd0364,0x437d8,0xd37d8]
          admitted = case instruction of
            A.BranchExchange {} -> True
            A.DataProcessing _ A.MOV _ destination _ (A.ShiftedImmediate _ A.LSL 0) -> destination == A.pc
            A.Load _ _ _ destination base _ _ -> destination == A.pc && (base == A.sp || scheduler)
            A.LoadMultiple _ base _ _ registers -> base == A.sp && A.pc `elem` registers
            A.Rejected A.ExceptionReturn -> irqReturn
            _ -> False
      unless admitted (Left (UnresolvedServiceTransfer (GuestAddress pc) instruction))

derivePlan :: SourceFiles -> Root.RootProof -> B.ByteString -> Either PlanError RootPlan
derivePlan files proof runtime = do
  (primary,secondary) <- publishedTables files
  let sources = sourceBodies files (sourceImage files)
  canonicalCalls <- fmap concat $ mapM (\pc -> do
    raw <- sourceWord sources pc
    pure (case A.decode pc raw of A.Branch _ True destination -> [destination]; _ -> [])) [rootLow,rootLow+4..rootHigh-4]
  let tableTargets body = [U.word32At body offset | offset <- [0,4..92*4]++[0x1618],offset+4 <= B.length body]
      staticSeeds = S.fromList (canonicalCalls++map Root.callTarget (Root.callSites proof)
        ++ [0x69c9f8,0x69cc00,0x69cc18,0x725b74,0x72835c,0x726478,0x725dd8,0x724924,0x72337c
           ,0x4d434,0x41140,0x4d1a4,0x453f0,0x43dd0,0x43688,0x412bc,0x4118c,0x43728]
        ++ filter (/=0) (tableTargets primary++tableTargets secondary))
  (entries,guarded,instructions,unresolved) <- footprints files staticSeeds
  let mutableLoaderCell pc = any (\(base,lastByte) -> pc >= base && pc < lastByte && not (any (\(low,high) -> fromIntegral pc >= low && fromIntegral pc < high) loaderProtected))
        [(0x40000,0x4dae0),(0xd0000,0xddaa0)]
      protected = map spanOf baseImmutable ++ wordSpans (S.filter (not . mutableLoaderCell) guarded)
      writes = [W.WatchClass span' [W.CpuWrite,W.HostWrite,W.ModelBusWrite] | span' <- Memory.unionRanges [[low,high] | (low,high) <- writeSpans]]
      fetched = [W.WatchClass [low,high] [W.CpuFetch] | Memory.PhysicalSpan (PhysicalBoundary low) (PhysicalBoundary high) <- fetchSpans]
      irqCores = M.fromList [(W.Address pc,if pc < 0xd0000 then W.Primary else W.Secondary) | (pc,_) <- irqSites]
      attributed = map spanOf [(0x40000,0x4dae0),(0xd0000,0xddaa0),(0x61c400,0x89ea8c)]
      capture = Memory.CapturePlan (W.Count 80000000) (writes++fetched) attributed protected irqCores
        [Memory.CodeImage (W.Address base) (W.Bytes body) | (base,body) <- sourceBodies files runtime]
      calls = M.fromList [(Root.callPc site,site) | site <- Root.callSites proof]
      required = S.fromList ([rootLow,rootLow+4..rootHigh-4]++[0x725dd8,0x724924,0x725f08,0x725f0c,0x725fa8,0x725fac]
        ++[0x69cc18,0x69cc1c..0x69cc68]++cleanupSites++map fst irqSites)
  pure (RootPlan capture primary secondary guarded (S.union instructions (S.fromList (Root.executionPcs proof))) unresolved entries calls required)

declaredBoundary :: RootPlan -> [(Word32,B.ByteString)] -> Word32 -> Boundary -> Bool
declaredBoundary plan sources location boundary = case boundary of
  Start -> pc == 0x725dd8
  RootEntry -> pc == rootLow
  RootInstruction -> rootLow <= pc && pc < rootHigh
  ServiceEntry -> M.member pc (rootCallMap plan) || S.member pc (serviceEntries plan)
    || pc `elem` cleanupSites || pc == 0x724924 || isIndirect pc
  ServiceReturn -> any (\site -> Root.callPc site+4 == pc) (M.elems (rootCallMap plan))
    || pc `elem` cleanupSites || afterCall pc
  TaskSubmit -> pc == 0x725dd8
  PeriodicCallback -> 0x69cc18 <= pc && pc <= 0x69cc68
  IrqEntry -> lookup pc irqSites == Just IrqEntry
  IrqFrame -> lookup pc irqSites == Just IrqFrame
  IrqReturn -> lookup pc irqSites == Just IrqReturn
  PostCleanup -> pc `elem` continuations || pc `elem` [0x453f0,0x453f4] || afterCall pc || S.member pc (serviceInstructions plan)
  where
    pc = physical location
    isIndirect target = case sourceWord sources target of
      Right raw -> case A.decode target raw of
        A.BranchExchange {} -> True
        A.DataProcessing _ A.MOV _ destination _ _ -> destination == A.pc
        A.Branch _ False destination -> destination == target
        _ -> False
      Left _ -> False
    afterCall target | target < 4 = False
    afterCall target = case sourceWord sources (target-4) of
      Right raw -> case A.decode (target-4) raw of A.Branch _ True _ -> True; A.BranchExchange _ True _ -> True; _ -> False
      Left _ -> False

conditionHolds :: A.Cond -> W.WordValue -> Bool
conditionHolds (A.Cond condition) (W.WordValue flags) = case condition of
  0 -> z; 1 -> not z; 2 -> c; 3 -> not c; 4 -> n; 5 -> not n; 6 -> v; 7 -> not v
  8 -> c && not z; 9 -> not c || z; 10 -> n == v; 11 -> n /= v; 12 -> not z && n == v
  13 -> z || n /= v; 14 -> True; _ -> False
  where n = testBit flags 31; z = testBit flags 30; c = testBit flags 29; v = testBit flags 28
