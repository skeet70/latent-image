-- | Register-branch target sets declared as data. A declaration names the images it was read from by
-- SHA-256, how the branch selects its target and the code that writes the words the selection reads.
-- `declaredSites` closes a site when every digest matches, every store the scan finds into those words
-- lies in the declared writers and has a known value, and the table words it reads have no writer.
module Replacement.StartupArtifact.CallbackTargets
  ( TargetDeclaration (..), ImageDigest (..), Selection (..), IndexWord (..)
  , Chart (..), ChartEntries (..), ChartCall (..)
  , OpenReason (..), CheckedSites (..), declaredSites
  ) where

import qualified A32
import Data.Bits (shiftR, (.&.))
import Data.Int (Int8)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.StartupArtifact.StackBounds (Segment (..), Tracing (AnyConstants), Watched (..), callerArguments, closedValues, segmentWordAt, writersOf)
import StockStack (CallbackSite (..), CodeRange (..), Condition (DeclaredTargetsClosed))

-- | The SHA-256 of the image segment loaded at a base, as the declaration read it.
data ImageDigest = ImageDigest {imageBase :: Word32, imageSha256 :: String} deriving (Eq, Show)

data TargetDeclaration = TargetDeclaration
  { declaredSite :: Word32
  , declaredImages :: [ImageDigest]
  , declaredSelection :: Selection
  , -- | Code holding every store into a word the selection reads.
    declaredWriters :: [CodeRange]
  }
  deriving (Eq, Show)

data Selection
  = -- | The target is the word at one of the slot addresses, which one chosen by the index words alone.
    Slots [Word32] [IndexWord]
  | StateChart Chart
  deriving (Eq, Show)

-- | A word or halfword choosing the slot, and the values the slot set covers.
data IndexWord = IndexWord {indexWatched :: Watched, indexValues :: [Word32]} deriving (Eq, Show)

-- | How a chart's transition table holds an entry: two bytes (action index, signed next: a positive
-- value selects a choice, others the negated next state), or a 12-byte record (kind byte, action word
-- at +4, word at +8 that is the choice for kinds 4 and 5 and otherwise the next state unless -1).
data ChartEntries = PairEntries | RecordEntries deriving (Eq, Show)

-- | Which of a dispatcher's register branches the site is.
data ChartCall = ActionCall | ChoiceCall deriving (Eq, Show)

-- | A state-chart dispatcher entered with the machine in r0. It reads machine m's entry at
-- @(event * columns + state) .&. 0xffff@ of its transition table, from per-machine word arrays of table
-- pointers and halfword arrays of column counts, current states and pending events; a machine whose
-- transition pointer is 0 dispatches nothing.
data Chart = Chart
  { chartEntry :: Word32
  , chartEntries :: ChartEntries
  , chartCall :: ChartCall
  , chartTransitions :: Word32
  , -- | Action and choice table arrays; `RecordEntries` reads neither.
    chartActions :: Word32
  , chartChoices :: Word32
  , chartColumns :: Word32
  , chartStates :: Word32
  , chartEvents :: Word32
  }
  deriving (Eq, Show)

data OpenReason
  = DigestMismatch Word32
  | -- | The watched address and a store into it outside the declared writers.
    UndeclaredWriter Word32 Word32
  | -- | A watched word some store writes with a value the scan does not know.
    OpenValue Word32
  | -- | An index word's value outside the declared ones.
    IndexOutside Word32 Word32
  | -- | A dispatcher whose callers do not all pass a constant machine.
    OpenMachine Word32
  | -- | A table word some store writes.
    TableWritten Word32 Word32
  | -- | A table word outside the image segments.
    NoTable Word32
  deriving (Eq, Show)

data CheckedSites = CheckedSites {closedSites :: M.Map Word32 CallbackSite, openSites :: [(Word32, OpenReason)]} deriving (Eq, Show)

-- | The declarations checked against the canonical segments' digests and the composed segments' stores
-- (owned code excluded from the scan); `cleared` are the ranges holding 0 before their first store.
declaredSites :: [Segment] -> [Segment] -> [(Word32, Word32)] -> [(Word32, Word32)] -> [TargetDeclaration] -> CheckedSites
declaredSites canonical composed owned cleared declarations =
  let checked = [(declaredSite declaration, check declaration) | declaration <- declarations]
   in CheckedSites (M.fromList [(site, CallbackSite targets DeclaredTargetsClosed) | (site, Right targets) <- checked])
        [(site, reason) | (site, Left reason) <- checked]
  where
    writers = writersOf composed owned
    values = closedValues AnyConstants composed owned cleared
    arguments = callerArguments AnyConstants composed owned
    digests = M.fromList [(base, SHA.sha256Hex (SHA.sha256 bytes)) | Segment base bytes <- canonical]

    check declaration = do
      mapM_ (\(ImageDigest base digest) -> if M.lookup base digests == Just digest then Right () else Left (DigestMismatch base)) (declaredImages declaration)
      let declared site = any (\(CodeRange low high) -> low <= site && site < high) (declaredWriters declaration)
          written watched = case filter (not . declared) (writers watched) of
            site : _ -> Left (UndeclaredWriter (watchedAddress watched) site)
            [] -> maybe (Left (OpenValue (watchedAddress watched))) Right (values watched)
      targets <- case declaredSelection declaration of
        Slots slots indices -> do
          mapM_ (\(IndexWord watched allowed) -> do
                   found <- written watched
                   case filter (`notElem` allowed) found of
                     value : _ -> Left (IndexOutside (watchedAddress watched) value)
                     [] -> Right ()) indices
          concat <$> mapM (\slot -> written (Watched slot 4)) slots
        StateChart chart -> chartTargets written chart
      Right (S.toList (S.fromList targets))

    chartTargets written chart = do
      machines <- maybe (Left (OpenMachine (chartEntry chart))) Right (arguments (chartEntry chart) (A32.Reg 0))
      reached <- mapM (machineTargets written chart) (S.toList (S.fromList machines))
      let touched = S.toList (S.fromList (concatMap snd reached))
      case [(word, site) | word <- touched, site : _ <- [writers (Watched word 4)]] of
        (word, site) : _ -> Left (TableWritten word site)
        [] -> Right (concatMap fst reached)

    -- A machine's targets from the entries its reachable (event, state) pairs select, the states its
    -- own entries move to included, and the table words read.
    machineTargets written chart machine = do
      let pointer = chartTransitions chart + 4 * machine
      transitions <- tableWord pointer
      if transitions == 0 then Right ([], [pointer]) else do
        columns <- tableHalf (chartColumns chart + 2 * machine)
        events <- written (Watched (chartEvents chart + 2 * machine) 2)
        initial <- written (Watched (chartStates chart + 2 * machine) 2)
        tables <- case chartEntries chart of
          PairEntries -> (,) <$> tableWord (chartActions chart + 4 * machine) <*> tableWord (chartChoices chart + 4 * machine)
          RecordEntries -> Right (0, 0)
        let explore _ [] calls = Right calls
            explore seen (state : pending) calls
              | S.member state seen = explore seen pending calls
              | otherwise = do
                  entries <- mapM (\event -> entry tables transitions ((event * columns + state) .&. 0xffff)) events
                  explore (S.insert state seen) (pending ++ [next | (_, _, Just next, _) <- entries]) (calls ++ entries)
        reached <- explore S.empty initial []
        let targets = case chartCall chart of
              ActionCall -> [action | (action, _, _, _) <- reached]
              ChoiceCall -> [choice | (_, Just choice, _, _) <- reached]
            pointers = [pointer, chartColumns chart + 2 * machine] ++ case chartEntries chart of
              PairEntries -> [chartActions chart + 4 * machine, chartChoices chart + 4 * machine]
              RecordEntries -> []
        pure (targets, map aligned (pointers ++ concat [fields | (_, _, _, fields) <- reached]))
      where
        -- (action, choice when selected, next state when set, words read)
        entry (actions, choices) table index = case chartEntries chart of
          PairEntries -> do
            let at = table + 2 * index
            action <- tableByte at
            next <- fromIntegral . (fromIntegral :: Word32 -> Int8) <$> tableByte (at + 1)
            actionTarget <- tableWord (actions + 4 * action)
            if (next :: Int) > 0
              then (\choice -> (actionTarget, Just choice, Nothing, [at, at + 1, actions + 4 * action, choices + 4 * fromIntegral next]))
                     <$> tableWord (choices + 4 * fromIntegral next)
              else Right (actionTarget, Nothing, Just (fromIntegral (negate next)), [at, at + 1, actions + 4 * action])
          RecordEntries -> do
            let record = table + 12 * index
            kind <- tableByte record
            action <- tableWord (record + 4)
            selected <- tableWord (record + 8)
            let fields = [record, record + 4, record + 8]
            Right (if (kind - 4) .&. 0xff <= 1 then (action, Just selected, Nothing, fields)
                   else (action, Nothing, if selected == 0xffffffff then Nothing else Just (selected .&. 0xffff), fields))

    aligned address = address - address .&. 3
    tableWord address = maybe (Left (NoTable (aligned address))) Right (segmentWordAt composed (aligned address))
    tableHalf address = (\value -> (value `shiftR` (8 * fromIntegral (address .&. 2))) .&. 0xffff) <$> tableWord address
    tableByte address = (\value -> (value `shiftR` (8 * fromIntegral (address .&. 3))) .&. 0xff) <$> tableWord address
