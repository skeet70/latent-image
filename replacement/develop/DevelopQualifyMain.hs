{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main where

import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless)
import qualified ArmElf as E
import Data.Aeson (Value, encode, object, parseJSON, withObject, (.:), (.=))
import Data.Aeson.Types (Parser, parseEither)
import Data.Aeson.Key (fromString)
import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Char (isHexDigit, toLower)
import Data.Int (Int32)
import Data.List (nub)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32)
import Numeric (readHex)
import Emulation.NativeCase (loadCase)
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import Scenarios hiding (base)
import DispatcherScenarios (dispatcherScenarios)
import PermitScenarios (permitScenarios, frameKindCount, frameKindTable)
import SaveScenarios (saveScenarios)
import TransactionScenarios (transactionScenarios)
import Publications (Publication (..), publications)
import EntryScenarios (entryScenarios)
import qualified SourceAdmission as A
import System.Directory (canonicalizePath, createDirectoryIfMissing, removeDirectoryRecursive)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>), takeDirectory)

hex :: Word32 -> String
hex = N.hexWord32
bytesHex :: B.ByteString -> String
bytesHex = concatMap (drop 8 . hex . fromIntegral) . B.unpack
registers :: [(N.Register, Word32)] -> Value
registers values = object [fromString (N.registerLabel register) .= hex value | (register,value) <- values]
patchJson :: Patch -> Value
patchJson (Patch (Address address) bytes) = object ["address" .= hex address,"offset" .= (0::Int),"hex" .= bytesHex bytes]
callJson :: Call -> Value
callJson (Call _ inputs outputs writes) = object ["inputs" .= registers inputs,"outputs" .= registers outputs,"reads" .= ([]::[Value]),"writes" .= map patchJson writes]
callJson (ScriptCall _ inputs outputs memoryReads memoryWrites) = object ["inputs" .= registers inputs,"outputs" .= registers outputs,"reads" .= map memoryJson memoryReads,"writes" .= map memoryJson memoryWrites]
memoryJson :: MemoryContract -> Value
memoryJson (Absolute patch) = patchJson patch
memoryJson (Relative register offset bytes) = object ["register" .= N.registerLabel register,"offset" .= offset,"hex" .= bytesHex bytes]
memoryJson (Indirect register offset bytes) = object ["indirect_register" .= N.registerLabel register,"offset" .= offset,"hex" .= bytesHex bytes]
eventJson :: Call -> Value
eventJson current = object ["address" .= hex (addressWord (callAddress current)),"inputs" .= registers (callInputs current),"outputs" .= registers (callOutputs current)]

apply :: Word32 -> B.ByteString -> [Patch] -> B.ByteString
apply base = foldl update
 where
  update bytes (Patch (Address address) value)
    | address >= base && fromIntegral (address-base) + B.length value <= B.length bytes = let offset = fromIntegral (address-base) in B.take offset bytes <> value <> B.drop (offset+B.length value) bytes
    | otherwise = bytes

loadAuthority :: B.ByteString -> IO A.SourceAdmission
loadAuthority bytes = do
  ensure (sha256Hex (sha256 bytes) == "e39a87981050a3f20114f9d8c856171b0530fdace42c5649be3708b629caa8cd") "source admission inventory differs from this qualifier"
  either failH pure (A.parseAdmission bytes)

admit :: B.ByteString -> A.SourceAdmission -> IO [(Word32,Int)]
admit image source = do
  ensure (A.sourceRevision source == A.Revision "55436fc85c0560dd252b0410af6f2427fe198cd3") "source commit differs"
  ensure (A.digestText (A.sourceImageDigest source) == fixedImageDigest && sha256Hex (sha256 image) == fixedImageDigest) "image5 identity differs"
  let functions = map A.functionExtent (A.sourceFunctions source)
  spans <- forM (functions ++ A.sourceTables source) $ \extent -> do
    let Address address = A.extentAddress extent
        size = fromIntegral (A.byteCount (A.extentSize extent))
        offset = fromIntegral (address-0xbf1a00)
    ensure (address >= 0xbf1a00 && offset+size <= B.length image) "stock extent outside image"
    ensure (sha256Hex (sha256 (B.take size (B.drop offset image))) == A.digestText (A.extentDigest extent)) "stock extent hash differs"
    pure (address,size)
  forM_ (A.sourceCalls source) $ \site -> do
    let Address pc = A.callPc site
        instruction = readWord image pc
        displacement = fromIntegral ((fromIntegral (instruction .&. 0xffffff) :: Int32) `shiftL` 8 `shiftR` 6)
    ensure (instruction == A.instructionWord (A.callInstruction site)) "stock call instruction differs"
    ensure (Address (pc + 8 + displacement) == A.callTarget site) "stock call target differs"
  pure (take (length functions) spans)
 where fixedImageDigest = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"

data Piece = Piece String Word32 String B.ByteString deriving (Eq,Show)
pieceName :: Piece -> String
pieceName (Piece name _ _ _) = name
pieceBytes :: Piece -> B.ByteString
pieceBytes (Piece _ _ _ bytes) = bytes
pieceRegion :: Piece -> Value
pieceRegion (Piece name address permissions bytes) = object ["id" .= name,"address" .= hex address,"size" .= B.length bytes,"permissions" .= permissions,"initial" .= object ["artifact" .= name,"offset" .= (0::Int),"length" .= B.length bytes]]
artifact :: Piece -> Value
artifact piece = object ["id" .= pieceName piece,"path" .= (pieceName piece++".bin"),"size" .= B.length (pieceBytes piece),"sha256" .= sha256Hex (sha256 (pieceBytes piece))]

elfPieces :: String -> (Integer -> Integer -> Bool) -> E.Elf -> IO [Piece]
elfPieces prefix inBounds elf = forM (zip [0::Int ..] (filter (\segment -> E.loadSegment segment && E.segmentMemorySize segment > 0) (E.elfSegments elf))) $ \(index,segment) -> do
  let address = E.segmentAddress segment; size = E.segmentMemorySize segment; stored = E.segmentFileSize segment
      end = toInteger address + toInteger size
  ensure (end <= 0x100000000 && inBounds (toInteger address) end && stored <= size && toInteger size <= toInteger (maxBound :: Int)) (prefix++" segment exceeds reserved footprint")
  pure (Piece (prefix++"-"++show index) address (if E.segmentExecutable segment then "rx" else "rw") (B.take (fromIntegral stored) (B.drop (fromIntegral (E.segmentOffset segment)) (E.elfBytes elf)) <> B.replicate (fromIntegral (size-stored)) 0))

candidateBounds :: Integer -> Integer -> Bool
candidateBounds address end = address >= 0x07000000 && end <= 0x07200000

serviceBounds :: Integer -> Integer -> Bool
serviceBounds address end = (address >= 0x721000 && end <= 0x726000) || (address >= 0x820000 && end <= 0x830000) || (address >= 0x11d5600 && end <= 0x11d6000) || (address >= 0xe75490 && end <= 0xe755d0)

entryFor :: E.Elf -> Scenario -> IO Word32
entryFor elf scenario = case [E.symbolValue entry | entry <- E.elfSymbols elf,E.symbolName entry == symbol (scenarioFunction scenario), E.symbolSection entry /= E.Undefined] of
  [address] -> pure address
  _ -> failH ("missing or duplicate candidate symbol: "++symbol (scenarioFunction scenario))

initialRegisters :: Scenario -> Value
initialRegisters scenario = registers (zip [N.R0,N.R1,N.R2,N.R3] (take 4 (scenarioArguments scenario++repeat 0)) ++ preserved ++ [(N.R12,0),(N.Sp,stackTop),(N.Lr,exit),(N.Cpsr,0x10)])
preserved :: [(N.Register,Word32)]
preserved = zip [N.R4,N.R5,N.R6,N.R7,N.R8,N.R9,N.R10,N.R11] [0x44444444,0x55555555,0x66666666,0x77777777,0x88888888,0x99999999,0xaaaaaaaa,0xbbbbbbbb]

caseValue :: String -> Word32 -> [(Word32,Int)] -> Scenario -> [Piece] -> Value
caseValue side entry functions scenario pieces = object
  ["schema_version" .= (1::Int),"id" .= (scenarioName scenario++"-"++side),"origin" .= ("synthetic"::String)
  ,"provenance" .= ("X-M1 1.50 develop transaction; admitted stock extents and scripted retained service ABI"::String)
  ,"cpu" .= object ["architecture" .= ("arm"::String),"model" .= ("cortex-r5f"::String),"mode" .= ("arm"::String),"endian" .= ("little"::String)]
  ,"artifacts" .= map artifact pieces,"entry" .= hex entry,"exit" .= hex exit,"registers" .= initialRegisters scenario,"regions" .= map pieceRegion pieces
  ,"functions" .= [object ["address" .= hex address,"size" .= size] | (address,size) <- functions,address `notElem` [0xe75490,0xeb0ea0] || all ((/= Address address) . callAddress) (scenarioCalls scenario)]
  ,"stubs" .= [object ["address" .= hex address,"calls" .= [callJson current | current <- scenarioCalls scenario,callAddress current == Address address]] | Address address <- nub (map callAddress (scenarioCalls scenario))]
  ,"limits" .= object ["instructions" .= (4000000::Int),"wall_ms" .= (4000::Int)]
  ,"expected" .= object ["status" .= N.runnerStatusLabel (interfaceStatus scenario),"registers" .= registers (interfaceRegisters scenario),"memory" .= ([]::[Value]),"events" .= map eventJson (scenarioCalls scenario)]
  ,"exclusions" .= (["Caller-clobbered registers and private stack bytes are not interface results; callback control identity cells [0x9a3014,0x9a301c) are relocated. Every other writable byte and ordered service event is checked." ]::[String])]

runCase :: FilePath -> FilePath -> String -> Word32 -> [(Word32,Int)] -> Scenario -> [Piece] -> IO (Value,Maybe (N.NativeObservation,EffectJournal))
runCase runner output side entry functions scenario pieces = do
  let directory = output </> scenarioName scenario </> side
  createDirectoryIfMissing True directory
  forM_ pieces $ \piece -> B.writeFile (directory </> (pieceName piece++".bin")) (pieceBytes piece)
  BL.writeFile (directory </> "case.json") (encode (caseValue side entry functions scenario pieces))
  outcome <- try $ do
    (_,regions,wire,_) <- loadCase (directory </> "case.json")
    executable <- canonicalizePath runner
    (timed,code,out,err) <- runBounded 8000000 (8*1024*1024) executable [] wire
    ensure (not timed) ("native runner timeout: "++BC.unpack err)
    report <- jsonRead out
    observation <- either (failH . N.renderObservationError) pure (N.observeNative code regions report)
    journal <- either failH pure (parseEffectJournal report)
    provenanceEntries <- getArray report "memory_provenance"
    B.writeFile (directory </> "wire.bin") wire
    B.writeFile (directory </> "exit.txt") (BC.pack (show code))
    let expectedMemory = [(name,apply address bytes (scenarioFinal scenario)) | Piece name address permissions bytes <- pieces, permissions == "rw",name /= "stack"]
        problems = ["native status differs from the declared observation boundary" | N.nativeStatus observation /= interfaceStatus scenario]
          ++ ["ABI register differs: "++N.registerLabel register | (register,value) <- interfaceRegisters scenario, M.lookup register (N.nativeRegisters observation) /= Just value]
          ++ ["memory differs: "++name | (name,bytes) <- expectedMemory, lookup (N.RegionId name) (N.nativeMemory observation) /= Just bytes]
          ++ ["ordered service journal differs" | get report "events" /= toEvents scenario]
          ++ ["memory provenance truncated" | N.provenanceWrites (N.nativeProvenance observation) == Nothing]
    let receipt = object ["status" .= (if null problems then "passed" else "failed"::String),"problems" .= problems,"native_status" .= N.runnerStatusLabel (N.nativeStatus observation),"runner_exit" .= (case code of ExitSuccess -> 0; ExitFailure value -> value),"wire_sha256" .= sha256Hex (sha256 wire),"memory_provenance_total" .= get report "memory_provenance_total","memory_provenance_recorded" .= length provenanceEntries,"events" .= get report "events","registers" .= registers [(register,value) | (register,_) <- interfaceRegisters scenario,Just value <- [M.lookup register (N.nativeRegisters observation)]],"memory_sha256" .= object [fromString (N.regionName name) .= sha256Hex (sha256 bytes) | (name,bytes) <- N.nativeMemory observation,name /= N.RegionId "stack"],"effect_journal" .= journalJson journal,"effect_journal_sha256" .= sha256Hex (sha256 (BL.toStrict (encode (journalJson journal))))]
    if null problems && scenarioName scenario /= "load-0" && scenarioFunction scenario `notElem` [PermitPrimary,PermitSecondary]
      then removeDirectoryRecursive directory >> createDirectoryIfMissing True directory
      else B.writeFile (directory </> "native-result.json") out
    BL.writeFile (directory </> "receipt.json") (encode receipt)
    pure (receipt,Just (observation,journal))
  case outcome of
    Left (exception :: IOException) -> pure (object ["status" .= ("refused"::String),"problems" .= [show exception]],Nothing)
    Right result -> pure result
 where toEvents current = arr (map eventJson (scenarioCalls current))

interfaceStatus :: Scenario -> N.RunnerStatus
interfaceStatus scenario = if scenarioFunction scenario == Entry then N.UnknownCall else N.Returned

interfaceRegisters :: Scenario -> [(N.Register,Word32)]
interfaceRegisters scenario = [(N.Pc,exit)] ++ (if scenarioFunction scenario == Entry then [] else preserved++[(N.Sp,stackTop)]) ++ maybe [] (\value -> [(N.R0,value)]) (scenarioReturn scenario)

functionSpans :: E.Elf -> [(Word32,Int)]
functionSpans elf = M.toAscList (M.fromListWith max [(E.symbolValue item,fromIntegral (E.symbolSize item)) | item <- E.elfSymbols elf,E.symbolKind item == E.FunctionSymbol,E.symbolSize item > 0,E.symbolSection item /= E.Undefined])

resolve :: E.Elf -> String -> IO Word32
resolve elf name = case [E.symbolValue item | item <- E.elfSymbols elf,E.symbolName item == name,E.symbolSection item /= E.Undefined] of
  [address] -> pure address
  _ -> failH ("required ELF publication missing: "++name)

canonicalMemory :: [String] -> N.NativeObservation -> [(N.RegionId,B.ByteString)]
canonicalMemory names observation = [(name,if name == N.RegionId "context" then apply context bytes [word 0x9a3014 0,word 0x9a3018 0] else bytes) | (name,bytes) <- N.nativeMemory observation,N.regionName name `elem` names]

data ByteWrite = ByteWrite Address Word8 deriving (Eq,Show)
newtype EffectJournal = EffectJournal [ByteWrite] deriving (Eq,Show)

parseEffectJournal :: Value -> Either String EffectJournal
parseEffectJournal = parseEither $ withObject "native report" $ \report -> do
  entries <- report .: "memory_provenance"
  writes <- mapM parseRow entries
  pure (EffectJournal [ByteWrite (Address address) byte | Just (Address base,bytes) <- writes,(offset,byte) <- zip [0::Word32 ..] (B.unpack bytes),let address = base+offset,observable address])
 where
  parseRow :: Value -> Parser (Maybe (Address,B.ByteString))
  parseRow = withObject "native provenance" $ \entry -> do
    kind <- entry .: "kind" :: Parser String
    case kind of
      "write" -> do
        address <- entry .: "address" >>= parseHex 8
        size <- entry .: "size" :: Parser Int
        stored <- entry .: "value" >>= parseHex 16
        unless (size >= 1 && size <= 8 && address + toInteger size <= 0x100000000) (fail "invalid native write extent")
        pure (Just (Address (fromInteger address),B.pack [fromInteger (stored `shiftR` (8*offset)) | offset <- [0..size-1]]))
      "read" -> pure Nothing
      _ -> fail "unexpected native provenance kind"
  parseHex :: Int -> String -> Parser Integer
  parseHex width ('0':'x':digits)
    | length digits == width && all isHexDigit digits, [(value,"")] <- readHex digits = pure value
  parseHex _ _ = fail "invalid native provenance hexadecimal value"
  observable address = any (\(low,high) -> address >= low && address < high) [(0x9a0000,0x9a4000),(0x9b0000,0x9b4000),(0x1500000,0x15c0000),(0x1600000,0x1620000)] && not (address >= 0x9a3014 && address < 0x9a301c)

journalJson :: EffectJournal -> [[String]]
journalJson (EffectJournal writes) = [[hex address,bytesHex (B.singleton byte)] | ByteWrite (Address address) byte <- writes]

equivalentObservations :: [String] -> Maybe (N.NativeObservation,EffectJournal) -> Maybe (N.NativeObservation,EffectJournal) -> Bool
equivalentObservations names (Just (original,originalJournal)) (Just (candidate,candidateJournal)) = canonicalMemory names original == canonicalMemory names candidate && originalJournal == candidateJournal
equivalentObservations _ _ _ = False

untilFailure :: (a -> IO Value) -> [a] -> IO [Value]
untilFailure _ [] = pure []
untilFailure action (current:rest) = do
  result <- action current
  if str (get result "status") == "passed" then (result:) <$> untilFailure action rest else pure [result]

classifierControl :: String -> IO a -> IO Value
classifierControl name action = do
  outcome <- try (action >> pure ())
  let refused = case outcome of Left (_ :: IOException) -> True; Right () -> False
  pure (object ["id" .= name,"status" .= (if refused then "refused" else "failed"::String)])

sourcePaths :: [FilePath]
sourcePaths = ["BUILD.bazel","standalone.rs","develop.ld","develop_stock.S","framework_stock.S","permit_stock.S","src/lib.rs","src/framework.rs","src/permit.rs","src/pipeline.rs","DevelopQualifyMain.hs","model/Scenarios.hs","model/TransactionScenarios.hs","model/DispatcherScenarios.hs","model/SaveScenarios.hs","model/PermitScenarios.hs","model/Publications.hs","model/services.rs","model/services.ld","../capture/src/lib.rs","../save/src/lib.rs","../save/src/abi.rs","../save/src/contract.rs","../save/src/facade.rs","../save/src/ports.rs","../save/save_stock.S","../media_permit/src/lib.rs","../media_permit/src/stock.rs","../media_permit/rb_media_permit_stock.S"]

data ProofSide = StockProof | OwnedProof
data QualificationMode = FreshQualification | HistoricalQualification FilePath
newtype AcceptedPair = AcceptedPair Value
newtype AcceptedControl = AcceptedControl Value
data HistoricalPrefix = HistoricalPrefix [AcceptedPair] [AcceptedControl]

getArray :: Value -> String -> IO [Value]
getArray document key = either failH pure (parseEither parseJSON (get document key))

compactJournal :: Value -> IO EffectJournal
compactJournal document = do
  rows <- either failH pure (parseEither parseJSON document :: Either String [[String]])
  writes <- forM rows $ \row -> case row of
    ['0':'x':addressText,byteText]
      | length addressText == 8 && all isHexDigit addressText
      , length byteText == 2 && all isHexDigit byteText
      , [(address,"")] <- readHex addressText
      , [(byteValue,"")] <- readHex byteText -> pure (ByteWrite (Address address) byteValue)
    _ -> failH "historical compact journal has an invalid byte"
  let journal = EffectJournal writes
  ensure (journalJson journal == rows) "historical compact journal is not canonical"
  pure journal

checkCompactRole :: Scenario -> [Piece] -> Value -> IO EffectJournal
checkCompactRole scenario pieces receipt = do
  ensure (get receipt "status" == s "passed" && get receipt "problems" == arr []) "historical compact role was not accepted"
  ensure (get receipt "registers" == registers (interfaceRegisters scenario)) "historical compact ABI registers differ"
  ensure (get receipt "events" == arr (map eventJson (scenarioCalls scenario))) "historical compact service events differ"
  let expectedHashes = object [fromString name .= sha256Hex (sha256 (if permissions == "rw" then apply address bytes (scenarioFinal scenario) else bytes)) | Piece name address permissions bytes <- pieces,name /= "stack"]
  ensure (get receipt "memory_sha256" == expectedHashes) "historical compact complete-region hashes differ"
  journal <- compactJournal (get receipt "effect_journal")
  ensure (get receipt "effect_journal_sha256" == s (sha256Hex (sha256 (BL.toStrict (encode (journalJson journal)))))) "historical compact valued journal digest differs"
  pure journal

checkCompactPair :: Scenario -> (ProofSide -> [Piece]) -> Value -> IO AcceptedPair
checkCompactPair scenario pieces row = do
  ensure (get row "id" == s (scenarioName scenario) && get row "status" == s "passed" && get row "equivalent" == b True) "historical compact pair identity or verdict differs"
  original <- checkCompactRole scenario (pieces StockProof) (get row "stock")
  candidate <- checkCompactRole scenario (pieces OwnedProof) (get row "owned")
  ensure (original == candidate) "historical compact ordered valued journals differ"
  pure (AcceptedPair (merge row [("provenance_tier",s "source-bound-historical-compact")]))

loadHistoricalPrefix :: FilePath -> FilePath -> Value -> Value -> [Scenario] -> (Scenario -> ProofSide -> [Piece]) -> IO HistoricalPrefix
loadHistoricalPrefix path root expectedSources identity matrix pieces = do
  bytes <- B.readFile path
  ensure (sha256Hex (sha256 bytes) == "9a50ca49179f3b18118880ebbaa3f5df66aec4ff6d46724f17d219e10e6343ca") "historical R10 compact report digest differs"
  document <- jsonRead bytes
  forM_ ["profile","source_admission","publications","runner_sha256","candidate_sha256","services_sha256","callback_roles"] $ \key ->
    ensure (get document key == get identity key) ("historical compact input identity differs: "++key)
  ensure (get document "source_sha256" == expectedSources) "historical compact source identities differ"
  generator <- B.readFile (root </> "model/PermitScenarios.hs")
  let oldLine = "  make name function arguments initial final result calls = Scenario (\"permit-\" ++ name) function arguments initial final result calls"
      newLine = "  make name function arguments initial final result calls = Scenario (\"permit-\" ++ map toLower name) function arguments initial final result calls"
      oldGenerator = BC.unlines [if line == newLine then oldLine else line | line <- BC.lines generator,line /= "import Data.Char (toLower)"]
  ensure (sha256Hex (sha256 oldGenerator) == "4b0c504b328f79bd8346215b483c115c111cb8a3c6f5c5934d140efcc44c8c5a") "historical generator differs beyond case-id serialization"
  rows <- getArray document "cases"
  controls <- getArray document "controls"
  ensure (length matrix == 632 && length rows == 579 && length controls == 17) "historical compact matrix or control cardinality differs"
  accepted <- forM (zip (take 578 matrix) (take 578 rows)) $ \(scenario,row) -> checkCompactPair scenario (pieces scenario) row
  case (drop 578 matrix,drop 578 rows) of
    (scenario:_,row:_) -> ensure (map toLower (str (get row "id")) == scenarioName scenario && get row "status" == s "failed") "historical stopping row is not the metadata-only refusal"
    _ -> failH "historical compact stopping row is missing"
  forM_ controls $ \control -> ensure (get control "status" == s "refused") "historical compact control was not refused"
  pure (HistoricalPrefix accepted [AcceptedControl (merge control [("provenance_tier",s "source-bound-historical-compact")]) | control <- controls])

main :: IO ()
main = do
  rawArguments <- getArgs
  let (mode,arguments) = case reverse rawArguments of
        path:"--source-bound-prefix":rest -> (HistoricalQualification path,reverse rest)
        _ -> (FreshQualification,rawArguments)
  case arguments of
    [runner,candidatePath,servicesPath,imagePath,admissionPath,output] -> do
      image <- B.readFile imagePath
      admission <- B.readFile admissionPath
      authority <- loadAuthority admission
      contract <- jsonRead admission
      spans <- admit image authority
      elf <- either failH pure . E.parseElf =<< B.readFile candidatePath
      serviceElf <- either failH pure . E.parseElf =<< B.readFile servicesPath
      owned <- elfPieces "candidate" candidateBounds elf
      servicePieces <- elfPieces "model" serviceBounds serviceElf
      let models = [piece | piece@(Piece _ address _ _) <- servicePieces,address < 0xe40000 || address >= 0xeb2000]++[Piece "observer" exit "rx" (wordBytes 0xeafffffe)]
      completion <- resolve elf "xm1_save_completion"
      releaseCompletion <- resolve elf "xm1_save_release_completion"
      ensure ([(A.bindingAddress binding,A.stockNameText (A.bindingSymbol binding)) | binding <- A.sourceBindings authority] == [(address,name) | Publication address name <- publications]) "source ownership publications differ from the fixed map"
      resolvedPublications <- forM publications $ \(Publication (Address address) name) -> do
        ensure (address `elem` map fst spans) ("publication has no admitted stock extent: "++name)
        target <- resolve elf name
        ensure (candidateBounds (toInteger target) (toInteger target + 1) && target `elem` map fst (functionSpans elf)) ("publication outside owned code: "++name)
        size <- case [E.symbolSize item | item <- E.elfSymbols elf,E.symbolName item == name,E.symbolValue item == target,E.symbolSize item > 0] of
          [value] -> pure value
          _ -> failH ("publication has no unique nonempty code extent: "++name)
        bytes <- maybe (failH ("publication bytes are unmapped: "++name)) (pure . B.pack) (mapM (E.byteAt elf) [target..target+size-1])
        sourceExtent <- case [A.functionExtent function | function <- A.sourceFunctions authority,A.extentAddress (A.functionExtent function) == Address address] of
          [value] -> pure value
          _ -> failH ("publication has no unique stock extent: "++name)
        pure (object ["symbol" .= name,"ownership" .= ("owned"::String),"stock_address" .= hex address,"stock_size" .= A.byteCount (A.extentSize sourceExtent),"stock_sha256" .= A.digestText (A.extentDigest sourceExtent),"owned_address" .= hex target,"owned_size" .= size,"owned_sha256" .= sha256Hex (sha256 bytes)])
      dispatch <- resolve elf "xm1_develop_dispatch"
      ensure (dispatch == 0x07000000 && E.elfEntry elf == dispatch) "candidate link entry differs from fixed dispatcher publication"
      runnerBytes <- B.readFile =<< canonicalizePath runner
      sourceInputs <- forM (sourcePaths++["model/SourceAdmission.hs","model/EntryScenarios.hs"]) $ \path -> do
        bytes <- B.readFile (takeDirectory admissionPath </> path)
        pure (path,sha256Hex (sha256 bytes))
      executable <- canonicalizePath =<< getExecutablePath
      executableBytes <- B.readFile executable
      createDirectoryIfMissing True output
      let matrix = scenarios image ++ transactionScenarios image ++ dispatcherScenarios image ++ saveScenarios image ++ permitScenarios image ++ entryScenarios image
          stockPiece = Piece "stock" 0xe40000 "rx" (apply 0xe40000 (B.take 0x72000 (B.drop (0xe40000-0xbf1a00) image)) [Patch (Address address) bytes | Piece _ address _ bytes <- servicePieces,address >= 0xe40000 && address < 0xeb2000])
          modelFunctions = functionSpans serviceElf
          ownedFunctions = functionSpans elf
          memory scenario controls = [Piece name address "rw" (apply address (B.replicate size 0) (scenarioInitial scenario++controls)) | (name,address,size) <- [("context",context,0x4000),("state",state,0x4000),("globals",0x01500000,0xc0000),("frames",0x01600000,0x20000),("stack",0x009a8000,0x4000)]]
          controlsStock = [word 0x9a3014 0xe4e5dc,word 0x9a3018 0xe4e5a8]
          controlsOwned = [word 0x9a3014 completion,word 0x9a3018 releaseCompletion]
          observable = ["context","state","globals","frames"]
      let sidePieces scenario side = memory scenario (case side of StockProof -> controlsStock; OwnedProof -> controlsOwned)++[stockPiece]++models++(case side of StockProof -> []; OwnedProof -> owned)
          identity = object ["profile" .= ("develop-transaction-native"::String),"source_admission" .= contract,"publications" .= resolvedPublications,"runner_sha256" .= sha256Hex (sha256 runnerBytes),"candidate_sha256" .= sha256Hex (sha256 (E.elfBytes elf)),"services_sha256" .= sha256Hex (sha256 (E.elfBytes serviceElf)),"callback_roles" .= object ["stock_completion" .= hex 0xe4e5dc,"owned_completion" .= hex completion,"stock_release" .= hex 0xe4e5a8,"owned_release" .= hex releaseCompletion]]
          expectedSources = obj [(path,s (case path of "DevelopQualifyMain.hs" -> "5996b868d7362cd96320ed6e94b1c30a43aa09ce19744c38e4ba1b83e26ae4e9"; "model/PermitScenarios.hs" -> "4b0c504b328f79bd8346215b483c115c111cb8a3c6f5c5934d140efcc44c8c5a"; _ -> digest)) | (path,digest) <- sourceInputs]
      historical <- case mode of
        FreshQualification -> pure Nothing
        HistoricalQualification path -> Just <$> loadHistoricalPrefix path (takeDirectory admissionPath) expectedSources identity matrix sidePieces
      let accepted = case historical of Nothing -> []; Just (HistoricalPrefix pairs _) -> [row | AcceptedPair row <- pairs]
      freshResults <- untilFailure (\scenario -> do
        entry <- entryFor elf scenario
        (original,originalObservation) <- runCase runner output "stock" (stock (scenarioFunction scenario)) (spans++[(0xeb0ea0,0x38)]++modelFunctions) scenario (memory scenario controlsStock++[stockPiece]++models)
        (candidate,candidateObservation) <- runCase runner output "owned" entry (ownedFunctions++modelFunctions) scenario (memory scenario controlsOwned++[stockPiece]++models++owned)
        let equivalent = equivalentObservations observable originalObservation candidateObservation
            passed = equivalent && str (get original "status") == "passed" && str (get candidate "status") == "passed"
        pure (object ["id" .= scenarioName scenario,"stock" .= original,"owned" .= candidate,"equivalent" .= equivalent,"provenance_tier" .= ("fresh-native"::String),"status" .= (if passed then "passed" else "failed"::String)])) (drop (length accepted) matrix)
      let results = accepted++freshResults
      controls <- case historical of
        Just (HistoricalPrefix _ retainedControls) -> do
          (scenario,row) <- case (matrix,accepted) of
            (firstScenario:_,firstRow:_) -> pure (firstScenario,firstRow)
            _ -> failH "historical compact prefix is empty"
          let original = get row "stock"
              wrongContext = merge row [("stock",merge original [("memory_sha256",merge (get original "memory_sha256") [("context",get (get (get row "owned") "memory_sha256") "context")])])]
              wrongJournal = merge row [("stock",merge original [("effect_journal",arr []),("effect_journal_sha256",s (sha256Hex (sha256 (BL.toStrict (encode ([]::[[String]]))))))])]
          refusals <- sequence
            [classifierControl "compact-context-callback-identity" (checkCompactPair scenario (sidePieces scenario) wrongContext)
            ,classifierControl "compact-valued-journal" (checkCompactPair scenario (sidePieces scenario) wrongJournal)]
          pure ([control | AcceptedControl control <- retainedControls]++refusals)
        Nothing -> do
          transitionControls <- forM [scenario | scenario <- transactionScenarios image,scenarioFunction scenario == Transition,scenarioReturn scenario == Just 6] $ \scenario -> do
            entry <- entryFor elf scenario
            let mutated = [Piece name address permissions (apply address bytes [wordsAt entry [0xe3a00000,0xe12fff1e]]) | Piece name address permissions bytes <- owned]
            (result,observed) <- runCase runner (output </> "controls") "wrong-transition" entry (ownedFunctions++modelFunctions) scenario (memory scenario controlsOwned++[stockPiece]++models++mutated)
            let refused = maybe False ((==N.Returned) . N.nativeStatus . fst) observed && str (get result "status") == "failed"
            pure (object ["id" .= (scenarioName scenario++"-incorrect-transition"),"status" .= (if refused then "refused" else "failed"::String),"witness" .= result])
          let select predicate = case filter predicate matrix of current:_ -> current; [] -> error "required control witness scenario is missing"
              fullSave = select (\scenario -> any (\(Patch (Address address) _) -> address == 0x9a3024) (scenarioInitial scenario))
              fullRelease = select (\scenario -> any (\(Patch (Address address) _) -> address == 0x9a3020) (scenarioFinal scenario))
              begin = select (\scenario -> scenarioName scenario == "begin")
              selectedKind = select (\scenario -> scenarioName scenario == "permit-decode-kind-selected-0-200")
              selectedOrder = select (\scenario -> scenarioName scenario == "permit-decode-kind-selected-0-3")
              boundedEntry = select (\scenario -> scenarioFunction scenario == Entry)
              nativeControl name scenario injected statuses = do
                entry <- entryFor elf scenario
                (result,observed) <- runCase runner (output </> "controls") name entry (ownedFunctions++modelFunctions) (scenario {scenarioName = name}) (memory scenario (controlsOwned++injected)++[stockPiece]++models++owned)
                let refused = maybe False ((`elem` statuses) . N.nativeStatus . fst) observed && str (get result "status") == "failed"
                pure (object ["id" .= name,"status" .= (if refused then "refused" else "failed"::String),"witness" .= result])
          nativeControls <- sequence
            [nativeControl "wrong-storage-callback" fullSave [word 0x9a3014 0] [N.InstructionBudget]
            ,nativeControl "wrong-release-callback" fullRelease [word 0x9a3018 0] [N.InstructionBudget]
            ,nativeControl "wrong-storage-descriptor" fullSave [word 0x9a3024 0] [N.InstructionBudget]
            ,nativeControl "missing-service-event" (begin {scenarioCalls = []}) [] [N.UnknownCall,N.FetchGuard]
            ,nativeControl "doubled-service-event" (begin {scenarioCalls = scenarioCalls begin++take 1 (scenarioCalls begin)}) [] [N.StubContract]
            ,nativeControl "wrong-state-write" (begin {scenarioFinal = [word (state+0x38) 0xffffffff]}) [] [N.Returned]
            ,nativeControl "wrong-retained-frame-kind" selectedKind [word (frameKindTable+200*4) 4] [N.Returned,N.UnknownCall]
            ,nativeControl "wrong-retained-frame-order" selectedOrder [word frameKindCount 1] [N.Returned]
            ,nativeControl "premature-task-observer" boundedEntry [word (context+0x3000) 1] [N.UnknownCall]]
          let journalScenario = begin {scenarioName = "intermediate-state-write",scenarioArguments = [],scenarioInitial = [],scenarioCalls = [],scenarioFinal = [word (state+0x38) 0],scenarioReturn = Just 0}
              journalEntry = 0x9c0000
              journalCode value = Piece "journal-control" journalEntry "rx" (B.concat (map wordBytes [0xe59fc014,0xe3a00000+value,0xe58c0038,0xe3a00000,0xe58c0038,0xe12fff1e,0xe1a00000,state]))
              journalPieces value = memory journalScenario []++[Piece "observer" exit "rx" (wordBytes 0xeafffffe),journalCode value]
          (journalOriginal,journalOriginalObservation) <- runCase runner (output </> "controls") "original" journalEntry [(journalEntry,24)] journalScenario (journalPieces 1)
          (journalMutant,journalMutantObservation) <- runCase runner (output </> "controls") "mutant" journalEntry [(journalEntry,24)] journalScenario (journalPieces 2)
          let journalRefused = case (journalOriginalObservation,journalMutantObservation) of
                (Just (original,EffectJournal originalWrites),Just (mutant,EffectJournal mutantWrites)) ->
                  str (get journalOriginal "status") == "passed" && str (get journalMutant "status") == "passed"
                    && canonicalMemory observable original == canonicalMemory observable mutant
                    && [address | ByteWrite address _ <- originalWrites] == [address | ByteWrite address _ <- mutantWrites]
                    && not (equivalentObservations observable journalOriginalObservation journalMutantObservation)
                _ -> False
              journalControl = object ["id" .= ("intermediate-state-write"::String),"status" .= (if journalRefused then "refused" else "failed"::String),"original" .= journalOriginal,"mutant" .= journalMutant]
          sourceControls <- sequence
            [classifierControl "wrong-source-image" (admit (B.cons 1 (B.drop 1 image)) authority)
            ,classifierControl "wrong-source-calltable" (admit image (authority {A.sourceCalls = [site {A.callInstruction = A.Instruction 0} | site <- take 1 (A.sourceCalls authority)]}))
            ,classifierControl "mutated-source-manifest" (loadAuthority (B.cons 32 (B.drop 1 admission)))
            ,classifierControl "overflow-elf-segment" (elfPieces "candidate" candidateBounds (elf {E.elfSegments = [E.Segment 1 0 0x9c0000 0 0xffffffff 6]}))
            ,classifierControl "missing-elf-publication" (resolve (elf {E.elfSymbols = [if E.symbolName item == "xm1_develop_dispatch" then item {E.symbolName = "incorrect_dispatch"} else item | item <- E.elfSymbols elf]}) "xm1_develop_dispatch")]
          pure (transitionControls++nativeControls++[journalControl]++sourceControls)
      let qualified = length results == length matrix && not (null controls) && all ((=="passed") . str . (`get` "status")) results && all ((=="refused") . str . (`get` "status")) controls
          report = object ["profile" .= ("develop-transaction-native"::String),"source_admission" .= contract,"publications" .= resolvedPublications,"runner_sha256" .= sha256Hex (sha256 runnerBytes),"candidate_sha256" .= sha256Hex (sha256 (E.elfBytes elf)),"services_sha256" .= sha256Hex (sha256 (E.elfBytes serviceElf)),"qualifier_executable" .= executable,"qualifier_sha256" .= sha256Hex (sha256 executableBytes),"qualifier_argv" .= rawArguments,"cases" .= results,"declared_cases" .= length matrix,"accepted_compact_prefix" .= length accepted,"reused_raw" .= (0::Int),"executed_cases" .= length freshResults,"controls" .= controls,"status" .= (if qualified then "qualified" else "failed"::String),"unit_native_qualified" .= qualified,"full_capture_qualified" .= False,"unreachable" .= (["supported status5: the three immutable image5 transition tables begin with the sentinel [0x80,0x101,0x101]; no table mutation is admitted"]::[String]),"retained_dependencies" .= (["pipeline operation bodies","storage machine","frame release and metadata providers","RTOS services","device/card/DMA/cache/timing"]::[String]),"observation_boundary" .= (["Entry is observed at its second receive with an active task frame and UnknownCall only at its exact synthetic observer exit. Task-private stack and active-frame registers have no caller-return contract.","Load admits a separate aligned 52-byte parameter message; overlap with context is not admitted.","Only model callback-identity control cells 0x9a3014 and 0x9a3018 are canonicalized between sides. Each side checks their exact bytes independently."]::[String]),"callback_roles" .= object ["stock_completion" .= hex 0xe4e5dc,"owned_completion" .= hex completion,"stock_release" .= hex 0xe4e5a8,"owned_release" .= hex releaseCompletion],"historical_compact" .= object ["used" .= not (null accepted),"report_sha256" .= ("9a50ca49179f3b18118880ebbaa3f5df66aec4ff6d46724f17d219e10e6343ca"::String),"driver_source_sha256" .= ("5996b868d7362cd96320ed6e94b1c30a43aa09ce19744c38e4ba1b83e26ae4e9"::String),"driver_source_commit" .= ("16632306"::String),"executable_authenticated" .= False,"limitation" .= ("Owner-authorized source-bound historical compact evidence. Original qualifier executable digest and per-role native status, process exit and provenance counts were not recorded. No raw-cache reuse or reconstruction is claimed."::String)],"acceptance" .= False]
      let boundReport = merge report [("source_sha256",obj [(path,s digest) | (path,digest) <- sourceInputs]),("unreachable_pipeline_commands",object ["commands" .= ([0x2b,0x2f] :: [Word32]),"reason" .= ("Absent from every authenticated operation-kind step list; both call arms remain source-admitted and implemented." :: String)]),("worker_provenance",s "DevelopTransactionOwnership; base 93950581; compiled whole-unit source and composed native oracle")]
      BL.writeFile (output </> "results.json") (encode boundReport)
      BL.putStr (encode boundReport <> "\n")
      unless qualified exitFailure
    _ -> failH "usage: develop-qualification RUNNER CANDIDATE_ELF SERVICES_ELF IMAGE5 SOURCE_ADMISSION OUTPUT_DIRECTORY [--source-bound-prefix R10_RESULTS]"
