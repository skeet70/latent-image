{-# LANGUAGE GHC2021 #-}
module BankRecordNative (JoinedCases(..), prepareJoinCases, prepareControlJoinCases) where

import Control.Monad (forM)
import qualified Data.ByteString as B
import Data.List (nub, nubBy, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified CustomNative as Custom
import CustomAdmission (bankEntries, customEntries, recordEntries, retiredSettingsRegions)
import Emulation.NativeCase (Region(..))
import Emulation.NativeObservation (GuestAddress(..), RegionId(..), Register(..), RunnerStatus(..))
import Emulation.OwnedCase
import qualified Native as N
import Oracle
import PropertyFields (PropertyId(..))
import qualified Replacement.PropertyService.Artifact as Artifact
import Replacement.PropertyService.Classify (Class(..), Property(..), classify)
import qualified Replacement.PropertyService.Oracle as P
import qualified Replacement.RecordStore.Arm as A
import qualified Replacement.RecordStore.Cases as C
import qualified Replacement.RecordStore.Harness as H
import Replacement.RecordStore.Hardware (hardware)
import qualified Replacement.RecordStore.Model as R
import ServiceInventory (Image(..))

data JoinedCases = JoinedCases
  { joinStockCases :: [CaseSpec]
  , joinOwnedCases :: [CaseSpec]
  , joinControlCases :: [(String,[CaseSpec])]
  }

data PersistenceIntent = RetainRecords | AppendCustom deriving (Eq, Show)

data JoinInput = JoinInput String CaseSpec PersistenceIntent R.Fault

prepareJoinCases :: B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> Artifact.LinkedArtifact -> B.ByteString -> M.Map String Symbol -> Either String JoinedCases
prepareJoinCases canonical loader ranges propertyImage artifact =
  prepareWithMetadata canonical loader ranges propertyImage (N.propertyMetadata artifact)

prepareWithMetadata :: B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> N.PropertyMetadata -> B.ByteString -> M.Map String Symbol -> Either String JoinedCases
prepareWithMetadata canonical loader ranges propertyImage metadata binary symbols = do
  properties <- classify (Image "image4" "" 0x61c400 canonical)
  let table = M.fromList [(propertyId p,p) | p <- properties]
      arms = [owned | Property _ _ _ (Owned owned) <- properties]
      fixture = P.withConfigVariants arms (P.fixture canonical)
      source = N.prepareNativeImage canonical
      matrix = N.storageMatrix source table fixture P.caseGrid
      entries = [(address,"settings_bank_" ++ role ++ "_entry") | (address,role) <- bankEntries]
      ownedImage = N.prepareComposedImage propertyImage metadata binary symbols entries (N.matrixFixtures matrix)
      setterOperation = P.OracleCase (PropertyId 1) (P.Set P.Bank0 (P.SlotArgument 0) P.SetOne)
      setterOwned = maybe (error "bank-record setter property unmodeled") id (P.modelled (P.standingOf table (PropertyId 1)))
      setter = (N.propertyCase source table fixture Task setterOperation setterOwned) { caseId = "header-close-property-1" }
      selected = [spec | spec <- N.matrixSequences matrix, caseId spec `elem` map ("sequence-canonical-" ++) ["initialize","exchange-restore","capture-change-restore","apply-0-bank-0","apply-73-bank-0"]]
      requests = [spec | spec <- N.matrixRequests matrix,caseId spec == "request-write-40-320"]
      inputs = [JoinInput (caseId spec) spec RetainRecords R.NoFault | spec <- setter:selected]
        ++ [JoinInput ("custom-" ++ tag) spec AppendCustom fault | spec <- requests,(tag,fault) <- [("program",R.NoFault),("program-failure",R.FailRequest (R.PersistenceStep 0)),("before-program",R.StopAfter (R.PersistenceStep 0)),("next-program-not-reached",R.StopAfter (R.PersistenceStep 1))]]
  if null selected || length requests /= 1 then Left "bank-record join matrix witnesses missing" else pure ()
  stock <- mapM (compose canonical loader ranges id) inputs
  owned <- mapM (compose canonical loader ranges (N.ownedCase ownedImage)) inputs
  candidate <- mapM (patchEntries binary symbols) owned
  controls <- forM [("custom-refresh-omitted","settings_custom_refresh")] $ \(name,function) -> do
    changed <- mapM (omitFunction function symbols) [spec | spec <- candidate,caseId spec == "bank-record-custom-program"]
    pure (name,changed)
  pure (JoinedCases stock candidate controls)
  where
    propertyId (Property identifier _ _ _) = identifier

prepareControlJoinCases :: B.ByteString -> B.ByteString -> [H.Function] -> B.ByteString -> N.PropertyMetadata -> B.ByteString -> M.Map String Symbol -> Either String JoinedCases
prepareControlJoinCases canonical loader ranges controlPropertyImage extentMetadata binary symbols = do
  joined <- prepareWithMetadata canonical loader ranges controlPropertyImage extentMetadata binary symbols
  let witness = filter ((== "bank-record-header-close-property-1") . caseId)
  pure joined {joinStockCases = witness (joinStockCases joined),joinOwnedCases = witness (joinOwnedCases joined),joinControlCases = []}

compose :: B.ByteString -> B.ByteString -> [H.Function] -> (CaseSpec -> CaseSpec) -> JoinInput -> Either String CaseSpec
compose image loader ranges own (JoinInput label bank intent fault) = do
  backing <- bytesOf bank "backing"
  expectedBacking <- maybe (Left "join backing expectation missing") Right (lookup (RegionId "backing") (caseExpectedMemory bank))
  expectedCallBytes <- maybe (Left "bank-record calls expectation missing") Right (lookup (RegionId "calls") (caseExpectedMemory bank))
  let custom = Custom.stockCase image loader ranges (Custom.CustomCase (C.CaseName label) Custom.ValidRecords Custom.Preserve Custom.ZeroConfiguration [Custom.Boot] fault True)
      ownedBank = own bank
      seed = Custom.payloadRecord 7
      geometry = R.geometry R.Kind3
      (bank0,bank1) = Custom.initialBanks Custom.ValidRecords
      initialMedium = R.putBank geometry R.Bank1 bank1 (R.putBank geometry R.Bank0 bank0 (R.Medium M.empty))
      initialStore = R.initialStore R.Kind3 initialMedium fault R.Hardware3
      (_,booted) = R.readLatest R.WriteRecoveryCount hardware initialStore
      live = B.take 40 (B.drop 0x140 expectedBacking)
      working = if intent == AppendCustom then R.replace 4 live seed else seed
      (_,appended) = R.append hardware (R.Seed 0) (R.CallerRecord working) booted
      finalStore = if intent == AppendCustom then snd (R.readLatest R.OmitRecoveryCount hardware appended) else booted
      persistence = R.persistence finalStore
      interrupted = R.stopped persistence
      latest = if intent == AppendCustom && fault /= R.FailRequest (R.PersistenceStep 0) then snd (hardware R.Prepare R.Hardware3 (R.half 8 <> B.drop 2 working)) else seed
      finalWorking = if intent == AppendCustom then R.replace 0 (B.take 2 latest) working else seed
      mediumBytes = B.concat [R.bankBytes geometry b (R.medium persistence) | b <- [R.Bank0,R.Bank1]]
      journal = B.concat (map C.encodeRequest (R.requests persistence))
      result = maybe 0 id (lookup R0 (caseExpectedRegisters bank))
      expectedEvents = decodeEvents expectedCallBytes
      expectedCalls = eventBytes (filter (not . refreshEvent) expectedEvents)
      bankExpectations = [(ident,if ident == RegionId "calls" then expectedCalls else bytes) | (ident,bytes) <- caseExpectedMemory bank,ident /= RegionId "backing"] ++ [(RegionId "backing",expectedBacking)]
      interruptedBank = [(ident,if ident == RegionId "calls" then eventBytes (takeWhile (not . refreshEvent) expectedEvents) else bytes) | (ident,bytes) <- bankExpectations]
      expected = if interrupted
        then interruptedBank ++ [(RegionId "medium",mediumBytes),(RegionId "custom-head",B.replicate 4 0xa5 <> word32le 0 <> seed),(RegionId "custom-working",working)]
        else bankExpectations ++ [(RegionId "medium",mediumBytes),(RegionId "join-output",latest),(RegionId "join-read-status",word32le 0),(RegionId "custom-head",B.replicate 4 0xa5 <> word32le 0 <> latest),(RegionId "custom-working",finalWorking)] ++ [(RegionId "join-bank-status",word32le result) | lookup R0 (caseExpectedRegisters bank) /= Nothing]
      requestExpectation = [(RegionId "requests",if B.null journal then B.replicate C.requestStride 0 else journal)]
      program = [A.at "join",A.push [4,14],A.bl "custom_boot_driver"]
        ++ copy 0x409c295c inputBase 40 ++ copy 0x958000 zeroBase 0x800
        ++ concat [A.load (registerNumber register) value | (register,value) <- caseInitial bank,register `elem` [R0,R1,R2,R3,R12]]
        ++ [A.bl "bank_entry"] ++ A.load 12 resultBase ++ [A.str 12 0 0]
        ++ A.load 0 outputBase ++ [A.mov 1 0 0,A.bl "custom_read"] ++ A.load 12 resultBase ++ [A.str 12 0 4,A.mov 0 0 0,A.pop [4,15]]
      (driver,_,functions) = A.assemble driverBase [("custom_boot_driver",C.driverBase),("bank_entry",fromIntegral (guestAddress (caseEntry bank))),("custom_read",0x6a865c),("copy",0x722054)] program
      copy destination source count = A.load 0 destination ++ A.load 1 source ++ A.load 2 count ++ [A.bl "copy"]
      customRegions = concatMap splitImages [r | r <- caseRegions custom,N.regionBytesOfId r `notElem` map RegionId ["record-code","runtime-code","custom-live","stack","custom-defaults"]]
      bankRegions = map schedulerSeam [r | r <- caseRegions ownedBank,N.regionBytesOfId r `notElem` map RegionId ["code-mirror","stack"]]
      mergeFunctions = nubBy (\(a,_) (b,_) -> a == b) (sortOn (\(a,size) -> (guestAddress a, negate (fromIntegral size :: Int))) (nub ([(GuestAddress (fromIntegral driverBase),fromIntegral (B.length driver))] ++ [(address,size) | (address,size) <- caseFunctions ownedBank,guestAddress address < 0x6a8484 || guestAddress address >= 0x6ae114,address `notElem` map (GuestAddress . fromIntegral) ([0x724b68,0x724c00] :: [Word32])] ++ caseFunctions custom ++ [(GuestAddress (fromIntegral address),size) | (address,size) <- functions] ++ [(GuestAddress (fromIntegral address),fromIntegral (length (schedulerWords address)*4)) | address <- [0x724b68,0x724c00]] ++ [(GuestAddress 0x42634,8),(GuestAddress 0x4263c,8)])))
  mirror <- mergedMirror image custom ownedBank
  pure bank
    { caseId = "bank-record-" ++ label
    , caseProvenance = "X-M1 1.50 E03/E04/E09 and record-service 0x6a86c4/0x6ac7d4/0x6ad3f4. Synthetic parity-sealed area3 records; independent typed record model and bank model."
    , caseExclusion = "Synthetic NAND atomic program=AND and erase=ff laws, not device physics. Configuration/reset and scheduler seams retained from qualified custom/record fixtures. Interruption observes medium before the selected persistence step."
    , caseEntry = GuestAddress (fromIntegral driverBase)
    , caseExit = caseExit custom
    , caseInitial = caseInitial custom
    , caseRegions = bankRegions ++ customRegions ++ [mirror,N.region "stack" C.stackBase ReadWrite (B.replicate 0x10000 0),N.region "join-runtime-seams" 0x42634 ReadExecute (assemble [0xe3a00000,0xe12fff1e,0xe3a00000,0xe12fff1e]),N.region "join-driver" driverBase ReadExecute driver,N.region "join-zero" zeroBase ReadOnly (B.replicate 0x800 0),N.region "join-input" inputBase ReadOnly (B.take 40 (B.drop 0x140 backing)),N.region "join-bank-status" resultBase ReadWrite (B.replicate 4 0xa5),N.region "join-read-status" (resultBase+4) ReadWrite (B.replicate 4 0xa5),N.region "join-output" outputBase ReadWrite (B.replicate 52 0xa5)]
    , caseFunctions = mergeFunctions
    , caseExpectedStatus = if interrupted then MemoryGuard else Returned
    , caseExpectedRegisters = if interrupted then [(R0,0x5150)] else caseExpectedRegisters custom
    , caseExpectedMemory = expected ++ requestExpectation
    , caseIgnoredMemory = []
    }
  where
    driverBase = 0x520000
    zeroBase = 0x521000
    inputBase = 0x522000
    resultBase = 0x523000
    outputBase = 0x524000
    registerNumber register = case register of R0 -> 0; R1 -> 1; R2 -> 2; R3 -> 3; R12 -> 12; _ -> error "join argument register"
    splitImages r
      | N.regionBytesOfId r == RegionId "custom-images" =
          let bytes = regionBytes (caseRegion r)
          in [N.region "custom-head" 0x89f830 ReadWrite (B.take 60 bytes),N.region "custom-handle" 0x89f86c ReadWrite (B.take 8 (B.drop 60 bytes)),N.region "custom-working" 0x89f874 ReadWrite (B.drop 68 bytes)]
      | otherwise = [r]
    schedulerWords address =
      let original = N.callback address
          prefix = [0xe350004b,0x03a00000,0x012fff1e] ++ init original
      in prefix ++ [N.branch (address+fromIntegral (length prefix*4)) N.logHelper]
    schedulerSeam r
      | N.regionBytesOfId r == RegionId "code-runtime" = N.region "code-runtime" 0x722054 ReadExecute (foldl (\bytes address -> R.replace (fromIntegral (address-0x722054)) (assemble (schedulerWords address)) bytes) (regionBytes (caseRegion r)) [0x724b68,0x724c00])
      | otherwise = r
    refreshEvent (Event site _ _ _) = site == 0x6a86c4
    decodeEvents bytes = [Event site a b c | index <- [0..fromIntegral (readWordAt bytes 0)-1],let offset = 4+16*index,let site = readWordAt bytes offset,let a = readWordAt bytes (offset+4),let b = readWordAt bytes (offset+8),let c = readWordAt bytes (offset+12)]

bytesOf :: CaseSpec -> String -> Either String B.ByteString
bytesOf spec name = case [regionBytes (caseRegion r) | r <- caseRegions spec,N.regionBytesOfId r == RegionId name] of
  [bytes] -> Right bytes
  _ -> Left ("bank-record missing region " ++ name)

mergedMirror :: B.ByteString -> CaseSpec -> CaseSpec -> Either String CaseRegion
mergedMirror image custom bank = do
  bankBytes <- bytesOf bank "code-mirror"
  customBytes <- bytesOf custom "record-code"
  let base = 0x6a59b8
      end = 0x6ae114
      canonical = B.take (end-base) (B.drop (base-0x61c400) image)
      withBank = R.replace 0 bankBytes canonical
      withCustom = R.replace (0x6a8484-base) (B.drop (0x6a8484-0x6a6b74) customBytes) withBank
  pure (N.region "code-mirror" (fromIntegral base) ReadExecute withCustom)

patchEntries :: B.ByteString -> M.Map String Symbol -> CaseSpec -> Either String CaseSpec
patchEntries binary symbols spec = do
  redirects <- forM ([(address,"settings_custom_" ++ role ++ "_entry") | (address,role) <- customEntries] ++ [(address,"record_store_entry_" ++ role) | (address,role) <- recordEntries]) $ \(address,name) -> do
    Symbol site size <- maybe (Left ("join symbol missing " ++ name)) Right (M.lookup name symbols)
    if site /= address || size /= 4 then Left ("join veneer moved " ++ name) else pure (address,readWordAt binary (fromIntegral (address-0x5d0000)))
  let fixedEntries = map (GuestAddress . fromIntegral . fst) (bankEntries ++ customEntries ++ recordEntries)
      retired address = any (\(low,high) -> guestAddress address >= fromIntegral low && guestAddress address < fromIntegral high) retiredSettingsRegions
      functions = [(address,if fixed then 4 else size) | (address,size) <- caseFunctions spec,let fixed = address `elem` fixedEntries,fixed || not (retired address)]
  pure spec { caseRegions = Custom.ownedRecordRegions (map (patch redirects) (caseRegions spec)),caseFunctions = functions }
  where
    patch redirects r
      | N.regionBytesOfId r == RegionId "code-mirror" = N.region "code-mirror" 0x6a59b8 ReadExecute (foldl (\bytes (address,instruction) -> R.replace (fromIntegral (address-0x6a59b8)) (word32le instruction) bytes) (regionBytes (caseRegion r)) redirects)
      | otherwise = r

omitFunction :: String -> M.Map String Symbol -> CaseSpec -> Either String CaseSpec
omitFunction name symbols spec = do
  Symbol address size <- maybe (Left ("join control symbol missing " ++ name)) Right (M.lookup name symbols)
  if size < 8 then Left ("join control extent " ++ name) else pure spec {caseRegions = map (patch address) (caseRegions spec)}
  where
    patch address r
      | N.regionBytesOfId r == RegionId "owned" = N.region "owned" 0x5d0000 ReadExecute (R.replace (fromIntegral (address-0x5d0000)) (word32le 0xe3a00000 <> word32le 0xe12fff1e) (regionBytes (caseRegion r)))
      | otherwise = r
