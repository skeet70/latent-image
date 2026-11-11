{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Synthetic tests for the stock-side coverage fold: no runner execution and
-- no firmware. The bitmaps and stock bytes are constructed here.
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), eitherDecodeStrict')
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR)
import Data.List (isInfixOf)
import Data.Word (Word32)
import Emulation.Coverage
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (get, num)
import Numeric (readHex)
import System.Exit (exitFailure)

main :: IO ()
main = do
  -- Census shapes.
  let census = BC.pack ("{\"entries\":[{\"entry\":\"0x006c1858\",\"extent\":24,\"stock\":{\"sha256\":\"" ++ digestText ++ "\"}}]}")
  extents <- either fail pure (parseExtents census)
  check "the census entry is read" (map extentEntry extents == [0x006c1858])
  check "the census extent is read" (map extentBytes extents == [24])
  check "the census digest is read" (map extentStock extents == [Just digestText])
  check "a bare array is read" (either (const False) (== [Extent 0x1000 8 Nothing]) (parseExtents "[{\"base\":\"0x1000\",\"bytes\":8}]"))

  -- A judged result's nested runner observation.
  let judged = "{\"status\":\"failed\",\"runner\":{\"executed\":[{\"base\":\"0x00001000\",\"bytes\":\"12\",\"executed\":\"07\",\"condition_passed\":\"07\",\"condition_failed\":\"00\"}],\"executed_unobserved\":[],\"memory\":[{\"address\":\"0x00001000\",\"bytes\":\"010000e0020000e01eff2fe1\"}]}}"
  observation <- either fail pure (parseObservation judged)
  check "the nested extent row is read" (length (observationExtents observation) == 1)
  check "the extent row base is read" (map extentObservationBase (observationExtents observation) == [0x1000])
  check "the extent row size is read" (map extentObservationBytes (observationExtents observation) == [12])
  check "the bitmaps are read" (map extentObservationExecuted (observationExtents observation) == [B.pack [7]])
  check "the memory is read" (map fst (observationMemory observation) == [0x1000])

  -- A straight-line function: every word executed, no branch, nothing left.
  let (straight, straightBytes) = extentAt 0x1000 [0xe2800001, 0xe2800001, 0xe12fff1e]
      straightObservation = observationOf 0x1000 12 "07" "07" "00" straightBytes
      straightCoverage = only (coverageFunctions (coverage [straight] [straightObservation]))
  check "a straight-line extent is authenticated" (functionAuthenticated straightCoverage)
  check "a straight-line extent records the case" (functionDeclaredCases straightCoverage == 1)
  check "a straight-line extent has no conditional branch" (null (functionBranches straightCoverage))
  check "a straight-line extent leaves no instruction unexecuted" (null (functionUnexecuted straightCoverage))

  -- Both directions of one branch across two cases.
  let (branchExtent, branchBytes) = extentAt 0x1000 [0xe3a01002, 0xe2511001, 0x1afffffd, 0xe12fff1e]
      passedCase = observationOf 0x1000 16 "0f" "0f" "00" branchBytes
      failedCase = observationOf 0x1000 16 "0f" "0b" "04" branchBytes
      bothCoverage = only (coverageFunctions (coverage [branchExtent] [passedCase, failedCase]))
  check "the conditional branch is found" (map branchAddress (functionBranches bothCoverage) == [0x1008])
  check "the conditional branch names its condition" (map branchCondition (functionBranches bothCoverage) == ["ne"])
  check "two cases reach both directions" (map branchDirection (functionBranches bothCoverage) == [Both])
  check "both directions count two declared cases" (functionDeclaredCases bothCoverage == 2)

  -- One direction and none.
  let oneCoverage = only (coverageFunctions (coverage [branchExtent] [passedCase]))
      neverCoverage = only (coverageFunctions (coverage [branchExtent] [observationOf 0x1000 16 "03" "03" "00" branchBytes]))
  check "one case reaches one direction" (map branchDirection (functionBranches oneCoverage) == [PassedOnly])
  check "an unentered branch is never" (map branchDirection (functionBranches neverCoverage) == [Never])
  check "an unentered branch leaves the rest unexecuted" (map fst (functionUnexecuted neverCoverage) == [0x1008, 0x100c])

  -- A report whose bytes differ from the census contributes nothing.
  let foreignCase = observationOf 0x1000 16 "0f" "0f" "00" (B.replicate 16 0xff)
      foreignCoverage = only (coverageFunctions (coverage [branchExtent] [foreignCase]))
  check "foreign bytes are not accepted" (functionDeclaredCases foreignCoverage == 0)
  check "foreign bytes are counted" (functionForeignCases foreignCoverage == 1)
  check "foreign bytes do not authenticate" (not (functionAuthenticated foreignCoverage))

  -- An extent the runner reported without bitmaps is counted as unobserved.
  let unobserved = Observation [] [(0x1000, 16)] []
      unobservedCoverage = only (coverageFunctions (coverage [branchExtent] [unobserved]))
  check "an unobserved extent is counted" (functionUnobservedCases unobservedCoverage == 1)

  -- A census extent inside a larger declared row is read at its offset.
  let (inner, innerBytes) = extentAt 0x1000 [0xe2800001, 0xe2800001, 0xe2800001, 0xe12fff1e]
      outer = Observation
        [ ExtentObservation 0x0ff0 32 (hexBytes "f0") (hexBytes "f0") (hexBytes "00") ]
        []
        [(0x0ff0, B.replicate 16 0x00 <> innerBytes)]
      innerCoverage = only (coverageFunctions (coverage [inner] [outer]))
  check "an extent inside a declared row is declared" (functionDeclaredCases innerCoverage == 1)
  check "an extent inside a declared row is authenticated" (functionAuthenticated innerCoverage)
  check "an extent inside a declared row finds every executed word" (null (functionUnexecuted innerCoverage))

  -- Branch classification.
  check "bne is a conditional branch" (fmap fst (conditionalBranch 0x1afffffd) == Just 1)
  check "an unconditional branch is not" (conditionalBranch 0xeafffffe == Nothing)
  check "an unconditional bx is not" (conditionalBranch 0xe12fff1e == Nothing)
  check "a conditional bx is" (fmap fst (conditionalBranch 0x112fff1e) == Just 1)
  check "a conditional ldr pc is" (fmap fst (conditionalBranch 0x059ff000) == Just 0)
  check "an unconditional ldr pc is not" (conditionalBranch 0xe59ff000 == Nothing)

  -- JSON and table carry the result.
  let json = eitherDecodeStrict' (BL.toStrict (coverageJson (coverage [branchExtent] [passedCase, failedCase])))
  case json of
    Right value -> do
      check "the JSON totals count the branch" (num (get (get value "totals") "branches") == 1)
      check "the JSON totals count both directions" (num (get (get value "totals") "both") == 1)
      check "the JSON names the function" (num (get (only (functionsOf value)) "bytes") == 16)
    Left problem -> check ("the JSON decodes: " ++ problem) False
  check "the table prints a branch direction" ("both" `isInfixOf` coverageTable (coverage [branchExtent] [passedCase, failedCase]))

  putStrLn "coverage synthetic tests passed"

functionsOf :: Value -> [Value]
functionsOf value = case get value "functions" of
  Array entries -> foldr (:) [] entries
  _ -> []

only :: [a] -> a
only (value : _) = value
only [] = error "synthetic coverage expected one element"

check :: String -> Bool -> IO ()
check label yes = unless yes (putStrLn ("FAIL: " ++ label) >> exitFailure)

digestText :: String
digestText = "aab533f64e7d3f50628f3b74c122bcfaae90f7a06e1282e0e04e4d03d9706543"

extentAt :: Word32 -> [Word32] -> (Extent, B.ByteString)
extentAt entry words_ =
  let bytes = wordsBytes words_
   in (Extent entry (fromIntegral (B.length bytes)) (Just (sha256Hex (sha256 bytes))), bytes)

observationOf :: Word32 -> Word32 -> String -> String -> String -> B.ByteString -> Observation
observationOf base size executed passed failed bytes = Observation
  { observationExtents =
      [ ExtentObservation
          { extentObservationBase = base
          , extentObservationBytes = size
          , extentObservationExecuted = hexBytes executed
          , extentObservationPassed = hexBytes passed
          , extentObservationFailed = hexBytes failed
          }
      ]
  , observationUnobserved = []
  , observationMemory = [(base, bytes)]
  }

hexBytes :: String -> B.ByteString
hexBytes text = B.pack (go text)
  where
    go [] = []
    go (high : low : rest) = case readHex [high, low] of
      [(byte, "")] -> fromIntegral (byte :: Integer) : go rest
      _ -> error "invalid synthetic bitmap"
    go _ = error "invalid synthetic bitmap"

wordsBytes :: [Word32] -> B.ByteString
wordsBytes = B.pack . concatMap bytes
  where
    bytes word = [fromIntegral word, fromIntegral (word `shiftR` 8), fromIntegral (word `shiftR` 16), fromIntegral (word `shiftR` 24)]
