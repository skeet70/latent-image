{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The IMAGE SIZE changed-selection gate: the stock and owned roles of one route must make the same
-- Menu submissions and handle the same pipeline completions. A route's receipts record, per
-- segment, the observed landings, the pipeline and menu reads, and the trace hits of the `0x4010`
-- submission (`0x0103a254`, `0x01029188`), its completion (`0x01029430` with pending state 2, the
-- saved callback), the record task's `0x13a5` send, the menu token release and stock's menu
-- notification `0x00d5b350`. Callback words in the owned role are compared through a declared
-- stock-to-owned correspondence (reports/owned-menu-engine-design.md#image-size-changed-selection).
module Replacement.ModalRuntime.ValueChange
  ( CallbackWord (..)
  , CallbackPair (..)
  , CallbackRef (..)
  , Correspondence
  , correspondence
  , parseCorrespondence
  , Landing (..)
  , Event (..)
  , SegmentRecord (..)
  , Terminal (..)
  , SettingsWrite (..)
  , RoleRecord (..)
  , SegmentVerdict (..)
  , Verdict (..)
  , gateReads
  , segmentEvents
  , compareRoles
  , readRoleRecord
  , verdictValue
  ) where

import Control.Monad (forM, unless)
import Data.Aeson ((.:), (.:?), (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word32)
import Numeric (readHex, showHex)
import Replacement.ModalRuntime.MenuEnginePair
  ( Hit (..), ObservationReader (..), Residency (..), Site (..), ValueSite (..), readObservationReader )
import System.Directory (doesFileExist)
import System.FilePath ((</>))

-- | A code address passed as a completion callback.
newtype CallbackWord = CallbackWord Word32 deriving (Eq, Ord, Show)

-- | One declared stock callback and the owned role's callback for the same completion.
data CallbackPair = CallbackPair
  { pairStock :: CallbackWord
  , pairOwned :: CallbackWord
  , pairInvocation :: T.Text
  }
  deriving (Eq, Show)

-- | Owned callback words keyed to their stock words; a word is in at most one pair per side.
data Correspondence = Correspondence
  { ownedToStock :: M.Map CallbackWord CallbackWord
  , replacedStock :: [CallbackWord]
  }
  deriving (Eq, Show)

correspondence :: [CallbackPair] -> Either String Correspondence
correspondence pairs = do
  let owned = map pairOwned pairs
      stock = map pairStock pairs
  unless (distinct owned) (Left "an owned callback word is declared twice")
  unless (distinct stock) (Left "a stock callback word is declared twice")
  Right (Correspondence (M.fromList [(pairOwned pair, pairStock pair) | pair <- pairs])
    [pairStock pair | pair <- pairs, pairStock pair /= pairOwned pair])
 where
  distinct values = let sorted = sort values in and (zipWith (/=) sorted (drop 1 sorted))

parseCorrespondence :: J.Value -> Either String Correspondence
parseCorrespondence value = do
  pairs <- J.parseEither (J.withObject "callback correspondence" $ \fields -> do
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "callback correspondence schema must be 1")
    fields .: "callbacks" >>= mapM (J.withObject "callback" $ \entry ->
      CallbackPair <$> (word =<< entry .: "stock") <*> (word =<< entry .: "owned") <*> entry .: "invocation")) value
  correspondence pairs
 where
  word text = case T.unpack text of
    '0' : 'x' : digits | length digits == 8, [(parsed, "")] <- readHex digits -> pure (CallbackWord parsed)
    _ -> fail ("callback words are 0x and eight hex digits: " ++ T.unpack text)

-- | A callback word in stock terms. In the owned role a declared owned word becomes its stock word;
-- a stock word whose declared owned counterpart differs is misplaced there.
data CallbackRef = StockCallback Word32 | MisplacedStockWord Word32 deriving (Eq, Show)

stockView :: Correspondence -> Word32 -> CallbackRef
stockView table word = case M.lookup (CallbackWord word) (ownedToStock table) of
  Just (CallbackWord stock) -> StockCallback stock
  Nothing
    | CallbackWord word `elem` replacedStock table -> MisplacedStockWord word
    | otherwise -> StockCallback word

-- | The menu state after one observed key press.
data Landing = Landing { landingKey :: T.Text, landingState :: (Int, Int, Int) } deriving (Eq, Show)

-- | A boundary event of the Menu's submission or completion handling.
data Event
  = Submitted { submittedCallback :: CallbackRef }
  | Received
      { receivedPayload :: B.ByteString, receivedCallback :: CallbackRef, receivedPriorCallback :: CallbackRef
      , receivedPending :: Word32, receivedState :: Word32 }
  | Completed { completedCallback :: CallbackRef }
  | CallbackInvoked { invokedStatus :: Word32, invokedFrom :: Maybe Word32 }
  | RecordReply { replyBytes :: B.ByteString }
  | TokenReleased { tokenBitmap :: B.ByteString }
  | Notified { notifiedKind :: Word32, notifiedNode :: Word32, notifiedId :: Word32, notifiedClass :: Word32 }
  | ValueApplied
  deriving (Eq, Show)

data SegmentRecord = SegmentRecord
  { segmentPhase :: T.Text
  , segmentLandings :: [Landing]
  , segmentReads :: [(T.Text, B.ByteString)]
  , segmentHits :: [Hit]
  }
  deriving (Eq, Show)

-- | How a role ended: its result's check verdicts, the unload refusal message, or a segment failure.
data Terminal
  = Finished { finishedChecks :: [(T.Text, T.Text)] }
  | UnloadRefused { refusedMessage :: T.Text }
  | SegmentFailed { failedPhase :: T.Text, failedMessage :: T.Text }
  | NoTerminal
  deriving (Eq, Show)

-- | One admitted live-settings write in recording order.
data SettingsWrite = SettingsWrite
  { writeAddress :: Word32, writeSize :: Int, writeBefore :: [Int], writeValue :: Integer, writeWriter :: T.Text }
  deriving (Eq, Show)

data RoleRecord = RoleRecord
  { roleSegments :: [SegmentRecord]
  , roleSettingsWrites :: Maybe [SettingsWrite]
  , roleTerminal :: Terminal
  }
  deriving (Eq, Show)

-- | The reads both roles must equal at every segment that records them. The pipeline's
-- completion-mask read is recorded but not compared: its bits follow the pipeline schedule.
gateReads :: [T.Text]
gateReads =
  [ "live-settings", "menu-block", "menu-last-move", "pipeline-pending", "pipeline-callback"
  , "pipeline-state", "menu-token-bitmaps" ]

-- | Callback words inside a gate read: the pending callback cell `0x016fe9f8`.
readCallbackOffsets :: T.Text -> [Int]
readCallbackOffsets label = [0 | label == "pipeline-callback"]

word32At :: B.ByteString -> Int -> Maybe Word32
word32At bytes offset
  | offset < 0 || offset + 4 > B.length bytes = Nothing
  | otherwise = Just (foldr (\index acc -> acc `shiftL` 8 .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3])

-- | A read with its callback words taken out and given in stock terms.
readView :: Correspondence -> T.Text -> B.ByteString -> (B.ByteString, [Maybe CallbackRef])
readView table label bytes =
  ( foldr (\offset rest -> B.take offset rest <> B.replicate 4 0 <> B.drop (offset + 4) rest) bytes offsets
  , [stockView table <$> word32At bytes offset | offset <- offsets] )
 where
  offsets = readCallbackOffsets label

-- | The receive site's first read starts at `0x016fea28`: the message header from `+4`, the
-- seven-word `0x4010` payload from `+0x10`, and its seventh word, the callback, at `+0x28`
-- (`0x01029188`).
payloadCallbackOffset :: Int
payloadCallbackOffset = 0x28

segmentEvents :: Correspondence -> [Hit] -> [Event]
segmentEvents table = mapMaybe event
 where
  callback = stockView table
  first hit = let (value, _, _, _) = hitRegisters hit in value
  second hit = let (_, value, _, _) = hitRegisters hit in value
  event hit = case hitSite hit of
    OtherSite "pipeline-4010-submit" -> Just (Submitted (callback (second hit)))
    OtherSite "pipeline-4010-receive" -> case hitMemory hit of
      [message, cell, pending, state] -> do
        callbackWord <- word32At message payloadCallbackOffset
        prior <- word32At cell 0
        pendingWord <- word32At pending 0
        stateWord <- word32At state 0
        let payload = B.take (payloadCallbackOffset - 4) (B.drop 4 message)
        Just (Received payload (callback callbackWord) (callback prior) pendingWord stateWord)
      _ -> Nothing
    OtherSite "pipeline-4010-complete" -> case hitMemory hit of
      [pending, cell] | word32At pending 0 == Just 2 -> Completed . callback <$> word32At cell 0
      _ -> Nothing
    OtherSite "pipeline-4010-callback" -> Just (CallbackInvoked (first hit) (hitLr hit))
    OtherSite "record-13a5-send" -> Just (RecordReply (B.concat (hitMemory hit)))
    OtherSite "menu-token-release" -> Just (TokenReleased (B.concat (hitMemory hit)))
    ValueSite ValueNotify -> let (a, b, c, d) = hitRegisters hit in Just (Notified a b c d)
    ValueSite ValueApply -> Just ValueApplied
    ValueSite StockValueArm -> Just ValueApplied
    _ -> Nothing

data SegmentVerdict = SegmentVerdict
  { verdictPhase :: T.Text
  , verdictLandings :: Bool
  , verdictReads :: [(T.Text, Bool)]
  , verdictStockEvents :: [Event]
  , verdictOwnedEvents :: [Event]
  }
  deriving (Eq, Show)

segmentPasses :: SegmentVerdict -> Bool
segmentPasses verdict =
  verdictLandings verdict && all snd (verdictReads verdict) && verdictStockEvents verdict == verdictOwnedEvents verdict

data Verdict = Verdict
  { verdictSegments :: [SegmentVerdict]
  , verdictProblems :: [String]
  , verdictSettingsWrites :: Maybe Bool
  , verdictTerminal :: Bool
  , verdictCallbacks :: [Word32]
  }
  deriving (Eq, Show)

-- | Pass only when every segment, the settings writes and the terminal agree and no structural
-- problem was found.
verdictPasses :: Verdict -> Bool
verdictPasses verdict =
  null (verdictProblems verdict) && all segmentPasses (verdictSegments verdict)
    && verdictSettingsWrites verdict /= Just False && verdictTerminal verdict

compareRoles :: Correspondence -> RoleRecord -> RoleRecord -> (Bool, Verdict)
compareRoles table stock owned = (verdictPasses verdict, verdict)
 where
  identity = Correspondence M.empty []
  stockSegments = roleSegments stock
  ownedSegments = roleSegments owned
  problems =
    [ "the roles recorded " ++ show (length stockSegments) ++ " and " ++ show (length ownedSegments) ++ " segments"
    | length stockSegments /= length ownedSegments ]
    ++ [ "segment " ++ show index ++ " is " ++ T.unpack (segmentPhase a) ++ " in stock and " ++ T.unpack (segmentPhase b) ++ " in owned"
       | (index, a, b) <- zip3 [0 :: Int ..] stockSegments ownedSegments, segmentPhase a /= segmentPhase b ]
  segments = zipWith segmentVerdict stockSegments ownedSegments
  segmentVerdict a b = SegmentVerdict
    { verdictPhase = segmentPhase a
    , verdictLandings = segmentLandings a == segmentLandings b
    , verdictReads =
        [ (label, (readView identity label <$> left) == (readView table label <$> right))
        | label <- gateReads
        , let left = lookup label (segmentReads a)
              right = lookup label (segmentReads b)
        , left /= Nothing || right /= Nothing ]
    , verdictStockEvents = segmentEvents identity (segmentHits a)
    , verdictOwnedEvents = segmentEvents table (segmentHits b)
    }
  verdict = Verdict
    { verdictSegments = segments
    , verdictProblems = problems
    , verdictSettingsWrites = case (roleSettingsWrites stock, roleSettingsWrites owned) of
        (Just left, Just right) -> Just (left == right)
        (Nothing, Nothing) -> Nothing
        _ -> Just False
    , verdictTerminal = roleTerminal stock == roleTerminal owned && roleTerminal stock /= NoTerminal
    , verdictCallbacks = sort (foldr collect [] (concatMap (segmentEvents identity . segmentHits) ownedSegments))
    }
  collect event acc = case event of
    Submitted word -> raw word : acc
    Received _ word prior _ _ -> raw word : raw prior : acc
    Completed word -> raw word : acc
    _ -> acc
  raw ref = case ref of
    StockCallback word -> word
    MisplacedStockWord word -> word

parseSteps :: J.Value -> J.Parser [Landing]
parseSteps = J.withObject "e2e observation" $ \fields -> do
  steps <- fields .:? "steps" J..!= []
  forM steps $ J.withObject "observed step" $ \step -> do
    key <- step .: "key"
    actual <- step .: "actual"
    case actual of
      [menu, setting, cursor] -> pure (Landing key (menu, setting, cursor))
      _ -> fail "an observed step's actual state is three integers"

readSegment :: FilePath -> IO (Either String SegmentRecord)
readSegment path = do
  bytes <- B.readFile path
  case J.eitherDecodeStrict' bytes >>= J.parseEither parseSteps of
    Left problem -> pure (Left (path ++ ": " ++ problem))
    Right landings -> do
      reader <- readObservationReader EngineResident path
      case reader of
        Left problem -> pure (Left (path ++ ": " ++ problem))
        Right observation -> do
          hits <- readerFoldHits observation (\acc hit -> Right (hit : acc)) []
          pure (SegmentRecord (readerPhase observation) landings (readerReads observation) . reverse <$> hits)

readTerminal :: FilePath -> IO (Either String Terminal)
readTerminal receipt = do
  let result = receipt </> "e2e-result.json"
      lifecycle = receipt </> "e2e-lifecycle-failure.json"
      failure = receipt </> "e2e-failure.json"
  hasResult <- doesFileExist result
  hasLifecycle <- doesFileExist lifecycle
  hasFailure <- doesFileExist failure
  if hasResult
    then decodeWith result (J.withObject "e2e result" $ \fields -> do
      checks <- fields .: "checks"
      pure (Finished (M.toList (checks :: M.Map T.Text T.Text))))
    else if hasLifecycle
      then decodeWith lifecycle (J.withObject "lifecycle failure" $ \fields ->
        UnloadRefused . messageOf <$> fields .: "failure")
      else if hasFailure
        then decodeWith failure (J.withObject "e2e failure" $ \fields ->
          SegmentFailed <$> (fields .:? "phase" J..!= "") <*> (messageOf <$> fields .:? "message" J..!= J.Null))
        else pure (Right NoTerminal)
 where
  decodeWith path parser = do
    bytes <- B.readFile path
    pure (either (Left . ((path ++ ": ") ++)) Right (J.eitherDecodeStrict' bytes >>= J.parseEither parser))
  messageOf value = case value of
    J.String text -> text
    J.Object _ -> TE.decodeUtf8 (B.toStrict (J.encode value))
    J.Null -> ""
    other -> TE.decodeUtf8 (B.toStrict (J.encode other))

readSettingsWrites :: FilePath -> IO (Either String (Maybe [SettingsWrite]))
readSettingsWrites receipt = do
  let path = receipt </> "e2e-modal-settings-writes.json"
  present <- doesFileExist path
  if not present
    then pure (Right Nothing)
    else do
      bytes <- B.readFile path
      pure (either (Left . ((path ++ ": ") ++)) (Right . Just) (J.eitherDecodeStrict' bytes >>= J.parseEither parser))
 where
  parser = J.withObject "settings writes" $ \fields -> fields .: "writes" >>= mapM (J.withObject "settings write" $ \row ->
    SettingsWrite <$> row .: "address" <*> row .: "size" <*> row .: "before" <*> row .: "value" <*> row .: "writer")

-- | Reads one role's receipt directory: every `e2e-observation-N.json` from 0 until the first
-- missing index, the settings writes when present and the terminal outcome.
readRoleRecord :: FilePath -> IO (Either String RoleRecord)
readRoleRecord receipt = do
  segments <- collectSegments 0
  writes <- readSettingsWrites receipt
  terminal <- readTerminal receipt
  pure (RoleRecord <$> segments <*> writes <*> terminal)
 where
  collectSegments :: Int -> IO (Either String [SegmentRecord])
  collectSegments index = do
    let path = receipt </> ("e2e-observation-" ++ show index ++ ".json")
    present <- doesFileExist path
    if not present
      then pure (Right [])
      else do
        segment <- readSegment path
        rest <- collectSegments (index + 1)
        pure ((:) <$> segment <*> rest)

hexWord :: Word32 -> String
hexWord value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

eventValue :: Event -> J.Value
eventValue event = case event of
  Submitted word -> J.object ["event" .= ("submitted" :: T.Text), "callback" .= ref word]
  Received payload word prior pending state -> J.object
    [ "event" .= ("received" :: T.Text), "header_and_payload" .= hex payload, "callback" .= ref word
    , "prior_callback" .= ref prior, "pending_before" .= pending, "pipeline_state" .= state ]
  Completed word -> J.object ["event" .= ("completed" :: T.Text), "callback" .= ref word]
  CallbackInvoked status from -> J.object ["event" .= ("callback-invoked" :: T.Text), "status" .= hexWord status, "return" .= fmap hexWord from]
  RecordReply bytes -> J.object ["event" .= ("record-0x13a5" :: T.Text), "bytes" .= hex bytes]
  TokenReleased bytes -> J.object ["event" .= ("token-released" :: T.Text), "bitmap" .= hex bytes]
  Notified kind node identity class_ -> J.object
    ["event" .= ("notified" :: T.Text), "kind" .= hexWord kind, "node" .= hexWord node, "id" .= hexWord identity, "class" .= hexWord class_]
  ValueApplied -> J.object ["event" .= ("value-applied" :: T.Text)]
 where
  hex bytes = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) (B.unpack bytes)
  ref callback = case callback of
    StockCallback word -> hexWord word
    MisplacedStockWord word -> "misplaced stock word " ++ hexWord word

verdictValue :: (Bool, Verdict) -> J.Value
verdictValue (passed, verdict) = J.object
  [ "schema" .= (1 :: Int)
  , "pass" .= passed
  , "problems" .= verdictProblems verdict
  , "settings_writes_equal" .= verdictSettingsWrites verdict
  , "terminal_equal" .= verdictTerminal verdict
  , "owned_callback_words_in_stock_terms" .= map hexWord (verdictCallbacks verdict)
  , "segments" .= map segmentValue (verdictSegments verdict)
  ]
 where
  segmentValue segment = J.object
    [ "phase" .= verdictPhase segment
    , "pass" .= segmentPasses segment
    , "landings_equal" .= verdictLandings segment
    , "reads" .= [J.object ["label" .= label, "equal" .= equal] | (label, equal) <- verdictReads segment]
    , "events_equal" .= (verdictStockEvents segment == verdictOwnedEvents segment)
    , "stock_events" .= map eventValue (verdictStockEvents segment)
    , "owned_events" .= map eventValue (verdictOwnedEvents segment)
    ]
