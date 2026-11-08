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
  , checkRoleReader
  , checkStackReader
  , comparePairReader
  , RtcByteIndex (..)
  , ClockEvidence (..)
  , checkConstantClockInputs
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
import qualified Data.Text.Encoding as TE
import Data.Word (Word32, Word64)
import Emulation.ObservationStream (foldTraceHitLines)
import Numeric (readHex)
import Replacement.ModalRuntime.MenuEngineClass
import Replacement.ModalRuntime.RenderObjectProvenance
import Replacement.ModalRuntime.RenderStackEvidence (InstructionCounter (..))
import System.FilePath (takeDirectory)

data Role = StockRole | OwnedRole deriving (Eq, Show)

-- | A trace site of the pair trajectories (`emulation/menu-engine-pair-trajectories`); every other
-- label is 'OtherSite'.
data RtcByteIndex = RtcByte0 | RtcByte1 | RtcByte2 | RtcByte3 deriving (Eq, Ord, Show)

data Site = StageEntry | KindSwitch | Seek | EventPost | GuardTrap | CalendarSample | RtcCounterByte RtcByteIndex | OtherSite T.Text
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
    state <- foldM sample (ClockInputState Nothing M.empty SampleMissing) (clockSessionHits evidence)
    final <- foldM segment state (clockSegments evidence)
    unless (clockSampleCoverage final == SampleSeen && M.size (clockRtcBytes final) == 4)
      (Left "controlled clock lacks successful calendar and all four MCU byte observations")
    pure final
  segment state observation = do
    current <- clockRead CurrentCalendar observation >>= observeCalendar state
    sampled <- clockRead SampledCalendar observation >>= observeCalendar current
    foldM sample sampled (observationHits observation)
  sample state hit = case hitSite hit of
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
