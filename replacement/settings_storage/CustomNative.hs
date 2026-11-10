{-# LANGUAGE GHC2021 #-}
module CustomNative where

import CustomAdmission (customEntries, recordEntries)
import Control.Monad (unless)
import Data.Bits (bit, xor, shiftR, complement, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Emulation.NativeCase (Region(..))
import Emulation.NativeObservation
import Emulation.OwnedCase (CaseId(..), CaseSpec(..), CaseRegion(..), Permission(..), Symbol(..), mkRegion, readWordAt, word32le)
import qualified Replacement.RecordStore.Arm as A
import qualified Replacement.RecordStore.Cases as C
import Replacement.RecordStore.Hardware (hardware)
import qualified Replacement.RecordStore.Harness as H
import qualified Replacement.RecordStore.Model as R

newtype RecordOffset = RecordOffset Int deriving (Eq, Show)
newtype RecordByte = RecordByte Word8 deriving (Eq, Show)
data Step = Boot | Read | ReadWithoutRecovery | Refresh | Defaults | ChangeWorking RecordOffset RecordByte deriving (Eq, Show)
newtype RowParityBit = RowParityBit Int deriving (Eq, Show)
data InputRecords = ValidRecords | CorrectableRecord | ErasedRecords | RejectedRecords | NewerRejectedRecords | HighParityRecord RowParityBit | NewerHighParityRecord RowParityBit | BoundaryProbeRecords | FullRecords deriving (Eq, Show)
data ResetFlag = Preserve | Reset deriving (Eq, Show)
data Configuration = ZeroConfiguration | MaximumConfiguration | SignedConfiguration deriving (Eq, Show)

configurationTag :: Configuration -> String
configurationTag config = case config of
  ZeroConfiguration -> "zero"
  MaximumConfiguration -> "maximum"
  SignedConfiguration -> "signed"

data CustomCase = CustomCase
  { customName :: C.CaseName
  , inputRecords :: InputRecords
  , resetFlag :: ResetFlag
  , configuration :: Configuration
  , customSteps :: [Step]
  , customFault :: R.Fault
  , noPersistence :: Bool
  } deriving (Eq, Show)

customCases :: [CustomCase]
customCases =
  [ make "boot-valid" ValidRecords [Boot,Read] True
  , make "boot-corrected" CorrectableRecord [Boot,Read] True
  , make "boot-empty" ErasedRecords [Boot,Read] False
  , make "boot-none-valid" RejectedRecords [Boot,Read] False
  , make "newer-rejected-recovery-output" NewerRejectedRecords [Boot,Read] True
  , make "recovery-boundary-probe" BoundaryProbeRecords [Boot,Read] True
  , make "recovery-boundary-null" BoundaryProbeRecords [Boot,ReadWithoutRecovery] True
  , make "sequence-reload" ValidRecords [Boot,ChangeWorking (RecordOffset 3) (RecordByte 0x71),Refresh,Read,Refresh,Read] False
  , (make "program-failure-reread" ValidRecords [Boot,ChangeWorking (RecordOffset 3) (RecordByte 0x71),Refresh,Read] False) { customFault = R.FailRequest (R.PersistenceStep 0) }
  , (make "destination-erase-failure" FullRecords [Boot,ChangeWorking (RecordOffset 3) (RecordByte 0x71),Refresh,Read] False) { customFault = R.FailRequest (R.PersistenceStep 0) }
  ]
  ++ [make ("row-parity-high-bit-" ++ show parityBit) (HighParityRecord (RowParityBit parityBit)) [Boot,Read] False | parityBit <- [11..31]]
  ++ [make ("newer-high-row-parity-bit-" ++ show parityBit) (NewerHighParityRecord (RowParityBit parityBit)) [Boot,Read] True | parityBit <- [11..31]]
  ++ [make ("refresh-compared-byte-" ++ show offset) ValidRecords [Boot,ChangeWorking (RecordOffset offset) (RecordByte 0x71),Refresh,Read] False | offset <- [2..43]]
  ++ [make ("refresh-ignores-byte-" ++ show offset) ValidRecords [Boot,ChangeWorking (RecordOffset offset) (RecordByte 0x71),Refresh,Read] True | offset <- [0,1] ++ [44..51]]
  ++ [(make ("reset-defaults-" ++ configurationTag config) ValidRecords [Boot,Read] False) { resetFlag = Reset, configuration = config } | config <- [ZeroConfiguration,MaximumConfiguration,SignedConfiguration]]
  ++ [(make ("explicit-defaults-" ++ configurationTag config) ValidRecords [Boot,Defaults] True) { configuration = config } | config <- [ZeroConfiguration,MaximumConfiguration,SignedConfiguration]]
  where
    make name records steps unchanged = CustomCase (C.CaseName name) records Preserve ZeroConfiguration steps R.NoFault unchanged

payloadRecord :: Word32 -> B.ByteString
payloadRecord sequenceNumber = snd (hardware R.Prepare R.Hardware3 (R.half (fromIntegral sequenceNumber) <> B.pack [fromIntegral (offset + 0x20) | offset <- [2..51::Int]]))

initialBanks :: InputRecords -> (B.ByteString, B.ByteString)
initialBanks input = case input of
  ValidRecords -> (padded (payloadRecord 7), erased)
  CorrectableRecord -> (padded (flipByte 18 8 (payloadRecord 7)), erased)
  ErasedRecords -> (erased,erased)
  RejectedRecords -> (padded (flipByte 44 3 (payloadRecord 7)),erased)
  NewerRejectedRecords -> (padded (payloadRecord 7 <> flipByte 44 3 (payloadRecord 8)),erased)
  HighParityRecord (RowParityBit parityBit) -> (padded (highParity parityBit (payloadRecord 7)),erased)
  NewerHighParityRecord (RowParityBit parityBit) -> (padded (payloadRecord 7 <> highParity parityBit (payloadRecord 8)),erased)
  BoundaryProbeRecords -> (padded (B.concat [payloadRecord n | n <- [0..126]]), padded (flipByte 44 3 (payloadRecord 127)))
  FullRecords -> (padded (B.concat [payloadRecord n | n <- [0..126]]), padded (B.concat [payloadRecord n | n <- [127..253]]))
  where
    erased = B.replicate R.blockBytes 0xff
    padded bytes = bytes <> B.replicate (R.blockBytes - B.length bytes) 0xff
    flipByte offset mask bytes = R.replace offset (B.singleton (B.index bytes offset `xor` mask)) bytes
    highParity parityBit = flipByte (44 + parityBit `div` 8) (bit (parityBit `mod` 8))

configBase, lifecycleBase, liveBase, probeBase :: Word32
configBase = 0x36000000
lifecycleBase = 0x89f830
liveBase = 0x409c295c
probeBase = 0x37000000

checkObserverBase :: Word32
checkObserverBase = 0x5a0000

checkObserver :: Word32 -> [A.Ins] -> (B.ByteString,[(Word32,Word32)])
checkObserver resume tailCode = (bytes,functions)
  where
    (bytes,_,functions) = A.assemble checkObserverBase [("check_resume",resume)]
      ([A.at "fn_custom_check_trace",A.push [0,1,2,3,12,14]] ++ A.load 12 (probeBase+0x2000)
      ++ [A.ldr 12 2 0,A.add 3 2 1,A.str 12 3 0,A.add 2 2 1,A.lsl 2 2 2,A.addR 12 12 2
         ,A.ldrb 0 0 0,A.str 12 0 0,A.pop [0,1,2,3,12,14]] ++ tailCode)

configurationBytes :: Configuration -> B.ByteString
configurationBytes config = foldl (\bytes (offset,value) -> R.replace offset (B.singleton value) bytes) (B.replicate 0x240 0) (zip [0x217,0x218,0x219,0x208,0x209,0x20a] values)
  where
    values = case config of
      ZeroConfiguration -> replicate 6 0
      MaximumConfiguration -> [3,1,3,2,2,2]
      SignedConfiguration -> replicate 6 0xff

branchWord :: Word32 -> Word32 -> Word32
branchWord site target = 0xea000000 .|. ((target - site - 8) `shiftR` 2 .&. 0xffffff)

stockCase :: B.ByteString -> B.ByteString -> [H.Function] -> CustomCase -> CaseSpec
stockCase image loader ranges c = template
  { caseId = C.caseName (customName c)
  , caseProvenance = "X-M1 1.50 custom settings lifecycle, settings-service manifest area3 contract; deterministic synthetic parity-sealed records"
  , caseExclusion = "Retained config reads and reset-modes are declared seams. Area2/header prerequisites are supplied complete. NAND erase=ff/program=AND and request failures are model laws. Record-store private state is not compared."
  , caseRegions = map patch (caseRegions template) ++ [region "custom-images" lifecycleBase ReadWrite images,region "custom-live" liveBase ReadWrite (B.replicate 40 0xa5),region "custom-config" configBase ReadOnly (configurationBytes (configuration c)),region "custom-config-entry" 0x69f8fc ReadExecute (word32le (branchWord 0x69f8fc (resolve "fn_custom_config"))),region "custom-defaults" 0x7cb804 ReadOnly (B.take 40 (B.drop (0x7cb804-0x61c400) image)),region "custom-probes" probeBase ReadWrite (B.replicate 0x4000 0)] ++ [region "custom-check-observer" checkObserverBase ReadExecute observer | inputRecords c == BoundaryProbeRecords]
  , caseFunctions = nub ([(GuestAddress (fromIntegral address),size) | H.Function address size <- closure] ++ [(GuestAddress (fromIntegral address),size) | (address,size) <- driverFunctions ++ modelFunctions ++ [row | inputRecords c == BoundaryProbeRecords,row <- observerFunctions]] ++ [(GuestAddress 0x69f8fc,4),(GuestAddress 0x6a6fd8,4)] ++ [(address,size) | (address,size) <- caseFunctions template, guestAddress address >= 0x6faf00 || guestAddress address < 0x500000])
  , caseExpectedMemory = []
  }
  where
    g = R.geometry R.Kind3
    (bank0,bank1) = initialBanks (inputRecords c)
    medium = R.putBank g R.Bank1 bank1 (R.putBank g R.Bank0 bank0 (R.Medium M.empty))
    fixture = C.OracleCase (customName c) R.Kind3 medium [C.Open,C.Register C.Normal] R.Hardware3 (customFault c)
    template = H.caseSpec image loader ranges fixture
    images = R.replace 4 (word32le (case resetFlag c of Preserve -> 0; Reset -> 1)) (B.replicate 0x80 0xa5)
    (observer,observerFunctions) = checkObserver 0x6a84d8 [A.Word checkPrologue,A.jump "check_resume"]
    checkPrologue = let word = readWordAt image (0x6a84d4-0x61c400)
                   in if word == 0xe92d4df0 then word else error "custom check observer prologue differs"
    region :: String -> Word32 -> Permission -> B.ByteString -> CaseRegion
    region name base permission bytes = mkRegion (RegionId name) (GuestAddress (fromIntegral base)) (B.length bytes) permission bytes
    closure = H.functionClosureExcept [0x69f8fc,0x6a6fd8] image ranges (0x6ac2b4 : map fst customEntries)
    (modelBytes,modelSymbols,modelFunctions) = A.assemble H.modelBase [("raw_resume",0x6aad98)] (H.mediumCode fixture ++ seams)
    resolve name = M.findWithDefault (error ("missing custom model symbol " ++ name)) (A.label name) modelSymbols
    seeds = ("record-open",0x6ac2b4) : [(role,address) | (address,role) <- customEntries] ++ [("fn_copy",resolve "fn_copy")]
    (driver,_,driverFunctions) = A.assemble C.driverBase seeds ([A.at "fn_custom_driver",A.push [4,14],A.mov 0 0 0,A.bl "record-open"] ++ concat (zipWith step [0::Int ..] (customSteps c)) ++ finish)
    step index operation = body operation ++ case operation of
      ChangeWorking {} -> []
      _ -> A.load 12 (C.resultBase + 16 + fromIntegral index * 4) ++ [A.str 12 0 0]
    body operation = case operation of
      Boot -> [A.bl "boot"]
      Read -> A.load 0 C.callerBase ++ A.load 1 (C.callerBase + 0x40) ++ [A.bl "read"]
      ReadWithoutRecovery -> A.load 0 C.callerBase ++ [A.mov 1 0 0,A.bl "read"]
      Refresh -> [A.bl "refresh"]
      Defaults -> [A.bl "defaults"]
      ChangeWorking (RecordOffset offset) (RecordByte value) -> A.load 12 (0x89f874 + fromIntegral offset) ++ [A.mov 0 0 (fromIntegral value),A.strb 12 0 0]
    finish = A.load 0 C.cellsBase ++ [A.ldr 0 1 0] ++ A.load 12 C.resultBase ++ [A.str 12 1 0]
      ++ A.load 0 (C.callerBase + 0x100) ++ A.load 1 (R.stagingAddress g) ++ A.load 2 0x600 ++ [A.bl "fn_copy",A.pop [4,15]]
    seams = [A.at "fn_custom_config",A.movR 2 0] ++ A.load 0 configBase ++ [A.addR 2 0 2,A.ldrb 2 0 0,A.ret,A.at "fn_custom_reset"]
      ++ A.load 12 (C.resultBase + 4) ++ [A.ldr 12 1 0,A.add 1 1 1,A.str 12 1 0,A.str 12 0 4,A.mov 0 0 0,A.ret]
      ++ [A.at "fn_custom_raw_trace",A.push [0,1,2,3,12,14]] ++ A.load 12 probeBase
      ++ [A.ldr 12 2 0,A.add 3 2 1,A.str 12 3 0,A.add 2 2 1,A.lsl 2 2 2,A.addR 12 12 2,A.str 12 0 0,A.pop [0,1,2,3,12,14],A.Word rawPrologue,A.jump "raw_resume"]
    rawPrologue = let word = readWordAt image (0x6aad94-0x61c400)
                  in if word == 0xe92d4800 then word else error "custom raw-read observer prologue differs at 0x6aad94"
    patch r
      | name == "driver" = region name C.driverBase ReadExecute (driver <> B.replicate (fromIntegral (H.modelBase-C.driverBase)-B.length driver) 0 <> modelBytes)
      | name == "record-code" =
          let old = regionBytes (caseRegion r)
              reset = R.replace (0x6a6fd8-0x6a6b74) (word32le (branchWord 0x6a6fd8 (resolve "fn_custom_reset"))) old
              traced = if inputRecords c == BoundaryProbeRecords then
                R.replace (0x6a84d4-0x6a6b74) (word32le (branchWord 0x6a84d4 checkObserverBase))
                  (R.replace (0x6aad94-0x6a6b74) (word32le (branchWord 0x6aad94 (resolve "fn_custom_raw_trace"))) reset)
                else reset
          in region name 0x6a6b74 ReadExecute traced
      | name == "record-ram" = region name 0x89f8b0 ReadWrite (R.replace (fromIntegral (R.stagingAddress g-0x89f8b0)) (B.replicate 0x600 0x6d) (regionBytes (caseRegion r)))
      | name == "results" = region name C.resultBase ReadWrite (R.replace 4 (word32le 0 <> word32le 0) (regionBytes (caseRegion r)))
      | otherwise = r
      where name = regionName (regionIdOf (caseRegion r))

data RecordWindow = RecordWindow GuestAddress GuestAddress RegionId

ownedRecordRegions :: [CaseRegion] -> [CaseRegion]
ownedRecordRegions = concatMap protect
  where
    windows =
      [ RecordWindow (GuestAddress 0) (GuestAddress 0x89f8b0) (RegionId "before-record")
      , RecordWindow (GuestAddress 0x89f8b0) (GuestAddress 0x89f8f0) (RegionId "record-prefix")
      , RecordWindow (GuestAddress 0x89f8f8) (GuestAddress 0x89fa60) (RegionId "record-state")
      , RecordWindow (GuestAddress 0x89fa60) (GuestAddress 0x89fa68) (RegionId "record-cache-ages")
      , RecordWindow (GuestAddress 0x89fa68) (GuestAddress 0x8a0080) (RegionId "record-staging")
      , RecordWindow (GuestAddress 0x8a0080) (GuestAddress 0x8e0080) (RegionId "record-cache-blocks")
      , RecordWindow (GuestAddress 0x8e0080) (GuestAddress 0x8e0088) (RegionId "record-cache-tags")
      , RecordWindow (GuestAddress 0x8e0088) (GuestAddress 0x8e008c) (RegionId "record-cache-timestamp")
      , RecordWindow (GuestAddress 0x8e008c) (GuestAddress 0x917fa8) (RegionId "record-other")
      , RecordWindow (GuestAddress 0x917fe4) (GuestAddress 0x100000000) (RegionId "after-record")
      ]
    protect original@(CaseRegion view permission)
      | physicalEnd <= 0x89f8b0 || physicalStart >= 0x917fe4 = [original]
      | otherwise =
          [ mkRegion (RegionId (regionName (regionIdOf view) ++ "-" ++ regionName identifier))
              (GuestAddress (fromInteger (low + alias))) count permission bytes
          | RecordWindow start end identifier <- windows
          , let low = max physicalStart (toInteger (guestAddress start))
          , let high = min physicalEnd (toInteger (guestAddress end))
          , low < high
          , let count = fromInteger (high-low)
          , let bytes = B.take count (B.drop (fromInteger (low-physicalStart)) (regionBytes view))
          ]
      where
        physicalStart = baseAddress view .&. complement 0x40000000
        physicalEnd = physicalStart + toInteger (B.length (regionBytes view))
        alias = baseAddress view - physicalStart

candidateCase :: Word32 -> B.ByteString -> M.Map String Symbol -> CaseSpec -> Either String CaseSpec
candidateCase base bytes symbols spec = do
  Symbol textEnd _ <- required "__custom_text_end"
  let lengthBytes = fromIntegral (textEnd-base)
  if textEnd < base || lengthBytes > B.length bytes then Left "custom text end outside image" else pure ()
  let functions = [(GuestAddress (fromIntegral address),size) | Symbol address size <- M.elems symbols,size > 0,(address >= base && toInteger address + toInteger size <= toInteger textEnd) || (address `elem` map fst entryTargets && size == 4)]
  let observed = any ((== RegionId "custom-check-observer") . regionIdOf . caseRegion) (caseRegions spec)
  redirects <- mapM (\(address,name) -> do
    Symbol site size <- required name
    if site /= address || size /= 4 then Left ("moved custom artifact entry " ++ name)
    else pure (address,readWordAt bytes (fromIntegral (site-base)))) entryTargets
  Symbol checkTarget checkSize <- required "settings_custom_codec_check"
  unless (checkTarget >= base && checkSize >= 4 && checkTarget + checkSize <= textEnd) (Left "custom codec observer target leaves text")
  let checkOffset = fromIntegral (checkTarget-base)
      checkWord = readWordAt bytes checkOffset
  unless (not observed || checkWord == 0xe1a02000) (Left "custom codec observer prologue differs")
  let (observer,observerFunctions) = checkObserver (checkTarget+4) [A.Word checkWord,A.jump "check_resume"]
  let patched = [if observed && regionIdOf (caseRegion r) == RegionId "custom-check-observer"
        then mkRegion (RegionId "custom-check-observer") (GuestAddress (fromIntegral checkObserverBase)) (B.length observer) ReadExecute observer
        else patch redirects r | r <- caseRegions spec]
      codeBytes = if observed then R.replace checkOffset (word32le (branchWord checkTarget checkObserverBase)) (B.take lengthBytes bytes) else B.take lengthBytes bytes
      code = mkRegion (RegionId "custom-owned") (GuestAddress (fromIntegral base)) lengthBytes ReadExecute codeBytes
      inReplaced address = any (\(site,_) -> address == site) redirects
  pure spec {caseRegions = ownedRecordRegions patched ++ [code],caseFunctions = nub (functions ++ [(GuestAddress (fromIntegral address),size) | observed,(address,size) <- observerFunctions] ++ [(address,size) | (address,size) <- caseFunctions spec,not (observed && guestAddress address == fromIntegral checkObserverBase),not (inReplaced (fromIntegral (guestAddress address))) && not (insideOwned (guestAddress address) lengthBytes) && (guestAddress address < 0x6ac2b4 || guestAddress address >= 0x6ae114)])}
  where
    required name = maybe (Left ("missing custom artifact symbol " ++ name)) Right (M.lookup name symbols)
    entryTargets = [(address,"settings_custom_" ++ role ++ "_entry") | (address,role) <- customEntries]
      ++ [(address,"record_store_entry_" ++ role) | (address,role) <- recordEntries]
    insideOwned address size = address >= fromIntegral base && address < fromIntegral base + fromIntegral size
    patch redirects r
      | regionName (regionIdOf (caseRegion r)) == "record-code" =
          let old = regionBytes (caseRegion r)
              changed = foldl (\current (address,instruction) -> R.replace (fromIntegral (address-0x6a6b74)) (word32le instruction) current) old redirects
          in mkRegion (RegionId "record-code") (GuestAddress 0x6a6b74) (B.length changed) ReadExecute changed
      | otherwise = r

data CustomRegion = CustomResults | CustomCaller | CustomMedium | CustomRequests | CustomImages | CustomLive | CustomProbes
  deriving (Eq, Ord, Show, Enum, Bounded)

data CustomObservation = CustomObservation
  { customObservationStatus :: !RunnerStatus
  , customObservationRegions :: !(M.Map CustomRegion B.ByteString)
  , customObservationAbi :: !(M.Map Register Word32)
  } deriving (Eq, Show)

customRegionName :: CustomRegion -> String
customRegionName region = case region of
  CustomResults -> "results"
  CustomCaller -> "caller"
  CustomMedium -> "medium"
  CustomRequests -> "requests"
  CustomImages -> "custom-images"
  CustomLive -> "custom-live"
  CustomProbes -> "custom-probes"

customAbiRegisters :: [Register]
customAbiRegisters = [R4,R5,R6,R7,R8,R9,R10,R11,Sp]

-- Partial result and image views are detached.
projectCustomObservation :: NativeObservation -> CustomObservation
projectCustomObservation observation = CustomObservation (forceStatus (nativeStatus observation))
  (M.fromList [(region,retain region bytes) | region <- [minBound..maxBound]
    ,Just bytes <- [lookup (RegionId (customRegionName region)) (nativeMemory observation)]])
  (M.fromList [(register,value) | register <- customAbiRegisters,Just value <- [lookupRegister register observation]])
 where
  forceStatus status = case status of
    OtherStatus label -> foldr seq () label `seq` status
    _ -> status
  retain CustomResults bytes
    | B.length bytes > C.resultSize = B.copy (B.take C.resultSize bytes)
    | otherwise = bytes
  retain CustomImages bytes
    | B.length bytes >= 120 = images bytes
    | otherwise = B.copy (images bytes)
  retain _ bytes = bytes
  images bytes = B.take 56 (B.drop 4 bytes) <> B.take 52 (B.drop 68 bytes)

customBytes :: CustomObservation -> CustomRegion -> Maybe B.ByteString
customBytes observation region = M.lookup region (customObservationRegions observation)

comparePair :: CustomObservation -> CustomObservation -> [String]
comparePair stock candidate =
  ["status" | customObservationStatus stock /= Returned || customObservationStatus candidate /= Returned]
  ++ ["memory " ++ customRegionName region | region <- [minBound..maxBound]
    ,customBytes stock region /= customBytes candidate region || customBytes stock region == Nothing || customBytes candidate region == Nothing]
  ++ ["preserved register " ++ show register | register <- customAbiRegisters
    ,M.lookup register (customObservationAbi stock) /= M.lookup register (customObservationAbi candidate)]

customInvariants :: CustomCase -> CustomObservation -> [String]
customInvariants c observed =
  ["unexpected persistence" | noPersistence c,maybe True (\bytes -> B.length bytes < 4 || readWordAt bytes 0 /= 0) (customBytes observed CustomResults)]
  ++ ["staging tail changed" | maybe True (\bytes -> B.take (0x600-52) (B.drop (0x100+52) bytes) /= B.replicate (0x600-52) 0xff) (customBytes observed CustomCaller)]

data CustomControlRole = ReferenceControlRole | SubjectControlRole deriving (Eq, Show)

data CustomControlError
  = NoCustomControlWitnesses
  | CustomControlDidNotReturn CaseId CustomControlRole RunnerStatus
  deriving (Eq, Show)

data CustomControlWitness = CustomControlWitness
  { controlCase :: CaseId
  , controlReference :: CustomObservation
  , controlSubject :: CustomObservation
  }

customControlKilled :: [CustomControlWitness] -> Either CustomControlError Bool
customControlKilled [] = Left NoCustomControlWitnesses
customControlKilled witnesses = go False witnesses
  where
    go !killed [] = Right killed
    go !killed (witness:rest) = do
      requireReturned (controlCase witness) ReferenceControlRole (controlReference witness)
      requireReturned (controlCase witness) SubjectControlRole (controlSubject witness)
      go (killed || not (null (comparePair (controlReference witness) (controlSubject witness)))) rest
    requireReturned identifier role observation =
      unless (customObservationStatus observation == Returned) (Left (CustomControlDidNotReturn identifier role (customObservationStatus observation)))
