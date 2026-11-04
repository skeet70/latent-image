{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Either (isLeft)
import Data.Word (Word32)
import Replacement.ModalRuntime.MenuEngineClass
import Replacement.ModalRuntime.MenuEnginePair
import Replacement.ModalRuntime.MenuEngineTree
import System.Exit (exitFailure)

node :: Word32 -> Word32 -> Word32 -> Maybe Word32 -> Maybe [TableRow] -> TableNode
node at ident type_ parent rows = TableNode
  { nodeAddress = Address at, nodeId = ident, nodeType = type_, nodeClass = 0x10, nodeRowsPerPage = 9
  , nodeNoWrap = 0, nodeNoAvailability = 0, nodeRemember = 1, nodeParent = Address <$> parent
  , nodeWord10 = 0, nodeRows = rows, nodeLabels = Nothing }

-- A grandparent (id 5), the registered node at slot 3 (id 1) whose row 0 leads to a child with
-- its parent's id and whose row 1 leads to a type-5 node, and a node that does not wrap.
tree :: MenuTree
tree = menuTree (TablesDocument "image4" "strings"
  [RegistryEntry 3 (Just (Address 0x1000)), RegistryEntry 4 Nothing, RegistryEntry 6 (Just (Address 0x4000))]
  [ node 0x500 5 0 Nothing (Just [TableRow 0 (Just (Address 0x1000))])
  , node 0x1000 1 0 (Just 0x500) (Just [TableRow 0 (Just (Address 0x2000)), TableRow 1 (Just (Address 0x3000))])
  , node 0x2000 1 0 (Just 0x1000) (Just [TableRow 0 Nothing])
  , node 0x3000 7 5 (Just 0x1000) (Just [TableRow 0 Nothing])
  , (node 0x4000 8 0 (Just 0x500) (Just [TableRow 0 Nothing])) {nodeNoWrap = 1} ])

blockBytesFor :: Word32 -> Word32 -> B.ByteString
blockBytesFor record cursor = word record <> B.replicate 4 0 <> word cursor <> B.replicate (0x430 - 12) 0
 where word value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

classOf :: Word32 -> Word32 -> StageEvent -> Either String EventClass
classOf record cursor event = (\block -> deriveClass tree block event) <$> menuBlock (blockBytesFor record cursor)

stage :: Word32 -> Word32 -> Word32 -> StageEvent
stage kind slot ident = StageEvent kind slot ident 0

entry :: Int -> Word32 -> Word32 -> StageEvent -> Hit
entry core record cursor (StageEvent kind slot ident extra) =
  Hit stageEntrySite core (kind, slot, ident, extra) (Just 0xca4680) [blockBytesFor record cursor, B.replicate 8 0]

fallback :: Int -> Hit
fallback core = Hit kindSwitchSite core (0, 0, 0, 0) (Just fallbackReturn) []

post :: Word32 -> Hit
post token = Hit eventPostSite 0 (0x1d, 0xf6, token, 0xf000) (Just 0xca47f0) []

segment :: [(B.ByteString)] -> [Hit] -> Observation
segment blocks hits = Observation "picker-keys"
  ([(label, B.replicate 8 1) | label <- drop 1 engineReads] ++ [("menu-block", block) | block <- blocks]) hits

main :: IO ()
main = do
  let cases =
        [ ("a row step on a wrapping queried node is owned", classOf 0x1000 0 (stage 4 3 1) == Right (OwnedEvent (RowMove Down)))
        , ("a step of the node's rows per page is a page move", classOf 0x1000 0 (stage 5 3 9) == Right (FallbackEvent PageStep))
        , ("a row step on a node that does not wrap falls back", classOf 0x4000 0 (stage 4 6 1) == Right (FallbackEvent NodeFlags))
        , ("a move with no open node falls back", classOf 0 0 (stage 4 3 1) == Right (FallbackEvent NoOpenNode))
        , ("a push to the cursor row's child is owned", classOf 0x1000 0 (stage 0xe 3 0x1d) == Right (OwnedEvent Push))
        , ("a push onto a type-5 node falls back", classOf 0x1000 1 (stage 0xe 3 0x1d) == Right (FallbackEvent FramedTarget))
        , ("a push past the rows has no child", classOf 0x1000 2 (stage 0xe 3 0x1d) == Right (FallbackEvent NoChild))
        , ("a seeking push falls back", classOf 0x1000 0 (stage 0xe 3 0x17) == Right (FallbackEvent SeekingId))
        , ("a push from node 0x7a falls back", classOf 0x1000 0 (stage 0xe 0x7a 0x1d) == Right (FallbackEvent PropertyNode))
        , ("a pop from a node with its parent's id is owned", classOf 0x2000 0 (stage 0xf 3 0x1d) == Right (OwnedEvent Pop))
        , ("a pop from a node with another parent id falls back", classOf 0x1000 0 (stage 0xf 3 0x1d) == Right (FallbackEvent RememberBranch))
        , ("an open of a registered node is owned", classOf 0 0 (stage 1 3 0x1d) == Right (OwnedEvent Open))
        , ("an open of node 0x38 falls back", classOf 0 0 (stage 1 0x38 0x1d) == Right (FallbackEvent SeekClassNode))
        , ("an open of an empty registry slot falls back", classOf 0 0 (stage 1 4 0x1d) == Right (FallbackEvent Unregistered))
        , ("a frame move falls back by kind", classOf 0x1000 0 (stage 6 3 1) == Right (FallbackEvent (KindNotOwned 6)))
        , ("a short block refuses", isLeft (menuBlock (B.replicate 0x42f 0)))
        , ("an owned class reaching the fallback site fails the owned role",
            isLeft (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), fallback 0]]))
        , ("a fallback class at the fallback site passes the owned role",
            fmap summaryFallbackEntries (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 5 3 9), fallback 0]]) == Right 1)
        , ("the class of the entry on the fallback's own core decides",
            isLeft (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 4 3 1), entry 1 0x1000 0 (stage 5 3 9), fallback 0]]))
        , ("the stock role reaches the fallback site for every class",
            fmap summaryFallbackEntries (checkRole tree StockRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), fallback 0]]) == Right 1)
        , ("an entry to the adapter's guard trap fails the role",
            isLeft (checkRole tree OwnedRole [segment [] [Hit guardSite 0 (0, 0, 0, 0) (Just 0) []]]))
        , ("a stage entry without its block read refuses",
            isLeft (checkRole tree OwnedRole [segment [] [Hit stageEntrySite 0 (4, 3, 1, 0) Nothing []]]))
        , ("equal reads and posts pair", comparePair [segment [B.replicate 0x430 2] [post 1]] [segment [B.replicate 0x430 2] [post 1]] == Right ())
        , ("a differing block read fails the pair", isLeft (comparePair [segment [B.replicate 0x430 2] []] [segment [B.replicate 0x430 3] []]))
        , ("a differing 0x1d post fails the pair", isLeft (comparePair [segment [B.replicate 0x430 2] [post 1]] [segment [B.replicate 0x430 2] [post 2]]))
        , ("a missing engine read fails the pair", isLeft (comparePair [segment [] []] [segment [] []]))
        ]
      failures = [label | (label, passed) <- cases, not passed]
  mapM_ (putStrLn . ("FAIL: " ++)) failures
  unless (null failures) exitFailure
  putStrLn ("menu engine pair tests passed: " ++ show (length cases))
