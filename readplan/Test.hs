{-# LANGUAGE OverloadedStrings #-}
-- Library tests on synthetic images only; no camera I/O.
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isInfixOf)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import FujiAF.AdjustmentTables
import FujiAF.ReadPlan
import FujiAF.Synthetic
import FujiAF.WindowCompare

assert :: String -> Bool -> IO ()
assert name condition = unless condition (error ("FAIL: " ++ name))

expect :: Either String a -> IO a
expect = either (error . ("FAIL: " ++)) pure

refused :: String -> Either String a -> IO ()
refused name result = case result of
  Left _ -> pure ()
  Right _ -> error ("FAIL: " ++ name ++ " was accepted")

offsets :: [Window] -> [Word32]
offsets windows = [offset | Window _ (AdjOffset offset) _ <- windows]

windowAt :: [Window] -> Word32 -> Window
windowAt windows offset = case [window | window@(Window _ (AdjOffset at) _) <- windows, at == offset] of
  (window : _) -> window
  [] -> error ("FAIL: no window at " ++ showOffset offset)

field :: Value -> String -> Value
field (Object fields) key = maybe (error ("FAIL: missing field " ++ key)) id (M.lookup (K.fromString key) fields)
field _ key = error ("FAIL: field " ++ key ++ " of a non-object")

replace :: String -> Value -> Value -> Value
replace key value (Object fields) = Object (M.insert (K.fromString key) value fields)
replace key _ other = error ("FAIL: cannot replace " ++ key ++ " in " ++ show other)

stringOf :: Value -> String
stringOf (String value) = T.unpack value
stringOf other = error ("FAIL: expected a string, got " ++ show other)

arrayOf :: Value -> [Value]
arrayOf (Array items) = V.toList items
arrayOf other = error ("FAIL: expected an array, got " ++ show other)

flipByte :: Int -> B.ByteString -> B.ByteString
flipByte position bytes =
  B.concat [B.take position bytes, B.singleton (B.index bytes position + 1), B.drop (position + 1) bytes]

isNotComparable :: Outcome -> Bool
isNotComparable (NotComparable _) = True
isNotComparable _ = False

main :: IO ()
main = do
  let image = decodedImage "/private/synthetic-image.bin" syntheticBase syntheticImage
      generator = sha256Of (C.pack "synthetic generator")
  name <- expect (planId "read-synthetic-v1")
  boundPlan <- expect (buildPlan Rc0Bound name generator [image])
  extentPlan <- expect (buildPlan Rc0Extent name generator [image])
  bulkPlan <- expect (buildPlan Adjustment42000 name generator [])
  gatePlan <- expect (buildPlan GateByte name generator [])
  let bound = planWindowList boundPlan
      extent = planWindowList extentPlan
      bulk = planWindowList bulkPlan
      gate = planWindowList gatePlan
      rowZero = planTables boundPlan

  -- Scope selection.
  assert "scopes are data"
    (map scopeRule [Rc0Bound, Rc0Extent] == [FromTables RowZeroOnly, FromTables WholeExtent]
      && case scopeRule Adjustment42000 of
           FixedRanges ranges -> map (\range -> (rangeFirst range, rangeLast range)) ranges
                                   == [(0x000000, 0x041fff), (0x400000, 0x40fcff)]
           FromTables _ -> False)
  assert "rc0-bound resolves every table at row 0"
    (map resolvedTable rowZero == developTables && all ((== RowZeroOnly) . resolvedExtent) rowZero
      && map (length . resolvedEntries) rowZero == map tableRowZeroEntries developTables)
  assert "rc0-extent resolves every table across its extent"
    (map (length . resolvedEntries) (planTables extentPlan) == map tableExtentEntries developTables)
  assert ("rc0-bound selects the row-0 windows and the fixed three, got " ++ show (offsets bound))
    (offsets bound == [0x000100, 0x000200, 0x000300, 0x00c000])
  assert "rc0-bound table windows exclude the fixed windows that no entry reaches"
    (tableWindowOffsets rowZero == [0x000100, 0x000300])
  assert ("rc0-extent adds the windows only the whole extents reach, got " ++ show (offsets extent))
    (offsets extent == [0x000100, 0x000200, 0x000300, beyondRowZero, 0x00c000])
  assert "adjustment-42000 covers 0x42000 and the 0xfd00 block"
    (length bulk == 1309 && readBytes bulk == 335104 && transferCount bulk == 5244)
  assert "adjustment-42000 window addresses"
    (take 2 (offsets bulk) == [0x000000, 0x000100] && offsets bulk !! 1055 == 0x041f00
      && offsets bulk !! 1056 == 0x400000 && drop 1308 (offsets bulk) == [0x40fc00])
  assert "adjustment-42000 resolves no table" (null (planTables bulkPlan) && null (planImages bulkPlan))
  assert "rc0 scopes price windows, bytes and transfers"
    (readBytes bound == 1024 && transferCount bound == 24 && readBytes extent == 1280 && transferCount extent == 28)
  assert ("gate-byte reads the gate window and the fixed three, got " ++ show (offsets gate))
    (offsets gate == [0x000000, 0x000100, 0x000200, 0x00c000] && readBytes gate == 1024 && transferCount gate == 24)
  assert "gate-byte names the gate byte in the purpose of its first window"
    ("0xf7" `isInfixOf` windowPurpose (windowAt gate 0x000000))
  assert "scope names round trip"
    (map parseScope scopeNames == map Right [Rc0Bound, Rc0Extent, Adjustment42000, GateByte])
  refused "unknown scope" (parseScope "rc0")
  refused "adjustment-42000 given an image" (buildPlan Adjustment42000 name generator [image])
  refused "rc0-bound without an image" (buildPlan Rc0Bound name generator [])
  let shortImage = decodedImage "/private/short.bin" (CodeAddress 0x1000) (B.replicate 32 0)
  refused "tables outside every image" (buildPlan Rc0Bound name generator [shortImage])
  refused "overlapping images"
    (imageSet [ decodedImage "/private/one.bin" (CodeAddress 0x1000) (B.replicate 32 0)
              , decodedImage "/private/two.bin" (CodeAddress 0x1010) (B.replicate 32 0) ])

  -- Oracle and purposes.
  oracles <- traverse (expect . oracleIndex) [bound, extent, bulk, gate]
  assert "the oracle window is named by index" (oracles == [0, 0, 1, 1])
  assert "every scope carries the oracle window at 0x000100"
    (all (\windows -> 0x000100 `elem` offsets windows) [bound, extent, bulk, gate])
  assert "the oracle window keeps both its own purpose and its table entries"
    ("identity oracle" `isInfixOf` windowPurpose (windowAt bound 0x000100)
      && "filter-matrix-selector" `isInfixOf` windowPurpose (windowAt bound 0x000100))
  assert "purposes stay inside the schema bound"
    (all (\window -> let text = windowPurpose window in not (null text) && length text <= 256) (bound ++ extent ++ bulk ++ gate))

  -- Window invariants.
  mapM_ (expect . validateWindows) [bound, extent, bulk, gate]
  expect (validateWindows [Window 0 (AdjOffset 0x000100) "oracle", Window 1 (AdjOffset 0x41ff00) "last window"])
  refused "empty plan" (validateWindows [])
  refused "non-aligned window"
    (validateWindows [Window 0 (AdjOffset 0x000100) "oracle", Window 1 (AdjOffset 0x000181) "unaligned"])
  refused "window past the adjustment area"
    (validateWindows [Window 0 (AdjOffset 0x000100) "oracle", Window 1 (AdjOffset 0x420000) "outside"])
  refused "descending offsets"
    (validateWindows [Window 0 (AdjOffset 0x000200) "second", Window 1 (AdjOffset 0x000100) "oracle"])
  refused "repeated offsets"
    (validateWindows [Window 0 (AdjOffset 0x000100) "oracle", Window 1 (AdjOffset 0x000100) "again"])
  refused "index that is not the array position" (validateWindows [Window 1 (AdjOffset 0x000100) "oracle"])
  refused "plan without the oracle window" (validateWindows [Window 0 (AdjOffset 0x000200) "signature"])
  refused "plan without a purpose" (validateWindows [Window 0 (AdjOffset 0x000100) ""])
  refused "more windows than one session carries"
    (validateWindows [ Window index (AdjOffset (0x000100 + 0x100 * fromIntegral index)) "filler"
                     | index <- [0 .. maximumWindows] ])
  refused "plan id with a space" (planId "read rc0 bound")
  refused "empty plan id" (planId "")

  -- Provenance.
  document <- expect (planValue boundPlan)
  bulkDocument <- expect (planValue bulkPlan)
  let basis = stringOf (field document "firmware_basis")
  assert "the plan binds the generator by digest and names its scope"
    (stringOf (field (field document "generated_by") "sha256") == sha256Text generator
      && stringOf (field (field document "generated_by") "tool") == "readplan/read-plan rc0-bound")
  assert "the firmware basis names the scope, the tables and every image digest"
    ("rc0-bound" `isInfixOf` basis && "9 develop tables row-zero" `isInfixOf` basis
      && sha256Text (sha256Of syntheticImage) `isInfixOf` basis && "image@0xd61e00" `isInfixOf` basis)
  assert "the fixed-range basis states that no image was read"
    ("no image read" `isInfixOf` stringOf (field bulkDocument "firmware_basis"))
  changedPlan <- expect (buildPlan Rc0Bound name generator
                           [decodedImage "/private/synthetic-image.bin" syntheticBase (B.snoc syntheticImage 7)])
  changedBasis <- expect (firmwareBasis changedPlan)
  assert "a changed image changes the bound basis" (changedBasis /= basis)
  crowded <- expect (buildPlan Rc0Bound name generator
                       [ image
                       , decodedImage "/private/extra-one.bin" (CodeAddress 0x01000000) (B.replicate 16 1)
                       , decodedImage "/private/extra-two.bin" (CodeAddress 0x02000000) (B.replicate 16 2) ])
  refused "a basis that would need shortening" (firmwareBasis crowded)
  assert "the plan carries the read channel and the oracle binding"
    (stringOf (field (field document "channel") "opcode") == "0x1a0002"
      && field (field document "channel") "function" == Number 2
      && field (field document "channel") "mb" == Number 1
      && field (field document "channel") "length" == Number 256
      && field (field document "oracle") "window_index" == Number 0
      && stringOf (field (field document "oracle") "byte_offset") == "0x5c"
      && stringOf (field (field document "oracle") "expect") == "product-string")
  assert "every window requests 256 bytes"
    (all (\window -> field window "length" == Number 256) (arrayOf (field document "windows")))

  -- Plan parsing.
  parsed <- expect (parsePlan document)
  assert "a written plan parses back to its windows" (parsed == bound)
  firstWindow <- case arrayOf (field document "windows") of
    (window : _) -> pure window
    [] -> error "FAIL: plan without windows"
  let withWindows windows = replace "windows" (Array (V.fromList windows)) document
  refused "a plan naming the adjustment-cache write function"
    (parsePlan (replace "channel" (replace "function" (Number 3) (field document "channel")) document))
  refused "a plan naming the debug read opcode"
    (parsePlan (replace "channel" (replace "opcode" (String "0x200001") (field document "channel")) document))
  refused "a plan with an oversized request length"
    (parsePlan (replace "channel" (replace "length" (Number 512) (field document "channel")) document))
  refused "a plan with an oversized window"
    (parsePlan (withWindows [replace "length" (Number 257) firstWindow]))
  refused "a plan with a non-aligned window"
    (parsePlan (withWindows [firstWindow, replace "index" (Number 1) (replace "offset" (String "0x000181") firstWindow)]))

  -- Comparison outcomes.
  let fallback = fallbackImage (B.pack [fromIntegral (position `mod` 251) | position <- [0 .. 0x3ff :: Int]])
      slice offset = B.take 256 (B.drop offset (fallbackBytes fallback))
      matched = compareWindow fallback (windowAt bound 0x000100) (Right (slice 0x100))
      differed = compareWindow fallback (windowAt bound 0x000300) (Right (flipByte 0x0c (slice 0x300)))
      missing = compareWindow fallback (windowAt bound 0x000200) (Left "no capture file 000200.bin")
      uncovered = compareWindow fallback (windowAt bound 0x00c000) (Right (B.replicate 256 0))
      shortCapture = compareWindow fallback (windowAt bound 0x000100) (Right (B.replicate 128 0))
      filtered = compareWindow fallback (windowAt bound 0x000100) (Right (flipByte 0x10 (slice 0x100)))
      unbound = compareWindow fallback (windowAt bound 0x000300) (Right (flipByte 0x01 (slice 0x300)))
  assert "the fallback load stops at 0x42000"
    (B.length (fallbackBytes (fallbackImage (B.replicate 0x50000 0))) == 0x42000)
  assert "captured bytes equal to the fallback match" (reportOutcome matched == Matched)
  assert "a changed byte is reported with its adjustment offset"
    (reportOutcome differed == Differed [0x00030c])
  assert "a missing capture is not comparable" (isNotComparable (reportOutcome missing))
  assert "a window the fallback does not cover is not comparable" (isNotComparable (reportOutcome uncovered))
  assert "a short capture is not comparable" (isNotComparable (reportOutcome shortCapture))

  let summary = summarize rowZero [matched, differed, missing, uncovered]
  assert "the summary counts each outcome"
    (summaryMatched summary == 1 && summaryDiffered summary == 1 && summaryNotComparable summary == 2)
  assert "the summary names the develop codes the differing bytes feed"
    (summaryDevelopCodes summary == [6] && summaryFilters summary == [] && summaryUnattributed summary == 0)
  assert "the summary names the monochrome filter index"
    (summaryFilters (summarize rowZero [filtered]) == [0])
  assert "bytes no table entry resolves to are counted as unattributed"
    (summaryUnattributed (summarize rowZero [unbound]) == 1)
  assert "the report names each outcome and the develop code"
    (let report = reportLines rowZero [matched, differed, missing]
     in any ("window 0x000100 match" `isInfixOf`) report
          && any ("window 0x000300 differ 1 bytes from 0x00030c" `isInfixOf`) report
          && any ("window 0x000200 not-comparable" `isInfixOf`) report
          && any ("develop codes fed by differing bytes: 6" `isInfixOf`) report)

  putStrLn ("PASS: three scopes (" ++ show (length bound) ++ "/" ++ show (length extent) ++ "/"
             ++ show (length bulk) ++ " synthetic windows), window invariants and refusals, plan id,"
             ++ " image set, provenance binding, plan parsing refusals, three comparison outcomes;"
             ++ " synthetic inputs only, no camera I/O")
