{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Synthetic tests for the suite audit rule. They exercise the audit without
-- a suite, an image, or a native run.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Emulation.SuiteAudit
import System.Exit (exitFailure)

main :: IO ()
main = do
  let firmware = B.pack [0 .. 255]
  -- A suite that enters every bound entry passes.
  let every = suite "every" [0x10, 0x20]
        [row "a" 0x10 StockImage, row "b" 0x20 StockImage]
  check "every bound entry entered" (auditStatus (run firmware every []) == AuditPassed)
  -- A suite that misses an entry fails and names it.
  let misses = suite "misses" [0x10, 0x20] [row "a" 0x10 StockImage]
      missed = auditOneSuite (Just ("image.bin", firmware)) misses [MissingEntry 0x10 4 "entry_10", MissingEntry 0x20 4 "entry_20"] [row "a" 0x10 StockImage] []
  check "missing entry fails" (auditStatus missed == AuditFailed)
  check "missing entry is named" (map missingAddress (auditUndeclaredMissing missed) == [0x20])
  check "missing entry keeps its name" (map missingName (auditUndeclaredMissing missed) == ["entry_20"])
  -- A stock mapping that differs from the firmware fails and names the entry.
  let wrong = row "stub" 0x10 (StockLiteral (B.pack [9, 9, 9, 9]))
      stub = auditOneSuite (Just ("image.bin", firmware)) (suite "stub" [0x10] [wrong]) [MissingEntry 0x10 4 "entry_10"] [wrong] []
  check "stubbed stock fails" (auditStatus stub == AuditFailed)
  check "stubbed stock names the entry" (map unauthAddress (auditUndeclaredUnauthenticated stub) == [0x10])
  check "stubbed stock reports both byte strings" (case auditUndeclaredUnauthenticated stub of
    [bad] -> unauthMapped bad == "10111213" && unauthExpected bad == "09090909"
    _ -> False)
  -- A digest mapping equal to the firmware passes; a different one fails.
  let digest = sha256Hex (sha256 (B.take 4 (B.drop 0x10 firmware)))
      goodDigest = row "digest" 0x10 (StockDigest digest)
      badDigest = row "digest" 0x10 (StockDigest (sha256Hex (sha256 (B.pack [1, 2, 3, 4]))))
  check "matching stock digest passes"
    (auditStatus (auditOneSuite (Just ("image.bin", firmware)) (suite "digest" [0x10] [goodDigest]) [MissingEntry 0x10 4 "entry_10"] [goodDigest] []) == AuditPassed)
  check "different stock digest fails"
    (auditStatus (auditOneSuite (Just ("image.bin", firmware)) (suite "digest" [0x10] [badDigest]) [MissingEntry 0x10 4 "entry_10"] [badDigest] []) == AuditFailed)
  -- Without firmware, byte authentication is not run and the entry counts stay.
  let noFirmware = auditOneSuite Nothing misses [MissingEntry 0x10 4 "entry_10", MissingEntry 0x20 4 "entry_20"] [row "a" 0x10 StockImage] []
  check "no firmware is not run" (auditAuthStatus noFirmware == AuthNotRun)
  check "no firmware still reports the entered entries" (auditEntered noFirmware == [0x10])
  check "no firmware still reports the missing entry" (map missingAddress (auditUndeclaredMissing noFirmware) == [0x20])
  -- A declared gap is printed and accepted, never silent.
  let gap = Gap 0x20 GapMissing "the suite declares the second entry unentered"
      declared = auditOneSuite (Just ("image.bin", firmware)) misses [MissingEntry 0x10 4 "entry_10", MissingEntry 0x20 4 "entry_20"] [row "a" 0x10 StockImage] [gap]
  check "declared gap passes" (auditStatus declared == AuditPassed)
  check "declared gap is reported" (map gapEntry (auditDeclaredGaps declared) == [0x20])
  -- The manifest and case table parsers are the data path the real audit uses.
  manifest <- either fail pure (parseManifest "{\"schema\":1,\"suites\":[{\"name\":\"s\",\"bound\":{\"kind\":\"declared\",\"source\":\"x\",\"entries\":[{\"address\":\"0x10\",\"size\":\"0x4\"}]},\"cases\":\"cases.json\"}]}")
  check "manifest parses one suite" (length (manifestSuites manifest) == 1)
  cases <- either fail pure (parseCaseTable "s" "{\"schema\":1,\"suite\":\"s\",\"entries\":[{\"case\":\"a\",\"entry\":\"0x10\",\"extent\":\"0x4\",\"stock\":\"image\"}]}")
  check "case table parses one entry" (map caseEntry cases == [0x10])
  putStrLn "suite-audit synthetic tests passed"

check :: String -> Bool -> IO ()
check label yes = unless yes (putStrLn ("FAIL: " ++ label) >> exitFailure)

suite :: String -> [Word32] -> [CaseRow] -> SuiteSpec
suite name bound _rows = SuiteSpec
  { suiteName = name
  , suiteNote = "synthetic"
  , suiteImage = Just (ImageSpec "image.bin" 0 Nothing)
  , suiteBound = BoundSpec BoundDeclared "declared" [] [] Nothing [] [(address, 4, "entry_" ++ show address) | address <- bound] Nothing False
  , suiteCasesPath = "cases.json"
  , suiteCasesSource = "cases.json"
  , suiteCasesDerivedAt = Nothing
  , suiteCasesVerified = False
  , suiteGapsPath = Nothing
  }

run :: B.ByteString -> SuiteSpec -> [Gap] -> SuiteAudit
run firmware spec gaps = auditOneSuite (Just ("image.bin", firmware)) spec bound (map (\address -> row ("case-" ++ show address) address StockImage) (map missingAddress bound)) gaps
  where bound = [MissingEntry address 4 ("entry_" ++ show address) | (address, _, _) <- boundDeclared (suiteBound spec)]

row :: String -> Word32 -> StockMapping -> CaseRow
row name entry stock = CaseRow name entry 4 stock
