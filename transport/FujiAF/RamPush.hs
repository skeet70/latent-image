{-# LANGUAGE OverloadedStrings #-}
module FujiAF.RamPush
  ( Address(..), WordValue(..), PublicationPhase(..), CoreScope(..), Publication(..)
  , RamPushEntries(..), RamPushManifest(..), RamAllocState(..), RetainedAllocation(..), RamPushState(..)
  , PeerCacheAvailability(..), xm1PeerCacheAvailability
  , RamAllocOutcome(..), RamPushOutcome(..), RamFreeOutcome(..), RamUnloadOutcome(..)
  , bootstrapAddress, bootstrapBytes, peerPrepareBytes, freeBootstrapBytes, callbackSlotAddress
  , callbackTablePointerAddress, callbackTableAddress, callbackStockWord
  , queryOutputAddress, configGateIndex, mailboxBytes
  , reviewedAllocationSize, installerAbiVersion, publicationCount, setupCount, activationCount
  , PayloadKind(..), Payload(..), bankD3Payload, classicChromePayload, classicChromeIconPayload, payloads, payloadOfSize, payloadPublicationCount, payloadEntries
  , readRamPushManifest, parseRamPushManifest, readRamAllocState, readRetainedAllocation, readRamPushState
  , consumedMarkerPath, writeAllocStateFile, writeStateFile
  , ramAllocArguments, ramPushArguments, ramFreeArguments, ramUnloadArguments, ramRelinkArguments
  , ramAllocJournalCapBytes, ramPushJournalCapBytes, ramFreeJournalCapBytes, ramUnloadJournalCapBytes
  , validateReviewedContract, requireReviewedBytes, requireReviewedState, ReviewedContract(..), publicationContractDigest
  , RamPushRelinker(..), runfilesRelinker, PinnedBuild(..)
  , AllowlistPolicy(..), OverrideReason(..), AllowlistOutcome(..), allowlistReceipt
  , pinnedReviewedManifest, withPinnedReviewedBuild, withPinnedReviewedBuildUsing, withRelinkedBuildUsing
  , runRamAlloc, runRamAllocWith, runRamPush, runRamPushWith, runRamRelink
  , runRamFree, runRamFreeWith, runRamUnload, runRamUnloadWith
  ) where

import Control.Exception
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value(..))
import Data.IORef
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import Data.Bits ((.&.), (.|.), shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (digitToInt)
import Data.List (sort)
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word8, Word32)
import Numeric (readHex, showHex)
import Text.Read (readMaybe)
import System.Directory (createDirectory, doesFileExist, getCurrentDirectory, makeAbsolute, removeDirectoryRecursive, removeFile)
import System.Environment (getExecutablePath, lookupEnv)
import System.Exit (ExitCode(..))
import System.FilePath ((</>), (<.>), makeRelative, takeDirectory)
import System.IO (hClose, openTempFile)
import System.IO.Error (isDoesNotExistError)
import System.Posix.Files (getSymbolicLinkStatus, setFileMode)
import System.Process (CreateProcess(cwd, env), proc, readCreateProcessWithExitCode)
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import FujiAF.PlanDocument (privateRegular)
import FujiAF.Read (ReadWindow(..), decodeReadReply, readReplyBytes, readRequestBytes, readRequestFrame)
import FujiAF.SHA256 (sha256)
import FujiAF.StoredGate (openReplyTail)
import FujiAF.Transport
import qualified FujiAF.Broker as P
import RamPushToolchain (RamPushToolchain(..), ramPushToolchain)
import BankAllowlist (Report, checkBankFile, renderFinding, reportValue, violations)

newtype Address = Address { unAddress :: Word32 } deriving (Eq, Ord, Show)
newtype WordValue = WordValue { unWordValue :: Word32 } deriving (Eq, Show)
data PublicationPhase = Setup | Activation deriving (Eq, Show)
data CoreScope = Cpu0 | Cpu1 | Both | UnknownCore deriving (Eq, Show)
data Publication = Publication
  { publicationIndex :: Int
  , publicationPhase :: PublicationPhase
  , publicationAddress :: Address
  , publicationOld :: WordValue
  , publicationNew :: WordValue
  , publicationCore :: CoreScope
  } deriving (Eq, Show)
data RamPushEntries = RamPushEntries
  { installEntry :: Address
  , rollbackEntry :: Address
  , statusEntry :: Address
  } deriving (Eq, Show)
data RamPushManifest = RamPushManifest
  { manifestPath :: FilePath
  , runtimePath :: FilePath
  , manifestDigest :: String
  , sourceIdentity :: [(String,String)]
  , allocationBase :: Address
  , allocationSize :: Int
  , runtimeDigest :: String
  , runtimeBytes :: B.ByteString
  , manifestEntries :: RamPushEntries
  , manifestPublications :: [Publication]
  } deriving (Eq, Show)
data RamAllocState = RamAllocState
  { allocBase :: Address
  , allocSize :: Int
  , allocCpu0 :: Word32
  , allocTablePointer :: Address
  , allocSlotWord :: WordValue
  , allocBootstrap :: B.ByteString
  , allocGuard :: String
  , allocProduct :: String
  , allocConfigF7 :: Word8
  } deriving (Eq, Show)

-- | An allocation state and the file it was read from.
data RetainedAllocation = RetainedAllocation
  { retainedStatePath :: FilePath
  , retainedAllocation :: RamAllocState
  } deriving (Eq, Show)

data RamPushState = RamPushState
  { stateManifestDigest :: String
  , stateRuntimeDigest :: String
  , stateBase :: Address
  , stateSize :: Int
  , stateEntries :: RamPushEntries
  , statePublications :: [Publication]
  , stateSlotWord :: WordValue
  , stateBootstrap :: B.ByteString
  , stateNonce :: Word32
  } deriving (Eq, Show)
data PeerCacheAvailability = PeerCacheBound | PeerCacheUnbound deriving (Eq, Show)
data RamAllocOutcome = RamAllocated FilePath | RamAllocRefused String | RamAllocUncertain String deriving (Eq, Show)
data RamPushOutcome = RamPushInstalled FilePath | RamPushRefused String | RamPushUncertain String deriving (Eq, Show)
data RamFreeOutcome = RamFreed | RamFreeRefused String | RamFreeUncertain String deriving (Eq, Show)
data RamUnloadOutcome = RamUnloaded | RamUnloadRefused String | RamUnloadUncertain String deriving (Eq, Show)

xm1PeerCacheAvailability :: PeerCacheAvailability
xm1PeerCacheAvailability = PeerCacheBound

bootstrapAddress, callbackSlotAddress, callbackTablePointerAddress, callbackTableAddress, queryOutputAddress :: Address
bootstrapAddress = Address 0x01469b20
callbackSlotAddress = Address 0x015c22ac
callbackTablePointerAddress = Address 0x0146a0c4
callbackTableAddress = Address 0x015c22a0
queryOutputAddress = Address 0x01469878

callbackStockWord :: WordValue
callbackStockWord = WordValue 0x00dc7874

cacheCleanInvalidate, cacheControl :: Address
cacheCleanInvalidate = Address 0x0061c4f4
cacheControl = Address 0x0061c490

-- | The source identity and publication contract digest a manifest must carry.
-- The camera binary's values are generated from transport/ram-push-review.json
-- and transport/ram-push-cc-review.json.
data ReviewedContract = ReviewedContract
  { reviewedSourceIdentity :: [(String,String)]
  , reviewedPublicationContractDigest :: String
  }

data PayloadKind = RecipeBankD3 | ClassicChrome | ClassicChromeIcon deriving (Eq, Show)

-- | What a payload kind's relink links, and the allocation, ABI word, counts
-- and entries its installer has. A runtime's allocation size names its kind.
data Payload = Payload
  { payloadKind :: PayloadKind
  , payloadAllocationSize :: Int
  , payloadAbiVersion :: Word32
  , payloadSetupCount :: Int
  , payloadActivationCount :: Int
  , payloadInstallerStart :: Word32 -> Word32
  , payloadRelinkScript :: FilePath
  , payloadRelinkBuilder :: FilePath
  , payloadBuildRoot :: FilePath
  , payloadRelinkInputs :: [(String,FilePath)]
  , payloadLinksBank :: Bool
  }

bankD3Payload, classicChromePayload, classicChromeIconPayload :: Payload
bankD3Payload = Payload
  { payloadKind = RecipeBankD3
  , payloadAllocationSize = reviewedAllocationSize
  , payloadAbiVersion = installerAbiVersion
  , payloadSetupCount = setupCount
  , payloadActivationCount = activationCount
  , payloadInstallerStart = \base ->
      let candidate = (base .&. 0xffff0000) .|. 0xb784
          nameRecord = if toInteger candidate < toInteger base + 0x9800 then candidate + 0x10000 else candidate
      in (nameRecord + 14*66 + 31) .&. 0xffffffe0
  , payloadRelinkScript = "build-recipe-bank-ram.sh"
  , payloadRelinkBuilder = "build-recipe-bank"
  , payloadBuildRoot = "private-data/analysis/x-m1-1.50/ram-push"
  , payloadRelinkInputs =
      [ ("SPEC","private-data/analysis/x-m1-1.50/recipes/bank-spec.json")
      , ("RC0_ID4","private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin")
      , ("RC0_ID5","private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin")
      , ("RC0_PLAIN","private-data/analysis/x-m1-1.50/lz/plain-01100000.bin")
      ]
  , payloadLinksBank = True
  }
-- | reports/classic-chrome-patch.md section 8.2: 84 setup and 47 activation
-- publications; menu/classic_chrome_ram.S takes 0x184 bytes at the base.
classicChromePayload = Payload
  { payloadKind = ClassicChrome
  , payloadAllocationSize = 0x1560
  , payloadAbiVersion = 3
  , payloadSetupCount = 84
  , payloadActivationCount = 47
  , payloadInstallerStart = \base -> (base + 0x184 + 31) .&. 0xffffffe0
  , payloadRelinkScript = "build-classic-chrome-ram.sh"
  , payloadRelinkBuilder = "build-classic-chrome-ram"
  , payloadBuildRoot = "private-data/analysis/classic-chrome-patch/ram-push"
  , payloadRelinkInputs =
      [ ("CC_PARAMETERS","private-data/analysis/classic-chrome/candidate.json")
      , ("RC0_ID4","private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin")
      , ("RC0_ID5","private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin")
      , ("RC0_PLAIN","private-data/analysis/x-m1-1.50/lz/plain-01100000.bin")
      , ("RC0_ID2","private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin")
      , ("CC_ID4","private-data/analysis/classic-chrome-patch/candidate-build-1/patched-id4.bin")
      , ("CC_ID5","private-data/analysis/classic-chrome-patch/candidate-build-1/patched-id5.bin")
      , ("CC_PLAIN","private-data/analysis/classic-chrome-patch/candidate-build-1/patched-plain.bin")
      ]
  , payloadLinksBank = False
  }
-- | The Classic Chrome payload's publications plus the icon's, built by the
-- Classic Chrome relink script with the icon builder; the glyph record and
-- Q arrays follow the installer in the allocation.
classicChromeIconPayload = classicChromePayload
  { payloadKind = ClassicChromeIcon
  , payloadAllocationSize = 0x1970
  , payloadSetupCount = 100
  , payloadActivationCount = 58
  , payloadRelinkBuilder = "build-classic-chrome-icon-ram"
  , payloadRelinkInputs = payloadRelinkInputs classicChromePayload
      ++ [("RC0_DAT","private-data/analysis/classic-chrome-patch/rc0-regression/FWUP0002.DAT")]
  }

payloads :: [Payload]
payloads = [bankD3Payload, classicChromePayload, classicChromeIconPayload]

payloadOfSize :: Int -> Maybe Payload
payloadOfSize size = case filter ((== size) . payloadAllocationSize) payloads of
  [payload] -> Just payload
  _ -> Nothing

requirePayload :: Int -> IO Payload
requirePayload size = maybe (P.reject ("allocation size "++show size++" is no RAM-push payload's extent")) pure (payloadOfSize size)

payloadPublicationCount :: Payload -> Int
payloadPublicationCount payload = payloadSetupCount payload + payloadActivationCount payload

-- | The installer's three entries, at fixed offsets from its start.
payloadEntries :: Payload -> Address -> RamPushEntries
payloadEntries payload (Address base) =
  let install = payloadInstallerStart payload base + 0x600
  in RamPushEntries (Address install) (Address (install+0x238)) (Address (install+0x264))

configGateIndex, mailboxBytes, reviewedAllocationSize :: Int
configGateIndex = 0xf7
mailboxBytes = 104
reviewedAllocationSize = 0x1b6d8

installerAbiVersion :: Word32
installerAbiVersion = 2

-- | The bank's 94 publications plus D3's 156 (reports/ram-push-d3-design.md).
publicationCount, setupCount, activationCount :: Int
publicationCount = 250
setupCount = 167
activationCount = 83

rendezvousSpinBound :: Word32
rendezvousSpinBound = 0x80000

consumedAllocationTag :: B.ByteString
consumedAllocationTag = C.pack "RPFREED\0"

word32le :: Word32 -> [Word8]
word32le value = [fromIntegral (value `shiftR` shift) | shift <- [0,8,16,24]]

fromWord32le :: [Word8] -> Word32
fromWord32le bytes = sum [fromIntegral byte * 256^index | (index,byte) <- zip [0..3::Int] (take 4 bytes)]

branchLinkEq :: Word32 -> Word32 -> Word32
branchLinkEq site target = 0x0b000000 .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)

branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - site - 8) `shiftR` 2) .&. 0x00ffffff)

bootstrapBytes :: B.ByteString
bootstrapBytes = B.pack (concatMap word32le
  [ 0xe92d4032, 0xe1a05001, 0xee124e10, 0xe3140a01
  , branchLinkEq 0x01469b30 0x0089cda4, 0xe8850011, 0xe3a00002, 0xe8bd8032 ])


peerPrepareBytes :: B.ByteString
peerPrepareBytes = B.pack (concatMap word32le
  [ 0xe92d4010, 0xe1a04001, 0xe5922000, 0xe5911000
  , branchLink 0x01469b30 0x006b0c3c, 0xe5840000, 0xe3800002, 0xe8bd8010 ])
freeBootstrapBytes :: B.ByteString
freeBootstrapBytes = B.pack (concatMap word32le
  [ 0xe92d4070, 0xee124e10, 0xe304c003, 0xe154000c
  , branchLinkEq 0x01469b30 0x0089cdc4, 0x03a00002, 0x13a00002, 0xe8bd8070 ])

require :: Bool -> String -> IO ()
require yes message = unless yes (P.reject message)

field :: Value -> String -> Value
field (Object object) key = maybe Null id (M.lookup (K.fromString key) object)
field _ _ = Null

closed :: Value -> [String] -> IO ()
closed (Object object) keys = require (sort (map K.toString (M.keys object)) == sort keys) "closed RAM-push document keys required"
closed _ _ = P.reject "RAM-push JSON object required"

integer :: Value -> Int -> Int -> IO Int
integer (Number n) low high = case toBoundedInteger n of
  Just value | value >= low && value <= high -> pure value
  _ -> P.reject "bounded exact RAM-push integer required"
integer _ _ _ = P.reject "bounded exact RAM-push integer required"

text :: Value -> Int -> IO String
text (String value) cap = do
  let string = T.unpack value
  require (not (null string) && length string <= cap && all (>= ' ') string) "bounded RAM-push text required"
  pure string
text _ _ = P.reject "RAM-push text required"

array :: Value -> Int -> Int -> IO [Value]
array (Array values) low high = do
  require (V.length values >= low && V.length values <= high) "bounded RAM-push list required"
  pure (V.toList values)
array _ _ _ = P.reject "RAM-push list required"

hex32 :: Value -> IO Word32
hex32 value = do
  encoded <- text value 10
  require (length encoded == 10 && take 2 encoded == "0x" && all (`elem` ("0123456789abcdef" :: String)) (drop 2 encoded)) "eight-digit lowercase hex word required"
  pure (foldl (\acc digit -> acc*16 + fromIntegral (digitToInt digit)) 0 (drop 2 encoded))

phaseValue :: Value -> IO PublicationPhase
phaseValue (String "setup") = pure Setup
phaseValue (String "activation") = pure Activation
phaseValue _ = P.reject "publication phase must be setup or activation"

coreValue :: Value -> IO CoreScope
coreValue (String "cpu0") = pure Cpu0
coreValue (String "cpu1") = pure Cpu1
coreValue (String "both") = pure Both
coreValue (String "unknown") = pure UnknownCore
coreValue _ = P.reject "publication core must be cpu0, cpu1, both or unknown"

parseEntries :: Value -> IO RamPushEntries
parseEntries value = do
  closed value ["install","rollback","status"]
  RamPushEntries <$> (Address <$> hex32 (field value "install"))
                 <*> (Address <$> hex32 (field value "rollback"))
                 <*> (Address <$> hex32 (field value "status"))

parsePublication :: Int -> Value -> IO Publication
parsePublication count value = do
  closed value ["address","core","index","new","old","phase"]
  Publication <$> integer (field value "index") 0 (count-1)
              <*> phaseValue (field value "phase")
              <*> (Address <$> hex32 (field value "address"))
              <*> (WordValue <$> hex32 (field value "old"))
              <*> (WordValue <$> hex32 (field value "new"))
              <*> coreValue (field value "core")

validateManifest :: RamPushManifest -> IO RamPushManifest
validateManifest manifest = do
  let Address base = allocationBase manifest
      size = allocationSize manifest
      end = toInteger base + toInteger size
      publications = manifestPublications manifest
      entries = manifestEntries manifest
      addresses = map (unAddress . publicationAddress) publications
      entryAddresses = map unAddress [installEntry entries,rollbackEntry entries,statusEntry entries]
  require (B.length (runtimeBytes manifest) == size) "runtime.bin length differs from manifest size"
  require (sha256 (runtimeBytes manifest) == runtimeDigest manifest) "runtime.bin SHA256 differs from manifest"
  payload <- requirePayload size
  let count = payloadPublicationCount payload
      setups = payloadSetupCount payload
      activations = payloadActivationCount payload
  require (base .&. 7 == 0 && end <= 0x100000000) "manifest allocation range is invalid"
  require (length publications == count && sort (map publicationIndex publications) == [0..count-1]) ("manifest publication indexes must be a permutation of 0.."++show (count-1))
  require (map publicationPhase publications == replicate setups Setup ++ replicate activations Activation) ("manifest publication phases must be "++show setups++" setup then "++show activations++" activation")
  require (length addresses == length (unique addresses) && all (\address -> address .&. 3 == 0) addresses) "manifest publication addresses must be distinct aligned words"
  require (all (\address -> toInteger base <= toInteger address && toInteger address + 4 <= end) entryAddresses) "manifest entries must lie inside runtime.bin"
  require (all (\address -> address .&. 3 == 0) entryAddresses && length (unique entryAddresses) == 3) "manifest entries must be distinct aligned words"
  require (all (\address -> not (inside address base size) && address /= unAddress callbackSlotAddress && not (inside address (unAddress bootstrapAddress) 32)) addresses) "manifest publication overlaps loader-owned RAM"
  pure manifest
  where
    inside :: Word32 -> Word32 -> Int -> Bool
    unique = foldr (\x xs -> if x `elem` xs then xs else x:xs) []
    inside value start bytes = toInteger start <= toInteger value && toInteger value < toInteger start + toInteger bytes

parseRamPushManifest :: FilePath -> B.ByteString -> B.ByteString -> IO RamPushManifest
parseRamPushManifest path document runtime = do
  value <- either P.reject pure (decodeStrictIntegers document)
  closed value ["base","entry","publications","runtime_sha256","schema","size","source_identity"]
  _ <- integer (field value "schema") 1 1
  Address base <- Address <$> hex32 (field value "base")
  size <- integer (field value "size") (minimum (map payloadAllocationSize payloads)) (maximum (map payloadAllocationSize payloads))
  payload <- requirePayload size
  digest <- text (field value "runtime_sha256") 64
  require (hexDigest digest) "runtime_sha256 must be lowercase SHA256"
  identity <- case field value "source_identity" of
    Object object -> do
      require (not (M.null object) && M.size object <= 32) "bounded nonempty source_identity required"
      forM (M.toList object) $ \(key,item) -> do
        name <- pure (K.toString key)
        hash <- text item 64
        require (not (null name) && length name <= 256 && hexDigest hash) "source_identity entry is invalid"
        pure (name,hash)
    _ -> P.reject "source_identity object required"
  entries <- parseEntries (field value "entry")
  publications <- array (field value "publications") (payloadPublicationCount payload) (payloadPublicationCount payload) >>= mapM (parsePublication (payloadPublicationCount payload))
  validateManifest (RamPushManifest path (takeDirectory path </> "runtime.bin") (sha256 document) identity (Address base) size digest runtime entries publications)

readRamPushManifest :: FilePath -> IO RamPushManifest
readRamPushManifest path = do
  document <- privateRegular "RAM-push manifest" path (512*1024)
  runtime <- privateRegular "RAM-push runtime" (takeDirectory path </> "runtime.bin") (512*1024)
  parseRamPushManifest path document runtime

hexWord :: Word32 -> String
hexWord value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

publicationValue :: Publication -> Value
publicationValue publication = Object (M.fromList
  [ ("index", Number (fromIntegral (publicationIndex publication)))
  , ("phase", String (case publicationPhase publication of Setup -> "setup"; Activation -> "activation"))
  , ("address", String (T.pack (hexWord (unAddress (publicationAddress publication)))))
  , ("old", String (T.pack (hexWord (unWordValue (publicationOld publication)))))
  , ("new", String (T.pack (hexWord (unWordValue (publicationNew publication)))))
  , ("core", String (case publicationCore publication of Cpu0 -> "cpu0"; Cpu1 -> "cpu1"; Both -> "both"; UnknownCore -> "unknown")) ])


publicationContractValue :: Publication -> Value
publicationContractValue publication = Object (M.fromList
  [ ("index", Number (fromIntegral (publicationIndex publication)))
  , ("phase", String (case publicationPhase publication of Setup -> "setup"; Activation -> "activation"))
  , ("address", String (T.pack (hexWord (unAddress (publicationAddress publication)))))
  , ("old", String (T.pack (hexWord (unWordValue (publicationOld publication)))))
  , ("core", String (case publicationCore publication of Cpu0 -> "cpu0"; Cpu1 -> "cpu1"; Both -> "both"; UnknownCore -> "unknown")) ])

publicationContractDigest :: [Publication] -> String
publicationContractDigest publications =
  sha256 (encodeCanonical (Array (V.fromList (map publicationContractValue publications))))

validateReviewedContract :: ReviewedContract -> RamPushManifest -> IO ()
validateReviewedContract contract manifest = do
  require (sort (sourceIdentity manifest) == sort (reviewedSourceIdentity contract)) "manifest source identity differs from the compiled reviewed contract"
  require (publicationContractDigest (manifestPublications manifest) == reviewedPublicationContractDigest contract) "manifest publication contract differs from the compiled reviewed contract"
  payload <- requirePayload (allocationSize manifest)
  require (manifestEntries manifest == payloadEntries payload (allocationBase manifest)) "manifest entry offsets differ from the compiled reviewed layout"

requireReviewedBytes :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> IO ()
requireReviewedBytes suppliedManifest suppliedRuntime reviewedManifest reviewedRuntime = do
  require (suppliedManifest == reviewedManifest) "supplied manifest differs byte-for-byte from the pinned builder output"
  require (suppliedRuntime == reviewedRuntime) "supplied runtime differs byte-for-byte from the pinned builder output"

requireReviewedState :: RamPushState -> RamPushManifest -> IO ()
requireReviewedState state reviewed = do
  require (stateBase state == allocationBase reviewed && stateSize state == allocationSize reviewed) "installed state allocation differs from the pinned builder output"
  require (stateManifestDigest state == manifestDigest reviewed) "installed state manifest hash differs from the pinned builder output"
  require (stateRuntimeDigest state == runtimeDigest reviewed) "installed state runtime hash differs from the pinned builder output"
  require (stateEntries state == manifestEntries reviewed) "installed state entries differ from the pinned builder output"
  require (statePublications state == manifestPublications reviewed) "installed state publications differ from the pinned builder output"
entriesValue :: RamPushEntries -> Value
entriesValue entries = Object (M.fromList
  [ ("install", String (T.pack (hexWord (unAddress (installEntry entries)))))
  , ("rollback", String (T.pack (hexWord (unAddress (rollbackEntry entries)))))
  , ("status", String (T.pack (hexWord (unAddress (statusEntry entries))))) ])

allocStateValue :: RamAllocState -> Value
allocStateValue state = Object (M.fromList
  [ ("schema", Number 1), ("kind", String "xm1-ram-alloc-state")
  , ("base", String (T.pack (hexWord (unAddress (allocBase state)))))
  , ("size", Number (fromIntegral (allocSize state)))
  , ("cpu0", String (T.pack (hexWord (allocCpu0 state))))
  , ("table_pointer", String (T.pack (hexWord (unAddress (allocTablePointer state)))))
  , ("slot_old", String (T.pack (hexWord (unWordValue (allocSlotWord state)))))
  , ("bootstrap", String (T.pack (P.hexBytes (B.unpack (allocBootstrap state)))))
  , ("guard", String (T.pack (allocGuard state)))
  , ("product", String (T.pack (allocProduct state)))
  , ("config_f7", Number (fromIntegral (allocConfigF7 state))) ])

stateValue :: RamPushState -> Value
stateValue state = Object (M.fromList
  [ ("schema", Number 1), ("kind", String "xm1-ram-push-state")
  , ("manifest_sha256", String (T.pack (stateManifestDigest state)))
  , ("runtime_sha256", String (T.pack (stateRuntimeDigest state)))
  , ("base", String (T.pack (hexWord (unAddress (stateBase state)))))
  , ("size", Number (fromIntegral (stateSize state)))
  , ("entry", entriesValue (stateEntries state))
  , ("publications", Array (V.fromList (map publicationValue (statePublications state))))
  , ("slot_old", String (T.pack (hexWord (unWordValue (stateSlotWord state)))))
  , ("bootstrap", String (T.pack (P.hexBytes (B.unpack (stateBootstrap state)))))
  , ("nonce", String (T.pack (hexWord (stateNonce state)))) ])

parseHexBytes :: Value -> Int -> IO B.ByteString
parseHexBytes value count = do
  encoded <- text value (2*count)
  require (length encoded == 2*count && all (`elem` ("0123456789abcdef" :: String)) encoded) "fixed lowercase hex bytes required"
  pure (B.pack [fromIntegral (16*digitToInt a + digitToInt b) | (a,b) <- pairs encoded])
  where pairs [] = []; pairs (a:b:rest) = (a,b):pairs rest; pairs _ = []

parseAllocState :: B.ByteString -> IO RamAllocState
parseAllocState document = do
  value <- either P.reject pure (decodeStrictIntegers document)
  closed value ["base","bootstrap","config_f7","cpu0","guard","kind","product","schema","size","slot_old","table_pointer"]
  _ <- integer (field value "schema") 1 1
  require (field value "kind" == String "xm1-ram-alloc-state") "RAM allocation state kind mismatch"
  base <- Address <$> hex32 (field value "base")
  size <- integer (field value "size") (minimum (map payloadAllocationSize payloads)) (maximum (map payloadAllocationSize payloads))
  _ <- requirePayload size
  cpu0 <- hex32 (field value "cpu0")
  table <- Address <$> hex32 (field value "table_pointer")
  slot <- WordValue <$> hex32 (field value "slot_old")
  original <- parseHexBytes (field value "bootstrap") 32
  guard <- text (field value "guard") 64
  modelName <- text (field value "product") 16
  f7 <- integer (field value "config_f7") 1 255
  require (hexDigest guard) "allocation state guard SHA256 is invalid"
  require (unAddress base /= 0 && unAddress base .&. 7 == 0) "allocation state base is invalid"
  require (cpu0 == 0x4003 && table == callbackTableAddress && slot == callbackStockWord) "allocation state X-M1 bindings are invalid"
  require (original == bootstrapBytes && modelName == "X-M1") "allocation state session identity is invalid"
  pure (RamAllocState base size cpu0 table slot original guard modelName (fromIntegral f7))

readRamAllocState :: FilePath -> IO RamAllocState
readRamAllocState path = privateRegular "RAM allocation state" path (64*1024) >>= parseAllocState

readRetainedAllocation :: FilePath -> IO RetainedAllocation
readRetainedAllocation path = RetainedAllocation path <$> readRamAllocState path

-- | Written beside the allocation state once ram-free ends freed or uncertain.
consumedMarkerPath :: RetainedAllocation -> FilePath
consumedMarkerPath retained = retainedStatePath retained <.> "consumed"

requireUnconsumed :: RetainedAllocation -> IO ()
requireUnconsumed retained = do
  let marker = consumedMarkerPath retained
  status <- try (getSymbolicLinkStatus marker)
  case status of
    Right _ -> P.reject ("allocation state is consumed by an earlier ram-free; marker "++marker++" exists; no camera I/O sent")
    Left failure
      | isDoesNotExistError failure -> pure ()
      | otherwise -> P.reject ("consumed marker "++marker++" cannot be checked: "++displayException failure)

data FreeEnd = FreeEndFreed | FreeEndUncertain

writeConsumedMarker :: RetainedAllocation -> FreeEnd -> IO ()
writeConsumedMarker retained end = writePrivateBytes (consumedMarkerPath retained) (encodeCanonical value <> C.pack "\n")
  where
    allocation = retainedAllocation retained
    value = Object (M.fromList
      [ ("schema", Number 1), ("kind", String "xm1-ram-alloc-consumed")
      , ("base", String (T.pack (hexWord (unAddress (allocBase allocation)))))
      , ("guard", String (T.pack (allocGuard allocation)))
      , ("free_outcome", String (case end of FreeEndFreed -> "freed"; FreeEndUncertain -> "uncertain")) ])

parseState :: B.ByteString -> IO RamPushState
parseState document = do
  value <- either P.reject pure (decodeStrictIntegers document)
  closed value ["base","bootstrap","entry","kind","manifest_sha256","nonce","publications","runtime_sha256","schema","size","slot_old"]
  _ <- integer (field value "schema") 1 1
  require (field value "kind" == String "xm1-ram-push-state") "RAM-unload state kind mismatch"
  manifestHash <- text (field value "manifest_sha256") 64
  runtimeHash <- text (field value "runtime_sha256") 64
  require (hexDigest manifestHash && hexDigest runtimeHash) "state SHA256 is invalid"
  base <- Address <$> hex32 (field value "base")
  size <- integer (field value "size") (minimum (map payloadAllocationSize payloads)) (maximum (map payloadAllocationSize payloads))
  payload <- requirePayload size
  entries <- parseEntries (field value "entry")
  publications <- array (field value "publications") (payloadPublicationCount payload) (payloadPublicationCount payload) >>= mapM (parsePublication (payloadPublicationCount payload))
  slot <- WordValue <$> hex32 (field value "slot_old")
  original <- parseHexBytes (field value "bootstrap") 32
  require (original == bootstrapBytes) "state bootstrap differs from the internally built allocator bootstrap"
  nonce <- hex32 (field value "nonce")
  let state = RamPushState manifestHash runtimeHash base size entries publications slot original nonce
      zeros = B.replicate size 0
      dummy = RamPushManifest "" "" manifestHash [] base size (sha256 zeros) zeros entries publications
  _ <- validateManifest dummy
  pure state

readRamPushState :: FilePath -> IO RamPushState
readRamPushState path = privateRegular "RAM-unload state" path (512*1024) >>= parseState

parseSizeToken :: String -> Maybe Int
parseSizeToken ('0':'x':digits) = case readHex digits of
  [(value,"")] -> Just value
  _ -> Nothing
parseSizeToken value = readMaybe value

ramAllocArguments :: [String] -> Maybe (Int,Maybe FilePath)
ramAllocArguments [size] = do
  bytes <- parseSizeToken size
  if any ((== bytes) . payloadAllocationSize) payloads then Just (bytes,Nothing) else Nothing
ramAllocArguments [size,out] | not (flag out) = do
  bytes <- parseSizeToken size
  if any ((== bytes) . payloadAllocationSize) payloads then Just (bytes,Just out) else Nothing
ramAllocArguments _ = Nothing

ramPushArguments :: [String] -> Maybe (FilePath,FilePath,Maybe FilePath)
ramPushArguments [state,manifest] | not (flag state) && not (flag manifest) = Just (state,manifest,Nothing)
ramPushArguments [state,manifest,out] | all (not . flag) [state,manifest,out] = Just (state,manifest,Just out)
ramPushArguments _ = Nothing

ramFreeArguments :: [String] -> Maybe (FilePath,Maybe FilePath)
ramFreeArguments = operationArguments
ramUnloadArguments :: [String] -> Maybe (FilePath,Maybe FilePath)
ramUnloadArguments = operationArguments
ramRelinkArguments :: [String] -> Maybe (FilePath,FilePath)
ramRelinkArguments [state,out] | not (flag state) && not (flag out) = Just (state,out)
ramRelinkArguments _ = Nothing

operationArguments :: [String] -> Maybe (FilePath,Maybe FilePath)
operationArguments [input] | not (flag input) = Just (input,Nothing)
operationArguments [input,out] | not (flag input) && not (flag out) = Just (input,Just out)
operationArguments _ = Nothing

flag :: String -> Bool
flag ('-':_) = True
flag _ = False

-- One protocol exchange of these operations, as its transfers journal at
-- their widest: packet bytes 0xff, endpoints at the descriptor maxima.
data Exchange
  = OpenExchange | IdentityExchange | CloseExchange
  | ReadExchange Int | WriteExchange Int
  | PaddedWriteExchange | PaddedQueryExchange Int | QueryExchange Int

exchangeTransfers :: Exchange -> [P.Transfer]
exchangeTransfers exchange = case exchange of
  OpenExchange -> [control, input 8]
  IdentityExchange -> request 32 128 ++ request readRequestBytes readReplyBytes
  CloseExchange -> [control, input 2]
  ReadExchange count -> request 36 (36+count)
  WriteExchange count -> request (36+count) 36
  PaddedWriteExchange -> request 0xa4 36
  PaddedQueryExchange replyBytes -> request 0xa4 replyBytes
  QueryExchange replyBytes -> request 32 replyBytes
  where
    control = P.controlTransfer (replicate 12 0xff)
    input = P.BulkIn 0x8f
    request out replyBytes = [control, P.BulkOut 15 (replicate out 0xff), control, input replyBytes]

chunkedExchanges :: Payload -> (Int -> [Exchange]) -> [Exchange]
chunkedExchanges payload exchanges = concat [exchanges (min 248 (size-offset)) | offset <- [0,248..size-1]]
  where size = payloadAllocationSize payload

verifiedWriteExchanges :: Int -> [Exchange]
verifiedWriteExchanges count = [WriteExchange count, ReadExchange count]

sessionExchanges :: [Exchange] -> [Exchange]
sessionExchanges body = OpenExchange : IdentityExchange : body ++ [CloseExchange]

bootstrapExchanges, retargetExchanges, outputCallExchanges :: [Exchange]
bootstrapExchanges = ReadExchange 8 : replicate 6 PaddedWriteExchange ++ replicate 3 (PaddedQueryExchange 32) ++ [ReadExchange 4]
retargetExchanges = verifiedWriteExchanges 4 ++ verifiedWriteExchanges 4 ++ [QueryExchange 32]
outputCallExchanges = retargetExchanges ++ [ReadExchange 8, ReadExchange mailboxBytes]
publicationReadExchanges :: Payload -> [Exchange]
publicationReadExchanges payload = replicate (payloadPublicationCount payload) (ReadExchange 4)

-- The note lines, the stop record and one failing transfer's outcome.
ramJournalMarginBytes :: Int
ramJournalMarginBytes = 65536

-- Each cap is the margin plus the acquisition stages plus every transfer of
-- the operation's completed path at the payload's allocation size.
ramJournalCapFor :: [Exchange] -> Int
ramJournalCapFor exchanges = ramJournalMarginBytes
  + sum (map stageJournalBytes ["init","list","open","claim"])
  + sum (map txJournalBytes (concatMap exchangeTransfers (sessionExchanges exchanges)))

ramAllocJournalCapBytes, ramPushJournalCapBytes, ramFreeJournalCapBytes, ramUnloadJournalCapBytes :: Payload -> Int
ramAllocJournalCapBytes payload = ramJournalCapFor $
  replicate 2 (ReadExchange 4) ++ bootstrapExchanges ++ [ReadExchange 8] ++ verifiedWriteExchanges 4
  ++ bootstrapExchanges ++ [ReadExchange 4] ++ chunkedExchanges payload (pure . ReadExchange)
ramPushJournalCapBytes payload = ramJournalCapFor $
  replicate 2 (ReadExchange 4) ++ publicationReadExchanges payload ++ chunkedExchanges payload (pure . ReadExchange)
  ++ chunkedExchanges payload verifiedWriteExchanges ++ retargetExchanges ++ retargetExchanges ++ outputCallExchanges
  ++ publicationReadExchanges payload
ramFreeJournalCapBytes payload = ramJournalCapFor $
  replicate 2 (ReadExchange 4) ++ chunkedExchanges payload (pure . ReadExchange)
  ++ verifiedWriteExchanges (B.length consumedAllocationTag) ++ bootstrapExchanges ++ [ReadExchange 4]
ramUnloadJournalCapBytes payload = ramJournalCapFor $
  replicate 2 (ReadExchange 4) ++ publicationReadExchanges payload ++ outputCallExchanges ++ publicationReadExchanges payload ++ [ReadExchange 4]

ramFrame :: Int -> Word32 -> [Word8] -> [Word8]
ramFrame function address payload = concatMap word32le [0x200001,0,0,0,fromIntegral function,0,0,address,fromIntegral (B.length (B.pack payload))] ++ payload

readFrame :: Address -> Int -> [Word8]
readFrame (Address address) count = concatMap word32le [0x200001,0,0,0,2,0,0,address,fromIntegral count]

queryFrame :: Word32 -> [Word8]
queryFrame nonce = word32le 0x450006 ++ replicate 12 0 ++ word32le nonce ++ replicate 12 0

data Live = Live Session Candidate Int

openLive :: Session -> Held -> IO Live
openLive session held = do
  device <- acquireDevice session held
  _ <- tx session (P.controlTransfer (P.control 0xc0 8 0)) 12
  reply <- tx session (P.BulkIn (inputEndpoint (descriptors device)) 8) 8
  require (drop 2 reply == openReplyTail) "C0 reply tail mismatch"
  let sid = fromIntegral (reply!!0) + 256*fromIntegral (reply!!1)
  pure (Live session device sid)

identityAndGate :: Live -> IO (String,Word8)
identityAndGate (Live session device sid) = do
  let input = inputEndpoint (descriptors device); output = outputEndpoint (descriptors device)
  _ <- tx session (P.controlTransfer (P.control 0xd0 32 sid)) 12
  _ <- tx session (P.BulkOut output P.identityPacket) 32
  _ <- tx session (P.controlTransfer (P.control 0xd1 128 sid)) 12
  identity <- tx session (P.BulkIn input 128) 128
  model <- either P.reject pure (P.decodeProduct identity)
  require (model == "X-M1") "RAM push requires an X-M1 identity"
  _ <- tx session (P.controlTransfer (P.control 0xd0 readRequestBytes sid)) 12
  _ <- tx session (P.BulkOut output (readRequestFrame (ReadWindow 0 0 "f7"))) readRequestBytes
  _ <- tx session (P.controlTransfer (P.control 0xd1 readReplyBytes sid)) 12
  reply <- tx session (P.BulkIn input readReplyBytes) readReplyBytes
  bytes <- either P.reject pure (decodeReadReply (ReadWindow 0 0 "f7") reply)
  require (bytes !! configGateIndex /= 0) "0xf7 reads zero; RAM push refused"
  pure (model, bytes !! configGateIndex)

readMemory :: Live -> Address -> Int -> IO B.ByteString
readMemory (Live session device sid) address count = do
  require (count >= 1 && count <= 248) "RAM read length outside 1..248"
  let input = inputEndpoint (descriptors device); output = outputEndpoint (descriptors device); frame = readFrame address count; replyBytes = 36+count
  _ <- tx session (P.controlTransfer (P.control 0xd0 36 sid)) 12
  _ <- tx session (P.BulkOut output frame) 36
  _ <- tx session (P.controlTransfer (P.control 0xd1 replyBytes sid)) 12
  reply <- tx session (P.BulkIn input replyBytes) replyBytes
  require (take 4 reply == word32le 0x200001 && fromWord32le (drop 4 reply) == 0 && fromWord32le (drop 8 reply) == 0 && fromWord32le (drop 16 reply) == 2 && fromWord32le (drop 28 reply) == unAddress address && fromWord32le (drop 32 reply) == fromIntegral count) "RAM read reply mismatch"
  pure (B.pack (drop 36 reply))

writeMemory :: Live -> Address -> B.ByteString -> IO ()
writeMemory (Live session device sid) address payload = do
  let count = B.length payload
  require (count >= 1 && count <= 256) "RAM write length outside 1..256"
  let input = inputEndpoint (descriptors device); output = outputEndpoint (descriptors device); frame = ramFrame 3 (unAddress address) (B.unpack payload)
  _ <- tx session (P.controlTransfer (P.control 0xd0 (36+count) sid)) 12
  _ <- tx session (P.BulkOut output frame) (36+count)
  _ <- tx session (P.controlTransfer (P.control 0xd1 36 sid)) 12
  reply <- tx session (P.BulkIn input 36) 36
  require (take 4 reply == word32le 0x200001 && fromWord32le (drop 4 reply) == 0 && fromWord32le (drop 8 reply) == 0 && fromWord32le (drop 16 reply) == 3 && fromWord32le (drop 28 reply) == unAddress address && fromWord32le (drop 32 reply) == fromIntegral count) "RAM write reply mismatch"
writeMemoryPadded :: Live -> Address -> B.ByteString -> B.ByteString -> IO ()
writeMemoryPadded (Live session device sid) address payload window = do
  let count = B.length payload
      prefix = ramFrame 3 (unAddress address) (B.unpack payload)
      frame = prefix ++ replicate (0x84 - length prefix) 0 ++ B.unpack window
      input = inputEndpoint (descriptors device)
      output = outputEndpoint (descriptors device)
  require (count >= 1 && count <= 256 && B.length window == 32 && length frame == 0xa4) "padded RAM write shape mismatch"
  _ <- tx session (P.controlTransfer (P.control 0xd0 0xa4 sid)) 12
  _ <- tx session (P.BulkOut output frame) 0xa4
  _ <- tx session (P.controlTransfer (P.control 0xd1 36 sid)) 12
  reply <- tx session (P.BulkIn input 36) 36
  require (take 4 reply == word32le 0x200001 && fromWord32le (drop 4 reply) == 0 && fromWord32le (drop 8 reply) == 0 && fromWord32le (drop 16 reply) == 3 && fromWord32le (drop 28 reply) == unAddress address && fromWord32le (drop 32 reply) == fromIntegral count) "padded RAM write reply mismatch"

queryPadded :: Live -> Word32 -> Int -> B.ByteString -> IO B.ByteString
queryPadded (Live session device sid) tag replyBytes window = do
  let prefix = queryFrame tag
      frame = prefix ++ replicate (0x84 - length prefix) 0 ++ B.unpack window
      input = inputEndpoint (descriptors device)
      output = outputEndpoint (descriptors device)
  require (B.length window == 32 && length frame == 0xa4) "padded query shape mismatch"
  _ <- tx session (P.controlTransfer (P.control 0xd0 0xa4 sid)) 12
  _ <- tx session (P.BulkOut output frame) 0xa4
  _ <- tx session (P.controlTransfer (P.control 0xd1 replyBytes sid)) 12
  B.pack <$> tx session (P.BulkIn input replyBytes) replyBytes

withCallbackRetarget :: String -> IO () -> IO a -> IO a
withCallbackRetarget target restoreSlot action = mask $ \restore -> do
  result <- try (restore action)
  restored <- try restoreSlot
  case (result,restored) of
    (Right value,Right ()) -> pure value
    (Left (failure::SomeException),Right ()) ->
      P.reject ("callback transaction failed after retarget; stock callback slot restored: "++displayException failure)
    (Right _,Left (failure::SomeException)) ->
      P.reject ("callback slot may still point at "++target++"; do not use camera; power it off now: "++displayException failure)
    (Left (failure::SomeException),Left (restoreFailure::SomeException)) ->
      P.reject ("callback slot may still point at "++target++"; do not use camera; power it off now: "
                ++displayException failure++"; restore failed: "++displayException restoreFailure)

bootstrapCall :: Live -> B.ByteString -> Word32 -> IO B.ByteString
bootstrapCall live window tag = do
  savedOutput <- readMemory live queryOutputAddress 8
  let boundedOutput = B.pack (word32le (unAddress bootstrapAddress) ++ word32le 32)
      restoreSlot = do
        writeMemoryPadded live callbackSlotAddress (B.pack (word32le (unWordValue callbackStockWord))) window
        restored <- wordAt <$> readMemory live callbackSlotAddress 4
        require (restored == unWordValue callbackStockWord) "callback slot restore after bootstrap mismatch"
  writeMemoryPadded live queryOutputAddress boundedOutput window
  withCallbackRetarget "request-buffer bootstrap bytes" restoreSlot $ do
    writeMemoryPadded live callbackSlotAddress (B.pack (word32le (unAddress cacheCleanInvalidate))) window
    _ <- queryPadded live 0 32 window
    writeMemoryPadded live callbackSlotAddress (B.pack (word32le (unAddress cacheControl))) window
    _ <- queryPadded live 0 32 window
    writeMemoryPadded live queryOutputAddress savedOutput window
    writeMemoryPadded live callbackSlotAddress (B.pack (word32le (unAddress bootstrapAddress))) window
    queryPadded live tag 32 window

requireGenericCallbackReply :: B.ByteString -> IO ()
requireGenericCallbackReply reply =
  require (B.take 8 (B.drop 4 reply) == B.pack (word32le 1 ++ word32le 0xffff)) "generic callback reply mismatch"

writeVerified :: Live -> Address -> B.ByteString -> IO ()
writeVerified live address bytes = do
  writeMemory live address bytes
  readback <- readMemory live address (B.length bytes)
  require (readback == bytes) "RAM write readback mismatch"

writeWordVerified :: Live -> Address -> Word32 -> IO ()
writeWordVerified live address value = writeVerified live address (B.pack (word32le value))

queryGeneric :: Live -> Word32 -> Int -> IO B.ByteString
queryGeneric (Live session device sid) tag replyBytes = do
  let input = inputEndpoint (descriptors device); output = outputEndpoint (descriptors device); frame = queryFrame tag
  _ <- tx session (P.controlTransfer (P.control 0xd0 32 sid)) 12
  _ <- tx session (P.BulkOut output frame) 32
  _ <- tx session (P.controlTransfer (P.control 0xd1 replyBytes sid)) 12
  B.pack <$> tx session (P.BulkIn input replyBytes) replyBytes

retargetCall :: Live -> Word32 -> Word32 -> Int -> IO B.ByteString
retargetCall live target tag replyBytes =
  withCallbackRetarget "stock cache-helper bytes"
    (writeWordVerified live callbackSlotAddress (unWordValue callbackStockWord)) $ do
      writeWordVerified live callbackSlotAddress target
      queryGeneric live tag replyBytes

-- | The stock 0x450006 reply is a 32-byte header naming the output length;
-- the mailbox itself is read from the callback's output descriptor.
retargetOutputCall :: Live -> Address -> Int -> Word32 -> Word32 -> IO B.ByteString
retargetOutputCall live allocationBase allocationLength target tag = do
  reply <- withCallbackRetarget "uploaded runtime bytes"
    (writeWordVerified live callbackSlotAddress (unWordValue callbackStockWord)) $ do
      writeWordVerified live callbackSlotAddress target
      queryGeneric live tag 32
  require (B.length reply == 32 && wordAt reply == 0x00450006 && wordAt (B.drop 20 reply) == fromIntegral mailboxBytes) "output callback reply header mismatch"
  descriptor <- readMemory live queryOutputAddress 8
  let outputAddress = wordAt descriptor
      outputLength = fromIntegral (wordAt (B.drop 4 descriptor))
      allocationStart = toInteger (unAddress allocationBase)
      allocationEnd = allocationStart + toInteger allocationLength
      outputStart = toInteger outputAddress
      outputEnd = outputStart + toInteger outputLength
  require (outputLength == mailboxBytes && outputAddress .&. 3 == 0 &&
           outputStart >= allocationStart && outputEnd <= allocationEnd) "output callback mailbox descriptor is invalid"
  readMemory live (Address outputAddress) mailboxBytes

synchronizeUploaded :: Live -> IO ()
synchronizeUploaded live = do
  _ <- retargetCall live (unAddress cacheCleanInvalidate) 0 32
  _ <- retargetCall live (unAddress cacheControl) 0 32
  pure ()

wordAt :: B.ByteString -> Word32
wordAt = fromWord32le . B.unpack

preflightSlot :: Live -> IO WordValue
preflightSlot live = do
  tablePointer <- wordAt <$> readMemory live callbackTablePointerAddress 4
  require (tablePointer == unAddress callbackTableAddress) "query callback table pointer mismatch"
  slot <- wordAt <$> readMemory live callbackSlotAddress 4
  require (slot == unWordValue callbackStockWord) "query callback slot mismatch"
  pure (WordValue slot)

preflight :: Live -> [Publication] -> IO WordValue
preflight live publications = do
  slot <- preflightSlot live
  forM_ publications $ \publication -> do
    current <- wordAt <$> readMemory live (publicationAddress publication) 4
    require (current == unWordValue (publicationOld publication)) ("publication old-word mismatch at "++hexWord (unAddress (publicationAddress publication)))
  pure slot

allocationDigest :: Live -> Address -> Int -> IO String
allocationDigest live base size = do
  chunks <- forM [0,248..size-1] $ \offset -> do
    let count = min 248 (size-offset)
    readMemory live (Address (unAddress base + fromIntegral offset)) count
  pure (sha256 (B.concat chunks))

uploadRuntime :: Live -> RamPushManifest -> IO ()
uploadRuntime live manifest = forM_ [0,248..allocationSize manifest-1] $ \offset -> do
  let chunk = B.take 248 (B.drop offset (runtimeBytes manifest))
      address = Address (unAddress (allocationBase manifest) + fromIntegral offset)
  writeVerified live address chunk

nonceFor :: RamPushManifest -> Word32
nonceFor manifest = fromWord32le (take 4 (B.unpack (C.pack (manifestDigest manifest)))) .&. 0x00ffffff

mailboxStatus :: Payload -> B.ByteString -> Word32 -> Word32 -> IO Bool
mailboxStatus payload mailbox nonce operation = do
  require (B.length mailbox == mailboxBytes) "installer mailbox length mismatch"
  let word offset = fromWord32le (drop offset (B.unpack mailbox))
      count = fromIntegral (payloadPublicationCount payload)
  require (word 0 == 0x31504d52 && word 4 == payloadAbiVersion payload && word 8 == nonce && word 12 == operation) "installer mailbox identity mismatch"
  if word 16 == 0xfffffff9
    then pure False
    else do
      when (word 16 == 0xfffffff8) $
        P.reject "installer completion timed out after rendezvous; publication state is uncertain; do not use camera; power it off now"
      require (word 16 == 0 && word 20 == count) "installer refused or did not validate all publications"
      when (operation == 1) (require (word 24 == fromIntegral (payloadSetupCount payload) && word 28 == fromIntegral (payloadActivationCount payload) && word 32 == 0) "installer publication counts mismatch")
      when (operation == 2) (require (word 32 == count) "rollback restore count mismatch")
      require (word 36 == 0x4003 && word 40 == nonce && word 44 /= word 36 && word 48 == 0 && word 52 == count) "installer core or peer report mismatch"
      require (word 56 == rendezvousSpinBound && word 60 < rendezvousSpinBound && word 76 < rendezvousSpinBound) "installer rendezvous spin report mismatch"
      require (word 64 == nonce && word 68 == nonce && word 72 == nonce) "installer rendezvous report mismatch"
      pure True

writeAllocStateFile :: FilePath -> RamAllocState -> IO FilePath
writeAllocStateFile directory state = do
  let path = directory </> "ram-alloc-state.json"
  writePrivateBytes path (encodeCanonical (allocStateValue state) <> C.pack "\n")
  pure path

runRamAlloc :: Int -> Clock -> UsbApi -> FilePath -> IO RamAllocOutcome
runRamAlloc = runRamAllocWith xm1PeerCacheAvailability

runRamAllocWith :: PeerCacheAvailability -> Int -> Clock -> UsbApi -> FilePath -> IO RamAllocOutcome
runRamAllocWith availability size clock api directory
  | availability == PeerCacheUnbound = pure (RamAllocRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
  | Nothing <- payloadOfSize size = pure (RamAllocRefused ("allocation size must equal a payload's extent: "++unwords [hexWord (fromIntegral (payloadAllocationSize p)) | p <- payloads]))
  | Just payload <- payloadOfSize size = do
      let chunks = (size+247) `div` 248
          cap = ramAllocJournalCapBytes payload
          deadline = transferTimeoutMs*(200 + 4*chunks)
      session <- newSession clock api directory cap deadline
      held <- newHeld
      active <- newIORef False
      opened <- newIORef Nothing
      result <- try $ mask $ \restore -> restore $ do
        live <- openLive session held
        writeIORef opened (Just live)
        (modelName,f7) <- identityAndGate live
        slot <- preflightSlot live
        writeIORef active True
        allocationReply <- bootstrapCall live bootstrapBytes (fromIntegral size)
        requireGenericCallbackReply allocationReply
        output <- readMemory live queryOutputAddress 8
        let base = wordAt output; core = fromWord32le (drop 4 (B.unpack output))
        require (core == 0x4003 && base /= 0 && base .&. 7 == 0) "allocator bootstrap returned an invalid base or CPU0 identity"
        note session "allocation-owned" ("base="++hexWord base++";size="++show size)
        writeWordVerified live (Address (unAddress queryOutputAddress + 4)) (fromIntegral size)
        peerReply <- bootstrapCall live peerPrepareBytes 6
        requireGenericCallbackReply peerReply
        peerStatus <- wordAt <$> readMemory live queryOutputAddress 4
        require (peerStatus == 0) "logical-core-1 pre-upload range maintenance failed"
        guard <- allocationDigest live (Address base) size
        path <- writeAllocStateFile directory (RamAllocState (Address base) size core callbackTableAddress slot bootstrapBytes guard modelName f7)
        note session "ram-alloc-complete" ("state="++path)
        writeIORef active False
        pure path
      isActive <- readIORef active
      liveForCleanup <- if isActive then pure Nothing else readIORef opened
      cleanupFailures <- cleanupHost held api liveForCleanup
      case (result,cleanupFailures) of
        (_, failure:_) -> pure (RamAllocUncertain ("cleanup uncertain: "++failure))
        (Right path,[]) -> pure (RamAllocated path)
        (Left (failure::SomeException),[]) -> do
          let message = displayException failure
          _ <- try (recordWithin cap directory "ram-alloc-stop" (message++";allocation-active="++show isActive)) :: IO (Either SomeException ())
          pure (if isActive then RamAllocUncertain message else RamAllocRefused message)

writeStateFile :: FilePath -> RamPushState -> IO FilePath
writeStateFile directory state = do
  let path = directory </> "ram-push-state.json"
  writePrivateBytes path (encodeCanonical (stateValue state) <> C.pack "\n")
  pure path

-- | Workspace root: BUILD_WORKSPACE_DIRECTORY under `bazel run`, else the
-- current directory.
reviewedWorkspace :: IO FilePath
reviewedWorkspace = lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= maybe getCurrentDirectory pure

-- | Passes the workspace root and the build directory relative to it.
withReviewedBuildDirectory :: Payload -> (FilePath -> FilePath -> IO a) -> IO a
withReviewedBuildDirectory payload action = do
  root <- reviewedWorkspace >>= makeAbsolute
  bracket (create root) (removeDirectoryRecursive . (root </>)) (action root)
  where
    create root = do
      (path,temporaryHandle) <- openTempFile (root </> payloadBuildRoot payload) ".transport-auth-"
      hClose temporaryHandle
      removeFile path
      createDirectory path
      setFileMode path 0o700
      pure (makeRelative root path)

-- | The relink script, builder and toolchain a pinned build runs, and the
-- payload they build.
data RamPushRelinker = RamPushRelinker
  { relinkerScript :: FilePath
  , relinkerBuilder :: FilePath
  , relinkerToolchain :: RamPushToolchain
  , relinkerPayload :: Payload
  }

-- | From the running executable's runfiles, which `//transport:camera-core`
-- populates in every binary linking it.
runfilesRelinker :: Payload -> IO RamPushRelinker
runfilesRelinker payload = do
  executable <- getExecutablePath
  let firmware = (executable ++ ".runfiles") </> "_main" </> "host" </> "firmware"
      relinker = RamPushRelinker (firmware </> payloadRelinkScript payload) (firmware </> payloadRelinkBuilder payload) ramPushToolchain payload
  forM_ [relinkerScript relinker, relinkerBuilder relinker] $ \path -> do
    present <- doesFileExist path
    require present ("RAM-push relinker is missing from this binary's runfiles: "++path)
  pure relinker

-- | A pinned build's outputs; `pinnedDirectory` is removed when the action returns.
data PinnedBuild = PinnedBuild
  { pinnedManifest :: RamPushManifest
  , pinnedDocument :: B.ByteString
  , pinnedRuntime :: B.ByteString
  , pinnedDirectory :: FilePath
  , pinnedAllowlist :: AllowlistOutcome
  }

-- | Whether a build whose bank ELF fails the allowlist is still returned.
data AllowlistPolicy = EnforceAllowlist | OverrideAllowlist OverrideReason deriving (Eq, Show)

-- | Why a failing bank build was let through.
newtype OverrideReason = OverrideReason String deriving (Eq, Show)

-- | NoBankCode: the payload links no recipe-bank object, so no bank ELF is checked.
data AllowlistOutcome = AllowlistPassed Report | AllowlistOverridden OverrideReason Report | NoBankCode

-- | The outcome as a receipt object: the check's report, with an overridden
-- result carrying its reason.
allowlistReceipt :: AllowlistOutcome -> Value
allowlistReceipt (AllowlistPassed report) = reportValue report
allowlistReceipt (AllowlistOverridden (OverrideReason reason) report) = case reportValue report of
  Object object -> Object (M.insert "result" (String "overridden") (M.insert "override_reason" (String (T.pack reason)) object))
  other -> other
allowlistReceipt NoBankCode = Object (M.fromList [("result", String "not-applicable"), ("reason", String "the payload links no recipe-bank object")])

bankAllowlistPath :: FilePath
bankAllowlistPath = "menu/recipe_bank_allowlist.json"

-- | Checks the build's kernel.elf against the workspace allowlist the
-- manifest's source identity names.
checkRelinkedBank :: AllowlistPolicy -> FilePath -> FilePath -> RamPushManifest -> IO AllowlistOutcome
checkRelinkedBank policy root output manifest = do
  allowlist <- B.readFile (root </> bankAllowlistPath)
  require (lookup bankAllowlistPath (sourceIdentity manifest) == Just (sha256 allowlist)) "bank allowlist differs from the manifest's source identity"
  elf <- B.readFile (output </> "kernel.elf")
  require (B.length allowlist <= 1024*1024 && B.length elf <= 4*1024*1024) "bank allowlist or kernel.elf exceeds its size bound"
  report <- either (P.reject . ("bank ELF allowlist check failed before USB: "++)) pure (checkBankFile allowlist elf)
  case (violations report, policy) of
    ([], _) -> pure (AllowlistPassed report)
    (_, OverrideAllowlist reason) -> pure (AllowlistOverridden reason report)
    (found, EnforceAllowlist) -> P.reject
      ("bank ELF allowlist refused the build before USB with " ++ show (length found) ++ " violations: "
        ++ unwords (map renderFinding (take 8 found)))

-- | Runs the pinned builder at `base` and returns its manifest after
-- 'validateReviewedContract'.
pinnedReviewedManifest :: ReviewedContract -> Payload -> Address -> IO RamPushManifest
pinnedReviewedManifest contract payload base = withPinnedReviewedBuild contract payload base (pure . pinnedManifest)

withPinnedReviewedBuild :: ReviewedContract -> Payload -> Address -> (PinnedBuild -> IO a) -> IO a
withPinnedReviewedBuild contract payload base action = do
  relinker <- runfilesRelinker payload
  withPinnedReviewedBuildUsing contract relinker base action

withPinnedReviewedBuildUsing :: ReviewedContract -> RamPushRelinker -> Address -> (PinnedBuild -> IO a) -> IO a
withPinnedReviewedBuildUsing contract relinker base action = withRelinkedBuildUsing relinker EnforceAllowlist base $ \built -> do
  validateReviewedContract contract (pinnedManifest built)
  action built

-- | The relinker's output without 'validateReviewedContract', after the bank
-- ELF allowlist check under `policy` for a payload that links the bank.
withRelinkedBuildUsing :: RamPushRelinker -> AllowlistPolicy -> Address -> (PinnedBuild -> IO a) -> IO a
withRelinkedBuildUsing relinker policy base action = withReviewedBuildDirectory payload $ \root directory -> do
  let tools = relinkerToolchain relinker
      environment =
        [ ("PATH",toolCoreutilsBin tools)
        , ("RAM_PUSH_BUILDER",relinkerBuilder relinker)
        , ("RAM_PUSH_CLANG",toolClang tools)
        , ("RAM_PUSH_LD_LLD",toolLdLld tools)
        , ("RAM_PUSH_OBJCOPY",toolObjcopy tools)
        , ("RAM_PUSH_NM",toolNm tools)
        , ("RAM_PUSH_OBJDUMP",toolObjdump tools)
        ] ++ payloadRelinkInputs payload
      command = (proc (toolBash tools) [relinkerScript relinker,hexWord (unAddress base),directory])
        { env = Just environment, cwd = Just root }
  (exitCode,_,errors) <- readCreateProcessWithExitCode command ""
  require (exitCode == ExitSuccess) ("pinned RAM-push builder failed before USB: "++drop (length errors - 1024) errors)
  let output = root </> directory
  let reviewedManifestPath = output </> "ram-push-manifest.json"
      reviewedRuntimePath = output </> "runtime.bin"
  reviewedDocument <- privateRegular "reviewed RAM-push manifest" reviewedManifestPath (512*1024)
  reviewedRuntime <- privateRegular "reviewed RAM-push runtime" reviewedRuntimePath (512*1024)
  reviewed <- parseRamPushManifest reviewedManifestPath reviewedDocument reviewedRuntime
  require (allocationSize reviewed == payloadAllocationSize payload) "relinked manifest is not the relinker's payload"
  allowlist <- if payloadLinksBank payload then checkRelinkedBank policy root output reviewed else pure NoBankCode
  action (PinnedBuild reviewed reviewedDocument reviewedRuntime output allowlist)
  where payload = relinkerPayload relinker

authenticateReviewedManifest :: ReviewedContract -> RamAllocState -> RamPushManifest -> IO RamPushManifest
authenticateReviewedManifest contract allocation supplied = do
  require (allocationBase supplied == allocBase allocation) "manifest base differs from the retained allocation"
  validateReviewedContract contract supplied
  suppliedDocument <- privateRegular "supplied RAM-push manifest" (manifestPath supplied) (512*1024)
  suppliedRuntime <- privateRegular "supplied RAM-push runtime" (runtimePath supplied) (512*1024)
  payload <- requirePayload (allocSize allocation)
  withPinnedReviewedBuild contract payload (allocBase allocation) $ \pinned -> do
    requireReviewedBytes suppliedDocument suppliedRuntime (pinnedDocument pinned) (pinnedRuntime pinned)
    pure supplied

authenticateReviewedState :: ReviewedContract -> RamPushState -> IO ()
authenticateReviewedState contract state = do
  require (publicationContractDigest (statePublications state) == reviewedPublicationContractDigest contract) "installed state publication contract differs from the compiled reviewed contract"
  payload <- requirePayload (stateSize state)
  require (stateEntries state == payloadEntries payload (stateBase state)) "installed state entry offsets differ from the compiled reviewed layout"
  withPinnedReviewedBuild contract payload (stateBase state) $ \pinned ->
    requireReviewedState state (pinnedManifest pinned)

-- | Writes the pinned build at the retained base into `directory` as
-- `ram-push-manifest.json` and `runtime.bin`, mode 0600; returns the manifest path.
runRamRelink :: ReviewedContract -> RetainedAllocation -> FilePath -> IO FilePath
runRamRelink contract retained directory = do
  requireUnconsumed retained
  payload <- requirePayload (allocSize (retainedAllocation retained))
  withPinnedReviewedBuild contract payload (allocBase (retainedAllocation retained)) $ \pinned -> do
    let path = directory </> "ram-push-manifest.json"
    writePrivateBytes (directory </> "runtime.bin") (pinnedRuntime pinned)
    writePrivateBytes path (pinnedDocument pinned)
    pure path

runRamPush :: ReviewedContract -> RetainedAllocation -> RamPushManifest -> Clock -> UsbApi -> FilePath -> IO RamPushOutcome
runRamPush contract retained supplied clock api directory = do
  let allocation = retainedAllocation retained
  authenticated <- try (requireUnconsumed retained >> authenticateReviewedManifest contract allocation supplied)
  case authenticated of
    Left (failure::SomeException) -> pure (RamPushRefused (displayException failure))
    Right manifest -> runRamPushWith xm1PeerCacheAvailability allocation manifest clock api directory

runRamPushWith :: PeerCacheAvailability -> RamAllocState -> RamPushManifest -> Clock -> UsbApi -> FilePath -> IO RamPushOutcome
runRamPushWith availability allocation manifest clock api directory
  | availability == PeerCacheUnbound = pure (RamPushRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
  | Nothing <- payloadOfSize (allocSize allocation) = pure (RamPushRefused ("retained allocation size "++show (allocSize allocation)++" is no payload's extent"))
  | allocationSize manifest /= allocSize allocation = pure (RamPushRefused "manifest size differs from the retained allocation's extent")
  | allocationBase manifest /= allocBase allocation = pure (RamPushRefused "manifest base differs from the retained allocation")
  | Just payload <- payloadOfSize (allocSize allocation) = do
      let cap = ramPushJournalCapBytes payload
          count = payloadPublicationCount payload
          runtimeChunks = (allocationSize manifest+247) `div` 248
          allocationChunks = (allocSize allocation+247) `div` 248
          deadline = transferTimeoutMs * (200 + 2*count + 8*runtimeChunks + 4*allocationChunks)
      session <- newSession clock api directory cap deadline
      held <- newHeld
      active <- newIORef False
      opened <- newIORef Nothing
      result <- try $ mask $ \restore -> restore $ do
        live <- openLive session held
        writeIORef opened (Just live)
        (modelName,_) <- identityAndGate live
        require (modelName == allocProduct allocation) "allocation session product identity mismatch"
        slot <- preflight live (manifestPublications manifest)
        require (slot == allocSlotWord allocation) "callback slot differs from allocation state"
        guard <- allocationDigest live (allocBase allocation) (allocSize allocation)
        require (guard == allocGuard allocation) "retained allocation guard changed before upload"
        note session "ram-push-preflight" ("allocation state base and size, C0 identity, f7, table, slot and "++show count++" old words matched;writes=0")
        note session "allocation-owned" ("base="++hexWord (unAddress (allocBase allocation))++";size="++show (allocSize allocation))
        writeIORef active True
        uploadRuntime live manifest
        synchronizeUploaded live
        nonce <- pure (nonceFor manifest)
        mailbox <- retargetOutputCall live (allocationBase manifest) (allocationSize manifest) (unAddress (installEntry (manifestEntries manifest))) nonce
        installed <- mailboxStatus payload mailbox nonce 1
        unless installed $ do
          writeIORef active False
          P.reject "installer refused before publication: CPU1 park rendezvous timed out"
        forM_ (manifestPublications manifest) $ \publication -> do
          current <- wordAt <$> readMemory live (publicationAddress publication) 4
          require (current == unWordValue (publicationNew publication)) ("installed word readback mismatch at "++hexWord (unAddress (publicationAddress publication)))
        statePath <- writeStateFile directory (RamPushState (manifestDigest manifest) (runtimeDigest manifest) (allocBase allocation) (allocSize allocation) (manifestEntries manifest) (manifestPublications manifest) slot bootstrapBytes nonce)
        note session "ram-push-installed" ("state="++statePath++";publications="++show count)
        writeIORef active False
        pure statePath
      isActive <- readIORef active
      liveForCleanup <- if isActive then pure Nothing else readIORef opened
      cleanupFailures <- cleanupHost held api liveForCleanup
      case (result,cleanupFailures) of
        (_, failure:_) -> do
          let message = "cleanup uncertain: "++failure
          _ <- try (recordWithin cap directory "ram-push-stop" (message++";allocation-active="++show isActive)) :: IO (Either SomeException ())
          pure (RamPushUncertain message)
        (Right path,[]) -> pure (RamPushInstalled path)
        (Left (failure::SomeException),[]) -> do
          let message = displayException failure
          _ <- try (recordWithin cap directory "ram-push-stop" (message++";allocation-active="++show isActive)) :: IO (Either SomeException ())
          pure (if isActive then RamPushUncertain message else RamPushRefused message)

closeLive :: Live -> IO ()
closeLive (Live session device sid) = do
  _ <- tx session (P.controlTransfer (P.control 0xc4 2 sid)) 12
  _ <- tx session (P.BulkIn (inputEndpoint (descriptors device)) 2) 2
  pure ()

cleanupHost :: Held -> UsbApi -> Maybe Live -> IO [String]
cleanupHost held api live = do
  failures <- newIORef []
  let attempt label action = action `catch` \(failure::SomeException) ->
        modifyIORef' failures (++[label++": "++displayException failure])
  forM_ live (attempt "protocol close" . closeLive)
  releaseHeld held api (\label action -> attempt label (action transferTimeoutMs))
  readIORef failures

runRamFree :: RetainedAllocation -> Clock -> UsbApi -> FilePath -> IO RamFreeOutcome
runRamFree retained clock api directory = do
  unconsumed <- try (requireUnconsumed retained)
  case unconsumed of
    Left (failure::SomeException) -> pure (RamFreeRefused (displayException failure))
    Right () -> do
      outcome <- runRamFreeWith xm1PeerCacheAvailability (retainedAllocation retained) clock api directory
      case outcome of
        RamFreeRefused _ -> pure outcome
        RamFreed -> consume FreeEndFreed "allocation freed" outcome
        RamFreeUncertain reason -> consume FreeEndUncertain reason outcome
  where
    consume end reason outcome = do
      written <- try (writeConsumedMarker retained end)
      pure $ case written of
        Right () -> outcome
        Left (failure::SomeException) -> RamFreeUncertain (reason++";consumed marker not written: "++displayException failure)

runRamFreeWith :: PeerCacheAvailability -> RamAllocState -> Clock -> UsbApi -> FilePath -> IO RamFreeOutcome
runRamFreeWith availability allocation clock api directory
  | availability == PeerCacheUnbound = pure (RamFreeRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
  | Nothing <- payloadOfSize (allocSize allocation) = pure (RamFreeRefused ("retained allocation size "++show (allocSize allocation)++" is no payload's extent"))
  | Just payload <- payloadOfSize (allocSize allocation) = do
      let chunks = (allocSize allocation+247) `div` 248
          cap = ramFreeJournalCapBytes payload
          deadline = transferTimeoutMs*(200 + 4*chunks)
      session <- newSession clock api directory cap deadline
      held <- newHeld
      active <- newIORef False
      opened <- newIORef Nothing
      result <- try $ mask $ \restore -> restore $ do
        live <- openLive session held
        writeIORef opened (Just live)
        (modelName,_) <- identityAndGate live
        require (modelName == allocProduct allocation) "allocation session product identity mismatch"
        slot <- preflightSlot live
        require (slot == allocSlotWord allocation) "callback slot differs from allocation state"
        guard <- allocationDigest live (allocBase allocation) (allocSize allocation)
        require (guard == allocGuard allocation) "retained allocation guard changed; ram-free requires an unmodified block"
        writeIORef active True
        -- Stock free leaves the payload intact; the tag makes this state's guard
        -- fail for any later ram-free or ram-push.
        writeVerified live (allocBase allocation) consumedAllocationTag
        freeReply <- bootstrapCall live freeBootstrapBytes (unAddress (allocBase allocation))
        requireGenericCallbackReply freeReply
        slotAfter <- wordAt <$> readMemory live callbackSlotAddress 4
        require (slotAfter == unWordValue slot) "callback slot changed during allocation free"
        writePrivateBytes (directory </> "ram-free-state.json") (C.pack "{\"schema\":1,\"status\":\"freed\"}\n")
        note session "ram-free-complete" ("base="++hexWord (unAddress (allocBase allocation)))
        writeIORef active False
      isActive <- readIORef active
      liveForCleanup <- if isActive then pure Nothing else readIORef opened
      cleanupFailures <- cleanupHost held api liveForCleanup
      case (result,cleanupFailures) of
        (_, failure:_) -> pure (RamFreeUncertain ("cleanup uncertain: "++failure))
        (Right (),[]) -> pure RamFreed
        (Left (failure::SomeException),[]) -> do
          let message = displayException failure
          _ <- try (recordWithin cap directory "ram-free-stop" (message++";free-started="++show isActive)) :: IO (Either SomeException ())
          pure (if isActive then RamFreeUncertain message else RamFreeRefused message)

runRamUnload :: ReviewedContract -> RamPushState -> Clock -> UsbApi -> FilePath -> IO RamUnloadOutcome
runRamUnload contract state clock api directory = do
  authenticated <- try (authenticateReviewedState contract state)
  case authenticated of
    Left (failure::SomeException) -> pure (RamUnloadRefused (displayException failure))
    Right () -> runRamUnloadWith xm1PeerCacheAvailability state clock api directory

runRamUnloadWith :: PeerCacheAvailability -> RamPushState -> Clock -> UsbApi -> FilePath -> IO RamUnloadOutcome
runRamUnloadWith availability state clock api directory
  | availability == PeerCacheUnbound = pure (RamUnloadRefused "X-M1 peer-core cache binding is unbound; no camera write sent")
  | Nothing <- payloadOfSize (stateSize state) = pure (RamUnloadRefused ("installed state size "++show (stateSize state)++" is no payload's extent"))
  | Just payload <- payloadOfSize (stateSize state) = do
      let count = payloadPublicationCount payload
          cap = ramUnloadJournalCapBytes payload; deadline = transferTimeoutMs*(600 + 2*count)
      session <- newSession clock api directory cap deadline
      held <- newHeld
      active <- newIORef False
      opened <- newIORef Nothing
      result <- try $ mask $ \restore -> restore $ do
        live <- openLive session held
        writeIORef opened (Just live)
        _ <- identityAndGate live
        slot <- preflightForUnload live state
        writeIORef active True
        nonce <- pure ((stateNonce state + 1) .&. 0x00ffffff)
        mailbox <- retargetOutputCall live (stateBase state) (stateSize state) (unAddress (rollbackEntry (stateEntries state))) nonce
        rolledBack <- mailboxStatus payload mailbox nonce 2
        unless rolledBack $ do
          writeIORef active False
          P.reject "installer refused before rollback: CPU1 park rendezvous timed out"
        forM_ (statePublications state) $ \publication -> do
          current <- wordAt <$> readMemory live (publicationAddress publication) 4
          require (current == unWordValue (publicationOld publication)) ("rollback readback mismatch at "++hexWord (unAddress (publicationAddress publication)))
        slotAfter <- wordAt <$> readMemory live callbackSlotAddress 4
        require (slotAfter == unWordValue slot) "callback slot changed during unload"
        note session "ram-unload-complete" (show count++" old words verified and callback slot restored; allocation retained until power-off")
        writePrivateBytes (directory </> "ram-unload-state.json") (C.pack "{\"schema\":1,\"status\":\"rolled-back\",\"allocation\":\"retained-until-power-off\"}\n")
        writeIORef active False
      isActive <- readIORef active
      liveForCleanup <- if isActive then pure Nothing else readIORef opened
      cleanupFailures <- cleanupHost held api liveForCleanup
      case (result,cleanupFailures) of
        (_, failure:_) -> do
          let message = "cleanup uncertain: "++failure
          _ <- try (recordWithin cap directory "ram-unload-stop" (message++";rollback-started="++show isActive)) :: IO (Either SomeException ())
          pure (RamUnloadUncertain message)
        (Right (),[]) -> pure RamUnloaded
        (Left (failure::SomeException),[]) -> do
          let message = displayException failure
          _ <- try (recordWithin cap directory "ram-unload-stop" (message++";rollback-started="++show isActive)) :: IO (Either SomeException ())
          pure (if isActive then RamUnloadUncertain message else RamUnloadRefused message)

preflightForUnload :: Live -> RamPushState -> IO WordValue
preflightForUnload live state = do
  require (stateBootstrap state == bootstrapBytes) "state bootstrap identity mismatch"
  table <- wordAt <$> readMemory live callbackTablePointerAddress 4
  require (table == unAddress callbackTableAddress) "query callback table pointer mismatch"
  slot <- wordAt <$> readMemory live callbackSlotAddress 4
  require (slot == unWordValue (stateSlotWord state)) "query callback slot differs from state"
  forM_ (statePublications state) $ \publication -> do
    current <- wordAt <$> readMemory live (publicationAddress publication) 4
    require (current == unWordValue (publicationNew publication)) ("active publication mismatch at "++hexWord (unAddress (publicationAddress publication)))
  pure (WordValue slot)
