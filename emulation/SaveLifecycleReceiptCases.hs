{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON, Value (..), object, (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Aeson.Types as JT
import Data.Bits (shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy.Char8 as BL
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word8, Word32)
import qualified Emulation.SaveLifecycle as L
import FujiAF.JSON (decodeStrictIntegers)
import System.Environment (getArgs)
import System.IO (IOMode (ReadMode), withBinaryFile)

newtype Address = Address Word32 deriving (Eq, Show)
newtype RecordIndex = RecordIndex Int deriving (Eq, Show)
newtype HitIndex = HitIndex Int deriving (Eq, Show)
newtype SnapshotIndex = SnapshotIndex Int deriving (Eq, Show)
data Location = JournalHit RecordIndex HitIndex | StateSnapshot SnapshotIndex | JournalBegin RecordIndex deriving (Eq, Show)
data Register = R0 | R1 | R2 deriving (Eq, Show)
data Mutation
  = RegisterWord Register Word32 | RamWord Address Word32 | AtcmWord Word32
  | RemoveBeginScope | ChangeBeginProgramKind
  deriving (Eq, Show)
data CaseKind
  = CatalogueCount | SelectedCount | Namespace | FramePointer | SampledOpcode | TcmOverlap | CallbackTask
  | MissingBeginScope | WrongBeginScope | ChangedSavePublication | ChangedPermitAPublication | ChangedPermitBPublication
  deriving (Eq, Show)
data RefusalCase = RefusalCase CaseKind L.RefusalCategory Location Mutation
data CallerPublication = SavePublication | PermitAPublication | PermitBPublication deriving (Eq, Show)
data ProgramKind = RetainedKind | OwnedKind

typed :: FromJSON a => Value -> Either String a
typed = JT.parseEither J.parseJSON
get :: FromJSON a => K.Key -> Value -> Either String a
get key (Object row) = maybe (Left ("missing field " ++ K.toString key)) typed (KM.lookup key row)
get _ _ = Left "expected object"
set :: K.Key -> Value -> Value -> Either String Value
set key replacement (Object row) = do
  unless (KM.member key row) (Left "mutation field is absent")
  pure (Object (KM.insert key replacement row))
set _ _ _ = Left "mutation target is not an object"
remove :: K.Key -> Value -> Either String Value
remove key (Object row) = do
  unless (KM.member key row) (Left "mutation field is absent")
  pure (Object (KM.delete key row))
remove _ _ = Left "mutation target is not an object"
changeElement :: Int -> (Value -> Either String Value) -> Value -> Either String Value
changeElement index change (Array values) = do
  old <- maybe (Left "mutation element is absent") pure (values V.!? index)
  changed <- change old
  pure (Array (values V.// [(index,changed)]))
changeElement _ _ _ = Left "mutation target is not an array"

hexBytes :: T.Text -> Either String B.ByteString
hexBytes text = do
  unless (even (T.length text) && T.all (`elem` ("0123456789abcdef" :: String)) text) (Left "invalid RAM bytes")
  pure (B.pack (go (T.unpack text)))
  where
    digit c = fromIntegral (if c <= '9' then fromEnum c-fromEnum '0' else fromEnum c-fromEnum 'a'+10) :: Word8
    go (a:b:rest) = (16*digit a+digit b):go rest
    go _ = []
encodeHex :: B.ByteString -> T.Text
encodeHex = T.pack . concatMap byte . B.unpack
  where
    alphabet = "0123456789abcdef"
    byte value = [alphabet !! fromIntegral (value `shiftR` 4),alphabet !! fromIntegral (value .&. 15)]
physical :: Address -> Either String Address
physical (Address address) = do
  let alias = address .&. 0xc0000000
      offset = address-alias
  unless (alias `elem` [0,0x40000000] && offset < 0x20000000) (Left "mutation address has unsupported alias")
  pure (Address offset)
readRamWord :: Value -> Address -> Either String Word32
readRamWord sample guest = do
  Address start <- physical guest
  spans <- get "ram" sample
  bytes <- forM [0..3] $ \offset -> do
    let address = start+offset
    candidates <- forM (spans :: [Value]) $ \span' -> do
      low <- get "address" span'
      text <- get "bytes" span'
      raw <- hexBytes text
      pure [B.index raw (fromIntegral (address-low)) | address >= low && toInteger address < toInteger low+toInteger (B.length raw)]
    case concat candidates of
      byte:_ -> pure byte
      [] -> Left "mutation word was not genuinely observed"
  pure (foldr (\byte value -> (value `shiftL` 8) .|. fromIntegral byte) 0 bytes)
changeRamWord :: Address -> Word32 -> Value -> Either String Value
changeRamWord guest replacement sample = do
  _ <- readRamWord sample guest
  Address start <- physical guest
  spans <- get "ram" sample
  changed <- forM (spans :: [Value]) $ \span' -> do
    Address low <- Address <$> get "address" span'
    text <- get "bytes" span'
    bytes <- hexBytes text
    let replace index original =
          let address = toInteger low+toInteger index
          in if address >= toInteger start && address < toInteger start+4
             then fromIntegral (replacement `shiftR` (fromInteger (address-toInteger start)*8))
             else original
        raw = B.pack (zipWith replace [0 :: Int ..] (B.unpack bytes))
    set "bytes" (J.toJSON (encodeHex raw)) span'
  set "ram" (J.toJSON changed) sample

registerIndex :: Register -> Int
registerIndex R0 = 0
registerIndex R1 = 1
registerIndex R2 = 2
mutateSample :: Mutation -> Value -> Either String Value
mutateSample (RegisterWord register replacement) sample = do
  registers <- get "registers" sample
  changed <- changeElement (registerIndex register) (const (pure (J.toJSON replacement))) registers
  set "registers" changed sample
mutateSample (RamWord address replacement) sample = changeRamWord address replacement sample
mutateSample (AtcmWord replacement) sample = do
  lifecycle <- get "lifecycle" sample
  banks <- get "tcm" lifecycle
  changed <- changeElement 0 (const (pure (J.toJSON replacement))) banks
  next <- set "tcm" changed lifecycle
  set "lifecycle" next sample
mutateSample RemoveBeginScope begin = remove "scope" begin
mutateSample ChangeBeginProgramKind begin = do
  scope <- get "scope" begin
  program <- get "program" scope
  rawKind <- get "kind" program
  kind <- case (rawKind :: T.Text) of
    "retained" -> pure RetainedKind
    "owned" -> pure OwnedKind
    _ -> Left "genuine begin has an unsupported program kind"
  changedProgram <- set "kind" (String (case kind of RetainedKind -> "owned"; OwnedKind -> "retained")) program
  changedScope <- set "program" changedProgram scope
  set "scope" changedScope begin

atLocation :: Location -> [Value] -> [Value] -> Either String Value
atLocation (StateSnapshot (SnapshotIndex index)) _ snapshots =
  maybe (Left "snapshot mutation index is absent") pure (V.fromList snapshots V.!? index)
atLocation (JournalHit (RecordIndex recordIndex) (HitIndex hitIndex)) records _ = do
  record <- maybe (Left "journal mutation index is absent") pure (V.fromList records V.!? recordIndex)
  value <- get "value" record
  hits <- get "hits" value
  maybe (Left "journal hit mutation index is absent") pure (V.fromList (hits :: [Value]) V.!? hitIndex)
atLocation (JournalBegin (RecordIndex index)) records _ = do
  record <- maybe (Left "journal begin mutation index is absent") pure (V.fromList records V.!? index)
  get "value" record
applyCase :: RefusalCase -> [Value] -> [Value] -> Either String ([Value],[Value])
applyCase (RefusalCase _ _ (StateSnapshot (SnapshotIndex index)) mutation) records snapshots = do
  changed <- changeElement index (mutateSample mutation) (J.toJSON snapshots)
  next <- typed changed
  pure (records,next)
applyCase (RefusalCase _ _ (JournalHit (RecordIndex recordIndex) (HitIndex hitIndex)) mutation) records snapshots = do
  changed <- changeElement recordIndex (\record -> do
    value <- get "value" record
    hits <- get "hits" value
    nextHits <- changeElement hitIndex (mutateSample mutation) hits
    nextValue <- set "hits" nextHits value
    set "value" nextValue record) (J.toJSON records)
  next <- typed changed
  pure (next,snapshots)
applyCase (RefusalCase _ _ (JournalBegin (RecordIndex index)) mutation) records snapshots = do
  changed <- changeElement index (\record -> do
    value <- get "value" record
    nextValue <- mutateSample mutation value
    set "value" nextValue record) (J.toJSON records)
  next <- typed changed
  pure (next,snapshots)

observedHits :: [Value] -> Either String [(Location,Value)]
observedHits records = concat <$> forM (zip [0..] records) (\(index,record) -> do
  label <- get "label" record
  if label /= ("capture-observation-page" :: T.Text) then pure [] else do
    value <- get "value" record
    hits <- get "hits" value
    selected <- forM (zip [0..] (hits :: [Value])) $ \(hit,sample) -> do
      site <- get "site" sample
      pure [(JournalHit (RecordIndex index) (HitIndex hit),sample) | "save-lifecycle-" `T.isPrefixOf` site]
    pure (concat selected))

makeCases :: [Value] -> [Value] -> Either String ([RefusalCase],[CaseKind])
makeCases records snapshots = do
  hits <- observedHits records
  beginCandidates <- forM (zip [0..] records) $ \(index,record) -> do
    label <- get "label" record
    pure [RecordIndex index | label == ("capture-observation-begin" :: T.Text)]
  beginIndex <- case concat beginCandidates of
    [index] -> pure index
    _ -> Left "genuine baseline requires one accepted begin"
  let beginCases =
        [ RefusalCase MissingBeginScope L.WireMismatch (JournalBegin beginIndex) RemoveBeginScope
        , RefusalCase WrongBeginScope L.ProgramMismatch (JournalBegin beginIndex) ChangeBeginProgramKind
        ]
  let firstSnapshot = StateSnapshot (SnapshotIndex 0)
  snapshot <- atLocation firstSnapshot records snapshots
  oldCount <- readRamWord snapshot (Address 0xe8a33c)
  let catalogue = RefusalCase CatalogueCount L.CatalogueMismatch firstSnapshot (RamWord (Address 0xe8a33c) (oldCount `xor` 1))
      overlap = RefusalCase TcmOverlap L.MappingMismatch firstSnapshot (AtcmWord 0x00e8a00d)
  tagged <- forM hits $ \(location,sample) -> do
    pc <- get "address" sample
    pure (location,Address pc,sample)
  opcode <- case tagged of
    (location,address,sample):_ -> do
      old <- readRamWord sample address
      pure [RefusalCase SampledOpcode L.OpcodeMismatch location (RamWord address (old `xor` 1))]
    [] -> pure []
  allocation <- case [(location,sample) | (location,Address 0xe88e18,sample) <- tagged] of
    (location,sample):_ -> do
      root <- readRamWord sample (Address 0x157dd3c)
      count <- selectedCount root snapshot
      pure [RefusalCase SelectedCount L.SelectedCountMismatch location (RegisterWord R0 count)
        ,RefusalCase Namespace L.NamespaceMismatch location (RegisterWord R0 201)]
    [] -> pure []
  pointer <- case [(location,sample) | (location,Address 0xe80e44,sample) <- tagged] of
    (location,sample):_ -> do
      registers <- get "registers" sample
      case registers :: [Word32] of
        first:_ -> pure [RefusalCase FramePointer L.FramePointerMismatch location (RegisterWord R0 (first+4))]
        _ -> Left "genuine frame publication lacks registers"
    [] -> pure []
  callback <- case [(location,sample) | (location,Address pc,sample) <- tagged,pc `elem` [0xe4e63c,0xe4e5d4]] of
    (location,sample):_ -> do
      registers <- get "registers" sample
      case registers :: [Word32] of
        _:mask:_ -> pure [RefusalCase CallbackTask L.CallbackTaskMismatch location (RegisterWord R1 (mask `xor` 1))]
        _ -> Left "genuine signal seam lacks registers"
    [] -> case [(location,sample) | (location,Address pc,sample) <- tagged,pc `elem` [0x11d5600,0x11d5688]] of
      (location,sample):_ -> do
        registers <- get "registers" sample
        case registers :: [Word32] of
          _:_:completion:_ -> pure [RefusalCase CallbackTask L.CallbackTaskMismatch location (RegisterWord R2 (completion `xor` 4))]
          _ -> Left "genuine storage seam lacks registers"
      [] -> pure []
  let cases = beginCases ++ [catalogue,overlap] ++ opcode ++ allocation ++ pointer ++ callback
      present kind = any (\(RefusalCase candidate _ _ _) -> candidate == kind) cases
      absent = filter (not . present) [CatalogueCount,SelectedCount,Namespace,FramePointer,SampledOpcode,TcmOverlap,CallbackTask]
  pure (cases,absent)
  where
    selectedCount root sample = do
      rows <- forM [0..12 :: Word32] $ \index -> do
        base <- readRamWord sample (Address (0xe8a340+index*16))
        count <- readRamWord sample (Address (0xe8a33c+index*16))
        pure (base,count)
      case [count | (base,count) <- rows,base == root] of
        count:_ -> pure count
        [] -> Left "genuine root is absent from observed catalogue"

parseCallerCode :: Value -> Either String L.CallerCode
parseCallerCode raw = L.CallerCode <$> bytes "save" <*> bytes "permit_a" <*> bytes "permit_b"
  where
    bytes key = do
      values <- get key raw
      unless (length (values :: [Word8]) == 4) (Left "genuine caller publication must contain one sampled word")
      pure (B.pack values)

mutateCallerCode :: CallerPublication -> L.CallerCode -> Either String L.CallerCode
mutateCallerCode publication original = do
  let bytes = case publication of
        SavePublication -> L.savePublicationCode original
        PermitAPublication -> L.permitPublicationACode original
        PermitBPublication -> L.permitPublicationBCode original
  changed <- case B.uncons bytes of
    Just (first,rest) -> pure (B.cons (first `xor` 1) rest)
    Nothing -> Left "genuine caller publication sample is absent"
  pure $ case publication of
    SavePublication -> L.CallerCode changed (L.permitPublicationACode original) (L.permitPublicationBCode original)
    PermitAPublication -> L.CallerCode (L.savePublicationCode original) changed (L.permitPublicationBCode original)
    PermitBPublication -> L.CallerCode (L.savePublicationCode original) (L.permitPublicationACode original) changed

expectRefusal :: CaseKind -> L.RefusalCategory -> Either L.Refusal a -> IO Value
expectRefusal kind expected result = case result of
  Left refusal | L.refusalCategory refusal == expected -> pure (object
    ["case" .= show kind,"expected" .= show expected,"observed" .= show (L.refusalCategory refusal),"refused" .= True])
  Left refusal -> fail ("mutation " ++ show kind ++ " refused under unexpected category: " ++ show refusal)
  Right _ -> fail ("mutation " ++ show kind ++ " was accepted")

required :: String -> [(String,String)] -> IO FilePath
required name options = maybe (fail ("missing " ++ name)) pure (lookup name options)
arguments :: [String] -> Either String [(String,String)]
arguments [] = pure []
arguments (name:value:rest) = do
  unless (name `elem` ["--image5","--image6","--program","--runtime","--journal","--snapshots","--caller-code"])
    (Left ("unknown argument " ++ name))
  pairs <- arguments rest
  unless (name `notElem` map fst pairs) (Left ("duplicate argument " ++ name))
  pure ((name,value):pairs)
arguments _ = Left "arguments require name/value pairs"
readBounded :: Int -> FilePath -> IO B.ByteString
readBounded limit path = withBinaryFile path ReadMode $ \handle -> do
  bytes <- B.hGet handle (limit+1)
  unless (B.length bytes <= limit) (fail "receipt input exceeds bound")
  pure bytes
strictFile :: Int -> FilePath -> IO Value
strictFile limit path = readBounded limit path >>= either fail pure . decodeStrictIntegers

main :: IO ()
main = do
  args <- getArgs >>= either fail pure . arguments
  image5Path <- required "--image5" args
  image6Path <- required "--image6" args
  programPath <- required "--program" args
  journalPath <- required "--journal" args
  snapshotsPath <- required "--snapshots" args
  callerPath <- required "--caller-code" args
  image5 <- readBounded (8*1024*1024) image5Path
  image6 <- readBounded (8*1024*1024) image6Path
  source <- either (fail . show) pure (L.deriveSource image5 image6)
  metadata <- strictFile (64*1024) programPath
  runtime <- maybe (pure B.empty) (readBounded (1024*1024)) (lookup "--runtime" args)
  program <- either (fail . show) pure (L.parseProgram source runtime metadata)
  journal <- readBounded (32*1024*1024) journalPath
  let lines' = filter (not . B.null) (B.split 10 journal)
  unless (length lines' <= 16384 && all ((<=1024*1024) . B.length) lines') (fail "journal exceeds bounded line protocol")
  records <- mapM (either fail pure . decodeStrictIntegers) lines'
  snapshotsValue <- strictFile (4*1024*1024) snapshotsPath
  snapshots <- either fail pure (typed snapshotsValue)
  baseline <- either (fail . ("genuine baseline refused: " ++) . show) pure (L.replayJournal source program records snapshots)
  callerValue <- strictFile 4096 callerPath
  caller <- either fail pure (parseCallerCode callerValue)
  either (fail . ("genuine caller publications refused: " ++) . show) pure (L.validateCallerCode source program caller)
  (cases,unobserved) <- either fail pure (makeCases records snapshots)
  results <- forM cases $ \refusalCase@(RefusalCase kind expected _ _) -> do
    (changedRecords,changedSnapshots) <- either fail pure (applyCase refusalCase records snapshots)
    expectRefusal kind expected (L.replayJournal source program changedRecords changedSnapshots)
  callerResults <- forM
    [(SavePublication,ChangedSavePublication),(PermitAPublication,ChangedPermitAPublication),(PermitBPublication,ChangedPermitBPublication)]
    $ \(publication,kind) -> do
      changed <- either fail pure (mutateCallerCode publication caller)
      expectRefusal kind L.ProgramMismatch (L.validateCallerCode source program changed)
  BL.putStrLn (J.encode (object ["genuine_baseline_accepted" .= True,"baseline" .= L.observationValue baseline
    ,"genuine_caller_publications_accepted" .= True
    ,"refusals" .= (results ++ callerResults),"unobserved_mutations" .= map show unobserved,"positive_firmware_receipt_created" .= False]))
