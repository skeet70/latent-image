{-# LANGUAGE GHC2021 #-}
module CustomPreemptionNative
  ( PreemptionCases(..), preparePreemptionCases, preparePreemptionControl ) where

import qualified CustomNative as N
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeCase (Region(..))
import Emulation.NativeObservation (GuestAddress(..), RegionId(..), regionIdOf)
import Emulation.OwnedCase (CaseSpec(..), CaseRegion(..), Permission(..), Symbol(..), mkRegion, readWordAt, word32le)
import qualified Replacement.RecordStore.Arm as A
import qualified Replacement.RecordStore.Cases as C
import Replacement.RecordStore.Hardware (hardware)
import qualified Replacement.RecordStore.Harness as H
import qualified Replacement.RecordStore.Model as R

data PreemptionCases = PreemptionCases
  { preemptionStockCases :: [CaseSpec]
  , preemptionOwnedCases :: [CaseSpec]
  }

data PreemptionPoint = OnUnlock | OnCallback deriving (Eq, Show)

preparePreemptionCases :: B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> Either String PreemptionCases
preparePreemptionCases image loader ranges linked symbols = do
  stock <- mapM (prepare image loader ranges Nothing) [OnUnlock,OnCallback]
  owned <- mapM (prepare image loader ranges (Just (linked,symbols))) [OnUnlock,OnCallback]
  pure (PreemptionCases stock owned)

preparePreemptionControl :: B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> M.Map String Symbol -> Either String [CaseSpec]
preparePreemptionControl image loader ranges linked symbols =
  mapM (prepare image loader ranges (Just (linked,symbols))) [OnUnlock]

hookBase, otherOutput, directOutput :: Word32
hookBase = 0x520000
otherOutput = C.callerBase + 0x1000
directOutput = C.callerBase + 0x1100

arm :: [A.Ins]
arm = A.load 12 (N.probeBase+4) ++ [A.mov 0 0 1,A.str 12 0 0]

region :: String -> Word32 -> Permission -> B.ByteString -> CaseRegion
region name base permission bytes = mkRegion (RegionId name) (GuestAddress (fromIntegral base)) (B.length bytes) permission bytes

prepare :: B.ByteString -> B.ByteString -> [H.Function] -> Maybe (B.ByteString,M.Map String Symbol) -> PreemptionPoint -> Either String CaseSpec
prepare image loader ranges artifact point = do
  let geometry = R.geometry R.Kind3
      label = case point of OnUnlock -> "cross-kind-preempt-unlock"; OnCallback -> "cross-kind-preempt-callback"
      input = N.CustomCase (C.CaseName label) N.ValidRecords N.Preserve N.ZeroConfiguration [] R.NoFault False
      initial = N.stockCase image loader ranges input
      other = B.pack [7,0,0xb6,0x5a]
      otherSecond = B.pack [9,0,0xa4,0x7c]
      record sequenceNumber = snd (hardware R.Prepare R.Hardware3 (R.replace 4 (B.singleton (fromIntegral sequenceNumber)) (N.payloadRecord sequenceNumber)))
      initialMedium = R.replace (6 * R.blockBytes) (B.concat [record sequenceNumber | sequenceNumber <- [0..126]]) (R.replace R.blockBytes otherSecond (R.replace 0 other (B.replicate (8 * R.blockBytes) 0xff)))
      physicalMedium = R.Medium (M.fromList [(R.PoolBlock (0x52 + fromIntegral index),B.take R.blockBytes (B.drop (index * R.blockBytes) initialMedium)) | index <- [0..7]])
      fixture = C.OracleCase (C.CaseName label) R.Kind3 physicalMedium [C.Open,C.Register C.Normal] R.Hardware3 R.NoFault
      worldFixture = fixture {C.operations = C.operations fixture ++ [C.On R.Kind1 (C.ReadAt (R.BytePosition 0) 4),C.On R.Kind1 (C.ReadAt (R.BytePosition 0x20000) 4)]}
      (worldModel,worldSymbols,_) = A.assemble H.modelBase [] (H.mediumCode worldFixture)
      (originalModel,_,_) = A.assemble H.modelBase [] (H.mediumCode fixture)
  if B.length worldModel /= B.length originalModel then Left "preemption medium-model relocation changes retained seam addresses" else pure ()
  copy <- maybe (Left "missing preemption medium copy function") Right (M.lookup (A.label "fn_copy") worldSymbols)
  let driverSeeds = [("record-open",0x6ac2b4),("custom-boot",0x6a8abc),("custom-read",0x6a865c),("custom-refresh",0x6a86c4),("record-read-at",0x6acda0),("fn_copy",copy)]
      (driver,_,driverFunctions) = A.assemble C.driverBase driverSeeds driverCode
      modified = R.replace 0 (R.half 127) (R.replace 3 (B.singleton 0x71) (record 126))
      expectedRecord = snd (hardware R.Prepare R.Hardware3 modified)
      expectedProgram = expectedRecord <> B.replicate (0x600-52) 0xff
      expectedCaller = R.replace 0 expectedRecord $ R.replace 0x40 (B.pack [0,0]) $ R.replace 0x100 expectedProgram $ R.replace 0x1000 (other <> otherSecond) $ R.replace 0x1100 (B.take 4 (record 0)) (B.take 0x1104 C.initialCaller)
      expectedRequests = C.encodeRequest (R.Program (R.PoolBlock 0x59) 0 expectedProgram) <> B.replicate (C.requestSize-C.requestStride) 0
      expectedMedium = R.replace (7 * R.blockBytes) expectedProgram initialMedium
      expectedProbes = B.concat (map word32le [0,0,1,0,0,0])
      patch original
        | name == "driver" = region name C.driverBase ReadExecute (driver <> B.replicate (fromIntegral (H.modelBase-C.driverBase)-B.length driver) 0 <> worldModel <> B.drop (B.length worldModel) (B.drop (fromIntegral (H.modelBase-C.driverBase)) bytes))
        | name == "record-ram" = region name 0x89f8b0 ReadWrite (R.replace (fromIntegral (R.stagingAddress geometry-0x89f8b0)) (B.replicate 0x600 0xff) bytes)
        | name == "medium" = region name C.mediumBase ReadWrite initialMedium
        | otherwise = original
        where name = regionName (regionIdOf (caseRegion original)); bytes = regionBytes (caseRegion original)
      extraClosure = [(GuestAddress (fromIntegral address),size) | H.Function address size <- H.functionClosureExcept [0x724b68,0x724c00,0x42634,0x4263c] image ranges [0x6acda0]]
      prepared = initial
        { caseRegions = map patch (caseRegions initial)
        , caseFunctions = nub ([(GuestAddress (fromIntegral address),size) | (address,size) <- driverFunctions] ++ extraClosure ++ [(address,size) | (address,size) <- caseFunctions initial,guestAddress address < fromIntegral C.driverBase || guestAddress address >= fromIntegral H.modelBase])
        , caseExpectedMemory = [(RegionId "caller",expectedCaller),(RegionId "medium",expectedMedium),(RegionId "requests",expectedRequests),(RegionId "results",word32le 1),(RegionId "custom-probes",expectedProbes),(RegionId "custom-live",B.take 40 (B.drop 4 expectedRecord))]
        }
  candidate <- case artifact of Nothing -> Right prepared; Just (linked,symbols) -> N.candidateCase 0x5d0000 linked symbols prepared
  checker <- case artifact of
    Nothing -> Right 0x6a84d4
    Just (_,symbols) -> case M.lookup "settings_custom_codec_check" symbols of
      Just (Symbol address size) | size >= 8 -> Right address
      _ -> Left "missing custom codec check callback for callback preemption"
  let bodyWords = [readWordAt (regionBytes view) (fromInteger (toInteger checker-baseAddress view)) | CaseRegion view _ <- caseRegions candidate,toInteger checker >= baseAddress view,toInteger checker+4 <= baseAddress view+toInteger (B.length (regionBytes view))]
  prologue <- case bodyWords of [word] -> Right word; _ -> Left "custom check function is not in exactly one executable region"
  continuation <- if prologue .&. 0xffff0000 == 0xe92d0000
    then Right [A.Word prologue,A.jump "check-resume"]
    else if prologue .&. 0xff000000 == 0xea000000
      then Right [A.jump "check-tail"]
      else if prologue .&. 0x0fff0ff0 == 0x01a00000 && (prologue `shiftR` 12) .&. 0xf /= 15 && prologue .&. 0xf /= 15
        then Right [A.Word prologue,A.jump "check-resume"]
        else Left "callback preemption requires a relocatable push, unconditional branch or register-move prologue"
  let displacement = ((prologue .&. 0xffffff) `shiftL` 2)
      signedDisplacement = if displacement .&. 0x2000000 /= 0 then displacement .|. 0xfc000000 else displacement
      checkTail = checker + 8 + signedDisplacement
      seeds = [("record-read-at",0x6acda0),("check-resume",checker+4),("check-tail",checkTail)]
      (hooks,hookSymbols,hookFunctions) = A.assemble hookBase seeds (hookCode point continuation)
      resolve name = M.findWithDefault (error ("missing preemption hook " ++ name)) (A.label name) hookSymbols
      redirect = if point == OnCallback then [(checker,resolve "fn_preempt_callback")] else []
      patchCallback original@(CaseRegion view permission) = case [(address,target) | (address,target) <- redirect,toInteger address >= baseAddress view,toInteger address+4 <= baseAddress view+toInteger (B.length (regionBytes view))] of
        [] -> original
        [(address,target)] -> region (regionName (regionIdOf view)) (fromInteger (baseAddress view)) permission (R.replace (fromInteger (toInteger address-baseAddress view)) (word32le (N.branchWord address target)) (regionBytes view))
        _ -> error "overlapping callback redirects"
      semSites = [(0x724b68,"fn_sem_take"),(0x724c00,"fn_sem_give"),(0x42634,"fn_other_semaphore"),(0x4263c,"fn_other_semaphore")]
      semRegions = [region ("preempt-semaphore-" ++ show address) address ReadExecute (word32le (N.branchWord address (resolve target))) | (address,target) <- semSites]
      removed = map fst semSites
      functions = [(address,size) | (address,size) <- caseFunctions candidate,fromIntegral (guestAddress address) `notElem` removed]
  pure candidate
    { caseRegions = map patchCallback (caseRegions candidate) ++ semRegions ++ [region "preempt-hooks" hookBase ReadExecute hooks]
    , caseFunctions = nub (functions ++ [(GuestAddress (fromIntegral address),size) | (address,size) <- hookFunctions] ++ [(GuestAddress (fromIntegral address),4) | (address,_) <- semSites])
    }
  where
    driverCode = [A.at "fn_preempt_driver",A.push [4,14],A.mov 0 0 0,A.bl "record-open",A.bl "custom-boot"]
      ++ A.load 12 0x89f877 ++ [A.mov 0 0 0x71,A.strb 12 0 0,A.bl "custom-refresh"]
      ++ (case point of OnUnlock -> arm; OnCallback -> [])
      ++ A.load 0 3 ++ [A.mov 1 0 0,A.mov 2 0 4] ++ A.load 3 directOutput ++ [A.bl "record-read-at"]
      ++ (case point of OnCallback -> arm; OnUnlock -> [])
      ++ A.load 0 C.callerBase ++ A.load 1 (C.callerBase+0x40) ++ [A.bl "custom-read"]
      ++ A.load 0 C.cellsBase ++ [A.ldr 0 1 0] ++ A.load 12 C.resultBase ++ [A.str 12 1 0]
      ++ A.load 0 (C.callerBase+0x100) ++ A.load 1 (R.stagingAddress (R.geometry R.Kind3)) ++ A.load 2 0x600 ++ [A.bl "fn_copy",A.pop [4,15]]

hookCode :: PreemptionPoint -> [A.Ins] -> [A.Ins]
hookCode point continuation =
  [A.at "fn_sem_take",A.push [12,14],A.cmp 0 0 0x4b,A.whenNe "take_done"] ++ A.load 12 N.probeBase
  ++ [A.ldr 12 0 0,A.cmp 0 0 0,A.whenNe "take_bad",A.mov 0 0 1,A.str 12 0 0,A.at "take_done",A.mov 0 0 0,A.pop [12,15],A.at "take_bad",A.mov 0 0 1,A.str 12 0 12]
  ++ A.load 0 0xffffffff ++ [A.pop [12,15],A.at "fn_sem_give",A.push [0,1,2,3,12,14],A.cmp 0 0 0x4b,A.whenNe "give_done"] ++ A.load 12 N.probeBase
  ++ [A.ldr 12 0 0,A.cmp 0 0 1,A.whenNe "give_bad",A.mov 0 0 0,A.str 12 0 0]
  ++ (case point of OnUnlock -> [A.bl "fn_preempt"]; OnCallback -> [])
  ++ [A.at "give_done",A.pop [0,1,2,3,12,14],A.mov 0 0 0,A.ret,A.at "give_bad",A.mov 0 0 1,A.str 12 0 12,A.pop [0,1,2,3,12,14]]
  ++ A.load 0 0xffffffff ++ [A.ret,A.at "fn_other_semaphore",A.mov 0 0 0,A.ret,A.at "fn_preempt",A.push [0,1,2,3,12,14]] ++ A.load 12 N.probeBase
  ++ [A.ldr 12 0 4,A.cmp 0 0 0,A.whenEq "preempt_done",A.mov 0 0 0,A.str 12 0 4,A.ldr 12 0 8,A.add 0 0 1,A.str 12 0 8,A.mov 0 0 1,A.mov 1 0 0,A.mov 2 0 4]
  ++ A.load 3 otherOutput ++ [A.bl "record-read-at"] ++ A.load 12 N.probeBase
  ++ [A.str 12 0 16,A.mov 0 0 1] ++ A.load 1 0x20000 ++ [A.mov 2 0 4]
  ++ A.load 3 (otherOutput+4) ++ [A.bl "record-read-at"] ++ A.load 12 N.probeBase
  ++ [A.str 12 0 20,A.at "preempt_done",A.pop [0,1,2,3,12,14],A.ret,A.at "fn_preempt_callback",A.push [0,1,2,3,12,14]] ++ A.load 12 N.probeBase
  ++ [A.ldr 12 0 0,A.cmp 0 0 0,A.whenEq "callback_unlocked",A.mov 0 0 1,A.str 12 0 12,A.at "callback_unlocked",A.bl "fn_preempt",A.pop [0,1,2,3,12,14]] ++ continuation
