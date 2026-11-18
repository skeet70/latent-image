{-# LANGUAGE OverloadedStrings #-}
module Emulation.PlaybackOwned
  ( Selection, selection, installedDirectory, packageDirectory
  , Package, loadPackage, parsePackage, validateInstalled, validateReady
  , guestArtifacts, checkGuestArtifacts, watchRequest, validateWatchBegin
  , originalBodies, checkOriginalBodies
  , Witness, sourceObserved, modeObserved, witness, witnessValue
  , QualificationScope (..), requiredObservations, qualificationObserved, qualificationScope, qualificationScopeText, ownsEntry
  , QualificationEvidence, qualificationEvidence, preparationWitness, playWitness
  , requiredEpochObservations, qualificationEpochsObserved, qualificationEvidenceValue
  , DeviceOwnership (..), deviceOwnership, deviceServices, releasedObservation
  ) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import Data.Foldable (toList)
import Data.List (sort)
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.WatchObservation as W
import Numeric (readHex, showHex)
import qualified PlaybackContract as C
import System.Directory (canonicalizePath)
import System.FilePath ((</>))

newtype InstalledDirectory = InstalledDirectory FilePath deriving (Eq, Show)
newtype PackageDirectory = PackageDirectory FilePath deriving (Eq, Show)
data Selection = Selection InstalledDirectory PackageDirectory deriving (Eq, Show)

selection :: Bool -> Maybe FilePath -> Maybe FilePath -> Either String (Maybe Selection)
selection _ Nothing Nothing = Right Nothing
selection True (Just installed) (Just package) = Right (Just (Selection (InstalledDirectory installed) (PackageDirectory package)))
selection False (Just _) (Just _) = Left "owned playback requires --route play"
selection _ _ _ = Left "--installed and --owned-playback are required together"

installedDirectory, packageDirectory :: Selection -> FilePath
installedDirectory (Selection (InstalledDirectory path) _) = path
packageDirectory (Selection _ (PackageDirectory path)) = path

newtype Digest = Digest T.Text deriving (Eq, Show)
newtype Address = Address Word32 deriving (Eq, Show)
type Entry = C.PlaybackEntry
data Binding = Binding Entry Address Address Address (Maybe Address)

data SourceIdentity = SourceIdentity Word32 Int B.ByteString Digest
data InstallerStorage = RendezvousStorage | MailboxStorage deriving (Eq, Ord, Show)
data DeviceFamily = RetainedDevice | OwnedDevice [(C.JpegDeviceService, Word32, Word32)]
data Package = Package Address B.ByteString Digest Digest [Binding] [(Address, B.ByteString)] [SourceIdentity] [(InstallerStorage, Address)] DeviceFamily

ensure :: Bool -> String -> Either String ()
ensure condition problem = unless condition (Left problem)

field :: T.Text -> Value -> Either String Value
field key (Object fields) = maybe (Left ("missing " ++ T.unpack key)) Right (KM.lookup (K.fromText key) fields)
field _ _ = Left "object required"

text :: Value -> Either String T.Text
text (String value) = Right value
text _ = Left "text required"

array :: Value -> Either String [Value]
array (Array values) = Right (toList values)
array _ = Left "array required"

address :: Value -> Either String Address
address value = do
  raw <- text value
  case T.unpack raw of
    '0':'x':digits | [(n, "")] <- readHex digits, n <= (0xffffffff :: Integer) -> Right (Address (fromInteger n))
    _ -> Left "32-bit hex address required"

at :: T.Text -> Value -> Either String Address
at key value = field key value >>= address

digest :: B.ByteString -> Digest
digest = Digest . T.pack . concatMap (\w -> let s = showHex w "" in replicate (2 - length s) '0' ++ s) . B.unpack . SHA.hash

digestValue :: Digest -> Value
digestValue (Digest value) = String value

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` n) | n <- [0,8,16,24]]

contract :: Entry -> (Word32, Word32, T.Text, T.Text)
contract entry = let (start, end, C.SourceSha256 hash, name) = C.playbackContract entry
  in (start, end, T.pack name, T.pack hash)

loadPackage :: Selection -> IO (Selection, Package)
loadPackage selected = do
  installed <- canonicalizePath (installedDirectory selected)
  package <- canonicalizePath (packageDirectory selected)
  let resolved = Selection (InstalledDirectory installed) (PackageDirectory package)
  manifest <- B.readFile (package </> "ram-push-manifest.json")
  metadata <- B.readFile (package </> "experiment.json")
  runtime <- B.readFile (package </> "runtime.bin")
  elf <- B.readFile (package </> "kernel.elf")
  parsed <- either (ioError . userError) pure (parsePackage manifest metadata runtime elf)
  pure (resolved, parsed)

parsePackage :: B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String Package
parsePackage manifestBytes metadataBytes runtime elfBytes = do
  manifest <- A.eitherDecodeStrict' manifestBytes
  metadata <- A.eitherDecodeStrict' metadataBytes
  elf <- E.parseElf elfBytes
  ensure (field "schema" manifest == Right (A.toJSON (1 :: Int)) && field "schema" metadata == Right (A.toJSON (1 :: Int))) "package schema differs"
  base@(Address start) <- field "base" manifest >>= address
  ensure (at "base" metadata == Right base) "package bases differ"
  ensure (not (B.null runtime) && toInteger start + toInteger (B.length runtime) <= 0x100000000) "runtime range invalid"
  ensure (field "size" manifest == Right (A.toJSON (B.length runtime))) "runtime size differs"
  let runtimeDigest = digest runtime
  ensure (field "runtime_sha256" manifest == Right (digestValue runtimeDigest) && field "runtime_sha256" metadata == Right (digestValue runtimeDigest)) "runtime digest differs"
  hooks <- field "hooks" metadata >>= array
  entries <- case length hooks of
    2 -> pure C.preparationEntries
    6 -> pure C.transactionEntries
    8 -> pure C.consumerPlaybackEntries
    17 -> pure (C.consumerPlaybackEntries ++ C.lifecycleEntries)
    20 -> pure C.playbackEntries
    _ -> Left "owned playback requires exactly the preparation pair, JPEG transaction, JPEG consumers, JPEG lifecycle or develop consumers"
  bindings <- forM entries $ \entry -> do
    let (site, end, name, sourceDigest) = contract entry
    hook <- case [h | h <- hooks, at "address" h == Right (Address site)] of
      [h] -> Right h
      _ -> Left "missing or duplicate playback hook"
    ensure (field "mode" hook == Right (String "replace_entry") && field "handler" hook == Right (String name)) "hook is not the owned playback replacement"
    source <- field "source" hook
    ensure (at "entry" source == Right (Address site)
      && at "end" source == Right (Address end)
      && field "sha256" source == Right (String sourceDigest)) "pinned source identity differs"
    body <- field "original_body" hook
    ensure (at "start" body == Right (Address (site + 8)) && at "end" body == Right (Address end)) "original body exclusion differs"
    target@(Address targetWord) <- field "trampoline" hook >>= address
    symbol <- case [s | s <- E.globalSymbols elf, E.symbolName s == T.unpack name, E.symbolKind s == E.FunctionSymbol] of
      [s] -> Right s
      _ -> Left "unique owned entry ELF function required"
    ensure (E.symbolValue symbol == targetWord && targetWord `mod` 4 == 0 && E.symbolSize symbol >= 4
      && targetWord >= start && toInteger targetWord + toInteger (E.symbolSize symbol) <= toInteger start + 0x5000) "owned ELF entry lies outside preparation code window"
    ensure (any (\s -> E.sectionExecutable s && not (E.sectionWritable s)
      && targetWord >= E.sectionAddress s && toInteger targetWord + toInteger (E.symbolSize symbol)
        <= toInteger (E.sectionAddress s) + toInteger (E.sectionSize s)) (E.elfSections elf)) "owned entry is not immutable executable code"
    ensure (Just (B.take (fromIntegral (E.symbolSize symbol)) (B.drop (fromIntegral (targetWord - start)) runtime))
      == (B.pack <$> mapM (E.byteAt elf . (targetWord +)) [0 .. E.symbolSize symbol - 1])) "owned entry runtime differs from ELF"
    symbols <- field "symbols" metadata
    ensure (at name symbols == Right target) "metadata symbol differs from ELF"
    released <- case entry of
      C.PlaybackJpegFinishRequest -> observation "xm1_playback_jpeg_finish_released" symbol elf
      C.PlaybackJpegCancelRequest -> observation "xm1_playback_jpeg_cancel_released" symbol elf
      _ -> pure Nothing
    pure (Binding entry (Address site) (Address end) target released)
  publications <- field "publications" manifest >>= array
  let historicalPadding = length hooks `elem` [2,6] && length publications == 16
        && case field "experiment_layout" manifest of Left _ -> True; Right _ -> False
  ensure (length publications == 2 * length hooks || historicalPadding) "publication count differs from actual hooks"
  artifacts <- forM publications $ \publication -> do
    location <- field "address" publication >>= address
    Address value <- field "new" publication >>= address
    pure (location, word value)
  ensure (length (sort (map (\(Address a, _) -> a) artifacts)) == length (unique (map (\(Address a, _) -> a) artifacts))) "duplicate publication address"
  forM_ bindings $ \(Binding _ (Address site) _ (Address target) _) -> do
    ensure (lookup (Address site) artifacts == Just (word 0xe51ff004) && lookup (Address (site + 4)) artifacts == Just (word target)) "publication does not bind owned entry"
    ensure (any (\p -> at "address" p == Right (Address site) && field "phase" p == Right (String "activation")) publications
      && any (\p -> at "address" p == Right (Address (site + 4)) && field "phase" p == Right (String "setup")) publications) "publication phases differ"
  case field "experiment_layout" manifest of
    Left _ -> ensure (length hooks < 17) "full JPEG lifecycle requires linked installer layout"
    Right layout -> do
      Address tableOffset <- at "table_offset" layout
      Address installerEnd <- at "installer_end_offset" layout
      let tableBytes = toInteger (length publications) * 16
          tableEnd = toInteger tableOffset + tableBytes
          linked :: String -> Either String Word32
          linked name = case [E.symbolValue s | s <- E.globalSymbols elf, E.symbolName s == name] of
            [value] -> Right value
            _ -> Left "unique installer layout ELF symbol required"
      ensure (tableOffset `mod` 4 == 0 && tableOffset >= 0x5000
        && tableEnd <= toInteger installerEnd && toInteger installerEnd <= toInteger (B.length runtime))
        "publication table lies outside linked installer allocation"
      forM_ [("__ram_push_publications_start",tableOffset)
        ,("__ram_push_publications_end",fromInteger tableEnd)
        ,("__experiment_installer_end",installerEnd)] $ \(name,offset) ->
          ensure (linked name == Right (start + offset)) "installer layout differs from ELF"
      encoded <- B.concat <$> forM publications (\publication -> do
        Address site <- at "address" publication
        Address old <- at "old" publication
        Address new <- at "new" publication
        phase <- field "phase" publication
        phaseWord <- case phase of
          String "setup" -> pure 0
          String "activation" -> pure 1
          _ -> Left "publication phase differs"
        pure (B.concat (map word [site,old,new,phaseWord])))
      ensure (B.take (B.length encoded) (B.drop (fromIntegral tableOffset) runtime) == encoded)
        "runtime publication vector differs from manifest"
  sources <- forM bindings $ \(Binding entry (Address site) (Address end) _ _) -> do
    header <- B.concat <$> forM [site, site + 4] (\location -> case [p | p <- publications, at "address" p == Right (Address location)] of
      [p] -> do
        Address old <- field "old" p >>= address
        pure (word old)
      _ -> Left "source publication word missing")
    let (_, _, _, hash) = contract entry
    pure (SourceIdentity (site + 8) (fromIntegral (end - site - 8)) header (Digest hash))
  let internalSources =
        [ SourceIdentity internalStart (fromIntegral (internalEnd - internalStart)) B.empty (Digest (T.pack hash))
        | C.PlaybackJpegAcquire `elem` entries
        , C.PinnedBody internalStart internalEnd (C.SourceSha256 hash) <- C.jpegInternalBodies ]
  let deviceSymbols service =
        let (_, name) = C.jpegDeviceContract service
        in [s | s <- E.globalSymbols elf, E.symbolName s == name]
      deviceDefinitions service = [s | s <- deviceSymbols service, E.InSection _ <- [E.symbolSection s]]
      present = [service | service <- C.jpegDeviceServices, not (null (deviceDefinitions service))]
  ensure (null present || present == C.jpegDeviceServices) "owned JPEG device exports are incomplete"
  forM_ C.jpegDeviceServices $ \service -> do
    let (body, _) = C.jpegDeviceContract service
    case deviceSymbols service of
      [] -> ensure (null present) "owned JPEG device symbol missing"
      [symbol] -> ensure (E.symbolKind symbol == E.FunctionSymbol && case E.symbolSection symbol of
        E.InSection _ -> not (null present)
        E.Absolute -> null present && E.symbolValue symbol == C.bodyStart body
        E.Undefined -> False) "JPEG device symbol differs from selected source/owned family"
      _ -> Left "duplicate JPEG device export"
  family <- if null present then pure RetainedDevice else do
    ensure (C.PlaybackJpegAcquire `elem` entries) "owned JPEG device requires the composed transaction"
    definitions <- forM C.jpegDeviceServices $ \service -> do
      symbol <- case deviceDefinitions service of
        [s] | E.symbolKind s == E.FunctionSymbol -> pure s
        _ -> Left "unique owned JPEG device ELF function required"
      let low = E.symbolValue symbol
          high = toInteger low + toInteger (E.symbolSize symbol)
      ensure (low `mod` 4 == 0 && E.symbolSize symbol >= 4 && low >= start && high <= toInteger start + 0x5000)
        "owned JPEG device definition lies outside immutable code window"
      ensure (any (\s -> E.sectionExecutable s && not (E.sectionWritable s)
        && low >= E.sectionAddress s && high <= toInteger (E.sectionAddress s) + toInteger (E.sectionSize s))
        (E.elfSections elf)) "owned JPEG device definition is not immutable executable code"
      ensure (Just (B.take (fromIntegral (E.symbolSize symbol)) (B.drop (fromIntegral (low - start)) runtime))
        == (B.pack <$> mapM (E.byteAt elf . (low +)) [0 .. E.symbolSize symbol - 1]))
        "owned JPEG device runtime differs from ELF"
      pure (service, low, fromInteger high)
    let spans = sort [(lo,hi) | (_,lo,hi) <- definitions]
    ensure (and [hi <= next | ((_,hi),(next,_)) <- zip spans (drop 1 spans)]) "owned JPEG device definitions overlap"
    pure (OwnedDevice definitions)
  let deviceSources = case family of
        RetainedDevice -> []
        OwnedDevice _ ->
          [ SourceIdentity low (fromIntegral (high-low)) B.empty (Digest (T.pack hash))
          | service <- C.jpegDeviceServices
          , let (C.PinnedBody low high (C.SourceSha256 hash), _) = C.jpegDeviceContract service ]
  let allocated = [s | s <- E.elfSections elf, E.sectionAllocated s, E.sectionSize s > 0]
  forM_ allocated $ \section -> do
    let low = toInteger (E.sectionAddress section)
        high = low + toInteger (E.sectionSize section)
    ensure (toInteger start <= low && high <= toInteger start + toInteger (B.length runtime)) "allocated ELF section lies outside runtime"
    ensure (not (E.sectionWritable section) || low >= toInteger start + 0x5000) "owned preparation code window contains writable state"
  let spans = sort [(toInteger (E.sectionAddress s), toInteger (E.sectionAddress s) + toInteger (E.sectionSize s)) | s <- allocated]
  ensure (and [end <= next | ((_,end),(next,_)) <- zip spans (drop 1 spans)]) "allocated ELF sections overlap"
  mutable <- fmap concat $ forM allocated $ \section -> case E.sectionName section of
    ".ram_push.rendezvous" -> storage RendezvousStorage section
    ".ram_push.mailbox" -> storage MailboxStorage section
    _ -> pure []
  ensure (null mutable || sort (map fst mutable) == [RendezvousStorage, MailboxStorage]) "installer scalar storage pair is incomplete"
  pure (Package base runtime (digest manifestBytes) runtimeDigest bindings artifacts (sources ++ internalSources ++ deviceSources) mutable family)
  where
    observation name owner elf = case [s | s <- E.globalSymbols elf, E.symbolName s == name] of
      [symbol] -> do
        ensure (E.symbolKind symbol == E.OtherSymbol && notype symbol elf
          && E.symbolSection symbol == E.symbolSection owner
          && E.symbolValue symbol `mod` 4 == 0
          && E.symbolValue symbol > E.symbolValue owner
          && toInteger (E.symbolValue symbol) + 4 <= toInteger (E.symbolValue owner) + toInteger (E.symbolSize owner))
          "release observation is not inside its immutable owned function"
        pure (Just (Address (E.symbolValue symbol)))
      _ -> Left "unique owned release observation ELF symbol required"
    notype symbol elf = case
      [ B.index (E.elfBytes elf) (fromIntegral (E.sectionOffset table) + index * 16 + 12)
      | table <- E.elfSections elf, E.sectionType table == 2
      , (index, candidate) <- zip [0..] (E.elfSymbols elf), candidate == symbol ] of
        [info] -> info .&. 15 == 0
        _ -> False
    storage kind section = do
      ensure (E.sectionWritable section && not (E.sectionExecutable section) && E.sectionSize section == 128) "installer scalar storage layout differs"
      pure [(kind, Address (E.sectionAddress section))]
    unique [] = []
    unique (x:xs) = x : unique (filter (/= x) xs)

validateInstalled :: Selection -> Package -> B.ByteString -> Value -> Either String Value
validateInstalled selected (Package base _ manifest runtime _ _ _ _ _) recordBytes options = do
  record <- A.eitherDecodeStrict' recordBytes
  listed <- field "installed" options >>= array
  candidate <- case [v | v <- listed, field "directory" v == Right (A.toJSON (installedDirectory selected))] of
    [v] -> Right v
    _ -> Left "selected installed directory is not uniquely listed"
  ensure (field "schema" record == Right (A.toJSON (1 :: Int)) && field "key" record == field "boot_key" options) "installed record boot identity differs"
  forM_ [record, candidate] $ \value -> do
    ensure (at "base" value == Right base) "installed base differs"
    ensure (field "manifest_sha256" value == Right (digestValue manifest) && field "runtime_sha256" value == Right (digestValue runtime)) "installed package identity differs"
  ensure (field "record_sha256" candidate == Right (digestValue (digest recordBytes))) "installed record digest differs"
  ensure (field "host" candidate == field "host" record) "installed host identity differs"
  snapshot <- field "snapshot_sha256" candidate >>= text
  ensure (T.length snapshot == 64 && T.all (`elem` ("0123456789abcdef" :: String)) snapshot) "installed snapshot digest missing"
  pure (object ["command" .= ("start" :: T.Text), "installed" .= installedDirectory selected, "expected_snapshot_sha256" .= snapshot])

validateReady :: Selection -> Value -> Value -> Either String ()
validateReady selected start ready = do
  checkpoint <- field "checkpoint" ready
  ensure (field "start" checkpoint == Right (String "installed") && field "installed" checkpoint == Right (A.toJSON (installedDirectory selected))) "bridge did not restore selected install"
  expected <- field "expected_snapshot_sha256" start
  ensure (field "snapshot_sha256" checkpoint == Right expected) "restored snapshot differs from selected installed identity"

guestArtifacts :: Package -> [(Word32, B.ByteString)]
guestArtifacts (Package (Address base) runtime _ _ _ publications _ _ _) = (base,runtime) : [(a,b) | (Address a,b) <- publications]

checkGuestArtifacts :: Package -> [(Word32, B.ByteString)] -> Either String ()
checkGuestArtifacts package@(Package (Address base) runtime _ _ _ artifacts _ mutable _) actual = do
  ensure (map (\(a,b) -> (a,B.length b)) actual == map (\(a,b) -> (a,B.length b)) (guestArtifacts package)) "guest artifact read layout differs"
  case actual of
    (_,observed) : publications -> do
      ensure (publications == drop 1 (guestArtifacts package)) "guest publication bytes differ from identified package"
      let fields = sort [(fromIntegral (storageAddress - base) + offset, count) | (kind,Address storageAddress) <- mutable, (offset,count) <- scalarFields kind]
          stable start [] = B.drop start observed == B.drop start runtime
          stable start ((offset,count):rest) = B.take (offset-start) (B.drop start observed) == B.take (offset-start) (B.drop start runtime) && stable (offset+count) rest
          bytes kind = case [B.take 128 (B.drop (fromIntegral (storageAddress-base)) observed) | (found,Address storageAddress) <- mutable, kind == found] of
            [value] -> value
            _ -> B.empty
      ensure (stable 0 fields) "guest immutable runtime bytes differ from identified package"
      unless (null mutable) (checkInstallerTerminal (length artifacts) (bytes RendezvousStorage) (bytes MailboxStorage))
    [] -> Left "guest runtime read missing"

scalarFields :: InstallerStorage -> [(Int,Int)]
scalarFields RendezvousStorage = [(0,16),(32,4),(64,12),(96,16)]
scalarFields MailboxStorage = [(0,100)]

-- The completed nonce is the peer completion callback's final store.
checkInstallerTerminal :: Int -> B.ByteString -> B.ByteString -> Either String ()
checkInstallerTerminal publications rendezvous mailbox = do
  let scalarWord bytes offset = fromIntegral (B.index bytes offset) + 256 * fromIntegral (B.index bytes (offset+1))
        + 65536 * fromIntegral (B.index bytes (offset+2)) + 16777216 * fromIntegral (B.index bytes (offset+3)) :: Word32
      m = scalarWord mailbox
      r = scalarWord rendezvous
      nonce = m 8
      peer = m 44
      count = fromIntegral publications
      hooks = count `div` 2
  ensure (B.length rendezvous == 128 && B.length mailbox == 128) "installer scalar report length differs"
  ensure (m 0 == 0x31504d52 && m 4 == 7 && nonce /= 0 && m 12 == 1 && m 16 == 0
    && m 20 == count && m 24 == hooks && m 28 == hooks && m 32 == 0 && m 36 == 0x4003
    && m 40 == nonce && peer /= m 36 && m 48 == 0 && m 52 == count && m 56 == 0x80000
    && m 64 == nonce && m 68 == nonce && m 72 == nonce) "installer mailbox is not a completed successful installation"
  ensure (r 0 == 4 && r 4 == nonce && r 8 == 0 && r 12 == m 36
    && r 32 == 3 && r 64 == nonce && r 96 == nonce && r 100 == peer && r 104 == 0 && r 108 == count)
    "installer rendezvous has not completed its declared peer callback"

originalBodies :: Package -> [(Word32, Int)]
originalBodies (Package _ _ _ _ _ _ sources _ _) = [(start, count) | SourceIdentity start count _ _ <- sources]

checkOriginalBodies :: Package -> [B.ByteString] -> Either String ()
checkOriginalBodies (Package _ _ _ _ _ _ sources _ _) bodies = do
  ensure (length sources == length bodies) "original playback body reads missing"
  forM_ (zip sources bodies) $ \(SourceIdentity _ count header expected, bytes) ->
    ensure (B.length bytes == count && digest (header <> bytes) == expected) "original playback source bytes differ from pinned source identity"

ranges :: Package -> ([W.WatchRange], [W.WatchRange])
ranges (Package (Address base) runtime _ _ bindings publications sources _ _) = (W.unionWatchRanges (publicationRanges ++ bodies ++ [range base (base + fromIntegral (B.length runtime))]), bodies ++ entries)
  where
    range lo hi = either error id (W.watchRange (W.watchAddress lo) (W.watchAddress hi))
    publicationRanges = W.unionWatchRanges [range a (a + 4) | (Address a,_) <- publications]
    bodies = [range start (start + fromIntegral count) | SourceIdentity start count _ _ <- sources]
    entries = [range target (target + 4) | Binding _ _ _ (Address target) _ <- bindings]

limit :: W.WatchLimit
limit = either error id (W.watchLimit 65536)

watchRequest :: Package -> Value
watchRequest package = object
  ["command" .= ("watch-begin" :: T.Text), "views" .= ("fixed" :: T.Text), "limit" .= W.watchLimitValue limit
  , "ranges" .= bounds (W.unionWatchRanges (protected ++ fetches)), "write_ranges" .= bounds protected, "fetch_ranges" .= bounds fetches]
  where
    (protected, fetches) = ranges package
    bounds = map (\r -> let (a,b) = W.watchRangeBounds r in [a,b])

validateWatchBegin :: Package -> Value -> Either String ()
validateWatchBegin package value = do
  ack <- W.parseFixedWatchAck value
  let state = case value of
        Object fields -> Object (KM.insert "enabled" (Bool True) (KM.insert "records" (A.toJSON ([] :: [Value])) (KM.insert "violation" Null (KM.delete "armed" fields))))
        _ -> Null
      (protected, fetches) = ranges package
      rangeLimit = either error id (W.watchRangeLimit 256)
  observation <- W.parseFixedWatchObservation limit state
  _ <- W.judgeExecutionWrites (W.checkExecutionWritesWithRangeLimit rangeLimit W.ForbidProtectedWrites limit protected fetches ack observation)
  pure ()

newtype Witness = Witness [(Entry, Bool)] deriving (Eq, Show)

entryObserved :: Entry -> Witness -> Bool
entryObserved entry (Witness entries) = lookup entry entries == Just True

sourceObserved, modeObserved :: Witness -> Bool
sourceObserved = entryObserved C.PlaybackSource
modeObserved = entryObserved C.PlaybackMode

hasJpeg :: Package -> Bool
hasJpeg package = ownsEntry package C.PlaybackJpegAcquire

ownsEntry :: Package -> Entry -> Bool
ownsEntry (Package _ _ _ _ bindings _ _ _ _) entry = any (\(Binding bound _ _ _ _) -> bound == entry) bindings

requiredObservations :: Package -> [(T.Text, Witness -> Bool)]
requiredObservations package
  | qualificationScope package `elem` [JpegConsumers, JpegLifecycle] =
      [ ("owned-playback-jpeg-complete-first", entryObserved C.PlaybackJpegCompleteFirst) ]
      ++ [("owned-playback-jpeg-cancel-request", entryObserved C.PlaybackJpegCancelRequest)
         | qualificationScope package == JpegLifecycle]
      ++ transactionObservations
  | hasJpeg package = transactionObservations
  | otherwise = [("owned-playback-source-entry", sourceObserved), ("owned-playback-mode-entry", modeObserved)]
  where
    transactionObservations =
      [ ("owned-playback-jpeg-acquire", entryObserved C.PlaybackJpegAcquire)
      , ("owned-playback-jpeg-interrupt", entryObserved C.PlaybackJpegInterrupt)
      , ("owned-playback-jpeg-release", entryObserved C.PlaybackJpegRelease) ]

qualificationObserved :: Package -> Witness -> Bool
qualificationObserved package value = qualificationScope package `notElem` [JpegConsumers, JpegLifecycle]
  && all (\(_, predicate) -> predicate value) (requiredObservations package)

data QualificationScope = PreparationPair | JpegTransaction | JpegConsumers | JpegLifecycle deriving (Eq, Show)

qualificationScope :: Package -> QualificationScope
qualificationScope (Package _ _ _ _ bindings _ _ _ _)
  | any (\(Binding entry _ _ _ _) -> entry == C.PlaybackJpegCancelRequest) bindings = JpegLifecycle
  | any (\(Binding entry _ _ _ _) -> entry == C.PlaybackJpegBuildRequest) bindings = JpegConsumers
  | any (\(Binding entry _ _ _ _) -> entry == C.PlaybackJpegAcquire) bindings = JpegTransaction
  | otherwise = PreparationPair

qualificationScopeText :: QualificationScope -> T.Text
qualificationScopeText PreparationPair = "preparation-pair"
qualificationScopeText JpegTransaction = "jpeg-transaction"
qualificationScopeText JpegConsumers = "jpeg-consumers"
qualificationScopeText JpegLifecycle = "jpeg-lifecycle"

data QualificationEvidence = QualificationEvidence Witness Witness deriving (Eq, Show)

qualificationEvidence :: Witness -> Witness -> QualificationEvidence
qualificationEvidence = QualificationEvidence

preparationWitness, playWitness :: QualificationEvidence -> Witness
preparationWitness (QualificationEvidence prefix _) = prefix
playWitness (QualificationEvidence _ play) = play

requiredEpochObservations :: Package -> [(T.Text, QualificationEvidence -> Bool)]
requiredEpochObservations package =
  [ (name, predicate . playWitness) | (name, predicate) <- requiredObservations package ]
  ++ [ ("owned-playback-preparation-jpeg-build-request", entryObserved C.PlaybackJpegBuildRequest . preparationWitness)
     | qualificationScope package `elem` [JpegConsumers, JpegLifecycle] ]
  ++ [ (name, entryObserved entry . preparationWitness)
     | (name, entry) <- [ ("owned-playback-preparation-jpeg-develop-job4", C.PlaybackJpegDevelopJob4)
                        , ("owned-playback-preparation-jpeg-record-notification", C.PlaybackJpegRecordNotification)
                        , ("owned-playback-preparation-jpeg-clear-record-release", C.PlaybackJpegClearRecordRelease) ]
     , ownsEntry package entry ]

qualificationEpochsObserved :: Package -> QualificationEvidence -> Bool
qualificationEpochsObserved package value = all (\(_, predicate) -> predicate value) (requiredEpochObservations package)

qualificationEvidenceValue :: QualificationEvidence -> Value
qualificationEvidenceValue value = object
  [ "preparation" .= witnessValue (preparationWitness value)
  , "play" .= witnessValue (playWitness value) ]

data DeviceOwnership = RetainedJpegDevice | OwnedJpegDevice deriving (Eq, Show)

deviceOwnership :: Package -> DeviceOwnership
deviceOwnership (Package _ _ _ _ _ _ _ _ RetainedDevice) = RetainedJpegDevice
deviceOwnership _ = OwnedJpegDevice

deviceServices :: Maybe Package -> [(C.JpegDeviceService, Word32, Word32)]
deviceServices (Just (Package _ _ _ _ _ _ _ _ (OwnedDevice definitions))) = definitions
deviceServices _ = [(service, C.bodyStart body, C.bodyEnd body)
  | service <- C.jpegDeviceServices, let (body, _) = C.jpegDeviceContract service]

releasedObservation :: Package -> C.PlaybackEntry -> Maybe Word32
releasedObservation (Package _ _ _ _ bindings _ _ _ _) entry = case
  [site | Binding kind _ _ _ (Just (Address site)) <- bindings, kind == entry] of
    [site] -> Just site
    _ -> Nothing

witness :: Package -> Value -> Value -> Value -> Either String Witness
witness package ackValue stateValue endValue = do
  ack <- W.parseFixedWatchAck ackValue
  state <- W.parseFixedWatchObservation limit stateValue
  ended <- W.parseFixedWatchObservation limit endValue
  let (protected, fetches) = ranges package
  let rangeLimit = either error id (W.watchRangeLimit 256)
  _ <- W.judgeExecutionWrites (W.checkExecutionWritesWithRangeLimit rangeLimit W.ForbidProtectedWrites limit protected fetches ack state)
  _ <- W.judgeExecutionWrites (W.checkExecutionWritesWithRangeLimit rangeLimit W.ForbidProtectedWrites limit protected fetches ack ended)
  ensure (W.fixedWatchRecords state == W.fixedWatchRecords ended) "watch changed between state and end"
  let records = W.fixedWatchRecords ended
      body physical bytes = any (\(lo,count) -> toInteger physical < toInteger lo + toInteger count
        && toInteger lo < toInteger physical + toInteger bytes) (originalBodies package)
  ensure (not (any (\r -> case r of
    W.CpuRecord {W.cpuAccess = W.CpuFetch, W.cpuPhysical = physical, W.cpuBytes = bytes} -> body (W.watchAddressValue physical) (W.watchBytesValue bytes)
    _ -> False) records)) "original playback body fetched"
  let Package _ _ _ _ bindings _ _ _ _ = package
      seen entry = any (\r -> case r of
        W.CpuRecord {W.cpuAccess = W.CpuFetch, W.cpuPhysical = physical} -> any (\(Binding kind _ _ (Address target) _) -> kind == entry && W.watchAddressValue physical == target) bindings
        _ -> False) records
  pure (Witness [(entry, seen entry) | Binding entry _ _ _ _ <- bindings])

witnessValue :: Witness -> Value
witnessValue value@(Witness entries) = object $
  ["source_entry_observed" .= sourceObserved value, "mode_entry_observed" .= modeObserved value
  , "scope" .= (if C.PlaybackJpegAcquire `elem` map fst entries
      then "installed bounded owned channel9 transaction; not pixel fidelity, whole playback acceptance or retirement/unload"
      else "installed bounded owned preparation entries; not whole playback acceptance or retirement/unload" :: T.Text)]
  ++ ["jpeg_transaction" .= object
      [ "acquire_observed" .= entryObserved C.PlaybackJpegAcquire value
      , "interrupt_observed" .= entryObserved C.PlaybackJpegInterrupt value
      , "release_observed" .= entryObserved C.PlaybackJpegRelease value
      , "restart_observed" .= entryObserved C.PlaybackJpegRestart value
      , "restart_in_actual_play_claim" .= False ]
      | C.PlaybackJpegAcquire `elem` map fst entries]
  ++ ["jpeg_consumers" .= object
      [ "build_request_observed" .= entryObserved C.PlaybackJpegBuildRequest value
      , "complete_first_observed" .= entryObserved C.PlaybackJpegCompleteFirst value ]
      | C.PlaybackJpegBuildRequest `elem` map fst entries]
  ++ ["jpeg_lifecycle" .= object
      [ "initialize_observed" .= entryObserved C.PlaybackJpegInitialize value
      , "complete_second_observed" .= entryObserved C.PlaybackJpegCompleteSecond value
      , "reset_state_observed" .= entryObserved C.PlaybackJpegResetState value
      , "restart_request_observed" .= entryObserved C.PlaybackJpegRestartRequest value
      , "cancel_timer_observed" .= entryObserved C.PlaybackJpegCancelTimer value
      , "preserve_prefix_observed" .= entryObserved C.PlaybackJpegPreservePrefix value
      , "finish_request_observed" .= entryObserved C.PlaybackJpegFinishRequest value
      , "cancel_request_observed" .= entryObserved C.PlaybackJpegCancelRequest value
      , "submit_observed" .= entryObserved C.PlaybackJpegSubmit value ]
      | C.PlaybackJpegCancelRequest `elem` map fst entries]
  ++ ["jpeg_develop" .= object
      [ "develop_job4_observed" .= entryObserved C.PlaybackJpegDevelopJob4 value
      , "record_notification_observed" .= entryObserved C.PlaybackJpegRecordNotification value
      , "clear_record_release_observed" .= entryObserved C.PlaybackJpegClearRecordRelease value ]
      | C.PlaybackJpegDevelopJob4 `elem` map fst entries]
