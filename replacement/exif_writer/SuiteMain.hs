{-# LANGUAGE OverloadedStrings #-}

-- | exif-writer-suite RUNNER LOADER_A IMAGE4 PLAIN OWNED_ELF OUTPUT CONTROL=ELF ...
--
-- Runs each scenario of "Replacement.ExifWriter.Cases" natively on the stock overlay-46 bytes,
-- the owned ELF linked over the same extents and each deliberately wrong owned ELF. Both roles
-- call the stock leaf services through a recorder that logs every call's service and arguments,
-- stack addresses replaced by a marker. The stock role runs once to observe its final memory,
-- log and results; every role then runs against those as its expectation. Equal requires both
-- stock runs and the owned run to pass, every Exif block to satisfy the independent layout
-- checks of "Replacement.ExifWriter.Structure", and every control to fail at least one case
-- after reaching the case exit.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt)
import Data.IORef (modifyIORef', newIORef, readIORef)
import Data.List (nub)
import Data.Maybe (fromMaybe, listToMaybe)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import qualified Emulation.Coverage as Coverage
import qualified Emulation.NativeCase as Native
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.ExifWriter.Cases
import Replacement.ExifWriter.Structure (checkHeader)
import Replacement.FileService.Native (Call (..), Extent (..), extentContains, hexWord, word32le, words32le)
import System.Directory (createDirectoryIfMissing, doesPathExist)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))

driverBase, resultsBase, stackBase, stackTop, exitAddress, recorderBase, callLog, trapWord :: Word32
driverBase = 0x300000
resultsBase = 0x301000
stackBase = 0x302000
stackTop = 0x304000
exitAddress = 0x305000
recorderBase = 0x200000
callLog = 0x210000
trapWord = 0xe7f000f0

callLogBytes :: Int
callLogBytes = 0x10000

-- | Recorded argument registers per service; services absent here are not recorded.
recordedArguments :: [(String, Word32)]
recordedArguments = [("copy-bytes", 3), ("strcpy", 2), ("strncpy", 3), ("strlen", 1), ("memcpy", 3), ("memset", 3)]

data Role = Stock | Owned String E.Elf

roleLabel :: Role -> String
roleLabel Stock = "stock"
roleLabel (Owned label _) = label

-- Encoders ---------------------------------------------------------------------

movw, movt :: Word32 -> Word32 -> Word32
movw register value = 0xe3000000 .|. ((value .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. (value .&. 0xfff)
movt register value =
  0xe3400000 .|. (((value `shiftR` 16) .&. 0xf000) `shiftL` 4) .|. (register `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff)

branch, branchLink :: Word32 -> Word32 -> Word32
branch pc target = 0xea000000 .|. (((target - pc - 8) `shiftR` 2) .&. 0xffffff)
branchLink pc target = 0xeb000000 .|. (((target - pc - 8) `shiftR` 2) .&. 0xffffff)

-- | Appends the service address and its first @arguments@ registers to the log, each register
-- that names the stack replaced by @0xffffffff@, then runs the displaced first instruction and
-- continues at the service's second instruction.
recorder :: Word32 -> Word32 -> Word32 -> Word32 -> [Word32]
recorder at service displaced arguments =
  body ++ [displaced, branch (at + 4 * fromIntegral (length body) + 4) (service + 4)]
  where
    body =
      [0xe92d0070, movw 4 callLog, movt 4 callLog, 0xe5945000, movw 6 service, movt 6 service, 0xe4856004]
        ++ concat [masked register | register <- [0 .. arguments - 1]]
        ++ [0xe5845000, 0xe8bd0070]
    masked register =
      [ movw 6 stackBase, movt 6 stackBase
      , 0xe0406006 .|. (register `shiftL` 16)
      , 0xe3560a02
      , 0x33e06000
      , 0x21a06000 .|. register
      , 0xe4856004
      ]

driverWords :: [Call] -> [Word32]
driverWords calls = 0xe92d4010 : go (driverBase + 4) 0 calls ++ [0xe8bd8010]
  where
    go _ _ [] = []
    go pc slot (Call entry arguments stack returns : rest) =
      let frame = 8 * ((fromIntegral (length stack) + 1) `div` 2) :: Word32
          reserve = if null stack then [] else [0xe24dd000 .|. frame]
          spill = concat [[movw 12 value, movt 12 value, 0xe58dc000 .|. (4 * index)] | (index, value) <- zip [0 ..] stack]
          loads = concat [[movw register value, movt register value] | (register, value) <- zip [0 ..] arguments]
          before = reserve ++ spill ++ loads
          callAt = pc + 4 * fromIntegral (length before)
          release = if null stack then [] else [0xe28dd000 .|. frame]
          store = if returns then [movw 12 (resultsBase .&. 0xffff), movt 12 resultsBase, 0xe58c0000 .|. (4 * slot)] else []
          block = before ++ [branchLink callAt entry] ++ release ++ store
       in block ++ go (pc + 4 * fromIntegral (length block)) (if returns then slot + 1 else slot) rest

-- Images ----------------------------------------------------------------------

data Loaded = Loaded Word32 B.ByteString

loadedBytes :: Loaded -> B.ByteString
loadedBytes (Loaded _ bytes) = bytes

slice :: [(String, Loaded)] -> String -> Word32 -> Word32 -> B.ByteString
slice loaded label low high = case lookup label loaded of
  Just (Loaded base bytes) | low >= base && fromIntegral (high - base) <= B.length bytes -> B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - base)) bytes)
  _ -> error ("no " ++ label ++ " bytes for " ++ hexWord low)

ownedBytes :: E.Elf -> Extent -> B.ByteString
ownedBytes elf extent@(Extent low high) = B.pack [byte address | address <- [low .. high - 1]]
  where
    sections = [s | s <- E.elfSections elf, E.sectionAllocated s, E.sectionSize s > 0, extentContains extent (E.sectionAddress s)]
    byte address = case [value | s <- sections, E.sectionAddress s <= address, address < E.sectionAddress s + E.sectionSize s, Just value <- [E.byteAt elf address]] of
      (value : _) -> value
      [] -> B.index (word32le trapWord) (fromIntegral (address .&. 3))

overlayBytes :: [(String, Loaded)] -> Role -> B.ByteString
overlayBytes loaded role = case role of
  Stock -> stock
  Owned _ elf -> foldl (patch elf) stock (map snd ownedExtents)
  where
    Extent low high = overlay
    stock = slice loaded "plain" low high
    patch elf bytes extent@(Extent from to) =
      let at = fromIntegral (from - low)
       in B.take at bytes <> ownedBytes elf extent <> B.drop (at + fromIntegral (to - from)) bytes

ownedFunctions :: E.Elf -> [(Word32, Word32)]
ownedFunctions elf =
  nub
    [ (E.symbolValue symbol, E.symbolSize symbol)
    | symbol <- E.elfSymbols elf
    , E.symbolKind symbol == E.FunctionSymbol
    , E.symbolSize symbol > 0
    , any (\(_, extent) -> extentContains extent (E.symbolValue symbol)) ownedExtents
    ]

-- Case documents ----------------------------------------------------------------

data Region = Region String Word32 String B.ByteString

registers :: [(String, Word32)] -> Value
registers pairs = object [Key.fromString name .= hexWord value | (name, value) <- pairs]

preserved :: [(String, Word32)]
preserved = [("r" ++ show index, 0x10101010 * fromIntegral index) | index <- [4 :: Int .. 11]]

-- | The final bytes each writable region must hold, and r0 of each returning call.
data Expectation = Expectation {expectedRegions :: [(String, B.ByteString)], expectedResults :: [Word32]}

serviceRegions :: [(String, Loaded)] -> ([Region], [(Word32, Word32)])
serviceRegions loaded = (code ++ [Region "recorders" recorderBase "rx" (words32le recorders)], functions)
  where
    recorded = [(service, arguments) | service <- services, Just arguments <- [lookup (serviceName service) recordedArguments]]
    placed = zip [recorderBase, recorderBase + 0x80 ..] recorded
    recorders = concat [take 32 (recorder at (serviceEntry service) (displaced service) arguments ++ repeat trapWord) | (at, (service, arguments)) <- placed]
    displaced service = wordAt (slice loaded (serviceImage service) (serviceEntry service) (serviceEntry service + 4))
    redirect service = case [at | (at, (s, _)) <- placed, serviceEntry s == serviceEntry service] of
      [at] -> word32le (branch (serviceEntry service) at) <> B.drop 4 (bytes service)
      _ -> bytes service
    bytes service = slice loaded (serviceImage service) (serviceEntry service) (serviceEnd service)
    code = [Region ("service-" ++ serviceName service) (serviceEntry service) "rx" (redirect service) | service <- services]
    functions =
      concat
        [ if any (\(_, (s, _)) -> serviceEntry s == serviceEntry service) placed
            then [(serviceEntry service, 4), (serviceEntry service + 4, serviceEnd service - serviceEntry service - 4)]
            else [(serviceEntry service, serviceEnd service - serviceEntry service)]
        | service <- services
        ]
        ++ [(at, 0x80) | (at, _) <- placed]
    wordAt word = foldr (\index acc -> acc `shiftL` 8 .|. fromIntegral (B.index word index)) 0 [0 .. 3]

caseDocument :: [(String, Loaded)] -> Role -> Scenario -> Expectation -> (String, Value, B.ByteString)
caseDocument loaded role scenario expectation = (ident, document, blob)
  where
    ident = scenarioName scenario ++ "-" ++ (case role of Stock -> "stock"; Owned "owned" _ -> "owned"; Owned _ _ -> "control")
    (serviceCode, serviceFunctions) = serviceRegions loaded
    driver = words32le (driverWords (scenarioCalls scenario))
    Extent overlayLow _ = overlay
    writable =
      [Region (blockName b) (blockAddress b) "rw" (blockBytes b) | b <- scenarioBlocks scenario]
        ++ [Region "log" callLog "rw" (word32le (callLog + 4) <> B.replicate (callLogBytes - 4) 0), Region "results" resultsBase "rw" (B.replicate 0x100 0)]
    regions =
      [Region "overlay46" overlayLow "rx" (overlayBytes loaded role)]
        ++ serviceCode
        ++ writable
        ++ [ Region "driver" driverBase "rx" driver
           , Region "stack" stackBase "rw" (B.replicate (fromIntegral (stackTop - stackBase)) 0xa5)
           , Region "exit" exitAddress "rx" (word32le 0xe12fff1e)
           ]
    blob = B.concat [bytes | Region _ _ _ bytes <- regions]
    blobName = sha256Hex (sha256 blob)
    blobId = "a-" ++ take 40 blobName
    offsets = scanl (+) 0 [B.length bytes | Region _ _ _ bytes <- regions]
    functions = case role of
      Stock -> stockFunctions
      Owned _ elf -> ownedFunctions elf
    initial = [("r" ++ show index, 0x0e0e0e00 + fromIntegral index) | index <- [0 :: Int .. 3]] ++ preserved ++ [("r12", 0x0c0c0c0c), ("sp", stackTop), ("lr", exitAddress), ("cpsr", 0x10)]
    expectedMemory =
      [ object ["region" .= name, "offset" .= (0 :: Int), "hex" .= Native.hexBytes bytes]
      | Region name _ _ _ <- writable
      , name /= "results"
      , Just bytes <- [lookup name (expectedRegions expectation)]
      ]
        ++ [ object ["region" .= ("results" :: String), "offset" .= (0 :: Int), "hex" .= Native.hexBytes (words32le (expectedResults expectation))]
           | not (null (expectedResults expectation))
           ]
    document =
      object
        [ "schema_version" .= (1 :: Int)
        , "id" .= ident
        , "origin" .= (case role of Stock -> "stock"; Owned _ _ -> "synthetic" :: String)
        , "provenance" .= (scenarioProvenance scenario ++ " Role " ++ roleLabel role ++ "; X-M1 1.50 plain image overlay 46, image 4 and loader A services.")
        , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
        , "artifacts" .= [object ["id" .= blobId, "path" .= (blobName ++ ".bin"), "size" .= B.length blob, "sha256" .= blobName]]
        , "entry" .= hexWord driverBase
        , "exit" .= hexWord exitAddress
        , "registers" .= registers initial
        , "regions" .=
            [ object
                [ "id" .= name
                , "address" .= hexWord address
                , "size" .= B.length bytes
                , "permissions" .= permissions
                , "initial" .= object ["artifact" .= blobId, "offset" .= offset, "length" .= B.length bytes]
                ]
            | (Region name address permissions bytes, offset) <- zip regions offsets
            ]
        , "functions" .= [object ["address" .= hexWord address, "size" .= size] | (address, size) <- functions ++ serviceFunctions ++ [(driverBase, fromIntegral (B.length driver))]]
        , "stubs" .= ([] :: [Value])
        , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (10000 :: Int)]
        , "ignored_memory" .= [object ["region" .= ("stack" :: String), "offset" .= (0 :: Int), "length" .= (fromIntegral (stackTop - stackBase) :: Int), "reason" .= ("private-frame" :: String)]]
        , "expected" .=
            object
              [ "status" .= ("returned" :: String)
              , "registers" .= registers (preserved ++ [("sp", stackTop), ("pc", exitAddress)])
              , "memory" .= expectedMemory
              , "events" .= ([] :: [Value])
              ]
        , "exclusions" .= ["Host Cortex-R5F native profile. Leaf services run from their stock bytes behind a recorder; no kernel, scheduling, interrupt or device qualification." :: String]
        ]

-- Execution -----------------------------------------------------------------------

data Observation = Observation
  { observationCase :: String
  , observationPassed :: Bool
  , observationStatus :: String
  , observationMismatches :: [String]
  , observationInstructions :: Integer
  , observationMemory :: [(Word32, B.ByteString)]
  , observationRegisters :: Value
  }

field :: String -> Value -> Value
field name (Object fields) = fromMaybe Null (KM.lookup (Key.fromString name) fields)
field _ _ = Null

text :: Value -> String
text (String value) = T.unpack value
text _ = ""

fromHex :: String -> B.ByteString
fromHex = B.pack . go
  where
    go (a : b : rest) = fromIntegral (digitToInt a * 16 + digitToInt b) : go rest
    go _ = []

readAddress :: String -> Word32
readAddress textual = foldl (\acc c -> acc * 16 + fromIntegral (digitToInt c)) 0 (drop 2 textual)

observe :: Native.CaseReport -> Observation
observe report =
  Observation
    { observationCase = Native.caseIdText (Native.reportCase report)
    , observationPassed = text (field "status" value) == "passed"
    , observationStatus = text (field "status" runner)
    , observationMismatches = case field "mismatches" value of
        Array items -> map text (V.toList items)
        _ -> []
    , observationInstructions = case field "instructions" runner of
        Number count -> truncate count
        String count | [(parsed, "")] <- reads (T.unpack count) -> parsed
        _ -> 0
    , observationMemory = case field "memory" runner of
        Array items -> [(readAddress (text (field "address" item)), fromHex (text (field "bytes" item))) | item <- V.toList items]
        _ -> []
    , observationRegisters = field "registers" runner
    }
  where
    value = Native.reportValue report
    runner = field "runner" value

runRole :: FilePath -> FilePath -> [(String, Loaded)] -> Role -> String -> [(Scenario, Expectation)] -> IO [Observation]
runRole runner directory loaded role phase cases = do
  let batchDirectory = directory </> (phase ++ "-" ++ roleLabel role)
  createDirectoryIfMissing True batchDirectory
  listed <- forM cases $ \(scenario, expectation) -> do
    let (ident, document, blob) = caseDocument loaded role scenario expectation
    B.writeFile (batchDirectory </> (sha256Hex (sha256 blob) ++ ".bin")) blob
    BL.writeFile (batchDirectory </> (ident ++ ".case.json")) (encode document)
    pure (object ["id" .= ident, "path" .= (ident ++ ".case.json")])
  let batchPath = batchDirectory </> "batch.json"
  BL.writeFile batchPath (encode (object ["schema_version" .= (1 :: Int), "cases" .= listed]))
  collected <- newIORef []
  summary <- Native.runBatch runner batchPath $ \report -> do
    BL.writeFile (batchDirectory </> (Native.caseIdText (Native.reportCase report) ++ ".report.json")) (encode (Native.reportValue report))
    modifyIORef' collected (observe report :)
  observations <- reverse <$> readIORef collected
  unless (Native.summaryStatus summary == Native.BatchComplete && length observations == length cases) $
    fail ("native batch did not complete for " ++ phase ++ " " ++ roleLabel role ++ ": " ++ show (Native.summaryReason summary))
  pure observations

-- | The expectation a stock observation fixes: every writable region's final bytes and r0 of
-- each returning call.
expectationOf :: Scenario -> Observation -> Expectation
expectationOf scenario observation =
  Expectation
    { expectedRegions = [(name, bytesAt address) | (name, address) <- writableRegions scenario, name /= "results"]
    , expectedResults = take returning (wordsOf (bytesAt resultsBase))
    }
  where
    bytesAt address = fromMaybe B.empty (lookup address (observationMemory observation))
    returning = length (filter callReturns (scenarioCalls scenario))

initialExpectation :: Scenario -> Expectation
initialExpectation scenario =
  Expectation
    { expectedRegions = [(blockName b, blockBytes b) | b <- scenarioBlocks scenario]
    , expectedResults = [0 | call <- scenarioCalls scenario, callReturns call]
    }

writableRegions :: Scenario -> [(String, Word32)]
writableRegions scenario = [(blockName b, blockAddress b) | b <- scenarioBlocks scenario] ++ [("log", callLog), ("results", resultsBase)]

-- | The stock functions' branch coverage over the judged stock reports, each extent
-- authenticated against the plain image's bytes.
stockCoverage :: [(String, Loaded)] -> FilePath -> [Scenario] -> IO Coverage.Coverage
stockCoverage loaded directory cases = do
  observations <- forM cases $ \scenario -> do
    bytes <- B.readFile (directory </> (scenarioName scenario ++ "-stock.report.json"))
    either (\problem -> fail (scenarioName scenario ++ " coverage: " ++ problem)) pure (Coverage.parseObservation bytes)
  pure (Coverage.coverage extents observations)
  where
    extents = [Coverage.Extent entry size (Just (sha256Hex (sha256 (slice loaded "plain" entry (entry + size))))) | (entry, size) <- stockFunctions]

-- | Every conditional branch direction no case reached, as (address, missing direction).
missingDirections :: Coverage.Coverage -> [(Word32, String)]
missingDirections result =
  [ (Coverage.branchAddress decision, missing)
  | function <- Coverage.coverageFunctions result
  , decision <- Coverage.functionBranches function
  , missing <- case Coverage.branchDirection decision of
      Coverage.Both -> []
      Coverage.PassedOnly -> ["not-taken"]
      Coverage.FailedOnly -> ["taken"]
      _ -> ["taken", "not-taken"]
  ]

wordsOf :: B.ByteString -> [Word32]
wordsOf bytes
  | B.length bytes < 4 = []
  | otherwise = foldr (\index acc -> acc `shiftL` 8 .|. fromIntegral (B.index bytes index)) 0 [0 .. 3] : wordsOf (B.drop 4 bytes)

-- | Recorded calls in the log: service address, then its recorded arguments.
serviceCalls :: Observation -> [[Word32]]
serviceCalls observation = go (drop 1 (wordsOf logged))
  where
    logged = fromMaybe B.empty (lookup callLog (observationMemory observation))
    arity address = fromMaybe 0 (listToMaybe [arguments | service <- services, serviceEntry service == address, Just arguments <- [lookup (serviceName service) recordedArguments]])
    go (address : rest) | address /= 0 = let n = fromIntegral (arity address) in (address : take n rest) : go (drop n rest)
    go _ = []

structureProblems :: Scenario -> Observation -> [String]
structureProblems scenario observation = case scenarioHeader scenario of
  Nothing -> []
  Just (HeaderCall format option) ->
    let output = fromMaybe B.empty (lookup outputBase (observationMemory observation))
        size = case wordsOf (fromMaybe B.empty (lookup resultsBase (observationMemory observation))) of
          (value : _) -> value
          [] -> 0
     in checkHeader format option size output

atExit :: Observation -> Bool
atExit observation = observationStatus observation == "returned"

-- Main ----------------------------------------------------------------------------------

loadElf :: FilePath -> IO (E.Elf, String)
loadElf path = do
  bytes <- B.readFile path
  elf <- either fail pure (E.parseElf bytes)
  forM_ entries $ \(name, address) ->
    case [symbol | symbol <- E.elfSymbols elf, E.symbolName symbol == name, E.symbolKind symbol == E.FunctionSymbol] of
      [symbol] | E.symbolValue symbol == address -> pure ()
      _ -> fail (path ++ ": " ++ name ++ " is missing or not at its stock address")
  let outside =
        [ E.sectionName section
        | section <- E.elfSections elf
        , E.sectionAllocated section
        , E.sectionSize section > 0
        , not (any (\(_, extent) -> extentContains extent (E.sectionAddress section) && extentContains extent (E.sectionAddress section + E.sectionSize section - 1)) ownedExtents)
        ]
  unless (null outside) (fail (path ++ ": sections outside the retired extents: " ++ unwords outside))
  pure (elf, sha256Hex (sha256 bytes))

splitControl :: String -> (String, FilePath)
splitControl argument = case break (== '=') argument of
  (name, '=' : path) -> (name, path)
  _ -> error ("control argument is not NAME=ELF: " ++ argument)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    runner : loaderPath : image4Path : plainPath : ownedPath : output : controlArguments@(_ : _) -> do
      exists <- doesPathExist output
      when exists (fail "output exists")
      createDirectoryIfMissing True output
      loaded <- forM (zip images [loaderPath, image4Path, plainPath]) $ \(image, path) -> do
        bytes <- B.readFile path
        unless (sha256Hex (sha256 bytes) == imageDigest image) (fail (imageLabel image ++ " identity differs"))
        pure (imageLabel image, Loaded (imageBase image) bytes)
      let plain = loadedBytes (fromMaybe (error "plain") (lookup "plain" loaded))
          cases = scenarios plain
      (owned, ownedDigest) <- loadElf ownedPath
      observed <- runRole runner output loaded Stock "observe" [(scenario, initialExpectation scenario) | scenario <- cases]
      forM_ observed $ \observation ->
        unless (atExit observation) (fail ("stock observation " ++ observationCase observation ++ " ended " ++ observationStatus observation ++ ": " ++ unwords (observationMismatches observation)))
      let expectations = zipWith expectationOf cases observed
          judged = zip cases expectations
      stocks <- runRole runner output loaded Stock "judge" judged
      covered <- stockCoverage loaded (output </> "judge-stock") cases
      owneds <- runRole runner output loaded (Owned "owned" owned) "judge" judged
      controls <- forM (map splitControl controlArguments) $ \(name, path) -> do
        (elf, digest) <- loadElf path
        results <- runRole runner output loaded (Owned name elf) "judge" judged
        let detected = [observationCase o | o <- results, atExit o, not (observationPassed o)]
            other = [(observationCase o, observationStatus o) | o <- results, not (atExit o)]
        pure (name, digest, detected, other)
      let compared =
            [ (scenario, stock, mine, problems)
            | (scenario, stock, mine) <- zip3 cases stocks owneds
            , let problems =
                    ["stock " ++ observationStatus stock ++ ": " ++ unwords (observationMismatches stock) | not (observationPassed stock)]
                      ++ ["owned " ++ observationStatus mine ++ ": " ++ unwords (observationMismatches mine) | not (observationPassed mine)]
                      ++ ["stock layout: " ++ problem | problem <- structureProblems scenario stock]
                      ++ ["owned layout: " ++ problem | problem <- structureProblems scenario mine]
                      ++ ["service calls differ" | serviceCalls stock /= serviceCalls mine]
            ]
          different = [() | (_, _, _, problems) <- compared, not (null problems)]
          undetected = [name | (name, _, detected, _) <- controls, null detected]
          branches = concatMap Coverage.functionBranches (Coverage.coverageFunctions covered)
          missing = missingDirections covered
          declared = [(address, direction) | (address, direction, _) <- unreachableDirections]
          uncovered = [gap | gap <- missing, gap `notElem` declared]
          staleUnreachable = [gap | gap <- declared, gap `notElem` missing]
          unauthenticated = [entry | function <- Coverage.coverageFunctions covered, let entry = Coverage.functionEntry function, not (Coverage.functionAuthenticated function)]
          complete = null uncovered && null staleUnreachable && null unauthenticated
          equal = null different && null undetected && complete
          report =
            object
              [ "suite" .= ("exif-writer" :: String)
              , "images" .= [object ["label" .= imageLabel image, "base" .= hexWord (imageBase image), "sha256" .= imageDigest image] | image <- images]
              , "plain_sha256" .= concat [imageDigest image | image <- images, imageLabel image == "plain"]
              , "owned_sha256" .= ownedDigest
              , "decomp_commit" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3" :: String)
              , "verdict" .= (if equal then "equal" else "different" :: String)
              , "cases" .=
                  [ object
                      [ "case" .= scenarioName scenario
                      , "stock_status" .= observationStatus stock
                      , "owned_status" .= observationStatus mine
                      , "stock_instructions" .= observationInstructions stock
                      , "owned_instructions" .= observationInstructions mine
                      , "service_calls" .= length (serviceCalls stock)
                      , "header_bytes" .= take 1 (wordsOf (fromMaybe B.empty (lookup resultsBase (observationMemory stock))))
                      , "differences" .= problems
                      ]
                  | (scenario, stock, mine, problems) <- compared
                  ]
              , "controls" .=
                  [ object
                      [ "name" .= name
                      , "sha256" .= digest
                      , "exit_detections" .= detected
                      , "other_statuses" .= [object ["case" .= ident, "status" .= status] | (ident, status) <- other]
                      , "detected" .= not (null detected)
                      ]
                  | (name, digest, detected, other) <- controls
                  ]
              , "stock_coverage" .=
                  object
                    [ "functions" .= length (Coverage.coverageFunctions covered)
                    , "unauthenticated" .= map hexWord unauthenticated
                    , "branches" .= length branches
                    , "directions" .= (2 * length branches)
                    , "directions_covered" .= (2 * length branches - length missing)
                    , "unreachable" .= [object ["address" .= hexWord address, "direction" .= direction, "reason" .= reason] | (address, direction, reason) <- unreachableDirections]
                    , "uncovered" .= [object ["address" .= hexWord address, "direction" .= direction] | (address, direction) <- uncovered]
                    , "stale_unreachable" .= [object ["address" .= hexWord address, "direction" .= direction] | (address, direction) <- staleUnreachable]
                    , "complete" .= complete
                    ]
              ]
      BL.writeFile (output </> "report.json") (encode report)
      result <- lookupEnv "XM1_RUN_RESULT"
      forM_ result $ \directory -> do
        BL.writeFile (directory </> "exif-writer.json") (encode report)
        BL.writeFile (directory </> "exif-writer-coverage.json") (Coverage.coverageJson covered)
      putStrLn
        ( show (length compared) ++ " cases, " ++ show (length different) ++ " different; "
            ++ show (length controls - length undetected) ++ "/" ++ show (length controls) ++ " controls detected; "
            ++ show (2 * length branches - length missing) ++ "/" ++ show (2 * length branches) ++ " stock branch directions covered, "
            ++ show (length declared) ++ " declared unreachable, " ++ show (length uncovered) ++ " uncovered; "
            ++ (if equal then "equal" else "different")
        )
      unless equal exitFailure
    _ -> fail "usage: exif-writer-suite RUNNER LOADER_A IMAGE4 PLAIN OWNED_ELF OUTPUT CONTROL=ELF ..."
