{-# LANGUAGE GHC2021 #-}

-- | The event class of a stage event at `0xd4dd2c`, derived from the menu tables, the event's
-- registers and the block at entry, by the rules of `reports/owned-menu-engine-design.md`
-- ("Coverage is by event class"), `reports/owned-menu-engine-increment-3.md` and
-- `reports/owned-menu-engine-increment-4.md`. A class is owned when the owned engine runs it: the
-- classes with oracle cases, each refused when a node the event can open fails the engine's
-- pre-checks.
module Replacement.ModalRuntime.MenuEngineClass
  ( StageEvent (..)
  , MenuBlock (..)
  , menuBlock
  , MenuTree
  , menuTree
  , Direction (..)
  , GroupKind (..)
  , FrameKind (..)
  , OwnedClass (..)
  , Fallback (..)
  , EventClass (..)
  , deriveClass
  , isOwned
  ) where

import Control.Monad (forM_, when)
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
-- (`0x009571d8`), the total rows (`0x009571e8`), the cursor-stack entries' cursors
-- (`0x00957554 + 0x10 * i`) and the stack depth (`0x009575f4`).
data MenuBlock = MenuBlock
  { blockOpenRecord :: Maybe Address
  , blockCursor :: Word32
  , blockTotal :: Word32
  , blockStackCursors :: [Word32]
  , blockDepth :: Word32
  }
  deriving (Eq, Show)

blockBytes :: Int
blockBytes = 0x430

stackFrames :: Int
stackFrames = 10

-- | The block from its 0x430 little-endian bytes at `0x009571d0`.
menuBlock :: B.ByteString -> Either String MenuBlock
menuBlock bytes
  | B.length bytes /= blockBytes = Left "a menu block is 0x430 bytes"
  | otherwise = Right MenuBlock
      { blockOpenRecord = if record == 0 then Nothing else Just (Address record)
      , blockCursor = word 8
      , blockTotal = word 0x18
      , blockStackCursors = [word (0x384 + 0x10 * frame) | frame <- [0 .. stackFrames - 1]]
      , blockDepth = word 0x424
      }
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

-- | Kinds `0x45`..`0x48`: `0xbf9c1c`'s arms.
data GroupKind
  = -- | `0x45`: back within a group of four.
    GroupBack
  | -- | `0x46`: forward within a group of four.
    GroupForward
  | -- | `0x47`: up to the fourth row not hidden.
    GroupStepUp
  | -- | `0x48`: down to the fourth row not hidden.
    GroupStepDown
  deriving (Eq, Show)

-- | The owned kinds of 6..`0xb`.
data FrameKind = PopMoveDownPush | PopMoveUpPush | PushMoveDownPop | PushMoveUpPop | SeekPushMoveDownPop | SeekPushMoveUpPop
  deriving (Eq, Show)

data OwnedClass
  = RowMove Direction
  | GroupMove GroupKind
  | Push
  | Pop
  | Open
  | Close
  | FrameMove FrameKind
  | Reopen
  deriving (Eq, Show)

-- | Why the engine leaves an event to stock.
data Fallback
  = KindNotOwned Word32
  | PageStep
  | ZeroStep
  | SeekingId
  | NoOpenNode
  | UnknownRecord Address
  | SeekClassNode
  | OutsideRegistry
  | Unregistered
  | NoChild
  | NoParent
  | FramedTarget
  | FramedSource
  | TargetWithoutRows
  | TargetRows
  | CursorPastRows
  | StackBounds
  | InnerMove
  | RowTotal
  | PackageChildRow
  deriving (Eq, Show)

data EventClass = OwnedEvent OwnedClass | FallbackEvent Fallback deriving (Eq, Show)

isOwned :: EventClass -> Bool
isOwned (OwnedEvent _) = True
isOwned (FallbackEvent _) = False

registrySlots, seekClassNode, rowStates :: Word32
registrySlots = 0x86
seekClassNode = 0x38
rowStates = 0x4f

framed :: TableNode -> Bool
framed candidate = nodeType candidate `elem` [5, 7]

deriveClass :: MenuTree -> MenuBlock -> StageEvent -> EventClass
deriveClass tree block event = either FallbackEvent OwnedEvent $ rowTotal *> case stageKind event of
  4 -> rowMove Down
  5 -> rowMove Up
  0x45 -> groupMove GroupBack
  0x46 -> groupMove GroupForward
  0x47 -> groupMove GroupStepUp
  0x48 -> groupMove GroupStepDown
  0xe -> push
  0xf -> pop
  1 -> open
  2 -> close
  6 -> popMovePush PopMoveDownPush
  7 -> popMovePush PopMoveUpPush
  8 -> pushMovePop PushMoveDownPop
  9 -> pushMovePop PushMoveUpPop
  0xa -> seekPushMovePop SeekPushMoveDownPop
  0xb -> seekPushMovePop SeekPushMoveUpPop
  0x13 -> reopen
  other -> Left (KindNotOwned other)
 where
  -- The engine refuses before any call when the open node's tree rows differ from the block's total.
  rowTotal = case blockOpenRecord block of
    Nothing -> Right ()
    Just at -> do
      current <- node at
      if rowCount current == blockTotal block then Right () else Left RowTotal
  node at = maybe (Left (UnknownRecord at)) Right (M.lookup at (treeNodes tree))
  source = maybe (Left NoOpenNode) node (blockOpenRecord block)
  rows candidate = fromMaybe [] (nodeRows candidate)
  rowCount candidate = fromIntegral (length (rows candidate)) :: Word32
  depth = blockDepth block
  stackCursor frame = if frame < fromIntegral stackFrames then Right (blockStackCursors block !! fromIntegral frame) else Left StackBounds
  stageSlot = slot (stageNode event)
  slot word = if word >= registrySlots then Left OutsideRegistry else Right ()
  cursorInRows = do
    current <- source
    when (blockCursor block > rowCount current) (Left CursorPastRows)
  cursorChild current = case drop (fromIntegral (blockCursor block)) (rows current) of
    row : _ -> rowChild row
    [] -> LeafChild
  -- `0xbfa800`'s needs of the node it opens and the cursor it restores.
  openable candidate restored = do
    case nodeRows candidate of
      Nothing -> Left TargetWithoutRows
      Just [] -> Left TargetWithoutRows
      Just found | fromIntegral (length found) > rowStates -> Left TargetRows
      Just _ -> Right ()
    when (restored >= rowStates) (Left TargetRows)
  -- The `0xbfad04` step after a landing on a node of type 5 or 7.
  framedTarget = either (const (Left FramedTarget)) Right
  -- `0xbfad04`'s push from `candidate`: every row child opens with the cursor `restored`.
  followPush candidate restored = forM_ (rows candidate) $ \row -> innerTarget (rowChild row) restored
  innerMove candidate ident =
    when (ident == 0 || (nodeRowsPerPage candidate == ident && stageNode event /= 0)) (Left InnerMove)
  rowMove direction = do
    current <- source
    if nodeRowsPerPage current == stageId event && stageNode event /= 0 then Left PageStep
      else if stageId event == 0 then Left ZeroStep
      else Right (RowMove direction)
  push = do
    when (stageId event `elem` [0x17, 0x18]) (Left SeekingId)
    stageSlot
    cursorInRows
    current <- source
    child <- case cursorChild current of
      NodeChild at -> node at
      PackageChild _ -> Left PackageChildRow
      LeafChild -> Left NoChild
    restored <- stackCursor (depth + 1)
    openable child restored
    when (framed child) . framedTarget $ do
      when (restored > rowCount child) (Left CursorPastRows)
      slot (nodeId child)
      stackCursor (depth + 2) >>= followPush child
    pure Push
  pop = do
    when (stageId event `elem` [0x17, 0x18]) (Left SeekingId)
    stageSlot
    cursorInRows
    current <- source
    parent <- maybe (Left NoParent) node (nodeParent current)
    when (depth == 0 || depth >= fromIntegral stackFrames) (Left StackBounds)
    restored <- stackCursor (depth - 1)
    openable parent restored
    when (framed parent) . framedTarget $ do
      slot (nodeId parent)
      when (nodeRemember parent /= 0 && restored > rowCount parent) (Left CursorPastRows)
      forM_ (nodeParent parent) $ \at -> do
        grandparent <- node at
        when (depth < 2) (Left StackBounds)
        stackCursor (depth - 2) >>= openable grandparent
    pure Pop
  open = do
    when (stageId event `elem` ([0xe .. 0x12] ++ [0x17, 0x18])) (Left SeekingId)
    index <- if stageNode event == seekClassNode then Left SeekClassNode
      else if stageNode event >= registrySlots then Left OutsideRegistry else Right (stageNode event)
    registered <- maybe (Left Unregistered) node (M.lookup index (treeRegistry tree))
    openable registered 0
    -- The open zeroes the stack, so the framed push restores cursor 0.
    when (framed registered) (framedTarget (followPush registered 0))
    pure Open
  close = do
    stageSlot
    _ <- source
    when (stageId event >= 0x1a && stageId event <= 0x1d) cursorInRows
    pure Close
  -- `0xbf9c1c`'s arms for kinds `0x45` and `0x46` read the cursor row's token.
  groupMove kind = do
    _ <- source
    when (kind `elem` [GroupBack, GroupForward]) cursorInRows
    pure (GroupMove kind)
  reopen = do
    current <- source
    openable current (blockCursor block)
    pure Reopen
  -- A push target or pop target of an inner step: a node `0xbfa800` can open, or a row without a child.
  innerTarget child restored = case child of
    NodeChild at -> do
      target <- node at
      openable target restored
      pure (Just target)
    PackageChild _ -> Left PackageChildRow
    LeafChild -> pure Nothing
  popMovePush kind = do
    stageSlot
    cursorInRows
    current <- source
    parent <- maybe (Left NoParent) node (nodeParent current)
    when (depth == 0 || depth >= fromIntegral stackFrames) (Left StackBounds)
    restored <- stackCursor (depth - 1)
    openable parent restored
    innerMove parent (stageId event)
    forM_ (rows parent) $ \row -> innerTarget (rowChild row) (blockCursor block)
    pure (FrameMove kind)
  pushMovePop kind = FrameMove kind <$ (stageSlot *> cursorInRows *> source >>= \current -> checkPushMovePop current (cursorChild current))
  checkPushMovePop current child = do
    when (depth + 1 >= fromIntegral stackFrames) (Left StackBounds)
    restored <- stackCursor (depth + 1)
    found <- innerTarget child restored
    forM_ found $ \target -> do
      when (restored > rowCount target) (Left CursorPastRows)
      innerMove target (stageId event)
      forM_ (nodeParent target) $ \at -> do
        parent <- node at
        openable parent (max (rowCount current - 1) (blockCursor block))
  seekPushMovePop kind = do
    stageSlot
    cursorInRows
    current <- source
    when (framed current) (Left FramedSource)
    found <- maybe (Left TargetWithoutRows) Right (nodeRows current)
    let zeroStep = blockTotal block == 0 || fmap rowToken (lookupRow found) == Just (stageExtra event)
        lookupRow list = case drop (fromIntegral (blockCursor block)) list of
          row : _ -> Just row
          [] -> Nothing
    if zeroStep
      then checkPushMovePop current (cursorChild current)
      else do
        innerMove current 1
        forM_ found $ \row -> checkPushMovePop current (rowChild row)
    pure (FrameMove kind)
