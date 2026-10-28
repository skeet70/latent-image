{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main where

import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import Data.List (nub)
import Data.String (fromString)
import Data.Word (Word32, Word64)
import FujiAF.Host
import Replacement.Service.Qualify
import System.Exit (exitFailure)

main :: IO ()
main = do
  rowTable
  consumer
  parser
  putStrLn "service qualification tests: passed"

check :: String -> Bool -> IO ()
check name ok =
  if ok
    then putStrLn ("ok " ++ name)
    else putStrLn ("FAIL " ++ name) >> exitFailure

rowTable :: IO ()
rowTable = do
  let rows = serviceRows
      tone = [row | row <- rows, serviceRowProperty row == 0x110]
      iso = [row | row <- rows, serviceRowProperty row == 0x141]
  check "26 direct rows" (length rows == 26)
  check "five highlight-tone rows" (map serviceRowToken tone == [0 .. 4])
  check "highlight-tone rows are offered" (all ((== Offered) . serviceRowExpected) tone)
  check "21 ISO rows" (length iso == 21)
  check "ISO row 0 is hidden" (maybe False ((== Hidden) . serviceRowExpected) (findRow 0x141 0))
  check "the other ISO rows are offered" (all ((== Offered) . serviceRowExpected) [row | row <- iso, serviceRowToken row /= 0])
  check "row ids are unique" (length (nub (map serviceRowId rows)) == length rows)

findRow :: Word32 -> Word32 -> Maybe ServiceRow
findRow property row =
  case [item | item <- serviceRows, serviceRowProperty item == property, serviceRowToken item == row] of
    (item : _) -> Just item
    [] -> Nothing

consumer :: IO ()
consumer = do
  let expectation = serviceExpectation
      good = goodObservation
      abi = mutateRegister good "r7" 0x7d7d7d7e
      immutable = mutateWrite good 0x009a0100 4
      overflow = mutateWrite good 0x009a1d00 4
      truncated = good {observationWrites = Nothing, observationProvenanceTotal = 5000}
  check "a conforming observation passes" (checkServiceCase expectation good == CheckPass)
  check "an r7 clobber is refused" (mentions "register r7" (checkServiceCase expectation abi))
  check "an immutable write is refused" (mentions "forbidden write to immutable region casebuf" (checkServiceCase expectation immutable))
  check "a stack write below the scratch span is refused" (mentions "stack write outside the scratch span" (checkServiceCase expectation overflow))
  check "truncated provenance is refused" (mentions "truncated" (checkServiceCase expectation truncated))

mentions :: String -> Check -> Bool
mentions needle (CheckMismatch problems) = any (needle `isInfix`) problems
mentions _ _ = False

isInfix :: String -> String -> Bool
isInfix needle haystack = any (needle `prefix`) (suffixes haystack)
  where
    prefix a b = take (length a) b == a
    suffixes [] = [[]]
    suffixes text@(_ : rest) = text : suffixes rest

serviceExpectation :: Expectation
serviceExpectation =
  Expectation
    { expectationExpectedR0 = 3
    , expectationPreserved = serviceSeeds ++ [("sp", serviceStackTop), ("pc", 0x009b0000)]
    , expectationInitialMemory =
        [ ("casebuf", serviceCasebufInitial)
        , ("mirror", B.replicate serviceTransientSize 0)
        ]
    , expectationBounds =
        [ ("casebuf", fromIntegral serviceStackBase, serviceCasebufSize)
        , ("mirror", 0x0089ec00, serviceTransientSize)
        ]
    , expectationStackBase = fromIntegral serviceStackBase
    , expectationStackOffset = serviceStackOffset
    , expectationStackSize = serviceStackSize
    , expectationStackTop = fromIntegral serviceStackTop
    , expectationTransientRegion = serviceTransientRegion
    , expectationTransientOffset = serviceTransientOffset
    , expectationTransientSize = serviceTransientSize
    }

goodObservation :: Observation
goodObservation =
  Observation
    { observationStatus = "returned"
    , observationRegisters =
        [("r0", 3)]
          ++ serviceSeeds
          ++ [ ("r1", 0)
             , ("r2", 0)
             , ("r3", 0)
             , ("r12", 0)
             , ("sp", serviceStackTop)
             , ("lr", 0x009b0000)
             , ("pc", 0x009b0000)
             , ("cpsr", 0x10)
             ]
    , observationMemory =
        [ ("casebuf", stackBytes)
        , ("mirror", B.replicate serviceTransientSize 0)
        ]
    , observationWrites = Just [(0x009a1fec, 4), (0x0089edc8, 4)]
    , observationProvenanceTotal = 2
    , observationInstructions = 100
    , observationEngineError = 0
    }
  where
    stackBytes =
      B.take serviceStackOffset serviceCasebufInitial
        <> B.replicate serviceStackSize 0
        <> B.drop (serviceStackOffset + serviceStackSize) serviceCasebufInitial

parser :: IO ()
parser = do
  let regions = [("casebuf", 0x009a0000, 4)]
      valid = runnerValue
  check "valid output parses" (case observe regions valid of
    Right observation -> observationStatus observation == "returned" && lookup "r7" (observationRegisters observation) == Just 0x7d7d7d7d
    Left _ -> False)
  check "missing status is refused" (case observe regions (without "status") of
    Left message -> "missing runner status" `isInfix` message
    Right _ -> False)
  check "missing register is refused" (case observe regions (withRegisters (filter ((/= "r7") . fst) allRegisters)) of
    Left message -> "missing registers" `isInfix` message
    Right _ -> False)
  check "malformed memory is refused" (case observe regions (withMemory [("0x009a0000", "zz")]) of
    Left message -> "malformed bytes" `isInfix` message
    Right _ -> False)
  check "provenance count mismatch is refused" (case observe regions (setProvenance "2" (provenanceEntry 0x009a0000)) of
    Left message -> "provenance count mismatch" `isInfix` message
    Right _ -> False)
  check "truncated provenance is not complete" (case observe regions (setProvenance "4097" (arr (replicate 4096 (provenanceEntry 0x009a0000)))) of
    Right observation -> observationWrites observation == Nothing && observationProvenanceTotal observation == 4097
    Left _ -> False)

allRegisters :: [(String, Word32)]
allRegisters =
  [ ("r0", 3)
  , ("r1", 0)
  , ("r2", 0)
  , ("r3", 0)
  ]
    ++ serviceSeeds
    ++ [ ("r12", 0)
       , ("sp", serviceStackTop)
       , ("lr", 0x009b0000)
       , ("cpsr", 0x10)
       , ("pc", 0x009b0000)
       ]

runnerValue :: Value
runnerValue =
  obj
    [ ("status", s "returned")
    , ("registers", registerObject allRegisters)
    , ("memory", memoryArray [("0x009a0000", "aabbccdd")])
    , ("memory_provenance_total", s "1")
    , ("memory_provenance", arr [provenanceEntry 0x009a0000])
    , ("instructions", s "10")
    , ("unicorn_error", n 0)
    ]

registerObject :: [(String, Word32)] -> Value
registerObject
  = object
  . map (\(name, value) -> fromString name .= s (hexWord32 value))

memoryArray :: [(String, String)] -> Value
memoryArray
  = arr
  . map (\(address, bytes) -> object [("address", s address), ("bytes", s bytes)])

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
    text = go value
    go 0 = "0"
    go number = reverse (digitsAt number)
    digitsAt 0 = []
    digitsAt number = digits !! fromIntegral (number `mod` 16) : digitsAt (number `div` 16)

without :: String -> Value
without key = case runnerValue of
  Object fields -> Object (KM.delete (K.fromString key) fields)
  _ -> runnerValue

withRegisters :: [(String, Word32)] -> Value
withRegisters registers = set "registers" (registerObject registers) runnerValue

withMemory :: [(String, String)] -> Value
withMemory memory = set "memory" (memoryArray memory) runnerValue

setProvenance :: String -> Value -> Value
setProvenance total provenance =
  set "memory_provenance_total" (s total) (set "memory_provenance" provenance runnerValue)
