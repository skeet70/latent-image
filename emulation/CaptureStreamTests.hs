{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (IOException, bracket, bracket_, try)
import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), eitherDecodeStrict', object, toJSON, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Data.Text as T
import Data.Word (Word64)
import Emulation.CaptureStream
import System.Directory (createDirectory, getTemporaryDirectory, removeDirectoryRecursive, removeFile)
import System.FilePath ((</>))
import System.IO (IOMode (ReadMode), hClose, openBinaryTempFile, withBinaryFile)
import System.IO.Error (isPermissionError)
import System.Posix.Files (setFileMode)

check :: String -> Bool -> IO ()
check name holds = unless holds (fail name)
right :: Either String a -> IO a
right = either fail pure

bounds :: Value
bounds = object
  [ "sites" .= (256 :: Int), "label_bytes" .= (64 :: Int), "details_per_site" .= (64 :: Int)
  , "details_total" .= (2048 :: Int), "ram_bytes_per_event" .= (512 :: Int), "page_details" .= (64 :: Int)
  , "retained_memory_bytes" .= (2097152 :: Int), "charged_storage_bytes" .= (2000000 :: Int) ]

replace :: K.Key -> Value -> Value -> Value
replace name value (Object o) = Object (KM.insert name value o)
replace _ _ other = other
field :: K.Key -> Value -> Maybe Value
field name (Object o) = KM.lookup name o
field _ _ = Nothing

site :: Word64 -> Word64 -> Word64 -> Word64 -> Word64 -> Value
site address total pending dropped through = object
  [ "address" .= address, "site" .= ("site-" <> T.pack (show address)), "total" .= total
  , "captured" .= (total - dropped), "pending" .= pending, "dropped" .= dropped
  , "first_instructions" .= (if total == 0 then Null else Number 100)
  , "last_instructions" .= (if total == 0 then Null else toJSON through)
  , "first_dropped_instructions" .= (if dropped == 0 then Null else Number 100)
  , "last_dropped_instructions" .= (if dropped == 0 then Null else toJSON through) ]

event :: Word64 -> Word64 -> Word64 -> Value
event sequenceNumber at core = object
  [ "sequence" .= sequenceNumber, "site" .= ("site-4096" :: T.Text), "address" .= (4096 :: Int)
  , "instructions" .= at, "core" .= core, "registers" .= ([11,22,33,44] :: [Int])
  , "sp" .= (128 :: Int), "lr" .= (256 :: Int), "cpsr" .= (16 :: Int)
  , "ram" .= ([] :: [Value]), "ram_incomplete" .= False ]

page :: Word64 -> Word64 -> Word64 -> Word64 -> Bool -> [Value] -> Value
page from captured pending through complete hits = object
  [ "from" .= from, "next" .= (from + fromIntegral (length hits)), "acknowledged" .= from
  , "pending" .= pending, "captured" .= captured, "total" .= captured, "dropped" .= (0 :: Int)
  , "incomplete" .= False, "through_instructions" .= through, "boundary_complete" .= complete
  , "first_instructions" .= (if captured == 0 then Null else Number 100)
  , "last_instructions" .= (if captured == 0 then Null else toJSON through)
  , "sites" .= [site 4096 captured pending 0 through], "hits" .= hits, "bounds" .= bounds ]

progress :: T.Text -> T.Text -> Word64 -> Word64 -> Value
progress state operation at endpoint = object
  [ "state" .= state, "operation" .= operation, "instructions" .= at, "intended_endpoint" .= endpoint ]

sequenceProgress :: T.Text -> Word64 -> Word64 -> Int -> T.Text -> [Value] -> Value
sequenceProgress state at endpoint phase position phases = case progress state "shutter-sequence" at endpoint of
  Object o -> Object (o <> KM.fromList (["phases" .= phases]
    ++ ["current_phase" .= phase | state == "drain-required"]
    ++ ["position" .= position | state == "drain-required"]))
  other -> other

withTemp :: (FilePath -> IO a) -> IO a
withTemp action = do
  directory <- getTemporaryDirectory
  bracket (do (path, handle) <- openBinaryTempFile directory "capture-stream-tests"; hClose handle; pure path)
    removeFile action

parserTests :: IO ()
parserTests = do
  right (validateBegin (object ["protocol" .= (2 :: Int), "active" .= True, "bounds" .= bounds]))
  forM_ [0,1,3 :: Int] $ \protocol -> check "reject incompatible protocol"
    (isLeft (validateBegin (object ["protocol" .= protocol, "active" .= True, "bounds" .= bounds])))
  let first = page 0 64 64 8000100 True [event n 8000100 (n `mod` 2) | n <- [0..63]]
  parsed <- right (parsePage initialCursor first)
  check "ordered multicore page" (map (field "sequence" . eventValue) (pageHits parsed) == map (Just . Number . fromIntegral) [0..63 :: Int])
  check "register observations preserved" (map eventRegisters (take 1 (pageHits parsed)) == [[11,22,33,44]])
  let cursor64 = pageNext parsed
  check "wrong cursor refused" (isLeft (parsePage cursor64 first))
  forM_ [replace "next" (Number 63) first, replace "acknowledged" (Number 1) first
        ,replace "hits" (toArray [event 1 8000100 0]) first
        ,replace "from" (Number (-1)) first, replace "next" (Number 18446744073709551616) first
        ,replace "pending" (Number 2049) first] $ \invalid ->
    check "reject inconsistent page" (isLeft (parsePage initialCursor invalid))
  right (validateAck cursor64 0 (object ["acknowledged" .= (64 :: Int), "pending" .= (0 :: Int)]))
  check "wrong acknowledgement refused" (isLeft (validateAck cursor64 0 (object ["acknowledged" .= (63 :: Int), "pending" .= (0 :: Int)])))
  check "unreleased pending acknowledgement refused" (isLeft (validateAck cursor64 0 (object ["acknowledged" .= (64 :: Int), "pending" .= (1 :: Int)])))
  high <- right (parsePage initialCursor (page 0 64 64 100 True [event n 100 0 | n <- [0..63]]))
  cursor4096 <- advanceCursor (pageNext high) 64 4096
  late <- right (parsePage cursor4096 (page 4096 4097 1 100 True [event 4096 100 1]))
  check "cursor survives arena reuse" (cursorValue (pageNext late) == 4097)
  let manySites = [site address 64 64 0 100 | address <- [4096..4127]]
      full = replace "sites" (toArray manySites) (page 0 2048 2048 100 True [event n 100 0 | n <- [0..63]])
  _ <- right (parsePage initialCursor full)
  check "65-event page rejected" (isLeft (parsePage initialCursor (page 0 65 65 100 True [event n 100 0 | n <- [0..64]])))
  let siteBoundary = replace "sites" (toArray [site address 0 0 0 100 | address <- [4096..4351]]) (page 0 0 0 100 True [])
  _ <- right (parsePage initialCursor siteBoundary)
  check "257 sites rejected" (isLeft (parsePage initialCursor
    (replace "sites" (toArray [site address 0 0 0 100 | address <- [4096..4352]]) siteBoundary)))
  let named label = replace "sites" (toArray [replace "site" (String label) (site 4096 1 1 0 100)])
        (page 0 1 1 100 True [replace "site" (String label) (event 0 100 0)])
  _ <- right (parsePage initialCursor (named (T.replicate 32 "é")))
  check "label bound counts UTF-8 bytes" (isLeft (parsePage initialCursor (named (T.replicate 33 "é"))))
  check "per-site pending limit enforced" (isLeft (parsePage initialCursor
    (page 0 65 65 100 True [event n 100 0 | n <- [0..63]])))
  let spanValue status bytes = object ["address" .= (if status == "complete" then 4096 else 0x70000000 :: Word64), "requested" .= (512 :: Int), "status" .= (status :: T.Text), "bytes" .= bytes]
      ramEvent = replace "ram" (toArray [spanValue "complete" (T.replicate 1024 "a")]) (event 0 100 0)
  _ <- right (parsePage initialCursor (page 0 1 1 100 True [ramEvent]))
  let gapEvent = replace "ram_incomplete" (Bool True) $ replace "ram" (toArray [spanValue "outside-ram" ("" :: T.Text)]) (event 0 100 0)
  gap <- right (parsePage initialCursor (replace "incomplete" (Bool True) (page 0 1 1 100 True [gapEvent])))
  check "unknown RAM address remains unchanged" (map (field "ram" . eventValue) (pageHits gap) == [field "ram" gapEvent])
  let spanBoundary = replace "ram_incomplete" (Bool True)
        $ replace "ram" (toArray (replicate 24 (spanValue "outside-ram" ("" :: T.Text)))) (event 0 100 0)
  _ <- right (parsePage initialCursor (replace "incomplete" (Bool True) (page 0 1 1 100 True [spanBoundary])))
  forM_ [replace "ram" (toArray [spanValue "invented" ("" :: T.Text)]) (event 0 100 0)
        ,replace "ram" (toArray [spanValue "complete" (T.replicate 1026 "a")]) (event 0 100 0)
        ,replace "ram" (toArray (replicate 25 (spanValue "outside-ram" ("" :: T.Text)))) (event 0 100 0)] $ \invalid ->
    check "reject invalid RAM evidence" (isLeft (parsePage initialCursor (page 0 1 1 100 True [invalid])))
  where
    toArray :: [Value] -> Value
    toArray = toJSON
    advanceCursor cursor captured endpoint
      | captured == endpoint = pure cursor
      | otherwise = do
          next <- right (parsePage cursor (page captured (captured + 64) 64 100 True [event n 100 0 | n <- [captured..captured+63]]))
          advanceCursor (pageNext next) (captured + 64) endpoint

coverageTests :: IO ()
coverageTests = do
  let initial = page 0 0 0 100 True []
  initialPage <- right (parsePage initialCursor initial)
  let unobserved = startCoverage (pageThrough initialPage)
  check "begin state is not a successfully collected interval" (field "last_successful_collected_through" (coverageValue unobserved) == Just Null)
  check "advancement requires initial empty boundary" (isLeft (openInterval ShutterSequence unobserved))
  initialCoverage <- right (collectBoundary initialPage unobserved)
  check "complete retained-event boundary does not prove instrumentation coverage"
    (field "retained_event_ram_context_gap" (coverageValue initialCoverage) == Just (Bool False)
      && field "instrumentation_coverage" (coverageValue initialCoverage)
        == Just (object ["scope" .= ("armed-sites-only" :: T.Text), "uninstrumented_gap" .= Null]))
  started <- right (openInterval ShutterSequence initialCoverage)
  paused <- right (parseAdvance ShutterSequence (sequenceProgress "drain-required" 8000100 8000100 0 "half" []))
  right (validateAdvance paused started)
  check "drain is not completion" (advanceState paused == DrainRequired)
  let phase position start finish = object ["position" .= (position :: T.Text), "started" .= (start :: Word64), "finished" .= (finish :: Word64)]
      phaseRows = [phase "half" 100 8000100, phase "full" 8000100 16000100, phase "half" 16000100 20000100, phase "released" 20000100 24000100]
  sequenceDone <- right (parseAdvance ShutterSequence (sequenceProgress "completed" 24000100 24000100 3 "released" phaseRows))
  right (validateAdvance sequenceDone started)
  check "fixed physical sequence completes at 24M" (advanceState sequenceDone == Completed && instructionValue (advanceInstructions sequenceDone) == 24000100)
  check "changed physical contact rejected" (isLeft (parseAdvance ShutterSequence (sequenceProgress "drain-required" 100 8000100 0 "full" [])))
  check "changed phase budget rejected" (isLeft (parseAdvance ShutterSequence (sequenceProgress "completed" 24000100 24000100 3 "released" (phase "half" 100 8000101 : drop 1 phaseRows))))
  let shiftedRows = [phase "half" 1000 8001000, phase "full" 8001000 16001000, phase "half" 16001000 20001000, phase "released" 20001000 24001000]
  check "completed phase tail must match declared endpoint" (isLeft (parseAdvance ShutterSequence
    (sequenceProgress "completed" 24000100 24000100 3 "released" shiftedRows)))
  shiftedOperation <- right (parseAdvance ShutterSequence
    (sequenceProgress "completed" 24001000 24001000 3 "released" shiftedRows))
  check "completed phase range must match opened interval" (isLeft (validateAdvance shiftedOperation started))
  check "phase-zero current cannot precede operation start" (isLeft (parseAdvance ShutterSequence
    (sequenceProgress "drain-required" 99 8000100 0 "half" [])))
  check "paused current cannot precede current phase start" (isLeft (parseAdvance ShutterSequence
    (sequenceProgress "drain-required" 8000099 16000100 1 "full" (take 1 phaseRows))))
  check "paused previous phase tail must match current phase start" (isLeft (parseAdvance ShutterSequence
    (sequenceProgress "drain-required" 8000100 16000100 1 "full" (take 1 shiftedRows))))
  forM_ [(0, "half", 100, 8000100, []), (1, "full", 8000100, 16000100, take 1 phaseRows)
        ,(1, "full", 16000100, 16000100, take 1 phaseRows)] $ \(phaseIndex, contact, at, endpoint, previous) -> do
    validPause <- right (parseAdvance ShutterSequence
      (sequenceProgress "drain-required" at endpoint phaseIndex contact previous))
    right (validateAdvance validPause started)
    check "phase starts and exact endpoints remain valid pauses"
      (advanceState validPause == DrainRequired && instructionValue (advanceInstructions validPause) == at)
  first <- right (parsePage initialCursor (page 0 64 64 8000100 True [event n 8000100 (n `mod` 2) | n <- [0..63]]))
  uncollected <- right (collectBoundary first started)
  check "nonempty page does not claim interval collected" (field "last_successful_collected_through" (coverageValue uncollected) == Just (Number 100))
  empty <- right (parsePage (pageNext first) (page 64 64 0 8000100 True []))
  collected <- right (collectBoundary empty uncollected)
  check "drained boundary advances collected-through" (field "last_successful_collected_through" (coverageValue collected) == Just (Number 8000100))
  check "pending operation survives phase-end drain" (field "pending_uncollected_interval" (coverageValue collected) == Just (object ["from_instructions" .= (8000100 :: Int), "intended_endpoint" .= (24000100 :: Int)]))
  check "unrelated advance refused while interval pending" (isLeft (openInterval Window collected))
  tailPage <- right (parsePage (pageNext first) (page 64 65 1 9000100 False [event 64 9000100 1]))
  failed <- right (collectBoundary tailPage collected)
  check "strict tail preserves last collected boundary" (field "last_successful_collected_through" (coverageValue failed) == Just (Number 8000100))
  check "strict tail remains incomplete" (coverageIncomplete failed)
  tailEmpty <- right (parsePage (pageNext tailPage) (page 65 65 0 9000100 True []))
  failedCollected <- right (collectBoundary tailEmpty failed)
  check "strict failure never reaches intended endpoint" (coverageIncomplete failedCollected && field "last_successful_collected_through" (coverageValue failedCollected) == Just (Number 9000100))
  completed <- right (parsePage (pageNext tailPage) (page 65 65 0 24000100 True []))
  finished <- right (collectBoundary completed failedCollected)
  check "all 65 events stay ordered across reuse" (map (field "sequence" . eventValue) (pageHits first ++ pageHits tailPage) == map (Just . Number . fromIntegral) [0..64 :: Int])
  check "endpoint collection completes interval" (not (coverageIncomplete finished))
  window <- right (openInterval Window finished)
  windowDone <- right (parseAdvance Window (progress "completed" "window" 64000100 64000100))
  right (validateAdvance windowDone window)
  check "fixed window budget preserved" (instructionValue (advanceInstructions windowDone) == 64000100)
  wrongEndpoint <- right (parseAdvance Window (progress "completed" "window" 64000101 64000101))
  check "changed instruction budget rejected" (isLeft (validateAdvance wrongEndpoint window))
  check "operation mismatch rejected" (isLeft (parseAdvance Window (progress "drain-required" "shutter-sequence" 100 200)))
  incompleteEmpty <- right (parsePage initialCursor (page 0 0 0 101 False []))
  incompleteCoverage <- right (collectBoundary incompleteEmpty initialCoverage)
  check "held hooks do not advance successful boundary" (field "last_successful_collected_through" (coverageValue incompleteCoverage) == Just (Number 100))
  check "held-hook event context remains unknown" (field "retained_event_ram_context_gap" (coverageValue incompleteCoverage) == Just Null)
  let lostFixture = replace "total" (Number 1) $ replace "dropped" (Number 1) $ replace "incomplete" (Bool True)
        $ replace "sites" (toJSON [site 4096 1 0 1 100]) (page 0 0 0 100 True [])
  lostPage <- right (parsePage initialCursor lostFixture)
  lost <- right (collectBoundary lostPage (startCoverage (pageThrough initialPage)))
  check "detail loss does not imply known RAM completeness" (field "detail_loss" (coverageValue lost) == Just (Bool True)
    && field "retained_event_ram_context_gap" (coverageValue lost) == Just Null)
  let ramGapFixture = replace "incomplete" (Bool True) $ page 0 1 1 100 True [replace "ram_incomplete" (Bool True) (event 0 100 0)]
  ramGapPage <- right (parsePage initialCursor ramGapFixture)
  ramGap <- right (collectBoundary ramGapPage (startCoverage (pageThrough initialPage)))
  check "RAM gap stays separate from detail loss" (field "detail_loss" (coverageValue ramGap) == Just (Bool False)
    && field "retained_event_ram_context_gap" (coverageValue ramGap) == Just (Bool True))

ioTests :: IO ()
ioTests = do
  withTemp $ \path -> do
    journal <- openJournal path
    let fixture = page 0 1 1 100 True [event 0 100 0]
    parsed <- journalPage journal initialCursor fixture
    persisted <- B.readFile path >>= right . eitherDecodeStrict'
    check "complete page persisted before acknowledgement becomes available" (persisted == object ["label" .= ("capture-observation-page" :: T.Text), "value" .= fixture])
    check "durable page yields exact acknowledgement cursor" (ackRequest (pageNext parsed) == object ["command" .= ("capture-observation-ack" :: T.Text), "next" .= (1 :: Int)])
    check "refused acknowledgement leaves journal evidence intact" (isLeft (validateAck (pageNext parsed) 0
      (object ["acknowledged" .= (0 :: Int), "pending" .= (1 :: Int)])))
    afterRefusal <- B.readFile path >>= right . eitherDecodeStrict'
    check "acknowledgement refusal does not erase or replace the page" (afterRefusal == persisted)
    closeJournal journal
    failed <- try (journalPage journal initialCursor fixture) :: IO (Either IOException Page)
    check "journal failure cannot yield an acknowledgeable page" (isLeft failed)
  withTemp $ \base -> do
    let parent = base ++ ".directory"
        output = parent </> "fresh-output"
        path = output </> "observations.jsonl"
    bracket_ (createDirectory parent) (removeDirectoryRecursive parent) $ do
      createDirectory output
      refused <- bracket_ (setFileMode parent 0o100) (setFileMode parent 0o700)
        (try (openJournal path) :: IO (Either IOException Journal))
      case refused of
        Left err -> check "unreadable output parent refuses journal acquisition" (isPermissionError err)
        Right journal -> closeJournal journal >> fail "journal acquired without synchronizing its parent directory"
      untouched <- B.readFile path
      check "directory synchronization failure publishes no page" (B.null untouched)
      bracket (openJournal path) closeJournal $ \journal -> do
        let fixture = page 0 1 1 100 True [event 0 100 0]
        _ <- journalPage journal initialCursor fixture
        persisted <- B.readFile path >>= right . eitherDecodeStrict'
        check "fresh output directory preserves its complete page"
          (persisted == object ["label" .= ("capture-observation-page" :: T.Text), "value" .= fixture])
  forM_ [0,4095,4096,1048576] $ \size -> withTemp $ \path -> do
    let line = B.replicate size 120
    B.writeFile path (line <> "\nsecond\n")
    withBinaryFile path ReadMode $ \handle -> do
      reader <- newBoundedReader handle
      first <- readBoundedLine reader
      second <- readBoundedLine reader
      check "bounded reader preserves exact line and buffered successor" (first == line && second == "second")
  withTemp $ \path -> do
    B.writeFile path (B.replicate 1048577 120 <> "\n")
    withBinaryFile path ReadMode $ \handle -> do
      reader <- newBoundedReader handle
      result <- try (readBoundedLine reader) :: IO (Either IOException B.ByteString)
      check "oversize reply rejected" (isLeft result)
  withTemp $ \path -> do
    B.writeFile path "unfinished"
    withBinaryFile path ReadMode $ \handle -> do
      reader <- newBoundedReader handle
      result <- try (readBoundedLine reader) :: IO (Either IOException B.ByteString)
      check "truncated response rejected" (isLeft result)

main :: IO ()
main = parserTests >> coverageTests >> ioTests
