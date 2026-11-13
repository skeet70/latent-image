{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Synthetic tests for the census: one listing per edge kind, a table of
-- tables, a @movw@/@movt@-built address, a literal-pool address, and the task,
-- interrupt and callback roots. No firmware and no emulator.
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.Vector as V
import Data.Word (Word32, Word8)
import Emulation.Census
import Emulation.Sha256 (sha256, sha256Hex)
import System.Exit (exitFailure)

main :: IO ()
main = do
  let functions =
        [ Function 0x00001000 0x40 "direct-caller" ""
        , Function 0x00001100 0x40 "code-word-caller" ""
        , Function 0x00001200 0x20 "target" ""
        , Function 0x00001300 0x20 "table-target" ""
        , Function 0x00001400 0x20 "nested-table-target" ""
        , Function 0x00001500 0x40 "table-indexer" ""
        , Function 0x00001600 0x80 "task-creator-immediate" ""
        , Function 0x00001700 0x80 "task-creator-argument" ""
        , Function 0x00001800 0x40 "task-creator-caller" ""
        , Function 0x00001900 0x40 "interrupt-attach" ""
        , Function 0x00001a00 0x40 "callback-store" ""
        , Function 0x00001b00 0x40 "table-of-tables-indexer" ""
        , Function 0x00001c00 0x40 "literal-caller" ""
        , Function 0x00001d00 0x40 "indexed-table-of-tables" ""
        , Function 0x00001e00 0x80 "task-create-shared" ""
        , Function 0x00001f00 0x40 "task-create-loop-immediate" ""
        , Function 0x00002200 0x40 "task-create-loop-register" ""
        , Function 0x00007000 0x10 "indexed-target-a" ""
        , Function 0x00007010 0x10 "indexed-target-b" ""
        , Function 0x00007200 0x10 "indexed-target-c" ""
        , Function 0x00009000 0x10 "loop-task-entry" ""
        , Function 0x00009300 0x04 "by-value-struct-entry" ""
        , Function 0x00009304 0x10 "by-value-struct-save" ""
        , Function 0x00009400 0x04 "leaf-prefix-entry" ""
        , Function 0x00009404 0x10 "leaf-prefix-save" ""
        , Function 0x00f01bc8 0x04 "task-43-actor" ""
        ]
      listingBytes = syntheticListing functions
  listing <- either fail pure (parseListing "test" listingBytes)
  check "the listing reads its base" (listingBase listing == 0x1000)
  check "the listing reads a data word" (M.lookup 0x2000 (listingWords listing) == Just 0)
  check "a label line carries no word" (M.lookup 0x0fff (listingWords listing) == Nothing)
  assigned <- either fail pure (assignImages [listing] functions)
  check "every function lands in the listing" (all ((== "test") . functionImage) assigned)

  -- Function-table parsing: both the index shape and the plain shape.
  parsedIndex <- either fail pure (parseFunctions (BC.unlines
    [ "range\tmain\tstream\t0x00001000\t4096\t64\tabc\tdirect-caller"
    , "range\tmain\tstream\t0x00001100\t4160\t64\tabc\tcode-word-caller"
    ]))
  check "the index shape is read" (map functionEntry parsedIndex == [0x1000, 0x1100])
  check "the index size is read" (map functionSize parsedIndex == [64, 64])
  parsedPlain <- either fail pure (parseFunctions "0x2000\t32\tplain\n0x2010\t16\tsecond\n")
  check "the plain shape is read" (map functionEntry parsedPlain == [0x2000, 0x2010])

  let census = buildCensus allEdgeKinds [listing] assigned
  check "the graph holds a direct edge" (hasEdge census 0x1000 0x1100 Direct)
  check "the graph holds a code-word edge from movw/movt" (hasEdge census 0x1100 0x1200 CodeWord)
  check "the graph holds a code-word edge from a literal load" (hasEdge census 0x1c00 0x1300 CodeWord)
  check "the table edge reaches its target" (hasEdge census 0x1500 0x1300 TableEdge)
  check "the table of tables reaches the nested target" (hasEdge census 0x1b00 0x1400 TableEdge)
  check "the table's extent includes its leading zeros" (any (\table -> tableStart table == 0x2000 && tableEnd table == 0x2010) (censusTables census))
  check "the table names its indexer" (any (\table -> tableStart table == 0x2000 && tableIndexers table == [0x1500]) (censusTables census))
  check "the table of tables names its nested table" (any (\table -> tableStart table == 0x3000 && 0x2000 `elem` tableNested table) (censusTables census))
  check "an indexed table of tables reaches every nested target" (all (\target -> hasEdge census 0x1d00 target TableEdge) [0x7000, 0x7010, 0x7200])
  check "an indexed table of tables does not reach a word no function starts at" (not (hasEdge census 0x1d00 0x7100 TableEdge))
  check "the indexed table of tables names its indexer and its nested tables"
    (any (\table -> tableStart table == 0x5000 && tableNested table == [0x6000, 0x6100] && tableIndexers table == [0x1d00]) (censusTables census))
  check "an immediate-bound loop creates every record's task" (all (\taskId -> hasRoot census (TaskRoot taskId) 0x9000) [1, 2, 3])
  check "a register-bound loop creates every record's task" (all (\taskId -> hasRoot census (TaskRoot taskId) 0x9000) [10, 11, 12])

  check "a direct call is reached" (S.member 0x1100 (reachFrom census [0x1000]))
  check "a code word is reached" (S.member 0x1200 (reachFrom census [0x1000]))
  check "a table target is reached" (S.member 0x1300 (reachFrom census [0x1500]))
  check "a nested table target is reached" (S.member 0x1400 (reachFrom census [0x1b00]))

  -- A sub sp, sp, #N followed by a push that saves the link register is one
  -- function: its declared 4-byte extent ends at the push.
  check "a by-value struct prologue is one function"
    (case M.lookup 0x9300 (censusFunctions census) of
      Just function -> functionSize function == 20 && M.notMember 0x9304 (censusFunctions census)
      Nothing -> False)
  check "the joined function reaches the call after the push" (S.member 0x1200 (reachFrom census [0x9300]))
  joined <- case M.lookup 0x9300 (censusFunctions census) of
    Just function -> pure function
    Nothing -> fail "the census dropped the by-value struct entry"
  joinedDigest <- either fail pure (extentDigest [listing] joined)
  let joinedWords = [subImm 13 13 16, push 0x4830, addImm 11 13 12, bl 0x930c 0x1200, bx]
  check "the joined extent digest covers the body after the push"
    (joinedDigest == sha256Hex (sha256 (B.pack (concatMap wordBytes joinedWords))))
  check "a push without the link register starts its own function"
    (case (M.lookup 0x9400 (censusFunctions census), M.lookup 0x9404 (censusFunctions census)) of
      (Just entry, Just save) -> functionSize entry == 4 && functionSize save == 16
      _ -> False)

  check "an immediate task descriptor is a root" (hasRoot census (TaskRoot 43) 0xf01bc8)
  check "a creator call site is a root" (hasRoot census (TaskRoot 7) 0x1300)
  check "an interrupt handler is a root" (hasRoot census InterruptRoot 0x1200)
  check "a stored code word is a callback root" (hasRoot census CallbackRoot 0x1200)
  check "a task root reports no edge kinds at its own entry"
    (case [kinds | (kind, kinds) <- M.findWithDefault [] 0xf01bc8 (reachByFunction census), kind == TaskRoot 43] of
      [kinds] -> S.null kinds
      _ -> False)
  check "no task site is left unresolved" (null (censusUnresolvedTasks census))

  -- Edge selection changes the graph.
  let directOnly = buildCensus (EdgeSelection True False False) [listing] assigned
  check "direct-only keeps direct edges" (hasEdge directOnly 0x1000 0x1100 Direct)
  check "direct-only drops table edges" (not (hasEdge directOnly 0x1500 0x1300 TableEdge))

  -- The extent digest is the digest of the extent's stock bytes.
  target <- case [function | function <- assigned, functionEntry function == 0x1200] of
    [function] -> pure function
    _ -> fail "the synthetic listing has no target function"
  digest <- either fail pure (extentDigest [listing] target)
  let expected = sha256Hex (sha256 (B.pack (concatMap wordBytes [0xe3a00001, 0xe12fff1e, 0, 0, 0, 0, 0, 0])))
  check "the extent digest hashes the stock bytes" (digest == expected)
  extents <- either fail pure (extentsJson census [listing] [0x1000])
  check "the extents carry every reached function" (extentRows extents == 3)

  -- A listing without words is refused, never read as an empty census.
  check "a listing without words is refused" (case parseListing "empty" "no rows\n" of Left _ -> True; Right _ -> False)

  putStrLn "census synthetic tests passed"

wordBytes :: Word32 -> [Word8]
wordBytes word = [fromIntegral (word .&. 0xff), fromIntegral ((word `shiftR` 8) .&. 0xff), fromIntegral ((word `shiftR` 16) .&. 0xff), fromIntegral (word `shiftR` 24)]

hasEdge :: Census -> Word32 -> Word32 -> EdgeKind -> Bool
hasEdge census from to kind = any (\edge -> edgeFrom edge == from && edgeTo edge == to && edgeKind edge == kind) (censusEdges census)

hasRoot :: Census -> RootKind -> Word32 -> Bool
hasRoot census kind entry = any (\root -> rootKind root == kind && rootEntry root == entry) (censusRoots census)

extentRows :: Value -> Int
extentRows (Object root) = case KM.lookup (K.fromString "entries") root of
  Just (Array entries) -> V.length entries
  _ -> -1
extentRows _ = -1

check :: String -> Bool -> IO ()
check label yes = unless yes (putStrLn ("FAIL: " ++ label) >> exitFailure)

-- * Synthetic listing

-- | Every word of every declared extent is present: code words from 'code',
-- zeros elsewhere, so an extent digest covers the whole extent.
syntheticListing :: [Function] -> B.ByteString
syntheticListing functions = BC.unlines (map row (M.toAscList filled))
  where
    filled = M.union (M.fromList code) (M.fromList
      [ (address, 0)
      | function <- functions
      , address <- [functionEntry function, functionEntry function + 4 .. functionEntry function + functionSize function - 4]
      , M.notMember address (M.fromList code)
      ])

row :: (Word32, Word32) -> BC.ByteString
row (address, word) = BC.pack (showHexWord address ++ ": " ++ showHexWord word ++ " \tmov\tr0, r0")

showHexWord :: Word32 -> String
showHexWord initial = go (8 :: Int) initial ""
  where
    go 0 _ text = text
    go count current text = go (count - 1) (current `shiftR` 4) (digit (current .&. 0xf) : text)
    digit nibble
      | nibble < 10 = toEnum (fromEnum '0' + fromIntegral nibble)
      | otherwise = toEnum (fromEnum 'a' + fromIntegral nibble - 10)

code :: [(Word32, Word32)]
code =
  [ (0x1000, bl 0x1000 0x1100)
  , (0x1004, bx)
  , (0x1100, movw 3 0x1200)
  , (0x1104, movt 3 0x0000)
  , (0x1108, blx 3)
  , (0x110c, bx)
  , (0x1200, movImm 0 1)
  , (0x1204, bx)
  , (0x1300, movImm 0 2)
  , (0x1304, bx)
  , (0x1400, movImm 0 3)
  , (0x1404, bx)
  , (0x1500, movw 3 0x2000)
  , (0x1504, movt 3 0x0000)
  , (0x1508, ldrReg 0 3 4 2)
  , (0x150c, bx)
  , (0x1600, movImm 3 43)
  , (0x1604, strImmNeg 3 11 0x38)
  , (0x1608, movw 3 0x9a8c)
  , (0x160c, movt 3 0x00ef)
  , (0x1610, strImmNeg 3 11 0x34)
  , (0x1614, movImm 3 5)
  , (0x1618, strImmNeg 3 11 0x30)
  , (0x161c, movImm 3 4096)
  , (0x1620, strImmNeg 3 11 0x2c)
  , (0x1624, movw 3 0x1bc8)
  , (0x1628, movt 3 0x00f0)
  , (0x162c, strImmNeg 3 11 0x28)
  , (0x1630, movImm 3 0)
  , (0x1634, strImmNeg 3 11 0x24)
  , (0x1638, subImm 0 11 0x38)
  , (0x163c, bl 0x163c 0x00725dd8)
  , (0x1640, bx)
  , (0x1700, movReg 4 0)
  , (0x1704, strImmNeg 4 11 0x38)
  , (0x1708, movImm 3 1)
  , (0x170c, strImmNeg 3 11 0x34)
  , (0x1710, movImm 3 2)
  , (0x1714, strImmNeg 3 11 0x30)
  , (0x1718, movImm 3 4096)
  , (0x171c, strImmNeg 3 11 0x2c)
  , (0x1720, movw 3 0x1300)
  , (0x1724, movt 3 0x0000)
  , (0x1728, strImmNeg 3 11 0x28)
  , (0x172c, movImm 3 0)
  , (0x1730, strImmNeg 3 11 0x24)
  , (0x1734, subImm 0 11 0x38)
  , (0x1738, bl 0x1738 0x00725dd8)
  , (0x173c, bx)
  , (0x1800, movImm 0 7)
  , (0x1804, bl 0x1804 0x1700)
  , (0x1808, bx)
  , (0x1900, movw 1 0x1200)
  , (0x1904, movt 1 0x0000)
  , (0x1908, bl 0x1908 0x0064e918)
  , (0x190c, bx)
  , (0x1a00, movw 3 0x1200)
  , (0x1a04, movt 3 0x0000)
  , (0x1a08, strImmNeg 3 11 0x40)
  , (0x1a0c, bx)
  , (0x1b00, movw 3 0x3000)
  , (0x1b04, movt 3 0x0000)
  , (0x1b08, ldrReg 0 3 4 2)
  , (0x1b0c, bx)
  , (0x1c00, ldrImm 3 15 0x08)
  , (0x1c04, strImmNeg 3 11 0x40)
  , (0x1c08, bx)
  , (0x1c10, 0x1300)
  , (0x1d00, movw 3 0x5000)
  , (0x1d04, movt 3 0x0000)
  , (0x1d08, ldrReg 2 3 0 2)
  , (0x1d0c, ldrReg 3 2 1 2)
  , (0x1d10, blx 3)
  , (0x1d14, bx)
  , (0x1e00, movReg 4 0)
  , (0x1e04, movReg 5 1)
  , (0x1e08, movReg 6 2)
  , (0x1e0c, strImmNeg 4 11 0x38)
  , (0x1e10, strImmNeg 5 11 0x34)
  , (0x1e14, strImmNeg 6 11 0x30)
  , (0x1e18, movImm 3 4096)
  , (0x1e1c, strImmNeg 3 11 0x2c)
  , (0x1e20, movw 3 0x9000)
  , (0x1e24, movt 3 0x0000)
  , (0x1e28, strImmNeg 3 11 0x28)
  , (0x1e2c, subImm 0 11 0x38)
  , (0x1e30, bl 0x1e30 0x00725dd8)
  , (0x1e34, bx)
  , (0x1f00, movImm 4 0)
  , (0x1f04, movw 5 0x8000)
  , (0x1f08, movt 5 0x0000)
  , (0x1f0c, addReg 3 5 4)
  , (0x1f10, ldrImm 0 3 0)
  , (0x1f14, ldrImm 1 3 4)
  , (0x1f18, ldrImm 2 3 8)
  , (0x1f1c, bl 0x1f1c 0x1e00)
  , (0x1f20, addImm 4 4 12)
  , (0x1f24, cmpImm 4 36)
  , (0x1f28, bne 0x1f28 0x1f0c)
  , (0x1f2c, bx)
  , (0x2200, movw 4 0x8108)
  , (0x2204, movt 4 0x0000)
  , (0x2208, addImm 6 4 36)
  , (0x220c, ldrImmNeg 0 4 8)
  , (0x2210, ldrImmNeg 1 4 4)
  , (0x2214, ldrImm 2 4 0)
  , (0x2218, bl 0x2218 0x1e00)
  , (0x221c, addImm 4 4 12)
  , (0x2220, cmpReg 4 6)
  , (0x2224, bne 0x2224 0x220c)
  , (0x2228, bx)
  , (0x2000, 0), (0x2004, 0), (0x2008, 0x1200), (0x200c, 0x1300)
  , (0x3000, 0x2000), (0x3004, 0x4000)
  , (0x4000, 0), (0x4004, 0), (0x4008, 0x1400)
  , (0x5000, 0x6000), (0x5004, 0x6100)
  , (0x6000, 0), (0x6004, 0), (0x6008, 0x7000), (0x600c, 0x7010)
  , (0x6100, 0), (0x6104, 0), (0x6108, 0x7100), (0x610c, 0x7200)
  , (0x7100, 0xe1a00000)
  , (0x8000, 1), (0x8004, 0x7000), (0x8008, 5)
  , (0x800c, 2), (0x8010, 0x7000), (0x8014, 5)
  , (0x8018, 3), (0x801c, 0x7000), (0x8020, 5)
  , (0x8100, 10), (0x8104, 0x7000), (0x8108, 5)
  , (0x810c, 11), (0x8110, 0x7000), (0x8114, 5)
  , (0x8118, 12), (0x811c, 0x7000), (0x8120, 5)
  , (0x9000, bx)
  , (0x9300, subImm 13 13 16)
  , (0x9304, push 0x4830)
  , (0x9308, addImm 11 13 12)
  , (0x930c, bl 0x930c 0x1200)
  , (0x9310, bx)
  , (0x9400, subImm 13 13 8)
  , (0x9404, push 0x0030)
  , (0x9408, movImm 0 1)
  , (0x940c, bl 0x940c 0x1200)
  , (0x9410, bx)
  , (0xf01bc8, bx)
  ]

bl :: Word32 -> Word32 -> Word32
bl site target = 0xeb000000 .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

bx :: Word32
bx = 0xe12fff1e

push :: Word32 -> Word32
push registers = 0xe92d0000 .|. registers

movReg :: Word32 -> Word32 -> Word32
movReg destination source = 0xe1a00000 .|. (destination `shiftL` 12) .|. source

ldrReg :: Word32 -> Word32 -> Word32 -> Word32 -> Word32
ldrReg destination base index shift = 0xe7900000 .|. (base `shiftL` 16) .|. (destination `shiftL` 12) .|. (shift `shiftL` 7) .|. index

ldrImm :: Word32 -> Word32 -> Word32 -> Word32
ldrImm destination base offset = 0xe5900000 .|. (base `shiftL` 16) .|. (destination `shiftL` 12) .|. offset

strImmNeg :: Word32 -> Word32 -> Word32 -> Word32
strImmNeg source base offset = 0xe5000000 .|. (base `shiftL` 16) .|. (source `shiftL` 12) .|. offset

movImm :: Word32 -> Word32 -> Word32
movImm destination immediate = 0xe3a00000 .|. (destination `shiftL` 12) .|. immediate

subImm :: Word32 -> Word32 -> Word32 -> Word32
subImm destination base immediate = 0xe2400000 .|. (base `shiftL` 16) .|. (destination `shiftL` 12) .|. immediate

movw :: Word32 -> Word32 -> Word32
movw register immediate = 0xe3000000 .|. (((immediate `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (register `shiftL` 12) .|. (immediate .&. 0xfff)

movt :: Word32 -> Word32 -> Word32
movt register immediate = 0xe3400000 .|. (((immediate `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (register `shiftL` 12) .|. (immediate .&. 0xfff)

blx :: Word32 -> Word32
blx register = 0xe12fff30 .|. register

addReg :: Word32 -> Word32 -> Word32 -> Word32
addReg destination source register = 0xe0800000 .|. (source `shiftL` 16) .|. (destination `shiftL` 12) .|. register

addImm :: Word32 -> Word32 -> Word32 -> Word32
addImm destination source immediate = 0xe2800000 .|. (source `shiftL` 16) .|. (destination `shiftL` 12) .|. immediate

cmpImm :: Word32 -> Word32 -> Word32
cmpImm register immediate = 0xe3500000 .|. (register `shiftL` 16) .|. immediate

cmpReg :: Word32 -> Word32 -> Word32
cmpReg left right = 0xe1500000 .|. (left `shiftL` 16) .|. right

bne :: Word32 -> Word32 -> Word32
bne site target = 0x1a000000 .|. (((target - site - 8) `shiftR` 2) .&. 0xffffff)

ldrImmNeg :: Word32 -> Word32 -> Word32 -> Word32
ldrImmNeg destination base offset = 0xe5100000 .|. (base `shiftL` 16) .|. (destination `shiftL` 12) .|. offset
