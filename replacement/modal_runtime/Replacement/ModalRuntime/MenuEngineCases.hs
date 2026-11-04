{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Owned menu engine oracle cases from `--menu-observe` receipts (emulation/Emulation/MenuObserve.hs).
--
-- Each `stage-entry` hit (`0xd4dd2c`) opens a window that runs to the first hit after the wrapper's
-- snapshot store (`frame-snapshot` with lr `0xbf95ec`), so it holds the store's writes. The hits
-- after the store and before the next stage entry give the consumer's verdict.
module Replacement.ModalRuntime.MenuEngineCases
  ( Config (..)
  , Site (..)
  , Receipt (..)
  , Hit (..)
  , WriteRecord (..)
  , LastMoveWord (..)
  , Call (..)
  , ServiceCall (..)
  , CaseResult (..)
  , Case (..)
  , ExpectedEvent (..)
  , parseEvents
  , datSha256
  , extract
  , rustCases
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson ((.:))
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find, sortOn)
import Data.Word (Word32, Word8)
import Numeric (readHex, showHex)

data Site = Site {siteLabel :: String, siteAddress :: Word32} deriving (Eq, Show)

data Config = Config {configSites :: [Site], configRanges :: [(Word32, Word32)], configLimit :: Int}
  deriving (Eq, Show)

data Hit = Hit
  { hitSite :: String
  , hitCore :: Int
  , hitInstructions :: Integer
  , hitLr :: Word32
  , hitRegisters :: (Word32, Word32, Word32, Word32)
  }
  deriving (Eq, Show)

data WriteRecord = WriteRecord
  { writeAddress :: Word32
  , writeBefore :: [Word8]
  , writeValue :: Word32
  , writeSize :: Int
  , writeInstructions :: Integer
  , writeCore :: Int
  }
  deriving (Eq, Show)

data Receipt = Receipt
  { receiptConfigSha :: String
  , receiptDatSha :: String
  , receiptGlobalsAddress :: Word32
  , receiptBefore :: B.ByteString
  , receiptAfter :: B.ByteString
  , receiptHits :: [Hit]
  , receiptWatchEnabled :: Bool
  , receiptWatchRanges :: [(Word32, Word32)]
  , receiptWatchViolation :: Bool
  , receiptWrites :: [WriteRecord]
  }
  deriving (Eq, Show)

hexWord :: String -> J.Parser Word32
hexWord text = case text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits -> pure value
  _ -> fail ("not a hex word: " ++ text)

hexBytes :: String -> J.Parser B.ByteString
hexBytes text = B.pack <$> go text
 where
  go (high : low : rest) | [(value, "")] <- readHex [high, low] = (fromIntegral (value :: Int) :) <$> go rest
  go [] = pure []
  go _ = fail "odd or non-hex byte string"

instance J.FromJSON Config where
  parseJSON = J.withObject "menu-observe config" $ \fields -> do
    sites <- mapM (J.withObject "trace site" (\site -> flip Site <$> (hexWord =<< site .: "address") <*> site .: "label")) =<< fields .: "trace_sites"
    ranges <- mapM (J.withObject "write range" (\range -> (,) <$> (hexWord =<< range .: "start") <*> (hexWord =<< range .: "end"))) =<< fields .: "write_ranges"
    Config sites ranges <$> fields .: "limit"

instance J.FromJSON Receipt where
  parseJSON = J.withObject "menu-observe receipt" $ \fields -> do
    inputs <- fields .: "inputs"
    globals <- fields .: "menu_globals"
    watch <- fields .: "watch"
    violation <- watch .: "violation"
    Receipt
      <$> inputs .: "config_sha256"
      <*> inputs .: "dat_sha256"
      <*> (hexWord =<< globals .: "address")
      <*> (hexBytes =<< globals .: "before")
      <*> (hexBytes =<< globals .: "after")
      <*> (mapM hit =<< fields .: "trace_hits")
      <*> watch .: "enabled"
      <*> (map (\(start, end) -> (fromInteger start, fromInteger end)) <$> (watch .: "ranges" :: J.Parser [(Integer, Integer)]))
      <*> pure (violation /= J.Null)
      <*> (mapM record =<< watch .: "records")
   where
    hit = J.withObject "trace hit" $ \value -> do
      registers <- value .: "registers"
      let register name = hexWord =<< registers .: name
      Hit
        <$> value .: "site"
        <*> value .: "core"
        <*> value .: "instructions"
        <*> (hexWord =<< value .: "lr")
        <*> ((,,,) <$> register "r0" <*> register "r1" <*> register "r2" <*> register "r3")
    record = J.withObject "watch record" $ \value ->
      WriteRecord
        <$> (fromInteger <$> value .: "address")
        <*> value .: "before"
        <*> (fromInteger <$> value .: "value")
        <*> value .: "size"
        <*> value .: "instructions"
        <*> value .: "core"

data LastMoveWord = LastMoveClass | LastMoveToken deriving (Eq, Show)

-- | One engine call to stock, as the hit at its entry shows it.
data Call
  = CallNotify (Word32, Word32, Word32, Word32)
  | CallAvailability Word32 Word32 Word32
  | CallInitialRow Word32 Word32 Word32 (Maybe Word32)
  | CallStore Word32
  | -- | `0x0061c91c(property, value, flags)`.
    CallPropertyWrite Word32 Word32 Word32
  deriving (Eq, Show)

-- | A call and the block as it stood when stock made it.
data ServiceCall = ServiceCall {serviceCall :: Call, serviceBlock :: B.ByteString} deriving (Eq, Show)

-- | The consumer's verdict on a window: the special post `0xd42a64` follows an accepted event and
-- the rollback restore `0x0061d3a8` a rejected one.
data CaseResult = Accepted | Rejected deriving (Eq, Show)

-- | One stage entry's window.
data Case = Case
  { caseEvent :: (Word32, Word32, Word32, Word32)
  , caseBefore :: B.ByteString
  , caseLastMoveBefore :: (Maybe Word32, Maybe Word32)
  , caseCalls :: [ServiceCall]
  , caseAfter :: B.ByteString
  , caseLastMoveWrites :: [(LastMoveWord, Word32)]
  , caseSnapshotWrites :: [(Word32, Word32)]
  , caseResult :: CaseResult
  }
  deriving (Eq, Show)

-- | A stage event a receipt must show, in order: kind, node and id, and whether its window is a
-- case (`KIND,NODE,ID`) or a fallback class recorded in the same press (`-KIND,NODE,ID`).
data ExpectedEvent = ExpectedEvent {expectedCase :: Bool, expectedWords :: (Word32, Word32, Word32)}
  deriving (Eq, Show)

-- | `EVENT[;EVENT...]`, each `KIND,NODE,ID` or `-KIND,NODE,ID` in hex with or without `0x`.
parseEvents :: String -> Either String [ExpectedEvent]
parseEvents text = mapM event (splitOn ';' text)
 where
  event item = do
    let (isCase, body) = case item of
          '-' : rest -> (False, rest)
          _ -> (True, item)
    case mapM number (splitOn ',' body) of
      Just [kind, node, ident] -> Right (ExpectedEvent isCase (kind, node, ident))
      _ -> Left ("not KIND,NODE,ID: " ++ item)
  number digits = case readHex (maybe digits id (stripHexPrefix digits)) of
    [(value, "")] -> Just value
    _ -> Nothing
  stripHexPrefix ('0' : 'x' : rest) = Just rest
  stripHexPrefix _ = Nothing
  splitOn separator value = case break (== separator) value of
    (first, _ : rest) -> first : splitOn separator rest
    (first, []) -> [first]

-- | FWUP0002.DAT of X-M1 1.50.
datSha256 :: String
datSha256 = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

blockAddress, blockLen, lastMoveAddress :: Word32
blockAddress = 0x009571d0
blockLen = 0x430
lastMoveAddress = 0x01467580

snapshotAreas :: [(Word32, Word32)]
snapshotAreas = [(0x00944414, 0x00944440), (0x0093eb74, 0x0093eb74 + 0x86 * 0x2c), (0x009412d4, 0x009412d4 + 0x86 * 0x2c)]

requiredRanges :: [(Word32, Word32)]
requiredRanges = (blockAddress, blockAddress + blockLen) : (lastMoveAddress, lastMoveAddress + 8) : snapshotAreas

requiredSites :: [Site]
requiredSites =
  [ Site "stage-entry" 0x00d4dd2c
  , Site "engine-kind" 0x00bfa234
  , Site "frame-snapshot" 0x0061ceac
  , Site "row-availability" 0x00d52684
  , Site "availability-return" 0x00bfa914
  , Site "initial-row" 0x00d588b8
  , Site "initial-row-return" 0x00bfaa3c
  , Site "engine-notify" 0x00d5b350
  , Site "special-post" 0x00d42a64
  ]

-- | The verdict of a rejected window; the increment-1 configs, whose cases are all accepted, lack it.
rollbackRestore :: Site
rollbackRestore = Site "rollback-restore" 0x0061d3a8

-- | `FUN_00ca4674` and `FUN_00ca4e98`, the stage consumers whose special post or rollback restore
-- follows the engine.
consumerBodies :: [(Word32, Word32)]
consumerBodies = [(0x00ca4674, 0x00ca4834), (0x00ca4e98, 0x00ca5088)]

-- | `0xbf95c8` up to `0xbfaf84`: the wrapper, kind switch and the functions under it.
engineBody :: (Word32, Word32)
engineBody = (0x00bf95c8, 0x00bfaf84)

wrapperStoreReturn, availabilityCall, initialRowCall :: Word32
wrapperStoreReturn = 0x00bf95ec
availabilityCall = 0x00bfa914
initialRowCall = 0x00bfaa3c

noSelection :: Word32
noSelection = 0x7ffffffe

inside :: (Word32, Word32) -> Word32 -> Bool
inside (start, end) at = at >= start && at < end

covered :: [(Word32, Word32)] -> (Word32, Word32) -> Bool
covered ranges (start, end) = foldl' extend start (sortOn fst ranges) >= end
 where
  extend reached (from, to) = if from <= reached && to > reached then to else reached

le :: Int -> Word32 -> [Word8]
le size value = [fromIntegral (value `shiftR` (8 * index)) | index <- [0 .. size - 1]]

word :: B.ByteString -> Int -> Word32
word bytes offset = foldr (\index acc -> (acc `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

-- | Arguments: the SHA-256 of the config bytes, the config, the receipt's expected stage events and
-- the receipt. One result per stage entry, `Nothing` for a window marked as a fallback class.
extract :: String -> Config -> [ExpectedEvent] -> Receipt -> Either String [Maybe Case]
extract configSha config expected receipt = do
  unless (receiptConfigSha receipt == configSha) (Left "receipt names another config")
  unless (receiptDatSha receipt == datSha256) (Left "receipt names another firmware")
  forM_ requiredSites $ \site ->
    unless (site `elem` configSites config) (Left ("config lacks trace site " ++ siteLabel site ++ " at " ++ hex (siteAddress site)))
  forM_ requiredRanges $ \range -> do
    unless (covered (configRanges config) range) (Left ("config write ranges miss " ++ show range))
    unless (covered (receiptWatchRanges receipt) range) (Left ("receipt watch ranges miss " ++ show range))
  unless (receiptWatchEnabled receipt) (Left "watch disabled")
  when (receiptWatchViolation receipt) (Left "watch recorded a violation")
  unless (length (receiptHits receipt) < configLimit config) (Left "trace hits reached the config limit")
  unless (receiptGlobalsAddress receipt == blockAddress) (Left "menu globals start elsewhere")
  unless (B.length (receiptBefore receipt) == fromIntegral blockLen && B.length (receiptAfter receipt) == fromIntegral blockLen)
    (Left "menu globals are not 0x430 bytes")
  let indexed = zip [0 :: Int ..] (receiptHits receipt)
      entries = [(index, hit) | (index, hit) <- indexed, hitSite hit == "stage-entry"]
      shown = [let (kind, node, ident, _) = hitRegisters hit in (kind, node, ident) | (_, hit) <- entries]
  unless (shown == map expectedWords expected)
    (Left ("stage entries " ++ show (map hexTriple shown) ++ ", expected " ++ show (map (hexTriple . expectedWords) expected)))
  (core, firstStart) <- case entries of
    (_, first) : _ -> Right (hitCore first, hitInstructions first)
    [] -> Left "no stage entry"
  let blockWrites = [record | record <- receiptWrites receipt, inBlock record]
  unless (all ((== core) . writeCore) (filter (\record -> inBlock record || inLastMove record) (receiptWrites receipt)))
    (Left "block or last-move writes on another core than the stage entries")
  replayedAll <- foldl' (\acc record -> acc >>= replay record) (Right (receiptBefore receipt)) blockWrites
  when (replayedAll /= receiptAfter receipt) (Left "replaying the receipt's block writes does not give the recorded after block")
  when (any (\record -> writeInstructions record < firstStart && (inBlock record || inLastMove record)) (receiptWrites receipt))
    (Left "block or last-move write before the first stage entry")
  let nextStarts = map (Just . fst) (drop 1 entries) ++ [Nothing]
  forM (zip3 entries nextStarts expected) $ \((entryIndex, entryHit), nextEntry, wanted) ->
    if expectedCase wanted then Just <$> window indexed blockWrites core entryIndex entryHit nextEntry else Right Nothing
 where
  hexTriple (a, b, c) = (hex a, hex b, hex c)
  inBlock record = inside (blockAddress, blockAddress + blockLen) (writeAddress record)
  inLastMove record = inside (lastMoveAddress, lastMoveAddress + 8) (writeAddress record)
  window indexed blockWrites core entryIndex entryHit nextEntry = do
    body <- maybe (Left ("stage entry from " ++ hex (hitLr entryHit) ++ ", outside both stage consumers")) Right
      (find (`inside` hitLr entryHit) consumerBodies)
    let beforeNext index = maybe True (index <) nextEntry
    storeIndex <- maybe (Left "no wrapper snapshot store after a stage entry") (Right . fst) $
      find (\(index, hit) -> index > entryIndex && beforeNext index && hitSite hit == "frame-snapshot" && hitLr hit == wrapperStoreReturn) indexed
    after <- maybe (Left "no hit after the wrapper store bounds its writes") (Right . snd) $ find ((> storeIndex) . fst) indexed
    let start = hitInstructions entryHit
        end = hitInstructions after
        windowHits = [hit | (index, hit) <- indexed, index >= entryIndex, index <= storeIndex]
        verdictHits = [hit | (index, hit) <- indexed, index > storeIndex, beforeNext index, inside body (hitLr hit)]
        site label = filter ((== label) . hitSite) windowHits
        r0 hit = let (value, _, _, _) = hitRegisters hit in value
        r1 hit = let (_, value, _, _) = hitRegisters hit in value
        r2 hit = let (_, _, value, _) = hitRegisters hit in value
    unless (all ((== core) . hitCore) windowHits) (Left "engine window hits on more than one core")
    result <- case (any ((== "special-post") . hitSite) verdictHits, any ((== "rollback-restore") . hitSite) verdictHits) of
      (True, _) -> Right Accepted
      (False, True) -> Right Rejected
      (False, False)
        | rollbackRestore `notElem` configSites config -> Left ("no special post follows the engine window, and the config lacks trace site rollback-restore at " ++ hex (siteAddress rollbackRestore))
        | otherwise -> Left "neither a special post nor a rollback restore follows the engine window"
    let queries = filter ((== availabilityCall) . hitLr) (site "row-availability")
        answers = site "availability-return"
    unless (length queries == length answers) (Left "availability queries and returns differ in number")
    unless (interleaved queries answers) (Left "availability returns out of order")
    availability <- mapM (\(query, answer) -> if r0 answer <= 3 then Right (query, CallAvailability (r0 query) (r1 query) (r0 answer)) else Left ("row state " ++ hex (r0 answer))) (zip queries answers)
    let initials = filter ((== initialRowCall) . hitLr) (site "initial-row")
        returns = site "initial-row-return"
    unless (length initials == length returns) (Left "initial-row calls and returns differ in number")
    unless (interleaved initials returns) (Left "initial-row returns out of order")
    let initialRows = [(call, CallInitialRow (r0 call) (r1 call) (r2 call) (if r0 answer == noSelection then Nothing else Just (r0 answer))) | (call, answer) <- zip initials returns]
        notifies = [(hit, CallNotify (hitRegisters hit)) | hit <- site "engine-notify", inside engineBody (hitLr hit)]
        stores = [(hit, CallStore (r0 hit)) | hit <- site "frame-snapshot", inside engineBody (hitLr hit)]
        properties = [(hit, CallPropertyWrite (r0 hit) (r1 hit) (r2 hit)) | hit <- site "property-write", inside engineBody (hitLr hit)]
        calls = sortOn (hitInstructions . fst) (availability ++ initialRows ++ notifies ++ stores ++ properties)
        engineWrites = [record | record <- receiptWrites receipt, writeInstructions record >= start, writeInstructions record < end]
        replayTo limit = foldl' (\acc record -> acc >>= replay record) (Right (receiptBefore receipt)) (filter ((< limit) . writeInstructions) blockWrites)
    before <- replayTo start
    replayed <- replayTo end
    serviceCalls <- mapM (\(hit, call) -> ServiceCall call <$> replayTo (hitInstructions hit)) calls
    let lastMoveWrites = filter inLastMove engineWrites
    forM_ lastMoveWrites $ \record ->
      unless (writeSize record == 4 && (writeAddress record - lastMoveAddress) `mod` 4 == 0) (Left "unaligned last-move write")
    let wordOf record = if writeAddress record == lastMoveAddress then LastMoveClass else LastMoveToken
        beforeOf which = case [record | record <- lastMoveWrites, wordOf record == which] of
          first : _ -> Just (word (B.pack (writeBefore first)) 0)
          [] -> Nothing
        snapshots = [record | record <- engineWrites, any (\area -> inside area (writeAddress record)) snapshotAreas]
    forM_ snapshots $ \record -> unless (writeSize record == 4) (Left "snapshot write is not a word")
    pure Case
      { caseEvent = hitRegisters entryHit
      , caseBefore = before
      , caseLastMoveBefore = (beforeOf LastMoveClass, beforeOf LastMoveToken)
      , caseCalls = serviceCalls
      , caseAfter = replayed
      , caseLastMoveWrites = [(wordOf record, writeValue record) | record <- lastMoveWrites]
      , caseSnapshotWrites = [(writeAddress record, writeValue record) | record <- snapshots]
      , caseResult = result
      }
  interleaved calls returns = and (zipWith (\call answer -> hitInstructions call < hitInstructions answer) calls returns)
    && and (zipWith (\answer next -> hitInstructions answer < hitInstructions next) returns (drop 1 calls))
  replay record bytes = do
    let offset = fromIntegral (writeAddress record - blockAddress)
        size = writeSize record
    when (offset + size > B.length bytes) (Left "block write runs past the block")
    unless (B.unpack (B.take size (B.drop offset bytes)) == writeBefore record)
      (Left ("block write at " ++ hex (writeAddress record) ++ " found other bytes before it"))
    let value = le size (writeValue record .&. (if size == 4 then 0xffffffff else (1 `shiftL` (8 * size)) - 1))
    pure (B.take offset bytes <> B.pack value <> B.drop (offset + size) bytes)

rowState :: Word32 -> String
rowState value = case value of
  0 -> "RowState::Offered"
  1 -> "RowState::Variant"
  2 -> "RowState::Disabled"
  _ -> "RowState::Hidden"

bytesText :: B.ByteString -> String
bytesText bytes = "[" ++ concatMap (\byte -> hex (fromIntegral byte) ++ ", ") (B.unpack bytes) ++ "]"

-- | One Rust slice expression of `owner::Case` values: (name, receipt sha256, config sha256, case).
rustCases :: [(String, String, String, Case)] -> String
rustCases cases =
  unlines $
    [ "// Generated by //replacement/modal_runtime:menu-engine-cases from owner --menu-observe receipts; not for redistribution."
    , "&[" ]
      ++ concatMap caseText cases
      ++ ["]"]
 where
  optional render = maybe "None" (\value -> "Some(" ++ render value ++ ")")
  caseText (name, receiptSha, configSha, c) =
    let (kind, node, identifier, extra) = caseEvent c
        (classBefore, tokenBefore) = caseLastMoveBefore c
     in [ "    Case {"
        , "        name: " ++ show name ++ ","
        , "        receipt_sha256: " ++ show receiptSha ++ ","
        , "        config_sha256: " ++ show configSha ++ ","
        , "        event: StageEvent { kind: " ++ hex kind ++ ", node: " ++ hex node ++ ", id: " ++ hex identifier ++ ", extra: " ++ hex extra ++ " },"
        , "        before: " ++ bytesText (caseBefore c) ++ ","
        , "        last_move_before: LastMoveBefore { class: " ++ optional (\v -> "MenuClass::new(" ++ hex v ++ ")") classBefore
            ++ ", token: " ++ optional (\v -> "RowToken::new(" ++ hex v ++ ")") tokenBefore ++ " },"
        , "        calls: &[" ++ concatMap callText (caseCalls c) ++ "],"
        , "        after: " ++ bytesText (caseAfter c) ++ ","
        , "        last_move_writes: &["
            ++ concatMap (\(which, value) -> case which of
                 LastMoveClass -> "LastMoveWrite::Class(MenuClass::new(" ++ hex value ++ ")), "
                 LastMoveToken -> "LastMoveWrite::Token(RowToken::new(" ++ hex value ++ ")), ") (caseLastMoveWrites c)
            ++ "],"
        , "        snapshot_writes: &[" ++ concatMap (\(at, value) -> "(" ++ hex at ++ ", " ++ hex value ++ "), ") (caseSnapshotWrites c) ++ "],"
        , "        result: EngineResult::" ++ show (caseResult c) ++ ","
        , "    },"
        ]
  callText (ServiceCall call block) = "\n            (" ++ callExpression call ++ ", " ++ bytesText block ++ "),"
  callExpression call = case call of
    CallNotify (a, b, d, e) -> "Expected::Notify { kind: " ++ hex a ++ ", node: " ++ hex b ++ ", id: " ++ hex d ++ ", class: " ++ hex e ++ " }"
    CallAvailability cls token state -> "Expected::Availability { class: MenuClass::new(" ++ hex cls ++ "), token: RowToken::new(" ++ hex token ++ "), state: " ++ rowState state ++ " }"
    CallInitialRow k n cls answer -> "Expected::InitialRow { kind: " ++ hex k ++ ", node: " ++ hex n ++ ", class: MenuClass::new(" ++ hex cls ++ "), answer: " ++ optional (\v -> "RowToken::new(" ++ hex v ++ ")") answer ++ " }"
    CallStore slot -> "Expected::Store { slot: " ++ hex slot ++ " }"
    CallPropertyWrite property value flags -> "Expected::PropertyWrite { property: " ++ hex property ++ ", value: " ++ hex value ++ ", flags: " ++ hex flags ++ " }"
