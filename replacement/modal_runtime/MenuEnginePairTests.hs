{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Either (isLeft)
import qualified Data.Map.Strict as M
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
-- whose one row leads to a package node, a node whose second row leads to a package node, and a
-- child of that node; a child of the type-5 node, and a node whose row leads to a type-7 node
-- (registered at slot 7) with a package-child row.
tree :: MenuTree
tree = menuTree (TablesDocument "image4" "strings"
  [ RegistryEntry 3 (Just (Address 0x1000)), RegistryEntry 4 Nothing, RegistryEntry 6 (Just (Address 0x4000))
  , RegistryEntry 7 (Just (Address 0xa000)), RegistryEntry 8 (Just (Address 0x3000)) ]
  [ node 0x500 5 0 Nothing (Just [TableRow 0 (NodeChild (Address 0x1000))])
  , node 0x1000 1 0 (Just 0x500) (Just [TableRow 0 (NodeChild (Address 0x2000)), TableRow 1 (NodeChild (Address 0x3000))])
  , node 0x2000 1 0 (Just 0x1000) (Just [TableRow 0 LeafChild])
  , node 0x3000 7 5 (Just 0x1000) (Just [TableRow 0 LeafChild])
  , (node 0x4000 8 0 (Just 0x500) (Just [TableRow 0 LeafChild])) {nodeNoWrap = 1}
  , node 0x7000 1 0 (Just 0x500) (Just [TableRow 0x7d (PackageChild (Address 0x00c4a000))])
  , node 0x8000 1 0 (Just 0x500) (Just [TableRow 0 (NodeChild (Address 0x2000)), TableRow 1 (PackageChild (Address 0x00c4a000))])
  , node 0x9000 1 0 (Just 0x8000) (Just [TableRow 0 LeafChild])
  , node 0xc000 7 0 (Just 0x3000) (Just [TableRow 0 LeafChild])
  , node 0xb000 2 0 (Just 0x500) (Just [TableRow 0 (NodeChild (Address 0xa000))])
  , node 0xa000 9 7 (Just 0xb000) (Just [TableRow 0x7d (PackageChild (Address 0x00c4a000))]) ])

-- | The rows of each test node, the block total stock keeps for it.
rowsIn :: Word32 -> Word32
rowsIn record = case record of
  0x1000 -> 2
  0x8000 -> 2
  0 -> 0
  _ -> 1

-- | A block with the open record, cursor and total at their offsets and the stack depth at
-- `0x424`; every stack frame is zero.
blockBytesAt :: Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
blockBytesAt depth record cursor total =
  word record <> B.replicate 4 0 <> word cursor <> B.replicate 12 0 <> word total <> B.replicate (0x424 - 0x1c) 0 <> word depth <> B.replicate 8 0
 where word value = B.pack [fromIntegral (value `shiftR` shift) | shift <- [0, 8, 16, 24]]

blockBytesFor :: Word32 -> Word32 -> Word32 -> B.ByteString
blockBytesFor = blockBytesAt 1

classOfAt :: Word32 -> Word32 -> Word32 -> StageEvent -> Either String EventClass
classOfAt depth record cursor event = (\block -> deriveClass tree block event) <$> menuBlock (blockBytesAt depth record cursor (rowsIn record))

classOf :: Word32 -> Word32 -> StageEvent -> Either String EventClass
classOf = classOfAt 1

-- | The class at `depth` with the stack frame cursors replaced.
classWithStackAt :: Word32 -> [Word32] -> Word32 -> Word32 -> StageEvent -> Either String EventClass
classWithStackAt depth cursors record cursor event =
  (\block -> deriveClass tree block {blockStackCursors = cursors} event) <$> menuBlock (blockBytesAt depth record cursor (rowsIn record))

classWithStack :: [Word32] -> Word32 -> Word32 -> StageEvent -> Either String EventClass
classWithStack = classWithStackAt 1

stage :: Word32 -> Word32 -> Word32 -> StageEvent
stage kind slot ident = StageEvent kind slot ident 0

entry :: Int -> Word32 -> Word32 -> StageEvent -> Hit
entry core record cursor (StageEvent kind slot ident extra) =
  Hit StageEntry core (kind, slot, ident, extra) (Just 0xca4680) Nothing [blockBytesFor record cursor (rowsIn record), B.replicate 8 0]

fallback :: Int -> Hit
fallback core = Hit KindSwitch core (0, 0, 0, 0) (Just fallbackReturn) Nothing []

-- | An entry to `0xbfa234` as the adapter's `kind_switch` service.
serviceSwitch :: Int -> Hit
serviceSwitch core = Hit KindSwitch core (0xf, 3, 0x15, 0) (Just 0x00bfa8f0) Nothing []

seek :: Int -> Hit
seek core = Hit Seek core (0xe, 3, 5, 0x1d) (Just 0x00c40100) Nothing []

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
        , ("a row step on a node that does not wrap is owned", classOf 0x4000 0 (stage 4 6 1) == Right (OwnedEvent (RowMove Down)))
        , ("a move with no open node falls back", classOf 0 0 (stage 4 3 1) == Right (FallbackEvent NoOpenNode))
        , ("a push to the cursor row's child is owned", classOf 0x1000 0 (stage 0xe 3 0x1d) == Right (OwnedEvent Push))
        , ("a push into a type-5 node whose row children open is owned", classOf 0x1000 1 (stage 0xe 3 0x1d) == Right (OwnedEvent Push))
        , ("a push into a type-5 node restoring a cursor past its rows falls back",
            classWithStack [0, 0, 2, 0, 0, 0, 0, 0, 0, 0] 0x1000 1 (stage 0xe 3 0x1d) == Right (FallbackEvent FramedTarget))
        , ("a push into a type-7 node with a package-child row falls back", classOf 0xb000 0 (stage 0xe 3 0x1d) == Right (FallbackEvent FramedTarget))
        , ("a pop onto a type-5 parent whose parent opens is owned", classOfAt 2 0xc000 0 (stage 0xf 3 0x1a) == Right (OwnedEvent Pop))
        , ("a pop onto a type-5 parent at stack depth 1 falls back", classOf 0xc000 0 (stage 0xf 3 0x1a) == Right (FallbackEvent FramedTarget))
        , ("a pop onto a remembering type-5 parent restoring a cursor past its rows falls back",
            classWithStackAt 2 [0, 2, 0, 0, 0, 0, 0, 0, 0, 0] 0xc000 0 (stage 0xf 3 0x1a) == Right (FallbackEvent FramedTarget))
        , ("an open of a type-5 node whose row children open is owned", classOf 0 0 (stage 1 8 0) == Right (OwnedEvent Open))
        , ("an open of a type-7 node with a package-child row falls back", classOf 0 0 (stage 1 7 0) == Right (FallbackEvent FramedTarget))
        , ("an open with id 0x13 is owned", classOf 0 0 (stage 1 3 0x13) == Right (OwnedEvent Open))
        , ("a push past the rows is rejected", classOf 0x1000 2 (stage 0xe 3 0x1d) == Right (OwnedEvent Rejected))
        , ("a pop without a parent is rejected", classOf 0x500 0 (stage 0xf 5 0x1d) == Right (OwnedEvent Rejected))
        , ("a seeking push falls back", classOf 0x1000 0 (stage 0xe 3 0x17) == Right (FallbackEvent SeekingId))
        , ("a push from node 0x7a is owned", classOf 0x1000 0 (stage 0xe 0x7a 0x1d) == Right (OwnedEvent Push))
        , ("a pop from a node with its parent's id is owned", classOf 0x2000 0 (stage 0xf 3 0x1d) == Right (OwnedEvent Pop))
        , ("a pop from a node with another parent id is owned", classOf 0x1000 0 (stage 0xf 3 0x1d) == Right (OwnedEvent Pop))
        , ("a pop at stack depth 0 falls back", classOfAt 0 0x2000 0 (stage 0xf 3 0x1d) == Right (FallbackEvent StackBounds))
        , ("an open of a registered node is owned", classOf 0 0 (stage 1 3 0x1d) == Right (OwnedEvent Open))
        , ("an open of node 0x38 falls back", classOf 0 0 (stage 1 0x38 0x1d) == Right (FallbackEvent SeekClassNode))
        , ("an open of an empty registry slot falls back", classOf 0 0 (stage 1 4 0x1d) == Right (FallbackEvent Unregistered))
        , ("a close of the open node is owned", classOf 0x1000 0 (stage 2 3 0x1d) == Right (OwnedEvent Close))
        , ("a close with no open node falls back", classOf 0 0 (stage 2 3 0x1d) == Right (FallbackEvent NoOpenNode))
        , ("a group move is owned", classOf 0x1000 0 (stage 0x48 3 1) == Right (OwnedEvent (GroupMove GroupStepDown)))
        , ("a group move within four rows with the cursor past the rows falls back", classOf 0x1000 3 (stage 0x45 3 1) == Right (FallbackEvent CursorPastRows))
        , ("a four-row group step with the cursor past the rows is owned", classOf 0x1000 3 (stage 0x47 3 1) == Right (OwnedEvent (GroupMove GroupStepUp)))
        , ("a re-open of the open node is owned", classOf 0x1000 0 (stage 0x13 3 1) == Right (OwnedEvent Reopen))
        , ("a pop, move and push whose targets all open is owned", classOf 0x2000 0 (stage 6 3 1) == Right (OwnedEvent (FrameMove PopMoveDownPush)))
        , ("a pop, move and push whose inner move is a page step falls back", classOf 0x2000 0 (stage 6 3 9) == Right (FallbackEvent InnerMove))
        , ("a pop, move and push whose parent has a package-child row falls back", classOf 0x9000 0 (stage 6 3 1) == Right (FallbackEvent PackageChildRow))
        , ("a seek that makes no step, then a push, move and pop, is owned", classOf 0x1000 0 (StageEvent 0xa 3 1 0) == Right (OwnedEvent (FrameMove SeekPushMoveDownPop)))
        , ("a seek that makes no step checks only the cursor row's child", classOf 0x8000 0 (StageEvent 0xa 3 1 0) == Right (OwnedEvent (FrameMove SeekPushMoveDownPop)))
        , ("a seek whose extra word differs from the cursor token checks every row's child", classOf 0x8000 0 (StageEvent 0xa 3 1 1) == Right (FallbackEvent PackageChildRow))
        , ("a seek on a type-5 node falls back", classOf 0x3000 0 (StageEvent 0xa 3 1 0) == Right (FallbackEvent FramedSource))
        , ("a push, move and pop into a package node falls back", classOf 0x7000 0 (stage 8 3 1) == Right (FallbackEvent PackageChildRow))
        , ("a push, move and pop at stack depth 9 falls back", classOfAt 9 0x1000 0 (stage 8 3 1) == Right (FallbackEvent StackBounds))
        , ("a push, move and pop restoring a cursor past the child's rows falls back",
            classWithStack [0, 0, 2, 0, 0, 0, 0, 0, 0, 0] 0x1000 0 (stage 8 3 1) == Right (FallbackEvent CursorPastRows))
        , ("a push, move and pop is owned", classOf 0x1000 0 (stage 8 3 1) == Right (OwnedEvent (FrameMove PushMoveDownPop)))
        , ("a push, move up and pop is owned", classOf 0x1000 0 (stage 9 3 1) == Right (OwnedEvent (FrameMove PushMoveUpPop)))
        , ("an unowned kind falls back by kind", classOf 0x1000 0 (stage 3 3 1) == Right (FallbackEvent (KindNotOwned 3)))
        , ("coverage counts each class by residency and ownership",
            fmap (M.toList . summaryClasses)
              (checkRole tree StockRole [segment [] [entry 0 0x1000 0 (stage 2 3 0x1d), entry 0 0x1000 0 (stage 3 3 1)], unloaded (segment [] [entry 0 0x1000 0 (stage 2 3 0x1d)])])
              == Right
                [ (Coverage EngineResident False "FallbackEvent (KindNotOwned 3)", 1)
                , (Coverage EngineResident True "OwnedEvent Close", 1)
                , (Coverage EngineUnloaded True "OwnedEvent Close", 1) ])
        , ("a short block refuses", isLeft (menuBlock (B.replicate 0x42f 0)))
        , ("an owned class reaching the fallback site fails the owned role",
            isLeft (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), fallback 0]]))
        , ("a fallback class at the fallback site passes the owned role",
            fmap summaryFallbackEntries (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 5 3 9), fallback 0]]) == Right 1)
        , ("the class of the entry on the fallback's own core decides",
            isLeft (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 4 3 1), entry 1 0x1000 0 (stage 5 3 9), fallback 0]]))
        , ("the stock role reaches the fallback site for every class",
            fmap summaryFallbackEntries (checkRole tree StockRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), fallback 0]]) == Right 1)
        , ("a seek entry for an owned class fails the owned role",
            isLeft (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), seek 0]]))
        , ("a seek entry after a kind-switch service entry passes the owned role",
            fmap summaryEvents (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), serviceSwitch 0, seek 0]]) == Right 1)
        , ("a kind-switch service entry exempts only the seeks of its own stage entry",
            isLeft (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), serviceSwitch 0, entry 0 0x1000 0 (stage 4 3 1), seek 0]]))
        , ("a seek entry for a fallback class passes the owned role",
            fmap summaryEvents (checkRole tree OwnedRole [segment [] [entry 0 0x1000 0 (stage 5 3 9), fallback 0, seek 0]]) == Right 1)
        , ("the stock role reaches the seek for every class",
            fmap summaryEvents (checkRole tree StockRole [segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), seek 0]]) == Right 1)
        , ("after unload the owned role's owned push and pop reach the fallback site like stock",
            fmap summaryFallbackEntries (checkRole tree OwnedRole
              [unloaded (segment [] [entry 0 0x1000 0 (stage 0xe 3 0x1d), fallback 0, entry 0 0x2000 0 (stage 0xf 3 0x1d), fallback 0])]) == Right 2)
        , ("a guard-trap entry fails the owned role after unload too",
            isLeft (checkRole tree OwnedRole [unloaded (segment [] [Hit GuardTrap 0 (0, 0, 0, 0) (Just 0) Nothing []])]))
        , ("a segment's residency is that of its position, so a phase repeated after ram-unload is unloaded there",
            let segments = segmentResidencies ["ram-push", "segment keys", "segment check", "ram-unload", "segment keys"]
             in map (\(index, phase) -> residencyAt segments index phase) [(0, "keys"), (1, "check"), (2, "keys")]
                  == [Right EngineResident, Right EngineResident, Right EngineUnloaded])
        , ("an observation whose phase is not the run's segment at its position refuses",
            isLeft (residencyAt (segmentResidencies ["segment keys", "ram-unload", "segment check"]) 1 "keys"))
        , ("an observation past the run's segments refuses",
            isLeft (residencyAt (segmentResidencies ["segment keys"]) 1 "keys"))
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
