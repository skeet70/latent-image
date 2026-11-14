{-# LANGUAGE BangPatterns, DeriveAnyClass, DerivingStrategies, GeneralizedNewtypeDeriving, OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless, when)
import Data.Aeson hiding (Key)
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import Data.String (IsString)
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word16, Word32, Word64)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256File, sha256Hex)
import GHC.Generics (Generic)
import Numeric (readHex)
import System.Directory (createDirectory)
import System.Environment (getArgs)
import System.Exit (die)
import System.FilePath ((</>))
import System.IO (Handle, IOMode (ReadMode, WriteMode), withBinaryFile)
import Text.Read (readMaybe)

newtype Instructions = Instructions Word64 deriving stock (Eq, Ord, Show) deriving newtype (FromJSON, ToJSON)
newtype Tick = Tick Word64 deriving stock (Eq, Ord, Show) deriving newtype (FromJSON, ToJSON)
newtype Count = Count Word64 deriving stock (Eq, Ord, Show) deriving newtype (FromJSON, ToJSON, Num)
newtype Label = Label Text deriving stock (Eq, Ord, Show) deriving newtype (FromJSON, ToJSON, FromJSONKey, ToJSONKey)
newtype Address = Address Word32 deriving stock (Eq, Ord, Show) deriving newtype (FromJSON, ToJSON, FromJSONKey, ToJSONKey)
newtype CoreId = CoreId Word32 deriving stock Eq deriving newtype (FromJSON, ToJSON)
newtype TaskId = TaskId Word16 deriving stock Eq deriving newtype (FromJSON, ToJSON)
newtype ContextDepth = ContextDepth Word32 deriving stock Eq deriving newtype (FromJSON, ToJSON)
newtype CpuMode = CpuMode Word32 deriving stock Eq deriving newtype (FromJSON, ToJSON)
newtype QueueId = QueueId Word16 deriving stock Eq deriving newtype (FromJSON, ToJSON)
newtype OutputPointer = OutputPointer Address deriving stock Eq deriving newtype (FromJSON, ToJSON)
newtype MenuCoordinate = MenuCoordinate Word32 deriving stock Eq deriving newtype (FromJSON, ToJSON)
data MenuPosition = MenuPosition MenuCoordinate MenuCoordinate MenuCoordinate deriving Eq
instance FromJSON MenuPosition where
  parseJSON value = do
    coordinates <- parseJSON value
    case coordinates of
      [a,b,c] -> pure (MenuPosition a b c)
      _ -> fail "menu position requires three coordinates"
instance ToJSON MenuPosition where toJSON (MenuPosition a b c) = toJSON [a,b,c]
data Extent = Extent Address Address deriving Eq
instance FromJSON Extent where
  parseJSON value = do
    addresses <- parseJSON value
    case addresses of
      [start,end] | start < end -> pure (Extent start end)
      _ -> fail "extent requires an increasing address pair"
newtype Digest = Digest Sha256 deriving Eq
instance FromJSON Digest where
  parseJSON = withText "SHA256" $ either fail (pure . Digest) . parseSha256Hex . T.unpack
instance ToJSON Digest where toJSON (Digest d) = toJSON (sha256Hex d)
data Clock = BoardTicks | InstructionClock deriving Eq
instance FromJSON Clock where
  parseJSON = withText "clock" $ \t -> case t of
    "board-ticks" -> pure BoardTicks
    "instructions" -> pure InstructionClock
    _ -> fail "unsupported clock"
instance ToJSON Clock where toJSON c = toJSON (case c of BoardTicks -> "board-ticks" :: Text; InstructionClock -> "instructions")
data Scenario = Boot | MenuRoutes | HalfPressScenario | Rtc deriving Eq
instance FromJSON Scenario where
  parseJSON = withText "scenario" $ \t -> case t of
    "boot" -> pure Boot
    "menu-routes" -> pure MenuRoutes
    "half-press" -> pure HalfPressScenario
    "rtc" -> pure Rtc
    _ -> fail "unsupported scenario"
instance ToJSON Scenario where toJSON s = toJSON (case s of Boot -> "boot" :: Text; MenuRoutes -> "menu-routes"; HalfPressScenario -> "half-press"; Rtc -> "rtc")
data Start = Reset | IndependentCheckpoint deriving Eq
instance FromJSON Start where
  parseJSON = withText "start" $ \t -> case t of
    "reset" -> pure Reset
    "independent-checkpoint" -> pure IndependentCheckpoint
    _ -> fail "unsupported start"
instance ToJSON Start where toJSON s = toJSON (case s of Reset -> "reset" :: Text; IndependentCheckpoint -> "independent-checkpoint")
data Phase = BootPhase | RoutePhase deriving Eq
instance FromJSON Phase where
  parseJSON = withText "raw phase" $ \t -> case t of
    "boot" -> pure BootPhase
    "route" -> pure RoutePhase
    _ -> fail "unsupported phase"
instance ToJSON Phase where toJSON p = toJSON (case p of BootPhase -> "boot" :: Text; RoutePhase -> "route")
data Key = Up | Down | LeftKey | RightKey | Ok | Back | Q deriving (Eq, Generic)
instance FromJSON Key where parseJSON = genericParseJSON defaultOptions {constructorTagModifier = keyName}
instance ToJSON Key where toJSON = genericToJSON defaultOptions {constructorTagModifier = keyName}
keyName :: String -> String
keyName name = case name of
  "LeftKey" -> "left"
  "RightKey" -> "right"
  _ -> camelTo2 '-' name

opts :: Options
opts = defaultOptions {fieldLabelModifier = camelTo2 '_' . drop 1}

data Origin = Origin {oBoardTick :: Tick, oInstructions :: Instructions} deriving (Eq, Generic)
instance FromJSON Origin where parseJSON = genericParseJSON opts
instance ToJSON Origin where toJSON = genericToJSON opts

data Stimulus = Press Key | Release Key | HalfPress | HalfRelease | End deriving Eq
instance FromJSON Stimulus where
  parseJSON = withObject "stimulus" $ \o -> do
    kind <- o .: "kind" :: Parser Text
    let key = o .: "key"
    case kind of
      "press" -> Press <$> key
      "release" -> Release <$> key
      "half-press" -> pure HalfPress
      "half-release" -> pure HalfRelease
      "end" -> pure End
      _ -> fail "unsupported stimulus"
instance ToJSON Stimulus where
  toJSON s = case s of
    Press k -> object ["kind" .= ("press" :: Text), "key" .= k]
    Release k -> object ["kind" .= ("release" :: Text), "key" .= k]
    HalfPress -> object ["kind" .= ("half-press" :: Text)]
    HalfRelease -> object ["kind" .= ("half-release" :: Text)]
    End -> object ["kind" .= ("end" :: Text)]
data Point = Point {pStimulus :: Stimulus, pTick :: Tick, pInstructions :: Instructions} deriving (Eq, Generic)
instance FromJSON Point where parseJSON = genericParseJSON opts
instance ToJSON Point where toJSON = genericToJSON opts
data Stimuli = Stimuli {tClock :: Clock, tOrigin :: Origin, tPoints :: [Point]} deriving (Eq, Generic)
instance FromJSON Stimuli where parseJSON = genericParseJSON opts
instance ToJSON Stimuli where toJSON = genericToJSON opts
data Scheduled = Scheduled {dStimulus :: Stimulus, dTick :: Tick} deriving (Eq, Generic)
instance FromJSON Scheduled where parseJSON = genericParseJSON opts
data Schedule = Schedule {hPoints :: [Scheduled]} deriving Generic
instance FromJSON Schedule where parseJSON = genericParseJSON opts
data Timing = Timing Clock Digest deriving Eq
instance FromJSON Timing where parseJSON = withObject "preparation timing" $ \o -> Timing <$> o .: "clock" <*> o .: "schedule_sha256"
instance ToJSON Timing where toJSON (Timing c d) = object ["clock" .= c, "schedule_sha256" .= d]
data Actor = Actor {aCore :: CoreId, aTask :: Maybe TaskId, aDepth :: ContextDepth, aMode :: CpuMode} deriving (Eq, Generic)
instance FromJSON Actor where parseJSON = genericParseJSON opts
instance ToJSON Actor where toJSON = genericToJSON opts
data Frame = Frame {fKey :: Actor, fSp :: Address, fResume :: Address, fLabel :: Maybe Label, fTrack :: Bool, fOutputs :: [OutputPointer], fBlockOutput :: Maybe OutputPointer, fQueue :: Maybe QueueId} deriving (Eq, Generic)
instance FromJSON Frame where parseJSON = genericParseJSON opts
instance ToJSON Frame where toJSON = genericToJSON opts
data Boundary = Boundary {bEventCount :: Count, bPendingCalls :: [Frame]} deriving (Eq, Generic)
instance FromJSON Boundary where parseJSON = genericParseJSON opts
instance ToJSON Boundary where toJSON = genericToJSON opts
data Preparation = Preparation {rTiming :: Timing, rOrigin :: Origin, rBoundary :: Origin, rHistoryBoundary :: Boundary, rRawEvents :: Count} deriving (Eq, Generic)
instance FromJSON Preparation where parseJSON = genericParseJSON opts
instance ToJSON Preparation where toJSON = genericToJSON opts
data Scan = Scan {cBoardTick :: Tick, cInstructions :: Instructions, cReleased :: [(Key, Bool)]} deriving (Eq, Generic)
instance FromJSON Scan where parseJSON = genericParseJSON opts
instance ToJSON Scan where toJSON = genericToJSON opts
data Summary = Summary
  { sSchema :: Word32, sComplete :: Bool, sError :: Maybe Text, sScenario :: Scenario
  , sRunSha256 :: Digest, sUpdateSha256 :: Digest, sBootRomSha256 :: Digest, sObserverSha256 :: Digest
  , sInterfaceSha256 :: Digest, sScheduleSha256 :: Maybe Digest, sCheckpoint :: Maybe ()
  , sPreparation :: Maybe Preparation, sStart :: Start, sObservationStartInstructions :: Instructions
  , sStimuli :: Stimuli, sScans :: [Scan], sEvents :: Count, sRawPhase :: Phase, sRawPhaseEvents :: Count
  , sEntries :: M.Map Label Count, sInstructions :: Instructions, sMenu :: Maybe MenuPosition
  } deriving (Eq, Generic)
instance FromJSON Summary where parseJSON = genericParseJSON opts
instance ToJSON Summary where toJSON = genericToJSON opts

data Entry = Entry Address Label
instance FromJSON Entry where
  parseJSON = withObject "entry" $ \o -> Entry <$> (o .: "address" >>= hexAddress) <*> o .: "label"
data Plan = Plan [Entry] [Extent]
instance FromJSON Plan where
  parseJSON = withObject "interface plan" $ \o -> do
    es <- o .: "entries"; imports <- o .:? "imports" .!= []; callbacks <- o .:? "callbacks" .!= []
    Plan (es ++ imports ++ callbacks) <$> o .: "extents"
data Site = Interface | Return | KeyScan deriving Eq
instance FromJSON Site where
  parseJSON = withText "raw site" $ \s -> case s of
    "time-interface" -> pure Interface
    "time-return" -> pure Return
    "key-scan" -> pure KeyScan
    _ -> fail "unsupported raw site"
data Raw = Raw {rawInstructions :: Instructions, rawAddress :: Address, rawSite :: Site}
instance FromJSON Raw where
  parseJSON = withObject "raw hit" $ \o -> Raw <$> o .: "instructions" <*> (o .: "address" >>= hexAddress) <*> o .: "site"
hexAddress :: Text -> Parser Address
hexAddress t = case T.stripPrefix "0x" t of
  Just x | [(n, "")] <- readHex (T.unpack x), n <= toInteger (maxBound :: Word32) -> pure (Address (fromInteger n))
  _ -> fail "invalid hexadecimal guest address"
data Fetch = Fetch {xStart :: Address, xEnd :: Address, xFetches :: Count} deriving (Eq, Generic)
instance FromJSON Fetch where parseJSON = genericParseJSON opts
instance ToJSON Fetch where toJSON = genericToJSON opts
data Coverage = Coverage {vFetchReceipts :: [Fetch], vVisited :: M.Map Address Count, vRetiredBodyFetches :: M.Map Address Count, vEvents :: Count, vPending :: Count} deriving (Eq, Generic)
instance FromJSON Coverage where parseJSON = genericParseJSON opts
instance ToJSON Coverage where toJSON = genericToJSON opts

newtype ObservationDirectory = ObservationDirectory FilePath deriving newtype ToJSON
newtype MismatchDiagnostic = MismatchDiagnostic Text deriving newtype (IsString, ToJSON)
data ObservationFiles = ObservationFiles Digest Digest Digest Digest
instance ToJSON ObservationFiles where
  toJSON (ObservationFiles summary traffic interface coverage) = toJSON
    [ object ["file" .= ("summary.json" :: Text), "sha256" .= summary]
    , object ["file" .= ("traffic.jsonl" :: Text), "sha256" .= traffic]
    , object ["file" .= ("interface.jsonl" :: Text), "sha256" .= interface]
    , object ["file" .= ("interface-summary.json" :: Text), "sha256" .= coverage]
    ]
data ProofSource = ProofSource ObservationDirectory ObservationFiles
data ProofIdentity = ProofIdentity ProofSource Digest Instructions
data EquivalenceProof
  = Unproven ProofIdentity
  | Mismatch ProofIdentity MismatchDiagnostic
  | Equivalent ProofIdentity ProofSource
instance ToJSON EquivalenceProof where
  toJSON proof =
    let identity = case proof of Unproven i -> i; Mismatch i _ -> i; Equivalent i _ -> i
        ProofIdentity (ProofSource directory files) plan boundary = identity
        fields = ["source" .= directory, "source_files" .= files, "plan_sha256" .= plan, "boundary_instructions" .= boundary]
    in object (fields ++ case proof of
      Unproven _ -> ["status" .= ("unproven" :: Text)]
      Mismatch _ diagnostic -> ["status" .= ("mismatch" :: Text), "first_mismatch" .= diagnostic]
      Equivalent _ (ProofSource independentDirectory independentFiles) ->
        ["status" .= ("equivalent" :: Text), "independent_source" .= independentDirectory,
         "independent_files" .= independentFiles,
         "comparison" .= ("exact raw/interface bytes; typed summary excluding unavailable prefix menu; complete declared extents" :: Text)])

bindSource :: ObservationDirectory -> IO ProofSource
bindSource directory@(ObservationDirectory path) = do
  let digest name = Digest <$> sha256File (path </> name)
  files <- ObservationFiles <$> digest "summary.json" <*> digest "traffic.jsonl" <*> digest "interface.jsonl" <*> digest "interface-summary.json"
  pure (ProofSource directory files)

load :: FromJSON a => FilePath -> IO a
load p = B.readFile p >>= either (die . ((p ++ ": ") ++)) pure . eitherDecodeStrict'
save :: ToJSON a => FilePath -> a -> IO ()
save p = L.writeFile p . encode
require :: Bool -> String -> IO ()
require ok message = unless ok (die message)

foldRows :: (a -> B.ByteString -> IO a) -> a -> Handle -> IO a
foldRows consume initial handle = refill initial []
 where
  joinFragments fragments = case fragments of
    [one] -> one
    _ -> B.concat (reverse fragments)
  refill !state partial = do
    chunk <- B.hGetSome handle 65536
    if B.null chunk
      then if null partial then pure state else consume state (joinFragments partial)
      else case B.elemIndex 10 chunk of
        Nothing -> refill state (chunk : partial)
        Just n -> do
          let (first, rest) = B.splitAt (n + 1) chunk
          next <- consume state (joinFragments (first : partial))
          complete next rest
  complete !state bytes = case B.elemIndex 10 bytes of
    Nothing -> refill state (if B.null bytes then [] else [bytes])
    Just n -> do
      let (line, rest) = B.splitAt (n + 1) bytes
      next <- consume state line
      complete next rest

data RawStats = RawStats !(Maybe Instructions) !Count !Count !(M.Map Label Count) !(M.Map Address Count)

sliceRaw :: FilePath -> FilePath -> Instructions -> M.Map Address Label -> IO RawStats
sliceRaw source destination boundary labels =
  withBinaryFile source ReadMode $ \input -> withBinaryFile destination WriteMode $ \output ->
    foldRows (consume output) (RawStats Nothing 0 0 M.empty M.empty) input
 where
  consume output (RawStats previous total selected entries visits) bytes = do
    hit <- either (die . ("raw hit: " ++)) pure (eitherDecodeStrict' bytes)
    require (maybe True (<= rawInstructions hit) previous) "raw instruction counts are not monotonic"
    let included = rawInstructions hit <= boundary
        label = M.findWithDefault (Label "kernel-or-return") (rawAddress hit) labels
    when included (B.hPut output bytes)
    pure (RawStats (Just (rawInstructions hit)) (total + 1)
      (if included then selected + 1 else selected)
      (if included then M.insertWith (+) label 1 entries else entries)
      (if included && rawSite hit == Interface then M.insertWith (+) (rawAddress hit) 1 visits else visits))

sliceInterface :: FilePath -> FilePath -> Count -> IO ()
sliceInterface source destination boundary =
  withBinaryFile source ReadMode $ \input -> withBinaryFile destination WriteMode $ \output -> do
    total <- foldRows (\count bytes -> do
      when (count < boundary) (B.hPut output bytes)
      pure (count + 1)) 0 input
    require (total >= boundary) "interface stream ends before boundary"

compareFiles :: FilePath -> FilePath -> IO (Maybe Integer)
compareFiles left right = withBinaryFile left ReadMode $ \a -> withBinaryFile right ReadMode $ \b -> go a b 0
 where
  go a b !offset = do
    expected <- B.hGet a 65536
    actual <- B.hGet b 65536
    if expected == actual
      then if B.null expected then pure Nothing else go a b (offset + fromIntegral (B.length expected))
      else pure (Just (offset + fromIntegral (firstDifference expected actual)))
  firstDifference a b = scan 0
   where
    common = min (B.length a) (B.length b)
    scan !index
      | index == common = index
      | B.index a index /= B.index b index = index
      | otherwise = scan (index + 1)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [src, plan, schedule, boundary, out] -> reduce src plan schedule boundary out Nothing
    [src, plan, schedule, boundary, out, standalone] -> reduce src plan schedule boundary out (Just standalone)
    _ -> die "usage: time-prefix SOURCE_DIR INTERFACE_PLAN BOOT_SCHEDULE BOUNDARY_INSTRUCTIONS NEW_OUTPUT_DIR [INDEPENDENT_BOOT_DIR]"

reduce :: FilePath -> FilePath -> FilePath -> String -> FilePath -> Maybe FilePath -> IO ()
reduce src planPath schedulePath expected out independent = do
  boundaryInstructions <- maybe (die "invalid boundary instruction count") (pure . Instructions) (readMaybe expected)
  s <- load (src </> "summary.json")
  Plan es extents <- load planPath
  Schedule schedule <- load schedulePath
  digest <- Digest <$> sha256File schedulePath
  prep <- maybe (die "source has no preparation boundary") pure (sPreparation s)
  actualPlanDigest <- Digest <$> sha256File planPath
  require (actualPlanDigest == sInterfaceSha256 s) "interface plan identity mismatch"
  let boundary = rHistoryBoundary prep
      end = rBoundary prep
      points = take (length schedule) (tPoints (sStimuli s))
      labels = M.fromList [(a,l) | Entry a l <- es]
  require (sSchema s == 4 && sComplete s && sError s == Nothing && sStart s == Reset && sCheckpoint s == Nothing && sObservationStartInstructions s == Instructions 0) "source is not a complete schema4 reset observation"
  require (rOrigin prep == Origin (Tick 0) (Instructions 0) && tOrigin (sStimuli s) == rOrigin prep && tClock (sStimuli s) == BoardTicks) "source is not a continuous board-clock observation"
  require (rTiming prep == Timing BoardTicks digest) "preparation schedule identity mismatch"
  require (not (null schedule) && [Scheduled (pStimulus p) (pTick p) | p <- points] == schedule && pStimulus (last points) == End) "boot schedule differs from original reached points"
  require (oInstructions end == boundaryInstructions && pInstructions (last points) == boundaryInstructions && pTick (last points) == oBoardTick end) "explicit boundary mismatch"
  createDirectory out
  RawStats _ total selected entries visits <- sliceRaw (src </> "traffic.jsonl") (out </> "traffic.jsonl") boundaryInstructions labels
  require (total == sEvents s && selected == rRawEvents prep) "raw source or prefix count differs from original receipt"
  sliceInterface (src </> "interface.jsonl") (out </> "interface.jsonl") (bEventCount boundary)
  let derived = s { sScenario = Boot, sPreparation = Nothing, sScheduleSha256 = Just digest
                  , sStimuli = (sStimuli s) {tPoints = points}, sScans = filter ((<= boundaryInstructions) . cInstructions) (sScans s)
                  , sEvents = rRawEvents prep, sRawPhase = BootPhase, sRawPhaseEvents = rRawEvents prep
                  , sEntries = entries, sInstructions = boundaryInstructions, sMenu = Nothing }
      noCoverage = Coverage [] visits M.empty (bEventCount boundary) (fromIntegral (length (bPendingCalls boundary)))
  save (out </> "summary.json") derived
  save (out </> "interface-summary.json") noCoverage
  save (out </> "boundary.json") prep
  source <- bindSource (ObservationDirectory src)
  let identity = ProofIdentity source actualPlanDigest boundaryInstructions
      proof = save (out </> "equivalence.json")
      mismatch diagnostic@(MismatchDiagnostic message) = do
        proof (Mismatch identity diagnostic)
        die (T.unpack message)
  proof (Unproven identity)
  case independent of
    Nothing -> pure ()
    Just boot -> do
      other <- load (boot </> "summary.json")
      let canonical x = x {sMenu = Nothing}
      unless (canonical other == canonical derived) $ do
        save (out </> "independent-summary.json") other
        mismatch "standalone summary semantic mismatch; retained independent-summary.json"
      let compareBytes name = do
            difference <- compareFiles (out </> name) (boot </> name)
            case difference of
              Nothing -> pure ()
              Just offset -> mismatch (MismatchDiagnostic (T.pack (name ++ " byte mismatch at offset " ++ show offset)))
      compareBytes "traffic.jsonl"
      compareBytes "interface.jsonl"
      coverage <- load (boot </> "interface-summary.json")
      unless (vEvents coverage == bEventCount boundary && vPending coverage == vPending noCoverage) (mismatch "coverage boundary mismatch")
      unless (not (null extents) && map (\f -> Extent (xStart f) (xEnd f)) (vFetchReceipts coverage) == extents && all (\f -> xStart f < xEnd f && xFetches f > 0) (vFetchReceipts coverage)) (mismatch "coverage does not prove every complete declared extent")
      unless (M.null (vRetiredBodyFetches coverage)) (mismatch "coverage contains retired-body fetches")
      independentSource <- bindSource (ObservationDirectory boot)
      save (out </> "interface-summary.json") coverage
      proof (Equivalent identity independentSource)
