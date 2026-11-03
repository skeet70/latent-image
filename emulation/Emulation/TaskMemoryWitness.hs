module Emulation.TaskMemoryWitness
  ( Memory(..), CoreOrdinals, ReplayError(..), CaptureCoverage(..), PhysicalSpan(..), CodeImage(..), CapturePlan(..)
  , coreOrdinals, coreOrdinal, unionRanges, instructionWord, checkWatch, checkFetch, advance
  ) where

import Control.Monad (unless)
import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.List (find, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word8, Word32, Word64)
import Emulation.TaskMemoryWire
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.KernelInstall.Wire as W

newtype Memory = Memory (M.Map Word32 Word8)
data CoreOrdinals = CoreOrdinals !W.Count !W.Count
data CaptureCoverage = ClassifiedAccess | AttributedInstruction
data ReplayError = WatchRejected | ClockRejected | FetchRejected | WriteRejected | ForeignWriterRejected
  | PreimageRejected | CodeRejected deriving (Eq, Show)
data PhysicalSpan = PhysicalSpan PhysicalBoundary PhysicalBoundary deriving (Eq, Show)
data CodeImage = CodeImage {imageBase :: W.Address, imageBytes :: W.Bytes}
data CapturePlan = CapturePlan
  { planBudget :: W.Count, planClasses :: [W.WatchClass], planAttributionRanges :: [PhysicalSpan]
  , planImmutableRanges :: [PhysicalSpan], planIrqCores :: M.Map W.Address W.Core, planCode :: [CodeImage] }

require :: Bool -> ReplayError -> Either ReplayError ()
require condition refusal = unless condition (Left refusal)
need :: ReplayError -> Maybe a -> Either ReplayError a
need refusal = maybe (Left refusal) Right
count :: W.Count -> Word64
count (W.Count value) = value
address :: W.Address -> Word32
address (W.Address value) = value
bytes :: W.Bytes -> B.ByteString
bytes (W.Bytes value) = value
physical :: Word32 -> Word32
physical value = value .&. 0x3fffffff
bounds :: PhysicalSpan -> (Word64,Word64)
bounds (PhysicalSpan (PhysicalBoundary low) (PhysicalBoundary high)) = (low,high)
coreOrdinals :: [W.Count] -> Either ReplayError CoreOrdinals
coreOrdinals [primary,secondary] = Right (CoreOrdinals primary secondary)
coreOrdinals _ = Left ClockRejected
coreOrdinal :: W.Core -> CoreOrdinals -> W.Count
coreOrdinal W.Primary (CoreOrdinals primary _) = primary
coreOrdinal W.Secondary (CoreOrdinals _ secondary) = secondary
withOrdinal :: W.Core -> W.Count -> CoreOrdinals -> CoreOrdinals
withOrdinal W.Primary value (CoreOrdinals _ secondary) = CoreOrdinals value secondary
withOrdinal W.Secondary value (CoreOrdinals primary _) = CoreOrdinals primary value
unionRanges :: [[Word64]] -> [[Word64]]
unionRanges = reverse . foldl merge [] . sort
  where merge ([low,high]:rest) [start,end] | start <= high = [low,max high end]:rest
        merge accumulated item = item:accumulated
instructionWord :: CapturePlan -> W.Address -> Either ReplayError Word32
instructionWord plan location = do
  let pc = address location
  source <- need CodeRejected (find (\image -> let base = address (imageBase image)
    in pc >= base && toInteger pc+4 <= toInteger base+toInteger (B.length (bytes (imageBytes image)))) (planCode plan))
  require (pc `mod` 4 == 0) CodeRejected
  pure (U.word32At (bytes (imageBytes source)) (fromIntegral (pc-address (imageBase source))))
eventShape :: CapturePlan -> CaptureCoverage -> W.MemoryEvent -> Either ReplayError (W.Core,W.Address,Word64,Word64)
eventShape plan coverage event = do
  core <- need FetchRejected (W.eventCore event)
  pc <- need FetchRejected (W.eventPc event)
  clock <- count <$> need ClockRejected (W.eventInstructions event)
  _ <- need ClockRejected (W.eventTick event)
  target <- need WatchRejected (W.eventPhysical event)
  require (clock <= count (planBudget plan) && W.eventAddress event <= 0xffffffff && target <= 0xffffffff
    && physical (fromIntegral (W.eventAddress event)) == fromIntegral target && W.eventByteOrder event == Just W.Little) WatchRejected
  let classes = case coverage of
        ClassifiedAccess -> planClasses plan
        AttributedInstruction -> [W.WatchClass [low,high] [W.CpuFetch] | span' <- planAttributionRanges plan,let (low,high) = bounds span']
  require (any (\classification -> case W.classRange classification of
    [low,high] -> target >= low && toInteger target+toInteger (W.eventSize event) <= toInteger high
      && W.eventSource event `elem` W.classKinds classification
    _ -> False) classes) WatchRejected
  pure (core,pc,clock,target)
checkFetch :: CapturePlan -> CaptureCoverage -> W.MemoryEvent -> Either ReplayError ()
checkFetch plan coverage event = do
  (core,pc,_,target) <- eventShape plan coverage event
  require (W.eventSource event == W.CpuFetch && W.eventAddress event == fromIntegral (address pc)
    && target == fromIntegral (address pc) && W.eventSize event == 4
    && W.eventValue event == Nothing && W.eventBefore event == Nothing) FetchRejected
  require (maybe True (==core) (M.lookup pc (planIrqCores plan))) FetchRejected
checkWriterFetch :: CapturePlan -> CapturedEvent -> Either ReplayError ()
checkWriterFetch plan observed = do
  fetched <- need WriteRejected (eventLatestFetch observed)
  let event = eventRecord observed; source = fetchRecord fetched
  checkFetch plan AttributedInstruction source
  require (W.eventCore source == W.eventCore event && W.eventPc source == W.eventPc event
    && W.eventInstructions source == W.eventInstructions event && W.eventTick source == W.eventTick event
    && eventCoreInstructions observed == Just (fetchCoreInstructions fetched)) WriteRejected
advance :: CapturePlan -> CoreOrdinals -> CoreOrdinals -> Word64 -> Word64
  -> (Memory,Word64,CoreOrdinals) -> CapturedEvent -> Either ReplayError (Memory,Word64,CoreOrdinals)
advance plan previousCounts nextCounts lower upper (memory,previousClock,ordinals) observed = do
  let event = eventRecord observed
  require (W.eventSource event /= W.HostWrite && W.eventSource event /= W.ModelBusWrite) ForeignWriterRejected
  (core,pc,clock,target) <- eventShape plan ClassifiedAccess event
  require (clock > lower && clock >= previousClock && clock <= upper) ClockRejected
  let refusal = if W.eventSource event == W.CpuWrite then WriteRejected else ClockRejected
  ordinal <- need refusal (eventCoreInstructions observed)
  require (count ordinal > count (coreOrdinal core previousCounts) && count ordinal <= count (coreOrdinal core nextCounts)
    && count ordinal >= count (coreOrdinal core ordinals)) refusal
  let advanced = withOrdinal core ordinal ordinals
  _ <- instructionWord plan pc
  case W.eventSource event of
    W.CpuFetch -> do
      checkFetch plan ClassifiedAccess event
      require (eventLatestFetch observed == Nothing) FetchRejected
      pure (memory,clock,advanced)
    W.CpuWrite -> do
      checkWriterFetch plan observed
      require (not (any (\span' -> let (low,high) = bounds span' in target < high && low < target+W.eventSize event) (planImmutableRanges plan))) WriteRejected
      let size = W.eventSize event
      require (size `elem` [1,2,4,8]) WriteRejected
      before <- bytes <$> need WriteRejected (W.eventBefore event)
      value <- need WriteRejected (W.eventValue event)
      require (B.length before == fromIntegral size && (size == 8 || value < (1 `shiftL` fromIntegral (8*size)))) WriteRejected
      let Memory old = memory
      actual <- B.pack <$> mapM (\offset -> need PreimageRejected (M.lookup (fromIntegral target+offset) old)) [0..fromIntegral size-1]
      require (actual == before) PreimageRejected
      let after = B.pack [fromIntegral (value `shiftR` fromIntegral (8*i)) | i <- [0..size-1]]
      pure (Memory (foldl (\contents (offset,byte) -> M.insert (fromIntegral target+offset) byte contents) old (zip [0..] (B.unpack after))),clock,advanced)
    _ -> Left WatchRejected
checkWatch :: CapturePlan -> CapturedWatch -> Either ReplayError ()
checkWatch plan watch = do
  require (captureEnabled watch && captureViolation watch == Nothing && captureViews watch == ObserveWatch
    && length (captureEvents watch) <= 1048576) WatchRejected
  require (captureAttribution watch == FetchAttribution [W.CpuWrite]
    [FetchRange low high | PhysicalSpan low high <- planAttributionRanges plan]) WatchRejected
  let kinds = [W.CpuFetch,W.CpuWrite,W.HostWrite,W.ModelBusWrite]
      rangesFor kind classes = unionRanges [W.classRange item | item <- classes,kind `elem` W.classKinds item]
  require (not (null (captureClasses watch)) && length (captureClasses watch) <= 64) WatchRejected
  mapM_ (\item -> do
    require (not (null (W.classKinds item)) && all (`elem` kinds) (W.classKinds item)) WatchRejected
    case W.classRange item of
      [low,high] -> require (low < high && high <= 0x100000000) WatchRejected
      _ -> Left WatchRejected) (captureClasses watch)
  mapM_ (\kind -> require (rangesFor kind (captureClasses watch) == rangesFor kind (planClasses plan)) WatchRejected) kinds
  require (captureRanges watch == unionRanges (map W.classRange (planClasses plan))) WatchRejected
