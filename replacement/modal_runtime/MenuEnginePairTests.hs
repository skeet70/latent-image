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
-- its parent's id and whose row 1 leads to a type-5 node, a node that does not wrap, and a node
-- whose one row leads to a package node.
tree :: MenuTree
tree = menuTree (TablesDocument "image4" "strings"
  [RegistryEntry 3 (Just (Address 0x1000)), RegistryEntry 4 Nothing, RegistryEntry 6 (Just (Address 0x4000))]
  [ node 0x500 5 0 Nothing (Just [TableRow 0 (NodeChild (Address 0x1000))])
  , node 0x1000 1 0 (Just 0x500) (Just [TableRow 0 (NodeChild (Address 0x2000)), TableRow 1 (NodeChild (Address 0x3000))])
  , node 0x2000 1 0 (Just 0x1000) (Just [TableRow 0 LeafChild])
  , node 0x3000 7 5 (Just 0x1000) (Just [TableRow 0 LeafChild])
  , (node 0x4000 8 0 (Just 0x500) (Just [TableRow 0 LeafChild])) {nodeNoWrap = 1}
  , node 0x7000 1 0 (Just 0x500) (Just [TableRow 0x7d (PackageChild (Address 0x00c4a000))]) ])

-- | The rows of each test node, the block total stock keeps for it.
rowsIn :: Word32 -> Word32
rowsIn record = case record of
  0x1000 -> 2
  0 -> 0
  _ -> 1

blockBytesFor :: Word32 -> Word32 -> Word32 -> B.ByteString
blockBytesFor record cursor total =
  word record <> B.replicate 4 0 <> word cursor <> B.replicate 12 0 <> word total <> B.replicate (0x430 - 0x1c) 0
 where word value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

classOf :: Word32 -> Word32 -> StageEvent -> Either String EventClass
classOf record cursor event = (\block -> deriveClass tree block event) <$> menuBlock (blockBytesFor record cursor (rowsIn record))

stage :: Word32 -> Word32 -> Word32 -> StageEvent
stage kind slot ident = StageEvent kind slot ident 0

entry :: Int -> Word32 -> Word32 -> StageEvent -> Hit
entry core record cursor (StageEvent kind slot ident extra) =
  Hit StageEntry core (kind, slot, ident, extra) (Just 0xca4680) Nothing [blockBytesFor record cursor (rowsIn record), B.replicate 8 0]

fallback :: Int -> Hit
fallback core = Hit KindSwitch core (0, 0, 0, 0) (Just fallbackReturn) Nothing []

post :: Word32 -> Hit
post token = Hit EventPost 0 (0x1d, 0xf6, token, 0xf000) (Just 0xca47f0) Nothing []

segment :: [(B.ByteString)] -> [Hit] -> Observation
segment blocks hits = Observation "picker-keys" EngineResident
  ([(label, B.replicate 8 1) | label <- drop 1 engineReads] ++ [("menu-block", block) | block <- blocks]) hits

unloaded :: Observation -> Observation
unloaded observation = observation {observationResidency = EngineUnloaded}

entryAt :: Word32 -> Hit
entryAt sp = (entry 0 0x1000 0 (stage 4 3 1)) {hitSp = Just sp}

-- | A segment whose task-1 stack read has one word changed at `offset` (none for 0).
stacked :: Int -> [Hit] -> Observation
stacked offset hits = Observation "picker-keys" EngineResident [("task1-stack", stack)] hits
 where
  stack = if offset == 0 then B.replicate 0xc00 0 else B.replicate offset 0 <> B.pack [1, 0, 0, 0] <> B.replicate (0xc00 - offset - 4) 0

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
        , ("after unload the owned role's owned push and pop reach the fallback site like stock",
            fmap summaryFallbackEntries (checkRole tree OwnedRole
              [unloaded (segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), fallback 0, entry 0 0x2000 0 (stage 0xf 3 0x1d), fallback 0])]) == Right 2)
        , ("a guard-trap entry fails the owned role after unload too",
            isLeft (checkRole tree OwnedRole [unloaded (segment [] [Hit GuardTrap 0 (0, 0, 0, 0) (Just 0) Nothing []])]))
        , ("the segments after ram-unload are the unloaded ones",
            unloadedPhases ["emulator-start", "ram-push", "segment picker-route", "segment owned-picker-keys", "ram-unload", "segment unloaded-picker"]
              == ["unloaded-picker"])
        , ("an entry to the adapter's guard trap fails the role",
            isLeft (checkRole tree OwnedRole [segment [] [Hit GuardTrap 0 (0, 0, 0, 0) (Just 0) Nothing []]]))
        , ("a stage entry without its block read refuses",
            isLeft (checkRole tree OwnedRole [segment [] [Hit StageEntry 0 (4, 3, 1, 0) Nothing Nothing []]]))
        , ("entries within the bound and an unchanged stack pass the stack check",
            checkStack 2000 [stacked 0 [entryAt 0x00a4cc78], stacked 0 []] == Right (StackReport (Just (0x00a4cc78 - 2000)) Nothing))
        , ("an entry whose sp less the bound falls below task 1's stack fails",
            isLeft (checkStack 2000 [stacked 0 [entryAt 0x00a4c900]]))
        , ("the lowest changed stack word is the high-water mark",
            fmap stackHighWater (checkStack 2000 [stacked 0 [entryAt 0x00a4cc78], stacked 0x800 []]) == Right (Just (0x00a4c200 + 0x800)))
        , ("a high-water mark below the bound's floor fails",
            isLeft (checkStack 2000 [stacked 0 [entryAt 0x00a4cc78], stacked 0x100 []]))
        , ("a stage entry without sp refuses the stack check",
            isLeft (checkStack 2000 [stacked 0 [entry 0 0x1000 0 (stage 4 3 1)]]))
        , ("a block total other than the open node's rows falls back for every kind",
            all (\kind -> fmap (\block -> deriveClass tree block (stage kind 3 1)) (menuBlock (blockBytesFor 0x1000 0 3)) == Right (FallbackEvent RowTotal)) [4, 5, 0xe, 0xf, 1])
        , ("a push to a package node falls back", classOf 0x7000 0 (stage 0xe 3 0x1d) == Right (FallbackEvent PackageChildRow))
        , ("equal reads and posts pair", comparePair [segment [B.replicate 0x430 2] [post 1]] [segment [B.replicate 0x430 2] [post 1]] == Right ())
        , ("a differing block read fails the pair", isLeft (comparePair [segment [B.replicate 0x430 2] []] [segment [B.replicate 0x430 3] []]))
        , ("a differing 0x1d post fails the pair", isLeft (comparePair [segment [B.replicate 0x430 2] [post 1]] [segment [B.replicate 0x430 2] [post 2]]))
        , ("a missing engine read fails the pair", isLeft (comparePair [segment [] []] [segment [] []]))
        ]
      failures = [label | (label, passed) <- cases, not passed]
  mapM_ (putStrLn . ("FAIL: " ++)) failures
  unless (null failures) exitFailure
  putStrLn ("menu engine pair tests passed: " ++ show (length cases))
