{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_)
import Data.Aeson (Value (..), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR)
import qualified Data.Map.Strict as M
import Data.String (fromString)
import Data.Word (Word32, Word64)
import Emulation.NativeCase (Region (..))
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import Replacement.Service.Qualify
import System.Directory (createDirectoryIfMissing, doesDirectoryExist, getTemporaryDirectory, removeDirectoryRecursive)
import System.Environment (getArgs, lookupEnv)
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
  consumerChecks
  parserChecks
  armEffectMutant runner
  putStrLn "service qualification tests: passed"

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

armEffectMutant :: FilePath -> IO ()
armEffectMutant runner = do
  base <- maybe getTemporaryDirectory pure =<< lookupEnv "TEST_TMPDIR"
  let directory = base </> "service-effect-mutant"
  exists <- doesDirectoryExist directory
  if exists then removeDirectoryRecursive directory else pure ()
  createDirectoryIfMissing True directory
  let code = B.pack (concatMap word32LE [0xe3001000, 0xe340109a, 0xe5910100, 0xe2802001, 0xe5812100, 0xe5810100, 0xe12fff1e])
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
      expectedRegisters =
        (N.R0, 0) : serviceSeeds ++ [(N.Sp, serviceStackTop), (N.Pc, 0x00801000)]
      caseValue =
        obj
          [ ("schema_version", n 1)
          , ("id", s "service-effect-mutant")
          , ("origin", s "synthetic")
          , ("provenance", s "test-only ARM effect mutant: writes casebuf+0x100 and restores it")
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
                , ("registers", registerObject expectedRegisters)
                , ("memory", arr [])
                , ("events", arr [])
                ]
            )
          , ("exclusions", arr [s "test-only ARM effect mutant"])
          ]
  BL.writeFile (directory </> "case.json") (encode caseValue)
  (_, result) <- runServiceCase runner (directory </> "case.json")
  check "a real ARM effect mutant is refused" $
    str (get result "status") == "failed" && not (null (list (get result "mismatches")))
  removeDirectoryRecursive directory

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
