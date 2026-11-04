{-# LANGUAGE GHC2021 #-}

-- | The event class of a stage event at `0xd4dd2c`, derived from the menu tables, the event's
-- registers and the block at entry, by the rules of `reports/owned-menu-engine-design.md`
-- ("Coverage is by event class"). A class is owned when the owned engine has an oracle case for
-- it (E-MENU-ENGINE-HOST-ORACLE-150): kind 4 and 5 row steps, push, pop and open.
module Replacement.ModalRuntime.MenuEngineClass
  ( StageEvent (..)
  , MenuBlock (..)
  , menuBlock
  , MenuTree
  , menuTree
  , Direction (..)
  , OwnedClass (..)
  , Fallback (..)
  , EventClass (..)
  , deriveClass
  , isOwned
  ) where

import qualified Data.ByteString as B
import Data.Bits (shiftL, (.|.))
import Data.Maybe (fromMaybe)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Replacement.ModalRuntime.MenuEngineTree (Address (..), RegistryEntry (..), RowChild (..), TableNode (..), TableRow (..), TablesDocument (..))

-- | The four words `0xd4dd2c` receives in r0-r3: kind, node, id, extra.
data StageEvent = StageEvent {stageKind :: Word32, stageNode :: Word32, stageId :: Word32, stageExtra :: Word32}
  deriving (Eq, Show)

-- | The block words the class reads: the open node record (`0x009571d0`), the cursor
-- (`0x009571d8`) and the total rows (`0x009571e8`).
data MenuBlock = MenuBlock {blockOpenRecord :: Maybe Address, blockCursor :: Word32, blockTotal :: Word32}
  deriving (Eq, Show)

blockBytes :: Int
blockBytes = 0x430

-- | The block from its 0x430 little-endian bytes at `0x009571d0`.
menuBlock :: B.ByteString -> Either String MenuBlock
menuBlock bytes
  | B.length bytes /= blockBytes = Left "a menu block is 0x430 bytes"
  | otherwise = Right (MenuBlock (if record == 0 then Nothing else Just (Address record)) (word 8) (word 0x18))
 where
  record = word 0
  word offset = foldr (\index acc -> (acc `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

-- | The menu tables by record address, with the registry `0x0081babc` by slot.
data MenuTree = MenuTree {treeNodes :: M.Map Address TableNode, treeRegistry :: M.Map Word32 Address}

menuTree :: TablesDocument -> MenuTree
menuTree document = MenuTree
  (M.fromList [(nodeAddress node, node) | node <- documentNodes document])
  (M.fromList [(registrySlot entry, at) | entry <- documentRegistry document, Just at <- [registryAddress entry]])

data Direction = Down | Up deriving (Eq, Show)

data OwnedClass = RowMove Direction | Push | Pop | Open deriving (Eq, Show)

-- | Why an event's class has no oracle case.
data Fallback
  = KindNotOwned Word32
  | PageStep
  | ZeroStep
  | SeekingId
  | NoOpenNode
  | UnknownRecord Address
  | NodeFlags
  | PropertyNode
  | SeekClassNode
  | OutsideRegistry
  | Unregistered
  | RememberBranch
  | NoChild
  | NoParent
  | FramedTarget
  | TargetWithoutRows
  | TargetRows
  | RowTotal
  | PackageChildRow
  deriving (Eq, Show)

data EventClass = OwnedEvent OwnedClass | FallbackEvent Fallback deriving (Eq, Show)

isOwned :: EventClass -> Bool
isOwned (OwnedEvent _) = True
isOwned (FallbackEvent _) = False

registrySlots, propertyNode, seekClassNode, rowStates :: Word32
registrySlots = 0x86
propertyNode = 0x7a
seekClassNode = 0x38
rowStates = 0x4f

deriveClass :: MenuTree -> MenuBlock -> StageEvent -> EventClass
deriveClass tree block event = either FallbackEvent OwnedEvent $ rowTotal *> case stageKind event of
  4 -> rowMove Down
  5 -> rowMove Up
  0xe -> push
  0xf -> pop
  1 -> open
  other -> Left (KindNotOwned other)
 where
  -- The engine refuses before any call when the open node's tree rows differ from the block's total.
  rowTotal = case blockOpenRecord block of
    Nothing -> Right ()
    Just at -> do
      current <- node at
      if maybe 0 length (nodeRows current) == fromIntegral (blockTotal block) then Right () else Left RowTotal
  node at = maybe (Left (UnknownRecord at)) Right (M.lookup at (treeNodes tree))
  source = maybe (Left NoOpenNode) node (blockOpenRecord block)
  wrapsAndQueried candidate = nodeNoWrap candidate == 0 && nodeNoAvailability candidate == 0
  stageSlot = do
    let slot = stageNode event
    if slot == propertyNode then Left PropertyNode
      else if slot >= registrySlots then Left OutsideRegistry else Right slot
  target candidate = do
    if nodeType candidate `elem` [5, 7] then Left FramedTarget
      else if not (wrapsAndQueried candidate) then Left NodeFlags
      else case nodeRows candidate of
        Nothing -> Left TargetWithoutRows
        Just [] -> Left TargetWithoutRows
        Just rows | fromIntegral (length rows) > rowStates -> Left TargetRows
        Just _ -> Right ()
  -- `0xbf9a5c`'s branch: the parent's id against the node's own.
  sameParentId candidate = do
    parent <- maybe (Left NoParent) node (nodeParent candidate)
    pure (nodeId parent == nodeId candidate)
  rowMove direction = do
    current <- source
    if nodeRowsPerPage current == stageId event && stageNode event /= 0 then Left PageStep
      else if stageId event == 0 then Left ZeroStep
      else if not (wrapsAndQueried current) then Left NodeFlags
      else Right (RowMove direction)
  pushPop = do
    if stageId event `elem` [0x17, 0x18] then Left SeekingId else Right ()
    _ <- stageSlot
    current <- source
    if nodeRemember current == 0 then Left RememberBranch else Right ()
    pure current
  push = do
    current <- pushPop
    same <- sameParentId current
    if same then Left RememberBranch else Right ()
    let cursor = fromIntegral (blockCursor block)
    child <- case drop cursor (fromMaybe [] (nodeRows current)) of
      row : _ | NodeChild at <- rowChild row -> node at
      row : _ | PackageChild _ <- rowChild row -> Left PackageChildRow
      _ -> Left NoChild
    target child
    pure Push
  pop = do
    current <- pushPop
    same <- sameParentId current
    if same then Right () else Left RememberBranch
    parent <- maybe (Left NoParent) node (nodeParent current)
    target parent
    pure Pop
  open = do
    if stageId event `elem` ([0xe .. 0x12] ++ [0x17, 0x18]) then Left SeekingId else Right ()
    slot <- if stageNode event == seekClassNode then Left SeekClassNode
      else if stageNode event >= registrySlots then Left OutsideRegistry else Right (stageNode event)
    registered <- maybe (Left Unregistered) node (M.lookup slot (treeRegistry tree))
    target registered
    pure Open
