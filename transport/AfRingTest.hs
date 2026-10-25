{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Exception (bracket, throwIO, try)
import Control.Monad (forM_, unless)
import Data.Aeson (Value(..), eitherDecodeStrict')
import qualified Data.Aeson.KeyMap as M
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import Data.Foldable (toList)
import Data.IORef
import Data.Word (Word8, Word32, Word64)
import System.Directory (doesPathExist, removeDirectoryRecursive)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))
import System.Posix.Files (setFileMode)
import System.Posix.Temp (mkdtemp)
import System.Process (readProcessWithExitCode)
import FujiAF.AfRing
import qualified FujiAF.Broker as P
import FujiAF.RamPush
import FujiAF.ReadBulk (bulkWindowAddress, bulkWindowBytes)
import FujiAF.Transport

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

le32 :: Word32 -> [Word8]
le32 value = [fromIntegral (value `shiftR` bits) | bits <- [0,8,16,24]]

-- Original synthetic fixtures encode the ring v1 layout; no camera bytes are used.
header :: Word32 -> Word32 -> [Word8]
header total overwritten = concatMap le32
  ([0x31524641,1,64,36,256,total,overwritten,0,0x4003] ++ replicate 7 0)

event :: Word32 -> Word32 -> [Word8]
event sequenceNumber kind = concatMap le32
  [sequenceNumber,kind,0x01234567,0x89abcdef,0x10203040,0x50607080,0xa1b2c3d4,0xe5f60718,0x4003]

pristine, normal :: B.ByteString
pristine = B.pack (header 0 0 ++ replicate 9216 0)
normal = B.pack (header 4 0 ++ concat [event 1 1,event 2 2,event 3 3,event 4 0x80000001] ++ replicate (252*36) 0)

wrapped :: Word32 -> B.ByteString
wrapped total = B.pack (header total (total-256) ++ concat
  [event sequenceNumber 0x80000001 | slot <- [0..255], let sequenceNumber = latestForSlot slot])
  where
    latestForSlot :: Integer -> Word32
    latestForSlot slot = fromInteger (toInteger total - ((toInteger total - 1 - slot) `mod` 256))

replaceWord :: Int -> Word32 -> B.ByteString -> B.ByteString
replaceWord at value bytes = B.take at bytes <> B.pack (le32 value) <> B.drop (at+4) bytes

capture :: B.ByteString -> IO AfRingCapture
capture = either throwIO pure . decodeAfRing

base :: Address
base = Address 0x00c40000

allocation :: RamAllocState
allocation = RamAllocState base 0x6800 0x4003 callbackTableAddress callbackStockWord bootstrapBytes (replicate 64 '0') "X-M1" 1

manifest :: RamPushManifest
manifest = RamPushManifest "synthetic.json" "synthetic.bin" (replicate 64 '0') [] base 0x6800 (replicate 64 '0') B.empty
  (RamPushEntries (Address 0x00c41600) (Address 0x00c41834) (Address 0x00c41860)) []

installed :: RamPushState
installed = RamPushState (replicate 64 '0') (replicate 64 '0') base 0x6800 (manifestEntries manifest) [] callbackStockWord bootstrapBytes 1

-- A synthetic USB peer supplies the fixed read-bulk session's replies.
peer :: B.ByteString -> IO (UsbApi, IO [P.Transfer])
peer ring = do
  seen <- newIORef []
  replies <- newIORef
    [ [0x61,0x2b,0,0,0,0,1,0]
    , [0,0,0x15,0] ++ replicate 12 0 ++ [88,0,45,0,77,0,49,0] ++ replicate 104 0
    , concatMap le32 [0x1a0002,0,0,0,2,1,0,0,256] ++ replicate 247 0 ++ [1] ++ replicate 8 0
    , concatMap le32 [0x450006,0,0,0,0,0x100000,0,0]
    , concatMap le32 [0x200001,0,0,0,2,0,0,0x01469878,8,0x4b2ece00,0x100000]
    , B.unpack ring
    , [0,0]
    ]
  let transfer packet _ = do
        modifyIORef' seen (packet:)
        case packet of
          P.ControlWrite _ _ _ _ bytes -> pure (length bytes,[])
          P.BulkOut _ bytes -> pure (length bytes,[])
          P.BulkIn _ count -> do
            response <- atomicModifyIORef' replies (\queuedReplies -> case queuedReplies of [] -> ([],[]); first:rest -> (rest,first))
            pure (min count (length response), take count response)
      api = UsbApi (\_ -> pure ())
        (\_ -> pure [Candidate "synthetic" 0x04cb 0xff80 (Descriptors 1 0 0 255 0 0 0x81 2 512 512 0 1 [])])
        (\_ _ -> pure ()) (\_ _ -> pure ()) transfer (\_ _ -> pure ()) (\_ -> pure ()) (\_ -> pure ())
  pure (api,reverse <$> readIORef seen)

main :: IO ()
main = do
  empty <- capture pristine
  check "pristine ring has no committed or overwritten events" (null (ringEvents empty)
    && unEventCount (ringCommittedTotal empty) == 0 && unEventCount (ringOverwritten empty) == 0)
  observed <- capture normal
  check "normal ring preserves sequence and kind" (map (unEventSequence . eventSequence) (ringEvents observed) == [1,2,3,4]
    && map eventKind (ringEvents observed) == [DirectionDecision,PeakDecision,AcceptedStatistic,Synthetic])
  check "every committed event identifies the CPU0 writer" (map eventCore (ringEvents observed) == replicate 4 AfCpu0)
  check "normal capture reports no contention loss" (ringContentionStatus observed == NoContentionLoss && not (ringContentionLoss observed))
  forM_ [pristine,normal,wrapped 300,wrapped maxBound] $ \original -> do
    beforeLoss <- capture original
    withLoss <- capture (replaceWord 36 1 original)
    check "contention loss preserves decodable committed events"
      (ringEvents withLoss == ringEvents beforeLoss && ringCommittedTotal withLoss == ringCommittedTotal beforeLoss
        && ringOverwritten withLoss == ringOverwritten beforeLoss)
    check "contention loss remains explicit" (ringContentionStatus withLoss == ContentionLossDetected && ringContentionLoss withLoss)
    case afRingValue withLoss of
      Object value -> check "JSON preserves contention loss" (M.lookup "contention_loss" value == Just (Bool True))
      _ -> error "capture JSON must be an object"
  forM_ (ringEvents observed) $ \entry -> check "little-endian register and snapshot words survive decoding"
    (map unObservedWord [eventR0 entry,eventR1 entry,eventR2 entry,eventR3 entry,eventSearchState entry,eventFocusStatistic entry]
      == [0x01234567,0x89abcdef,0x10203040,0x50607080,0xa1b2c3d4,0xe5f60718])
  forM_ [256,257,300,511,512,513,maxBound-1,maxBound] $ \total -> do
    overflow <- capture (wrapped total)
    check "wrapped ring retains oldest-to-newest tail" (map (toInteger . unEventSequence . eventSequence) (ringEvents overflow)
      == [toInteger total-255 .. toInteger total])
    check "overflow loss is reported" (unEventCount (ringOverwritten overflow) == total-256)
  case afRingValue observed of
    Object value -> do
      check "JSON labels capture best-effort" (M.lookup "capture_semantics" value == Just (String "best-effort capture; not a coherent snapshot"))
      check "JSON makes no timestamp claim" (M.lookup "timestamp_available" value == Just (Bool False))
      check "JSON publishes committed count" (M.lookup "committed_total" value == Just (Number 4))
      check "JSON reports no contention loss" (M.lookup "contention_loss" value == Just (Bool False))
      check "JSON publishes 36-byte events" (M.lookup "event_bytes" value == Just (Number 36))
    _ -> error "capture JSON must be an object"
  saturated <- capture (wrapped maxBound)
  case afRingValue saturated of
    Object value -> check "JSON detects overflow and saturation" (M.lookup "overflowed" value == Just (Bool True)
      && M.lookup "saturated" value == Just (Bool True) && M.lookup "overwritten" value == Just (Number 4294967039))
    _ -> error "capture JSON must be an object"
  let reject label bytes predicate = check label (case decodeAfRing bytes of Left failure -> predicate failure; Right _ -> False)
      malformed _ = True
  reject "active writer refused" (replaceWord 28 1 normal) (== ActiveWriter)
  forM_ [(0,0),(4,2),(8,32),(12,32),(12,64),(16,255),(28,2),(32,0x4002),(36,2),(36,maxBound),(40,1),(60,1)] $ \(at,value) ->
    reject "malformed header refused" (replaceWord at value normal) (\failure -> case failure of HeaderMismatch _ _ -> True; _ -> False)
  forM_ [B.empty,B.take 63 pristine,B.take 9279 pristine,pristine <> B.singleton 0] $ \bytes ->
    reject "exact extent enforced" bytes (\failure -> case failure of RingExtentMismatch _ -> True; _ -> False)
  reject "wrong endian magic refused" (B.pack [0x31,0x52,0x46,0x41] <> B.drop 4 pristine) malformed
  reject "future slot in mixed capture refused" (replaceWord 64 257 normal)
    (\failure -> case failure of SlotSequenceMismatch _ _ _ -> True; _ -> False)
  reject "stale slot after wrap refused" (replaceWord 64 1 (wrapped 300))
    (\failure -> case failure of SlotSequenceMismatch _ _ _ -> True; _ -> False)
  reject "nonzero unused event refused" (replaceWord (64+4*36) 5 normal)
    (\failure -> case failure of NonzeroUnusedSlot _ -> True; _ -> False)
  reject "unknown event kind refused" (replaceWord 68 4 normal)
    (\failure -> case failure of UnknownEventKind _ _ -> True; _ -> False)
  forM_ [0,0x4002,0x4007] $ \wrongCore ->
    forM_ [64+32,64+3*36+32] $ \at ->
      reject "committed event with another core identity refused" (replaceWord at wrongCore normal)
        (\failure -> case failure of UnexpectedEventCore _ _ -> True; _ -> False)
  forM_ [replaceWord 24 1 normal, replaceWord 24 43 (wrapped 300), replaceWord 24 45 (wrapped 300), replaceWord 24 maxBound (wrapped maxBound)] $ \bytes ->
    reject "inconsistent overwritten count refused" bytes (\failure -> case failure of OverwrittenMismatch _ _ -> True; _ -> False)
  check "AF payload registration resolves allocation kind" (fmap payloadKind (payloadOfSize 0x6800) == Just AfRing)
  check "AF payload has fixed installer and ABI" (payloadInstallerStart afRingPayload 0x00c40000 == 0x00c41000
    && payloadInstallerStart afRingPayload 0x00c40008 == 0x00c41020
    && payloadEntries afRingPayload base == manifestEntries manifest && payloadAbiVersion afRingPayload == 5
    && payloadSetupCount afRingPayload == 3 && payloadActivationCount afRingPayload == 3)
  forM_ [AfRingManifest manifest,AfRingAllocation allocation,AfRingState installed] $ \source -> do
    window <- either throwIO pure (afRingWindow source)
    check "reader derives fixed ring from allocation base" (bulkWindowAddress window == 0x00c44000 && bulkWindowBytes window == 9280)
  forM_ [ AfRingAllocation allocation { allocSize = reviewedAllocationSize }
        , AfRingManifest manifest { allocationSize = 0x6808 }
        , AfRingAllocation allocation { allocCpu0 = 0x4002 }
        , AfRingAllocation allocation { allocBase = Address 0xffffc000 }
        , AfRingAllocation allocation { allocBase = Address 0x00c40004 }
        , AfRingAllocation allocation { allocBase = Address 0xfff50000 }
        , AfRingManifest manifest { manifestEntries = payloadEntries bankD3Payload base }
        , AfRingState installed { stateSize = 0x1560 }
        ] $ \source -> check "invalid reader source refused" (case afRingWindow source of Left _ -> True; Right _ -> False)
  bracket (mkdtemp "/dev/shm/fuji-af-ring-tests-XXXXXX") removeDirectoryRecursive $ \root -> do
    setFileMode root 0o700
    (api,transfers) <- peer normal
    tick <- newIORef (0 :: Word64)
    let clock = TimedClock (atomicModifyIORef' tick (\n -> (n+1000,n))) (\_ -> pure ())
    readBack <- readAfRing (AfRingManifest manifest) clock api root
    check "reader decodes actual bulk capture" (readBack == observed)
    saved <- B.readFile (root </> "ram-00c44000-00002440.bin")
    check "reader retains raw ring capture" (saved == normal)
    sent <- transfers
    let e1s = [bytes | P.ControlWrite _ _ _ _ bytes@(0xe1:_) <- sent]
    check "reader sends only the derived fixed ring bulk read"
      (e1s == [[0xe1,0,0xb5,0x95,0x72,0,0,0,0x24,0x40,0x2b,0x61]])
    bad <- try (readAfRing (AfRingAllocation allocation { allocSize = 1 }) clock api root) :: IO (Either AfRingReadError AfRingCapture)
    after <- transfers
    check "invalid allocation refused before USB" (case bad of Left (NotAfRingAllocation 1) -> after == sent; _ -> False)
    arguments <- getArgs
    (camera,decoder) <- case arguments of [cameraPath,decoderPath] -> pure (cameraPath,decoderPath); _ -> error "expected camera and offline decoder paths"
    statePath <- writeAllocStateFile root allocation
    retained <- readRamAllocState statePath
    check "live refusal fixture is a valid AF allocation state" (retained == allocation)
    let refusedOutput = root </> "live-refused"
    (liveExit,_,_) <- readProcessWithExitCode camera ["ram-relink",statePath,refusedOutput] ""
    outputExists <- doesPathExist refusedOutput
    check "live CLI refuses AF ring before relinking" (liveExit /= ExitSuccess && not outputExists)
    let jsonOutput = root </> "af-ring.json"
    (decodeExit,_,_) <- readProcessWithExitCode decoder ["decode",root </> "ram-00c44000-00002440.bin",jsonOutput] ""
    check "offline CLI accepts valid ring" (decodeExit == ExitSuccess)
    json <- B.readFile jsonOutput
    check "offline CLI writes decoded JSON" ((eitherDecodeStrict' json :: Either String Value) == Right (afRingValue observed))
    let invalidInput = root </> "invalid.bin"
        invalidOutput = root </> "invalid.json"
    B.writeFile invalidInput (replaceWord 0 0 normal)
    (invalidExit,_,_) <- readProcessWithExitCode decoder ["decode",invalidInput,invalidOutput] ""
    invalidExists <- doesPathExist invalidOutput
    check "offline CLI refuses malformed ring without output" (invalidExit /= ExitSuccess && not invalidExists)
  check "JSON event exposes hook and synthetic distinction" (case afRingValue observed of
    Object value -> case M.lookup "events" value of
      Just (Array events) -> [M.lookup "hook_address" fields | Object fields <- toList events]
        == [Just (Number 0x00f41c48),Just (Number 0x00f42abc),Just (Number 0x00f44fec),Just Null]
      _ -> False
    _ -> False)
  check "JSON events identify CPU0" (case afRingValue observed of
    Object value -> case M.lookup "events" value of
      Just (Array events) -> [(M.lookup "core" fields,M.lookup "core_identity" fields) | Object fields <- toList events]
        == replicate 4 (Just (String "cpu0"),Just (Number 0x4003))
      _ -> False
    _ -> False)
