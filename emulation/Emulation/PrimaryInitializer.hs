{-# LANGUAGE OverloadedStrings #-}
module Emulation.PrimaryInitializer
  ( Contract, Receipt, Role (..), Status (..), Program, RunInput, BootRomInput
  , retainedProgram, ownedProgram, parseRunInput, parseBootRomInput, deriveContract
  , retainedCounterpart
  , contractValue, contractRole, contractSource, contractLoaderBytes
  , parseReceipt, parseReceiptValue, validateReceipt, compareReceipts, receiptStatus
  , receiptAfter, receiptFinal, receiptTableAfter, receiptInputIdentityMatches
  ) where

import qualified A32 as A
import Control.Monad (foldM, forM, forM_, unless)
import Data.Aeson (FromJSON, Object, Value (..), object, (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Pair)
import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32, Word64)
import Emulation.FirstStageWitness (encodeLeWord, leWordAt, sha256Hex)
import qualified Emulation.InterfaceInitializer as I
import Emulation.InterfaceInitializer
  ( Context (..), Views (..), Source (..), ByteOrder (..), Event (..), Mode (..)
  , PhysicalSpan (..), Window (..), BankSnapshot (..), TcmSnapshot (..), WatchClass (..)
  , FixedViews (..), NoFault (..), Rule (..), Watch (..), Alias (..), Decoder (..)
  )
import qualified Emulation.PrimaryInitializerArtifact as PA
import qualified Emulation.PrimaryInitializerSource as PS
import qualified FujiAF.FirstStage as F
import FujiAF.JSON (decodeStrictIntegers)

newtype Guest = Guest Word32 deriving (Eq, Ord)
newtype Physical = Physical Word64 deriving (Eq, Ord)
newtype Instructions = Instructions Word64 deriving (Eq, Ord)
newtype Tick = Tick Word64 deriving (Eq, Ord)
newtype AccessBytes = AccessBytes B.ByteString deriving (Eq)
data Role = RetainedPrimary | OwnedPrimary deriving (Eq, Show)
data Program = RetainedProgram PS.Source | OwnedProgram PS.Source PA.Artifact F.Sha256
newtype RunInput = RunInput F.Sha256
newtype BootRomInput = BootRomInput F.Sha256
data Status = Observed | Absent | Refused deriving (Eq, Show)
data Result = Passed | Failed deriving (Eq)
data Boundary = Caller | Before | Entered | After deriving (Eq, Ord)
data Stop = Stop Boundary Context deriving (Eq)
data OwnedHashes = OwnedHashes F.Sha256 F.Sha256 F.Sha256 F.Sha256 F.Sha256 deriving (Eq)
data ProgramIdentity = RetainedIdentity | OwnedIdentity OwnedHashes deriving (Eq)
data Identity = Identity ProgramIdentity F.Sha256 F.Sha256 F.Sha256 F.Sha256 deriving (Eq)
data Store = Store Guest Word32 Guest Instructions

data FramePremise = FramePremise Guest Guest Guest

data Contract = Contract
  { identity :: Identity
  , loaderBytes :: B.ByteString
  , framePremise :: FramePremise
  , expectedStores :: [Store]
  , expectedReturn :: [Word32]
  , leafInstructions :: Instructions
  , program :: Program
  }

data Publication = Publication Guest Physical Word32 Guest Instructions

data RawEvent = RawEvent Event AccessBytes Tick
data ProgramMemory = RetainedMemory | OwnedMemory Views Views

data Observation = Observation
  { observedIdentity :: Identity
  , callerContext :: Context
  , beforeContext :: Context
  , enteredContext :: Context
  , afterContext :: Context
  , frameBefore :: Views
  , frameAfter :: Views
  , tableBefore :: Views
  , tableAfter :: Views
  , tailBefore :: Views
  , tailAfter :: Views
  , poolsBefore :: Views
  , poolsAfter :: Views
  , codeBefore :: Views
  , codeAfter :: Views
  , decoder :: Decoder
  , aliases :: [Alias]
  , publications :: [Publication]
  , rawWatch :: Watch RawEvent
  , programMemory :: ProgramMemory
  }

data Receipt = Receipt Identity Result Status (Maybe String) (Maybe String) Instructions [Word32] Context [Stop] (Maybe Observation)

budget :: Word64
budget = 50000000

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

retainedProgram :: PS.Source -> Program
retainedProgram = RetainedProgram

ownedProgram :: PS.Source -> PA.Artifact -> B.ByteString -> Either String Program
ownedProgram source artifact candidate = do
  require (B.length candidate == B.length (PS.sourceBytes source)) "owned primary DAT length differs from canonical source"
  staged <- PA.stageArtifact source artifact
  require (candidate == PA.stagedBytes staged) "owned primary DAT differs from the admitted exact source staging"
  pure (OwnedProgram source artifact (PA.stagedDigest staged))

parseRunInput :: B.ByteString -> Either String RunInput
parseRunInput bytes = do
  require (not (B.null bytes) && B.length bytes <= 1024 * 1024) "primary run extent differs"
  digest <- F.parseSha256 (sha256Hex bytes)
  canonical <- F.parseSha256 "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
  require (digest == canonical) "primary requires the canonical natural-reset run"
  pure (RunInput digest)

parseBootRomInput :: B.ByteString -> Either String BootRomInput
parseBootRomInput bytes = do
  require (B.length bytes == 0x4000) "primary boot ROM extent differs"
  digest <- F.parseSha256 (sha256Hex bytes)
  canonical <- F.parseSha256 "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"
  require (digest == canonical) "primary requires the captured canonical boot ROM"
  pure (BootRomInput digest)

programSource :: Program -> PS.Source
programSource (RetainedProgram source) = source
programSource (OwnedProgram source _ _) = source

contractSource :: Contract -> PS.Source
contractSource = programSource . program

identityRole :: Identity -> Role
identityRole (Identity RetainedIdentity _ _ _ _) = RetainedPrimary
identityRole (Identity (OwnedIdentity _) _ _ _ _) = OwnedPrimary

contractRole :: Contract -> Role
contractRole = identityRole . identity

retainedCounterpart :: Contract -> Either String Contract
retainedCounterpart contract =
  let Identity _ _ _ run rom = identity contract
   in deriveContract (retainedProgram (contractSource contract)) (RunInput run) (BootRomInput rom)

contractLoaderBytes :: Contract -> B.ByteString
contractLoaderBytes = loaderBytes

ownedHashesFor :: PA.Artifact -> F.Sha256 -> OwnedHashes
ownedHashesFor artifact inputDigest =
  let bound = PA.metadataIdentity (PA.artifactMetadata artifact)
   in OwnedHashes inputDigest (PA.identitySourceLoader bound) (PA.identityImage bound)
        (PA.identitySymbols bound) (PA.identityPublication bound)

deriveContract :: Program -> RunInput -> BootRomInput -> Either String Contract
deriveContract selected (RunInput runDigest) (BootRomInput romDigest) = do
  let source = programSource selected
      loader = PS.sourceLoader source
  premise <- deriveFrame loader
  (stores,returned,steps,activeLoader,loaderDigest,programIdentity) <- case selected of
    RetainedProgram _ -> do
      (stores,returned,steps) <- derivePublications loader
      require (length stores == 1419 && sort [guest | Store guest _ _ _ <- stores] == map Guest [0x4004dae0,0x4004dae4 .. 0x4004f108])
        "primary source does not publish exactly its complete table"
      require (returned == [0x4005beac,0x4005d160,0x40061930,0x4004ef80,120] && steps == Instructions 4634)
        "primary source return values or instruction extent differs"
      pure (stores,returned,steps,loader,PS.sourceLoaderDigest source,RetainedIdentity)
    OwnedProgram _ artifact inputDigest -> do
      let metadata = PA.artifactMetadata artifact
          artifactIdentity = PA.metadataIdentity metadata
          PA.InstructionCount instructionCount = PA.metadataInstructions metadata
          PS.ReturnValues r0 r1 r2 r3 r12 = PA.metadataReturn metadata
          guest (PS.Guest value) = value
          emittedStore publication =
            let PA.InstructionCount ordinal = PA.emittedOrdinal publication
             in Store (Guest (guest (PA.emittedGuest publication))) (PA.emittedValue publication)
                  (Guest (guest (PA.emittedPc publication))) (Instructions ordinal)
          image = PA.artifactImage artifact
          active = B.take 0x430 loader <> image <> B.drop 0x9ac loader
          hashes = ownedHashesFor artifact inputDigest
      pure (map emittedStore (PA.artifactPublications artifact),map guest [r0,r1,r2,r3] ++ [r12],
        Instructions instructionCount,active,PA.identityOwnedLoader artifactIdentity,OwnedIdentity hashes)
  pure (Contract (Identity programIdentity (PS.sourceDigest source) loaderDigest runDigest romDigest)
    activeLoader premise stores returned steps selected)

instruction :: B.ByteString -> Word32 -> Either String A.Instruction
instruction loader pc = do
  require (pc >= 0x40000 && pc < 0x4dae0 && pc .&. 3 == 0) "primary source instruction leaves loader A"
  A.decode pc <$> leWordAt loader (fromIntegral (pc - 0x40000))

deriveFrame :: B.ByteString -> Either String FramePremise
deriveFrame loader = do
  load <- instruction loader 0x400a8
  literal <- case load of
    A.Load (A.Cond 14) A.Word False (A.Reg 4) base (A.OffsetImmediate offset) A.OffsetOnly | base == A.pc ->
      pure (0xb0 + toInteger offset)
    _ -> Left "primary stack literal load differs"
  require (literal >= 0 && literal + 4 <= toInteger (B.length loader)) "primary stack literal leaves loader A"
  top <- leWordAt loader (fromInteger literal)
  let same pc expected = instruction loader pc >>= \actual -> require (actual == expected) "primary caller or callee source premise differs"
      always = A.Cond 14
      reg = A.Reg
      immediate = A.Immediate
      copy destination source = A.DataProcessing always A.MOV False destination (reg 0) (A.ShiftedImmediate source A.LSL 0)
  same 0x400ac (copy A.sp (reg 4))
  same 0x400c4 (A.Branch always False 0x409ac)
  same 0x409ac (A.StoreMultiple always A.sp A.DecrementBefore True [reg 4,reg 5,reg 11,A.lr])
  same 0x409b0 (A.DataProcessing always A.ADD False (reg 11) A.sp (immediate 12))
  same 0x409b4 (copy (reg 4) (reg 0))
  same 0x409d8 (A.Branch always True 0x434f0)
  same 0x409dc (A.Branch always True 0x40430)
  same 0x40430 (A.StoreMultiple always A.sp A.DecrementBefore True [reg 4,reg 5,reg 11])
  same 0x40434 (A.DataProcessing always A.ADD False (reg 11) A.sp (immediate 8))
  same 0x409a0 (A.DataProcessing always A.SUB False A.sp (reg 11) (immediate 8))
  same 0x409a4 (A.LoadMultiple always A.sp A.IncrementAfter True [reg 4,reg 5,reg 11])
  same 0x409a8 (A.BranchExchange always False A.lr)
  require (top == 0x40063460) "primary source stack top differs"
  pure (FramePremise (Guest top) (Guest (top - 16)) (Guest (top - 4)))

-- The scalar slice excludes the callee's save and restore instructions.
derivePublications :: B.ByteString -> Either String ([Store], [Word32], Instructions)
derivePublications loader = walk 0x40438 M.empty Nothing 2 []
  where
    known registers register = maybe (Left "primary source uses an unknown scalar") Right (M.lookup register registers)
    operand registers value = case value of
      A.Immediate immediate -> Right immediate
      A.ShiftedImmediate register A.LSL amount | amount >= 0 && amount < 32 -> (`shiftL` amount) <$> known registers register
      _ -> Left "primary source scalar operand differs"
    walk pc registers comparison steps stores = do
      require (steps <= 5000) "primary source scalar slice exceeds its finite extent"
      if pc == 0x409a0 then do
        returned <- mapM (known registers . A.Reg) [0,1,2,3,12]
        require (comparison == Just True) "primary source loop did not finish at equality"
        pure (reverse stores, returned, Instructions (steps + 3))
      else do
        require (pc >= 0x40438 && pc < 0x409a0) "primary source branch leaves its scalar slice"
        decoded <- instruction loader pc
        let next changed flags writes = walk (pc + 4) changed flags (steps + 1) writes
            assign register value = next (M.insert register value registers) comparison stores
        case decoded of
          A.MoveWide (A.Cond 14) register value -> assign register value
          A.MoveTop (A.Cond 14) register value -> do
            low <- known registers register
            assign register ((low .&. 0xffff) .|. (value `shiftL` 16))
          A.DataProcessing (A.Cond 14) operation flags destination source value -> do
            right <- operand registers value
            case (operation,flags) of
              (A.MOV,False) -> assign destination right
              (A.ADD,False) -> known registers source >>= \left -> assign destination (left + right)
              (A.RSB,False) -> known registers source >>= \left -> assign destination (right - left)
              (A.CMP,True) -> known registers source >>= \left -> next registers (Just (left == right)) stores
              _ -> Left "primary source scalar instruction differs"
          A.Store (A.Cond 14) A.Word value base displacement A.OffsetOnly -> do
            root <- known registers base
            word <- known registers value
            offset <- case displacement of
              A.OffsetImmediate immediate -> Right (fromIntegral immediate)
              A.OffsetRegister True register A.LSL 0 -> known registers register
              _ -> Left "primary source publication displacement differs"
            let guest = root + offset
            require (guest >= 0x4004dae0 && guest + 4 <= 0x4004f10c && guest .&. 3 == 0)
              "primary source store leaves its publication table"
            next registers comparison (Store (Guest guest) word (Guest pc) (Instructions steps) : stores)
          A.Branch condition False target -> do
            taken <- case condition of
              A.Cond 14 -> Right True
              A.Cond 1 -> maybe (Left "primary source conditional branch lacks comparison") (Right . not) comparison
              _ -> Left "primary source branch condition differs"
            walk (if taken then target else pc + 4) registers comparison (steps + 1) stores
          _ -> Left "primary source instruction is outside the retained scalar slice"

contractValue :: Contract -> Value
contractValue contract = object ["schema" .= (8 :: Word32),"primary" .= object
  (identityFields (identity contract) ++ ["budget" .= budget] ++ case program contract of
    RetainedProgram _ -> []
    OwnedProgram source artifact inputDigest -> ["owned" .= ownedContractValue source artifact (ownedHashesFor artifact inputDigest)])]

identityFields :: Identity -> [Pair]
identityFields bound@(Identity _ source loader run rom) =
  [ "role" .= roleName (identityRole bound),"source_sha256" .= F.sha256Hex source
  , "loader_a_sha256" .= F.sha256Hex loader,"run_sha256" .= F.sha256Hex run,"boot_rom_sha256" .= F.sha256Hex rom
  ]

roleName :: Role -> String
roleName RetainedPrimary = "retained-primary"
roleName OwnedPrimary = "owned-primary"

ownedHashFields :: OwnedHashes -> [Pair]
ownedHashFields (OwnedHashes input sourceLoader image symbols publication) =
  [ "input_sha256" .= F.sha256Hex input,"source_loader_a_sha256" .= F.sha256Hex sourceLoader
  , "image_sha256" .= F.sha256Hex image,"symbols_sha256" .= F.sha256Hex symbols
  , "publication_sha256" .= F.sha256Hex publication
  ]

ownedContractValue :: PS.Source -> PA.Artifact -> OwnedHashes -> Value
ownedContractValue source artifact hashes = object (ownedHashFields hashes ++
  [ "source_loader" .= B.unpack (PS.sourceLoader source),"image" .= B.unpack (PA.artifactImage artifact)
  , "symbols" .= B.unpack (PA.artifactSymbols artifact)
  , "scratch" .= object ["low" .= guest (PA.scratchLow scratch),"high" .= guest (PA.scratchHigh scratch)
      ,"rust_bytes" .= count (PA.scratchRustBytes scratch),"adapter_bytes" .= count (PA.scratchAdapterBytes scratch)]
  , "instructions" .= instructions
  , "publications" .= [object ["guest" .= guest (PA.emittedGuest publication),"value" .= PA.emittedValue publication
      ,"pc" .= guest (PA.emittedPc publication),"ordinal" .= ordinal (PA.emittedOrdinal publication)]
      | publication <- PA.artifactPublications artifact]
  , "accesses" .= [object ["guest" .= guest (PA.accessGuest access),"pc" .= guest (PA.accessPc access)
      ,"ordinal" .= ordinal (PA.accessOrdinal access),"kind" .= accessName (PA.accessKind access)
      ,"scratch_value" .= PA.accessScratchValue access]
      | access <- PA.artifactAccesses artifact]
  ])
  where
    scratch = PA.metadataScratch (PA.artifactMetadata artifact)
    PA.InstructionCount instructions = PA.metadataInstructions (PA.artifactMetadata artifact)
    guest (PS.Guest value) = value
    count (PS.ByteCount value) = value
    ordinal (PA.InstructionCount value) = value
    accessName kind = case kind of
      PA.ScratchRead -> "scratch-read" :: String
      PA.ScratchWrite -> "scratch-write"
      PA.ImageRead -> "image-read"
      PA.PublicationWrite -> "publication-write"

fields :: [String] -> Value -> Either String Object
fields names (Object row) = do
  require (sort names == sort (map Key.toString (KM.keys row))) "primary receipt fields differ"
  pure row
fields _ _ = Left "primary receipt value is not an object"

field :: Object -> String -> Either String Value
field row name = maybe (Left ("missing primary receipt field: " ++ name)) Right (KM.lookup (Key.fromString name) row)

json :: FromJSON a => Value -> Either String a
json value = case J.fromJSON value of
  J.Error message -> Left message
  J.Success parsed -> Right parsed

get :: FromJSON a => Object -> String -> Either String a
get row name = field row name >>= json

nullable :: (Value -> Either String a) -> Value -> Either String (Maybe a)
nullable _ Null = Right Nothing
nullable parse value = Just <$> parse value

parseRole :: Value -> Either String Role
parseRole value = do
  text <- json value :: Either String String
  case text of
    "retained-primary" -> Right RetainedPrimary
    "owned-primary" -> Right OwnedPrimary
    _ -> Left "unsupported primary role"

parseIdentity :: Value -> Either String Identity
parseIdentity value = do
  selected <- case value of
    Object row -> field row "role" >>= parseRole
    _ -> Left "primary identity is not an object"
  row <- fields (["role","source_sha256","loader_a_sha256","run_sha256","boot_rom_sha256"] ++ ["owned" | selected == OwnedPrimary]) value
  let digest name = get row name >>= F.parseSha256
  selectedIdentity <- case selected of
    RetainedPrimary -> pure RetainedIdentity
    OwnedPrimary -> do
      owned <- field row "owned" >>= fields ["input_sha256","source_loader_a_sha256","image_sha256","symbols_sha256","publication_sha256"]
      let ownedDigest name = get owned name >>= F.parseSha256
      OwnedIdentity <$> (OwnedHashes <$> ownedDigest "input_sha256" <*> ownedDigest "source_loader_a_sha256"
        <*> ownedDigest "image_sha256" <*> ownedDigest "symbols_sha256" <*> ownedDigest "publication_sha256")
  Identity selectedIdentity <$> digest "source_sha256" <*> digest "loader_a_sha256" <*> digest "run_sha256" <*> digest "boot_rom_sha256"

parseContext :: Value -> Either String Context
parseContext value = do
  _ <- fields ["pc","registers","sp","lr","cpsr","counts","tcm"] value
  context@(Context pc registers sp _ _ counts tcm) <- json value
  require (length registers == 13 && length counts == 2 && length tcm == 2 && all ((== 2) . length) tcm)
    "primary context register, count or TCM width differs"
  require (pc .&. 3 == 0 && sp .&. 3 == 0 && all (<= budget) counts && sum counts <= budget)
    "primary context alignment or finite instruction count differs"
  pure context

parseViews :: Value -> Either String Views
parseViews value = fields ["virtual","physical"] value >> json value

parseAlias :: Value -> Either String Alias
parseAlias value = fields ["core","guest","physical","size","sha256"] value >> json value

parseDecoder :: Value -> Either String Decoder
parseDecoder value = fields ["choices","pending"] value >> json value

parseSnapshot :: Value -> Either String TcmSnapshot
parseSnapshot value = do
  row <- fields ["core","atcm","btcm"] value
  forM_ ["atcm","btcm"] $ \name -> field row name >>= fields ["register","window"] >> pure ()
  json value

parseRule :: Value -> Either String Rule
parseRule value = do
  row <- fields ["cores","kind","mode","spans","tcm"] value
  spans <- get row "spans" :: Either String [Value]
  forM_ spans $ \span' -> fields ["physical"] span' >> pure ()
  snapshots <- get row "tcm" >>= mapM parseSnapshot
  Rule <$> get row "cores" <*> get row "kind" <*> get row "mode" <*> get row "spans" <*> pure snapshots

parseEvent :: Value -> Either String RawEvent
parseEvent value = do
  row <- case value of
    Object object' -> fields (["source","core","address","physical","size","value","pc","instructions","byte_order","before","tick"]
      ++ ["tcm" | KM.member "tcm" object']) value
    _ -> Left "primary watch event is not an object"
  before <- get row "before" :: Either String [Word8]
  require (length before == 4) "primary publication preimage is not four bytes"
  case KM.lookup "tcm" row of
    Nothing -> pure ()
    Just Null -> pure ()
    _ -> Left "primary publication resolves to TCM"
  RawEvent <$> json value <*> pure (AccessBytes (B.pack before)) <*> (Tick <$> get row "tick")

parseWatch :: Value -> Either String (Watch RawEvent)
parseWatch value = do
  row <- fields ["enabled","ranges","violation","records","classes","views","fault","tcm","confinement","counters","coverage"] value
  ranges <- get row "ranges" :: Either String [Window]
  counters <- get row "counters" :: Either String [Value]
  coverage <- get row "coverage" :: Either String [Value]
  require (ranges == [Window (0x40000,0x63460)] && null counters && null coverage)
    "primary watch ranges, counters or coverage differ"
  classes <- get row "classes" :: Either String [Value]
  forM_ classes $ \class' -> fields ["range","kinds"] class' >> pure ()
  events <- get row "records" >>= mapM parseEvent
  rules <- get row "confinement" >>= mapM parseRule
  snapshots <- get row "tcm" >>= mapM parseSnapshot
  Watch <$> get row "enabled" <*> get row "violation" <*> pure events <*> pure rules <*> get row "classes"
    <*> get row "views" <*> get row "fault" <*> pure snapshots

parsePublication :: Value -> Either String Publication
parsePublication value = do
  row <- fields ["guest","physical","value","pc","instructions"] value
  Publication <$> (Guest <$> get row "guest") <*> (Physical <$> get row "physical") <*> get row "value"
    <*> (Guest <$> get row "pc") <*> (Instructions <$> get row "instructions")

parseObservation :: Value -> Either String Observation
parseObservation value = do
  bound <- case value of
    Object initial -> field initial "contract" >>= parseIdentity
    _ -> Left "primary observation is not an object"
  row <- fields (["contract","caller","before","entered","after","frame_before","frame_after","table_before","table_after"
    ,"tail_before","tail_after","pools_before","pools_after","code_before","code_after","decoder","aliases","publications","watch"]
    ++ (if identityRole bound == OwnedPrimary then ["scratch_before","scratch_after"] else [])) value
  let context name = field row name >>= parseContext
      views name = field row name >>= parseViews
  memory <- case identityRole bound of
    RetainedPrimary -> pure RetainedMemory
    OwnedPrimary -> OwnedMemory <$> views "scratch_before" <*> views "scratch_after"
  Observation bound <$> context "caller" <*> context "before" <*> context "entered" <*> context "after"
    <*> views "frame_before" <*> views "frame_after" <*> views "table_before" <*> views "table_after"
    <*> views "tail_before" <*> views "tail_after" <*> views "pools_before" <*> views "pools_after"
    <*> views "code_before" <*> views "code_after" <*> (field row "decoder" >>= parseDecoder)
    <*> (get row "aliases" >>= mapM parseAlias) <*> (get row "publications" >>= mapM parsePublication) <*> (field row "watch" >>= parseWatch) <*> pure memory

parseStop :: Value -> Either String Stop
parseStop value = do
  row <- fields ["boundary","context"] value
  name <- get row "boundary" :: Either String String
  boundary <- case name of
    "caller" -> Right Caller
    "before" -> Right Before
    "entered" -> Right Entered
    "after" -> Right After
    _ -> Left "unsupported primary boundary"
  context <- field row "context" >>= parseContext
  let expectedPc = case boundary of Caller -> 0x409ac; Before -> 0x409dc; Entered -> 0x40430; After -> 0x409e0
      Context pc _ _ _ _ _ _ = context
  require (pc == expectedPc) "primary boundary PC differs"
  pure (Stop boundary context)

parseReceipt :: B.ByteString -> Either String Receipt
parseReceipt bytes = do
  require (B.length bytes <= 32 * 1024 * 1024) "primary receipt exceeds 32 MiB"
  decodeStrictIntegers bytes >>= parseReceiptValue

parseReceiptValue :: Value -> Either String Receipt
parseReceiptValue value = do
  row <- fields ["schema","role","contract","result","status","error","machine_failure","instructions","cp14_identities","final","boundaries","primary"] value
  schema <- get row "schema" :: Either String Word32
  require (schema == 8) "unsupported primary receipt schema"
  role <- field row "role" >>= parseRole
  receiptIdentity <- field row "contract" >>= parseIdentity
  require (role == identityRole receiptIdentity) "primary top-level role differs from receipt identity"
  resultName <- get row "result" :: Either String String
  result <- case resultName of "passed" -> Right Passed; "failed" -> Right Failed; _ -> Left "unsupported primary result"
  statusName <- get row "status" :: Either String String
  status <- case statusName of
    "observed" -> Right Observed
    "absent" -> Right Absent
    "refused" -> Right Refused
    _ -> Left "unsupported primary status"
  failure <- get row "error"
  machineFailure <- get row "machine_failure"
  instructions <- get row "instructions"
  identities <- get row "cp14_identities"
  final <- field row "final" >>= parseContext
  boundaries <- get row "boundaries" >>= mapM parseStop
  primary <- field row "primary" >>= nullable parseObservation
  let Context _ _ _ _ _ finalCounts _ = final
      boundaryNames = [boundary | Stop boundary _ <- boundaries]
  require (instructions == sum finalCounts && instructions <= budget && identities == [0x4003,0x5003])
    "primary receipt whole-machine progress or identities differ"
  require (boundaryNames == take (length boundaryNames) [Caller,Before,Entered,After]) "primary boundary order differs"
  forM_ (zip boundaries (drop 1 boundaries)) $ \(Stop _ first,Stop _ second) -> progressing first second
  forM_ boundaries $ \(Stop _ context) -> progressing context final
  case status of
    Observed -> do
      require (result == Passed && failure == Nothing && machineFailure == Nothing && length boundaries == 4)
        "observed primary receipt did not pass all boundaries"
      observation <- maybe (Left "observed primary receipt lacks evidence") Right primary
      require (observedIdentity observation == receiptIdentity) "primary observation identity differs from the whole receipt"
      require (boundaries == [Stop Caller (callerContext observation),Stop Before (beforeContext observation)
        ,Stop Entered (enteredContext observation),Stop After (afterContext observation)] && final == afterContext observation)
        "primary boundary contexts or final context differ from the observation"
    Absent -> require (result == Failed && failure /= Nothing && machineFailure == Nothing && instructions == budget
      && length boundaries < 4 && absent primary) "primary absence is not a finite unobserved deadline"
    Refused -> require (result == Failed && failure /= Nothing && absent primary) "primary refused result carries accepted evidence"
  pure (Receipt receiptIdentity result status failure machineFailure (Instructions instructions) identities final boundaries primary)
  where
    absent Nothing = True
    absent (Just _) = False

receiptStatus :: Receipt -> Status
receiptStatus (Receipt _ _ status _ _ _ _ _ _ _) = status

receiptFinal :: Receipt -> Context
receiptFinal (Receipt _ _ _ _ _ _ _ final _ _) = final

receiptAfter :: Receipt -> Either String Context
receiptAfter (Receipt _ _ _ _ _ _ _ _ _ evidence) =
  maybe (Left "primary receipt has no observed return") (Right . afterContext) evidence

receiptTableAfter :: Receipt -> Either String B.ByteString
receiptTableAfter (Receipt _ _ _ _ _ _ _ _ _ evidence) =
  maybe (Left "primary receipt has no committed table") (viewBytes 0x162c . tableAfter) evidence

receiptInputIdentityMatches :: Contract -> Receipt -> Either String ()
receiptInputIdentityMatches contract (Receipt bound _ _ _ _ _ _ _ _ _) =
  require (bound == identity contract) "primary receipt whole-input identity differs"

data SemanticContext = SemanticContext Word32 [Word32] Word32 Word32 Word32 [[Word32]] deriving (Eq)

semanticContext :: Context -> SemanticContext
semanticContext (Context pc registers sp lr cpsr _ banks) = SemanticContext pc registers sp lr cpsr banks

data Semantics = Semantics SemanticContext SemanticContext SemanticContext SemanticContext [Views] [(B.ByteString,B.ByteString)] deriving (Eq)

semantics :: Receipt -> Either String Semantics
semantics (Receipt _ _ _ _ _ _ _ _ _ evidence) = do
  observation <- maybe (Left "primary comparison lacks an observation") Right evidence
  code <- forM [codeBefore observation,codeAfter observation] $ \view -> do
    bytes <- viewBytes 0xdae0 view
    pure (B.take 0x430 bytes,B.drop 0x9ac bytes)
  pure (Semantics (semanticContext (callerContext observation)) (semanticContext (beforeContext observation))
    (semanticContext (enteredContext observation)) (semanticContext (afterContext observation))
    [frameBefore observation,frameAfter observation,tableBefore observation,tableAfter observation
    ,tailBefore observation,tailAfter observation,poolsBefore observation,poolsAfter observation] code)

compareReceipts :: Contract -> Receipt -> Contract -> Receipt -> Either String ()
compareReceipts retained before owned after = do
  require (contractRole retained == RetainedPrimary && contractRole owned == OwnedPrimary)
    "primary comparison requires retained then owned"
  validateReceipt retained before
  validateReceipt owned after
  require (PS.sourceDigest (contractSource retained) == PS.sourceDigest (contractSource owned))
    "primary comparison canonical source differs"
  left <- semantics before
  right <- semantics after
  require (left == right) "primary comparison caller, preserved state or memory semantics differ"

progressing :: Context -> Context -> Either String ()
progressing (Context _ _ _ _ _ before _) (Context _ _ _ _ _ after _) =
  require (and (zipWith (<=) before after)) "primary core progress decreased"

viewBytes :: Int -> Views -> Either String B.ByteString
viewBytes size (Views virtual physical) = do
  require (length virtual == size && virtual == physical) "primary virtual/physical readback extent or bytes differ"
  pure (B.pack physical)

validateReceipt :: Contract -> Receipt -> Either String ()
validateReceipt contract receipt@(Receipt _ result status failure machineFailure _ _ _ _ evidence) = do
  receiptInputIdentityMatches contract receipt
  require (result == Passed && status == Observed && failure == Nothing && machineFailure == Nothing)
    "primary validation accepts only an observed initializer"
  observation <- maybe (Left "primary observation is absent") Right evidence
  require (observedIdentity observation == identity contract) "primary observation whole-input identity differs"
  let Context callerPc caller callerSp callerLr callerCpsr callerCounts callerTcm = callerContext observation
      Context incomingPc incoming sp incomingLr cpsr counts tcm = beforeContext observation
      Context enteringPc entering enteringSp enteringLr enteringCpsr enteringCounts enteringTcm = enteredContext observation
      Context outgoingPc outgoing outgoingSp outgoingLr outgoingCpsr outgoingCounts outgoingTcm = afterContext observation
      FramePremise (Guest top) (Guest expectedSp) (Guest expectedFp) = framePremise contract
      Instructions leafCount = leafInstructions contract
      admittedCpsr value = value .&. 0x07f0feff == 0xd3
  require (callerPc == 0x409ac && callerSp == top && caller !! 4 == top && callerLr == 0x400c0 && caller !! 0 == 0x69cdf8
    && admittedCpsr callerCpsr && callerTcm == tcm)
    "primary natural caller stack or entry argument differs from source"
  require (incomingPc == 0x409dc && sp == expectedSp && incoming !! 11 == expectedFp && incoming !! 4 == caller !! 0
    && incoming !! 5 == caller !! 5 && take 5 (drop 6 incoming) == take 5 (drop 6 caller)
    && incomingLr == 0x409dc && admittedCpsr cpsr && callerCpsr .&. 0x0fffffff == cpsr .&. 0x0fffffff)
    "primary pre-BL caller frame or preserved registers differ"
  require (enteringPc == 0x40430 && entering == incoming && enteringSp == sp && enteringLr == 0x409e0
    && enteringCpsr == cpsr && enteringTcm == tcm) "primary BL changed state beyond PC/LR"
  case (callerCounts,counts,enteringCounts,outgoingCounts) of
    ([s0,s1],[b0,b1],[e0,e1],[a0,a1]) -> require
      (s0 < b0 && s1 <= b1 && e0 == b0 + 1 && e1 >= b1 && a0 == e0 + leafCount && a1 >= e1)
      "primary source-bound core instruction progress differs"
    _ -> Left "primary core count width differs"
  require (outgoingPc == 0x409e0 && outgoingSp == sp && outgoingLr == 0x409e0
    && drop 4 (take 12 outgoing) == drop 4 (take 12 incoming)
    && take 4 outgoing ++ [outgoing !! 12] == expectedReturn contract
    && outgoingCpsr == ((cpsr .&. 0x0fffffff) .|. 0x60000000) && outgoingTcm == tcm)
    "primary exact leaf return state differs"
  frame <- viewBytes 16 (frameBefore observation)
  _ <- viewBytes 16 (frameAfter observation)
  require (frameBefore observation == frameAfter observation
    && mapM (leWordAt frame) [0,4,8,12] == Right [caller !! 4,caller !! 5,caller !! 11,callerLr])
    "primary caller frame preimage or preservation differs"
  prestate <- viewBytes 0x162c (tableBefore observation)
  committed <- viewBytes 0x162c (tableAfter observation)
  forM_ [(16,tailBefore observation,tailAfter observation),(0x5aa0,poolsBefore observation,poolsAfter observation)] $ \(size,before,after) -> do
    _ <- viewBytes size before
    _ <- viewBytes size after
    require (before == after) "primary retained tail or pools changed"
  forM_ [codeBefore observation,codeAfter observation] $ \view -> do
    code <- viewBytes 0xdae0 view
    require (code == loaderBytes contract) "primary before/after loader A bytes differ from canonical source"
  validateWatch contract observation tcm prestate committed enteringCounts outgoingCounts
  validateAliases contract (aliases observation)

validateWatch :: Contract -> Observation -> [[Word32]] -> B.ByteString -> B.ByteString -> [Word64] -> [Word64] -> Either String ()
validateWatch contract observation tcm prestate committed enteringCounts outgoingCounts = do
  let Watch enabled violation events rules classes FixedViews NoFault snapshots = rawWatch observation
      Decoder choices pending = decoder observation
  require (enabled && violation == Nothing && choices == [1] && pending == [Nothing]) "primary continuous watch or decoder differs"
  expectedTcm <- forM (zip [0,1] tcm) $ \(core,banks) -> case banks of
    [atcm,btcm] -> TcmSnapshot core <$> I.bankSnapshot atcm <*> I.bankSnapshot btcm
    _ -> Left "primary TCM bank context width differs"
  require (snapshots == expectedTcm) "primary watch both-core TCM snapshots differ"
  let windows = [Window (alias + low,alias + high) | alias <- [0,0x40000000],(low,high) <- [(0x40000,0x4dae0),(0x4dae0,0x63460)]]
      disjoint (Window (low,high)) (Window (otherLow,otherHigh)) = high <= otherLow || otherHigh <= low
  forM_ expectedTcm $ \(TcmSnapshot _ atcm btcm) ->
    forM_ [atcm,btcm] $ \(BankSnapshot _ active) ->
      forM_ active $ \window -> require (all (disjoint window) windows) "primary TCM remaps a loader, table, pool or stack alias"
  let physical low high = PhysicalSpan [low,high]
      rule cores kind mode spans = Rule cores kind mode spans [snapshot | snapshot@(TcmSnapshot core _ _) <- expectedTcm,core `elem` cores]
      (Physical scratchLow,Physical scratchHigh) = scratchBounds contract
      expectedRules =
        [ rule [0] CpuWrite Allow [physical 0x4dae0 0x4f10c,physical scratchLow scratchHigh]
        , rule [1] CpuWrite Forbid [physical 0x4dae0 0x63460,physical 0x40000 0x4dae0]
        , case program contract of
            RetainedProgram _ -> rule [0] CpuRead Forbid [physical 0x4dae0 0x4f10c]
            OwnedProgram _ _ _ -> rule [0] CpuRead Allow [physical 0x40430 0x409ac,physical scratchLow scratchHigh]
        ] ++ case program contract of
          RetainedProgram _ -> []
          OwnedProgram _ _ _ -> [rule [1] CpuFetch Forbid [physical 0x40430 0x409ac]]
      writes = [CpuWrite,HostWrite,ModelBusWrite]
      expectedClasses =
        [ WatchClass (Window (0x4dae0,0x4f10c)) writes
        , WatchClass (Window (0x4dae0,0x63460)) [HostWrite,ModelBusWrite]
        , WatchClass (Window (0x40000,0x4dae0)) writes
        ] ++ case program contract of
          RetainedProgram _ -> []
          OwnedProgram _ _ _ ->
            [ WatchClass (Window (0x40430,0x409ac)) [CpuRead]
            , WatchClass (Window (scratchLow,scratchHigh)) [CpuRead,CpuWrite]
            ]
      normalizeClass (WatchClass range kinds) = WatchClass range (sort kinds)
  require (rules == expectedRules && sort (map normalizeClass classes) == sort (map normalizeClass expectedClasses))
    "primary exact watch confinement or source classes differ"
  let stores = expectedStores contract
      observedPublications = publications observation
      publicationEvents = case program contract of
        RetainedProgram _ -> events
        OwnedProgram _ _ _ -> filter isPublication events
  require (length publicationEvents == 1419 && length observedPublications == 1419 && length stores == 1419) "primary publication coverage differs"
  forM_ (zip3 stores observedPublications publicationEvents) $ \(Store (Guest guest) value expectedPc (Instructions ordinal),
      Publication observedGuest observedPhysical observedValue pc (Instructions instructions),
      RawEvent (Event kind core address rawPhysical width eventValue eventPc eventInstructions order) (AccessBytes before) _) -> do
    let physicalAddress = fromIntegral guest - 0x40000000
        offset = fromIntegral (guest - 0x4004dae0)
    require (observedGuest == Guest guest && observedPhysical == Physical physicalAddress && observedValue == value && pc == expectedPc
      && instructions >= sum enteringCounts + ordinal && instructions < sum outgoingCounts
      && kind == CpuWrite && core == Just 0 && address == fromIntegral guest && rawPhysical == Just physicalAddress && width == 4
      && eventValue == Just (fromIntegral value) && eventPc == Just (let Guest location = pc in location)
      && eventInstructions == Just instructions && order == Just LittleEndian)
      "primary publication order, value, source PC or raw attribution differs"
    require (before == B.take 4 (B.drop offset prestate) && leWordAt committed offset == Right value)
      "primary raw publication preimage or committed readback differs"
  let clocks = [clock | Publication _ _ _ _ clock <- observedPublications]
      ticks = [tick | RawEvent _ _ tick <- events]
      progresses = case program contract of
        RetainedProgram _ -> (<)
        OwnedProgram _ _ _ -> (<=)
  require (and (zipWith progresses clocks (drop 1 clocks)) && and (zipWith (<=) ticks (drop 1 ticks)))
    "primary publication instruction groups or raw ticks decreased"
  validateProgramMemory contract observation events prestate committed enteringCounts outgoingCounts

scratchBounds :: Contract -> (Physical,Physical)
scratchBounds contract = case program contract of
  RetainedProgram _ -> (Physical 0x63444,Physical 0x63450)
  OwnedProgram _ artifact _ ->
    let scratch = PA.metadataScratch (PA.artifactMetadata artifact)
        PS.Guest low = PA.scratchLow scratch
        PS.Guest high = PA.scratchHigh scratch
     in (Physical (fromIntegral low - 0x40000000),Physical (fromIntegral high - 0x40000000))

isPublication :: RawEvent -> Bool
isPublication (RawEvent (Event kind _ _ physical _ _ _ _ _) _ _) =
  kind == CpuWrite && maybe False (\location -> location >= 0x4dae0 && location < 0x4f10c) physical

validateProgramMemory :: Contract -> Observation -> [RawEvent] -> B.ByteString -> B.ByteString -> [Word64] -> [Word64] -> Either String ()
validateProgramMemory contract observation events prestate committed enteringCounts outgoingCounts =
  case (program contract,programMemory observation) of
    (RetainedProgram _,RetainedMemory) -> pure ()
    (OwnedProgram _ artifact _,OwnedMemory before after) -> do
      let scratch = PA.metadataScratch (PA.artifactMetadata artifact)
          PS.Guest low = PA.scratchLow scratch
          PS.Guest high = PA.scratchHigh scratch
          bytes = fromIntegral (high - low)
          accesses = PA.artifactAccesses artifact
      beforeBytes <- viewBytes bytes before
      afterBytes <- viewBytes bytes after
      initial <- M.fromList <$> forM [low,low + 4 .. high - 4] (\location ->
        (,) location <$> leWordAt beforeBytes (fromIntegral (location - low)))
      require (length events == length accesses) "owned primary emitted access coverage differs"
      case (enteringCounts,outgoingCounts) of
        ([e0,e1],[a0,a1]) -> do
          (final,_,_) <- foldM (step e0 e1 a0 a1) (initial,e1,Nothing) (zip accesses events)
          forM_ (M.toList final) $ \(location,value) ->
            require (leWordAt afterBytes (fromIntegral (location - low)) == Right value) "owned primary scratch readback differs from raw writes"
        _ -> Left "owned primary access count width differs"
    _ -> Left "primary scratch evidence role differs"
  where
    step e0 e1 a0 a1 (memory,lastCore1,previous)
      (access,RawEvent (Event kind core address physical width eventValue eventPc eventInstructions order) (AccessBytes before) _) = do
      let PS.Guest guest = PA.accessGuest access
          PS.Guest pc = PA.accessPc access
          PA.InstructionCount ordinal = PA.accessOrdinal access
          expectedKind = case PA.accessKind access of
            PA.ScratchWrite -> CpuWrite
            PA.PublicationWrite -> CpuWrite
            PA.ScratchRead -> CpuRead
            PA.ImageRead -> CpuRead
          expectedPhysical = if guest < 0x100000 then fromIntegral guest else fromIntegral guest - 0x40000000
          enteredCore0 = e0 + ordinal + 1
      instructions <- maybe (Left "owned primary access lacks instruction clock") Right eventInstructions
      value <- maybe (Left "owned primary access lacks value") Right eventValue
      require (kind == expectedKind && core == Just 0 && address == fromIntegral guest && physical == Just expectedPhysical
        && width == 4 && eventPc == Just pc && order == Just LittleEndian && value <= 0xffffffff)
        "owned primary emitted access attribution differs"
      require (enteredCore0 <= a0 && instructions >= enteredCore0 && instructions < a0 + a1)
        "owned primary access leaves its instruction interval"
      let actualCore1 = instructions - enteredCore0
          wordValue = fromIntegral value
      require (actualCore1 >= e1 && actualCore1 >= lastCore1 && actualCore1 <= a1)
        "owned primary access clock lacks monotone other-core progress"
      forM_ previous $ \(lastOrdinal,lastInstructions) ->
        require (if ordinal == lastOrdinal then instructions == lastInstructions else ordinal > lastOrdinal && instructions > lastInstructions)
          "owned primary raw access instruction groups differ"
      next <- case PA.accessKind access of
        PA.ScratchWrite -> do
          old <- maybe (Left "owned primary scratch write leaves its admitted frame") Right (M.lookup guest memory)
          require (before == encodeLeWord old) "owned primary scratch write preimage differs"
          provenance <- maybe (Left "owned primary scratch write lacks value provenance") Right (PA.accessScratchValue access)
          let Context _ registers _ lr cpsr _ _ = enteredContext observation
              expectedValue = case provenance of
                PA.ScratchConstant word -> Just word
                PA.EnteredRegister (A.Reg register) -> Just (registers !! fromIntegral register)
                PA.EnteredLr -> Just lr
                PA.EnteredCpsr -> Just cpsr
                PA.UnknownScratch -> Nothing
          forM_ expectedValue $ \expected ->
            require (wordValue == expected) "owned primary scratch write differs from its entered value provenance"
          pure (M.insert guest wordValue memory)
        PA.ScratchRead -> do
          old <- maybe (Left "owned primary scratch read leaves its admitted frame") Right (M.lookup guest memory)
          require (before == encodeLeWord old && wordValue == old) "owned primary scratch read differs from raw saved words"
          pure memory
        PA.ImageRead -> do
          let offset = fromIntegral (guest - 0x40000)
          expected <- leWordAt (loaderBytes contract) offset
          require (before == encodeLeWord expected && wordValue == expected) "owned primary immutable image read differs"
          pure memory
        PA.PublicationWrite -> do
          let offset = fromIntegral (guest - 0x4004dae0)
          require (before == B.take 4 (B.drop offset prestate) && leWordAt committed offset == Right wordValue)
            "owned primary publication preimage or final value differs"
          pure memory
      pure (next,actualCore1,Just (ordinal,instructions))

validateAliases :: Contract -> [Alias] -> Either String ()
validateAliases contract bindings = do
  require (length bindings == 16) "primary full-loader/workspace alias binding coverage differs"
  let expectedKeys = [(core,alias + low,low,high - low) | core <- [0,1],(low,high) <- [(0x40000,0x4dae0),(0x4dae0,0x63460)],alias <- [0,0x40000000]]
      keys aliases' = [(core,guest,physical,size) | Alias core guest physical size _ <- aliases']
      Identity _ _ loader _ _ = identity contract
  forM_ [take 8 bindings,drop 8 bindings] $ \boundary -> do
    require (sort (keys boundary) == sort expectedKeys) "primary boundary alias mappings differ"
    let codeDigests = [digest | Alias _ _ physical _ digest <- boundary,physical == 0x40000]
        workspaceDigests = [digest | Alias _ _ physical _ digest <- boundary,physical == 0x4dae0]
    require (all (== loader) codeDigests) "primary whole-loader alias identity differs"
    case workspaceDigests of
      first:rest -> require (all (== first) rest) "primary boundary workspace aliases disagree"
      [] -> Left "primary workspace alias identity is absent"
