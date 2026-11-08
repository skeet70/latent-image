{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Runs the stock getter 0x61e110 and setter 0x624694 from the canonical image4 and from the staged
-- candidate image4 for every property id, or for the ids `--ids` lists, checks the stock runs of
-- owned ids against the owned model, compares the candidate with stock case by case and runs the
-- guest controls.
module Main (main) where

import qualified A32 as A
import Control.Exception (evaluate)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, eitherDecodeStrict', object, (.=))
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (isPrefixOf, nub)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, isNothing, listToMaybe)
import qualified Data.Set as S
import Data.String (fromString)
import Data.Word (Word32)
import Emulation.CachedNativeBatch (BatchReuse (..), cachedObservedBatch)
import Emulation.NativeObservation (runnerStatusLabel)
import Emulation.OwnedCase (CaseId (..), CaseSpec, loadObservedBatch, readWordAt, required, writeBatch)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import FujiAF.Host (ensure, failH, get, readRegular, str)
import FujiAF.JSON (encodeCanonical)
import FujiModNativePaths (fujimodNativeCase)
import Numeric (readHex, showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Admission (FunctionExtent (..), FunctionName (..))
import Replacement.PropertyService.Artifact
import Replacement.PropertyService.ClassRecord (ownedLabel, reasonLabel, stockLabel)
import Replacement.PropertyService.Classify
import Replacement.PropertyService.Encoding (EncodedEntry (..), EncodedSlot (..), EncodedSpecial (..), SlotIndex (..), SpecialArm (..), TableEntry (..), encodeEntry, encodeSlot, encodeSpecial)
import Replacement.PropertyService.Oracle
import qualified Replacement.PropertyService.Placement as P
import ServiceInventory (Image (..))
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesPathExist, makeAbsolute, setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath (takeDirectory, takeFileName, (</>))

canonicalImage4Sha256 :: String
canonicalImage4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

batchLimit :: Int
batchLimit = 4096

-- | The stock pops of the getter's and setter's prologue frames.
getterPopWord, setterPopWord :: Word32
getterPopWord = 0xe8bd8830
setterPopWord = 0xe8bd89f8

-- | One word of a setter-stub layout: an instruction, a BL to an address, or a BEQ to a word index.
data StubWord = Fixed Word32 | CallTo Word32 | IfZeroTo Int

-- | The Rust entry points the setter stub calls: `xm1_set_bracketed`, `xm1_property_set` and
-- `xm1_property_set_after`.
data StubCallees = StubCallees Word32 Word32 Word32

stubWordAt :: Word32 -> Int -> StubWord -> Word32
stubWordAt stub index stubWord = case stubWord of
  Fixed fixed -> fixed
  CallTo target -> branchLinkWord site target
  IfZeroTo target -> 0x0a000000 .|. (((stub + 4 * fromIntegral target - site - 8) `shiftR` 2) .&. 0xffffff)
  where
    site = stub + 4 * fromIntegral index

openWord, closeWord :: Word32
openWord = 0x61da3c
closeWord = 0x61d944

-- | The setter stub of entry.S, and two control layouts of the same length: the open call after
-- `xm1_property_set`, so that the set reads bank 0 before the open; and the close before it.
setterStubLayout, openAfterSetLayout, closeBeforeSetLayout :: StubCallees -> [StubWord]
setterStubLayout (StubCallees bracketed set after) =
  [ Fixed 0xe1a00004, Fixed 0xe1a01006, CallTo bracketed, Fixed 0xe1a07000, Fixed 0xe3570000, IfZeroTo 9, Fixed 0xe30c0c64, Fixed 0xe3400096, CallTo openWord
  , Fixed 0xe1a00004, Fixed 0xe1a01005, Fixed 0xe1a02006, Fixed 0xe1a03008, CallTo set, Fixed 0xe3570000, IfZeroTo 23
  , Fixed 0xe30c0c64, Fixed 0xe3400096, CallTo closeWord, Fixed 0xe1a00004, Fixed 0xe1a01005, Fixed 0xe1a02006, CallTo after, Fixed setterPopWord
  ]
openAfterSetLayout (StubCallees bracketed set after) =
  [ Fixed 0xe1a00004, Fixed 0xe1a01006, CallTo bracketed, Fixed 0xe1a07000
  , Fixed 0xe1a00004, Fixed 0xe1a01005, Fixed 0xe1a02006, Fixed 0xe1a03008, CallTo set, Fixed 0xe3570000, IfZeroTo 23
  , Fixed 0xe30c0c64, Fixed 0xe3400096, CallTo openWord, Fixed 0xe30c0c64, Fixed 0xe3400096, CallTo closeWord
  , Fixed 0xe1a00004, Fixed 0xe1a01005, Fixed 0xe1a02006, CallTo after, Fixed movR0R0Word, Fixed movR0R0Word, Fixed setterPopWord
  ]
closeBeforeSetLayout (StubCallees bracketed set after) =
  [ Fixed 0xe1a00004, Fixed 0xe1a01006, CallTo bracketed, Fixed 0xe1a07000, Fixed 0xe3570000, IfZeroTo 12
  , Fixed 0xe30c0c64, Fixed 0xe3400096, CallTo openWord, Fixed 0xe30c0c64, Fixed 0xe3400096, CallTo closeWord
  , Fixed 0xe1a00004, Fixed 0xe1a01005, Fixed 0xe1a02006, Fixed 0xe1a03008, CallTo set, Fixed 0xe3570000, IfZeroTo 23
  , Fixed 0xe1a00004, Fixed 0xe1a01005, Fixed 0xe1a02006, CallTo after, Fixed setterPopWord
  ]

-- | `mov r0, r0` and `mov r1, #0`. The native runner refuses `nop`.
movR0R0Word, movR1ZeroWord :: Word32
movR0R0Word = 0xe1a00000
movR1ZeroWord = 0xe3a01000

-- | `c-tail16-base`'s base: the halfword at 0x81b078 is 0x1d in image4, where 0x1e6's default at
-- 0x81b074 is 0, or 0x5a00 under `TailWindow`.
tailHalfMovedBase :: Word32
tailHalfMovedBase = tailDefaultsBase + 4

-- | `ldrh r0, [r12, r0]` and `ldrb r0, [r12, r0]`.
loadHalfWord, loadByteWord :: Word32
loadHalfWord = 0xe19c00b0
loadByteWord = 0xe7dc0000

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
-- A cache root reuses only batches whose complete observations are proven for the current cases,
-- runner and artifacts; the current comparisons run on the returned observations either way.
runBatch :: Maybe FilePath -> FilePath -> FilePath -> [CaseSpec] -> IO (BatchRecord, M.Map CaseLabel Run)
runBatch cache runner directory specs = do
  ensure (length specs <= batchLimit) "batch over the case limit"
  batchPath <- writeBatch directory specs
  (reuse, observed) <- case cache of
    Nothing -> (BatchExecuted,) <$> loadObservedBatch runner batchPath
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

-- | Decodes each control word with the independent A32 decoder before it is used.
checkEncodings :: [(Word32, Word32)] -> IO ()
checkEncodings bases = do
  let always = A.Cond 14
      stack registers = A.LoadMultiple always A.sp A.IncrementAfter True (map A.Reg registers)
  ensure (A.decode 0 getterPopWord == stack [4, 5, 11, 15]) "pop {r4, r5, r11, pc} encoding"
  ensure (A.decode 0 setterPopWord == stack [3, 4, 5, 6, 7, 8, 11, 15]) "pop {r3-r8, r11, pc} encoding"
  ensure (A.decode 0 loadHalfWord == A.Load always A.Half False (A.Reg 0) (A.Reg 12) (A.OffsetRegister True (A.Reg 0) A.LSL 0) A.OffsetOnly) "ldrh r0, [r12, r0] encoding"
  ensure (A.decode 0 loadByteWord == A.Load always A.Byte False (A.Reg 0) (A.Reg 12) (A.OffsetRegister True (A.Reg 0) A.LSL 0) A.OffsetOnly) "ldrb r0, [r12, r0] encoding"
  forM_ bases $ \(site, base) ->
    ensure (A.decode site (movwWord base) == A.MoveWide always (A.Reg 12) (base .&. 0xffff) && A.decode (site + 4) (movtWord base) == A.MoveTop always (A.Reg 12) (base `shiftR` 16))
      ("movw/movt r12, " ++ hex base ++ " encoding")

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
  bundled <- if "--smoke" `elem` arguments then pure Nothing else Just <$> mapM (\name -> required arguments name >>= makeAbsolute) ["--linked", "--symbols"]
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  case bundled of
    Nothing -> smoke arguments runner
    Just [linkedPath, symbolsPath] -> oracle arguments runner linkedPath symbolsPath
    Just _ -> failH "unreachable argument shape"

-- | The optional native batch cache root. Absent keeps the uncached one-shot batch behavior.
cacheOption :: [String] -> IO (Maybe FilePath)
cacheOption arguments = case dropWhile (/= "--cache") arguments of
  _ : value : _ -> Just <$> makeAbsolute value
  [_] -> failH "--cache needs a value"
  [] -> pure Nothing

freshOutput :: FilePath -> IO FilePath
freshOutput path = do
  exists <- doesPathExist path
  when exists (failH (path ++ " already exists"))
  absolute <- makeAbsolute path
  createDirectoryIfMissing True (takeDirectory absolute)
  createDirectory absolute
  pure absolute

-- | Each `--defect NAME BIN SYMBOLS`: a defect build's name and its linked image and symbol list.
defectArguments :: [String] -> [(String, FilePath, FilePath)]
defectArguments arguments = case arguments of
  "--defect" : name : binPath : symbolsPath : rest -> (name, binPath, symbolsPath) : defectArguments rest
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

oracle :: [String] -> FilePath -> FilePath -> FilePath -> IO ()
oracle arguments runner linkedPath symbolsPath = do
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
  linked <- readRegular 0x2000 linkedPath
  symbols <- readRegular 0x10000 symbolsPath
  contractBytes <- readRegular 0x1000000 contractPath
  contract <- either (\problem -> failH (contractPath ++ ": " ++ problem)) pure (eitherDecodeStrict' contractBytes :: Either String Value)
  let candidateSha = sha256Hex (sha256 candidate)
      contractSha = str (get contract "staged_image4_sha256")
  ensure (candidateSha == contractSha) ("candidate image4 sha256 " ++ candidateSha ++ " differs from the contract's staged_image4_sha256 " ++ contractSha)
  source <- orFail "canonical sources" (canonicalSource dat image4 loaderA loaderB image5 plain image2)
  chosenCarrier <- maybe (failH "the hull scan leaves no carrier") pure (P.hullCarrier (sourceHull source))
  let classified = sourceProperties source
      properties = M.fromList [(propertyId property, property) | property <- classified]
  artifact <- orFail "linked artifact" (deriveArtifact classified chosenCarrier linked symbols)
  rederived <- orFail "staging" (stagedImage4Bytes source artifact)
  ensure (rederived == candidate) "candidate image4 differs from the image4 re-derived from the canonical sources and the linked artifact"
  defects <- fmap M.fromList . forM (defectArguments arguments) $ \(name, binPath, defectSymbolsPath) -> do
    defectLinked <- readRegular 0x2000 binPath
    defectSymbols <- readRegular 0x10000 defectSymbolsPath
    defectArtifact <- orFail ("defect " ++ name) (deriveArtifact classified chosenCarrier defectLinked defectSymbols)
    defectImage <- orFail ("defect " ++ name ++ " staging") (stagedImage4Bytes source defectArtifact)
    ensure (defectImage /= candidate) ("defect " ++ name ++ " stages the candidate image4")
    let P.Carrier (P.GuestAddress low) (P.GuestAddress high) = artifactCarrier defectArtifact
        spans = splitFunctions (low, high) [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions defectArtifact]
    pure (DefectName name, (defectImage, spans, sha256Hex (sha256 defectLinked)))
  out <- freshOutput outPath
  let P.Carrier (P.GuestAddress carrierStart) (P.GuestAddress carrierEnd) = artifactCarrier artifact
      P.StubAddresses (P.GuestAddress getterStub) (P.GuestAddress setterStub) = artifactStubs artifact
      P.GuestAddress tableAddress = artifactTable artifact
      P.GuestAddress slotTableAddress = artifactSlotTable artifact
      P.GuestAddress specialTableAddress = artifactSpecialTable artifact
      ownedArms = [owned | Property _ _ _ (Owned owned) <- classified]
      fx = withConfigVariants ownedArms (fixture image4)
      P.GuestAddress getterTable = P.getterTableBase
      P.GuestAddress setterTable = P.setterTableBase
      ownedSpans = [FunctionSpan start end | FunctionExtent _ (P.GuestAddress start) (P.GuestAddress end) <- artifactFunctions artifact]
      candidateFunctions = splitFunctions (carrierStart, carrierEnd) ownedSpans
      standing identifier = standingOf properties identifier
      tableWord (PropertyId n) = tableAddress + 4 * (n - 1)
      accessorAt name = listToMaybe [start | FunctionExtent (FunctionName functionName) (P.GuestAddress start) _ <- artifactFunctions artifact, functionName == name]
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
        _ -> Right (Control name MustFail (DefectBuild (DefectName defect)) cases note)
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
      stubCallees = StubCallees <$> accessorAt "xm1_set_bracketed" <*> accessorAt "xm1_property_set" <*> accessorAt "xm1_property_set_after"
      layoutWords layout = zipWith (stubWordAt setterStub) [0 ..] layout
      setterPop = setterStub + 4 * (fromIntegral (length (setterStubLayout (StubCallees 0 0 0))) - 1)
      openSite = setterStub + 32
      stubPatches layout = [Patch address (candidateWord address) after | (address, after) <- zip [setterStub, setterStub + 4 ..] (layoutWords layout), candidateWord address /= after]
      bracketedIds chosen = [identifier | (identifier, _) <- armsWhere (\owned -> bracketedOwned owned && chosen owned)]
      pairArm owned = case owned of
        OwnedWriteOnlyPair _ -> True
        _ -> False
      setsOn bank identifiers = [canonical (OracleCase identifier o) | identifier <- identifiers, o@(Set b _ _) <- operations, b == bank]
      bracketControl name patches cases note = case cases of
        [] -> Left (name ++ ": no bracketed property")
        _ -> Right (Control name MustFail (PatchedCandidate patches) cases note)
      stubLayoutControl name layout cases note = case stubCallees of
        Nothing -> Left (name ++ ": the setter stub's callees are not linked")
        Just callees -> bracketControl name (stubPatches (layout callees)) cases note
      readBeforeOpen name chosen = case exposingOpenAfterSet fx chosen classified of
        Just (Property identifier _ _ _) -> stubLayoutControl name openAfterSetLayout (setsOn Bank0 [identifier])
          ("setter stub with the open call after xm1_property_set, which then reads bank 0 before the open, for " ++ propertyHex identifier)
        Nothing -> Left (name ++ ": no bracketed property exposes the open order")
      pairSecondControl = case [(identifier, arm) | (identifier, OwnedWriteOnlyPair arm) <- armsWhere (const True)] of
        (identifier, arm@(PairArm _ (BankOffset second) _ _)) : _ -> do
          patches <- specialPatches "c-pair-second-byte" identifier (WriteOnlyPair arm) (WriteOnlyPair arm {pairSecond = BankOffset (second + 1)})
          Right (Control "c-pair-second-byte" MustFail (PatchedCandidate patches) [canonical (OracleCase identifier o) | o@(Set {}) <- operations]
            ("SPECIALS second byte " ++ hex (fromIntegral second) ++ " -> " ++ hex (fromIntegral second + 1) ++ " for " ++ propertyHex identifier))
        [] -> Left "c-pair-second-byte: no write-only pair"
      bracketControls =
        [ bracketControl "c-open-removed" [Patch openSite (branchLinkWord openSite openWord) movR0R0Word] (setsOn Bank0 (bracketedIds (const True)))
            "setter stub's open call replaced by mov r0, r0, for every bracketed property's bank-0 sets"
        , stubLayoutControl "c-close-before-set" closeBeforeSetLayout (setsOn Bank0 (bracketedIds (const True)))
            "setter stub with the close call before xm1_property_set, for every bracketed property's bank-0 sets"
        , bracketControl "c-bracket-bank1" [Patch (setterStub + 4) 0xe1a01006 movR1ZeroWord] (setsOn Bank1 (bracketedIds (const True)))
            "setter stub passing bank 0 to xm1_set_bracketed, which brackets bank-1 sets too, for every bracketed property's bank-1 sets"
        , pairSecondControl
        , readBeforeOpen "c-read-before-open-field" (not . pairArm)
        , readBeforeOpen "c-read-before-open-pair" pairArm
        ]
      -- The shared exit adjusts 0x34 only when r7 is 1, which the stub replaces with
      -- `xm1_set_bracketed`'s 0; 0xe9's record-0 write at 0x6308b4 runs for every set.
      setterExitControl = case [identifier | (identifier, OwnedSlotField (SlotArm _ _ _ _ MirrorRecord0)) <- armsWhere (const True)] of
        mirrored : _ -> Right (Control "c-setter-stub-exit" MustFail (PatchedCandidate [Patch setterPop setterPopWord (branchWord setterPop 0x630200)])
          (setsOn Bank0 [mirrored])
          ("setter stub ends b 0x630200, for " ++ propertyHex mirrored ++ "'s bank-0 sets"))
        [] -> Left "c-setter-stub-exit: no record-0 mirrored slot field"
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
        , setterExitControl
        ]
          ++ bracketControls
      loadOf width = case width of
        Byte -> Load8
        Half -> Load16
      storeOf width = case width of
        Byte -> Store8
        Half -> Store16
      basePatches site before after = [Patch site (movwWord before) (movwWord after), Patch (site + 4) (movtWord before) (movtWord after)]
      accessorSite name accessor = maybe (Left (name ++ ": no " ++ accessorName accessor)) Right (accessorAt (accessorName accessor))
      slotControl name control = case exposingSlot fx control classified of
        Nothing -> Left (name ++ ": no slot field exposes the mutation")
        Just (Property identifier _ _ _, arm, mutation) -> do
          index@(SlotIndex slotIndex) <- maybe (Left (name ++ ": " ++ propertyHex identifier ++ " has no slot index")) Right (slotIndexOf identifier)
          let RecordBranch (RecordOffset recordAt) recordWidth' _ = recordArm arm
              BankBranch (BankOffset bankAt) _ _ _ = bankArm arm
              cases = [canonical (OracleCase identifier o) | o <- slotControlOperations control]
              changed before after address = [Patch address before after | before /= after]
              accessorPatches accessor before after = do
                site <- accessorSite name accessor
                Right (basePatches site before after, [(site, after)], accessorName accessor ++ " base " ++ hex before ++ " -> " ++ hex after ++ " for " ++ propertyHex identifier)
          (patches, bases, note) <- case (mutation, control) of
            (MutatedArm mutated, _) -> do
              let EncodedEntry entryBefore = encodeEntry (SlotEntry (bankArm arm) (slotMirror arm) index)
                  EncodedEntry entryAfter = encodeEntry (SlotEntry (bankArm mutated) (slotMirror mutated) index)
                  EncodedSlot slotBefore = encodeSlot arm
                  EncodedSlot slotAfter = encodeSlot mutated
              Right (changed entryBefore entryAfter (tableWord identifier) ++ changed slotBefore slotAfter (slotTableAddress + 4 * slotIndex), [], "PROPERTIES and SLOTS words of " ++ propertyHex identifier)
            (MutatedLayout _, RecordSavedAtBankOffset) ->
              accessorPatches (Accessor SaveImage (storeOf recordWidth')) saveBase (saveBase + fromIntegral bankAt - (recordBase + fromIntegral recordAt))
            (MutatedLayout layout, SlotDefaultsMoved) ->
              accessorPatches (Accessor SlotDefaultsImage (loadOf recordWidth')) slotDefaultsBase (layoutSlotDefaults layout)
            (MutatedLayout layout, TailDefaultsMoved) ->
              accessorPatches (Accessor TailDefaultsImage Load8) tailDefaultsBase (layoutTailDefaults layout)
            (MutatedLayout _, _) -> Left (name ++ ": the control changes no accessor")
          Right (Control name MustFail (PatchedCandidate patches) cases note, bases)
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
      firstField = listToMaybe [p | p@(Property _ _ _ (Owned (OwnedField _))) <- classified]
      halfTail = listToMaybe [p | p@(Property _ _ _ (Owned (OwnedField (FieldArm _ _ _ (FieldDefaultOnly (DefaultTail _ Half)) _ _)))) <- classified]
      getterPop = getterStub + 8
      getterPopControl = case firstField of
        Nothing -> Left "c-getter-pop-setter-list: no owned field"
        Just (Property identifier _ _ _) -> Right $ Control "c-getter-pop-setter-list" MustFail
          (PatchedCandidate [Patch getterPop getterPopWord setterPopWord])
          [canonical (OracleCase identifier o) | o@(Get {}) <- operations] ("getter stub pops the setter frame's list {r3-r8, r11, pc}, for " ++ propertyHex identifier)
      tailHalfControl name patches note = case halfTail of
        Nothing -> Left (name ++ ": no field has a halfword tail default")
        Just (Property identifier _ _ _) -> do
          site <- accessorSite name (Accessor TailDefaultsImage Load16)
          Right (Control name MustFail (PatchedCandidate (patches site)) [RunCase variant (OracleCase identifier o) | variant <- [CanonicalFixture, TailWindow], o <- defaultOperations] (note ++ " for " ++ propertyHex identifier))
      tailHalfBase = tailHalfControl "c-tail16-base" (\site -> basePatches site tailDefaultsBase tailHalfMovedBase) ("xm1_tail_defaults_load16 base " ++ hex tailDefaultsBase ++ " -> " ++ hex tailHalfMovedBase)
      tailHalfByte = tailHalfControl "c-tail16-byte" (\site -> [Patch (site + 8) loadHalfWord loadByteWord]) "xm1_tail_defaults_load16 reads a byte"
      referenceId = if isJust (M.lookup (PropertyId 0x34) properties >>= ownedProperty) then PropertyId 0x34 else PropertyId 0xd
      ownedProperty p = case propertyClass p of
        Owned _ -> Just p
        Delegated _ -> Nothing
      stockArmsReference = case M.lookup referenceId properties >>= ownedProperty of
        Just (Property identifier@(PropertyId n) (GuestAddress getterArm) (GuestAddress setterArm) _)
          | not (carrierStart <= setterArm && setterArm < carrierEnd) -> Right $ Control "r-stock-arms" MustPass
              (PatchedCandidate [Patch (getterTable + 4 * (n - 1)) getterStub getterArm, Patch (setterTable + 4 * (n - 1)) setterStub setterArm])
              [canonical (OracleCase identifier o) | o <- operations] ("reference: " ++ propertyHex identifier ++ "'s getter and setter table words restored to " ++ hex getterArm ++ " and " ++ hex setterArm ++ ", both outside the carrier")
        _ -> Left ("r-stock-arms: " ++ propertyHex referenceId ++ " is not owned with its setter arm outside the carrier")
      controls =
        [ entryControl "c-offset" OffsetMoved
        , entryControl "c-save" SaveCleared
        , entryControl "c-default" DefaultFlipped
        , entryControl "c-tail-offset" TailOffsetMoved
        , getterPopControl
        , tailHalfBase
        , tailHalfByte
        , stockArmsReference
        ]
          ++ map (fmap fst) slotControls
          ++ stage3Controls
      planned = [control | Right control <- controls]
      unavailable = [problem | Left problem <- controls]
      inSubset identifier = maybe True (S.member identifier) subset
      variantCases =
        [RunCase TailWindow (OracleCase identifier o) | Just (Property identifier _ _ _) <- [halfTail], inSubset identifier, o <- defaultOperations]
          ++ [RunCase variant (OracleCase identifier o) | Property identifier _ _ (Owned owned) <- classified, inSubset identifier, variant <- configVariantsOf, o <- variantOperations owned]
      gridCases = [canonical c | c <- caseGrid, inSubset (oracleProperty c)] ++ variantCases
      runCases = nub (gridCases ++ concatMap controlCases planned)
      fxOf rc = variantFixture fx (runVariant rc)
  tailHalfSite <- maybe (failH "no xm1_tail_defaults_load16") pure (accessorAt (accessorName (Accessor TailDefaultsImage Load16)))
  checkEncodings ((tailHalfSite, tailHalfMovedBase) : concat [bases | Right (_, bases) <- slotControls])
  ensure (A.decode setterPop (branchWord setterPop 0x630200) == A.Branch (A.Cond 14) False 0x630200) "b 0x630200 encoding"
  callees <- maybe (failH "the setter stub's callees are not linked") pure stubCallees
  ensure (map candidateWord (take 24 [setterStub, setterStub + 4 ..]) == layoutWords (setterStubLayout callees)) "the candidate's setter stub differs from its layout"
  ensure (A.decode openSite (branchLinkWord openSite openWord) == A.Branch (A.Cond 14) True openWord
    && A.decode 0 movR0R0Word == A.DataProcessing (A.Cond 14) A.MOV False (A.Reg 0) (A.Reg 0) (A.ShiftedImmediate (A.Reg 0) A.LSL 0)
    && A.decode 0 movR1ZeroWord == A.DataProcessing (A.Cond 14) A.MOV False (A.Reg 1) (A.Reg 0) (A.Immediate 0)) "bl, mov r0, r0 and mov r1, #0 encodings"

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
    pure (caseSpec candidate candidateFunctions (fxOf c) (runLabel c) (standing (runProperty c)) (runCase c) (observedExpectation run))
  (candidateBatches, candidateRuns) <- runBatches cache runner out "candidate" candidateSpecs
  verdicts <- forM runCases $ \c -> do
    reference <- stockRun c
    subject <- lookupRun candidateRuns (runLabel c)
    pure (c, compareRuns reference subject)

  -- Guest controls.
  controlImages <- forM planned $ \control -> case controlImage control of
    PatchedCandidate patches -> do
      image <- either (failH . ((controlName control ++ ": ") ++)) pure (applyPatches patches candidate)
      pure (control, image, candidateFunctions)
    DefectBuild defect@(DefectName name) -> case M.lookup defect defects of
      Just (image, spans, _) -> pure (control, image, spans)
      Nothing -> failH (controlName control ++ ": no --defect " ++ name)
  controlSpecs <- forM (concat [[(control, image, spans, c) | c <- controlCases control] | (control, image, spans) <- controlImages]) $ \(control, image, spans, c) -> do
    run <- stockRun c
    pure (caseSpec image spans (fxOf c) (runPrefixed (controlName control) c) (standing (runProperty c)) (runCase c) (observedExpectation run))
  (controlBatches, controlRuns) <- runBatches cache runner out "controls" controlSpecs
  controlOutcomes <- forM controlImages $ \(control, _, _) -> do
    failures <- fmap concat . forM (controlCases control) $ \c -> do
      reference <- stockRun c
      subject <- lookupRun controlRuns (runPrefixed (controlName control) c)
      pure [(runPrefixed (controlName control) c, d) | d <- compareRuns reference subject]
    let failedCases = M.size (M.fromListWith (++) [(label, [d]) | (label, d) <- failures])
    pure (control, failedCases, failures)
  let controlsHold = null unavailable && and [controlHolds control failed | (control, failed, _) <- controlOutcomes]

  -- Results.
  runnerSha <- sha256Hex <$> (canonicalizePath runner >>= sha256File)
  inputs <- forM
    ( [("image4", image4Path), ("candidate_image4", candidatePath), ("contract", contractPath), ("linked", linkedPath), ("symbols", symbolsPath)]
        ++ zip ["dat", "loader_a", "loader_b", "image5", "plain", "image2"] sourcePaths
        ++ concat [[("defect_" ++ name, binPath), ("defect_" ++ name ++ "_symbols", defectSymbolsPath)] | (name, binPath, defectSymbolsPath) <- defectArguments arguments]
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
      passed = null modelFailures && null verdictFailures && controlsHold
      allBatches = stockBatches ++ candidateBatches ++ controlBatches
      executedBatches = length [() | record <- allBatches, recordReuse record == BatchExecuted]
      reusedBatches = length [() | record <- allBatches, recordReuse record == BatchReused]
      executedCases = sum [recordCases record | record <- allBatches, recordReuse record == BatchExecuted]
      reusedCases = sum [recordCases record | record <- allBatches, recordReuse record == BatchReused]
      batchValue record =
        object
          [ "name" .= recordName record
          , "cases" .= recordCases record
          , "batch_sha256" .= recordBatchSha record
          , "summary_sha256" .= recordSummarySha record
          , "reuse" .= reuseLabel (recordReuse record)
          ]
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
            , "stubs" .= object ["getter" .= hex getterStub, "setter" .= hex setterStub, "properties" .= hex tableAddress]
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
                      DefectBuild defect@(DefectName name) -> object ["defect" .= name, "linked_sha256" .= fmap (\(_, _, digest) -> digest) (M.lookup defect defects)]
                  , "cases" .= length (controlCases control)
                  , "failed_cases" .= failed
                  , "holds" .= controlHolds control failed
                  , "first_differences" .= [object ["case" .= caseLabelText label, "kind" .= differenceKind d, "detail" .= differenceDetail d] | (label, d) <- take 20 failures]
                  ]
              | (control, failed, failures) <- controlOutcomes
              ]
                ++ [ object
                       [ "name" .= problem
                       , "kind" .= ("unavailable" :: String)
                       , "note" .= problem
                       , "cases" .= (0 :: Int)
                       , "failed_cases" .= (0 :: Int)
                       , "holds" .= False
                       , "first_differences" .= ([] :: [Value])
                       ]
                   | problem <- unavailable
                   ]
            )
        , "differences" .= differenceSamples
            ( [("model", runLabel c, d) | (c, ds) <- modelFailures, d <- ds]
                ++ [("pass2", runLabel c, d) | (c, ds) <- verdictFailures, d <- ds]
            )
        , "passed" .= passed
        ]
  B.writeFile (out </> "results.json") (encodeCanonical results)
  putStrLn ("model: " ++ show (length modelFailures) ++ " disagreeing of " ++ show (length modelRows) ++ " owned-id stock cases")
  putStrLn ("pass 2: " ++ show (length verdictFailures) ++ " failing of " ++ show (length verdicts))
  forM_ controlOutcomes $ \(control, failed, _) -> putStrLn (controlName control ++ ": " ++ show failed ++ " failing of " ++ show (length (controlCases control)))
  forM_ unavailable putStrLn
  when (isJust cache) $
    putStrLn
      ( "execution: " ++ show executedBatches ++ " executed, " ++ show reusedBatches ++ " reused batches; "
          ++ show executedCases ++ " executed, " ++ show reusedCases ++ " reused cases"
      )
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
  P.Carrier (P.GuestAddress carrierStart) (P.GuestAddress carrierEnd) <- maybe (failH "no owned interval") pure (listToMaybe (P.carrierIntervals classified))
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
