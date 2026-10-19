module Main where

import Control.Exception
import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.IORef
import Data.List (isInfixOf)
import Data.Word (Word8, Word64)
import Numeric (showHex)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Read (readOpcode, readRequestBytes, readReplyBytes, word32At, word32le)
import FujiAF.Probe
  ( configReplyBytes, configWriteBytes, configWriteLength, debugOpcode
  , debugRequestBytes, gateReadbackEnabled, gateReadbackRestored )
import FujiAF.ReadMedia
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

-- A clock read from the given source whose pauses return at once.
instantClock :: IO Word64 -> Clock
instantClock now = TimedClock now (\_ -> pure ())

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

sessionSid :: Int
sessionSid = 0x117c

hex8 :: Int -> String
hex8 value = replicate (8-length digits) '0' ++ digits
  where digits = showHex value ""

le32 :: Int -> [Word8]
le32 value = [at 0, at 1, at 2, at 3]
  where
    at :: Int -> Word8
    at index = fromIntegral ((value `div` 256^index) `mod` 256)

-- The window walk one bank must emit, written out rather than taken from the
-- operation: 528 requests of 248 bytes and one 128-byte remainder from the
-- bank's own offset in the window, all inside that bank's 0x20000-byte block.
expectedBankSlices :: Int -> [(Int,Int)]
expectedBankSlices offset =
  [ (0x4b2ece00 + offset + index*248, min 248 (0x20000 - index*248)) | index <- [0..528] ]

-- The two banks the synthetic camera serves, written out from the documented
-- entry rule rather than taken from the operation: entry 0x2c at
-- map + 0x10 + 2*0x56 and entry 0x32 at map + 0x10 + 2*0x57, raw block
-- entry + the map's start word, selector block `div` 8 and offset (block `mod` 8)*0x20000.
-- bankModels scripts the captured map whose start word is 0x380;
-- shiftedModels scripts the same entries under a start word of 0x303, whose
-- blocks 0x32f/0x335 give different selectors and offsets than 0x380 would.
bankModels, shiftedModels :: [(Int,Int,Int,Int,Int)]
bankModels =
  [ (0x56,0x2c,0x3ac,0x75,0x80000)
  , (0x57,0x32,0x3b2,0x76,0x40000) ]
shiftedModels =
  [ (0x56,0x2c,0x32f,0x65,0xe0000)
  , (0x57,0x32,0x335,0x66,0xa0000) ]

-- The header words each model's camera serves at the map base: the start
-- word and the entry count the 2026-10-08 captures hold at map + 8.
capturedStart, shiftedStart, capturedCount :: Int
capturedStart = 0x380
shiftedStart = 0x303
capturedCount = 0x6c

parsedStart :: Int -> MapStart
parsedStart value = case mkMapStart value of
  Right start -> start
  Left reason -> error reason

modelSelector :: [(Int,Int,Int,Int,Int)] -> Int -> Word8
modelSelector model position = let (_,_,_,selector,_) = model !! position in fromIntegral selector

modelOffset :: [(Int,Int,Int,Int,Int)] -> Int -> Int
modelOffset model position = let (_,_,_,_,offset) = model !! position in offset

-- The synthetic cache bytes: 0x5a everywhere except the fields the plan
-- names. The ten staging fields are distinct and the gate byte is neither
-- zero nor the enable value, so a permutation or a constant in the restore
-- cannot match.
windowWith :: [(Int,Word8)] -> [Word8]
windowWith patches = [maybe 0x5a id (lookup index patches) | index <- [0..255]]

restoreFieldValues :: [Word8]
restoreFieldValues = [0x51..0x5a]

snapshotGateByte :: Word8
snapshotGateByte = 2

snapshotWindow :: [Word8]
snapshotWindow = windowWith ([(0x80,0x77),(0xf7,snapshotGateByte)] ++ zip [0x81..0x8a] restoreFieldValues)

enabledWindow :: [Word8]
enabledWindow = windowWith [(0x80,0x77),(0xf7,1)]

restoredWindow :: Word8 -> [Word8]
restoredWindow gate = windowWith ([(0x80,0x33),(0xf7,gate)] ++ zip [0x81..0x8a] restoreFieldValues)

pollWindow :: Word8 -> Word8 -> [Word8]
pollWindow selector code = windowWith [(0x80,0x35),(0x81,0),(0x82,selector),(0x83,code),(0xf7,1)]

-- The synthetic staging bytes depend on the address and the in-slice offset,
-- so a swapped, shifted or repeated composition cannot match the capture.
sliceFor :: Int -> Int -> [Word8]
sliceFor address size = [fromIntegral ((address+i) `mod` 251) | i <- [0..size-1]]

data Request
  = WindowReq String
  | PollReq Int Int
  | WriteReq String Int Word8
  | RamReq String Int Int
  | SliceReq Int Int
  | CloseReq
  deriving (Eq, Show)

sessionStart :: [Request]
sessionStart =
  [ WindowReq "cache-snapshot"
  , WriteReq "gate-enable" 0xf7 1
  , WindowReq gateReadbackEnabled
  , RamReq "group-2-state" 0x015c20b0 2
  , RamReq "map-pointer" 0x00919a20 4
  , RamReq "map-header" 0x0095dae0 12
  , RamReq "map-entry-0x56" 0x0095db9c 2
  , RamReq "map-entry-0x57" 0x0095db9e 2 ]

sessionEnd :: [Request]
sessionEnd =
  [ WriteReq "end-disarm" 0x80 0x33 ]
  ++ [WriteReq ("restore-"++hex8 index) index value | (index,value) <- zip [0x81..0x8a] restoreFieldValues]
  ++ [ WriteReq "gate-restore" 0xf7 snapshotGateByte
     , WindowReq gateReadbackRestored
     , CloseReq ]

type PollScript = [[Word8]]

bankRequests :: [(Int,Int,Int,Int,Int)] -> PollScript -> Int -> [Request]
bankRequests model polls position =
  [ WriteReq ("bank"++show position++"-disarm") 0x80 0x33
  , WriteReq ("bank"++show position++"-spare") 0x81 0
  , WriteReq ("bank"++show position++"-block") 0x82 (modelSelector model position)
  , WriteReq ("bank"++show position++"-sentinel") 0x83 0xff
  , WriteReq ("bank"++show position++"-trigger") 0x80 0x35 ]
  ++ [PollReq position index | index <- [0 .. length (polls !! position) - 1]]
  ++ map (uncurry SliceReq) (expectedBankSlices (modelOffset model position))

planForWith :: [(Int,Int,Int,Int,Int)] -> PollScript -> [Request]
planForWith model polls = sessionStart ++ concatMap (bankRequests model polls) [0,1] ++ sessionEnd

planFor :: PollScript -> [Request]
planFor = planForWith bankModels

happyPolls :: PollScript
happyPolls = [[0xff,0xff,0],[0xff,0xff,0]]

requestPlan :: [Request]
requestPlan = planFor happyPolls

-- A plan that stops after the named request and still runs the teardown.
stopsAfterWith :: PollScript -> Request -> [Request]
stopsAfterWith polls target = case break (== target) (planFor polls) of
  (before, _:_) -> before ++ [target] ++ sessionEnd
  _ -> error ("stop request not in the plan: "++show target)

stopsAfter :: Request -> [Request]
stopsAfter = stopsAfterWith happyPolls

-- Wire shapes written out from the documented layouts rather than taken from
-- the operation.
windowFrame :: [Word8]
windowFrame =
  [0x02,0x00,0x1a,0x00] ++ replicate 12 0
  ++ [0x02,0,0,0, 0x01,0,0,0, 0,0,0,0, 0,0,0,0, 0x00,0x01,0,0]

cacheWriteFrameBytes :: Int -> Word8 -> [Word8]
cacheWriteFrameBytes index value =
  [0x02,0x00,0x1a,0x00] ++ replicate 12 0
  ++ [0x03,0,0,0, 0x01,0,0,0, 0,0,0,0] ++ le32 index ++ [0x01,0,0,0, value]

ramFrame :: Int -> Int -> [Word8]
ramFrame address size =
  [0x01,0x00,0x20,0x00] ++ replicate 12 0
  ++ [0x02,0,0,0, 0,0,0,0, 0,0,0,0] ++ le32 address ++ le32 size

-- The audited trigger frame and the audited first and last staging frames of
-- each bank.
triggerFrameBytes :: [Word8]
triggerFrameBytes =
  [0x02,0x00,0x1a,0x00] ++ replicate 12 0
  ++ [0x03,0,0,0, 0x01,0,0,0, 0,0,0,0, 0x80,0,0,0, 0x01,0,0,0, 0x35]

bank0FirstSliceFrame, bank0LastSliceFrame, bank1FirstSliceFrame, bank1LastSliceFrame :: [Word8]
bank0FirstSliceFrame = ramFrame 0x4b36ce00 248
bank0LastSliceFrame = ramFrame 0x4b38cd80 128
bank1FirstSliceFrame = ramFrame 0x4b32ce00 248
bank1LastSliceFrame = ramFrame 0x4b34cd80 128

windowReplyWith :: Int -> Int -> [Word8] -> [Word8]
windowReplyWith err1 err2 payload = concat
  [ word32le readOpcode, word32le err1, word32le err2, word32le 0
  , word32le 2, word32le 1, word32le 0, word32le 0, word32le 256 ]
  ++ payload

windowReply :: [Word8] -> [Word8]
windowReply = windowReplyWith 0 0

writeReplyWith :: Int -> Int -> Int -> [Word8]
writeReplyWith err1 err2 index = concat
  [ word32le readOpcode, word32le err1, word32le err2, word32le 0
  , word32le 3, word32le 1, word32le 0, word32le index, word32le configWriteLength ]
  ++ replicate 256 0

writeReply :: Int -> Word8 -> [Word8]
writeReply index value
  | index == 0x80 && value == 0x33 = writeReplyWith 1 0xffff index
  | otherwise = writeReplyWith 0 0 index

ramReplyWith :: Int -> Int -> Int -> Int -> Int -> [Word8] -> [Word8]
ramReplyWith opcode err1 err2 address size payload = concat
  [ word32le opcode, word32le err1, word32le err2, word32le 0
  , word32le 2, word32le 0, word32le 0
  , word32le address, word32le size ]
  ++ payload

ramReply :: Int -> Int -> [Word8] -> [Word8]
ramReply = ramReplyWith debugOpcode 0 0

-- The RAM replies are keyed by the address on the wire, so a map entry read
-- at any address but the firmware's own has no scripted bytes at all. The
-- map base answers the model's own start word and entry count.
ramFor :: Int -> Int -> Int -> [Word8]
ramFor start address size = case (address,size) of
  (0x015c20b0,2) -> [0x01,0x00]
  (0x00919a20,4) -> le32 0x0095dae0
  (0x0095dae0,12) -> le32 start ++ le32 0 ++ le32 capturedCount
  (0x0095db9c,2) -> [0x2c,0]
  (0x0095db9e,2) -> [0x32,0]
  _ -> error ("no scripted RAM reply at 0x"++hex8 address++" size "++show size)

outcomeForWith :: Int -> [(Int,Int,Int,Int,Int)] -> PollScript -> Request -> P.Outcome
outcomeForWith start model polls request = case request of
  WindowReq label -> P.Reply (windowReply (windowFor label))
  PollReq position index ->
    P.Reply (windowReply (pollWindow (modelSelector model position) (polls !! position !! index)))
  WriteReq _ index value -> P.Reply (writeReply index value)
  RamReq _ address size -> P.Reply (ramReply address size (ramFor start address size))
  SliceReq address size -> P.Reply (ramReply address size (sliceFor address size))
  CloseReq -> P.Reply [0x7c,0x11]
  where
    windowFor label
      | label == "cache-snapshot" = snapshotWindow
      | label == gateReadbackEnabled = enabledWindow
      | label == gateReadbackRestored = restoredWindow snapshotGateByte
      | otherwise = error ("unknown window "++label)

outcomeFor :: PollScript -> Request -> P.Outcome
outcomeFor = outcomeForWith capturedStart bankModels

happyOutcome :: Request -> P.Outcome
happyOutcome = outcomeFor happyPolls

mutateWith :: (Request -> P.Outcome) -> Request -> P.Outcome -> Request -> P.Outcome
mutateWith base target replacement request = if request == target then replacement else base request

mutate :: Request -> P.Outcome -> Request -> P.Outcome
mutate = mutateWith happyOutcome

openSteps :: [P.Step]
openSteps =
  [ P.Step (P.controlTransfer (P.control 0xc0 8 0)) (P.Transferred 12)
  , P.Step (P.BulkIn 0x81 8) (P.Reply [0x7c,0x11,0,0,0,0,1,0]) ]

stepsFor :: [Request] -> (Request -> P.Outcome) -> [P.Step]
stepsFor plan outcome = openSteps ++ concatMap step plan
  where
    step request = case request of
      WindowReq _ -> windowSteps (outcome request)
      PollReq _ _ -> windowSteps (outcome request)
      WriteReq _ index value ->
        [ P.Step (P.controlTransfer (P.control 0xd0 configWriteBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkOut 2 (cacheWriteFrameBytes index value)) (P.Transferred configWriteBytes)
        , P.Step (P.controlTransfer (P.control 0xd1 configReplyBytes sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 configReplyBytes) (outcome request) ]
      RamReq _ address size -> ramSteps address size (outcome request)
      SliceReq address size -> ramSteps address size (outcome request)
      CloseReq ->
        [ P.Step (P.controlTransfer (P.control 0xc4 2 sessionSid)) (P.Transferred 12)
        , P.Step (P.BulkIn 0x81 2) (outcome request) ]
    windowSteps reply =
      [ P.Step (P.controlTransfer (P.control 0xd0 readRequestBytes sessionSid)) (P.Transferred 12)
      , P.Step (P.BulkOut 2 windowFrame) (P.Transferred readRequestBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 readReplyBytes sessionSid)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 readReplyBytes) reply ]
    ramSteps address size reply =
      [ P.Step (P.controlTransfer (P.control 0xd0 debugRequestBytes sessionSid)) (P.Transferred 12)
      , P.Step (P.BulkOut 2 (ramFrame address size)) (P.Transferred debugRequestBytes)
      , P.Step (P.controlTransfer (P.control 0xd1 (0x24+size) sessionSid)) (P.Transferred 12)
      , P.Step (P.BulkIn 0x81 (0x24+size)) reply ]

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

runCase :: Clock -> FilePath -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCase = runCaseWith (\_ -> pure ())

-- The dispatch hook lets a test mark a clock at an exact transfer.
runCaseWith :: (P.Transfer -> IO ()) -> Clock -> FilePath -> [P.Step] -> IO (Either SomeException Int,[P.Transfer],[CleanupEvent])
runCaseWith onDispatch clock dir steps = do
  createDirectoryIfMissing True dir
  emitted <- newIORef []
  events <- newIORef []
  script <- newIORef (take (length steps-2) steps)
  closeScript <- newIORef (drop (length steps-2) steps)
  let device = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors
      api = UsbApi (\_ -> pure ()) (\_ -> pure [device]) (\_ _ -> pure ()) (\_ _ -> pure ())
        (\packet _ -> do
          modifyIORef' emitted (++[packet])
          onDispatch packet
          let isClose = packet == P.controlTransfer (P.control 0xc4 2 sessionSid) || packet == P.BulkIn 0x81 2
              activeScript = if isClose then closeScript else script
          remainingSteps <- readIORef activeScript
          case remainingSteps of
            [] -> P.reject "script exhausted"
            P.Step expected outcome:rest -> do
              writeIORef activeScript rest
              check ("packet differs from the audited sequence: "++show packet) (packet == expected)
              case outcome of
                P.Transferred n -> pure (n,[])
                P.Reply bytes -> pure (length bytes,bytes)
                P.LostReply -> P.reject "injected transport failure")
        (\_ _ -> modifyIORef' events (++[Released]))
        (\_ -> modifyIORef' events (++[Closed]))
        (\_ -> modifyIORef' events (++[Exited]))
  outcome <- try (runReadMedia clock api dir)
  packets <- readIORef emitted
  cleanupEvents <- readIORef events
  pure (outcome,packets,cleanupEvents)

-- Durable-before-dispatch asserted at the moment it claims: the outgoing log
-- must already hold a frame when the dispatch hook sees it. Each hit is
-- recorded, so a frame that never dispatches cannot pass silently.
durableBeforeDispatch :: FilePath -> IORef [String] -> [[Word8]] -> P.Transfer -> IO ()
durableBeforeDispatch dir checked frames packet = case packet of
  P.BulkOut _ frame | frame `elem` frames -> do
    outgoing <- readFile (dir </> "outgoing.v1")
    check ("frame is not in the outgoing log at its dispatch: "++P.hexBytes frame)
      (P.hexBytes frame `isInfixOf` outgoing)
    modifyIORef' checked (++[P.hexBytes frame])
  _ -> pure ()

bulkFrame :: P.Transfer -> [Word8]
bulkFrame (P.BulkOut _ frame) = frame
bulkFrame _ = []

isWriteFrame :: P.Transfer -> Bool
isWriteFrame packet = word32At 0 frame == readOpcode && word32At 0x10 frame == 3
  where frame = bulkFrame packet

isWindowFrame :: P.Transfer -> Bool
isWindowFrame packet = word32At 0 frame == readOpcode && word32At 0x10 frame == 2
  where frame = bulkFrame packet

isStagingFrame :: P.Transfer -> Bool
isStagingFrame packet = word32At 0 frame == debugOpcode && word32At 0x1c frame >= 0x4b2ece00
  where frame = bulkFrame packet

isRamFrame :: P.Transfer -> Bool
isRamFrame packet = word32At 0 (bulkFrame packet) == debugOpcode

writeIndexValue :: [Word8] -> (Int,Word8)
writeIndexValue frame = (256*word32At 0x18 frame + word32At 0x1c frame, frame !! (length frame - 1))

frameAddressSize :: [Word8] -> (Int,Int)
frameAddressSize frame = (word32At 0x1c frame, word32At 0x20 frame)

writeFramesOf :: [P.Transfer] -> [[Word8]]
writeFramesOf packets = [frame | packet <- packets, isWriteFrame packet, let frame = bulkFrame packet]

-- The disarm and the eleven restores the teardown must emit before the close.
teardownWriteValues :: [(Int,Word8)]
teardownWriteValues =
  [(0x80,0x33)] ++ zip [0x81..0x8a] restoreFieldValues ++ [(0xf7,snapshotGateByte)]

-- A stop is checked for its message, the exact number of staging requests
-- before the refusal, the end disarm and the restores before the close, one
-- protocol close, complete host cleanup and a durable refusal or uncertainty
-- record.
expectStop :: FilePath -> (String,String) -> Int -> [P.Step] -> IO ()
expectStop dir (label,fragment) expectedSliceRequests steps = do
  (outcome,packets,cleanupEvents) <- runCase (instantClock (pure 0)) dir steps
  let sliceRequestCount = length (filter isStagingFrame packets)
      writes = map writeIndexValue (writeFramesOf packets)
  check (label++": session must stop") (either (const True) (const False) outcome)
  check (label++": stop message") (either (isInfixOf fragment . displayException) (const False) outcome)
  check (label++": "++show sliceRequestCount++" staging requests, expected "++show expectedSliceRequests)
    (sliceRequestCount == expectedSliceRequests)
  check (label++": the end disarm and the restores precede the close")
    (drop (length writes-12) writes == teardownWriteValues)
  check (label++": exactly one protocol close") (length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets] == 1)
  check (label++": every host resource released once") (cleanupEvents == [Released,Closed,Exited])
  journal <- readFile (dir </> "journal.v1")
  check (label++": refusal or uncertainty is durable")
    (("read-media-refused" `isInfixOf` journal) || ("read-media-precondition" `isInfixOf` journal)
      || ("session-uncertain" `isInfixOf` journal))

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let base = tmp </> ("fuji-af-read-media-tests-"++show pid)
  createDirectory base

  -- The window walk: count, stride, tail and containment inside the bank's
  -- own block.
  check "each bank walk is 529 requests"
    (all (\offset -> length (expectedBankSlices offset) == 529) [0x80000,0x40000])
  check "the walk is the 248-byte stride with a 128-byte remainder"
    (take 2 (expectedBankSlices 0x80000) == [(0x4b36ce00,248),(0x4b36ce00+248,248)]
      && take 1 (reverse (expectedBankSlices 0x80000)) == [(0x4b38cd80,128)]
      && sum [size | (_,size) <- expectedBankSlices 0x80000] == 0x20000)
  check "every request carries 1..248 bytes and stays inside its bank's block"
    (all (\offset -> all (\(address,size) -> size >= 1 && size <= 248 && address >= 0x4b2ece00 + offset
                              && address + size <= 0x4b2ece00 + offset + 0x20000) (expectedBankSlices offset))
         [0x80000,0x40000])
  check "the shipped walk equals the written-out walk"
    (concatMap bankSlices [0x80000,0x40000] == expectedBankSlices 0x80000 ++ expectedBankSlices 0x40000
      && bankSliceCount == 529 && mediaSliceCount == 1058)

  -- The map entry addressing and the per-bank derivation, against the
  -- documented entries 0x2c/0x32 of the 2026-10-08 capture and the documented
  -- identity entries 0x56/0x57 (raw-media-read.md section 5).
  check "mapEntryAddress places each entry at map + 0x10 + 2 * index"
    (mapEntryAddress 0x0095dae0 0x56 == 0x0095db9c && mapEntryAddress 0x0095dae0 0x57 == 0x0095db9e
      && mapEntryIndexes == [0x56,0x57])
  check "the captured map entries derive one plan per bank"
    (case deriveBankPlans (parsedStart capturedStart) capturedCount [(0x56,0x2c),(0x57,0x32)] of
       Right plans -> map (\plan -> (bankEntryIndex plan,bankEntry plan,bankBlock plan
                                    ,fromIntegral (blockSelectorByte (bankSelector plan)) :: Int,bankOffset plan)) plans
                        == bankModels
       _ -> False)
  check "identity entries derive one window and their own offsets"
    (case deriveBankPlans (parsedStart capturedStart) capturedCount [(0x56,0x56),(0x57,0x57)] of
       Right plans -> map (blockSelectorByte . bankSelector) plans == [0x7a,0x7a]
                        && map bankOffset plans == [0xc0000,0xe0000]
       _ -> False)
  check "the derived selectors and offsets follow the start word"
    (case deriveBankPlans (parsedStart shiftedStart) capturedCount [(0x56,0x2c),(0x57,0x32)] of
       Right plans -> map bankBlock plans == [0x32f,0x335]
                        && map (blockSelectorByte . bankSelector) plans == [0x65,0x66]
                        && map bankOffset plans == [0xe0000,0xa0000]
       _ -> False)
  check "an entry index at or above the entry count is refused naming the index and the count"
    (case deriveBankPlans (parsedStart capturedStart) 0x55 [(0x56,0x2c),(0x57,0x32)] of
       Left reason -> "0x56" `isInfixOf` reason && "0x55" `isInfixOf` reason
       _ -> False)
  check "an index equal to the entry count is refused and one below it is not"
    (either (const True) (const False) (deriveBankPlans (parsedStart capturedStart) 0x57 [(0x57,0x32)])
      && either (const False) (const True) (deriveBankPlans (parsedStart capturedStart) 0x58 [(0x57,0x32)]))
  check "a 0xffff entry is refused naming its index"
    (case deriveBankPlans (parsedStart capturedStart) capturedCount [(0x56,0xffff)] of
       Left reason -> "0x56" `isInfixOf` reason && "0xffff" `isInfixOf` reason
       _ -> False)
  check "a selector above 0x7f is refused"
    (case deriveBankPlans (parsedStart capturedStart) capturedCount [(0x57,0x80)] of
       Left reason -> "0x7f" `isInfixOf` reason
       _ -> False)
  check "two entries naming one raw block are refused"
    (case deriveBankPlans (parsedStart capturedStart) capturedCount [(0x56,0x2c),(0x57,0x2c)] of
       Left reason -> "same raw block" `isInfixOf` reason
       _ -> False)
  check "mkBlockSelector accepts 0..0x7f and refuses the rest"
    (all (either (const False) (const True) . mkBlockSelector) [0..0x7f]
      && all (either (const True) (const False) . mkBlockSelector) [-1, 0x80, 0x100, 0xffff])
  check "mkMapStart accepts the device's raw blocks and refuses the rest"
    (all (either (const False) (const True) . mkMapStart) [0, capturedStart, 0x3ff]
      && all (either (const True) (const False) . mkMapStart) [-1, 0x400, 0x1000])
  check "a start that is not a device block is refused naming it"
    (case mkMapStart 0x400 of
       Left reason -> "map start" `isInfixOf` reason && "0x400" `isInfixOf` reason
       _ -> False)

  -- The snapshot parse and the restore plan.
  check "the snapshot parse takes the ten staging fields and the gate byte"
    (cacheSnapshotOf snapshotWindow == Right (CacheSnapshot restoreFieldValues snapshotGateByte))
  check "a snapshot window of the wrong length is refused"
    (either (const True) (const False) (cacheSnapshotOf (take 255 snapshotWindow)))
  check "the restore plan writes 0x81..0x8a and then the gate"
    (map (\(index,value) -> (cacheIndexOf index, value)) (restorePlan (CacheSnapshot restoreFieldValues snapshotGateByte))
      == zip [0x81..0x8a] restoreFieldValues ++ [(0xf7,snapshotGateByte)])

  -- The reply decoders, exact and refusing, apart from any session.
  check "exact cache write decode"
    (decodeCacheWriteReply BlockIndex (0,0) (writeReply 0x82 0x75) == Right ()
      && decodeCacheWriteReply TriggerIndex (0,0) (writeReply 0x80 0x35) == Right ())
  check "the disarm reply contract is (1,0xffff) with the echo"
    (decodeCacheWriteReply TriggerIndex (1,0xffff) (writeReply 0x80 0x33) == Right ()
      && either (const True) (const False) (decodeCacheWriteReply TriggerIndex (1,0xffff) (writeReplyWith 0 0 0x80)))
  forM_ [ writeReplyWith 1 0xffff 0x80
        , writeReplyWith 0 0 0x81
        , take (configReplyBytes-1) (writeReply 0x83 0xff)
        , take 12 (writeReply 0x83 0xff) ++ word32le 1 ++ drop 16 (writeReply 0x83 0xff)
        , writeReply 0x81 0x5a
        , writeReplyWith 2 7 0x80 ] $ \bytes ->
    check "cache write reply refusal" (either (const True) (const False) (decodeCacheWriteReply TriggerIndex (0,0) bytes))
  check "exact staging read decode"
    (decodeMediaReadReply 0x4b36ce00 248 (ramReply 0x4b36ce00 248 (replicate 248 7))
      == Right (replicate 248 7))
  forM_ [ ramReplyWith debugOpcode 1 0xffff 0x4b36ce00 248 (replicate 248 0)
        , ramReplyWith debugOpcode 9 3 0x4b36ce00 248 (replicate 248 0)
        , ramReplyWith debugOpcode 2 7 0x4b36ce00 248 (replicate 248 0)
        , ramReplyWith 0x00200002 0 0 0x4b36ce00 248 (replicate 248 0)
        , take (0x24+248-1) (ramReply 0x4b36ce00 248 (replicate 248 7))
        , ramReply (0x4b36ce00+4) 248 (replicate 248 7)
        , ramReply 0x4b36ce00 247 (replicate 247 7)
        , take 12 (ramReply 0x4b36ce00 248 (replicate 248 7)) ++ word32le 1
            ++ drop 16 (ramReply 0x4b36ce00 248 (replicate 248 7)) ] $ \bytes ->
    check "staging read reply refusal" (either (const True) (const False) (decodeMediaReadReply 0x4b36ce00 248 bytes))
  check "the short final slice decodes at its own length"
    (decodeMediaReadReply 0x4b38cd80 128 (ramReply 0x4b38cd80 128 (replicate 128 7)) == Right (replicate 128 7))

  -- The documented bounds.
  check "aggregate deadline bound" (mediaAggregateDeadlineMs == 62780000)
  check "journal bound" (mediaJournalCapBytes == 12787712)
  check "poll bound" (mediaPollCap == 1024)

  -- One full session: every frame, the composition of the captures and the
  -- journal, with durable-before-dispatch asserted from the dispatch hook on
  -- the audited frames as they go by.
  let auditedFrames = [ triggerFrameBytes, bank0FirstSliceFrame, bank0LastSliceFrame
                      , bank1FirstSliceFrame, bank1LastSliceFrame, cacheWriteFrameBytes 0x83 0xff ]
  durableChecks <- newIORef []
  (happy,packets,cleanupEvents) <- runCaseWith (durableBeforeDispatch (base </> "happy") durableChecks auditedFrames)
    (instantClock (pure 0)) (base </> "happy") (stepsFor requestPlan happyOutcome)
  check ("happy session captures 262144 bytes: "++show happy) (either (const False) (== 0x40000) happy)
  check "happy session releases every resource once" (cleanupEvents == [Released,Closed,Exited])
  bank0 <- B.readFile (base </> "happy" </> "bank-0.bin")
  bank1 <- B.readFile (base </> "happy" </> "bank-1.bin")
  check "each capture is its bank's served slices in request order"
    (bank0 == B.pack (concatMap (uncurry sliceFor) (expectedBankSlices 0x80000))
      && bank1 == B.pack (concatMap (uncurry sliceFor) (expectedBankSlices 0x40000)))
  check "each capture holds one 0x20000-byte bank block"
    (B.length bank0 == 0x20000 && B.length bank1 == 0x20000)
  snapshotBytes <- B.readFile (base </> "happy" </> "cache-snapshot.bin")
  enabledBytes <- B.readFile (base </> "happy" </> (gateReadbackEnabled++".bin"))
  restoredBytes <- B.readFile (base </> "happy" </> (gateReadbackRestored++".bin"))
  check "the session-start window is captured" (B.unpack snapshotBytes == snapshotWindow)
  check "enable readback capture" (B.unpack enabledBytes == enabledWindow)
  check "restore readback capture" (B.unpack restoredBytes == restoredWindow snapshotGateByte)

  -- The emitted cache writes, in order: the gate enable, per bank the disarm,
  -- the forced 0x81 = 0, the derived selector, the sentinel and the trigger,
  -- the end disarm, the ten restores and the gate restore.
  let writes = map writeIndexValue (writeFramesOf packets)
  check "the emitted cache writes are the audited value sequence"
    (writes == [(0xf7,1)]
              ++ concat [[(0x80,0x33),(0x81,0),(0x82,modelSelector bankModels position),(0x83,0xff),(0x80,0x35)]
                        | position <- [0,1]]
              ++ [(0x80,0x33)] ++ zip [0x81..0x8a] restoreFieldValues ++ [(0xf7,snapshotGateByte)])
  check "no value but 0x35 and 0x33 reaches index 0x80"
    ([value | (index,value) <- writes, index == 0x80] == [0x33,0x35,0x33,0x35,0x33])
  check "every cache write is the 37-byte length-1 form at an allowed index"
    (all (\frame -> length frame == 37 && word32At 0 frame == readOpcode
                    && word32At 0x10 frame == 3 && word32At 0x14 frame == 1
                    && word32At 0x18 frame == 0 && word32At 0x20 frame == 1
                    && fst (writeIndexValue frame) `elem` [0x80..0x8a] ++ [0xf7])
         (writeFramesOf packets))
  check "each bank's trigger frame is the audited bytes"
    (writeFramesOf packets !! 5 == triggerFrameBytes
      && writeFramesOf packets !! 10 == triggerFrameBytes
      && cacheWriteFrameBytes 0x80 0x35 == triggerFrameBytes)

  -- The emitted RAM reads: the state halfword, the map pointer, the map
  -- header (start word and entry count), the two map entries at their own
  -- addresses and then the two bank walks in order.
  let ramShapes = [frameAddressSize (bulkFrame packet) | packet <- packets, isRamFrame packet]
  check "the RAM reads are the precondition, the map reads and then the bank walks"
    (ramShapes == [(0x015c20b0,2),(0x00919a20,4),(0x0095dae0,12),(0x0095db9c,2),(0x0095db9e,2)]
                 ++ expectedBankSlices 0x80000 ++ expectedBankSlices 0x40000)
  let stagingFrames = [bulkFrame packet | packet <- packets, isStagingFrame packet]
  check "the first and last staging frame of each bank is the audited bytes"
    (take 1 stagingFrames == [bank0FirstSliceFrame]
      && take 1 (drop 528 stagingFrames) == [bank0LastSliceFrame]
      && take 1 (drop 529 stagingFrames) == [bank1FirstSliceFrame]
      && take 1 (reverse stagingFrames) == [bank1LastSliceFrame])

  -- Order: the snapshot precedes every write, each bank's sentinel precedes
  -- its trigger, each bank's polls precede its staging requests, the teardown
  -- follows the last slice, the restore readback follows the last write and
  -- the close is last.
  let positions predicate = [index | (index,packet) <- zip [0::Int ..] packets, predicate packet]
      writePositions = positions isWriteFrame
      windowPositions = positions isWindowFrame
      stagingPositions = positions isStagingFrame
      polls = take 6 (drop 2 windowPositions)
  check "the session-start snapshot precedes every cache write"
    (case (windowPositions, writePositions) of
       (snapshotRead:_, firstWrite:_) -> snapshotRead < firstWrite
       _ -> False)
  check "each bank's sentinel write precedes its trigger write"
    (writePositions !! 4 < writePositions !! 5 && writePositions !! 9 < writePositions !! 10)
  check "every poll follows its bank's trigger and precedes its bank's staging"
    (all (writePositions !! 5 <) (take 3 polls) && all (< stagingPositions !! 0) (take 3 polls)
      && all (writePositions !! 10 <) (drop 3 polls) && all (< stagingPositions !! 529) (drop 3 polls))
  check "the end disarm and the restores follow the last staging request"
    (case (reverse stagingPositions, reverse writePositions) of
       (lastStaging:_, lastWrite:_) -> lastStaging < writePositions !! 11 && writePositions !! 11 < lastWrite
       _ -> False)
  check "the restore readback follows the last write and precedes the close"
    (case (reverse writePositions, reverse windowPositions) of
       (lastWrite:_, lastWindow:_) -> lastWrite < lastWindow
       _ -> False)
  check "the close is the final pair of the session"
    (drop (length packets-6) packets
      == [ P.controlTransfer (P.control 0xd0 readRequestBytes sessionSid)
         , P.BulkOut 2 windowFrame
         , P.controlTransfer (P.control 0xd1 readReplyBytes sessionSid)
         , P.BulkIn 0x81 readReplyBytes
         , P.controlTransfer (P.control 0xc4 2 sessionSid), P.BulkIn 0x81 2 ])

  -- The journal: one hashed summary per slice, the poll trail, the derived
  -- plan and the whole-file hashes, all within the cap.
  happyJournal <- readFile (base </> "happy" </> "journal.v1")
  happyOutgoingLog <- readFile (base </> "happy" </> "outgoing.v1")
  let sliceLines = filter ("slice-complete" `isInfixOf`) (lines happyJournal)
  check "one slice-complete line per staging request" (length sliceLines == 1058)
  check "the first and last slice summary of each bank carries its address, length and hash"
    (all (\(bank,index,address,size) ->
           any (isInfixOf ("bank="++show bank++";index="++show index++";address=0x"++hex8 address
                           ++";length="++show size++";sha256="++sha256 (B.pack (sliceFor address size))))
               sliceLines)
         ([(0,0,0x4b36ce00,248),(0,528,0x4b38cd80,128),(1,0,0x4b32ce00,248),(1,528,0x4b34cd80,128)] :: [(Int,Int,Int,Int)]))
  check "the poll trail records the sentinel then the terminal code of each bank"
    (length (filter ("result-poll" `isInfixOf`) (lines happyJournal)) == 6
      && "bank=0;poll=0;result=255" `isInfixOf` happyJournal
      && "bank=0;poll=2;result=0" `isInfixOf` happyJournal
      && "bank=1;poll=0;result=255" `isInfixOf` happyJournal
      && "bank=1;poll=2;result=0" `isInfixOf` happyJournal)
  check "the derived plan is journaled"
    ("bank=0;index=0x56;entry=0x2c;block=0x3ac;selector=0x75;offset=0x00080000" `isInfixOf` happyJournal
      && "bank=1;index=0x57;entry=0x32;block=0x3b2;selector=0x76;offset=0x00040000" `isInfixOf` happyJournal
      && "group-2-state" `isInfixOf` happyJournal && "state=1" `isInfixOf` happyJournal)
  check "the live map header is journaled"
    ("map-header" `isInfixOf` happyJournal && "start=0x00000380" `isInfixOf` happyJournal
      && "count=0x6c" `isInfixOf` happyJournal)
  check "one bank-complete record per bank with its file hash"
    (length (filter ("\"bank-complete\"," `isInfixOf`) (lines happyJournal)) == 2
      && ("bank=0;bytes=131072;sha256="++sha256 bank0) `isInfixOf` happyJournal
      && ("bank=1;bytes=131072;sha256="++sha256 bank1) `isInfixOf` happyJournal)
  checkedFrames <- readIORef durableChecks
  check "every audited frame was already durable at its dispatch"
    (all (`elem` checkedFrames) (map P.hexBytes auditedFrames))
  check "the journal files stay inside the cap"
    (length happyJournal <= mediaJournalCapBytes && length happyOutgoingLog <= mediaJournalCapBytes)

  -- The same entries under a start word that is not 0x380: the run reads the
  -- start word live, so the selectors and in-window offsets follow it and a
  -- reintroduced constant fails against this scripted map.
  (shifted,shiftedPackets,shiftedCleanup) <- runCase (instantClock (pure 0)) (base </> "shifted-start")
    (stepsFor (planForWith shiftedModels happyPolls) (outcomeForWith shiftedStart shiftedModels happyPolls))
  check ("shifted-start session captures 262144 bytes: "++show shifted) (either (const False) (== 0x40000) shifted)
  check "shifted-start session releases every resource once" (shiftedCleanup == [Released,Closed,Exited])
  let shiftedWrites = map writeIndexValue (writeFramesOf shiftedPackets)
      shiftedRamShapes = [frameAddressSize (bulkFrame packet) | packet <- shiftedPackets, isRamFrame packet]
  check "the emitted selectors and write sequence follow the live start word"
    (shiftedWrites == [(0xf7,1)]
              ++ concat [[(0x80,0x33),(0x81,0),(0x82,modelSelector shiftedModels position),(0x83,0xff),(0x80,0x35)]
                        | position <- [0,1]]
              ++ [(0x80,0x33)] ++ zip [0x81..0x8a] restoreFieldValues ++ [(0xf7,snapshotGateByte)])
  check "the shifted bank walks sit at the in-window offsets the start word derives"
    (shiftedRamShapes == [(0x015c20b0,2),(0x00919a20,4),(0x0095dae0,12),(0x0095db9c,2),(0x0095db9e,2)]
                       ++ expectedBankSlices 0xe0000 ++ expectedBankSlices 0xa0000)
  shiftedBank0 <- B.readFile (base </> "shifted-start" </> "bank-0.bin")
  shiftedBank1 <- B.readFile (base </> "shifted-start" </> "bank-1.bin")
  check "each shifted capture is its bank's served slices at the derived offset"
    (shiftedBank0 == B.pack (concatMap (uncurry sliceFor) (expectedBankSlices 0xe0000))
      && shiftedBank1 == B.pack (concatMap (uncurry sliceFor) (expectedBankSlices 0xa0000)))
  shiftedJournal <- readFile (base </> "shifted-start" </> "journal.v1")
  check "the shifted plan and start word are journaled"
    ("bank=0;index=0x56;entry=0x2c;block=0x32f;selector=0x65;offset=0x000e0000" `isInfixOf` shiftedJournal
      && "bank=1;index=0x57;entry=0x32;block=0x335;selector=0x66;offset=0x000a0000" `isInfixOf` shiftedJournal
      && "start=0x00000303" `isInfixOf` shiftedJournal && "count=0x6c" `isInfixOf` shiftedJournal)

  -- A stop before the cache snapshot writes only the disarm, because nothing
  -- had been changed yet.
  (preSnapshot,preSnapshotPackets,preSnapshotCleanup) <- runCase (instantClock (pure 0)) (base </> "stop-pre-snapshot")
    (stepsFor [WindowReq "cache-snapshot", WriteReq "end-disarm" 0x80 0x33, CloseReq]
      (mutateWith happyOutcome (WindowReq "cache-snapshot")
        (P.Reply (windowReplyWith 9 3 snapshotWindow))))
  check "a refused snapshot stops before any cache write"
    (either (isInfixOf "rejected arguments" . displayException) (const False) preSnapshot
      && map writeIndexValue (writeFramesOf preSnapshotPackets) == [(0x80,0x33)]
      && length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- preSnapshotPackets] == 1
      && preSnapshotCleanup == [Released,Closed,Exited])
  preSnapshotJournal <- readFile (base </> "stop-pre-snapshot" </> "journal.v1")
  check "the skipped restore is journaled" ("restore-skipped" `isInfixOf` preSnapshotJournal)

  -- Stop conditions, each against the exact reply that must cause it and the
  -- exact number of staging requests that may precede the refusal. Each one
  -- still runs the end disarm and the restores before the close.
  expectStop (base </> "stop-precondition") ("precondition", "precondition") 0
    (stepsFor (stopsAfter (RamReq "group-2-state" 0x015c20b0 2))
      (mutate (RamReq "group-2-state" 0x015c20b0 2) (P.Reply (ramReply 0x015c20b0 2 [0x04,0x00]))))
  preconditionJournal <- readFile (base </> "stop-precondition" </> "journal.v1")
  check "the state-4 stop is reported as a precondition"
    ("read-media-precondition" `isInfixOf` preconditionJournal)

  expectStop (base </> "stop-map-start") ("a map start outside the device", "not a raw block") 0
    (stepsFor (stopsAfter (RamReq "map-header" 0x0095dae0 12))
      (mutate (RamReq "map-header" 0x0095dae0 12)
        (P.Reply (ramReply 0x0095dae0 12 (le32 0x400 ++ le32 0 ++ le32 capturedCount)))))

  expectStop (base </> "stop-entry-count") ("an entry index above the count", "0x56 is at or above the map's entry count 0x30") 0
    (stepsFor (stopsAfter (RamReq "map-entry-0x57" 0x0095db9e 2))
      (mutate (RamReq "map-header" 0x0095dae0 12)
        (P.Reply (ramReply 0x0095dae0 12 (le32 0x380 ++ le32 0 ++ le32 0x30)))))

  expectStop (base </> "stop-unmapped-entry") ("unmapped entry", "map entry 0x57 is 0xffff") 0
    (stepsFor (stopsAfter (RamReq "map-entry-0x57" 0x0095db9e 2))
      (mutate (RamReq "map-entry-0x57" 0x0095db9e 2) (P.Reply (ramReply 0x0095db9e 2 [0xff,0xff]))))
  expectStop (base </> "stop-unmapped-first-entry") ("unmapped first entry", "map entry 0x56 is 0xffff") 0
    (stepsFor (stopsAfter (RamReq "map-entry-0x57" 0x0095db9e 2))
      (mutate (RamReq "map-entry-0x56" 0x0095db9c 2) (P.Reply (ramReply 0x0095db9c 2 [0xff,0xff]))))

  expectStop (base </> "stop-selector") ("a selector above 0x7f", "0x7f") 0
    (stepsFor (stopsAfter (RamReq "map-entry-0x57" 0x0095db9e 2))
      (mutate (RamReq "map-entry-0x57" 0x0095db9e 2) (P.Reply (ramReply 0x0095db9e 2 [0x80,0]))))
  expectStop (base </> "stop-same-block") ("same raw block", "same raw block") 0
    (stepsFor (stopsAfter (RamReq "map-entry-0x57" 0x0095db9e 2))
      (mutate (RamReq "map-entry-0x57" 0x0095db9e 2) (P.Reply (ramReply 0x0095db9e 2 [0x2c,0]))))
  expectStop (base </> "stop-disarm-reply") ("a wrong disarm reply", "differs from the expected") 0
    (stepsFor (stopsAfter (WriteReq "bank0-disarm" 0x80 0x33))
      (mutate (WriteReq "bank0-disarm" 0x80 0x33) (P.Reply (writeReplyWith 0 0 0x80))))
  expectStop (base </> "stop-trigger-reply") ("a wrong trigger reply", "differs from the expected") 0
    (stepsFor (stopsAfter (WriteReq "bank0-trigger" 0x80 0x35))
      (mutate (WriteReq "bank0-trigger" 0x80 0x35) (P.Reply (writeReplyWith 1 0xffff 0x80))))
  expectStop (base </> "stop-gate-enable") ("gate enable is not visible", "gate enable is not visible") 0
    (stepsFor (stopsAfter (WindowReq gateReadbackEnabled))
      (mutate (WindowReq gateReadbackEnabled) (P.Reply (windowReply (restoredWindow 0)))))
  expectStop (base </> "stop-impossible-result") ("an impossible result byte", "left the sentinel") 0
    (stepsFor (stopsAfterWith [[7],[]] (PollReq 0 0)) (outcomeFor [[7],[]]))
  expectStop (base </> "stop-result-1") ("result code 1", "result code 1") 0
    (stepsFor (stopsAfterWith [[1],[]] (PollReq 0 0)) (outcomeFor [[1],[]]))
  expectStop (base </> "stop-result-2") ("result code 2", "result code 2") 0
    (stepsFor (stopsAfterWith [[0xff,2],[]] (PollReq 0 1)) (outcomeFor [[0xff,2],[]]))

  -- The poll bound itself: 1024 sentinel reads, then the stop and the
  -- teardown.
  expectStop (base </> "stop-poll-exhaust") ("poll exhaustion", "never left the sentinel") 0
    (stepsFor (stopsAfterWith (replicate 2 (replicate 1024 0xff)) (PollReq 0 1023))
      (outcomeFor (replicate 2 (replicate 1024 0xff))))

  -- A staging reply that echoes a different address stops the capture of the
  -- second bank and keeps that bank's slices already appended, while the
  -- first bank's capture stands complete.
  let thirdSlice = expectedBankSlices 0x40000 !! 2
  expectStop (base </> "stop-echo") ("echoed address", "differs from the request") (529+3)
    (stepsFor (stopsAfter (uncurry SliceReq thirdSlice))
      (mutate (uncurry SliceReq thirdSlice)
        (P.Reply (ramReply (fst thirdSlice + 4) (snd thirdSlice) (sliceFor (fst thirdSlice) (snd thirdSlice))))))
  echoBank0 <- B.readFile (base </> "stop-echo" </> "bank-0.bin")
  echoBank1 <- B.readFile (base </> "stop-echo" </> "bank-1.bin")
  check "the first bank's capture stands complete and the second holds the slices before the refusal"
    (B.length echoBank0 == 0x20000 && B.length echoBank1 == 2*248)

  -- A lost reply mid-session still runs the teardown and the one close.
  expectStop (base </> "stop-lost") ("injected transport failure", "injected transport failure") (529+4)
    (stepsFor (stopsAfter (uncurry SliceReq (expectedBankSlices 0x40000 !! 3)))
      (mutate (uncurry SliceReq (expectedBankSlices 0x40000 !! 3)) P.LostReply))

  -- A transfer that exceeds its 5000 ms budget stops the session and still
  -- disarms and restores, because the aggregate deadline has not passed.
  lateCount <- newIORef (0::Int)
  lateJump <- newIORef False
  let lateClock = instantClock (readIORef lateJump >>= \late -> pure (if late then 6000000000 else 0))
      markLate packet = case packet of
        P.BulkIn _ size | size == 0x24+248 -> do
          count <- readIORef lateCount
          writeIORef lateCount (count+1)
          unless (count < 3) (writeIORef lateJump True)
        _ -> pure ()
  (lateOutcome,latePackets,lateCleanup) <- runCaseWith markLate lateClock (base </> "transfer-timeout")
    (stepsFor (stopsAfter (uncurry SliceReq (expectedBankSlices 0x80000 !! 3))) happyOutcome)
  check "a transfer past its budget stops the session"
    (either (isInfixOf "deadline exceeded" . displayException) (const False) lateOutcome)
  check "a transfer past its budget still disarms and restores before the close"
    (map writeIndexValue (writeFramesOf latePackets)
       == [ (0xf7,1), (0x80,0x33), (0x81,0), (0x82,modelSelector bankModels 0), (0x83,0xff), (0x80,0x35) ]
          ++ teardownWriteValues
      && lateCleanup == [Released,Closed,Exited]
      && length (filter isStagingFrame latePackets) == 4)

  -- An expired aggregate deadline stops further I/O, so the restore is
  -- attempted but not dispatched and the outcome is uncertain.
  expiredCount <- newIORef (0::Int)
  expiredJump <- newIORef False
  let expiredClock = instantClock (readIORef expiredJump >>= \late -> pure (if late then 1000000000000000000 else 0))
      markExpired packet = case packet of
        P.BulkIn _ size | size == 0x24+248 -> do
          count <- readIORef expiredCount
          writeIORef expiredCount (count+1)
          unless (count < 3) (writeIORef expiredJump True)
        _ -> pure ()
  (expiredOutcome,expiredPackets,expiredCleanup) <- runCaseWith markExpired expiredClock (base </> "deadline-expired")
    (stepsFor (stopsAfter (uncurry SliceReq (expectedBankSlices 0x80000 !! 3))) happyOutcome)
  check "an expired aggregate deadline stops the session"
    (either (isInfixOf "deadline" . displayException) (const False) expiredOutcome)
  check "an expired aggregate deadline dispatches no restore and no close"
    (length (filter isWriteFrame expiredPackets) == 6 && null expiredCleanup
      && length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- expiredPackets] == 0)
  expiredJournal <- readFile (base </> "deadline-expired" </> "journal.v1")
  check "the expired deadline is durable" ("session-uncertain" `isInfixOf` expiredJournal)

  -- A restore readback that does not show the restored gate makes the outcome
  -- uncertain while the disarm and the restores stand.
  expectStop (base </> "stop-restore-readback") ("gate restore is not visible", "gate restore is not visible") 0
    (stepsFor (stopsAfterWith [[1],[]] (PollReq 0 0))
      (mutateWith (outcomeFor [[1],[]]) (WindowReq gateReadbackRestored) (P.Reply (windowReply (restoredWindow 1)))))

  removeDirectoryRecursive base
  putStrLn "read-media: bank walks, map resolution, frame-byte, gate-order, capture, stop and bound checks passed"
