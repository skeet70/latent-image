{-# LANGUAGE OverloadedStrings #-}
-- Each camera operation runs twice against the same scripted device: once
-- through the native provider with native_usb.c's entry points answered by the
-- device, once through the emulator bridge provider with the bridge's JSON
-- lines answered by the device. Both runs must leave the same files with the
-- same bytes, journal clock column aside. No USB device is listed or opened.
module Main (main) where

import Control.Exception (SomeException, displayException, finally, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), eitherDecode, encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt, isDigit)
import Data.IORef
import Data.List (sort)
import qualified Data.Text as T
import Data.Word (Word8, Word64)
import Foreign.C (newCString)
import Foreign.Marshal.Array (peekArray, pokeArray)
import Foreign.Storable (poke)
import System.Directory
import System.FilePath
import System.Posix.Process (getProcessID)
import EmulatorUsb (bridgeUsbApi)
import FujiAF.DumpRam (DumpSegment (..))
import FujiAF.Native (NativeCalls (..), nativeApiWith)
import FujiAF.RamPush
import FujiAF.Read (ReadOracle (..), ReadPlan (..), ReadWindow (..))
import FujiAF.ReadRam (mkPacing, mkRamWatch)
import FujiAF.ReadBulk (mkBulkWindow, readBulkResultLine, runReadBulk)
import FujiAF.Rehearsal
import FujiAF.Routes (RouteWindow (..), routeFixedWindows)
import FujiAF.StoredGate (CommitDirection (..), mkReferenceWindow)
import FujiAF.Transport
import qualified FujiAF.Broker as P
import RamPushModel (manifest, modelApi, newModel)

-- What the camera answers to one transfer: the count and the bytes read.
type Device = P.Transfer -> IO (Int, [Word8])

-- A provider standing over a device that lists as the given candidate.
type Provider = Candidate -> Device -> IO UsbApi

-- native_usb.c as the device sees it: one listed device whose fields are the
-- candidate's, and transfers that copy the device's reply into the buffer.
nativeProvider :: Provider
nativeProvider candidate device = do
  path <- newCString (physicalPath candidate)
  let d = descriptors candidate
      extras = extraEndpoints d
      fields = [ vid candidate, pid candidate, configuration d, interfaceNumber d, alternateSetting d
               , interfaceClass d, interfaceSubclass d, interfaceProtocol d, inputEndpoint d, outputEndpoint d
               , inputPacketSize d, outputPacketSize d, length extras ]
               ++ concat [[address,attributes,size,interval] | (address,attributes,size,interval) <- extras]
      field index
        | index == 37 = inputInterval d
        | index == 38 = outputInterval d
        | index < length fields = fields !! index
        | otherwise = 0
      transfer kind endpoint buffer size _ countPtr = do
        let count = fromIntegral size
        packet <- case kind of
          0 -> P.controlTransfer <$> peekArray count buffer
          1 -> P.BulkOut (fromIntegral endpoint) <$> peekArray count buffer
          _ -> pure (P.BulkIn (fromIntegral endpoint) count)
        (transferred,bytes) <- device packet
        pokeArray buffer (take count bytes)
        poke countPtr (fromIntegral transferred)
        pure 0
  pure (nativeApiWith NativeCalls
    { callLoad = pure 0, callInit = pure 0, callList = pure 1
    , callPath = \_ -> pure path, callField = \_ index -> pure (fromIntegral (field (fromIntegral index)))
    , callOpen = \_ -> pure 0, callClaim = pure 0, callTransfer = transfer
    , callRelease = pure 0, callClose = pure 0, callExit = pure 0 })

-- The RAM-push bridge's command loop as the device sees it, refusing a control
-- payload that is not twelve bytes as the bridge does.
emulatorProvider :: Provider
emulatorProvider _ = pure . bridgeUsbApi . respond
  where
    respond device line = do
      request <- case eitherDecode line of
        Right (Object fields) -> pure fields
        other -> fail ("bridge request is not an object: "++show other)
      reply <- case (text "command" request, text "transfer" request) of
        (Just "list", _) -> pure (Right [])
        (Just "transfer", Just "control") -> do
          bytes <- hex "bytes" request
          if length bytes /= 12
            then pure (Left "control transfer must carry the internal twelve-byte request")
            else answer device (P.controlTransfer bytes)
        (Just "transfer", Just "bulk-out") -> do
          packet <- P.BulkOut <$> int "endpoint" request <*> hex "bytes" request
          answer device packet
        (Just "transfer", Just "bulk-in") -> do
          packet <- P.BulkIn <$> int "endpoint" request <*> int "count" request
          answer device packet
        _ -> pure (Left "unknown RAM-push bridge command")
      pure (BL.toStrict (encode (object (case reply of
        Right fields -> ("ok" .= True) : fields
        Left message -> ["ok" .= False, "message" .= (message :: String)]))))
    answer device packet = do
      (count,bytes) <- device packet
      pure (Right ["count" .= count, "bytes" .= P.hexBytes bytes])
    text name fields = case KM.lookup (K.fromString name) fields of
      Just (String value) -> Just (T.unpack value)
      _ -> Nothing
    int name fields = case KM.lookup (K.fromString name) fields of
      Just (Number value) -> pure (round value)
      _ -> fail ("bridge request lacks integer "++name)
    hex name fields = maybe (fail ("bridge request lacks hex "++name)) (pure . decodeHex) (text name fields)
    decodeHex (high:low:rest) = fromIntegral (16*digitToInt high + digitToInt low) : decodeHex rest
    decodeHex _ = []

-- Advances one microsecond per reading and by the pause on a pause.
steppingClock :: IO Clock
steppingClock = do
  now <- newIORef (1000000000 :: Word64)
  pure (TimedClock (atomicModifyIORef' now (\tick -> (tick+1000, tick)))
                   (\ms -> modifyIORef' now (+ fromIntegral ms*1000000)))

-- Every file under the directory with its bytes; journal lines lose their
-- monotonic clock column, which the journal writer reads from the host.
snapshot :: FilePath -> IO [(FilePath, [B.ByteString])]
snapshot root = do
  files <- walk ""
  forM (sort files) $ \relative -> do
    bytes <- B.readFile (root </> relative)
    pure (relative, if takeExtension relative == ".v1" then map withoutClock (C.lines bytes) else [bytes])
  where
    walk relative = do
      entries <- listDirectory (root </> relative)
      concat <$> forM entries (\entry -> do
        let path = relative </> entry
        directory <- doesDirectoryExist (root </> path)
        if directory then walk path else pure [path])
    prefix = C.pack "(1,\"camera\",\""
    withoutClock line = case B.stripPrefix prefix line of
      Just rest -> prefix <> C.dropWhile isDigit rest
      Nothing -> line

-- An operation's result line or the first line of its stop, and what it wrote.
data Run = Run { runOutcome :: Either String String, runFiles :: [(FilePath, [B.ByteString])] }

-- Runs the action in a fresh private directory, at one path for both
-- providers; journals record output paths.
inSession :: FilePath -> (FilePath -> IO String) -> IO Run
inSession directory action = do
  exists <- doesPathExist directory
  when exists (removeDirectoryRecursive directory)
  ensurePrivateDirectory directory
  outcome <- try (action directory)
  files <- snapshot directory
  removeDirectoryRecursive directory
  pure (Run (either (\(e::SomeException) -> Left (takeWhile (/= '\n') (displayException e))) Right outcome) files)

scripted :: FilePath -> ScriptedOperation -> Provider -> IO Run
scripted directory operation provider = do
  script <- newIORef (scriptedSteps operation)
  api <- provider rehearsalCandidate (\packet -> usbTransfer (scriptedApi script) packet transferTimeoutMs)
  clock <- steppingClock
  inSession directory $ \dir -> do
    result <- scriptedRun operation clock api dir
    leftover <- readIORef script
    unless (null leftover) (fail (show (length leftover)++" planned transfers unsent"))
    pure result

-- ram-alloc, ram-push and ram-unload on one camera, then ram-alloc and
-- ram-free on a second.
ramSessions :: FilePath -> Provider -> IO Run
ramSessions directory provider = inSession directory $ \root -> do
  let over model = provider rehearsalCandidate (\packet -> usbTransfer (modelApi model) packet transferTimeoutMs)
      session name = let path = root </> name in ensurePrivateDirectory path >> pure path
      allocate clock api name = session name >>= runRamAlloc reviewedAllocationSize clock api >>= \outcome -> case outcome of
        RamAllocated path -> readRetainedAllocation path
        other -> fail ("ram-alloc: "++show other)
  clock <- steppingClock
  pushModel <- newModel Nothing
  pushApi <- over pushModel
  retained <- allocate clock pushApi "alloc"
  pushed <- session "push" >>= runRamPushWith PeerCacheBound (retainedAllocation retained) manifest clock pushApi
  state <- case pushed of
    RamPushInstalled path -> readRamPushState path
    other -> fail ("ram-push: "++show other)
  unloaded <- session "unload" >>= runRamUnloadWith PeerCacheBound state clock pushApi
  freeModel <- newModel Nothing
  freeApi <- over freeModel
  freeRetained <- allocate clock freeApi "free-alloc"
  freed <- session "free" >>= runRamFree freeRetained clock freeApi
  pure (show (unloaded, freed))

-- read-bulk over two and a half chunks of the bulk camera.
bulkSession :: FilePath -> Provider -> IO Run
bulkSession directory provider = do
  device <- bulkCameraDevice stockBulkCamera
  api <- provider rehearsalCandidate device
  clock <- steppingClock
  window <- either fail pure (mkBulkWindow 0x00bf1a00 0x280000)
  inSession directory (\dir -> readBulkResultLine <$> runReadBulk window clock api dir)

-- The first file line or outcome where the runs differ; a file one run lacks
-- reads as empty.
firstDifference :: Run -> Run -> Maybe String
firstDifference native emulator
  | (path,index,a,b):_ <- [ (path,index,a,b) | path <- paths
                                             , (index,a,b) <- lineDifferences (contents native path) (contents emulator path) ] =
      Just (path++" line "++show index++":\n  native:   "++show a++"\n  emulator: "++show b)
  | runOutcome native /= runOutcome emulator = Just ("outcomes differ:\n  native:   "++show (runOutcome native)++"\n  emulator: "++show (runOutcome emulator))
  | otherwise = Nothing
  where
    paths = nubOrd (sort (map fst (runFiles native ++ runFiles emulator)))
    contents run path = maybe [] id (lookup path (runFiles run))
    lineDifferences a b =
      [ (index,x,y) | (index,x,y) <- zip3 [1::Int ..] (pad a) (pad b), x /= y ]
      where width = max (length a) (length b)
            pad xs = take width (map Just xs ++ repeat Nothing)
    nubOrd (x:y:rest) | x == y = nubOrd (y:rest)
    nubOrd (x:rest) = x : nubOrd rest
    nubOrd [] = []

main :: IO ()
main = do
  pid <- getProcessID
  let root = "/dev/shm" </> ("fuji-af-provider-parity-"++show pid)
      session = root </> "session"
      plan = ReadPlan "parity-plan" "synthetic 1.50 basis"
        [ReadWindow 0 0x000000 "config window 0", ReadWindow 1 0x000100 "product window", ReadWindow 2 0x000200 "signature window"]
        (ReadOracle 1 0x5c) "synthetic generator" (replicate 64 '0')
      ramWindows = [RouteWindow 0 0x400a0800 64 "log store header", RouteWindow 1 0x400c0800 4 "boot tick cell"]
  watch <- either fail pure (mkRamWatch ramWindows =<< mkPacing 2 100)
  reference <- either fail pure (mkReferenceWindow (B.replicate 256 0))
  exists <- doesPathExist root
  when exists (removeDirectoryRecursive root)
  createDirectory root
  let operations =
        [ identifyScript, readConfigScript plan, endSettingScript, probeRamScript, readLogScript
        , probeBootRomScript, probeArea2Script, probeStrapsScript, probeCodeScript, probeOwnershipScript
        , probeRoutesScript "probe-routes fixed" routeFixedWindows, dumpRamScript Loaders, readMediaScript
        , armAutoActScript, commitF7Script EnableStoredGate reference, commitF7Script RestoreStoredGate reference
        , readRamScript watch ]
      compareRuns name runOn = do
        native <- runOn nativeProvider
        emulator <- runOn emulatorProvider
        forM_ (firstDifference native emulator) $ \difference ->
          fail (name++": the native and emulator providers diverge at "++difference)
        result <- either (\stop -> fail (name++" stopped under both providers: "++stop)) pure (runOutcome native)
        when (null (runFiles native)) (fail (name++" wrote no files"))
        putStrLn ("parity: "++name++": "++show (length (runFiles native))++" files equal; "++result)
  flip finally (removeDirectoryRecursive root) $ do
    forM_ operations $ \operation -> compareRuns (scriptedName operation) (scripted session operation)
    compareRuns "ram-alloc, ram-push, ram-unload, ram-free" (ramSessions session)
    compareRuns "read-bulk" (bulkSession session)
  putStrLn "provider parity: ok"
