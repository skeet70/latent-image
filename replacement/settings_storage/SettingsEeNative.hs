{-# LANGUAGE GHC2021 #-}
module SettingsEeNative
  ( EeCases(..), EeScenario(..), EeControl(..), EeTraceRefusal(..)
  , eeControlName, eeScenarioName, checkEeTrace, prepareEeCases
  ) where

import Control.Monad (foldM, forM, unless)
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import CustomAdmission (bankEntries)
import Emulation.NativeCase (Region(..))
import Emulation.NativeObservation (GuestAddress(..), RegionId(..), Register(..))
import Emulation.OwnedCase
import qualified Native as N
import Oracle
import PropertyFields (PropertyId(..))
import qualified Replacement.PropertyService.Artifact as Artifact
import Replacement.PropertyService.Classify (Class(..), Property(..), classify)
import qualified Replacement.PropertyService.Oracle as P
import qualified Replacement.RecordStore.Arm as A
import qualified Replacement.RecordStore.Harness as H
import ServiceInventory (Image(..))

data EeScenario = EeEqual | EeDifferent deriving (Eq, Ord, Show, Enum, Bounded)
data EeControl = OmitCache | StoreWrongCache | SwapResets | OmitApplication | OmitFinalSetter | UseVoidResult | UseStaleBacking
  deriving (Eq, Ord, Show, Enum, Bounded)
data EeTraceRefusal = EeTraceShape | EeTraceCalls | EeTraceCache | EeTraceBankOrder deriving (Eq, Show)
data EeCases = EeCases
  { eeStockCases :: [CaseSpec]
  , eeOwnedCases :: [CaseSpec]
  , eeControlCases :: [(EeControl,[CaseSpec])]
  }

eeScenarioName :: EeScenario -> String
eeScenarioName EeEqual = "ee-equal"
eeScenarioName EeDifferent = "ee-different"
eeControlName :: EeControl -> String
eeControlName control = case control of
  OmitCache -> "ee-cache-omitted"
  StoreWrongCache -> "ee-cache-stored-code"
  SwapResets -> "ee-reset-order"
  OmitApplication -> "ee-application-omitted"
  OmitFinalSetter -> "ee-final-setter-omitted"
  UseVoidResult -> "ee-void-result-used"
  UseStaleBacking -> "ee-stale-prehelper-backing"

newCode :: EeScenario -> Word32
newCode EeEqual = 5
newCode EeDifferent = 6

traceBase, observerBase, controlBase, staleBase :: Word32
traceBase = 0x95a000
observerBase = 0x590000
controlBase = 0x591000
staleBase = 0x592000
traceBytes :: Int
traceBytes = 4+9*24

callsBeforeFinal :: EeScenario -> [[Word32]]
callsBeforeFinal EeEqual = []
callsBeforeFinal EeDifferent =
  [[0x634650,0,index,record] | record <- [0,1],index <- [0,8,9]]
  ++ [[0x644d84,0,0,0],[0x644e84,0,0,0]]

checkEeTrace :: EeScenario -> B.ByteString -> Either EeTraceRefusal ()
checkEeTrace scenario bytes = do
  unless (B.length bytes == traceBytes) (Left EeTraceShape)
  let expected=callsBeforeFinal scenario ++ [[0xd4d604,77,0xee,0]]
      count=length expected
      rows=[[readWordAt bytes (4+row*24+column*4) | column <- [0..5]] | row <- [0..count-1]]
      bankCounts=[readWordAt bytes (4+row*24+20) | row <- [0..count-1]]
  unless (readWordAt bytes 0 == fromIntegral count && B.all (==0) (B.drop (4+count*24) bytes)) (Left EeTraceShape)
  unless (map (take 4) rows == expected) (Left EeTraceCalls)
  unless (all (\row -> readWordAt bytes (4+row*24+16) == 30-newCode scenario) [0..count-1]) (Left EeTraceCache)
  unless (case (scenario,bankCounts) of
    (EeEqual,[final]) -> final > 0 && final < 128
    (EeDifferent,[0,0,0,0,0,0,0,first,final]) -> first > 0 && final > first && final < 128
    _ -> False) (Left EeTraceBankOrder)

prepareEeCases :: B.ByteString -> B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> Artifact.LinkedArtifact -> B.ByteString -> M.Map String Symbol -> Either String EeCases
prepareEeCases image image5 loader ranges propertyImage artifact binary symbols = do
  prefix <- imageSlice image 0x61c400 0x61c91c (0x61d59c-0x61c91c)
  dispatcher <- imageSlice image5 0xbf1a00 0xd4fc74 (0xd52684-0xd4fc74)
  division <- imageSlice loader 0x40000 0x4d978 0x114
  rows <- imageSlice image 0x61c400 0x775ca4 0x180
  pointer <- imageSlice image 0x61c400 0x81b454 4
  node <- imageSlice image 0x61c400 0x78cbd0 44
  parent <- imageSlice image 0x61c400 0x7866f8 44
  values <- imageSlice image 0x61c400 0x7909ec 0x100
  unless (readWordAt pointer 0 == 0x775ca4
    && rows == B.concat ([B.concat (map word32le [index,0x7ffffffe,30-index]) | index <- [0..30]] ++ [B.concat (replicate 3 (word32le 0xffff))])
    && map (readWordAt node) [0,8,12,28,32] == [11,0xee,6,0x7866f8,0x7909ec]
    && readWordAt parent 0 == 11) (Left "EE fixture differs from the canonical row table and current-node descriptors")
  resetHead <- imageSlice image 0x61c400 0x634650 4
  unless (readWordAt resetHead 0 == 0xe52db004 && readWordAt dispatcher 0 == 0xe92d49f8
    && readWordAt dispatcher (0xd52680-0xd4fc74) == 0xe8bd89f8) (Left "EE canonical dispatcher or reset publication boundary differs")
  classified <- classify (Image "image4" "" 0x61c400 image)
  let properties=M.fromList [(identifier,property) | property@(Property identifier _ _ _) <- classified]
      owned identifier=case M.lookup (PropertyId identifier) properties of
        Just (Property _ _ _ (Owned field)) -> Right field
        _ -> Left "EE fixture needs an owned property field"
  ee <- owned 0xee
  firstField <- owned 0x115
  secondField <- owned 0x116
  fixture <- foldM (\fx (field,value) -> do
    effect <- maybe (Left "EE seed leaves the admitted property fixture") Right (P.modelEffect fx field (P.Set P.Bank0 (P.SlotArgument 1) (P.SetRaw value)))
    pure fx {P.fixtureBanks=P.effectBanks effect,P.fixturePrevious=P.effectPrevious effect})
    (P.fixture image) [(firstField,1),(secondField,1),(ee,5)]
  let source=N.prepareNativeImage image
      apply state operation=maybe (Left "EE declared service write leaves the admitted bank model") Right (modelOperation (N.backupKeys source) Task state operation)
      entries=[(address,"settings_bank_" ++ role ++ "_entry") | (address,role) <- bankEntries]
      prepared=N.prepareComposedImage propertyImage (N.propertyMetadata artifact) binary symbols entries [fixture]
      (observer,observerSymbols,observerFunctions)=A.assemble observerBase [("reset_resume",0x634654),("set_property",0x61c91c)] observerCode
      resolve name=maybe (Left ("missing EE observer symbol " ++ name)) Right (M.lookup (A.label name) observerSymbols)
  reset <- resolve "fn_ee_reset"
  firstApply <- resolve "fn_ee_apply_first"
  secondApply <- resolve "fn_ee_apply_second"
  notify <- resolve "fn_ee_notify"
  let make own scenario = do
        let operation=P.OracleCase (PropertyId 0xee) (P.Set P.Bank0 (P.SlotArgument 1) (P.SetRaw (newCode scenario)))
            template=N.propertyCase source properties fixture Task operation ee
        backing <- oneRegion "backing" template
        let initial=StorageState fixture backing []
            applied field identifier=ApplyField (P.OracleCase (PropertyId identifier) (P.Set P.Bank0 (P.SlotArgument 1) P.SetOne)) field
        first <- if scenario == EeDifferent then apply initial (applied firstField 0x115) else pure initial
        second <- if scenario == EeDifferent then apply first (applied secondField 0x116) else pure first
        final <- apply second (ApplyField operation ee)
        let count state=fromIntegral (length (storageEvents state))
            counts=case scenario of EeEqual -> [count final]; EeDifferent -> replicate 7 0 ++ [count first,count final]
            trace=encodeTrace scenario counts
            finalFixture=storageFixture final
            initialEe=replace 0x2c (word32le 0) (B.replicate 0x4c 0x6d)
            finalEe=foldl (\bytes offset -> replace offset (word32le 0) bytes)
              (replace 0x30 (word32le (30-newCode scenario)) initialEe) [offset | scenario == EeDifferent,offset <- [0x34,0x38..0x48]]
            menu=replace 8 (word32le (newCode scenario)) (replace 0 (word32le 0x78cbd0) (B.replicate 0x430 0))
            opaque=[0x644d84,0x644e84,0xd4d604]
            stop=opaque ++ map (fromIntegral . guestAddress . fst) (caseFunctions template)
            closure=H.functionClosureExcept stop image ranges [0x61c91c,0x61c93c,0x61d514,0x61d154,0x63398c,0x634650]
            actual=own template
            arguments=[(R0,0xee),(R1,newCode scenario)]
            spec=actual
              { caseId=eeScenarioName scenario
              , caseProvenance="Canonical X-M1 1.50 d4fc74 EE dispatcher, three real 31-row conversions, real cache/RAM publishers, real current-node snapshot, getter and final property/bank path; declared retained-service fixtures"
              , caseExclusion="HOST-only ordinary-node cases. WB mode zero helpers retain ordered service observations and execute declared synthetic property 0x115/0x116 writes of one through the real bank/property path. Their full internal storage/device behavior and M77 notification implementation remain retained. Helper and root r0 are void. Area3 lifecycle uses the existing native matrix boundary. No natural EE or frame-37 reachability claim."
              , caseEntry=GuestAddress 0xd4fc74
              , caseInitial=map (\pair@(register,_) -> maybe pair ((,) register) (lookup register arguments)) (caseInitial actual)
              , caseExpectedRegisters=filter ((/=R0) . fst) (caseExpectedRegisters actual)
              , caseRegions=caseRegions actual ++
                  [ N.region "ee-prefix" 0x61c91c ReadExecute prefix,N.region "ee-dispatcher" 0xd4fc74 ReadExecute dispatcher
                  , N.region "ee-loader-divide" 0x4d978 ReadExecute division,N.region "ee-row-table" 0x775ca4 ReadOnly rows
                  , N.region "ee-row-pointer" 0x81b454 ReadOnly pointer,N.region "ee-node" 0x78cbd0 ReadOnly node
                  , N.region "ee-parent" 0x7866f8 ReadOnly parent,N.region "ee-values" 0x7909ec ReadOnly values
                  , N.region "ee-menu" 0x9571d0 ReadWrite menu,N.region "ee-state" 0x956838 ReadWrite initialEe
                  , N.region "ee-effects" traceBase ReadWrite (B.replicate traceBytes 0),N.region "ee-observer" observerBase ReadExecute observer
                  , N.region "ee-first-entry" 0x644d84 ReadExecute (word32le (N.branch 0x644d84 firstApply))
                  , N.region "ee-second-entry" 0x644e84 ReadExecute (word32le (N.branch 0x644e84 secondApply))
                  , N.region "ee-notify-entry" 0xd4d604 ReadExecute (word32le (N.branch 0xd4d604 notify)) ]
              , caseFunctions=nub (caseFunctions actual ++ [(GuestAddress (fromIntegral address),size) | H.Function address size <- closure]
                  ++ [(GuestAddress 0xd4fc74,0xd52684-0xd4fc74),(GuestAddress 0x4d978,0xf4),(GuestAddress 0x4da6c,0x20)]
                  ++ [(GuestAddress (fromIntegral address),size) | (address,size) <- observerFunctions]
                  ++ [(GuestAddress address,4) | address <- [0x644d84,0x644e84,0xd4d604]])
              , caseExpectedMemory=
                  [ (RegionId "banks",P.fixtureBanks finalFixture)
                  , (RegionId "previous",replace 0x1c8 (word32le 1) (P.fixturePrevious finalFixture))
                  , (RegionId "config",P.fixtureConfig finalFixture),(RegionId "calls",eventBytes (storageEvents final))
                  , (RegionId "backing",storageBacking final),(RegionId "ee-state",finalEe),(RegionId "ee-menu",menu),(RegionId "ee-effects",trace)]
              }
        either (Left . show) Right (checkEeTrace scenario trace)
        patchWord 0x634650 (N.branch 0x634650 reset) spec
  stock <- mapM (make id) [EeEqual,EeDifferent]
  candidate <- mapM (make (N.ownedCase prepared)) [EeEqual,EeDifferent]
  controls <- forM [minBound..maxBound] $ \control -> do
    selected <- mapM (controlCase control) [spec | spec <- candidate,caseId spec == eeScenarioName EeDifferent]
    pure (control,selected)
  pure (EeCases stock candidate controls)

imageSlice :: B.ByteString -> Word32 -> Word32 -> Int -> Either String B.ByteString
imageSlice bytes base address count
  | address >= base && toInteger address+toInteger count <= toInteger base+toInteger (B.length bytes) = Right (B.take count (B.drop (fromIntegral (address-base)) bytes))
  | otherwise = Left "EE canonical source view is truncated"

oneRegion :: String -> CaseSpec -> Either String B.ByteString
oneRegion name spec=case [regionBytes (caseRegion region) | region <- caseRegions spec,N.regionBytesOfId region == RegionId name] of
  [bytes] -> Right bytes
  _ -> Left ("EE fixture requires exactly one " ++ name ++ " region")

encodeTrace :: EeScenario -> [Word32] -> B.ByteString
encodeTrace scenario counts=word32le (fromIntegral (length calls)) <> B.concat
  [B.concat (map word32le (call ++ [30-newCode scenario,count])) | (call,count) <- zip calls counts]
  <> B.replicate (traceBytes-4-length calls*24) 0
 where calls=callsBeforeFinal scenario ++ [[0xd4d604,77,0xee,0]]

observerCode :: [A.Ins]
observerCode =
  [A.at "fn_ee_reset",A.push [0,1,2,3,12,14]] ++ A.load 12 0x634650 ++
  [A.bl "fn_ee_log",A.pop [0,1,2,3,12,14],A.Word 0xe52db004,A.jump "reset_resume"]
  ++ application "fn_ee_apply_first" 0x644d84 0x115 ++ application "fn_ee_apply_second" 0x644e84 0x116
  ++ [A.at "fn_ee_notify",A.mov 2 0 0] ++ A.load 12 0xd4d604 ++ [A.jump "fn_ee_log"]
  ++ [A.at "fn_ee_log",A.push [3,4,5,6,7,14]] ++ A.load 4 traceBase ++
  [ A.ldr 4 5 0,A.lsl 6 5 1,A.addR 6 6 5,A.lsl 6 6 3,A.addR 6 4 6,A.add 6 6 4
  , A.str 6 12 0,A.str 6 0 4,A.str 6 1 8,A.str 6 2 12 ]
  ++ A.load 7 0x956868 ++ [A.ldr 7 7 0,A.str 6 7 16] ++ A.load 7 0x958000 ++
  [A.ldr 7 7 0,A.str 6 7 20,A.add 5 5 1,A.str 4 5 0,A.pop [3,4,5,6,7,15]]
 where
  application name address identifier=[A.at name,A.push [4,14],A.mov 2 0 0] ++ A.load 12 address
    ++ [A.bl "fn_ee_log"] ++ A.load 0 identifier ++ [A.mov 1 0 1,A.mov 2 0 0,A.mov 3 0 0,A.bl "set_property",A.mov 0 0 9,A.pop [4,15]]

patchWord :: Word32 -> Word32 -> CaseSpec -> Either String CaseSpec
patchWord address value spec=case [N.regionBytesOfId region | region@(CaseRegion view ReadExecute) <- caseRegions spec
  ,toInteger address >= baseAddress view,toInteger address+4 <= baseAddress view+toInteger (B.length (regionBytes view))] of
  [owner] -> Right spec {caseRegions=map (patch owner) (caseRegions spec)}
  _ -> Left "EE control instruction is not owned by one executable region"
 where
  patch owner original@(CaseRegion view permission)
    | N.regionBytesOfId original == owner = N.region (regionName owner) (fromInteger (baseAddress view)) permission
        (replace (fromInteger (toInteger address-baseAddress view)) (word32le value) (regionBytes view))
    | otherwise = original

controlCase :: EeControl -> CaseSpec -> Either String CaseSpec
controlCase control spec=case control of
  OmitCache -> patchWord 0x63399c 0xe1a00000 spec
  StoreWrongCache -> patchWord 0xd50c34 0xe1a00008 spec
  SwapResets -> patchWord 0xd50c6c 0xe3a01008 spec >>= patchWord 0xd50c7c 0xe1a01000
  OmitApplication -> patchWord 0xd50cd8 0xe1a00000 spec
  OmitFinalSetter -> patchWord 0xd50cf4 0xe1a00000 spec
  UseVoidResult -> redirect ([A.movR 8 0] ++ finish) []
  UseStaleBacking -> do
    backing <- oneRegion "backing" spec
    redirect ([A.push [0,1,2,3,12,14]] ++ A.load 0 0x409c281c ++ A.load 1 staleBase ++ A.load 2 (fromIntegral (B.length backing))
      ++ [A.bl "copy",A.pop [0,1,2,3,12,14]] ++ finish) [N.region "ee-stale-input" staleBase ReadOnly backing]
 where
  finish=A.load 0 0xee ++ [A.movR 1 8,A.mov 2 0 0,A.jump "final_setter"]
  redirect program extra=do
    let (code,_,spans)=A.assemble controlBase [("copy",0x722054),("final_setter",0xd50cf4)] (A.at "fn_ee_control":program)
    changed <- patchWord 0xd50ce8 (N.branch 0xd50ce8 controlBase) spec
    pure changed {caseRegions=caseRegions changed ++ N.region "ee-control" controlBase ReadExecute code:extra
      ,caseFunctions=caseFunctions changed ++ [(GuestAddress (fromIntegral address),size) | (address,size) <- spans]}
