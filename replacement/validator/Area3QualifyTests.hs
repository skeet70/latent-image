{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main where

import Control.Monad (forM_)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.List (find)
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import Replacement.Area3.Domain (Record, buildRecord, recordBytes)
import System.Directory (createDirectory)
import System.Environment (getArgs, setEnv, unsetEnv)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [qualify, candidate, faulty, fakeCandidate, fakeRunner, armImage, armSymbols, runner] ->
      withTemporary "area3-tests" $ \root -> do
        let anchorDir = root </> "anchors"
        createDirectory anchorDir
        record <- writeFixture anchorDir
        writeContract root record
        single <- runCommand qualify (common root anchorDir candidate ++ ["--mutations", "single"])
        ensure (statusOf single == "partial") ("logic single: " ++ showValue single)
        ensure (count single "single_bit" "enumerated" == 352) "single-bit enumeration"
        ensure (count single "two_bit" "enumerated" == 0) "single selection excludes two-bit"
        ensure (count single "ground" "enumerated" == 56) "ground case count"
        ensure (not (boolOf (field single ["capability"]))) "logic scope is not capability"
        forM_ (arrayOf (field single ["refusals", "probes"])) $ \probe ->
          ensure (stringOf (get probe "observed") == stringOf (get probe "expected")) "refusal probe matched"
        allMutations <- runCommand qualify (common root anchorDir candidate ++ ["--faulty", faulty])
        ensure (statusOf allMutations == "partial") ("logic all: " ++ showValue allMutations)
        ensure (count allMutations "two_bit" "enumerated" == 61776) "two-bit enumeration"
        ensure (count allMutations "two_bit" "candidate_matched" == 61776) "two-bit candidate coverage"
        ensure (count allMutations "single_bit" "candidate_matched" == 352) "single-bit candidate coverage"
        ensure (stringOf (field allMutations ["faulty", "status"]) == "detected") "faulty candidate detected"
        blocked <- runCommand qualify (common root anchorDir candidate ++ ["--scope", "capability", "--mutations", "single", "--stock", "none"])
        ensure (statusOf blocked == "blocked") ("capability without runner: " ++ showValue blocked)
        ensure (count blocked "single_bit" "stock_passed" == 0) "stock did not run"
        ensure (count blocked "single_bit" "arm_passed" == 352) "declared ARM candidate ran"
        partial <- runCommand qualify (["--contract", root </> "contract.json", "--case-dir", anchorDir, "--candidate", candidate, "--runner", runner, "--arm-image", armImage, "--arm-symbols", armSymbols, "--arm-base", "0x00800000", "--scope", "capability", "--mutations", "single", "--stock", "none"])
        ensure (statusOf partial == "blocked") ("partial capability: " ++ showValue partial)
        ensure (not (boolOf (field partial ["capability"]))) "partial invocation is not capability"
        ensure (not (null (arrayOf (field partial ["coverage_gaps"])))) "partial coverage gaps named"
        ensure (count partial "single_bit" "stock_passed" == 0) "partial stock not executed"
        arm <- runCommand qualify (common root anchorDir candidate ++ ["--runner", runner, "--arm-image", armImage, "--arm-symbols", armSymbols, "--arm-base", "0x00800000", "--mutations", "single"])
        ensure (statusOf arm == "partial") ("ARM candidate: " ++ showValue arm)
        ensure (stringOf (field arm ["arm", "status"]) == "passed") "ARM candidate status"
        ensure (count arm "single_bit" "arm_passed" == 352) "ARM single-bit coverage"
        ensure (count arm "ground" "arm_passed" == 56) "ARM ground-case coverage"
        let counterexample = field allMutations ["faulty", "counterexample"]
        ensure (countValue counterexample ["final_bits"] == 2) ("reduced witness bits: " ++ showValue counterexample)
        ensure (countValue counterexample ["initial_bits"] > countValue counterexample ["final_bits"]) "reduction shrank the witness"
        forM_ (arrayOf (field counterexample ["differences"])) $ \difference ->
          ensure (num (get difference "byte") < 44) "witness stays inside the protected bytes"
        let witness = stringOf (field counterexample ["record"])
        candidateToken <- adapterOutcome candidate witness
        ensure (candidateToken == "rejected") "candidate rejects the reduced witness"
        faultyToken <- adapterOutcome faulty witness
        ensure (faultyToken == "accepted") "faulty candidate accepts the reduced witness"
        faultyCandidate <- runCommand qualify (["--contract", root </> "contract.json", "--case-dir", anchorDir, "--candidate", faulty, "--scope", "logic", "--mutations", "single", "--stock", "none"])
        ensure (statusOf faultyCandidate == "failed") ("faulty candidate as candidate: " ++ showValue faultyCandidate)
        ensure (count faultyCandidate "ground" "candidate_matched" < count faultyCandidate "ground" "enumerated") "faulty candidate mismatches ground cases"
        malformed <- runCommand qualify (["--contract", root </> "contract-invalid.json", "--case-dir", anchorDir, "--candidate", candidate, "--scope", "logic", "--stock", "none"])
        ensure (statusOf malformed == "refused") ("invalid contract: " ++ showValue malformed)
        omitted <- runCommand qualify (["--contract", root </> "contract.json", "--case-dir", anchorDir, "--candidate", fakeCandidate, "--scope", "logic", "--mutations", "single", "--stock", "none"])
        ensure (statusOf omitted == "failed") ("omitted refusal reply: " ++ showValue omitted)
        let badVerb =
              fromMaybe Null
                (find (\probe -> stringOf (get probe "expected") == "bad_verb") (arrayOf (field omitted ["refusals", "probes"])))
        ensure (stringOf (get badVerb "observed") /= "bad_verb") "omitted refusal reply detected"
        let runFakeRunner mode = do
              setEnv "FAKE_RUNNER_MODE" mode
              result <- runCommand qualify (["--contract", root </> "contract.json", "--case-dir", anchorDir, "--candidate", candidate, "--runner", fakeRunner, "--arm-image", armImage, "--arm-symbols", armSymbols, "--arm-base", "0x00800000", "--scope", "logic", "--mutations", "single", "--stock", "none", "--limit", "1"])
              unsetEnv "FAKE_RUNNER_MODE"
              pure result
        nonzeroExit <- runFakeRunner "nonzero-exit-returned"
        ensure (statusOf nonzeroExit == "blocked" && not (boolOf (field nonzeroExit ["capability"]))) "nonzero exit is not a pass"
        ensure (countValue nonzeroExit ["arm", "passed"] == 0) "nonzero exit ARM case not passed"
        nonzeroUnicorn <- runFakeRunner "returned-nonzero-unicorn"
        ensure (statusOf nonzeroUnicorn == "blocked") "nonzero unicorn is not a pass"
        negativeCounter <- runFakeRunner "negative-counter"
        ensure (statusOf negativeCounter == "blocked") "negative counter is not a pass"
        fractional <- runFakeRunner "fractional-unicorn"
        ensure (statusOf fractional == "blocked") "fractional numeric is not a pass"
        pure ()
    _ -> failH "usage: area3-qualify-tests QUALIFY CANDIDATE FAULTY"

common :: FilePath -> FilePath -> FilePath -> [String]
common root anchorDir candidate =
  [ "--contract", root </> "contract.json"
  , "--case-dir", anchorDir
  , "--candidate", candidate
  , "--scope", "logic"
  , "--stock", "none"
  ]

statusOf :: Value -> String
statusOf value = stringOf (get value "status")

boolOf :: Value -> Bool
boolOf (Bool value) = value
boolOf _ = False

stringOf :: Value -> String
stringOf = str

arrayOf :: Value -> [Value]
arrayOf = list

field :: Value -> [String] -> Value
field = foldl get

count :: Value -> String -> String -> Integer
count report section key = num (get (get (get report "coverage") section) key)

countValue :: Value -> [String] -> Integer
countValue value path = num (field value path)

showValue :: Value -> String
showValue = BC.unpack . encodeCanonical

writeFixture :: FilePath -> IO Record
writeFixture directory = do
  let body = B.pack [fromIntegral (index * 7 + 3) | index <- [0 .. 43 :: Int]]
  record <- either failH pure (buildRecord body)
  let code = B.replicate 16 0
      spec = littleEndian 44
      recordFile = recordBytes record
      codeDigest = sha256Hex (sha256 code)
      specDigest = sha256Hex (sha256 spec)
      recordDigest = sha256Hex (sha256 recordFile)
      hex :: Word32 -> String
      hex value = "0x" ++ pad 8 (showHex value "")
      artifact :: String -> String -> B.ByteString -> String -> Value
      artifact ident path bytes digest =
        obj [("id", s ident), ("path", s path), ("size", n (fromIntegral (B.length bytes))), ("sha256", s digest)]
      region :: String -> Word32 -> Int -> String -> Value -> Value
      region ident address size permissions initial =
        obj [("id", s ident), ("address", s (hex address)), ("size", n (fromIntegral size)), ("permissions", s permissions), ("initial", initial)]
      zero = obj [("zero", b True)]
      caseValue =
        obj
          [ ("schema_version", n 1)
          , ("id", s "anchor")
          , ("origin", s "synthetic")
          , ("provenance", s "Synthetic area-3 qualification fixture: constructed valid record and dummy code region; code never executes in logic scope.")
          , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
          , ( "artifacts"
            , arr
                [ artifact "test-code" "test-code.bin" code codeDigest
                , artifact "spec" "spec.bin" spec specDigest
                , artifact "record-accept" "record-accept.bin" recordFile recordDigest
                ]
            )
          , ("entry", s (hex 0x1000))
          , ("exit", s (hex 0x0800))
          , ( "registers"
            , obj
                [ ("r0", s (hex 0x2000))
                , ("r1", s (hex 0x3000))
                , ("r2", s (hex 0))
                , ("r3", s (hex 0))
                , ("r4", s (hex 0))
                , ("r5", s (hex 0))
                , ("r6", s (hex 0))
                , ("r7", s (hex 0))
                , ("r8", s (hex 0))
                , ("r9", s (hex 0))
                , ("r10", s (hex 0))
                , ("r11", s (hex 0))
                , ("r12", s (hex 0))
                , ("sp", s (hex 0x4100))
                , ("lr", s (hex 0x0800))
                , ("cpsr", s (hex 0x10))
                ]
            )
          , ( "regions"
            , arr
                [ region "test-code" 0x1000 16 "rx" (obj [("artifact", s "test-code"), ("offset", n 0), ("length", n 16)])
                , region "record" 0x2000 52 "rw" (obj [("artifact", s "record-accept"), ("offset", n 0), ("length", n 52)])
                , region "spec" 0x3000 4 "rw" (obj [("artifact", s "spec"), ("offset", n 0), ("length", n 4)])
                , region "stack" 0x4000 256 "rw" zero
                , region "exit" 0x0800 4 "rx" zero
                ]
            )
          , ("functions", arr [obj [("address", s (hex 0x1000)), ("size", n 8)]])
          , ("stubs", arr [])
          , ("limits", obj [("instructions", n 20000), ("wall_ms", n 2000)])
          , ( "expected"
            , obj
                [ ("status", s "returned")
                , ( "registers"
                  , obj
                      [ ("r0", s (hex 0))
                      , ("r4", s (hex 0))
                      , ("r5", s (hex 0))
                      , ("r6", s (hex 0))
                      , ("r7", s (hex 0))
                      , ("r8", s (hex 0))
                      , ("r9", s (hex 0))
                      , ("r10", s (hex 0))
                      , ("r11", s (hex 0))
                      , ("sp", s (hex 0x4100))
                      , ("pc", s (hex 0x0800))
                      ]
                  )
                , ("memory", arr [])
                , ("events", arr [])
                ]
            )
          , ("exclusions", arr [s "Synthetic fixture: code bytes never execute in logic scope"])
          ]
  B.writeFile (directory </> "anchor.json") (encodeCanonical caseValue)
  B.writeFile (directory </> "test-code.bin") code
  B.writeFile (directory </> "spec.bin") spec
  B.writeFile (directory </> "record-accept.bin") recordFile
  pure record

writeContract :: FilePath -> Record -> IO ()
writeContract directory record = do
  let code = B.replicate 16 0
      spec = littleEndian 44
      codeDigest = sha256Hex (sha256 code)
      specDigest = sha256Hex (sha256 spec)
      recordDigest = sha256Hex (sha256 (recordBytes record))
      contract =
        obj
          [ ("schema_version", n 1)
          , ("id", s "area3-test-contract")
          , ("target", obj [("body", s "X-M1"), ("firmware", s "1.50"), ("image", n 4), ("entry", s "0x00001000"), ("size", n 8), ("mode", s "compatibility")])
          , ( "abi"
            , obj
                [ ("convention", s "aapcs32")
                , ("record", s "r0")
                , ("protected_length", s "r1")
                , ("result", s "r0")
                , ("accept", s "0x00000000")
                , ("reject", s "0x00000001")
                , ("return", s "lr")
                , ("preserved", arr (map s ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "sp"]))
                ]
            )
          , ( "input"
            , obj
                [ ("record_size", n 52)
                , ("protected_size", n 44)
                , ("protected_length", s "0x0000002c")
                , ("row_count", n 11)
                , ("column_count", n 32)
                , ("row_parity_offset", n 44)
                , ("column_parity_offset", n 48)
                , ("row_parity_bits", n 11)
                , ("row_parity_high_bits", s "zero")
                ]
            )
          , ( "outputs"
            , obj
                [ ("accept", s "0x00000000")
                , ("reject", s "0x00000001")
                , ("classes", arr (map s ["accepted", "corrected", "rejected"]))
                , ("refusals", arr (map s ["bad_request", "bad_verb", "bad_record_hex", "bad_length_field", "unsupported_length", "row_parity_high_bits"]))
                ]
            )
          , ( "effects"
            , obj
                [ ("record_write", s "corrected bit only")
                , ("max_record_write_bytes", n 1)
                , ("scratch_regions", arr [s "stack"])
                , ("forbidden_regions", arr (map s ["test-code", "spec", "exit"]))
                , ("persistent_changes", s "none")
                ]
            )
          , ("engine", obj [("cpu", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
          , ( "dependencies"
            , arr
                [ obj [("id", s "test-code"), ("size", n 16), ("sha256", s codeDigest)]
                , obj [("id", s "spec"), ("size", n 4), ("sha256", s specDigest)]
                , obj [("id", s "record-accept"), ("size", n 52), ("sha256", s recordDigest)]
                ]
            )
          , ("functions", arr [obj [("address", s "0x00001000"), ("size", n 8)]])
          , ("budgets", obj [("instructions", n 20000), ("wall_ms", n 2000)])
          , ("anchors", arr [obj [("case", s "anchor"), ("expected", s "accepted")]])
          , ("coverage", obj [("record_artifact", s "record-accept"), ("protected_bits", n 352), ("single_bit", n 352), ("two_bit", n 61776)])
          , ("exclusions", arr [s "Synthetic fixture contract"])
          ]
      invalid = set "input" (set "record_size" (n 51) (get contract "input")) contract
  B.writeFile (directory </> "contract.json") (encodeCanonical contract)
  B.writeFile (directory </> "contract-invalid.json") (encodeCanonical invalid)

littleEndian :: Word32 -> B.ByteString
littleEndian value =
  B.pack [fromIntegral ((value `div` (256 ^ shift)) `mod` 256) | shift <- [0, 1, 2, 3 :: Int]]

runCommand :: FilePath -> [String] -> IO Value
runCommand executable arguments = do
  (timedOut, exit, out, err) <- runBounded 120000000 (16 * 1024 * 1024) executable arguments B.empty
  ensure (not timedOut) "command timed out"
  case exit of
    ExitSuccess -> jsonRead out
    ExitFailure code -> do
      value <- jsonRead out
      ensure (get value "status" /= Null) ("command exit " ++ show code ++ ": " ++ BC.unpack err)
      pure value

adapterOutcome :: FilePath -> String -> IO String
adapterOutcome adapter hexText = do
  (timedOut, exit, out, err) <- runBounded 30000000 (1024 * 1024) adapter [] (BC.pack ("validate " ++ hexText ++ " 0000002c\n"))
  ensure (not timedOut && exit == ExitSuccess) ("adapter failed: " ++ BC.unpack err)
  case words (BC.unpack out) of
    (token : _) -> pure token
    [] -> failH "adapter reply missing"

pad :: Int -> String -> String
pad width text = replicate (max 0 (width - length text)) '0' ++ text
