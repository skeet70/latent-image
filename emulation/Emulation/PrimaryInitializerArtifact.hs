{-# LANGUAGE OverloadedStrings #-}
module Emulation.PrimaryInitializerArtifact
  ( Artifact
  , ArtifactIdentity (..)
  , Placement (..)
  , Scratch (..)
  , Access (..)
  , AccessKind (..)
  , ScratchValue (..)
  , Obligation (..)
  , PublicationWords (..)
  , InstructionCount (..)
  , ArtifactMetadata (..)
  , EmittedPublication (..), StagedFirmware, stagedBytes, stagedDigest
  , artifactImage, artifactSymbols, artifactPublications, artifactAccesses
  , artifactMetadata
  , deriveArtifact
  , stageArtifact
  , artifactValue
  ) where

import qualified A32 as A
import Control.Monad (foldM, forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (ToJSON (..), Value, object, (.=))
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR, testBit)
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Lazy.Char8 as BLC
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word64)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import qualified Emulation.PollPeriod as K
import Emulation.PrimaryInitializerSource
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U

ownedStart, ownedEnd :: Word32
ownedStart = 0x40430
ownedEnd = 0x409ac

data ArtifactIdentity = ArtifactIdentity
  { identitySource :: F.Sha256
  , identitySourceLoader :: F.Sha256
  , identityOwnedLoader :: F.Sha256
  , identityImage :: F.Sha256
  , identitySymbols :: F.Sha256
  , identityPublication :: F.Sha256
  } deriving (Eq, Show)

data Placement = Placement
  { placementStart :: Guest
  , placementEnd :: Guest
  , placementLinkedEnd :: Guest
  , placementRustStart :: Guest
  , placementRustEnd :: Guest
  , placementContainer :: ContainerOffset
  , placementFlash :: Physical
  } deriving (Eq, Show)

data Scratch = Scratch
  { scratchAdapterBytes :: ByteCount
  , scratchRustBytes :: ByteCount
  , scratchLow :: Guest
  , scratchHigh :: Guest
  } deriving (Eq, Show)

data AccessKind = ScratchRead | ScratchWrite | ImageRead | PublicationWrite
  deriving (Eq, Show)

data ScratchValue = ScratchConstant Word32 | EnteredRegister A.Reg | EnteredLr | EnteredCpsr
  deriving (Eq, Show)

instance ToJSON ScratchValue where
  toJSON value = case value of
    ScratchConstant wordValue -> object ["kind" .= ("constant" :: String),"value" .= wordValue]
    EnteredRegister (A.Reg register) -> object ["kind" .= ("entered-register" :: String),"register" .= register]
    EnteredLr -> object ["kind" .= ("entered-lr" :: String)]
    EnteredCpsr -> object ["kind" .= ("entered-cpsr" :: String)]

data Access = Access
  { accessPc :: Guest
  , accessGuest :: Guest
  , accessKind :: AccessKind
  , accessOrdinal :: InstructionCount
  , accessScratchValue :: Maybe ScratchValue
  } deriving (Eq, Show)

data Obligation
  = NaturalCallerContext
  | BothCoreLowHighRamAliases
  | CurrentRamDecoder
  | BothCoreItcmExclusion
  | BothCoreDtcmExclusion
  | EarlyCarrierLifetime
  | ScratchLifetime
  deriving (Eq, Show)

newtype PublicationWords = PublicationWords Word32 deriving (Eq, Show)
newtype InstructionCount = InstructionCount Word64 deriving (Eq, Show)

data ArtifactMetadata = ArtifactMetadata
  { metadataIdentity :: ArtifactIdentity
  , metadataPlacement :: Placement
  , metadataScratch :: Scratch
  , metadataCaller :: CallerFrame
  , metadataReturn :: ReturnValues
  , metadataCallerSites :: [Guest]
  , metadataAccesses :: [Access]
  , metadataInstructions :: InstructionCount
  , metadataPublicationWords :: PublicationWords
  , metadataObligations :: [Obligation]
  } deriving (Eq, Show)

data EmittedPublication = EmittedPublication
  { emittedGuest :: Guest
  , emittedValue :: Word32
  , emittedPc :: Guest
  , emittedOrdinal :: InstructionCount
  } deriving (Eq, Show)

data Artifact = Artifact ArtifactMetadata B.ByteString B.ByteString [EmittedPublication]
data StagedFirmware = StagedFirmware F.Sha256 B.ByteString

artifactMetadata :: Artifact -> ArtifactMetadata
artifactMetadata (Artifact metadata _ _ _) = metadata

artifactImage :: Artifact -> B.ByteString
artifactImage (Artifact _ image _ _) = image

artifactSymbols :: Artifact -> B.ByteString
artifactSymbols (Artifact _ _ symbols _) = symbols

artifactPublications :: Artifact -> [EmittedPublication]
artifactPublications (Artifact _ _ _ emitted) = emitted

artifactAccesses :: Artifact -> [Access]
artifactAccesses = metadataAccesses . artifactMetadata

stagedBytes :: StagedFirmware -> B.ByteString
stagedBytes (StagedFirmware _ bytes) = bytes

stagedDigest :: StagedFirmware -> F.Sha256
stagedDigest (StagedFirmware digest' _) = digest'

require :: Bool -> String -> Either String ()
require instructionCondition message = unless instructionCondition (Left message)

digest :: B.ByteString -> Either String F.Sha256
digest = F.parseSha256 . sha256Hex

digestHash :: B.ByteString -> Either String F.Sha256
digestHash = F.parseSha256 . BLC.unpack . BB.toLazyByteString . BB.byteStringHex

word :: Guest -> Word32
word (Guest instructionAddress) = instructionAddress

count :: ByteCount -> Word32
count (ByteCount bytes) = bytes

physical :: Word32 -> Maybe Word32
physical instructionAddress
  | instructionAddress < 0x100000 = Just instructionAddress
  | instructionAddress >= 0x40000000 && instructionAddress < 0x40100000 = Just (instructionAddress - 0x40000000)
  | otherwise = Nothing

checkedAddress :: Integer -> Either String Word32
checkedAddress instructionAddress = do
  require (instructionAddress >= 0 && instructionAddress <= 0xffffffff) "primary initializer address wraps"
  pure (fromInteger instructionAddress)

-- Incoming words retain their identity through spills and register moves.
data ProofWord = Constant Word32 | Incoming A.Reg | StackAddress Integer | ReturnAddress | Unknown
  deriving (Eq, Show)

data Machine = Machine
  { machinePc :: Word32
  , machineRegisters :: M.Map A.Reg ProofWord
  , machineFlags :: K.Flags
  , machineStack :: M.Map Integer ProofWord
  , machineRemaining :: [Publication]
  , machineDeepest :: Integer
  , machineAccesses :: [Access]
  , machineSteps :: Word64
  }

scalar :: ProofWord -> Maybe Word32
scalar (Constant value) = Just value
scalar _ = Nothing

readRegister :: Machine -> A.Reg -> ProofWord
readRegister machine register
  | register == A.pc = Constant (machinePc machine + 8)
  | otherwise = M.findWithDefault Unknown register (machineRegisters machine)

setRegister :: A.Reg -> ProofWord -> Machine -> Either String Machine
setRegister register value machine = do
  require (register /= A.pc) "primary initializer has an unsupported PC write"
  case (register, value) of
    (A.Reg 13, StackAddress offset) -> do
      require (offset <= 0 && offset >= -0x400 && offset `mod` 4 == 0) "primary initializer SP leaves its bounded Rust frame"
      pure machine
        { machineRegisters = M.insert register value (machineRegisters machine)
        , machineStack = M.filterWithKey (\location _ -> location >= offset) (machineStack machine)
        , machineDeepest = max (machineDeepest machine) (negate offset)
        }
    (A.Reg 13, _) -> Left "primary initializer SP is not derived from its incoming SP"
    _ -> pure machine {machineRegisters = M.insert register value (machineRegisters machine)}

stackOffset :: Machine -> Either String Integer
stackOffset machine = case readRegister machine A.sp of
  StackAddress offset -> Right offset
  _ -> Left "primary initializer lost its stack provenance"

condition :: A.Instruction -> Either String A.Cond
condition instruction = case instruction of
  A.DataProcessing cond _ _ _ _ _ -> pure cond
  A.MoveWide cond _ _ -> pure cond
  A.MoveTop cond _ _ -> pure cond
  A.Load cond _ _ _ _ _ _ -> pure cond
  A.Store cond _ _ _ _ _ -> pure cond
  A.LoadMultiple cond _ _ _ _ -> pure cond
  A.StoreMultiple cond _ _ _ _ -> pure cond
  A.Branch cond _ _ -> pure cond
  A.BranchExchange cond _ _ -> pure cond
  A.Multiply cond _ _ _ _ -> pure cond
  _ -> Left "primary initializer contains an unsupported instruction"

known :: Maybe a -> Either String a
known = maybe (Left "primary initializer control, address or publication depends on an incoming value") Right

shiftOperand :: Machine -> Word32 -> A.Operand -> Either String (ProofWord, Maybe Bool)
shiftOperand machine instruction operand = case operand of
  A.Immediate value -> pure (Constant value, if (instruction `shiftR` 8) .&. 15 == 0 then carry else Just (testBit value 31))
  A.ShiftedImmediate register A.LSL 0 -> pure (readRegister machine register, carry)
  A.ShiftedImmediate register kind amount -> do
    let value = readRegister machine register
    require (case value of StackAddress _ -> False; _ -> True) "primary initializer shifts a stack alias"
    let (result, nextCarry) = K.shiftImmediateKnown kind amount (scalar value) carry
    pure (maybe Unknown Constant result, nextCarry)
  A.ShiftedRegister {} -> Left "primary initializer uses an unsupported register-controlled shift"
  where carry = K.flagC (machineFlags machine)

address :: Machine -> A.Reg -> A.Offset -> A.Indexing -> Either String (ProofWord, ProofWord)
address machine base offset indexing = do
  require (indexing == A.OffsetOnly || base /= A.pc) "primary initializer writes back PC"
  displacement <- case offset of
    A.OffsetImmediate amount -> pure (toInteger amount)
    A.OffsetRegister up register kind amount -> do
      require (register /= A.pc) "primary initializer uses a PC transfer index"
      value <- known (scalar (readRegister machine register))
      shifted <- known (fst (K.shiftImmediateKnown kind amount (Just value) (K.flagC (machineFlags machine))))
      pure (if up then toInteger shifted else negate (toInteger shifted))
  updated <- case readRegister machine base of
    StackAddress relative -> pure (StackAddress (relative + displacement))
    Constant value -> pure (Constant (value + fromInteger displacement))
    _ -> Left "primary initializer transfer base is unproved"
  pure (if indexing == A.PostIndexed then readRegister machine base else updated, updated)

-- The Rust step count includes this instruction; three adapter instructions precede Rust.
recordAccess :: Word32 -> Word32 -> AccessKind -> Maybe ScratchValue -> Machine -> Machine
recordAccess pc location kind value machine = machine
  {machineAccesses = Access (Guest pc) (Guest location) kind (InstructionCount (machineSteps machine + 2)) value : machineAccesses machine}

frameAddress :: Word32 -> Machine -> Integer -> Either String Word32
frameAddress incomingSp machine relative = do
  sp <- stackOffset machine
  require (relative >= sp && relative + 4 <= 0 && relative `mod` 4 == 0) "primary initializer access leaves its allocated Rust frame"
  checkedAddress (toInteger incomingSp + relative)

load :: K.CodeImage -> Word32 -> ProofWord -> Machine -> Either String (ProofWord, Machine)
load image incomingSp location machine = case location of
  StackAddress relative -> do
    guest <- frameAddress incomingSp machine relative
    value <- maybe (Left "primary initializer reads an unwritten Rust stack word") Right (M.lookup relative (machineStack machine))
    pure (value, recordAccess here guest ScratchRead Nothing machine)
  Constant guest -> do
    value <- maybe (Left "primary initializer reads outside its linked immutable image") Right (K.imageWord image guest)
    pure (Constant value, recordAccess here guest ImageRead Nothing machine)
  _ -> Left "primary initializer load address is unproved"
  where here = machinePc machine

store :: Word32 -> ProofWord -> ProofWord -> Machine -> Either String Machine
store incomingSp location value machine = case location of
  StackAddress relative -> do
    guest <- frameAddress incomingSp machine relative
    require (case value of StackAddress _ -> False; _ -> True) "primary initializer spills a stack alias"
    expected <- case value of
      Constant wordValue -> pure (ScratchConstant wordValue)
      Incoming (A.Reg 4) -> pure EnteredCpsr
      Incoming register -> pure (EnteredRegister register)
      ReturnAddress -> pure (ScratchConstant (ownedStart + 12))
      _ -> Left "primary initializer scratch write lacks proved value provenance"
    pure (recordAccess here guest ScratchWrite (Just expected) machine)
      {machineStack = M.insert relative value (machineStack machine)}
  Constant guest -> case machineRemaining machine of
    expected : rest -> do
      actual <- known (scalar value)
      require (guest == word (publicationGuest expected) && actual == publicationValue expected) "primary initializer store differs from the complete ordered source publication"
      pure (recordAccess here guest PublicationWrite Nothing machine) {machineRemaining = rest}
    [] -> Left "primary initializer writes beyond its complete source publication"
  _ -> Left "primary initializer store address is unproved"
  where here = machinePc machine

block :: ProofWord -> A.BlockMode -> [A.Reg] -> Either String ([ProofWord], ProofWord)
block base mode registers = do
  require (not (null registers)) "primary initializer uses an empty register transfer"
  let bytes = 4 * toInteger (length registers)
      start = case mode of
        A.IncrementAfter -> 0
        A.IncrementBefore -> 4
        A.DecrementAfter -> 4 - bytes
        A.DecrementBefore -> negate bytes
      final = if mode `elem` [A.IncrementAfter, A.IncrementBefore] then bytes else negate bytes
      plus offset = case base of
        StackAddress relative -> pure (StackAddress (relative + offset))
        Constant value -> Constant <$> checkedAddress (toInteger value + offset)
        _ -> Left "primary initializer block base is unproved"
  locations <- mapM plus [start, start + 4 .. start + bytes - 4]
  updated <- plus final
  pure (locations, updated)

prove :: Source -> B.ByteString -> F.Symbol -> Either String (Scratch, [Access], Word64)
prove source bytes rust = go initial
  where
    start = F.symbolAddress rust
    end = toInteger start + toInteger (F.symbolSize rust)
    incomingSp = word (callerSp (sourceCaller source)) - 8
    image = K.CodeImage ownedStart bytes
    initial = Machine start (M.fromList ([(A.Reg index, Incoming (A.Reg index)) | index <- [0 .. 12]] ++ [(A.sp, StackAddress 0), (A.lr, ReturnAddress)]))
      (K.Flags Nothing Nothing Nothing Nothing) M.empty (sourcePublications source) 0 [] 0
    finish destination machine = do
      require (destination == ReturnAddress) "primary initializer return target lacks incoming LR provenance"
      require (readRegister machine A.sp == StackAddress 0) "primary initializer return does not restore SP"
      require (all (\register -> readRegister machine register == Incoming register) [A.Reg index | index <- [4 .. 11]]) "primary initializer return does not restore AAPCS preserved registers"
      require (null (machineRemaining machine)) "primary initializer returns before its complete source publication"
      let rustBytes = fromInteger (machineDeepest machine)
          low = incomingSp - rustBytes
          high = word (callerTop (sourceCaller source))
      lowPhysical <- known (physical low)
      highPhysical <- known (physical high)
      forM_ (sourcePublications source) $ \publication -> do
        location <- known (physical (word (publicationGuest publication)))
        require (toInteger location + 4 <= toInteger lowPhysical || location >= highPhysical) "primary initializer publication aliases its scratch, adapter or caller frame"
      pure (Scratch (ByteCount 8) (ByteCount rustBytes) (Guest low) (callerSp (sourceCaller source)), reverse (machineAccesses machine), machineSteps machine)
    go machine = do
      let here = machinePc machine
      require (machineSteps machine < 100000) "primary initializer linked proof exceeds 100000 instructions"
      require (here >= start && toInteger here + 4 <= end && here .&. 3 == 0) "primary initializer execution leaves its linked Rust function"
      instructionWord <- maybe (Left "primary initializer instruction fetch leaves its image") Right (K.imageWord image here)
      let instruction = A.decode here instructionWord
          entered = machine {machineSteps = machineSteps machine + 1}
          advance next = go next {machinePc = here + 4}
          writeback base indexing updated state = if indexing == A.OffsetOnly then pure state else setRegister base updated state
          branch destination state = go state {machinePc = destination}
      cond <- condition instruction
      taken <- known (K.holds (machineFlags machine) cond)
      if not taken then advance entered else case instruction of
        A.DataProcessing _ operation flags destination sourceRegister operand -> do
          (right, carry) <- shiftOperand machine instructionWord operand
          let left = readRegister machine sourceRegister
              (result, nextFlags) = K.alu operation (scalar left) (scalar right) carry (machineFlags machine)
              inputs = [right] ++ [left | operation `notElem` [A.MOV, A.MVN]]
              aliases = [relative | StackAddress relative <- inputs]
              identity = case (operation, operand) of
                (A.MOV, A.ShiftedImmediate _ A.LSL 0) -> Just right
                (A.ADD, A.Immediate 0) -> Just left
                (A.SUB, A.Immediate 0) -> Just left
                _ -> Nothing
              derived = case (operation, left, right) of
                (A.ADD, StackAddress relative, Constant amount) -> Just (StackAddress (relative + toInteger amount))
                (A.SUB, StackAddress relative, Constant amount) -> Just (StackAddress (relative - toInteger amount))
                _ -> identity
              value = maybe (maybe Unknown Constant result) id derived
              next = if flags then entered {machineFlags = nextFlags} else entered
          require (null aliases || derived /= Nothing) "primary initializer performs an unsupported stack-alias computation"
          if operation `elem` [A.CMP, A.CMN, A.TST, A.TEQ]
            then advance next
            else if destination == A.pc
              then do
                require (not flags) "primary initializer return writes CPSR"
                finish value next
              else setRegister destination value next >>= advance
        A.MoveWide _ destination value -> setRegister destination (Constant value) entered >>= advance
        A.MoveTop _ destination value -> do
          old <- known (scalar (readRegister machine destination))
          setRegister destination (Constant ((old .&. 0xffff) .|. (value `shiftL` 16))) entered >>= advance
        A.Load _ A.Word False destination base offset indexing -> do
          require (destination /= A.pc && (indexing == A.OffsetOnly || destination /= base)) "primary initializer uses an unsupported load destination or writeback overlap"
          (location, updated) <- address machine base offset indexing
          (value, loaded) <- load image incomingSp location entered
          writeback base indexing updated loaded >>= setRegister destination value >>= advance
        A.Store _ A.Word sourceRegister base offset indexing -> do
          require (sourceRegister /= A.pc && (indexing == A.OffsetOnly || sourceRegister /= base)) "primary initializer uses an unsupported store source or writeback overlap"
          (location, updated) <- address machine base offset indexing
          prepared <- writeback base indexing updated entered
          store incomingSp location (readRegister machine sourceRegister) prepared >>= advance
        A.StoreMultiple _ base mode updates registers -> do
          require (base /= A.pc && A.pc `notElem` registers && A.sp `notElem` registers && (not updates || base `notElem` registers)) "primary initializer uses unsupported STM registers"
          (locations, updated) <- block (readRegister machine base) mode registers
          prepared <- if updates then setRegister base updated entered else pure entered
          foldM (\state (location, register) -> store incomingSp location (readRegister machine register) state) prepared (zip locations registers) >>= advance
        A.LoadMultiple _ base mode updates registers -> do
          require (base /= A.pc && A.sp `notElem` registers && (not updates || base `notElem` registers)) "primary initializer uses unsupported LDM registers"
          (locations, updated) <- block (readRegister machine base) mode registers
          (target, restored) <- foldM (\(target, state) (register, location) -> do
            (value, loaded) <- load image incomingSp location state
            if register == A.pc
              then pure (Just value, loaded)
              else do
                changed <- setRegister register value loaded
                pure (target, changed)) (Nothing, entered) (zip registers locations)
          based <- if updates then setRegister base updated restored else pure restored
          case target of
            Just destination -> finish destination based
            Nothing -> advance based
        -- Multiply carries only non-flag-setting MUL/MLA.
        A.Multiply _ destination leftRegister rightRegister accumulator -> do
          require (destination /= A.pc && leftRegister /= A.pc && rightRegister /= A.pc
            && maybe True (/= A.pc) accumulator) "primary initializer uses unpredictable multiply registers"
          require (maybe (instructionWord .&. 0x0000f000 == 0) (const True) accumulator) "primary initializer MUL has a nonzero reserved accumulator field"
          left <- known (scalar (readRegister machine leftRegister))
          right <- known (scalar (readRegister machine rightRegister))
          extra <- maybe (pure 0) (known . scalar . readRegister machine) accumulator
          setRegister destination (Constant (left * right + extra)) entered >>= advance
        A.Branch _ False destination -> branch destination entered
        A.BranchExchange _ False register -> do
          require (register /= A.pc) "primary initializer branches through PC"
          finish (readRegister machine register) entered
        _ -> Left "primary initializer instruction is outside the supported linked proof surface"

deriveArtifact :: Source -> B.ByteString -> B.ByteString -> Either String Artifact
deriveArtifact source image symbolBytes = do
  require (B.length image == fromIntegral (ownedEnd - ownedStart)) "primary initializer image must occupy exactly the entire owned window"
  require (B.length symbolBytes <= 0x10000) "primary initializer symbol table exceeds 64 KiB"
  symbols <- F.parseSymbols symbolBytes
  let at = F.lookupSymbol symbols
  entry <- at "xm1_primary_initializer_entry"
  rust <- at "xm1_primary_initializer_publish"
  linkedStart <- at "__primary_initializer_image_start"
  linkedEnd <- at "__primary_initializer_payload_end"
  imageEnd <- at "__primary_initializer_image_end"
  textStart <- at "__primary_initializer_text_start"
  textEnd <- at "__primary_initializer_text_end"
  rodataStart <- at "__primary_initializer_rodata_start"
  rodataEnd <- at "__primary_initializer_rodata_end"
  capacity <- at "__primary_initializer_capacity"
  require (F.symbolAddress entry == ownedStart && F.symbolSize entry == 68 && F.symbolAddress linkedStart == ownedStart
    && F.symbolAddress textStart == ownedStart && F.symbolAddress imageEnd == ownedEnd
    && F.symbolAddress capacity == ownedEnd - ownedStart) "primary initializer entry or placement differs"
  let rustStart = F.symbolAddress rust
      rustEnd = toInteger rustStart + toInteger (F.symbolSize rust)
      end = F.symbolAddress linkedEnd
  require (rustStart >= ownedStart + 68 && rustStart .&. 3 == 0 && F.symbolSize rust > 0
    && rustEnd <= toInteger (F.symbolAddress textEnd)
    && F.symbolAddress textEnd <= F.symbolAddress rodataStart
    && F.symbolAddress rodataStart <= F.symbolAddress rodataEnd
    && F.symbolAddress rodataEnd <= end && end <= ownedEnd && end .&. 3 == 0) "primary initializer linked code or rodata exceeds its owned window"
  require (B.all (== 0) (B.drop (fromIntegral (end - ownedStart)) image)) "primary initializer image padding is not canonical zero fill"
  let callOffset = toInteger rustStart - toInteger (ownedStart + 16)
  require (callOffset `mod` 4 == 0 && callOffset >= -0x2000000 && callOffset < 0x2000000) "primary initializer adapter BL cannot reach its linked Rust entry"
  let call = 0xeb000000 .|. (fromInteger (callOffset `div` 4) .&. 0xffffff)
      expected = [0xe92d4010,0xe10f4000,call,0xe3c4420f,0xe3844206,0xe128f004,0xe8bd4010
        ,0xe30b0eac,0xe3440005,0xe30d1160,0xe3441005,0xe3012930,0xe3442006,0xe30e3f80,0xe3443004,0xe3a0c078,0xe12fff1e]
  observed <- mapM (leWordAt image) [0,4 .. 64]
  require (observed == expected) "primary initializer adapter does not implement its exact eight-byte frame and return ABI"
  let returns = sourceReturn source
  require (map word [returnR0 returns,returnR1 returns,returnR2 returns,returnR3 returns] == [0x4005beac,0x4005d160,0x40061930,0x4004ef80] && returnR12 returns == 120) "primary initializer source return ABI differs"
  require (length (sourcePublications source) == 1419) "primary initializer source does not supply the complete 1419-word publication"
  let geometry = sourceGeometry source
      loadBase = word (geometryLoad geometry)
      loaderBytes = count (geometryBytes geometry)
      ContainerOffset container = geometryContainerStart geometry
      Physical flash = geometryFlash geometry
  require (ownedStart >= loadBase && toInteger ownedEnd <= toInteger loadBase + toInteger loaderBytes) "primary initializer owned window leaves source segment zero"
  let displacement = fromIntegral (ownedStart - loadBase)
  require (container <= maxBound - displacement && flash <= maxBound - displacement) "primary initializer placement geometry overflows"
  let callerSites = [0x409dc,0x40aac]
  mapM_ (\pc -> do
    require (pc >= loadBase) "primary initializer caller precedes segment zero"
    instruction <- A.decode pc <$> leWordAt (sourceLoader source) (fromIntegral (pc - loadBase))
    require (instruction == A.Branch (A.Cond 14) True ownedStart) "primary initializer static caller BL differs") callerSites
  (scratch, accesses, steps) <- prove source image rust
  imageDigest <- digest image
  symbolsDigest <- digest symbolBytes
  let sourceLoaderBytes = sourceLoader source
      ownedOffset = fromIntegral (ownedStart - loadBase)
      prefix = SHA256.update SHA256.init (B.take ownedOffset sourceLoaderBytes)
      published = SHA256.update prefix image
      complete = SHA256.update published (B.drop (ownedOffset + B.length image) sourceLoaderBytes)
      publicationBytes = BB.toLazyByteString (foldMap (\publication ->
        BB.word32LE (word (publicationGuest publication)) <> BB.word32LE (publicationValue publication)) (sourcePublications source))
  ownedLoaderDigest <- digestHash (SHA256.finalize complete)
  publicationDigest <- digestHash (SHA256.finalize (BL.foldlChunks SHA256.update SHA256.init publicationBytes))
  let identity = ArtifactIdentity (sourceDigest source) (sourceLoaderDigest source) ownedLoaderDigest imageDigest symbolsDigest publicationDigest
      placement = Placement (Guest ownedStart) (Guest ownedEnd) (Guest end) (Guest rustStart) (Guest (fromInteger rustEnd)) (ContainerOffset (container + displacement)) (Physical (flash + displacement))
      obligations = [NaturalCallerContext,BothCoreLowHighRamAliases,CurrentRamDecoder,BothCoreItcmExclusion,BothCoreDtcmExclusion,EarlyCarrierLifetime,ScratchLifetime]
      adapterSlots = [word (callerSp (sourceCaller source)) - 8, word (callerSp (sourceCaller source)) - 4]
      adapterWrites = [Access (Guest ownedStart) (Guest location) ScratchWrite (InstructionCount 0) (Just value)
        | (location,value) <- zip adapterSlots [EnteredRegister (A.Reg 4),EnteredLr]]
      adapterReads = [Access (Guest (ownedStart + 24)) (Guest location) ScratchRead (InstructionCount (steps + 6)) Nothing | location <- adapterSlots]
      metadata = ArtifactMetadata identity placement scratch (sourceCaller source) returns (map Guest callerSites)
        (adapterWrites ++ accesses ++ adapterReads) (InstructionCount (steps + 17)) (PublicationWords 1419) obligations
      emitted = [EmittedPublication (accessGuest access) (publicationValue sourceWord) (accessPc access) (accessOrdinal access)
        | (sourceWord,access) <- zip (sourcePublications source) (filter ((== PublicationWrite) . accessKind) accesses)]
  pure (Artifact metadata image symbolBytes emitted)

stageArtifact :: Source -> Artifact -> Either String StagedFirmware
stageArtifact source (Artifact metadata image _ _) = do
  require (identitySource (metadataIdentity metadata) == sourceDigest source && identitySourceLoader (metadataIdentity metadata) == sourceLoaderDigest source) "primary initializer artifact and canonical source identities differ"
  let original = sourceBytes source
      parsed = sourceContainer source
      parts = U.parsedParts parsed
      geometry = sourceGeometry source
      Guest loadBase = geometryLoad geometry
      ContainerOffset segmentStart = geometryContainerStart geometry
      Physical flash = geometryFlash geometry
      mainStart = toInteger U.stagingLength + toInteger (U.hMainOffset (U.parsedHeader parsed))
      displacement = toInteger ownedStart - toInteger loadBase
      absolute = toInteger segmentStart + displacement
      relative = absolute - mainStart
      old = U.partPreTable parts
      encoded = B.map complement image
  segment <- case filter ((== 0) . U.segId) (U.partSegments parts) of
    [entry] -> pure entry
    _ -> Left "primary initializer source lacks a unique segment zero"
  require (U.segLoad segment == loadBase && U.segSize segment == count (geometryBytes geometry) && toInteger (U.segFlash segment) == toInteger flash && toInteger segmentStart == mainStart + toInteger flash) "primary initializer staging geometry differs from segment zero"
  require (relative >= 0 && relative + toInteger (B.length image) <= toInteger (B.length old)) "primary initializer placement leaves the carried main pre-table span"
  let offset = fromInteger relative
      patched = B.take offset old <> encoded <> B.drop (offset + B.length image) old
  staged <- U.rebuildContainer parsed (parts {U.partPreTable = patched})
  require (B.length staged == B.length original) "primary initializer staging changes the DAT length"
  let start = fromInteger absolute
      stop = start + B.length image
      checksum = U.mainChecksumOffset
      same low high = B.take (high - low) (B.drop low staged) == B.take (high - low) (B.drop low original)
  require (checksum + 4 <= start && same 0 checksum && same (checksum + 4) start && same stop (B.length original)) "primary initializer staging changes bytes outside its entire owned window and main checksum"
  require (B.take (B.length image) (B.drop start staged) == encoded) "primary initializer staging does not contain the entire owned image"
  stagedHash <- digest staged
  pure (StagedFirmware stagedHash staged)

artifactValue :: Artifact -> Value
artifactValue (Artifact metadata _ _ _) = object
  [ "schema" .= (1 :: Int)
  , "kind" .= ("primary-initializer-source-artifact" :: String)
  , "identity" .= object
      [ "source_sha256" .= F.sha256Hex (identitySource identity)
      , "source_loader_a_sha256" .= F.sha256Hex (identitySourceLoader identity)
      , "owned_loader_a_sha256" .= F.sha256Hex (identityOwnedLoader identity)
      , "image_sha256" .= F.sha256Hex (identityImage identity)
      , "symbols_sha256" .= F.sha256Hex (identitySymbols identity)
      , "ordered_publication_sha256" .= F.sha256Hex (identityPublication identity)
      ]
  , "placement" .= object
      [ "owned_window" .= [word (placementStart placement),word (placementEnd placement)]
      , "linked_end" .= word (placementLinkedEnd placement)
      , "rust" .= [word (placementRustStart placement),word (placementRustEnd placement)]
      , "container_offset" .= container, "flash_offset" .= flash
      ]
  , "scratch" .= object
      [ "adapter_bytes" .= count (scratchAdapterBytes scratch)
      , "rust_bytes" .= count (scratchRustBytes scratch)
      , "window" .= [word (scratchLow scratch),word (scratchHigh scratch)]
      ]
  , "static_caller" .= object
      [ "top" .= word (callerTop caller), "sp" .= word (callerSp caller), "fp" .= word (callerFp caller)
      , "bl_sites" .= map word (metadataCallerSites metadata)
      ]
  , "return" .= object
      [ "r0_r3" .= map word [returnR0 returns,returnR1 returns,returnR2 returns,returnR3 returns]
      , "r12" .= returnR12 returns, "nzcv" .= (6 :: Int)
      ]
  , "proof" .= object
      [ "instructions" .= instructions
      , "publication_words" .= publicationWords
      , "accesses" .= map accessValue (metadataAccesses metadata)
      ]
  , "unqualified_source_obligations" .= map obligationName (metadataObligations metadata)
  ]
  where
    identity = metadataIdentity metadata
    placement = metadataPlacement metadata
    scratch = metadataScratch metadata
    caller = metadataCaller metadata
    returns = metadataReturn metadata
    ContainerOffset container = placementContainer placement
    PublicationWords publicationWords = metadataPublicationWords metadata
    InstructionCount instructions = metadataInstructions metadata
    Physical flash = placementFlash placement
    accessValue access = object ["pc" .= word (accessPc access), "guest" .= word (accessGuest access), "kind" .= accessName (accessKind access)
      , "bytes" .= (4 :: Int), "scratch_value" .= accessScratchValue access]
    accessName kind = case kind of
      ScratchRead -> "scratch-read" :: String
      ScratchWrite -> "scratch-write"
      ImageRead -> "image-read"
      PublicationWrite -> "publication-write"
    obligationName obligation = case obligation of
      NaturalCallerContext -> "natural-caller-context" :: String
      BothCoreLowHighRamAliases -> "both-core-low-high-ram-aliases"
      CurrentRamDecoder -> "current-ram-decoder"
      BothCoreItcmExclusion -> "both-core-itcm-exclusion"
      BothCoreDtcmExclusion -> "both-core-dtcm-exclusion"
      EarlyCarrierLifetime -> "early-carrier-lifetime"
      ScratchLifetime -> "scratch-lifetime"
