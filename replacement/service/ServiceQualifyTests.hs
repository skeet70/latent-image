{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_)
import Data.Aeson (Value (..), encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR)
import qualified Data.Map.Strict as M
import Data.Maybe (listToMaybe)
import Data.String (fromString)
import Data.Word (Word32, Word64)
import Emulation.NativeCase (Region (..), loadCase)
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import Replacement.Service.Qualify
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [runner] -> run runner
    _ -> putStrLn "usage: qualification-tests RUNNER" >> exitFailure

run :: FilePath -> IO ()
run runner = do
  aggregationChecks
  consumerChecks
  parserChecks
  armEffectChecks runner
  putStrLn "service qualification tests: passed"

aggregationChecks :: IO ()
aggregationChecks = do
  let baseline = map resultFor serviceRows
      baselineReport = serviceQualificationReport baseline
      firstId = maybe "" serviceRowId (listToMaybe serviceRows)
      missingControl =
        [ if str (get result "id") == firstId then set "mutants" (arr []) result else result
        | result <- baseline
        ]
      unknownControl =
        [ if str (get result "id") == firstId
            then set "mutants" (arr (mutantsFor result ++ [mutantValue "unknown-control"])) result
            else result
        | result <- baseline
        ]
      duplicateControl =
        [ if str (get result "id") == firstId
            then set "mutants" (arr (mutantsFor result ++ take 1 (mutantsFor result))) result
            else result
        | result <- baseline
        ]
      duplicateCase = baseline ++ take 1 baseline
      missingCase = drop 1 baseline
      missingStatus =
        [ if str (get result "id") == firstId then set "status" Null result else result
        | result <- baseline
        ]
      unknownStatus =
        [ if str (get result "id") == firstId then set "status" (s "mystery") result else result
        | result <- baseline
        ]
      emptyStatus =
        [ if str (get result "id") == firstId then set "status" (s "") result else result
        | result <- baseline
        ]
      failedEmptyDiagnostics =
        [ if str (get result "id") == firstId
            then set "status" (s "failed") (set "mismatches" (arr []) result)
            else result
        | result <- baseline
        ]
      failedMissingDiagnostics =
        [ if str (get result "id") == firstId
            then set "status" (s "failed") (set "mismatches" Null result)
            else result
        | result <- baseline
        ]
  check "the complete control domain qualifies" (qualified baselineReport)
  check "a missing control fails closed" (not (qualified (serviceQualificationReport missingControl)))
  check "an unknown control fails closed" (not (qualified (serviceQualificationReport unknownControl)))
  check "a duplicate control fails closed" (not (qualified (serviceQualificationReport duplicateControl)))
  check "a duplicate case id fails closed" (not (qualified (serviceQualificationReport duplicateCase)))
  check "a missing case is partial" (str (get (serviceQualificationReport missingCase) "status") == "partial")
  check "a missing status fails closed" (not (qualified (serviceQualificationReport missingStatus)))
  check "an unknown status fails closed" (not (qualified (serviceQualificationReport unknownStatus)))
  check "an empty status fails closed" (not (qualified (serviceQualificationReport emptyStatus)))
  check "failed with empty diagnostics fails closed" (not (qualified (serviceQualificationReport failedEmptyDiagnostics)))
  check "failed with missing diagnostics fails closed" (not (qualified (serviceQualificationReport failedMissingDiagnostics)))

qualified :: Value -> Bool
qualified report = str (get report "status") == "qualified" && get report "capability" == b True

resultFor :: ServiceRow -> Value
resultFor row =
  obj
    [ ("id", s (serviceRowId row))
    , ("status", s "passed")
    , ("mismatches", arr [])
    , ("runner_sha256", s "00")
    , ("artifacts", arr [])
    , ("mutants", arr [mutantValue name | name <- serviceMutantNames])
    ]

mutantsFor :: Value -> [Value]
mutantsFor result = list (get result "mutants")

mutantValue :: String -> Value
mutantValue name =
  obj
    [ ("name", s name)
    , ("status", s "refused")
    , ("mismatches", arr [])
    ]

check :: String -> Bool -> IO ()
check name ok =
  if ok
    then putStrLn ("ok " ++ name)
    else putStrLn ("FAIL " ++ name) >> exitFailure

consumerChecks :: IO ()
consumerChecks = do
  let expectation = serviceExpectation
      good = goodObservation
  check "a conforming observation passes" $
    case checkServiceCase expectation good of
      CheckPass -> True
      _ -> False
  check "an ABI clobber is refused" $
    case checkServiceCase expectation (mutateRegister good N.R7 0x01010101) of
      CheckMismatch _ -> True
      _ -> False
  check "an immutable write is refused" $
    case checkServiceCase expectation (mutateWrite good (N.GuestAddress 0x009a0100) 4) of
      CheckMismatch _ -> True
      _ -> False
  check "truncated provenance is refused" $
    case checkServiceCase expectation (good {N.nativeProvenance = N.Truncated (N.ProvenanceTotal 5000)}) of
      CheckTruncated _ -> True
      _ -> False

serviceExpectation :: Expectation
serviceExpectation =
  Expectation
    { expectationExpectedR0 = 3
    , expectationPreserved = serviceSeeds ++ [(N.Sp, serviceStackTop), (N.Pc, 0x009b0000)]
    , expectationInitialMemory =
        [ (N.RegionId "casebuf", serviceCasebufInitial)
        , (N.RegionId "mirror", B.replicate serviceTransientSize 0)
        ]
    , expectationBounds =
        [ (N.RegionId "casebuf", N.GuestAddress (fromIntegral serviceStackBase), serviceCasebufSize)
        , (N.RegionId "mirror", N.GuestAddress 0x0089ec00, serviceTransientSize)
        ]
    , expectationStackBase = N.GuestAddress (fromIntegral serviceStackBase)
    , expectationStackOffset = serviceStackOffset
    , expectationStackSize = serviceStackSize
    , expectationStackTop = N.GuestAddress (fromIntegral serviceStackTop)
    , expectationTransientRegion = N.RegionId "mirror"
    , expectationTransientOffset = serviceTransientOffset
    , expectationTransientSize = serviceTransientSize
    }

goodObservation :: N.NativeObservation
goodObservation =
  N.NativeObservation
    { N.nativeStatus = N.Returned
    , N.nativeRegisters =
        M.fromList
          ([(N.R0, 3)] ++ serviceSeeds
            ++ [ (N.R1, 0)
               , (N.R2, 0)
               , (N.R3, 0)
               , (N.R12, 0)
               , (N.Sp, serviceStackTop)
               , (N.Lr, 0x009b0000)
               , (N.Cpsr, 0x10)
               , (N.Pc, 0x009b0000)
               ])
    , N.nativeMemory =
        [ (N.RegionId "casebuf", stackBytes)
        , (N.RegionId "mirror", B.replicate serviceTransientSize 0)
        ]
    , N.nativeProvenance =
        N.Complete
          (N.ProvenanceTotal 2)
          [ N.Write (N.GuestAddress 0x009a1fec) (N.WriteBytes 4)
          , N.Write (N.GuestAddress 0x0089edc8) (N.WriteBytes 4)
          ]
    , N.nativeInstructions = N.InstructionCount 100
    , N.nativeEngineError = N.EngineError 0
    }
  where
    stackBytes =
      B.take serviceStackOffset serviceCasebufInitial
        <> B.replicate serviceStackSize 0
        <> B.drop (serviceStackOffset + serviceStackSize) serviceCasebufInitial

parserChecks :: IO ()
parserChecks = do
  let regions = [Region (obj [("id", s "casebuf")]) 0x009a0000 4 (B.pack [0xaa, 0xbb, 0xcc, 0xdd])]
      valid = runnerValue
  check "a valid runner value parses" $
    case N.observeNative ExitSuccess regions valid of
      Right observation -> N.nativeStatus observation == N.Returned
      Left _ -> False
  check "a returned status with a nonzero exit is a typed error" $
    case N.observeNative (ExitFailure 1) regions valid of
      Left N.ReturnedWithNonzeroExit -> True
      _ -> False
  check "a missing status is a typed error" $
    case N.observeNative ExitSuccess regions (set "status" Null runnerValue) of
      Left N.MissingStatus -> True
      _ -> False
  check "a missing register is a typed error" $
    case N.observeNative ExitSuccess regions (set "registers" (registerObject (filter ((/= N.R7) . fst) allRegisters)) runnerValue) of
      Left (N.MissingRegister N.R7) -> True
      _ -> False
  check "malformed memory is a typed error" $
    case N.observeNative ExitSuccess regions (set "memory" (arr [obj [("address", s "0x009a0000"), ("bytes", s "zz")]]) runnerValue) of
      Left (N.MalformedMemoryBytes (N.RegionId "casebuf")) -> True
      _ -> False
  check "a provenance count mismatch is a typed error" $
    case N.observeNative ExitSuccess regions (set "memory_provenance_total" (s "2") runnerValue) of
      Left (N.ProvenanceCountMismatch (N.ProvenanceTotal 2) 1) -> True
      _ -> False
  check "truncated provenance is marked truncated" $
    case N.observeNative ExitSuccess regions (truncatedRunnerValue) of
      Right observation -> case N.nativeProvenance observation of
        N.Truncated (N.ProvenanceTotal 4097) -> True
        _ -> False
      Left _ -> False

allRegisters :: [(N.Register, Word32)]
allRegisters =
  [ (N.R0, 3)
  , (N.R1, 0)
  , (N.R2, 0)
  , (N.R3, 0)
  ]
    ++ serviceSeeds
    ++ [ (N.R12, 0)
       , (N.Sp, serviceStackTop)
       , (N.Lr, 0x009b0000)
       , (N.Cpsr, 0x10)
       , (N.Pc, 0x009b0000)
       ]

runnerValue :: Value
runnerValue =
  obj
    [ ("status", s "returned")
    , ("registers", registerObject allRegisters)
    , ("memory", arr [obj [("address", s "0x009a0000"), ("bytes", s "aabbccdd")]])
    , ("memory_provenance_total", s "1")
    , ("memory_provenance", arr [provenanceEntry 0x009a0000])
    , ("instructions", s "10")
    , ("unicorn_error", n 0)
    ]

truncatedRunnerValue :: Value
truncatedRunnerValue =
  set
    "memory_provenance_total"
    (s "4097")
    (set "memory_provenance" (arr (replicate 4096 (provenanceEntry 0x009a0000))) runnerValue)

registerObject :: [(N.Register, Word32)] -> Value
registerObject =
  object
    . map (\(register, value) -> fromString (N.registerLabel register) .= s (N.hexWord32 value))

provenanceEntry :: Word64 -> Value
provenanceEntry address =
  object
    [ ("kind", s "write")
    , ("address", s (hexWord64 address))
    , ("size", n 4)
    ]

hexWord64 :: Word64 -> String
hexWord64 value = "0x" ++ replicate (8 - length text) '0' ++ text
  where
    digits = "0123456789abcdef"
    text = reverse (digitsAt value)
    digitsAt 0 = []
    digitsAt number = digits !! fromIntegral (number `mod` 16) : digitsAt (number `div` 16)

data ArmEffect = StoreThenRestore | NoStores
  deriving (Eq, Show)

armEffectChecks :: FilePath -> IO ()
armEffectChecks runner = do
  (mutantResult, mutantObservation) <- armEffectCase runner StoreThenRestore
  check "the ARM effect mutant returns with the expected ABI" $
    requiredAbi mutantObservation
  check "the ARM effect mutant leaves casebuf and mirror unchanged" $
    unchangedCasebufAndMirror mutantObservation
  check "the ARM effect mutant supplies complete provenance" $
    completeProvenance mutantObservation
  check "the ARM effect mutant records both writes at casebuf+0x100" $
    effectWrites mutantObservation == Just (replicate 2 (N.Write (N.GuestAddress 0x009a0100) (N.WriteBytes 4)))
  check "the ARM effect mutant fails qualification" $
    str (get mutantResult "status") == "failed"
  (controlResult, controlObservation) <- armEffectCase runner NoStores
  check "the ARM effect no-write control returns with the expected ABI" $
    requiredAbi controlObservation
  check "the ARM effect no-write control leaves casebuf and mirror unchanged" $
    unchangedCasebufAndMirror controlObservation
  check "the ARM effect no-write control supplies complete provenance" $
    completeProvenance controlObservation
  check "the ARM effect no-write control writes nothing at casebuf+0x100" $
    effectWrites controlObservation == Just []
  check "the ARM effect no-write control qualifies" $
    str (get controlResult "status") == "passed"

requiredAbi :: N.NativeObservation -> Bool
requiredAbi observation =
  N.nativeStatus observation == N.Returned
    && all (\(register, value) -> M.lookup register (N.nativeRegisters observation) == Just value) expectedEffectRegisters

unchangedCasebufAndMirror :: N.NativeObservation -> Bool
unchangedCasebufAndMirror observation =
  lookup (N.RegionId "casebuf") (N.nativeMemory observation) == Just (B.replicate serviceCasebufSize 0)
    && lookup (N.RegionId "mirror") (N.nativeMemory observation) == Just (B.replicate serviceTransientSize 0)

completeProvenance :: N.NativeObservation -> Bool
completeProvenance observation = case N.nativeProvenance observation of
  N.Complete _ _ -> True
  N.Truncated _ -> False

effectWrites :: N.NativeObservation -> Maybe [N.Write]
effectWrites = N.provenanceWrites . N.nativeProvenance

expectedEffectRegisters :: [(N.Register, Word32)]
expectedEffectRegisters = (N.R0, 0) : serviceSeeds ++ [(N.Sp, serviceStackTop), (N.Pc, 0x00801000)]

armEffectCase :: FilePath -> ArmEffect -> IO (Value, N.NativeObservation)
armEffectCase runner effect = withTemporary "service-effect-mutant" $ \directory -> do
  let code = B.concat (map word32LE (effectCode effect))
      casebuf = B.replicate serviceCasebufSize 0
      mirror = B.replicate serviceTransientSize 0
      exitBytes = word32LE 0xe12fff1e
      artifacts =
        [ ("code", "code.bin", code)
        , ("casebuf", "casebuf.bin", casebuf)
        , ("mirror", "mirror.bin", mirror)
        , ("exit", "exit.bin", exitBytes)
        ]
  forM_ artifacts $ \(_, name, bytes) -> B.writeFile (directory </> name) bytes
  let initialRegisters =
        [(N.R0, 0), (N.R1, 0), (N.R2, 0), (N.R3, 0)]
          ++ serviceSeeds
          ++ [ (N.R12, 0)
             , (N.Sp, serviceStackTop)
             , (N.Lr, 0x00801000)
             , (N.Cpsr, 0x10)
             ]
      caseValue =
        obj
          [ ("schema_version", n 1)
          , ("id", s (effectId effect))
          , ("origin", s "synthetic")
          , ("provenance", s (effectDescription effect))
          , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
          , ("artifacts", arr [artifact artifactId path bytes | (artifactId, path, bytes) <- artifacts])
          , ("entry", s "0x00800000")
          , ("exit", s "0x00801000")
          , ("registers", registerObject initialRegisters)
          , ( "regions"
            , arr
                [ region "code" 0x00800000 28 "rx" "code.bin"
                , region "casebuf" 0x009a0000 serviceCasebufSize "rw" "casebuf.bin"
                , region "mirror" 0x0089ec00 serviceTransientSize "rw" "mirror.bin"
                , region "exit" 0x00801000 4 "rx" "exit.bin"
                ]
            )
          , ("functions", arr [obj [("address", s "0x00800000"), ("size", n 28)]])
          , ("stubs", arr [])
          , ("limits", obj [("instructions", n 1000), ("wall_ms", n 1000)])
          , ( "expected"
            , obj
                [ ("status", s "returned")
                , ("registers", registerObject expectedEffectRegisters)
                , ("memory", arr [])
                , ("events", arr [])
                ]
            )
          , ("exclusions", arr [s (effectDescription effect)])
          ]
  let path = directory </> "case.json"
  BL.writeFile path (encode caseValue)
  (_, regions, _, _) <- loadCase path
  (report, result) <- runServiceCase runner path
  observation <- either (ioError . userError . N.renderObservationError) pure (N.observeNative ExitSuccess regions (get report "runner"))
  pure (result, observation)

effectCode :: ArmEffect -> [Word32]
effectCode StoreThenRestore = [0xe3001000, 0xe340109a, 0xe5910100, 0xe2802001, 0xe5812100, 0xe5810100, 0xe12fff1e]
effectCode NoStores = [0xe3001000, 0xe340109a, 0xe5910100, 0xe2802001, 0xe1a00000, 0xe1a00000, 0xe12fff1e]

effectId :: ArmEffect -> String
effectId StoreThenRestore = "service-effect-mutant"
effectId NoStores = "service-effect-no-write"

effectDescription :: ArmEffect -> String
effectDescription StoreThenRestore = "test-only ARM effect mutant: writes casebuf+0x100 and restores it"
effectDescription NoStores = "test-only ARM effect control: writes nothing"

artifact :: String -> String -> B.ByteString -> Value
artifact artifactId path bytes =
  obj
    [ ("id", s artifactId)
    , ("path", s path)
    , ("size", n (fromIntegral (B.length bytes)))
    , ("sha256", s (sha256Hex (sha256 bytes)))
    ]

region :: String -> Word32 -> Int -> String -> String -> Value
region regionId address size permissions path =
  obj
    [ ("id", s regionId)
    , ("address", s (N.hexWord32 address))
    , ("size", n (fromIntegral size))
    , ("permissions", s permissions)
    , ( "initial"
      , obj
          [ ("artifact", s (takeWhile (/= '.') path))
          , ("offset", n 0)
          , ("length", n (fromIntegral size))
          ]
      )
    ]

word32LE :: Word32 -> B.ByteString
word32LE value =
  B.pack
    [ fromIntegral value
    , fromIntegral (value `shiftR` 8)
    , fromIntegral (value `shiftR` 16)
    , fromIntegral (value `shiftR` 24)
    ]
