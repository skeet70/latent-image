{-# LANGUAGE OverloadedStrings #-}
module Emulation.CriticalPairWitness
  ( Contract, deriveContract, contractValue, validateReceipt, compareReceipts, maxReceiptBytes
  ) where

import qualified A32 as A
import Control.Monad (foldM, forM_, unless, zipWithM_)
import Data.Aeson (FromJSON (..), Value, object, withObject, withText, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import Data.Bits (complement, rotateR, shiftL, shiftR, testBit, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Int (Int32)
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32, Word64)
import qualified Emulation.CriticalPair as C
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import qualified Emulation.InterfaceInitializer as I
import Emulation.InterfaceInitializer (Context (..), Views (..), Source (..), ByteOrder (..), Event (..), Mode (..), PhysicalSpan (..), Window (..), BankSnapshot (..), TcmSnapshot (..), WatchClass (..), FixedViews (..), NoFault (..), Rule (..), Watch (..), Alias (..), Decoder (..))
import qualified FujiAF.FirstStage as F
import FujiAF.JSON (decodeStrictIntegers)
import qualified FujiAF.UpdateContainer as U

newtype Physical = Physical Word64 deriving (Eq, Ord)
newtype FrameBytes = FrameBytes Int
newtype CodeBytes = CodeBytes B.ByteString
newtype ImageDigest = ImageDigest F.Sha256
newtype SymbolDigest = SymbolDigest F.Sha256
newtype SourceDigest = SourceDigest F.Sha256

data Resource = Resource Physical Views deriving (Eq)
instance FromJSON Resource where
  parseJSON = withObject "critical resource" $ \r -> Resource . Physical <$> r .: "start" <*> r .: "views"

data Contract = Contract
  { initializerContract :: I.Contract
  , pairRole :: I.Role
  , sourceDigest :: SourceDigest
  , inputDigest :: SourceDigest
  , image4Digest :: ImageDigest
  , carrierDigest :: ImageDigest
  , symbolsDigest :: Maybe SymbolDigest
  , pairEntry :: Word32
  , pairExit :: Word32
  , entryFrame :: FrameBytes
  , exitFrame :: FrameBytes
  , pairCarrier :: CodeBytes
  , canonicalPair :: CodeBytes
  , canonicalWrapper :: CodeBytes
  , canonicalLoaderA :: CodeBytes
  , canonicalLoaderB :: CodeBytes
  , expectedResources :: [Resource]
  }

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

slice :: Int -> Int -> B.ByteString -> Either String B.ByteString
slice start count bytes = do
  require (start >= 0 && count > 0 && start + count <= B.length bytes) "critical source slice exceeds its image"
  pure (B.take count (B.drop start bytes))

loaderBytes :: U.Parts -> Word32 -> Either String B.ByteString
loaderBytes parts ident = case filter ((== ident) . U.segId) (U.partSegments parts) of
  [segment] -> B.map complement <$> slice (fromIntegral (U.segFlash segment)) (fromIntegral (U.segSize segment)) (U.partPreTable parts)
  _ -> Left "critical loader segment is not unique"

resource :: Word64 -> B.ByteString -> Resource
resource start bytes = Resource (Physical start) (Views (B.unpack bytes) (B.unpack bytes))

wordBytes :: [Word32] -> B.ByteString
wordBytes = B.concat . map U.putWord32

-- Decoded inputs are the decoder's outputs for the supplied DAT records.
deriveContract :: I.Role -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Either String Contract
deriveContract role source image4 candidate decoded image symbolBytes = do
  original <- C.deriveOriginal source image4
  parsedSource <- U.parseContainer source
  (staged, enter, exit, enterBytes, exitBytes) <- case role of
    I.Retained -> do
      require (candidate == source && decoded == image4 && B.null image && B.null symbolBytes) "retained critical witness requires original inputs only"
      pure (source, 0x7235d8, 0x7236b0, 40, 32)
    I.Owned -> do
      artifact <- C.deriveLinkedArtifact image symbolBytes
      staging <- C.stageStartup original artifact
      C.verifyFinalDat staging candidate
      require (decoded == C.patchedImage4 staging) "critical candidate decoded image4 differs from the confined branch patches"
      let C.WitnessLayout (C.Guest enter) (C.Guest exit) (C.FrameBytes enterBytes) (C.FrameBytes exitBytes) = C.witnessLayout artifact
      pure (C.startupDat staging, enter, exit, enterBytes, exitBytes)
  initializer <- I.deriveContract role source staged image symbolBytes
  parts <- U.parsedParts <$> U.parseContainer staged
  carrier <- F.recordSlotBytes F.SecondRecord parts >>= slice 0x400 0x1400 . B.map complement
  let sourceParts = U.parsedParts parsedSource
  loaderA <- loaderBytes sourceParts 0
  loaderB <- loaderBytes sourceParts 1
  stagedA <- loaderBytes parts 0
  stagedB <- loaderBytes parts 1
  pair <- slice (0x7235d8 - 0x61c400) 440 image4
  wrapper <- slice (0x725dd8 - 0x61c400) 704 image4
  livePair <- slice (0x7235d8 - 0x61c400) 440 decoded
  phase4 <- slice (0x69cdf8 - 0x61c400) (0x69cf38 - 0x69cdf8) decoded
  tailBytes <- slice 0xab0 0x40 stagedB
  sourceHash <- SourceDigest <$> F.parseSha256 (sha256Hex source)
  inputHash <- SourceDigest <$> F.parseSha256 (sha256Hex candidate)
  imageHash <- ImageDigest <$> F.parseSha256 (sha256Hex image4)
  carrierHash <- ImageDigest <$> F.parseSha256 (sha256Hex carrier)
  symbolsHash <- if role == I.Retained then pure Nothing else Just . SymbolDigest <$> F.parseSha256 (sha256Hex symbolBytes)
  pure (Contract initializer role sourceHash inputHash imageHash carrierHash symbolsHash enter exit (FrameBytes enterBytes) (FrameBytes exitBytes)
    (CodeBytes carrier) (CodeBytes pair) (CodeBytes wrapper) (CodeBytes loaderA) (CodeBytes loaderB)
    [resource 0x10400 carrier,resource 0x7235d8 livePair,resource 0x725dd8 wrapper,resource 0x96bd20 (wordBytes [0x4004dae0,0x400ddaa0])
    ,resource 0x4dae0 B.empty,resource 0xddaa0 B.empty,resource 0xa0f824 B.empty,resource 0xc0804 B.empty
    ,resource 0x40000 stagedA,resource 0xd0000 stagedB,resource 0x69cdf8 phase4,resource 0xd0ab0 tailBytes])

roleName :: I.Role -> String
roleName I.Retained = "retained"
roleName I.Owned = "owned"

contractValue :: Contract -> Value
contractValue c = let
  SourceDigest source = sourceDigest c
  SourceDigest input = inputDigest c
  ImageDigest image4 = image4Digest c
  ImageDigest carrier = carrierDigest c
  FrameBytes enterBytes = entryFrame c
  FrameBytes exitBytes = exitFrame c
  CodeBytes pair = canonicalPair c
  CodeBytes wrapper = canonicalWrapper c
  CodeBytes loaderA = canonicalLoaderA c
  CodeBytes loaderB = canonicalLoaderB c
  CodeBytes carrierBytes = pairCarrier c
  in object ["schema" .= (10 :: Int),"initializer" .= I.contractValue (initializerContract c),"pair" .= object
    ["role" .= roleName (pairRole c),"source_sha256" .= F.sha256Hex source,"input_sha256" .= F.sha256Hex input,"image4_sha256" .= F.sha256Hex image4
    ,"image_sha256" .= F.sha256Hex carrier,"symbols_sha256" .= fmap (\(SymbolDigest digest) -> F.sha256Hex digest) (symbolsDigest c)
    ,"carrier" .= B.unpack carrierBytes,"entry" .= pairEntry c,"exit" .= pairExit c,"entry_frame_bytes" .= enterBytes,"exit_frame_bytes" .= exitBytes
    ,"original_pair" .= B.unpack pair,"wrapper" .= B.unpack wrapper,"loader_a" .= B.unpack loaderA,"loader_b" .= B.unpack loaderB]]


newtype InitializerEvidence = InitializerEvidence B.ByteString
instance FromJSON InitializerEvidence where
  parseJSON value = pure (InitializerEvidence (L.toStrict (J.encode value)))

data Result = Passed | Failed deriving (Eq)
instance FromJSON Result where
  parseJSON = withText "critical result" $ \value -> case value of
    "passed" -> pure Passed
    "failed" -> pure Failed
    _ -> fail "unknown critical result"

newtype AccessBytes = AccessBytes [Word8] deriving (Eq)
instance FromJSON AccessBytes where
  parseJSON value = AccessBytes <$> parseJSON value

newtype Tick = Tick Word64 deriving (Eq, Ord)
instance FromJSON Tick where
  parseJSON value = Tick <$> parseJSON value

data PairEvent = PairEvent Event (Maybe AccessBytes) Tick deriving (Eq)
instance FromJSON PairEvent where
  parseJSON value = withObject "critical event" (\row -> do
    location <- row .:? "tcm" :: Parser (Maybe Value)
    unless (location == Nothing) (fail "critical event uses a TCM translation")
    PairEvent <$> parseJSON value <*> row .: "before" <*> (Tick <$> row .: "tick")) value

data Empty = Empty deriving (Eq)
instance FromJSON Empty where
  parseJSON = J.withArray "empty critical watch extension" $ \items -> if null items then pure Empty else fail "unsupported critical watch counters or coverage"

data PairWatch = PairWatch (Watch PairEvent) [(Word64,Word64)] Empty Empty deriving (Eq)
instance FromJSON PairWatch where
  parseJSON value = withObject "critical watch" (\r -> PairWatch <$> parseJSON value <*> r .: "ranges" <*> r .: "counters" <*> r .: "coverage") value

data Boundary = Boundary Context (Maybe Context) [Context] [Resource] [Resource] (Maybe PairWatch) deriving (Eq)
instance FromJSON Boundary where
  parseJSON = withObject "critical boundary" $ \r -> Boundary <$> r .: "incoming" <*> r .: "returned" <*> r .: "steps" <*> r .: "before" <*> r .: "after" <*> r .: "watch"

data PairReceipt = PairReceipt I.Role Context (Maybe Boundary) (Maybe PairWatch) (Maybe Boundary) (Maybe Context) (Maybe PairWatch) (Maybe PairWatch) [Resource] (Maybe Resource) (Maybe Resource) Decoder [Alias]
instance FromJSON PairReceipt where
  parseJSON = withObject "critical evidence" $ \r ->
    PairReceipt <$> r .: "role" <*> r .: "prelude_start" <*> r .: "entry" <*> r .: "transition_watch" <*> r .: "exit" <*> r .: "returned" <*> r .: "prelude_watch" <*> r .: "caller_watch" <*> r .: "resources" <*> r .: "caller_frame_before" <*> r .: "caller_frame_after" <*> r .: "decoder" <*> r .: "aliases"

data Receipt = Receipt Word32 Result (Maybe String) (Maybe String) InitializerEvidence (Maybe PairReceipt)
instance FromJSON Receipt where
  parseJSON = withObject "schema10 receipt" $ \r -> Receipt <$> r .: "schema" <*> r .: "result" <*> r .: "error" <*> r .: "machine_failure" <*> r .: "initializer" <*> r .: "pair"

maxReceiptBytes :: Int
maxReceiptBytes = 64 * 1024 * 1024

viewsBytes :: Views -> Either String B.ByteString
viewsBytes (Views virtual committed) = do
  require (virtual == committed) "critical virtual and physical views differ"
  pure (B.pack committed)

pcOf :: Context -> Word32
pcOf (Context pc _ _ _ _ _ _) = pc
regsOf :: Context -> [Word32]
regsOf (Context _ registers sp lr _ _ _) = registers ++ [sp,lr]
cpsrOf :: Context -> Word32
cpsrOf (Context _ _ _ _ cpsr _ _) = cpsr
countsOf :: Context -> [Word64]
countsOf (Context _ _ _ _ _ counts _) = counts
tcmOf :: Context -> [[Word32]]
tcmOf (Context _ _ _ _ _ _ tcm) = tcm

contextShape :: Context -> Either String ()
contextShape context@(Context pc registers sp _ cpsr counts tcm) = do
  require (length registers == 13 && length counts == 2 && all (<= 50000000) counts && sum counts <= 50000000) "critical context register/count shape or budget differs"
  require (pc .&. 3 == 0 && sp .&. 3 == 0 && cpsr .&. 0x07f0fe20 == 0 && cpsr .&. 0x5f == 0x53) "critical context is not admitted little-endian A32 SVC state"
  require (length tcm == 2 && all ((== 2) . length) tcm && length (regsOf context) == 15) "critical context TCM shape differs"

snapshots :: Context -> Either String [TcmSnapshot]
snapshots context = case tcmOf context of
  [[a,b],[c,d]] -> sequence [TcmSnapshot 0 <$> I.bankSnapshot a <*> I.bankSnapshot b,TcmSnapshot 1 <$> I.bankSnapshot c <*> I.bankSnapshot d]
  _ -> Left "critical context lacks both-core raw TCM registers"

spans :: [(Word64,Word64)]
spans = [(0x10400,0x11800),(0x7235d8,0x723790),(0x725dd8,0x726098),(0x96bd20,0x96bd28),(0x4dae0,0x4f10c),(0xddaa0,0xdf0cc),(0xa0f824,0xa0f828),(0xc0804,0xc0808),(0x40000,0x4dae0),(0xd0000,0xddaa0),(0x69cdf8,0x69cf38),(0xd0ab0,0xd0af0)]

immutableSpans :: [(Word64,Word64)]
immutableSpans = [span' | (index,span') <- zip [0 :: Int ..] spans,index /= 6 && index /= 7]

resourceBytes :: Resource -> Either String (Word64,B.ByteString)
resourceBytes (Resource (Physical address) views) = (address,) <$> viewsBytes views

validateResources :: Contract -> I.ValidatedInitializer -> [Resource] -> Either String ()
validateResources contract initializer resources = do
  require (length resources == 12) "critical common resource coverage differs"
  zipWithM_ (\(low,high) observed -> do
    (address,bytes) <- resourceBytes observed
    require (address == low && fromIntegral (B.length bytes) == high-low) "critical resource order or extent differs") spans resources
  zipWithM_ (\expected observed -> do
    (_,bytes) <- resourceBytes expected
    (_,actual) <- resourceBytes observed
    require (B.null bytes || actual == bytes) "critical resource differs from the admitted source/staged bytes") (expectedResources contract) resources
  case drop 5 resources of
    Resource _ table:_ -> require (table == I.initializerTable initializer) "critical tableB differs from the independently validated initializer"
    _ -> Left "critical tableB is absent"
  tableA <- snd <$> resourceBytes (resources !! 4)
  let CodeBytes loaderA = canonicalLoaderA contract
  publications <- tablePublications loaderA
  require (B.length loaderA == 0xdae0 && length publications == 1419) "critical canonical loaderA publication extent differs"
  forM_ publications $ \(offset,value) ->
    require (leWordAt tableA offset == Right value) "critical complete tableA source publication differs"

tablePublications :: B.ByteString -> Either String [(Int,Word32)]
tablePublications loader = do
  let immediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
      wide low high = (.|.) <$> (immediate <$> leWordAt loader low) <*> ((`shiftL` 16) . immediate <$> leWordAt loader high)
  services <- mapM (\index -> do
    let offset = 0x440+12*index
    low <- leWordAt loader offset
    high <- leWordAt loader (offset+4)
    store <- leWordAt loader (offset+8)
    require (low .&. 0xfff0f000 == 0xe3002000 && high .&. 0xfff0f000 == 0xe3402000 && store == 0xe5832000+fromIntegral index*4) "critical loaderA service source encoding differs"
    pure (index*4,immediate low .|. (immediate high `shiftL` 16))) [0..93]
  extensions <- mapM (uncurry wide) [(0x8a8,0x8b0),(0x8b8,0x8c0),(0x8c8,0x8d0),(0x8d8,0x8e0),(0x8e8,0x8f0)]
  pool <- wide 0x8fc 0x900
  extended <- wide 0x904 0x908
  let descriptor index =
        (0x178+index*44,fromIntegral index+1)
        : [(0x17c+index*44+field*4,pool+fromIntegral (index*40+field*4)) | field <- [0..7]]
        ++ [(0x19c+index*44,extended+fromIntegral (index*56+24)),(0x1a0+index*44,extended+fromIntegral (index*56+28))]
  pure (services++zip [0x1618,0x161c..] extensions++concatMap descriptor [0..119])

physicalAddress :: Word64 -> Either String Word64
physicalAddress address
  | address < 0x40000000 = Right address
  | address >= 0x40000000 && address < 0x41000000 = Right (address-0x40000000)
  | otherwise = Left "critical access has no admitted linear RAM alias"

data Phase = Prelude | Transition | BoundaryPhase Word64 Word64

watchClasses :: Phase -> [WatchClass]
watchClasses phase = let
  writes = [CpuWrite,HostWrite,ModelBusWrite]
  boundary = case phase of Prelude -> False; _ -> True
  kinds index
    | boundary = CpuFetch:CpuRead:writes
    | index `elem` [3,4,5,6,7] = CpuRead:writes
    | index `elem` [0,1,2,10,11] = CpuFetch:writes
    | otherwise = writes
  base = [WatchClass (Window span') (kinds index) | (index,span') <- zip [0 :: Int ..] spans]
  extra = case phase of
    BoundaryPhase low high -> [WatchClass (Window (low,high)) (CpuFetch:CpuRead:writes),WatchClass (Window (0xf33a8,0xf33e0)) (CpuRead:writes)]
    Prelude -> [WatchClass (Window (0x7caf34,0x7caf38)) (CpuRead:writes),WatchClass (Window (0xc080c,0xc0810)) [CpuRead]]
    Transition -> [WatchClass (Window (0xf33a8,0xf33e0)) (CpuRead:writes)]
  in base ++ extra ++ [WatchClass (Window (0x10000,0x14c80)) writes]

watchRules :: Contract -> Phase -> [TcmSnapshot] -> [Rule]
watchRules contract phase banks = let
  rule cores kind mode ranges = Rule cores kind mode [PhysicalSpan [low,high] | (low,high) <- ranges] [bank | bank@(TcmSnapshot core _ _) <- banks,core `elem` cores]
  record = [(0x10000,0x14c80)]
  disjoint = filter (`notElem` [(0x10400,0x11800),(0xd0ab0,0xd0af0)])
  primary = case phase of
    BoundaryPhase low high -> [rule [1] CpuWrite Allow [(0xa0f824,0xa0f828),(0xc0804,0xc0808),(low,high)],rule [0] CpuWrite Forbid (disjoint spans++[(low,high),(0xf33a8,0xf33e0)]++record)]
    Prelude -> [rule [1] CpuWrite Forbid (disjoint immutableSpans++[(0x7caf34,0x7caf38)]++record),rule [0] CpuWrite Forbid (disjoint spans++[(0x7caf34,0x7caf38)]++record)]
    Transition -> [rule [1] CpuWrite Forbid (disjoint immutableSpans++record),rule [0] CpuWrite Forbid (disjoint spans++[(0xf33a8,0xf33e0)]++record)]
  in primary ++ [rule [1] kind Forbid [(0xd0430,0xd09ac)] | pairRole contract == I.Owned,kind <- [CpuRead,CpuFetch]]

validateWatch :: Contract -> Phase -> Context -> PairWatch -> Either String [PairEvent]
validateWatch contract phase origin (PairWatch (Watch enabled violation events rules classes FixedViews NoFault banks) ranges Empty Empty) = do
  expectedBanks <- snapshots origin
  require (enabled && violation == Nothing && sort banks == sort expectedBanks) "critical watch was disabled, violated or changed TCM scope"
  let normalize (WatchClass window kinds) = WatchClass window (sort kinds)
  require (sort (map normalize classes) == sort (map normalize (watchClasses phase)) && sort ranges == sort [range | WatchClass (Window range) _ <- classes]) "critical watch classes or ranges differ"
  require (rules == watchRules contract phase expectedBanks) "critical confinement rules differ"
  let ticks = [tick | PairEvent _ _ tick <- events]
  require (and (zipWith (<=) ticks (drop 1 ticks))) "critical watch ticks decreased"
  forM_ events $ \(PairEvent (Event source core address translated width value pc clock order) before _) -> do
    require (core `elem` [Just 0,Just 1] && width `elem` [4,8] && pc /= Nothing && clock /= Nothing && order == Just LittleEndian) "critical event attribution, width or byte order differs"
    physical <- physicalAddress address
    require (translated == Just physical) "critical event physical provenance differs"
    require (maybe False (<= 50000001) clock) "critical event exceeds the instruction budget"
    case source of
      CpuFetch -> require (width == 4 && before == Nothing && value == Nothing && physical == address && pc == Just (fromIntegral address)) "critical fetch metadata differs"
      CpuRead -> checkData width before value
      CpuWrite -> checkData width before value
      _ -> Left "critical protected resource has a host/model access"
  pure events
  where
    checkData width (Just (AccessBytes bytes)) (Just value) =
      require (fromIntegral (length bytes) == width && (width == 8 || value <= 0xffffffff)) "critical data preimage or value width differs"
    checkData _ _ _ = Left "critical data event lacks its value or preimage"

wordEvents :: [PairEvent] -> [PairEvent]
wordEvents = concatMap split
  where
    split (PairEvent (Event source core address (Just physical) 8 (Just value) pc count order) (Just (AccessBytes bytes)) tick) =
      [PairEvent (Event source core (address+offset) (Just (physical+offset)) 4 (Just ((value `shiftR` fromIntegral (offset*8)) .&. 0xffffffff)) pc count order)
        (Just (AccessBytes (take 4 (drop (fromIntegral offset) bytes)))) tick | offset <- [0,4]]
    split event = [event]

memory :: [Resource] -> Either String (M.Map Word64 Word8)
memory resources = M.fromList . concat <$> mapM (\r -> do
  (start,bytes) <- resourceBytes r
  pure (zip [start..] (B.unpack bytes))) resources

readMemory :: M.Map Word64 Word8 -> Word64 -> Either String Word32
readMemory bytes start = do
  word <- mapM (\address -> maybe (Left "critical instruction read leaves observed resources") Right (M.lookup address bytes)) [start..start+3]
  leWordAt (B.pack word) 0

writeMemory :: M.Map Word64 Word8 -> Word64 -> Word32 -> M.Map Word64 Word8
writeMemory bytes start value = foldr (uncurry M.insert) bytes (zip [start..] (B.unpack (U.putWord32 value)))

register :: Context -> A.Reg -> Word32
register context (A.Reg index)
  | index == 15 = pcOf context + 8
  | otherwise = regsOf context !! index

condition :: Word32 -> Word32 -> Bool
condition cpsr value = let n = testBit cpsr 31; z = testBit cpsr 30; c = testBit cpsr 29; v = testBit cpsr 28 in case value of
  0 -> z; 1 -> not z; 2 -> c; 3 -> not c; 4 -> n; 5 -> not n; 6 -> v; 7 -> not v
  8 -> c && not z; 9 -> not c || z; 10 -> n == v; 11 -> n /= v; 12 -> not z && n == v; 13 -> z || n /= v; 14 -> True; _ -> False

shiftValue :: A.Shift -> Int -> Word32 -> Word32
shiftValue kind amount value = case kind of
  A.LSL -> if amount >= 32 then 0 else value `shiftL` amount
  A.LSR -> if amount >= 32 then 0 else value `shiftR` amount
  A.ASR -> fromIntegral ((fromIntegral value :: Int32) `shiftR` min 31 amount)
  A.ROR -> rotateR value amount

operand :: Context -> A.Operand -> Either String Word32
operand context value = case value of
  A.Immediate n -> pure n
  A.ShiftedImmediate reg kind amount -> do
    require (amount /= 0 || kind == A.LSL) "critical zero shift has unadmitted carry semantics"
    pure (shiftValue kind amount (register context reg))
  A.ShiftedRegister reg kind amount -> pure (shiftValue kind (fromIntegral (register context amount .&. 0xff)) (register context reg))

arithmetic :: A.DataOp -> Word32 -> Word32 -> Word32 -> Either String (Word32,Maybe (Bool,Bool))
arithmetic operation left right cpsr = let
  addition x y carryIn = let wide = toInteger x + toInteger y + carryIn; result = fromInteger wide in (result,Just (wide > 0xffffffff,testBit (complement (x `xor` y) .&. (x `xor` result)) 31))
  subtraction x y borrow = let wide = toInteger x - toInteger y - borrow; result = fromInteger wide in (result,Just (wide >= 0,testBit ((x `xor` y) .&. (x `xor` result)) 31))
  carry = if testBit cpsr 29 then 1 else 0
  in pure $ case operation of
    A.ADD -> addition left right 0; A.CMN -> addition left right 0; A.ADC -> addition left right carry
    A.SUB -> subtraction left right 0; A.CMP -> subtraction left right 0; A.SBC -> subtraction left right (1-carry)
    A.RSB -> subtraction right left 0; A.RSC -> subtraction right left (1-carry)
    A.AND -> (left .&. right,Nothing); A.TST -> (left .&. right,Nothing); A.EOR -> (left `xor` right,Nothing); A.TEQ -> (left `xor` right,Nothing)
    A.ORR -> (left .|. right,Nothing); A.MOV -> (right,Nothing); A.MVN -> (complement right,Nothing); A.BIC -> (left .&. complement right,Nothing)

flags :: Word32 -> Word32 -> Maybe (Bool,Bool) -> Word32
flags old result carryOverflow = (old .&. 0x0fffffff) .|. (result .&. 0x80000000) .|. (if result == 0 then 0x40000000 else 0) .|. case carryOverflow of
  Nothing -> old .&. 0x30000000
  Just (carry,overflow) -> (if carry then 0x20000000 else 0) .|. (if overflow then 0x10000000 else 0)

data Machine = Machine Context (M.Map Word64 Word8) [PairEvent]

-- Core-1 data events are consumed by their source instruction.
step :: Contract -> Machine -> Context -> Either String Machine
step contract (Machine before bytes pendingEvents) after = do
  events <- drainPeerReads bytes pendingEvents
  let pc = pcOf before
      cpsr = cpsrOf before
      readReg = register before
      oldRegisters = regsOf before
      set values = foldl (\registers (A.Reg index,value) -> take index registers ++ [value] ++ drop (index+1) registers) oldRegisters values
      finish registers next status updated remaining = do
        require (regsOf after == registers && pcOf after == next && cpsrOf after == status) "critical instruction register, PC or CPSR result differs"
        require (tcmOf before == tcmOf after) "critical boundary changed raw TCM registers"
        case (countsOf before,countsOf after) of
          ([a,b],[c,d]) -> require (c >= a && d == b+1) "critical steps omit or invent a core1 instruction"
          _ -> Left "critical step counters have invalid width"
        pure (Machine after updated remaining)
      unchanged = finish oldRegisters (pc+4) cpsr bytes events
      load address = do
        physical <- physicalAddress (fromIntegral address)
        value <- readMemory bytes physical
        remaining <- consume CpuRead physical value events
        pure (value,remaining)
      store address value = do
        physical <- physicalAddress (fromIntegral address)
        _ <- readMemory bytes physical
        remaining <- consume CpuWrite physical value events
        pure (writeMemory bytes physical value,remaining)
      consume kind physical value pending = case pending of
        PairEvent (Event source (Just 1) _ (Just location) 4 (Just actual) (Just site) (Just clock) (Just LittleEndian)) (Just (AccessBytes preimage)) _ : rest -> do
          require (source == kind && location == physical && actual == fromIntegral value && site == pc && clock > sum (countsOf before) && clock <= sum (countsOf after)) "critical instruction data event differs"
          require (readMemory bytes physical == leWordAt (B.pack preimage) 0) "critical data preimage differs from chronological memory"
          pure rest
        _ -> Left "critical instruction has no matching attributed data event"
      transfer base offset indexing = do
        delta <- case offset of
          A.OffsetImmediate value -> pure (fromIntegral value)
          A.OffsetRegister positive reg kind amount -> pure ((if positive then id else negate) (shiftValue kind amount (readReg reg)))
        let original = readReg base; updated = original+delta
        pure (if indexing == A.PostIndexed then original else updated,[(base,updated) | indexing /= A.OffsetOnly])
      block mode base registers = let
        count = fromIntegral (length registers) * 4
        initial = readReg base
        (start,end) = case mode of A.IncrementAfter -> (initial,initial+count); A.IncrementBefore -> (initial+4,initial+count); A.DecrementAfter -> (initial-count+4,initial-count); A.DecrementBefore -> (initial-count,initial-count)
        in (zip registers [start,start+4..],end)
  word <- readMemory bytes (fromIntegral pc)
  let inPair = pc >= 0x7235d8 && pc < 0x723790
      inCarrier = pc >= 0x10400 && pc < 0x11800
      inHelper = any (\(low,high) -> pc >= low && pc < high) [(0x41140,0x4118c),(0x412bc,0x41320),(0x427b8,0x42830),(0xd1140,0xd118c),(0xd12bc,0xd1320),(0xd27b8,0xd2830)]
      admittedPair = case pairRole contract of I.Retained -> inPair; I.Owned -> pc `elem` [0x7235d8,0x7236b0] || inCarrier
  require (admittedPair || inHelper) "critical boundary executed an undeclared instruction"
  if not (condition cpsr (word `shiftR` 28)) then unchanged else
    case A.decode pc word of
      A.Branch _ link target -> finish (set [(A.lr,pc+4) | link]) target cpsr bytes events
      A.BranchExchange _ link reg -> do
        require (readReg reg .&. 3 == 0) "critical indirect branch leaves A32"
        finish (set [(A.lr,pc+4) | link]) (readReg reg) cpsr bytes events
      A.MoveWide _ reg value -> finish (set [(reg,value)]) (pc+4) cpsr bytes events
      A.MoveTop _ reg value -> finish (set [(reg,(readReg reg .&. 0xffff) .|. (value `shiftL` 16))]) (pc+4) cpsr bytes events
      A.ZeroExtend _ destination source rotation width ->
        finish (set [(destination,rotateR (readReg source) rotation .&. ((1 `shiftL` width)-1))]) (pc+4) cpsr bytes events
      A.Multiply _ destination left right accumulator ->
        finish (set [(destination,readReg left*readReg right+maybe 0 readReg accumulator)]) (pc+4) cpsr bytes events
      A.DataProcessing _ operation update destination left right -> do
        value <- operand before right
        (result,carry) <- arithmetic operation (readReg left) value cpsr
        let test = operation `elem` [A.CMP,A.CMN,A.TST,A.TEQ]
        require (not (update && carry == Nothing && case right of A.Immediate _ -> word .&. 0xf00 /= 0; A.ShiftedImmediate _ _ n -> n /= 0; A.ShiftedRegister {} -> True)) "critical flag-setting logical shift is outside its oracle"
        let newFlags = if update then flags cpsr result carry else cpsr
        if destination == A.pc && not test
          then finish oldRegisters result newFlags bytes events
          else finish (set [(destination,result) | not test]) (pc+4) newFlags bytes events
      A.Load _ A.Word False destination base offset indexing -> do
        (address,writeback) <- transfer base offset indexing
        (value,remaining) <- load address
        finish (set (writeback++[(destination,value) | destination /= A.pc])) (if destination == A.pc then value else pc+4) cpsr bytes remaining
      A.Load _ A.Dual False destination@(A.Reg index) base offset indexing -> do
        require (index < 12 && even index) "critical dual load register pair differs"
        (address,writeback) <- transfer base offset indexing
        physical <- physicalAddress (fromIntegral address)
        first <- readMemory bytes physical
        second <- readMemory bytes (physical+4)
        pending <- consume CpuRead physical first events
        remaining <- consume CpuRead (physical+4) second pending
        finish (set (writeback++[(destination,first),(A.Reg (index+1),second)])) (pc+4) cpsr bytes remaining
      A.Store _ A.Word source base offset indexing -> do
        (address,writeback) <- transfer base offset indexing
        (updated,remaining) <- store address (readReg source)
        finish (set writeback) (pc+4) cpsr updated remaining
      A.Store _ A.Dual source@(A.Reg index) base offset indexing -> do
        require (index < 12 && even index) "critical dual store register pair differs"
        (address,writeback) <- transfer base offset indexing
        physical <- physicalAddress (fromIntegral address)
        pending <- consume CpuWrite physical (readReg source) events
        remaining <- consume CpuWrite (physical+4) (readReg (A.Reg (index+1))) pending
        finish (set writeback) (pc+4) cpsr (writeMemory (writeMemory bytes physical (readReg source)) (physical+4) (readReg (A.Reg (index+1)))) remaining
      A.LoadMultiple _ base mode writeback registers -> do
        let (locations,end) = block mode base registers
        (values,remaining) <- foldM (\(values,pending) (reg,address) -> do
          physical <- physicalAddress (fromIntegral address)
          value <- readMemory bytes physical
          rest <- consume CpuRead physical value pending
          pure (values++[(reg,value)],rest)) ([],events) locations
        finish (set ([(base,end) | writeback]++[(reg,value) | (reg,value) <- values,reg /= A.pc])) (maybe (pc+4) id (lookup A.pc values)) cpsr bytes remaining
      A.StoreMultiple _ base mode writeback registers -> do
        let (locations,end) = block mode base registers
        (updated,remaining) <- foldM (\(current,pending) (reg,address) -> do
          physical <- physicalAddress (fromIntegral address)
          rest <- consume CpuWrite physical (readReg reg) pending
          pure (writeMemory current physical (readReg reg),rest)) (bytes,events) locations
        finish (set [(base,end) | writeback]) (pc+4) cpsr updated remaining
      A.Hint _ -> require (word .&. 0x0fffffff == 0x0320f000) "critical hint is outside its oracle" >> unchanged
      _
        | word .&. 0x0fff0fff == 0x010f0000 -> let reg = A.Reg (fromIntegral ((word `shiftR` 12) .&. 15)) in finish (set [(reg,cpsr)]) (pc+4) cpsr bytes events
        | word .&. 0x0ffffff0 `elem` [0x0121f000,0x0128f000,0x012cf000] -> do
            let reg = A.Reg (fromIntegral (word .&. 15)); mask = case (word `shiftR` 16) .&. 15 of 1 -> 0xff; 8 -> 0xff000000; 12 -> 0xffff0000; _ -> 0
            finish oldRegisters (pc+4) ((cpsr .&. complement mask) .|. (readReg reg .&. mask)) bytes events
        | word == 0xee121e10 && pc `elem` [0x427b8,0xd27b8] -> finish (set [(A.Reg 1,0x5003)]) (pc+4) cpsr bytes events
        | word .&. 0x0fffffff == 0x0e072fba && pc `elem` [0x427ec,0x42818,0xd27ec,0xd2818] -> unchanged
        | word .&. 0x0ff00fff == 0x01900f9f -> do
            let base = A.Reg (fromIntegral ((word `shiftR` 16) .&. 15)); destination = A.Reg (fromIntegral ((word `shiftR` 12) .&. 15))
            (value,remaining) <- load (readReg base)
            finish (set [(destination,value)]) (pc+4) cpsr bytes remaining
        | word .&. 0x0ff00ff0 == 0x01800f90 -> do
            let base = A.Reg (fromIntegral ((word `shiftR` 16) .&. 15)); destination = A.Reg (fromIntegral ((word `shiftR` 12) .&. 15)); source = A.Reg (fromIntegral (word .&. 15)); status = register after destination
            require (status <= 1) "critical exclusive status is neither success nor failure"
            if status == 0 then do
              (updated,remaining) <- store (readReg base) (readReg source)
              finish (set [(destination,0)]) (pc+4) cpsr updated remaining
            else finish (set [(destination,1)]) (pc+4) cpsr bytes events
        | otherwise -> Left "critical instruction is outside the independent execution oracle"

boundaryResult :: Boundary -> Either String Context
boundaryResult (Boundary _ returned _ _ _ _) = maybe (Left "critical boundary did not return") Right returned

boundaryIncoming :: Boundary -> Context
boundaryIncoming (Boundary incoming _ _ _ _ _) = incoming

validateBoundary :: Contract -> Resource -> [PairEvent] -> FrameBytes -> Boundary -> Either String (Context,M.Map Word64 Word8,[PairEvent])
validateBoundary contract callerFrame preceding (FrameBytes frameBytes) boundary@(Boundary incoming _ steps before after watch) = do
  returned <- boundaryResult boundary
  mapM_ contextShape (incoming:returned:steps)
  first <- case steps of [] -> Left "critical boundary contains no instruction steps"; context:_ -> Right context
  require (pcOf first == pcOf incoming && regsOf first == regsOf incoming && cpsrOf first == cpsrOf incoming
    && tcmOf first == tcmOf incoming && drop 1 (countsOf first) == drop 1 (countsOf incoming)
    && head (countsOf first) >= head (countsOf incoming)) "critical boundary steps do not start at the natural branchsite"
  let sp = fromIntegral (register incoming A.sp); low = sp-fromIntegral frameBytes-0x40000000; high = sp-0x40000000
  require (length before == 13 && length after == 13) "critical boundary resource coverage differs"
  zipWithM_ (\(start,end) r -> do (address,bytes) <- resourceBytes r; require (address == start && fromIntegral (B.length bytes) == end-start) "critical boundary resource extent differs") (spans++[(low,high)]) before
  zipWithM_ (\(Resource address a) (Resource address' b) -> do
    a' <- viewsBytes a; b' <- viewsBytes b; require (address == address' && B.length a' == B.length b') "critical after-resource geometry differs") before after
  observedWatch <- maybe (Left "critical boundary watch is absent") Right watch
  events <- validateWatch contract (BoundaryPhase low high) incoming observedWatch
  forM_ events $ \(PairEvent (Event _ _ _ _ _ _ _ clock _) _ _) ->
    require (maybe False (\count -> count >= sum (countsOf incoming) && count <= sum (countsOf returned)+1) clock) "critical boundary event is outside its chronological window"
  let dataEvents = wordEvents [event | event@(PairEvent (Event source _ _ _ _ _ _ _ _) _ _) <- events,source /= CpuFetch]
  validateBoundaryFetches incoming returned steps preceding events
  initialMemory <- memory (before++[callerFrame])
  Machine _ finalMemory pending <- foldM (step contract) (Machine first initialMemory dataEvents) (drop 1 steps++[returned])
  remaining <- drainPeerReads finalMemory pending
  require (null remaining) "critical boundary contains unconsumed data effects"
  afterMemory <- memory (after++[callerFrame])
  require (finalMemory == afterMemory) "critical boundary committed readback differs from instruction effects"
  let beforeRegisters = regsOf incoming; afterRegisters = regsOf returned
  require (all (\index -> beforeRegisters !! index == afterRegisters !! index) [4..11] && register incoming A.sp == register returned A.sp) "critical boundary violated its callee-saved or SP ABI"
  pure (returned,finalMemory,events)

validatePrelude :: Context -> Context -> [Resource] -> [PairEvent] -> Either String ()
validatePrelude origin incoming resources events = do
  let fetched = [site | PairEvent (Event CpuFetch (Just 1) _ _ _ _ (Just site) _ _) _ _ <- events]
      expected = [0xd0ab0,0xd0ab4..0xd0aec] ++ [0x69cdf8,0x69cdfc..0x69ce08]
        ++ [0x69ce44,0x69ce48,0x69ce90,0x69ce94,0x69ce98,0x69ce9c,0x69cea0,0x69cea4]
        ++ [0x69ceec,0x69cef0..0x69cf34] ++ [0x725dd8,0x725ddc..0x725dfc] ++ [0x7235d8]
      selector = [(value,site) | PairEvent (Event CpuRead (Just 1) _ (Just 0x7caf34) _ (Just value) (Just site) _ _) _ _ <- events]
      magic = [(value,site) | PairEvent (Event CpuRead (Just 1) _ (Just 0xc080c) _ (Just value) (Just site) _ _) _ _ <- events]
  require (fetched == expected && selector == [(2,0x725df8)]) "critical natural phase4/task103 selector path differs"
  require (not (null magic) && last magic == (0x87654321,0xd391c) && all (\(_,site) -> site `elem` [0xd38b8,0xd38cc,0xd391c]) magic) "critical prelude lacks natural peer magic completion"
  require (tcmOf origin == tcmOf incoming && sum (countsOf origin) < sum (countsOf incoming)) "critical prelude changed TCM or failed to advance naturally"
  protected <- memory resources
  forM_ events $ \(PairEvent (Event source core _ physical width value site clock _) before _) -> do
    require (width == 4 && source /= CpuWrite && maybe False (\n -> n >= sum (countsOf origin) && n <= sum (countsOf incoming)+1) clock) "critical prelude changed a protected resource or has an out-of-window event"
    case (source,core,physical,value,site,before) of
      (CpuFetch,Just 1,_,_,_,_) -> pure ()
      (CpuRead,Just 1,Just 0x7caf34,Just 2,Just 0x725df8,Just (AccessBytes bytes)) ->
        require (leWordAt (B.pack bytes) 0 == Right 2) "critical task103 selector preimage differs"
      (CpuRead,_,Just 0xc080c,Just observed,Just pc,Just (AccessBytes bytes)) -> do
        require (pc `elem` [0xd38b8,0xd38cc,0xd391c] && leWordAt (B.pack bytes) 0 == Right (fromIntegral observed)) "critical natural peer magic provenance differs"
      (CpuRead,Just 0,Just physical',Just observed,_,Just (AccessBytes bytes))
        | physical' >= 0xddaa0 && physical'+4 <= 0xdf0cc -> do
            expected <- readMemory protected physical'
            require (observed == fromIntegral expected && leWordAt (B.pack bytes) 0 == Right expected) "critical peer prelude table read differs from validated publication"
      _ -> Left "critical prelude contains an unclassified protected access"

validateAliases :: Context -> [[Resource]] -> [Alias] -> Either String ()
validateAliases origin snapshots' observed = do
  banks <- snapshots origin
  expected <- concat <$> mapM (\resources -> concat <$> mapM bindings resources) snapshots'
  require (observed == expected) "critical low/high both-core alias readbacks differ"
  forM_ observed $ \(Alias core guest _ extent _) -> forM_ [bank | TcmSnapshot selected atcm btcm <- banks,selected == core,bank <- [atcm,btcm]] $ \bank -> case bank of
    BankSnapshot _ Nothing -> pure ()
    BankSnapshot _ (Just (Window (low,high))) -> require (guest+extent <= low || high <= guest) "critical alias overlaps an enabled TCM bank"
  where
    bindings r = do
      (start,bytes) <- resourceBytes r
      digest <- F.parseSha256 (sha256Hex bytes)
      pure [Alias core (start+alias) start (fromIntegral (B.length bytes)) digest | core <- [0,1],alias <- [0,0x40000000]]

data Accepted = Accepted Context Context Context Context Context [Resource] [Resource]

validateReceipt :: Contract -> B.ByteString -> Either String ()
validateReceipt contract bytes = validateEvidence contract bytes >> pure ()

validateEvidence :: Contract -> B.ByteString -> Either String Accepted
validateEvidence contract bytes = do
  require (B.length bytes <= maxReceiptBytes) "critical receipt exceeds 64 MiB"
  document <- decodeStrictIntegers bytes
  Receipt schema result failure machineFailure (InitializerEvidence nested) evidence <- case J.fromJSON document of
    J.Error problem -> Left problem
    J.Success receipt -> pure receipt
  initializer <- I.validateReceiptEvidence (initializerContract contract) nested
  require (schema == 10 && result == Passed && failure == Nothing && machineFailure == Nothing) "critical receipt did not pass its distinct schema10 role"
  PairReceipt role origin entryEvidence transitionEvidence exitEvidence finalEvidence preludeEvidence callerEvidence resources callerBeforeEvidence callerAfterEvidence decoder aliases <- maybe (Left "critical pair evidence is absent") Right evidence
  require (role == pairRole contract && origin == I.initializerExit initializer && decoder == Decoder [1] [Nothing]) "critical role, initializer continuation or decoder identity differs"
  entry <- maybe (Left "critical entry evidence is absent") Right entryEvidence
  exit <- maybe (Left "critical exit evidence is absent") Right exitEvidence
  final <- maybe (Left "critical original caller return is absent") Right finalEvidence
  prelude <- maybe (Left "critical prelude watch is absent") Right preludeEvidence
  transition <- maybe (Left "critical wrapper watch is absent") Right transitionEvidence
  caller <- maybe (Left "critical caller watch is absent") Right callerEvidence
  callerBefore <- maybe (Left "critical saved caller frame is absent") Right callerBeforeEvidence
  callerAfter <- maybe (Left "critical final caller frame is absent") Right callerAfterEvidence
  let incoming = boundaryIncoming entry; exitIncoming = boundaryIncoming exit
  mapM_ contextShape [origin,incoming,exitIncoming,final]
  require (pcOf incoming == 0x7235d8 && register incoming A.sp == 0x400f33a8 && register incoming A.lr == 0x725e00 && register incoming A.sp .&. 7 == 0 && cpsrOf incoming .&. 0x80 == 0x80) "critical entry was not the admitted natural original caller boundary"
  validateResources contract initializer resources
  let Boundary _ _ _ entryBefore entryAfter _ = entry
      Boundary _ _ _ exitBefore exitAfter _ = exit
  require (take 12 entryBefore == resources) "critical initial resources differ from entry snapshots"
  preludeEvents <- validateWatch contract Prelude origin prelude
  validatePrelude origin incoming resources preludeEvents
  (entryReturned,_,entryEvents) <- validateBoundary contract callerBefore preludeEvents (entryFrame contract) entry
  validateSourceEntry entry entryReturned entryEvents
  require (pcOf entryReturned == 0x725e00 && register entryReturned (A.Reg 0) == cpsrOf incoming .&. 0x80 && cpsrOf entryReturned .&. 0x80 == 0x80) "critical entry prior-mask result or masked IRQ state differs"
  require (pcOf exitIncoming == 0x7236b0 && register exitIncoming A.lr `elem` [0x725e88,0x725fb8,0x726068]
    && register exitIncoming A.sp == register incoming A.sp && register exitIncoming (A.Reg 0) == register entryReturned (A.Reg 0)
    && register exitIncoming (A.Reg 7) == register entryReturned (A.Reg 0)) "critical exit lacks an actual wrapper call and saved prior mask"
  transitionEvents <- validateWatch contract Transition entryReturned transition
  transitionFrame <- replayWrapper entryReturned exitIncoming entryAfter callerBefore transitionEvents
  validateTransition entryReturned exitIncoming transitionEvents entryAfter exitBefore
  (exitReturned,_,exitEvents) <- validateBoundary contract transitionFrame transitionEvents (exitFrame contract) exit
  validateSourceExit exit exitReturned exitEvents
  callerEvents <- validateWatch contract Transition exitReturned caller
  finalFrame <- replayWrapper exitReturned final exitAfter transitionFrame callerEvents
  require (finalFrame == callerAfter) "critical wrapper final committed frame differs from its chronological effects"
  validateCrossWindowTicks [preludeEvents,entryEvents,transitionEvents,exitEvents,callerEvents]
  validateCaller exitReturned final callerEvents
  require (pcOf exitReturned == register exitIncoming A.lr && cpsrOf exitReturned .&. 0x80 == register exitIncoming (A.Reg 0) .&. 0x80) "critical exit did not restore the prior IRQ mask at its actual caller return"
  require (pcOf final == 0x69cf38 && register final A.sp == 0x400f33e0 && tcmOf final == tcmOf origin) "critical original wrapper epilogue did not return to its caller"
  validateCallerFrame incoming final callerBefore callerAfter
  validateAliases origin [resources,[callerBefore],entryBefore,entryAfter,exitBefore,exitAfter,take 12 exitAfter,[callerAfter]] aliases
  pure (Accepted incoming entryReturned exitIncoming exitReturned final entryAfter exitAfter)

validateTransition :: Context -> Context -> [PairEvent] -> [Resource] -> [Resource] -> Either String ()
validateTransition entered exit events entryAfter exitBefore = do
  let fetched = [site | PairEvent (Event CpuFetch (Just 1) _ _ _ _ (Just site) _ _) _ _ <- events]
      callsite = register exit A.lr-4
  require (reverse (take 3 (reverse fetched)) == [callsite-4,callsite,0x7236b0]) "critical wrapper did not execute its saved-mask MOV, actual BL and natural exit fetch"
  require (all (\site -> (site >= 0x725e00 && site < 0x726098) || site == 0x7236b0) fetched) "critical transition fetched outside the original wrapper and exit endpoint"
  require (sum (countsOf entered) < sum (countsOf exit) && tcmOf entered == tcmOf exit) "critical wrapper transition counters or TCM differ"
  require (take 12 entryAfter == take 12 exitBefore && not (null exitBefore)) "critical wrapper changed a protected resource between boundaries"
  require (not (null events)) "critical wrapper transition evidence is absent"

validateCaller :: Context -> Context -> [PairEvent] -> Either String ()
validateCaller exited final events = do
  let fetched = [site | PairEvent (Event CpuFetch (Just 1) _ _ _ _ (Just site) _ _) _ _ <- events]
  require (reverse (take 2 (reverse fetched)) == [0x726090,0x726094]) "critical original wrapper epilogue was not executed"
  require (all (\site -> site >= pcOf exited && site < 0x726098) fetched && sum (countsOf exited) < sum (countsOf final)) "critical caller continuation route or counters differ"
  require (not (null events)) "critical caller continuation evidence is absent"

validateCallerFrame :: Context -> Context -> Resource -> Resource -> Either String ()
validateCallerFrame incoming final before after = do
  (start,bytes) <- resourceBytes before
  (end,returned) <- resourceBytes after
  require (start == 0xf33a8 && end == start && B.length bytes == 56 && B.length returned == 56
    && register incoming (A.Reg 11) == 0x400f33dc && B.drop 32 bytes == B.drop 32 returned) "critical original wrapper saved frame differs"
  require (all (\index -> register final (A.Reg index) == register incoming (A.Reg index)) [8,9,10]) "critical original wrapper changed an unsaved callee-saved register"
  zipWithM_ (\offset value -> require (leWordAt bytes offset == Right value) "critical original caller callee-saved restore differs") [32,36,40,44,48,52]
    [register final (A.Reg 4),register final (A.Reg 5),register final (A.Reg 6),register final (A.Reg 7),register final (A.Reg 11),pcOf final]

compareReceipts :: (Contract,B.ByteString) -> (Contract,B.ByteString) -> Either String ()
compareReceipts (retained,retainedBytes) (owned,ownedBytes) = do
  require (pairRole retained == I.Retained && pairRole owned == I.Owned) "critical comparison requires retained then owned contracts"
  Accepted a b c d e before after <- validateEvidence retained retainedBytes
  Accepted a' b' c' d' e' before' after' <- validateEvidence owned ownedBytes
  let visible context = (pcOf context,regsOf context,cpsrOf context,tcmOf context)
      effects resources = [(address,views) | Resource address@(Physical start) views <- resources,start `elem` [0x96bd20,0x4dae0,0xddaa0,0xa0f824,0xc0804]]
  require (map visible [a,b,c,d,e] == map visible [a',b',c',d',e'] && effects before == effects before' && effects after == effects after') "critical retained/owned caller ABI, counter, lock or publication effects differ"

replayWrapper :: Context -> Context -> [Resource] -> Resource -> [PairEvent] -> Either String Resource
replayWrapper origin returned resources frame events = do
  initial <- memory (resources++[frame])
  final <- foldM replay initial (wordEvents events)
  values <- mapM (\address -> maybe (Left "critical caller frame replay lost a byte") Right (M.lookup address final)) [0xf33a8..0xf33df]
  pure (resource 0xf33a8 (B.pack values))
  where
    replay bytes (PairEvent (Event source core _ (Just physical) _ value site clock _) before _) = do
      require (maybe False (\count -> count >= sum (countsOf origin) && count <= sum (countsOf returned)+1) clock) "critical wrapper event is outside its chronological window"
      case source of
        CpuFetch -> pure bytes
        CpuRead -> do
          actual <- readMemory bytes physical
          require (value == Just (fromIntegral actual) && before == Just (AccessBytes (B.unpack (U.putWord32 actual)))) "critical wrapper read or preimage differs from committed state"
          pure bytes
        CpuWrite -> do
          require (core == Just 1 && physical >= 0xf33a8 && physical+4 <= 0xf33c8) "critical wrapper wrote outside its original local frame"
          actual <- readMemory bytes physical
          require (before == Just (AccessBytes (B.unpack (U.putWord32 actual)))) "critical wrapper local write preimage differs"
          pc <- maybe (Left "critical wrapper write lacks a source PC") Right site
          word <- readMemory bytes (fromIntegral pc)
          require (case A.decode pc word of A.Store {} -> True; A.StoreMultiple {} -> True; _ -> False) "critical wrapper local write is not attributed to a canonical store"
          written <- maybe (Left "critical wrapper local write lacks its value") Right value
          pure (writeMemory bytes physical (fromIntegral written))
        _ -> Left "critical wrapper contains a non-CPU access"
    replay _ _ = Left "critical wrapper event lacks physical provenance"

data Service = Identity | Mask | Observe | Retry | Release | Restore deriving (Eq)
data ServiceCall = ServiceCall Service Context Context

serviceAt :: Word32 -> Maybe Service
serviceAt pc = lookup pc [(0x412bc,Identity),(0xd1140,Mask),(0xd1304,Observe),(0xd12cc,Retry),(0xd12e8,Release),(0xd1164,Restore)]

serviceCalls :: [Context] -> Either String [ServiceCall]
serviceCalls [] = pure []
serviceCalls (incoming:remaining) = case serviceAt (pcOf incoming) of
  Nothing -> serviceCalls remaining
  Just service -> do
    let suffix = dropWhile ((/= register incoming A.lr) . pcOf) remaining
    case suffix of
      [] -> Left "critical retained service has no observed real return"
      returned:rest -> (ServiceCall service incoming returned:) <$> serviceCalls rest

callKind :: ServiceCall -> Service
callKind (ServiceCall kind _ _) = kind

callValue :: ServiceCall -> Word32
callValue (ServiceCall _ _ returned) = register returned (A.Reg 0)

checkService :: Service -> ServiceCall -> Either String ()
checkService expected (ServiceCall actual incoming _) = do
  require (actual == expected) "critical retained-service order differs from source conditionals"
  require (expected `notElem` [Observe,Retry,Release] || register incoming (A.Reg 0) == 0) "critical retained lock service lacks its original zero argument"
  require (expected /= Retry || register incoming (A.Reg 1) == 0x22) "critical retry service lacks its counted core1 tag"

counterValue :: [Resource] -> Either String Word32
counterValue resources = do
  bytes <- memory resources
  readMemory bytes 0xa0f824

counterEffects :: Word32 -> Word32 -> Bool -> [PairEvent] -> Either String ()
counterEffects before after changed events = do
  let writes = [(value,preimage) | PairEvent (Event CpuWrite _ _ (Just 0xa0f824) 4 value _ _ _) preimage _ <- wordEvents events]
      expected = [(Just (fromIntegral after),Just (AccessBytes (B.unpack (U.putWord32 before)))) | changed]
  require (writes == expected) "critical wrapping counter effects differ from the observed source branch"

sourceScratch :: Context -> [Word32] -> Word32 -> Word32 -> Either String ()
sourceScratch returned scratch lr status = do
  require (map (register returned . A.Reg) [0,1,2,3,12] == scratch && register returned A.lr == lr && cpsrOf returned == status) "critical retained-result scratch, LR or CPSR ABI differs from original source"

validateSourceEntry :: Boundary -> Context -> [PairEvent] -> Either String ()
validateSourceEntry (Boundary incoming _ steps before after _) returned events = do
  calls <- serviceCalls (steps++[returned])
  old <- counterValue before
  new <- counterValue after
  case calls of
    identity:mask:first:remaining -> do
      checkService Identity identity
      require (callValue identity == 1) "critical source core identity is not natural core1"
      checkService Mask mask
      require (callValue mask == cpsrOf incoming .&. 0x80) "critical mask service returned a different prior IRQ bit"
      checkService Observe first
      let direct = callValue first == 0x12
          status = (cpsrOf incoming .&. 0x0fffffff) .|. 0x60000080
          prior = cpsrOf incoming .&. 0x80
          inherited = case mask of ServiceCall _ _ maskReturned -> register maskReturned (A.Reg 1)
          preservedR12 = register incoming (A.Reg 12)
      if direct then do
        require (null remaining && new == old) "critical direct-tag entry performed an extra observation, retry or counter update"
        counterEffects old old False events
        sourceScratch returned [prior,inherited,0x400ddaa0,0x12,preservedR12] 0x723620 status
      else case remaining of
        second:retries -> do
          checkService Observe second
          let counted = callValue second == 0x22
          if counted then require (null retries) "critical counted-tag entry retried despite its second observation"
          else do
            require (not (null retries)) "critical unmatched second observation omitted the retry loop"
            mapM_ (checkService Retry) retries
            require (all ((/= 0) . callValue) (init retries) && callValue (last retries) == 0) "critical retry loop did not stop on its first observed success"
          require (new == old+1) "critical entry counter increment is not wrapping u32 addition"
          counterEffects old new True events
          sourceScratch returned [prior,if counted then inherited else 0x22,new,0x40a0f3d0,preservedR12] (if counted then 0x723644 else 0x72368c) status
        [] -> Left "critical entry collapsed the source's separate first and second observations"
    _ -> Left "critical entry omitted its identity, mask or first observation service"

validateSourceExit :: Boundary -> Context -> [PairEvent] -> Either String ()
validateSourceExit (Boundary incoming _ steps before after _) returned events = do
  calls <- serviceCalls (steps++[returned])
  old <- counterValue before
  new <- counterValue after
  case calls of
    identity:observed:remaining -> do
      checkService Identity identity
      require (callValue identity == 1) "critical exit source identity differs"
      checkService Observe observed
      let direct = callValue observed == 0x12
          decrement = not direct && old /= 0
          expectedCounter = if decrement then old-1 else old
          release = decrement && expectedCounter == 0
          comparison = if decrement && not release then (if testBit expectedCounter 31 then 0xa0000000 else 0x20000000) else 0x60000000
          status = (cpsrOf incoming .&. 0x0fffff7f) .|. comparison .|. (register incoming (A.Reg 0) .&. 0x80)
          lr = if direct then 0x72370c else if old == 0 then 0x72373c else 0x72378c
          r2 = if decrement then if release then 0 else expectedCounter else 0x400ddaa0
      require (map callKind remaining == (if release then [Release,Restore] else [Restore])) "critical exit release/restore sequence differs from source counter branch"
      mapM_ (\call -> checkService (callKind call) call) remaining
      let ServiceCall _ restoreIncoming _ = last remaining
      require (register restoreIncoming (A.Reg 0) == register incoming (A.Reg 0)) "critical restore service did not receive the caller's saved prior mask"
      require (new == expectedCounter) "critical exit counter decrement or zero/direct skip differs"
      counterEffects old new decrement events
      sourceScratch returned [status,status .&. complement 0x80,r2,0xd1164,register incoming (A.Reg 12)] lr status
    _ -> Left "critical exit omitted source identity or observation"

validateBoundaryFetches :: Context -> Context -> [Context] -> [PairEvent] -> [PairEvent] -> Either String ()
validateBoundaryFetches incoming returned steps preceding events = do
  let fetched = [(site,clock) | PairEvent (Event CpuFetch (Just 1) _ _ _ _ (Just site) (Just clock) _) _ _ <- events]
      expected = [(pcOf context,sum (countsOf context)+1) | context <- steps]
      terminal = (pcOf returned,sum (countsOf returned)+1)
      withoutTerminal = if not (null fetched) && last fetched == terminal then init fetched else fetched
      prior = [(site,clock) | PairEvent (Event CpuFetch (Just 1) _ _ _ _ (Just site) (Just clock) _) _ _ <- preceding]
      handoff = not (null prior) && last prior == (pcOf incoming,sum (countsOf incoming)+1)
  require (handoff && (withoutTerminal == expected || withoutTerminal == drop 1 expected)) "critical step/fetch chronology or preexecution handoff differs"

drainPeerReads :: M.Map Word64 Word8 -> [PairEvent] -> Either String [PairEvent]
drainPeerReads bytes events = case events of
  PairEvent (Event CpuRead (Just 0) _ (Just physical) 4 (Just value) _ _ _) before _ : rest -> do
    actual <- readMemory bytes physical
    require (value == fromIntegral actual && before == Just (AccessBytes (B.unpack (U.putWord32 actual)))) "critical peer read differs from chronological protected memory"
    drainPeerReads bytes rest
  PairEvent (Event _ (Just 0) _ _ _ _ _ _ _) _ _ : _ -> Left "critical peer wrote a protected resource"
  _ -> pure events

validateCrossWindowTicks :: [[PairEvent]] -> Either String ()
validateCrossWindowTicks windows = do
  let ticks = [tick | events <- windows,PairEvent _ _ tick <- events]
  require (and (zipWith (<=) ticks (drop 1 ticks))) "critical event ticks decreased across continuous watch windows"
