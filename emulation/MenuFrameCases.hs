{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- | Original-versus-owned X-M1 1.50 menu-frame snapshot cases: FUN_0061ca2c,
-- FUN_0061cd14, FUN_0061ceac and the eight word accessors against the owned
-- exports of replacement/owned_menu/src/lib.rs. Every expectation is a
-- literal read from the 1.50 bytes and node data, independent of either body.
module Main where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (object, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , Register (..)
  , RegionId (..)
  , RunnerStatus (..)
  , hexWord32
  , registerLabel
  , runnerStatusLabel
  )
import Emulation.OwnedCase
import FujiAF.JSON (encodeCanonical)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

-- * Guest layout

codeBase, codeEnd, nodesBase, nodesEnd, slotsBase, cellsBase, destBase, syntheticBase, guestBase, exitAddr, stackBase, loaderBase :: GuestAddress
codeBase = GuestAddress 0x0061ca2c
codeEnd = GuestAddress 0x0061d384
nodesBase = GuestAddress 0x00776000
nodesEnd = GuestAddress 0x00792000
slotsBase = GuestAddress 0x0093e000
cellsBase = GuestAddress 0x009571d0
destBase = GuestAddress 0x00958000
syntheticBase = GuestAddress 0x00958800
guestBase = GuestAddress 0x00958c00
exitAddr = GuestAddress 0x00958f00
stackBase = GuestAddress 0x00959000
loaderBase = GuestAddress 0x00040000

slotsSize, cellsSize, destSize, destOffset, syntheticSize, stackSize, callerFrameOffset :: Int
slotsSize = 0x7000
cellsSize = 0x160
destSize = 0x40
destOffset = 8
syntheticSize = 0x100
stackSize = 0x1000
callerFrameOffset = 0xff0

stackPointer :: Word32
stackPointer = guestWord stackBase + fromIntegral callerFrameOffset

slotsFill, destFill, stackFill :: B.ByteString
slotsFill = B.replicate slotsSize 0xee
destFill = B.replicate destSize 0x5a
stackFill = B.replicate stackSize 0x3c

-- | Offsets inside the cells region at 0x009571d0.
frameCellOffset, cursorCellOffset, visibleCellOffset, scrollCellOffset, navigationCellOffset, currentRowCellOffset :: Int
frameCellOffset = 0
cursorCellOffset = 8
visibleCellOffset = 0xc
scrollCellOffset = 0x10
navigationCellOffset = 0x14
currentRowCellOffset = 0x15c

-- | 1.50 entry words that identify the supplied images.
anchors :: [(GuestAddress, Word32)]
anchors =
  [ (GuestAddress 0x0061ca2c, 0xe52db004)
  , (GuestAddress 0x0061cd14, 0xe92d4830)
  , (GuestAddress 0x0061ceac, 0xe92d4818)
  , (GuestAddress 0x0061d0e4, 0xe92d4800)
  ]

loaderDivideEntry :: Word32
loaderDivideEntry = 0xe2512001

loaderFunctions :: [(GuestAddress, Word32)]
loaderFunctions =
  [ (GuestAddress 0x0004d978, 244)
  , (GuestAddress 0x00043604, 16)
  , (GuestAddress 0x00043614, 12)
  ]

-- * Snapshot contract

newtype NodeId = NodeId Word32

newtype MenuClass = MenuClass Word32

-- | The eleven words in FUN_0061cd14's order.
newtype Snapshot = Snapshot [Word32]

snapshot :: [Word32] -> Snapshot
snapshot words'
  | length words' == 11 = Snapshot words'
  | otherwise = error "a menu-frame snapshot has eleven words"

snapshotBytes :: Snapshot -> B.ByteString
snapshotBytes (Snapshot words') = assemble words'

data SnapshotWord
  = DepthWord
  | NodeWord
  | NodeTypeWord
  | ClassWord
  | CursorTokenWord
  | CurrentTokenWord
  | PageWord
  | NavigationWord
  deriving (Bounded, Enum)

wordAt :: Snapshot -> SnapshotWord -> Word32
wordAt (Snapshot words') field = words' !! index
  where
    index = case field of
      DepthWord -> 0
      NodeWord -> 1
      NodeTypeWord -> 2
      ClassWord -> 3
      CursorTokenWord -> 4
      CurrentTokenWord -> 5
      PageWord -> 9
      NavigationWord -> 10

stockAccessor :: SnapshotWord -> GuestAddress
stockAccessor field = GuestAddress $ case field of
  DepthWord -> 0x0061d0e4
  NodeWord -> 0x0061d154
  NodeTypeWord -> 0x0061d1a4
  ClassWord -> 0x0061d1f4
  CursorTokenWord -> 0x0061d244
  CurrentTokenWord -> 0x0061d294
  PageWord -> 0x0061d2e4
  NavigationWord -> 0x0061d334

ownedAccessor :: SnapshotWord -> String
ownedAccessor field = case field of
  DepthWord -> "rb_menu_frame_depth"
  NodeWord -> "rb_menu_frame_node"
  NodeTypeWord -> "rb_menu_frame_node_type"
  ClassWord -> "rb_menu_frame_class"
  CursorTokenWord -> "rb_menu_frame_cursor_token"
  CurrentTokenWord -> "rb_menu_frame_current_token"
  PageWord -> "rb_menu_frame_page"
  NavigationWord -> "rb_menu_frame_navigation"

accessorLabel :: SnapshotWord -> String
accessorLabel field = case field of
  DepthWord -> "depth"
  NodeWord -> "node"
  NodeTypeWord -> "node-type"
  ClassWord -> "class"
  CursorTokenWord -> "cursor-token"
  CurrentTokenWord -> "current-token"
  PageWord -> "page"
  NavigationWord -> "navigation"

data SyntheticParent = SyntheticParent NodeId MenuClass

-- | Id, node type, class, rows per page, parent and `{token, child}` rows; a
-- missing row list leaves the values pointer null.
data SyntheticNode = SyntheticNode NodeId Word32 MenuClass Word32 (Maybe SyntheticParent) (Maybe [(Word32, Word32)])

data Frames = NoFrame | RealFrame GuestAddress | SyntheticFrame SyntheticNode

data Cells = Cells
  { cursorCell :: Word32
  , visibleCell :: Word32
  , scrollCell :: Word32
  , navigationCell :: Word32
  , currentRowCell :: Word32
  }

data Completion = Completes | StopsAt RunnerStatus

data StoreSlot = SlotAt GuestAddress | NullSlot

data Destination = Buffer | NullDestination

data Operation = Classify | SnapshotInto Destination | Store | Accessor SnapshotWord

data Scenario = Scenario
  { scenarioId :: String
  , scenarioProvenance :: String
  , scenarioFrames :: Frames
  , scenarioCells :: Cells
  , scenarioRoot :: NodeId
  , scenarioCompletion :: Completion
  , scenarioSnapshot :: Snapshot
  , scenarioSlot :: StoreSlot
  , scenarioOperations :: [Operation]
  }

scenario :: String -> String -> Frames -> Cells -> NodeId -> Completion -> Snapshot -> StoreSlot -> Scenario
scenario identifier provenance frames cells root completion words' slot =
  Scenario identifier provenance frames cells root completion words' slot (operationsFor completion)

operationsFor :: Completion -> [Operation]
operationsFor Completes = [Classify, SnapshotInto Buffer, Store] ++ map Accessor [minBound .. maxBound]
operationsFor (StopsAt _) = [Classify, SnapshotInto Buffer, Store, Accessor CursorTokenWord]

operationLabel :: Operation -> String
operationLabel Classify = "classify"
operationLabel (SnapshotInto Buffer) = "snapshot"
operationLabel (SnapshotInto NullDestination) = "snapshot-null"
operationLabel Store = "store"
operationLabel (Accessor field) = "word-" ++ accessorLabel field

-- | Real frames are X-M1 1.50 menu nodes in image 4 (package id4-final). Each
-- expected snapshot is read from that node, its parent and values rows.
scenarios :: [Scenario]
scenarios =
  [ scenario "no-frame" "no current frame: the detached defaults" NoFrame
      (Cells 0 0 0 0 0) (NodeId 0x10) Completes
      (snapshot [0x1, 0x86, 0xc, 0, 0, 0, 0, 0, 0, 0, 0]) (SlotAt (GuestAddress 0x93ee34))
  , scenario "root-menu" "menu root node, id 0 and class 5" (RealFrame (GuestAddress 0x787acc))
      (Cells 0x2 0x2 0x0 0x0 0x5) (NodeId 0x0) Completes
      (snapshot [0x0, 0x0, 0x0, 0x5, 0xba, 0xbd, 0x2, 0x2, 0x0, 0x3, 0x0]) (SlotAt (GuestAddress 0x944414))
  , (scenario "shooting-page" "shooting-menu page, class 5 under the root" (RealFrame (GuestAddress 0x78ea98))
      (Cells 0x3 0x3 0x0 0x1 0x0) (NodeId 0x1) Completes
      (snapshot [0x1, 0x1, 0x0, 0x5, 0x3c, 0x37, 0x3, 0x3, 0x0, 0x1, 0x1]) (SlotAt (GuestAddress 0x93eba0)))
      { scenarioOperations = operationsFor Completes ++ [SnapshotInto NullDestination] }
  , scenario "iso-token-1b" "ISO list, class 0x141, on the row whose token is 0x1b" (RealFrame (GuestAddress 0x77e54c))
      (Cells 0x14 0x6 0xe 0x0 0x14) (NodeId 0x63) Completes
      (snapshot [0x2, 0x63, 0x9, 0x141, 0x1b, 0x1b, 0x14, 0x6, 0xe, 0x1, 0x0]) (SlotAt (GuestAddress 0x9423d8))
  , scenario "iso-other-row" "ISO list, class 0x141, parentless, on a row without a child" (RealFrame (GuestAddress 0x77e54c))
      (Cells 0x9 0x2 0x7 0x0 0x3) (NodeId 0x63) Completes
      (snapshot [0x2, 0x63, 0x9, 0x141, 0x10, 0x16, 0x9, 0x2, 0x7, 0x1, 0x0]) (SlotAt (GuestAddress 0x9423d8))
  , scenario "ed-token-3" "class 0xed list on the row whose token is 3" (RealFrame (GuestAddress 0x78fe5c))
      (Cells 0x2 0x2 0x0 0x0 0x2) (NodeId 0x62) Completes
      (snapshot [0x2, 0x62, 0x8, 0xed, 0x3, 0x3, 0x2, 0x2, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x9423ac))
  , scenario "ed-other-row" "class 0xed list whose parent has the same id" (RealFrame (GuestAddress 0x78fe5c))
      (Cells 0x1 0x1 0x0 0x0 0x2) (NodeId 0x62) Completes
      (snapshot [0x2, 0x62, 0x8, 0xed, 0x1, 0x3, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x9423ac))
  , scenario "e5-token-0" "class 0xe5 list on the row whose token is 0" (RealFrame (GuestAddress 0x787044))
      (Cells 0x0 0x0 0x0 0x0 0x1) (NodeId 0x7) Completes
      (snapshot [0x2, 0x7, 0x1, 0xe5, 0x0, 0x4, 0x0, 0x0, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x941408))
  , scenario "e5-other-row" "class 0xe5 list under a class-5 page, on a row without a child" (RealFrame (GuestAddress 0x787044))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x7) Completes
      (snapshot [0x2, 0x7, 0x1, 0xe5, 0x4, 0x0, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x941408))
  , scenario "class7-token-1" "class 7 list on the row whose token is 1" (RealFrame (GuestAddress 0x78eff8))
      (Cells 0x1 0x1 0x0 0x0 0x1) (NodeId 0x4e) Completes
      (snapshot [0x1, 0x4e, 0x0, 0x7, 0x1, 0x1, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x93f8dc))
  , scenario "class7-child-row" "class 7 list under a class-5 page, on a row with a child" (RealFrame (GuestAddress 0x78eff8))
      (Cells 0x0 0x0 0x0 0x0 0x1) (NodeId 0x4e) Completes
      (snapshot [0x1, 0x4e, 0x0, 0x7, 0x0, 0x1, 0x0, 0x0, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x93f8dc))
  , scenario "class7-terminator-row" "class 7 list on its 0x7ffffffe terminator row" (RealFrame (GuestAddress 0x78eff8))
      (Cells 0x5 0x5 0x0 0x0 0x2) (NodeId 0x4e) Completes
      (snapshot [0x2, 0x4e, 0x0, 0x7, 0x7ffffffe, 0x2, 0x5, 0x5, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x94203c))
  , scenario "class47-page" "class 0x47 page" (RealFrame (GuestAddress 0x77c178))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x37) Completes
      (snapshot [0x1, 0x37, 0x3, 0x47, 0x4, 0x0, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x93f4e8))
  , scenario "leaf-parent-same-id" "class 0x16c list whose class-5 parent has the same id" (RealFrame (GuestAddress 0x77c14c))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x1) Completes
      (snapshot [0x2, 0x1, 0x0, 0x16c, 0x1, 0x0, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x941300))
  , (scenario "store-root-wraps-to-null" "depth-2 store whose root makes 0x009412d4 + root * 0x2c wrap to 0" (RealFrame (GuestAddress 0x77c14c))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x3459b9c1) Completes
      (snapshot [0x2, 0x1, 0x0, 0x16c, 0x1, 0x0, 0x1, 0x1, 0x0, 0x1, 0x0]) NullSlot)
      { scenarioOperations = [Store] }
  , scenario "page-row-with-submenu" "class 0x55 list under a class-5 page, on a row with a child" (RealFrame (GuestAddress 0x784860))
      (Cells 0x0 0x0 0x0 0x0 0x3) (NodeId 0x80) Completes
      (snapshot [0x1, 0x80, 0x0, 0x55, 0x0, 0x3, 0x0, 0x0, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x940174))
  , scenario "parent-other-class" "class 0x69 list whose parent has class 0x56 and another id" (RealFrame (GuestAddress 0x77d7dc))
      (Cells 0x0 0x0 0x0 0x0 0x0) (NodeId 0x84) Completes
      (snapshot [0x1, 0x84, 0x0, 0x69, 0x0, 0x0, 0x0, 0x0, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x940224))
  , scenario "parent-null-no-child" "parentless class 0x1d8 list on a row without a child" (RealFrame (GuestAddress 0x77c0f0))
      (Cells 0x3 0x3 0x0 0x0 0x1) (NodeId 0x10) Completes
      (snapshot [0x2, 0x10, 0x4, 0x1d8, 0x9, 0x0, 0x3, 0x3, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x941594))
  , scenario "class43-list" "class 0x43 list" (RealFrame (GuestAddress 0x7864a8))
      (Cells 0x4 0x4 0x0 0x0 0x2) (NodeId 0x47) Completes
      (snapshot [0x2, 0x47, 0x0, 0x43, 0x4, 0x0, 0x4, 0x4, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x941f08))
  , scenario "values-null" "class 0x220 frame without a values array"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x220) 0x7 Nothing Nothing))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x91) Completes
      (snapshot [0x1, 0x91, 0x2, 0x220, 0x0, 0x0, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x940460))
  , scenario "class166-token0" "class 0x166 frame on the row whose token is 0"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x166) 0x7 Nothing (Just [(0x0, 0x0), (0x1, 0x0)])))
      (Cells 0x0 0x0 0x0 0x0 0x1) (NodeId 0x91) Completes
      (snapshot [0x1, 0x91, 0x2, 0x166, 0x0, 0x1, 0x0, 0x0, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x940460))
  , scenario "class166-other-row" "class 0x166 parentless frame on a row without a child"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x166) 0x7 Nothing (Just [(0x0, 0x0), (0x1, 0x0)])))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x91) Completes
      (snapshot [0x2, 0x91, 0x2, 0x166, 0x1, 0x0, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x942bc0))
  , scenario "class27-token0" "class 0x27 frame on the row whose token is 0"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x27) 0x7 Nothing (Just [(0x0, 0x958880), (0x5, 0x958880)])))
      (Cells 0x0 0x0 0x0 0x0 0x1) (NodeId 0x91) Completes
      (snapshot [0x2, 0x91, 0x2, 0x27, 0x0, 0x5, 0x0, 0x0, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x942bc0))
  , scenario "class27-child-row" "class 0x27 parentless frame on a row with a child"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x27) 0x7 Nothing (Just [(0x0, 0x958880), (0x5, 0x958880)])))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x91) Completes
      (snapshot [0x1, 0x91, 0x2, 0x27, 0x5, 0x0, 0x1, 0x1, 0x0, 0x1, 0x0]) (SlotAt (GuestAddress 0x940460))
  , scenario "parent-page-no-child" "generic class under a class-5 parent with another id, on a row without a child"
      (SyntheticFrame (SyntheticNode (NodeId 0x93) 0x1 (MenuClass 0x300) 0x7 (Just (SyntheticParent (NodeId 0x92) (MenuClass 0x5))) (Just [(0x0, 0x958840), (0x1, 0x0)])))
      (Cells 0x1 0x1 0x0 0x2 0x0) (NodeId 0x93) Completes
      (snapshot [0x2, 0x93, 0x1, 0x300, 0x1, 0x0, 0x1, 0x1, 0x0, 0x1, 0x2]) (SlotAt (GuestAddress 0x942c18))
  , scenario "parent-page-child" "generic class under a class-5 parent with another id, on a row with a child"
      (SyntheticFrame (SyntheticNode (NodeId 0x93) 0x1 (MenuClass 0x300) 0x7 (Just (SyntheticParent (NodeId 0x92) (MenuClass 0x5))) (Just [(0x0, 0x958840), (0x1, 0x0)])))
      (Cells 0x0 0x0 0x0 0x2 0x1) (NodeId 0x93) Completes
      (snapshot [0x1, 0x93, 0x1, 0x300, 0x0, 0x1, 0x0, 0x0, 0x0, 0x1, 0x2]) (SlotAt (GuestAddress 0x9404b8))
  , scenario "page-wrap" "visible ordinal 0xfffffffc wraps rows + ordinal to 3"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x220) 0x7 Nothing (Just [(0x10, 0x0), (0x11, 0x0)])))
      (Cells 0x1 0xfffffffc 0x5 0x3 0x0) (NodeId 0x91) Completes
      (snapshot [0x1, 0x91, 0x2, 0x220, 0x11, 0x10, 0x1, 0xfffffffc, 0x5, 0x0, 0x3]) (SlotAt (GuestAddress 0x940460))
  , scenario "scrolled-page" "six rows per page, visible ordinal 0x13"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x220) 0x6 Nothing (Just [(0x10, 0x0), (0x11, 0x0)])))
      (Cells 0x1 0x13 0xc 0x3 0x0) (NodeId 0x91) Completes
      (snapshot [0x1, 0x91, 0x2, 0x220, 0x11, 0x10, 0x1, 0x13, 0xc, 0x4, 0x3]) (SlotAt (GuestAddress 0x940460))
  , scenario "zero-rows" "zero rows per page enters the loader divide-by-zero SVC after word 8"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x220) 0x0 Nothing (Just [(0x10, 0x0), (0x11, 0x0)])))
      (Cells 0x1 0x1 0x0 0x0 0x0) (NodeId 0x91) (StopsAt UnsupportedInstruction)
      (snapshot [0x1, 0x91, 0x2, 0x220, 0x11, 0x10, 0x1, 0x1, 0x0, 0x0, 0x0]) (SlotAt (GuestAddress 0x940460))
  , scenario "cursor-unmapped" "cursor 0x1000 indexes past the values array into unmapped memory after word 3"
      (SyntheticFrame (SyntheticNode (NodeId 0x91) 0x2 (MenuClass 0x220) 0x7 Nothing (Just [(0x10, 0x0), (0x11, 0x0)])))
      (Cells 0x1000 0x1 0x0 0x0 0x0) (NodeId 0x91) (StopsAt MemoryGuard)
      (snapshot [0x1, 0x91, 0x2, 0x220, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0]) (SlotAt (GuestAddress 0x940460))
  ]

-- * Regions

syntheticImage :: SyntheticNode -> B.ByteString
syntheticImage (SyntheticNode (NodeId ident) nodeType (MenuClass cls) rows parent values) =
  pad 0x40 node <> pad 0x40 parentBytes <> pad 0x80 valueBytes
  where
    base = guestWord syntheticBase
    node = assemble [ident, nodeType, cls, rows, 0, 0, 0, maybe 0 (const (base + 0x40)) parent, maybe 0 (const (base + 0x80)) values]
    parentBytes = case parent of
      Just (SyntheticParent (NodeId parentId) (MenuClass parentClass)) -> assemble [parentId, 0, parentClass, 7]
      Nothing -> B.empty
    valueBytes = assemble (concat [[token, child] | (token, child) <- fromMaybe [] values])
    pad size bytes
      | B.length bytes <= size = bytes <> B.replicate (size - B.length bytes) 0
      | otherwise = error "synthetic node layout overflow"

frameAddress :: Frames -> Word32
frameAddress NoFrame = 0
frameAddress (RealFrame address) = guestWord address
frameAddress (SyntheticFrame _) = guestWord syntheticBase

cellsImage :: Frames -> Cells -> B.ByteString
cellsImage frames cells = foldl poke (B.replicate cellsSize 0)
  [ (frameCellOffset, frameAddress frames)
  , (cursorCellOffset, cursorCell cells)
  , (visibleCellOffset, visibleCell cells)
  , (scrollCellOffset, scrollCell cells)
  , (navigationCellOffset, navigationCell cells)
  , (currentRowCellOffset, currentRowCell cells)
  ]
  where
    poke bytes (offset, value) = B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

data Images = Images
  { imageCode :: B.ByteString
  , imageNodes :: B.ByteString
  , imageLoader :: B.ByteString
  }

data Implementation = Original | Owned

implementationLabel :: Implementation -> String
implementationLabel Original = "stock"
implementationLabel Owned = "owned"

sharedRegions :: Images -> Scenario -> [CaseRegion]
sharedRegions images sc =
  [ mkRegion (RegionId "slots") slotsBase slotsSize ReadWrite slotsFill
  , mkRegion (RegionId "cells") cellsBase cellsSize ReadOnly (cellsImage (scenarioFrames sc) (scenarioCells sc))
  , mkRegion (RegionId "dest") destBase destSize ReadWrite destFill
  , mkRegion (RegionId "loader") loaderBase (B.length (imageLoader images)) ReadExecute (imageLoader images)
  , mkRegion (RegionId "stack") stackBase stackSize ReadWrite stackFill
  , mkRegion (RegionId "exit") exitAddr 4 ReadExecute (word32le 0xe12fff1e)
  ]
    ++ case scenarioFrames sc of
      NoFrame -> []
      RealFrame _ -> [mkRegion (RegionId "nodes") nodesBase (B.length (imageNodes images)) ReadOnly (imageNodes images)]
      SyntheticFrame node -> [mkRegion (RegionId "synthetic") syntheticBase syntheticSize ReadOnly (syntheticImage node)]

stockFunctions :: [(GuestAddress, Word32)]
stockFunctions =
  [ (GuestAddress 0x0061ca2c, 744)
  , (GuestAddress 0x0061cd14, 264)
  , (GuestAddress 0x0061ceac, 104)
  ]
    ++ [(stockAccessor field, 80) | field <- [minBound .. maxBound]]
    ++ loaderFunctions

ownedSymbols :: [String]
ownedSymbols =
  [ "rb_menu_frame_classify"
  , "rb_menu_frame_snapshot"
  , "rb_menu_frame_store"
  , "rb_menu_frame_store_root_depth"
  , "rb_menu_frame_store_first_depth"
  , "rb_menu_frame_store_second_depth"
  , "rb_call_loader_uidiv"
  ]
    ++ map ownedAccessor [minBound .. maxBound]

requiredSymbols :: [String]
requiredSymbols = "__rb_image_start" : ownedSymbols

data Context = Context
  { contextRunner :: FilePath
  , contextOutput :: FilePath
  , contextSymbols :: M.Map String Symbol
  , contextKernelText :: B.ByteString
  , contextImages :: Images
  }

entryOf :: Context -> Implementation -> Operation -> GuestAddress
entryOf _ Original operation = case operation of
  Classify -> GuestAddress 0x0061ca2c
  SnapshotInto _ -> GuestAddress 0x0061cd14
  Store -> GuestAddress 0x0061ceac
  Accessor field -> stockAccessor field
entryOf context Owned operation = symbolAddress (contextSymbols context) $ case operation of
  Classify -> "rb_menu_frame_classify"
  SnapshotInto _ -> "rb_menu_frame_snapshot"
  Store -> "rb_menu_frame_store"
  Accessor field -> ownedAccessor field

argumentOf :: Scenario -> Operation -> Word32
argumentOf _ (SnapshotInto Buffer) = guestWord destBase + fromIntegral destOffset
argumentOf sc Store = let NodeId root = scenarioRoot sc in root
argumentOf _ _ = 0

-- | The owned run maps the stock bodies without execute permission.
implementationRegions :: Context -> Implementation -> [CaseRegion]
implementationRegions context Original =
  [mkRegion (RegionId "code") codeBase (B.length (imageCode (contextImages context))) ReadExecute (imageCode (contextImages context))]
implementationRegions context Owned =
  [ mkRegion (RegionId "code") codeBase (B.length (imageCode (contextImages context))) ReadOnly (imageCode (contextImages context))
  , mkRegion (RegionId "ktext") (symbolAddress (contextSymbols context) "__rb_image_start") (B.length (contextKernelText context)) ReadExecute (contextKernelText context)
  ]

implementationFunctions :: Context -> Implementation -> [(GuestAddress, Word32)]
implementationFunctions _ Original = stockFunctions
implementationFunctions context Owned = map (functionSymbol (contextSymbols context)) ownedSymbols ++ loaderFunctions

sentinel :: Int -> Word32
sentinel n = 0xa5a50000 + fromIntegral n

calleeSaved :: [(Register, Word32)]
calleeSaved = [(R4, sentinel 4), (R5, sentinel 5), (R6, sentinel 6), (R7, sentinel 7), (R8, sentinel 8), (R9, sentinel 9), (R10, sentinel 10), (R11, sentinel 11)]

initialRegisters :: Word32 -> [(Register, Word32)]
initialRegisters argument =
  [(R0, argument), (R1, 0), (R2, 0), (R3, 0)] ++ calleeSaved
    ++ [(R12, 0), (Sp, stackPointer), (Lr, guestWord exitAddr), (Cpsr, 0x10)]

exclusion :: String
exclusion = "one call into the menu-frame snapshot service; the current frame, its values and the navigation cells are fixed read-only inputs; the loader divide runs from its 1.50 bytes; no scheduler, interrupt or concurrent frame change"

-- * Expectations

data Expectation = Expectation
  { expectStatus :: RunnerStatus
  , expectReturn :: Maybe Word32
  , expectDest :: B.ByteString
  , expectSlots :: B.ByteString
  }

overlay :: B.ByteString -> Int -> B.ByteString -> B.ByteString
overlay bytes offset value = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

completionStatus :: Completion -> RunnerStatus
completionStatus Completes = Returned
completionStatus (StopsAt status) = status

expectation :: Scenario -> Operation -> Expectation
expectation sc operation = case operation of
  Classify -> unchanged Returned (Just (wordAt words' DepthWord))
  SnapshotInto NullDestination -> unchanged Returned (Just 0)
  SnapshotInto Buffer ->
    Expectation status (returned 1) (overlay destFill destOffset (snapshotBytes words')) slotsFill
  Store -> case scenarioSlot sc of
    NullSlot -> unchanged status (returned 0)
    SlotAt (GuestAddress address) ->
      Expectation status (returned 1) destFill
        (overlay slotsFill (fromIntegral address - fromIntegral (guestAddress slotsBase)) (snapshotBytes words'))
  Accessor field -> unchanged status (returned (wordAt words' field))
  where
    words' = scenarioSnapshot sc
    status = completionStatus (scenarioCompletion sc)
    returned value = case scenarioCompletion sc of
      Completes -> Just value
      StopsAt _ -> Nothing
    unchanged status' value = Expectation status' value destFill slotsFill

problems :: Expectation -> NativeObservation -> [String]
problems expected observation =
  [ "status " ++ runnerStatusLabel (nativeStatus observation) ++ ", not " ++ runnerStatusLabel (expectStatus expected)
  | nativeStatus observation /= expectStatus expected
  ]
    ++ concat
      [ [ "r0 " ++ hexWord32 actual ++ ", not " ++ hexWord32 value
        | let actual = registerValue observation R0
        , actual /= value
        ]
          ++ [ "register " ++ registerLabel register ++ " " ++ hexWord32 actual ++ ", not " ++ hexWord32 value'
             | (register, value') <- calleeSaved ++ [(Sp, stackPointer), (Pc, guestWord exitAddr)]
             , let actual = registerValue observation register
             , actual /= value'
             ]
          ++ [ "control state " ++ hexWord32 control ++ ", not 0x00000010"
             | let control = cpsrControl (registerValue observation Cpsr)
             , control /= 0x10
             ]
      | nativeStatus observation == Returned
      , Just value <- [expectReturn expected]
      ]
    ++ [ "dest bytes differ: " ++ show (changedWords (expectDest expected) actual)
       | let actual = regionBytesOf observation (RegionId "dest")
       , actual /= expectDest expected
       ]
    ++ [ "slot bytes differ: " ++ show (changedWords (expectSlots expected) actual)
       | let actual = regionBytesOf observation (RegionId "slots")
       , actual /= expectSlots expected
       ]
    ++ [ "caller frame above sp changed"
       | B.drop callerFrameOffset (regionBytesOf observation (RegionId "stack")) /= B.drop callerFrameOffset stackFill
       ]

-- | Word offsets whose bytes differ, at most eight.
changedWords :: B.ByteString -> B.ByteString -> [String]
changedWords expected actual =
  take 8
    [ hexWord32 (fromIntegral offset) ++ "=" ++ hexWord32 (readWordAt actual offset)
    | offset <- [0, 4 .. min (B.length expected) (B.length actual) - 4]
    , B.take 4 (B.drop offset expected) /= B.take 4 (B.drop offset actual)
    ]

-- | Bytes below the entry sp that the call left changed.
stackDepth :: NativeObservation -> Int
stackDepth observation =
  case [offset | offset <- [0 .. callerFrameOffset - 1], B.index stack offset /= B.index stackFill offset] of
    [] -> 0
    (lowest : _) -> callerFrameOffset - lowest
  where
    stack = regionBytesOf observation (RegionId "stack")

-- * Runs

caseSpec :: Context -> Implementation -> Scenario -> Operation -> CaseSpec
caseSpec context implementation sc operation =
  CaseSpec
    { caseId = "mf-" ++ scenarioId sc ++ "-" ++ operationLabel operation ++ "-" ++ implementationLabel implementation
    , caseProvenance = "menu-frame case " ++ scenarioId sc ++ ": " ++ scenarioProvenance sc ++ "; X-M1 1.50 FUN_0061ca2c/FUN_0061cd14/FUN_0061ceac and accessors 0x0061d0e4..0x0061d384 from image 4, unsigned divide FUN_0004d978 from loader A"
    , caseExclusion = exclusion
    , caseEntry = entryOf context implementation operation
    , caseExit = exitAddr
    , caseRegions = sharedRegions (contextImages context) sc ++ implementationRegions context implementation
    , caseFunctions = implementationFunctions context implementation
    , caseInitial = initialRegisters (argumentOf sc operation)
    , caseExpectedStatus = expectStatus expected
    , caseExpectedRegisters = maybe [(R4, sentinel 4)] (\value -> [(R0, value)]) (expectReturn expected)
    , caseExpectedMemory = []
    }
  where
    expected = expectation sc operation

data Outcome = Outcome
  { outcomeId :: String
  , outcomeStock :: NativeObservation
  , outcomeOwned :: NativeObservation
  , outcomeProblems :: [String]
  }

runSpec :: Context -> CaseSpec -> IO NativeObservation
runSpec context spec = do
  path <- writeCase (contextOutput context </> caseId spec) spec
  loadObserved (contextRunner context) path

runOperation :: Context -> Scenario -> Operation -> IO Outcome
runOperation context sc operation = do
  stock <- runSpec context (caseSpec context Original sc operation)
  owned <- runSpec context (caseSpec context Owned sc operation)
  let expected = expectation sc operation
      tagged label = map ((label ++ ": ") ++)
  pure Outcome
    { outcomeId = scenarioId sc ++ "/" ++ operationLabel operation
    , outcomeStock = stock
    , outcomeOwned = owned
    , outcomeProblems =
        tagged "stock" (problems expected stock)
          ++ tagged "owned" (problems expected owned)
          ++ [ "owned status " ++ runnerStatusLabel (nativeStatus owned) ++ " differs from stock " ++ runnerStatusLabel (nativeStatus stock)
             | nativeStatus owned /= nativeStatus stock
             ]
    }

-- * Controls

movw, movt :: Word32 -> Word32 -> Word32
movw rd value = 0xe3000000 .|. (((value `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (value .&. 0xfff)
movt rd value = 0xe3400000 .|. (((high `shiftR` 12) .&. 0xf) `shiftL` 16) .|. (rd `shiftL` 12) .|. (high .&. 0xfff)
  where
    high = value `shiftR` 16

-- | `str rt, [rn, #offset]`.
storeWord :: Word32 -> Word32 -> Word32 -> Word32
storeWord rt rn offset = 0xe5800000 .|. (rn `shiftL` 16) .|. (rt `shiftL` 12) .|. offset

-- | Stores each word at r0 + 4 * index through r1, then returns 1.
writerGuest :: [Word32] -> [Word32] -> [Word32]
writerGuest words' extra =
  concat [[movw 1 w, movt 1 w, storeWord 1 0 (4 * index)] | (index, w) <- zip [0 ..] words']
    ++ extra
    ++ [0xe3a00001, 0xe12fff1e]

guestCase :: Context -> Scenario -> String -> String -> [Word32] -> CaseSpec
guestCase context sc identifier provenance code =
  (caseSpec context Owned sc (SnapshotInto Buffer))
    { caseId = identifier
    , caseProvenance = provenance
    , caseEntry = guestBase
    , caseRegions = caseRegions (caseSpec context Owned sc (SnapshotInto Buffer)) ++ [mkRegion (RegionId "guest") guestBase (B.length bytes) ReadExecute bytes]
    , caseFunctions = [(guestBase, fromIntegral (B.length bytes))]
    }
  where
    bytes = assemble code

data Control = Control
  { controlId :: String
  , controlAccepted :: Bool
  , controlRequired :: Bool
  }

runControls :: Context -> Scenario -> IO [Control]
runControls context sc = do
  let Snapshot words' = scenarioSnapshot sc
      expected = expectation sc (SnapshotInto Buffer)
      judged identifier provenance code required' = do
        observation <- runSpec context (guestCase context sc identifier provenance code)
        pure (Control identifier (null (problems expected observation)) required')
      wrong = take 4 words' ++ [words' !! 4 + 1] ++ drop 5 words'
  exact <- judged "mf-control-exact-writer" "positive control: a guest that stores the expected snapshot is accepted" (writerGuest words' []) True
  wrongWord <- judged "mf-control-wrong-word" "negative control: cursor token off by one" (writerGuest wrong []) False
  overrun <- judged "mf-control-overrun" "negative control: a twelfth word at dest + 0x2c" (writerGuest words' [movw 1 0, storeWord 1 0 0x2c]) False
  clobber <- judged "mf-control-r4-clobber" "negative control: r4 cleared before return" (writerGuest words' [0xe3a04000]) False
  callerFrame <- judged "mf-control-caller-frame" "negative control: a store to the caller's frame at sp" (writerGuest words' [storeWord 1 13 0]) False
  fetch <- runSpec context
    (guestCase context sc "mf-control-forbidden-fetch" "negative control: a transfer to the stock FUN_0061cd14 entry in the owned configuration" [0xe59fc000, 0xe12fff1c, 0x0061cd14])
  pure
    [ exact
    , wrongWord
    , overrun
    , clobber
    , callerFrame
    , Control "mf-control-forbidden-fetch" (nativeStatus fetch /= FetchGuard) False
    ]

-- * Main

sliceImage :: String -> B.ByteString -> Word32 -> GuestAddress -> GuestAddress -> B.ByteString
sliceImage label image base (GuestAddress low) (GuestAddress high)
  | fromIntegral low < base || B.length slice /= count = error (label ++ " does not cover " ++ hexWord32 (fromIntegral low) ++ ".." ++ hexWord32 (fromIntegral high))
  | otherwise = slice
  where
    count = fromIntegral (high - low)
    slice = B.take count (B.drop (fromIntegral low - fromIntegral base) image)

main :: IO ()
main = do
  arguments <- getArgs
  runner <- required arguments "--runner"
  build <- required arguments "--build"
  output <- required arguments "--out"
  id4Path <- required arguments "--id4"
  id4Base <- parseAddress "--id4-base" <$> required arguments "--id4-base"
  loaderPath <- required arguments "--loader"
  symbols <- parseSymbols requiredSymbols (build </> "kernel.syms")
  kernelText <- B.readFile (build </> "kernel-text.bin")
  when (B.null kernelText) (error "kernel-text.bin is empty")
  id4 <- B.readFile id4Path
  loader <- B.readFile loaderPath
  let code = sliceImage "--id4" id4 id4Base codeBase codeEnd
      nodes = sliceImage "--id4" id4 id4Base nodesBase nodesEnd
      images = Images code nodes loader
  forM_ anchors $ \(GuestAddress address, word) ->
    unless (readWordAt code (fromIntegral address - fromIntegral (guestAddress codeBase)) == word) $
      error ("--id4 is not X-M1 1.50 image 4 at " ++ hexWord32 (fromIntegral address))
  unless (B.length loader > 0xd978 && readWordAt loader 0xd978 == loaderDivideEntry) $
    error "--loader is not the X-M1 1.50 loader A image at 0x00040000"
  let context = Context runner output symbols kernelText images
      runs = [(sc, operation) | sc <- scenarios, operation <- scenarioOperations sc]
  outcomes <- forM runs $ \(sc, operation) -> runOperation context sc operation
  controlScenario <- maybe (error "shooting-page scenario missing") pure (lookupScenario "shooting-page")
  controls <- runControls context controlScenario
  let failed = filter (not . null . outcomeProblems) outcomes
      controlsOk = all (\control -> controlAccepted control == controlRequired control) controls
      ownedDepth = maximum (map (stackDepth . outcomeOwned) outcomes)
      stockDepth = maximum (map (stackDepth . outcomeStock) outcomes)
  B.writeFile (output </> "menu-frame-results.json") (encodeCanonical (object
    [ "cases" .=
        [ object
            [ "id" .= outcomeId outcome
            , "stock_status" .= runnerStatusLabel (nativeStatus (outcomeStock outcome))
            , "owned_status" .= runnerStatusLabel (nativeStatus (outcomeOwned outcome))
            , "owned_stack_bytes" .= stackDepth (outcomeOwned outcome)
            , "stock_stack_bytes" .= stackDepth (outcomeStock outcome)
            , "problems" .= outcomeProblems outcome
            ]
        | outcome <- outcomes
        ]
    , "controls" .= [object ["id" .= controlId control, "accepted" .= controlAccepted control, "required" .= controlRequired control] | control <- controls]
    , "max_owned_stack_bytes" .= ownedDepth
    , "max_stock_stack_bytes" .= stockDepth
    ]))
  mapM_ (\outcome -> putStrLn (outcomeId outcome ++ ": " ++ show (outcomeProblems outcome))) failed
  putStrLn ("ran " ++ show (length outcomes) ++ " original/owned pairs over " ++ show (length scenarios) ++ " scenarios: "
    ++ show (length outcomes - length failed) ++ " matched; stack bytes owned " ++ show ownedDepth ++ ", stock " ++ show stockDepth
    ++ "; controls " ++ (if controlsOk then "judged as required" else "FAILED"))
  when (not (null failed) || not controlsOk) exitFailure
  where
    lookupScenario name = case filter ((== name) . scenarioId) scenarios of
      (sc : _) -> Just sc
      [] -> Nothing
