{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}

module Main (main) where

import Control.Exception (SomeException, try)
import Control.Monad (unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation
import Emulation.Sha256 (parseSha256Hex, sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import Replacement.ModalRuntime.EntryBinding
import qualified Replacement.ModalRuntime.Qualify as Q
import qualified StockInventory as SI
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

newtype ControllerWord = ControllerWord Word32
newtype CpsrWord = CpsrWord Word32
newtype EntryCaseId = EntryCaseId String deriving (Eq, Ord)
data Route = Retained | Owned75 deriving (Eq)
data Fixture = Fixture EntryCaseId ControllerWord CpsrWord GuestAddress Route

data Context = Context
  { runner :: FilePath
  , output :: FilePath
  , kernelBase :: Word32
  , kernelText :: B.ByteString
  , stockBytes :: B.ByteString
  , entryBytes :: B.ByteString
  , seamAddress :: Word32
  , ownedAddress :: Word32
  }

stockEntry, stockResume, probeBase, exitAddress, stackBase :: Word32
stockEntry = 0x00bfb028
stockResume = stockEntry + 4
probeBase = 0x00977e00
exitAddress = 0x0097a000
stackBase = 0x00979000

fixtures :: [Fixture]
fixtures =
  [ Fixture (EntryCaseId ("fallback-" ++ Q.hex value)) (ControllerWord value)
      (CpsrWord (if even index then 0xf80a0010 else 0x10))
      (GuestAddress (fromIntegral stackBase + 0x180 + if even index then 0 else 4)) Retained
  | (index, value) <- zip [0 :: Int ..] [0, 74, 76, 173, 174, 0xffffffff, 0x0001004b, 0x8000004b]
  ] ++
  [ Fixture (EntryCaseId ("owned75-" ++ show index)) (ControllerWord 75) (CpsrWord flags)
      (GuestAddress (fromIntegral stackBase + 0x180 + if even index then 0 else 4)) Owned75
  | (index, flags) <- zip [0 :: Int ..] [0x10, 0xf80a0010, 0x90050010, 0x680a0010]
  ]

fixtureName :: Fixture -> String
fixtureName (Fixture (EntryCaseId name) _ _ _ _) = name

registers :: Fixture -> [(Register, Word32)]
registers (Fixture _ (ControllerWord controller) (CpsrWord flags) incoming _) =
  (R0, controller) : [(register, 0xa5370100 + fromIntegral index * 0x101) | (index, register) <- zip [1 :: Int ..] [R1, R2, R3, R4, R5, R6, R7, R8, R9, R10, R11, R12]]
    ++ [(Sp, fromIntegral (guestAddress incoming)), (Lr, 0x007eca44), (Cpsr, flags)]

stackBytes :: B.ByteString
stackBytes = B.pack [fromIntegral (index * 17 + 0x53) | index <- [0 .. 511 :: Int]]

branch :: Word32 -> Word32 -> Word32
branch site destination = 0xea000000 .|. (((destination - site - 8) `shiftR` 2) .&. 0x00ffffff)

probeBytes :: Fixture -> B.ByteString
probeBytes fixture@(Fixture _ _ (CpsrWord flags) incoming _) = B.concat (map Q.word32le words_)
 where
  words_ =
    [ 0xe59fd010 -- LDR sp,[pc,#16]
    , 0xe59fe010 -- LDR lr,[pc,#16]
    , 0xe59fc010 -- LDR r12,[pc,#16]
    , 0xe12cf00c -- MSR APSR_nzcvqg,r12
    , 0xe59fc00c -- LDR r12,[pc,#12]
    , branch (probeBase + 20) stockEntry
    , fromIntegral (guestAddress incoming)
    , initial Lr
    , flags
    , initial R12
    ]
  initial register = fromMaybe (error "missing probe register") (lookup register (registers fixture))

wireRegisters :: Fixture -> [(Register, Word32)]
wireRegisters fixture = map valid (registers fixture)
 where
  valid (Sp, _) = (Sp, stackBase + 0x180)
  valid (Lr, _) = (Lr, exitAddress)
  valid (Cpsr, _) = (Cpsr, 0x10)
  valid other = other

data EntryImage = StockPrefix | SeamImage B.ByteString | CallerWriteImage

observe :: Context -> Fixture -> String -> EntryImage -> IO NativeObservation
observe context fixture@(Fixture _ _ _ incoming route) suffix image = do
  let expectedSp = guestAddress incoming - if route == Retained then 36 else 0
      faultStub = probeBase + 40
      stock = case image of
        StockPrefix -> stockBytes context
        SeamImage _ -> Q.patchWord (stockBytes context) 0 (branch stockEntry (seamAddress context))
        CallerWriteImage -> Q.patchWord (stockBytes context) 0 (branch stockEntry faultStub)
      text = case image of
        SeamImage code ->
          let offset = fromIntegral (seamAddress context - kernelBase context)
           in B.take offset (kernelText context) <> code <> B.drop (offset + B.length code) (kernelText context)
        _ -> kernelText context
      (probe, functions) = case image of
        StockPrefix -> (probeBytes fixture, [])
        SeamImage _ -> (probeBytes fixture, [(seamAddress context, 56)])
        CallerWriteImage ->
          (probeBytes fixture <> Q.word32le 0xe58d0000 <> Q.word32le (branch (faultStub + 4) (seamAddress context))
          , [(seamAddress context, 56), (faultStub, 8)])
      spec = Q.CaseSpec
        { Q.caseSpecId = fixtureName fixture ++ suffix
        , Q.caseSpecEntry = probeBase
        , Q.caseSpecRegions =
            [ Q.mkRegion (RegionId "stock") stockEntry 0xfc Q.ReadExecute stock
            , Q.mkRegion (RegionId "probe") probeBase (B.length probe) Q.ReadExecute probe
            , Q.mkRegion (RegionId "exit") exitAddress 4 Q.ReadExecute (Q.word32le 0xe12fff1e)
            , Q.mkRegion (RegionId "kernel") (kernelBase context) (B.length text) Q.ReadExecute text
            , Q.mkRegion (RegionId "stack") stackBase (B.length stackBytes) Q.ReadWrite stackBytes
            ]
        , Q.caseSpecFunctions = [(probeBase, 24), (stockEntry, 4)] ++ functions
        , Q.caseSpecStatus = UnknownCall
        }
      boundary = Q.CaseBoundary (wireRegisters fixture) (GuestAddress (fromIntegral exitAddress)) (GuestAddress expectedSp)
        "X-M1 modal entry routing seam only; stop before stock+4 or the bound owned dispatcher executes"
  path <- Q.writeCaseWith boundary (output context </> (fixtureName fixture ++ suffix)) spec
  Q.loadObserved (runner context) path

stackOf :: NativeObservation -> B.ByteString
stackOf observation = fromMaybe (error "missing stack observation") (lookup (RegionId "stack") (nativeMemory observation))

data EntryShape = CanonicalEntry | CallerWriteEntry

check :: Context -> EntryShape -> Fixture -> Maybe NativeObservation -> NativeObservation -> [String]
check context shape fixture@(Fixture _ _ (CpsrWord flags) incoming route) oracle candidate =
  ["boundary status is not unknown_call" | nativeStatus candidate /= UnknownCall]
    ++ ["engine error" | nativeEngineError candidate /= EngineError 0]
    ++ ["instruction count" | nativeInstructions candidate /= InstructionCount expectedInstructions]
    ++ ["register " ++ registerLabel register | (register, value) <- expectedRegisters, lookupRegister register candidate /= Just value]
    ++ ["complete stack image" | stackOf candidate /= expectedStack]
    ++ ["caller-owned stack" | B.drop incomingOffset (stackOf candidate) /= B.drop incomingOffset stackBytes]
    ++ ["write history" | nativeProvenance candidate /= expectedWrites]
 where
  expectedInstructions = (if route == Retained then 17 else 16) + case shape of
    CanonicalEntry -> 0
    CallerWriteEntry -> 2
  incomingWord = fromIntegral (guestAddress incoming)
  incomingOffset = fromIntegral (incomingWord - stackBase)
  expectedRegisters = case (route, oracle) of
    (Retained, Just original) -> M.toList (nativeRegisters original)
    (Owned75, Nothing) -> registers fixture ++ [(Pc, ownedAddress context)]
    _ -> error "route/oracle mismatch"
  r12 = fromMaybe (error "missing initial r12") (lookup R12 (registers fixture))
  expectedStack = case oracle of
    Just original -> stackOf original
    Nothing -> Q.patchWord (Q.patchWord stackBytes (incomingOffset - 8) flags) (incomingOffset - 4) r12
  temporaryWrites = [Write (GuestAddress (guestAddress incoming - 4)) (WriteBytes 4), Write (GuestAddress (guestAddress incoming - 8)) (WriteBytes 4)]
  expectedWrites = case oracle of
    Just original -> case nativeProvenance original of
      Complete (ProvenanceTotal count) writes -> Complete (ProvenanceTotal (count + 4)) (temporaryWrites ++ writes)
      Truncated _ -> error "truncated stock write history"
    Nothing -> Complete (ProvenanceTotal 8) temporaryWrites

checkOracle :: Fixture -> NativeObservation -> [String]
checkOracle fixture@(Fixture _ _ _ incoming _) original =
  ["stock boundary status" | nativeStatus original /= UnknownCall]
    ++ ["stock engine error" | nativeEngineError original /= EngineError 0]
    ++ ["stock instruction count" | nativeInstructions original /= InstructionCount 7]
    ++ ["stock register " ++ registerLabel register | (register, value) <- expected, lookupRegister register original /= Just value]
    ++ ["stock complete stack image" | stackOf original /= savedStack]
    ++ ["stock write history" | nativeProvenance original /= Complete (ProvenanceTotal 13) writes]
 where
  saved = [fromMaybe (error "missing saved register") (lookup register (registers fixture)) | register <- [R4, R5, R6, R7, R8, R9, R10, R11, Lr]]
  bottom = guestAddress incoming - 36
  expected = [(register, if register == Sp then fromIntegral bottom else value) | (register, value) <- registers fixture] ++ [(Pc, stockResume)]
  savedStack = foldl (\bytes (index, value) -> Q.patchWord bytes (fromIntegral (bottom - fromIntegral stackBase) + 4 * index) value) stackBytes (zip [0 ..] saved)
  writes = [Write (GuestAddress (bottom + 4 * index)) (WriteBytes 4) | index <- [0 .. 8]]

data Control = FlagsRetained | FlagsOwned | ScratchRegister | CallerStack | WrongContinuation

controls :: [Control]
controls = [FlagsRetained, FlagsOwned, ScratchRegister, CallerStack, WrongContinuation]

controlName :: Control -> String
controlName FlagsRetained = "control-fallback-flags"
controlName FlagsOwned = "control-owned-flags"
controlName ScratchRegister = "control-r12"
controlName CallerStack = "control-caller-stack"
controlName WrongContinuation = "control-wrong-continuation"

controlFixture :: Control -> Fixture
controlFixture FlagsOwned = namedFixture (EntryCaseId "owned75-1")
controlFixture _ = namedFixture (EntryCaseId "fallback-0x00000000")

namedFixture :: EntryCaseId -> Fixture
namedFixture selected = case [fixture | fixture@(Fixture ident _ _ _ _) <- fixtures, ident == selected] of
  [fixture] -> fixture
  _ -> error "unique control prerequisite required"

mutated :: Context -> Control -> B.ByteString -> EntryImage
mutated _ FlagsRetained bytes = SeamImage (Q.patchWord bytes 24 0xe1a00000)
mutated _ FlagsOwned bytes = SeamImage (Q.patchWord bytes 44 0xe1a00000)
mutated _ ScratchRegister bytes = SeamImage (Q.patchWord bytes 28 0xe5bdc004)
mutated _ CallerStack _ = CallerWriteImage
mutated context WrongContinuation bytes = SeamImage (Q.patchWord bytes 36 (branch (seamAddress context + 36) (stockResume + 4)))

controlEvidence :: Control -> [String]
controlEvidence FlagsRetained = ["register cpsr"]
controlEvidence FlagsOwned = ["register cpsr"]
controlEvidence ScratchRegister = ["register r12"]
controlEvidence CallerStack = ["complete stack image", "caller-owned stack", "write history"]
controlEvidence WrongContinuation = ["register pc"]

readBound :: FilePath -> Int -> IO B.ByteString
readBound path maximumSize = do
  bytes <- B.readFile path
  unless (not (B.null bytes) && B.length bytes <= maximumSize) $ error (path ++ ": invalid size")
  pure bytes

run :: [String] -> IO Bool
run arguments = do
  options <- either error pure (parseEntryOptions arguments)
  let runnerPath = entryRunnerPath options
      id5Path = entryId5Path options
      kernelPath = entryKernelPath options
      outputPath = entryOutputPath options
  exists <- doesPathExist outputPath
  when exists $ error "output path already exists"
  backend <- readBound runnerPath (64 * 1024 * 1024)
  runnerDigest <- either error pure (parseSha256Hex "b51544c62053d1bf31816d2c3e4b439e3847296b2eef1f99c8f17b5d75da2860")
  unless (sha256 backend == runnerDigest) $ error "runner differs from E-NATIVE-APSR-MSR-150"
  id5 <- readBound id5Path 5185120
  id5Digest <- either error pure (parseSha256Hex "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
  unless (B.length id5 == 5185120 && sha256 id5 == id5Digest) $ error "id5 identity mismatch"
  derived <- either error pure (SI.deriveFunction SI.xm1TaskRecords (SI.GuestAddress 0x00bf1a00) id5 (SI.FunctionWindow (SI.GuestAddress stockEntry) (SI.GuestAddress 0x00bfb124)) [])
  either error pure (SI.checkDeclarations [SI.FunctionDeclaration (SI.GuestAddress stockEntry) (SI.FunctionSize 0xfc)] [SI.derivedDeclaration derived])
  kernel <- readBound kernelPath (2 * 1024 * 1024)
  binding <- either error pure (bindEntry (entryKernelDigest options) kernel)
  let textBase = entryAddressWord (entryTextAddress binding)
      text = entryText binding
      mechanism = entrySeamBytes binding
      mechanismBase = entryAddressWord (entrySeamAddress binding)
      owned = entryAddressWord (entryOwnedAddress binding)
  let stockOffset = fromIntegral (stockEntry - 0x00bf1a00)
      context = Context runnerPath outputPath textBase text (B.take 0xfc (B.drop stockOffset id5)) mechanism mechanismBase owned
  createDirectoryIfMissing True outputPath
  let artifact path bytes = object ["path" .= path, "sha256" .= sha256Hex (sha256 bytes)]
  B.writeFile (outputPath </> "inputs.json") $ encodeCanonical $ object
    [ "runner" .= artifact runnerPath backend, "id5" .= artifact id5Path id5, "kernel" .= artifact kernelPath kernel
    , "kernel_elf_sha256" .= sha256Hex (entryKernelDigest options)
    , "kernel_text" .= object ["address" .= Q.hex textBase, "bytes" .= B.length text, "sha256" .= sha256Hex (sha256 text)]
    , "entry" .= object ["symbol" .= ("rb_modal_entry" :: String), "address" .= Q.hex mechanismBase, "bytes" .= B.length mechanism, "sha256" .= sha256Hex (sha256 mechanism)]
    , "owned" .= object ["symbol" .= ("rb_modal_dispatch" :: String), "address" .= Q.hex owned, "bytes" .= entryExtentBytes (entryOwnedSize binding)]
    , "stock_resume" .= Q.hex stockResume
    , "global_admission" .= False
    , "limitations" .= (["routing seam only; owned callbacks and retained continuation do not execute", "extra transient stack stores require asynchronous access and alias exclusions", "no natural reset, interrupt, device or cancellation route"] :: [String])
    ]
  fixtureResults <- runFixtures context fixtures [] M.empty
  case fixtureResults of
    (False, reports, _) -> writeReport outputPath False reports [] >> pure False
    (True, reports, oracles) -> do
      controlResults <- runControls context oracles controls []
      let (ok, controlReports) = controlResults
      writeReport outputPath ok reports controlReports
      pure ok
 where
  runFixtures _ [] reports oracles = pure (True, reverse reports, oracles)
  runFixtures context (fixture@(Fixture ident _ _ _ route) : rest) reports oracles = do
    result <- try $ do
      original <- if route == Retained then Just <$> observe context fixture "-stock" StockPrefix else pure Nothing
      case original of
        Just observation -> unless (null (checkOracle fixture observation)) $ error (show (checkOracle fixture observation))
        Nothing -> pure ()
      candidate <- observe context fixture "-entry" (SeamImage (entryBytes context))
      pure (original, check context CanonicalEntry fixture original candidate)
    let (ok, problems, original) = case result of
          Left (exception :: SomeException) -> (False, ["exception: " ++ show exception], Nothing)
          Right (observed, mismatches) -> (null mismatches, mismatches, observed)
        report = object ["id" .= fixtureName fixture, "ok" .= ok, "mismatches" .= problems]
        accumulated = report : reports
    writeReport (output context) False (reverse accumulated) []
    if ok then runFixtures context rest accumulated (M.insert ident original oracles)
      else pure (False, reverse accumulated, oracles)
  runControls _ _ [] reports = pure (True, reverse reports)
  runControls context oracles (control : rest) reports = do
    let fixture@(Fixture ident _ _ _ _) = controlFixture control
        original = fromMaybe (error "missing control prerequisite") (M.lookup ident oracles)
    result <- try $ do
      candidate <- observe context fixture ("-" ++ controlName control) (mutated context control (entryBytes context))
      let shape = case control of
            CallerStack -> CallerWriteEntry
            _ -> CanonicalEntry
          problems = check context shape fixture original candidate
      pure (problems == controlEvidence control, problems)
    let (ok, problems) = case result of
          Left (exception :: SomeException) -> (False, ["exception: " ++ show exception])
          Right observed -> observed
        report = object ["id" .= controlName control, "refused_as_expected" .= ok, "mismatches" .= problems]
        accumulated = report : reports
    if ok then runControls context oracles rest accumulated else pure (False, reverse accumulated)

writeReport :: FilePath -> Bool -> [Value] -> [Value] -> IO ()
writeReport directory ok cases mutations = B.writeFile (directory </> "entry-results.json") $ encodeCanonical $ object
  [ "qualified" .= ok, "global_admission" .= False, "scope" .= ("56-byte modal entry boundary, not dispatcher callback/frame equivalence" :: String)
  , "cases" .= cases, "controls" .= mutations
  , "complete_matrix" .= (length cases == length fixtures && length mutations == length controls)
  ]

main :: IO ()
main = do
  ok <- getArgs >>= run
  unless ok exitFailure
