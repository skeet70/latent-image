{-# LANGUAGE OverloadedStrings #-}
-- | Standalone schema-4 and composed schema-5 loader-prefix observations.
module Emulation.LoaderPrefixWitness
  ( Scenario (..), scenarioName, parseScenario, scenarioControl
  , Guest (..), Span (..), RegisterIndex, registerIndex
  , ProgramKind (..), programKindName, Program (..), ProgramFacts (..)
  , Contract (..), StartupFacts, budget, contractValue, parseContract, isComposed
  , retainedFacts, deriveOwnedFacts, StagedImages (..), stagedImages, verifyStagedOwned
  , deriveComposedContract
  , Receipt, ValidatedReceipt, receiptArgument, parseReceipt, validateReceipt, compareReceipts
  , maxReceiptBytes
  ) where

import A32 (Cond (..), Indexing (..), Instruction (..), Offset (..), Reg (..), Width (..))
import qualified A32
import Control.Monad (forM, forM_, unless, when, zipWithM_)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (FromJSON, Object, Result (..), Value (..), fromJSON, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (complement, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy.Char8 as BLC
import Data.List (nub, sort, sortOn, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, isNothing)
import Data.Word (Word32, Word64, Word8)
import Emulation.FirstStageWitness (Core (..), Cpsr (..), WitnessResult (..), encodeLeWord, leWordAt, sha256Hex)
import Emulation.PeerStageWitness (Registers (..))
import qualified Emulation.PeerStageWitness as P
import qualified Emulation.PeerStartupWitness as S
import Emulation.PollPeriod (CodeImage (..), PollModel (..), PollPeriod (..), imageWord, pollPeriod)
import qualified FujiAF.CompletionControl as C
import qualified FujiAF.FirstStage as F
import FujiAF.JSON (decodeStrictIntegers)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import qualified StartupStageContract as Stage

-- * Contract

data Scenario = Natural | Held | Delayed | NonOne deriving (Eq, Show, Enum, Bounded)

scenarioName :: Scenario -> String
scenarioName Natural = "natural"
scenarioName Held = "held"
scenarioName Delayed = "delayed"
scenarioName NonOne = "non-one"

parseScenario :: String -> Either String Scenario
parseScenario = closed scenarioName "loader-prefix scenario"

-- | The loader-A control the scenario's DAT carries; 'Natural' runs the source DAT.
scenarioControl :: Scenario -> Maybe C.CompletionControl
scenarioControl Natural = Nothing
scenarioControl Held = Just C.HeldCompletion
scenarioControl Delayed = Just C.DelayedCompletion
scenarioControl NonOne = Just C.NonOneCompletion

newtype Guest = Guest Word32 deriving (Eq, Ord, Show)

-- | Half-open.
data Span = Span Word64 Word64 deriving (Eq, Ord, Show)

-- | Index into r0..r12.
newtype RegisterIndex = RegisterIndex Int deriving (Eq, Show)

registerIndex :: Int -> Either String RegisterIndex
registerIndex value
  | value >= 0 && value <= 12 = Right (RegisterIndex value)
  | otherwise = Left "loaded register must be r0..r12"

data ProgramKind = RetainedProgram | OwnedProgram deriving (Eq, Show, Enum, Bounded)

programKindName :: ProgramKind -> String
programKindName RetainedProgram = "retained"
programKindName OwnedProgram = "owned"

data ProgramFacts = ProgramFacts
  { factRecord :: F.Sha256
  , factRecordImmutable :: F.Sha256
  , factLoader :: F.Sha256
  , factHandoff :: Guest
  , factEntry :: Guest
  , factLoad :: Guest
  , factPostLoad :: Guest
  , factLoaded :: RegisterIndex
  , factPeriod :: PollPeriod
  , factImage :: Maybe Span
  , factRefusal :: Maybe Guest
  , factForbidFetch :: [Span]
  , factForbidRead :: [Span]
  } deriving (Eq, Show)

data Program = Program ProgramKind ProgramFacts deriving (Eq, Show)

data Contract = Contract Scenario Program | ComposedContract Scenario ProgramFacts StartupFacts deriving (Eq, Show)

data StartupFacts = StartupFacts
  { startupRecord :: F.Sha256, startupImageDigest :: F.Sha256, startupSymbols :: F.Sha256
  , startupSource :: F.Sha256, startupImage :: Span, startupEntry :: Guest, startupHandoff :: Guest
  , startupTarget :: Guest, startupArgument :: Guest, startupSecondary :: Guest
  , startupSecondaryImage :: F.Sha256, startupSecondarySymbols :: F.Sha256, startupSecondarySource :: F.Sha256
  } deriving (Eq, Show)

isComposed :: Contract -> Bool
isComposed ComposedContract {} = True
isComposed Contract {} = False

contractParts :: Contract -> (Scenario, Program, Maybe StartupFacts)
contractParts (Contract scenario program) = (scenario, program, Nothing)
contractParts (ComposedContract scenario facts startup) = (scenario, Program OwnedProgram facts, Just startup)

budget :: Word64
budget = 50000000

argumentWord :: Word32
argumentWord = 0x69cdf8

completionCell :: Word64
completionCell = 0xa00c4

cellSpan, loaderB, record2, zeroRange, zeroClaim, entryPool, veneerSpan, loaderACompletion, loaderALiteral :: Span
cellSpan = Span completionCell (completionCell + 4)
loaderB = Span 0xd0000 0xddaa0
record2 = Span 0x10000 0x14c80
zeroRange = Span 0xddaa0 0xf3420
zeroClaim = Span 0xddaa0 0xf33ec
entryPool = Span 0xd00c8 0xd00fc
veneerSpan = Span (fromIntegral C.veneerEntry) (fromIntegral C.veneerEnd)
-- | Loader A's 0x40a00-0x40a0c completion sequence and its 0x40afc literal.
loaderACompletion = Span 0x40a00 0x40a10
loaderALiteral = Span 0x40afc 0x40b00

replacedCode :: [Span]
replacedCode = [Span 0xd0020 0xd00c8, Span 0xd09ac 0xd09d0, Span 0xd0a44 0xd0a64]

coverageWords :: Word64
coverageWords = 0x5660

exitAddress, frameReturn, stackTop, exitSp, exitFrame, vectorBase :: Word32
exitAddress = 0xd0aac
frameReturn = 0xd00c0
stackTop = 0x400f3420
exitSp = 0x400f3410
exitFrame = 0x400f341c
vectorBase = 0xffff0000

vectorHandlers :: [Word32]
vectorHandlers = [0xd0100, 0xd01b4, 0xd0128, 0xd0148, 0xd0168, 0xd0204, 0xd0184]

-- | Stock 0xd0094 pushes four words and 0xd34f0 nine on the entry stack.
entryStackBytes :: Word64
entryStackBytes = 52

headBound, tailBound, headPolls :: Int
headBound = 8
tailBound = 2
headPolls = 4

-- | Completed core-1 zero loads the delayed control requires between its
-- veneer entry and the original store.
delayedZeroLoads :: Word64
delayedZeroLoads = 4

-- | Record 1's reset literal: core 0's first record-1 instruction after the
-- boot ROM loads it.
record1Entry :: Word32
record1Entry = 0x1800

retainedRecordHex, retainedRecordImmutableHex, loaderBHex :: String
retainedRecordHex = "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"
retainedRecordImmutableHex = "0c10e1264bccdd36fe167cd4c22a3df48134945a710f2554817f9cafd76b83d0"
loaderBHex = "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"

spanValue :: Span -> [Word64]
spanValue (Span low high) = [low, high]

guestWord :: Guest -> Word32
guestWord (Guest value) = value

contractValue :: Contract -> Value
contractValue contract = object $
  [ "schema" .= (if isComposed contract then 5 else 4 :: Int), "budget" .= budget
  , "scenario" .= object ["kind" .= scenarioName scenario]
  , "program" .= object
      [ "kind" .= programKindName kind
      , "record_sha256" .= F.sha256Hex (factRecord facts), "loader_sha256" .= F.sha256Hex (factLoader facts)
      , "record_immutable_sha256" .= F.sha256Hex (factRecordImmutable facts)
      , "handoff" .= guestWord (factHandoff facts), "entry" .= guestWord (factEntry facts)
      , "load" .= guestWord (factLoad facts), "post_load" .= guestWord (factPostLoad facts)
      , "loaded_register" .= let RegisterIndex value = factLoaded facts in value
      , "poll_period" .= let PollPeriod value = factPeriod facts in value
      , "image" .= fmap spanValue (factImage facts), "refusal" .= fmap guestWord (factRefusal facts)
      , "forbid_fetch" .= map spanValue (factForbidFetch facts), "forbid_read" .= map spanValue (factForbidRead facts)
      ]
  ] ++ ["startup" .= startupValue startup | Just startup <- [selected]]
  where
    (scenario, Program kind facts, selected) = contractParts contract

startupValue :: StartupFacts -> Value
startupValue facts = object
  [ "kind" .= ("composed" :: String), "record1_sha256" .= F.sha256Hex (startupRecord facts)
  , "image_sha256" .= F.sha256Hex (startupImageDigest facts), "symbols_sha256" .= F.sha256Hex (startupSymbols facts)
  , "source_sha256" .= F.sha256Hex (startupSource facts), "image" .= spanValue (startupImage facts)
  , "entry" .= guestWord (startupEntry facts), "handoff" .= guestWord (startupHandoff facts)
  , "handoff_target" .= guestWord (startupTarget facts), "handoff_argument" .= guestWord (startupArgument facts)
  , "secondary_entry" .= guestWord (startupSecondary facts)
  , "secondary_image_sha256" .= F.sha256Hex (startupSecondaryImage facts)
  , "secondary_symbols_sha256" .= F.sha256Hex (startupSecondarySymbols facts)
  , "secondary_source_sha256" .= F.sha256Hex (startupSecondarySource facts)
  ]

parseStartup :: Value -> Either String StartupFacts
parseStartup value = do
  row <- fields ["kind", "record1_sha256", "image_sha256", "symbols_sha256", "source_sha256", "image", "entry", "handoff"
    , "handoff_target", "handoff_argument", "secondary_entry", "secondary_image_sha256", "secondary_symbols_sha256", "secondary_source_sha256"] value
  kind <- get row "kind" :: Either String String
  require (kind == "composed") "startup kind is not composed"
  let digest name = get row name >>= F.parseSha256
  facts <- StartupFacts <$> digest "record1_sha256" <*> digest "image_sha256" <*> digest "symbols_sha256" <*> digest "source_sha256"
    <*> (field row "image" >>= parseSpan) <*> guestField row "entry" <*> guestField row "handoff"
    <*> guestField row "handoff_target" <*> guestField row "handoff_argument" <*> guestField row "secondary_entry"
    <*> digest "secondary_image_sha256" <*> digest "secondary_symbols_sha256" <*> digest "secondary_source_sha256"
  let Span low high = startupImage facts
      inside (Guest address) = fromIntegral address >= low && fromIntegral address < high && address .&. 3 == 0
      Guest target = startupTarget facts
  require (low == 0x6000 && high <= 0x10000 && high .&. 3 == 0
    && startupEntry facts == Guest 0x6000 && inside (startupHandoff facts) && startupHandoff facts /= startupEntry facts)
    "composed firststage sites or image extent differ"
  require (target >= 0x40000 && target < 0x50000 && target .&. 3 == 0 && startupArgument facts == Guest argumentWord
    && startupSecondary facts == Guest 0x10400) "composed handoff or secondary entry differs"
  require (startupSource facts == startupSecondarySource facts) "composed images name different source DATs"
  pure facts

parseContract :: B.ByteString -> Either String Contract
parseContract bytes = do
  require (B.length bytes <= 1024 * 1024) "loader-prefix contract exceeds 1 MiB"
  value <- decodeStrictIntegers bytes
  initial <- objectOf value
  schema <- get initial "schema" :: Either String Word32
  require (schema `elem` [4, 5]) "unsupported loader-prefix contract schema"
  row <- fields (["schema", "budget", "scenario", "program"] ++ ["startup" | schema == 5]) value
  stated <- get row "budget"
  require (stated == budget) "loader-prefix contract budget differs"
  scenario <- field row "scenario" >>= parseScenarioObject
  program <- field row "program" >>= parseProgram
  case schema of
    4 -> pure (Contract scenario program)
    _ -> do
      startup <- field row "startup" >>= parseStartup
      let Program kind facts = program
      require (kind == OwnedProgram && maybe False (\(Span low _) -> low == 0x10400) (factImage facts))
        "composed startup requires the owned secondary-plus-prefix image at 0x10400"
      pure (ComposedContract scenario facts startup)

parseScenarioObject :: Value -> Either String Scenario
parseScenarioObject value = fields ["kind"] value >>= \row -> get row "kind" >>= parseScenario

parseProgram :: Value -> Either String Program
parseProgram value = do
  row <- fields ["kind", "record_sha256", "record_immutable_sha256", "loader_sha256", "handoff", "entry", "load", "post_load", "loaded_register"
    , "poll_period", "image", "refusal", "forbid_fetch", "forbid_read"] value
  kind <- get row "kind" >>= closed programKindName "loader-prefix program"
  facts <- ProgramFacts <$> (get row "record_sha256" >>= F.parseSha256) <*> (get row "record_immutable_sha256" >>= F.parseSha256)
    <*> (get row "loader_sha256" >>= F.parseSha256)
    <*> guestField row "handoff" <*> guestField row "entry" <*> guestField row "load" <*> guestField row "post_load"
    <*> (get row "loaded_register" >>= registerIndex) <*> (PollPeriod <$> get row "poll_period")
    <*> (field row "image" >>= nullable parseSpan) <*> (field row "refusal" >>= nullable (fmap Guest . json))
    <*> (field row "forbid_fetch" >>= json >>= mapM parseSpan) <*> (field row "forbid_read" >>= json >>= mapM parseSpan)
  checkFacts kind facts
  pure (Program kind facts)

retainedProgramFacts :: PollPeriod -> Either String ProgramFacts
retainedProgramFacts period = do
  record <- F.parseSha256 retainedRecordHex
  immutable <- F.parseSha256 retainedRecordImmutableHex
  loader <- F.parseSha256 loaderBHex
  pure ProgramFacts
    { factRecord = record, factRecordImmutable = immutable, factLoader = loader, factHandoff = Guest 0x11850, factEntry = Guest 0xd0020
    , factLoad = Guest 0xd0a44, factPostLoad = Guest 0xd0a48, factLoaded = RegisterIndex 3, factPeriod = period
    , factImage = Nothing, factRefusal = Nothing, factForbidFetch = [], factForbidRead = []
    }

checkFacts :: ProgramKind -> ProgramFacts -> Either String ()
checkFacts kind facts = do
  let PollPeriod period = factPeriod facts
  require (period > 0 && period <= 100000) "poll period lies outside 1..100000"
  case kind of
    RetainedProgram -> do
      expected <- retainedProgramFacts (factPeriod facts)
      require (facts == expected) "retained program facts differ from the stock sites"
    OwnedProgram -> do
      Span low high <- maybe (Left "owned program lacks its record-2 image") Right (factImage facts)
      let inside (Guest site) = fromIntegral site >= low && fromIntegral site < high && site .&. 3 == 0
          sites = [factHandoff facts, factEntry facts, factLoad facts, factPostLoad facts] ++ maybe [] pure (factRefusal facts)
      require (low >= 0x103e8 && high <= 0x11800 && low < high && low .&. 3 == 0 && high .&. 3 == 0)
        "owned image lies outside record-2 padding [0x103e8,0x11800)"
      require (all inside sites && length (nub sites) == length sites) "owned sites lie outside the image or repeat"
      require (low <= 0x10400 && high > 0x10400) "owned image omits the secondary entry at 0x10400"
      require (all (/= Guest 0x10400) sites) "owned witness site collides with the secondary entry"
      require (factLoaded facts == RegisterIndex 0) "owned completion value is not returned in r0"
      require (sort (factForbidFetch facts) == replacedCode && factForbidRead facts == [entryPool])
        "owned forbidden spans differ from the replaced loader-B code and pool"

-- * Program facts

completionModel :: (Word32, Word32) -> M.Map Int Word32 -> PollModel
completionModel = PollModel (0x40000000 + fromIntegral completionCell) 0

-- | Record-2 log globals occupy [0x14c60,0x14c6c).
recordImmutableDigest :: B.ByteString -> Either String F.Sha256
recordImmutableDigest record = do
  require (B.length record == 0x4c80) "record-2 image does not span [0x10000,0x14c80)"
  let prefix = SHA256.update SHA256.init (B.take 0x4c60 record)
      context = SHA256.update prefix (B.drop 0x4c6c record)
  F.parseSha256 (BLC.unpack (BB.toLazyByteString (BB.byteStringHex (SHA256.finalize context))))

-- | Retained facts from the staged record 2 and loader B, both pinned.
retainedFacts :: StagedImages -> Either String ProgramFacts
retainedFacts (StagedImages record loader) = do
  require (sha256Hex record == retainedRecordHex) "retained record 2 differs from the original"
  immutable <- recordImmutableDigest record
  require (F.sha256Hex immutable == retainedRecordImmutableHex) "retained immutable record 2 differs from the original"
  require (sha256Hex loader == loaderBHex) "retained loader B differs from the original"
  forM_ [(0xd09c0, 0x13001000), (0xd09c4, 0x1344100a), (0xd09c8, 0x13a02064)] $ \(address, expected) ->
    require (leWordAt loader (address - 0xd0000) == Right expected) "stock wait register setup differs"
  let model = completionModel (0, 0) (M.fromList [(1, 0x400a0000), (2, 100)])
  let branchesTo target = [() | offset <- [0, 4 .. B.length loader - 4], Right word <- [leWordAt loader offset]
        , Branch _ _ destination <- [A32.decode (0xd0000 + fromIntegral offset) word], destination == target]
  require (null (branchesTo 0xd0a48)) "a stock branch enters the retained post-load site"
  pollPeriod model (CodeImage 0xd0000 loader) 0xd0a44 0xd0a48 >>= retainedProgramFacts

-- | Owned facts from the linked image and symbols and the staged record 2
-- and loader B.
deriveOwnedFacts :: B.ByteString -> B.ByteString -> StagedImages -> Either String ProgramFacts
deriveOwnedFacts bin symbolBytes images = do
  rows <- F.parseSymbols symbolBytes
  deriveOwnedFactsParsed (U.MediaImage bin) rows images

deriveOwnedFactsParsed :: U.MediaImage -> [F.Symbol] -> StagedImages -> Either String ProgramFacts
deriveOwnedFactsParsed (U.MediaImage bin) rows (StagedImages record loader) = do
  let at name = F.symbolAddress <$> F.lookupSymbol rows name
      extentOf name = do
        found <- F.lookupSymbol rows name
        let low = F.symbolAddress found
        require (F.symbolSize found > 0 && low .&. 3 == 0) (name ++ " has no size or is not word aligned")
        pure (low, low + F.symbolSize found)
  start <- at "__secondary_stage_start"
  entry <- at "xm1_secondary_entry"
  end <- at "__secondary_stage_end"
  require (start == 0x10400 && entry >= start && entry < end && not (B.null bin) && B.length bin `mod` 4 == 0
    && toInteger start + toInteger (B.length bin) == toInteger end)
    "owned image does not span [__secondary_stage_start, __secondary_stage_end)"
  require (B.take (B.length bin) (B.drop (fromIntegral start - 0x10000) record) == bin) "staged record 2 does not hold the owned image"
  let code = CodeImage start bin
      wordAt location = maybe (Left ("owned word outside the image at " ++ hex location)) Right (imageWord code location)
      decodeExtent (low, high) = mapM (\location -> (,) location . A32.decode location <$> wordAt location) [low, low + 4 .. high - 4]
  handoff <- at "xm1_secondary_handoff_vector"
  handoffWord <- wordAt handoff
  require (handoffWord == 0xe1a0f003) "record-2 handoff vector is not MOV PC, R3"
  prefixEntry <- at "xm1_loader_prefix_entry"
  require (leWordAt loader 0xc == Right prefixEntry) "staged loader-B header entry does not name xm1_loader_prefix_entry"
  exitWord <- at "xm1_loader_prefix_exit" >>= wordAt
  require (exitWord == 0xe12fff10) "prefix exit is not BX R0"
  cell <- extentOf "xm1_loader_prefix_completion_cell"
  cellWords <- decodeExtent cell
  let literals = [(rt, location + 8 + fromIntegral displacement) | (location, Load (Cond 14) Word False rt rn (OffsetImmediate displacement) OffsetOnly) <- cellWords, rn == A32.pc]
      cellLoads = [(location, base) | (location, Load (Cond 14) Word False (Reg 0) base (OffsetImmediate 0) OffsetOnly) <- cellWords, base /= A32.pc]
  load <- case (cellLoads, literals) of
    ([(location, base)], [(rt, literalAt)]) | rt == base -> do
      literal <- wordAt literalAt
      require (literal == 0x40000000 + fromIntegral completionCell) "completion-cell literal is not 0x400a00c4"
      pure location
    _ -> Left "completion cell is not one literal base and one word load into r0"
  returnWord <- A32.decode (load + 4) <$> wordAt (load + 4)
  require (case returnWord of
    LoadMultiple (Cond 14) rn _ _ list -> rn == A32.sp && A32.pc `elem` list && Reg 0 `notElem` list
    BranchExchange (Cond 14) False rm -> rm == A32.lr
    _ -> False) "completion-cell load is not followed by its return"
  dispatchWords <- extentOf "xm1_loader_prefix_dispatch" >>= decodeExtent
  call <- case [location | (location, Branch (Cond 14) True callee) <- dispatchWords, callee == fst cell] of
    [location] -> Right location
    _ -> Left "dispatch does not call the completion cell exactly once"
  require (null [() | (_, Branch _ _ destination) <- dispatchWords, destination == call + 4]) "a branch enters the owned post-load site"
  period <- pollPeriod (completionModel (0x400f3000, stackTop) (M.singleton 13 0x400f3400)) code call (call + 4)
  let refusal = either (const Nothing) (Just . Guest . F.symbolAddress) (F.lookupSymbol rows "xm1_loader_prefix_refusal")
  fetchSpans <- forbidSpans rows "xm1_loader_b_forbid_fetch_"
  readSpans <- forbidSpans rows "xm1_loader_b_forbid_read_"
  recordDigest <- F.parseSha256 (sha256Hex record)
  recordImmutable <- recordImmutableDigest record
  loaderDigest <- F.parseSha256 (sha256Hex loader)
  let facts = ProgramFacts
        { factRecord = recordDigest, factRecordImmutable = recordImmutable, factLoader = loaderDigest, factHandoff = Guest handoff, factEntry = Guest prefixEntry
        , factLoad = Guest load, factPostLoad = Guest (call + 4), factLoaded = RegisterIndex 0, factPeriod = period
        , factImage = Just (Span (fromIntegral start) (fromIntegral end)), factRefusal = refusal
        , factForbidFetch = sort fetchSpans, factForbidRead = sort readSpans
        }
  checkFacts OwnedProgram facts
  pure facts

forbidSpans :: [F.Symbol] -> String -> Either String [Span]
forbidSpans rows prefix = do
  let tagged = [(stem, F.symbolAddress symbol) | symbol <- rows, Just stem <- [stripPrefix prefix (F.symbolName symbol)]]
      split suffix = [(stem, location) | (name, location) <- tagged, Just stem <- [stripSuffix suffix name]]
      starts = split "_start"
      ends = split "_end"
  require (length starts + length ends == length tagged && sort (map fst starts) == sort (map fst ends)) (prefix ++ " symbols do not pair")
  forM starts $ \(stem, low) -> case lookup stem ends of
    Just high | low < high -> Right (Span (fromIntegral low) (fromIntegral high))
    _ -> Left (prefix ++ stem ++ " is empty")
  where
    stripSuffix suffix text = reverse <$> stripPrefix (reverse suffix) (reverse text)

-- * Staged DAT

-- | Record 2 [0x10000,0x14c80) and loader B [0xd0000,0xddaa0) as a DAT loads them.
data StagedImages = StagedImages {stagedRecord :: B.ByteString, stagedLoader :: B.ByteString}

stagedImages :: B.ByteString -> Either String StagedImages
stagedImages dat = U.parseContainer dat >>= stagedImagesFromParts (Guest record1Entry) . U.parsedParts

stagedImagesFromParts :: Guest -> U.Parts -> Either String StagedImages
stagedImagesFromParts (Guest selectedEntry) parts = do
  first <- F.recordSlotBytes F.FirstRecord parts
  second <- F.recordSlotBytes F.SecondRecord parts
  segment <- loaderSegment (F.LoaderId 1) parts
  require (U.segLoad segment == 0xd0000 && U.segSize segment == 0xdaa0) "loader-B segment extent differs"
  let loader = B.map complement (B.take 0xdaa0 (B.drop (fromIntegral (U.segFlash segment)) (U.partPreTable parts)))
  require (U.mediaWordAt first F.resetVectorWordOffset == selectedEntry) "record-1 reset literal differs from the selected contract"
  pure (StagedImages (B.map complement (B.take 0x4c80 second)) loader)

loaderSegment :: F.LoaderId -> U.Parts -> Either String U.SegmentEntry
loaderSegment (F.LoaderId wanted) parts = do
  segment <- case filter ((== wanted) . U.segId) (U.partSegments parts) of
    [found] -> Right found
    _ -> Left "loader segment is not unique"
  let offset = fromIntegral (U.segFlash segment)
      size = fromIntegral (U.segSize segment)
      bytes = U.partPreTable parts
  require (size > 0 && offset + size <= B.length bytes) "loader segment exceeds its carrier pre-table"
  pure segment

-- | Staged equals the reference (the source DAT or its completion control)
-- outside the main checksum word, the record-2 reset vector word, the
-- candidate image and loader B's header entry word.
verifyStagedOwned :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String StagedImages
verifyStagedOwned reference staged bin symbolBytes = do
  rows <- F.parseSymbols symbolBytes
  entry <- F.symbolAddress <$> F.lookupSymbol rows "xm1_secondary_entry"
  start <- F.symbolAddress <$> F.lookupSymbol rows "__secondary_stage_start"
  prefixEntry <- F.symbolAddress <$> F.lookupSymbol rows "xm1_loader_prefix_entry"
  referenceParsed <- U.parseContainer reference
  stagedParsed <- U.parseContainer staged
  forM_ (U.checks stagedParsed ++ U.carriedChecks stagedParsed) $ \check ->
    require (U.checkPassed check) ("staged container contract: " ++ U.checkField check)
  require (B.length staged == B.length reference && U.hMainOffset (U.parsedHeader referenceParsed) == U.hMainOffset (U.parsedHeader stagedParsed))
    "staged DAT length or main offset differs"
  let mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader referenceParsed))
      slot = mainStart + F.recordSlotOffset F.SecondRecord
      load = fromIntegral start - F.recordGuestBase F.SecondRecord
  stock <- F.armEntry 0xd0020
  published <- F.armEntry prefixEntry
  publication <- F.loaderEntryPublication (F.LoaderId 1) 0xd0000 stock published
  header <- (mainStart +) <$> F.loaderWordPatchOffset (U.parsedParts referenceParsed) publication
  require (load >= F.resetVectorWordOffset + 4 && load + B.length bin <= F.firstRecordSlotLength) "candidate image does not fit record 2 after its reset vector"
  require (equalOutside [(U.mainChecksumOffset, 4), (slot + F.resetVectorWordOffset, 4), (slot + load, B.length bin), (header, 4)] reference staged)
    "staged DAT differs outside its declared extents"
  require (U.mediaWordAt staged (slot + F.resetVectorWordOffset) == entry) "staged reset vector does not name the record-2 entry"
  require (B.map complement (B.take (B.length bin) (B.drop (slot + load) staged)) == bin) "staged record-2 image differs from the linked image"
  require (U.mediaWordAt staged header == prefixEntry) "staged loader-B header does not publish the prefix entry"
  stagedImagesFromParts (Guest record1Entry) (U.parsedParts stagedParsed)

deriveComposedContract :: Scenario -> Stage.ComposedStartupInput -> Guest -> Either String Contract
deriveComposedContract scenario input (Guest argument) = do
  require (argument == argumentWord) "composed image-4 argument differs from 0x69cdf8"
  let original = Stage.originalStartupInput input
      finalParsed = Stage.stagedStartupParsed (Stage.stagedStartupInput input)
      artifacts = Stage.startupArtifacts input
      first = Stage.firstStageArtifact artifacts
      second = Stage.secondaryPrefixArtifact artifacts
      firstPlan = Stage.firstStageArtifactPlan first
      firstBin = U.mediaImageBytes (F.planImage firstPlan)
      firstRows = Stage.firstStageArtifactSymbols first
      secondRows = Stage.secondaryPrefixArtifactSymbols second
      firstStart = F.recordGuestBase F.FirstRecord + F.slotOffsetBytes (F.planLoadOffset firstPlan)
      firstEntry = F.armEntryAddress (F.planEntry firstPlan)
      firstEnd = firstStart + B.length firstBin
      at rows name = F.symbolAddress <$> F.lookupSymbol rows name
  reference <- case scenarioControl scenario of
    Nothing -> Right (Stage.originalStartupParsed original)
    Just control -> C.controlParsed <$> C.buildCompletionControl control (Stage.originalStartupParsed original)
  plan <- Stage.composedPlans artifacts
  F.verifyComposedRecords plan reference finalParsed
  images <- stagedImagesFromParts (Guest firstEntry) (U.parsedParts finalParsed)
  facts <- deriveOwnedFactsParsed (F.planImage (Stage.secondaryPrefixArtifactPlan second)) secondRows images
  handoff <- at firstRows "xm1_first_stage_handoff_vector"
  require (leWordAt firstBin (fromIntegral handoff - firstStart) == Right 0xe1a0f002)
    "firststage handoff vector is not MOV PC, R2"
  secondary <- at secondRows "xm1_secondary_entry"
  record <- B.map complement <$> F.recordSlotBytes F.FirstRecord (U.parsedParts finalParsed)
  recordDigest <- F.parseSha256 (sha256Hex record)
  loaderA <- loaderSegment (F.LoaderId 0) (U.parsedParts finalParsed)
  require (U.segLoad loaderA == 0x40000 && U.segSize loaderA >= 0x10) "composed loader-A extent differs"
  let target = U.mediaWordAt (U.partPreTable (U.parsedParts finalParsed)) (fromIntegral (U.segFlash loaderA) + 0xc)
  let startup = StartupFacts
        { startupRecord = recordDigest
        , startupImageDigest = Stage.firstStageArtifactImageDigest first
        , startupSymbols = Stage.firstStageArtifactSymbolsDigest first
        , startupSource = Stage.originalStartupDigest original
        , startupImage = Span (fromIntegral firstStart) (fromIntegral firstEnd)
        , startupEntry = Guest firstEntry, startupHandoff = Guest handoff
        , startupTarget = Guest target, startupArgument = Guest argument, startupSecondary = Guest secondary
        , startupSecondaryImage = Stage.secondaryPrefixArtifactImageDigest second
        , startupSecondarySymbols = Stage.secondaryPrefixArtifactSymbolsDigest second
        , startupSecondarySource = Stage.originalStartupDigest original
        }
  checked <- parseStartup (startupValue startup)
  require (firstStart == 0x6000 && secondary == 0x10400) "composed source-defined image starts or entries differ"
  pure (ComposedContract scenario facts checked)

-- | The two strings are equal outside the sorted, disjoint (offset, length) extents.
equalOutside :: [(Int, Int)] -> B.ByteString -> B.ByteString -> Bool
equalOutside extents left right = B.length left == B.length right && go 0 (sortOn fst extents)
  where
    window from to = B.take (to - from) . B.drop from
    go cursor [] = window cursor (B.length left) left == window cursor (B.length left) right
    go cursor ((start, size) : rest) = start >= cursor && window cursor start left == window cursor start right && go (start + size) rest

-- * Receipt

data Access = CpuRead | CpuWrite | CpuFetch | HostRead | HostWrite | ModelBusRead | ModelBusWrite
  deriving (Eq, Ord, Show, Enum, Bounded)

accessName :: Access -> String
accessName access = case access of
  CpuRead -> "cpu-read"
  CpuWrite -> "cpu-write"
  CpuFetch -> "cpu-fetch"
  HostRead -> "host-read"
  HostWrite -> "host-write"
  ModelBusRead -> "model-bus-read"
  ModelBusWrite -> "model-bus-write"

writes :: [Access]
writes = [CpuWrite, HostWrite, ModelBusWrite]

data TcmBankName = Btcm | Atcm deriving (Eq, Ord, Show, Enum, Bounded)

bankName :: TcmBankName -> String
bankName Btcm = "btcm"
bankName Atcm = "atcm"

data ScopeSpan = PhysicalSpan Span | TcmSpan TcmBankName Span deriving (Eq, Ord, Show)

-- | A CPU row's core, PC, aggregate entered instructions and tick.
data Attribution = Attribution Core Word32 Word64 Word64 deriving (Eq, Show)

data Row = Row
  { rowAccess :: Access, rowLocation :: ScopeSpan, rowValue :: Maybe Word64
  , rowBefore :: Maybe B.ByteString, rowAttribution :: Maybe Attribution
  } deriving (Eq, Show)

data ClassState = ClassState Span [Access] deriving (Eq, Ord, Show)

data ClassifiedViews = ObserveViews | FixedViews deriving (Eq, Show)

data ClassifiedWatch = ClassifiedWatch ClassifiedViews [ClassState] [Row] deriving (Eq, Show)

data Mode = Allow | Forbid deriving (Eq, Ord, Show, Enum, Bounded)

modeName :: Mode -> String
modeName Allow = "allow"
modeName Forbid = "forbid"

data Confinement = Confinement [Int] Access Mode [ScopeSpan] deriving (Eq, Ord, Show)

data CounterTotals = CounterTotals {totalCount :: Word64, totalZero :: Word64, totalNonzero :: Word64} deriving (Eq, Show)

data CounterState = CounterState
  { counterCores :: [Int], counterCell :: Span, counterHeadBound :: Int, counterTailBound :: Int
  , counterTotals :: CounterTotals, counterFirstNonzero :: Maybe (Row, Word64)
  , counterHead :: [Row], counterTail :: [Row]
  } deriving (Eq, Show)

data CoverageState = CoverageState [Int] Span Word64 (Maybe Word64) deriving (Eq, Show)

-- | Core, then its ATCM and BTCM region registers.
data TcmSnapshot = TcmSnapshot Int Word32 Word32 deriving (Eq, Ord, Show)

data ScopedWatch = ScopedWatch
  { scopedClasses :: [ClassState], scopedRows :: [Row], scopedTcm :: [TcmSnapshot], scopedConfinement :: [Confinement]
  , scopedCounters :: [CounterState], scopedCoverage :: [CoverageState]
  } deriving (Eq, Show)

data Label
  = Record1Entry | FirstStageHandoff | SecondaryEntry | Record2Handoff | PrefixEntry | PostLoad | HeldGate | DelayEntry | DelayRelease | CompletionStore | PrefixExit | Refusal
  deriving (Eq, Ord, Show, Enum, Bounded)

labelName :: Label -> String
labelName label = case label of
  Record1Entry -> "record-1-entry"
  FirstStageHandoff -> "first-stage-handoff"
  SecondaryEntry -> "secondary-entry"
  Record2Handoff -> "record-2-handoff"
  PrefixEntry -> "prefix-entry"
  PostLoad -> "post-load"
  HeldGate -> "held-gate"
  DelayEntry -> "delay-entry"
  DelayRelease -> "delay-release"
  CompletionStore -> "completion-store"
  PrefixExit -> "prefix-exit"
  Refusal -> "refusal"

labelCore :: Label -> Core
labelCore label
  | label `elem` [Record1Entry, FirstStageHandoff, HeldGate, DelayEntry, DelayRelease, CompletionStore] = Core0
  | otherwise = Core1

-- | ATCM and BTCM region registers of core 0, then core 1.
data TcmWords = TcmWords (Word32, Word32) (Word32, Word32) deriving (Eq, Show)

data Stop = Stop
  { stopLabel :: Label, stopAddress :: Guest, stopCore :: Core, stopCounts :: (Word64, Word64)
  , stopRegisters :: Registers, stopSp :: Word32, stopLr :: Word32, stopCpsr :: Cpsr
  , stopCompletion :: Word32, stopTcm :: TcmWords, stopCounter :: Maybe CounterTotals
  -- | Scoped-watch row count at the stop.
  , stopCursor :: Maybe Int
  } deriving (Eq, Show)

-- | A readable core-1 static mapping: guest address, physical address, size.
data StaticMap = StaticMap Word64 Word64 Word64 deriving (Eq, Show)

-- | The physical address of guest [address, address + size) through the map.
through :: StaticMap -> Word64 -> Word64 -> Either String Word64
through (StaticMap guest physical size) address extent
  | address >= guest && address + extent <= guest + size = Right (physical + address - guest)
  | otherwise = Left "bound extent leaves its static mapping"

data Bindings = Bindings
  { bindingSlotsMap :: StaticMap, bindingStackWindow :: ScopeSpan
  , bindingZeroMap :: StaticMap, bindingVeneer :: Maybe (B.ByteString, B.ByteString)
  } deriving (Eq, Show)

-- | Core counts before and after one entered instruction, and the completion
-- cell after it.
data Microstep = Microstep (Word64, Word64) (Word64, Word64) Word32 deriving (Eq, Show)

data CommittedStore = CommittedStore
  { storeInstruction :: (Word32, Word32), storeBeforePc :: Word32, storeAfterPc :: Word32
  , storeCpsr :: (Cpsr, Cpsr), storeSteps :: [Microstep], storeReadback :: (Word32, Word32)
  , storeCounter :: CounterTotals, storeCursor :: Int
  } deriving (Eq, Show)

data Endpoint = Endpoint
  { endpointFrame :: (B.ByteString, B.ByteString), endpointVectors :: (B.ByteString, B.ByteString)
  , endpointZero :: (Span, Maybe Word64), endpointRecordImmutable :: F.Sha256, endpointLoader :: F.Sha256
  } deriving (Eq, Show)

-- | Core-0 PC, registers, SP, LR and CPSR.
data Primary = Primary Word32 Registers Word32 Word32 Cpsr deriving (Eq, Show)

data Receipt = Receipt
  { receiptScenario :: Scenario, receiptProgram :: ProgramKind, receiptResult :: WitnessResult
  , receiptError :: Maybe String, receiptMachineFailure :: Maybe String, receiptIdentities :: (Word32, Word32)
  , receiptBoot :: ClassifiedWatch, receiptLoaded :: ClassifiedWatch, receiptScoped :: Maybe ScopedWatch, receiptStops :: [Stop]
  , receiptBindings :: Maybe Bindings, receiptStore :: Maybe CommittedStore, receiptEndpoint :: Maybe Endpoint
  , receiptPrimary :: Maybe Primary, receiptStartup :: Maybe StartupFacts
  , receiptInitialRecord :: Maybe F.Sha256, receiptHandoffRecordImmutable :: Maybe F.Sha256
  } deriving (Eq, Show)

-- | A receipt accepted under its contract.
data ValidatedReceipt = ValidatedReceipt Contract Receipt

maxReceiptBytes :: Int
maxReceiptBytes = 16 * 1024 * 1024

maxStops :: Int
maxStops = 64

parseReceipt :: B.ByteString -> Either String Receipt
parseReceipt bytes = do
  require (B.length bytes <= maxReceiptBytes) "loader-prefix receipt exceeds 16 MiB"
  value <- decodeStrictIntegers bytes
  initial <- objectOf value
  schema <- get initial "schema" :: Either String Word32
  require (schema `elem` [4, 5]) "unsupported loader-prefix receipt schema"
  row <- fields (["schema", "scenario", "program", "result", "error", "machine_failure", "instructions", "cp14_identities"
    , "boot_watch", "loaded_watch", "scoped_watch", "stops", "bindings", "completion", "endpoint", "final_primary"
    , "initial_record_sha256", "handoff_record_immutable_sha256"]
    ++ ["startup" | schema == 5]) value
  scenario <- field row "scenario" >>= parseScenarioObject
  program <- get row "program" >>= closed programKindName "loader-prefix program"
  result <- get row "result" >>= \text -> case text :: String of
    "passed" -> Right Passed
    "failed" -> Right Failed
    _ -> Left "unknown receipt result"
  identities <- get row "cp14_identities" >>= twoOf "cp14_identities"
  _ <- get row "instructions" :: Either String Word64
  stops <- field row "stops" >>= json >>= mapM parseStop
  require (length stops <= maxStops) "loader-prefix stop bound exceeded"
  Receipt scenario program result <$> get row "error" <*> get row "machine_failure" <*> pure identities
    <*> (field row "boot_watch" >>= parseClassified) <*> (field row "loaded_watch" >>= parseClassified)
    <*> (field row "scoped_watch" >>= nullable parseScoped) <*> pure stops
    <*> (field row "bindings" >>= nullable parseBindings) <*> (field row "completion" >>= nullable parseStore)
    <*> (field row "endpoint" >>= nullable parseEndpoint) <*> (field row "final_primary" >>= nullable parsePrimary)
    <*> (if schema == 5 then Just <$> (field row "startup" >>= parseStartup) else pure Nothing)
    <*> (field row "initial_record_sha256" >>= nullable (\entry -> json entry >>= F.parseSha256))
    <*> (field row "handoff_record_immutable_sha256" >>= nullable (\entry -> json entry >>= F.parseSha256))

twoOf :: String -> [a] -> Either String (a, a)
twoOf _ [a, b] = Right (a, b)
twoOf name _ = Left (name ++ " must hold two entries")

parseCore :: Int -> Either String Core
parseCore 0 = Right Core0
parseCore 1 = Right Core1
parseCore _ = Left "core must be 0 or 1"

parseRow :: Value -> Either String Row
parseRow value = do
  row <- objectOf value
  access <- get row "source" >>= closed accessName "watch access"
  let cpu = access `elem` [CpuRead, CpuWrite, CpuFetch]
      tcmRow = isJust (KM.lookup "tcm" row)
  _ <- fields (["source", "address", "physical", "size", "core", "pc", "instructions", "tick"]
    ++ (if cpu then ["value", "before", "byte_order"] else []) ++ ["tcm" | tcmRow]) value
  _ <- get row "address" :: Either String Word64
  size <- get row "size" :: Either String Word64
  require (size > 0 && size <= 16) "watch row width differs"
  location <- if tcmRow
    then do
      physical <- field row "physical"
      require (cpu && physical == Null) "only CPU rows carry TCM attribution, without a physical address"
      tcm <- field row "tcm" >>= fields ["bank", "offset"]
      bank <- get tcm "bank" >>= closed bankName "TCM bank"
      offset <- get tcm "offset"
      pure (TcmSpan bank (Span offset (offset + size)))
    else (\physical -> PhysicalSpan (Span physical (physical + size))) <$> get row "physical"
  if cpu
    then do
      byteOrder <- get row "byte_order" :: Either String String
      require (byteOrder == "little") "watch row is not little-endian"
      before <- get row "before" :: Either String (Maybe [Word8])
      require (maybe (access == CpuFetch) ((== fromIntegral size) . length) before) "watch row before bytes differ from its width"
      core <- get row "core" >>= parseCore
      cpuAttribution <- Attribution core <$> get row "pc" <*> get row "instructions" <*> get row "tick"
      valueField <- get row "value"
      pure (Row access location valueField (B.pack <$> before) (Just cpuAttribution))
    else do
      forM_ ["core", "pc", "instructions", "tick"] $ \name -> do
        entry <- field row name
        require (entry == Null) "transfer row carries CPU attribution"
      pure (Row access location Nothing Nothing Nothing)

parseClasses :: Value -> Either String [ClassState]
parseClasses value = json value >>= fmap sort . mapM parseClass
  where
    parseClass entry = do
      row <- fields ["range", "kinds"] entry
      kinds <- get row "kinds" >>= mapM (closed accessName "watch access")
      ClassState <$> (field row "range" >>= parseSpan) <*> pure (sort kinds)

noViolation :: Object -> Either String ()
noViolation row = do
  violation <- get row "violation" :: Either String (Maybe String)
  require (isNothing violation) ("memory watch violation: " ++ fromMaybe "" violation)

parseClassified :: Value -> Either String ClassifiedWatch
parseClassified value = do
  row <- fields ["enabled", "ranges", "records", "violation", "classes", "views"] value
  noViolation row
  stated <- get row "views" :: Either String String
  views <- case stated of
    "observe" -> Right ObserveViews
    "fixed" -> Right FixedViews
    _ -> Left "unknown classified watch views"
  ClassifiedWatch views <$> (field row "classes" >>= parseClasses) <*> (field row "records" >>= json >>= mapM parseRow)

parseScopeSpan :: Value -> Either String ScopeSpan
parseScopeSpan value = do
  row <- objectOf value
  case (KM.lookup "physical" row, KM.lookup "tcm" row) of
    (Just physical, Nothing) -> PhysicalSpan <$> parseSpan physical
    (Nothing, Just tcm) -> do
      inner <- fields ["bank", "offsets"] tcm
      TcmSpan <$> (get inner "bank" >>= closed bankName "TCM bank") <*> (field inner "offsets" >>= parseSpan)
    _ -> Left "scope span names neither physical nor TCM"

parseTotals :: Object -> Either String CounterTotals
parseTotals row = do
  totals <- CounterTotals <$> get row "count" <*> get row "zero_count" <*> get row "nonzero_count"
  require (totalZero totals + totalNonzero totals == totalCount totals) "counter zero and nonzero counts do not sum to the count"
  pure totals

parseSnapshot :: Value -> Either String TcmSnapshot
parseSnapshot value = do
  row <- fields ["core", "btcm", "atcm"] value
  let register name = field row name >>= fields ["register", "window"] >>= \bank -> get bank "register"
  TcmSnapshot <$> get row "core" <*> register "atcm" <*> register "btcm"

parseScoped :: Value -> Either String ScopedWatch
parseScoped value = do
  row <- fields ["enabled", "ranges", "records", "violation", "classes", "views", "fault", "tcm", "confinement", "counters", "coverage"] value
  noViolation row
  fault <- field row "fault"
  require (fault == Null) "scoped watch recorded a fault"
  views <- get row "views" :: Either String String
  require (views == "fixed") "scoped watch views are not fixed"
  snapshotValues <- get row "tcm" :: Either String [Value]
  snapshot <- mapM parseSnapshot snapshotValues
  let coreOf (TcmSnapshot core _ _) = core
      parseConfinement entry = do
        inner <- fields ["cores", "kind", "mode", "spans", "tcm"] entry
        cores <- sort <$> get inner "cores"
        described <- get inner "tcm" >>= mapM parseSnapshot
        require (sort described == sort (filter ((`elem` cores) . coreOf) snapshot)) "confinement TCM snapshot differs from the watch snapshot"
        spans <- get inner "spans" >>= mapM parseScopeSpan
        Confinement cores <$> (get inner "kind" >>= closed accessName "confinement kind")
          <*> (get inner "mode" >>= closed modeName "confinement mode") <*> pure (sort spans)
      parseCounter entry = do
        inner <- fields ["cores", "cell", "head_bound", "tail_bound", "count", "zero_count", "nonzero_count", "first_nonzero"
          , "first_nonzero_index", "head", "tail"] entry
        first <- field inner "first_nonzero" >>= nullable parseRow
        index <- get inner "first_nonzero_index"
        paired <- case (first, index) of
          (Just sample, Just at) -> Right (Just (sample, at))
          (Nothing, Nothing) -> Right Nothing
          _ -> Left "counter first nonzero sample and index disagree"
        CounterState <$> (sort <$> get inner "cores") <*> (field inner "cell" >>= parseSpan) <*> get inner "head_bound"
          <*> get inner "tail_bound" <*> parseTotals inner <*> pure paired
          <*> (field inner "head" >>= json >>= mapM parseRow) <*> (field inner "tail" >>= json >>= mapM parseRow)
      parseCoverage entry = do
        inner <- fields ["cores", "range", "covered_words", "first_uncovered"] entry
        CoverageState <$> (sort <$> get inner "cores") <*> (field inner "range" >>= parseSpan) <*> get inner "covered_words" <*> get inner "first_uncovered"
  confinement <- get row "confinement" >>= mapM parseConfinement
  counters <- get row "counters" >>= mapM parseCounter
  coverage <- get row "coverage" >>= mapM parseCoverage
  classes <- field row "classes" >>= parseClasses
  records <- field row "records" >>= json >>= mapM parseRow
  pure (ScopedWatch classes records (sort snapshot) (sort confinement) counters coverage)

parseStop :: Value -> Either String Stop
parseStop value = do
  row <- fields ["label", "address", "core", "instructions", "counts", "registers", "sp", "lr", "cpsr", "completion", "tcm", "counter", "watch_records"] value
  label <- get row "label" >>= closed labelName "loader-prefix stop label"
  core <- get row "core" >>= parseCore
  counts <- get row "counts" >>= twoOf "stop counts"
  tcm <- get row "tcm" >>= \list -> case list of
    [[a0, b0], [a1, b1]] -> Right (TcmWords (a0, b0) (a1, b1))
    _ -> Left "stop TCM words must hold two banks per core"
  _ <- get row "instructions" :: Either String Word64
  Stop label <$> guestField row "address" <*> pure core <*> pure counts <*> (field row "registers" >>= P.parseRegisters)
    <*> get row "sp" <*> get row "lr" <*> (Cpsr <$> get row "cpsr") <*> get row "completion" <*> pure tcm
    <*> (field row "counter" >>= nullable (\entry -> fields ["count", "zero_count", "nonzero_count"] entry >>= parseTotals))
    <*> get row "watch_records"

bytesField :: Object -> String -> Int -> Either String B.ByteString
bytesField row name size = do
  list <- get row name :: Either String [Word8]
  require (length list == size) (name ++ " width differs")
  pure (B.pack list)

viewPair :: Int -> Value -> Either String (B.ByteString, B.ByteString)
viewPair size value = do
  row <- fields ["virtual", "physical"] value
  (,) <$> bytesField row "virtual" size <*> bytesField row "physical" size

parseBindings :: Value -> Either String Bindings
parseBindings value = do
  row <- fields ["vector_slots", "stack_window", "zero_range", "veneer"] value
  let bound name = do
        entry <- field row name >>= fields ["guest", "size", "map"]
        inner <- field entry "map" >>= fields ["core", "address", "physical", "size"]
        core <- get inner "core" :: Either String Int
        require (core == 1) (name ++ " map is not core 1's")
        mapped <- StaticMap <$> get inner "address" <*> get inner "physical" <*> get inner "size"
        guest <- get entry "guest"
        size <- get entry "size"
        _ <- through mapped guest size
        pure ((guest, size), mapped)
  ((slotsGuest, slotsSize), slotsMap) <- bound "vector_slots"
  ((zeroGuest, zeroSize), zeroMap) <- bound "zero_range"
  require (slotsGuest == fromIntegral vectorBase + 0x24 && slotsSize == 28 && zeroGuest == 0x400ddaa0 && zeroSize == 0x15980)
    "bound vector-slot or zero-range extent differs"
  Bindings slotsMap <$> (field row "stack_window" >>= parseScopeSpan) <*> pure zeroMap
    <*> (field row "veneer" >>= nullable (viewPair (fromIntegral (C.veneerEnd - C.veneerEntry))))

parseStore :: Value -> Either String CommittedStore
parseStore value = do
  row <- fields ["instruction", "before_pc", "after_pc", "before_cpsr", "after_cpsr", "microsteps", "readback", "counter", "watch_records"] value
  instruction <- field row "instruction" >>= fields ["virtual", "physical"]
  readback <- field row "readback" >>= fields ["virtual", "physical"]
  steps <- get row "microsteps" >>= mapM parseStep
  require (not (null steps) && length steps <= 2048) "committed store microstep bound exceeded"
  CommittedStore <$> ((,) <$> get instruction "virtual" <*> get instruction "physical") <*> get row "before_pc" <*> get row "after_pc"
    <*> ((,) <$> (Cpsr <$> get row "before_cpsr") <*> (Cpsr <$> get row "after_cpsr")) <*> pure steps
    <*> ((,) <$> get readback "virtual" <*> get readback "physical")
    <*> (field row "counter" >>= fields ["count", "zero_count", "nonzero_count"] >>= parseTotals) <*> get row "watch_records"
  where
    parseStep entry = do
      step <- fields ["before", "after", "cell"] entry
      Microstep <$> (get step "before" >>= twoOf "microstep counts") <*> (get step "after" >>= twoOf "microstep counts") <*> get step "cell"

parseEndpoint :: Value -> Either String Endpoint
parseEndpoint value = do
  row <- fields ["frame", "vectors", "zero", "record_immutable_sha256", "loader_sha256"] value
  zero <- field row "zero" >>= fields ["range", "first_nonzero"]
  Endpoint <$> (field row "frame" >>= viewPair 16) <*> (field row "vectors" >>= viewPair 28)
    <*> ((,) <$> (field zero "range" >>= parseSpan) <*> get zero "first_nonzero")
    <*> (get row "record_immutable_sha256" >>= F.parseSha256) <*> (get row "loader_sha256" >>= F.parseSha256)

parsePrimary :: Value -> Either String Primary
parsePrimary value = do
  row <- fields ["address", "registers", "sp", "lr", "cpsr", "instructions"] value
  _ <- get row "instructions" :: Either String Word64
  Primary <$> get row "address" <*> (field row "registers" >>= P.parseRegisters) <*> get row "sp" <*> get row "lr" <*> (Cpsr <$> get row "cpsr")

-- * Acceptance

expectedClasses :: Scenario -> [ClassState]
expectedClasses scenario = sort $
  [ ClassState cellSpan writes, ClassState loaderB writes, ClassState record2 writes, ClassState zeroRange [HostWrite, ModelBusWrite]
  , ClassState loaderACompletion writes, ClassState loaderALiteral writes ]
    ++ [ClassState veneerSpan writes | scenario == Delayed]

secondaryFetch, secondaryRead :: [Span]
secondaryFetch = [Span low high | S.Interval low high <- S.forbidFetchIntervals]
secondaryRead = [Span low high | S.Interval low high <- S.forbidReadIntervals]

startupClasses :: Maybe StartupFacts -> [ClassState]
startupClasses Nothing = []
startupClasses (Just facts) =
  [ClassState (startupImage facts) writes, ClassState (Span 0 0x40) writes, ClassState (Span 0x1800 0x1804) [CpuFetch]]
    ++ [ClassState span' [CpuFetch] | span' <- secondaryFetch]
    ++ [ClassState span' [CpuRead] | span' <- secondaryRead]

expectedConfinement :: ProgramKind -> Maybe StartupFacts -> Word64 -> ScopeSpan -> [Confinement]
expectedConfinement kind selected slots window = sort $
  [ Confinement [1] CpuWrite Allow (sort [PhysicalSpan zeroRange, PhysicalSpan (Span slots (slots + 28)), window])
  , Confinement [0] CpuWrite Forbid (sort (PhysicalSpan zeroRange : immutable)) ]
    ++ case kind of
      RetainedProgram -> []
      OwnedProgram ->
        [ Confinement [1] CpuFetch Forbid (sort (map PhysicalSpan replacedCode ++ fetches))
        , Confinement [1] CpuRead Forbid (sort (PhysicalSpan entryPool : readSpans)) ]
        ++ [rule | isJust selected, rule <- [Confinement [0] CpuFetch Forbid fetches, Confinement [0] CpuRead Forbid readSpans]]
  where
    immutable = [PhysicalSpan span' | Just startup <- [selected], span' <- [startupImage startup, Span 0 0x40]]
    fetches = [PhysicalSpan span' | isJust selected, span' <- Span 0x1800 0x1804 : secondaryFetch]
    readSpans = [PhysicalSpan span' | isJust selected, span' <- secondaryRead]

-- | A physical CPU write's core, PC, physical address and preimage.
cpuWrite :: Row -> Maybe (Core, Word32, Word64, B.ByteString)
cpuWrite row = case (rowAccess row, rowAttribution row, rowLocation row, rowBefore row) of
  (CpuWrite, Just (Attribution core pc _ _), PhysicalSpan (Span physical _), Just before) -> Just (core, pc, physical, before)
  _ -> Nothing

attribution :: Row -> Either String Attribution
attribution row = maybe (Left "watch row lacks CPU attribution") Right (rowAttribution row)

uniqueStop :: [Stop] -> Label -> Either String Stop
uniqueStop stops label = case filter ((== label) . stopLabel) stops of
  [found] -> Right found
  _ -> Left ("receipt lacks exactly one " ++ labelName label ++ " stop")

counterAt :: Stop -> Either String CounterTotals
counterAt stop = maybe (Left (labelName (stopLabel stop) ++ " stop lacks its counter snapshot")) Right (stopCounter stop)

masked :: Cpsr -> Bool
masked (Cpsr value) = value .&. 0x010002ff == 0xd3

-- | NZCV excluded.
compared :: Cpsr -> Word32
compared (Cpsr value) = value .&. 0x0fffffff

registerList :: Registers -> [Word32]
registerList registers = map ($ registers) [r0, r1, r2, r3, r4, r5, r6, r7, r8, r9, r10, r11, r12]

registerAt :: RegisterIndex -> Registers -> Word32
registerAt (RegisterIndex index) registers = registerList registers !! index

-- | Registers rN..rM inclusive.
registerRange :: Int -> Int -> Registers -> [Word32]
registerRange low high = take (high - low + 1) . drop low . registerList

tcmWindow :: Word32 -> Either String (Maybe Span)
tcmWindow value = fmap (\(S.Interval low high) -> Span low high) <$> S.tcmWindow value

-- | The entry-stack window below the handoff SP as offsets in the core-1 TCM
-- bank holding it.
stackWindow :: Stop -> Either String ScopeSpan
stackWindow handoff = do
  let TcmWords _ (atcm, btcm) = stopTcm handoff
      top = fromIntegral (stopSp handoff) :: Word64
      low = top - entryStackBytes
  windows <- forM [(Atcm, atcm), (Btcm, btcm)] $ \(bank, register) -> fmap ((,) bank) <$> tcmWindow register
  case [(bank, base) | Just (bank, Span base end) <- windows, low >= base && top <= end] of
    [(bank, base)] -> Right (TcmSpan bank (Span (low - base) (top - base)))
    _ -> Left "entry-stack window does not lie in one core-1 TCM bank"

-- | Labels each scenario requires exactly once, beyond any number of post-load stops.
requiredLabels :: Scenario -> [Label]
requiredLabels scenario = [Record1Entry, SecondaryEntry, Record2Handoff, PrefixEntry] ++ case scenario of
  Held -> [HeldGate]
  Delayed -> [DelayEntry, DelayRelease, CompletionStore, PrefixExit]
  _ -> [CompletionStore, PrefixExit]

validateReceipt :: Contract -> Receipt -> Either String ValidatedReceipt
validateReceipt contract receipt = do
  let (scenario, Program kind facts, selected) = contractParts contract
  require (receiptStartup receipt == selected) "receipt startup identity or schema differs from its contract"
  require (receiptScenario receipt == scenario && receiptProgram receipt == kind) "receipt scenario or program differs from its contract"
  let stops = receiptStops receipt
  require (Refusal `notElem` map stopLabel stops) "core-1 prefix refusal reached"
  require (receiptResult receipt == Passed && isNothing (receiptError receipt) && isNothing (receiptMachineFailure receipt))
    ("loader-prefix receipt did not pass: " ++ fromMaybe "" (receiptError receipt))
  require (receiptIdentities receipt == (0x4003, 0x5003)) "CP14 identities differ"
  require (sort [stopLabel stop | stop <- stops, stopLabel stop /= PostLoad]
    == sort (requiredLabels scenario ++ [FirstStageHandoff | isJust selected]))
    "stop labels differ from the scenario"
  forM_ stops $ \stop -> require (stopCore stop == labelCore (stopLabel stop)) "stop has the wrong core"
  let pairs = zip stops (drop 1 stops)
      snapshots = [loads | Just loads <- map stopCounter stops]
  require (all (\(earlier, later) -> fst (stopCounts earlier) <= fst (stopCounts later) && snd (stopCounts earlier) <= snd (stopCounts later)) pairs)
    "a core's entered-instruction count decreased between stops"
  require (and (zipWith (\a b -> totalCount a <= totalCount b && totalNonzero a <= totalNonzero b) snapshots (drop 1 snapshots)))
    "counter snapshots decreased between stops"
  scoped <- maybe (Left "scoped watch evidence absent") Right (receiptScoped receipt)
  bindings <- maybe (Left "handoff bindings absent") Right (receiptBindings receipt)
  let ClassifiedWatch bootViews bootClasses bootRows = receiptBoot receipt
      ClassifiedWatch loadedViews loadedClasses loadedRows = receiptLoaded receipt
  require (bootViews == ObserveViews && loadedViews == (if isJust selected then FixedViews else ObserveViews))
    "classified watch views differ from the startup role"
  require (bootClasses == [ClassState cellSpan writes] && null bootRows) "boot watch classes differ or it recorded a write"
  require (loadedClasses == sort (ClassState cellSpan writes : [ClassState veneerSpan writes | scenario == Delayed] ++ startupClasses selected))
    "loaded watch classes differ"
  case loadedRows of
    [row] | Just (Core0, 0x1fec, physical, before) <- cpuWrite row, physical == completionCell, rowValue row == Just 0
      , before == B.replicate 4 0 -> pure ()
    _ -> Left "loaded watch does not hold exactly record 1's zero write of the completion cell"
  record1 <- uniqueStop stops Record1Entry
  let selectedEntry = maybe (Guest record1Entry) startupEntry selected
  require (stopAddress record1 == selectedEntry && take 1 stops == [record1] && isNothing (stopCounter record1)) "record-1 entry is not the first stop"
  handoff <- uniqueStop stops Record2Handoff
  require (stopAddress handoff == factHandoff facts && masked (stopCpsr handoff)) "handoff site or mode differs"
  require (r3 (stopRegisters handoff) == guestWord (factEntry facts) && r0 (stopRegisters handoff) == argumentWord && stopCompletion handoff == 0)
    "handoff target, argument or completion cell differs"
  require (isNothing (stopCounter handoff)) "handoff stop carries a scoped counter"
  secondary <- uniqueStop stops SecondaryEntry
  let secondaryAddress = case kind of RetainedProgram -> Guest 0x11800; OwnedProgram -> Guest 0x10400
      beforeHandoff = takeWhile ((/= Record2Handoff) . stopLabel) stops
  require (secondary `elem` beforeHandoff && stopAddress secondary == secondaryAddress) "secondary entry site or order differs"
  require (masked (stopCpsr secondary) && isNothing (stopCounter secondary)) "secondary entry context differs"
  require (receiptInitialRecord receipt == Just (factRecord facts)) "initial full record-2 identity differs"
  require (receiptHandoffRecordImmutable receipt == Just (factRecordImmutable facts)) "handoff immutable record-2 identity differs"
  forM_ selected $ \startup -> do
    firstHandoff <- uniqueStop stops FirstStageHandoff
    let beforeCore0Continuation = takeWhile ((`notElem` [HeldGate, DelayEntry, CompletionStore]) . stopLabel) stops
    require (firstHandoff `elem` beforeCore0Continuation)
      "firststage handoff does not precede core-0 continuation"
    require (stopAddress firstHandoff == startupHandoff startup && masked (stopCpsr firstHandoff)
      && r2 (stopRegisters firstHandoff) == guestWord (startupTarget startup)
      && r0 (stopRegisters firstHandoff) == guestWord (startupArgument startup)) "firststage handoff context differs"
    if firstHandoff `elem` beforeHandoff
      then require (isNothing (stopCounter firstHandoff) && isNothing (stopCursor firstHandoff))
        "pre-scoped firststage handoff carries scoped evidence"
      else do
        loads <- counterAt firstHandoff
        require (totalNonzero loads == 0 && stopCursor firstHandoff == Just 0)
          "scoped firststage handoff lacks its pre-completion observation"
  window <- stackWindow handoff
  require (bindingStackWindow bindings == window) "bound entry-stack window differs from the handoff SP and TCM"
  zeroPhysical <- through (bindingZeroMap bindings) 0x400ddaa0 0x15980
  require (Span zeroPhysical (zeroPhysical + 0x15980) == zeroRange) "bound zero range does not map to physical [0xddaa0,0xf3420)"
  let TcmWords (atcm0, btcm0) (atcm1, btcm1) = stopTcm handoff
  require (scopedTcm scoped == sort [TcmSnapshot 0 atcm0 btcm0, TcmSnapshot 1 atcm1 btcm1]) "scoped TCM snapshot differs from the handoff TCM"
  forM_ (dropWhile (/= handoff) stops) $ \stop -> require (stopTcm stop == stopTcm handoff) "TCM registers changed after the handoff"
  -- Identity and 0x40000000-alias views of every guarded span, on either core.
  let guardedViews = [view | guardedSpan <- [cellSpan, record2, loaderB, loaderACompletion, loaderALiteral] ++ [veneerSpan | scenario == Delayed]
                               ++ [span' | ClassState span' _ <- startupClasses selected]
                           , view <- [guardedSpan, aliased guardedSpan]]
      aliased (Span start end) = Span (start + 0x40000000) (end + 0x40000000)
      meets (Span start end) (Span otherStart otherEnd) = start < otherEnd && otherStart < end
  tcmRegions <- mapM tcmWindow [atcm0, btcm0, atcm1, btcm1]
  require (and [not (meets region view) | Just region <- tcmRegions, view <- guardedViews]) "enabled TCM window overlaps a guarded guest view"
  case (scenario, bindingVeneer bindings) of
    (Delayed, Just (virtualBytes, physicalBytes)) -> do
      veneer <- C.delayVeneer
      require (virtualBytes == veneer && physicalBytes == veneer) "delayed veneer bytes differ in the run"
    (Delayed, Nothing) -> Left "delayed veneer binding absent"
    (_, Just _) -> Left "veneer binding outside the delayed control"
    _ -> pure ()
  entry <- uniqueStop stops PrefixEntry
  require (entry `elem` drop 1 (dropWhile (/= handoff) stops)) "prefix entry does not follow record-2 handoff"
  require (stopAddress entry == factEntry facts && r0 (stopRegisters entry) == argumentWord && stopSp entry == stopSp handoff
    && stopCpsr entry == stopCpsr handoff && registerRange 6 11 (stopRegisters entry) == registerRange 6 11 (stopRegisters handoff))
    "prefix entry differs from the handoff context"
  require (snd (stopCounts entry) == snd (stopCounts handoff) + 1) "prefix entry is not the core-1 instruction after the handoff"
  require (stopCounter entry == Just (CounterTotals 0 0 0) && stopCursor entry == Just 0)
    "scoped watch was not active and empty at the prefix entry"
  require (scopedClasses scoped == sort (expectedClasses scenario ++ startupClasses selected)) "scoped watch classes differ"
  slots <- through (bindingSlotsMap bindings) (fromIntegral vectorBase + 0x24) 28
  require (scopedConfinement scoped == expectedConfinement kind selected slots (bindingStackWindow bindings)) "scoped confinement rules differ"
  counter <- case scopedCounters scoped of
    [found] | counterCores found == [1] && counterCell found == cellSpan && counterHeadBound found == headBound
      && counterTailBound found == tailBound -> Right found
    _ -> Left "scoped completion counter differs"
  case scopedCoverage scoped of
    [CoverageState [1] range covered uncovered] | range == zeroRange ->
      require (covered == coverageWords && isNothing uncovered) "core-1 zero coverage of [0xddaa0,0xf3420) is incomplete"
    _ -> Left "scoped zero coverage rule differs"
  let totals = counterTotals counter
      expected = fromIntegral (C.completionValue (scenarioControl scenario)) :: Word64
      PollPeriod period = factPeriod facts
  forM_ (counterHead counter ++ counterTail counter) $ \row -> do
    Attribution core pc _ _ <- attribution row
    require (rowAccess row == CpuRead && core == Core1 && pc == guestWord (factLoad facts) && rowLocation row == PhysicalSpan cellSpan)
      "counter sample is not the core-1 completion load"
  require (length (counterHead counter) == min headBound (fromIntegral (totalCount totals))
    && length (counterTail counter) == min tailBound (fromIntegral (totalCount totals))) "counter sample counts differ"
  require (all ((== Just 0) . rowValue) (counterHead counter ++ counterTail counter)
    || totalNonzero totals > 0) "counter sample of a zero count is nonzero"
  let posts = filter ((== PostLoad) . stopLabel) stops
  postCounts <- mapM counterAt posts
  forM_ (zip posts postCounts) $ \(stop, loads) -> do
    require (stopAddress stop == factPostLoad facts && totalCount loads > 0) "post-load stop site differs or precedes every load"
    require (registerAt (factLoaded facts) (stopRegisters stop) == fromIntegral (if totalNonzero loads == 0 then 0 else expected))
      "post-load register differs from the loaded completion value"
    when (kind == RetainedProgram) $
      require (r1 (stopRegisters stop) + 0xc4 == 0x40000000 + fromIntegral completionCell) "retained load base register differs"
  let waiting = [(stop, loads) | (stop, loads) <- zip posts postCounts, totalNonzero loads == 0]
      (beforeGate, fromGate) = break ((== HeldGate) . stopLabel) stops
      armed = [filter ((== PostLoad) . stopLabel) group | group <- beforeGate : [drop 1 fromGate | not (null fromGate)]]
  require (length waiting >= min headPolls (fromIntegral (totalZero totals))) "fewer waiting post-load stops than head polls"
  forM_ armed $ \group -> do
    loads <- mapM counterAt group
    zipWithM_ (\(earlier, a) (later, b) -> do
      require (totalCount b == totalCount a + 1) "consecutive post-load stops do not pair with exactly one completed load"
      when (totalNonzero b == 0) $
        require (snd (stopCounts later) - snd (stopCounts earlier) == period) "consecutive waiting loads differ from the poll period")
      (zip group loads) (drop 1 (zip group loads))
  if scenario == Held
    then acceptHeld scoped counter stops receipt
    else acceptCompletion scenario facts expected scoped counter stops receipt handoff
  pure (ValidatedReceipt contract receipt)

acceptHeld :: ScopedWatch -> CounterState -> [Stop] -> Receipt -> Either String ()
acceptHeld scoped counter stops receipt = do
  require (null (scopedRows scoped)) "held control recorded a guarded write"
  require (totalNonzero (counterTotals counter) == 0 && isNothing (counterFirstNonzero counter)) "held control observed a nonzero completion load"
  gate <- uniqueStop stops HeldGate
  require (stopAddress gate == Guest C.heldGate && stopCompletion gate == 0) "held gate site or cell differs"
  let after = [stop | stop <- drop 1 (dropWhile ((/= HeldGate) . stopLabel) stops), stopLabel stop == PostLoad]
  require (length after >= headPolls) "held control lacks post-gate waiting loads"
  Primary address _ _ _ _ <- maybe (Left "held primary context absent") Right (receiptPrimary receipt)
  require (address == C.heldGate) "held core 0 left its gate"
  require (isNothing (receiptStore receipt) && isNothing (receiptEndpoint receipt)) "held control carries completion or endpoint evidence"

acceptCompletion :: Scenario -> ProgramFacts -> Word64 -> ScopedWatch -> CounterState -> [Stop] -> Receipt -> Stop -> Either String ()
acceptCompletion scenario facts expected scoped counter stops receipt handoff = do
  let totals = counterTotals counter
  case scopedRows scoped of
    [row] | Just (Core0, 0x40a08, physical, before) <- cpuWrite row, physical == completionCell
      , before == B.replicate 4 0, rowValue row == Just expected -> pure ()
    _ -> Left "scoped watch does not hold exactly the original 0x40a08 completion store"
  store <- uniqueStop stops CompletionStore
  require (stopAddress store == Guest 0x40a08 && stopCompletion store == 0 && r3 (stopRegisters store) == 0x400a0000 && r5 (stopRegisters store) == 1)
    "completion-store stop context differs"
  storeCount <- counterAt store
  require (totalNonzero storeCount == 0) "a nonzero completion load preceded the store"
  committed <- maybe (Left "committed store readback absent") Right (receiptStore receipt)
  let instruction = C.completionStoreWord (scenarioControl scenario)
      value = fromIntegral expected
  require (storeInstruction committed == (instruction, instruction) && storeBeforePc committed == 0x40a08 && storeAfterPc committed == 0x40a0c
    && storeCpsr committed == (stopCpsr store, stopCpsr store) && storeReadback committed == (value, value))
    "original store lacks its instruction, completion PC or immediate readback"
  -- Core 1 stays paused across the committing step: its count is unchanged
  -- and no nonzero load precedes the post-commit snapshot.
  let steps = storeSteps committed
      Microstep (before0, before1) (after0, after1) committedCell = last steps
      single (Microstep (b0, b1) (a0, a1) _) = (a0 - b0, a1 - b1) `elem` [(1, 0), (0, 1)]
  require (all single steps && after0 == before0 + 1 && after1 == before1 && committedCell == value
    && all (\(Microstep _ _ cell) -> cell == 0) (init steps)) "the original store did not commit in one core-0 step with core 1 paused"
  let after = storeCounter committed
  storeCursor' <- maybe (Left "completion-store stop lacks its watch cursor") Right (stopCursor store)
  require (totalNonzero after == 0 && totalCount after >= totalCount storeCount && storeCursor' == 0 && storeCursor committed == 1)
    "the original store row or a nonzero load lies outside the commit window"
  (first, index) <- maybe (Left "no nonzero completion load") Right (counterFirstNonzero counter)
  Attribution firstCore firstPc _ _ <- attribution first
  require (totalNonzero totals == 1 && index == totalCount totals - 1 && index >= totalCount after)
    "completion loads are not all zero before one final nonzero load after the commit"
  require (firstCore == Core1 && firstPc == guestWord (factLoad facts) && rowValue first == Just expected
    && rowBefore first == Just (encodeLeWord value)) "final completion load differs"
  require (lastOf (counterTail counter) == Just first) "counter tail does not end with the nonzero load"
  when (scenario == Delayed) $ do
    delayEntry <- uniqueStop stops DelayEntry
    release <- uniqueStop stops DelayRelease
    let Cpsr entryCpsr = stopCpsr delayEntry
        Cpsr releaseCpsr = stopCpsr release
    require (stopAddress delayEntry == Guest C.veneerEntry && stopLr delayEntry == C.heldGate + 4) "delay entry differs"
    require (stopAddress release == Guest C.veneerRelease && releaseCpsr .&. 0xf8000000 == entryCpsr .&. 0xf8000000
      && stopSp release == stopSp delayEntry && r11 (stopRegisters release) == r11 (stopRegisters delayEntry))
      "delay release did not restore NZCVQ, SP or r11"
    entryCount <- counterAt delayEntry
    require (totalCount storeCount >= totalCount entryCount + delayedZeroLoads) "delayed control did not observe the required zero loads during the delay"
  exit <- uniqueStop stops PrefixExit
  let regs = stopRegisters exit
      handoffRegs = stopRegisters handoff
  require (stopAddress exit == Guest exitAddress) "exit stop site differs"
  require (r4 regs == argumentWord && r5 regs == argumentWord && r6 regs == vectorBase
    && registerRange 7 10 regs == registerRange 7 10 handoffRegs && r11 regs == exitFrame && stopSp exit == exitSp
    && compared (stopCpsr exit) == compared (stopCpsr handoff)) "exit registers, SP or CPSR outside NZCV differ from the stock contract"
  exitCount <- counterAt exit
  require (exitCount == totals) "loads continued after the exit stop"
  endpoint <- maybe (Left "endpoint readback absent") Right (receiptEndpoint receipt)
  let frame = B.concat (map encodeLeWord [stackTop, argumentWord, r11 handoffRegs, frameReturn])
      vectors = B.concat (map encodeLeWord vectorHandlers)
  require (endpointFrame endpoint == (frame, frame)) "exit frame differs"
  require (endpointVectors endpoint == (vectors, vectors)) "exit vector slots differ"
  require (endpointZero endpoint == (zeroClaim, Nothing)) "exit zero range holds a nonzero byte"
  require (endpointRecordImmutable endpoint == factRecordImmutable facts) "exit immutable record-2 bytes differ from the contract"
  require (endpointLoader endpoint == factLoader facts) "exit loader-B bytes differ from the contract"
  require (isNothing (receiptPrimary receipt)) "completion scenario carries a held primary context"
  where
    lastOf list = if null list then Nothing else Just (last list)

-- | Each receipt accepted under its own contract; then handoff and exit
-- state, frame, vectors, zero range, stored value and coverage agree.
compareReceipts :: (Contract, Receipt) -> (Contract, Receipt) -> Either String ()
compareReceipts (retainedContract, retainedReceipt) (ownedContract, ownedReceipt) = do
  let (retainedScenario, Program retainedKind _, _) = contractParts retainedContract
      (ownedScenario, Program ownedKind _, _) = contractParts ownedContract
  require (retainedKind == RetainedProgram && ownedKind == OwnedProgram) "comparison requires a retained and an owned contract"
  require (retainedScenario == ownedScenario) "compared contracts differ in scenario"
  ValidatedReceipt _ retained <- validateReceipt retainedContract retainedReceipt
  ValidatedReceipt _ owned <- validateReceipt ownedContract ownedReceipt
  let final label receipt = uniqueStop (receiptStops receipt) label
      sameContext low high left right =
        registerRange low high (stopRegisters left) == registerRange low high (stopRegisters right)
          && stopSp left == stopSp right && compared (stopCpsr left) == compared (stopCpsr right)
  retainedHandoff <- final Record2Handoff retained
  ownedHandoff <- final Record2Handoff owned
  require (sameContext 6 11 retainedHandoff ownedHandoff) "handoff r6-r11, SP or CPSR outside NZCV differ"
  unless (retainedScenario == Held) $ do
    retainedExit <- final PrefixExit retained
    ownedExit <- final PrefixExit owned
    require (sameContext 4 11 retainedExit ownedExit) "exit r4-r11, SP or CPSR outside NZCV differ"
    let views receipt = fmap (\endpoint -> (endpointFrame endpoint, endpointVectors endpoint, endpointZero endpoint)) (receiptEndpoint receipt)
    require (views retained == views owned) "exit frame, vectors or zero range differ"
    require (fmap storeReadback (receiptStore retained) == fmap storeReadback (receiptStore owned)) "committed completion value differs"
  require (fmap scopedCoverage (receiptScoped retained) == fmap scopedCoverage (receiptScoped owned)) "zero coverage differs"

receiptArgument :: ValidatedReceipt -> Either String Word32
receiptArgument (ValidatedReceipt _ receipt) = r0 . stopRegisters <$> uniqueStop (receiptStops receipt) Record2Handoff

-- * JSON

objectOf :: Value -> Either String Object
objectOf (Object row) = Right row
objectOf _ = Left "loader-prefix witness value is not an object"

fields :: [String] -> Value -> Either String Object
fields names value = do
  row <- objectOf value
  unless (sort names == sort (map K.toString (KM.keys row))) (Left ("loader-prefix witness fields differ: " ++ unwords names))
  pure row

field :: Object -> String -> Either String Value
field row name = maybe (Left ("missing loader-prefix witness field: " ++ name)) Right (KM.lookup (K.fromString name) row)

json :: FromJSON a => Value -> Either String a
json value = case fromJSON value of
  Error problem -> Left problem
  Success parsed -> Right parsed

get :: FromJSON a => Object -> String -> Either String a
get row name = field row name >>= json

nullable :: (Value -> Either String a) -> Value -> Either String (Maybe a)
nullable _ Null = Right Nothing
nullable parse value = Just <$> parse value

guestField :: Object -> String -> Either String Guest
guestField row name = Guest <$> get row name

parseSpan :: Value -> Either String Span
parseSpan value = do
  bounds <- json value :: Either String [Word64]
  case bounds of
    [low, high] | low < high && high <= 0x100000000 -> Right (Span low high)
    _ -> Left "span must be an increasing [low, high] pair within 4 GiB"

closed :: (Enum a, Bounded a) => (a -> String) -> String -> String -> Either String a
closed render what text = case [item | item <- [minBound .. maxBound], render item == text] of
  [found] -> Right found
  _ -> Left ("unknown " ++ what ++ ": " ++ text)

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""
