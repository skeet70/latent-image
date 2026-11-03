{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.List (find)
import Data.Word (Word32)
import Replacement.ModalRuntime.MenuTables

main :: IO ()
main = do
  let tables = decodeMenuTables (image4 imageFixture) (stringSlot slotFixture)
      entryAt address = find ((== Address address) . entryAddress) (tablesNodes tables)
      recordAt address = case entryAt address of
        Just NodeEntry {entryRecord = Right record} -> pure record
        _ -> fail ("no resolved node at " ++ show address)
      slot identity = slotEntry <$> find ((== NodeId identity) . slotId) (tablesRegistry tables)
  check "registry has 0x86 slots" (length (tablesRegistry tables) == 0x86)
  check "registry slots" (map slot [0, 1, 2, 5] == map Just [SlotNode (Address nodeA), SlotNode (Address nodeB), EmptySlot, SlotNode (Address nodeA)])
  check "node set" (map entryAddress (tablesNodes tables) == map Address [outside, nodeA, nodeB, nodeD])
  check "registry ids" (fmap entryRegistryIds (entryAt nodeA) == Just [NodeId 0, NodeId 5] && fmap entryRegistryIds (entryAt nodeD) == Just [])
  a <- recordAt nodeA
  check "raw record" (nodeRaw a == words32 nodeAWords)
  check "node fields" (nodeIdOf a == NodeId 0x10 && nodePropertyOrClass a == 0xfe && nodeKind a == 7 && nodeFlag a == 1
    && nodeParent a == Nothing && nodeValues a == Just (Address 0x00781000) && nodeWord10 a == 0x15b)
  check "rows end at {0x7ffffffe, 0}" (nodeRows a == Walked [Row 3 Nothing, Row 0x7ffffffe (Just (Address nodeB)), Row 1 (Just (Address outside))])
  check "labels resolve strings" (nodeLabelRows a == Walked
    [ Label (StringRef (StringId 0x83c) Nothing) (StringRef (StringId 1) (Just "ISO")) (StringRef (StringId 5) Nothing) (StringRef (StringId 2) (Just "A\nB"))
    , Label (StringRef (StringId 0xffffffff) Nothing) (StringRef (StringId 0xffffffff) Nothing) (StringRef (StringId 0xffffffff) Nothing) (StringRef (StringId 0xff000005) Nothing) ])
  check "outside child unresolved" (fmap entryRecord (entryAt outside) == Just (Left PointerOutsideImage)
    && fmap entryReferences (entryAt outside) == Just [Reference (Address nodeA) ChildOf])
  check "unregistered child reported" (fmap entryReferences (entryAt nodeD) == Just [Reference (Address nodeB) ChildOf])
  check "parent reference" (fmap entryReferences (entryAt nodeA) == Just [Reference (Address nodeB) ParentOf])
  d <- recordAt nodeD
  check "labels past image end" (nodeLabelRows d == WalkUnresolved RunsPastImageEnd && nodeRows d == NullPointer)
  let property number = propertyTable <$> find ((== PropertyId number) . propertyId) (tablesProperties tables)
  check "conversion table ends at three 0xffff words" (property 0xfe == Just (Table (Address 0x00783000)
    (Walked [ConversionRow 0 1 2, ConversionRow 0xffff 0x7ffffffe 4])))
  check "property beyond converter bound" (property 0x300 == Just BeyondConverterBound)
  check "null conversion pointer" (property 0x200 == Just NullTablePointer)
  check "slot 0x86 is outside the registry" (all ((/= Address nodeC) . entryAddress) (tablesNodes tables))

check :: String -> Bool -> IO ()
check label passed = unless passed (fail label)

nodeA, nodeB, nodeC, nodeD, outside :: Word32
nodeA = 0x00780000
nodeB = 0x00780100
nodeC = 0x00780200
nodeD = 0x00780300
outside = 0x00200000

imageEnd :: Word32
imageEnd = 0x0081c000

nodeAWords :: [Word32]
nodeAWords = [0x10, 0, 0xfe, 7, 0, 0, 1, 0, 0x00781000, 0x00782000, 0x15b]

imageFixture :: B.ByteString
imageFixture = foldl place (B.replicate (fromIntegral (imageEnd - 0x0061c400)) 0)
  [ (0x0081babc, words32 [nodeA, nodeB, 0, 0, 0, nodeA])
  , (0x0081babc + 4 * 0x86, words32 [nodeC])
  , (nodeA, words32 nodeAWords)
  , (0x00781000, words32 [3, 0, 0x7ffffffe, nodeB, 1, outside, 0x7ffffffe, 0, 9, 0])
  , (0x00782000, words32 [0x83c, 1, 5, 2, 0xffffffff, 0xffffffff, 0xffffffff, 0xff000005, 0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff, 1, 1, 1, 1])
  , (nodeB, words32 [0x11, 0, 0x300, 7, 0, 0, 1, nodeA, 0x00781100, 0, 0])
  , (0x00781100, words32 [0, nodeD, 0x7ffffffe, 0])
  , (nodeC, words32 [0x12, 0, 0xfe, 7, 0, 0, 1, 0, 0, 0x00782000, 0])
  , (nodeD, words32 [0x40, 0, 0x200, 7, 0, 0, 1, nodeB, 0, imageEnd - 0x18, 0])
  , (0x0081b09c + 4 * 0xfe, words32 [0x00783000])
  , (0x00783000, words32 [0, 1, 2, 0xffff, 0x7ffffffe, 4, 0xffff, 0xffff, 0xffff, 7, 7, 7])
  ]
 where
  place image (address, bytes) = let offset = fromIntegral (address - 0x0061c400) in B.take offset image <> bytes <> B.drop (offset + B.length bytes) image

slotFixture :: B.ByteString
slotFixture = foldl place (B.replicate 0x20000 0)
  [ (0x3754 + 8 * 1, words32 [0x00010000, 0x0039c100])
  , (0x3754 + 8 * 2, words32 [0x00020000, 0x0039c110])
  , (0x3754 + 8 * 5, words32 [0, 0x0039c100])
  , (0x100, words32 [0x0039c200, 0, 0])
  , (0x110, words32 [0x0039c300, 0, 0, 0x0039c310, 0, 0])
  , (0x200, utf16 "ISO")
  , (0x300, utf16 "A")
  , (0x310, utf16 "B")
  ]
 where
  place slot (offset, bytes) = B.take offset slot <> bytes <> B.drop (offset + B.length bytes) slot
  utf16 text = B.pack (concatMap (\char -> [fromIntegral (fromEnum char), 0]) text ++ [0, 0])

words32 :: [Word32] -> B.ByteString
words32 = B.pack . concatMap (\word -> [fromIntegral (word `div` (256 ^ index)) | index <- [0 :: Int .. 3]])
