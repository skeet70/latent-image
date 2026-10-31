{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Stock media-route diagnostics through the RAM-push bridge.
module Main (main) where

import Control.Applicative ((<|>))
import Control.DeepSeq (NFData (..), force)
import Control.Exception (Exception, IOException, SomeException, displayException, evaluate, throwIO, try)
import Control.Monad (forM, forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', encode, fromJSON, object, toJSON, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Foldable (toList)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (find, isInfixOf, isPrefixOf, nub)
import qualified Data.Map.Strict as Map
import Data.Maybe (fromMaybe, isJust, isNothing, mapMaybe)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word32, Word64, Word8)
import qualified Emulation.CaptureStream as CS
import GHC.Generics (Generic)
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, createDirectory, createDirectoryIfMissing, doesFileExist, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (takeDirectory, takeFileName, splitDirectories, (</>))
import System.IO (Handle, IOMode (..), hClose, hFlush, hPutStrLn, openBinaryFile, stderr)
import System.Posix.Files (fileID, fileSize, getSymbolicLinkStatus, isRegularFile, modificationTimeHiRes)
import System.Process (CreateProcess (..), StdStream (..), callProcess, createProcess, proc, waitForProcess)

-- Inputs ------------------------------------------------------------------------------------

data Route = PlayRoute | ShutterContactRoute | ShutterSequenceRoute deriving (Eq)

routeName :: Route -> String
routeName PlayRoute = "play"
routeName ShutterContactRoute = "s2"
routeName ShutterSequenceRoute = "s1-s2"

routeInput :: Route -> Value
routeInput PlayRoute = command "key" ["key" .= ("play" :: T.Text)]
routeInput ShutterContactRoute = command "key" ["key" .= ("full-press" :: T.Text)]
routeInput ShutterSequenceRoute = command "shutter-sequence" []

newtype Digest = Digest String deriving (Eq, Show)

-- | A run document the bridge receives unchanged as `--run`.
newtype RunDocumentPath = RunDocumentPath FilePath

-- | Row width of a `raw16-index-ramp12` sensor raster: sample i of the row-major raster holds i mod 4096.
newtype RampRasterWidth = RampRasterWidth Int deriving (Show)

data Options = Options
  { optSdk :: FilePath
  , optBridgeSha :: Digest
  , optCard :: FilePath
  , optFirmware :: FilePath
  , optNand :: FilePath
  , optBootRom :: FilePath
  , optOutput :: FilePath
  , optRoute :: Route
  , optCheckpoints :: Maybe FilePath
  , optWindows :: Int
  , optShootWithoutLens :: Bool
  , optRunDocument :: Maybe RunDocumentPath
  , optRunDocumentSha :: Maybe Digest
  , optRamp :: Maybe RampRasterWidth
  , optCaptureContinuationObservation :: Bool
  }

usage :: String
usage = unlines
  [ "media-route-smoke --sdk STORE --bridge-sha256 HEX --route play|s2|s1-s2 --output NEW_PRIVATE_DIR"
  , "  [--checkpoints DIR] [--windows N] [--shoot-without-lens] [--card IMG] [--firmware DAT]"
  , "  [--installed-nand NAND] [--boot-rom ROM] [--run-document JSON [--run-document-sha256 HEX]]"
  , "  [--expect-ramp12-width N] [--capture-continuation-observation (requires --route s1-s2 --windows 1)]"
  , "Without --checkpoints the bridge cold-boots with --fresh-boot into OUTPUT/checkpoints."
  , "With it, the bridge restores its keyed attach checkpoint there or cold-boots and saves one."
  , "Without --run-document the bridge runs the SDK's share/fujimod-xm1/xm1-150.json." ]

parseOptions :: [String] -> IO Options
parseOptions = go Nothing Nothing Nothing Nothing defaults
  where
    defaults = Options "" (Digest "") "private-data/scratch/issue117-camera-fixture/DSCF3095-DCIM.img"
      "private-data/firmware/x-m1-1.50/FWUP0002.DAT" "private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand"
      "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin" "" PlayRoute Nothing 12 False Nothing Nothing Nothing False
    hex v = length v == 64 && all (`elem` ("0123456789abcdef" :: String)) v
    go (Just sdk) (Just sha) (Just output) (Just route) o []
      | optCaptureContinuationObservation o && (route /= ShutterSequenceRoute || optWindows o /= 1) =
          usageError "--capture-continuation-observation requires --route s1-s2 --windows 1"
      | otherwise = pure o { optSdk = sdk, optBridgeSha = sha, optOutput = output, optRoute = route }
    go _ _ _ _ _ [] = usageError "--sdk, --bridge-sha256, --output and --route are required"
    go _ h out r o ("--sdk" : v : rest) = go (Just v) h out r o rest
    go s _ out r o ("--bridge-sha256" : v : rest)
      | hex v = go s (Just (Digest v)) out r o rest
    go s h _ r o ("--output" : v : rest) = go s h (Just v) r o rest
    go s h out _ o ("--route" : "play" : rest) = go s h out (Just PlayRoute) o rest
    go s h out _ o ("--route" : "s2" : rest) = go s h out (Just ShutterContactRoute) o rest
    go s h out _ o ("--route" : "s1-s2" : rest) = go s h out (Just ShutterSequenceRoute) o rest
    go s h out r o ("--checkpoints" : v : rest) = go s h out r o { optCheckpoints = Just v } rest
    go s h out r o ("--windows" : v : rest)
      | [(n, "")] <- reads v, n >= 1, n <= 30 = go s h out r o { optWindows = n } rest
    go s h out r o ("--shoot-without-lens" : rest) = go s h out r o { optShootWithoutLens = True } rest
    go s h out r o ("--capture-continuation-observation" : rest) = go s h out r o { optCaptureContinuationObservation = True } rest
    go s h out r o ("--card" : v : rest) = go s h out r o { optCard = v } rest
    go s h out r o ("--firmware" : v : rest) = go s h out r o { optFirmware = v } rest
    go s h out r o ("--installed-nand" : v : rest) = go s h out r o { optNand = v } rest
    go s h out r o ("--boot-rom" : v : rest) = go s h out r o { optBootRom = v } rest
    go s h out r o ("--run-document" : v : rest) = go s h out r o { optRunDocument = Just (RunDocumentPath v) } rest
    go s h out r o ("--run-document-sha256" : v : rest)
      | hex v = go s h out r o { optRunDocumentSha = Just (Digest v) } rest
    go s h out r o ("--expect-ramp12-width" : v : rest)
      | [(n, "")] <- reads v, n >= 2, n <= 0x4000 = go s h out r o { optRamp = Just (RampRasterWidth n) } rest
    go _ _ _ _ _ (argument : _) = usageError ("unknown or invalid argument " ++ argument)
    usageError message = ioError (userError (message ++ "\n" ++ usage))

-- | DCIM/963_FUJI/DSCF3095.JPG as independently verified on the host.
fixtureFolder, fixtureName :: B.ByteString
fixtureFolder = "963_FUJI   "
fixtureName = "DSCF3095JPG"

fixtureBytes :: Word32
fixtureBytes = 3515192

fixtureSha :: Digest
fixtureSha = Digest "40bc1f32a0091307e1d2e18b5855ce110373afa60d81878296ee62f7e2f05969"

-- | The run document's sd node declares 8388608 sectors.
cardBytes :: Integer
cardBytes = 4294967296

-- | Re-holding P stores the same ADC3 sample that is already held; FujiMod's setter only replaces
-- the converter's external sample (emulator/src/session/inputs.rs:25-35, emulator-exr2-io/src/adc/
-- controller.rs:54-56). The bridge then advances its fixed MODE_DIAL_SETTLE of 40,000,000 instructions.
windowDial :: T.Text
windowDial = "P"

-- Errors ------------------------------------------------------------------------------------

-- | A failed bridge command or closed transport.
newtype BridgeStopped = BridgeStopped String deriving (Show)
instance Exception BridgeStopped

harness :: String -> IO a
harness = ioError . userError

require :: Bool -> String -> IO ()
require ok message = unless ok (harness message)

-- Hex, digests and little-endian fields ---------------------------------------------------

hexByte :: Word8 -> String
hexByte w = let s = showHex w "" in if length s == 1 then '0' : s else s

hexBytes :: B.ByteString -> String
hexBytes = concatMap hexByte . B.unpack

digest :: B.ByteString -> Digest
digest = Digest . hexBytes . SHA256.hash

fileDigest :: FilePath -> IO Digest
fileDigest path = Digest . hexBytes . SHA256.hashlazy <$> BL.readFile path

digestText :: Digest -> String
digestText (Digest d) = d

decodeHex :: T.Text -> Either String B.ByteString
decodeHex text
  | odd (B.length raw) = Left "odd hex length"
  | B.length decoded /= half = Left "invalid lowercase hex"
  | otherwise = Right decoded
  where
    raw = TE.encodeUtf8 text
    half = B.length raw `div` 2
    decoded = fst (B.unfoldrN half step 0)
    nibble w | w >= 48 && w <= 57 = Just (w - 48)
             | w >= 97 && w <= 102 = Just (w - 87)
             | otherwise = Nothing
    step i = do
      high <- nibble (B.index raw i)
      low <- nibble (B.index raw (i + 1))
      Just (high `shiftL` 4 .|. low, i + 2)

le :: B.ByteString -> Int -> Int -> Word64
le bytes offset width = foldr (\i acc -> acc `shiftL` 8 .|. fromIntegral (B.index bytes (offset + i))) 0 [0 .. width - 1]

hex32 :: Word32 -> String
hex32 value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

parseHex32 :: T.Text -> Maybe Word32
parseHex32 text = case T.unpack text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits -> Just value
  _ -> Nothing

-- JSON ----------------------------------------------------------------------------------------

type Fields = KM.KeyMap Value

num :: FromJSON a => K.Key -> Fields -> Maybe a
num field fields = KM.lookup field fields >>= \value -> case fromJSON value of
  A.Success x -> Just x
  A.Error _ -> Nothing

textAt :: K.Key -> Fields -> Maybe T.Text
textAt field fields = case KM.lookup field fields of
  Just (String t) -> Just t
  _ -> Nothing

objectAt :: K.Key -> Fields -> Maybe Fields
objectAt field fields = case KM.lookup field fields of
  Just (Object o) -> Just o
  _ -> Nothing

asObject :: Value -> Either String Fields
asObject (Object o) = Right o
asObject _ = Left "bridge line is not an object"

-- Bridge client -------------------------------------------------------------------------------

data Bridge = Bridge
  { bridgeIn :: Handle
  , bridgeReader :: CS.BoundedReader
  , bridgeRequests :: Handle
  , bridgeStdout :: Handle
  }

send :: Bridge -> Value -> IO ()
send bridge value = do
  let line = BL.toStrict (encode value) <> "\n"
  B.hPut (bridgeRequests bridge) line >> hFlush (bridgeRequests bridge)
  B.hPut (bridgeIn bridge) line >> hFlush (bridgeIn bridge)

receive :: Bridge -> IO B.ByteString
receive bridge = do
  line <- CS.readBoundedLine (bridgeReader bridge)
  B.hPut (bridgeStdout bridge) (line <> "\n") >> hFlush (bridgeStdout bridge)
  pure line

-- | One request and its one reply; ok:false or a closed pipe throws 'BridgeStopped'.
call :: Bridge -> Value -> IO Fields
call bridge value = do
  exchanged <- try (send bridge value >> receive bridge)
  line <- either (\e -> throwIO (BridgeStopped ("bridge pipe: " ++ displayException (e :: IOException)))) pure exchanged
  fields <- either (\e -> harness ("invalid bridge reply: " ++ e)) pure (eitherDecodeStrict' line >>= asObject)
  case KM.lookup "ok" fields of
    Just (Bool True) -> pure (KM.delete "ok" fields)
    Just (Bool False) -> throwIO (BridgeStopped (maybe "bridge error without message" T.unpack (textAt "message" fields)))
    _ -> harness "bridge reply lacks boolean ok"

command :: T.Text -> [(K.Key, Value)] -> Value
command name rest = Object (KM.fromList (("command", String name) : rest))

event :: Bridge -> T.Text -> IO Fields
event bridge name = do
  line <- receive bridge
  fields <- either (\e -> harness ("invalid bridge event: " ++ e)) pure (eitherDecodeStrict' line >>= asObject)
  require (KM.lookup "ok" fields == Just (Bool True) && textAt "event" fields == Just name)
    ("bridge did not send " ++ T.unpack name ++ ": " ++ C.unpack line)
  pure fields

-- Journal -------------------------------------------------------------------------------------

type Journal = CS.Journal

note :: Journal -> T.Text -> Value -> IO ()
note = CS.note

-- Card through media-read ---------------------------------------------------------------------

newtype Sector = Sector Word32 deriving (Eq, Ord, Show)
newtype Cluster = Cluster Word32 deriving (Eq, Ord, Show)

-- | Half-open sector extent.
data Extent = Extent { extentFirst :: Sector, extentSectors :: Word32 } deriving (Eq, Show)

extentValue :: Extent -> Value
extentValue (Extent (Sector first) count) = object ["first" .= first, "sectors" .= count]

data Volume = Volume
  { volClusterSectors :: Word32
  , volReserved :: Word32
  , volFats :: Word32
  , volFatSectors :: Word32
  , volRootCluster :: Cluster
  , volTotalSectors :: Word32
  }

volumeValue :: Volume -> Value
volumeValue v = object
  [ "sectors_per_cluster" .= volClusterSectors v, "reserved_sectors" .= volReserved v, "fats" .= volFats v
  , "fat_sectors" .= volFatSectors v, "root_cluster" .= (let Cluster c = volRootCluster v in c)
  , "total_sectors" .= volTotalSectors v, "data_start" .= dataStart v ]

dataStart :: Volume -> Word32
dataStart v = volReserved v + volFats v * volFatSectors v

clusterSector :: Volume -> Cluster -> Sector
clusterSector v (Cluster c) = Sector (dataStart v + (c - 2) * volClusterSectors v)

parseVolume :: B.ByteString -> Either String Volume
parseVolume boot
  | B.length boot /= 512 = Left "boot sector length"
  | B.index boot 510 /= 0x55 || B.index boot 511 /= 0xaa = Left "boot sector lacks 55aa"
  | le boot 11 2 /= 512 = Left "bytes per sector is not 512"
  | B.take 8 (B.drop 82 boot) /= "FAT32   " = Left "file-system type is not FAT32"
  | le boot 17 2 /= 0 || le boot 22 2 /= 0 = Left "FAT12/16 fields are nonzero"
  | spc == 0 || fats == 0 || fatSectors == 0 = Left "zero geometry field"
  | otherwise = Right (Volume spc reserved fats fatSectors (Cluster (fromIntegral (le boot 44 4))) (fromIntegral (le boot 32 4)))
  where
    spc = fromIntegral (B.index boot 13)
    reserved = fromIntegral (le boot 14 2)
    fats = fromIntegral (B.index boot 16)
    fatSectors = fromIntegral (le boot 36 4)

maxMediaRead :: Word32
maxMediaRead = 512

readSectors :: Bridge -> Sector -> Word32 -> IO B.ByteString
readSectors _ _ 0 = pure B.empty
readSectors bridge (Sector first) count = B.concat <$> mapM chunk (takeWhile (< count) (iterate (+ maxMediaRead) 0))
  where
    chunk offset = do
      let n = min maxMediaRead (count - offset)
          at = first + offset
      reply <- call bridge (command "media-read" ["first" .= at, "count" .= n])
      require (num "first" reply == Just at && num "count" reply == Just n) "media-read echoed a different extent"
      bytes <- either (harness . ("media-read: " ++)) pure (maybe (Left "missing bytes") decodeHex (textAt "bytes" reply))
      require (B.length bytes == fromIntegral n * 512) "media-read returned a different length"
      pure bytes

data Card = Card { cardBridge :: Bridge, cardVolume :: Volume, cardFat :: IORef (Map.Map Word32 B.ByteString) }

fatEntry :: Card -> Cluster -> IO Word32
fatEntry card (Cluster c) = do
  let offset = fromIntegral c * 4 :: Word64
      sector = volReserved (cardVolume card) + fromIntegral (offset `div` 512)
      within = fromIntegral (offset `mod` 512)
  cached <- Map.lookup sector <$> readIORef (cardFat card)
  bytes <- case cached of
    Just b -> pure b
    Nothing -> do
      b <- readSectors (cardBridge card) (Sector sector) 1
      modifyIORef' (cardFat card) (Map.insert sector b)
      pure b
  pure (fromIntegral (le bytes within 4) .&. 0x0fffffff)

chain :: Card -> Cluster -> Int -> IO [Cluster]
chain card start limit = go start 0 []
  where
    go cluster@(Cluster c) n acc
      | c < 2 = harness ("FAT chain reaches reserved cluster " ++ show c)
      | n >= limit = harness "FAT chain exceeds its bound"
      | otherwise = do
          next <- fatEntry card cluster
          if next >= 0x0ffffff8 then pure (reverse (cluster : acc))
          else if next == 0 || next == 0x0ffffff7 then harness ("FAT chain at cluster " ++ show c ++ " is free or bad")
          else go (Cluster next) (n + 1) (cluster : acc)

runsOf :: [Cluster] -> [(Cluster, Word32)]
runsOf [] = []
runsOf (Cluster c : rest) = go c 1 rest
  where
    go s n (Cluster x : xs) | x == s + n = go s (n + 1) xs
    go s n xs = (Cluster s, n) : runsOf xs

extentsOf :: Volume -> [Cluster] -> [Extent]
extentsOf v clusters = [Extent (clusterSector v start) (n * volClusterSectors v) | (start, n) <- runsOf clusters]

readClusters :: Card -> [Cluster] -> IO B.ByteString
readClusters card clusters = B.concat <$> mapM readExtent (extentsOf (cardVolume card) clusters)
  where readExtent (Extent first n) = readSectors (cardBridge card) first n

data DirEntry = DirEntry { entryName :: B.ByteString, entryAttributes :: Word8, entryCluster :: Cluster, entrySize :: Word32 }

entryValue :: DirEntry -> Value
entryValue e = object
  [ "name" .= C.unpack (entryName e), "attributes" .= entryAttributes e
  , "cluster" .= (let Cluster c = entryCluster e in c), "size" .= entrySize e ]

parseEntries :: B.ByteString -> [DirEntry]
parseEntries bytes = go 0
  where
    go off
      | off + 32 > B.length bytes || B.index bytes off == 0 = []
      | B.index bytes off == 0xe5 || attr == 0x0f || attr .&. 0x08 /= 0 = go (off + 32)
      | otherwise = DirEntry (B.take 11 e) attr cluster (fromIntegral (le e 28 4)) : go (off + 32)
      where
        e = B.take 32 (B.drop off bytes)
        attr = B.index e 11
        cluster = Cluster (fromIntegral (le e 20 2) `shiftL` 16 .|. fromIntegral (le e 26 2))

isDirectory :: DirEntry -> Bool
isDirectory e = entryAttributes e .&. 0x10 /= 0 && entryName e `notElem` [".          ", "..         "]

readDirectory :: Card -> Cluster -> IO ([Cluster], [DirEntry])
readDirectory card start = do
  clusters <- chain card start 4096
  entries <- parseEntries <$> readClusters card clusters
  pure (clusters, entries)

-- | A file's clusters and its bytes, trimmed to the directory size.
readFileBytes :: Card -> DirEntry -> IO ([Cluster], B.ByteString)
readFileBytes card e
  | entrySize e == 0 = pure ([], B.empty)
  | otherwise = do
      let clusterBytes = volClusterSectors (cardVolume card) * 512
          expected = fromIntegral ((entrySize e + clusterBytes - 1) `div` clusterBytes)
      clusters <- chain card (entryCluster e) (expected + 1)
      require (length clusters == expected) (C.unpack (entryName e) ++ " chain length differs from its size")
      bytes <- B.take (fromIntegral (entrySize e)) <$> readClusters card clusters
      pure (clusters, bytes)

data Folder = Folder { folderName :: B.ByteString, folderExtents :: [Extent], folderEntries :: [DirEntry] }

data CardView = CardView { viewVolume :: Volume, viewCard :: Card, viewDcimExtents :: [Extent], viewFolders :: [Folder] }

walkCard :: Bridge -> IO CardView
walkCard bridge = do
  boot <- readSectors bridge (Sector 0) 1
  volume <- either (harness . ("card boot sector: " ++)) pure (parseVolume boot)
  cache <- newIORef Map.empty
  let card = Card bridge volume cache
  (_, root) <- readDirectory card (volRootCluster volume)
  dcim <- maybe (harness "card root has no DCIM directory") pure (find (\e -> entryName e == "DCIM       " && isDirectory e) root)
  (dcimClusters, dcimEntries) <- readDirectory card (entryCluster dcim)
  folders <- forM (filter isDirectory dcimEntries) $ \e -> do
    (clusters, entries) <- readDirectory card (entryCluster e)
    pure (Folder (entryName e) (extentsOf volume clusters) entries)
  pure (CardView volume card (extentsOf volume dcimClusters) folders)

viewValue :: CardView -> Value
viewValue view = object
  [ "volume" .= volumeValue (viewVolume view)
  , "dcim_extents" .= map extentValue (viewDcimExtents view)
  , "folders" .= [ object ["name" .= C.unpack (folderName f), "extents" .= map extentValue (folderExtents f), "entries" .= map entryValue (folderEntries f)]
                 | f <- viewFolders view ] ]

data Target = Target { targetFolder :: [Extent], targetFile :: [Extent], targetBytes :: B.ByteString }

-- | Requires the inserted copy to hold the verified JPEG and returns where it lives.
verifyFixture :: CardView -> IO (Target, Value)
verifyFixture view = do
  folder <- maybe (harness "card has no DCIM/963_FUJI") pure (find ((== fixtureFolder) . folderName) (viewFolders view))
  entry <- maybe (harness "963_FUJI has no DSCF3095.JPG") pure (find ((== fixtureName) . entryName) (folderEntries folder))
  require (entrySize entry == fixtureBytes) "DSCF3095.JPG size differs from the verified fixture"
  (clusters, bytes) <- readFileBytes (viewCard view) entry
  require (digest bytes == fixtureSha) ("media-read DSCF3095.JPG SHA-256 " ++ digestText (digest bytes) ++ " differs from the verified fixture")
  let extents = extentsOf (viewVolume view) clusters
  pure (Target (folderExtents folder) extents bytes, object
    [ "entry" .= entryValue entry, "extents" .= map extentValue extents, "sha256" .= digestText (digest bytes)
    , "soi" .= (B.take 2 bytes == "\xff\xd8"), "eoi" .= (B.drop (B.length bytes - 2) bytes == "\xff\xd9") ])

-- SD host state -------------------------------------------------------------------------------

data SdCommand = SdCommand { sdIndex :: Word32, sdArgument :: Word32, sdQuantity :: Word32 } deriving (Eq)

data SdState = SdState
  { sdCompleted :: Word64
  , sdAccepted :: Word64
  , sdDirty :: Word64
  , sdProtected :: Maybe Bool
  , sdPresent :: Maybe Bool
  , sdHistory :: [SdCommand]
  }

parseSd :: Fields -> Either String SdState
parseSd fields = do
  completed <- need "completed"
  accepted <- need "accepted_writes"
  dirty <- need "dirty_sectors"
  history <- case KM.lookup "history" fields of
    Just (Array items) -> mapM entry (toList items)
    _ -> Left "media-state lacks history"
  Right (SdState completed accepted dirty (num "write_protected" fields) (num "present" fields) history)
  where
    need field = maybe (Left ("media-state lacks " ++ K.toString field)) Right (num field fields)
    entry (Object o) = maybe (Left "media-state history entry") Right
      (SdCommand <$> num "command" o <*> num "argument" o <*> num "quantity" o)
    entry _ = Left "media-state history entry is not an object"

mediaState :: Bridge -> IO (Fields, SdState)
mediaState bridge = do
  fields <- call bridge (command "media-state" [])
  state <- either harness pure (parseSd fields)
  pure (fields, state)

-- | Commands completed since `before`, newest last, and how many fell outside the 32-entry history.
completedSince :: SdState -> SdState -> ([SdCommand], Word64)
completedSince before now =
  let delta = sdCompleted now - sdCompleted before
      kept = fromIntegral (min delta (fromIntegral (length (sdHistory now))))
  in (drop (length (sdHistory now) - kept) (sdHistory now), delta - fromIntegral kept)

commandSectors :: SdCommand -> Word64
commandSectors c = max 1 ((fromIntegral (sdQuantity c) + 511) `div` 512)

touches :: [Extent] -> SdCommand -> Bool
touches extents c = any overlap extents
  where
    first = fromIntegral (sdArgument c) :: Word64
    end = first + commandSectors c
    overlap (Extent (Sector f) n) = first < fromIntegral f + fromIntegral n && fromIntegral f < end

sdCommandValue :: Int -> SdCommand -> Value
sdCommandValue window c = object
  ["window" .= window, "command" .= sdIndex c, "argument" .= sdArgument c, "quantity" .= sdQuantity c]

isRead, isWrite :: SdCommand -> Bool
isRead = (== 18) . sdIndex
isWrite = (== 25) . sdIndex

-- Guest memory cells --------------------------------------------------------------------------

data Cell = Cell { cellName :: T.Text, cellAddress :: Word32, cellWidth :: Int, cellSource :: T.Text }

-- | DRAM-physical cells; uncached 0x4xxxxxxx aliases are given at their physical address.
cells :: [Cell]
cells =
  [ Cell "ui_consumer_state" 0x01467778 2 "reports/full-system-emulation.md:492"
  , Cell "modal_4a_state" 0x0146761c 2 "reports/af-observation-plan.md:73"
  , Cell "shooting_mode" 0x00956838 4 "emulation/Xm1ViewerInputsE2E.hs:236-250"
  , Cell "ui_mode_current" 0x009500ec 4 "reports/service-shoot-feasibility.md:127"
  , Cell "ui_mode_previous" 0x009500f0 4 "reports/service-shoot-feasibility.md:127"
  , Cell "ui_mode_pending" 0x009500f8 4 "reports/service-shoot-feasibility.md:127"
  , Cell "ui_mode_request" 0x00950100 4 "reports/service-shoot-feasibility.md:127"
  , Cell "ui_mode_transition" 0x00950110 1 "reports/service-shoot-feasibility.md:127"
  , Cell "media_status" 0x00918af8 4 "reports/cardless-capture-feasibility.md:37"
  , Cell "cached_warning" 0x00950114 4 "reports/cardless-capture-feasibility.md:38"
  , Cell "active_warnings" 0x00950118 4 "reports/cardless-capture-feasibility.md:38"
  , Cell "warning_bitset" 0x00950138 8 "reports/af-observation-plan.md:77"
  , Cell "sd_status_flags" 0x009738c4 4 "reports/cardless-capture-feasibility.md:35"
  , Cell "pending_replay" 0x00951fc4 1 "reports/cardless-capture-feasibility.md:266-270"
  , Cell "lens_word_0" 0x009504d4 4 "reports/autofocus-static-map.md:51"
  , Cell "lens_word_1" 0x00950508 4 "reports/autofocus-static-map.md:51"
  , Cell "image_pool_selector" 0x015480d4 4 "reports/cardless-capture-feasibility.md:447"
  , Cell "save_wait_word" 0x0154d970 4 "reports/cardless-capture-feasibility.md:474"
  , Cell "reccont_states" 0x01545858 0xae "reports/af-observation-plan.md:122-134"
  , Cell "modal_94_event" 0x014677a0 2 "reports/classic-chrome-icon.md:133"
  , Cell "modal_94_state" 0x01467644 2 "reports/classic-chrome-icon.md:133"
  , Cell "retained_header" 0x009c281c 16 "reports/adjustment-and-service-interfaces.md:171 (0x409c281c)"
  , Cell "last_frame_name" 0x009c2904 64 "reports/srecord-route.md:21 (0x409c2904)"
  , Cell "decode_params" 0x014c5888 0x28 "private-data/analysis/x-m1-1.50/af/functions/00d99714.c, 0065a58c.c"
  , Cell "playback_decode_job" 0x014eae2c 0x50 "private-data/analysis/x-m1-1.50/af/functions/01220294.c, 0122013c.c"
  , Cell "playback_file_io" 0x014eaca8 0x30 "private-data/analysis/x-m1-1.50/af/functions/01220314.c, 01220378.c"
  , Cell "property_1b1" 0x00956844 4 "af/functions/0063386c.c, 00c595f4.c"
  , Cell "property_13a" 0x00956974 4 "af/functions/00634690.c, 00c595f4.c"
  , Cell "mailbox_divert_71" 0x00950848 8 "af/functions/00d1740c.c"
  , Cell "request_206_result" 0x00954d68 4 "af/functions/00db415c.c"
  , Cell "modal_1f_state" 0x014675c6 2 "bfb028 state cell 0x01467588+0x1f*2"
  , Cell "modal_100_state" 0x01467650 2 "bfb028 state cell 0x01467588+100*2"
  , Cell "completion_selector" 0x00951548 8 "af/functions/00d27c7c.c, 00c6ce48.c (0x0095154c)"
  , Cell "reccont_rows" 0x01545790 0xc0 "af/functions/00df74ec.c (row cell 0x01545790+m*2)"
  , Cell "reccont_message" 0x01526a78 0x20 "af/functions/00dfefe4.c, 00dd5d8c.c"
  , Cell "reccont_request" 0x0152c774 0x70 "af/functions/00dd5d8c.c, 00e1ce74.c, 00e35204.c"
  , Cell "reccont_query" 0x0152a884 0x0c "af/functions/00dd6820.c"
  , Cell "image_input_channels" 0x009431f8 0x60 "af/functions/00663040.c"
  , Cell "raw_stream" 0x01698a50 0xf0 "af/functions/00edb630.c, 00ee0f4c.c, 00ed84cc.c (mode +0x04, client +0x08, descriptor +0x24)"
  , Cell "raw_stream_ready" 0x01698b4c 4 "af/functions/00ed84cc.c, 00edbad4.c"
  , Cell "raw_image_input_handle" 0x016994f8 4 "af/functions/00edb630.c:134"
  , Cell "raw_window_flags" 0x01699698 4 "af/functions/00ee71a0.c:12"
  , Cell "raw_window_rect" 0x016996b8 8 "af/functions/00ee71a0.c:12, 0065f51c.c"
  , Cell "raw_width_base" 0x01699848 2 "af/functions/0065f8bc.c caller 0x00ee740c"
  , Cell "raw_line_36_gate" 0x0089f6e0 1 "af/functions/00664c28.c"
  , Cell "raw_client_queue" 0x016f0aa4 4 "af/functions/00fe9580.c, 00fe9024.c:17"
  , Cell "reccont_class" 0x0152bd5c 4 "af/functions/00ddb3ac.c, 00dfa144.c"
  ]

type CellValues = Map.Map T.Text B.ByteString

readCells :: Bridge -> IO CellValues
readCells bridge = Map.fromList <$> forM cells (\cell -> do
  reply <- call bridge (command "read-memory" ["address" .= cellAddress cell, "count" .= cellWidth cell])
  bytes <- either (harness . ("read-memory: " ++)) pure (maybe (Left "missing bytes") decodeHex (textAt "bytes" reply))
  require (B.length bytes == cellWidth cell) "read-memory returned a different length"
  pure (cellName cell, bytes))

cellWord :: CellValues -> T.Text -> Maybe Word64
cellWord values name = do
  bytes <- Map.lookup name values
  if B.length bytes <= 8 then Just (le bytes 0 (B.length bytes)) else Nothing

cellsValue :: CellValues -> Value
cellsValue values = object
  [ K.fromText (cellName cell) .= object
      (["address" .= hex32 (cellAddress cell), "bytes" .= maybe "" hexBytes bytes, "source" .= cellSource cell]
        ++ ["value" .= w | Just w <- [cellWord values (cellName cell)]])
  | cell <- cells, let bytes = Map.lookup (cellName cell) values ]

-- Trace sites ---------------------------------------------------------------------------------

data Site = Site { siteAddress :: Word32, siteLabel :: T.Text, siteSource :: T.Text }

-- | Decode-engine sites shared with live-view develop job 4; they do not count as route activity.
developSites :: [Site]
developSites =
  [ Site 0x00d9882c "decode-request" "private-data/analysis/x-m1-1.50/af/functions/00d9882c.c"
  , Site 0x00d99714 "decode-submit" "private-data/analysis/x-m1-1.50/af/functions/00d99714.c"
  , Site 0x0065a58c "decode-program" "private-data/analysis/x-m1-1.50/af/functions/0065a58c.c"
  , Site 0x0065a548 "fff63-start" "reports/full-system-emulation.md:496"
  , Site 0x00d99570 "fff63-irq-handler" "reports/full-system-emulation.md:496" ]

commonSites :: [Site]
commonSites =
  [ Site 0x00c09d24 "input-consumer" "reports/full-system-emulation.md:492"
  , Site 0x00d350c4 "ui-mode-request" "private-data/analysis/x-m1-1.50/af/functions/00d350c4.c"
  , Site 0x00d1ba2c "ui-mode-3-to-4-setup" "private-data/analysis/x-m1-1.50/af/functions/00d3c3b4.c"
  , Site 0x00d28fe4 "ui-mode-4-to-3-setup" "private-data/analysis/x-m1-1.50/af/functions/00d3c3b4.c"
  , Site 0x0071c648 "file-open" "reports/af-observe-instruments-150.md:167"
  , Site 0x0071c6fc "file-read" "E-FILE-ABI-150"
  , Site 0x0071c75c "file-write" "reports/cardless-capture-feasibility.md:475"
  , Site 0x0071c7bc "file-close" "reports/cardless-capture-feasibility.md:475"
  , Site 0x00d1d464 "warning-select-entry" "reports/af-observation-plan.md:76-77"
  , Site 0x00d3bb18 "warning-select" "reports/af-observation-plan.md:77"
  , Site 0x00d3b64c "warning-present" "reports/af-observation-plan.md:77"
  , Site 0x00d39410 "indicator-request" "reports/af-observation-plan.md:79"
  , Site 0x006a4d84 "indicator-driver" "reports/af-observation-plan.md:79"
  ] ++ developSites

playSites :: [Site]
playSites =
  [ Site 0x00d0ad8c "play-mode-redecision" "private-data/analysis/x-m1-1.50/af/functions/00d0ad8c.c"
  , Site 0x01220294 "playback-decode-request" "private-data/analysis/x-m1-1.50/af/functions/01220294.c"
  , Site 0x011c2ecc "exif-parser-init" "reports/adjustment-and-service-interfaces.md:187"
  , Site 0x011c36f8 "exif-parser" "reports/adjustment-and-service-interfaces.md:187"
  , Site 0x01118a00 "playback-info" "reports/adjustment-and-service-interfaces.md:187" ]

shutterSites :: [Site]
shutterSites =
  [ Site 0x006a2f94 "s1-irq" "emulation/Xm1HalfPressE2E.hs:120"
  , Site 0x006a2f80 "s2-irq" "af/functions/006a2f80.c; GPIO0.3 IRQ11"
  , Site 0x00c58c38 "s1-action" "reports/af-observation-plan.md:73"
  , Site 0x00d24e34 "admission-predicate" "reports/af-observation-plan.md:74"
  , Site 0x00c58ccc "admission-result" "reports/af-observation-plan.md:74"
  , Site 0x00d19324 "media-predicate" "reports/af-observation-plan.md:75"
  , Site 0x00c58d60 "admission-refusal" "reports/af-observation-plan.md:74"
  , Site 0x00c59578 "s2-state-4-warmup" "reports/af-observation-plan.md:87"
  , Site 0x00c595f4 "s2-state-6-capture" "reports/af-observation-plan.md:87"
  , Site 0x00d24b58 "s2-precapture" "reports/af-observation-plan.md:87"
  , Site 0x00c58fc4 "s2-ordinary-action" "reports/cardless-capture-feasibility.md:465"
  , Site 0x00df48dc "af-request" "reports/autofocus-static-map.md:47"
  , Site 0x00e531ac "codec-setup" "reports/cardless-capture-feasibility.md:455"
  , Site 0x011d4a20 "codec-start" "reports/cardless-capture-feasibility.md:455"
  , Site 0x0065adbc "fff62-start" "private-data/analysis/x-m1-1.50/af/functions/0065adbc.c"
  , Site 0x0070fbfc "dcf-advance" "reports/af-observe-instruments-150.md:167"
  , Site 0x00716324 "dcf-allocate" "reports/af-observation-plan.md:171"
  , Site 0x00ea0934 "save-dispatch" "reports/cardless-capture-feasibility.md:467"
  , Site 0x00e99d4c "save-frame" "reports/cardless-capture-feasibility.md:467"
  , Site 0x00e518c0 "save-descriptor" "reports/cardless-capture-feasibility.md:467"
  , Site 0x011d5600 "storage-machine-a" "reports/cardless-capture-feasibility.md:475"
  , Site 0x011d5688 "storage-machine-b" "reports/cardless-capture-feasibility.md:475"
  , Site 0x00df7cd8 "completion-route" "reports/cardless-capture-feasibility.md:491"
  , Site 0x00d1350c "request-206" "af/functions/00d1350c.c"
  , Site 0x00d27c7c "ui-837-816" "af/functions/00d27c7c.c"
  , Site 0x00c2feb4 "modal-1f-837" "id5 0x00ce02b4 slot 45; modal 0x1f state 1 event 0x1a"
  , Site 0x00c30c54 "modal-1f-816" "id5 0x00ce02b4 slot 98; modal 0x1f state 1 event 0x30"
  , Site 0x00c6c938 "modal-4a-97-state-6" "id5 cell 0x00cf00a8 slot 0x00d01c84"
  , Site 0x00c6ce48 "modal-4a-97-state-8" "id5 cell 0x00cf00b0 slot 0x00d01c8c"
  , Site 0x00c6ce54 "modal-4a-97-state-8-return" "id5.dis 0x00c6ce50 bl 0x00c12aac; r0 is its return"
  , Site 0x00e1dc9c "reccont-15-command-19-state-5" "id5 machine 0x15 row 4 state 5"
  , Site 0x00e1dcf4 "reccont-15-df2378-call" "id5.dis 0x00e1dcf4 bl 0x00df2378; r0 r1 r2 are its arguments"
  , Site 0x00e1dcf8 "reccont-15-df2378-result" "id5.dis 0x00e1dcf8 cmp r0,#0x7e; r0 is the 0x00df2378 result"
  , Site 0x00ddb914 "reccont-4d-completion-inputs" "id5.dis 0x00ddb914 bl 0x00dd68b8; r0 machine-0x14 result, r1 machine-0x12 result"
  , Site 0x00dcb6c0 "ui-notification-post" "af/functions/00e1dc9c.c, 00e1c758.c; r0 is the UI id"
  , Site 0x00edb630 "raw-stream-start" "af/functions/00edb630.c; r0 param_1, r2 configuration, r3 client"
  , Site 0x00ed84cc "raw-stream-callback" "af/functions/00ed84cc.c"
  , Site 0x00ed8d20 "raw-stream-stop" "af/functions/00ed8d20.c"
  , Site 0x00ee0e50 "raw-stream-clear" "af/functions/00ee0e50.c"
  , Site 0x00fe9580 "raw-client-post" "af/functions/00fe9580.c; r0 is the message id"
  , Site 0x00dfeff8 "reccont-4e-message" "id5.dis 0x00dfeff4 ldrh r3,[0x01526a78]; r3 is the message id"
  , Site 0x00dd5d8c "reccont-15-command" "af/functions/00dd5d8c.c, 00dfacd8.c; r0 is the command"
  , Site 0x00e1ce74 "reccont-15-command-1a" "id5 machine 0x15 row 3 state 0"
  , Site 0x00e1d3cc "reccont-15-command-19-idle" "id5 machine 0x15 row 4 state 0; returns 1"
  , Site 0x00e1d3e4 "reccont-15-command-19" "id5 machine 0x15 row 4 state 1"
  , Site 0x00df384c "reccont-submit" "af/functions/00e1d3e4.c, 00e1ce74.c; r0 is the operation"
  , Site 0x00e360ec "reccont-4e-state-6-completion" "id5 machine 0x4e row 12 state 6"
  , Site 0x00e36208 "reccont-4e-state-9-completion" "id5 machine 0x4e row 12 state 9"
  , Site 0x00e36218 "reccont-4e-state-9-query" "id5.dis 0x00e36214 bl 0x00dd6820; r0 is its return"
  , Site 0x00e1e5e0 "reccont-201-start" "reports/cardless-capture-feasibility.md:290"
  , Site 0x00e1eaf4 "reccont-816-publisher" "reports/cardless-capture-feasibility.md:290"
  , Site 0x00663040 "image-input-channel-open" "af/functions/00663040.c; r0 channel, r1 callback"
  , Site 0x00d9f348 "raw-open-call" "id5 A32 ebe30f3c ->00663040"
  , Site 0x00d9f34c "raw-open-result" "id5 A32 cmp r0,0; return from00663040"
  , Site 0x00663174 "image-input-register-irq-call" "id4.dis; BL 0064e918, r0 descriptor r1 handler r2 handle output"
  , Site 0x00663178 "image-input-register-irq-result" "id4.dis; r0 is 0064e918 result"
  , Site 0x00663198 "image-input-channel-2-inhibition" "id4.dis; r3 is 0089f6e0 at registration"
  , Site 0x006631bc "image-input-channel-2-enable-call" "id4.dis; BL 00664760 with channel,1,1"
  , Site 0x006631f8 "image-input-channel-open-result" "id4.dis; r0 is 00663040 result"
  , Site 0x00dd4474 "recorder-service-callback" "af/functions/00dd4474.c; event103 ->1354, event114 ->1351"
  , Site 0x00e1557c "machine-14-event-1354-state-2" "id5 matrix e3f8e8; transition bytefd ->state3"
  , Site 0x00e71a54 "recorder-service-alternate-103" "af/functions/00e71a54.c"
  , Site 0x00e7d6f0 "recorder-service-103-producer" "af/functions/00e7d6f0.c"
  , Site 0x00e7e2ec "recorder-service-action-94" "af/functions/00e7e2ec.c"
  , Site 0x00e74954 "recorder-service-103-predicate" "af/functions/00e74954.c"
  , Site 0x00e843d4 "recorder-service-action" "af/functions/00e843d4.c; r0 context r1 action"
  , Site 0x00e85378 "recorder-service-scan" "af/functions/00e85378.c; table e8a5bc"
  ] ++ imageInputStoreSites ++
  [ Site 0x00664c28 "image-input-line-36" "id4 0x007c2d28/0x007c2d3c; descriptor 0x27 line 36"
  , Site 0x00664c80 "image-input-line-37" "id4 0x007c2d28/0x007c2d3c; descriptor 0x28 line 37"
  , Site 0x00664cd8 "image-input-line-38" "id4 0x007c2d28/0x007c2d3c; descriptor 0x29 line 38" ]

-- | `str r2, [r3]` instructions whose r3 is an image-input register; at entry r2 is the value.
imageInputStoreSites :: [Site]
imageInputStoreSites =
  [ Site a ("image-input-store-" <> T.pack (drop 2 (hex32 a))) s
  | (a, s) <-
      [ (0x006639bc, "id4.dis 0x0066382c channel-2/3 programming")
      , (0x00663a4c, "id4.dis 0x0066382c channel-2/3 programming")
      , (0x00663a88, "id4.dis 0x0066382c channel-2/3 programming")
      , (0x00663abc, "id4.dis 0x0066382c channel-2/3 programming")
      , (0x0065f724, "id4.dis 0x0065f51c window +0x14")
      , (0x0065f790, "id4.dis 0x0065f51c window +0x18")
      , (0x0065f954, "id4.dis 0x0065f8bc width +0x24")
      , (0x0065e380, "id4.dis 0x0065e2c4 route +0x004")
      , (0x0066484c, "id4.dis 0x00664760 interrupt enable")
      , (0x00664968, "id4.dis 0x00664760 interrupt enable")
      , (0x00664a3c, "id4.dis 0x00664760 interrupt enable") ] ]

isImageInputStore :: Hit -> Bool
isImageInputStore hit = "image-input-store-" `T.isPrefixOf` hitLabel hit

routeSites :: Route -> [Site]
routeSites PlayRoute = commonSites ++ playSites
routeSites ShutterContactRoute = commonSites ++ shutterSites
routeSites ShutterSequenceRoute = commonSites ++ shutterSites

-- | X-M1 1.50 image4/image5 bindings from emulation/README.md at def5d89 and the pinned lz/*.dis.
captureContinuationSites :: [Site]
captureContinuationSites =
  [ Site 0x00ef9064 "capture-request" "image5 ef9064; r0 request callback, r1-r3 request arguments"
  , Site 0x00ef9130 "capture-operation-7-post" "image5 eb000257 BL ef9a94; r1 payload, r2=0x3c"
  , Site 0x00ef9a94 "frontend-post" "image5 ef9a94; r0 operation, r1 payload, r2 length"
  , Site 0x00ef9ae0 "frontend-enqueue-call" "image5 ebe0ad55 BL 72503c; r0=0x2b, r1 descriptor12"
  , Site 0x00ef9ae4 "frontend-enqueue-result" "image5 e24bd00c; r0 enqueue result, zero success"
  , Site 0x00f01bf8 "frontend-receive-result" "image5 e3500000; r0 queue-0x2b receive result"
  , Site 0x00f01c40 "frontend-receiver-dispatch" "image5 ebffebd0 BL efcb88; r0 operation, message0169d374"
  , Site 0x00efca6c "frontend-selector-entry" "image5 efca6c; r0 selector, r1/r2 action arguments"
  , Site 0x00efcaf0 "frontend-selector-state-row" "image5 eb012850 BL f46c38; r0 selector, r1 state, r2 row"
  , Site 0x00efcb20 "frontend-action-invoke" "image5 e12fff33 BLX r3; r3 action pointer, r0/r1 arguments"
  , Site 0x00efcb24 "frontend-action-result" "image5 e1a04000; r0 selected action result"
  , Site 0x00f023e4 "capture-root-state-4" "image5 root operation7 state4 action; emulation/README.md def5d89"
  , Site 0x00f02430 "capture-selector-b-result" "image5 e3500001; r0 efe584(0xab,context) result"
  , Site 0x00f037c8 "capture-selector-3-e9" "image5 selector3 evente9 state0; request byte0169d508"
  , Site 0x00f0ba44 "capture-frontend-start" "image5 selector12 event103 state0; context016b7510"
  , Site 0x00f29160 "capture-queue30-request" "image5 f29160 calls1039dd4 with callback f2907c"
  , Site 0x01039dd4 "capture-queue30-api" "image5 1039dd4; r0/r1 args, r2 callback; 4007 payload12"
  , Site 0x01039e18 "capture-queue30-enqueue-call" "image5 ebdbac87 BL72503c; r0=0x30, r1 descriptor12"
  , Site 0x01039e1c "capture-queue30-enqueue-result" "image5 e3500000; r0 enqueue result, zero success"
  , Site 0x01028b4c "capture-queue30-received" "image5 e1d5c0b4; queue30 header016fea2c"
  , Site 0x01028bc4 "capture-queue30-dispatch" "image5 e12fff33 BLX r3; selected receiver action"
  , Site 0x0102957c "capture-queue30-callback-a" "image5 e12fff33 BLX r3; pointer016fe9d4, r0=0"
  , Site 0x0102fd20 "capture-queue30-callback-b" "image5 e12fff33 BLX r3; pointer016fe9d4, r0=0"
  , Site 0x010300a4 "capture-queue30-callback-c" "image5 e12fff33 BLX r3; pointer016fe9d4, r0=0"
  , Site 0x01030284 "capture-queue30-callback-d" "image5 e12fff33 BLX r3; pointer016fe9d4, r0=0"
  , Site 0x01030310 "capture-queue30-callback-e" "image5 e12fff33 BLX r3; pointer016fe9d4, r0=0"
  , Site 0x00f2907c "capture-queue30-callback-entry" "image5 f2907c; r0 callback result; posts operation83"
  , Site 0x00f0bafc "capture-frontend-83" "image5 selector12 event83 state1; emulation/README.md def5d89"
  , Site 0x00f0bb80 "capture-controller-selector-call" "image5 BL f00094; event106 selects selector16"
  , Site 0x00f0c0dc "capture-controller-preparation" "image5 selector16 event106 state0"
  , Site 0x00f2d8b0 "capture-controller-parameters" "image5 f2d8b0 builds capture parameters0x98"
  , Site 0x00f2dce8 "capture-controller-api-call" "image5 BL feac00; six callbacks and parameters"
  , Site 0x00feac00 "capture-controller-api" "image5 feac00; 3f49 payload0xb0"
  , Site 0x00fe95bc "capture-imgdc-enqueue-call" "image5 ebdcee9e BL72503c; r1 descriptor12, queue016f0aa4"
  , Site 0x00fe95c0 "capture-imgdc-enqueue-result" "image5 e24bd00c; r0 enqueue result, zero success"
  , Site 0x00fe8ac4 "capture-imgdc-receiver" "image5 fe8ac4; message016f0aa8"
  , Site 0x0100297c "capture-controller-event" "image5 100297c; r0 event, state016f83ac"
  , Site 0x01000860 "capture-controller-install" "image5 1000860 copies parameters and six callbacks"
  , Site 0x00ffe610 "capture-timing-subscribe-call" "image5 ebdd0aeb BL7411c4; selector1, callback ffe654"
  , Site 0x00ffe614 "capture-timing-subscribe-result" "image5 e8bd8800; r0 subscription result, handle016f83c4"
  , Site 0x00740f0c "capture-timing-irq41" "image4 selector1 tag2b IRQ41 handler; emulation/README.md def5d89"
  , Site 0x00740f64 "capture-timing-callback-invoke" "image4 e12fff33 BLX r3; subscriber pointer"
  , Site 0x00ffe654 "capture-timing-subscriber" "image5 ffe654 calls fe96a8"
  , Site 0x00fe96a8 "capture-timing-post" "image5 fe96a8 posts3f00 through fe9580"
  , Site 0x00664e90 "capture-timing-ack-store" "image4 664e90; timing acknowledge store operands"
  , Site 0x00665074 "capture-timing-config-store" "image4 665074; timing configuration store operands"
  , Site 0x0100231c "capture-controller-state-4-tick" "image5 state4 event3f00 counter/target handler"
  , Site 0x01002708 "capture-controller-callback-invoke" "image5 e12fff33 BLX r3; pointer016f8308, r0=0, r1 result"
  , Site 0x00f2c9c4 "capture-controller-callback-5e" "image5 f2c9c4 posts operation5e, not receipt137e"
  , Site 0x01001ed0 "capture-controller-state-5-tick" "image5 state5 event3f00 increments016f8419"
  , Site 0x00edbdd8 "capture-raw-gate-release-wrapper" "image5 edbdd8 calls006651c8"
  , Site 0x006651c8 "capture-raw-gate-release" "image4 6651c8 clears0089f6e0, acknowledges2/3, enables2"
  , Site 0x00efae7c "capture-request-callback-invoke" "image5 e12fff33 BLX r3; request callback r3, result r0"
  , Site 0x00e7f9ec "capture-receipt-callback-a" "image5 e7f9ec; r0 result; zero arm sends137e"
  , Site 0x00e6efd4 "capture-receipt-callback-b" "image5 e6efd4; r0 result; zero arm sends137e"
  , Site 0x00e6f100 "capture-receipt-callback-c" "image5 e6f100; r0 result; zero arm sends137e"
  , Site 0x00e6c444 "capture-receipt-callback-d" "image5 e6c444; r0 result; zero arm sends137e"
  , Site 0x00e7fca0 "capture-receipt-enqueue-a" "image5 ebe294e5 BL72503c; r1 descriptor12, payload4"
  , Site 0x00e7fca4 "capture-receipt-result-a" "image5 e3500000; r0 enqueue result, zero success"
  , Site 0x00e6f0e0 "capture-receipt-enqueue-b" "image5 ebe2d7d5 BL72503c; r1 descriptor12, payload4"
  , Site 0x00e6f0e4 "capture-receipt-result-b" "image5 e3500000; r0 enqueue result, zero success"
  , Site 0x00e6f260 "capture-receipt-enqueue-c" "image5 ebe2d775 BL72503c; r1 descriptor12, payload4"
  , Site 0x00e6f264 "capture-receipt-result-c" "image5 e3500000; r0 enqueue result, zero success"
  , Site 0x00e6c490 "capture-receipt-enqueue-d" "image5 ebe2e2e9 BL72503c; r1 descriptor12, payload4"
  , Site 0x00e6c494 "capture-receipt-result-d" "image5 e3500000; r0 enqueue result, zero success"
  , Site 0x00e8659c "capture-receipt-dispatch" "image5 entry; r0 caller descriptor, before copy to015ad430"
  , Site 0x00e869b8 "capture-receipt-propagate-bit-3" "image5 ebffbb04 BL e755d0; r1=3"
  , Site 0x00e869cc "capture-receipt-propagate-bit-1" "image5 ebffbaff BL e755d0; r1=1"
  , Site 0x00e86a1c "capture-receipt-set-bit-3" "image5 eb00a91f BL eb0ea0; r1=3"
  , Site 0x00e86a30 "capture-receipt-set-bit-1" "image5 eb00a91a BL eb0ea0; r1=1"
  ]

data Hit = Hit { hitLabel :: T.Text, hitRegisters :: [Maybe Word32], hitInstructions :: Maybe Integer, hitRaw :: Value }
  deriving (Generic)
instance NFData Hit

hitR :: Int -> Hit -> Maybe Word32
hitR index hit = case drop index (hitRegisters hit) of
  r : _ -> r
  [] -> Nothing

hitR0 :: Hit -> Maybe Word32
hitR0 = hitR 0

parseHit :: Value -> Maybe Hit
parseHit value@(Object o) = do
  site <- textAt "site" o
  let register name = objectAt "registers" o >>= textAt name >>= parseHex32
  pure (Hit site (map register ["r0", "r1", "r2", "r3"]) (num "instructions" o) value)
parseHit _ = Nothing

traceHits :: Bridge -> Int -> IO (Int, [Hit])
traceHits bridge from = do
  reply <- call bridge (command "trace-hits" ["from" .= from])
  count <- maybe (harness "trace-hits lacks count") pure (num "count" reply)
  hits <- case KM.lookup "hits" reply of
    Just (Array items) -> pure (mapMaybe parseHit (toList items))
    _ -> harness "trace-hits lacks hits"
  pure (count, hits)

data CaptureProgress = CaptureProgress
  { captureCoverage :: CS.Coverage
  , captureLastPage :: Maybe CS.Page
  , captureCursor :: CS.Cursor
  , capturePagesComplete :: Bool
  }

modifyCaptureProgress :: Observed -> (CaptureProgress -> CaptureProgress) -> IO ()
modifyCaptureProgress observed update = modifyIORef' (obsCapture observed) (fmap update)

captureIncomplete :: CaptureProgress -> Bool
captureIncomplete progress = not (capturePagesComplete progress) || CS.coverageIncomplete (captureCoverage progress)

captureProgressValue :: CaptureProgress -> Value
captureProgressValue progress = case CS.coverageValue (captureCoverage progress) of
  Object coverage -> Object (KM.fromList
    [ "metadata_collected" .= isJust (captureLastPage progress)
    , "incomplete" .= captureIncomplete progress
    , "total" .= fmap CS.pageTotal (captureLastPage progress)
    , "captured" .= fmap CS.pageCaptured (captureLastPage progress)
    , "pending" .= (if capturePagesComplete progress then Just (0 :: Word64) else Nothing)
    , "last_page_pending" .= fmap CS.pagePending (captureLastPage progress)
    , "last_page_through_instructions" .= fmap CS.pageThrough (captureLastPage progress)
    , "dropped" .= fmap CS.pageDropped (captureLastPage progress)
    , "first_instructions" .= (captureLastPage progress >>= CS.pageFirst)
    , "last_instructions" .= (captureLastPage progress >>= CS.pageLast)
    , "sites" .= fmap (map CS.siteValue . CS.pageSites) (captureLastPage progress) ] <> coverage)
  _ -> Null

captureObservationHits :: Bridge -> Journal -> Observed -> IO Bool
captureObservationHits bridge journal observed = do
  previous <- readIORef (obsCapture observed) >>= maybe (harness "capture was not begun") pure
  let previousTotal = maybe 0 CS.pageTotal (captureLastPage previous)
      go = do
        progress <- readIORef (obsCapture observed) >>= maybe (harness "capture was not begun") pure
        reply <- call bridge (CS.hitsRequest (captureCursor progress))
        page <- CS.journalPage journal (captureCursor progress) (Object reply)
        coverage <- either harness pure (CS.collectBoundary page (captureCoverage progress))
        metadata <- evaluate (CS.pageMetadata page)
        modifyCaptureProgress observed (\p -> p { captureCoverage = coverage, captureLastPage = Just metadata })
        unless (null (CS.pageHits page)) $ do
          ack <- call bridge (CS.ackRequest (CS.pageNext page))
          either harness pure (CS.validateAck (CS.pageNext page)
            (CS.pagePending page - fromIntegral (length (CS.pageHits page))) (Object ack))
        let hits = [Hit (CS.eventLabel hit) (map Just (CS.eventRegisters hit))
                      (Just (toInteger (CS.instructionValue (CS.eventInstructions hit)))) Null
                   | hit <- CS.pageHits page]
        foldCaptureHits observed hits
        let complete = null hits && CS.pagePending page == 0 && CS.pageBoundaryComplete page
        modifyCaptureProgress observed (\p -> p
          { captureCoverage = coverage, captureLastPage = Just metadata
          , captureCursor = CS.pageNext page, capturePagesComplete = complete })
        if complete then do
          modifyIORef' (obsLedger observed) $ \ledger ->
            let totals = Map.fromList [(CS.siteLabel s, toInteger (CS.siteTotal s)) | s <- CS.pageSites metadata]
            in force ledger { ledgerCounts = Map.union (Map.unionWith (+) totals (ledgerBeforePress ledger)) (ledgerCounts ledger) }
          collected <- readIORef (obsCapture observed)
          note journal "capture-observation-interval" (maybe Null captureProgressValue collected)
          pure (CS.pageTotal page > previousTotal || CS.pageIncomplete page)
        else do
          require (not (null hits)) "capture boundary remains incomplete without collectable detail"
          go
  modifyCaptureProgress observed (\p -> p { capturePagesComplete = False })
  go

captureAdvance :: Bridge -> Journal -> Observed -> CS.Operation -> Value -> IO Fields
captureAdvance bridge journal observed operation input = do
  progress <- readIORef (obsCapture observed) >>= maybe (harness "capture was not begun") pure
  coverage <- either harness pure (CS.openInterval operation (captureCoverage progress))
  modifyCaptureProgress observed (\p -> p { captureCoverage = coverage, capturePagesComplete = False })
  let advance request = do
        reply <- call bridge request
        progressReply <- either harness pure (CS.parseAdvance operation (Object reply))
        current <- readIORef (obsCapture observed) >>= maybe (harness "capture was not begun") pure
        either harness pure (CS.validateAdvance progressReply (captureCoverage current))
        note journal "capture-observation-progress" (Object reply)
        _ <- captureObservationHits bridge journal observed
        collected <- readIORef (obsCapture observed) >>= maybe (harness "capture was not begun") pure
        require (fmap CS.pageThrough (captureLastPage collected) == Just (CS.advanceInstructions progressReply))
          "capture collection changed the guest instruction count"
        case CS.advanceState progressReply of
          CS.Completed -> pure reply
          CS.DrainRequired -> do
            require (captureCursor collected > captureCursor current)
              "capture drain-required response has no pending event detail"
            advance CS.resumeRequest
  advance input

-- | Existing verdict samples retain registers and instruction counts; event RAM remains in the page journal.
foldCaptureHits :: Observed -> [Hit] -> IO ()
foldCaptureHits observed fullHits = do
  ledger <- readIORef (obsLedger observed)
  let (ledger', kept) = absorb ledger fullHits
      evidenceLabels =
        [ "admission-result", "image-input-channel-open", "raw-stream-start", "raw-open-call", "raw-open-result"
        , "image-input-register-irq-call", "image-input-register-irq-result", "image-input-channel-2-inhibition"
        , "image-input-channel-2-enable-call", "raw-client-post", "ui-notification-post", "reccont-15-df2378-call"
        , "reccont-15-df2378-result", "reccont-4d-completion-inputs", "reccont-4e-message", "reccont-15-command"
        , "reccont-submit", "reccont-4e-state-9-query", "modal-4a-97-state-8-return", "file-read" ]
  evaluatedLedger <- evaluate (force ledger')
  writeIORef (obsLedger observed) evaluatedLedger
  compactHits <- forM (filter ((`elem` evidenceLabels) . hitLabel) kept) $ \hit -> do
    let label = hitLabel hit
        registers = hitRegisters hit
        at = hitInstructions hit
        compact = object ["site" .= label, "registers" .= registers, "instructions" .= at]
    evaluate (rnf (label, registers, at, compact) `seq` Hit label registers at compact)
  appendKeptHits (obsHits observed) compactHits

appendKeptHits :: IORef [Hit] -> [Hit] -> IO ()
appendKeptHits _ [] = pure ()
appendKeptHits ref hits = do
  previous <- readIORef ref
  combined <- evaluate (force (previous ++ hits))
  writeIORef ref combined

hitsAt :: T.Text -> [Hit] -> [Hit]
hitsAt label = filter ((== label) . hitLabel)

-- | Sites that recur once per live-view develop pass or per RAW stream frame.
periodicLabels :: [T.Text]
periodicLabels = map siteLabel developSites ++ map siteLabel imageInputStoreSites
  ++ ["image-input-line-36", "image-input-line-37", "image-input-line-38", "raw-stream-callback", "raw-client-post"]

-- | Route-site entries other than periodic ones; FIXED_SITES hits are excluded.
routeActivity :: Route -> Hit -> Bool
routeActivity route hit = hitLabel hit `elem` map siteLabel (routeSites route) && hitLabel hit `notElem` periodicLabels

-- | Hits kept per label; every hit is still counted and folded into the register file.
hitCap :: Int
hitCap = 64

-- | Image-input values from the latest traced store per offset, with the aggregate instruction count at that store.
-- Untraced stores and device-side register changes are absent.
type TracedConfiguration = Map.Map Word32 (Word32, Integer)

imageInputBase :: Word32
imageInputBase = 0xfff10000

-- | The traced configuration as of a line-36 handler entry.
data Line36 = Line36 { line36Instructions :: Integer, line36Configuration :: TracedConfiguration }
  deriving (Generic)
instance NFData Line36

data TracePhase = BeforePress | AfterPress

data TraceLedger = TraceLedger
  { ledgerCounts :: Map.Map T.Text Integer
  , ledgerBeforePress :: Map.Map T.Text Integer
  , ledgerConfiguration :: TracedConfiguration
  , ledgerFirstLine36 :: Maybe Line36
  , ledgerLastLine36 :: Maybe Line36
  }
  deriving (Generic)
instance NFData TraceLedger

emptyLedger :: TraceLedger
emptyLedger = TraceLedger Map.empty Map.empty Map.empty Nothing Nothing

-- | Hits of a label since the press; the cap applies to these.
sincePress :: TraceLedger -> T.Text -> Integer
sincePress ledger label = Map.findWithDefault 0 label (ledgerCounts ledger) - Map.findWithDefault 0 label (ledgerBeforePress ledger)

-- | Folds one window's hits in order; returns the ledger and the hits kept from this window.
absorb :: TraceLedger -> [Hit] -> (TraceLedger, [Hit])
absorb start hits =
  let (ledger, reversed) = foldl' step (start, []) hits
  in (ledger, reverse reversed)
  where
    step (ledger, kept) hit =
      let label = hitLabel hit
          seen = sincePress ledger label
          instructions = fromMaybe 0 (hitInstructions hit)
          registers = case (isImageInputStore hit, hitR 2 hit, hitR 3 hit) of
            (True, Just value, Just address) | address >= imageInputBase && address < imageInputBase + 0x400 ->
              Map.insert (address - imageInputBase) (value, instructions) (ledgerConfiguration ledger)
            _ -> ledgerConfiguration ledger
          line36 = Line36 instructions registers
          isLine36 = label == "image-input-line-36"
          ledger' = ledger
            { ledgerCounts = Map.insertWith (+) label 1 (ledgerCounts ledger)
            , ledgerConfiguration = registers
            , ledgerFirstLine36 = if isLine36 && isNothing (ledgerFirstLine36 ledger) then Just line36 else ledgerFirstLine36 ledger
            , ledgerLastLine36 = if isLine36 then Just line36 else ledgerLastLine36 ledger }
      in (ledger', if seen < fromIntegral hitCap then hit : kept else kept)

tracedConfigurationValue :: TracedConfiguration -> Value
tracedConfigurationValue file = object
  [ K.fromString (hex32 offset) .= object ["value" .= hex32 value, "instructions" .= at] | (offset, (value, at)) <- Map.toList file ]

-- Route ---------------------------------------------------------------------------------------

data Shot = Shot { shotSha :: T.Text, shotValue :: Value }

capture :: Bridge -> T.Text -> IO Shot
capture bridge label = do
  reply <- call bridge (command "capture" ["label" .= label])
  sha <- maybe (harness "capture lacks sha256") pure (textAt "sha256" reply)
  pure (Shot sha (Object reply))

menuOf :: Fields -> Maybe (Word32, Word32, Word32)
menuOf reply = do
  menu <- objectAt "menu" reply
  (,,) <$> num "menu_id" menu <*> num "setting_id" menu <*> num "cursor" menu

key :: Bridge -> T.Text -> IO Fields
key bridge name = call bridge (command "key" ["key" .= name])

-- | SHOOT WITHOUT LENS ON through ordinary MENU keys, the stock route of
-- emulation/Xm1HalfPressE2E.hs prepareNoLens: shooting-menu row 20, setting 0xa3, ON cursor 0.
shootWithoutLens :: Bridge -> IO [Value]
shootWithoutLens bridge = do
  opened <- key bridge "ok"
  rows <- seek (64 :: Int) [opened] opened
  submenu <- key bridge "right"
  require (fmap (\(_, s, _) -> s) (menuOf submenu) == Just 0xa3) "stock navigation did not open SHOOT WITHOUT LENS (setting 0xa3)"
  selected <- key bridge "up"
  require (fmap (\(_, _, c) -> c) (menuOf selected) == Just 0) "stock navigation did not select SHOOT WITHOUT LENS ON"
  committed <- key bridge "ok"
  closed <- key bridge "back"
  pure (map Object (rows ++ [submenu, selected, committed, closed]))
  where
    seek remaining history current = case menuOf current of
      Just (_, _, 20) -> pure (reverse history)
      _ -> do
        require (remaining > 0) "stock navigation did not reach shooting-menu row 20"
        next <- key bridge "down"
        seek (remaining - 1) (next : history) next

data Observed = Observed
  { obsPressReply :: IORef (Maybe Fields)
  , obsCommands :: IORef [(Int, SdCommand)]
  , obsLost :: IORef Word64
  , obsHits :: IORef [Hit]
  , obsCellsBefore :: IORef (Maybe CellValues)
  , obsCellsAfterPress :: IORef (Maybe CellValues)
  , obsCellsFinal :: IORef (Maybe CellValues)
  , obsSdBefore :: IORef (Maybe SdState)
  , obsSdFinal :: IORef (Maybe SdState)
  , obsShotBefore :: IORef (Maybe Shot)
  , obsShotFinal :: IORef (Maybe Shot)
  , obsTarget :: IORef (Maybe Target)
  , obsTargetAfter :: IORef (Maybe Bool)
  , obsNewFiles :: IORef [NewFile]
  , obsStop :: IORef (Maybe String)
  , obsPrePressStable :: IORef (Maybe Bool)
  , obsDecode :: IORef (Maybe (Bool, Value))
  , obsLedger :: IORef TraceLedger
  , obsRaw :: IORef (Maybe (Maybe Bool, Value))
  , obsBeforeHits :: IORef [Hit]
  -- | The bridge hit index the next trace-hits request starts from, once the before-press hits are folded.
  , obsHitIndex :: IORef (Maybe Int)
  -- | Bridge hits read from emulator-session.json after the bridge left its command loop.
  , obsRecovered :: IORef (Maybe Int)
  , obsCapture :: IORef (Maybe CaptureProgress)
  }

data NewFile = NewFile { newJpeg :: Bool, newValue :: Value }

newObserved :: IO Observed
newObserved = Observed <$> newIORef Nothing <*> newIORef [] <*> newIORef 0 <*> newIORef [] <*> newIORef Nothing
  <*> newIORef Nothing <*> newIORef Nothing <*> newIORef Nothing <*> newIORef Nothing <*> newIORef Nothing
  <*> newIORef Nothing <*> newIORef Nothing <*> newIORef Nothing <*> newIORef [] <*> newIORef Nothing <*> newIORef Nothing
  <*> newIORef Nothing <*> newIORef emptyLedger <*> newIORef Nothing <*> newIORef [] <*> newIORef Nothing <*> newIORef Nothing
  <*> newIORef Nothing

-- | Folds hits into the ledger and returns their journal record; after the press, kept hits also feed the verdict.
foldHits :: Observed -> TracePhase -> [Hit] -> IO Value
foldHits observed phase hits = do
  ledger <- readIORef (obsLedger observed)
  let (ledger', kept) = absorb ledger hits
      counts = Map.fromListWith (+) [(hitLabel hit, 1 :: Int) | hit <- hits]
  evaluatedLedger <- evaluate (force ledger')
  case phase of
    BeforePress -> do
      writeIORef (obsLedger observed) evaluatedLedger { ledgerBeforePress = ledgerCounts evaluatedLedger }
      appendKeptHits (obsBeforeHits observed) kept
    AfterPress -> do
      writeIORef (obsLedger observed) evaluatedLedger
      appendKeptHits (obsHits observed) kept
  pure (object [ "trace_hits_kept" .= map hitRaw kept, "trace_hit_counts" .= counts
               , "trace_hits_over_cap" .= (length hits - length kept) ])

ledgerValue :: TraceLedger -> Value
ledgerValue ledger = object
  [ "counts" .= ledgerCounts ledger, "counts_before_press" .= ledgerBeforePress ledger
  , "traced_configuration" .= tracedConfigurationValue (ledgerConfiguration ledger)
  , "first_line_36" .= fmap line36Value (ledgerFirstLine36 ledger)
  , "last_line_36" .= fmap line36Value (ledgerLastLine36 ledger) ]
  where line36Value l = object ["instructions" .= line36Instructions l, "traced_configuration" .= tracedConfigurationValue (line36Configuration l)]

-- | render.rs physical(): the uncached 0x4000_0000 alias of the first 512 MiB.
physicalAddress :: Word32 -> Word32
physicalAddress address
  | address >= 0x40000000 && address < 0x60000000 = address - 0x40000000
  | otherwise = address

readGuest :: Bridge -> Word32 -> Int -> IO B.ByteString
readGuest bridge address count = do
  reply <- call bridge (command "read-memory" ["address" .= physicalAddress address, "count" .= count])
  bytes <- either (harness . ("read-memory: " ++)) pure (maybe (Left "missing bytes") decodeHex (textAt "bytes" reply))
  require (B.length bytes == count) "read-memory returned a different length"
  pure bytes

-- | Relates the last programmed 0xfff63000 job to the buffer the file client filled. FujiMod's
-- jpeg-decoder reads its input from +0x10c for +0x110 bytes and writes packed 4:2:2 to +0x100;
-- 0x0065a58c loads those from param words 0, 1 and 2 at 0x014c5890.
decodeBinding :: Bridge -> Target -> [Hit] -> Maybe CellValues -> IO (Maybe (Bool, Value))
decodeBinding bridge target hits values = case [(buf, len) | hit <- hitsAt "file-read" hits, Just len <- [hitR 2 hit], Just buf <- [hitR 3 hit]] of
  [] -> pure Nothing
  fileReads@((start, firstLength) : _) -> do
    let contiguous = and (zipWith (\(b, l) (b', _) -> b + l == b') fileReads (drop 1 fileReads))
        end = if contiguous then start + sum (map snd fileReads) else start + firstLength
        prefix = min 4096 (fromIntegral firstLength)
        word offset = values >>= Map.lookup "decode_params" >>= \bytes ->
          if B.length bytes >= offset + 4 then Just (fromIntegral (le bytes offset 4) :: Word32) else Nothing
        source = word 8
        count = word 12
        inBuffer = case (source, count) of
          (Just s, Just n) -> s >= start && s < end && s + n <= end
          _ -> False
    head' <- readGuest bridge start prefix
    let prefixMatches = head' == B.take prefix (targetBytes target)
    pure (Just (prefixMatches && inBuffer, object
      [ "file_reads" .= [object ["buffer" .= hex32 b, "length" .= l] | (b, l) <- fileReads]
      , "file_reads_contiguous" .= contiguous, "file_buffer_end" .= hex32 end
      , "file_buffer_prefix_bytes" .= prefix, "file_buffer_prefix_matches_fixture" .= prefixMatches
      , "decode_job" .= fmap hex32 (word 0), "decode_source" .= fmap hex32 source, "decode_source_count" .= count
      , "decode_source_offset_in_file" .= fmap (\s -> toInteger s - toInteger start) source
      , "decode_destination" .= fmap hex32 (word 16), "decode_capacity" .= word 0x1c, "decode_shift" .= fmap (.&. 3) (word 0x24)
      , "decode_source_in_file_buffer" .= inBuffer ]))

data RawGeometry = RawGeometry
  { rgDestination :: Word32, rgLeft :: Int, rgTop :: Int, rgWidth :: Int, rgHeight :: Int
  , rgPacked12 :: Bool, rgFormat :: Word32, rgRowBytes :: Int, rgPitch :: Int }

-- | FujiMod channel-2 window, packing, format, destination and stride.
rawGeometry :: TracedConfiguration -> Either String RawGeometry
rawGeometry file = do
  let get offset = maybe (Left ("register " ++ hex32 offset ++ " is unavailable")) (Right . fst) (Map.lookup offset file)
  r004 <- get 0x004
  r014 <- get 0x014
  r018 <- get 0x018
  r024 <- get 0x024
  r354 <- get 0x354
  r358 <- get 0x358
  r360 <- get 0x360
  let packed = r004 .&. 0x10000 /= 0
      format = r354 `shiftR` 20 .&. 7
      width = fromIntegral (r024 .&. 0x3fff)
      rowBytes w = if packed then w * 3 `div` 2 else w * 2
      alignment = (if packed then 4 else 1) `shiftL` fromIntegral (format + 3)
      padded = (width + alignment - 1) `div` alignment * alignment
  unless (r004 .&. 2 == 0) (Left "0x004 bit 1 selects a transport outside the modeled one")
  unless (format `elem` [3, 4]) (Left ("channel-2 format " ++ show format ++ " is outside the modeled 3 and 4"))
  unless (width > 0 && even width) (Left "channel-2 width is zero or odd")
  Right (RawGeometry r358 (fromIntegral (r014 `shiftR` 16 .&. 0x3fff)) (fromIntegral (r018 `shiftR` 16 .&. 0x3fff))
    width (fromIntegral (r018 .&. 0x3fff) + 1) packed format (rowBytes width) (rowBytes padded + fromIntegral r360))

geometryValue :: RawGeometry -> Value
geometryValue g = object
  [ "destination" .= hex32 (rgDestination g), "left" .= rgLeft g, "top" .= rgTop g, "width" .= rgWidth g
  , "height" .= rgHeight g, "packing" .= (if rgPacked12 g then "packed12" else "u16" :: T.Text), "format" .= rgFormat g
  , "row_bytes" .= rgRowBytes g, "pitch" .= rgPitch g ]

-- | Sample pairs of one row segment: U16 is two little-endian halfwords, Packed12 is
-- {a, a>>8 | b<<4, b>>4} (FujiMod emulator-parts-sensor sensor.rs write_rows).
decodePairs :: Bool -> B.ByteString -> [Word32]
decodePairs packed bytes
  | packed = concat [ [b 0 .|. (b 1 .&. 0xf) `shiftL` 8, b 1 `shiftR` 4 .|. b 2 `shiftL` 4] | i <- [0, 3 .. B.length bytes - 3], let b k = fromIntegral (B.index bytes (i + k)) ]
  | otherwise = [ fromIntegral (le bytes i 2) | i <- [0, 2 .. B.length bytes - 2] ]

newtype GuestInstructions = GuestInstructions Integer
newtype DeviceRegisterBank = DeviceRegisterBank [Word32]

data ImageInputObservation = ImageInputObservation
  { inputInstructions :: GuestInstructions, inputRegisters :: DeviceRegisterBank
  , inputCaptureAck :: Bool, inputCaptureIrqSent :: Bool }

instance FromJSON ImageInputObservation where
  parseJSON = A.withObject "image-input-fingerprint reply" $ \reply -> do
    instructions <- GuestInstructions <$> reply A..: "instructions"
    fingerprint <- reply A..: "fingerprint"
    state <- fingerprint A..: "state"
    registers <- state A..: "r"
    unless (length registers == 256) (fail "image-input register bank does not contain 256 words")
    ImageInputObservation instructions (DeviceRegisterBank registers)
      <$> state A..: "capture_ack" <*> state A..: "capture_irq_sent"

imageInputState :: Bridge -> IO (ImageInputObservation, Fields)
imageInputState bridge = do
  reply <- call bridge (command "image-input-fingerprint" ["node" .= ("image_input" :: T.Text)])
  case fromJSON (Object reply) of
    A.Error reason -> harness ("image-input-fingerprint: " ++ reason)
    A.Success observation -> pure (observation, reply)

newtype RecorderContextIndex = RecorderContextIndex Word32
newtype RecorderContextBytes = RecorderContextBytes B.ByteString
data RecorderServiceState = NoRecorderContext | RecorderServiceState RecorderContextIndex RecorderContextBytes

recorderServiceState :: Bridge -> IO RecorderServiceState
recorderServiceState bridge = do
  selection <- readGuest bridge 0x015ad53c 4
  let index = fromIntegral (le selection 0 4) :: Word32
  if index > 200 then pure NoRecorderContext else do
    bytes <- readGuest bridge (0x015968f4 + index * 0x1d0) 0x1d0
    pure (RecorderServiceState (RecorderContextIndex index) (RecorderContextBytes bytes))

recorderServiceValue :: RecorderServiceState -> Value
recorderServiceValue NoRecorderContext = object ["context" .= Null, "unknown" .= ("no valid context selected" :: T.Text)]
recorderServiceValue (RecorderServiceState (RecorderContextIndex index) (RecorderContextBytes bytes)) = object
  [ "context" .= index, "bytes" .= hexBytes bytes
  , "goal_bits" .= bits 0, "completed_bits" .= bits 36, "deferred_bits" .= bits 64 ]
  where bits offset = [n | n <- [0 :: Int .. 216], B.index bytes (offset + n `div` 8) .&. (1 `shiftL` (n `mod` 8)) /= 0]

-- | Reads RAM independently of interrupt delivery, using the model's current destination and geometry.
rawSamples :: Bridge -> Maybe RampRasterWidth -> ImageInputObservation -> IO (Maybe (Maybe Bool, Value))
rawSamples bridge ramp observation = case rawGeometry configuration of
    Left reason -> pure (Just (Nothing, object ["geometry_unknown" .= reason
      , "device_configuration" .= tracedConfigurationValue configuration]))
    Right g
      | toInteger (physicalAddress (rgDestination g)) + toInteger (rgPitch g) * toInteger (rgHeight g) > 0x20000000 ->
          pure (Just (Nothing, object ["device_geometry" .= geometryValue g, "not_read" .= ("destination extent lies outside the first 512 MiB" :: T.Text)]))
      | otherwise -> do
          let unit = if rgPacked12 g then 3 else 4
              chunk = min (rgRowBytes g) (16 * unit) `div` unit * unit
              rows = nub [0, rgHeight g `div` 2, rgHeight g - 1]
              segments = nub [0, (rgRowBytes g - chunk) `div` unit * unit]
          readings <- forM rows $ \row -> forM segments $ \offset -> do
            let address = rgDestination g + fromIntegral (row * rgPitch g + offset)
            bytes <- readGuest bridge address chunk
            let firstColumn = offset `div` unit * 2
                actual = decodePairs (rgPacked12 g) bytes
                expected = fmap (\(RampRasterWidth w) -> [ fromIntegral (((rgTop g + row) * w + rgLeft g + firstColumn + k) `mod` 4096) | k <- [0 .. length actual - 1] ]) ramp
            pure (fmap (== actual) expected, object
              [ "row" .= row, "first_column" .= firstColumn, "address" .= hex32 address, "bytes" .= hexBytes bytes
              , "samples" .= actual, "expected" .= expected
              , "mismatches" .= fmap (\e -> length (filter id (zipWith (/=) e actual))) expected ])
          let flat = concat readings
          pure (Just (fmap and (mapM fst flat), object
            [ "device_snapshot_instructions" .= instructions, "device_geometry" .= geometryValue g
            , "device_configuration" .= tracedConfigurationValue configuration
            , "capture_ack" .= inputCaptureAck observation, "capture_irq_sent" .= inputCaptureIrqSent observation
            , "geometry_basis" .= ("image-input model fingerprint, not traced operands" :: T.Text)
            , "ramp_raster_width" .= fmap (\(RampRasterWidth w) -> w) ramp
            , "ramp_assumes" .= ("frame raster equals sensor raster, so readout sample (x, y) is frame pixel (x, y)" :: T.Text)
            , "segments" .= map snd flat ]))
  where
    GuestInstructions instructions = inputInstructions observation
    DeviceRegisterBank registers = inputRegisters observation
    configuration = Map.fromList [(fromIntegral (index * 4), (value, instructions)) | (index, value) <- zip [0 :: Int ..] registers]

newFiles :: FilePath -> CardView -> CardView -> IO [NewFile]
newFiles directory before after = fmap concat . forM (viewFolders after) $ \folder -> do
  let old = maybe [] folderEntries (find ((== folderName folder) . folderName) (viewFolders before))
      added = [e | e <- folderEntries folder, not (isDirectory e), entryName e `notElem` map entryName old]
  forM added $ \e -> do
    if entrySize e > 64 * 1024 * 1024
      then pure (NewFile False (object ["folder" .= C.unpack (folderName folder), "entry" .= entryValue e, "read" .= False]))
      else do
        (clusters, bytes) <- readFileBytes (viewCard after) e
        let name = C.unpack (folderName folder) ++ "_" ++ C.unpack (entryName e)
            soi = B.take 2 bytes == "\xff\xd8"
            eoi = B.length bytes >= 2 && B.drop (B.length bytes - 2) bytes == "\xff\xd9"
            fujifilm = "FUJIFILM" `B.isInfixOf` B.take 65536 bytes
        createDirectoryIfMissing True (directory </> "card-files")
        B.writeFile (directory </> "card-files" </> map (\c -> if c == ' ' then '_' else c) name) bytes
        pure (NewFile (soi && eoi && B.length bytes > 2)
          (object [ "folder" .= C.unpack (folderName folder), "entry" .= entryValue e
                  , "extents" .= map extentValue (extentsOf (viewVolume after) clusters)
                  , "sha256" .= digestText (digest bytes), "soi" .= soi, "eoi" .= eoi, "fujifilm_in_first_64k" .= fujifilm ]))

runRoute :: Options -> FilePath -> Bridge -> Journal -> Observed -> IO ()
runRoute options directory bridge journal observed = do
  let route = optRoute options
      sites = routeSites route
  _ <- call bridge (command "trace-sites" ["sites" .= object [K.fromString (hex32 (siteAddress s)) .= siteLabel s | s <- sites]])
  note journal "trace-sites" (toJSON [object ["address" .= hex32 (siteAddress s), "label" .= siteLabel s, "source" .= siteSource s] | s <- sites])
  before <- walkCard bridge
  note journal "card-before" (viewValue before)
  (target, fixture) <- verifyFixture before
  writeIORef (obsTarget observed) (Just target)
  note journal "fixture-through-media-read" fixture
  when (optShootWithoutLens options) $ shootWithoutLens bridge >>= note journal "shoot-without-lens-keys" . toJSON
  dial <- call bridge (command "mode-dial" ["position" .= windowDial])
  note journal "mode-dial" (Object dial)
  prePressA <- capture bridge "pre-press-a"
  note journal "capture-pre-press-a" (shotValue prePressA)
  settleDial <- call bridge (command "mode-dial" ["position" .= windowDial])
  note journal "mode-dial-before-press" (Object settleDial)
  (startHits, preHits) <- traceHits bridge 0
  foldHits observed BeforePress preHits >>= note journal "trace-before-press"
  writeIORef (obsHitIndex observed) (Just startHits)
  (sdFields0, sd0) <- mediaState bridge
  writeIORef (obsSdBefore observed) (Just sd0)
  note journal "media-state-before" (Object sdFields0)
  cells0 <- readCells bridge
  writeIORef (obsCellsBefore observed) (Just cells0)
  note journal "cells-before" (cellsValue cells0)
  shot0 <- capture bridge "pre-press"
  writeIORef (obsShotBefore observed) (Just shot0)
  writeIORef (obsPrePressStable observed) (Just (shotSha prePressA == shotSha shot0))
  note journal "capture-pre-press" (shotValue shot0)
  (input0, fingerprint0) <- imageInputState bridge
  note journal "image-input-before-press" (Object fingerprint0)
  recorderServiceState bridge >>= note journal "recorder-service-before-press" . recorderServiceValue
  rawSamples bridge (optRamp options) input0 >>= note journal "raw-samples-before-press" . maybe Null snd
  when (optCaptureContinuationObservation options) $ do
    let armed = sites ++ captureContinuationSites
    -- The SDK also arms nine fixed instruction sites.
    require (length armed + 9 <= 256 && all ((<= 64) . B.length . TE.encodeUtf8 . siteLabel) armed)
      "capture continuation sites exceed the SDK site or UTF-8 label bound"
    _ <- call bridge (command "trace-sites" ["sites" .= object [K.fromString (hex32 (siteAddress s)) .= siteLabel s | s <- armed]])
    note journal "capture-continuation-sites" (toJSON
      [object ["address" .= hex32 (siteAddress s), "label" .= siteLabel s, "source" .= siteSource s] | s <- captureContinuationSites])
    (_, remaining) <- traceHits bridge startHits
    foldHits observed BeforePress remaining >>= note journal "trace-before-observation"
    begun <- call bridge CS.beginRequest
    either harness pure (CS.validateBegin (Object begun))
    note journal "capture-observation-begin" (Object begun)
    writeIORef (obsHitIndex observed) Nothing
    state <- call bridge (command "capture-observation-state" [])
    note journal "capture-observation-pre-sequence" (Object state)
    at <- maybe (harness "capture state lacks instruction boundary") pure (objectAt "snapshot" state >>= num "instructions")
    writeIORef (obsCapture observed) (Just (CaptureProgress (CS.startCoverage at) Nothing CS.initialCursor False))
    _ <- captureObservationHits bridge journal observed
    pure ()
  pressed <- if optCaptureContinuationObservation options
    then captureAdvance bridge journal observed CS.ShutterSequence (routeInput route)
    else call bridge (routeInput route)
  writeIORef (obsPressReply observed) (Just pressed)
  note journal "press" (object ["input" .= routeInput route, "reply" .= Object pressed])
  when (optCaptureContinuationObservation options) $
    note journal "capture-observation-phases" (fromMaybe Null (KM.lookup "phases" pressed))
  cells1 <- readCells bridge
  writeIORef (obsCellsAfterPress observed) (Just cells1)
  note journal "cells-after-press" (cellsValue cells1)
  (input1, fingerprint1) <- imageInputState bridge
  note journal "image-input-after-press" (Object fingerprint1)
  recorderServiceState bridge >>= note journal "recorder-service-after-press" . recorderServiceValue
  rawSamples bridge (optRamp options) input1 >>= note journal "raw-samples-after-press" . maybe Null snd
  let window index quiet previous hitIndex
        | index > optWindows options || quiet >= 2 = pure previous
        | otherwise = do
            (sdFields, sd) <- if index == 0 then mediaState bridge else do
              reply <- if optCaptureContinuationObservation options
                then captureAdvance bridge journal observed CS.Window (command "capture-observation-window" [])
                else call bridge (command "capture-observation-window" [])
              note journal "capture-observation-window" (object ["window" .= index, "reply" .= Object reply])
              mediaState bridge
            values <- readCells bridge
            (count, traced, otherLabels, traceActive) <-
              if optCaptureContinuationObservation options then do
                activity <- captureObservationHits bridge journal observed
                progress <- readIORef (obsCapture observed)
                pure (hitIndex, maybe Null captureProgressValue progress, Null, activity)
              else do
                (next, hits) <- traceHits bridge hitIndex
                record <- foldHits observed AfterPress (filter (\hit -> hitLabel hit `elem` map siteLabel sites) hits)
                let other = Map.fromListWith (+) [(hitLabel hit, 1 :: Int) | hit <- hits, hitLabel hit `notElem` map siteLabel sites]
                pure (next, record, toJSON other, any (routeActivity route) hits)
            let (seen, lost) = completedSince previous sd
                active = sdCompleted sd /= sdCompleted previous || sdAccepted sd /= sdAccepted previous || traceActive
            modifyIORef' (obsCommands observed) (++ [(index, c) | c <- seen])
            modifyIORef' (obsLost observed) (+ lost)
            unless (optCaptureContinuationObservation options) $
              writeIORef (obsHitIndex observed) (Just count)
            writeIORef (obsCellsFinal observed) (Just values)
            (device, fingerprint) <- imageInputState bridge
            note journal "image-input-window" (object ["window" .= index, "reply" .= Object fingerprint])
            service <- recorderServiceState bridge
            note journal "recorder-service-window" (object ["window" .= index, "state" .= recorderServiceValue service])
            samples <- rawSamples bridge (optRamp options) device
            writeIORef (obsRaw observed) samples
            note journal "raw-samples-window" (object ["window" .= index, "samples" .= maybe Null snd samples])
            note journal "window" (object
              [ "window" .= index, "media_state" .= Object sdFields, "cells" .= cellsValue values
              , "trace" .= traced, "other_site_hit_counts" .= otherLabels
              , "sd_commands" .= map (sdCommandValue index) seen, "sd_commands_outside_history" .= lost ])
            window (index + 1) (if active || index == 0 then 0 else quiet + 1 :: Int) sd count
  final <- window (0 :: Int) (0 :: Int) sd0 startHits
  writeIORef (obsSdFinal observed) (Just final)
  ledger <- readIORef (obsLedger observed)
  note journal "trace-ledger" (ledgerValue ledger)
  (device, fingerprint) <- imageInputState bridge
  note journal "image-input-final" (Object fingerprint)
  raw <- rawSamples bridge (optRamp options) device
  writeIORef (obsRaw observed) raw
  note journal "raw-samples" (maybe Null snd raw)
  routeHits <- readIORef (obsHits observed)
  finalCells <- readIORef (obsCellsFinal observed)
  decode <- decodeBinding bridge target routeHits finalCells
  writeIORef (obsDecode observed) decode
  note journal "decode-binding" (maybe Null snd decode)
  after <- walkCard bridge
  note journal "card-after" (viewValue after)
  intact <- try (verifyFixture after)
  writeIORef (obsTargetAfter observed) (Just (either (\e -> const False (e :: IOException)) (const True) intact))
  added <- newFiles directory before after
  writeIORef (obsNewFiles observed) added
  note journal "card-new-files" (toJSON (map newValue added))
  shot <- capture bridge "final"
  writeIORef (obsShotFinal observed) (Just shot)
  note journal "capture-final" (shotValue shot)

-- Verdict -------------------------------------------------------------------------------------

-- | A gate must hold for a meaningful route; context is recorded without deciding it.
data Role = Gate | Context deriving (Eq)

data Finding = Finding { findingName :: T.Text, findingRole :: Role, findingHolds :: Bool, findingDetail :: Value, findingSource :: T.Text }

-- | A finding the run did not observe enough to decide carries the reason.
data Judged = Judged Finding (Maybe T.Text)

judgedValue :: Judged -> Value
judgedValue (Judged f unknown) = object
  [ "name" .= findingName f, "gate" .= (findingRole f == Gate)
  , "holds" .= (if isJust unknown then Nothing else Just (findingHolds f)), "unknown" .= unknown
  , "detail" .= findingDetail f, "source" .= findingSource f ]

-- | Ordered findings; the first gate that does not hold, or is unknown, names where the route stopped.
verdict :: Route -> Bool -> Observed -> IO ([Judged], Maybe T.Text)
verdict route lensOn observed = do
  pressReply <- readIORef (obsPressReply observed)
  stop <- readIORef (obsStop observed)
  commands <- readIORef (obsCommands observed)
  lost <- readIORef (obsLost observed)
  hits <- readIORef (obsHits observed)
  cellsAfterPress <- readIORef (obsCellsAfterPress observed)
  cellsFinal <- readIORef (obsCellsFinal observed)
  cellsBefore <- readIORef (obsCellsBefore observed)
  sdBefore <- readIORef (obsSdBefore observed)
  sdFinal <- readIORef (obsSdFinal observed)
  shotBefore <- readIORef (obsShotBefore observed)
  shotFinal <- readIORef (obsShotFinal observed)
  target <- readIORef (obsTarget observed)
  targetAfter <- readIORef (obsTargetAfter observed)
  added <- readIORef (obsNewFiles observed)
  prePressStable <- readIORef (obsPrePressStable observed)
  decode <- readIORef (obsDecode observed)
  ledger <- readIORef (obsLedger observed)
  raw <- readIORef (obsRaw observed)
  beforeHits <- readIORef (obsBeforeHits observed)
  recovered <- readIORef (obsRecovered observed)
  captureProgress <- readIORef (obsCapture observed)
  let settle = pressReply >>= objectAt "settle" >>= textAt "status"
      r0s label = [r | hit <- hitsAt label hits, Just r <- [hitR0 hit]]
      count = sincePress ledger
      registersOf label = [map (fmap hex32) (hitRegisters hit) | hit <- hitsAt label hits]
      cell values name = values >>= \v -> cellWord v name
      warning = cell cellsFinal "cached_warning"
      mediaStatus = cell cellsFinal "media_status"
      readCommands = [c | (_, c) <- commands, isRead c]
      writeCommands = [c | (_, c) <- commands, isWrite c]
      common =
        [ Finding "press-admitted" Gate (if route == ShutterSequenceRoute then isJust pressReply else isJust settle && settle /= Just "prior-render-pending")
            (if route == ShutterSequenceRoute then maybe Null Object pressReply else toJSON settle)
            "ram_push.rs shutter-sequence or render.rs settled key"
        , Finding "no-strict-model-stop" Gate (not (any ("the machine stopped" `isInfixOf`) stop)) (toJSON stop) "emulation/fujimod-xm1/src/machine.rs:224-229"
        , Finding "bridge-answered-every-command" Context (stop == Nothing) (toJSON stop) "emulation/fujimod-xm1/src/ram_push.rs:646-650"
        , Finding "no-card-warning" Gate (mediaStatus == Just 1 && maybe True (`notElem` [0x11, 0x12, 0x13]) warning)
            (object ["media_status" .= mediaStatus, "cached_warning" .= warning]) "reports/cardless-capture-feasibility.md:37-38"
        , Finding "pre-press-shooting-ui-mode" Gate (cell cellsBefore "ui_mode_current" == Just 3)
            (object ["ui_mode_current" .= cell cellsBefore "ui_mode_current", "shooting_mode" .= cell cellsBefore "shooting_mode"])
            "reports/service-shoot-feasibility.md:127; reports/autofocus-static-map.md:35"
        , Finding "pre-press-frames-identical" Context (prePressStable == Just True) (toJSON prePressStable) "screenshots pre-press-a.png and pre-press.png" ]
      play =
        [ Finding "playback-ui-mode" Gate (cell cellsAfterPress "ui_mode_current" == Just 4 && cell cellsAfterPress "ui_mode_previous" == Just 3)
            (object [ "ui_mode_current_after_press" .= cell cellsAfterPress "ui_mode_current"
                    , "ui_mode_previous_after_press" .= cell cellsAfterPress "ui_mode_previous"
                    , "ui_mode_request_r0" .= r0s "ui-mode-request", "mode_3_to_4_setup_hits" .= count "ui-mode-3-to-4-setup"
                    , "play_redecision_hits" .= count "play-mode-redecision"
                    , "ui_mode_current_final" .= cell cellsFinal "ui_mode_current" ])
            "private-data/analysis/x-m1-1.50/af/functions/00d350c4.c (selector 2 sets pending 4); 00d3c3b4.c"
        , Finding "target-sectors-read" Gate (maybe False (\t -> any (touches (targetFile t)) readCommands) target)
            (object [ "reads" .= length readCommands
                    , "folder_reads" .= maybe 0 (\t -> length (filter (touches (targetFolder t)) readCommands)) target
                    , "target_reads" .= maybe 0 (\t -> length (filter (touches (targetFile t)) readCommands)) target
                    , "commands_outside_history" .= lost
                    , "file_open_hits" .= count "file-open", "file_read_hits" .= count "file-read"
                    , "exif_parser_hits" .= count "exif-parser" ])
            "emulation/fujimod-xm1/src/sd_host.rs:540-552,786-792"
        , Finding "final-frame-differs-from-pre-press-frame" Gate (isJust shotFinal && isJust shotBefore && fmap shotSha shotFinal /= fmap shotSha shotBefore)
            (object ["pre_press" .= fmap shotSha shotBefore, "final" .= fmap shotSha shotFinal])
            "emulation/fujimod-xm1/src/ram_push.rs:548-561"
        , Finding "decode-source-in-loaded-file" Context (maybe False fst decode) (maybe Null snd decode)
            "FujiMod crates/emulator-exr2-codecs/src/jpeg/decoder.rs:80-96; af/functions/00d99714.c, 0065a58c.c"
        , Finding "fixture-unchanged-after" Gate (targetAfter == Just True) (toJSON targetAfter) "emulation/fujimod-xm1/src/sd_host.rs:364-374" ]
      results = r0s "admission-result"
      shutter =
        [ Finding "shoot-without-lens-on" Context lensOn
            (object [ "lens_word_0_before" .= cell cellsBefore "lens_word_0", "lens_word_1_before" .= cell cellsBefore "lens_word_1"
                    , "lens_word_0_final" .= cell cellsFinal "lens_word_0", "lens_word_1_final" .= cell cellsFinal "lens_word_1" ])
            "reports/autofocus-static-map.md:45-47"
        , Finding "s2-consumer-released" Context (cell cellsAfterPress "ui_consumer_state" == Just 3 && cell cellsBefore "ui_consumer_state" /= Just 3)
            (object ["before" .= cell cellsBefore "ui_consumer_state", "after_press" .= cell cellsAfterPress "ui_consumer_state"])
            "reports/full-system-emulation.md:492"
        , Finding "admitted" Gate (not (null results) && all (<= 1) results)
            (object [ "admission_results" .= results, "s2_warmup_hits" .= count "s2-state-4-warmup"
                    , "s2_capture_hits" .= count "s2-state-6-capture", "refusal_hits" .= count "admission-refusal" ])
            "reports/af-observation-plan.md:74-76; reports/autofocus-static-map.md:45"
        , Finding "save-chain-entered" Context (any ((> 0) . count) ["save-frame", "storage-machine-a", "storage-machine-b"])
            (object [ K.fromText (siteLabel s) .= count (siteLabel s) | s <- shutterSites ++ commonSites ])
            "reports/cardless-capture-feasibility.md:467-475"
        , Finding "card-written" Gate (maybe False (\b -> maybe False (\f -> sdAccepted f > sdAccepted b) sdFinal) sdBefore)
            (object [ "accepted_writes_before" .= fmap sdAccepted sdBefore, "accepted_writes_final" .= fmap sdAccepted sdFinal
                    , "dirty_before" .= fmap sdDirty sdBefore, "dirty_final" .= fmap sdDirty sdFinal
                    , "write_commands_seen" .= length writeCommands, "commands_outside_history" .= lost
                    , "write_protected_final" .= (sdFinal >>= sdProtected), "present_final" .= (sdFinal >>= sdPresent) ])
            "emulation/fujimod-xm1/src/sd_host.rs:742-746,802-808,1087-1096"
        , Finding "new-jpeg-on-card" Gate (any newJpeg added) (toJSON (map newValue added)) "DCIM walk through media-read"
        , Finding "fixture-unchanged-after" Gate (targetAfter == Just True) (toJSON targetAfter) "emulation/fujimod-xm1/src/sd_host.rs:364-374"
        , Finding "raw-open-requested" Context (any (== 0x00ed84cc) [r | hit <- hitsAt "image-input-channel-open" hits, Just r <- [hitR 1 hit]])
            (object [ "channel_open_registers" .= registersOf "image-input-channel-open"
                    , "raw_stream_start_registers" .= registersOf "raw-stream-start"
                    , "raw_stream_stop_hits" .= count "raw-stream-stop", "raw_stream_clear_hits" .= count "raw-stream-clear"
                    , "raw_stream_cells_final" .= fmap hexBytes (cellsFinal >>= Map.lookup "raw_stream")
                    , "raw_stream_mode_final" .= fmap (\b -> le b 4 4) (cellsFinal >>= Map.lookup "raw_stream")
                    , "raw_stream_client_final" .= fmap (\b -> le b 8 4) (cellsFinal >>= Map.lookup "raw_stream") ])
            "af/functions/00edb630.c, 00d9f0b0.c, 00663040.c"
        , Finding "raw-open-returned-success" Context (0 `elem` r0s "raw-open-result")
            (object ["caller_arguments" .= registersOf "raw-open-call", "return_values" .= r0s "raw-open-result"
                    , "registrar_arguments" .= registersOf "image-input-register-irq-call"
                    , "registrar_results" .= r0s "image-input-register-irq-result"
                    , "inhibition_values" .= map (hitR 3) (hitsAt "image-input-channel-2-inhibition" hits)
                    , "enable_calls" .= registersOf "image-input-channel-2-enable-call"])
            "id5 d9f348/d9f34c; id4 663174/663178/663198/6631bc"
        , Finding "raw-irq-handler-entered" Context (count "image-input-line-36" > 0)
            (object [ "line_36_hits" .= count "image-input-line-36", "line_37_hits" .= count "image-input-line-37"
                    , "line_38_hits" .= count "image-input-line-38", "raw_callback_hits" .= count "raw-stream-callback"
                    , "raw_client_posts" .= r0s "raw-client-post", "raw_client_post_hits" .= count "raw-client-post"
                    , "first_line_36_instructions" .= fmap line36Instructions (ledgerFirstLine36 ledger)
                    , "last_line_36_instructions" .= fmap line36Instructions (ledgerLastLine36 ledger)
                    , "image_input_stores_since_press" .= sum [count (siteLabel s) | s <- imageInputStoreSites] ])
            "af/functions/00664c28.c, 00ed84cc.c; FujiMod emulator-exr2-isp-input/src/input.rs:625-674,809-817"
        , Finding "raw-dma-matches-ramp12" Context (maybe False ((== Just True) . fst) raw) (maybe Null snd raw)
            "FujiMod emulator-exr2-isp-input/src/input.rs:675-727; emulator-parts-sensor/src/sensor.rs:665-733"
        , Finding "ui-816-posted-after-press" Context (0x816 `elem` r0s "ui-notification-post")
            (object [ "ui_816_posts_before_press_kept" .= [hitRaw hit | hit <- hitsAt "ui-notification-post" beforeHits, hitR0 hit == Just 0x816]
                    , "ui_816_posts_after_press" .= [hitRaw hit | hit <- hitsAt "ui-notification-post" hits, hitR0 hit == Just 0x816]
                    , "ui_notification_ids" .= map hex32 (r0s "ui-notification-post"), "ui_notification_hits" .= count "ui-notification-post"
                    , "machine_15_command_19_state_5_hits" .= count "reccont-15-command-19-state-5"
                    , "df2378_call_registers" .= registersOf "reccont-15-df2378-call"
                    , "df2378_results" .= map hex32 (r0s "reccont-15-df2378-result")
                    , "machine_4d_completion_inputs" .= [ (fmap hex32 (hitR 0 hit), fmap hex32 (hitR 1 hit)) | hit <- hitsAt "reccont-4d-completion-inputs" hits ]
                    , "reccont_4e_messages" .= map (fmap hex32 . hitR 3) (hitsAt "reccont-4e-message" hits)
                    , "reccont_15_commands" .= r0s "reccont-15-command", "reccont_submit_ops" .= r0s "reccont-submit"
                    , "reccont_4e_state_9_query" .= r0s "reccont-4e-state-9-query"
                    , "modal_4a_state_8_returns" .= r0s "modal-4a-97-state-8-return"
                    , "modal_4a_state_final" .= cell cellsFinal "modal_4a_state"
                    , "reccont_class_final" .= cell cellsFinal "reccont_class" ])
            "af/functions/00e1dc9c.c, 00ddb5cc.c, 00dd68b8.c, 00c6ce48.c, 00e36208.c" ]
      findings = common ++ (if route == PlayRoute then play else shutter)
      routeEnded = isJust targetAfter
      traceComplete = isNothing stop || isJust recovered
      reasons =
        [ ("raw-dma-matches-ramp12", "no ramp comparison was made") | maybe True (isNothing . fst) raw ]
        ++ [ (name, "the route ended before its final card walk and capture") | not routeEnded
           , name <- ["final-frame-differs-from-pre-press-frame", "decode-source-in-loaded-file", "card-written", "new-jpeg-on-card", "fixture-unchanged-after"] ]
        ++ [ (name, "trace hits after the last collected window were not recovered") | not traceComplete
           , name <- ["target-sectors-read", "admitted", "save-chain-entered", "raw-open-requested", "raw-open-returned-success", "raw-irq-handler-entered", "ui-816-posted-after-press"] ]
        ++ [ (name, "bounded capture observation has missing event detail or an uncollected interval")
           | Just progress <- [captureProgress]
           , captureIncomplete progress
           , name <- ["admitted", "save-chain-entered", "raw-open-requested", "raw-open-returned-success", "raw-irq-handler-entered", "ui-816-posted-after-press"] ]
      judged = [Judged f (lookup (findingName f) reasons) | f <- findings]
      undecided (Judged f unknown) = findingRole f == Gate && (isJust unknown || not (findingHolds f))
  pure (judged, (\(Judged f _) -> findingName f) <$> find undecided judged)

-- Main ----------------------------------------------------------------------------------------

data CardStat = CardStat Integer Rational Integer deriving (Eq)

cardStat :: FilePath -> IO CardStat
cardStat path = do
  status <- getSymbolicLinkStatus path
  require (isRegularFile status) (path ++ " is not a regular file")
  pure (CardStat (fromIntegral (fileSize status)) (toRational (modificationTimeHiRes status)) (fromIntegral (fileID status)))

statValue :: CardStat -> Value
statValue (CardStat size modified inode) = object ["size" .= size, "mtime" .= (fromRational modified :: Double), "inode" .= inode]

freshOutput :: FilePath -> IO FilePath
freshOutput supplied = do
  private <- canonicalizePath "private-data"
  path <- makeAbsolute supplied
  parent <- canonicalizePath (takeDirectory path)
  exists <- doesPathExist path
  require (splitDirectories private `isPrefixOf` splitDirectories parent && not exists && takeFileName path `notElem` ["", ".", ".."])
    "--output must be a new directory beneath private-data/"
  createDirectory path
  pure (parent </> takeFileName path)

writeJson :: FilePath -> Value -> IO ()
writeJson path value = BL.writeFile path (encode value <> "\n")

main :: IO ()
main = do
  options <- getArgs >>= parseOptions
  directory <- freshOutput (optOutput options)
  card <- makeAbsolute (optCard options)
  firmware <- makeAbsolute (optFirmware options)
  nand <- makeAbsolute (optNand options)
  rom <- makeAbsolute (optBootRom options)
  runDocument <- case optRunDocument options of
    Just (RunDocumentPath path) -> makeAbsolute path
    Nothing -> pure (optSdk options </> "share/fujimod-xm1/xm1-150.json")
  let bridgePath = optSdk options </> "bin/fujimod-xm1-ram-push"
  forM_ [bridgePath, runDocument, firmware, nand, rom] $ \path -> doesFileExist path >>= \present -> require present (path ++ " is missing")
  bridgeSha <- fileDigest bridgePath
  require (bridgeSha == optBridgeSha options) ("bridge SHA-256 " ++ digestText bridgeSha ++ " differs from --bridge-sha256")
  runSha <- fileDigest runDocument
  forM_ (optRunDocumentSha options) $ \expected ->
    require (runSha == expected) ("run document SHA-256 " ++ digestText runSha ++ " differs from --run-document-sha256")
  originalBefore <- cardStat card
  let CardStat originalSize _ _ = originalBefore
  require (originalSize == cardBytes) "card image is not the declared 4 GiB capacity"
  createDirectory (directory </> "card")
  let copy = directory </> "card" </> "card.img"
  callProcess "cp" ["--reflink=auto", "--", card, copy]
  copyStat <- cardStat copy
  require (copyStat /= originalBefore && (let CardStat s _ _ = copyStat in s) == cardBytes) "card copy is not a distinct 4 GiB file"
  checkpoints <- maybe (pure (directory </> "checkpoints")) makeAbsolute (optCheckpoints options)
  createDirectoryIfMissing True checkpoints
  let fresh = optCheckpoints options == Nothing
      arguments =
        [ "--run", runDocument, "--boot-rom", rom, "--installed-dat", firmware, "--adjustment-nand", nand
        , "--output", directory </> "bridge", "--ram-push-checkpoints", checkpoints, "--sd-card-image", copy ]
        ++ ["--fresh-boot" | fresh]
  environment <- getEnvironment
  errors <- openBinaryFile (directory </> "bridge.stderr") WriteMode
  requests <- openBinaryFile (directory </> "bridge.stdin.jsonl") WriteMode
  replies <- openBinaryFile (directory </> "bridge.stdout.jsonl") WriteMode
  journal <- CS.openJournal (directory </> "observations.jsonl")
  writeJson (directory </> "invocation.json") (object
    [ "route" .= routeName (optRoute options), "bridge" .= bridgePath, "bridge_sha256" .= digestText bridgeSha
    , "run_document" .= runDocument, "run_document_sha256" .= digestText runSha
    , "run_document_selected" .= isJust (optRunDocument options)
    , "expect_ramp12_width" .= fmap (\(RampRasterWidth w) -> w) (optRamp options)
    , "arguments" .= arguments, "card_original" .= card, "card_original_stat" .= statValue originalBefore
    , "card_copy_stat" .= statValue copyStat, "windows" .= optWindows options
    , "capture_continuation_observation" .= optCaptureContinuationObservation options
    , "shoot_without_lens" .= optShootWithoutLens options, "window_dial" .= windowDial ])
  (Just input, Just output, _, process) <- createProcess (proc bridgePath arguments)
    { std_in = CreatePipe, std_out = CreatePipe, std_err = UseHandle errors, cwd = Just directory
    , env = Just (environment ++ [("XM1_KEY_RELEASE_LOG", directory </> "key-release.jsonl"), ("XM1_KEY_RELEASE_ROUTE", "issue124-" ++ routeName (optRoute options))]) }
  reader <- CS.newBoundedReader output
  let bridge = Bridge input reader requests replies
  observed <- newObserved
  outcome <- try $ do
    startOptions <- event bridge "start-options"
    note journal "start-options" (Object startOptions)
    send bridge (object ["command" .= ("start" :: T.Text), "installed" .= Null])
    ready <- event bridge "ready"
    note journal "ready" (object ["checkpoint" .= KM.lookup "checkpoint" ready, "pre_attach" .= KM.lookup "pre_attach" ready])
    stopped <- try (runRoute options directory bridge journal observed)
    case stopped of
      Left (BridgeStopped message) -> do
        writeIORef (obsStop observed) (Just message)
        note journal "bridge-stopped" (toJSON message)
        captureProgress <- readIORef (obsCapture observed)
        when (isJust captureProgress) $ do
          tailResult <- try $ do
            _ <- captureObservationHits bridge journal observed
            pure ()
          case tailResult of
            Left e -> note journal "capture-observation-tail-error" (toJSON (displayException (e :: SomeException)))
            Right () -> pure ()
          finished <- try (call bridge (command "finish" []))
          case finished of
            Left e -> note journal "capture-observation-finish-error" (toJSON (displayException (e :: SomeException)))
            Right reply -> note journal "finish" (Object reply)
      Right () -> do
        finished <- call bridge (command "finish" [])
        note journal "finish" (Object finished)
  harnessError <- case outcome of
    Left e -> pure (Just (displayException (e :: SomeException)))
    Right () -> pure Nothing
  _ <- try (hClose input) :: IO (Either IOException ())
  exit <- waitForProcess process
  mapM_ hClose [errors, requests, replies]
  session <- do
    present <- doesFileExist (directory </> "bridge" </> "emulator-session.json")
    if present then either (const Null) id . A.eitherDecodeStrict' <$> B.readFile (directory </> "bridge" </> "emulator-session.json") else pure Null
  stopMessage <- readIORef (obsStop observed)
  hitIndex <- readIORef (obsHitIndex observed)
  case (stopMessage, hitIndex, session) of
    (Just _, Just from, Object o)
      | Just (Array contexts) <- KM.lookup "contexts" o, from <= length contexts
      , not (optCaptureContinuationObservation options)
      , rest <- drop from (toList contexts), parsed <- mapMaybe parseHit rest, length parsed == length rest -> do
          let sites = map siteLabel (routeSites (optRoute options))
          traced <- foldHits observed AfterPress (filter ((`elem` sites) . hitLabel) parsed)
          writeIORef (obsRecovered observed) (Just (length rest))
          note journal "trace-recovered-from-session" (object ["from" .= from, "session_hits" .= length contexts, "trace" .= traced])
          readIORef (obsLedger observed) >>= note journal "trace-ledger" . ledgerValue
    (Just _, _, _) -> note journal "trace-recovered-from-session" Null
    _ -> pure ()
  originalAfter <- cardStat card
  (findings, stopped) <- verdict (optRoute options) (optShootWithoutLens options) observed
  ledger <- readIORef (obsLedger observed)
  recoveredCount <- readIORef (obsRecovered observed)
  captureProgress <- readIORef (obsCapture observed)
  forM_ captureProgress (note journal "capture-observation-interval" . captureProgressValue)
  let unchanged = originalAfter == originalBefore
      bridgeSucceeded = exit == ExitSuccess
      reached = harnessError == Nothing && stopMessage == Nothing && stopped == Nothing && unchanged && bridgeSucceeded
      sessionField name = case session of
        Object o -> KM.lookup name o
        _ -> Nothing
  note journal "bridge-exit" (toJSON (show exit))
  CS.closeJournal journal
  writeJson (directory </> "result.json") (object
    [ "schema" .= (1 :: Int)
    , "scope" .= ("emulated X-M1 1.50 stock " ++ routeName (optRoute options) ++ " route on the granted FujiMod SDK; diagnostic reachability, not acceptance"
        ++ (case optRoute options of
              ShutterContactRoute -> "; one S2 contact (GPIO0.3, row6) with no host S1"
              ShutterSequenceRoute -> "; physical S1 hold, S1+S2, S1 hold, then both released; no firmware-state seeding"
              PlayRoute -> ""))
    , "route" .= routeName (optRoute options)
    , "run_document" .= runDocument, "run_document_sha256" .= digestText runSha
    , "trace_hit_counts" .= ledgerCounts ledger, "trace_hit_counts_before_press" .= ledgerBeforePress ledger
    , "trace_hits_kept_per_label_after_press" .= hitCap
    , "capture_continuation_observation" .= fmap captureProgressValue captureProgress
    , "diagnostic_reachability" .= reached
    , "acceptance" .= String "none: a changed frame with target reads can be READ ERROR or BUSY; rendered-JPEG playback and capture fidelity need inspection of the PNGs and receipts"
    , "stopped_at" .= (if reached then Nothing else stopped <|> (if bridgeSucceeded then Nothing else Just "bridge-exit"))
    , "trace_hits_recovered_from_session" .= recoveredCount
    , "findings" .= map judgedValue findings
    , "harness_error" .= harnessError
    , "original_bridge_error" .= stopMessage
    , "bridge_exit" .= show exit
    , "bridge_exit_success" .= bridgeSucceeded
    , "bridge_boundary" .= sessionField "boundary"
    , "bridge_error" .= sessionField "bridge_error"
    , "checkpoint" .= sessionField "checkpoint"
    , "card_original_unchanged" .= unchanged ])
  forM_ findings $ \(Judged f unknown) -> putStrLn
    ((if isJust unknown then "unknown " else if findingHolds f then "holds " else "fails ")
      ++ (if findingRole f == Gate then "gate    " else "context ") ++ T.unpack (findingName f))
  unless unchanged (hPutStrLn stderr "media-route-smoke: the original card file's size, mtime or inode changed")
  unless bridgeSucceeded (hPutStrLn stderr ("media-route-smoke: bridge exited " ++ show exit))
  putStrLn ("result: " ++ directory </> "result.json")
  case (harnessError, reached) of
    (Just e, _) -> hPutStrLn stderr ("media-route-smoke: " ++ e) >> exitWith (ExitFailure 1)
    (Nothing, True) -> putStrLn "media-route-smoke: every diagnostic gate held; no playback or capture acceptance"
    (Nothing, False) -> do
      hPutStrLn stderr ("media-route-smoke: route stopped at " ++ maybe (if bridgeSucceeded then "card-original-changed" else "bridge-exit") T.unpack stopped)
      exitWith (ExitFailure 3)
