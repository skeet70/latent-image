{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (foldM, forM, forM_, unless)
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.Bits ((.&.), complement, shiftL, (.|.), xor)
import Data.List (findIndex, stripPrefix)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.Text as T
import qualified Data.Vector as V
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
data LoaderCopy = PrimaryCopy | SecondaryCopy
data CopyChange = MoveEarlier LoaderCopy | WrongCopySource | PartialCopy | MisplacedCopy | ExtraCopy | UncertainCopy
data WriteBoundary = BeforeLoaderEntry | AfterLoaderEntry | DuringLoaderFrame deriving Eq
data Writer = HostWriter | ModelWriter
data Gpr = R0 | R3 | R4
data PlainExtent = Extent0 | Extent2 | Extent3
data Arguments = Arguments Observer AnalysisRoot ObservationDirectory OutputDirectory
data CreationField = CreationArgument | CreationTarget

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
  { originalRoot :: FilePath, receipt :: J.Value, rawTrace :: J.Value, watches :: [J.Value], codeImages :: [CodeImage] }

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
    readJson 0x4000000 (root </> name)
  loaderA <- H.readRegular 0x20000 (analysis </> "decode/loader-40000.bin")
  image4 <- H.readRegular 0x1000000 (analysis </> "lz/id4-0061c400.bin")
  image5 <- H.readRegular 0x1000000 (analysis </> "lz/id5-00bf1a00.bin")
  pure (Fixture root document trace files [CodeImage (GuestAddress 0x40000) loaderA, CodeImage (GuestAddress 0x61c400) image4, CodeImage (GuestAddress 0xbf1a00) image5])

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
  pure fixture {rawTrace=trace}

gprIndex :: Gpr -> Int
gprIndex R0 = 0
gprIndex R3 = 3
gprIndex R4 = 4

setRegister :: EventId -> Gpr -> Word32 -> Fixture -> Either String Fixture
setRegister identifier register value fixture = modifyEvent identifier [field "registers",field "r",Element (gprIndex register)] (const (Right (J.toJSON value))) fixture >>= syncEvent identifier

setLink :: EventId -> GuestAddress -> Fixture -> Either String Fixture
setLink identifier address fixture = modifyEvent identifier [field "registers"] (setGuest [field "lr"] address) fixture >>= syncEvent identifier

changeCode :: EventId -> GuestAddress -> Fixture -> Either String Fixture
changeCode identifier address@(GuestAddress guest) fixture = do
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
  syncEvent identifier next''

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

phaseCall :: GuestAddress -> Fixture -> Either String Int
phaseCall site fixture = do
  loaders <- callIndices fixture Loader
  matches <- forM loaders $ \index -> do
    call <- callAt fixture index
    before <- eventReference "before" call >>= eventAt fixture
    pc <- guestAt [field "pc"] before
    pure (index,pc)
  firstIndex "phase call absent" [index | (index,pc) <- matches,pc==site]

phaseOrder :: Fixture -> Either String Fixture
phaseOrder fixture = do
  first <- phaseCall (GuestAddress 0x40a34) fixture
  second <- phaseCall (GuestAddress 0x40a3c) fixture
  let change index address@(GuestAddress pc) phase original = do
        call <- callAt original index
        before <- eventReference "before" call
        entered <- eventReference "entry" call
        returned <- eventReference "returned" call
        next <- setCallGuest index "continuation" (GuestAddress (pc+4)) original
        next' <- setRegister before R0 phase next >>= setRegister entered R0 phase >>= setLink entered (GuestAddress (pc+4))
        changeCode before address next' >>= changeCode returned (GuestAddress (pc+4))
  next <- change first (GuestAddress 0x40a3c) 3 fixture >>= change second (GuestAddress 0x40a34) 1
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
  pure fixture {receipt=document,rawTrace=J.toJSON [hit | (old,hit) <- zip [0..] trace,S.notMember (EventId old) removed]}

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
  pure fixture {watches=updated}

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
      identifier <- firstEvent fixture (GuestAddress 0x409ac)
      count <- eventAt fixture identifier >>= numberAt [field "instructions"]
      RowIndex position <- firstCpuAfter (InstructionCount count) remaining
      let (before,after) = splitAt position remaining
      modifyWatch 0 (setAt [field "records"] (before++row:after)) fixture

writerControl :: WriteBoundary -> Writer -> Fixture -> Either String Fixture
writerControl boundary writer fixture = do
  identifier <- firstEvent fixture (GuestAddress 0x409ac)
  event <- eventAt fixture identifier
  count <- numberAt [field "instructions"] event
  (RowIndex original,_,rows) <- copyRow PrimaryCopy fixture
  position <- if boundary==BeforeLoaderEntry then pure original else do RowIndex index <- firstCpuAfter (InstructionCount count) rows;pure (index+1)
  let address = if boundary==DuringLoaderFrame then 0x63450 else 0x409ac :: Word32
      source = case writer of HostWriter -> "host-write"; ModelWriter -> "model-bus-write" :: String
  value <- if boundary==DuringLoaderFrame then pure 0 else numberAt [field "code",field "values",Element 0] event
  let row = J.object ["source" J..= source,"address" J..= address,"physical" J..= address,"size" J..= (4::Int),"value" J..= value,"core" J..= J.Null,"pc" J..= J.Null,"instructions" J..= J.Null,"tick" J..= J.Null]
      (before,after) = splitAt position rows
  modifyWatch 0 (setAt [field "records"] (before++row:after)) fixture

forgedCut :: Fixture -> Either String Fixture
forgedCut fixture = do
  changed <- copyControl UncertainCopy fixture
  identifier <- firstEvent changed (GuestAddress 0x409ac)
  (RowIndex position,_,_) <- copyRow PrimaryCopy changed
  modifyEvent identifier [] (\value -> case value of
    J.Object object -> Right (J.Object (KM.insert "watch" (J.object ["epoch" J..= ("prefix"::String),"rows" J..= (position+1)]) object))
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
  call <- phaseCall (GuestAddress 0x40a34) fixture >>= callAt fixture
  before <- eventReference "before" call
  entered <- eventReference "entry" call
  setRegister before R0 4 fixture >>= setRegister entered R0 4

wrongTarget :: Fixture -> Either String Fixture
wrongTarget fixture = do
  index <- phaseCall (GuestAddress 0x40a34) fixture
  call <- callAt fixture index
  identifiers <- traverse (`eventReference` call) ["before","entry","returned"]
  next <- setCallGuest index "target" (GuestAddress 0x69cdfc) fixture
  next' <- foldM (\value identifier -> setRegister identifier R4 0x69cdfc value) next identifiers
  entered <- eventReference "entry" call
  changeCode entered (GuestAddress 0x69cdfc) next'

wrongReturn :: Fixture -> Either String Fixture
wrongReturn fixture = do
  call <- phaseCall (GuestAddress 0x40a68) fixture >>= callAt fixture
  returned <- eventReference "returned" call
  calls <- arrayAt [field "calls"] (receipt fixture)
  let follows identifier value = pathAt [field "before"] value == Right (J.toJSON (let EventId index=identifier in index))
  publisher <- firstIndex "publisher call absent" (filter (follows returned) calls)
  published <- eventReference "returned" publisher
  ready <- firstIndex "auto-ready call absent" (filter (follows published) calls)
  publisherEntry <- eventReference "entry" publisher
  readyEntry <- eventReference "entry" ready
  foldM (\value identifier -> setRegister identifier R0 0x69cc70 value) fixture [returned,publisherEntry,published,readyEntry]

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

data Control = ReorderCallRecords | WrongPhase | WrongTarget | WrongReturnedCode | WrongSavedPc | OutOfRangeWrite
  | MissingTask | MissingHook | SyntheticDeferred | MissingDeferred | PhaseRawOrder | ConstructorRawOrder
  | MissingLoaderBss | MissingFirstImage | MissingFirstBss | MissingFirstPlain | MisplacedProjection
  | MissingImage5Return | MissingPlainReturn
  | MissingTrampoline | WrongCreationArgument | WrongCreationTarget | WrongCleanup
  | CpuClock ClockMode | TcmMisaligned | TcmPastEnd | TcmExactEnd | MissingExtent PlainExtent | MissingObservedBss GuestAddress
  | LoaderCopyControl CopyChange | UntimedWriter WriteBoundary Writer | ForgedEntryCut

controlName :: Control -> String
controlName control = case control of
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

controlVerdict :: Control -> Verdict
controlVerdict control = case control of
  ReorderCallRecords -> Admit; SyntheticDeferred -> Admit; CpuClock AtEnd -> Admit; CpuClock ResumedBegin -> Admit; TcmExactEnd -> Admit; LoaderCopyControl (MoveEarlier _) -> Admit; _ -> Refuse

controlOrigin :: Control -> Origin
controlOrigin control = case control of
  SyntheticDeferred -> SerializedSynthetic; MissingDeferred -> SerializedSynthetic; PhaseRawOrder -> SerializedSynthetic
  ConstructorRawOrder -> SerializedSynthetic; CpuClock AtEnd -> SerializedSynthetic; CpuClock ResumedBegin -> SerializedSynthetic
  TcmExactEnd -> SerializedSynthetic; WrongTarget -> SerializedSynthetic; WrongReturnedCode -> SerializedSynthetic
  OutOfRangeWrite -> SerializedSynthetic; UntimedWriter _ _ -> SerializedSynthetic; ForgedEntryCut -> SerializedSynthetic
  LoaderCopyControl (MoveEarlier _) -> SerializedSynthetic; LoaderCopyControl ExtraCopy -> SerializedSynthetic; LoaderCopyControl UncertainCopy -> SerializedSynthetic; _ -> ActualMutation

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
    ++[MissingObservedBss address | address <- [GuestAddress 0x6a8c04,GuestAddress 0x6a9734],address `elem` addresses])

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
  names <- either H.failH pure (traverse (textAt [field "file",field "path"]) epochs)
  let excluded = S.fromList (["continuations.json","verification.json","observer-error.json",traceName]++names)
  entries <- listDirectory (originalRoot fixture)
  forM_ entries $ \name -> unless (S.member name excluded) $ do
    status <- getSymbolicLinkStatus (originalRoot fixture </> name)
    if isRegularFile status then createLink (originalRoot fixture </> name) (output </> name) else pure ()
  traceHash <- emit (output </> "control-trace.json") (rawTrace fixture)
  document <- either H.failH pure (setAt [field "trace"] (J.object ["path" J..= ("control-trace.json"::String),"sha256" J..= SHA.sha256Hex traceHash]) (receipt fixture))
  H.ensure (length epochs==length (watches fixture)) "epoch/watch cardinality differs"
  updated <- foldM (\value (index,watch) -> do
    let name = "control-watch-"++show index++".json"
    digest <- emit (output </> name) watch
    count <- either H.failH pure (length <$> arrayAt [field "records"] watch)
    either H.failH pure (setAt [field "epochs",Element index,field "file"] (J.object ["path" J..= name,"sha256" J..= SHA.sha256Hex digest]) value >>= setAt [field "epochs",Element index,field "record_count"] count)) document (zip [0..] (watches fixture))
  _ <- emit (output </> "continuations.json") updated
  pure ()

data ProcessResult = ProcessResult {processTimedOut :: Bool, processExit :: ExitCode}
invoke :: Observer -> AnalysisRoot -> FilePath -> FilePath -> IO ProcessResult
invoke (Observer observer) (AnalysisRoot analysis) fixture output = do
  (timedOut,code,stdoutBytes,stderrBytes) <- H.runBounded 120000000 1048576 observer ["--verify",fixture,"--analysis",analysis] B.empty
  B.writeFile (output <.> "stdout") stdoutBytes
  B.writeFile (output <.> "stderr") stderrBytes
  pure (ProcessResult timedOut code)

exitNumber :: ExitCode -> Int
exitNumber ExitSuccess = 0
exitNumber (ExitFailure number) = number

expectedExit :: Verdict -> ExitCode
expectedExit Admit = ExitSuccess
expectedExit Refuse = ExitFailure 1

arguments :: [String] -> IO Arguments
arguments [observer,analysis,baseline,output] = do
  executable <- canonicalizePath observer
  sources <- canonicalizePath analysis
  original <- canonicalizePath baseline
  pure (Arguments (Observer executable) (AnalysisRoot sources) (ObservationDirectory original) (OutputDirectory output))
arguments _ = H.failH "usage: loader-continuation-controls OBSERVER ANALYSIS_ROOT OBSERVATION_DIRECTORY NEW_OUTPUT_DIRECTORY"

main :: IO ()
main = do
  Arguments (observer@(Observer executable)) (analysis@(AnalysisRoot sources)) (ObservationDirectory original) (OutputDirectory output) <- getArgs >>= arguments
  createDirectory output
  baseline <- invoke observer analysis original (output </> "baseline")
  if processTimedOut baseline || processExit baseline/=ExitSuccess then do
    _ <- emit (output </> "controls.json") (J.object ["schema" J..= (1::Int),"baseline_exit" J..= exitNumber (processExit baseline),"baseline_timed_out" J..= processTimedOut baseline,"cases" J..= ([]::[J.Value]),"passed" J..= False])
    exitFailure
  else pure ()
  fixture <- loadFixture original sources
  selected <- either H.failH pure (controls fixture)
  results <- forM selected $ \control -> do
    let name = controlName control
        expected = expectedExit (controlVerdict control)
        baseFields = ["name" J..= name,"scope" J..= originText (controlOrigin control),"expected_exit" J..= exitNumber expected]
    result <- case applyControl control fixture of
      Left problem -> pure (J.object (baseFields++["actual_exit" J..= J.Null,"preparation_error" J..= problem,"passed" J..= False]))
      Right mutated -> do
        let directory = output </> name
        writeFixture directory mutated
        actual <- invoke observer analysis directory (output </> name)
        pure (J.object (baseFields++["actual_exit" J..= exitNumber (processExit actual),"timed_out" J..= processTimedOut actual,"passed" J..= (not (processTimedOut actual) && processExit actual==expected)]))
    _ <- emit (output </> name <.> "result.json") result
    pure result
  own <- getExecutablePath
  observerHash <- SHA.sha256File executable
  controlsHash <- SHA.sha256File own
  baselineHash <- SHA.sha256File (original </> "continuations.json")
  let passed = all (\value -> pathAt [field "passed"] value==Right (J.Bool True)) results
      manifest = J.object ["schema" J..= (1::Int),"baseline_exit" J..= (0::Int),"observer_sha256" J..= SHA.sha256Hex observerHash,"controls_executable_sha256" J..= SHA.sha256Hex controlsHash,"baseline_receipt_sha256" J..= SHA.sha256Hex baselineHash,"cases" J..= results,"passed" J..= passed]
  _ <- emit (output </> "controls.json") manifest
  unless passed exitFailure
