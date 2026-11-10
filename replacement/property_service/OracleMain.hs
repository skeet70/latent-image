{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Runs the stock getter 0x61e110 and setter 0x624694 from the canonical image4 and from the staged
-- candidate image4 for every property id, or for the ids `--ids` lists, checks the stock runs of
-- owned ids against the owned model, compares the candidate with stock case by case and runs the
-- guest controls.
module Main (main) where

import Control.Exception (evaluate)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, eitherDecodeStrict', object, (.=))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (isPrefixOf, nub)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, isNothing, listToMaybe)
import qualified Data.Set as S
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeObservation (RunnerStatus (Returned), runnerStatusLabel)
import Emulation.OwnedCase (CaseId (..), CaseSpec, loadObservedBatch, readWordAt, required, writeBatch)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import FujiAF.Host (ensure, failH, get, readRegular, str)
import FujiAF.JSON (encodeCanonical)
import FujiModNativePaths (fujimodNativeCase)
import Numeric (readHex, showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Admission (FunctionExtent (..))
import Replacement.PropertyService.Artifact
import Replacement.PropertyService.ClassRecord (ownedLabel, reasonLabel, stockLabel)
import Replacement.PropertyService.Classify
import Replacement.PropertyService.Encoding (EncodedEntry (..), EncodedSlot (..), EncodedSpecial (..), SlotIndex (..), SpecialArm (..), TableEntry (..), encodeEntry, encodeSlot, encodeSpecial)
import Replacement.PropertyService.Oracle
import qualified Replacement.PropertyService.Placement as P
import ServiceInventory (Image (..))
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesFileExist, doesPathExist, listDirectory, makeAbsolute, removeFile, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath (takeDirectory, takeFileName, (</>))

canonicalImage4Sha256 :: String
canonicalImage4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

batchLimit :: Int
batchLimit = 4096

movR0R0Word :: Word32
movR0R0Word = 0xe1a00000

fixtureImage :: [(DefaultImage,P.GuestAddress)] -> Fixture -> B.ByteString -> B.ByteString
fixtureImage windows fx image = foldl inject image windows
  where
    inject bytes (kind,P.GuestAddress address) =
      let offset = fromIntegral (address-image4Base)
          value = case kind of
            SlotDefaultBytes -> fixtureSlotDefaults fx
            TailDefaultBytes -> B.take 40 (fixtureTailDefaults fx)
      in B.take offset bytes <> value <> B.drop (offset+B.length value) bytes

data BatchRecord = BatchRecord
  { recordName :: String
  , recordCases :: Int
  , recordBatchSha :: String
  , recordSummarySha :: String
  , recordReuse :: BatchReuse
  }

reuseLabel :: BatchReuse -> String
reuseLabel BatchExecuted = "executed"
reuseLabel BatchReused = "reused"

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

propertyHex :: PropertyId -> String
propertyHex (PropertyId n) = hex n

chunks :: Int -> [a] -> [[a]]
chunks size items = case splitAt size items of
  (chunk, []) -> [chunk | not (null chunk)]
  (chunk, rest) -> chunk : chunks size rest

orFail :: Show e => String -> Either e a -> IO a
orFail context = either (\problem -> failH (context ++ ": " ++ show problem)) pure

-- | One runner session over at most 'batchLimit' cases in @directory@, its runs keyed by case label.
runBatch :: Maybe FilePath -> FilePath -> FilePath -> [CaseSpec] -> IO (BatchRecord, M.Map CaseLabel Run)
runBatch cache runner directory specs = do
  ensure (length specs <= batchLimit) "batch over the case limit"
  batchPath <- writeBatch directory specs
  (reuse, observed) <- case cache of
    Nothing -> do
      observed <- loadObservedBatch runner batchPath
      pure (BatchExecuted, observed)
    Just cacheRoot -> cachedObservedBatch cacheRoot runner batchPath
  ensure (length observed == length specs) (batchPath ++ ": observation count differs from the case count")
  rows <- forM observed $ \(CaseId label, observation) -> do
    let reportPath = directory </> label ++ ".native-result.json"
    reportBytes <- B.readFile reportPath
    report <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (eitherDecodeStrict' reportBytes :: Either String Value)
    writes <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (reportWrites report)
    run <- either (\problem -> failH (reportPath ++ ": " ++ problem)) pure (runOf observation writes)
    forced <- evaluate (forceRun run)
    pure (CaseLabel label, forced, sha256Hex (sha256 reportBytes))
  batchBytes <- B.readFile batchPath
  let batchSha = sha256Hex (sha256 batchBytes)
      summary = encodeCanonical $ object
        [ "batch_sha256" .= batchSha
        , "cases" .= [object ["id" .= label, "status" .= runnerStatusLabel (runStatus run), "result_sha256" .= digest] | (CaseLabel label, run, digest) <- rows]
        ]
  B.writeFile (directory </> "summary.json") summary
  generated <- listDirectory directory
  forM_ generated $ \name -> when (name `notElem` ["batch.json", "summary.json"]) $ do
    let path = directory </> name
    regular <- doesFileExist path
    when regular (removeFile path)
  pure (BatchRecord (takeFileName directory) (length specs) batchSha (sha256Hex (sha256 summary)) reuse, M.fromList [(label, run) | (label, run, _) <- rows])

runBatches :: Maybe FilePath -> FilePath -> FilePath -> String -> [CaseSpec] -> IO ([BatchRecord], M.Map CaseLabel Run)
runBatches cache runner out prefix specs = do
  results <- forM (zip [0 :: Int ..] (chunks batchLimit specs)) $ \(index, chunk) ->
    runBatch cache runner (out </> prefix ++ "-" ++ (if index < 10 then "0" else "") ++ show index) chunk
  pure (map fst results, M.unions (map snd results))

lookupRun :: M.Map CaseLabel Run -> CaseLabel -> IO Run
lookupRun runs label = maybe (failH ("no run for " ++ caseLabelText label)) pure (M.lookup label runs)

prefixed :: String -> OracleCase -> CaseLabel
prefixed prefix oracleCase = CaseLabel (prefix ++ "-" ++ caseLabelText (caseLabel oracleCase))

-- | A case under one fixture variant.
data RunCase = RunCase { runVariant :: FixtureVariant, runCase :: OracleCase } deriving (Eq, Ord)

canonical :: OracleCase -> RunCase
canonical = RunCase CanonicalFixture

runLabel :: RunCase -> CaseLabel
runLabel (RunCase variant c) = case variant of
  CanonicalFixture -> caseLabel c
  _ -> CaseLabel (variantTag variant ++ "-" ++ caseLabelText (caseLabel c))

runPrefixed :: String -> RunCase -> CaseLabel
runPrefixed prefix rc = CaseLabel (prefix ++ "-" ++ caseLabelText (runLabel rc))

runProperty :: RunCase -> PropertyId
runProperty = oracleProperty . runCase

classLabel :: Standing -> String
classLabel standing = case standing of
  Classified (Owned owned) -> ownedLabel owned
  Classified (Delegated _) -> "delegated"
  OutOfRange -> "out-of-range"

statusCounts :: [Run] -> Value
statusCounts runs = object [fromString status .= count | (status, count) <- M.toList (M.fromListWith (+) [(runnerStatusLabel (runStatus run), 1 :: Int) | run <- runs])]

-- | The first 20 differences of each kind, in case order.
differenceSamples :: [(String, CaseLabel, Difference)] -> Value
differenceSamples rows = object
  [ fromString kind .= [object ["case" .= caseLabelText label, "detail" .= differenceDetail difference] | (label, difference) <- take 20 (reverse samples)]
  | (kind, samples) <- M.toList (M.fromListWith (++) [(phase ++ ":" ++ differenceKind difference, [(label, difference)]) | (phase, label, difference) <- rows])
  ]

stockSpec :: Fixture -> B.ByteString -> M.Map PropertyId Property -> CaseLabel -> [FunctionSpan] -> OracleCase -> CaseSpec
stockSpec fx image properties label functions oracleCase =
  caseSpec image functions fx label standing oracleCase (fromMaybe placeholderExpectation (modelled standing >>= \owned -> modelExpectation fx owned (oracleOperation oracleCase)))
  where
    standing = standingOf properties (oracleProperty oracleCase)


-- | Whether a control's patched image must fail a case or pass every case.
data ControlKind = MustFail | MustPass deriving (Eq)

-- | A control's image: the candidate with words replaced, or a defect build staged like the candidate.
data ControlImage = PatchedCandidate [Patch] | DefectBuild DefectName

-- | One `property_defect_*` cfg of the Rust crate, as `--defect` names it.
newtype DefectName = DefectName String deriving (Eq, Ord)

data Control = Control
  { controlName :: String
  , controlKind :: ControlKind
  , controlImage :: ControlImage
  , controlCases :: [RunCase]
  , controlNote :: String
  }

controlHolds :: Control -> Int -> Bool
controlHolds control failed = case controlKind control of
  MustFail -> failed > 0
  MustPass -> failed == 0

main :: IO ()
main = do
  arguments <- getArgs
  runner <- case dropWhile (/= "--runner") arguments of
    [] -> pure fujimodNativeCase
    [_] -> failH "--runner needs a value"
    _ : value : _ -> makeAbsolute value
  bundled <- if "--smoke" `elem` arguments then pure Nothing else Just <$> mapM (\name -> required arguments name >>= makeAbsolute) ["--linked", "--symbols", "--elf"]
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  case bundled of
    Nothing -> smoke arguments runner
    Just [linkedPath, symbolsPath, elfPath] -> oracle arguments runner linkedPath symbolsPath elfPath
    Just _ -> failH "unreachable argument shape"

freshOutput :: FilePath -> IO FilePath
freshOutput path = do
  exists <- doesPathExist path
  when exists (failH (path ++ " already exists"))
  absolute <- makeAbsolute path
  createDirectoryIfMissing True (takeDirectory absolute)
  createDirectory absolute
  pure absolute

-- | Each defect binds its binary, symbols and ELF.
defectArguments :: [String] -> [(String, FilePath, FilePath, FilePath)]
defectArguments arguments = case arguments of
  "--defect" : name : binPath : symbolsPath : elfPath : rest -> (name, binPath, symbolsPath, elfPath) : defectArguments rest
  _ : rest -> defectArguments rest
  [] -> []

-- | The ids `--ids` lists as comma-separated hex, or Nothing for every id.
subsetIds :: [String] -> IO (Maybe (S.Set PropertyId))
subsetIds arguments = case dropWhile (/= "--ids") arguments of
  _ : text : _ -> Just . S.fromList <$> mapM parse (splitOn text)
  [_] -> failH "--ids needs a value"
  [] -> pure Nothing
  where
    splitOn text = case break (== ',') text of
      (item, []) -> [item]
      (item, _ : rest) -> item : splitOn rest
    parse item = case item of
      '0' : 'x' : digits | [(n, "")] <- readHex digits, n <= 646 -> pure (PropertyId n)
      _ -> failH ("--ids: " ++ item ++ " is not a hex id from 0x0 to 0x286")

-- | The optional native batch cache root. Absent keeps the uncached one-shot batch behavior.
cacheOption :: [String] -> IO (Maybe FilePath)
cacheOption arguments = case dropWhile (/= "--cache") arguments of
  _ : value : _ -> Just <$> makeAbsolute value
  [_] -> failH "--cache needs a value"
  [] -> pure Nothing

oracle :: [String] -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
oracle arguments runner linkedPath symbolsPath elfPath = do
  cache <- cacheOption arguments
  let input name = required arguments name
  image4Path <- input "--image4"
  candidatePath <- input "--candidate-image4"
  contractPath <- input "--contract"
  sourcePaths <- mapM input ["--dat", "--loader-a", "--loader-b", "--image5", "--plain", "--image2"]
  outPath <- input "--out"
  subset <- subsetIds arguments
  scratch <- makeAbsolute "private-data/scratch/PropertyService2" >>= canonicalizePath
  absoluteOut <- makeAbsolute outPath >>= canonicalizePath
  ensure ((scratch ++ "/") `isPrefixOf` absoluteOut) "--out must be a fresh directory under private-data/scratch/PropertyService2"
  image4 <- readRegular 0x300000 image4Path
  candidate <- readRegular 0x300000 candidatePath
  ensure (sha256Hex (sha256 image4) == canonicalImage4Sha256) "--image4 is not the canonical X-M1 1.50 image4"
  [dat, loaderA, loaderB, image5, plain, image2] <- forM (zip [64 * 1024 * 1024, 0x20000, 0x20000, 0x800000, 0x800000, 0x800000] sourcePaths) (uncurry readRegular)
  linked <- readRegular (P.carrierBytes P.stage4Carrier) linkedPath
  symbols <- readRegular 0x10000 symbolsPath
  elf <- readRegular (16 * 1024 * 1024) elfPath
  contractBytes <- readRegular 0x1000000 contractPath
  contract <- either (\problem -> failH (contractPath ++ ": " ++ problem)) pure (eitherDecodeStrict' contractBytes :: Either String Value)
  let candidateSha = sha256Hex (sha256 candidate)
      contractSha = str (get contract "staged_image4_sha256")
  ensure (candidateSha == contractSha) ("candidate image4 sha256 " ++ candidateSha ++ " differs from the contract's staged_image4_sha256 " ++ contractSha)
  ensure (str (get contract "elf_sha256") == sha256Hex (sha256 elf)) "linked ELF differs from the contract's elf_sha256"
  source <- orFail "canonical sources" (canonicalSource dat image4 loaderA loaderB image5 plain image2)
  chosenCarrier <- maybe (failH "the hull scan leaves no carrier") pure (P.hullCarrier (sourceHull source))
  let classified = sourceProperties source
      properties = M.fromList [(propertyId property, property) | property <- classified]
  artifact <- orFail "linked artifact" (deriveArtifact ProductionArtifact classified chosenCarrier (ArtifactInput linked symbols elf))
  rederived <- orFail "staging" (stagedImage4Bytes source artifact)
  ensure (rederived == candidate) "candidate image4 differs from the image4 re-derived from the canonical sources and the linked artifact"
  defects <- fmap M.fromList . forM (defectArguments arguments) $ \(name, binPath, defectSymbolsPath, defectElfPath) -> do
    defectLinked <- readRegular (P.carrierBytes P.stage4Carrier) binPath
    defectSymbols <- readRegular 0x10000 defectSymbolsPath
    defectElf <- readRegular (16 * 1024 * 1024) defectElfPath
    defectArtifact <- orFail ("defect " ++ name) (deriveArtifact ProductionArtifact classified chosenCarrier (ArtifactInput defectLinked defectSymbols defectElf))
    defectImage <- orFail ("defect " ++ name ++ " staging") (stagedImage4Bytes source defectArtifact)
    ensure (defectImage /= candidate) ("defect " ++ name ++ " stages the candidate image4")
    let spans = ownedFunctions [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions defectArtifact]
    pure (DefectName name, (defectImage, spans, sha256Hex (sha256 defectLinked),artifactDefaultWindows defectArtifact))
  out <- freshOutput outPath
  let P.Carrier (P.GuestAddress carrierStart) (P.GuestAddress carrierEnd) = artifactCarrier artifact
      P.EntryAddresses (P.GuestAddress getterStub) (P.GuestAddress setterEntry) = artifactEntries artifact
      P.GuestAddress tableAddress = artifactTable artifact
      P.GuestAddress slotTableAddress = artifactSlotTable artifact
      P.GuestAddress specialTableAddress = artifactSpecialTable artifact
      ownedArms = [owned | Property _ _ _ (Owned owned) <- classified]
      fx = withConfigVariants ownedArms (fixture image4)
      ownedSpans = [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions artifact]
      candidateFunctions = ownedFunctions ownedSpans
      standing identifier = standingOf properties identifier
      tableWord (PropertyId n) = tableAddress + 4 * (n - 1)
      slotIndexOf identifier = listToMaybe [SlotIndex index | (index, Property other _ _ _) <- zip [0 ..] [p | p@(Property _ _ _ (Owned (OwnedSlotField _))) <- classified], other == identifier]
      specialArms = [(identifier, special) | Property identifier _ _ (Owned owned) <- classified, Just special <- [specialOf owned]]
      specialOf owned = case owned of
        OwnedField (FieldArm _ _ _ (FieldDefaultOnly _) _ NoHook) -> Nothing
        OwnedField arm -> Just (HookedField arm)
        OwnedReadOnlyField arm -> Just (ReadOnlyField arm)
        OwnedWriteOnlyPair arm -> Just (WriteOnlyPair arm)
        _ -> Nothing
      specialAddressOf identifier = listToMaybe [specialTableAddress + 8 * index | (index, (other, _)) <- zip [0 ..] specialArms, other == identifier]
      specialPatches name identifier before after = do
        address <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no SPECIALS entry")) Right (specialAddressOf identifier)
        EncodedSpecial before0 before1 <- either (Left . ((name ++ ": ") ++) . show) Right (encodeSpecial before)
        EncodedSpecial after0 after1 <- either (Left . ((name ++ ": ") ++) . show) Right (encodeSpecial after)
        Right ([Patch address before0 after0 | before0 /= after0] ++ [Patch (address + 4) before1 after1 | before1 /= after1])
      slotPatch name identifier before after = do
        SlotIndex slotIndex <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no slot index")) Right (slotIndexOf identifier)
        let EncodedSlot wordBefore = encodeSlot before
            EncodedSlot wordAfter = encodeSlot after
        Right [Patch (slotTableAddress + 4 * slotIndex) wordBefore wordAfter | wordBefore /= wordAfter]
      allVariants = CanonicalFixture : configVariantsOf
      acrossVariants identifier chosen = [RunCase variant (OracleCase identifier o) | variant <- allVariants, o <- chosen]
      armsWhere predicate = [(identifier, owned) | Property identifier _ _ (Owned owned) <- classified, predicate owned]
      stockDefaulted owned = isJust (stockOf owned)
      configDefaulted owned = case owned of
        OwnedField (FieldArm _ _ _ (FieldConfig _ _) _ _) -> True
        _ -> False
      gated owned = case owned of
        OwnedField (FieldArm _ _ _ _ _ (ConfigGatedResave _)) -> True
        _ -> False
      defaultRequests bank = [o | o@(Set b _ SetDefaultRequest) <- operations, maybe True (== b) bank]
      defectControl name defect cases note = case cases of
        [] -> Left (name ++ ": no property has the class the defect changes")
        _ -> Right (Control name MustFail (DefectBuild (DefectName defect)) (nub cases) note)
      stockSwapControl = case exposingStockSwap fx classified of
        Nothing -> Left "c-stock-swapped: no stock default exposes a swap"
        Just (Property identifier _ _ _, owned, swapped) -> do
          patches <- case (owned, swapped) of
            (OwnedField before, OwnedField after) -> specialPatches "c-stock-swapped" identifier (HookedField before) (HookedField after)
            (OwnedSlotField before, OwnedSlotField after) -> slotPatch "c-stock-swapped" identifier before after
            _ -> Left "c-stock-swapped: the swap changes the class"
          Right (Control "c-stock-swapped" MustFail (PatchedCandidate patches) (acrossVariants identifier defaultOperations)
            ("stock default of " ++ propertyHex identifier ++ " " ++ maybe "" stockLabel (stockOf owned) ++ " -> " ++ maybe "" stockLabel (stockOf swapped)))
      selectorControl = case [(identifier, arm) | (identifier, OwnedSlotField arm@(SlotArm _ _ _ (SlotStock (Stock61d65c Profile2)) _)) <- armsWhere (const True)] of
        (identifier, arm) : _ -> do
          patches <- slotPatch "c-selector-3" identifier arm arm {slotDefault = SlotStock (Stock61d65c Profile3)}
          Right (Control "c-selector-3" MustFail (PatchedCandidate patches) (acrossVariants identifier defaultOperations) ("0x61d65c selector 2 -> 3 for " ++ propertyHex identifier))
        [] -> Left "c-selector-3: no slot field calls 0x61d65c with selector 2"
      gateByteControl = case [(identifier, arm, gate) | (identifier, OwnedField arm@(FieldArm _ _ _ _ _ (ConfigGatedResave (ConfigOffset gate)))) <- armsWhere gated] of
        (identifier, arm, gate) : _ -> do
          patches <- specialPatches "c-gate-byte" identifier (HookedField arm) (HookedField arm {fieldHook = ConfigGatedResave (ConfigOffset (gate + 1))})
          Right (Control "c-gate-byte" MustFail (PatchedCandidate patches) (acrossVariants identifier [o | o@(Set {}) <- operations])
            ("gate byte " ++ hex (configBase + fromIntegral gate) ++ " -> " ++ hex (configBase + fromIntegral gate + 1) ++ " for " ++ propertyHex identifier))
        [] -> Left "c-gate-byte: no gated field"
      mirrorControl name chosen note = case [(identifier, arm) | (identifier, OwnedSlotField arm@(SlotArm _ _ _ _ MirrorRecord0)) <- armsWhere (const True)] of
        (identifier, arm) : _ -> do
          index <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no slot index")) Right (slotIndexOf identifier)
          let EncodedEntry before = encodeEntry (SlotEntry (bankArm arm) MirrorRecord0 index)
              EncodedEntry after = encodeEntry (SlotEntry (bankArm arm) NoMirror index)
          Right (Control name MustFail (PatchedCandidate [Patch (tableWord identifier) before after]) [canonical (OracleCase identifier o) | o <- chosen] (note ++ " for " ++ propertyHex identifier))
        [] -> Left (name ++ ": no record-0 mirrored slot field")
      readOnlyControl name change chosen note = case [(identifier, arm) | (identifier, OwnedReadOnlyField arm) <- armsWhere (const True)] of
        (identifier, arm) : _ -> do
          patches <- specialPatches name identifier (ReadOnlyField arm) (change arm)
          Right (Control name MustFail (PatchedCandidate patches) [canonical (OracleCase identifier o) | o <- chosen] (note ++ " for " ++ propertyHex identifier))
        [] -> Left (name ++ ": no read-only field")
      bumpedDefault arm = case fieldSource arm of
        FieldDefaultOnly (DefaultConstant value) -> ReadOnlyField arm {fieldSource = FieldDefaultOnly (DefaultConstant (value + 1))}
        _ -> ReadOnlyField arm
      candidateWord address = readWordAt candidate (fromIntegral (address - image4Base))
      bracketedIds chosen = [identifier | (identifier, _) <- armsWhere (\owned -> bracketedOwned owned && chosen owned)]
      pairArm owned = case owned of
        OwnedWriteOnlyPair _ -> True
        _ -> False
      setsOn bank identifiers = [canonical (OracleCase identifier o) | identifier <- identifiers, o@(Set b _ _) <- operations, b == bank]
      readBeforeOpen name chosen = case exposingOpenAfterSet fx chosen classified of
        Just (Property identifier _ _ _) -> defectControl name "storage_read_before_open" (setsOn Bank0 [identifier])
          ("defect build: reads bank 0 before opening, for " ++ propertyHex identifier)
        Nothing -> Left (name ++ ": no bracketed property exposes the open order")
      pairSecondControl = case [(identifier, arm) | (identifier, OwnedWriteOnlyPair arm) <- armsWhere (const True)] of
        (identifier, arm@(PairArm _ (BankOffset second) _ _)) : _ -> do
          patches <- specialPatches "c-pair-second-byte" identifier (WriteOnlyPair arm) (WriteOnlyPair arm {pairSecond = BankOffset (second + 1)})
          Right (Control "c-pair-second-byte" MustFail (PatchedCandidate patches) [canonical (OracleCase identifier o) | o@(Set {}) <- operations]
            ("SPECIALS second byte " ++ hex (fromIntegral second) ++ " -> " ++ hex (fromIntegral second + 1) ++ " for " ++ propertyHex identifier))
        [] -> Left "c-pair-second-byte: no write-only pair"
      bracketControls =
        [ defectControl "c-open-removed" "storage_open_removed" (setsOn Bank0 (bracketedIds (const True)))
            "defect build: bank-0 header open omitted"
        , defectControl "c-close-before-set" "storage_close_before_set" (setsOn Bank0 (bracketedIds (const True)))
            "defect build: header closed before the field store"
        , defectControl "c-bracket-bank1" "storage_bracket_bank1" (setsOn Bank1 (bracketedIds (const True)))
            "defect build: bank-1 stores open a header session"
        , pairSecondControl
        , readBeforeOpen "c-read-before-open-field" (not . pairArm)
        , readBeforeOpen "c-read-before-open-pair" pairArm
        , defectControl "c-pair-before-close" "storage_pair_before_close" (setsOn Bank0 [identifier | (identifier, _) <- armsWhere pairArm])
            "defect build: second pair store precedes header close"
        ]
      stage3Controls =
        [ stockSwapControl
        , selectorControl
        , defectControl "c-drop-provisional" "drop_provisional" [canonical (OracleCase identifier o) | (identifier, _) <- armsWhere stockDefaulted, o <- defaultRequests Nothing]
            "defect build: a stock default request skips the provisional save and store"
        , defectControl "c-drop-second-save" "drop_second_save" [canonical (OracleCase identifier o) | (identifier, _) <- armsWhere stockDefaulted, o <- defaultRequests (Just Bank0)]
            "defect build: a stock default request stores the leaf's result without the second save"
        , defectControl "c-config-masked" "mask_config_default" [RunCase variant (OracleCase identifier o) | (identifier, _) <- armsWhere configDefaulted, variant <- allVariants, o@(Get _ DefaultRead _) <- operations]
            "defect build: the getter's configuration default keeps only the field's bits"
        , defectControl "c-config-save" "config_save" [RunCase variant (OracleCase identifier o) | (identifier, _) <- armsWhere configDefaulted, variant <- allVariants, o <- defaultRequests (Just Bank0)]
            "defect build: the configuration default's store saves first"
        , defectControl "c-gate-inverted" "invert_gate" [RunCase variant (OracleCase identifier o) | (identifier, _) <- armsWhere gated, variant <- allVariants, o@(Set {}) <- operations]
            "defect build: the gated resave runs when the gate byte is not 0"
        , gateByteControl
        , mirrorControl "c-mirror-getter" [o | o@(Get {}) <- operations] "PROPERTIES record-0 mirror bit cleared; the getter reads the requested slot"
        , mirrorControl "c-mirror-write" [o | o@(Set {}) <- operations] "PROPERTIES record-0 mirror bit cleared; the set omits the record-0 write"
        , readOnlyControl "c-read-only-stores" (\arm -> HookedField arm) [o | o@(Set {}) <- operations] "SPECIALS subkind read-only -> hooked field; the set stores the field"
        , readOnlyControl "c-read-only-default" bumpedDefault [o | o@(Get _ DefaultRead _) <- operations] "SPECIALS default + 1"
        ]
          ++ bracketControls
      slotControl name control = case exposingSlot fx control classified of
        Nothing -> Left (name ++ ": no slot field exposes the mutation")
        Just (Property identifier _ _ _, arm, mutation) -> do
          index@(SlotIndex slotIndex) <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no slot index")) Right (slotIndexOf identifier)
          let cases = [canonical (OracleCase identifier o) | o <- slotControlOperations control]
              changed before after address = [Patch address before after | before /= after]
          case (mutation, control) of
            (MutatedArm mutated, _) -> do
              let EncodedEntry entryBefore = encodeEntry (SlotEntry (bankArm arm) (slotMirror arm) index)
                  EncodedEntry entryAfter = encodeEntry (SlotEntry (bankArm mutated) (slotMirror mutated) index)
                  EncodedSlot slotBefore = encodeSlot arm
                  EncodedSlot slotAfter = encodeSlot mutated
              Right (Control name MustFail (PatchedCandidate (changed entryBefore entryAfter (tableWord identifier) ++ changed slotBefore slotAfter (slotTableAddress + 4 * slotIndex))) cases ("PROPERTIES and SLOTS words of " ++ propertyHex identifier))
            (MutatedLayout _, RecordSavedAtBankOffset) -> defectControl name "storage_record_save_at_bank" cases "defect build: previous record stored at bank offset"
            (MutatedLayout _, SlotDefaultsMoved) -> defectControl name "storage_slot_defaults_moved" cases "defect build: slot defaults base moved"
            (MutatedLayout _, TailDefaultsMoved) -> defectControl name "storage_tail_defaults_moved" cases "defect build: tail defaults base moved"
            (MutatedLayout _, _) -> Left (name ++ ": the control changes no storage layout")
      slotControls =
        [ slotControl "c-record-offset" RecordOffsetMoved
        , slotControl "c-branches-swapped" BranchesSwapped
        , slotControl "c-slot1-default-record0" Slot1DefaultFromRecord0
        , slotControl "c-record-save-at-bank" RecordSavedAtBankOffset
        , slotControl "c-slot-defaults-base" SlotDefaultsMoved
        , slotControl "c-tail-base" TailDefaultsMoved
        ]
      entryControl name ec = case exposingField fx ec classified of
        Nothing -> Left (name ++ ": no owned field exposes the mutation")
        Just (Property identifier _ _ (Owned (OwnedField arm)), mutated) ->
          let EncodedEntry before = encodeEntry (FieldEntry arm)
              EncodedEntry after = encodeEntry (FieldEntry mutated)
           in Right (Control name MustFail (PatchedCandidate [Patch (tableWord identifier) before after]) [canonical (OracleCase identifier o) | o <- entryControlOperations ec] ("PROPERTIES entry of " ++ propertyHex identifier))
        Just (Property identifier _ _ _, _) -> Left (name ++ ": " ++ propertyHex identifier ++ " is not an owned field")
      halfTail = listToMaybe [p | p@(Property _ _ _ (Owned (OwnedField (FieldArm _ _ _ (FieldDefaultOnly (DefaultTail _ Half)) _ _)))) <- classified]
      getterPopControl = Right (Control "getter-adapter-wrong-target" MustFail
        (PatchedCandidate [Patch 0x61e110 (candidateWord 0x61e110) (branchWord 0x61e110 setterEntry)])
        [canonical (OracleCase (PropertyId 1) (Get Bank0 LiveRead (SlotArgument 0)))] "getter adapter branches to setter")
      tailHalfControl name defect note = case halfTail of
        Nothing -> Left (name ++ ": no field has a halfword tail default")
        Just (Property identifier _ _ _) -> defectControl name defect
          [RunCase variant (OracleCase identifier o) | variant <- [CanonicalFixture, TailWindow], o <- defaultOperations]
          (note ++ " for " ++ propertyHex identifier)
      tailHalfBase = tailHalfControl "c-tail16-base" "storage_tail16_moved" "defect build: halfword tail defaults base moved by 4"
      tailHalfByte = tailHalfControl "c-tail16-byte" "storage_tail16_byte" "defect build: halfword tail default read as a byte"
      stage4Defects =
        [ defectControl name defect [canonical (entryOperation c) | c <- entryCases, relevant c] ("stage4 " ++ name)
        | (name, defect, relevant) <-
            [ ("index-without-subtract", "index_without_subtract", \c -> oracleProperty (entryOperation c) `elem` map PropertyId [1, 645])
            , ("signed-bound", "signed_bound", \c -> oracleProperty (entryOperation c) `elem` map PropertyId [0, 0x80000000, 0xffffffff])
            , ("upper-bound-plus-one", "upper_bound_plus_one", \c -> oracleProperty (entryOperation c) == PropertyId 646)
            ]
        ]
      patched name patches cases = Right (Control name MustFail (PatchedCandidate patches) (map canonical cases) name)
      stage4Patches =
        [ patched "setter-adapter-wrong-target" [Patch 0x624694 (candidateWord 0x624694) (branchWord 0x624694 getterStub)]
            [OracleCase (PropertyId 1) (Set Bank0 (SlotArgument 0) SetOne)]
        , defectControl "bank-slot-swap" "storage_bank_slot_swap"
            [canonical (OracleCase (PropertyId 0xe9) (Set Bank1 (SlotArgument 2) SetOne))]
            "defect build: bank and slot arguments exchanged"
        ]
      linkedRefusal name patches =
        let local = [Patch (image4Base + address - carrierStart) before after | Patch address before after <- patches]
        in (name, case applyPatches local linked of
          Left _ -> False
          Right mutated -> isLeft (deriveArtifact ProductionArtifact classified chosenCarrier (ArtifactInput mutated symbols elf)))
      imageRefusal name patches = (name, case applyPatches patches candidate of
        Left _ -> False
        Right mutated -> isLeft (validateStagedImage4 (sourceHull source) artifact image4 mutated))
      at address after = Patch address (candidateWord address) after
      admissionControls :: [(String, Bool)]
      admissionControls =
        [ imageRefusal "adapter-bl-not-b" [at 0x61e110 (branchLinkWord 0x61e110 getterStub)]
        , linkedRefusal "setter-entry-not-tail" [at setterEntry movR0R0Word]
        , linkedRefusal "getter-entry-not-tail" [at getterStub 0xe12fff1e]
        , linkedRefusal "repeat-getter-tail" [at getterStub (branchWord getterStub 0x624400)]
        , linkedRefusal "repeat-setter-tail" [at setterEntry (branchWord setterEntry 0x630200)]
        , imageRefusal "restore-table-dispatch" [at 0x61e110 (readWordAt image4 (fromIntegral (0x61e110 - image4Base)))]
        , linkedRefusal "retired-arm-branch" [at getterStub (branchWord getterStub 0x61e184)]
        , imageRefusal "overwrite-retained-wrapper" [at 0x6245f8 movR0R0Word]
        , case [address | region@(P.Carrier (P.GuestAddress address) _) <- P.retainedRegions (sourceHull source), region /= P.retainedWrappers] of
            address : _ -> imageRefusal "overwrite-reserved-target" [at address (candidateWord address + 1)]
            [] -> ("overwrite-reserved-target", False)
        ]
      controls =
        [ entryControl "c-offset" OffsetMoved
        , entryControl "c-save" SaveCleared
        , entryControl "c-default" DefaultFlipped
        , entryControl "c-tail-offset" TailOffsetMoved
        , getterPopControl
        , tailHalfBase
        , tailHalfByte
        ]
          ++ slotControls
          ++ stage3Controls
          ++ stage4Defects ++ stage4Patches
      planned = [control | Right control <- controls]
      unavailable = [problem | Left problem <- controls]
      inSubset identifier = maybe True (S.member identifier) subset
      variantCases =
        [RunCase TailWindow (OracleCase identifier o) | Just (Property identifier _ _ _) <- [halfTail], inSubset identifier, o <- defaultOperations]
          ++ [RunCase variant (OracleCase identifier o) | Property identifier _ _ (Owned owned) <- classified, inSubset identifier, variant <- configVariantsOf, o <- variantOperations owned]
      gridCases = [canonical c | c <- caseGrid, inSubset (oracleProperty c)] ++ variantCases
      runCases = nub (gridCases ++ concatMap controlCases planned)
      -- Every grid case already enters through the adapters; call-context cases run for ids 1 and 645, the
      -- lowest id of each class, ids 0 and 646, and, with 646, the raw ids above it.
      entryRepresentatives = S.fromList (PropertyId 1 : PropertyId 645 : M.elems (M.fromListWith min [(classLabel (standing (PropertyId n)), PropertyId n) | n <- [1 .. 645]]))
      adapterCases = [c | c <- entryCases, let identifier@(PropertyId raw) = oracleProperty (entryOperation c), if raw > 646 then inSubset (PropertyId 646) else inSubset identifier && (raw == 0 || raw == 646 || S.member identifier entryRepresentatives)]
      fxOf rc = variantFixture fx (runVariant rc)

  -- Pass 1: stock.
  (stockBatches, stockRuns) <- runBatches cache runner out "stock" [stockSpec (fxOf rc) image4 properties (runLabel rc) stockFunctions (runCase rc) | rc <- runCases]
  let stockRun rc = lookupRun stockRuns (runLabel rc)
  modelRows <- fmap concat . forM runCases $ \rc -> case modelled (standing (runProperty rc)) of
    Nothing -> pure []
    Just owned -> do
      run <- stockRun rc
      pure [(rc, modelDifferences (fxOf rc) owned (oracleOperation (runCase rc)) run)]

  -- Pass 2: candidate against stock.
  candidateSpecs <- forM runCases $ \c -> do
    run <- stockRun c
    pure (caseSpec (fixtureImage (artifactDefaultWindows artifact) (fxOf c) candidate) candidateFunctions (fxOf c) (runLabel c) (standing (runProperty c)) (runCase c) (observedExpectation run))
  (candidateBatches, candidateRuns) <- runBatches cache runner out "candidate" candidateSpecs
  verdicts <- forM runCases $ \c -> do
    reference <- stockRun c
    subject <- lookupRun candidateRuns (runLabel c)
    pure (c, compareRuns reference subject)
  let entryStanding c = standing (oracleProperty (entryOperation c))
      entryExpected c = fromMaybe placeholderExpectation (modelled (entryStanding c) >>= \owned -> modelExpectation fx owned (oracleOperation (entryOperation c)))
  (entryStockBatches, entryStockRuns) <- runBatches cache runner out "entry-stock"
    [entryCaseSpec image4 stockFunctions fx (entryLabel c) (entryStanding c) c (entryExpected c) | c <- adapterCases]
  (entryCandidateBatches, entryCandidateRuns) <- runBatches cache runner out "entry-candidate"
    [entryCaseSpec candidate candidateFunctions fx (entryLabel c) (entryStanding c) c (entryExpected c) | c <- adapterCases]
  entryVerdicts <- forM adapterCases $ \c -> do
    reference <- lookupRun entryStockRuns (entryLabel c)
    subject <- lookupRun entryCandidateRuns (entryLabel c)
    pure (c, compareRuns reference subject ++ case modelled (entryStanding c) of
      Nothing -> []
      Just owned -> modelDifferences fx owned (oracleOperation (entryOperation c)) subject)
  let wrappers = [(wrapper, OracleCase identifier (Get Bank0 LiveRead (SlotArgument 0))) | wrapper <- [minBound .. maxBound], identifier <- propertyIds, inSubset identifier]
      wrapperLabel wrapper c = CaseLabel ("wrapper-" ++ show (fromEnum wrapper) ++ "-" ++ caseLabelText (caseLabel c))
      wrapperSpecs image functions = [wrapperCaseSpec wrapper image functions fx (wrapperLabel wrapper c) (standing (oracleProperty c)) c | (wrapper, c) <- wrappers]
  (wrapperStockBatches, wrapperStockRuns) <- runBatches cache runner out "wrapper-stock" (wrapperSpecs image4 stockFunctions)
  (wrapperCandidateBatches, wrapperCandidateRuns) <- runBatches cache runner out "wrapper-candidate" (wrapperSpecs candidate candidateFunctions)
  wrapperVerdicts <- forM wrappers $ \(wrapper, c) -> do
    let label = wrapperLabel wrapper c
    reference <- lookupRun wrapperStockRuns label
    subject <- lookupRun wrapperCandidateRuns label
    pure (label, compareRuns reference subject)
  entryControlImages <- forM
    [("nonzero-means-default", "nonzero_means_default"), ("sentinel-plus-one", "sentinel_plus_one")] $ \(name, defect) ->
      case M.lookup (DefectName defect) defects of
        Nothing -> failH ("no --defect " ++ defect)
        Just (image, spans, _, windows) -> pure (name, fixtureImage windows fx image, spans)
  veneerImages <- forM [("id-from-r12", 0xe1a0000c), ("clobber-r4-or-r11", 0xe3a0b000)] $ \(name, instruction) -> do
    image <- orFail name (applyPatches [at 0x61e110 (branchWord 0x61e110 0x61e114), at 0x61e114 instruction, at 0x61e118 (branchWord 0x61e118 getterStub)] candidate)
    pure (name, image, candidateFunctions ++ [FunctionSpan 0x61e114 0x61e11c])
  let entryControlCases name = [c | c <- entryCases, entryCallMode c == DirectCall, not (entryReturnVariant c),
        oracleProperty (entryOperation c) == PropertyId (if name `elem` ["id-from-r12", "clobber-r4-or-r11"] then 1 else 3)]
      controlEntryLabel name c = CaseLabel ("ec-" ++ take 3 name ++ "-" ++ caseLabelText (caseLabel (entryOperation c)) ++ "-" ++ showHex (entrySelector c) "")
  (entryControlBatches, entryControlRuns) <- runBatches cache runner out "entry-controls"
    [entryCaseSpec image spans fx (controlEntryLabel name c) (entryStanding c) c (entryExpected c)
    | (name, image, spans) <- entryControlImages ++ veneerImages, c <- entryControlCases name]
  entryControlOutcomes <- forM (entryControlImages ++ veneerImages) $ \(name, _, _) -> do
    rows <- forM (entryControlCases name) $ \c -> do
      subject <- lookupRun entryControlRuns (controlEntryLabel name c)
      let differences = expectationDifferences (entryExpected c) subject
      pure (subject, differences)
    let failed = length [() | (_, differences) <- rows, not (null differences)]
        statuses = map (runnerStatusLabel . runStatus . fst) rows
        holds = failed > 0 && all ((== Returned) . runStatus . fst) rows
    pure (object ["name" .= name, "holds" .= holds, "failed_cases" .= failed, "statuses" .= statuses], holds)

  -- Guest controls.
  controlImages <- forM planned $ \control -> case controlImage control of
    PatchedCandidate patches -> do
      image <- either (failH . ((controlName control ++ ": ") ++)) pure (applyPatches patches candidate)
      pure (control, image, candidateFunctions,artifactDefaultWindows artifact)
    DefectBuild defect@(DefectName name) -> case M.lookup defect defects of
      Just (image, spans, _, windows) -> pure (control, image, spans,windows)
      Nothing -> failH (controlName control ++ ": no --defect " ++ name)
  controlSpecs <- forM (concat [[(control, image, spans, windows, c) | c <- controlCases control] | (control, image, spans, windows) <- controlImages]) $ \(control, image, spans, windows, c) -> do
    run <- stockRun c
    pure (caseSpec (fixtureImage windows (fxOf c) image) spans (fxOf c) (runPrefixed (controlName control) c) (standing (runProperty c)) (runCase c) (observedExpectation run))
  (controlBatches, controlRuns) <- runBatches cache runner out "controls" controlSpecs
  controlOutcomes <- forM controlImages $ \(control, _, _, _) -> do
    failures <- fmap concat . forM (controlCases control) $ \c -> do
      reference <- stockRun c
      subject <- lookupRun controlRuns (runPrefixed (controlName control) c)
      pure [(runPrefixed (controlName control) c, d) | d <- compareRuns reference subject]
    let failedCases = M.size (M.fromListWith (++) [(label, [d]) | (label, d) <- failures])
    pure (control, failedCases, failures)
  let controlsHold = null unavailable && and [controlHolds control failed | (control, failed, _) <- controlOutcomes]
        && and (map snd admissionControls) && and (map snd entryControlOutcomes)
        && all ((== Returned) . runStatus) (M.elems controlRuns)

  -- Results.
  runnerSha <- sha256Hex <$> (canonicalizePath runner >>= sha256File)
  inputs <- forM
    ( [("image4", image4Path), ("candidate_image4", candidatePath), ("contract", contractPath), ("linked", linkedPath), ("symbols", symbolsPath), ("elf", elfPath)]
        ++ zip ["dat", "loader_a", "loader_b", "image5", "plain", "image2"] sourcePaths
        ++ concat [[("defect_" ++ name, binPath), ("defect_" ++ name ++ "_symbols", defectSymbolsPath), ("defect_" ++ name ++ "_elf", defectElfPath)] | (name, binPath, defectSymbolsPath, defectElfPath) <- defectArguments arguments]
    ) $ \(name, path) -> do
    digest <- sha256Hex <$> sha256File path
    pure (fromString name .= object ["path" .= path, "sha256" .= digest])
  let modelFailures = [(c, ds) | (c, ds) <- modelRows, not (null ds)]
      verdictFailures = [(c, ds) | (c, ds) <- verdicts, not (null ds)]
      modelledIds = M.keys (M.fromList [(runProperty c, ()) | (c, _) <- modelRows])
      disagreeingIds = M.keys (M.fromList [(runProperty c, ()) | (c, _) <- modelFailures])
      perId = M.fromListWith (+) [(runProperty c, 1 :: Int) | (c, _) <- verdictFailures]
      modelPerId = M.fromListWith (+) [(runProperty c, 1 :: Int) | (c, _) <- modelFailures]
      classOf identifier = classLabel (standing identifier)
      byClass = M.fromListWith (\(a, b, c) (d, e, f) -> (a + d, b + e, c + f))
        [(classOf (runProperty c), (1 :: Int, if null ds then 1 else 0 :: Int, if null ds then 0 else 1 :: Int)) | (c, ds) <- verdicts]
      runIds = M.keys (M.fromList [(runProperty c, ()) | c <- runCases])
      stockAll = M.elems stockRuns
      candidateAll = M.elems candidateRuns
      passed = null modelFailures && null verdictFailures && all (null . snd) entryVerdicts && all (null . snd) wrapperVerdicts && controlsHold
      allBatches = stockBatches ++ candidateBatches ++ controlBatches ++ entryStockBatches ++ entryCandidateBatches ++ entryControlBatches ++ wrapperStockBatches ++ wrapperCandidateBatches
      executedBatches = length [() | record <- allBatches, recordReuse record == BatchExecuted]
      reusedBatches = length [() | record <- allBatches, recordReuse record == BatchReused]
      executedCases = sum [recordCases record | record <- allBatches, recordReuse record == BatchExecuted]
      reusedCases = sum [recordCases record | record <- allBatches, recordReuse record == BatchReused]
      batchValue record = object ["name" .= recordName record, "cases" .= recordCases record, "batch_sha256" .= recordBatchSha record, "summary_sha256" .= recordSummarySha record, "reuse" .= reuseLabel (recordReuse record)]
      results = object
        [ "schema_version" .= (1 :: Int)
        , "inputs" .= object inputs
        , "contract_staged_image4_sha256" .= contractSha
        , "runner" .= object ["path" .= runner, "sha256" .= runnerSha]
        , "execution" .= object
            [ "cache" .= cache
            , "batches_executed" .= executedBatches
            , "batches_reused" .= reusedBatches
            , "cases_executed" .= executedCases
            , "cases_reused" .= reusedCases
            ]
        , "batches" .= map batchValue allBatches
        , "wrappers" .= object ["cases" .= length wrappers, "failed" .= length [() | (_, ds) <- wrapperVerdicts, not (null ds)]]
        , "entry_adapters" .= object ["cases" .= length adapterCases, "failed" .= length [() | (_, ds) <- entryVerdicts, not (null ds)]]
        , "case_shape" .= object
            [ "ids" .= maybe ("0..646" :: String) (\chosen -> unwords (map propertyHex (S.toList chosen))) subset
            , "subset" .= isJust subset
            , "fixture_variants" .=
                [ object
                    [ "variant" .= variantTag variant
                    , "note" .= variantNote variant
                    , "cases" .= length [() | rc <- runCases, runVariant rc == variant]
                    ]
                | variant <- [minBound .. maxBound]
                ]
            , "cases_per_id" .= length operations, "grid_cases" .= length gridCases, "cases_per_role" .= length runCases
            , "carrier" .= object ["start" .= hex carrierStart, "end" .= hex carrierEnd]
            , "entries" .= object ["getter" .= hex getterStub, "setter" .= hex setterEntry, "properties" .= hex tableAddress]
            , "candidate_functions" .= [object ["start" .= hex s, "end" .= hex e] | FunctionSpan s e <- candidateFunctions]
            ]
        , "totals" .= object
            [ "classes" .= object [fromString name .= object ["cases" .= n, "pass2_passed" .= p, "pass2_failed" .= f] | (name, (n, p, f)) <- M.toList byClass]
            , "ids" .=
                [ object
                    [ "property" .= propertyHex identifier
                    , "class" .= classOf identifier
                    , "reason" .= case standing identifier of
                        Classified (Delegated reason) -> Just (reasonLabel reason)
                        _ -> Nothing
                    , "model_disagreeing_cases" .= (if isJust (modelled (standing identifier)) then Just (M.findWithDefault 0 identifier modelPerId) else Nothing)
                    , "pass2_failed_cases" .= M.findWithDefault 0 identifier perId
                    ]
                | identifier <- runIds
                ]
            ]
        , "model_agreement" .= object
            [ "modelled_ids" .= length modelledIds, "disagreeing_ids" .= map propertyHex disagreeingIds
            , "modelled_cases" .= length modelRows, "disagreeing_cases" .= length modelFailures
            ]
        , "pass2" .= object ["passed" .= (length verdicts - length verdictFailures), "failed" .= length verdictFailures]
        , "status_distribution" .= object ["stock" .= statusCounts stockAll, "candidate" .= statusCounts candidateAll]
        , "controls" .=
            ( [ object
                  [ "name" .= controlName control
                  , "kind" .= (case controlKind control of MustFail -> "must-fail"; MustPass -> "must-pass" :: String)
                  , "note" .= controlNote control
                  , "image" .= case controlImage control of
                      PatchedCandidate patches -> object ["patches" .= [object ["address" .= hex a, "before" .= hex b, "after" .= hex c] | Patch a b c <- patches]]
                      DefectBuild defect@(DefectName name) -> object ["defect" .= name, "linked_sha256" .= fmap (\(_, _, digest, _) -> digest) (M.lookup defect defects)]
                  , "cases" .= length (controlCases control)
                  , "failed_cases" .= failed
                  , "holds" .= controlHolds control failed
                  , "first_differences" .= [object ["case" .= caseLabelText label, "kind" .= differenceKind d, "detail" .= differenceDetail d] | (label, d) <- take 20 failures]
                  ]
              | (control, failed, failures) <- controlOutcomes
              ]
                ++ [object ["name" .= problem, "holds" .= False] | problem <- unavailable]
                ++ [object ["name" .= name, "kind" .= ("admission-refusal" :: String), "holds" .= holds] | (name, holds) <- admissionControls]
                ++ map fst entryControlOutcomes
            )
        , "differences" .= differenceSamples
            ( [("model", runLabel c, d) | (c, ds) <- modelFailures, d <- ds]
                ++ [("pass2", runLabel c, d) | (c, ds) <- verdictFailures, d <- ds]
                ++ [("entry", entryLabel c, d) | (c, ds) <- entryVerdicts, d <- ds]
            )
        , "passed" .= passed
        ]
  B.writeFile (out </> "results.json") (encodeCanonical results)
  putStrLn ("model: " ++ show (length modelFailures) ++ " disagreeing of " ++ show (length modelRows) ++ " owned-id stock cases")
  putStrLn ("pass 2: " ++ show (length verdictFailures) ++ " failing of " ++ show (length verdicts))
  forM_ controlOutcomes $ \(control, failed, _) -> putStrLn (controlName control ++ ": " ++ show failed ++ " failing of " ++ show (length (controlCases control)))
  forM_ unavailable putStrLn
  putStrLn ("results: " ++ out </> "results.json")
  unless passed exitFailure

-- | One batch of 64 cases of the canonical image4: 16 operations over every slot argument of the first
-- slot field whose branches' bits differ and of the first owned field, once with the stock functions
-- and once with the setter split around the largest arm-aligned owned interval.
smoke :: [String] -> FilePath -> IO ()
smoke arguments runner = do
  image4Path <- required arguments "--image4"
  outPath <- required arguments "--out"
  image4 <- readRegular 0x300000 image4Path
  ensure (sha256Hex (sha256 image4) == canonicalImage4Sha256) "--image4 is not the canonical X-M1 1.50 image4"
  classified <- either (\problem -> failH ("classification: " ++ problem)) pure (classify (Image "image4" "" 0x61c400 image4))
  let P.Carrier (P.GuestAddress carrierStart) (P.GuestAddress carrierEnd) = P.stage4Carrier
  let properties = M.fromList [(propertyId property, property) | property <- classified]
      fx = fixture image4
      differing (SlotArm (BankBranch _ bankWidth' bankBits' _) (RecordBranch _ recordWidth' recordBits') _ _ _) = (bankWidth', bankBits') /= (recordWidth', recordBits')
      chosen = take 1 [propertyId p | p@(Property _ _ _ (Owned (OwnedSlotField arm))) <- classified, differing arm] ++ take 1 [propertyId p | p@(Property _ _ _ (Owned (OwnedField _))) <- classified]
      slot = SlotArgument
      smokeOperations =
        [Get Bank0 LiveRead s | s <- slotArguments] ++ [Get Bank1 DefaultRead (slot s) | s <- [0, 1, 3]]
          ++ [Set Bank0 (slot s) SetFieldMax | s <- [0, 1, 0xffffffff]] ++ [Set Bank1 (slot s) SetDefaultRequest | s <- [1, 2, 3]]
          ++ [Set Bank0 (slot 0) SetDefaultRequest, Set Bank1 (slot 0xffffffff) SetAboveMax]
      cases = [OracleCase identifier o | identifier <- chosen, o <- smokeOperations]
      split = splitFunctions (carrierStart, carrierEnd) [FunctionSpan carrierStart carrierEnd]
      specs = [stockSpec fx image4 properties (prefixed "stock" c) stockFunctions c | c <- cases] ++ [stockSpec fx image4 properties (prefixed "split" c) split c | c <- cases]
  out <- freshOutput outPath
  (record, runs) <- runBatch Nothing runner (out </> "smoke") specs
  rows <- forM cases $ \c -> do
    stockRun <- lookupRun runs (prefixed "stock" c)
    splitRun <- lookupRun runs (prefixed "split" c)
    let model = (\owned -> modelDifferences fx owned (oracleOperation c) stockRun) <$> modelled (standingOf properties (oracleProperty c))
        verdict = compareRuns stockRun splitRun
    putStrLn (caseLabelText (caseLabel c) ++ " stock " ++ runnerStatusLabel (runStatus stockRun) ++ " split " ++ runnerStatusLabel (runStatus splitRun)
      ++ maybe "" (\ds -> " model " ++ (if null ds then "agrees" else unwords (map differenceDetail ds))) model
      ++ " verdict " ++ (if null verdict then "equal" else unwords (map differenceDetail verdict)))
    pure (c, model, verdict)
  let results = object
        [ "batch" .= object ["cases" .= recordCases record, "batch_sha256" .= recordBatchSha record, "summary_sha256" .= recordSummarySha record]
        , "cases" .=
            [ object ["case" .= caseLabelText (caseLabel c), "model_differences" .= fmap (map differenceDetail) model, "split_differences" .= map differenceDetail verdict]
            | (c, model, verdict) <- rows
            ]
        ]
  B.writeFile (out </> "results.json") (encodeCanonical results)
  let failures = length [() | (_, model, verdict) <- rows, maybe False (not . null) model || not (null verdict)]
  putStrLn (show failures ++ " of " ++ show (length rows) ++ " smoke cases differ")
  when (failures > 0 || isNothing (listToMaybe rows)) exitFailure
