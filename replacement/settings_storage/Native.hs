{-# LANGUAGE GHC2021 #-}
module Native where

import Control.Monad (foldM)
import qualified Data.ByteString as B
import Data.List (isInfixOf, isPrefixOf, nub)
import Numeric (showHex)
import Data.Bits ((.&.), (.|.))
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.NativeObservation (GuestAddress(..), RegionId(..), Register(..), RunnerStatus(..), NativeObservation(..), regionIdOf)
import Emulation.NativeCase (Region(..))
import Emulation.OwnedCase
import qualified Replacement.PropertyService.Oracle as P
import Replacement.PropertyService.Classify (Property, Owned(..), FieldArm(..), FieldHook(..))
import qualified Replacement.PropertyService.HeaderBoundary as H
import qualified Replacement.PropertyService.Artifact as Artifact
import qualified Replacement.PropertyService.Placement as Placement
import Replacement.PropertyService.Admission (FunctionExtent(..))
import PropertyFields (PropertyId(..))
import Oracle

codeBase, codeEnd, logHelper :: Word32
codeBase = 0x61c400
codeEnd = 0x72b3fc
logHelper = 0x580000

data CodeWindow = CodeWindow { windowRegion :: RegionId, windowBase :: Word32, windowEnd :: Word32 }

codeWindows :: [CodeWindow]
codeWindows = [CodeWindow (RegionId "code-primary") 0x61d59c 0x6439e8,CodeWindow (RegionId "code-mirror") 0x6a59b8 0x6a86dc,CodeWindow (RegionId "code-runtime") 0x722054 0x72b3fc]

sparseCode :: B.ByteString -> [CaseRegion]
sparseCode bytes = [mkRegion (windowRegion window) (GuestAddress (fromIntegral (windowBase window))) (fromIntegral (windowEnd window-windowBase window)) ReadExecute (B.take (fromIntegral (windowEnd window-windowBase window)) (B.drop (fromIntegral (windowBase window-codeBase)) bytes)) | window <- codeWindows]

isCodeRegion :: CaseRegion -> Bool
isCodeRegion r = regionBytesOfId r `elem` map windowRegion codeWindows

data NativeImage = NativeImage
  { canonicalBytes :: B.ByteString
  , stockCodeRegions :: M.Map TaskContext [CaseRegion]
  , stockCodeBytes :: M.Map TaskContext B.ByteString
  , backupKeys :: [BackupKey]
  }

prepareNativeImage :: B.ByteString -> NativeImage
prepareNativeImage image = NativeImage image (M.map sparseCode code) code (keysFromImage image)
  where code = M.fromList [(context,chainCode context image) | context <- [Task,Interrupt]]

stockRegions :: NativeImage -> TaskContext -> [CaseRegion]
stockRegions image context = maybe (error "missing prepared stock code") id (M.lookup context (stockCodeRegions image))

data ExportScope = InitializedPrefix | RetainedResidue deriving (Eq, Ord)
newtype DefaultsInput = DefaultsInput (B.ByteString,B.ByteString) deriving (Eq, Ord)
data OwnedImage = OwnedImage
  { ownedCodeRegions :: M.Map (TaskContext,ExportScope,DefaultsInput) [CaseRegion]
  , ownedDefaultRegions :: M.Map DefaultsInput CaseRegion
  , ownedFunctionSpans :: [(GuestAddress,Word32)]
  , ownedRemovedEntries :: [GuestAddress]
  }

prepareOwnedImage :: NativeImage -> B.ByteString -> M.Map String Symbol -> [P.Fixture] -> OwnedImage
prepareOwnedImage source binary symbols fixtures = OwnedImage codes defaults functions []
  where
    groups = [((context,scope),sparseCode (foldl patch (maybe (error "missing prepared stock bytes") id (M.lookup context (stockCodeBytes source))) (selectedEntries scope))) | context <- [Task,Interrupt],scope <- [InitializedPrefix,RetainedResidue]]
    codes = M.fromList [((context,scope,input),regions) | ((context,scope),regions) <- groups,input <- inputs]
    selectedEntries scope = [entry | entry@(address,_) <- entryNames, address `notElem` [0x6a6e68,0x72b3a4] || scope == InitializedPrefix]
    patch bytes (address,name) = case M.lookup name symbols of
      Just (Symbol target _) -> placeCode address [branch address target] bytes
      Nothing -> error ("owned entry missing: " ++ name)
    inputs = M.keys (M.fromList [(DefaultsInput (B.take 40 (P.fixtureTailDefaults fx),P.fixtureSlotDefaults fx),()) | fx <- fixtures])
    defaults = M.fromList [(input,region "owned" 0x5c0000 ReadExecute (inject input)) | input <- inputs]
    inject (DefaultsInput (tailBytes,slotBytes)) = foldl place binary [("TAIL_BYTES",tailBytes),("SLOT_BYTES",slotBytes)]
    place bytes (name,value) =
      let Symbol address size = maybe (error ("missing owned defaults " ++ name)) id (M.lookup name symbols)
      in if fromIntegral size /= B.length value then error ("owned default size " ++ name)
         else replace (fromIntegral (address-0x5c0000)) value bytes
    functions = nub [(GuestAddress (fromIntegral address),size) | (name,Symbol address size) <- M.toList symbols, size > 0, name `notElem` ["TAIL_BYTES","SLOT_BYTES"]]

data PropertyMetadata = PropertyMetadata
  { metadataFunctions :: [FunctionExtent]
  , metadataDefaultWindows :: [(Artifact.DefaultImage,Placement.GuestAddress)]
  }

propertyMetadata :: Artifact.LinkedArtifact -> PropertyMetadata
propertyMetadata artifact = PropertyMetadata (Artifact.artifactFunctions artifact) (Artifact.artifactDefaultWindows artifact)

prepareComposedImage :: B.ByteString -> PropertyMetadata -> B.ByteString -> M.Map String Symbol -> [(Word32,String)] -> [P.Fixture] -> OwnedImage
prepareComposedImage propertyImage description binary symbols entries fixtures = OwnedImage codes defaults functions removed
  where
    base = 0x5d0000
    Symbol textEnd _ = composedSymbol "__custom_text_end"
    code = B.take (fromIntegral (textEnd-base)) binary
    composedSymbol name = maybe (error ("missing composed symbol " ++ name)) id (M.lookup name symbols)
    groups = [(context,sparseCode (foldl patch (chainCode context propertyImage) entries)) | context <- [Task,Interrupt]]
    variants = [((context,input),map (injectPropertyDefaults propertyDefaults input) regions) | (context,regions) <- groups,input <- inputs]
    codes = M.fromList [((context,scope,input),regions) | ((context,input),regions) <- variants,scope <- [InitializedPrefix,RetainedResidue]]
    patch bytes (address,name) = let Symbol site size = composedSymbol name
      in if site /= address || size /= 4 then error ("composed entry moved " ++ name)
         else placeCode address [readWordAt binary (fromIntegral (address-base))] bytes
    inputs = M.keys (M.fromList [(DefaultsInput (B.take 40 (P.fixtureTailDefaults fx),P.fixtureSlotDefaults fx),()) | fx <- fixtures])
    defaults = M.fromList [(input,region "owned" base ReadExecute (inject input)) | input <- inputs]
    inject (DefaultsInput (tailBytes,slotBytes)) = foldl place code [("TAIL_BYTES",tailBytes),("SLOT_BYTES",slotBytes)]
    place bytes (name,value) = let Symbol address size = composedSymbol name
      in if fromIntegral size /= B.length value || address < base || address+size > textEnd then error ("composed default extent " ++ name)
         else replace (fromIntegral (address-base)) value bytes
    compiled = [(GuestAddress (fromIntegral address),size) | (name,Symbol address size) <- M.toList symbols,size > 0,name `notElem` ["TAIL_BYTES","SLOT_BYTES"],(address >= base && address+size <= textEnd) || (address `elem` map fst entries && size == 4)]
    propertyOwned = [(GuestAddress (fromIntegral low),high-low) | P.FunctionSpan low high <- P.ownedFunctions [P.FunctionSpan low high | FunctionExtent _ (Placement.GuestAddress low) (Placement.GuestAddress high) <- metadataFunctions description],low >= 0x61e110,low < 0x6309c4]
    functions = nub (compiled ++ propertyOwned)
    propertyDefaults = [(kind,GuestAddress (fromIntegral address)) | (kind,Placement.GuestAddress address) <- metadataDefaultWindows description]
    removed = map (GuestAddress . fromIntegral) (0x61e110 : 0x624694 : map fst entries)

mov :: Int -> Word32 -> [Word32]
mov = H.mov

branch :: Word32 -> Word32 -> Word32
branch = H.branch

callback :: Word32 -> [Word32]
callback address
  | address == 0x6a86c4 = [0xe3a00000,0xe3a01000,0xe3a02000] ++ mov 12 address ++ [branch (address+20) logHelper]
  | otherwise = H.callback address

callbacks :: [Word32]
callbacks = [0x6a59b8,0x726c74,0x633594,0x6a86c4]

helperWords :: [Word32]
helperWords = H.helperWords

-- 0x722054 is the declared byte-copy boundary shared by both images.
copyWords :: [Word32]
copyWords = H.copyWords

placeCode :: Word32 -> [Word32] -> B.ByteString -> B.ByteString
placeCode address words' = replace (fromIntegral (address-codeBase)) (assemble words')

chainCode :: TaskContext -> B.ByteString -> B.ByteString
chainCode _ image = foldl (\bytes (at,ws) -> placeCode at ws bytes) raw patches
  where
    raw = B.take (fromIntegral (codeEnd-codeBase)) image
    patches = [(a,callback a) | a <- callbacks] ++ [(0x722054,copyWords),(0x6439e0,[0xe3a00001,0xe12fff1e])]

chainFunctions :: [(GuestAddress,Word32)]
chainFunctions = [(GuestAddress (fromIntegral a), b-a) | (a,b) <-
  [(0x61d8dc,0x61d92c),(0x61d92c,0x61d944),(0x61d944,0x61d994),(0x61d994,0x61d9ec),(0x61d9ec,0x61da3c),(0x61da3c,0x61da8c),(0x61daa8,0x61db04),(0x61db04,0x61db18),(0x61db18,0x61db3c),(0x61db3c,0x61db5c),(0x6309c4,0x630a34),(0x630a34,0x630b14),(0x6439e0,0x6439e8),(0x6a6ddc,0x6a6e68),(0x6a6e68,0x6a6f3c),(0x6a6f3c,0x6a6fd8),(0x72b3a4,0x72b3d0),(0x72b3d0,0x72b3fc),(0x722054,0x722070),(0x723cb8,0x723ce8),(0x724b68,0x724c00),(0x724c00,0x724cb4),(0x724cb4,0x724cbc)] ++ [(a,a+fromIntegral (length (callback a)*4)) | a <- callbacks]] ++ [(GuestAddress (fromIntegral logHelper),fromIntegral (length helperWords*4))]

region :: String -> Word32 -> Permission -> B.ByteString -> CaseRegion
region name base permission bytes = mkRegion (RegionId name) (GuestAddress (fromIntegral base)) (B.length bytes) permission bytes

propertyCase :: NativeImage -> M.Map PropertyId Property -> P.Fixture -> TaskContext -> P.OracleCase -> Owned -> CaseSpec
propertyCase source properties fx context oracleCase owned = base
  { caseProvenance = "X-M1 1.50 E02/E03/E06/E07/E08/E09, independent field model with real stock header chain to backup-register driver"
  , caseRegions = stockRegions source context ++ filter keep (caseRegions base) ++ extra
  , caseFunctions = filter (\(GuestAddress address,size) -> (fromIntegral address,fromIntegral address+size) `notElem` H.functionSpans) (caseFunctions base) ++ chainFunctions
  , caseExpectedMemory = [(RegionId "banks", P.effectBanks effect),(RegionId "previous",P.effectPrevious effect),(RegionId "config",P.fixtureConfig fx),(RegionId "calls",eventBytes events),(RegionId "backing",finalBacking)]
  , caseIgnoredMemory = deadPrivateFrame base
  }
  where
    image = canonicalBytes source
    standing = P.standingOf properties (P.oracleProperty oracleCase)
    expected = maybe (error "field outside admitted fixture") id (P.modelExpectation fx owned (P.oracleOperation oracleCase))
    base = P.caseSpec image P.stockFunctions fx (P.caseLabel oracleCase) standing oracleCase expected
    (effect,mirror,events) = maybe (error "field outside admitted fixture") id (propertyResult (backupKeys source) context fx owned (P.oracleOperation oracleCase))
    keep r = regionBytesOfId r `notElem` map RegionId ["code","backup","calls","backing","header-mirror-code","header-runtime-code","header-reporter","header-log","slot-defaults","kernel-slots","stack","caller-frame"]
    initialBacking = replace 0x19c (shadowBytes (initialMirror fx)) (replace 0 (liveBytes (initialMirror fx)) (B.pack [fromIntegral (i*37+19) | i <- [0..0x1ab::Int]]))
    finalBacking = replace 0 (liveBytes mirror) (replace 0x19c (shadowBytes mirror) initialBacking)
    taskWord = case context of Task -> 1; Interrupt -> 0
    extra = [region "backing" 0x409c281c ReadWrite initialBacking,region "calls" 0x958000 ReadWrite (B.replicate 0x800 0),region "helper" logHelper ReadExecute (assemble helperWords),region "slot-defaults" 0x7cb7d4 ReadOnly (B.take 8 (B.drop (0x7cb7d4-0x61c400) image) <> P.fixtureSlotDefaults fx),region "kernel-slots" H.slotBase ReadExecute (H.slotCode taskWord),region "stack" 0x959000 ReadWrite (B.replicate 0x800 0)]

deadPrivateFrame :: CaseSpec -> [IgnoredMemory]
deadPrivateFrame spec
  | caseExpectedStatus spec /= Returned = []
  | Just _ <- caseKernel spec = []
  | otherwise = case (lookup Sp (caseInitial spec), [caseRegion r | r <- caseRegions spec, regionIdOf (caseRegion r) == RegionId "stack"]) of
      (Just pointer, [stack]) ->
        let dead = max 0 (min (B.length (regionBytes stack)) (fromIntegral pointer - fromIntegral (baseAddress stack)))
        in [IgnoredMemory (RegionId "stack") 0 dead PrivateFrame | dead > 0]
      _ -> []

regionBytesOfId :: CaseRegion -> RegionId
regionBytesOfId = regionIdOf . caseRegion

ownedCase :: OwnedImage -> CaseSpec -> CaseSpec
ownedCase image spec = spec
  { caseRegions = filter (not . isCodeRegion) (caseRegions spec) ++ requiredCache (ownedCodeRegions image) (context,scope,defaults) ++ [requiredCache (ownedDefaultRegions image) defaults]
  , caseFunctions = nub (filter (\(address,_) -> address `notElem` ownedRemovedEntries image) (caseFunctions spec) ++ ownedFunctionSpans image)
  }
  where
    requiredCache :: Ord key => M.Map key value -> key -> value
    requiredCache cache key = maybe (error "missing prepared owned region") id (M.lookup key cache)
    original name = case [regionBytes (caseRegion r) | r <- caseRegions spec, regionBytesOfId r == RegionId name] of
      [bytes] -> bytes
      _ -> error ("missing owned case input " ++ name)
    context = case readWordAt (original "kernel-slots") 0x10 .&. 0xff of
      1 -> Task
      0 -> Interrupt
      _ -> error "invalid prepared task context"
    scope = case [regionBytes (caseRegion r) | r <- caseRegions spec, regionBytesOfId r == RegionId "descriptor"] of
      [bytes] -> let end = (readWordAt bytes 4 .&. 0xffff) + (readWordAt bytes 8 .&. 0xffff) in if end <= 0x168 || end > 0x19a then InitializedPrefix else RetainedResidue
      _ -> InitializedPrefix
    defaults = DefaultsInput (B.take 40 (original "tail-defaults"),B.take 200 (B.drop 8 (original "slot-defaults")))

injectPropertyDefaults :: [(Artifact.DefaultImage,GuestAddress)] -> DefaultsInput -> CaseRegion -> CaseRegion
injectPropertyDefaults windows defaults current = foldl inject current windows
  where
    inject candidate (kind,GuestAddress address) =
      let input = caseRegion candidate
          value = case (kind,defaults) of
            (Artifact.SlotDefaultBytes,DefaultsInput (_,slotBytes)) -> slotBytes
            (Artifact.TailDefaultBytes,DefaultsInput (tailBytes,_)) -> tailBytes
          addressInteger = toInteger address
          low = baseAddress input
          high = low + toInteger (B.length (regionBytes input))
      in if addressInteger >= low && addressInteger + toInteger (B.length value) <= high
         then candidate { caseRegion = input { regionBytes = replace (fromInteger (addressInteger-low)) value (regionBytes input) } }
         else candidate

callerFramePreserved :: CaseSpec -> NativeObservation -> Bool
callerFramePreserved spec observation = stackPreserved && callerPreserved
  where
    regionsNamed name = [caseRegion r | r <- caseRegions spec, regionBytesOfId r == RegionId name]
    memory name = lookup (RegionId name) (nativeMemory observation)
    stackPreserved = case (lookup Sp (caseInitial spec),regionsNamed "stack",memory "stack") of
      (Just pointer,[initial],Just actual) ->
        let base = baseAddress initial
            offset = fromInteger (fromIntegral pointer-base)
            initialBytes = regionBytes initial
        in fromIntegral pointer >= base && offset <= B.length initialBytes
           && B.drop offset actual == B.drop offset initialBytes
      _ -> False
    callerPreserved = case (regionsNamed "caller-frame",memory "caller-frame") of
      ([],_) -> True
      ([initial],Just actual) -> actual == regionBytes initial
      _ -> False

entryNames :: [(Word32,String)]
entryNames = [(0x61e110,"xm1_getter_owned"),(0x624694,"xm1_setter_owned"),(0x61da3c,"xm1_storage_open"),(0x61d944,"xm1_storage_close"),(0x61db3c,"xm1_storage_capture"),(0x61db18,"xm1_storage_restore"),(0x61db04,"xm1_storage_copy"),(0x61daa8,"xm1_storage_exchange"),(0x630a34,"xm1_storage_initialize"),(0x6309c4,"xm1_storage_apply"),(0x6a6e68,"xm1_storage_export"),(0x6a6f3c,"xm1_storage_import"),(0x72b3a4,"xm1_storage_read_request"),(0x72b3d0,"xm1_storage_write_request")]

data MirrorOperation = OpenHeader | CloseHeader deriving (Eq, Show)

mirrorCase :: CaseSpec -> String -> TaskContext -> MirrorOperation -> B.ByteString -> MirrorState -> [BackupKey] -> CaseSpec
mirrorCase template label context operation header old keys = template
  { caseId = label
  , caseEntry = GuestAddress (if operation == OpenHeader then 0x61da3c else 0x61d944)
  , caseInitial = (R0,0x96cc64) : filter ((/= R0) . fst) (caseInitial template)
  , caseExpectedRegisters = [(register, if register == R0 then 0 else value) | (register,value) <- caseExpectedRegisters template]
  , caseRegions = map initialize (caseRegions template)
  , caseExpectedMemory = [(RegionId "banks",finalBanks),(RegionId "previous",replace 0x1c4 (word32le 0) (original "previous")),(RegionId "backing",backing final),(RegionId "calls",eventBytes events)]
  }
  where
    original name = case [regionBytes (caseRegion r) | r <- caseRegions template, regionBytesOfId r == RegionId name] of
      [bytes] -> bytes
      _ -> error ("missing fixture region " ++ name)
    backing state = replace 0x19c (shadowBytes state) (replace 0 (liveBytes state) (original "backing"))
    initialBanks = replace 0 header (original "banks")
    (final,drivers) = if operation == CloseHeader then closeMirror keys header old else (old,[])
    finalBanks = if operation == OpenHeader then replace 0 (shadowBytes old) initialBanks else initialBanks
    events = requestEdges context (driverEvents (pairByte initialBanks) drivers)
    initialize r
      | regionBytesOfId r == RegionId "banks" = region "banks" 0x96cc64 ReadWrite initialBanks
      | regionBytesOfId r == RegionId "backing" = region "backing" 0x409c281c ReadWrite (backing old)
      | otherwise = r

sequenceCase :: CaseSpec -> String -> P.Fixture -> TaskContext -> [BackupKey] -> M.Map PropertyId Property -> [StorageOperation] -> CaseSpec
sequenceCase template label fx context keys properties operations = template
  { caseId = label
  , caseProvenance = "X-M1 1.50 E01/E03/E04/E06/E07/E08/E09/E14 storage sequence " ++ label
  , caseExclusion = "Area2/header startup and custom payload are independently supplied backing inputs. Area3 lifecycle is outside increments 1 and 2."
  , caseEntry = GuestAddress (fromIntegral caller)
  , caseRegions = map extend (caseRegions template)
  , caseFunctions = caseFunctions template ++ [(GuestAddress (fromIntegral caller),fromIntegral (B.length callerBytes))]
  , caseExpectedRegisters = filter ((/= R0) . fst) (caseExpectedRegisters template)
  , caseExpectedMemory = [(RegionId "banks",P.fixtureBanks finalFixture),(RegionId "previous",P.fixturePrevious finalFixture),(RegionId "config",P.fixtureConfig finalFixture),(RegionId "calls",eventBytes (storageEvents final)),(RegionId "backing",storageBacking final)]
  }
  where
    caller = logHelper + 0x100
    initialBacking = case [regionBytes (caseRegion r) | r <- caseRegions template, regionBytesOfId r == RegionId "backing"] of
      [bytes] -> bytes
      _ -> error "missing sequence backing"
    final = maybe (error "sequence outside admitted fixture") id (foldM (modelOperation keys context) (StorageState fx initialBacking []) operations)
    finalFixture = storageFixture final
    callerBytes = assemble (0xe92d4010 : program (caller+4) operations ++ [0xe8bd8010])
    program _ [] = []
    program site (operation:rest) =
      let (entry,args,beforeCall) = case operation of
            Capture -> (0x61db3c,[],[])
            CopyActiveToPrevious -> (0x61db04,[0x89ec00,0x96cc64],[])
            Exchange direction -> (0x61daa8,[case direction of CaptureAlternate -> 0; RestoreAlternate -> 1; IgnoreInvalidDirection -> 2],[])
            Restore -> (0x61db18,[],[])
            Initialize _ -> (0x630a34,[],[])
            FieldWrite c _ -> (0x624694,arguments c,[])
            ApplyField c _ -> (0x6309c4,take 3 (arguments c),[])
            RetainedHeaderWrite (HeaderByteValue value) -> (0x6a6ddc,[0x409c281e,fromIntegral value],[0xe5c01000])
          setup = concat [mov register value | (register,value) <- zip [0..] args] ++ beforeCall
          callSite = site + fromIntegral (length setup*4)
      in setup ++ [0xeb000000 .|. (branch callSite entry .&. 0xffffff)] ++ program (callSite+4) rest
    arguments c = case P.oracleOperation c of
      P.Set bank slot@(P.SlotArgument slotWord) choice -> [identifier c,P.setValue (P.standingOf properties (P.oracleProperty c)) slot choice,fromIntegral (fromEnum bank),slotWord]
      _ -> error "sequence field operation must be a setter"
    identifier c = let PropertyId value = P.oracleProperty c in value
    extend r
      | regionBytesOfId r == RegionId "helper" = region "helper" logHelper ReadExecute (assemble helperWords <> B.replicate (0x100-length helperWords*4) 0 <> callerBytes)
      | otherwise = r

data RequestOperation = ReadRequest | WriteRequest deriving (Eq, Show)
newtype RequestLength = RequestLength Word32
newtype RequestOffset = RequestOffset Word32

requestCase :: CaseSpec -> String -> TaskContext -> RequestOperation -> RequestLength -> RequestOffset -> [BackupKey] -> CaseSpec
requestCase template label context operation (RequestLength rawLength) (RequestOffset rawOffset) keys = template
  { caseId = label
  , caseEntry = GuestAddress (if operation == ReadRequest then 0x72b3a4 else 0x72b3d0)
  , caseInitial = (R0,0x958d00) : filter ((/= R0) . fst) (caseInitial template)
  , caseExpectedRegisters = [(r,if r == R0 then result else value) | (r,value) <- caseExpectedRegisters template]
  , caseRegions = caseRegions template ++ [region "descriptor" 0x958d00 ReadWrite (B.concat (map word32le [0x958800,rawLength,rawOffset])),region "request-buffer" 0x958800 ReadWrite buffer]
  , caseExpectedMemory = [(RegionId "banks",original "banks"),(RegionId "previous",original "previous"),(RegionId "calls",eventBytes events),(RegionId "backing",finalBacking)] ++ [(RegionId "request-buffer",observedPrefix) | operation == ReadRequest && admitted && not (B.null observedPrefix)]
  , caseIgnoredMemory = caseIgnoredMemory template ++ [(IgnoredMemory (RegionId "request-buffer") initializedCount (fromIntegral length' - initializedCount) CallerOutput) | operation == ReadRequest && admitted && initializedCount < fromIntegral length']
  }
  where
    original name = case [regionBytes (caseRegion r) | r <- caseRegions template, regionBytesOfId r == RegionId name] of
      [bytes] -> bytes
      _ -> error ("missing request region " ++ name)
    length' = rawLength .&. 0xffff
    offset = rawOffset .&. 0xffff
    end = offset + length'
    admitted = end <= 0x19a
    result = if admitted then 0 else 0xffffffc1
    buffer = B.pack [fromIntegral (i*11+3) | i <- [0..0x199::Int]]
    backing = original "backing"
    assembled = B.take 16 (B.drop 0x19c backing) <> B.take (0x168-16) (B.drop 16 backing)
    initializedCount = max 0 (min (fromIntegral length') (0x168-fromIntegral offset))
    observedPrefix = B.take initializedCount (B.drop (fromIntegral offset) assembled)
    imported = replace (fromIntegral offset) (B.take (fromIntegral length') buffer) backing
    (mirror,drivers) = closeMirror keys (B.take 16 imported) (MirrorState (B.take 16 imported) (B.take 16 (B.drop 0x19c backing)))
    finalBacking = if admitted && operation == WriteRequest then replace 0 (liveBytes mirror) (replace 0x19c (shadowBytes mirror) imported) else backing
    effects = if admitted && operation == WriteRequest then driverEvents (pairByte (original "banks")) drivers ++ [Event 0x6a86c4 0 0 0 | end > 0x140] else []
    events = requestEdges context effects

data StockControl = StockControl String [(Word32,Word32,Word32)]

stockControls :: [StockControl]
stockControls =
  [StockControl "open-reads-live-not-shadow" [(0x6a6ec8,0xe2841f67,0xe1a01004)]
  ,StockControl "checksum-includes-byte15" [(0x6a6df0,0xe280000e,0xe280000f)]
  ,StockControl "resync-skips-final-byte" [(0x6a6e5c,0xe3540020,0xe354001e)]
  ,StockControl "driver-write-all-bytes" [(0x6a6e40,0x0a000004,0xe1a00000)]
  ,StockControl "request-status-not-mapped" [(0x72b3c8,0x13e0003e,0xe1a00000),(0x72b3f4,0x13e0003e,0xe1a00000)]
  ,StockControl "range-exclusive-end" [(0x6a6eac,0xe300319a,0xe3003199),(0x6a6f7c,0xe300319a,0xe3003199)]
  ,StockControl "refresh-at-end140" [(0x6a6fa8,0xe3560d05,0xe3560f4f)]
  ,StockControl "skip-refresh-end141" [(0x6a6fa8,0xe3560d05,0xe3560f51)]
  ,StockControl "descriptor-uses-fullwords" [(0x72b3b4,0xe1d310b4,0xe5931004),(0x72b3b8,0xe1d320b8,0xe5932008),(0x72b3e0,0xe1d310b4,0xe5931004),(0x72b3e4,0xe1d320b8,0xe5932008)]
  ]

mutateStock :: StockControl -> CaseSpec -> Either String CaseSpec
mutateStock (StockControl _ patches) spec = do
  changed <- mapM patchRegion (caseRegions spec)
  pure spec { caseRegions = changed }
  where
    patchRegion r = case [window | window <- codeWindows, windowRegion window == regionBytesOfId r] of
      [] -> Right r
      [window] -> do
        let applicable = [change | change@(at,_,_) <- patches, windowBase window <= at && at+4 <= windowEnd window]
        bytes <- foldM (patch (windowBase window)) (regionBytes (caseRegion r)) applicable
        let RegionId name = windowRegion window
        pure (region name (windowBase window) ReadExecute bytes)
      _ -> Left "duplicate code window"
    patch base bytes (at,before,after)
      | readWordAt bytes (fromIntegral (at-base)) /= before = Left ("stock control word mismatch at " ++ show at)
      | otherwise = Right (replace (fromIntegral (at-base)) (word32le after) bytes)

refusalCase :: NativeImage -> M.Map PropertyId Property -> P.Fixture -> Owned -> CaseSpec
refusalCase source properties fx owned = base
  { caseId = "header-refused-setter-success"
  , caseProvenance = "E07-E09 request-admission exception fixture: underlying export/import are declared refused (-30); stock short adapters, header wrappers, reporter and property setter execute unchanged. Ordinary mirror cases execute the complete real stock chain."
  , caseRegions = map refuse (caseRegions base)
  , caseExpectedMemory = [(ident,if ident == RegionId "calls" then eventBytes events else if ident == RegionId "previous" then replace 0x1c4 (word32le 0xffffffc1) bytes else bytes) | (ident,bytes) <- caseExpectedMemory base]
  }
  where
    noHook = case owned of OwnedField arm -> OwnedField arm { fieldHook = NoHook }; _ -> error "refusal fixture requires an ordinary header field"
    base = propertyCase source properties fx Task (P.OracleCase (PropertyId 1) (P.Set P.Bank0 (P.SlotArgument 0) P.SetOne)) noHook
    events = concat (replicate 2 (requestEdges Task [] ++ [Event 0x633594 0xffffffc1 0 0]))
    refuse r
      | regionBytesOfId r == RegionId "code-mirror" = region "code-mirror" 0x6a59b8 ReadExecute (foldl (\bytes at -> replace (fromIntegral (at-0x6a59b8)) (assemble (backend at)) bytes) (regionBytes (caseRegion r)) [0x6a6e68,0x6a6f3c])
      | otherwise = r
    backend at = [0xe92d4010,0xe3a00025,0xe3e01000,0xeb000000 .|. (branch (at+12) 0x724b68 .&. 0xffffff),0xe3a00025,0xeb000000 .|. (branch (at+20) 0x724c00 .&. 0xffffff),0xe3e0001d,0xe8bd8010]

data StorageMatrix = StorageMatrix
  { matrixFixtures :: [P.Fixture]
  , matrixFields :: [CaseSpec]
  , matrixMirrors :: [CaseSpec]
  , matrixSequences :: [CaseSpec]
  , matrixRequests :: [CaseSpec]
  , matrixHandoverReads :: [CaseSpec]
  , matrixCallerFrames :: [CaseSpec]
  }

matrixCases :: StorageMatrix -> [CaseSpec]
matrixCases matrix = matrixFields matrix ++ matrixMirrors matrix ++ matrixSequences matrix ++ matrixRequests matrix ++ matrixHandoverReads matrix ++ matrixCallerFrames matrix

storageMatrix :: NativeImage -> M.Map PropertyId Property -> P.Fixture -> [P.OracleCase] -> StorageMatrix
storageMatrix source properties fx grid = StorageMatrix inputs fields mirrors sequences requests handoverReads callerFrames
  where
    variants = [P.CanonicalFixture,P.ConfigV1,P.ConfigV2,P.ConfigV3,P.TailWindow]
    inputs = [P.variantFixture fx variant | variant <- variants]
    fields = [(propertyCase source properties (P.variantFixture fx variant) Task c owned) {caseId = P.variantTag variant ++ "-" ++ P.caseLabelText (P.caseLabel c)}
      | variant <- variants,c <- grid,Just owned <- [P.modelled (P.standingOf properties (P.oracleProperty c))],variant == P.CanonicalFixture || (variant == P.TailWindow && P.oracleProperty c == PropertyId 0x1e6 && P.oracleOperation c `elem` P.defaultOperations) || (variant `elem` P.configVariantsOf && P.oracleOperation c `elem` P.variantOperations owned)]
    ownedFor identifier = maybe (error ("unmodeled sequence property " ++ show identifier)) id (P.modelled (P.standingOf properties (PropertyId identifier)))
    template context = propertyCase source properties fx context (P.OracleCase (PropertyId 1) (P.Set P.Bank0 (P.SlotArgument 0) P.SetZero)) (ownedFor 1)
    zero = B.replicate 16 0
    headers = [("stable",zero,zero),("none",zero,zero),("last-shadow",zero,replace 15 (B.singleton 1) zero),("all",B.replicate 16 0xff,zero)] ++ [("byte-" ++ show i,replace i (B.singleton 1) zero,zero) | i <- [0..15]] ++ [("sum-" ++ show total,B.pack (take 15 (replicate (total `div` 255) 255 ++ [fromIntegral (total `mod` 255)] ++ repeat 0)) <> B.singleton 73,zero) | total <- [0,255,256,511::Int]]
    contextName Task = "task"
    contextName Interrupt = "interrupt"
    operationName OpenHeader = "open"
    operationName CloseHeader = "close"
    mirrors = [mirrorCase (template context) ("mirror-" ++ contextName context ++ "-" ++ operationName operation ++ "-" ++ label) context operation header (MirrorState (if label == "stable" then shadow else B.replicate 16 0xa7) shadow) (backupKeys source) | context <- [Task,Interrupt],operation <- [OpenHeader,CloseHeader],(label,header,shadow) <- headers]
    field identifier bank selection choice = let c = P.OracleCase (PropertyId identifier) (P.Set bank selection choice) in FieldWrite c (ownedFor identifier)
    applied identifier bank choice = let c = P.OracleCase (PropertyId identifier) (P.Set bank (P.SlotArgument 1) choice) in ApplyField c (ownedFor identifier)
    sequenceInputs =
      [("capture-change-restore",[Capture,field 1 P.Bank0 (P.SlotArgument 0) P.SetOne,field 2 P.Bank0 (P.SlotArgument 0) P.SetOne,Restore])
      ,("copy-to-previous",[CopyActiveToPrevious])
      ,("exchange-capture",[Exchange CaptureAlternate])
      ,("exchange-restore",[Exchange RestoreAlternate])
      ,("exchange-invalid",[Exchange IgnoreInvalidDirection])
      ,("exchange-capture-change-restore",[Exchange CaptureAlternate,field 0x34 P.Bank0 (P.SlotArgument 1) P.SetOne,Exchange RestoreAlternate])
      ,("repeat-field",[field 0x49 P.Bank0 (P.SlotArgument 1) P.SetOne,field 0x49 P.Bank0 (P.SlotArgument 1) P.SetZero])
      ,("disjoint-fields",[field 1 P.Bank0 (P.SlotArgument 0) P.SetOne,field 2 P.Bank0 (P.SlotArgument 0) P.SetZero])
      ,("handover-pending-restore",[field 0x34 P.Bank0 (P.SlotArgument 1) P.SetOne,Restore])
      ,("handover-shared-retained",[RetainedHeaderWrite (HeaderByteValue 5),field 1 P.Bank0 (P.SlotArgument 0) P.SetOne])
      ,("initialize",[Initialize (ownedFor 0x49)])
      ,("reinitialize",[Initialize (ownedFor 0x49),applied 0x49 P.Bank0 P.SetOne,Initialize (ownedFor 0x49)])]
      ++ [("default-" ++ show identifier,[Capture,field identifier P.Bank0 (P.SlotArgument 1) P.SetDefaultRequest]) | identifier <- [0x34,0x38,0xf6,0xf9,0xfa,0x15b,0x15c,0x15d,0x49,0xa9]]
      ++ [("e9-selector-" ++ show index,[field 0xe9 P.Bank0 selection P.SetDefaultRequest,field 0xe9 P.Bank0 selection P.SetOne]) | (index,selection) <- zip [0::Int ..] P.slotArguments]
      ++ [("apply-" ++ show identifier ++ "-bank-" ++ show (fromEnum bank),[Capture,applied identifier bank P.SetOne,Restore]) | identifier <- [0,0x49],bank <- [P.Bank0,P.Bank1]]
    sequences = [sequenceCase (propertyCase source properties variantFx Task (P.OracleCase (PropertyId 1) (P.Set P.Bank0 (P.SlotArgument 0) P.SetZero)) (ownedFor 1)) ("sequence-" ++ P.variantTag variant ++ "-" ++ label) variantFx Task (backupKeys source) properties actions | variant <- variants,let variantFx = P.variantFixture fx variant,(label,actions) <- sequenceInputs]
    handoverReads = [(propertyCase source properties fx Task (P.OracleCase (PropertyId 1) (P.Get P.Bank0 P.LiveRead (P.SlotArgument 1))) (ownedFor 1)) {caseId = "handover-adopt-read"}]
    requestName ReadRequest = "read"
    requestName WriteRequest = "write"
    requests = [requestCase (template Task) ("request-" ++ requestName operation ++ "-" ++ show count ++ "-" ++ show offset) Task operation (RequestLength count) (RequestOffset offset) (backupKeys source) | operation <- [ReadRequest,WriteRequest],(count,offset) <- [(0,0),(0x140,0),(0x141,0),(0x168,0),(0x19a,0),(0x19b,0),(0x10010,0),(16,0x10000),(12,0x128),(40,0x140)]]
    callerFrames =
      [ withCallerFrame label (pick name)
      | (label,name) <-
          [ ("caller-open-abi","mirror-task-open-stable")
          , ("caller-close-abi","mirror-task-close-stable")
          , ("caller-read-request-full","request-read-410-0")
          , ("caller-read-request-prefix","request-read-360-0")
          , ("caller-write-request-full","request-write-410-0")
          , ("caller-write-request-prefix","request-write-360-0")
          , ("caller-sequence-capture-restore","sequence-canonical-capture-change-restore")
          ]
      ]
    pick name = case [spec | spec <- mirrors ++ sequences ++ requests,caseId spec == name] of
      [spec] -> spec
      _ -> error ("missing caller-frame witness " ++ name)

callerFrameBytes :: B.ByteString
callerFrameBytes = B.pack [fromIntegral (0x41 + index) | index <- [0..0xf :: Int]]

withCallerFrame :: String -> CaseSpec -> CaseSpec
withCallerFrame label spec = spec
  { caseId = label
  , caseRegions = map patch (caseRegions spec)
  }
  where
    patch r
      | regionBytesOfId r == RegionId "stack"
      , Just pointer <- lookup Sp (caseInitial spec) =
          let view = caseRegion r
              offset = fromInteger (toInteger pointer - baseAddress view)
          in r { caseRegion = view { regionBytes = replace offset callerFrameBytes (regionBytes view) } }
      | otherwise = r
requiredControls :: [String]
requiredControls =
  ["wrong-bank-stride","slot-projections-independent","wrapped-selector-rejected","save-on-bank1","halfword-default-truncated"
  ,"snapshot-entire-bank-per-set","second-save-omitted","save-after-final-store","restore-without-body-write","e9-record0-mirror-omitted"
  ,"load-whole-bank-from-area3","zero-loader-gaps","tail-loaded-from-record","initialize-only-active-bank","skip-startup-resave"
  ,"open-reads-live-not-shadow","read-bank-before-refresh","close-before-field-store","checksum-includes-byte15","checksum-written-into-bank"
  ,"resync-skips-final-byte","driver-write-all-bytes","close-appends-area3","bracket-bank1","pair-close-after-both-writes","request-status-not-mapped"
  ,"range-exclusive-end","refresh-at-end140","skip-refresh-end141","descriptor-uses-fullwords"
  ,"body-write-on-unassigned","body-not-refreshed","application-appends-bank-as-area3","commit-clears-previous","handover-reinitialize-all-banks","mirror-private-copy-diverges","full-export-tail-zero-invented"]

controlWitnesses :: String -> [CaseSpec] -> [CaseSpec] -> [CaseSpec] -> [CaseSpec]
controlWitnesses name fields mirrors sequences
  | name == "halfword-default-truncated" = fieldIds [0x1e6]
  | name `elem` ["slot-projections-independent","wrapped-selector-rejected","e9-record0-mirror-omitted"] = fieldIds [0xe9]
  | name `elem` ["wrong-bank-stride","save-on-bank1","save-after-final-store"] = fieldIds [1,0x34]
  | name `elem` ["read-bank-before-refresh","close-before-field-store","bracket-bank1","pair-close-after-both-writes"] = fieldIds [1,0x283]
  | name `elem` ["checksum-written-into-bank","close-appends-area3","mirror-private-copy-diverges"] = mirrors ++ fieldIds [1]
  | name == "handover-reinitialize-all-banks" = [spec | spec <- fields, "handover-" `isPrefixOf` caseId spec]
  | otherwise = sequences
  where
    fieldIds ids = [spec | spec <- fields, any (\identifier -> let raw = showHex identifier "" in ("-p" ++ replicate (3-length raw) '0' ++ raw ++ "-") `isInfixOf` caseId spec) (ids :: [Int])]
