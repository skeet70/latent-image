-- | Provenance of the payload bytes a stock message builder never stores (docs/principles.md):
-- marked where the builder sends the message, carried through reviewed byte copies, cleared by
-- any other write, and checked against every read of a marked byte. Watched writes keep only
-- the bytes that no longer carry a mark.
module Replacement.PlaybackActor.Provenance
  ( Origin (..)
  , Copy (..)
  , Event (..)
  , Finding (..)
  , MaskedWrite (..)
  , Config (..)
  , Tracker
  , tracker
  , step
  , track
  , marked
  , builtMessages
  , builderStoredOffsets
  ) where

import Data.Bits (shiftR, (.&.))
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, mapMaybe)
import Data.Word (Word32, Word8)

-- | A marked byte: the message it belongs to, counted from 0 in build order, and its payload offset.
data Origin = Origin {originMessage :: Int, originOffset :: Word32}
  deriving (Eq, Show)

-- | One call of the reviewed byte copy.
data Copy = Copy {copyDestination :: Word32, copySource :: Word32, copyLength :: Word32}
  deriving (Eq, Show)

-- | Trace events in execution order. Cores are the trace's core numbers.
data Event
  = -- | The builder sends a payload of the given length at the address, having stored the offsets.
    Built Int Word32 Word32 [Word32]
  | -- | The builder's frame dies: the core's last built payload stops being the message.
    Sent Int
  | CopyBegins Int Copy
  | -- | The core's copy returned; with True its source is released.
    CopyEnds Int Bool
  | -- | core, pc, address, size
    Read Int Word32 Word32 Int
  | -- | core, pc, address, bytes before, bytes written
    Write Int Word32 Word32 [Word8] [Word8]
  deriving (Eq, Show)

data Finding
  = -- | A marked byte read by an instruction other than a reviewed copy.
    Interpreted {findingPc :: Word32, findingAddress :: Word32, findingOrigin :: Origin}
  | -- | A marked byte read by the copy routine outside a reviewed call of it.
    Untracked {findingPc :: Word32, findingAddress :: Word32, findingOrigin :: Origin}
  | -- | A mark would land outside the watched memory.
    Escaped {findingAddress :: Word32, findingOrigin :: Origin}
  deriving (Eq, Show)

-- | A watched write with marked bytes as Nothing, before and after the write.
data MaskedWrite = MaskedWrite {maskedBefore :: [Maybe Word8], maskedValue :: [Maybe Word8]}
  deriving (Eq, Show)

data Config = Config
  { configWatched :: [(Word32, Word32)]
  , configCopyLoads :: [Word32]
  , configCopyStores :: [Word32]
  }

data Tracker = Tracker
  { trackerConfig :: !Config
  , trackerMarks :: !(M.Map Word32 Origin)
  , trackerCopies :: !(M.Map Int Copy)
  , trackerFrames :: !(M.Map Int (Word32, Word32))
  , trackerBuilt :: !Int
  }

tracker :: Config -> Tracker
tracker config = Tracker config M.empty M.empty M.empty 0

marked :: Tracker -> M.Map Word32 Origin
marked = trackerMarks

builtMessages :: Tracker -> Int
builtMessages = trackerBuilt

watched :: Tracker -> Word32 -> Bool
watched t a = any (\(s, e) -> s <= a && a < e) (configWatched (trackerConfig t))

within :: Word32 -> Word32 -> Word32 -> Int -> Bool
within start count address size = start <= address && fromIntegral address + toInteger size <= toInteger start + toInteger count

-- | The first count offsets from 0.
offsetsBelow :: Word32 -> [Word32]
offsetsBelow count = if count == 0 then [] else [0 .. count - 1]

clear :: Word32 -> Word32 -> M.Map Word32 Origin -> M.Map Word32 Origin
clear base count marks = foldr (M.delete . (base +)) marks (offsetsBelow count)

step :: Tracker -> Event -> (Tracker, [Finding], Maybe MaskedWrite)
step t e = case e of
  Built core base count stored ->
    let message = trackerBuilt t
        offsets = [o | o <- offsetsBelow count, o `notElem` stored]
        escaped = [Escaped (base + o) (Origin message o) | o <- offsets, not (watched t (base + o))]
        marks = foldr (\o -> M.insert (base + o) (Origin message o)) (clear base count (trackerMarks t)) [o | o <- offsets, watched t (base + o)]
     in (t {trackerMarks = marks, trackerBuilt = message + 1, trackerFrames = M.insert core (base, count) (trackerFrames t)}, escaped, Nothing)
  Sent core -> case M.lookup core (trackerFrames t) of
    Just (base, count) -> (t {trackerMarks = clear base count (trackerMarks t), trackerFrames = M.delete core (trackerFrames t)}, [], Nothing)
    Nothing -> (t, [], Nothing)
  CopyBegins core copy ->
    let escaped =
          [ Escaped (copyDestination copy + k) origin
          | k <- offsetsBelow (copyLength copy)
          , Just origin <- [M.lookup (copySource copy + k) (trackerMarks t)]
          , not (watched t (copyDestination copy + k))
          ]
     in (t {trackerCopies = M.insert core copy (trackerCopies t)}, escaped, Nothing)
  CopyEnds core release -> case M.lookup core (trackerCopies t) of
    Just copy ->
      let marks = if release then clear (copySource copy) (copyLength copy) (trackerMarks t) else trackerMarks t
       in (t {trackerMarks = marks, trackerCopies = M.delete core (trackerCopies t)}, [], Nothing)
    Nothing -> (t, [], Nothing)
  Read core pc address size ->
    let hit = [(address + fromIntegral i, origin) | i <- [0 .. size - 1], Just origin <- [M.lookup (address + fromIntegral i) (trackerMarks t)]]
        copying = case M.lookup core (trackerCopies t) of
          Just copy -> within (copySource copy) (copyLength copy) address size
          Nothing -> False
     in case hit of
          [] -> (t, [], Nothing)
          (a, origin) : _
            | pc `elem` configCopyLoads (trackerConfig t) && copying -> (t, [], Nothing)
            | pc `elem` configCopyLoads (trackerConfig t) -> (t, [Untracked pc a origin], Nothing)
            | otherwise -> (t, [Interpreted pc a origin], Nothing)
  Write core pc address before value ->
    let addresses = [address + fromIntegral i | i <- [0 .. length value - 1]]
        prior = [M.lookup a (trackerMarks t) | a <- addresses]
        copied = case M.lookup core (trackerCopies t) of
          Just copy
            | pc `elem` configCopyStores (trackerConfig t) && within (copyDestination copy) (copyLength copy) address (length value) ->
                Just [M.lookup (copySource copy + (a - copyDestination copy)) (trackerMarks t) | a <- addresses]
          _ -> Nothing
        after = fromMaybe (map (const Nothing) addresses) copied
        marks = foldr (\(a, m) -> maybe (M.delete a) (M.insert a) m) (trackerMarks t) (zip addresses after)
        masked = MaskedWrite (zipWith keep prior before) (zipWith keep after value)
        keep m b = if isJust m then Nothing else Just b
     in (t {trackerMarks = marks}, [], Just masked)

-- | Steps through the events in order: the tracker after them, their findings in order and each
-- event's masked write. Each step is evaluated before the next.
track :: Tracker -> [Event] -> (Tracker, [Finding], [Maybe MaskedWrite])
track = go [] []
  where
    go found masks t [] = (t, reverse found, reverse masks)
    go !found !masks !t (e : rest) =
      let (t', findings, masked) = step t e
       in forced masked `seq` go (foldl (flip (:)) found findings) (masked : masks) t' rest
    forced (Just (MaskedWrite before value)) = foldr seq () before `seq` foldr seq () value
    forced Nothing = ()

-- | The payload offsets a builder stores, from its A32 words up to the send call: the payload is
-- the last `sub r2, r11, #F` before the call, and each `str rX, [r11, #-I]` stores the word at
-- offset F - I. Any other store, or a store outside the payload, refuses the builder.
builderStoredOffsets :: Word32 -> [Word32] -> Either String [Word32]
builderStoredOffsets count wordsBefore = do
  frame <- case mapMaybe payloadFrame wordsBefore of
    [] -> Left "the builder sets no payload pointer"
    frames -> Right (last frames)
  stores <- mapM (classify frame) (filter isStore wordsBefore)
  pure (concat stores)
  where
    payloadFrame w
      | w .&. 0xfffff000 == 0xe24b2000 && w .&. 0xf00 == 0 = Just (w .&. 0xff)
      | otherwise = Nothing
    isStore w =
      let cls = (w `shiftR` 25) .&. 7
          load = (w `shiftR` 20) .&. 1 == 1
          op2 = (w `shiftR` 5) .&. 3
          extra = cls == 0 && w .&. 0x90 == 0x90
          swap = w .&. 0x0fb00ff0 == 0x01000090
       in (cls `elem` [2, 3] && not load && not (cls == 3 && w .&. 0x10 /= 0))
            || (extra && op2 /= 0 && not load && op2 /= 2)
            || swap
            || (cls `elem` [4, 6] && not load)
    classify frame w
      | w .&. 0xffff0000 == 0xe50b0000 =
          let immediate = w .&. 0xfff
              offset = frame - immediate
           in if immediate <= frame && offset + 4 <= count then Right [offset .. offset + 3] else Left ("the builder stores outside its payload: " ++ show w)
      | w .&. 0xffff0000 == 0xe92d0000 = Right []
      | otherwise = Left ("the builder stores in an unreviewed form: " ++ show w)
