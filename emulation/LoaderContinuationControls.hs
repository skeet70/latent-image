{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (foldM, forM, forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Bits ((.&.), complement, shiftL, (.|.), xor)
import Data.List (findIndex, sortOn, stripPrefix)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Vector as V
import qualified Data.Text.Encoding as TE
import Data.Word (Word32, Word64)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, createDirectory, doesPathExist, listDirectory)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode(..), exitFailure)
import System.FilePath ((</>), (<.>))
import System.Posix.Files (createLink, getSymbolicLinkStatus, isRegularFile)

newtype GuestAddress = GuestAddress Word32 deriving (Eq, Ord, Show)
newtype EventId = EventId Int deriving (Eq, Ord, Show)
newtype TcmRegisterWord = TcmRegisterWord Word32
newtype Observer = Observer FilePath
newtype AnalysisRoot = AnalysisRoot FilePath
newtype ObservationDirectory = ObservationDirectory FilePath
newtype OutputDirectory = OutputDirectory FilePath
newtype RowIndex = RowIndex Int
newtype InstructionCount = InstructionCount Integer

data Verdict = Admit | Refuse deriving Eq
data Origin = ActualMutation | SerializedSynthetic
data CallKind = Loader | Image5Wait | Image5Entry | Image5Service | Image5Constructor
  | PlainProducer | PlainService | PlainHook | Deferred | Task deriving Eq
data Resource = LoaderA | LoaderB | Image5 | Image5Bss | Plain | Plain0 | Plain2 | Plain3 deriving Eq
data Access = CpuFetch | CpuRead | CpuWrite | HostRead | HostWrite | ModelBusRead | ModelBusWrite deriving Eq
data ClockMode = AtBegin | AfterEnd | ReversedClock | ResumedBegin | AtEnd deriving Eq
data LoaderCopy = PrimaryCopy | SecondaryCopy deriving Eq
data CopyChange = MoveEarlier LoaderCopy | WrongCopySource | PartialCopy | MisplacedCopy | ExtraCopy | UncertainCopy
data WriteBoundary = BeforeLoaderEntry | AfterLoaderEntry | DuringLoaderFrame deriving Eq
data Writer = HostWriter | ModelWriter
data Gpr = R0 | R3 | R4
data PlainExtent = Extent0 | Extent2 | Extent3
data ObservationInputs = Single ObservationDirectory | Paired ObservationDirectory ObservationDirectory
data Arguments = Arguments Observer AnalysisRoot ObservationInputs OutputDirectory
data ProgramRole = Retained | Owned deriving Eq
data Artifact = Contract | LoaderArtifact LoaderCopy
data ArtifactChange = WrongArtifactHash Artifact | ChangeArtifactBytes Artifact | RebindArtifactBytes Artifact
data FetchChange = TailFetch | WrongFetchCore | WrongFetchPhysical | UnsupportedFetch | AliasFetch | OwnedPrefixFetch | OwnedDataFetch | OwnedPaddingFetch
data GuardChange = WrongEpochStage | EpochGap | RawObserve | DeclaredObserve | MissingCarrierFetch | MissingInitialProjection | MissingOrigin | WrongOrigin | FixedViewViolation | MissingBoundaryFetch
data LoaderChange = PendingCall | WrongCallCore | WrongOriginalFrame | WrongPublication | WrongLiveInput deriving Eq
data CreationField = CreationArgument | CreationTarget
data TimerChange = TimerBinding | TimerSchema | TimerBatch | TimerError | TimerOmitted | TimerWindow | TimerRange | TimerLimit
  | TimerOrder | TimerCpu | TimerPc | TimerAddress | TimerSize | TimerOperation | TimerAccepted | TimerTick | TimerValue | TimerMissingRead | TimerFetchTick
  | TimerSubtraction | TimerRingState | TimerRingWrite | TimerRingWriteTick | TimerCopyRegister | TimerCopyState | TimerCopyWrite | TimerCopyWriteTick | TimerLifetime
  | TimerSourceCore | TimerSourceChronology | TimerSourceInstruction
  | TimerCpuCodeWrite | TimerHostCodeWrite | TimerModelCodeWrite | TimerRestoredCodeWrite
  deriving (Enum, Bounded)
data CodeWriteStyle = IdenticalWord | RestoredWord

creationIndex :: CreationField -> Int
creationIndex CreationArgument = 1
creationIndex CreationTarget = 2

extentResource :: PlainExtent -> Resource
extentResource Extent0 = Plain0
extentResource Extent2 = Plain2
extentResource Extent3 = Plain3

data Part = Field K.Key | Element Int
field :: String -> Part
field = Field . K.fromString

pathAt :: [Part] -> J.Value -> Either String J.Value
pathAt [] value = Right value
pathAt (Field key:rest) (J.Object object) = maybe (Left "wire field absent") (pathAt rest) (KM.lookup key object)
pathAt (Element index:rest) (J.Array values) = maybe (Left "wire element absent") (pathAt rest) (values V.!? index)
pathAt _ _ = Left "wire path shape differs"

modifyAt :: [Part] -> (J.Value -> Either String J.Value) -> J.Value -> Either String J.Value
modifyAt [] change value = change value
modifyAt (Field key:rest) change (J.Object object) = do
  old <- maybe (Left "wire field absent") Right (KM.lookup key object)
  new <- modifyAt rest change old
  pure (J.Object (KM.insert key new object))
modifyAt (Element index:rest) change (J.Array values) = do
  old <- maybe (Left "wire element absent") Right (values V.!? index)
  new <- modifyAt rest change old
  pure (J.Array (values V.// [(index,new)]))
modifyAt _ _ _ = Left "wire mutation path shape differs"

setAt :: J.ToJSON a => [Part] -> a -> J.Value -> Either String J.Value
setAt path value = modifyAt path (const (Right (J.toJSON value)))

arrayAt :: [Part] -> J.Value -> Either String [J.Value]
arrayAt path value = pathAt path value >>= \selected -> case selected of
  J.Array values -> Right (V.toList values)
  _ -> Left "wire array required"

textValue :: J.Value -> Either String String
textValue (J.String value) = Right (T.unpack value)
textValue _ = Left "wire string required"

textAt :: [Part] -> J.Value -> Either String String
textAt path value = pathAt path value >>= textValue

wireNumber :: J.Value -> Either String Integer
wireNumber (J.String value) = case stripPrefix "0x" (T.unpack value) of
  Just hexadecimal -> case readHex hexadecimal of
    [(number,"")] | number >= 0 && number <= toInteger (maxBound :: Word64) -> Right number
    _ -> Left "wire hexadecimal word differs"
  Nothing -> Left "wire word prefix absent"
wireNumber value = case J.fromJSON value :: J.Result Integer of
  J.Success number | number >= 0 && number <= toInteger (maxBound :: Word64) -> Right number
  _ -> Left "wire unsigned integer required"

numberAt :: [Part] -> J.Value -> Either String Integer
numberAt path value = pathAt path value >>= wireNumber

guestAt :: [Part] -> J.Value -> Either String GuestAddress
guestAt path value = do
  number <- numberAt path value
  unless (number <= toInteger (maxBound :: Word32)) (Left "guest address exceeds word")
  pure (GuestAddress (fromInteger number))

eventIdValue :: J.Value -> Either String EventId
eventIdValue value = do
  number <- wireNumber value
  unless (number <= toInteger (maxBound :: Int)) (Left "event index exceeds host bounds")
  pure (EventId (fromInteger number))

eventReference :: String -> J.Value -> Either String EventId
eventReference name value = pathAt [field name] value >>= eventIdValue

hex :: Integer -> String
hex value = "0x" ++ showHex value ""

wordLike :: Integer -> J.Value -> Either String J.Value
wordLike value (J.String _) = Right (J.toJSON (hex value))
wordLike value (J.Number _) = Right (J.toJSON value)
wordLike _ _ = Left "wire word encoding differs"

setGuest :: [Part] -> GuestAddress -> J.Value -> Either String J.Value
setGuest path (GuestAddress address) = modifyAt path (wordLike (toInteger address))

parseCallKind :: J.Value -> Either String CallKind
parseCallKind value = textAt [field "kind"] value >>= \kind -> case kind of
  "loader" -> Right Loader; "image5-wait" -> Right Image5Wait; "image5-entry" -> Right Image5Entry
  "image5-service" -> Right Image5Service; "image5-constructor" -> Right Image5Constructor
  "plain-producer" -> Right PlainProducer; "plain-service" -> Right PlainService
  "plain-hook" -> Right PlainHook; "deferred" -> Right Deferred; "task" -> Right Task
  _ -> Left "unknown wire call kind"

instance J.ToJSON CallKind where
  toJSON kind = J.toJSON (case kind of
    Loader -> "loader"; Image5Wait -> "image5-wait"; Image5Entry -> "image5-entry"
    Image5Service -> "image5-service"; Image5Constructor -> "image5-constructor"
    PlainProducer -> "plain-producer"; PlainService -> "plain-service"
    PlainHook -> "plain-hook"; Deferred -> "deferred"; Task -> "task" :: String)

parseResource :: J.Value -> Either String Resource
parseResource value = textAt [field "resource"] value >>= \resource -> case resource of
  "loader-a" -> Right LoaderA; "loader-b" -> Right LoaderB; "image5" -> Right Image5
  "image5-bss" -> Right Image5Bss; "plain" -> Right Plain; "plain0" -> Right Plain0
  "plain2" -> Right Plain2; "plain3" -> Right Plain3; _ -> Left "unknown wire resource"

parseAccess :: J.Value -> Either String Access
parseAccess value = textAt [field "source"] value >>= \source -> case source of
  "cpu-fetch" -> Right CpuFetch; "cpu-read" -> Right CpuRead; "cpu-write" -> Right CpuWrite
  "host-read" -> Right HostRead; "host-write" -> Right HostWrite
  "model-bus-read" -> Right ModelBusRead; "model-bus-write" -> Right ModelBusWrite
  _ -> Left "unknown wire access source"

resourceText :: Resource -> String
resourceText resource = case resource of
  LoaderA -> "loader-a"; LoaderB -> "loader-b"; Image5 -> "image5"; Image5Bss -> "image5-bss"
  Plain -> "plain"; Plain0 -> "plain0"; Plain2 -> "plain2"; Plain3 -> "plain3"

data CodeImage = CodeImage {imageStart :: GuestAddress, imageBytes :: B.ByteString}
data Fixture = Fixture
  { originalRoot :: FilePath, receipt :: J.Value, rawTrace :: J.Value, watches :: [J.Value], codeImages :: [CodeImage]
  , programRole :: ProgramRole, fileChanges :: M.Map FilePath B.ByteString, watchChanges :: S.Set Int, traceChanged :: Bool
  , timerTransactions :: J.Value, timerChanged :: Bool }

readJson :: Int -> FilePath -> IO J.Value
readJson limit path = H.readRegular limit path >>= either H.failH pure . decodeStrictIntegers

loadFixture :: FilePath -> FilePath -> IO Fixture
loadFixture root analysis = do
  document <- readJson 0x4000000 (root </> "continuations.json")
  traceName <- either H.failH pure (textAt [field "trace",field "path"] document)
  trace <- readJson 0x4000000 (root </> traceName)
  epochs <- either H.failH pure (arrayAt [field "epochs"] document)
  files <- forM epochs $ \epoch -> do
    name <- either H.failH pure (textAt [field "file",field "path"] epoch)
    readJson 0x40000000 (root </> name)
  schema <- either H.failH pure (numberAt [field "schema"] document)
  H.ensure (schema==3 && length epochs==3) "schema3 three-epoch observation required"
  timerName <- either H.failH pure (textAt [field "timer_transactions",field "path"] document)
  timer <- readJson 0x40000000 (root </> timerName)
  role <- either H.failH pure (textAt [field "program",field "role"] document >>= \value -> case value of
    "retained" -> Right Retained; "owned" -> Right Owned; _ -> Left "unknown program role")
  artifacts <- if role==Retained then pure M.empty else M.fromList <$> forM [Contract,LoaderArtifact PrimaryCopy,LoaderArtifact SecondaryCopy] (\artifact -> do
    name <- either H.failH pure (textAt (artifactPath artifact++[field "path"]) document)
    bytes <- H.readRegular 0x20000 (root </> name)
    pure (name,bytes))
  loaders <- forM [(PrimaryCopy,0x40000,"decode/loader-40000.bin"),(SecondaryCopy,0xd0000,"decode/loader-d0000.bin")] $ \(side,start,source) -> do
    name <- if role==Retained then pure (analysis </> source)
      else (root </>) <$> either H.failH pure (textAt (artifactPath (LoaderArtifact side)++[field "path"]) document)
    CodeImage (GuestAddress start) <$> H.readRegular 0x20000 name
  image4 <- H.readRegular 0x1000000 (analysis </> "lz/id4-0061c400.bin")
  image5 <- H.readRegular 0x1000000 (analysis </> "lz/id5-00bf1a00.bin")
  pure (Fixture root document trace files (loaders++[CodeImage (GuestAddress 0x61c400) image4,CodeImage (GuestAddress 0xbf1a00) image5]) role artifacts S.empty False timer False)

artifactPath :: Artifact -> [Part]
artifactPath artifact = [field "program",field "artifacts"]++case artifact of
  Contract -> [field "manifest"]
  LoaderArtifact side -> [field "loaders",Element (if side==PrimaryCopy then 0 else 1)]

eventAt :: Fixture -> EventId -> Either String J.Value
eventAt fixture (EventId index) = pathAt [field "events",Element index] (receipt fixture)

callAt :: Fixture -> Int -> Either String J.Value
callAt fixture index = pathAt [field "calls",Element index] (receipt fixture)

modifyReceipt :: [Part] -> (J.Value -> Either String J.Value) -> Fixture -> Either String Fixture
modifyReceipt path change fixture = do
  updated <- modifyAt path change (receipt fixture)
  pure fixture {receipt=updated}

modifyEvent :: EventId -> [Part] -> (J.Value -> Either String J.Value) -> Fixture -> Either String Fixture
modifyEvent (EventId index) path = modifyReceipt (field "events":Element index:path)

setCall :: J.ToJSON a => Int -> String -> a -> Fixture -> Either String Fixture
setCall index name value = modifyReceipt [field "calls",Element index,field name] (const (Right (J.toJSON value)))

setCallGuest :: Int -> String -> GuestAddress -> Fixture -> Either String Fixture
setCallGuest index name address = modifyReceipt [field "calls",Element index] (setGuest [field name] address)

firstEvent :: Fixture -> GuestAddress -> Either String EventId
firstEvent fixture address = do
  events <- arrayAt [field "events"] (receipt fixture)
  addresses <- traverse (guestAt [field "pc"]) events
  maybe (Left "required source boundary absent") (Right . EventId) (findIndex (==address) addresses)

callIndices :: Fixture -> CallKind -> Either String [Int]
callIndices fixture kind = do
  values <- arrayAt [field "calls"] (receipt fixture)
  kinds <- traverse parseCallKind values
  pure [index | (index,actual) <- zip [0..] kinds, actual==kind]

firstIndex :: String -> [a] -> Either String a
firstIndex _ (value:_) = Right value
firstIndex message [] = Left message

syncEvent :: EventId -> Fixture -> Either String Fixture
syncEvent identifier@(EventId index) fixture = do
  event <- eventAt fixture identifier
  pc <- numberAt [field "pc"] event
  sp <- numberAt [field "registers",field "sp"] event
  lr <- numberAt [field "registers",field "lr"] event
  core <- pathAt [field "core"] event
  count <- pathAt [field "instructions"] event
  cpsr <- pathAt [field "registers",field "cpsr"] event
  registers <- arrayAt [field "registers",field "r"] event >>= traverse wireNumber
  unless (length registers==13) (Left "complete GPR wire record required")
  let values = J.object [K.fromString ("r"++show register) J..= hex value | (register,value) <- zip [0::Int ..] registers]
      change old = foldM (\value (name,new) -> setAt [field name] new value) old
        [("address",J.toJSON (hex pc)),("core",core),("instructions",count),("cpsr",cpsr),("sp",J.toJSON (hex sp)),("lr",J.toJSON (hex lr)),("registers",values)]
  trace <- modifyAt [Element index] change (rawTrace fixture)
  pure fixture {rawTrace=trace,traceChanged=True}

gprIndex :: Gpr -> Int
gprIndex R0 = 0
gprIndex R3 = 3
gprIndex R4 = 4

setRegister :: EventId -> Gpr -> Word32 -> Fixture -> Either String Fixture
setRegister identifier register value fixture = modifyEvent identifier [field "registers",field "r",Element (gprIndex register)] (const (Right (J.toJSON value))) fixture >>= syncEvent identifier

setRegisterWord :: EventId -> Int -> (Integer -> Integer) -> Fixture -> Either String Fixture
setRegisterWord identifier register change fixture =
  modifyEvent identifier [field "registers",field "r",Element register]
    (\value -> wireNumber value >>= \word -> wordLike (change word) value) fixture >>= syncEvent identifier

changeStateWord :: EventId -> GuestAddress -> Fixture -> Either String Fixture
changeStateWord identifier (GuestAddress address) fixture = do
  event <- eventAt fixture identifier
  views <- arrayAt [field "state"] event
  candidates <- forM (zip [0::Int ..] views) $ \(index,view) -> do
    start <- numberAt [field "mapping",field "guest"] view
    values <- arrayAt [field "values"] view
    pure (index,start,start<=toInteger address && toInteger address<start+4*toInteger (length values))
  (index,start,_) <- firstIndex "control state word absent" [candidate | candidate@(_,_,True) <- candidates]
  modifyEvent identifier [field "state",Element index,field "values",Element (fromInteger ((toInteger address-start) `div` 4))]
    (\value -> wireNumber value >>= \word -> wordLike (word `xor` 1) value) fixture

modifyTimer :: (J.Value -> Either String J.Value) -> Fixture -> Either String Fixture
modifyTimer change fixture = do
  updated <- change (timerTransactions fixture)
  pure fixture {timerTransactions=updated,timerChanged=True}

timerReadIndex :: Fixture -> Either String Int
timerReadIndex fixture = do
  rows <- arrayAt [field "batch",field "transactions"] (timerTransactions fixture)
  candidates <- forM (zip [0::Int ..] rows) $ \(index,row) -> do
    pc <- numberAt [field "pc"] row
    pure (index,pc==0x41ad8)
  firstIndex "actual reload device read absent" [index | (index,True) <- candidates]

changeRawTimerRow :: Access -> GuestAddress -> String -> (J.Value -> Either String J.Value) -> Fixture -> Either String Fixture
changeRawTimerRow kind source name change fixture = do
  watch <- pathAt [Element 1] (J.toJSON (watches fixture))
  rows <- arrayAt [field "records"] watch
  candidates <- forM (zip [0::Int ..] rows) $ \(index,row) -> do
    access <- parseAccess row
    pc <- pathAt [field "pc"] row
    matches <- if access==kind && pc/=J.Null then (==source) <$> guestAt [field "pc"] row else Right False
    pure (index,matches)
  index <- firstIndex "actual timer source RAM row absent" [index | (index,True) <- candidates]
  modifyWatch 1 (modifyAt [field "records",Element index,field name] change) fixture

setLink :: EventId -> GuestAddress -> Fixture -> Either String Fixture
setLink identifier address fixture = modifyEvent identifier [field "registers"] (setGuest [field "lr"] address) fixture >>= syncEvent identifier

changeCode :: EventId -> GuestAddress -> Fixture -> Either String Fixture
changeCode identifier address@(GuestAddress guest) fixture = do
  original <- eventAt fixture identifier
  oldPc <- guestAt [field "pc"] original
  let physical = guest .&. complement 0x40000000
      contains image = let GuestAddress start = imageStart image in physical>=start && toInteger physical+4<=toInteger start+toInteger (B.length (imageBytes image))
  image <- firstIndex "control source outside image4/image5" (filter contains (codeImages fixture))
  let GuestAddress start = imageStart image
      offset = fromIntegral (physical-start)
      bytes = imageBytes image
      word = fromIntegral (B.index bytes offset) .|. shiftL (fromIntegral (B.index bytes (offset+1))) 8
        .|. shiftL (fromIntegral (B.index bytes (offset+2))) 16 .|. shiftL (fromIntegral (B.index bytes (offset+3))) 24 :: Word32
  sources <- arrayAt [field "identity",field "sources"] (receipt fixture)
  starts <- traverse (guestAt [field "start"]) sources
  source <- firstIndex "source binding absent" [value | (value,base) <- zip sources starts,base==imageStart image]
  kind <- pathAt [field "kind"] source
  next <- modifyEvent identifier [] (setGuest [field "pc"] address) fixture
  next' <- modifyEvent identifier [field "source"] (const (Right kind)) next
  next'' <- modifyEvent identifier [field "code"] (\code -> setGuest [field "mapping",field "guest"] address code >>= setGuest [field "mapping",field "physical"] (GuestAddress physical) >>= setAt [field "values"] [word]) next'
  changed <- syncEvent identifier next''
  let carrier (GuestAddress pc) = (0x409ac<=pc && pc<0x40b00) || (0xd09ac<=pc && pc<0xd0b00)
  if oldPc==address || not (carrier oldPc && carrier address) then Right changed else do
    count <- numberAt [field "instructions"] original
    core <- numberAt [field "core"] original
    modifyWatch 1 (modifyAt [field "records"] (\value -> do
      rows <- arrayAt [] value
      updated <- forM rows $ \row -> do
        source' <- parseAccess row
        if source'/=CpuFetch then Right row else do
          pc <- guestAt [field "address"] row
          clock <- numberAt [field "instructions"] row
          cpu <- numberAt [field "core"] row
          if pc/=oldPc || clock/=count+1 || cpu/=core then Right row else do
            relocated <- setGuest [field "address"] address row >>= setGuest [field "pc"] address >>= setGuest [field "physical"] (GuestAddress physical)
            modifyAt [field "value"] (\old -> if old==J.Null then Right J.Null else wordLike (toInteger word) old) relocated
      pure (J.toJSON updated))) changed

omitCall :: CallKind -> Fixture -> Either String Fixture
omitCall kind fixture = do
  index <- callIndices fixture kind >>= firstIndex "required call category absent"
  modifyReceipt [field "calls"] (\value -> do
    values <- arrayAt [] value
    pure (J.toJSON [entry | (position,entry) <- zip [0..] values,position/=index])) fixture

makeDeferred :: Fixture -> Either String Fixture
makeDeferred fixture = do
  hooks <- callIndices fixture PlainHook
  extents <- forM hooks $ \index -> do
    call <- callAt fixture index
    before <- eventReference "before" call >>= eventAt fixture
    extentWord <- numberAt [field "registers",field "r",Element 7] before
    extent <- case extentWord of 0 -> Right Extent0; 2 -> Right Extent2; 3 -> Right Extent3; _ -> Left "extent is outside the canonical table"
    pure (index,extent)
  selected <- firstIndex "second extent3 hook absent" (drop 1 [index | (index,Extent3) <- extents])
  call <- callAt fixture selected
  before <- eventReference "before" call
  entered <- eventReference "entry" call
  returned <- eventReference "returned" call
  next <- setCall selected "kind" Deferred fixture >>= setCallGuest selected "continuation" (GuestAddress 0x73beb0)
  next' <- changeCode before (GuestAddress 0x73beac) next >>= setLink entered (GuestAddress 0x73beb0) >>= setLink returned (GuestAddress 0x73beb0)
  changeCode returned (GuestAddress 0x73beb0) next'

swapCalls :: Int -> Int -> Fixture -> Either String Fixture
swapCalls first second fixture = do
  a <- callAt fixture first
  b <- callAt fixture second
  modifyReceipt [field "calls"] (\values -> modifyAt [Element first] (const (Right b)) values >>= modifyAt [Element second] (const (Right a))) fixture

phaseCall :: Word32 -> Fixture -> Either String Int
phaseCall phase fixture = do
  loaders <- callIndices fixture Loader
  matches <- forM loaders $ \index -> do
    call <- callAt fixture index
    before <- eventReference "before" call >>= eventAt fixture
    target <- guestAt [field "target"] call
    core <- numberAt [field "core"] call
    argument <- numberAt [field "registers",field "r",Element 0] before
    pure (index,target==GuestAddress 0x69cdf8 && core==0 && argument==toInteger phase)
  firstIndex "phase call absent" [index | (index,True) <- matches]

phaseOrder :: Fixture -> Either String Fixture
phaseOrder fixture = do
  first <- phaseCall 1 fixture
  second <- phaseCall 3 fixture
  let change index address@(GuestAddress pc) phase original = do
        call <- callAt original index
        before <- eventReference "before" call
        entered <- eventReference "entry" call
        returned <- eventReference "returned" call
        next <- setCallGuest index "continuation" (GuestAddress (pc+4)) original
        next' <- setRegister before R0 phase next >>= setRegister entered R0 phase >>= setLink entered (GuestAddress (pc+4))
        changeCode before address next' >>= changeCode returned (GuestAddress (pc+4))
  firstPc <- callAt fixture first >>= eventReference "before" >>= eventAt fixture >>= guestAt [field "pc"]
  secondPc <- callAt fixture second >>= eventReference "before" >>= eventAt fixture >>= guestAt [field "pc"]
  next <- change first secondPc 3 fixture >>= change second firstPc 1
  swapCalls first second next

constructorOrder :: Fixture -> Either String Fixture
constructorOrder fixture = do
  selected <- callIndices fixture Image5Constructor
  (first,second) <- case selected of a:b:_ -> Right (a,b); _ -> Left "two constructor calls absent"
  a <- callAt fixture first >>= guestAt [field "target"]
  b <- callAt fixture second >>= guestAt [field "target"]
  let retarget index target@(GuestAddress address) original = do
        call <- callAt original index
        before <- eventReference "before" call
        entered <- eventReference "entry" call
        next <- setCallGuest index "target" target original >>= setRegister before R3 address >>= setRegister entered R3 address
        changeCode entered target next
  next <- retarget first b fixture >>= retarget second a
  swapCalls first second next

omitProjection :: Resource -> Maybe GuestAddress -> Fixture -> Either String Fixture
omitProjection resource pc fixture = do
  values <- arrayAt [field "snapshots"] (receipt fixture)
  matches <- forM (zip [0::Int ..] values) $ \(index,value) -> do
    kind <- parseResource value
    event <- eventReference "event" value >>= eventAt fixture
    address <- guestAt [field "pc"] event
    pure (index,kind==resource && maybe True (==address) pc)
  selected <- firstIndex "required projection absent" [index | (index,True) <- matches]
  modifyReceipt [field "snapshots"] (const (Right (J.toJSON [value | (index,value) <- zip [0::Int ..] values,index/=selected]))) fixture

removeEvents :: S.Set EventId -> Fixture -> Either String Fixture
removeEvents removed fixture = do
  events <- arrayAt [field "events"] (receipt fixture)
  trace <- arrayAt [] (rawTrace fixture)
  calls <- arrayAt [field "calls"] (receipt fixture)
  samples <- arrayAt [field "snapshots"] (receipt fixture)
  let kept = [(EventId old,value) | (old,value) <- zip [0..] events,S.notMember (EventId old) removed]
      remap = M.fromList [(old,new) | (new,(old,_)) <- zip [0::Int ..] kept]
      reference value = do
        old <- eventIdValue value
        maybe (Left "removed event is still referenced") (Right . J.toJSON) (M.lookup old remap)
      callRefs value = foldM (\entry name -> modifyAt [field name] (\old -> if old==J.Null then Right J.Null else reference old) entry) value ["before","entry","returned"]
  events' <- forM (zip [0::Int ..] kept) $ \(new,(_,value)) -> setAt [field "id"] new value
  calls' <- traverse callRefs calls
  samples' <- traverse (modifyAt [field "event"] reference) samples
  document <- setAt [field "events"] events' (receipt fixture) >>= setAt [field "calls"] calls' >>= setAt [field "snapshots"] samples'
    >>= modifyAt [field "origin"] reference
  pure fixture {receipt=document,rawTrace=J.toJSON [hit | (old,hit) <- zip [0..] trace,S.notMember (EventId old) removed],traceChanged=True}

creationView :: Fixture -> Either String (EventId,Int,J.Value)
creationView fixture = do
  identifier <- firstEvent fixture (GuestAddress 0x43758)
  event <- eventAt fixture identifier
  address <- guestAt [field "registers",field "r",Element 2] event
  values <- arrayAt [field "state"] event
  matching <- forM (zip [0..] values) $ \(index,value) -> do
    base <- guestAt [field "mapping",field "guest"] value
    words' <- arrayAt [field "values"] value
    pure (index,value,base==address && length words'==3)
  (index,value) <- firstIndex "creation view absent" [(index,value) | (index,value,True) <- matching]
  pure (identifier,index,value)

changeCreation :: CreationField -> (Integer -> Integer) -> Fixture -> Either String Fixture
changeCreation selected change fixture = do
  let slot = creationIndex selected
  (identifier,index,view) <- creationView fixture
  value <- numberAt [field "values",Element slot] view
  modifyEvent identifier [field "state",Element index,field "values",Element slot] (const (Right (J.toJSON (change value)))) fixture

data CpuRows = CpuRows {cpuEpoch :: Int, cpuIndices :: [Int]}
safeCpuRows :: Fixture -> Either String CpuRows
safeCpuRows fixture = do
  candidates <- forM (zip [0..] (watches fixture)) $ \(epoch,watch) -> do
    rows <- arrayAt [field "records"] watch
    selected <- forM (zip [0..] rows) $ \(index,row) -> do
      source <- parseAccess row
      physical <- numberAt [field "physical"] row
      let protected = any (\(low,high) -> low<=physical && physical<high) [(0x409ac,0x40b00),(0xd09ac,0xd0b00),(0x63450,0x63460),(0xf3410,0xf3420)]
      pure (index,source==CpuWrite && not protected)
    pure (CpuRows epoch [index | (index,True) <- selected])
  firstIndex "two non-carrier CPU writes absent" (filter ((>=2) . length . cpuIndices) candidates)

modifyWatch :: Int -> (J.Value -> Either String J.Value) -> Fixture -> Either String Fixture
modifyWatch index change fixture = do
  updated <- modifyAt [Element index] change (J.toJSON (watches fixture)) >>= arrayAt []
  pure fixture {watches=updated,watchChanges=S.insert index (watchChanges fixture)}

copyBase :: LoaderCopy -> GuestAddress
copyBase PrimaryCopy = GuestAddress 0x40000
copyBase SecondaryCopy = GuestAddress 0xd0000

copyRow :: LoaderCopy -> Fixture -> Either String (RowIndex,J.Value,[J.Value])
copyRow selected fixture = do
  watch <- firstIndex "prefix watch absent" (watches fixture)
  rows <- arrayAt [field "records"] watch
  candidates <- forM (zip [0..] rows) $ \(index,row) -> do
    source <- parseAccess row
    physical <- guestAt [field "physical"] row
    size <- numberAt [field "size"] row
    pure (RowIndex index,row,source==ModelBusWrite && physical==copyBase selected && size==131072)
  (index,row,_) <- firstIndex "canonical model loader copy absent" [candidate | candidate@(_,_,True) <- candidates]
  pure (index,row,rows)

firstCpuAfter :: InstructionCount -> [J.Value] -> Either String RowIndex
firstCpuAfter (InstructionCount count) = go 0
  where
    go _ [] = Left "later CPU row absent"
    go index (row:rest) = do
      source <- parseAccess row
      later <- if source `elem` [CpuRead,CpuFetch,CpuWrite] then (>count) <$> numberAt [field "instructions"] row else pure False
      if later then pure (RowIndex index) else go (index+1) rest

copyControl :: CopyChange -> Fixture -> Either String Fixture
copyControl change fixture = do
  let selected = case change of MoveEarlier side -> side; _ -> PrimaryCopy
  (RowIndex index,row,rows) <- copyRow selected fixture
  let remaining = [value | (position,value) <- zip [0..] rows,position/=index]
      replace updated = modifyWatch 0 (setAt [field "records",Element index] updated) fixture
  case change of
    MoveEarlier _ -> modifyWatch 0 (setAt [field "records"] (row:remaining)) fixture
    WrongCopySource -> setAt [field "source"] ("host-write"::String) row >>= replace
    PartialCopy -> setAt [field "size"] (131071::Int) row >>= replace
    MisplacedCopy -> do
      GuestAddress base <- pure (copyBase selected)
      updated <- setGuest [field "address"] (GuestAddress (base+4)) row >>= setGuest [field "physical"] (GuestAddress (base+4))
      replace updated
    ExtraCopy -> modifyWatch 0 (setAt [field "records"] (row:rows)) fixture
    UncertainCopy -> do
      modifyWatch 0 (setAt [field "records"] (remaining++[row])) fixture

writerControl :: WriteBoundary -> Writer -> Fixture -> Either String Fixture
writerControl boundary writer fixture = do
  identifier <- firstEvent fixture (GuestAddress 0x409ac)
  count <- eventAt fixture identifier >>= numberAt [field "instructions"]
  (RowIndex original,_,bootstrap) <- copyRow PrimaryCopy fixture
  startup <- pathAt [field "records"] =<< pathAt [Element 1] (J.toJSON (watches fixture))
  startupRows <- arrayAt [] startup
  let epoch = if boundary==BeforeLoaderEntry then 0 else 1
      rows = if epoch==0 then bootstrap else startupRows
  position <- if boundary==BeforeLoaderEntry then pure original else do RowIndex index <- firstCpuAfter (InstructionCount count) rows;pure (index+1)
  let address = if boundary==DuringLoaderFrame then 0x63450 else 0x409ac :: Word32
      source = case writer of HostWriter -> "host-write"; ModelWriter -> "model-bus-write" :: String
      row = J.object ["source" J..= source,"address" J..= address,"physical" J..= address,"size" J..= (4::Int),"value" J..= J.Null,"core" J..= J.Null,"pc" J..= J.Null,"instructions" J..= J.Null]
      (before,after) = splitAt position rows
  modifyWatch epoch (setAt [field "records"] (before++row:after)) fixture

forgedCut :: Fixture -> Either String Fixture
forgedCut fixture = do
  changed <- copyControl UncertainCopy fixture
  identifier <- firstEvent changed (GuestAddress 0x409ac)
  (RowIndex position,_,_) <- copyRow PrimaryCopy changed
  modifyEvent identifier [] (\value -> case value of
    J.Object object -> Right (J.Object (KM.insert "watch" (J.object ["epoch" J..= ("bootstrap"::String),"rows" J..= (position+1)]) object))
    _ -> Left "event object required") changed

clockControl :: ClockMode -> Fixture -> Either String Fixture
clockControl mode fixture = do
  selected <- safeCpuRows fixture
  epoch <- pathAt [field "epochs",Element (cpuEpoch selected)] (receipt fixture)
  watch <- pathAt [Element (cpuEpoch selected)] (J.toJSON (watches fixture))
  rows <- arrayAt [field "records"] watch
  begin <- numberAt [field "start"] epoch
  end <- numberAt [field "end"] epoch
  first <- firstIndex "CPU write absent" (cpuIndices selected)
  let rowAt index = pathAt [Element index] (J.toJSON rows)
  changed <- if mode==ReversedClock then do
      original <- rowAt first >>= numberAt [field "instructions"]
      clocks <- forM (drop 1 (cpuIndices selected)) $ \index -> do
        count <- rowAt index >>= numberAt [field "instructions"]
        pure (index,count)
      (second,other) <- firstIndex "distinct CPU clock absent" [(index,count) | (index,count) <- clocks,count/=original]
      setAt [Element first,field "instructions"] other (J.toJSON rows) >>= setAt [Element second,field "instructions"] original
    else do
      let index = if mode==AtEnd then last (cpuIndices selected) else first
      count <- case mode of AtBegin -> Right begin; AfterEnd -> Right (end+1); ResumedBegin -> Right (begin+1); AtEnd -> Right end; ReversedClock -> Left "clock permutation is not a point replacement"
      row <- rowAt index >>= setAt [field "instructions"] count
      if mode==ResumedBegin || mode==AtEnd || mode==AfterEnd
        then pure (J.toJSON (if mode==ResumedBegin then row:[value | (position,value) <- zip [0..] rows,position/=index] else [value | (position,value) <- zip [0..] rows,position/=index]++[row]))
        else setAt [Element index] row (J.toJSON rows)
  modifyWatch (cpuEpoch selected) (setAt [field "records"] changed) fixture

wrongPhase :: Fixture -> Either String Fixture
wrongPhase fixture = do
  call <- phaseCall 1 fixture >>= callAt fixture
  before <- eventReference "before" call
  entered <- eventReference "entry" call
  setRegister before R0 4 fixture >>= setRegister entered R0 4

wrongTarget :: Fixture -> Either String Fixture
wrongTarget fixture = do
  index <- phaseCall 2 fixture
  call <- callAt fixture index
  before <- eventReference "before" call
  indices <- callIndices fixture Loader
  references <- forM indices $ \position -> do
    current <- callAt fixture position
    core <- numberAt [field "core"] current
    identifiers <- forM ["before","entry","returned"] $ \name -> do
      value <- pathAt [field name] current
      if value==J.Null then Right [] else (:[]) <$> eventIdValue value
    pure [identifier | identifier <- concat identifiers,core==0 && identifier>=before]
  next <- setCallGuest index "target" (GuestAddress 0x69cdfc) fixture
  changed <- foldM (\value identifier -> setRegister identifier R4 0x69cdfc value) next (S.toList (S.fromList (concat references)))
  entered <- eventReference "entry" call
  changeCode entered (GuestAddress 0x69cdfc) changed

wrongReturn :: Fixture -> Either String Fixture
wrongReturn fixture = do
  call <- phaseCall 2 fixture >>= callAt fixture
  returned <- eventReference "returned" call
  publisher <- loaderService (GuestAddress 0x40c6c) fixture >>= callAt fixture
  publisherBefore <- eventReference "before" publisher
  publisherEntry <- eventReference "entry" publisher
  foldM (\value identifier -> setRegister identifier R0 0x69cc70 value) fixture [returned,publisherBefore,publisherEntry]

wrongSavedPc :: Fixture -> Either String Fixture
wrongSavedPc fixture = do
  identifier <- firstEvent fixture (GuestAddress 0x436e8)
  event <- eventAt fixture identifier
  pointer <- numberAt [field "registers",field "r",Element 1] event
  views <- arrayAt [field "state"] event
  matching <- forM (zip [0..] views) $ \(index,view) -> do
    address <- numberAt [field "mapping",field "guest"] view
    words' <- arrayAt [field "values"] view
    pure (index,view,address==pointer+24 && length words'==2)
  (index,view) <- firstIndex "selected context view absent" [(index,view) | (index,view,True) <- matching]
  value <- numberAt [field "values",Element 1] view
  modifyEvent identifier [field "state",Element index,field "values",Element 1] (const (Right (J.toJSON (value+4)))) fixture

outOfRangeWrite :: Fixture -> Either String Fixture
outOfRangeWrite fixture = do
  selected <- safeCpuRows fixture
  epoch <- pathAt [field "epochs",Element (cpuEpoch selected)] (receipt fixture)
  end <- numberAt [field "end"] epoch
  watch <- pathAt [Element (cpuEpoch selected)] (J.toJSON (watches fixture))
  rows <- arrayAt [field "records"] watch
  row <- pathAt [Element (last (cpuIndices selected))] (J.toJSON rows)
  changed <- setGuest [field "address"] (GuestAddress 0x20000000) row >>= setGuest [field "physical"] (GuestAddress 0x20000000) >>= setAt [field "instructions"] end
  modifyWatch (cpuEpoch selected) (setAt [field "records"] (rows++[changed])) fixture

moveProjection :: Fixture -> Either String Fixture
moveProjection fixture = do
  samples <- arrayAt [field "snapshots"] (receipt fixture)
  matches <- forM (zip [0..] samples) $ \(index,sample) -> do
    resource <- parseResource sample
    event <- eventReference "event" sample >>= eventAt fixture
    address <- guestAt [field "pc"] event
    pure (index,resource==Image5Bss && address==GuestAddress 0xd985a4)
  selected <- firstIndex "first image5 BSS view absent" [index | (index,True) <- matches]
  EventId destination <- firstEvent fixture (GuestAddress 0x6a9840)
  modifyReceipt [field "snapshots",Element selected,field "event"] (const (Right (J.toJSON destination))) fixture

removeBoundary :: GuestAddress -> Fixture -> Either String Fixture
removeBoundary address fixture = do
  events <- arrayAt [field "events"] (receipt fixture)
  addresses <- traverse (guestAt [field "pc"]) events
  let removed = S.fromList [EventId index | (index,pc) <- zip [0..] addresses,pc==address]
  unless (not (S.null removed)) (Left "producer return boundary absent")
  samples <- arrayAt [field "snapshots"] (receipt fixture)
  kept <- forM samples $ \sample -> do
    identifier <- eventReference "event" sample
    pure (sample,S.notMember identifier removed)
  next <- modifyReceipt [field "snapshots"] (const (Right (J.toJSON [sample | (sample,True) <- kept]))) fixture
  removeEvents removed next
loaderService :: GuestAddress -> Fixture -> Either String Int
loaderService target fixture = do
  indices <- callIndices fixture Loader
  matches <- forM indices $ \index -> do
    call <- callAt fixture index
    actual <- guestAt [field "target"] call
    pure (index,actual==target)
  firstIndex "loader service absent" [index | (index,True) <- matches]

loaderControl :: LoaderChange -> Fixture -> Either String Fixture
loaderControl change fixture = do
  index <- loaderService (GuestAddress (if change==WrongPublication then 0x40c6c else 0x434f0)) fixture
  call <- callAt fixture index
  case change of
    PendingCall -> setCall index "returned" J.Null fixture
    WrongCallCore -> setCall index "core" (1::Int) fixture
    WrongOriginalFrame -> do
      identifiers <- traverse (`eventReference` call) ["before","entry","returned"]
      foldM (\next identifier -> modifyEvent identifier [field "frame",field "Ok",field "values",Element 16]
        (\value -> wireNumber value >>= \word -> wordLike (word `xor` 1) value) next) fixture identifiers
    WrongPublication -> do
      returned <- eventReference "returned" call
      event <- eventAt fixture returned
      state <- arrayAt [field "state"] event
      locations <- traverse (numberAt [field "mapping",field "guest"]) state
      view <- firstIndex "callback publication projection absent" [position | (position,start) <- zip [0..] locations,start<=0x4f114 && 0x4f114<start+12]
      start <- numberAt [field "mapping",field "guest"] (state!!view)
      modifyEvent returned [field "state",Element view,field "values",Element (fromInteger ((0x4f114-start) `div` 4))]
        (\value -> wireNumber value >>= \word -> wordLike (word `xor` 4) value) fixture
    WrongLiveInput -> do
      before <- eventReference "before" call
      entered <- eventReference "entry" call
      setRegister before R0 1 fixture >>= setRegister entered R0 1

artifactControl :: ArtifactChange -> Fixture -> Either String Fixture
artifactControl change fixture = do
  let artifact = case change of WrongArtifactHash selected -> selected; ChangeArtifactBytes selected -> selected; RebindArtifactBytes selected -> selected
      path = artifactPath artifact
  case change of
    WrongArtifactHash _ -> modifyReceipt (path++[field "sha256"]) (const (Right (J.toJSON (replicate 64 '0')))) fixture
    _ -> do
      name <- textAt (path++[field "path"]) (receipt fixture)
      bytes <- maybe (Left "bound artifact bytes absent") Right (M.lookup name (fileChanges fixture))
      let offset = case artifact of Contract -> 0; LoaderArtifact _ -> 0x9c0
      unless (offset<B.length bytes) (Left "bound artifact byte absent")
      let changed = B.take offset bytes<>B.singleton (B.index bytes offset `xor` 1)<>B.drop (offset+1) bytes
          updated = fixture {fileChanges=M.insert name changed (fileChanges fixture)}
      case change of
        RebindArtifactBytes _ -> modifyReceipt (path++[field "sha256"]) (const (Right (J.toJSON (SHA.sha256Hex (SHA.sha256 changed))))) updated
        _ -> Right updated

guardControl :: GuardChange -> Fixture -> Either String Fixture
guardControl change fixture = case change of
  WrongEpochStage -> modifyReceipt [field "epochs",Element 1,field "stage"] (const (Right (J.toJSON ("tail"::String)))) fixture
  EpochGap -> modifyReceipt [field "epochs",Element 1,field "start"] (\value -> wireNumber value >>= \word -> wordLike (word+1) value) fixture
  RawObserve -> modifyWatch 1 (setAt [field "views"] ("observe"::String)) fixture
  DeclaredObserve -> modifyWatch 1 (setAt [field "views"] ("observe"::String)) fixture
    >>= modifyReceipt [field "epochs",Element 1,field "views"] (const (Right (J.toJSON ("observe"::String))))
  MissingCarrierFetch -> do
    let remove value = do
          classes <- arrayAt [] value
          updated <- forM classes $ \entry -> do
            span' <- arrayAt [field "range"] entry >>= traverse wireNumber
            if span'==[0x409ac,0x40b00] || span'==[0xd09ac,0xd0b00]
              then modifyAt [field "kinds"] (\kinds -> J.toJSON . filter (/=J.toJSON ("cpu-fetch"::String)) <$> arrayAt [] kinds) entry
              else Right entry
          pure (J.toJSON updated)
    modifyWatch 1 (modifyAt [field "classes"] remove) fixture >>= modifyReceipt [field "epochs",Element 1,field "classes"] remove
  MissingInitialProjection -> omitProjection LoaderA (Just (GuestAddress 0xd09ac)) fixture
  MissingOrigin -> modifyReceipt [field "origin"] (const (Right J.Null)) fixture
  WrongOrigin -> do
    EventId identifier <- firstEvent fixture (GuestAddress 0x409ac)
    modifyReceipt [field "origin"] (const (Right (J.toJSON identifier))) fixture
  FixedViewViolation -> modifyWatch 1 (setAt [field "violation"] (J.object ["kind" J..= ("synthetic-fixed-view-change"::String)])) fixture
  MissingBoundaryFetch -> do
    origin <- eventReference "origin" (receipt fixture) >>= eventAt fixture
    address <- guestAt [field "pc"] origin
    count <- numberAt [field "instructions"] origin
    core <- numberAt [field "core"] origin
    modifyWatch 1 (modifyAt [field "records"] (\value -> do
      rows <- arrayAt [] value
      selected <- forM rows $ \row -> do
        access <- parseAccess row
        remove <- if access/=CpuFetch then Right False else do
          pc <- guestAt [field "address"] row
          clock <- numberAt [field "instructions"] row
          cpu <- numberAt [field "core"] row
          pure (pc==address && clock==count+1 && cpu==core)
        pure (row,not remove)
      unless (any (not . snd) selected) (Left "origin resumed carrier fetch absent")
      pure (J.toJSON [row | (row,True) <- selected]))) fixture

fetchControl :: FetchChange -> Fixture -> Either String Fixture
fetchControl change fixture = do
  startup <- pathAt [Element 1] (J.toJSON (watches fixture))
  rows <- arrayAt [field "records"] startup
  candidates <- forM (zip [0::Int ..] rows) $ \(index,row) -> do
    access <- parseAccess row
    physical <- numberAt [field "physical"] row
    pure (index,row,access==CpuFetch && 0x409ac<=physical && physical<0x40b00)
  (index,row,_) <- firstIndex "startup carrier fetch absent" [candidate | candidate@(_,_,True) <- candidates]
  case change of
    TailFetch -> do
      end <- numberAt [field "epochs",Element 2,field "end"] (receipt fixture)
      updated <- setAt [field "instructions"] end row
      modifyWatch 2 (modifyAt [field "records"] (\value -> do existing <- arrayAt [] value;pure (J.toJSON (existing++[updated])))) fixture
    _ -> do
      let setPc address = setGuest [field "address"] (GuestAddress address) row >>= setGuest [field "pc"] (GuestAddress address) >>= setGuest [field "physical"] (GuestAddress (address .&. complement 0x40000000))
      updated <- case change of
        WrongFetchCore -> setAt [field "core"] (1::Int) row
        WrongFetchPhysical -> do
          GuestAddress address <- guestAt [field "physical"] row
          setGuest [field "physical"] (GuestAddress (address+4)) row
        UnsupportedFetch -> setPc 0x409ad
        AliasFetch -> setPc 0x400409ac
        OwnedPrefixFetch -> setPc 0x409b8
        OwnedDataFetch -> setPc 0x40ab8
        OwnedPaddingFetch -> setPc 0x40ac4
      modifyWatch 1 (setAt [field "records",Element index] updated) fixture

timerCodeWrite :: Access -> CodeWriteStyle -> Fixture -> Either String Fixture
timerCodeWrite access style fixture = do
  source <- case access of
    CpuWrite -> Right ("cpu-write"::String)
    HostWrite -> Right "host-write"
    ModelBusWrite -> Right "model-bus-write"
    _ -> Left "code-write control requires a writer"
  identifier <- firstEvent fixture (GuestAddress 0x41ae8)
  canonical <- eventAt fixture identifier >>= numberAt [field "code",field "values",Element 0]
  modifyWatch 1 (modifyAt [field "records"] (\value -> do
    rows <- arrayAt [] value
    selected <- forM (zip [0::Int ..] rows) $ \(index,row) -> do
      kind <- parseAccess row
      selectedPc <- if kind==CpuFetch then (==GuestAddress 0x41ae8) <$> guestAt [field "pc"] row else Right False
      pure (index,row,selectedPc)
    (index,row,_) <- firstIndex "timer source fetch absent" [candidate | candidate@(_,_,True) <- selected]
    stored <- setAt [field "source"] source row >>= setAt [field "value"] canonical
    writes <- case style of
      IdenticalWord -> Right [stored]
      RestoredWord -> do changed <- setAt [field "value"] (canonical `xor` 1) stored;Right [changed,stored]
    let (before,after) = splitAt index rows
    Right (J.toJSON (before++writes++after)))) fixture

timerControl :: TimerChange -> Fixture -> Either String Fixture
timerControl change fixture = case change of
  TimerBinding -> modifyReceipt [field "timer_transactions",field "sha256"] (const (Right (J.toJSON (replicate 64 '0')))) fixture
  TimerSchema -> modifyTimer (setAt [field "batch",field "omitted"] ("invalid"::String)) fixture
  TimerBatch -> modifyTimer (setAt [field "batch"] J.Null) fixture
  TimerError -> modifyTimer (setAt [field "error"] ("synthetic device observation failure"::String)) fixture
  TimerOmitted -> modifyTimer (setAt [field "batch",field "omitted"] (1::Int)) fixture
  TimerWindow -> modifyTimer (modifyAt [field "start"] increment) fixture
  TimerRange -> modifyTimer (setAt [field "range"] ([0xfff91000,0xfff91004]::[Word64])) fixture
  TimerLimit -> modifyTimer (setAt [field "limit"] (1::Int)) fixture
  TimerCpuCodeWrite -> timerCodeWrite CpuWrite IdenticalWord fixture
  TimerHostCodeWrite -> timerCodeWrite HostWrite IdenticalWord fixture
  TimerModelCodeWrite -> timerCodeWrite ModelBusWrite IdenticalWord fixture
  TimerRestoredCodeWrite -> timerCodeWrite HostWrite RestoredWord fixture
  TimerSourceCore -> do
    identifier <- firstEvent fixture (GuestAddress 0x41ad4)
    modifyEvent identifier [field "core"] flipBit fixture >>= syncEvent identifier
  TimerSourceChronology -> do
    identifier <- firstEvent fixture (GuestAddress 0x41adc)
    modifyEvent identifier [field "instructions"] (\value -> wireNumber value >>= \word -> wordLike (word-1) value) fixture >>= syncEvent identifier
  TimerSourceInstruction -> do
    identifier <- firstEvent fixture (GuestAddress 0x41ad8)
    modifyEvent identifier [field "code",field "values",Element 0] flipBit fixture
  TimerOrder -> modifyTimer (modifyAt [field "batch",field "transactions"] (\value -> J.toJSON . reverse <$> arrayAt [] value)) fixture
  TimerCpu -> readField "cpu" flipBit
  TimerPc -> readField "pc" (wordLike 0x41ad4)
  TimerAddress -> readField "address" (wordLike 0xfff91000)
  TimerSize -> readField "size" (const (Right (J.toJSON (2::Int))))
  TimerOperation -> readField "operation" (const (Right (J.toJSON ("write"::String))))
  TimerAccepted -> readField "accepted" (const (Right (J.Bool False)))
  TimerTick -> do
    index <- timerReadIndex fixture
    modifyTimer (modifyAt [field "batch",field "transactions"] (\value -> do
      changed <- modifyAt [Element index,field "tick"] flipBit value
      rows <- arrayAt [] changed
      timed <- traverse (\row -> do tick <- numberAt [field "tick"] row;pure (tick,row)) rows
      pure (J.toJSON (map snd (sortOn fst timed))))) fixture
  TimerValue -> readField "value" flipBit
  TimerMissingRead -> do
    index <- timerReadIndex fixture
    modifyTimer (modifyAt [field "batch",field "transactions"] (\value -> do
      rows <- arrayAt [] value
      pure (J.toJSON [row | (position,row) <- zip [0::Int ..] rows,position/=index]))) fixture
  TimerFetchTick -> changeRawTimerRow CpuFetch (GuestAddress 0x41ad8) "tick" flipBit fixture
  TimerSubtraction -> firstEvent fixture (GuestAddress 0x41ae4) >>= \identifier -> setRegisterWord identifier 2 (`xor` 1) fixture
  TimerRingState -> do
    identifier <- firstEvent fixture (GuestAddress 0x41ae8)
    base <- eventAt fixture identifier >>= numberAt [field "registers",field "r",Element 3]
    changeStateWord identifier (GuestAddress (fromInteger (base+0x40))) fixture
  TimerRingWrite -> changeRawTimerRow CpuWrite (GuestAddress 0x41ae4) "value" flipBit fixture
  TimerRingWriteTick -> changeRawTimerRow CpuWrite (GuestAddress 0x41ae4) "tick" flipBit fixture
  TimerCopyRegister -> firstEvent fixture (GuestAddress 0x41b7c) >>= \identifier -> setRegisterWord identifier 2 (`xor` 1) fixture
  TimerCopyState -> firstEvent fixture (GuestAddress 0x41b80) >>= \identifier -> changeStateWord identifier (GuestAddress 0xa0830) fixture
  TimerCopyWrite -> changeRawTimerRow CpuWrite (GuestAddress 0x41b7c) "value" flipBit fixture
  TimerCopyWriteTick -> changeRawTimerRow CpuWrite (GuestAddress 0x41b7c) "tick" flipBit fixture
  TimerLifetime -> do
    events <- arrayAt [field "events"] (receipt fixture)
    identifier <- firstIndex "late elapsed projection absent" (reverse [EventId index | (index,_) <- zip [0..] events])
    changeStateWord identifier (GuestAddress 0xa0830) fixture
  where
    increment value = wireNumber value >>= \word -> wordLike (word+1) value
    flipBit value = wireNumber value >>= \word -> wordLike (word `xor` 1) value
    readField name mutation = do
      index <- timerReadIndex fixture
      modifyTimer (modifyAt [field "batch",field "transactions",Element index,field name] mutation) fixture

timerControlName :: TimerChange -> String
timerControlName change = "timer-"++case change of
  TimerBinding -> "wrong-binding-hash"; TimerSchema -> "malformed-transactions"; TimerBatch -> "missing-batch"; TimerError -> "capture-error"
  TimerOmitted -> "omitted-transactions"; TimerWindow -> "wrong-startup-window"; TimerRange -> "wrong-device-range"; TimerLimit -> "wrong-capture-limit"
  TimerOrder -> "reversed-transactions"; TimerCpu -> "reload-wrong-core"; TimerPc -> "reload-wrong-source-pc"
  TimerAddress -> "reload-wrong-address"; TimerSize -> "reload-wrong-size"; TimerOperation -> "reload-wrong-operation"
  TimerAccepted -> "reload-unaccepted"; TimerTick -> "reload-wrong-tick"; TimerValue -> "reload-wrong-value"; TimerMissingRead -> "missing-reload-read"
  TimerFetchTick -> "reload-wrong-source-fetch-tick"
  TimerSubtraction -> "wrong-subtraction-result"; TimerRingState -> "wrong-ring-word"; TimerRingWrite -> "wrong-raw-ring-write"
  TimerCopyRegister -> "wrong-copy-register"; TimerCopyState -> "wrong-latched-word"; TimerCopyWrite -> "wrong-raw-latched-write"; TimerLifetime -> "wrong-latched-lifetime"
  TimerRingWriteTick -> "wrong-raw-ring-write-tick"; TimerCopyWriteTick -> "wrong-raw-latched-write-tick"
  TimerSourceCore -> "wrong-source-core"; TimerSourceChronology -> "wrong-source-chronology"; TimerSourceInstruction -> "wrong-source-instruction"
  TimerCpuCodeWrite -> "same-value-source-cpu-write"; TimerHostCodeWrite -> "same-value-source-host-write"
  TimerModelCodeWrite -> "same-value-source-model-write"; TimerRestoredCodeWrite -> "source-write-then-restore"

timerReason :: TimerChange -> String
timerReason change = case change of
  TimerBinding -> "timer-transactions identity differs"
  TimerSchema -> "timer-transactions schema differs"
  TimerBatch -> coverage; TimerError -> coverage; TimerOmitted -> coverage; TimerWindow -> coverage; TimerRange -> coverage; TimerLimit -> coverage
  TimerOrder -> "timer-transactions order differs"
  TimerCpu -> readReason; TimerPc -> readReason; TimerAddress -> readReason; TimerSize -> readReason; TimerOperation -> readReason
  TimerAccepted -> readReason; TimerTick -> readReason; TimerValue -> readReason; TimerMissingRead -> readReason; TimerFetchTick -> readReason
  TimerSubtraction -> "timer-ring provenance"; TimerRingState -> "timer-ring provenance"
  TimerRingWrite -> writeReason; TimerRingWriteTick -> writeReason; TimerCopyWrite -> writeReason; TimerCopyWriteTick -> writeReason
  TimerCopyRegister -> "timer-copy provenance"; TimerCopyState -> "timer-copy provenance"
  TimerLifetime -> "timer-state provenance"
  TimerSourceCore -> "timer-source provenance"; TimerSourceChronology -> "timer-source provenance"
  TimerSourceInstruction -> "entered source instruction differs"
  TimerCpuCodeWrite -> "timer-source code write"; TimerHostCodeWrite -> "timer-source code write"
  TimerModelCodeWrite -> "timer-source code write"; TimerRestoredCodeWrite -> "timer-source code write"
  where
    coverage = "timer-transactions coverage differs"
    readReason = "timer-read provenance"
    writeReason = "timer-write provenance"

data Control = ReorderCallRecords | WrongPhase | WrongTarget | WrongReturnedCode | WrongSavedPc | OutOfRangeWrite
  | MissingTask | MissingHook | SyntheticDeferred | MissingDeferred | PhaseRawOrder | ConstructorRawOrder
  | MissingLoaderBss | MissingFirstImage | MissingFirstBss | MissingFirstPlain | MisplacedProjection
  | MissingImage5Return | MissingPlainReturn
  | MissingTrampoline | WrongCreationArgument | WrongCreationTarget | WrongCleanup
  | CpuClock ClockMode | TcmMisaligned | TcmPastEnd | TcmExactEnd | MissingExtent PlainExtent | MissingObservedBss GuestAddress
  | LoaderCopyControl CopyChange | UntimedWriter WriteBoundary Writer | ForgedEntryCut
  | ArtifactControl ArtifactChange | FetchControl FetchChange | GuardControl GuardChange | LoaderControl LoaderChange | TimerControl TimerChange

controlName :: Control -> String
controlName control = case control of
  TimerControl change -> timerControlName change
  ReorderCallRecords -> "call-record-order-is-not-execution-order"; WrongPhase -> "wrong-phase-argument"
  WrongTarget -> "wrong-dispatch-target"; WrongReturnedCode -> "wrong-returned-code"; WrongSavedPc -> "wrong-saved-pc"
  OutOfRangeWrite -> "out-of-range-write"; MissingTask -> "missing-task-call"; MissingHook -> "missing-plain-hook-call"
  SyntheticDeferred -> "synthetic-deferred-call"; MissingDeferred -> "missing-deferred-call"
  PhaseRawOrder -> "phase-raw-order"; ConstructorRawOrder -> "constructor-raw-order"
  MissingLoaderBss -> "missing-loader-return-bss"; MissingFirstImage -> "missing-first-image5-image"
  MissingImage5Return -> "missing-image5-return-and-projections"; MissingPlainReturn -> "missing-plain-return-and-projections"
  MissingFirstBss -> "missing-first-image5-bss"; MissingFirstPlain -> "missing-first-image5-plain"
  MisplacedProjection -> "projection-attached-to-wrong-boundary"; MissingTrampoline -> "missing-trampoline-transfer"
  WrongCreationArgument -> "wrong-trampoline-argument"; WrongCreationTarget -> "wrong-trampoline-target"; WrongCleanup -> "wrong-trampoline-cleanup"
  CpuClock mode -> case mode of AtBegin -> "cpu-clock-at-begin"; AfterEnd -> "cpu-clock-after-end"; ReversedClock -> "cpu-clock-reversal"; AtEnd -> "cpu-clock-at-end"; ResumedBegin -> "cpu-clock-resumed-begin-plus-one"
  TcmMisaligned -> "misaligned-tcm-window"; TcmPastEnd -> "tcm-window-past-address-space"; TcmExactEnd -> "tcm-window-exact-address-space-end"
  MissingExtent extent -> "missing-extent-"++resourceText (extentResource extent)
  MissingObservedBss (GuestAddress address) -> "missing-observed-bss-"++showHex address ""
  LoaderCopyControl change -> case change of MoveEarlier PrimaryCopy -> "earlier-exact-loader-a-copy"; MoveEarlier SecondaryCopy -> "earlier-exact-loader-b-copy"; WrongCopySource -> "loader-copy-wrong-source"; PartialCopy -> "loader-copy-partial-extent"; MisplacedCopy -> "loader-copy-misplaced-extent"; ExtraCopy -> "loader-copy-duplicate"; UncertainCopy -> "loader-copy-ambiguous-entry-clock"
  ForgedEntryCut -> "forged-entry-cut-over-model-copy"
  UntimedWriter boundary writer -> (case boundary of BeforeLoaderEntry -> "before-loader-entry"; AfterLoaderEntry -> "after-loader-entry"; DuringLoaderFrame -> "during-live-loader-frame")++(case writer of HostWriter -> "-host-write"; ModelWriter -> "-model-bus-write")
  ArtifactControl change -> case change of
    WrongArtifactHash artifact -> artifactName artifact++"-wrong-binding-hash"
    ChangeArtifactBytes artifact -> artifactName artifact++"-changed-file-bytes"
    RebindArtifactBytes artifact -> artifactName artifact++"-rebound-changed-file"
  LoaderControl change -> case change of
    PendingCall -> "pending-loader-service"; WrongCallCore -> "wrong-loader-service-core"
    WrongOriginalFrame -> "equally-forged-loader-saved-frame"; WrongPublication -> "wrong-committed-loader-callback"
    WrongLiveInput -> "wrong-loader-service-live-input"
  GuardControl change -> case change of
    WrongEpochStage -> "wrong-startup-epoch-stage"; EpochGap -> "gap-before-startup-epoch"
    RawObserve -> "startup-raw-watch-observe"; DeclaredObserve -> "startup-declared-watch-observe"
    MissingCarrierFetch -> "startup-missing-carrier-fetch-coverage"; MissingInitialProjection -> "missing-initial-peer-entry-loader-a-projection"
    MissingOrigin -> "missing-first-carrier-origin"; WrongOrigin -> "primary-carrier-forged-as-origin"
    FixedViewViolation -> "fixed-view-decoder-change-refusal"
    MissingBoundaryFetch -> "startup-missing-observed-carrier-fetch"
  FetchControl change -> case change of
    TailFetch -> "tail-loader-carrier-fetch"; WrongFetchCore -> "startup-loader-fetch-wrong-core"
    WrongFetchPhysical -> "startup-loader-fetch-wrong-physical"; UnsupportedFetch -> "startup-loader-fetch-unaligned"
    AliasFetch -> "startup-loader-fetch-high-alias"; OwnedPrefixFetch -> "owned-compiler-prefix-fetch"
    OwnedDataFetch -> "owned-literal-data-fetch"; OwnedPaddingFetch -> "owned-padding-fetch"

artifactName :: Artifact -> String
artifactName Contract = "owned-contract"
artifactName (LoaderArtifact PrimaryCopy) = "owned-loader-a"
artifactName (LoaderArtifact SecondaryCopy) = "owned-loader-b"

controlReason :: Control -> Maybe String
controlReason control | controlVerdict control==Admit = Nothing
controlReason control = Just (case control of
  TimerControl change -> timerReason change
  WrongPhase -> "phase argument or target differs"
  WrongTarget -> "phase argument or target differs"
  WrongReturnedCode -> "dispatcher result differs"
  WrongSavedPc -> "first loaded context differs"
  OutOfRangeWrite -> "raw access lies outside its declared classes"
  MissingTask -> "observed task entry was omitted or repeated"
  MissingHook -> "observed source call was omitted or repeated"
  MissingDeferred -> "observed source call was omitted or repeated"
  PhaseRawOrder -> "phase argument or target differs"
  ConstructorRawOrder -> "image5 reverse constructor order differs"
  MissingImage5Return -> "required producer return boundary absent"
  MissingPlainReturn -> "required producer return boundary absent"
  MisplacedProjection -> "required boundary/resource projection absent or repeated"
  MissingTrampoline -> "initial C-entry transfer absent"
  WrongCreationArgument -> "C-record argument, caller or cleanup context differs"
  WrongCreationTarget -> "initial C function entry absent"
  WrongCleanup -> "C-record argument, caller or cleanup context differs"
  CpuClock _ -> "CPU access attribution or clock differs"
  TcmMisaligned -> "projection TCM window has invalid alignment or endpoint"
  TcmPastEnd -> "projection TCM window has invalid alignment or endpoint"
  LoaderCopyControl UncertainCopy -> "loader carrier write is not a proven earlier canonical copy"
  LoaderCopyControl _ -> "canonical loader-copy census differs"
  ForgedEntryCut -> "loader carrier write is not a proven earlier canonical copy"
  UntimedWriter DuringLoaderFrame _ -> "untimed writer could reach the live saved loader frame"
  UntimedWriter _ _ -> "loader carrier write is not a proven earlier canonical copy"
  ArtifactControl (RebindArtifactBytes Contract) -> "owned contract is not the reviewed staged artifact"
  ArtifactControl (RebindArtifactBytes _) -> "owned linked/staged identity differs"
  ArtifactControl _ -> "owned artifact file identity differs"
  LoaderControl PendingCall -> "a required retained call has not returned"
  LoaderControl WrongCallCore -> "callee entry context differs"
  LoaderControl WrongOriginalFrame -> "loader saved frame differs from entry"
  LoaderControl WrongPublication -> "committed diagnostic callback differs"
  LoaderControl WrongLiveInput -> "loader service live-in differs"
  GuardControl RawObserve -> "raw watch view policy differs"
  GuardControl DeclaredObserve -> "raw watch view policy differs"
  GuardControl MissingCarrierFetch -> "loader carrier watch coverage absent"
  GuardControl WrongEpochStage -> "classified watch interval differs"
  GuardControl EpochGap -> "classified watch interval differs"
  GuardControl MissingOrigin -> "loader origin absent"
  GuardControl WrongOrigin -> "first carrier entry differs"
  GuardControl FixedViewViolation -> "classified watch is incomplete"
  GuardControl MissingBoundaryFetch -> "observed loader boundary lacks raw carrier fetch"
  FetchControl TailFetch -> "loader continuation executes after the admitted prefix"
  FetchControl WrongFetchCore -> "loader carrier fetch attribution differs"
  FetchControl WrongFetchPhysical -> "loader carrier fetch attribution differs"
  FetchControl UnsupportedFetch -> "loader carrier fetch attribution differs"
  FetchControl AliasFetch -> "loader carrier fetch attribution differs"
  FetchControl _ -> "loader carrier execution point is not admitted"
  _ -> "resource projection census differs")

controlVerdict :: Control -> Verdict
controlVerdict control = case control of
  ReorderCallRecords -> Admit; SyntheticDeferred -> Admit; CpuClock AtEnd -> Admit; CpuClock ResumedBegin -> Admit; TcmExactEnd -> Admit; LoaderCopyControl (MoveEarlier _) -> Admit; _ -> Refuse

controlOrigin :: Control -> Origin
controlOrigin control = case control of
  SyntheticDeferred -> SerializedSynthetic; MissingDeferred -> SerializedSynthetic; PhaseRawOrder -> SerializedSynthetic
  ConstructorRawOrder -> SerializedSynthetic; CpuClock AtEnd -> SerializedSynthetic; CpuClock ResumedBegin -> SerializedSynthetic
  TcmExactEnd -> SerializedSynthetic; WrongTarget -> SerializedSynthetic; WrongReturnedCode -> SerializedSynthetic
  OutOfRangeWrite -> SerializedSynthetic; UntimedWriter _ _ -> SerializedSynthetic; ForgedEntryCut -> SerializedSynthetic
  LoaderCopyControl (MoveEarlier _) -> SerializedSynthetic; LoaderCopyControl ExtraCopy -> SerializedSynthetic; LoaderCopyControl UncertainCopy -> SerializedSynthetic
  FetchControl _ -> SerializedSynthetic; GuardControl DeclaredObserve -> SerializedSynthetic
  GuardControl FixedViewViolation -> SerializedSynthetic; GuardControl WrongOrigin -> SerializedSynthetic
  LoaderControl WrongOriginalFrame -> SerializedSynthetic; LoaderControl WrongLiveInput -> SerializedSynthetic
  TimerControl _ -> SerializedSynthetic
  ArtifactControl (RebindArtifactBytes _) -> SerializedSynthetic; _ -> ActualMutation

originText :: Origin -> String
originText ActualMutation = "actual-observation-mutation"
originText SerializedSynthetic = "serialized-synthetic-from-actual"

controls :: Fixture -> Either String [Control]
controls fixture = do
  events <- arrayAt [field "events"] (receipt fixture)
  addresses <- traverse (guestAt [field "pc"]) events
  pure ([ReorderCallRecords,WrongPhase,WrongTarget,WrongReturnedCode,WrongSavedPc,OutOfRangeWrite,MissingTask,MissingHook,SyntheticDeferred,MissingDeferred,PhaseRawOrder,ConstructorRawOrder,MissingLoaderBss,MissingFirstImage,MissingFirstBss,MissingFirstPlain,MisplacedProjection,MissingTrampoline,WrongCreationArgument,WrongCreationTarget,WrongCleanup,CpuClock AtBegin,CpuClock AfterEnd,CpuClock ReversedClock,CpuClock AtEnd,CpuClock ResumedBegin,TcmMisaligned,TcmPastEnd,TcmExactEnd,MissingExtent Extent0,MissingExtent Extent2,MissingExtent Extent3]
    ++[MissingImage5Return,MissingPlainReturn]
    ++map LoaderCopyControl [MoveEarlier PrimaryCopy,MoveEarlier SecondaryCopy,WrongCopySource,PartialCopy,MisplacedCopy,ExtraCopy,UncertainCopy]
    ++[ForgedEntryCut]
    ++[UntimedWriter boundary writer | boundary <- [BeforeLoaderEntry,AfterLoaderEntry,DuringLoaderFrame],writer <- [HostWriter,ModelWriter]]
    ++[MissingObservedBss address | address <- [GuestAddress 0x6a8c04,GuestAddress 0x6a9734],address `elem` addresses]
    ++map LoaderControl [PendingCall,WrongCallCore,WrongOriginalFrame,WrongPublication,WrongLiveInput]
    ++map GuardControl [WrongEpochStage,EpochGap,RawObserve,DeclaredObserve,MissingCarrierFetch,MissingInitialProjection,MissingOrigin,WrongOrigin,FixedViewViolation,MissingBoundaryFetch]
    ++map FetchControl [TailFetch,WrongFetchCore,WrongFetchPhysical,UnsupportedFetch,AliasFetch]
    ++map TimerControl [minBound..maxBound]
    ++if programRole fixture==Owned then
      [ArtifactControl change | artifact <- [Contract,LoaderArtifact PrimaryCopy,LoaderArtifact SecondaryCopy],change <- [WrongArtifactHash artifact,ChangeArtifactBytes artifact,RebindArtifactBytes artifact]]
      ++map FetchControl [OwnedPrefixFetch,OwnedDataFetch,OwnedPaddingFetch]
      else [])

applyControl :: Control -> Fixture -> Either String Fixture
applyControl control fixture = case control of
  ReorderCallRecords -> modifyReceipt [field "calls"] (\value -> J.toJSON . reverse <$> arrayAt [] value) fixture
  WrongPhase -> wrongPhase fixture; WrongTarget -> wrongTarget fixture; WrongReturnedCode -> wrongReturn fixture
  WrongSavedPc -> wrongSavedPc fixture; OutOfRangeWrite -> outOfRangeWrite fixture
  MissingTask -> omitCall Task fixture; MissingHook -> omitCall PlainHook fixture
  SyntheticDeferred -> makeDeferred fixture; MissingDeferred -> makeDeferred fixture >>= omitCall Deferred
  PhaseRawOrder -> phaseOrder fixture; ConstructorRawOrder -> constructorOrder fixture
  MissingLoaderBss -> omitProjection Image5Bss (Just (GuestAddress 0x6a9840)) fixture
  MissingImage5Return -> removeBoundary (GuestAddress 0x6a9840) fixture
  MissingPlainReturn -> removeBoundary (GuestAddress 0x6a9934) fixture
  MissingFirstImage -> omitProjection Image5 (Just (GuestAddress 0xd985a4)) fixture
  MissingFirstBss -> omitProjection Image5Bss (Just (GuestAddress 0xd985a4)) fixture
  MissingFirstPlain -> omitProjection Plain (Just (GuestAddress 0xd985a4)) fixture
  MisplacedProjection -> moveProjection fixture
  MissingTrampoline -> firstEvent fixture (GuestAddress 0x43758) >>= \identifier -> removeEvents (S.singleton identifier) fixture
  WrongCreationArgument -> changeCreation CreationArgument (`xor` 1) fixture
  WrongCreationTarget -> changeCreation CreationTarget (+4) fixture
  WrongCleanup -> do
    identifier <- firstEvent fixture (GuestAddress 0x43758)
    event <- eventAt fixture identifier
    GuestAddress address <- guestAt [field "registers",field "lr"] event
    setLink identifier (GuestAddress (address+4)) fixture
  CpuClock mode -> clockControl mode fixture
  TcmMisaligned -> setTcm (TcmRegisterWord 0x00201011)
  TcmPastEnd -> setTcm (TcmRegisterWord 0xfffff039)
  TcmExactEnd -> setTcm (TcmRegisterWord 0xff800039)
  MissingExtent extent -> omitProjection (extentResource extent) Nothing fixture
  MissingObservedBss address -> omitProjection Image5Bss (Just address) fixture
  LoaderCopyControl change -> copyControl change fixture
  ForgedEntryCut -> forgedCut fixture
  UntimedWriter boundary writer -> writerControl boundary writer fixture
  ArtifactControl change -> artifactControl change fixture
  FetchControl change -> fetchControl change fixture
  GuardControl change -> guardControl change fixture
  LoaderControl change -> loaderControl change fixture
  TimerControl change -> timerControl change fixture
  where setTcm (TcmRegisterWord bank) = modifyReceipt [field "events",Element 0,field "code",field "mapping",field "tcm",Element 0,Element 0] (const (Right (J.toJSON bank))) fixture

emit :: FilePath -> J.Value -> IO SHA.Sha256
emit path value = do
  exists <- doesPathExist path
  H.ensure (not exists) "control output already exists"
  let bytes = encodeCanonical value
  B.writeFile path bytes
  pure (SHA.sha256 bytes)

writeFixture :: FilePath -> Fixture -> IO ()
writeFixture output fixture = do
  createDirectory output
  traceName <- either H.failH pure (textAt [field "trace",field "path"] (receipt fixture))
  epochs <- either H.failH pure (arrayAt [field "epochs"] (receipt fixture))
  timerName <- either H.failH pure (textAt [field "timer_transactions",field "path"] (receipt fixture))
  names <- either H.failH pure (traverse (textAt [field "file",field "path"]) epochs)
  let excluded = S.fromList (["continuations.json","verification.json","observer-error.json",traceName,timerName]++names++M.keys (fileChanges fixture))
  entries <- listDirectory (originalRoot fixture)
  forM_ entries $ \name -> unless (S.member name excluded) $ do
    status <- getSymbolicLinkStatus (originalRoot fixture </> name)
    if isRegularFile status then createLink (originalRoot fixture </> name) (output </> name) else pure ()
  forM_ (M.toList (fileChanges fixture)) $ \(name,bytes) -> B.writeFile (output </> name) bytes
  document <- if traceChanged fixture then do
    traceHash <- emit (output </> "control-trace.json") (rawTrace fixture)
    either H.failH pure (setAt [field "trace"] (J.object ["path" J..= ("control-trace.json"::String),"sha256" J..= SHA.sha256Hex traceHash]) (receipt fixture))
    else do
      createLink (originalRoot fixture </> traceName) (output </> traceName)
      pure (receipt fixture)
  H.ensure (length epochs==length (watches fixture)) "epoch/watch cardinality differs"
  updated <- foldM (\value (index,watch,originalName) ->
    if S.notMember index (watchChanges fixture) then do
      createLink (originalRoot fixture </> originalName) (output </> originalName)
      pure value
    else do
      let name = "control-watch-"++show index++".json"
      digest <- emit (output </> name) watch
      count <- either H.failH pure (length <$> arrayAt [field "records"] watch)
      either H.failH pure (setAt [field "epochs",Element index,field "file"] (J.object ["path" J..= name,"sha256" J..= SHA.sha256Hex digest]) value >>= setAt [field "epochs",Element index,field "record_count"] count))
    document (zip3 [0..] (watches fixture) names)
  withTimer <- if timerChanged fixture then do
    digest <- emit (output </> "control-timer-transactions.json") (timerTransactions fixture)
    either H.failH pure (setAt [field "timer_transactions"] (J.object ["path" J..= ("control-timer-transactions.json"::String),"sha256" J..= SHA.sha256Hex digest]) updated)
    else do
      createLink (originalRoot fixture </> timerName) (output </> timerName)
      pure updated
  _ <- emit (output </> "continuations.json") withTimer
  pure ()

data ProcessResult = ProcessResult
  { processTimedOut :: Bool, processExit :: ExitCode, processStdout :: B.ByteString, processStderr :: B.ByteString }

runObserver :: Observer -> [String] -> FilePath -> IO ProcessResult
runObserver (Observer observer) options output = do
  (timedOut,code,stdoutBytes,stderrBytes) <- H.runBounded 120000000 1048576 observer options B.empty
  B.writeFile (output <.> "stdout") stdoutBytes
  B.writeFile (output <.> "stderr") stderrBytes
  pure (ProcessResult timedOut code stdoutBytes stderrBytes)

invoke :: Observer -> AnalysisRoot -> FilePath -> FilePath -> IO ProcessResult
invoke observer (AnalysisRoot analysis) fixture =
  runObserver observer ["--verify",fixture,"--analysis",analysis]

exitNumber :: ExitCode -> Int
exitNumber ExitSuccess = 0
exitNumber (ExitFailure number) = number

expectedExit :: Verdict -> ExitCode
expectedExit Admit = ExitSuccess
expectedExit Refuse = ExitFailure 1

refusalMatches :: String -> ProcessResult -> Bool
refusalMatches reason actual = TE.encodeUtf8 (T.pack reason) `B.isInfixOf` (processStdout actual<>processStderr actual)

verificationMatches :: SHA.Sha256 -> ProcessResult -> Bool
verificationMatches digest actual = case decodeStrictIntegers (processStdout actual) of
  Left _ -> False
  Right verified -> numberAt [field "schema"] verified==Right 4
    && textAt [field "receipt"] verified==Right (SHA.sha256Hex digest)

resultPassed :: Verdict -> Maybe String -> Bool -> ProcessResult -> Bool
resultPassed verdict reason observation actual = not (processTimedOut actual)
  && processExit actual==expectedExit verdict
  && case reason of Just expected -> refusalMatches expected actual; Nothing -> observation

runSuite :: Observer -> AnalysisRoot -> ObservationDirectory -> FilePath -> IO J.Value
runSuite observer@(Observer executable) analysis@(AnalysisRoot sources) (ObservationDirectory original) output = do
  createDirectory output
  baselineHash <- SHA.sha256File (original </> "continuations.json")
  baseline <- invoke observer analysis original (output </> "baseline")
  let baselinePassed = resultPassed Admit Nothing (verificationMatches baselineHash baseline) baseline
  results <- if not baselinePassed then pure [] else do
    fixture <- loadFixture original sources
    selected <- either H.failH pure (controls fixture)
    forM selected $ \control -> do
      let name = controlName control
          verdict = controlVerdict control
          reason = controlReason control
          baseFields = ["name" J..= name,"scope" J..= originText (controlOrigin control),"expected_exit" J..= exitNumber (expectedExit verdict),"expected_refusal" J..= reason]
      result <- case applyControl control fixture of
        Left problem -> pure (J.object (baseFields++["actual_exit" J..= J.Null,"preparation_error" J..= problem,"passed" J..= False]))
        Right mutated -> do
          let directory = output </> name
          writeFixture directory mutated
          digest <- SHA.sha256File (directory </> "continuations.json")
          actual <- invoke observer analysis directory (output </> name)
          pure (J.object (baseFields++["actual_exit" J..= exitNumber (processExit actual),"timed_out" J..= processTimedOut actual
            ,"observation_matched" J..= verificationMatches digest actual
            ,"refusal_matched" J..= maybe False (`refusalMatches` actual) reason
            ,"passed" J..= resultPassed verdict reason (verificationMatches digest actual) actual]))
      _ <- emit (output </> name <.> "result.json") result
      pure result
  own <- getExecutablePath
  observerHash <- SHA.sha256File executable
  controlsHash <- SHA.sha256File own
  let passed = baselinePassed && all casePassed results
      manifest = J.object ["schema" J..= (3::Int),"baseline_exit" J..= exitNumber (processExit baseline),"baseline_timed_out" J..= processTimedOut baseline
        ,"baseline_passed" J..= baselinePassed,"observer_sha256" J..= SHA.sha256Hex observerHash,"controls_executable_sha256" J..= SHA.sha256Hex controlsHash
        ,"baseline_receipt_sha256" J..= SHA.sha256Hex baselineHash,"cases" J..= results,"passed" J..= passed]
  _ <- emit (output </> "controls.json") manifest
  pure manifest

casePassed :: J.Value -> Bool
casePassed value = pathAt [field "passed"] value==Right (J.Bool True)

data PairControl = NaturalPair | ReorderedPair | SwappedPair | PhasePair | FramePair | PublicationPair
  | MissingRawCallPair | LateWritePair | DescriptorPair | UnprovenElapsedPair | UnrelatedHeaderPair deriving Eq

pairName :: PairControl -> String
pairName control = case control of
  NaturalPair -> "paired-natural-observations"
  ReorderedPair -> "paired-reordered-owned-call-records"
  SwappedPair -> "paired-swapped-program-roles"
  PhasePair -> "paired-owned-wrong-phase"
  FramePair -> "paired-owned-forged-original-frame"
  PublicationPair -> "paired-owned-forged-callback-publication"
  MissingRawCallPair -> "paired-owned-missing-raw-call"
  LateWritePair -> "paired-owned-late-publication-effect-mismatch"
  DescriptorPair -> "paired-owned-task-descriptor-effect-mismatch"
  UnprovenElapsedPair -> "paired-owned-elapsed-without-source-relation"
  UnrelatedHeaderPair -> "paired-owned-unrelated-header-word-effect-mismatch"

pairReason :: PairControl -> Maybe String
pairReason control = case control of
  NaturalPair -> Nothing
  ReorderedPair -> Nothing
  SwappedPair -> Just "paired observation roles differ"
  PhasePair -> controlReason WrongPhase
  FramePair -> controlReason (LoaderControl WrongOriginalFrame)
  PublicationPair -> controlReason (LoaderControl WrongPublication)
  MissingRawCallPair -> Just "raw/event trace coverage differs"
  LateWritePair -> Just "paired semantic effects differ"
  DescriptorPair -> Just "paired semantic effects differ"
  UnprovenElapsedPair -> controlReason (TimerControl TimerCopyState)
  UnrelatedHeaderPair -> Just "paired semantic effects differ"

pairMutation :: PairControl -> Fixture -> Either String Fixture
pairMutation control fixture = case control of
  NaturalPair -> Right fixture
  ReorderedPair -> applyControl ReorderCallRecords fixture
  SwappedPair -> Right fixture
  PhasePair -> wrongPhase fixture
  FramePair -> loaderControl WrongOriginalFrame fixture
  PublicationPair -> loaderControl WrongPublication fixture
  UnprovenElapsedPair -> timerControl TimerCopyState fixture
  UnrelatedHeaderPair -> do
    indices <- callIndices fixture Task
    selected <- firstIndex "returned task call absent" (reverse indices)
    returned <- callAt fixture selected >>= eventReference "returned"
    changeStateWord returned (GuestAddress 0xa0834) fixture
  MissingRawCallPair -> do
    call <- loaderService (GuestAddress 0x434f0) fixture >>= callAt fixture
    EventId selected <- eventReference "entry" call
    trace <- arrayAt [] (rawTrace fixture)
    pure fixture {rawTrace=J.toJSON [hit | (index,hit) <- zip [0::Int ..] trace,index/=selected],traceChanged=True}
  LateWritePair -> do
    rows <- arrayAt [field "records"] =<< pathAt [Element 2] (J.toJSON (watches fixture))
    candidates <- forM (zip [0::Int ..] rows) $ \(index,row) -> do
      source <- parseAccess row
      physical <- numberAt [field "physical"] row
      pure (index,source==CpuWrite && physical==0x9662d4)
    selected <- firstIndex "late publication write at 0x9662d4 absent" [index | (index,True) <- candidates]
    modifyWatch 2 (modifyAt [field "records",Element selected,field "value"]
      (\value -> wireNumber value >>= \word -> wordLike (word `xor` 1) value)) fixture
  DescriptorPair -> do
    indices <- callIndices fixture Task
    selected <- firstIndex "returned task call absent" (reverse indices)
    call <- callAt fixture selected
    returned <- eventReference "returned" call
    modifyEvent returned [field "descriptor",field "values",Element 6]
      (\value -> wireNumber value >>= \word -> wordLike (word `xor` 1) value) fixture

bindingMatches :: FilePath -> J.Value -> IO Bool
bindingMatches root binding = do
  name <- either H.failH pure (textAt [field "path"] binding)
  expected <- either H.failH pure (textAt [field "sha256"] binding)
  digest <- SHA.sha256File (root </> name)
  pure (SHA.sha256Hex digest==expected)

timerRelation :: J.Value -> Either String Bool
timerRelation value = do
  kind <- textAt [field "kind"] value
  case kind of
    "exact" -> numberAt [field "value"] value >> Right False
    "timer-elapsed" -> do
      core <- numberAt [field "value",field "core"] value
      _ <- numberAt [field "value",field "occurrence"] value
      ring <- numberAt [field "value",field "ring"] value
      let physical = ring .&. complement 0x40000000
      unless (core<=1 && ring<=0xffffffff && ring .&. 0x40000000/=0 && physical>=0xa0840 && physical+4<=0xc0900 && physical `mod` 16==0)
        (Left "serialized timer sample differs")
      Right True
    _ -> Left "serialized scalar kind differs"

timerEffectsMatch :: FilePath -> J.Value -> IO Bool
timerEffectsMatch root binding = do
  name <- either H.failH pure (textAt [field "path"] binding)
  effects <- readJson 0x4000000 (root </> name)
  pure (either (const False) id (do
    calls <- arrayAt [field "calls"] effects
    states <- concat <$> traverse (\call -> concat <$> traverse (\boundary -> arrayAt [field boundary,field "state"] call) ["entry","returned"]) calls
    headerRelations <- forM states $ \state -> do
      address <- numberAt [field "address"] state
      if address/=0xa0800 then Right False else do
        elapsed <- pathAt [field "values",Element 12] state >>= timerRelation
        adjacent <- pathAt [field "values",Element 13] state >>= timerRelation
        unless (not adjacent) (Left "unrelated header word has a timer relation")
        Right elapsed
    writes <- arrayAt [field "publications",field (show (0xa0830::Int))] effects
    writeRelations <- traverse (\row -> pathAt [field "value"] row >>= \value -> if value==J.Null then Right False else timerRelation value) writes
    Right (or headerRelations && or writeRelations)))

comparisonMatches :: FilePath -> FilePath -> FilePath -> Bool -> IO Bool
comparisonMatches root retained owned equivalent = do
  exists <- doesPathExist (root </> "comparison.json")
  if not exists then pure False else do
    compared <- readJson 0x4000000 (root </> "comparison.json")
    source <- SHA.sha256File (retained </> "continuations.json")
    replacement <- SHA.sha256File (owned </> "continuations.json")
    let selected path = either H.failH pure (pathAt path compared)
    bindings <- traverse selected
      [[field "retained",field "receipt"],[field "owned",field "receipt"],[field "effects"],[field "owned_effects"]]
    valid <- traverse (bindingMatches root) bindings
    relations <- traverse (timerEffectsMatch root) (drop 2 bindings)
    pure (and valid && and relations && numberAt [field "schema"] compared==Right 2
      && textAt [field "scope"] compared==Right "paired-natural-loader-orchestration"
      && maybe False (elem (J.toJSON ("device-read-elapsed"::String))) (either (const Nothing) Just (arrayAt [field "admitted_timing"] compared))
      && pathAt [field "accepted"] compared==Right (J.Bool equivalent)
      && pathAt [field "refusal"] compared==Right (if equivalent then J.Null else J.toJSON ("paired semantic effects differ"::String))
      && textAt [field "outcome"] compared==Right (if equivalent then "equivalent" else "refused-semantic-mismatch")
      && textAt [field "retained",field "receipt",field "sha256"] compared==Right (SHA.sha256Hex source)
      && textAt [field "owned",field "receipt",field "sha256"] compared==Right (SHA.sha256Hex replacement)
      && textAt [field "retained",field "verification",field "receipt"] compared==Right (SHA.sha256Hex source)
      && textAt [field "owned",field "verification",field "receipt"] compared==Right (SHA.sha256Hex replacement)
      && all (\side -> numberAt [field side,field "verification",field "schema"] compared==Right 4) ["retained","owned"]
      && pathAt [field "menu"] compared==Right (J.toJSON ([1,5,2]::[Int]))
      && ((textAt [field "effects",field "sha256"] compared==textAt [field "owned_effects",field "sha256"] compared)==equivalent))

comparisonRefusalMatches :: FilePath -> FilePath -> FilePath -> PairControl -> IO Bool
comparisonRefusalMatches root retained owned control = do
  exists <- doesPathExist (root </> "comparison.json")
  if not exists then pure False else do
    compared <- readJson 0x4000000 (root </> "comparison.json")
    source <- SHA.sha256File (retained </> "continuations.json")
    replacement <- SHA.sha256File (owned </> "continuations.json")
    bindings <- either H.failH pure (traverse (\side -> pathAt [field side,field "receipt"] compared) ["retained","owned"])
    valid <- traverse (bindingMatches root) bindings
    pure (and valid && numberAt [field "schema"] compared==Right 2
      && textAt [field "scope"] compared==Right "paired-natural-loader-orchestration"
      && pathAt [field "accepted"] compared==Right (J.Bool False)
      && textAt [field "outcome"] compared==Right (if control==SwappedPair then "refused-roles" else "refused-owned-observation")
      && pathAt [field "refusal"] compared==Right (J.toJSON (pairReason control))
      && textAt [field "retained",field "receipt",field "sha256"] compared==Right (SHA.sha256Hex source)
      && textAt [field "owned",field "receipt",field "sha256"] compared==Right (SHA.sha256Hex replacement)
      && textAt [field "retained",field "verification",field "receipt"] compared==Right (SHA.sha256Hex source)
      && (if control==SwappedPair
        then textAt [field "owned",field "verification",field "receipt"] compared==Right (SHA.sha256Hex replacement)
        else pathAt [field "owned",field "verification"] compared==Right J.Null)
      && all (\name -> pathAt [field name] compared==Right J.Null) ["effects","owned_effects","menu"])

runPairControls :: Observer -> AnalysisRoot -> ObservationDirectory -> ObservationDirectory -> FilePath -> IO [J.Value]
runPairControls observer analysis@(AnalysisRoot sources) (ObservationDirectory retained) (ObservationDirectory owned) output = do
  fixture <- loadFixture owned sources
  forM [NaturalPair,ReorderedPair,SwappedPair,PhasePair,FramePair,PublicationPair,MissingRawCallPair,LateWritePair,DescriptorPair,UnprovenElapsedPair,UnrelatedHeaderPair] $ \control -> do
    let name = pairName control
        directory = output </> name
        reason = pairReason control
        verdict = maybe Admit (const Refuse) reason
        semanticMismatch = control `elem` [LateWritePair,DescriptorPair,UnrelatedHeaderPair]
        individualVerdict = if control `elem` [PhasePair,FramePair,PublicationPair,MissingRawCallPair,UnprovenElapsedPair] then Refuse else Admit
        individualReason = if individualVerdict==Refuse then reason else Nothing
        fields = ["name" J..= name,"scope" J..= (if control==NaturalPair then "actual-observation-pair" else originText SerializedSynthetic)
          ,"expected_exit" J..= exitNumber (expectedExit verdict),"expected_refusal" J..= reason]
    createDirectory directory
    result <- case pairMutation control fixture of
      Left problem -> pure (J.object (fields++["preparation_error" J..= problem,"passed" J..= False]))
      Right mutated -> do
        right <- if control==NaturalPair || control==SwappedPair then pure owned else do
          let destination = directory </> "owned"
          writeFixture destination mutated
          pure destination
        let (leftInput,rightInput) = if control==SwappedPair then (owned,retained) else (retained,right)
            comparison = directory </> "comparison"
        digest <- SHA.sha256File (right </> "continuations.json")
        independent <- invoke observer analysis right (directory </> "independent")
        let independentlyMatched = resultPassed individualVerdict individualReason (verificationMatches digest independent) independent
        actual <- runObserver observer ["--compare-retained",leftInput,"--compare-owned",rightInput,"--analysis",sources,"--output",comparison] (directory </> "comparator")
        observed <- if verdict==Admit || semanticMismatch then comparisonMatches comparison leftInput rightInput (verdict==Admit)
          else comparisonRefusalMatches comparison leftInput rightInput control
        pure (J.object (fields++["actual_exit" J..= exitNumber (processExit actual),"timed_out" J..= processTimedOut actual
          ,"independent_expected_exit" J..= exitNumber (expectedExit individualVerdict),"independent_actual_exit" J..= exitNumber (processExit independent)
          ,"independent_observation_matched" J..= independentlyMatched,"comparison_observation_matched" J..= observed
          ,"refusal_matched" J..= maybe False (`refusalMatches` actual) reason
          ,"passed" J..= (independentlyMatched && observed && resultPassed verdict reason observed actual)]))
    _ <- emit (directory </> "result.json") result
    pure result

arguments :: [String] -> IO Arguments
arguments values = case values of
  [observer,analysis,baseline,output] -> do
    executable <- canonicalizePath observer
    sources <- canonicalizePath analysis
    original <- canonicalizePath baseline
    pure (Arguments (Observer executable) (AnalysisRoot sources) (Single (ObservationDirectory original)) (OutputDirectory output))
  [observer,analysis,retained,owned,output] -> do
    executable <- canonicalizePath observer
    sources <- canonicalizePath analysis
    source <- canonicalizePath retained
    replacement <- canonicalizePath owned
    pure (Arguments (Observer executable) (AnalysisRoot sources) (Paired (ObservationDirectory source) (ObservationDirectory replacement)) (OutputDirectory output))
  _ -> H.failH "usage: loader-continuation-controls OBSERVER ANALYSIS_ROOT OBSERVATION_DIRECTORY NEW_OUTPUT_DIRECTORY; OBSERVER ANALYSIS_ROOT RETAINED_DIRECTORY OWNED_DIRECTORY NEW_OUTPUT_DIRECTORY"

main :: IO ()
main = do
  Arguments observer analysis inputs (OutputDirectory output) <- getArgs >>= arguments
  result <- case inputs of
    Single original -> runSuite observer analysis original output
    Paired retained owned -> do
      createDirectory output
      source <- runSuite observer analysis retained (output </> "retained")
      replacement <- runSuite observer analysis owned (output </> "owned")
      paired <- if pathAt [field "baseline_passed"] source==Right (J.Bool True) && pathAt [field "baseline_passed"] replacement==Right (J.Bool True)
        then runPairControls observer analysis retained owned output
        else pure []
      let passed = casePassed source && casePassed replacement && not (null paired) && all casePassed paired
          manifest = J.object ["schema" J..= (3::Int),"retained" J..= source,"owned" J..= replacement,"paired_cases" J..= paired,"passed" J..= passed]
      _ <- emit (output </> "controls.json") manifest
      pure manifest
  unless (casePassed result) exitFailure
