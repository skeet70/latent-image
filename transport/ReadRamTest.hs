module Main where

import Control.Exception
import Control.Monad (forM_, unless, when)
import qualified Data.ByteString as B
import Data.IORef
import Data.List (isInfixOf)
import Data.Word (Word8, Word64)
import System.Directory
import System.FilePath
import System.Posix.Files (setFileMode)
import System.Posix.Process (getProcessID)
import FujiAF.Transport
import FujiAF.Routes (RouteWindow(..))
import FujiAF.ReadRam
import FujiAF.PlanDocument (readRoutePlanFile)
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

syntheticDescriptors :: Descriptors
syntheticDescriptors = Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 []

le32 :: Int -> [Word8]
le32 value = [fromIntegral ((value `div` 256^index) `mod` 256) | index <- [0..3::Int]]

u32 :: Int -> [Word8] -> Int
u32 at bytes = sum [fromIntegral byte * 256^index | (index,byte) <- zip [0..3::Int] (take 4 (drop at bytes))]

-- Wire shapes written out from the documented layouts.
sid :: Int
sid = 0x2b61

control :: Word8 -> Int -> Int -> P.Transfer
control op size session = P.ControlWrite 0x43 0 0 0
  [op,0,0,0,0,0,0,0, fromIntegral (size `div` 256), fromIntegral (size `mod` 256), fromIntegral (session `div` 256), fromIntegral (session `mod` 256)]

identityFrame :: [Word8]
identityFrame = [0,0,0x15,0] ++ replicate 12 0 ++ [4,0,0,0] ++ replicate 12 0

window0Frame :: [Word8]
window0Frame = [0x02,0x00,0x1a,0x00] ++ replicate 12 0 ++ [0x02,0,0,0, 0x01,0,0,0, 0,0,0,0, 0,0,0,0, 0x00,0x01,0,0]

ramFrame :: Int -> Int -> [Word8]
ramFrame address size = [0x01,0x00,0x20,0x00] ++ replicate 12 0 ++ [0x02,0,0,0] ++ replicate 8 0 ++ le32 address ++ le32 size

-- The owner's first live plan: the log-store header, the tick cell and the
-- menu frame/cursor window.
samplePlanJson :: String
samplePlanJson = concat
  [ "{\"windows\":[{\"index\":0,\"address\":\"0x400a0800\",\"length\":64,\"purpose\":\"log store header\"}"
  , ",{\"index\":1,\"address\":\"0x400c0800\",\"length\":4,\"purpose\":\"boot tick cell\"}"
  , ",{\"index\":2,\"address\":\"0x009571d0\",\"length\":16,\"purpose\":\"menu frame and cursor\"}]}" ]

sampleWindows :: [RouteWindow]
sampleWindows =
  [ RouteWindow 0 0x400a0800 64 "log store header"
  , RouteWindow 1 0x400c0800 4 "boot tick cell"
  , RouteWindow 2 0x009571d0 16 "menu frame and cursor" ]

data Req = ReqOpen | ReqClose | ReqIdentity | ReqWindow0 | ReqRam Int Int
  deriving (Eq, Show)

-- The synthetic body. The RAM bytes of the n-th read of an address follow
-- the address and n, so passes differ.
data Fake = Fake
  { fakeOpenTail :: [Word8]
  , fakeIdentityText :: [Word8]
  , fakeGate :: Word8
  , fakeLost :: Req -> Int -> Bool
  , fakeStatus :: Req -> (Int,Int)
  }

fake :: Fake
fake = Fake [0,0,0,0,1,0] [88,0,45,0,77,0,49,0] 0x01 (\_ _ -> False) (const (0,0))

ramBytes :: Int -> Int -> Int -> [Word8]
ramBytes address size ordinal = [fromIntegral ((address + offset + 7*ordinal) `mod` 251) | offset <- [0 .. size-1]]

window0Bytes :: Word8 -> [Word8]
window0Bytes gate = [if index == 0xf7 then gate else 0x5a | index <- [0..255::Int]]

data Event = Sent P.Transfer Word64 | Paused Int
  deriving (Eq, Show)

data CleanupEvent = Released | Closed | Exited deriving (Eq, Show)

data Run = Run
  { runOutcome :: Either SomeException Int
  , runEvents :: [Event]
  , runCleanup :: [CleanupEvent]
  , runDurable :: Bool
  }

packets :: Run -> [P.Transfer]
packets run = [packet | Sent packet _ <- runEvents run]

bulkFrames :: Run -> [[Word8]]
bulkFrames run = [frame | P.BulkOut _ frame <- packets run]

ramFrames :: Run -> [[Word8]]
ramFrames run = [frame | frame <- bulkFrames run, take 4 frame == [0x01,0x00,0x20,0x00]]

closes :: Run -> Int
closes run = length [() | P.ControlWrite _ _ _ _ (0xc4:_) <- packets run]

-- Every emitted transfer of every run, for the no-write check.
type Ledger = IORef [P.Transfer]

-- The fake answers by request content. The clock steps 1 ms per transfer and
-- pauses advance it by their own length.
runCase :: Ledger -> Fake -> RamWatch -> FilePath -> IO Run
runCase ledger body watch dir = do
  createDirectoryIfMissing True dir
  pending <- newIORef ReqOpen
  seen <- newIORef ([] :: [Req])
  events <- newIORef []
  cleanup <- newIORef []
  durable <- newIORef True
  clockRef <- newIORef (0 :: Word64)
  let device = Candidate "001:2.3" 0x04cb 0xff80 syntheticDescriptors
      recordDurable bytes = do
        outgoing <- readFile (dir </> "outgoing.v1")
        unless (P.hexBytes bytes `isInfixOf` outgoing) (writeIORef durable False)
      parse frame
        | frame == identityFrame = ReqIdentity
        | frame == window0Frame = ReqWindow0
        | length frame == 36 && u32 0 frame == 0x200001 && u32 0x10 frame == 2 = ReqRam (u32 0x1c frame) (u32 0x20 frame)
        | otherwise = error ("frame outside the read-only set: "++P.hexBytes frame)
      reply request ordinal = case request of
        ReqOpen -> [0x61,0x2b] ++ fakeOpenTail body
        ReqClose -> [0x61,0x2b]
        ReqIdentity -> [0,0,0x15,0] ++ replicate 12 0 ++ fakeIdentityText body ++ replicate (112 - length (fakeIdentityText body)) 0
        ReqWindow0 -> concatMap le32 [0x1a0002,0,0,0,2,1,0,0,256] ++ window0Bytes (fakeGate body)
        ReqRam address size ->
          let (first,second) = fakeStatus body request
          in concatMap le32 [0x200001,first,second,0,2,0,0,address,size] ++ ramBytes address size ordinal
      api = UsbApi (\_ -> pure ()) (\_ -> pure [device]) (\_ _ -> pure ()) (\_ _ -> pure ())
        (\packet _ -> do
          modifyIORef' ledger (++[packet])
          now <- readIORef clockRef
          modifyIORef' events (++[Sent packet now])
          modifyIORef' clockRef (+ 1000000)
          case packet of
            P.ControlWrite 0x43 0 0 0 bytes -> do
              recordDurable bytes
              case bytes of
                0xc0:_ -> writeIORef pending ReqOpen
                0xc4:_ -> writeIORef pending ReqClose
                _ -> pure ()
              pure (12,[])
            P.BulkOut _ frame -> do
              recordDurable frame
              writeIORef pending (parse frame)
              pure (length frame,[])
            P.BulkIn _ count -> do
              request <- readIORef pending
              ordinal <- length . filter (== request) <$> readIORef seen
              modifyIORef' seen (++[request])
              when (fakeLost body request ordinal) (P.reject "injected transport failure")
              let bytes = reply request ordinal
              check ("reply size for "++show request) (length bytes == count)
              pure (count,bytes)
            _ -> error "unexpected transfer")
        (\_ _ -> modifyIORef' cleanup (++[Released]))
        (\_ -> modifyIORef' cleanup (++[Closed]))
        (\_ -> modifyIORef' cleanup (++[Exited]))
      clock = TimedClock (readIORef clockRef) (\ms -> do
        modifyIORef' events (++[Paused ms])
        modifyIORef' clockRef (+ fromIntegral ms * 1000000))
  outcome <- try (runReadRam watch clock api dir)
  Run outcome <$> readIORef events <*> readIORef cleanup <*> readIORef durable

watchOf :: [RouteWindow] -> Pacing -> RamWatch
watchOf windows pacing = either error id (mkRamWatch windows pacing)

paced :: Int -> Int -> Pacing
paced passes interval = either error id (mkPacing passes interval)

isLeft' :: Either a b -> Bool
isLeft' = either (const True) (const False)

expectStop :: String -> Run -> String -> Int -> IO ()
expectStop label run fragment ramSent = do
  check (label++": session must stop") (isLeft' (runOutcome run))
  check (label++": stop message "++either displayException show (runOutcome run))
    (either (isInfixOf fragment . displayException) (const False) (runOutcome run))
  check (label++": "++show (length (ramFrames run))++" RAM requests sent, expected "++show ramSent)
    (length (ramFrames run) == ramSent)
  check (label++": exactly one protocol close") (closes run == 1)
  check (label++": every host resource released once") (runCleanup run == [Released,Closed,Exited])

main :: IO ()
main = do
  pid <- getProcessID
  tmp <- getTemporaryDirectory
  let root = tmp </> ("fuji-af-read-ram-tests-"++show pid)
  createDirectory root
  ledger <- newIORef []

  -- Pacing and plan caps.
  check "600 passes at 100 ms is allowed" (not (isLeft' (mkPacing 600 100)))
  check "10000 ms is allowed" (not (isLeft' (mkPacing 2 10000)))
  check "601 passes is refused" (isLeft' (mkPacing 601 100))
  check "zero passes is refused" (isLeft' (mkPacing 0 100))
  check "99 ms is refused" (isLeft' (mkPacing 2 99))
  check "10001 ms is refused" (isLeft' (mkPacing 2 10001))
  check "three windows at 600 passes fit the request cap" (not (isLeft' (mkRamWatch sampleWindows (paced 600 100))))
  check "four windows at 600 passes exceed the request cap"
    (isLeft' (mkRamWatch (sampleWindows ++ [RouteWindow 3 0x00a0f86c 4 "cursor"]) (paced 600 100)))
  check "a 248-byte window is one request" (not (isLeft' (mkRamWatch [RouteWindow 0 0x0096bd20 248 "table"] singlePass)))
  check "a 249-byte window is refused" (isLeft' (mkRamWatch [RouteWindow 0 0x0096bd20 249 "table"] singlePass))
  check "a 256-byte window is refused" (isLeft' (mkRamWatch [RouteWindow 0 0x0096bd20 256 "table"] singlePass))
  check "an empty plan is refused" (isLeft' (mkRamWatch [] singlePass))
  check "a repeated address is refused"
    (isLeft' (mkRamWatch [RouteWindow 0 0x400c0800 4 "tick", RouteWindow 1 0x400c0800 4 "tick"] singlePass))
  check "a window past the address space is refused" (isLeft' (mkRamWatch [RouteWindow 0 0xfffffffe 4 "edge"] singlePass))
  check "a peripheral window is refused" (isLeft' (mkRamWatch [RouteWindow 0 0xfff6a000 4 "mmio"] singlePass))
  check "a boot-ROM window is refused" (isLeft' (mkRamWatch [RouteWindow 0 0x80000000 4 "rom"] singlePass))
  check "a window crossing the end of DRAM is refused" (isLeft' (mkRamWatch [RouteWindow 0 0x1ffffffc 8 "edge"] singlePass))
  check "a window in the gap between DRAM and its alias is refused" (isLeft' (mkRamWatch [RouteWindow 0 0x30000000 4 "gap"] singlePass))
  check "the last word of the DRAM alias is allowed" (not (isLeft' (mkRamWatch [RouteWindow 0 0x5ffffffc 4 "alias"] singlePass)))
  check "read-ram refuses each SD host register window"
    (all (\window -> isLeft' (mkRamWatch [window] singlePass)) sdHostWindows)
  check "no SD host window touches the +0x20 data port"
    (all (\window -> routeWindowAddress window + routeWindowBytes window <= 0xfff50020 || routeWindowAddress window >= 0xfff50024) sdHostWindows)

  -- The CLI boundary.
  check "plan alone" (readRamArguments ["/p.json"] == Just ("/p.json",Nothing,Nothing))
  check "plan and OUTDIR" (readRamArguments ["/p.json","/out"] == Just ("/p.json",Nothing,Just "/out"))
  check "plan, pacing and OUTDIR"
    (readRamArguments ["/p.json","--repeat","600","--interval-ms","100","/out"] == Just ("/p.json",Just (600,100),Just "/out"))
  check "pacing without OUTDIR" (readRamArguments ["/p.json","--repeat","3","--interval-ms","250"] == Just ("/p.json",Just (3,250),Nothing))
  forM_ [ [], ["--repeat","3"], ["/p.json","--repeat","3"], ["/p.json","--interval-ms","250","--repeat","3"]
        , ["/p.json","--repeat","-3","--interval-ms","250"], ["/p.json","--repeat","0x3","--interval-ms","250"]
        , ["/p.json","--repeat","9999999","--interval-ms","250"], ["/p.json","/a","/b"], ["/p.json","--verbose"] ] $ \tokens ->
    check ("arguments refused: "++show tokens) (readRamArguments tokens == Nothing)

  -- The sample plan file reads back as the route-plan window list.
  let planPath = root </> "read-ram-first.json"
  writeFile planPath samplePlanJson
  setFileMode planPath 0o600
  planWindows <- readRoutePlanFile planPath
  check ("the sample plan reads back: "++show planWindows) (planWindows == sampleWindows)

  -- Happy single pass: exactly these transfers.
  let happyWatch = watchOf sampleWindows singlePass
      happyDir = root </> "happy"
  happy <- runCase ledger fake happyWatch happyDir
  check ("happy session completes three requests: "++either displayException show (runOutcome happy))
    (either (const False) (== 3) (runOutcome happy))
  let ramExchange address size =
        [ control 0xd0 0x24 sid, P.BulkOut 2 (ramFrame address size), control 0xd1 (0x24+size) sid, P.BulkIn 0x81 (0x24+size) ]
  check "the happy session's transfers are exactly C0, identity, window 0, three reads, C4"
    (packets happy ==
      [ control 0xc0 8 0, P.BulkIn 0x81 8
      , control 0xd0 0x20 sid, P.BulkOut 2 identityFrame, control 0xd1 0x80 sid, P.BulkIn 0x81 0x80
      , control 0xd0 0x24 sid, P.BulkOut 2 window0Frame, control 0xd1 0x124 sid, P.BulkIn 0x81 0x124 ]
      ++ ramExchange 0x400a0800 64 ++ ramExchange 0x400c0800 4 ++ ramExchange 0x009571d0 16
      ++ [ control 0xc4 2 sid, P.BulkIn 0x81 2 ])
  check "the happy session never pauses" (null [() | Paused _ <- runEvents happy])
  check "every outgoing frame was durable before its dispatch" (runDurable happy)
  check "happy session releases every resource once" (runCleanup happy == [Released,Closed,Exited])

  -- probe-sd-host: the same session over the fixed SD host register windows.
  sdHost <- runCase ledger fake (either error id (sdHostWatch singlePass)) (root </> "sd-host")
  check ("probe-sd-host completes four requests: "++either displayException show (runOutcome sdHost))
    (either (const False) (== 4) (runOutcome sdHost))
  check "probe-sd-host reads +0x00 (32), +0x24 (6), +0x2c (16) and +0x3e (2) of 0xfff50000, in that order"
    (ramFrames sdHost == [ramFrame 0xfff50000 32, ramFrame 0xfff50024 6, ramFrame 0xfff5002c 16, ramFrame 0xfff5003e 2])
  sdHostJournal <- readFile (root </> "sd-host" </> "journal.v1")
  check "probe-sd-host journals its own operation name"
    ("probe-sd-host;windows=4" `isInfixOf` sdHostJournal && "probe-sd-host-complete-stock-unverified" `isInfixOf` sdHostJournal)
  check "probe-sd-host fits 508 passes and refuses 509" (not (isLeft' (sdHostWatch (paced 508 100))) && isLeft' (sdHostWatch (paced 509 100)))
  sdHostTwice <- runCase ledger fake (either error id (sdHostWatch (paced 2 250))) (root </> "sd-host-twice")
  check "probe-sd-host repeats its four windows per pass with the pause between passes"
    (ramFrames sdHostTwice == concat (replicate 2 [ramFrame 0xfff50000 32, ramFrame 0xfff50024 6, ramFrame 0xfff5002c 16, ramFrame 0xfff5003e 2])
     && [ms | Paused ms <- runEvents sdHostTwice] == [250])
  happyJournal <- readFile (happyDir </> "journal.v1")
  forM_ sampleWindows $ \window -> do
    let address = routeWindowAddress window
        expected = ramBytes address (routeWindowBytes window) 0
    capture <- B.readFile (happyDir </> "ram" </> (ramCaptureName 0 address++".bin"))
    check ("capture of "++show address) (B.unpack capture == expected)
    check ("journaled hash of "++show address) (sha256 (B.pack expected) `isInfixOf` happyJournal)
  config <- B.readFile (happyDir </> "config-000000.bin")
  check "config window 0 is captured" (B.unpack config == window0Bytes 0x01)
  check "the gate value is journaled" ("f7=0x01" `isInfixOf` happyJournal)
  check "the completion record names the count and no write"
    ("read-ram-complete-stock-unverified" `isInfixOf` happyJournal && "3 read requests;passes=1;no write sent" `isInfixOf` happyJournal)
  check "the journal stays inside its cap" (length happyJournal <= readRamJournalCapBytes happyWatch)

  -- Any stored nonzero gate value is accepted.
  gateTwo <- runCase ledger fake { fakeGate = 0x02 } happyWatch (root </> "gate-two")
  check "0xf7 = 0x02 opens the route" (either (const False) (== 3) (runOutcome gateTwo))

  -- Refusals before any RAM request.
  closedGate <- runCase ledger fake { fakeGate = 0x00 } happyWatch (root </> "f7-zero")
  expectStop "f7 = 0" closedGate "0xf7 reads 0x00" 0
  check "f7 = 0 still read window 0" (window0Frame `elem` bulkFrames closedGate)
  identity <- runCase ledger fake { fakeIdentityText = [88,0,45,0,69,0,49,0] } happyWatch (root </> "identity")
  expectStop "wrong identity" identity "X-M1" 0
  check "wrong identity sends no config read" (bulkFrames identity == [identityFrame])
  openTail <- runCase ledger fake { fakeOpenTail = [0,0,0,0,2,0] } happyWatch (root </> "open-tail")
  expectStop "C0 tail" openTail "C0 reply tail" 0
  check "a wrong C0 tail sends no D0 frame" (null (bulkFrames openTail))

  -- A lost reply stops the session with no retry.
  lost <- runCase ledger fake { fakeLost = \request _ -> request == ReqRam 0x400c0800 4 } happyWatch (root </> "lost")
  expectStop "lost reply" lost "injected transport failure" 2
  check "the lost request was sent once and nothing followed it"
    (ramFrames lost == [ramFrame 0x400a0800 64, ramFrame 0x400c0800 4])
  rejected <- runCase ledger fake { fakeStatus = \request -> if request == ReqRam 0x400a0800 64 then (1,0xffff) else (0,0) }
    happyWatch (root </> "closed-gate-status")
  expectStop "closed-gate status" rejected "closed gate" 1

  -- Repeat mode: passes separated by exactly the interval, nothing else paced.
  let watchedWindows = [RouteWindow 0 0x400c0800 4 "boot tick cell", RouteWindow 1 0x009571d0 16 "menu frame and cursor"]
      repeatWatch = watchOf watchedWindows (paced 3 250)
      repeatDir = root </> "repeat"
  repeated <- runCase ledger fake repeatWatch repeatDir
  check ("repeat session completes six requests: "++either displayException show (runOutcome repeated))
    (either (const False) (== 6) (runOutcome repeated))
  let onePass = [Left (ramFrame 0x400c0800 4), Left (ramFrame 0x009571d0 16)]
      shape = [token | event <- runEvents repeated, Just token <- [classify event]]
      classify (Sent (P.BulkOut _ frame) _) | take 4 frame == [0x01,0x00,0x20,0x00] = Just (Left frame)
      classify (Paused ms) = Just (Right ms)
      classify _ = Nothing
  check ("three passes, 250 ms apart: "++show shape)
    (shape == onePass ++ [Right 250] ++ onePass ++ [Right 250] ++ onePass)
  let ramTimes = [time | Sent (P.BulkOut _ frame) time <- runEvents repeated, take 4 frame == [0x01,0x00,0x20,0x00]]
  check ("each pass starts at least 250 ms after the previous pass's last request: "++show ramTimes)
    (and [ next - previous >= 250000000 | (previous,next) <- [(ramTimes!!1,ramTimes!!2),(ramTimes!!3,ramTimes!!4)] ])
  forM_ [0..2] $ \pass -> forM_ watchedWindows $ \window -> do
    let address = routeWindowAddress window
    capture <- B.readFile (repeatDir </> "ram" </> (ramCaptureName pass address++".bin"))
    check ("pass "++show pass++" capture of "++show address) (B.unpack capture == ramBytes address (routeWindowBytes window) pass)
  repeatJournal <- readFile (repeatDir </> "journal.v1")
  check "the repeat journal records each pass start"
    (all (\pass -> ("pass="++show pass++";clock-ns=") `isInfixOf` repeatJournal) [0,1,2::Int])

  -- No run ever emitted a write: every bulk OUT is the identity request, the
  -- window-0 config read or a 0x200001 function 2 read, and every control is
  -- C0, D0, D1 or C4.
  everything <- readIORef ledger
  check "no write frame in any run"
    (all (\frame -> frame == identityFrame || frame == window0Frame
                    || (length frame == 36 && u32 0 frame == 0x200001 && u32 0x10 frame == 2))
         [frame | P.BulkOut _ frame <- everything])
  check "no control other than C0, D0, D1 and C4 in any run"
    (all (`elem` [0xc0,0xd0,0xd1,0xc4]) [op | P.ControlWrite _ _ _ _ (op:_) <- everything])

  removeDirectoryRecursive root
  putStrLn "read-ram: frame, cap, refusal, lost-reply, pacing and no-write checks passed"
