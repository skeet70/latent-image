{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.CaptureStream
  ( Cursor, initialCursor, cursorValue, Instructions, instructionValue
  , Operation (..), Advance, advanceState, AdvanceState (..), advanceInstructions
  , advanceEndpoint, parseAdvance, beginRequest, hitsRequest, ackRequest, resumeRequest
  , validateBegin, validateAck, parsePage, Page, pageFrom, pageNext, pagePending
  , pageTotal, pageCaptured, pageDropped, pageIncomplete, pageThrough, pageBoundaryComplete
  , pageFirst, pageLast, pageSites, pageHits, pageMetadata, pageValue
  , SiteTotals, siteLabel, siteTotal, siteValue
  , Event, eventLabel, eventRegisters, eventInstructions, eventValue
  , Coverage, startCoverage, openInterval, validateAdvance, collectBoundary, coverageValue, coverageIncomplete
  , BoundedReader, newBoundedReader, readBoundedLine
  , Journal, openJournal, note, journalPage, synchronizeJournal, closeJournal
  ) where

import Control.DeepSeq (NFData, force)
import Control.Exception (bracket, bracketOnError, finally)
import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), ToJSON (..), Value (..), object, encode, (.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Types as A
import Data.Aeson.Key (Key)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (IORef, newIORef, readIORef, writeIORef)
import Data.List (foldl', nub)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word32, Word64)
import GHC.Generics (Generic)
import GHC.IO.Handle (hDuplicate)
import System.FilePath (takeDirectory)
import System.IO (Handle, IOMode (WriteMode), hClose, hFlush, openBinaryFile)
import System.Posix.IO (OpenMode (ReadOnly), closeFd, defaultFileFlags, handleToFd, openFd)
import System.Posix.Types (Fd)
import System.Posix.Unistd (fileSynchronise)

newtype Cursor = Cursor Word64 deriving (Eq, Ord, Show, Generic, NFData)
newtype Instructions = Instructions Word64 deriving (Eq, Ord, Show, Generic, NFData)
newtype SiteLabel = SiteLabel T.Text deriving (Eq, Ord, Generic, NFData)
newtype Address = Address Word32 deriving (Eq, Ord, Generic, NFData)
newtype Core = Core Word64 deriving (Generic, NFData)
newtype Register = Register Word32 deriving (Generic, NFData)
newtype RamBytes = RamBytes T.Text deriving (Generic, NFData)
newtype RequestedBytes = RequestedBytes Word64 deriving (Generic, NFData)

instance FromJSON Cursor where parseJSON v = Cursor <$> parseJSON v
instance ToJSON Cursor where toJSON = toJSON . cursorValue
instance FromJSON Instructions where parseJSON v = Instructions <$> parseJSON v
instance ToJSON Instructions where toJSON = toJSON . instructionValue
instance FromJSON Address where parseJSON v = Address <$> parseJSON v
instance ToJSON Address where toJSON (Address a) = toJSON a
instance FromJSON Register where parseJSON v = Register <$> parseJSON v
instance ToJSON Register where toJSON (Register r) = toJSON r
instance FromJSON SiteLabel where
  parseJSON = A.withText "site label" $ \t -> do
    unless (not (T.null t) && B.length (TE.encodeUtf8 t) <= 64) (fail "invalid site label length")
    pure (SiteLabel t)
instance ToJSON SiteLabel where toJSON (SiteLabel t) = toJSON t

initialCursor :: Cursor
initialCursor = Cursor 0
cursorValue :: Cursor -> Word64
cursorValue (Cursor n) = n
instructionValue :: Instructions -> Word64
instructionValue (Instructions n) = n

request :: T.Text -> [(Key, Value)] -> Value
request name fields = object (("command" .= name) : fields)
beginRequest, resumeRequest :: Value
beginRequest = request "capture-observation-begin" []
resumeRequest = request "capture-observation-resume" []
hitsRequest, ackRequest :: Cursor -> Value
hitsRequest from = request "capture-observation-hits" ["from" .= from]
ackRequest next = request "capture-observation-ack" ["next" .= next]

validateBounds :: Value -> A.Parser ()
validateBounds = A.withObject "capture bounds" $ \o -> do
  let bound name expected = do
        n <- o .: name
        unless (n == (expected :: Word64)) (fail "unsupported capture bound")
  bound "sites" 256
  bound "label_bytes" 64
  bound "details_per_site" 64
  bound "details_total" 2048
  bound "ram_bytes_per_event" 512
  bound "page_details" 64
  bound "retained_memory_bytes" 2097152
  charged <- o .: "charged_storage_bytes"
  unless (charged > (0 :: Word64) && charged <= 2097152) (fail "capture arena exceeds bound")

validateBegin :: Value -> Either String ()
validateBegin = A.parseEither $ A.withObject "capture begin" $ \o -> do
  protocol <- o .: "protocol"
  active <- o .: "active"
  unless (protocol == (2 :: Word64) && active) (fail "capture protocol2 required")
  o .: "bounds" >>= validateBounds

validateAck :: Cursor -> Word64 -> Value -> Either String ()
validateAck next pending = A.parseEither $ A.withObject "capture acknowledgement" $ \o -> do
  acknowledged <- o .: "acknowledged"
  remaining <- o .: "pending"
  unless (acknowledged == next && remaining == pending) (fail "inconsistent capture acknowledgement")

data Operation = ShutterSequence | Window deriving (Eq, Show)
data AdvanceState = Completed | DrainRequired deriving (Eq, Show)
data Contact = Half | Full | Released deriving (Eq)
data Phase = HalfInitial | FullHeld | HalfFinal | ReleasedFinal deriving (Eq)
data PhaseObservation = PhaseObservation Contact Instructions Instructions
instance FromJSON Contact where
  parseJSON = A.withText "shutter contact" $ \p -> case p of
    "half" -> pure Half; "full" -> pure Full; "released" -> pure Released
    _ -> fail "unknown shutter contact"
instance FromJSON PhaseObservation where
  parseJSON = A.withObject "shutter phase observation" $ \o ->
    PhaseObservation <$> o .: "position" <*> o .: "started" <*> o .: "finished"
data Advance = Advance
  { advanceState :: AdvanceState
  , advanceInstructions :: Instructions
  , advanceEndpoint :: Instructions
  , advancePhase :: Maybe Phase
  }

parseAdvance :: Operation -> Value -> Either String Advance
parseAdvance operation = A.parseEither $ A.withObject "capture advancement" $ \o -> do
  op <- o .: "operation"
  unless (op == (case operation of ShutterSequence -> "shutter-sequence"; Window -> "window" :: T.Text))
    (fail "capture continuation changed operation")
  state <- o .: "state" >>= A.withText "capture progress state" (\s -> case s of
    "completed" -> pure Completed
    "drain-required" -> pure DrainRequired
    _ -> fail "unknown capture progress state")
  at <- o .: "instructions"
  endpoint <- o .: "intended_endpoint"
  unless (at <= endpoint && (state /= Completed || at == endpoint)) (fail "invalid capture advancement endpoint")
  phase <- case operation of
    Window -> pure Nothing
    ShutterSequence -> do
      (current, contact) <- case state of
        Completed -> pure (ReleasedFinal, Released)
        DrainRequired -> do
          current <- o .: "current_phase" >>= \p -> case (p :: Word64) of
            0 -> pure HalfInitial; 1 -> pure FullHeld; 2 -> pure HalfFinal; 3 -> pure ReleasedFinal
            _ -> fail "unknown shutter phase"
          contact <- o .: "position"
          pure (current, contact)
      unless (contact == case current of HalfInitial -> Half; FullHeld -> Full; HalfFinal -> Half; ReleasedFinal -> Released)
        (fail "shutter phase and contact disagree")
      phases <- o .: "phases"
      let specs = [(Half,8000000),(Full,8000000),(Half,4000000),(Released,4000000)]
          validPhase (PhaseObservation actual start finish) (expected, budget) =
            actual == expected && instructionValue finish >= instructionValue start
              && instructionValue finish - instructionValue start == budget
          adjacent (PhaseObservation _ _ finish) (PhaseObservation _ start _) = finish == start
          phaseCount = case current of HalfInitial -> 0; FullHeld -> 1; HalfFinal -> 2; ReleasedFinal -> 3
          currentBudget = case current of HalfInitial -> 8000000; FullHeld -> 8000000; HalfFinal -> 4000000; ReleasedFinal -> 4000000
          endpointCount = instructionValue endpoint
          lastFinish = foldl' (\_ (PhaseObservation _ _ finish) -> Just finish) Nothing phases
      unless (length phases == (if state == Completed then 4 else phaseCount)
          && and (zipWith validPhase phases specs)
          && and (zipWith adjacent phases (drop 1 phases)))
        (fail "invalid shutter phase observations")
      unless (endpointCount >= currentBudget) (fail "shutter phase endpoint precedes its budget")
      let currentStart = Instructions (endpointCount - currentBudget)
          expectedTail = case state of Completed -> endpoint; DrainRequired -> currentStart
      unless (at >= currentStart && maybe (state == DrainRequired) (== expectedTail) lastFinish)
        (fail "shutter phase evidence disagrees with its instruction boundary")
      pure (Just current)
  pure (Advance state at endpoint phase)

data RamStatus = Complete | OutsideRam | ReadFailed | ByteLimit deriving (Eq, Generic, NFData)
data Span = Span Address RequestedBytes RamStatus RamBytes deriving (Generic, NFData)
instance FromJSON Span where
  parseJSON = A.withObject "RAM span" $ \o -> do
    address <- o .: "address"
    requested <- o .: "requested"
    unless (requested <= (65535 :: Word64)) (fail "RAM request exceeds wire width")
    status <- o .: "status" >>= A.withText "RAM status" (\s -> case s of
      "complete" -> pure Complete; "outside-ram" -> pure OutsideRam
      "read-failed" -> pure ReadFailed; "byte-limit" -> pure ByteLimit
      _ -> fail "unknown RAM status")
    bytes <- o .: "bytes"
    unless (T.length bytes <= 1024 && even (T.length bytes) && T.all (`elem` ("0123456789abcdef" :: String)) bytes)
      (fail "invalid RAM bytes")
    let size = fromIntegral (T.length bytes `div` 2)
    unless (size <= requested && (status /= Complete || size == requested)) (fail "RAM span length disagrees with status")
    pure (Span address (RequestedBytes requested) status (RamBytes bytes))
instance ToJSON Span where
  toJSON (Span address (RequestedBytes requested) status (RamBytes bytes)) = object
    ["address" .= address, "requested" .= requested, "status" .= statusText status, "bytes" .= bytes]
    where
      statusText :: RamStatus -> T.Text
      statusText Complete = "complete"
      statusText OutsideRam = "outside-ram"
      statusText ReadFailed = "read-failed"
      statusText ByteLimit = "byte-limit"

data Event = Event
  { eventSequence :: Cursor, eventSite :: SiteLabel, eventAddress :: Address, eventCore :: Core
  , eventInstructions :: Instructions, eventRegisterWords :: (Register, Register, Register, Register)
  , eventSp :: Register, eventLr :: Register, eventCpsr :: Register
  , eventRam :: [Span], eventRamIncomplete :: Bool
  } deriving (Generic, NFData)
instance FromJSON Event where
  parseJSON = A.withObject "capture event" $ \o -> do
    registers <- o .: "registers" >>= \rs -> case rs of
      [a,b,c,d] -> pure (a,b,c,d)
      _ -> fail "capture event must contain r0-r3"
    ram <- o .: "ram"
    let bytes (Span _ _ _ (RamBytes t)) = T.length t `div` 2
    unless (length ram <= 24 && sum (map bytes ram) <= 512) (fail "capture event RAM exceeds bound")
    incomplete <- o .: "ram_incomplete"
    unless (incomplete || all (\(Span _ _ status _) -> status == Complete) ram) (fail "RAM gap hidden by event")
    core <- o .: "core"
    Event <$> o .: "sequence" <*> o .: "site" <*> o .: "address" <*> pure (Core core)
      <*> o .: "instructions" <*> pure registers <*> o .: "sp" <*> o .: "lr" <*> o .: "cpsr"
      <*> pure ram <*> pure incomplete

eventLabel :: Event -> T.Text
eventLabel event = let SiteLabel label = eventSite event in label
eventRegisters :: Event -> [Word32]
eventRegisters event = let (Register a, Register b, Register c, Register d) = eventRegisterWords event in [a,b,c,d]
eventValue :: Event -> Value
eventValue event = let Core core = eventCore event in object
  [ "sequence" .= eventSequence event, "site" .= eventSite event, "address" .= eventAddress event
  , "core" .= core, "instructions" .= eventInstructions event, "registers" .= eventRegisters event
  , "sp" .= eventSp event, "lr" .= eventLr event, "cpsr" .= eventCpsr event
  , "ram" .= eventRam event, "ram_incomplete" .= eventRamIncomplete event ]

data SiteTotals = SiteTotals
  { siteAddress :: Address, siteName :: SiteLabel, siteTotal :: Word64, siteCaptured :: Word64
  , siteDropped :: Word64, sitePending :: Word64, siteFirst :: Maybe Instructions, siteLast :: Maybe Instructions
  , siteFirstDropped :: Maybe Instructions, siteLastDropped :: Maybe Instructions
  } deriving (Generic, NFData)
instance FromJSON SiteTotals where
  parseJSON = A.withObject "capture site totals" $ \o -> do
    site <- SiteTotals <$> o .: "address" <*> o .: "site" <*> o .: "total" <*> o .: "captured"
      <*> o .: "dropped" <*> o .: "pending" <*> o .: "first_instructions" <*> o .: "last_instructions"
      <*> o .: "first_dropped_instructions" <*> o .: "last_dropped_instructions"
    unless (siteCaptured site <= siteTotal site && siteDropped site == siteTotal site - siteCaptured site
      && sitePending site <= min 64 (siteCaptured site)) (fail "inconsistent capture site counts")
    pure site
siteLabel :: SiteTotals -> T.Text
siteLabel site = let SiteLabel label = siteName site in label
siteValue :: SiteTotals -> Value
siteValue site = object
  [ "address" .= siteAddress site, "site" .= siteName site, "total" .= siteTotal site
  , "captured" .= siteCaptured site, "dropped" .= siteDropped site, "pending" .= sitePending site
  , "first_instructions" .= siteFirst site, "last_instructions" .= siteLast site
  , "first_dropped_instructions" .= siteFirstDropped site, "last_dropped_instructions" .= siteLastDropped site ]

data Page = Page
  { pageFrom :: Cursor, pageNext :: Cursor, pageAcknowledged :: Cursor
  , pagePending :: Word64, pageCaptured :: Word64, pageTotal :: Word64, pageDropped :: Word64
  , pageIncomplete :: Bool, pageThrough :: Instructions, pageBoundaryComplete :: Bool
  , pageFirst :: Maybe Instructions, pageLast :: Maybe Instructions
  , pageSites :: [SiteTotals], pageHits :: [Event]
  } deriving (Generic, NFData)

parsePage :: Cursor -> Value -> Either String Page
parsePage expected = A.parseEither $ A.withObject "capture page" $ \o -> do
  o .: "bounds" >>= validateBounds
  page <- Page <$> o .: "from" <*> o .: "next" <*> o .: "acknowledged"
    <*> o .: "pending" <*> o .: "captured" <*> o .: "total" <*> o .: "dropped"
    <*> o .: "incomplete" <*> o .: "through_instructions" <*> o .: "boundary_complete"
    <*> o .: "first_instructions" <*> o .: "last_instructions" <*> o .: "sites" <*> o .: "hits"
  let from = cursorValue (pageFrom page)
      next = cursorValue (pageNext page)
      hits = pageHits page
      sites = pageSites page
      sumField field = sum (map (toInteger . field) sites)
  unless (pageFrom page == expected && pageAcknowledged page == expected && next >= from
    && next - from == fromIntegral (length hits) && length hits <= 64
    && pageCaptured page >= next && pageCaptured page - from == pagePending page
    && pagePending page <= 2048 && (null hits == (pagePending page == 0))
    && map (cursorValue . eventSequence) hits == take (length hits) [from..]
    && all ((<= pageThrough page) . eventInstructions) hits)
    (fail "inconsistent capture page cursors or events")
  unless (length sites <= 256 && length (nub (map siteAddress sites)) == length sites
    && length (nub (map siteName sites)) == length sites
    && sumField siteTotal == toInteger (pageTotal page)
    && sumField siteCaptured == toInteger (pageCaptured page)
    && sumField siteDropped == toInteger (pageDropped page)
    && sumField sitePending == toInteger (pagePending page)
    && all (\hit -> any (\site -> siteAddress site == eventAddress hit && siteName site == eventSite hit) sites) hits
    && (pageIncomplete page || (pageDropped page == 0 && all (not . eventRamIncomplete) hits)))
    (fail "inconsistent capture site metadata")
  pure page

pageMetadata :: Page -> Page
pageMetadata page = force page { pageHits = [] }
pageValue :: Page -> Value
pageValue page = object
  [ "from" .= pageFrom page, "next" .= pageNext page, "acknowledged" .= pageAcknowledged page
  , "pending" .= pagePending page, "captured" .= pageCaptured page, "total" .= pageTotal page
  , "dropped" .= pageDropped page, "incomplete" .= pageIncomplete page
  , "through_instructions" .= pageThrough page, "boundary_complete" .= pageBoundaryComplete page
  , "first_instructions" .= pageFirst page, "last_instructions" .= pageLast page
  , "sites" .= map siteValue (pageSites page), "hits" .= map eventValue (pageHits page) ]

data Interval = Interval Instructions Instructions deriving (Eq, Generic, NFData)
data EventContextGap = NoObservedGap | ObservedGap | UnknownGap deriving (Eq, Generic, NFData)
data InstrumentationCoverage = ArmedSitesOnly
instance ToJSON InstrumentationCoverage where
  toJSON ArmedSitesOnly = object ["scope" .= ("armed-sites-only" :: T.Text), "uninstrumented_gap" .= Null]
data Coverage = Coverage
  { coverageBegin :: Instructions, coverageCollected :: Maybe Instructions
  , coveragePending :: Maybe Interval, coverageObserved :: Instructions
  , coverageDetailLoss :: Bool, coverageEventContextGap :: EventContextGap
  } deriving (Generic, NFData)
startCoverage :: Instructions -> Coverage
startCoverage at = Coverage at Nothing Nothing at False UnknownGap
openInterval :: Operation -> Coverage -> Either String Coverage
openInterval operation coverage = do
  unless (coveragePending coverage == Nothing) (Left "previous capture interval remains uncollected")
  from <- maybe (Left "capture has no successfully collected boundary") Right (coverageCollected coverage)
  unless (from == coverageObserved coverage) (Left "capture has an uncollected boundary")
  let start = instructionValue (coverageObserved coverage)
      budget = case operation of ShutterSequence -> 24000000; Window -> 40000000
  unless (start <= maxBound - budget) (Left "capture endpoint overflows instruction counter")
  pure coverage { coveragePending = Just (Interval from (Instructions (start + budget))) }
validateAdvance :: Advance -> Coverage -> Either String ()
validateAdvance advancement coverage = case coveragePending coverage of
  Just (Interval _ endpoint) ->
    let remaining = case advancePhase advancement of
          Just HalfInitial -> 16000000
          Just FullHeld -> 8000000
          Just HalfFinal -> 4000000
          Just ReleasedFinal -> 0
          Nothing -> 0
    in unless (instructionValue endpoint >= remaining
        && instructionValue (advanceEndpoint advancement) == instructionValue endpoint - remaining
        && advanceInstructions advancement >= coverageObserved coverage)
      (Left "capture continuation changed its endpoint or regressed")
  Nothing -> Left "capture advancement has no pending interval"
collectBoundary :: Page -> Coverage -> Either String Coverage
collectBoundary page coverage = do
  unless (pageThrough page >= coverageObserved coverage) (Left "capture instruction boundary regressed")
  case coveragePending coverage of
    Just (Interval _ endpoint) -> unless (pageThrough page <= endpoint) (Left "capture boundary exceeded intended endpoint")
    Nothing -> pure ()
  let complete = null (pageHits page) && pagePending page == 0 && pageBoundaryComplete page
      pending = case coveragePending coverage of
        Just (Interval _ end) | complete -> if pageThrough page >= end then Nothing else Just (Interval (pageThrough page) end)
        other -> other
  pure $ force coverage
    { coverageCollected = if complete then Just (pageThrough page) else coverageCollected coverage
    , coverageObserved = pageThrough page, coveragePending = pending
    , coverageDetailLoss = coverageDetailLoss coverage || pageDropped page /= 0
    , coverageEventContextGap =
        if coverageEventContextGap coverage == ObservedGap || any eventRamIncomplete (pageHits page)
            || (pageIncomplete page && pageDropped page == 0)
          then ObservedGap
          else if pageDropped page /= 0 || not (pageBoundaryComplete page) then UnknownGap else NoObservedGap }
coverageIncomplete :: Coverage -> Bool
coverageIncomplete coverage = coverageCollected coverage == Nothing || coveragePending coverage /= Nothing || coverageDetailLoss coverage
  || coverageEventContextGap coverage /= NoObservedGap
coverageValue :: Coverage -> Value
coverageValue coverage = object
  [ "from_instructions" .= coverageBegin coverage, "through_instructions" .= coverageCollected coverage
  , "last_successful_collected_through" .= coverageCollected coverage
  , "pending_uncollected_interval" .= fmap intervalValue (coveragePending coverage)
  , "observed_through_instructions" .= coverageObserved coverage
  , "detail_loss" .= coverageDetailLoss coverage
  , "retained_event_ram_context_gap" .= (case coverageEventContextGap coverage of
      NoObservedGap -> Just False; ObservedGap -> Just True; UnknownGap -> Nothing :: Maybe Bool)
  , "instrumentation_coverage" .= ArmedSitesOnly
  , "incomplete" .= coverageIncomplete coverage ]
  where intervalValue (Interval from endpoint) = object ["from_instructions" .= from, "intended_endpoint" .= endpoint]

data BoundedReader = BoundedReader Handle (IORef B.ByteString)
newBoundedReader :: Handle -> IO BoundedReader
newBoundedReader handle = BoundedReader handle <$> newIORef B.empty
readBoundedLine :: BoundedReader -> IO B.ByteString
readBoundedLine (BoundedReader handle buffered) = readIORef buffered >>= go [] 0
  where
    limit = 1024 * 1024
    go chunks size bytes = case B.elemIndex 10 bytes of
      Just end -> do
        unless (size + end <= limit) (ioError (userError "bridge response exceeds 1 MiB"))
        writeIORef buffered (B.drop (end + 1) bytes)
        pure (B.concat (reverse (B.take end bytes : chunks)))
      Nothing -> do
        let nextSize = size + B.length bytes
        unless (nextSize <= limit) (ioError (userError "bridge response exceeds 1 MiB"))
        next <- B.hGetSome handle (min 4096 (limit - nextSize + 1))
        when (B.null next) (ioError (userError "bridge response ended before newline"))
        go (bytes : chunks) nextSize next

data Journal = Journal Handle Fd
openJournal :: FilePath -> IO Journal
openJournal path = bracketOnError (openBinaryFile path WriteMode) hClose $ \handle ->
  bracketOnError (bracketOnError (hDuplicate handle) hClose handleToFd) closeFd $ \fd -> do
    fileSynchronise fd
    let directory = takeDirectory path
        parent = takeDirectory directory
        syncDirectory name = bracket (openFd name ReadOnly defaultFileFlags) closeFd fileSynchronise
    syncDirectory directory
    when (parent /= directory) (syncDirectory parent)
    pure (Journal handle fd)
note :: Journal -> T.Text -> Value -> IO ()
note (Journal handle _) label value = do
  BL.hPut handle (encode (object ["label" .= label, "value" .= value]) <> "\n")
  hFlush handle
journalPage :: Journal -> Cursor -> Value -> IO Page
journalPage journal from value = do
  page <- either (ioError . userError) pure (parsePage from value)
  note journal "capture-observation-page" value
  synchronizeJournal journal
  pure page
synchronizeJournal :: Journal -> IO ()
synchronizeJournal (Journal handle fd) = hFlush handle >> fileSynchronise fd
closeJournal :: Journal -> IO ()
closeJournal (Journal handle fd) = hClose handle `finally` closeFd fd
