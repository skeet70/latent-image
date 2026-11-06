{-# LANGUAGE OverloadedStrings #-}

-- | Native kernel cases of the S1 scheduler helpers (#156): the case grid, the stock data layout
-- each case builds, the bodies under test, the batch runs and the per-case comparison.
module Replacement.RtosScheduler.Oracle
  ( Env (..)
  , mkEnv
  , Locality (..)
  , LockChoice (..)
  , Row (..)
  , CaseKey (..)
  , caseGrid
  , sampleKeys
  , rowSpins
  , dispatchValues
  , notifyIds
  , lockWords
  , stateBytes
  , flagBytes
  , taskSlot
  , taskAddress
  , recordAddress
  , lockWordAddress
  , bankWords
  , stackBase
  , stackPointer
  , dataPokes
  , dataRegions
  , pokesOutside
  , Body (..)
  , StockCode (..)
  , stockCode
  , stockRaiseIrq
  , OwnedImage
  , ownedImage
  , stockBody
  , ownedBody
  , ownEntryFunction
  , replaceEntry
  , patchOwn
  , caseSpec
  , uncomparedRegisters
  , uncomparedReason
  , findTableReason
  , Difference (..)
  , differenceText
  , compareCase
  , orderHolds
  , endsAsExpected
  , stripScratch
  , caseLabel
  , runCases
  , Outcome (..)
  , outcomeOf
  , statusCounts
  ) where

import Control.Exception (evaluate)
import Control.Monad (forM, unless, when)
import Data.Bits (shiftL, shiftR, xor, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (find, nub, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (fromMaybe, isJust, mapMaybe)
import Data.Word (Word32, Word8)
import qualified Emulation.NativeObservation as N
import Emulation.NativeObservation (Barrier (..), GuestAddress (..), KernelObservation (..), NativeObservation (..), Ordered (..), Register (..), RegionId (..), RunnerStatus (..), Write (..))
import Emulation.OwnedCase
  ( CaseId (..)
  , CaseOutcome (..)
  , CaseRegion (..)
  , CaseSpec (..)
  , InstructionLimit (..)
  , KernelCase (..)
  , KernelDifference
  , Permission (..)
  , Symbol (..)
  , caseOutcome
  , compareKernelCase
  , loadObservedBatch
  , mkRegion
  , readWordAt
  , word32le
  , writeBatch
  )
import Numeric (showHex)
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel, domainLoaderBase, domainLoaderSize, domains)
import qualified Replacement.RtosOwnFirst.Regions as OwnFirst
import Replacement.RtosScheduler.Layout
import Replacement.RtosScheduler.Regions
import System.FilePath ((</>))

-- | The parsed inputs every case is built from.
data Env = Env
  { envLayout :: Layout
  , envRegions :: SchedulerRegions
  }

-- | Refuses a layout whose ready heads or task records the grid's construction cannot address.
mkEnv :: Layout -> SchedulerRegions -> Either String Env
mkEnv layout regions = do
  unless (layoutPriorities layout == 16) (Left "the grid assumes 16 priorities")
  unless (layoutTaskSlots layout >= fromIntegral taskWindow) (Left "task table smaller than the window")
  unless (layoutHeadSize layout == 8) (Left "find's equivalent assumes 8-byte ready heads")
  let env = Env layout regions
  mapM_ (\domain -> lockEntries env domain) domains
  forM_' domains $ \domain -> do
    let (low, high) = blockBounds layout (domainWords layout domain)
    unless (high - low <= 0x1000) (Left ("domain " ++ domainLabel domain ++ " words span more than 4 KiB"))
  pure env
  where
    forM_' xs f = mapM_ f xs

-- * Synthetic memory outside the kernels' data

definitionBase, definitionStride, stackBase, stackPointer, callerTop, exitAddress :: Word32
definitionBase = 0x0096c000
definitionStride = 0x20
stackBase = 0x00959700
stackPointer = 0x00959800
callerTop = 0x00959810
exitAddress = 0x00958f00

-- | The IRQ 120 set-pending words of core 0's and core 1's interrupt-controller banks, which stock
-- raise-IRQ writes.
bankWords :: [Word32]
bankWords = [0xfffb30cc, 0xfffb40cc]

-- | Stock raise-IRQ (A 0x433e4, B 0xd33e4) ends with its @bx lr@ at +0x5c.
stockRaiseIrqBytes :: Word32
stockRaiseIrqBytes = 0x60

-- | Stock task first-entry trampoline, which context record +0x7c holds.
trampoline :: Domain -> Word32
trampoline domain = case domain of
  DomainA -> 0x43728
  DomainB -> 0xd3728

-- | T records 1..12 of each table are mapped.
taskWindow :: Int
taskWindow = 12

taskSlot, selectedSlot, extraSlot, targetSlot, runningSlot :: Int
taskSlot = 1
targetSlot = 5
selectedSlot = 8
extraSlot = 9
runningSlot = 12

instructionLimit :: InstructionLimit
instructionLimit = InstructionLimit 2000

entryCpsr :: Word32
entryCpsr = 0xd3

-- * Grid

-- | Whether r0's context record describes the executing loader's own domain or the other one.
data Locality = Local | Remote deriving (Eq, Show, Enum, Bounded)

data LockChoice = SchedulerLock | ObjectLock deriving (Eq, Show, Enum, Bounded)

data Row = Row
  { rowLocality :: Locality
  , -- | Ready lists of the described domain: priority to slots, head first.
    rowLists :: M.Map Int [Int]
  , rowSelected :: Maybe Int
  , -- | A bitmap bit set for an empty list.
    rowStale :: Maybe Int
  , rowDispatch :: Word32
  , -- | T+0x0d of 'taskSlot' when it is in no list.
    rowTaskPriority :: Word8
  , -- | T+0x0c and T+0x0e of 'taskSlot'.
    rowState :: Word8
  , rowFlags :: Word8
  , -- | The priority word and initial-SP terms of 'taskSlot''s definition.
    rowDefinitionPriority :: Word32
  , rowSpTerms :: (Word32, Word32)
  , -- | requeue's new priority, rotate's priority or notify's id.
    rowArgument :: Word32
  , rowLock :: LockChoice
  , rowLockWord :: Word32
  , -- | The grid values the row holds, as @dimension=value@; the sample covers each.
    rowTags :: [String]
  }
  deriving (Eq, Show)

newtype CaseKey = CaseKey Int deriving (Eq, Ord, Show)

dispatchValues, notifyIds, lockWords :: [Word32]
dispatchValues = [0, 1, 2, 0x80000000]
notifyIds = [0, 1, 2, 3, 0xffffffff, 0x80000000]
lockWords = [0, 1, 0x80000000, 0xffffffff]

-- | The reachable T+0x0c combinations: dormant, ready, wait and suspended alone and with each other,
-- and wait with each wait kind.
stateBytes :: Layout -> [Word8]
stateBytes layout =
  nub
    [ stateDormant s
    , stateReady s
    , w
    , stateSuspended s
    , w .|. stateSuspended s
    , w .|. stateSleeping s
    , w .|. stateSuspended s .|. stateSleeping s
    , w .|. stateObjectQueued s
    , w .|. stateSuspended s .|. stateObjectQueued s
    , w .|. stateOwnerLockQueued s
    , w .|. stateSuspended s .|. stateObjectQueued s .|. stateOwnerLockQueued s
    ]
  where
    s = layoutStates layout
    w = stateWait s

flagBytes :: [Word8]
flagBytes = [0, 1, 2, 4, 6, 7, 0xf9, 0xff]

definitionPriorities :: [Word32]
definitionPriorities = [0, 5, 15, 0x5a5a5a0f]

spTermPairs :: [(Word32, Word32)]
spTermPairs = [(0x00958000, 0x800), (0xfffffff0, 0x20), (0x7ffffffc, 0x7ffffffc)]

data Shape = Empty | One | Many deriving (Eq, Show, Enum, Bounded)

data Position = Only | AtHead | AtMiddle | AtTail deriving (Eq, Show, Enum, Bounded)

-- | The selected cell relative to the task under test.
data Relation = SelNull | SelHigher | SelLower | SelEqual | SelTask deriving (Eq, Show, Enum, Bounded)

focusPriorities :: [Int]
focusPriorities = [0, 5, 15]

lowestUrgency :: Int
lowestUrgency = 15

baseRow :: Layout -> Row
baseRow layout = Row Local M.empty Nothing Nothing 1 0 (stateReady (layoutStates layout)) 0 0 (0x00958000, 0x800) 0 SchedulerLock 0 []

shapeMembers :: Shape -> [Int] -> [Int]
shapeMembers shape = take (case shape of Empty -> 0; One -> 1; Many -> 3)

positionMembers :: Position -> [Int]
positionMembers position = case position of
  Only -> [taskSlot]
  AtHead -> [taskSlot, 2, 3]
  AtMiddle -> [2, taskSlot, 3]
  AtTail -> [2, 3, taskSlot]

listsFrom :: [(Int, [Int])] -> M.Map Int [Int]
listsFrom entries = M.filter (not . null) (M.fromListWith (flip (++)) entries)

-- | The lists with the selected task the relation needs; Nothing when the relation cannot hold.
relate :: Int -> Relation -> M.Map Int [Int] -> Maybe (M.Map Int [Int], Maybe Int)
relate p relation lists = case relation of
  SelNull -> Just (lists, Nothing)
  SelHigher | p > 0 -> Just (M.insertWith (flip (++)) (p - 1) [selectedSlot] lists, Just selectedSlot)
  SelLower | p < lowestUrgency -> Just (M.insertWith (flip (++)) (p + 1) [selectedSlot] lists, Just selectedSlot)
  SelEqual -> case filter (/= taskSlot) (M.findWithDefault [] p lists) of
    peer : _ -> Just (lists, Just peer)
    [] -> Nothing
  SelTask | taskSlot `elem` M.findWithDefault [] p lists -> Just (lists, Just taskSlot)
  _ -> Nothing

-- | The most urgent empty list outside @avoid@, for a stale bitmap bit.
staleBit :: Bool -> [Int] -> M.Map Int [Int] -> Maybe (Maybe Int)
staleBit False _ _ = Just Nothing
staleBit True avoid lists = Just <$> find (\q -> not (M.member q lists) && q `notElem` avoid) [0 .. lowestUrgency]

extraPriority :: Int -> Int
extraPriority p = if p + 2 <= lowestUrgency then p + 2 else p - 2

hexTag :: Word32 -> String
hexTag value = "0x" ++ showHex value ""

tag :: String -> String -> String
tag dimension value = dimension ++ "=" ++ value

relationTag :: Relation -> String
relationTag relation = case relation of
  SelNull -> "null"
  SelHigher -> "higher"
  SelLower -> "lower"
  SelEqual -> "equal"
  SelTask -> "task"

cycleOver :: [a] -> Int -> a
cycleOver values index = values !! (index `mod` length values)

-- | Gives row @i@ the @i@-th of each value the helper does not sweep.
cycled :: Layout -> Helper -> [Row] -> [Row]
cycled layout helper = zipWith fill [0 ..]
  where
    fill i row =
      let state = if helper `elem` [Insert, Prepare] then cycleOver (stateBytes layout) i else rowState row
          spTerms = if helper == Prepare then cycleOver spTermPairs i else rowSpTerms row
          definition = if helper == Reset then cycleOver definitionPriorities i else rowDefinitionPriority row
          stale = if helper == Requeue && odd i then staleBit True [] (rowLists row) else Just (rowStale row)
          extraTags =
            [tag "state" (hexTag (fromIntegral state)) | helper `elem` [Insert, Prepare]]
              ++ [tag "sp" (hexTag (fst spTerms)) | helper == Prepare]
              ++ [tag "definition" (hexTag definition) | helper == Reset]
              ++ [tag "stale" (show (isJust (fromMaybe Nothing stale))) | helper == Requeue]
       in row {rowState = state, rowSpTerms = spTerms, rowDefinitionPriority = definition, rowStale = fromMaybe Nothing stale, rowTags = rowTags row ++ extraTags}

caseGrid :: Layout -> Helper -> [Row]
caseGrid layout helper = cycled layout helper $ case helper of
  Acquire -> lockRows
  Release -> lockRows
  Find ->
    [ base {rowLocality = locality, rowLists = lists, rowStale = stale, rowTags = [tag "loc" (show locality), tag "p" (show p), tag "shape" (show shape), tag "extra" (show extra), tag "stale" (show stale')]}
    | locality <- [minBound .. maxBound]
    , p <- focusPriorities
    , shape <- [One, Many]
    , extra <- [False, True]
    , stale' <- [False, True]
    , let lists = listsFrom ((p, shapeMembers shape [taskSlot, 2, 3]) : [(extraPriority p, [extraSlot]) | extra])
    , Just stale <- [staleBit stale' [] lists]
    ]
  Insert -> insertRows
  Prepare -> insertRows
  Remove ->
    [ base {rowLocality = locality, rowLists = lists, rowSelected = selected, rowStale = stale, rowDispatch = dispatch, rowTags = tags}
    | locality <- [minBound .. maxBound]
    , p <- focusPriorities
    , position <- [minBound .. maxBound]
    , relation <- [minBound .. maxBound]
    , extra <- [False, True]
    , stale' <- [False, True]
    , dispatch <- dispatchValues
    , Just (lists, selected) <- [relate p relation (listsFrom ((p, positionMembers position) : [(extraPriority p, [extraSlot]) | extra]))]
    , Just stale <- [staleBit stale' [p] lists]
    , let tags = [tag "loc" (show locality), tag "p" (show p), tag "pos" (show position), tag "sel" (relationTag relation), tag "sel-pos" (relationTag relation ++ "-" ++ show position), tag "extra" (show extra), tag "stale" (show stale'), tag "dispatch" (hexTag dispatch)]
    ]
  Reset ->
    [ base {rowLocality = locality, rowState = state, rowFlags = flags, rowTaskPriority = 9, rowTags = [tag "loc" (show locality), tag "state" (hexTag (fromIntegral state)), tag "flags" (hexTag (fromIntegral flags))]}
    | locality <- [minBound .. maxBound]
    , state <- stateBytes layout
    , flags <- flagBytes
    ]
  Notify -> [base {rowArgument = ident, rowTags = [tag "id" (hexTag ident)]} | ident <- notifyIds]
  Requeue ->
    [ base {rowLocality = locality, rowLists = lists, rowSelected = selected, rowDispatch = dispatch, rowArgument = fromIntegral to, rowTags = tags}
    | locality <- [minBound .. maxBound]
    , p <- focusPriorities
    , position <- [minBound .. maxBound]
    , to <- nub [q | q <- [p, p - 1, p + 1, 0, lowestUrgency], q >= 0, q <= lowestUrgency]
    , target <- if to == p then [Empty] else [Empty, One]
    , relation <- [SelHigher, SelLower, SelEqual, SelTask]
    , dispatch <- dispatchValues
    , Just (lists, selected) <- [relate p relation (listsFrom ((p, positionMembers position) : [(to, [targetSlot]) | target == One]))]
    , let tags =
            [tag "loc" (show locality), tag "p" (show p), tag "pos" (show position), tag "to" (direction p to), tag "target" (show target), tag "sel" (relationTag relation), tag "sel-pos" (relationTag relation ++ "-" ++ show position), tag "sel-to" (relationTag relation ++ "-" ++ direction p to), tag "dispatch" (hexTag dispatch)]
              ++ [tag "selected-behind-peers" (hexTag dispatch) | relation == SelTask, to == p, position == AtHead]
    ]
  Rotate ->
    [ base {rowLocality = locality, rowLists = lists, rowSelected = selected, rowStale = stale, rowDispatch = dispatch, rowArgument = fromIntegral p, rowTags = tags}
    | locality <- [minBound .. maxBound]
    , p <- focusPriorities
    , shape <- [minBound .. maxBound]
    , relation <- [SelNull, SelTask, SelHigher, SelLower]
    , stale' <- [False, True]
    , dispatch <- dispatchValues
    , Just (lists, selected) <- [relate p relation (listsFrom [(p, shapeMembers shape [taskSlot, 2, 3])])]
    , Just stale <- [staleBit stale' [] lists]
    , let tags = [tag "loc" (show locality), tag "p" (show p), tag "shape" (show shape), tag "sel" (relationTag relation), tag "sel-shape" (relationTag relation ++ "-" ++ show shape), tag "stale" (show stale'), tag "dispatch" (hexTag dispatch)]
    ]
  where
    base = baseRow layout
    direction p to
      | to == p = "same"
      | to < p = "urgent" ++ show to
      | otherwise = "lax" ++ show to
    lockRows =
      [ base {rowLocality = locality, rowLock = lock, rowLockWord = word, rowTags = [tag "loc" (show locality), tag "lock" (show lock), tag "word" (hexTag word)]}
      | lock <- [minBound .. maxBound]
      , locality <- [minBound .. maxBound]
      , word <- lockWords
      ]
    insertRows =
      [ base {rowLocality = locality, rowLists = lists, rowSelected = selected, rowStale = stale, rowDispatch = dispatch, rowTaskPriority = fromIntegral p, rowTags = tags}
      | locality <- [minBound .. maxBound]
      , p <- focusPriorities
      , shape <- [minBound .. maxBound]
      , relation <- [SelNull, SelHigher, SelLower, SelEqual]
      , stale' <- [False, True]
      , dispatch <- dispatchValues
      , Just (lists, selected) <- [relate p relation (listsFrom [(p, shapeMembers shape [2, 3, 4])])]
      , Just stale <- [staleBit stale' [p] lists]
      , let tags = [tag "loc" (show locality), tag "p" (show p), tag "shape" (show shape), tag "sel" (relationTag relation), tag "sel-shape" (relationTag relation ++ "-" ++ show shape), tag "stale" (show stale'), tag "dispatch" (hexTag dispatch)]
      ]

-- | The first row holding each tag, in grid order.
sampleKeys :: Layout -> Helper -> [CaseKey]
sampleKeys layout helper = sort (nub [key | t <- nub (concatMap rowTags grid), Just key <- [firstWith t]])
  where
    grid = caseGrid layout helper
    firstWith t = CaseKey . fst <$> find ((t `elem`) . rowTags . snd) (zip [0 ..] grid)

-- | An acquire of a held word spins.
rowSpins :: Helper -> Row -> Bool
rowSpins helper row = helper == Acquire && rowLockWord row /= 0

-- * Data layout

other :: Domain -> Domain
other domain = case domain of
  DomainA -> DomainB
  DomainB -> DomainA

described :: Domain -> Row -> Domain
described domain row = case rowLocality row of
  Local -> domain
  Remote -> other domain

taskAddress :: Layout -> Domain -> Int -> Word32
taskAddress layout domain slot = wordsTaskTable (domainWords layout domain) + fromIntegral (slot - 1) * taskSize (layoutTask layout)

-- | In @executing@'s kernel, the context record describing @of'@.
recordAddress :: Layout -> Domain -> Domain -> Word32
recordAddress layout executing of' = case of' of
  DomainA -> wordsContextA (domainWords layout executing)
  DomainB -> wordsContextB (domainWords layout executing)

lockWordAddress :: Layout -> Domain -> LockChoice -> Word32
lockWordAddress layout domain lock = case lock of
  SchedulerLock -> wordsSchedulerLock (domainWords layout domain)
  ObjectLock -> wordsObjectLock (domainWords layout domain)

definitionAddress :: Domain -> Int -> Word32
definitionAddress domain slot = definitionBase + fromIntegral ((domainIndex domain * taskWindow + slot - 1) * fromIntegral definitionStride)
  where
    domainIndex d = case d of
      DomainA -> 0
      DomainB -> 1

-- | The stock lock pair entries of a loader, which context records hold.
lockEntries :: Env -> Domain -> Either String (Word32, Word32)
lockEntries env domain = do
  let table = schedulerDomain (envRegions env) domain
  acquire <- regionEntry <$> regionOf table Acquire
  release <- regionEntry <$> regionOf table Release
  pure (acquire, release)

-- | The lowest and end address of a domain's lock words, context records and scheduler cells.
blockBounds :: Layout -> DomainWords -> (Word32, Word32)
blockBounds layout w = (minimum (map fst spans), maximum [start + size | (start, size) <- spans])
  where
    spans =
      [ (wordsSchedulerLock w, 4)
      , (wordsObjectLock w, 4)
      , (wordsContexts w, 2 * contextSize (layoutContext layout))
      , (wordsReadyHeads w, layoutPriorities layout * layoutHeadSize layout)
      , (wordsReadyBitmap w, 4)
      , (wordsRunning w, 4)
      , (wordsDispatchEnabled w, 4)
      , (wordsSelected w, 4)
      ]

type Poke = (Word32, B.ByteString)

pokeWord :: Word32 -> Word32 -> Poke
pokeWord address value = (address, word32le value)

pokeByte :: Word32 -> Word8 -> Poke
pokeByte address value = (address, B.singleton value)

-- | Every byte the row sets in the data regions, later entries over earlier ones.
dataPokes :: Env -> Domain -> Row -> [Poke]
dataPokes env executing row =
  concat [recordPokes block of' | block <- domains, of' <- domains]
    ++ concatMap cellPokes domains
    ++ taskPokes
  where
    layout = envLayout env
    c = layoutContext layout
    l = layoutLock layout
    t = layoutTask layout
    d = layoutDefinition layout
    e = described executing row
    entries domain = either error id (lockEntries env domain)
    recordPokes block of' =
      let record = recordAddress layout block of'
          w = domainWords layout of'
          (acquire, release) = entries block
          lockRecord offset word = [pokeWord (record + offset + lockWord l) word, pokeWord (record + offset + lockAcquire l) acquire, pokeWord (record + offset + lockRelease l) release]
       in [ pokeWord (record + contextId c) (if block == of' then layoutOwnId layout else wordsId w)
          , pokeWord (record + contextOwner c) (wordsId w)
          , pokeWord (record + contextReadyHeads c) (wordsReadyHeads w)
          , pokeWord (record + contextReadyBitmap c) (wordsReadyBitmap w)
          , pokeWord (record + contextRunning c) (wordsRunning w)
          , pokeWord (record + contextSelected c) (wordsSelected w)
          , pokeWord (record + contextDispatchEnabled c) (wordsDispatchEnabled w)
          , pokeWord (record + contextFirstEntry c) (trampoline of')
          ]
            ++ lockRecord (contextSchedulerLock c) (wordsSchedulerLock w)
            ++ lockRecord (contextObjectLock c) (wordsObjectLock w)
    cellPokes domain =
      let w = domainWords layout domain
          active = domain == e
          lists = if active then rowLists row else M.empty
          headAt q = wordsReadyHeads w + fromIntegral q * layoutHeadSize layout
          task = taskAddress layout domain
          heads =
            concat
              [ [pokeWord (headAt q + layoutHeadNext layout) (maybe h (task . fst) (uncons' members)), pokeWord (headAt q + layoutHeadPrev layout) (if null members then h else task (last members))]
              | q <- [0 .. lowestUrgency]
              , let members = M.findWithDefault [] q lists
                    h = headAt q
              ]
          bitmap = foldl' (.|.) 0 [1 `shiftL` q | q <- M.keys lists] .|. maybe 0 (1 `shiftL`) (if active then rowStale row else Nothing)
          lockValue lock = if active && rowLock row == lock then rowLockWord row else 0
       in heads
            ++ [ pokeWord (wordsReadyBitmap w) bitmap
               , pokeWord (wordsSelected w) (if active then maybe 0 task (rowSelected row) else 0)
               , pokeWord (wordsRunning w) (if active then task runningSlot else 0)
               , pokeWord (wordsDispatchEnabled w) (if active then rowDispatch row else 1)
               , pokeWord (wordsSchedulerLock w) (lockValue SchedulerLock)
               , pokeWord (wordsObjectLock w) (lockValue ObjectLock)
               ]
    taskPokes =
      let task = taskAddress layout e
          inLists = concat (M.elems (rowLists row))
          records =
            concat
              [ [ pokeWord (task slot + taskDefinition t) (definitionAddress e slot)
                , pokeWord (task slot + taskDomain t) (wordsId (domainWords layout e))
                , pokeByte (task slot + taskState t) (stateDormant (layoutStates layout))
                , pokeByte (task slot + taskPriority t) (fromIntegral (slot `mod` 16))
                , pokeByte (task slot + taskFlags t) 0
                , pokeWord (definitionAddress e slot + definitionPriority d) (fromIntegral (slot `mod` 16))
                , pokeWord (definitionAddress e slot + fst (definitionSpTerms d)) (0x00950000 + fromIntegral slot * 0x100)
                , pokeWord (definitionAddress e slot + snd (definitionSpTerms d)) 0x100
                ]
              | slot <- [1 .. taskWindow]
              ]
          members =
            concat
              [ [ pokeWord (task slot + taskNext t) (if i + 1 < length ms then task (ms !! (i + 1)) else h)
                , pokeWord (task slot + taskPrev t) (if i > 0 then task (ms !! (i - 1)) else h)
                , pokeByte (task slot + taskPriority t) (fromIntegral q)
                , pokeByte (task slot + taskState t) (stateReady (layoutStates layout))
                ]
              | (q, ms) <- M.toList (rowLists row)
              , let h = wordsReadyHeads (domainWords layout e) + fromIntegral q * layoutHeadSize layout
              , (i, slot) <- zip [0 ..] ms
              ]
          argument =
            [pokeByte (task taskSlot + taskPriority t) (rowTaskPriority row) | taskSlot `notElem` inLists]
              ++ [ pokeByte (task taskSlot + taskState t) (rowState row)
                 , pokeByte (task taskSlot + taskFlags t) (rowFlags row)
                 , pokeWord (definitionAddress e taskSlot + definitionPriority d) (rowDefinitionPriority row)
                 , pokeWord (definitionAddress e taskSlot + fst (definitionSpTerms d)) (fst (rowSpTerms row))
                 , pokeWord (definitionAddress e taskSlot + snd (definitionSpTerms d)) (snd (rowSpTerms row))
                 ]
       in records ++ members ++ argument
    uncons' xs = case xs of
      x : rest -> Just (x, rest)
      [] -> Nothing

stackId, callerFrameId, exitId :: RegionId
stackId = RegionId "stack"
callerFrameId = RegionId "caller-frame"
exitId = RegionId "exit"

-- | The data regions as (id, base, size, permission, background).
dataRegions :: Layout -> [(RegionId, Word32, Int, Permission, B.ByteString)]
dataRegions layout =
  concat
    [ [ (RegionId ("block-" ++ domainLabel domain), low, fromIntegral (high - low), ReadWrite, background (0x5c4ed000 + index) (fromIntegral (high - low)))
      , (RegionId ("tasks-" ++ domainLabel domain), wordsTaskTable w, tasksBytes, ReadWrite, background (0x7a5c0000 + index) tasksBytes)
      ]
    | (index, domain) <- zip [0 ..] domains
    , let w = domainWords layout domain
          (low, high) = blockBounds layout w
    ]
    ++ [ (RegionId "definitions", definitionBase, 2 * taskWindow * fromIntegral definitionStride, ReadWrite, background 0xdef10000 (2 * taskWindow * fromIntegral definitionStride))
       , (stackId, stackBase, fromIntegral (stackPointer - stackBase), ReadWrite, B.replicate (fromIntegral (stackPointer - stackBase)) 0)
       , (callerFrameId, stackPointer, fromIntegral (callerTop - stackPointer), ReadWrite, B.concat [word32le (0xca11e500 + n) | n <- [0 .. (callerTop - stackPointer) `div` 4 - 1]])
       , (exitId, exitAddress, 4, ReadExecute, word32le 0xe12fff1e)
       ]
    ++ [(RegionId ("bank-" ++ show core), address, 4, Device, B.replicate 4 0) | (core, address) <- zip [0 :: Int ..] bankWords]
  where
    tasksBytes = taskWindow * fromIntegral (taskSize (layoutTask layout))

-- | xorshift bytes.
background :: Word32 -> Int -> B.ByteString
background seed size = B.take size (B.concat (map word32le (take (size `div` 4 + 1) (drop 1 (iterate step seed)))))
  where
    step x0 =
      let x1 = x0 `xor` (x0 `shiftL` 13)
          x2 = x1 `xor` (x1 `shiftR` 17)
       in x2 `xor` (x2 `shiftL` 5)

overlay :: Word32 -> B.ByteString -> [Poke] -> B.ByteString
overlay base = foldl' apply
  where
    apply bytes (address, value) =
      let at = fromIntegral (address - base)
       in B.take at bytes <> value <> B.drop (at + B.length value) bytes

-- | The pokes no data region holds.
pokesOutside :: Env -> Domain -> Row -> [Word32]
pokesOutside env executing row = [address | (address, value) <- dataPokes env executing row, not (any (inside address (B.length value)) (dataRegions (envLayout env)))]

inside :: Word32 -> Int -> (RegionId, Word32, Int, Permission, B.ByteString) -> Bool
inside address width (_, base, size, _, _) = base <= address && toInteger address + toInteger width <= toInteger base + toInteger size

-- | Each data region's initial bytes; fails on a poke outside every region.
dataImages :: Env -> Domain -> Row -> [CaseRegion]
dataImages env executing row =
  case pokesOutside env executing row of
    [] -> [mkRegion ident (GuestAddress (fromIntegral base)) size permission (overlay base bytes [p | p@(address, value) <- pokes, inside address (B.length value) region]) | region@(ident, base, size, permission, bytes) <- dataRegions (envLayout env)]
    outside -> error ("data pokes outside the regions: " ++ unwords [hexTag a | a <- outside])
  where
    pokes = dataPokes env executing row

-- * Bodies

-- | Executable regions, declared functions as (start, size), and the helper's own code, where
-- mutants patch.
data Body = Body
  { bodyEntry :: Word32
  , bodyCode :: [(Word32, B.ByteString)]
  , bodyFunctions :: [(Word32, Word32)]
  , bodyOwn :: [Extent]
  , -- | Read-only loader data the body reads.
    bodyData :: [(Word32, B.ByteString)]
  }
  deriving (Eq, Show)

-- | One loader's bytes, admitted at its pinned size.
data StockCode = StockCode
  { stockDomain :: Domain
  , stockLoader :: B.ByteString
  }

stockCode :: Domain -> B.ByteString -> Either String StockCode
stockCode domain loader = do
  unless (B.length loader == domainLoaderSize domain) (Left ("loader " ++ domainLabel domain ++ " size"))
  pure (StockCode domain loader)

-- | The stock raise-IRQ code as (address, bytes).
stockRaiseIrq :: SchedulerDomain -> StockCode -> Either String (Word32, B.ByteString)
stockRaiseIrq table code = (,) (domainRaiseIrq table) <$> stockSlice code (Extent (domainRaiseIrq table) (domainRaiseIrq table + stockRaiseIrqBytes))

stockSlice :: StockCode -> Extent -> Either String B.ByteString
stockSlice (StockCode domain loader) (Extent start end) = do
  let OwnFirst.GuestAddress base = domainLoaderBase domain
  unless (start >= base && end > start && fromIntegral (end - base) <= B.length loader) (Left ("extent " ++ hexTag start ++ " outside loader " ++ domainLabel domain))
  pure (B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - base)) loader))

-- | The table stock find indexes, at the movw/movt pair's base minus one, 16 bytes.
findTable :: StockCode -> Extent -> Either String (Word32, B.ByteString)
findTable code extent = do
  bytes <- stockSlice code extent
  let words' = [readWordAt bytes offset | offset <- [0, 4 .. B.length bytes - 4]]
      pairs = [(low .|. (high `shiftL` 16)) | (w0, w1) <- zip words' (drop 1 words'), Just (rd, low) <- [movw w0], Just (rd', high) <- [movt w1], rd == rd']
  case pairs of
    [base] -> do
      table <- stockSlice code (Extent (base - 1) (base + 15))
      pure (base - 1, table)
    _ -> Left "stock find has no single movw/movt table base"
  where
    movw w = if w .&. 0x0ff00000 == 0x03000000 then Just ((w `shiftR` 12) .&. 0xf, ((w `shiftR` 4) .&. 0xf000) .|. (w .&. 0xfff)) else Nothing
    movt w = if w .&. 0x0ff00000 == 0x03400000 then Just ((w `shiftR` 12) .&. 0xf, ((w `shiftR` 4) .&. 0xf000) .|. (w .&. 0xfff)) else Nothing

-- | Stock code of the helper and its callees, from the loader at the regions' stock extents.
stockBody :: SchedulerDomain -> StockCode -> Helper -> Either String Body
stockBody table code helper = do
  own <- regionOf table helper
  callees <- forM (regionCalls own) $ \callee -> case callee of
    CalleeHelper h -> regionExtent <$> regionOf table h
    CalleeStockRaiseIrq -> Right (Extent (domainRaiseIrq table) (domainRaiseIrq table + stockRaiseIrqBytes))
  let extents = regionExtent own : callees
  slices <- mapM (stockSlice code) extents
  findExtent <- regionExtent <$> regionOf table Find
  tableData <-
    if Find `elem` (helper : [h | CalleeHelper h <- regionCalls own])
      then (: []) <$> findTable code findExtent
      else Right []
  pure
    Body
      { bodyEntry = regionEntry own
      , bodyCode = zip (map extentStart extents) slices
      , bodyFunctions = [(start, end - start) | Extent start end <- extents]
      , bodyOwn = [regionExtent own]
      , bodyData = tableData
      }

-- | One loader's linked scheduler image: the flat bin from its lowest region start, with every
-- region symbol at its entry and every shim body symbol at its body.
data OwnedImage = OwnedImage
  { ownedBase :: Word32
  , ownedBin :: B.ByteString
  }

ownedImage :: SchedulerDomain -> B.ByteString -> M.Map String Symbol -> Either String OwnedImage
ownedImage table bin symbols = do
  let at name = maybe (Left ("owned symbol missing: " ++ name)) (\(Symbol address _) -> Right address) (M.lookup name symbols)
  forM_' (domainRegions table) $ \region -> do
    address <- at (regionSymbol region)
    unless (address == regionEntry region) (Left (regionSymbol region ++ " is not at its entry"))
  forM_' (domainShims table) $ \shim -> do
    address <- at (shimBodySymbol shim)
    unless (address == extentStart (shimBody shim)) (Left (shimBodySymbol shim ++ " is not at its body"))
  let base = minimum (map (extentStart . regionExtent) (domainRegions table))
      top = maximum (map (extentEnd . regionExtent) (domainRegions table))
  unless (B.length bin == fromIntegral (top - base)) (Left "owned bin does not span the regions")
  pure (OwnedImage base bin)
  where
    forM_' xs f = mapM_ f xs

-- | The owned helper, its shim bodies, its callees' regions and the raise-IRQ code given as
-- (address, bytes).
ownedBody :: SchedulerDomain -> OwnedImage -> (Word32, B.ByteString) -> Helper -> Either String Body
ownedBody table image (raiseAt, raiseBytes) helper = do
  own <- regionOf table helper
  let shimsOf h = [shim | shim <- domainShims table, shimHelper shim == h]
      reachHelpers = nub (helper : map shimHost (shimsOf helper) ++ concat [h : map shimHost (shimsOf h) | CalleeHelper h <- regionCalls own])
  regions <- mapM (regionOf table) reachHelpers
  let slice (Extent start end) = B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - ownedBase image)) (ownedBin image))
      hosted region = sort [shimBody shim | shim <- domainShims table, shimHost shim == regionHelper region]
      functions region =
        let Extent start end = regionExtent region
            cuts = nub (sort ([start, end] ++ concat [[s, e] | Extent s e <- hosted region]))
         in [(a, b - a) | (a, b) <- zip cuts (drop 1 cuts)]
      raise = [(raiseAt, raiseBytes) | CalleeStockRaiseIrq `elem` regionCalls own]
      entryFunction = [Extent a (a + size) | (a, size) <- functions own, a == regionEntry own]
  pure
    Body
      { bodyEntry = regionEntry own
      , bodyCode = [(extentStart (regionExtent region), slice (regionExtent region)) | region <- regions] ++ raise
      , bodyFunctions = concatMap functions regions ++ [(a, fromIntegral (B.length bytes)) | (a, bytes) <- raise]
      , bodyOwn = entryFunction ++ map shimBody (shimsOf helper)
      , bodyData = []
      }

-- | The declared function at the entry.
ownEntryFunction :: Body -> Either String (Word32, Word32)
ownEntryFunction body = maybe (Left "entry is not a declared function") (\size -> Right (bodyEntry body, size)) (lookup (bodyEntry body) (bodyFunctions body))

rewriteWords :: Word32 -> [Word32] -> Body -> Body
rewriteWords at replacement body = body {bodyCode = map rewrite (bodyCode body)}
  where
    bytes = B.concat (map word32le replacement)
    rewrite (base, code)
      | base <= at && at < base + fromIntegral (B.length code) =
          let offset = fromIntegral (at - base) in (base, B.take offset code <> bytes <> B.drop (offset + B.length bytes) code)
      | otherwise = (base, code)

-- | The body with the function at its entry rewritten as @code@ followed by fill words.
replaceEntry :: Word32 -> [Word32] -> Body -> Either String Body
replaceEntry fill code body = do
  (entry, size) <- ownEntryFunction body
  unless (4 * length code <= fromIntegral size) (Left ("code of " ++ show (length code) ++ " words exceeds the " ++ show size ++ "-byte function"))
  pure (rewriteWords entry (code ++ replicate (fromIntegral size `div` 4 - length code) fill) body)

-- | Rewrites the single word of the helper's own code that @matches@.
patchOwn :: (Word32 -> Bool) -> (Word32 -> Word32) -> Body -> Either String Body
patchOwn matches rewrite body =
  case [(address, word) | Extent start end <- bodyOwn body, address <- [start, start + 4 .. end - 4], Just word <- [wordAt address], matches word] of
    [(address, word)] -> Right (rewriteWords address [rewrite word] body)
    found -> Left ("the patch matches " ++ show (length found) ++ " words of the helper's code, not one")
  where
    wordAt address = case [(base, code) | (base, code) <- bodyCode body, base <= address, address + 4 <= base + fromIntegral (B.length code)] of
      (base, code) : _ -> Just (readWordAt code (fromIntegral (address - base)))
      [] -> Nothing

-- * Cases

-- | r1-r3, r12, lr and the CPSR flags. Every direct caller of each helper, in both loaders, sets
-- the flags before testing them and writes r1-r3, r12 and lr before reading them.
uncomparedRegisters :: Helper -> Row -> [Register]
uncomparedRegisters helper row = [R1, R2, R3, R12, Lr, Cpsr] ++ [r | rowSpins helper row, r <- [R11, Sp, Pc]]

uncomparedReason :: String
uncomparedReason =
  "r1-r3, r12, lr and the CPSR flags are not compared: every direct caller of each helper in both loaders sets the flags before testing them and writes r1-r3, r12 and lr before reading them (caller scan of RtosContracts.ReviewS1Crate finding 2). A spinning acquire also leaves r11, sp and pc uncompared, since it stops inside its own frame."

findTableReason :: String
findTableReason =
  "Stock find reads one byte of its lowest-set-bit table in the loader (A 0x4daa0, B 0xdda64); owned find uses rbit/clz and reads no table. The table is mapped read-only for the stock role only; reads are not compared, and every bitmap with a bit in 0..15 selects the same list (ReviewS1Crate finding 5)."

caseSpec :: Env -> Domain -> Body -> String -> Helper -> Row -> CaseSpec
caseSpec env domain body label helper row =
  CaseSpec
    { caseId = label
    , caseProvenance = "X-M1 1.50 loader " ++ domainLabel domain ++ " scheduler " ++ helperName helper ++ " over the rtos_layout stock data layout"
    , caseExclusion = "one call of the helper with its callees; reads or calls outside the mapped regions refuse"
    , caseEntry = GuestAddress (fromIntegral (bodyEntry body))
    , caseExit = GuestAddress (fromIntegral exitAddress)
    , caseRegions =
        [mkRegion (RegionId ("code-" ++ showHex base "")) (GuestAddress (fromIntegral base)) (B.length bytes) ReadExecute bytes | (base, bytes) <- bodyCode body]
          ++ [mkRegion (RegionId ("table-" ++ showHex base "")) (GuestAddress (fromIntegral base)) (B.length bytes) ReadOnly bytes | (base, bytes) <- bodyData body]
          ++ dataImages env domain row
    , caseFunctions = [(GuestAddress (fromIntegral start), size) | (start, size) <- bodyFunctions body]
    , caseInitial =
        [(register, M.findWithDefault (sentinel register) register arguments) | register <- [R0 .. R12]]
          ++ [(Sp, stackPointer), (Lr, exitAddress), (Cpsr, entryCpsr)]
    , caseExpectedStatus = if rowSpins helper row then InstructionBudget else Returned
    , caseExpectedRegisters = [(R4, sentinel R4)]
    , caseExpectedMemory = []
    , caseCp14Identity = Nothing
    , caseKernel = Just (KernelCase instructionLimit Nothing Nothing Nothing Nothing (uncomparedRegisters helper row))
    }
  where
    layout = envLayout env
    e = described domain row
    record = recordAddress layout domain e
    task = taskAddress layout e taskSlot
    lockRecord = record + case rowLock row of
      SchedulerLock -> contextSchedulerLock (layoutContext layout)
      ObjectLock -> contextObjectLock (layoutContext layout)
    arguments = M.fromList $ case helper of
      Acquire -> [(R0, lockRecord)]
      Release -> [(R0, lockRecord)]
      Find -> [(R0, record)]
      Insert -> [(R0, record), (R1, task)]
      Remove -> [(R0, record), (R1, task)]
      Prepare -> [(R0, record), (R1, task)]
      Reset -> [(R0, task)]
      Notify -> [(R0, rowArgument row)]
      Requeue -> [(R0, record), (R1, task), (R2, rowArgument row)]
      Rotate -> [(R0, record), (R1, rowArgument row)]
    sentinel register = 0xa5a50000 + fromIntegral (fromEnum register)

-- | Returned, or for a held word the spin on that word.
endsAsExpected :: Env -> Domain -> Helper -> Row -> CaseSpec -> NativeObservation -> Bool
endsAsExpected env domain helper row spec observation = case caseOutcome spec observation of
  OutcomeReturned -> not (rowSpins helper row)
  OutcomeNoReturnWithin _ (Just (GuestAddress reserved)) -> rowSpins helper row && reserved == fromIntegral (lockWordAddress (envLayout env) (described domain row) (rowLock row))
  _ -> False

isScratch :: Word32 -> Bool
isScratch address = stackBase <= address && address < stackPointer

-- | Drops the stack below entry sp and the code regions: its bytes, and its writes from the order.
stripScratch :: NativeObservation -> NativeObservation
stripScratch observation =
  observation
    { nativeMemory = [(ident, B.copy bytes) | (ident, bytes) <- nativeMemory observation, ident /= stackId, take 5 (regionName ident) /= "code-", take 6 (regionName ident) /= "table-"]
    , nativeKernel = fmap dropScratch (nativeKernel observation)
    }
  where
    dropScratch kernel = kernel {observedOrder = filter (not . scratchWrite) <$> observedOrder kernel}
    scratchWrite item = case item of
      OrderedWrite (Write (GuestAddress address) _) _ -> isScratch (fromIntegral address)
      _ -> False

data Difference
  = KernelDiffers KernelDifference
  | -- | The owned order assertion fails.
    BarrierOrder
  | Missing
  deriving (Eq, Show)

differenceText :: Difference -> String
differenceText difference = case difference of
  KernelDiffers kernel -> show kernel
  BarrierOrder -> "barrier order"
  Missing -> "missing run"

-- | Kernel-case comparison over the regions above the stack, and with @asserted@ the subject's barrier order.
compareCase :: Env -> Domain -> Helper -> Row -> CaseSpec -> Bool -> NativeObservation -> NativeObservation -> [Difference]
compareCase env domain helper row spec asserted reference subject =
  map KernelDiffers (compareKernelCase spec {caseRegions = filter compared (caseRegions spec)} reference subject)
    ++ [BarrierOrder | asserted, not (orderHolds env domain helper row subject)]
  where
    compared (CaseRegion region _) = N.regionIdOf region /= stackId

-- | Prepare: the writes before the first dmb are exactly T+0x18 and T+0x1c, and a write follows it.
-- Notify: a dsb precedes every bank write.
orderHolds :: Env -> Domain -> Helper -> Row -> NativeObservation -> Bool
orderHolds env domain helper row observation = case nativeKernel observation >>= observedOrder of
  Nothing -> False
  Just items -> case helper of
    Prepare ->
      let (before, rest) = break (== OrderedBarrier Dmb) items
       in sort (writes before) == sort [task + taskSavedSp t, task + taskResumePc t] && not (null (writes (drop 1 rest)))
    Notify -> fenced False items
    _ -> True
  where
    layout = envLayout env
    t = layoutTask layout
    task = taskAddress layout (described domain row) taskSlot
    writes items = [fromIntegral address | OrderedWrite (Write (GuestAddress address) _) _ <- items] :: [Word32]
    fenced seen items = case items of
      [] -> True
      OrderedBarrier Dsb : rest -> fenced True rest
      OrderedWrite (Write (GuestAddress address) _) _ : rest -> (seen || fromIntegral address `notElem` bankWords) && fenced seen rest
      _ : rest -> fenced seen rest

-- * Runs

caseLabel :: String -> Domain -> Helper -> CaseKey -> String
caseLabel role domain helper (CaseKey index) = role ++ "-" ++ domainLabel domain ++ "-" ++ helperName helper ++ "-" ++ pad (show index)
  where
    pad text = replicate (5 - length text) '0' ++ text

batchLimit :: Int
batchLimit = 4096

-- | Runs the labelled cases in @directory@, one runner session per 4096; each observation stripped.
runCases :: FilePath -> FilePath -> [(String, CaseSpec)] -> IO (M.Map String NativeObservation)
runCases runner directory cases = do
  results <- forM (zip [0 :: Int ..] (chunks cases)) $ \(batch, labelled) -> do
    path <- writeBatch (directory </> ("batch-" ++ show batch)) (map snd labelled)
    observed <- loadObservedBatch runner path
    when (length observed /= length labelled) (fail (path ++ ": observation count differs from the case count"))
    forM observed $ \(CaseId label, observation) -> do
      let stripped = stripScratch observation
      _ <- evaluate (forceObservation stripped)
      pure (label, stripped)
  pure (M.fromList (concat results))
  where
    chunks items = case splitAt batchLimit items of
      (chunk, []) -> [chunk | not (null chunk)]
      (chunk, rest) -> chunk : chunks rest

forceObservation :: NativeObservation -> Int
forceObservation observation =
  sum (map (B.length . snd) (nativeMemory observation)) + maybe 0 (maybe 0 length . observedOrder) (nativeKernel observation) + M.size (nativeRegisters observation)

data Outcome = Outcome
  { outcomeCases :: Int
  , outcomeMatches :: Int
  , outcomeFirst :: Maybe (CaseKey, [Difference])
  , -- | Differing cases whose subject ended as the row expects.
    outcomeDifferingCompleted :: Int
  , outcomeDifferingOther :: Int
  , outcomeStatuses :: M.Map String Int
  }
  deriving (Eq, Show)

-- | Each subject case against the reference case of the same key; a case missing on either side differs.
outcomeOf :: (CaseKey -> NativeObservation -> Bool) -> M.Map CaseKey NativeObservation -> M.Map CaseKey NativeObservation -> (CaseKey -> NativeObservation -> NativeObservation -> [Difference]) -> Outcome
outcomeOf completed reference subject compareOne =
  Outcome
    { outcomeCases = length rows
    , outcomeMatches = length (filter (null . snd) rows)
    , outcomeFirst = find (not . null . snd) rows
    , outcomeDifferingCompleted = length (filter done differing)
    , outcomeDifferingOther = length (filter (not . done) differing)
    , outcomeStatuses = statusCounts subject
    }
  where
    rows = mapMaybe row (M.keys (M.union reference subject))
    differing = [key | (key, differences) <- rows, not (null differences)]
    done key = maybe False (completed key) (M.lookup key subject)
    row key = case (M.lookup key reference, M.lookup key subject) of
      (Just expected, Just actual) -> Just (key, compareOne key expected actual)
      (Nothing, Nothing) -> Nothing
      _ -> Just (key, [Missing])

statusCounts :: M.Map CaseKey NativeObservation -> M.Map String Int
statusCounts runs = M.fromListWith (+) [(N.runnerStatusLabel (nativeStatus run), 1) | run <- M.elems runs]
