{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified ArmElf as E
import Data.Aeson (Value (..))
import qualified Data.Aeson.KeyMap as KM
import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Emulation.NativeCase (Region, loadCase, runnerDigest, judgeRunner)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.ParallelCases (mapBounded)
import FujiAF.Host (get, jsonRead, str)
import FujiAF.JSON (encodeCanonical)
import System.Directory (canonicalizePath)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory)

pin :: String
pin = "x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; id5 SHA256 3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a; publisher ee2adc and setters ee1c4c-ee2adc checked against X-M1 instructions"

address :: Word32 -> GuestAddress
address = GuestAddress . fromIntegral

mov :: Word32 -> Word32 -> [Word32]
mov register value = [0xe3000000 .|. ((value .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (value .&. 0xfff), 0xe3400000 .|. (((value `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff)]

callbacks :: [(Word32, B.ByteString)]
callbacks = [(0x74213c, assemble (mov 12 0x210018 ++ [0xe58c2000,0xe59c3004,0xe2833001,0xe58c3004,0xe3520006,0x112fff1e,0xe591c000,0xe580c000,0xe1d1c0b4,0xe1c0c0b4,0xe12fff1e]))]
  ++ [(entry, assemble (mov 12 (0x210000 + slot) ++ [0xe58c0000] ++ (if entry == 0xee5ec0 then [0xe58c1004] else []) ++ [0xe12fff1e])) | (entry,slot) <- [(0xee5ec0,0),(0xeea6a8,8),(0xeea6f8,16)]]
  ++ [(0xee418c,assemble (journal 1)),(0xee364c,assemble (mov 0 0x16995e0 ++ [0xe5900000] ++ mov 1 0x1698a37 ++ [0xe5d11000] ++ mov 2 0 ++ journal 2))]
  where
    journal ident = mov 12 0x210020 ++ [0xe59c3000,0xe2833001,0xe58c3000,0xe08cc203,0xe58c0000,0xe58c1004,0xe58c2008] ++ mov 3 ident ++ [0xe58c300c,0xe12fff1e]

data PublicationEntry = BarePublisher | LockedService deriving (Eq,Show)
data PublicationScope = OneFilm | AllFields | Selected ParameterCase deriving (Eq,Show)
newtype FieldOffset = FieldOffset Int deriving (Eq,Show)
data AuxDelta = FirstOnly | SecondOnly | BothValues | EqualValues | RememberedMismatch deriving (Eq,Show,Enum,Bounded)
data AuxMode = ModeZero | ModeOne | ModeOther deriving (Eq,Show,Enum,Bounded)
data LaneSelection = NoLane | FirstLane | SecondLane | BothLanes deriving (Eq,Show,Enum,Bounded)
data AuxGates = NoAuxGate | FirstAuxGate | SecondAuxGate | BothAuxGates deriving (Eq,Show,Enum,Bounded)
data AuxiliaryCase = AuxiliaryCase AuxDelta AuxMode LaneSelection AuxGates deriving (Eq,Show)
data ParameterCase = Equality | Field FieldOffset | Aliased | OneShotRequest | Auxiliary AuxiliaryCase deriving (Eq,Show)

caseKey :: ParameterCase -> String
caseKey Equality = "equality"
caseKey (Field (FieldOffset field)) = "field-"++show field
caseKey Aliased = "alias"
caseKey OneShotRequest = "request"
caseKey (Auxiliary (AuxiliaryCase delta mode lanes gates)) =
  "aux-"++deltaLabel delta++"-"++modeLabel mode++"-"++laneLabel lanes++"-"++gateLabel gates
  where
    deltaLabel FirstOnly = "first"
    deltaLabel SecondOnly = "second"
    deltaLabel BothValues = "both"
    deltaLabel EqualValues = "equal"
    deltaLabel RememberedMismatch = "deferred"
    modeLabel ModeZero = "m0"
    modeLabel ModeOne = "m1"
    modeLabel ModeOther = "m2"
    laneLabel NoLane = "l0"
    laneLabel FirstLane = "l1"
    laneLabel SecondLane = "l2"
    laneLabel BothLanes = "l3"
    gateLabel NoAuxGate = "g0"
    gateLabel FirstAuxGate = "g1"
    gateLabel SecondAuxGate = "g2"
    gateLabel BothAuxGates = "g3"

selectedCase :: String -> IO ParameterCase
selectedCase name = case name of
  "equality" -> pure Equality
  "film" -> pure (Field (FieldOffset 0x5c))
  "alias" -> pure Aliased
  "request" -> pure OneShotRequest
  "white-balance" -> pure (Field (FieldOffset 0x3c))
  "aux-first" -> pure (aux FirstOnly ModeZero)
  "aux-second" -> pure (aux SecondOnly ModeZero)
  "aux-both" -> pure (aux BothValues ModeZero)
  "aux-equal" -> pure (aux EqualValues ModeZero)
  "aux-deferred" -> pure (aux RememberedMismatch ModeOne)
  _ -> fail "unknown publication semantic case"
  where aux delta mode = Auxiliary (AuxiliaryCase delta mode BothLanes BothAuxGates)

stockRanges :: [(Word32,Word32)]
stockRanges = zip starts (zipWith (-) (drop 1 starts ++ [0xee2c0c]) starts)
  where starts = [0xee1c4c,0xee1d94,0xee1dc8,0xee1e24,0xee1e80,0xee1edc,0xee1f58,0xee1ffc,0xee20a0,0xee2148,0xee21a4,0xee2200,0xee225c,0xee2290,0xee22c4,0xee2320,0xee237c,0xee23b0,0xee240c,0xee2468,0xee24c4,0xee2524,0xee26ac,0xee2708,0xee273c,0xee2794,0xee27f0,0xee284c,0xee28a8,0xee2904,0xee2960,0xee29bc,0xee2a18,0xee2a74,0xee2aa8,0xee2adc]

poke :: Int -> B.ByteString -> B.ByteString -> B.ByteString
poke offset value bytes = B.take offset bytes <> value <> B.drop (offset+B.length value) bytes

source :: E.Elf -> Either String (Word32, [CaseRegion], [(GuestAddress,Word32)])
source elf = do
  entry <- case [s | s <- E.elfSymbols elf, E.symbolName s == "xm1_liveview_publish", E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [s] -> Right (E.symbolValue s)
    _ -> Left "publisher ELF function is missing or ambiguous"
  let allocated = [s | s <- E.elfSections elf, E.sectionAllocated s, E.sectionSize s > 0]
  unless (not (null allocated) && all (\s -> not (E.sectionWritable s) && E.sectionType s == 1 && E.sectionAddress s >= 0x2000000 && toInteger (E.sectionAddress s) + toInteger (E.sectionSize s) <= 0x2010000) allocated) (Left "ELF escapes read-only 64 KiB artifact")
  regions <- forM (zip [0::Int ..] allocated) $ \(i,s) -> do
    bytes <- forM [E.sectionAddress s .. E.sectionAddress s + E.sectionSize s - 1] $ \a -> maybe (Left "allocated ELF byte is missing") Right (E.byteAt elf a)
    pure (mkRegion (RegionId ("owned-"++show i)) (address (E.sectionAddress s)) (fromIntegral (E.sectionSize s)) (if E.sectionExecutable s then ReadExecute else ReadOnly) (B.pack bytes))
  let functions = M.toAscList (M.fromListWith max [(address (E.symbolValue s),E.symbolSize s) | s <- E.elfSymbols elf, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0, E.symbolValue s >= 0x2000000, E.symbolValue s < 0x2010000])
  pure (entry,regions,functions)

specification :: String -> Word32 -> [CaseRegion] -> [(GuestAddress,Word32)] -> ParameterCase -> Bool -> CaseSpec
specification role entry code functions parameter gate = CaseSpec
  { caseId = role++"-"++caseKey parameter++(if gate then "-gate-on" else "-gate-off"), caseProvenance = pin++"; synthetic parameter bytes and deterministic retained-service receipt boundaries", caseExclusion = "No sensor, flash, display, lock or develop device law is modeled. Retained WB service calls are receipts, not numerical qualification.", caseEntry = address entry, caseExit = address 0x220000
  , caseRegions = code ++ [mkRegion (RegionId "state") (address 0x1698a00) 0x1400 ReadWrite state, mkRegion (RegionId "input") (address 0x200000) 200 ReadOnly input, mkRegion (RegionId "receipt") (address 0x210000) 128 ReadWrite (B.replicate 128 0), mkRegion (RegionId "stack") (address 0x300000) 0x1000 ReadWrite (B.replicate 0x1000 0), mkRegion (RegionId "exit") (address 0x220000) 4 ReadExecute (word32le 0xe12fff1e)] ++ [mkRegion (RegionId ("service-"++show a)) (address a) (B.length bytes) ReadExecute bytes | (a,bytes) <- callbacks]
  , caseFunctions = functions ++ [(address a,fromIntegral (B.length bytes)) | (a,bytes) <- callbacks]
  , caseInitial = [(R0,if parameter == Aliased then 0x1699580 else 0x200000),(Sp,0x300800),(Lr,0x220000),(Cpsr,0x13)] ++ [(r,0xa5000000+fromIntegral (fromEnum r)) | r <- [R1,R2,R3,R4,R5,R6,R7,R8,R9,R10,R11,R12]]
  , caseExpectedStatus = Returned, caseExpectedRegisters = [(Sp,0x300800)] ++ [(r,0xa5000000+fromIntegral (fromEnum r)) | r <- [R4,R5,R6,R7,R8,R9,R10,R11]], caseExpectedMemory = [], caseCp14Identity = Nothing, caseKernel = Just (KernelCase plainLimit Nothing Nothing Nothing Nothing [] [])
  , caseIgnoredMemory = []
  }
  where
    putWord offset value = poke offset (word32le value)
    input = case parameter of
      Field (FieldOffset field) -> poke field (B.singleton (if field == 0x5c then 12 else 0x81)) (B.replicate 200 0)
      OneShotRequest -> putWord 0x8c 0x81 (B.replicate 200 0)
      Auxiliary (AuxiliaryCase delta _ _ _) -> putWord 0x8c 1 $ putWord 0x90 (if delta `elem` [FirstOnly,BothValues] then 49 else 9) $ putWord 0x94 (if delta `elem` [SecondOnly,BothValues] then 83 else 11) (B.replicate 200 0)
      _ -> B.replicate 200 0
    baseState = poke 0x40 (B.singleton (if gate then 128 else 0)) $ poke 0xc4c (B.concat (replicate 4 (word32le 1))) $ foldr (\offset bytes -> putWord offset 1 bytes) (B.replicate 0x1400 0) [0x150,0x350,0x550,0x708]
    state = case parameter of
      Aliased -> poke 0xb80 (B.concat (map word32le [1,2,3,4])) baseState
      Auxiliary (AuxiliaryCase _ mode lanes gates) -> putWord 0xc88 (fromIntegral (fromEnum mode)) $ putWord 0xc8c (if mode == ModeZero then 9 else 7) $ putWord 0xc14 9 $ putWord 0xc18 11 $ poke 0x41 (B.singleton (fromIntegral (fromEnum gates))) $ putWord 0xc54 (if lanes `elem` [FirstLane,BothLanes] then 1 else 0) $ putWord 0xc58 (if lanes `elem` [SecondLane,BothLanes] then 1 else 0) baseState
      _ -> baseState

orderedBytes :: NativeObservation -> Maybe [(GuestAddress,Word8)]
orderedBytes observation = fmap (concatMap bytes) (nativeKernel observation >>= observedOrder)
  where
    bytes (OrderedWrite write (WriteValue value))
      | let a = guestAddress (writeAddress write), (a>=0x1698a00 && a<0x1699e00) || (a>=0x210000 && a<0x210080) =
          [(GuestAddress (guestAddress (writeAddress write)+offset),fromIntegral (value `shiftR` (fromIntegral offset*8))) | offset <- take (fromIntegral (writeBytes (writeLength write))) [0..]]
    bytes _ = []

comparable :: NativeObservation -> (RunnerStatus, [(RegionId,B.ByteString)], [(Register,Maybe Word32)], Maybe [(GuestAddress,Word8)])
comparable observation = (nativeStatus observation, [(ident,bytes) | (ident,bytes) <- nativeMemory observation, ident `elem` [RegionId "state",RegionId "receipt"]], [(r,lookupRegister r observation) | r <- [R4,R5,R6,R7,R8,R9,R10,R11,Sp]], orderedBytes observation)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["cache",runner,imagePath,elfPath,out,scenario] -> runCache Execute runner imagePath elfPath out scenario
    ["cache-stage",runner,imagePath,elfPath,out,scenario] -> runCache Stage runner imagePath elfPath out scenario
    ["cache-execute",runner,stockPath,ownedPath] -> executeCache runner stockPath ownedPath
    ["publisher-rejudge",runner,imagePath,elfPath,stockPath,ownedPath] -> do
      entry <- admitPublisher imagePath elfPath stockPath ownedPath
      SavedObservation _ _ stock <- readSaved runner stockPath
      SavedObservation _ _ owned <- readSaved runner ownedPath
      comparePublication entry stock owned
      putStrLn "publisher receipts rejudged without execution"
    ["publisher-resume",runner,imagePath,elfPath,stockPath,ownedPath] -> resumePublisher runner imagePath elfPath stockPath ownedPath
    ["publisher-control",kind,runner,imagePath,elfPath,out,stockPath] -> do control <- parseControl kind; runControl control runner imagePath elfPath out stockPath
    ["publisher-case",runner,imagePath,elfPath,out,name] -> do parameter <- selectedCase name; runPublisher (Selected parameter) runner imagePath elfPath out
    ["publisher-one",runner,imagePath,elfPath,out] -> runPublisher OneFilm runner imagePath elfPath out
    [runner,imagePath,elfPath,out] -> runPublisher AllFields runner imagePath elfPath out
    _ -> fail "usage: native-suite [publisher-one] RUNNER IMAGE5 ELF OUTPUT | native-suite cache RUNNER IMAGE4 ELF OUTPUT SCENARIO"

runPublisher :: PublicationScope -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runPublisher scope runner imagePath elfPath out = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  (ownedEntry,ownedCode,ownedFunctions) <- either fail pure (source elf)
  ownedService <- case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == "xm1_liveview_service", E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [entry] -> pure entry
    _ -> fail "service ELF function is missing or ambiguous"
  let original = B.take (0xee2c0c-0xee1c4c) (B.drop (0xee1c4c-0xbf1a00) image)
      serviceCode = B.take 0x40 (B.drop (0xed8b6c-0xbf1a00) image)
      stockCode = [mkRegion (RegionId "stock") (address 0xee1c4c) (B.length original) ReadExecute original,mkRegion (RegionId "stock-service") (address 0xed8b6c) 0x40 ReadExecute serviceCode]
      fields = [0,4,8,0xc,0x10,0xc4,0x14,0x18,0x1c,0x20,0x2c,0x30,0x34,0x38,0x3c,0x40,0x44,0x74,0x48,0x4c,0x50,0x5c,0x60,0x64,0x68,0x6c,0x70,0x78,0x8c,0x98,0x9c,0x54,0xa0,0xa2,0xa4,0xa8,0xac,0xb0,0xbc,0xb4,0xb8,0xc0]
      auxiliaries = nub ([Auxiliary (AuxiliaryCase delta mode BothLanes gates) | delta <- [minBound..maxBound],mode <- [minBound..maxBound],gates <- [minBound..maxBound]] ++ [Auxiliary (AuxiliaryCase FirstOnly ModeZero lanes BothAuxGates) | lanes <- [minBound..maxBound]])
      parameters = case scope of OneFilm -> [Field (FieldOffset 0x5c)]; Selected parameter -> [parameter]; AllFields -> Equality:Aliased:OneShotRequest:map (Field . FieldOffset) fields++auxiliaries
      cases = [(parameter,gate,entry) | parameter <- parameters,gate <- [False,True],entry <- [BarePublisher,LockedService],scope /= OneFilm || (gate && entry == LockedService)]
  _ <- mapBounded 4 (\(parameter,gate,entry) -> do
    let stockSpec = specification "stock" (if entry == LockedService then 0xed8b6c else 0xee2adc) stockCode ((address 0xed8b6c,0x40):[(address a,n) | (a,n) <- stockRanges]) parameter gate
        ownedSpec = specification "owned" (if entry == LockedService then ownedService else ownedEntry) ownedCode ownedFunctions parameter gate
        directory = out </> (caseKey parameter++"-"++show gate++"-"++show entry)
    stockPath <- writeCase (directory </> "stock") stockSpec
    ownedPath <- writeCase (directory </> "owned") ownedSpec
    stock <- loadObserved runner stockPath
    owned <- loadObserved runner ownedPath
    comparePublication entry stock owned) cases
  putStrLn "publisher differential complete"

newtype PrivateFrameBytes = PrivateFrameBytes Int

privateFrame :: PrivateFrameBytes
-- Linked returning paths: stock bare 32, stock service 48, owned roots at most 200 bytes.
privateFrame = PrivateFrameBytes 208

callerStackPreserved :: NativeObservation -> Bool
callerStackPreserved observation = case lookup (RegionId "stack") (nativeMemory observation) of
  Just bytes | B.length bytes == 0x1000 ->
    let PrivateFrameBytes allowance = privateFrame
        lower = 0x800-allowance
        allowed write = let start=guestAddress (writeAddress write); end=start+fromIntegral (writeBytes (writeLength write))
                        in start>=0x301000 || end<=0x300000 || (start>=0x300000+fromIntegral lower && end<=0x300800)
    in B.all (==0) (B.take lower bytes) && B.all (==0) (B.drop 0x800 bytes) && maybe True (all allowed) (provenanceWrites (nativeProvenance observation))
  _ -> False

comparePublication :: PublicationEntry -> NativeObservation -> NativeObservation -> IO ()
comparePublication entry stock owned = do
  unless (callerStackPreserved stock && callerStackPreserved owned) (fail "publication caller stack changed")
  unless (nativeStatus stock == Returned && orderedBytes stock /= Nothing && orderedBytes owned /= Nothing && comparable stock == comparable owned) (fail "native publication difference")
  forM_ [stock,owned] $ \observation -> case lookup (RegionId "receipt") (nativeMemory observation) of
    Just receipt | readWordAt receipt 28 == 0 || readWordAt receipt 24 == 6 -> pure ()
    _ -> fail "white-balance copy length is not six"
  forM_ [R4,R5,R6,R7,R8,R9,R10,R11,Sp] $ \r -> unless (M.lookup r (nativeRegisters owned) == M.lookup r (nativeRegisters stock)) (fail "ABI mismatch")
  unless (entry /= LockedService || (lookupRegister R0 stock == Just 0 && lookupRegister R0 owned == Just 0)) (fail "service return value differs")
  unless (fmap (.&. 0x1ff) (lookupRegister Cpsr stock) == fmap (.&. 0x1ff) (lookupRegister Cpsr owned)) (fail "CPSR control differs")

data SavedObservation = SavedObservation Value [Region] NativeObservation

readSaved :: FilePath -> FilePath -> IO SavedObservation
readSaved runner path = do
  (spec,regions,_,identity) <- loadCase path
  report <- jsonRead =<< B.readFile (takeDirectory path </> "native-result.json")
  forM_ identity $ \(key,value) -> unless (get report key == value) (fail ("saved receipt identity differs: "++key))
  digest <- runnerDigest =<< canonicalizePath runner
  unless (str (get report "runner_sha256") == digest) (fail "saved receipt runner identity differs")
  exit <- either (fail . renderObservationError) pure (reportRunnerExit report)
  (_,rejudged) <- judgeRunner spec regions report exit (encodeCanonical (get report "runner")) B.empty
  unless (str (get rejudged "status") /= "runner_failure") (fail "saved receipt runner protocol validation failed")
  observation <- either (fail . renderObservationError) pure (observeNative exit regions (get rejudged "runner"))
  pure (SavedObservation spec regions observation)

admitPublisher :: FilePath -> FilePath -> FilePath -> FilePath -> IO PublicationEntry
admitPublisher imagePath elfPath stockPath ownedPath = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  (ownedEntry,ownedCode,ownedFunctions) <- either fail pure (source elf)
  ownedService <- case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == "xm1_liveview_service", E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [entry] -> pure entry
    _ -> fail "service ELF function is missing or ambiguous"
  (stockSpec,_,_,_) <- loadCase stockPath
  entry <- case str (get stockSpec "entry") of
    "0x00ee2adc" -> pure BarePublisher
    "0x00ed8b6c" -> pure LockedService
    _ -> fail "unknown saved publication entry"
  let fields = [0,4,8,0xc,0x10,0xc4,0x14,0x18,0x1c,0x20,0x2c,0x30,0x34,0x38,0x3c,0x40,0x44,0x74,0x48,0x4c,0x50,0x5c,0x60,0x64,0x68,0x6c,0x70,0x78,0x8c,0x98,0x9c,0x54,0xa0,0xa2,0xa4,0xa8,0xac,0xb0,0xbc,0xb4,0xb8,0xc0]
      parameters = Equality:Aliased:OneShotRequest:map (Field . FieldOffset) fields ++ [Auxiliary (AuxiliaryCase delta mode lanes gates) | delta <- [minBound..maxBound], mode <- [minBound..maxBound], lanes <- [minBound..maxBound], gates <- [minBound..maxBound]]
      stockCode = [mkRegion (RegionId "stock") (address 0xee1c4c) 0xfc0 ReadExecute (B.take 0xfc0 (B.drop (0xee1c4c-0xbf1a00) image)),mkRegion (RegionId "stock-service") (address 0xed8b6c) 0x40 ReadExecute (B.take 0x40 (B.drop (0xed8b6c-0xbf1a00) image))]
      stockFunctions = (address 0xed8b6c,0x40):[(address a,n) | (a,n) <- stockRanges]
      candidates = [(parameter,gate) | parameter <- parameters, gate <- [False,True], caseId (specification "stock" 0 stockCode stockFunctions parameter gate) == str (get stockSpec "id")]
  (parameter,gate) <- case candidates of
    [scenario] -> pure scenario
    _ -> fail "unknown saved publication scenario or role"
  let expectedStock = specification "stock" (if entry == LockedService then 0xed8b6c else 0xee2adc) stockCode stockFunctions parameter gate
      expectedOwned = specification "owned" (if entry == LockedService then ownedService else ownedEntry) ownedCode ownedFunctions parameter gate
  forM_ [("stock",stockPath,expectedStock),("owned",ownedPath,expectedOwned)] $ \(role,path,expected) -> do
    expectedPath <- writeCase (takeDirectory ownedPath </> "admission" </> role) expected
    (actual,actualRegions,_,_) <- loadCase path
    (wanted,wantedRegions,_,_) <- loadCase expectedPath
    let contract (Object value) = Object (KM.delete "artifacts" value)
        contract value = value
    unless (contract actual == contract wanted && regionBounds actualRegions == regionBounds wantedRegions && regionInitialBytes actualRegions == regionInitialBytes wantedRegions) (fail ("publication "++role++" source contract differs"))
  pure entry

resumePublisher :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
resumePublisher runner imagePath elfPath stockPath ownedPath = do
  entry <- admitPublisher imagePath elfPath stockPath ownedPath
  SavedObservation _ _ stock <- readSaved runner stockPath
  owned <- loadObserved runner ownedPath
  comparePublication entry stock owned
  putStrLn "publisher pair complete; stock receipt reused and rejudged"

data Control = WrongField | WrongDirty | Reordered | WrongRequest | AliasedRead | CopyZero | CopyFive | CallerCorruption deriving (Eq,Show)

parseControl :: String -> IO Control
parseControl name = case name of
  "wrong-field" -> pure WrongField
  "wrong-dirty" -> pure WrongDirty
  "ordering" -> pure Reordered
  "request" -> pure WrongRequest
  "alias" -> pure AliasedRead
  "copy-zero" -> pure CopyZero
  "copy-five" -> pure CopyFive
  "caller-stack" -> pure CallerCorruption
  _ -> fail "unknown publication control"

runControl :: Control -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runControl control runner imagePath elfPath out stockPath = do
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "control image5 identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  (_,code,functions) <- either fail pure (source elf)
  service <- case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == "xm1_liveview_service", E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [entry] -> pure entry
    _ -> fail "control service function is missing or ambiguous"
  let parameter = case control of WrongRequest -> OneShotRequest; AliasedRead -> Aliased; CopyZero -> Field (FieldOffset 0x3c); CopyFive -> Field (FieldOffset 0x3c); _ -> Field (FieldOffset 0x5c)
      original = B.take 0xfc0 (B.drop (0xee1c4c-0xbf1a00) image)
      originalService = B.take 0x40 (B.drop (0xed8b6c-0xbf1a00) image)
      stockCode = [mkRegion (RegionId "stock") (address 0xee1c4c) 0xfc0 ReadExecute original,mkRegion (RegionId "stock-service") (address 0xed8b6c) 0x40 ReadExecute originalService]
      expected = specification "stock" 0xed8b6c stockCode ((address 0xed8b6c,0x40):[(address a,n) | (a,n) <- stockRanges]) parameter True
  SavedObservation spec regions stock <- readSaved runner stockPath
  unless (str (get spec "id") == caseId expected && str (get spec "entry") == "0x00ed8b6c" && str (get spec "provenance") == caseProvenance expected) (fail "control stock case contract differs")
  forM_ (caseInitial expected) $ \(register,value) -> unless (parseWord32 (str (get (get spec "registers") (registerLabel register))) == Just value) (fail "control stock initial registers differ")
  forM_ (caseRegions expected) $ \item -> case [r | r <- regions, regionIdOf r == regionIdOf (caseRegion item)] of
    [r] | regionBounds [r] == regionBounds [caseRegion item], regionInitialBytes [r] == regionInitialBytes [caseRegion item] -> pure ()
    _ -> fail "control stock wire inputs differ from the current model"
  path <- writeCase out (specification "owned" service code functions parameter True)
  owned <- loadObserved runner path
  unless (nativeStatus stock == Returned && nativeStatus owned == Returned && orderedBytes stock /= Nothing && orderedBytes owned /= Nothing) (fail "control did not reach both publication returns with complete write values")
  verdict <- try (comparePublication LockedService stock owned) :: IO (Either IOException ())
  unless (either (const True) (const False) verdict) (fail "publication control was accepted")
  let withoutOrder observation = let (status,memory,registers,_) = comparable observation in (status,memory,registers)
      addresses observation = fmap (map fst) (orderedBytes observation)
  whenControl (control == WrongRequest) $ unless (withoutOrder stock == withoutOrder owned && addresses stock == addresses owned) (fail "request control differs beyond intermediate stored values")
  whenControl (control == Reordered) $ unless (withoutOrder stock == withoutOrder owned && fmap sort (orderedBytes stock) == fmap sort (orderedBytes owned)) (fail "ordering control changes more than ordering")
  whenControl (control == CallerCorruption) $ case lookup (RegionId "stack") (nativeMemory owned) of
    Just bytes | callerStackPreserved stock && not (callerStackPreserved owned) && readWordAt bytes 0x804 == 0xdeadbeef && withoutOrder stock == withoutOrder owned -> pure ()
    _ -> fail "caller control did not preserve the semantic result while corrupting entry-SP+4"
  whenControl (control `elem` [CopyZero,CopyFive]) $ case lookup (RegionId "receipt") (nativeMemory owned) of
    Just bytes | readWordAt bytes 28 == 1 && readWordAt bytes 24 == (if control == CopyZero then 0 else 5) -> pure ()
    _ -> fail "copy-length control did not exercise its declared argument"
  putStrLn ("publication control refused: "++show control++"; compatible stock receipt reused")
  where whenControl condition action = if condition then action else pure ()

data CacheScenario = Primary | Fallback | DefaultTable | Reload Word32 | Accessors deriving (Eq,Show)

parseCacheScenario :: String -> IO CacheScenario
parseCacheScenario value = case value of
  "primary" -> pure Primary
  "fallback" -> pure Fallback
  "defaults" -> pure DefaultTable
  "reload0" -> pure (Reload 0)
  "reload31" -> pure (Reload 31)
  "reload32" -> pure (Reload 32)
  "reload255" -> pure (Reload 255)
  "reload-max" -> pure (Reload 0xffffffff)
  "accessors" -> pure Accessors
  _ -> fail "cache scenario must be primary, fallback, defaults, reload0, reload31, reload32, reload255 or reload-max"

cacheCallbacks :: CacheScenario -> [(Word32,B.ByteString)]
cacheCallbacks scenario =
  [ (0x6a9e98, service 1 (signature (scenario == Primary)) False)
  , (0x6a9c70, service 2 (mov 3 0x69 ++ [0xe5c03000]) True)
  , (0x6a9ac4, service 3 (case scenario of Reload _ -> mov 3 0x46 ++ [0xe5c03000]; _ -> signature (scenario /= DefaultTable)) True)
  ]
  where
    signature valid = mov 12 0x41acda02 ++ mov 3 (if valid then 0x55 else 0) ++ [0xe5cc3000] ++ mov 3 (if valid then 0xaa else 0) ++ [0xe5cc3001]
    service ident effects hasThird = assemble $
      mov 12 0x210000 ++ [0xe59c3000,0xe2833001,0xe58c3000,0xe08cc203,0xe58c0000,0xe58c1004] ++
      (if hasThird then [0xe58c2008] else []) ++ mov 3 ident ++ [0xe58c300c] ++ effects ++ [0xe3e00054,0xe12fff1e]

data CacheAction = Stage | Execute deriving (Eq,Show)

runCache :: CacheAction -> FilePath -> FilePath -> FilePath -> FilePath -> String -> IO ()
runCache action runner imagePath elfPath out scenarioText = do
  scenario <- parseCacheScenario scenarioText
  image <- B.readFile imagePath
  unless (sha256Hex (sha256 image) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 identity differs")
  let slice base count = B.take count (B.drop (base-0x61c400) image)
      defaults = slice 0x7c8a44 504
  unless (sha256Hex (sha256 defaults) == "8179e82dd07c45460f20ca51150e4b14d119dd27878690454a3f0cb8c5b263b1") (fail "defaults resource identity differs")
  elf <- either fail pure . E.parseElf =<< B.readFile elfPath
  (_,ownedCode,ownedFunctions) <- either fail pure (source elf)
  let symbolName = case scenario of Reload _ -> "xm1_calibration_reload"; _ -> "xm1_calibration_initialize"
  ownedEntry <- case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == symbolName, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
    [value] -> pure value
    _ -> fail "cache entry is missing or ambiguous"
  let retained = [(0x69f144,0x1c8),(0x69f8b8,0x68),(0x69fc94,0x18c),(0x69fe20,0x88),(0x69fec8,0x160),(0x7221a0,0x78)]
      retainedFunctions = [(0x69f144,0xc0),(0x69f204,0x108),(0x69f8b8,0x2c),(0x69f8e4,0x18),(0x69f8fc,0x24),(0x69fc94,0x24),(0x69fcb8,0x20),(0x69fcd8,0x24),(0x69fcfc,0x124),(0x69fe20,0x88),(0x69fec8,0x28),(0x69fef0,0x34),(0x69ff24,0x44),(0x69ff68,0x14),(0x69ff7c,0x28),(0x69ffa4,0x44),(0x69ffe8,0x14),(0x69fffc,0x14),(0x6a0010,0x18),(0x7221a0,0x28),(0x7221c8,0x50)]
      stockCode = [mkRegion (RegionId ("stock-"++show base)) (address base) count ReadExecute (slice (fromIntegral base) count) | (base,count) <- retained]
      callbackCode = [mkRegion (RegionId ("storage-"++show base)) (address base) (B.length bytes) ReadExecute bytes | (base,bytes) <- cacheCallbacks scenario]
      initialPointer = word32le 0x41acd800
      initialCache = poke 0 (B.pack [0x80,0xfe,0xab,0xcd,0x55,0xff,2,1]) (B.replicate 0x420000 0xa5)
      ownedSymbol name = case [E.symbolValue s | s <- E.elfSymbols elf, E.symbolName s == name, E.symbolKind s == E.FunctionSymbol, E.symbolSize s > 0] of
        [value] -> value
        _ -> error ("missing accessor ELF symbol "++name)
      readEntries = [(0x69f8fc,"xm1_calibration_u8"),(0x69fec8,"xm1_calibration_u16"),(0x69fef0,"xm1_calibration_u24"),(0x69ff24,"xm1_calibration_u32"),(0x69ff68,"xm1_calibration_i8"),(0x69ff7c,"xm1_calibration_i16"),(0x69ffa4,"xm1_calibration_u32"),(0x69ffe8,"xm1_calibration_i8"),(0x69fffc,"xm1_calibration_byte_halfword")]
      calls = [(stock,name,offset) | (stock,name) <- readEntries, offset <- [0,1,0xf7,0x41fffc]] ++ [(0x69f8b8,"xm1_calibration_fixed_halfword",0),(0x69f8e4,"xm1_calibration_zero",0),(0x69fcb8,"xm1_calibration_base",0),(0x69fcd8,"xm1_calibration_resource_base",0),(0x6a0010,"xm1_calibration_pointer",0x420)] ++ [(0x69fcfc,"xm1_calibration_region",region) | region <- [0..9]]
      program owned = assemble $
        [0xe92d4010] ++ mov 4 0x240000 ++ mov 0 0xf7 ++ mov 1 0x102 ++ mov 12 (if owned then ownedSymbol "xm1_calibration_set_byte" else 0x69fc94) ++ [0xe12fff3c] ++
        concat [mov 0 argument ++ mov 12 (if owned then ownedSymbol name else stock) ++ [0xe12fff3c,0xe4840004] | (stock,name,argument) <- calls] ++ [0xe3a00000,0xe8bd8010]
      programRegion owned = mkRegion (RegionId "accessor-caller") (address 0x230000) (B.length (program owned)) ReadExecute (program owned)
      programFunction owned = (address 0x230000,fromIntegral (B.length (program owned)))
      common = [mkRegion (RegionId "cache") (address 0x41acd800) 0x420000 ReadWrite initialCache, mkRegion (RegionId "cache-pointer") (address 0x96ca50) 4 ReadWrite initialPointer, mkRegion (RegionId "resource") (address 0x7c8a44) 504 ReadOnly defaults, mkRegion (RegionId "storage-journal") (address 0x210000) 80 ReadWrite (B.replicate 80 0), mkRegion (RegionId "accessor-results") (address 0x240000) 256 ReadWrite (B.replicate 256 0), mkRegion (RegionId "stack") (address 0x300000) 0x1000 ReadWrite (B.replicate 0x1000 0), mkRegion (RegionId "exit") (address 0x220000) 4 ReadExecute (word32le 0xe12fff1e)] ++ callbackCode
      abi = [(r,0xa5000000+fromIntegral (fromEnum r)) | r <- [R4,R5,R6,R7,R8,R9,R10,R11]]
      make role entry code functions = CaseSpec
        { caseId = role++"-cache-"++scenarioText, caseProvenance = pin++"; image4 defaults resource 7c8a44 extent504 SHA2568179e82dd07c45460f20ca51150e4b14d119dd27878690454a3f0cb8c5b263b1; complete 0x420000-byte synthetic cache backing"
        , caseExclusion = "Storage callbacks supply deterministic synthetic bytes and return an ignored error status. No flash/device behavior is modeled; complete final cache and ordered storage-call journal are compared. Bulk-fill write logs may be truncated."
        , caseEntry = address entry, caseExit = address 0x220000, caseRegions = code++common
        , caseFunctions = functions++[(address a,fromIntegral (B.length bytes)) | (a,bytes) <- cacheCallbacks scenario]
        , caseInitial = [(R0,case scenario of Reload page -> page; _ -> 0),(Sp,0x300800),(Lr,0x220000),(Cpsr,0x13)]++abi++[(r,0xa5000000+fromIntegral (fromEnum r)) | r <- [R1,R2,R3,R12]]
        , caseExpectedStatus = Returned, caseExpectedRegisters = (R0,0):(Sp,0x300800):abi, caseExpectedMemory = [], caseCp14Identity = Nothing
        , caseKernel = Just (KernelCase (InstructionLimit (if scenario `elem` [Fallback,DefaultTable] then 16000000 else 4000000)) Nothing Nothing Nothing Nothing [] [])
        , caseIgnoredMemory = []
        }
      stockEntry = case scenario of Reload _ -> 0x69fe20; Accessors -> 0x230000; _ -> 0x69f204
      stockSpec = make "stock" stockEntry (stockCode ++ [programRegion False | scenario == Accessors]) ([(address a,n) | (a,n) <- retainedFunctions] ++ [programFunction False | scenario == Accessors])
      ownedSpec = make "owned" (if scenario == Accessors then 0x230000 else ownedEntry) (ownedCode ++ [programRegion True | scenario == Accessors]) (ownedFunctions ++ [programFunction True | scenario == Accessors])
  stockPath <- writeCaseWithProfile Resource8 (out </> "stock") stockSpec
  ownedPath <- writeCaseWithProfile Resource8 (out </> "owned") ownedSpec
  case action of
    Stage -> putStrLn ("cache pair staged: "++stockPath++" "++ownedPath)
    Execute -> executeCache runner stockPath ownedPath

executeCache :: FilePath -> FilePath -> FilePath -> IO ()
executeCache runner stockPath ownedPath = do
  stock <- loadObserved runner stockPath
  owned <- loadObserved runner ownedPath
  unless (callerStackPreserved stock && callerStackPreserved owned) (fail "cache caller stack changed")
  let memory observation = [(ident,bytes) | (ident,bytes) <- nativeMemory observation, ident `elem` [RegionId "cache",RegionId "cache-pointer",RegionId "storage-journal",RegionId "accessor-results"]]
      registers observation = [(r,lookupRegister r observation) | r <- [R0,R4,R5,R6,R7,R8,R9,R10,R11,Sp]]
  unless (nativeStatus stock == Returned && nativeStatus owned == Returned && memory stock == memory owned && registers stock == registers owned) (fail "cache native difference")
  unless (fmap (.&. 0x1ff) (lookupRegister Cpsr stock) == fmap (.&. 0x1ff) (lookupRegister Cpsr owned)) (fail "cache CPSR control differs")
  putStrLn "complete-cache native pair agrees"
