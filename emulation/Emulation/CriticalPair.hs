{-# LANGUAGE OverloadedStrings #-}
module Emulation.CriticalPair
  ( Original, LinkedArtifact, Staging, deriveOriginal, deriveLinkedArtifact
  , stageStartup, startupDat, patchedImage4, stagingValue, verifyFinalDat
  ) where

import qualified A32 as A
import Control.Monad (foldM, forM, forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement, rotateR, shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Set as Set
import Data.Int (Int32)
import Data.Word (Word32)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import qualified Emulation.InterfaceInitializer as I
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import StageBuild (StageDeclarations (..))
import qualified StartupStageContract as S

newtype Guest = Guest Word32 deriving (Eq, Ord, Show)
newtype FrameBytes = FrameBytes Int
newtype InterfaceOffset = InterfaceOffset Word32
newtype RetainedCode = RetainedCode Word32
newtype ImageWordPatch = ImageWordPatch (Guest, Word32, Word32)
data Core = Core0 | Core1 deriving (Eq, Ord)

data Services = Services Core Guest F.Sha256 [(InterfaceOffset, RetainedCode)]
data Original = Original B.ByteString S.OriginalStartupDat B.ByteString [Services]
data LinkedArtifact = LinkedArtifact B.ByteString B.ByteString [F.Symbol] Guest Guest FrameBytes FrameBytes
data Staging = Staging B.ByteString B.ByteString B.ByteString [ImageWordPatch] [Services] LinkedArtifact

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

at :: [F.Symbol] -> String -> Either String Word32
at symbols name = F.symbolAddress <$> F.lookupSymbol symbols name

branch :: Bool -> Word32 -> Word32 -> Either String Word32
branch link site target = do
  let displacement = toInteger target - toInteger site - 8
  require (target .&. 3 == 0 && displacement >= -0x2000000 && displacement < 0x2000000)
    "critical-pair target is outside the A32 branch range"
  pure ((if link then 0xeb000000 else 0xea000000) .|. (fromInteger (displacement `div` 4) .&. 0xffffff))

wordsAt :: B.ByteString -> Int -> [Word32] -> Either String ()
wordsAt bytes offset expected = do
  observed <- mapM (leWordAt bytes . (offset +)) [0,4 .. length expected * 4 - 4]
  require (observed == expected) "critical-pair instruction contract differs"

loader :: U.Parts -> Core -> F.Sha256 -> Either String B.ByteString
loader parts core digest = do
  let (ident,base,extent) = case core of
        Core0 -> (0,0x40000,0xdae0)
        Core1 -> (1,0xd0000,0xdaa0)
  segment <- case filter ((== ident) . U.segId) (U.partSegments parts) of
    [found] -> Right found
    _ -> Left "critical-pair loader segment is not unique"
  let offset = fromIntegral (U.segFlash segment)
      count = fromIntegral (U.segSize segment)
      carrier = U.partPreTable parts
  require (U.segLoad segment == base && count == extent && offset + count <= B.length carrier)
    "critical-pair loader geometry differs"
  let bytes = B.map complement (B.take count (B.drop offset carrier))
  observed <- F.parseSha256 (sha256Hex bytes)
  require (observed == digest) "critical-pair loader identity differs"
  pure bytes

services :: Core -> Guest -> B.ByteString -> Either String Services
services core (Guest base) bytes = do
  let wide low high = do
        lo <- leWordAt bytes low
        hi <- leWordAt bytes high
        require (lo .&. 0xfff00000 == 0xe3000000 && hi .&. 0xfff00000 == 0xe3400000)
          "critical-pair publication is not MOVW/MOVT"
        let immediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
        pure (immediate lo .|. (immediate hi `shiftL` 16))
  root <- wide 0x438 0x43c
  header <- leWordAt bytes 0x10
  require (root == header) "critical-pair loader header does not name its published interface"
  slots <- forM [(0x164,0x86c,0x870,0x874),(0x168,0x878,0x87c,0x880)] $ \(offset,low,high,store) -> do
    target <- wide low high
    wordsAt bytes store [0xe5832000 + offset]
    pure (InterfaceOffset offset, RetainedCode target)
  extensions <- forM [(0x1618,0x8a8,0x8b0,0x8b4),(0x161c,0x8b8,0x8c0,0x8c4)
                    ,(0x1620,0x8c8,0x8d0,0x8d4),(0x1624,0x8d8,0x8e0,0x8e4)] $ \(offset,low,high,store) -> do
    target <- wide low high
    wordsAt bytes store [0xe7831002]
    pure (InterfaceOffset offset, RetainedCode target)
  require ([target | (_,RetainedCode target) <- slots ++ extensions]
    == map (base +) [0x1140,0x1164,0x12bc,0x12cc,0x12e8,0x1304])
    "critical-pair installed service targets differ"
  wordsAt bytes 0x8ac [0xe3012618]
  forM_ [0x8bc,0x8cc,0x8dc] $ \offset -> wordsAt bytes offset [0xe2822004]
  wordsAt bytes 0x1140 [0xe52db004,0xe28db000,0xe10f0000,0xe3801080,0xe121f001
                     ,0xe2000080,0xe28bd000,0xe8bd0800,0xe12fff1e]
  wordsAt bytes 0x1164 [0xe52db004,0xe28db000,0xe2000080,0xe10f1000,0xe3c11080
                     ,0xe1800001,0xe121f000,0xe28bd000,0xe8bd0800,0xe12fff1e]
  wordsAt bytes 0x12bc [0xe92d4800,0xe28db004,0xeb00053b,0xe8bd8800]
  forM_ [(0x12cc,0xeb00053b),(0x12e8,0xeb00053e),(0x1304,0xeb000542)] $ \(offset,call) ->
    wordsAt bytes offset [0xe92d4800,0xe28db004,0xe3500000,0x03000804,0x0344000c,call,0xe8bd8800]
  wordsAt bytes 0x27b8 [0xee121e10,0xe3a02901,0xe2822003,0xe1510002,0x03a00000,0x13a00001,0xe12fff1e]
  wordsAt bytes 0x27d4 [0xe1902f9f,0xe3520000,0x13e00000,0x112fff1e,0x01802f91,0xe3520000
                     ,0x0e072fba,0x03a00000,0x012fff1e,0xeafffff5]
  wordsAt bytes 0x27fc [0xe3a01000,0xe1902f9f,0xe3520000,0x03e00000,0x012fff1e,0x11802f91
                     ,0xe3520000,0x0e072fba,0x03a00000,0x012fff1e,0xeafffff4,0xe5900000,0xe12fff1e]
  digest <- F.parseSha256 (sha256Hex bytes)
  pure (Services core (Guest root) digest (slots ++ extensions))

deriveOriginal :: B.ByteString -> B.ByteString -> Either String Original
deriveOriginal dat image4 = do
  original <- I.parseOriginalSource dat
  require (sha256Hex image4 == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
    "critical-pair requires canonical decoded image4"
  let parts = U.parsedParts (S.originalStartupParsed original)
  primaryDigest <- F.parseSha256 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  secondaryDigest <- F.parseSha256 "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
  primary <- loader parts Core0 primaryDigest
  secondary <- loader parts Core1 secondaryDigest
  bindings <- sequence [services Core0 (Guest 0x40000) primary,services Core1 (Guest 0xd0000) secondary]
  wordsAt image4 (0x7235d8 - 0x61c400)
    [0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000,0xe3013618,0xe7923003
    ,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000]
  wordsAt image4 (0x725dd8 - 0x61c400) [0xe92d48f0,0xe28db014,0xe24dd020]
  wordsAt image4 (0x725dfc - 0x61c400) [0xebfff5f5,0xe1a07000]
  wordsAt image4 (0x725e80 - 0x61c400) [0xe1a00007,0xebfff609]
  wordsAt image4 (0x725fb0 - 0x61c400) [0xe1a00007,0xebfff5bd]
  wordsAt image4 (0x726060 - 0x61c400) [0xe1a00007,0xebfff591]
  wordsAt image4 (0x726090 - 0x61c400) [0xe24bd014,0xe8bd88f0]
  wordsAt image4 (0x725a0c - 0x61c400) [0xe30b0d20,0xe3400096,0xe3003000,0xe3403004,0xe5933010
    ,0xe5803000,0xe3003000,0xe340300d,0xe5933010,0xe5803004]
  pure (Original dat original image4 bindings)

adapterProof :: B.ByteString -> [F.Symbol] -> Either String ()
adapterProof image symbols = do
  origin <- at symbols "__secondary_stage_start"
  let check name expected = do
        symbol <- F.lookupSymbol symbols name
        require (F.symbolSize symbol == fromIntegral (length expected * 4)) "critical-pair adapter extent differs"
        wordsAt image (fromIntegral (F.symbolAddress symbol - origin)) expected
  enter <- at symbols "xm1_critical_enter_adapter"
  enterBody <- at symbols "xm1_critical_enter_continue"
  enterCall <- branch True (enter + 84) enterBody
  exit <- at symbols "xm1_critical_exit_adapter"
  exitBody <- at symbols "xm1_critical_exit"
  exitCall <- branch True (exit + 32) exitBody
  check "xm1_critical_enter_adapter" [0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000
    ,0xe3013618,0xe7923003,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000
    ,0xe24dd020,0xe88d100f,0xe58de014,0xe10f2000,0xe58d2018
    ,0xe1a0000d,0xe1a01004,0xe1a0200a,enterCall,0xe59d2018,0xe12cf002,0xe89d100f
    ,0xe59de014,0xe28dd020,0xe8bd8df0]
  check "xm1_critical_exit_adapter" [0xe92d4878,0xe28db014,0xe24dd020,0xe88d100f,0xe58de014
    ,0xe10f2000,0xe58d2018,0xe1a0000d,exitCall,0xe59d2018,0xe12cf002,0xe89d100f
    ,0xe59de014,0xe28dd020,0xe8bd8878]
  check "xm1_critical_retained_call" [0xe92d4070,0xe1a04000,0xe1a05001,0xe594e018,0xe12cf00e
    ,0xe894100f,0xe12fff35,0xe884100f,0xe10f0000,0xe5840018,0xe8bd8070]

-- Frame offsets are below incoming SP; Scratch fields occupy [SP, SP+28).
data Address = FrameAddress Int | ScratchAddress Int | InterfaceAddress Core Int deriving (Eq, Ord)
data ScratchField = ScratchR0 | ScratchR1 | ScratchR2 | ScratchR3 | ScratchR12 | ScratchLr | ScratchCpsr
  deriving (Eq, Ord, Enum, Bounded)
data ServiceKind = MaskIrq | RestoreIrq | IdentifyCore | RetryExclusive | ReleaseExclusive | ObserveWord
  deriving (Eq, Ord)
data PublishedTarget = PublishedTarget Core ServiceKind deriving (Eq, Ord)
data AbstractWord
  = UnknownWord
  | KnownWords (Set.Set Word32)
  | Addresses (Set.Set Address)
  | PublishedTargets (Set.Set PublishedTarget)
  | CallerWord A.Reg
  | ReturnWord
  deriving (Eq, Ord)
data Frame = Frame
  { frameRegisters :: M.Map A.Reg AbstractWord
  , frameWords :: M.Map Int AbstractWord
  , scratchWords :: M.Map ScratchField AbstractWord
  } deriving (Eq, Ord)
data Body = EntryBody | ExitBody
data MemoryAddress = Relative Address | Absolute Word32 deriving (Eq, Ord)
data TransferPlan = TransferPlan AbstractWord (Maybe AbstractWord)

bodyName :: Body -> String
bodyName EntryBody = "xm1_critical_enter_continue"
bodyName ExitBody = "xm1_critical_exit"

knownWord :: Word32 -> AbstractWord
knownWord = KnownWords . Set.singleton

addressWord :: Address -> AbstractWord
addressWord = Addresses . Set.singleton

boundedWords :: Set.Set Word32 -> AbstractWord
boundedWords values
  | Set.null values || Set.size values > 16 = UnknownWord
  | otherwise = KnownWords values

joinWords :: [AbstractWord] -> Either String AbstractWord
joinWords [] = Left "critical-pair Rust read has no possible target"
joinWords values@(first:rest)
  | all (== first) rest = pure first
  | all isNumber values = pure (boundedWords (Set.unions [words' | KnownWords words' <- values]))
  | all isAddress values = do
      let addresses = Set.unions [addresses' | Addresses addresses' <- values]
      require (Set.size addresses <= 16) "critical-pair Rust address alternatives exceed its proof"
      pure (Addresses addresses)
  | all isPublished values = pure (PublishedTargets (Set.unions [targets | PublishedTargets targets <- values]))
  | otherwise = pure UnknownWord
  where
    isNumber (KnownWords _) = True
    isNumber _ = False
    isAddress (Addresses _) = True
    isAddress _ = False
    isPublished (PublishedTargets _) = True
    isPublished _ = False

hasAddressProvenance :: AbstractWord -> Bool
hasAddressProvenance (Addresses _) = True
hasAddressProvenance (PublishedTargets _) = True
hasAddressProvenance ReturnWord = True
hasAddressProvenance _ = False

scalarUnary :: (Word32 -> Word32) -> AbstractWord -> Either String AbstractWord
scalarUnary operation value = do
  require (not (hasAddressProvenance value)) "critical-pair Rust transforms address provenance outside its proof"
  pure $ case value of
    KnownWords words' -> boundedWords (Set.map operation words')
    _ -> UnknownWord

scalarBinary :: (Word32 -> Word32 -> Word32) -> AbstractWord -> AbstractWord -> Either String AbstractWord
scalarBinary operation left right = do
  require (not (hasAddressProvenance left || hasAddressProvenance right))
    "critical-pair Rust combines address provenance outside its proof"
  pure $ case (left,right) of
    (KnownWords xs,KnownWords ys) -> boundedWords (Set.fromList [operation x y | x <- Set.toList xs,y <- Set.toList ys])
    _ -> UnknownWord

advanceAddress :: Address -> Word32 -> Either String Address
advanceAddress address delta = do
  let signed = toInteger (fromIntegral delta :: Int32)
      old = case address of
        FrameAddress previous -> previous
        ScratchAddress previous -> previous
        InterfaceAddress _ previous -> previous
      offset = toInteger old + signed
  require (offset >= -0x80000000 && offset <= 0x7fffffff) "critical-pair Rust relative address wraps outside its proof"
  pure $ case address of
    FrameAddress _ -> FrameAddress (fromInteger offset)
    ScratchAddress _ -> ScratchAddress (fromInteger offset)
    InterfaceAddress core _ -> InterfaceAddress core (fromInteger offset)

addWords :: AbstractWord -> AbstractWord -> Either String AbstractWord
addWords (Addresses addresses) (KnownWords offsets) = do
  advanced <- Set.fromList <$> mapM (uncurry advanceAddress)
    [(address,offset) | address <- Set.toList addresses,offset <- Set.toList offsets]
  require (not (Set.null advanced) && Set.size advanced <= 16) "critical-pair Rust address alternatives exceed its proof"
  pure (Addresses advanced)
addWords numbers@(KnownWords _) addresses@(Addresses _) = addWords addresses numbers
addWords left right = scalarBinary (+) left right

dataResult :: A.DataOp -> AbstractWord -> AbstractWord -> Either String AbstractWord
dataResult operation left right = case operation of
  A.MOV -> pure right
  A.MVN -> scalarUnary complement right
  A.ADD -> addWords left right
  A.SUB -> scalarUnary negate right >>= addWords left
  A.RSB -> dataResult A.SUB right left
  A.AND -> scalarBinary (.&.) left right
  A.EOR -> scalarBinary xor left right
  A.ORR -> scalarBinary (.|.) left right
  A.BIC -> scalarBinary (\x y -> x .&. complement y) left right
  _ -> do
    require (not (hasAddressProvenance left || hasAddressProvenance right))
      "critical-pair Rust arithmetic loses address provenance"
    pure UnknownWord

readRegister :: Word32 -> Frame -> A.Reg -> AbstractWord
readRegister pc frame register
  | register == A.pc = knownWord (pc+8)
  | otherwise = M.findWithDefault UnknownWord register (frameRegisters frame)

stackOffset :: Frame -> Either String Int
stackOffset frame = case M.lookup A.sp (frameRegisters frame) of
  Just (Addresses addresses) | [FrameAddress offset] <- Set.toList addresses -> do
    require (offset <= 0 && offset >= -0x400 && offset `mod` 4 == 0)
      "critical-pair Rust SP leaves its admitted frame"
    pure offset
  _ -> Left "critical-pair Rust SP has no unique frame provenance"

insertWord :: Ord key => key -> AbstractWord -> M.Map key AbstractWord -> M.Map key AbstractWord
insertWord key UnknownWord = M.delete key
insertWord key value = M.insert key value

writeRegister :: A.Reg -> AbstractWord -> Frame -> Either String Frame
writeRegister register value frame = do
  require (register /= A.pc) "critical-pair Rust writes PC outside an admitted return"
  let updated = frame {frameRegisters = insertWord register value (frameRegisters frame)}
  if register == A.sp then do
    offset <- stackOffset updated
    pure updated {frameWords = M.filterWithKey (\address _ -> address >= offset) (frameWords updated)}
  else pure updated

shiftScalar :: A.Shift -> Int -> Word32 -> Word32
shiftScalar operation amount value = case operation of
  A.LSL -> if amount >= 32 then 0 else value `shiftL` amount
  A.LSR -> if amount >= 32 then 0 else value `shiftR` amount
  A.ASR -> fromIntegral ((fromIntegral value :: Int32) `shiftR` min amount 31)
  A.ROR -> rotateR value amount

operandWord :: Word32 -> Frame -> A.Operand -> Either String AbstractWord
operandWord pc frame operand = case operand of
  A.Immediate value -> pure (knownWord value)
  A.ShiftedImmediate register A.LSL 0 -> pure (readRegister pc frame register)
  A.ShiftedImmediate register operation amount -> do
    let value = readRegister pc frame register
    if operation == A.ROR && amount == 0 then do
      require (not (hasAddressProvenance value)) "critical-pair Rust RRX loses address provenance"
      pure UnknownWord
    else scalarUnary (shiftScalar operation (if amount == 0 then 32 else amount)) value
  A.ShiftedRegister register operation shiftRegister -> do
    require (register /= A.pc && shiftRegister /= A.pc) "critical-pair Rust register shift uses an unadmitted PC operand"
    let value = readRegister pc frame register
        shift = readRegister pc frame shiftRegister
    require (not (hasAddressProvenance value || hasAddressProvenance shift))
      "critical-pair Rust register shift loses address provenance"
    pure $ case (value,shift) of
      (KnownWords values,KnownWords amounts) -> boundedWords (Set.fromList
        [shiftScalar operation (fromIntegral (amount .&. 0xff)) word | word <- Set.toList values,amount <- Set.toList amounts])
      _ -> UnknownWord

transferPlan :: Word32 -> Frame -> A.Reg -> A.Offset -> A.Indexing -> Either String TransferPlan
transferPlan pc frame base offset indexing = do
  delta <- case offset of
    A.OffsetImmediate value -> pure (knownWord (fromIntegral value))
    A.OffsetRegister positive register shift amount -> do
      value <- operandWord pc frame (A.ShiftedImmediate register shift amount)
      if positive then pure value else scalarUnary negate value
  let original = readRegister pc frame base
  adjusted <- addWords original delta
  pure (TransferPlan (if indexing == A.PostIndexed then original else adjusted)
    (if indexing == A.OffsetOnly then Nothing else Just adjusted))

memoryAddresses :: AbstractWord -> Either String [MemoryAddress]
memoryAddresses value = case value of
  Addresses addresses | not (Set.null addresses) -> pure (map Relative (Set.toList addresses))
  KnownWords addresses | not (Set.null addresses) -> pure (map Absolute (Set.toList addresses))
  _ -> Left "critical-pair Rust memory target has no confined provenance"

scratchField :: Int -> Either String ScratchField
scratchField offset = case lookup offset [(4*fromEnum field,field) | field <- [minBound..maxBound]] of
  Just field -> pure field
  Nothing -> Left "critical-pair Rust access leaves the defined Scratch fields"

publishedKind :: Int -> Either String ServiceKind
publishedKind offset = case lookup offset
  [(0x164,MaskIrq),(0x168,RestoreIrq),(0x1618,IdentifyCore),(0x161c,RetryExclusive),(0x1620,ReleaseExclusive),(0x1624,ObserveWord)] of
  Just kind -> pure kind
  Nothing -> Left "critical-pair Rust reads an unbound interface slot"

loadWord :: Word32 -> B.ByteString -> Frame -> AbstractWord -> Either String AbstractWord
loadWord origin image frame pointer = memoryAddresses pointer >>= mapM load >>= joinWords
  where
    load address = case address of
      Relative (FrameAddress offset) -> do
        sp <- stackOffset frame
        require (offset >= sp && offset+4 <= 0 && offset `mod` 4 == 0) "critical-pair Rust read leaves its allocated frame"
        pure (M.findWithDefault UnknownWord offset (frameWords frame))
      Relative (ScratchAddress offset) -> do
        field <- scratchField offset
        pure (M.findWithDefault UnknownWord field (scratchWords frame))
      Relative (InterfaceAddress core offset) ->
        PublishedTargets . Set.singleton . PublishedTarget core <$> publishedKind offset
      Absolute address'
        | address' == 0x96bd20 -> pure (addressWord (InterfaceAddress Core0 0))
        | address' == 0x96bd24 -> pure (addressWord (InterfaceAddress Core1 0))
        | address' == 0x40a0f824 -> pure UnknownWord
        | address' >= origin && toInteger address'+4 <= toInteger origin+toInteger (B.length image) && address' .&. 3 == 0 ->
            knownWord <$> leWordAt image (fromIntegral (address'-origin))
        | otherwise -> Left "critical-pair Rust read leaves its admitted source memory"

storeWord :: Frame -> AbstractWord -> AbstractWord -> Either String Frame
storeWord frame pointer value = do
  addresses <- memoryAddresses pointer
  address <- case addresses of
    [single] -> pure single
    _ -> Left "critical-pair Rust store has multiple possible targets"
  case address of
    Relative (FrameAddress offset) -> do
      sp <- stackOffset frame
      require (offset >= sp && offset+4 <= 0 && offset `mod` 4 == 0) "critical-pair Rust store leaves its allocated frame"
      pure frame {frameWords = insertWord offset value (frameWords frame)}
    Relative (ScratchAddress offset) -> do
      field <- scratchField offset
      pure frame {scratchWords = insertWord field value (scratchWords frame)}
    Absolute 0x40a0f824 -> pure frame
    _ -> Left "critical-pair Rust store leaves frame, Scratch and counter"

applyWriteback :: A.Reg -> Maybe AbstractWord -> Frame -> Either String Frame
applyWriteback _ Nothing frame = pure frame
applyWriteback base (Just value) frame = writeRegister base value frame

transferRegisters :: A.Width -> A.Reg -> Either String [A.Reg]
transferRegisters A.Word register = pure [register]
transferRegisters A.Dual (A.Reg register) | even register && register < 12 = pure [A.Reg register,A.Reg (register+1)]
transferRegisters _ _ = Left "critical-pair Rust transfer width is outside its provenance proof"

bridgeSummary :: Frame -> Either String Frame
bridgeSummary frame = do
  require (M.lookup (A.Reg 0) (frameRegisters frame) == Just (addressWord (ScratchAddress 0)))
    "critical-pair Rust bridge argument is not the admitted Scratch base"
  targets <- case M.lookup (A.Reg 1) (frameRegisters frame) of
    Just (PublishedTargets values) | not (Set.null values) -> pure (Set.toList values)
    _ -> Left "critical-pair Rust bridge target lacks closed publication provenance"
  outcomes <- mapM summarize targets
  fields <- forM [minBound..maxBound] $ \field -> do
    value <- joinWords [M.findWithDefault UnknownWord field outcome | outcome <- outcomes]
    pure (field,value)
  pure frame
    { frameRegisters = foldr M.delete (frameRegisters frame) [A.Reg 0,A.Reg 1,A.Reg 2,A.Reg 3,A.Reg 12,A.lr]
    , scratchWords = M.fromList [(field,value) | (field,value) <- fields,value /= UnknownWord]
    }
  where
    before = scratchWords frame
    memoryArgument = case M.findWithDefault UnknownWord ScratchR0 before of
      KnownWords values -> not (Set.null values) && all (`elem` [0,0x400c0804]) (Set.toList values)
      _ -> False
    summarize (PublishedTarget _ kind) = do
      require (kind `notElem` [RetryExclusive,ReleaseExclusive,ObserveWord] || memoryArgument)
        "critical-pair Rust retained memory service can escape its admitted lock cell"
      let assign fields = foldl (\words' (field,value) -> insertWord field value words') before fields
          unknown = [(ScratchR0,UnknownWord),(ScratchCpsr,UnknownWord)]
      pure $ case kind of
        IdentifyCore -> assign (unknown ++ [(ScratchR0,KnownWords (Set.fromList [0,1])),(ScratchR1,UnknownWord),(ScratchR2,knownWord 0x4003)])
        MaskIrq -> assign (unknown ++ [(ScratchR0,KnownWords (Set.fromList [0,0x80])),(ScratchR1,UnknownWord)])
        RestoreIrq -> assign (unknown ++ [(ScratchR1,UnknownWord)])
        ObserveWord -> assign unknown
        RetryExclusive -> assign (unknown ++ [(ScratchR0,KnownWords (Set.fromList [0,0xffffffff])),(ScratchR2,UnknownWord)])
        ReleaseExclusive -> assign (unknown ++ [(ScratchR0,KnownWords (Set.fromList [0,0xffffffff])),(ScratchR1,knownWord 0),(ScratchR2,knownWord 0)])

blockPlan :: A.BlockMode -> Int -> AbstractWord -> Either String ([AbstractWord],AbstractWord)
blockPlan mode count base = do
  let bytes = 4*count
      first = case mode of
        A.IncrementAfter -> 0
        A.IncrementBefore -> 4
        A.DecrementAfter -> 4-bytes
        A.DecrementBefore -> -bytes
      change = case mode of
        A.IncrementAfter -> bytes
        A.IncrementBefore -> bytes
        A.DecrementAfter -> -bytes
        A.DecrementBefore -> -bytes
  addresses <- mapM (addWords base . knownWord . fromIntegral) [first,first+4..first+bytes-4]
  updated <- addWords base (knownWord (fromIntegral change))
  pure (addresses,updated)

frameExtent :: B.ByteString -> [F.Symbol] -> Body -> Either String FrameBytes
frameExtent image symbols body = do
  origin <- at symbols "__secondary_stage_start"
  helper <- at symbols "xm1_critical_retained_call"
  symbol <- F.lookupSymbol symbols (bodyName body)
  let start = F.symbolAddress symbol
      end = start+F.symbolSize symbol
      preserved = map A.Reg [4..11]
      coreArgument = case body of EntryBody -> [(A.Reg 1,KnownWords (Set.fromList [0,1]))]; ExitBody -> []
      savedScratch = case body of
        EntryBody -> M.singleton ScratchR3 (PublishedTargets (Set.fromList [PublishedTarget Core0 MaskIrq,PublishedTarget Core1 MaskIrq]))
        ExitBody -> M.empty
      initial = Frame (M.fromList ((A.sp,addressWord (FrameAddress 0)) : (A.Reg 0,addressWord (ScratchAddress 0))
        : (A.lr,ReturnWord) : [(register,CallerWord register) | register <- preserved] ++ coreArgument)) M.empty savedScratch
      finish frame = do
        sp <- stackOffset frame
        require (sp == 0) "critical-pair Rust return changes SP"
        require (all (\register -> M.lookup register (frameRegisters frame) == Just (CallerWord register)) preserved)
          "critical-pair Rust return changes a preserved register"
      walk [] _ deepest returns = do
        require (returns > (0 :: Int)) "critical-pair Rust has no admitted return"
        pure (FrameBytes deepest)
      walk ((pc,frame):pending) seen deepest returns
        | Set.member (pc,frame) seen = walk pending seen deepest returns
        | otherwise = do
          require (Set.size seen < 20000 && pc >= start && pc < end && pc .&. 3 == 0)
            "critical-pair Rust control flow leaves its provenance proof"
          sp <- stackOffset frame
          word <- leWordAt image (fromIntegral (pc-origin))
          let visited = Set.insert (pc,frame) seen
              deepest' = max deepest (-sp)
              readReg = readRegister pc frame
              skipped condition = [(pc+4,frame) | condition /= A.Cond 14]
              next condition updated = walk ((pc+4,updated):skipped condition ++ pending) visited deepest' returns
              returned condition updated value = do
                require (value == ReturnWord) "critical-pair Rust return has no caller-return provenance"
                finish updated
                walk (skipped condition ++ pending) visited deepest' (returns+1)
              assign condition register value = if register == A.pc then returned condition frame value
                else writeRegister register value frame >>= next condition
              call condition target tailCall = do
                require (target == helper && sp `mod` 8 == 0) "critical-pair Rust calls an unbound bridge or misaligns SP"
                summarized <- bridgeSummary frame
                let depth = max deepest' (24-sp)
                if tailCall then do
                  finish frame
                  require (readReg A.lr == ReturnWord) "critical-pair Rust tail call loses its caller return"
                  walk (skipped condition ++ pending) visited depth (returns+1)
                else walk ((pc+4,summarized):skipped condition ++ pending) visited depth returns
              loadRegisters condition base writeback registers addresses = do
                require (not (null registers) && A.sp `notElem` registers) "critical-pair Rust loads an unbound SP"
                require (writeback == Nothing || base `notElem` registers) "critical-pair Rust load writeback overlaps its destination"
                values <- mapM (loadWord origin image frame) addresses
                updated <- foldM (\state (register,value) -> if register == A.pc then pure state else writeRegister register value state)
                  frame (zip registers values)
                restored <- applyWriteback base writeback updated
                case lookup A.pc (zip registers values) of
                  Just value -> returned condition restored value
                  Nothing -> next condition restored
              storeRegisters condition base writeback registers addresses = do
                require (not (null registers) && A.pc `notElem` registers) "critical-pair Rust stores an unbound PC"
                let values = map readReg registers
                updated <- applyWriteback base writeback frame
                oldSp <- stackOffset frame
                newSp <- stackOffset updated
                let memoryFrame = if newSp < oldSp then updated else frame
                stored <- foldM (\state (address,value) -> storeWord state address value) memoryFrame (zip addresses values)
                final <- applyWriteback base writeback stored
                next condition final
          case A.decode pc word of
            A.Branch condition link target
              | link -> call condition target False
              | target < start || target >= end -> call condition target True
              | otherwise -> walk ((target,frame):skipped condition ++ pending) visited deepest' returns
            A.BranchExchange condition False register -> returned condition frame (readReg register)
            A.DataProcessing condition operation _ destination base operand
              | operation `elem` [A.CMP,A.CMN,A.TST,A.TEQ] -> next (A.Cond 14) frame
              | otherwise -> do
                  require (case operand of A.ShiftedRegister _ _ _ -> base /= A.pc; _ -> True)
                    "critical-pair Rust register-shift arithmetic uses an unadmitted PC base"
                  operandWord pc frame operand >>= dataResult operation (readReg base) >>= assign condition destination
            A.MoveWide condition destination value -> assign condition destination (knownWord value)
            A.MoveTop condition destination value ->
              scalarUnary (\low -> (low .&. 0xffff) .|. (value `shiftL` 16)) (readReg destination) >>= assign condition destination
            A.ZeroExtend condition destination source rotation width -> do
              require (source /= A.pc) "critical-pair Rust extension uses an unadmitted PC operand"
              scalarUnary (\value -> rotateR value rotation .&. ((1 `shiftL` width)-1)) (readReg source) >>= assign condition destination
            A.Multiply condition destination left right accumulator -> do
              require (A.pc `notElem` [destination,left,right] && accumulator /= Just A.pc)
                "critical-pair Rust multiply uses an unadmitted PC operand"
              product' <- scalarBinary (*) (readReg left) (readReg right)
              value <- maybe (pure product') (\register -> addWords product' (readReg register)) accumulator
              assign condition destination value
            A.Load condition width False destination base offset indexing -> do
              registers <- transferRegisters width destination
              TransferPlan address writeback <- transferPlan pc frame base offset indexing
              addresses <- mapM (addWords address . knownWord . fromIntegral) [0,4..4*length registers-4]
              loadRegisters condition base writeback registers addresses
            A.Store condition width source base offset indexing -> do
              registers <- transferRegisters width source
              TransferPlan address writeback <- transferPlan pc frame base offset indexing
              addresses <- mapM (addWords address . knownWord . fromIntegral) [0,4..4*length registers-4]
              storeRegisters condition base writeback registers addresses
            A.StoreMultiple condition base mode writeback registers -> do
              require (not writeback || base `notElem` registers) "critical-pair Rust block-store writeback overlaps its source"
              (addresses,updated) <- blockPlan mode (length registers) (readReg base)
              storeRegisters condition base (if writeback then Just updated else Nothing) registers addresses
            A.LoadMultiple condition base mode writeback registers -> do
              (addresses,updated) <- blockPlan mode (length registers) (readReg base)
              loadRegisters condition base (if writeback then Just updated else Nothing) registers addresses
            A.Hint _ -> do
              require (word .&. 0x0fffffff == 0x0320f000) "critical-pair Rust hint is outside its provenance proof"
              next (A.Cond 14) frame
            instruction -> Left ("critical-pair Rust instruction is outside its provenance proof: " ++ show instruction)
  walk [(start,initial)] Set.empty 0 0

deriveLinkedArtifact :: B.ByteString -> B.ByteString -> Either String LinkedArtifact
deriveLinkedArtifact image symbolBytes = do
  symbols <- F.parseSymbols symbolBytes
  start <- at symbols "__secondary_stage_start"
  end <- at symbols "__secondary_stage_end"
  require (start == 0x10400 && end <= 0x11800 && fromIntegral end - fromIntegral start == B.length image)
    "critical-pair linked image leaves established record-2 padding"
  forM_ ["xm1_critical_enter_adapter","xm1_critical_enter_continue","xm1_critical_exit_adapter"
        ,"xm1_critical_exit","xm1_critical_retained_call"] $ \name -> do
    symbol <- F.lookupSymbol symbols name
    require (F.symbolAddress symbol >= start && F.symbolSize symbol > 0
      && toInteger (F.symbolAddress symbol) + toInteger (F.symbolSize symbol) <= toInteger end)
      "critical-pair linked function leaves its artifact"
  adapterProof image symbols
  enter <- Guest <$> at symbols "xm1_critical_enter_adapter"
  exit <- Guest <$> at symbols "xm1_critical_exit_adapter"
  enterFrame <- frameExtent image symbols EntryBody
  exitFrame <- frameExtent image symbols ExitBody
  pure (LinkedArtifact image symbolBytes symbols enter exit enterFrame exitFrame)

stageStartup :: Original -> LinkedArtifact -> Either String Staging
stageStartup (Original reference original image4 bindings) artifact@(LinkedArtifact image symbolBytes symbols (Guest enter) (Guest exit) _ _) = do
  declared <- I.declarations symbols
  let parsed = S.originalStartupParsed original
  start <- at symbols "__secondary_stage_start"
  entry <- at symbols "xm1_secondary_entry" >>= F.armEntry
  offset <- F.slotOffset (fromIntegral start - 0x10000)
  plan <- F.stagePlan F.SecondRecord entry offset (U.MediaImage image) (declaredRetained declared)
  parts <- F.replaceRecordSlot plan (U.parsedParts parsed) >>= F.applyLoaderWordPatches (declaredLoaderPatches declared)
  staged <- U.rebuildContainer parsed parts
  reparsed <- U.parseContainer staged
  F.verifyStagedRecord plan (declaredLoaderPatches declared) parsed reparsed
  _ <- I.deriveContract I.Owned reference staged image symbolBytes
  enterBranch <- branch False 0x7235d8 enter
  exitBranch <- branch False 0x7236b0 exit
  let patches = [ImageWordPatch (Guest 0x7235d8,0xe92d4df0,enterBranch),ImageWordPatch (Guest 0x7236b0,0xe92d4878,exitBranch)]
      apply bytes (ImageWordPatch (Guest address,before,after)) = do
        let index = fromIntegral (address - 0x61c400)
        observed <- leWordAt bytes index
        require (observed == before) "critical-pair image4 patch preimage differs"
        pure (B.take index bytes <> U.putWord32 after <> B.drop (index+4) bytes)
  patched <- foldl (\previous patch -> previous >>= (`apply` patch)) (Right image4) patches
  pure (Staging reference staged patched patches bindings artifact)

startupDat :: Staging -> B.ByteString
startupDat (Staging _ bytes _ _ _ _) = bytes

patchedImage4 :: Staging -> B.ByteString
patchedImage4 (Staging _ _ bytes _ _ _) = bytes

verifyFinalDat :: Staging -> B.ByteString -> Either String ()
verifyFinalDat staging final = do
  before <- U.parseContainer (startupDat staging)
  after <- U.parseContainer final
  let originalParts = U.parsedParts before
      finalParts = U.parsedParts after
      restored = finalParts {U.partRecord4 = U.partRecord4 originalParts,U.partGap45 = U.partGap45 originalParts}
  reconstructed <- U.rebuildContainer before restored
  require (reconstructed == startupDat staging) "critical-pair final DAT changes bytes outside its image4 record window"
  require (U.recDecodedSize (U.recHeader (U.partRecord4 finalParts)) == fromIntegral (B.length (patchedImage4 staging)))
    "critical-pair final DAT changes decoded image4 extent"

stagingValue :: Staging -> B.ByteString -> Value
stagingValue (Staging reference staged patched patches bindings (LinkedArtifact image symbolBytes _ (Guest enter) (Guest exit) (FrameBytes enterFrame) (FrameBytes exitFrame))) final = object
  [ "schema" .= (1 :: Int),"kind" .= ("critical-pair-source-staging" :: String)
  , "source_sha256" .= sha256Hex reference,"startup_dat_sha256" .= sha256Hex staged
  , "final_dat_sha256" .= sha256Hex final,"decoded_image4_sha256" .= sha256Hex patched
  , "linked_image_sha256" .= sha256Hex image,"symbols_sha256" .= sha256Hex symbolBytes
  , "entry_adapter" .= enter,"exit_adapter" .= exit
  , "entry_frame_bytes" .= (64+enterFrame),"exit_frame_bytes" .= (56+exitFrame)
  , "owned_spans" .= ([[0x7235d8,0x7236b0],[0x7236b0,0x723790]] :: [[Word32]])
  , "patches" .= [object ["site" .= site,"before" .= before,"after" .= after] | ImageWordPatch (Guest site,before,after) <- patches]
  , "retained_services" .= [object ["core" .= (case core of Core0 -> 0 :: Int; Core1 -> 1)
      ,"table" .= table,"loader_sha256" .= F.sha256Hex digest
      ,"slots" .= [object ["offset" .= offset,"target" .= target] | (InterfaceOffset offset,RetainedCode target) <- slots]]
      | Services core (Guest table) digest slots <- bindings]
  , "qualification" .= ("source-only; no natural acceptance, retirement or SDK protocol claim" :: String)
  , "premises" .= (["record-2 padding remains executable, coherent and unmodified through the natural task103 return"
      ,"task103 supplies an eight-byte-aligned writable stack, disjoint from roots, tables, counter and retained lock"
      ,"normalized CP14 identity and the installed A/B service publications remain stable across the paired boundary"
      ,"retained IRQ, exclusive-monitor and MCR p15,c7,c10,5 ordering semantics remain dependencies"
      ,"no scheduler ownership, acquisition inference, finite contention bound or general caller/alias closure is established"] :: [String])
  ]
