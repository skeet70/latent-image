{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Owned menu engine oracle cases from `--menu-observe` receipts (emulation/Emulation/MenuObserve.hs).
--
-- The engine window runs from the one `stage-entry` hit (`0xd4dd2c`) to the first hit after the
-- wrapper's snapshot store (`frame-snapshot` with lr `0xbf95ec`), so it holds the store's writes.
module Replacement.ModalRuntime.MenuEngineCases
  ( Config (..)
  , Site (..)
  , Receipt (..)
  , Hit (..)
  , WriteRecord (..)
  , LastMoveWord (..)
  , Case (..)
  , extract
  , rustCases
  ) where

import Control.Monad (forM_, unless, when)
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

data Case = Case
  { caseEvent :: (Word32, Word32, Word32, Word32)
  , caseBefore :: B.ByteString
  , caseLastMoveBefore :: (Maybe Word32, Maybe Word32)
  , caseAvailability :: [(Word32, Word32, Word32)]
  , caseInitialRows :: [(Word32, Word32, Word32, Maybe Word32)]
  , caseAfter :: B.ByteString
  , caseLastMoveWrites :: [(LastMoveWord, Word32)]
  , caseSnapshotWrites :: [(Word32, Word32)]
  , caseNotifies :: [(Word32, Word32, Word32, Word32)]
  , caseAccepted :: Bool
  }
  deriving (Eq, Show)

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

-- | `FUN_00ca4674`, the stage consumer whose result the special post follows.
consumerBody :: (Word32, Word32)
consumerBody = (0x00ca4674, 0x00ca4834)

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

-- | Arguments: the SHA-256 of the config bytes, the config and the receipt.
extract :: String -> Config -> Receipt -> Either String Case
extract configSha config receipt = do
  unless (receiptConfigSha receipt == configSha) (Left "receipt names another config")
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
  let hits = receiptHits receipt
      indexed = zip [0 :: Int ..] hits
  entry <- case [(index, hit) | (index, hit) <- indexed, hitSite hit == "stage-entry"] of
    [one] -> Right one
    entries -> Left ("expected one stage-entry hit, found " ++ show (length entries))
  let (entryIndex, entryHit) = entry
      core = hitCore entryHit
  unless (inside consumerBody (hitLr entryHit)) (Left ("stage entry from " ++ hex (hitLr entryHit) ++ ", outside FUN_00ca4674"))
  storeIndex <- maybe (Left "no wrapper snapshot store after the stage entry") (Right . fst) $
    find (\(index, hit) -> index > entryIndex && hitSite hit == "frame-snapshot" && hitLr hit == wrapperStoreReturn) indexed
  after <- maybe (Left "no hit after the wrapper store bounds its writes") (Right . snd) $ find ((> storeIndex) . fst) indexed
  let start = hitInstructions entryHit
      end = hitInstructions after
      window = [hit | (index, hit) <- indexed, index >= entryIndex, index <= storeIndex]
      site label = filter ((== label) . hitSite) window
      r0 hit = let (value, _, _, _) = hitRegisters hit in value
      r1 hit = let (_, value, _, _) = hitRegisters hit in value
      r2 hit = let (_, _, value, _) = hitRegisters hit in value
  unless (all ((== core) . hitCore) window) (Left "engine window hits on more than one core")
  let queries = filter ((== availabilityCall) . hitLr) (site "row-availability")
      answers = site "availability-return"
  unless (length queries == length answers) (Left "availability queries and returns differ in number")
  unless (interleaved queries answers) (Left "availability returns out of order")
  availability <- mapM (\(query, answer) -> if r0 answer <= 3 then Right (r0 query, r1 query, r0 answer) else Left ("row state " ++ hex (r0 answer))) (zip queries answers)
  let initials = filter ((== initialRowCall) . hitLr) (site "initial-row")
      returns = site "initial-row-return"
  unless (length initials == length returns) (Left "initial-row calls and returns differ in number")
  unless (interleaved initials returns) (Left "initial-row returns out of order")
  let initialRows = [(r0 call, r1 call, r2 call, if r0 answer == noSelection then Nothing else Just (r0 answer)) | (call, answer) <- zip initials returns]
      notifies = [hitRegisters hit | hit <- site "engine-notify", inside engineBody (hitLr hit)]
      accepted = any (\hit -> hitSite hit == "special-post" && inside consumerBody (hitLr hit)) [hit | (index, hit) <- indexed, index > storeIndex]
      writes = filter ((== core) . writeCore) (receiptWrites receipt)
      inBlock record = inside (blockAddress, blockAddress + blockLen) (writeAddress record)
      inLastMove record = inside (lastMoveAddress, lastMoveAddress + 8) (writeAddress record)
      engineWrites = [record | record <- writes, writeInstructions record >= start, writeInstructions record < end]
  when (any (\record -> writeInstructions record < start && (inBlock record || inLastMove record)) (receiptWrites receipt))
    (Left "block or last-move write before the stage entry")
  replayed <- foldl' (\acc record -> acc >>= replay record) (Right (receiptBefore receipt)) (filter inBlock engineWrites)
  when (accepted && replayed /= receiptAfter receipt) (Left "replayed engine writes differ from the recorded after block")
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
    , caseBefore = receiptBefore receipt
    , caseLastMoveBefore = (beforeOf LastMoveClass, beforeOf LastMoveToken)
    , caseAvailability = availability
    , caseInitialRows = initialRows
    , caseAfter = replayed
    , caseLastMoveWrites = [(wordOf record, writeValue record) | record <- lastMoveWrites]
    , caseSnapshotWrites = [(writeAddress record, writeValue record) | record <- snapshots]
    , caseNotifies = notifies
    , caseAccepted = accepted
    }
 where
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
        , "        availability: &["
            ++ concatMap (\(cls, token, state) -> "AvailabilityAnswer { class: MenuClass::new(" ++ hex cls ++ "), token: RowToken::new(" ++ hex token ++ "), state: " ++ rowState state ++ " }, ") (caseAvailability c)
            ++ "],"
        , "        initial_rows: &["
            ++ concatMap (\(k, n, cls, answer) -> "InitialRow { kind: " ++ hex k ++ ", node: " ++ hex n ++ ", class: MenuClass::new(" ++ hex cls ++ "), answer: " ++ optional (\v -> "RowToken::new(" ++ hex v ++ ")") answer ++ " }, ") (caseInitialRows c)
            ++ "],"
        , "        after: " ++ bytesText (caseAfter c) ++ ","
        , "        last_move_writes: &["
            ++ concatMap (\(which, value) -> case which of
                 LastMoveClass -> "LastMoveWrite::Class(MenuClass::new(" ++ hex value ++ ")), "
                 LastMoveToken -> "LastMoveWrite::Token(RowToken::new(" ++ hex value ++ ")), ") (caseLastMoveWrites c)
            ++ "],"
        , "        snapshot_writes: &[" ++ concatMap (\(at, value) -> "(" ++ hex at ++ ", " ++ hex value ++ "), ") (caseSnapshotWrites c) ++ "],"
        , "        notifies: &["
            ++ concatMap (\(a, b, d, e) -> "NotifyCall { kind: " ++ hex a ++ ", node: " ++ hex b ++ ", id: " ++ hex d ++ ", class: " ++ hex e ++ " }, ") (caseNotifies c)
            ++ "],"
        , "        result: " ++ (if caseAccepted c then "EngineResult::Accepted" else "EngineResult::Rejected") ++ ","
        , "    },"
        ]
