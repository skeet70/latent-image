{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The owned menu engine's guest pair checks over RAM-push e2e observations
-- (`e2e-observation-N.json`): each role's events are classified at `0xd4dd2c`, the owned role
-- fails on an entry to `0xbfa234` from the fallback call or to `0xbfad04` for an owned class, or
-- on the adapter's guard trap, and the two roles must read the same engine state and post the
-- same `0x1d` events.
module Replacement.ModalRuntime.MenuEnginePair
  ( Role (..)
  , Hit (..)
  , Observation (..)
  , Residency (..)
  , parseObservation
  , ObservationReader (..)
  , readObservationReader
  , receiptObservations
  , checkRoleReader
  , checkStackReader
  , comparePairReader
  , ValueSite (..)
  , Notification (..)
  , ValueEntries (..)
  , ValueSegment (..)
  , ArmRequirement (..)
  , ValueReport (..)
  , checkValueSeam
  , checkValueSeamObservations
  , checkValueSeamReader
  , requireValueDeclarations
  , RtcByteIndex (..)
  , ClockEvidence (..)
  , checkConstantClockInputs
  , ClockEvidenceReader (..)
  , checkConstantClockInputsReader
  , parseSessionClockHits
  , observationPhaseOf
  , segmentResidencies
  , residencyAt
  , Coverage (..)
  , RoleSummary (..)
  , checkRole
  , comparePair
  , Site (..)
  , parseSite
  , fallbackReturn
  , engineReads
  , StackReport (..)
  , checkStack
  , taskStackStart
  ) where

import Control.Exception (evaluate)
import Control.Monad (foldM, forM, unless, when)
import qualified Data.Attoparsec.ByteString.Char8 as A
import Data.Aeson ((.:), (.:?))
import qualified Data.Aeson as J
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as J
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Char (isDigit)
import Data.List (sortOn, stripPrefix)
import qualified Data.Text.Encoding as TE
import Data.Word (Word32, Word64)
import Emulation.ObservationStream (foldTraceHitLines, loadReceiptObservationValues)
import Numeric (readHex)
import Replacement.ModalRuntime.MenuEngineClass
import Replacement.ModalRuntime.RenderObjectProvenance
import Replacement.ModalRuntime.RenderStackEvidence (InstructionCounter (..))
import System.Directory (listDirectory)
import System.FilePath ((</>), takeDirectory, takeFileName)

data Role = StockRole | OwnedRole deriving (Eq, Show)

-- | A trace site of the pair trajectories (`emulation/menu-engine-pair-trajectories`); every other
-- label is 'OtherSite'.
data RtcByteIndex = RtcByte0 | RtcByte1 | RtcByte2 | RtcByte3 deriving (Eq, Ord, Show)

data ValueSite = ValueApply | StockValueArm | ValueNotify
  deriving (Eq, Ord, Show, Enum, Bounded)

data Site = StageEntry | KindSwitch | Seek | EventPost | GuardTrap | CalendarSample | RtcCounterByte RtcByteIndex | ValueSite ValueSite | OtherSite T.Text
  deriving (Eq, Show)

parseSite :: T.Text -> Site
parseSite label = case label of
  "menu-stage-entry" -> StageEntry
  "menu-kind-switch" -> KindSwitch
  "menu-seek" -> Seek
  "menu-event-post" -> EventPost
  "menu-engine-guard-failure" -> GuardTrap
  "rtc-calendar-sampled" -> CalendarSample
  "rtc-counter-byte-0" -> RtcCounterByte RtcByte0
  "rtc-counter-byte-1" -> RtcCounterByte RtcByte1
  "rtc-counter-byte-2" -> RtcCounterByte RtcByte2
  "rtc-counter-byte-3" -> RtcCounterByte RtcByte3
  "owned-value-apply" -> ValueSite ValueApply
  "stock-value-size-arm" -> ValueSite StockValueArm
  "value-notify" -> ValueSite ValueNotify
  other -> OtherSite other

-- | One recorded trace hit: site, core, r0-r3, lr, sp and the site's reads at entry.
data Hit = Hit
  { hitSite :: Site
  , hitCore :: Int
  , hitRegisters :: (Word32, Word32, Word32, Word32)
  , hitLr :: Maybe Word32
  , hitSp :: Maybe Word32
  , hitMemory :: [B.ByteString]
  , hitProvenanceSequence :: Maybe ProvenanceSequence
  , hitInstructions :: Maybe InstructionCounter
  }
  deriving (Eq, Show)

-- | Whether the package, and with it the engine's branch at `0xbf95c8`, was published during a
-- segment. After `ram-unload` restores the stock words, `0xbf95c8` runs stock in the owned role.
data Residency = EngineResident | EngineUnloaded deriving (Eq, Ord, Show)

-- | One segment's observation: its phase, residency, labelled reads and trace hits in recording order.
data Observation = Observation
  { observationPhase :: T.Text
  , observationResidency :: Residency
  , observationReads :: [(T.Text, B.ByteString)]
  , observationHits :: [Hit]
  , observationProvenance :: Maybe ProvenanceBatch
  }
  deriving (Eq, Show)

-- | The return address of `0xbf95c8`'s call to `0xbfa234`: the owned adapter's fallback resumes
-- stock there, and every service call into `0xbfa234` returns elsewhere.
fallbackReturn :: Word32
fallbackReturn = 0x00bf95d4

-- | The reads both roles must equal: the block, the last-move pair and the snapshot areas.
engineReads :: [T.Text]
engineReads =
  [ "menu-block", "menu-last-move", "snapshot-root"
  , "snapshot-depth1-0", "snapshot-depth1-1", "snapshot-depth2-0", "snapshot-depth2-1" ]

hexBytes :: T.Text -> J.Parser B.ByteString
hexBytes text
  | odd (B.length raw) = fail "not hex bytes"
  | B.length decoded /= count = fail "not hex bytes"
  | otherwise = pure decoded
 where
  raw = TE.encodeUtf8 text
  count = B.length raw `div` 2
  decoded = fst (B.unfoldrN count step 0)
  nibble byte
    | byte >= 48 && byte <= 57 = Just (byte - 48)
    | byte >= 65 && byte <= 70 = Just (byte - 55)
    | byte >= 97 && byte <= 102 = Just (byte - 87)
    | otherwise = Nothing
  step offset = do
    high <- nibble (B.index raw offset)
    low <- nibble (B.index raw (offset + 1))
    pure (high `shiftL` 4 .|. low,offset + 2)

hexWord :: String -> J.Parser Word32
hexWord text = case text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits -> pure value
  _ -> fail ("not a hex word: " ++ text)

-- | The segments of the receipt's `e2e-result.json` phases in run order, each with its residency:
-- resident before the `ram-unload` phase, unloaded after it.
segmentResidencies :: [T.Text] -> [(T.Text, Residency)]
segmentResidencies names =
  [(phase, EngineResident) | phase <- segments before] ++ [(phase, EngineUnloaded) | phase <- segments (drop 1 after)]
 where
  (before, after) = break (== "ram-unload") names
  segments list = [phase | name <- list, Just phase <- [T.stripPrefix "segment " name]]

-- | The `index`th observation file (0-based) is the `index`th segment; its phase must match.
residencyAt :: [(T.Text, Residency)] -> Int -> T.Text -> Either String Residency
residencyAt segments index phase = case drop index segments of
  (listed, residency) : _ | listed == phase -> Right residency
  (listed, _) : _ -> Left ("observation " ++ show index ++ " has phase " ++ T.unpack phase ++ ", the run's segment " ++ show index ++ " is " ++ T.unpack listed)
  [] -> Left ("observation " ++ show index ++ " has no segment phase in the run")

parseHitValue :: J.Value -> J.Parser Hit
parseHitValue = J.withObject "trace hit" $ \hit -> do
  let register name = hexWord =<< hit .: name
  registers <- (,,,) <$> register "r0" <*> register "r1" <*> register "r2" <*> register "r3"
  lr <- traverse hexWord =<< hit .:? "lr"
  sp <- traverse hexWord =<< hit .:? "sp"
  memory <- maybe (pure []) (mapM hexBytes) =<< hit .:? "memory"
  provenanceSequence <- fmap ProvenanceSequence <$> hit .:? "render_object_provenance_sequence"
  instructions <- hit .:? "instructions"
  Hit <$> (parseSite <$> hit .: "site") <*> hit .: "core" <*> pure registers <*> pure lr <*> pure sp <*> pure memory <*> pure provenanceSequence <*> pure instructions

parseHitLine :: B.ByteString -> Either String Hit
parseHitLine = A.parseOnly hitLine
 where
  hitLine = do
    A.skipSpace
    _ <- A.char '{'
    A.skipSpace
    fields <- field `A.sepBy` (A.skipSpace *> A.char ',' *> A.skipSpace)
    A.skipSpace
    _ <- A.char '}'
    A.skipSpace
    A.endOfInput
    either fail pure (build fields)
  field = do
    _ <- A.char '"'
    key <- A.takeWhile (/= '"')
    _ <- A.char '"'
    A.skipSpace
    _ <- A.char ':'
    A.skipSpace
    fieldValue key
  fieldValue key
    | key == "site" = FSite . parseSite . T.pack . C.unpack <$> string
    | key == "core" = FCore <$> A.decimal
    | key == "instructions" = FInstructions <$> A.decimal
    | key == "r0" = FReg 0 <$> word
    | key == "r1" = FReg 1 <$> word
    | key == "r2" = FReg 2 <$> word
    | key == "r3" = FReg 3 <$> word
    | key == "lr" = FLr <$> word
    | key == "sp" = FSp <$> word
    | key == "memory" = FMemory <$> memory
    | key == "render_object_provenance_sequence" = FProvenance <$> A.decimal
    | otherwise = fail ("unknown trace hit field " ++ show key)
  string = do
    _ <- A.char '"'
    text <- A.takeWhile (/= '"')
    _ <- A.char '"'
    pure text
  word = string >>= either fail pure . hexWordText . C.unpack
  memory = do
    _ <- A.char '['
    A.skipSpace
    values <- string `A.sepBy` (A.skipSpace *> A.char ',' *> A.skipSpace)
    A.skipSpace
    _ <- A.char ']'
    mapM (either fail pure . hexBytesText . C.unpack) values
  build fields = do
    let site = first [value | FSite value <- fields]
        core = first [value | FCore value <- fields]
        instructions = first [value | FInstructions value <- fields]
        registers = [first [value | FReg regIndex value <- fields, regIndex == index] | index <- [0..3 :: Int]]
        lr = first [value | FLr value <- fields]
        sp = first [value | FSp value <- fields]
        bytes = concat [value | FMemory value <- fields]
        provenance = first [value | FProvenance value <- fields]
    site_ <- maybe (Left "trace hit lacks site") Right site
    core_ <- maybe (Left "trace hit lacks core") Right core
    r0 <- maybe (Left "trace hit lacks r0") Right (registers !! 0)
    r1 <- maybe (Left "trace hit lacks r1") Right (registers !! 1)
    r2 <- maybe (Left "trace hit lacks r2") Right (registers !! 2)
    r3 <- maybe (Left "trace hit lacks r3") Right (registers !! 3)
    pure (Hit site_ core_ (r0,r1,r2,r3) lr sp bytes (ProvenanceSequence <$> provenance)
      (InstructionCounter . fromIntegral <$> instructions))
  first [] = Nothing
  first (value : _) = Just value
  hexWordText text = case text of
    '0' : 'x' : digits | [(value, "")] <- readHex digits, value <= 0xffffffff -> Right (value :: Word32)
    _ -> Left ("not a hexadecimal word: " ++ text)
  hexBytesText text
    | odd (length text) = Left "odd hexadecimal length"
    | otherwise = B.pack <$> mapM pair (pairs text)
   where
    pairs [] = []
    pairs (high : low : rest) = [high, low] : pairs rest
    pairs _ = []
    pair [high, low] = case readHex [high, low] of
      [(value, "")] -> Right value
      _ -> Left "invalid hexadecimal byte"
    pair _ = Left "invalid hexadecimal byte"

data RowField = FSite Site | FCore Int | FInstructions Integer | FReg Int Word32 | FLr Word32 | FSp Word32 | FMemory [B.ByteString] | FProvenance Word64

parseSessionHit :: J.Value -> J.Parser Hit
parseSessionHit = J.withObject "session hit" $ \hit -> do
  registers <- hit .: "registers"
  let register name = hexWord =<< registers .: name
  (r0, r1, r2, r3) <- (,,,) <$> register "r0" <*> register "r1" <*> register "r2" <*> register "r3"
  lr <- traverse hexWord =<< hit .:? "lr"
  sp <- traverse hexWord =<< hit .:? "sp"
  memory <- maybe (pure []) (mapM hexBytes) =<< hit .:? "memory"
  provenanceSequence <- fmap ProvenanceSequence <$> hit .:? "render_object_provenance_sequence"
  instructions <- hit .:? "instructions"
  Hit <$> (parseSite <$> hit .: "site") <*> hit .: "core" <*> pure (r0, r1, r2, r3) <*> pure lr <*> pure sp <*> pure memory <*> pure provenanceSequence <*> pure instructions

parseSessionClockHits :: B.ByteString -> Either String [Hit]
parseSessionClockHits bytes = do
  value <- J.eitherDecodeStrict' bytes
  contexts <- J.parseEither (J.withObject "emulator session" (\fields -> fields J..: "contexts")) value :: Either String [J.Value]
  mapM (\hit -> J.parseEither parseSessionHit hit) (filter relevant contexts)
 where
  relevant (J.Object fields) = case KM.lookup "site" fields of
    Just (J.String site) -> case parseSite site of
      CalendarSample -> True
      RtcCounterByte _ -> True
      _ -> False
    _ -> False
  relevant _ = False

parseObservationHeader :: Residency -> J.Value -> J.Parser (T.Text, [(T.Text, B.ByteString)], Maybe ProvenanceBatch)
parseObservationHeader _ value = J.withObject "e2e observation" (\fields -> do
  phase <- fields .: "phase"
  segmentReads <- fields .: "reads" >>= mapM (J.withObject "read" $ \read_ -> (,) <$> read_ .: "label" <*> (hexBytes =<< read_ .: "bytes"))
  provenance <- traverse parseProvenanceBatch =<< fields .:? "render_object_provenance"
  pure (phase, segmentReads, provenance)) value

parseObservation :: Residency -> J.Value -> J.Parser Observation
parseObservation residency value = do
  (phase, segmentReads, provenance) <- parseObservationHeader residency value
  hits <- J.withObject "e2e observation" (\fields -> fields .: "trace_hits" >>= mapM parseHitValue) value
  pure (Observation phase residency segmentReads hits provenance)

data ObservationReader = ObservationReader
  { readerPhase :: T.Text
  , readerResidency :: Residency
  , readerReads :: [(T.Text, B.ByteString)]
  , readerProvenance :: Maybe ProvenanceBatch
  , readerFoldHits :: forall a. (a -> Hit -> Either String a) -> a -> IO (Either String a)
  }

readObservationReader :: Residency -> FilePath -> IO (Either String ObservationReader)
readObservationReader residency path = do
  bytes <- B.readFile path
  case J.eitherDecodeStrict' bytes of
    Left problem -> pure (Left problem)
    Right value -> case J.parseEither (parseObservationHeader residency) value of
      Left problem -> pure (Left problem)
      Right (phase, segmentReads, provenance) -> do
        pure (Right (ObservationReader phase residency segmentReads provenance (foldHitLines (takeDirectory path) value)))

foldHitLines :: FilePath -> J.Value -> (a -> Hit -> Either String a) -> a -> IO (Either String a)
foldHitLines directory value step initial = do
  result <- foldTraceHitLines directory value
    (\acc line -> do
      let next = acc >>= \state -> parseHitLine line >>= \hit -> step state hit
      forced <- evaluate next
      case forced of
        Left problem -> pure (Left problem)
        Right value_ -> evaluate value_ >> pure forced)
    (Right initial)
  pure (result >>= id)

-- | An observation's phase, read before its residency is known.
observationPhaseOf :: J.Value -> J.Parser T.Text
observationPhaseOf = J.withObject "e2e observation" (.: "phase")

-- | The receipt's observations, `e2e-observation-N.json` for each of its N segments in run order;
-- each takes the residency of the segment at its position.
receiptObservations :: FilePath -> IO [ObservationReader]
receiptObservations directory = do
  resultBytes <- B.readFile (directory </> "e2e-result.json")
  resultValue <- either (fail . ((directory </> "e2e-result.json: ") ++)) pure (J.eitherDecodeStrict' resultBytes)
  phases <- either (fail . ((directory </> "e2e-result.json: ") ++)) pure
    (J.parseEither (J.withObject "e2e result" $ \fields ->
      fields .: "phases" >>= mapM (J.withObject "phase" (.: "name"))) resultValue)
  let segments = segmentResidencies phases
  names <- listDirectory directory
  let numbered = sortOn fst [(read digits :: Int, name) | name <- names, Just rest <- [stripPrefix "e2e-observation-" name]
        , let digits = takeWhile isDigit rest, not (null digits), drop (length digits) rest == ".json"]
  unless (map fst numbered == [0 .. length segments - 1])
    (fail (directory ++ ": observation files " ++ show (map fst numbered) ++ " are not one per segment of the run's " ++ show (length segments)))
  verified <- either (fail . ((directory ++ ": ") ++)) pure =<< loadReceiptObservationValues directory resultValue
  unless (map (takeFileName . fst) verified == map snd numbered)
    (fail (directory ++ ": observation references do not name its numbered documents"))
  forM numbered $ \(index, name) -> do
    let path = directory </> name
    bytes <- B.readFile path
    value <- either (fail . ((path ++ ": ") ++)) pure (J.eitherDecodeStrict' bytes)
    phase <- either (fail . ((path ++ ": ") ++)) pure (J.parseEither observationPhaseOf value)
    residency <- either (fail . ((path ++ ": ") ++)) pure (residencyAt segments index phase)
    either (fail . ((path ++ ": ") ++)) pure =<< readObservationReader residency path

-- | Task 1's stack, `[0x00a4c200, 0x00a4ce00)` (E-RECOVERY-MAINUI-CREATE-150).
taskStackStart, taskStackBytes :: Word32
taskStackStart = 0x00a4c200
taskStackBytes = 0xc00

-- | The read of task 1's whole stack in each segment.
taskStackRead :: T.Text
taskStackRead = "task1-stack"

-- | The lowest address the static bound lets any stage event reach, and the lowest stack word the
-- run changed.
data StackReport = StackReport {stackFloor :: Maybe Integer, stackHighWater :: Maybe Word32}
  deriving (Eq, Show)

-- | Each stage entry's sp less `required` (the engine's and modal dispatcher's static depth plus
-- the stock margin) must stay at or above the stack's start; the lowest stack word that changed
-- between any two segments' reads must not lie below the lowest such floor.
checkStack :: Word32 -> [Observation] -> Either String StackReport
checkStack required observations = do
  floors <- forM [(observationPhase observation, hit) | observation <- observations, hit <- observationHits observation, hitSite hit == StageEntry] $
    \(phase, hit) -> case hitSp hit of
      Nothing -> Left ("stage entry without sp in " ++ T.unpack phase)
      Just sp -> Right (toInteger sp - toInteger required)
  case [floor_ | floor_ <- floors, floor_ < toInteger taskStackStart] of
    floor_ : _ -> Left ("a stage entry leaves the bound " ++ show (toInteger taskStackStart - floor_) ++ " bytes below task 1's stack")
    [] -> Right ()
  stacks <- forM observations $ \observation -> case [bytes | (name, bytes) <- observationReads observation, name == taskStackRead] of
    [bytes] | B.length bytes == fromIntegral taskStackBytes -> Right bytes
    _ -> Left ("task 1's stack read is missing or short in " ++ T.unpack (observationPhase observation))
  let changed = case stacks of
        first : rest -> [offset | offset <- [0, 4 .. fromIntegral taskStackBytes - 4], any (\later -> B.take 4 (B.drop offset later) /= B.take 4 (B.drop offset first)) rest]
        [] -> []
      highWater = case changed of
        offset : _ -> Just (taskStackStart + fromIntegral offset)
        [] -> Nothing
      lowest = if null floors then Nothing else Just (minimum floors)
  case (highWater, lowest) of
    (Just mark, Just floor_) | toInteger mark < floor_ ->
      Left ("task 1's stack changed at " ++ show mark ++ ", below the bound's floor " ++ show floor_)
    _ -> Right (StackReport lowest highWater)

-- | A class as one role ran it: the segment's residency and whether the engine owns the class.
data Coverage = Coverage {coverageResidency :: Residency, coverageOwned :: Bool, coverageClass :: String}
  deriving (Eq, Ord, Show)

-- | A role's events by class and its fallback-site entries.
data RoleSummary = RoleSummary
  { summaryClasses :: !(M.Map Coverage Int)
  , summaryFallbackEntries :: !Int
  , summaryEvents :: !Int
  }
  deriving (Eq, Show)

-- | The latest stage entry's class on a core, and whether `0xbfa234` has since been entered as the
-- `kind_switch` service.
data Latest = Latest {latestClass :: !EventClass, latestServiceSwitch :: !Bool}

data Walk = Walk {walkLatest :: !(M.Map Int Latest), walkSummary :: !RoleSummary, walkFailures :: ![String]}

-- | Classifies each stage entry from its registers and its block read; for the owned role while the
-- engine is published, an entry to `0xbfa234` returning to 'fallbackReturn' fails when the latest
-- stage entry on its core has an owned class, and so does an entry to `0xbfad04` unless a
-- `kind_switch` service entry follows that stage entry on the core; any guard-trap entry fails.
checkRole :: MenuTree -> Role -> [Observation] -> Either String RoleSummary
checkRole tree role observations = do
  walk <- foldM step (Walk M.empty (RoleSummary M.empty 0 0) [])
    [(observationPhase observation, observationResidency observation, hit) | observation <- observations, hit <- observationHits observation]
  case walkFailures walk of
    [] -> Right (walkSummary walk)
    failures -> Left (unlines (reverse failures))
 where
  step walk (phase, residency, hit)
    | hitSite hit == StageEntry = do
        block <- case hitMemory hit of
          bytes : _ -> menuBlock bytes
          [] -> Left ("stage entry without its block read in " ++ T.unpack phase)
        let (kind, node, ident, extra) = hitRegisters hit
            class_ = deriveClass tree block (StageEvent kind node ident extra)
            summary = walkSummary walk
        pure walk
          { walkLatest = M.insert (hitCore hit) (Latest class_ False) (walkLatest walk)
          , walkSummary = summary
              { summaryClasses = M.insertWith (+) (Coverage residency (isOwned class_) (show class_)) 1 (summaryClasses summary)
              , summaryEvents = summaryEvents summary + 1 } }
    | hitSite hit == KindSwitch && hitLr hit == Just fallbackReturn = do
        let summary = walkSummary walk
            counted = walk {walkSummary = summary {summaryFallbackEntries = summaryFallbackEntries summary + 1}}
        pure $ case (role, residency, latestClass <$> M.lookup (hitCore hit) (walkLatest walk)) of
          (OwnedRole, EngineResident, Just class_) | isOwned class_ ->
            counted {walkFailures = ("fallback entry for owned class " ++ show class_ ++ " in " ++ T.unpack phase) : walkFailures walk}
          (OwnedRole, EngineResident, Nothing) ->
            counted {walkFailures = ("fallback entry with no stage entry on its core in " ++ T.unpack phase) : walkFailures walk}
          _ -> counted
    | hitSite hit == KindSwitch && hitLr hit == Nothing =
        Left ("kind-switch entry without lr in " ++ T.unpack phase)
    | hitSite hit == KindSwitch =
        pure walk {walkLatest = M.adjust (\latest -> latest {latestServiceSwitch = True}) (hitCore hit) (walkLatest walk)}
    | hitSite hit == Seek = pure $ case (role, residency, M.lookup (hitCore hit) (walkLatest walk)) of
        (OwnedRole, EngineResident, Just (Latest class_ False)) | isOwned class_ ->
          walk {walkFailures = ("seek entry for owned class " ++ show class_ ++ " in " ++ T.unpack phase) : walkFailures walk}
        _ -> walk
    | hitSite hit == GuardTrap =
        pure walk {walkFailures = ("adapter guard trap entered in " ++ T.unpack phase) : walkFailures walk}
    | otherwise = pure walk

-- | Both roles ran the same segments, read the same engine state at the end of each and posted the
-- same `0x1d` events in the same order.
comparePair :: [Observation] -> [Observation] -> Either String ()
comparePair stock owned = do
  unless (map observationPhase stock == map observationPhase owned)
    (Left "the roles' observations have different phases")
  differences <- fmap concat . forM (zip stock owned) $ \(left, right) ->
    forM engineReads (\label -> do
      let lookupRead observation = case [bytes | (name, bytes) <- observationReads observation, name == label] of
            [bytes] -> Right bytes
            _ -> Left ("read " ++ T.unpack label ++ " is missing or repeated in " ++ T.unpack (observationPhase observation))
      a <- lookupRead left
      b <- lookupRead right
      pure [T.unpack (observationPhase left) ++ " " ++ T.unpack label | a /= b])
  when (concat differences /= []) (Left ("engine reads differ: " ++ unwords (concat differences)))
  let posts observations = [(r1, r2, r3) | observation <- observations, hit <- observationHits observation
        , hitSite hit == EventPost, let (r0, r1, r2, r3) = hitRegisters hit, r0 == 0x1d]
  unless (posts stock == posts owned)
    (Left ("0x1d posts differ: stock " ++ show (posts stock) ++ ", owned " ++ show (posts owned)))

-- | One call of stock's menu notification `0xd5b350` as r0 to r3: kind, landed node, event id and
-- class.
data Notification = Notification !Word32 !Word32 !Word32 !Word32
  deriving (Eq, Show)

-- | One segment's entries of stock's IMAGE SIZE arm `0xd51b58` and of the owned apply seam, and its
-- `0xd5b350` notifications.
data ValueEntries = ValueEntries
  { stockArmEntries :: !Int
  , ownedApplyEntries :: !Int
  , notifications :: ![Notification]
  }
  deriving (Eq, Show)

noEntries :: ValueEntries
noEntries = ValueEntries 0 0 []

-- | Notifications collect newest first; 'inCallOrder' turns a finished segment's into call order.
countValueEntry :: ValueEntries -> Hit -> Either String ValueEntries
countValueEntry entries hit = Right $ case hitSite hit of
  ValueSite StockValueArm -> entries {stockArmEntries = stockArmEntries entries + 1}
  ValueSite ValueApply -> entries {ownedApplyEntries = ownedApplyEntries entries + 1}
  ValueSite ValueNotify ->
    let (kind, node, ident, class_) = hitRegisters hit
     in entries {notifications = Notification kind node ident class_ : notifications entries}
  _ -> entries

inCallOrder :: ValueEntries -> ValueEntries
inCallOrder entries = entries {notifications = reverse (notifications entries)}

-- | One segment as the value checks read it: its entries and the `live-settings` read at its end,
-- if the segment has one.
data ValueSegment = ValueSegment
  { valuePhase :: !T.Text
  , valueResidency :: !Residency
  , valueEntries :: !ValueEntries
  , valueLiveSettings :: !(Maybe B.ByteString)
  }
  deriving (Eq, Show)

-- | Whether the route must enter stock's IMAGE SIZE arm in a resident segment.
data ArmRequirement = ArmOptional | ArmRequired deriving (Eq, Show)

-- | What the value checks compared over a route: stock arm entries in resident segments, each
-- matched by one owned apply, and the phases they fell in; `0xd5b350` calls, the same in both
-- roles; and segments whose live-settings reads both roles made.
data ValueReport = ValueReport
  { residentArmEntries :: !Int
  , armPhases :: ![T.Text]
  , notificationCalls :: !Int
  , liveSettingsSegments :: !Int
  , valueSegments :: !Int
  }
  deriving (Eq, Show)

liveSettingsOf :: T.Text -> [(T.Text, B.ByteString)] -> Either String (Maybe B.ByteString)
liveSettingsOf phase segmentReads = case [bytes | (name, bytes) <- segmentReads, name == "live-settings"] of
  [] -> Right Nothing
  [bytes] -> Right (Just bytes)
  _ -> Left ("read live-settings is repeated in " ++ T.unpack phase)

-- | The owned seam replaces stock's IMAGE SIZE arm and hands every notification to stock. In each
-- segment the package is resident, the owned role applies exactly as often as the stock role enters
-- the arm and never enters the arm. In every segment both roles make the same `0xd5b350` calls in
-- the same order, and either both read the same live settings at its end or neither reads them.
-- With 'ArmRequired' the stock role enters the arm in at least one resident segment. A failure
-- lists every failing rule and segment, one per line.
checkValueSeam :: ArmRequirement -> [ValueSegment] -> [ValueSegment] -> Either String ValueReport
checkValueSeam requirement stock owned = do
  unless (map valuePhase stock == map valuePhase owned)
    (Left "the roles' observations have different phases")
  let segments = zip stock owned
      resident = [left | (left, right) <- segments, valueResidency right == EngineResident]
      armEntries = sum (map (stockArmEntries . valueEntries) resident)
      required =
        [ "IMAGE SIZE arm: the route requires stock's arm 0xd51b58 in a resident segment and stock entered it in none"
        | requirement == ArmRequired, armEntries == 0 ]
      failures = concatMap segmentFailures segments ++ required
  unless (null failures) (Left (unlines failures))
  pure ValueReport
    { residentArmEntries = armEntries
    , armPhases = [valuePhase left | left <- resident, stockArmEntries (valueEntries left) > 0]
    , notificationCalls = sum [length (notifications (valueEntries left)) | left <- stock]
    , liveSettingsSegments = length [() | (ValueSegment {valueLiveSettings = Just _}, _) <- segments]
    , valueSegments = length segments
    }
 where
  segmentFailures (left, right) =
    let phase = T.unpack (valuePhase left)
        stockEntries = valueEntries left
        ownedEntries = valueEntries right
        resident = valueResidency right == EngineResident
     in [ "IMAGE SIZE arm: the resident value seam executed the replaced arm in " ++ phase
        | resident, stockArmEntries ownedEntries /= 0 ]
        ++ [ "IMAGE SIZE arm: the owned role applied " ++ show (ownedApplyEntries ownedEntries)
               ++ " times where stock entered its arm " ++ show (stockArmEntries stockEntries) ++ " times in " ++ phase
           | resident, ownedApplyEntries ownedEntries /= stockArmEntries stockEntries ]
        ++ [ "0xd5b350 notifications: in " ++ phase ++ " stock made " ++ show (notifications stockEntries)
               ++ ", owned " ++ show (notifications ownedEntries)
           | notifications stockEntries /= notifications ownedEntries ]
        ++ liveFailures phase (valueLiveSettings left) (valueLiveSettings right)
  liveFailures phase (Just a) (Just b)
    | a /= b = ["live settings: the roles' reads differ at the end of " ++ phase ++ firstDifference a b]
  liveFailures phase (Just _) Nothing = ["live settings: only the stock role reads them in " ++ phase]
  liveFailures phase Nothing (Just _) = ["live settings: only the owned role reads them in " ++ phase]
  liveFailures _ _ _ = []
  firstDifference a b = case [offset | offset <- [0 .. min (B.length a) (B.length b) - 1], B.index a offset /= B.index b offset] of
    offset : _ -> ", first at byte " ++ show offset ++ ": stock " ++ show (B.index a offset) ++ ", owned " ++ show (B.index b offset)
    [] -> " in length"

checkValueSeamObservations :: ArmRequirement -> [Observation] -> [Observation] -> Either String ValueReport
checkValueSeamObservations requirement stock owned = do
  left <- mapM segmentOf stock
  right <- mapM segmentOf owned
  checkValueSeam requirement left right
 where
  segmentOf observation = do
    entries <- foldM countValueEntry noEntries (observationHits observation)
    live <- liveSettingsOf (observationPhase observation) (observationReads observation)
    pure (ValueSegment (observationPhase observation) (observationResidency observation) (inCallOrder entries) live)

checkValueSeamReader :: ArmRequirement -> [ObservationReader] -> [ObservationReader] -> IO (Either String ValueReport)
checkValueSeamReader requirement stock owned = do
  left <- mapM segmentOf stock
  right <- mapM segmentOf owned
  pure $ do
    stockSegments <- sequence left
    ownedSegments <- sequence right
    checkValueSeam requirement stockSegments ownedSegments
 where
  segmentOf reader = do
    folded <- readerFoldHits reader countValueEntry noEntries
    pure $ do
      entries <- folded
      live <- liveSettingsOf (readerPhase reader) (readerReads reader)
      pure (ValueSegment (readerPhase reader) (readerResidency reader) (inCallOrder entries) live)

-- | Both roles declare stock's IMAGE SIZE arm at `0xd51b58` and its notification at `0xd5b350`
-- once, and the owned role declares its apply seam once.
requireValueDeclarations :: Role -> J.Value -> Either String ()
requireValueDeclarations role = J.parseEither $ J.withObject "e2e result" $ \fields -> do
  declarations <- fields .: "declared_trace_sites" >>= mapM (J.withObject "trace declaration" $ \declaration ->
    (,) <$> (declaration .: "label" :: J.Parser T.Text) <*> (declaration .: "address" :: J.Parser T.Text))
  unless ([address | ("stock-value-size-arm", address) <- declarations] == ["00d51b58"])
    (fail "the trajectory does not declare stock's IMAGE SIZE arm 0x00d51b58 once")
  unless ([address | ("value-notify", address) <- declarations] == ["00d5b350"])
    (fail "the trajectory does not declare stock's notification 0x00d5b350 once")
  when (role == OwnedRole) $
    unless (length [() | ("owned-value-apply", _) <- declarations] == 1)
      (fail "the owned trajectory does not declare its value apply seam once")

checkRoleReader :: MenuTree -> Role -> [ObservationReader] -> IO (Either String RoleSummary)
checkRoleReader tree role readers = go (Walk M.empty (RoleSummary M.empty 0 0) []) readers
 where
  go walk [] = pure (case walkFailures walk of [] -> Right (walkSummary walk); failures -> Left (unlines (reverse failures)))
  go walk (reader : rest) = do
    result <- readerFoldHits reader (\state hit -> step (readerPhase reader) (readerResidency reader) state hit) walk
    case result of
      Left problem -> pure (Left problem)
      Right next -> go next rest
  step phase residency walk hit
    | hitSite hit == StageEntry = do
        block <- case hitMemory hit of
          bytes : _ -> menuBlock bytes
          [] -> Left ("stage entry without its block read in " ++ T.unpack phase)
        let (kind, node, ident, extra) = hitRegisters hit
            class_ = deriveClass tree block (StageEvent kind node ident extra)
            summary = walkSummary walk
        pure walk
          { walkLatest = M.insert (hitCore hit) (Latest class_ False) (walkLatest walk)
          , walkSummary = summary
              { summaryClasses = M.insertWith (+) (Coverage residency (isOwned class_) (show class_)) 1 (summaryClasses summary)
              , summaryEvents = summaryEvents summary + 1 } }
    | hitSite hit == KindSwitch && hitLr hit == Just fallbackReturn = do
        let summary = walkSummary walk
            counted = walk {walkSummary = summary {summaryFallbackEntries = summaryFallbackEntries summary + 1}}
        pure $ case (role, residency, latestClass <$> M.lookup (hitCore hit) (walkLatest walk)) of
          (OwnedRole, EngineResident, Just class_) | isOwned class_ ->
            counted {walkFailures = ("fallback entry for owned class " ++ show class_ ++ " in " ++ T.unpack phase) : walkFailures walk}
          (OwnedRole, EngineResident, Nothing) ->
            counted {walkFailures = ("fallback entry with no stage entry on its core in " ++ T.unpack phase) : walkFailures walk}
          _ -> counted
    | hitSite hit == KindSwitch && hitLr hit == Nothing =
        Left ("kind-switch entry without lr in " ++ T.unpack phase)
    | hitSite hit == KindSwitch =
        pure walk {walkLatest = M.adjust (\latest -> latest {latestServiceSwitch = True}) (hitCore hit) (walkLatest walk)}
    | hitSite hit == Seek = pure $ case (role, residency, M.lookup (hitCore hit) (walkLatest walk)) of
        (OwnedRole, EngineResident, Just (Latest class_ False)) | isOwned class_ ->
          walk {walkFailures = ("seek entry for owned class " ++ show class_ ++ " in " ++ T.unpack phase) : walkFailures walk}
        _ -> walk
    | hitSite hit == GuardTrap =
        pure walk {walkFailures = ("adapter guard trap entered in " ++ T.unpack phase) : walkFailures walk}
    | otherwise = pure walk

checkStackReader :: Word32 -> [ObservationReader] -> IO (Either String StackReport)
checkStackReader required readers = do
  floorsResult <- goFloors [] readers
  case floorsResult of
    Left problem -> pure (Left problem)
    Right floors -> case [floor_ | floor_ <- floors, floor_ < toInteger taskStackStart] of
      floor_ : _ -> pure (Left ("a stage entry leaves the bound " ++ show (toInteger taskStackStart - floor_) ++ " bytes below task 1's stack"))
      [] -> do
        let stacks = [bytes | reader <- readers, (name, bytes) <- readerReads reader, name == taskStackRead]
            valid = and [B.length bytes == fromIntegral taskStackBytes | bytes <- stacks]
            phases = [readerPhase reader | reader <- readers]
            missingPhase = case drop (length stacks) phases of phase : _ -> phase; [] -> "unknown"
        if length stacks /= length readers || not valid
          then pure (Left ("task 1's stack read is missing or short in " ++ T.unpack missingPhase))
          else do
            let changed = case stacks of
                  first : rest -> [offset | offset <- [0, 4 .. fromIntegral taskStackBytes - 4], any (\later -> B.take 4 (B.drop offset later) /= B.take 4 (B.drop offset first)) rest]
                  [] -> []
                highWater = case changed of
                  offset : _ -> Just (taskStackStart + fromIntegral offset)
                  [] -> Nothing
                lowest = if null floors then Nothing else Just (minimum floors)
            pure $ case (highWater, lowest) of
              (Just mark, Just floor_) | toInteger mark < floor_ ->
                Left ("task 1's stack changed at " ++ show mark ++ ", below the bound's floor " ++ show floor_)
              _ -> Right (StackReport lowest highWater)
 where
  goFloors floors [] = pure (Right floors)
  goFloors floors (reader : rest) = do
    result <- readerFoldHits reader step floors
    case result of
      Left problem -> pure (Left problem)
      Right next -> goFloors next rest
  step floors hit
    | hitSite hit == StageEntry = case hitSp hit of
        Nothing -> Left "stage entry without sp"
        Just sp -> Right (toInteger sp - toInteger required : floors)
    | otherwise = Right floors

comparePairReader :: [ObservationReader] -> [ObservationReader] -> IO (Either String ())
comparePairReader stock owned = case readsCheck of
  Left problem -> pure (Left problem)
  Right () -> do
    leftPosts <- postsOf stock
    rightPosts <- postsOf owned
    pure $ case (leftPosts, rightPosts) of
      (Left problem, _) -> Left problem
      (_, Left problem) -> Left problem
      (Right a, Right b) -> if a == b then Right () else Left ("0x1d posts differ: stock " ++ show a ++ ", owned " ++ show b)
 where
  readsCheck = do
    unless (map readerPhase stock == map readerPhase owned)
      (Left "the roles' observations have different phases")
    differences <- fmap concat . forM (zip stock owned) $ \(left, right) ->
      forM engineReads (\label -> do
        let lookupRead reader = case [bytes | (name, bytes) <- readerReads reader, name == label] of
              [bytes] -> Right bytes
              _ -> Left ("read " ++ T.unpack label ++ " is missing or repeated in " ++ T.unpack (readerPhase reader))
        a <- lookupRead left
        b <- lookupRead right
        pure [T.unpack (readerPhase left) ++ " " ++ T.unpack label | a /= b])
    when (concat differences /= []) (Left ("engine reads differ: " ++ unwords (concat differences)))
  postsOf readers = go [] readers
   where
    go posts [] = pure (Right (reverse posts))
    go posts (reader : rest) = do
      result <- readerFoldHits reader step posts
      case result of
        Left problem -> pure (Left problem)
        Right next -> go next rest
    step posts hit = case hitSite hit of
      EventPost | let (r0, r1, r2, r3) = hitRegisters hit, r0 == 0x1d -> Right ((r1, r2, r3) : posts)
      _ -> Right posts

newtype CalendarInput = CalendarInput B.ByteString deriving (Eq)
newtype RtcInputByte = RtcInputByte Word32 deriving (Eq)
data ClockRead = CurrentCalendar | SampledCalendar
data SampleCoverage = SampleMissing | SampleSeen deriving (Eq)
data ClockInputState = ClockInputState
  { clockCalendar :: Maybe CalendarInput
  , clockRtcBytes :: M.Map RtcByteIndex RtcInputByte
  , clockSampleCoverage :: SampleCoverage
  } deriving (Eq)

calendarInput :: B.ByteString -> Either String CalendarInput
calendarInput bytes
  | B.length bytes /= 6 = Left "calendar input is not six bytes"
  | B.index bytes 0 > 50 || B.index bytes 1 < 1 || B.index bytes 1 > 12
      || B.index bytes 2 < 1 || B.index bytes 2 > 31 || B.index bytes 3 >= 24
      || B.index bytes 4 >= 60 || B.index bytes 5 >= 61 = Left "calendar input is outside the retained validator's bounds"
  | otherwise = Right (CalendarInput bytes)

clockRead :: ClockRead -> Observation -> Either String CalendarInput
clockRead field observation = case [bytes | (name,bytes) <- observationReads observation, name == label] of
  [bytes] -> calendarInput bytes
  _ -> Left ("missing or repeated clock input in " ++ T.unpack (observationPhase observation))
 where label = case field of CurrentCalendar -> "rtc-calendar-current"; SampledCalendar -> "rtc-calendar-sample"

observeCalendar :: ClockInputState -> CalendarInput -> Either String ClockInputState
observeCalendar state value = case clockCalendar state of
  Nothing -> Right state {clockCalendar = Just value}
  Just previous | previous == value -> Right state
                | otherwise -> Left "calendar input changed within the controlled-clock fixture"

data ClockEvidence = ClockEvidence
  { clockSessionHits :: [Hit]
  , clockSegments :: [Observation]
  } deriving (Eq, Show)

checkConstantClockInputs :: ClockEvidence -> ClockEvidence -> Either String ()
checkConstantClockInputs stock owned = do
  left <- collect stock
  right <- collect owned
  unless (left == right) (Left "the roles' observed MCU/calendar inputs differ")
 where
  collect evidence = do
    state <- foldM sampleClockHit (ClockInputState Nothing M.empty SampleMissing) (clockSessionHits evidence)
    final <- foldM segment state (clockSegments evidence)
    unless (clockSampleCoverage final == SampleSeen && M.size (clockRtcBytes final) == 4)
      (Left "controlled clock lacks successful calendar and all four MCU byte observations")
    pure final
  segment state observation = do
    current <- clockRead CurrentCalendar observation >>= observeCalendar state
    sampled <- clockRead SampledCalendar observation >>= observeCalendar current
    foldM sampleClockHit sampled (observationHits observation)

data ClockEvidenceReader = ClockEvidenceReader
  { clockReaderSessionHits :: [Hit]
  , clockReaders :: [ObservationReader]
  }

checkConstantClockInputsReader :: ClockEvidenceReader -> ClockEvidenceReader -> IO (Either String ())
checkConstantClockInputsReader stock owned = do
  left <- collect stock
  right <- collect owned
  pure $ do
    l <- left
    r <- right
    unless (l == r) (Left "the roles' observed MCU/calendar inputs differ")
 where
  collect :: ClockEvidenceReader -> IO (Either String ClockInputState)
  collect evidence = do
    case foldM sampleClockHit (ClockInputState Nothing M.empty SampleMissing) (clockReaderSessionHits evidence) of
      Left problem -> pure (Left problem)
      Right state -> go state (clockReaders evidence)
  go :: ClockInputState -> [ObservationReader] -> IO (Either String ClockInputState)
  go state [] = pure (Right state)
  go state (reader:rest) = do
    result <- segmentReader state reader
    case result of
      Left problem -> pure (Left problem)
      Right next -> go next rest
  segmentReader state reader = do
    case clockReadReader CurrentCalendar reader >>= observeCalendar state of
      Left problem -> pure (Left problem)
      Right current -> case clockReadReader SampledCalendar reader >>= observeCalendar current of
        Left problem -> pure (Left problem)
        Right sampled -> readerFoldHits reader sampleClockHit sampled
  clockReadReader field reader = case [bytes | (name,bytes) <- readerReads reader, name == label] of
    [bytes] -> calendarInput bytes
    _ -> Left ("missing or repeated clock input in " ++ T.unpack (readerPhase reader))
   where label = case field of CurrentCalendar -> "rtc-calendar-current"; SampledCalendar -> "rtc-calendar-sample"

sampleClockHit :: ClockInputState -> Hit -> Either String ClockInputState
sampleClockHit state hit = case hitSite hit of
    CalendarSample -> do
      let (status,_,_,_) = hitRegisters hit
      unless (status == 0) (Left "MCU calendar sampling failed")
      value <- case hitMemory hit of
        [bytes] -> calendarInput bytes
        _ -> Left "MCU calendar sample lacks its six-byte result"
      updated <- observeCalendar state value
      pure updated {clockSampleCoverage = SampleSeen}
    RtcCounterByte index -> do
      let (value,_,_,_) = hitRegisters hit
      unless (value <= 255) (Left "MCU RTC input is not one byte")
      let byte = RtcInputByte value
      case M.lookup index (clockRtcBytes state) of
        Nothing -> Right state {clockRtcBytes = M.insert index byte (clockRtcBytes state)}
        Just previous | previous == byte -> Right state
                      | otherwise -> Left "MCU RTC byte changed within the controlled-clock fixture"
    _ -> Right state
