{-# LANGUAGE PatternSynonyms #-}
-- Device selection, the injected USB API, the session journal and the bounded
-- transfer loop every camera operation shares.
module FujiAF.Transport where

import Control.Concurrent (threadDelay)
import Control.Exception
import Control.Monad (unless, when)
import Data.Bits ((.&.))
import qualified Crypto.Hash.SHA256 as H
import qualified Data.ByteString as B
import Data.IORef
import Data.Word (Word8, Word64)
import Foreign.Ptr (castPtr)
import GHC.Clock (getMonotonicTimeNSec)
import System.Directory hiding (isSymbolicLink)
import System.FilePath
import System.Posix.Files
import System.Posix.IO
import System.Posix.User (getEffectiveUserID)
import System.Posix.Unistd (fileSynchronise)
import qualified FujiAF.Broker as P

data Descriptors = Descriptors
  { configuration :: Int, interfaceNumber :: Int, alternateSetting :: Int
  , interfaceClass :: Int, interfaceSubclass :: Int, interfaceProtocol :: Int
  , inputEndpoint :: Int, outputEndpoint :: Int, inputPacketSize :: Int, outputPacketSize :: Int
  , inputInterval :: Int, outputInterval :: Int
  , extraEndpoints :: [(Int,Int,Int,Int)]
  } deriving (Eq, Show, Read)

-- physicalPath is the provider's locator for a listed device.
data Candidate = Candidate { physicalPath :: String, vid :: Int, pid :: Int, descriptors :: Descriptors }
  deriving (Eq, Show, Read)

-- init/list/open/claim/transfer/release/close/exit are injected as one API.
-- Synthetic tests never call native_usb.c. Each stage receives remaining
-- milliseconds. A transfer reports a provider failure as ProviderFailure.
data UsbApi = UsbApi
  { usbInit :: Int -> IO (), usbList :: Int -> IO [Candidate]
  , usbOpen :: Candidate -> Int -> IO (), usbClaim :: Int -> Int -> IO ()
  , usbTransfer :: P.Transfer -> Int -> IO (Int, [Word8])
  , usbRelease :: Int -> Int -> IO (), usbClose :: Int -> IO (), usbExit :: Int -> IO ()
  }

-- pauseMs waits that many milliseconds of this clock's time.
data Clock = TimedClock { nowNS :: IO Word64, pauseMs :: Int -> IO () }

-- A clock read from the given source that pauses in real time.
pattern Clock :: IO Word64 -> Clock
pattern Clock now <- TimedClock now _
  where Clock now = TimedClock now (\ms -> threadDelay (ms*1000))

realClock :: Clock
realClock = Clock getMonotonicTimeNSec

data DeviceRefusal
  = NoEligibleDevice
  | MultipleEligibleDevices Int
  | InventoryCapExceeded Int
  | UnsupportedProfile Descriptors
  deriving (Eq, Show)

refusalMessage :: DeviceRefusal -> String
refusalMessage NoEligibleDevice = "no connected 04cb:ff80 device"
refusalMessage (MultipleEligibleDevices count) = show count++" eligible 04cb:ff80 devices; disconnect all but one"
refusalMessage (InventoryCapExceeded count) = "device inventory of "++show count++" exceeds the cap"
refusalMessage (UnsupportedProfile d) = "unsupported descriptor profile: "++show d

-- Interface 0 alternate 0 with one bulk IN and one bulk OUT. Endpoint numbers
-- and packet sizes come from the live descriptors; no value is expected.
supportedProfile :: Descriptors -> Bool
supportedProfile d =
  configuration d > 0 && configuration d < 256
  && interfaceNumber d == 0 && alternateSetting d == 0
  && all (\n -> n >= 0 && n <= 255) [interfaceClass d,interfaceSubclass d,interfaceProtocol d]
  && inputEndpoint d >= 0x81 && inputEndpoint d <= 0x8f
  && outputEndpoint d >= 1 && outputEndpoint d <= 15
  && all (\n -> n > 0 && n <= 1024) [inputPacketSize d,outputPacketSize d]
  && all (\n -> n >= 0 && n <= 255) [inputInterval d,outputInterval d]
  && length (extraEndpoints d) <= 6
  && all (\(a,t,s,i) -> a >= 1 && a <= 255 && a .&. 0x0f /= 0 && a /= inputEndpoint d && a /= outputEndpoint d
                        && t >= 0 && t <= 255 && t .&. 3 /= 2 && s >= 1 && s <= 65535 && i >= 0 && i <= 255)
         (extraEndpoints d)

selectDevice :: [Candidate] -> Either DeviceRefusal Candidate
selectDevice devices
  | length devices > 64 = Left (InventoryCapExceeded (length devices))
  | otherwise = case filter (\d -> vid d == 0x04cb && pid d == 0xff80) devices of
      [] -> Left NoEligibleDevice
      [d] | supportedProfile (descriptors d) -> Right d
          | otherwise -> Left (UnsupportedProfile (descriptors d))
      eligible -> Left (MultipleEligibleDevices (length eligible))

-- The budget of every transfer but a held reply.
transferTimeoutMs :: Int
transferTimeoutMs = 5000

-- The native provider refuses a larger budget; one transfer never waits
-- longer. Only commit-f7's trigger reply uses more than transferTimeoutMs.
transferCeilingMs :: Int
transferCeilingMs = 33000

-- native_usb.c refuses a larger control or bulk OUT transfer.
transferCeilingBytes :: Int
transferCeilingBytes = 292

-- native_usb.c refuses a larger bulk IN: one read-bulk chunk.
bulkInCeilingBytes :: Int
bulkInCeilingBytes = 0x100000

transferLength :: P.Transfer -> Int
transferLength (P.ControlWrite _ _ _ _ bytes) = length bytes
transferLength (P.BulkOut _ bytes) = length bytes
transferLength (P.BulkIn _ count) = count

-- The shapes native_usb.c sends: the twelve-byte vendor control write, bulk
-- OUT of 1..transferCeilingBytes and bulk IN of 1..bulkInCeilingBytes.
supportedTransfer :: P.Transfer -> Bool
supportedTransfer packet = case packet of
  P.ControlWrite 0x43 0 0 0 bytes -> length bytes == 12
  P.ControlWrite {} -> False
  P.BulkOut _ bytes -> length bytes >= 1 && length bytes <= transferCeilingBytes
  P.BulkIn _ count -> count >= 1 && count <= bulkInCeilingBytes

-- A provider transfer that ended with a nonzero status or a count outside its
-- packet: the status, the count and the bytes read.
data ProviderFailure = ProviderFailure Int Int [Word8] deriving (Show)
instance Exception ProviderFailure

-- Which provider outcomes a session journals as native-outcome lines. dump-ram
-- pre-announces its frame sequence and journals one summary line per request,
-- so it journals only a failing or short outcome.
data OutcomeJournal = EveryOutcome | FailingOutcomes deriving (Eq, Show)

remaining :: Clock -> Word64 -> IO Int
remaining clock deadline = do
  tick <- nowNS clock
  when (tick >= deadline) (P.reject "aggregate deadline expired")
  let ms = fromIntegral ((deadline-tick) `div` 1000000)
  if ms < 1 then P.reject "no whole millisecond remains" else pure ms

recordWithin :: Int -> FilePath -> String -> String -> IO ()
recordWithin cap dir kind body = do
  tick <- getMonotonicTimeNSec
  recordBatchWithin cap dir kind [(show tick, body)]

journalLine :: String -> String -> String -> String
journalLine tick kind body = show (1::Int,"camera"::String,tick,kind,body)++"\n"

nativeOutcomeBody :: Int -> Int -> [Word8] -> String
nativeOutcomeBody result count bytes = show (result,count,P.hexBytes bytes)

-- The native-outcome body of one transfer: status, count and the reply bytes
-- as hex, or their SHA-256 for a bulk IN above transferCeilingBytes.
nativeOutcomeFor :: P.Transfer -> Int -> Int -> [Word8] -> String
nativeOutcomeFor packet result count bytes = case packet of
  P.BulkIn _ n | n > transferCeilingBytes ->
    show (result,count,"sha256="++P.hexBytes (B.unpack (H.hash (B.pack bytes))))
  _ -> nativeOutcomeBody result count bytes

-- The tick of a journal line at its widest.
widestJournalTick :: String
widestJournalTick = show (maxBound :: Word64)

-- journal.v1 bytes of one bounded stage that completes.
stageJournalBytes :: String -> Int
stageJournalBytes label =
  length (journalLine widestJournalTick "stage-intent" label) + length (journalLine widestJournalTick "stage-complete" label)

-- journal.v1 bytes of one transfer that completes through tx in a session
-- journaling every outcome. A packet of 0xff bytes bounds every packet of its
-- length. A transfer adds fewer bytes to outgoing.v1 than to journal.v1.
txJournalBytes :: P.Transfer -> Int
txJournalBytes packet =
  stageJournalBytes ("dispatch "++show packet)
  + length (journalLine widestJournalTick "native-outcome" (nativeOutcomeBody 0 count reply))
  + length (journalLine widestJournalTick "reply-before-deadline-check" (show (count,P.hexBytes reply)))
  where
    (count,reply) = case packet of
      P.BulkIn _ n -> (n,replicate n 0)
      P.BulkOut _ bytes -> (length bytes,[])
      P.ControlWrite _ _ _ _ bytes -> (length bytes,[])

-- recordWithin for several schema lines of one kind in one append: the same
-- file choice and checks, one write and one fsync pair. Each entry carries its
-- own decimal monotonic nanosecond clock.
recordBatchWithin :: Int -> FilePath -> String -> [(String,String)] -> IO ()
recordBatchWithin cap dir kind entries = do
  let file = if kind == "outgoing-before-dispatch" then "outgoing.v1" else "journal.v1"
      path = dir </> file
      content = concatMap (\(tick,body) -> journalLine tick kind body) entries
  exists <- doesPathExist path
  when exists $ do
    status <- getSymbolicLinkStatus path
    uid <- getEffectiveUserID
    unless (isRegularFile status && not (isSymbolicLink status) && fileOwner status == uid && fileMode status .&. 0o077 == 0 && fileSize status + fromIntegral (length content) <= fromIntegral cap) (P.reject "private bounded regular journal required")
  P.appendDurable path content
  bracket (openFd dir ReadOnly defaultFileFlags {directory=True,cloexec=True}) closeFd fileSynchronise

ensurePrivateDirectory :: FilePath -> IO ()
ensurePrivateDirectory dir = do
  exists <- doesPathExist dir
  unless exists (createDirectory dir >> setFileMode dir 0o700)
  st <- getSymbolicLinkStatus dir
  uid <- getEffectiveUserID
  unless (isDirectory st && not (isSymbolicLink st) && fileOwner st == uid && fileMode st .&. 0o077 == 0) (P.reject "owned private nonsymlink state directory required")

writePrivateBytes :: FilePath -> B.ByteString -> IO ()
writePrivateBytes path bytes = bracket (openFd path WriteOnly defaultFileFlags {creat=Just 0o600,exclusive=True,nofollow=True,cloexec=True}) closeFd $ \fd -> do
  status <- getFdStatus fd
  unless (isRegularFile status) (P.reject "capture must be regular")
  B.useAsCStringLen bytes $ \(buffer,size) -> do
    count <- fdWriteBuf fd (castPtr buffer) (fromIntegral size)
    unless (count == fromIntegral size) (P.reject "short private capture write")
  fileSynchronise fd

safeIdentifier :: String -> Bool
safeIdentifier s = not (null s) && length s <= 80 && all (`elem` (['a'..'z']++['A'..'Z']++['0'..'9']++"-_")) s

hexDigest :: String -> Bool
hexDigest s = length s == 64 && all (`elem` ("0123456789abcdef" :: String)) s

data Session = Session
  { sessionClock :: Clock
  , sessionApi :: UsbApi
  , sessionDirectory :: FilePath
  , sessionJournalCap :: Int
  , sessionDeadline :: Word64
  , sessionOutcomes :: OutcomeJournal
  }

newSession :: Clock -> UsbApi -> FilePath -> Int -> Int -> IO Session
newSession = newSessionJournaling EveryOutcome

newSessionJournaling :: OutcomeJournal -> Clock -> UsbApi -> FilePath -> Int -> Int -> IO Session
newSessionJournaling outcomes clock api dir cap aggregateMs = do
  start <- nowNS clock
  pure (Session clock api dir cap (start + fromIntegral aggregateMs*1000000) outcomes)

note :: Session -> String -> String -> IO ()
note s = recordWithin (sessionJournalCap s) (sessionDirectory s)

-- The intent is durable before the remaining budget is recomputed, so a session
-- that crosses its deadline while journaling stops before entering the provider.
bounded :: Session -> String -> (Int -> IO a) -> IO a
bounded = boundedWithin transferTimeoutMs

boundedWithin :: Int -> Session -> String -> (Int -> IO a) -> IO a
boundedWithin budget s label action = do
  unless (budget >= 1 && budget <= transferCeilingMs) (P.reject (label++" budget outside the transfer ceiling"))
  _ <- remaining (sessionClock s) (sessionDeadline s)
  note s "stage-intent" label
  before <- nowNS (sessionClock s)
  ms <- remaining (sessionClock s) (sessionDeadline s)
  value <- action (min ms budget)
  after <- nowNS (sessionClock s)
  when (after >= sessionDeadline s || after >= before + fromIntegral budget*1000000) (P.reject (label++" deadline exceeded"))
  note s "stage-complete" label
  pure value

tx :: Session -> P.Transfer -> Int -> IO [Word8]
tx = txWithin transferTimeoutMs

-- The one path from a session into a provider's transfer: the shape refusal and
-- the native-outcome lines live here, not in a provider.
dispatchTransfer :: Session -> P.Transfer -> Int -> IO (Int,[Word8])
dispatchTransfer s packet ms = do
  unless (supportedTransfer packet) (P.reject "unsupported native transfer")
  outcome <- try (usbTransfer (sessionApi s) packet ms)
  case outcome of
    Left (ProviderFailure result count bytes) -> do
      record result count bytes
      if count < 0 || count > transferLength packet
        then P.reject "invalid native byte count; raw count/status journaled without dereferencing bytes"
        else P.reject ("native transfer status "++show result++"; partial count="++show count++"; no retry")
    Right (count,bytes) -> do
      when (sessionOutcomes s == EveryOutcome || count /= transferLength packet) (record 0 count bytes)
      pure (count,bytes)
  where record result count bytes = note s "native-outcome" (nativeOutcomeFor packet result count bytes)

-- A bulk IN above transferCeilingBytes is a read-bulk chunk, which calls
-- dispatchTransfer directly; through here it is refused before the provider.
txWithin :: Int -> Session -> P.Transfer -> Int -> IO [Word8]
txWithin budget s packet count = do
  case packet of
    P.ControlWrite _ _ _ _ bytes -> note s "outgoing-before-dispatch" (show packet++";bytes="++P.hexBytes bytes)
    P.BulkOut _ bytes -> note s "outgoing-before-dispatch" (show packet++";bytes="++P.hexBytes bytes)
    P.BulkIn _ n | n > transferCeilingBytes ->
      P.reject ("bulk IN of "++show n++" bytes above "++show transferCeilingBytes++" outside the read-bulk chunk path")
    _ -> pure ()
  (n,bytes) <- boundedWithin budget s ("dispatch "++show packet) $ \ms -> do
    outcome <- dispatchTransfer s packet ms
    note s "reply-before-deadline-check" (show (fst outcome,P.hexBytes (snd outcome)))
    pure outcome
  unless (n == count) (P.reject "short/partial/oversized transfer; no retry")
  case packet of
    P.BulkIn _ _ -> unless (length bytes == count) (P.reject "read count/bytes mismatch")
    _ -> unless (null bytes) (P.reject "write returned bytes")
  pure bytes

-- Each flag is set before its stage, so a stage that fails part way still
-- releases what it may have taken.
data Held = Held { heldContext :: IORef Bool, heldHandle :: IORef Bool, heldInterface :: IORef Bool }

newHeld :: IO Held
newHeld = Held <$> newIORef False <*> newIORef False <*> newIORef False

acquireDevice :: Session -> Held -> IO Candidate
acquireDevice s held = acquireDeviceSelecting s held $ do
  devices <- bounded s "list" (usbList (sessionApi s))
  either (P.reject . refusalMessage) pure (selectDevice devices)

acquireDeviceSelecting :: Session -> Held -> IO Candidate -> IO Candidate
acquireDeviceSelecting s held select = do
  writeIORef (heldContext held) True
  bounded s "init" (usbInit (sessionApi s))
  device <- select
  writeIORef (heldHandle held) True
  bounded s "open" (usbOpen (sessionApi s) device)
  writeIORef (heldInterface held) True
  bounded s "claim" (usbClaim (sessionApi s) 0)
  pure device

releaseHeld :: Held -> UsbApi -> (String -> (Int -> IO ()) -> IO ()) -> IO ()
releaseHeld held api attempt = do
  readIORef (heldInterface held) >>= \yes -> when yes (attempt "release" (usbRelease api 0))
  readIORef (heldHandle held) >>= \yes -> when yes (attempt "close" (usbClose api))
  readIORef (heldContext held) >>= \yes -> when yes (attempt "exit" (usbExit api))
