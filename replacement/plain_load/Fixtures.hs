module Fixtures (Fault (..), fixture, expectedRefusal) where

import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.PlainLoad.Admission (AdmissionError (..))

-- Project-owned complete A32 programs assembled independently of compiler output.
data Fault = Control | EquivalentImmediate | UnreachableNop
  | WrongLookupId | MissingExtent | NullReturns | NullCallsMode | WrongModeComparison
  | WrongReadTarget | WrongReadOffset | HardcodedOffset | HardcodedCount | HardcodedDestination | WrongDestinationField
  | DroppedOffsetRead | DroppedLengthRead | WrongFifthArgument | FifthBelowSp | WrongCallback
  | WrongFirstResource | WrongSecondResource | WrongTimeout | ReversedRelease | WrongDelay
  | MissingDelay | MissingProgress | WrongProgressScale | WrongRounding | NonwrappingCount
  | CachedFinalLength | WrongFinalValue | HardcodedFinalLength | DroppedFinalProgress | WrongErrorStatus | LostReadStatus | LeakedIgnoredStatus
  | WrongRestore | WrongLr | BelowSpStore | BelowSpLoad | UninitializedLoad | MisalignedCall
  | StackOutside | UnprovenSp | DeallocatedLoad | OutOfRangeLoad | OutOfRangeControl
  | VolatileRegister | VolatileFlags | ZeroLsr | RegisterShift | YieldInstruction
  | StoreOverlapFault | LoadOverlapFault | PushSp | PopSp
  deriving (Eq, Show, Enum, Bounded)

data Label = Body | Nonzero | Loop | Release | Final | BodyReturn | Spin | Padding deriving (Eq, Ord, Show)
data Site = AdapterPush | AdapterCall | AdapterLow | AdapterHigh | AdapterReturn | BodyPush
  | LookupArg | LookupCall | SaveSegment | ExtentCall | SaveExtent | NullCompare | NullBranch
  | ModeCall | RoundingLow | RoundingHigh | ModeCompare | ModeBranch
  | OffsetLoad | LengthLoad | RoundAdd | OffsetShift | CountShift | DestinationLoad
  | FifthSet | FifthStore | CallbackLow | CallbackHigh | ReadCall | SaveStatus | SkipDry
  | DryLengthLoad | DryRoundAdd | DryCountShift | FirstResource | FirstTimeout | FirstTake
  | SecondResource | SecondTimeout | SecondTake | IndexZero | LoopCompare | LoopExit
  | DelayArg | DelayCall | Increment | ProgressScale | ProgressCall | LoopBack
  | FirstReleaseArg | FirstRelease | SecondReleaseArg | SecondRelease | DryStatus
  | FinalLoad | FinalProbe | FinalCall | StatusMove | ReturnBody | SpinBranch | PaddingWord | InjectedValue
  deriving (Eq, Ord, Show)
data Op = Raw Word32 | Jump Word32 Label | Invoke Word32 | Literal Word32 Label
data Item = Mark Label | At Site Op

origin :: Word32
origin = 0x6a9844

fixture :: Fault -> B.ByteString
fixture fault = B.concat (map U.putWord32 (reverse encoded))
  where
    items = concatMap alter program
    alter (At PaddingWord _) | fault == NonwrappingCount = []
    alter item@(At DryCountShift _) | fault == NonwrappingCount = [item,At InjectedValue (Raw 0x23866c80)]
    alter (At FinalProbe _) | fault == CachedFinalLength = []
    alter (At PaddingWord _) | fault == CachedFinalLength = []
    alter item@(At LengthLoad _) | fault == CachedFinalLength = [item,At InjectedValue (Raw 0xe1a07001)]
    alter item@(At DryLengthLoad _) | fault == CachedFinalLength = [item,At InjectedValue (Raw 0xe1a07001)]
    alter (At FinalProbe _) | fault == HardcodedDestination = []
    alter item@(At DestinationLoad _) | fault == HardcodedDestination = [item,At InjectedValue (Literal 2 Padding)]
    alter item = [item]
    (_,labels) = foldl locate (origin,M.empty) items
    locate (pc,found) (Mark label) = (pc,M.insert label pc found)
    locate (pc,found) _ = (pc+4,found)
    (_,encoded) = foldl emit (origin,[]) items
    emit (pc,wordsSoFar) (Mark _) = (pc,wordsSoFar)
    emit (pc,wordsSoFar) (At site original) =
      let operation = maybe original id (lookup site (changes fault))
          word = case operation of
            Raw value -> value
            Jump opcode label -> branch opcode pc (labels M.! label)
            Invoke target -> branch 0xeb000000 pc target
            Literal register label -> 0xe59f0000 .|. (register `shiftL` 12) .|. (labels M.! label-pc-8)
      in (pc+4,word:wordsSoFar)

wide :: Word32 -> Word32 -> Word32
wide register word = 0xe3000000 .|. (register `shiftL` 12) .|. ((word .&. 0xf000) `shiftL` 4) .|. (word .&. 0xfff)
top :: Word32 -> Word32 -> Word32
top register word = 0xe3400000 .|. (register `shiftL` 12) .|. ((word .&. 0xf000) `shiftL` 4) .|. (word .&. 0xfff)
branch :: Word32 -> Word32 -> Word32 -> Word32
branch opcode site target = opcode .|. (fromInteger ((toInteger target-toInteger site-8) `div` 4) .&. 0xffffff)

program :: [Item]
program =
  [ At AdapterPush (Raw 0xe92d4800),At AdapterCall (Jump 0xeb000000 Body)
  , At AdapterLow (Raw (wide 14 0x992c)),At AdapterHigh (Raw (top 14 0x6a)),At AdapterReturn (Raw 0xe8bd8800)
  , Mark Body,At BodyPush (Raw 0xe92d407c),At LookupArg (Raw 0xe3a00006),At LookupCall (Invoke 0x657114)
  , At SaveSegment (Raw 0xe1a05000),At ExtentCall (Invoke 0x6571a0),At SaveExtent (Raw 0xe1a04000)
  , At NullCompare (Raw 0xe3550000),At NullBranch (Jump 0x0a000000 Spin),At ModeCall (Invoke 0x657184)
  , At RoundingLow (Raw (wide 6 0xffff)),At RoundingHigh (Raw (top 6 1))
  , At ModeCompare (Raw 0xe3500000),At ModeBranch (Jump 0x1a000000 Nonzero)
  , At OffsetLoad (Raw 0xe5950014),At LengthLoad (Raw 0xe5951018),At RoundAdd (Raw 0xe0811006)
  , At OffsetShift (Raw 0xe1a008a0),At CountShift (Raw 0xe1a018a1),At DestinationLoad (Raw 0xe5942004)
  , At FifthSet (Raw 0xe3a03001),At FifthStore (Raw 0xe58d3000)
  , At CallbackLow (Raw (wide 3 0xb924)),At CallbackHigh (Raw (top 3 0x73)),At ReadCall (Invoke 0x6a8f1c)
  , At SaveStatus (Raw 0xe1a04000),At SkipDry (Jump 0xea000000 Final)
  , Mark Nonzero,At DryLengthLoad (Raw 0xe5951018),At DryRoundAdd (Raw 0xe0816006),At DryCountShift (Raw 0xe1a068a6)
  , At FirstResource (Raw 0xe3a00024),At FirstTimeout (Raw 0xe3e01000),At FirstTake (Invoke 0x724b68)
  , At SecondResource (Raw 0xe3a00034),At SecondTimeout (Raw 0xe3e01000),At SecondTake (Invoke 0x724b68)
  , At IndexZero (Raw 0xe3a04000),Mark Loop,At LoopCompare (Raw 0xe1540006),At LoopExit (Jump 0x0a000000 Release)
  , At DelayArg (Raw 0xe3a0000e),At DelayCall (Invoke 0x69b9c4),At Increment (Raw 0xe2844001)
  , At ProgressScale (Raw 0xe1a00884),At ProgressCall (Invoke 0x73b924),At LoopBack (Jump 0xea000000 Loop)
  , Mark Release,At FirstReleaseArg (Raw 0xe3a00034),At FirstRelease (Invoke 0x724c00)
  , At SecondReleaseArg (Raw 0xe3a00024),At SecondRelease (Invoke 0x724c00),At DryStatus (Raw 0xe3a04000)
  , Mark Final,At FinalLoad (Raw 0xe5950018),At FinalProbe (Raw 0xe320f000),At FinalCall (Invoke 0x73b924)
  , At StatusMove (Raw 0xe1a00004),Mark BodyReturn,At ReturnBody (Raw 0xe8bd807c)
  , Mark Spin,At SpinBranch (Jump 0xea000000 Spin),Mark Padding,At PaddingWord (Raw 0xffffffff)
  ]

changes :: Fault -> [(Site,Op)]
changes fault = case fault of
  Control -> []
  EquivalentImmediate -> [(LookupArg,Raw (wide 0 6))]
  UnreachableNop -> [(PaddingWord,Raw 0xe320f000)]
  WrongLookupId -> [(LookupArg,Raw 0xe3a00005)]
  MissingExtent -> [(ExtentCall,Raw 0xe320f000)]
  NullReturns -> [(NullBranch,Jump 0x0a000000 BodyReturn)]
  NullCallsMode -> [(NullBranch,Raw 0xe320f000)]
  WrongModeComparison -> [(ModeCompare,Raw 0xe3500001)]
  WrongReadTarget -> [(ReadCall,Invoke 0x6a8f20)]
  WrongReadOffset -> [(OffsetShift,Raw 0xe1a00820)]
  HardcodedOffset -> [(OffsetShift,Raw 0xe3a0002d)]
  HardcodedCount -> [(CountShift,Raw 0xe3a0101b)]
  HardcodedDestination -> [(PaddingWord,Raw 0x1100000)]
  WrongDestinationField -> [(DestinationLoad,Raw 0xe5942008)]
  DroppedOffsetRead -> [(OffsetLoad,Raw 0xe3a0002d)]
  DroppedLengthRead -> [(LengthLoad,Raw 0xe3a0101b)]
  WrongFifthArgument -> [(FifthSet,Raw 0xe3a03000)]
  FifthBelowSp -> [(FifthStore,Raw 0xe50d3004)]
  WrongCallback -> [(CallbackLow,Raw (wide 3 0xb928))]
  WrongFirstResource -> [(FirstResource,Raw 0xe3a00018)]
  WrongSecondResource -> [(SecondResource,Raw 0xe3a00022)]
  WrongTimeout -> [(SecondTimeout,Raw 0xe3a01000)]
  ReversedRelease -> [(FirstReleaseArg,Raw 0xe3a00024)]
  WrongDelay -> [(DelayArg,Raw 0xe3a0000d)]
  MissingDelay -> [(DelayCall,Raw 0xe320f000)]
  MissingProgress -> [(ProgressCall,Raw 0xe320f000)]
  WrongProgressScale -> [(ProgressScale,Raw 0xe1a00804)]
  WrongRounding -> [(RoundingLow,Raw (wide 6 0xfffe))]
  NonwrappingCount -> [(DryRoundAdd,Raw 0xe0916006)]
  CachedFinalLength -> [(BodyPush,Raw 0xe92d40f4),(ReturnBody,Raw 0xe8bd80f4),(FinalLoad,Raw 0xe1a00007)]
  WrongFinalValue -> [(FinalProbe,Raw 0xe3a00000)]
  HardcodedFinalLength -> [(FinalProbe,Literal 0 Padding),(PaddingWord,Raw 0x360000)]
  DroppedFinalProgress -> [(FinalCall,Raw 0xe320f000)]
  WrongErrorStatus -> [(StatusMove,Raw 0xe3a00000)]
  LostReadStatus -> [(SaveStatus,Raw 0xe320f000)]
  LeakedIgnoredStatus -> [(DryStatus,Raw 0xe1a04000)]
  WrongRestore -> [(ReturnBody,Raw 0xe8bd80bc)]
  WrongLr -> [(AdapterLow,Raw (wide 14 0x9930))]
  BelowSpStore -> [(FinalProbe,Raw 0xe50d0004)]
  BelowSpLoad -> [(FinalProbe,Raw 0xe51dc004)]
  UninitializedLoad -> [(FinalLoad,Raw 0xe24dd008),(FinalProbe,Raw 0xe59dc000)]
  MisalignedCall -> [(FinalProbe,Raw 0xe24dd004)]
  StackOutside -> [(FinalProbe,Raw 0xe24dda02)]
  UnprovenSp -> [(FinalProbe,Raw 0xe1a0d000)]
  DeallocatedLoad -> [(FinalLoad,Raw 0xe28dd008),(FinalProbe,Raw 0xe51dc008)]
  OutOfRangeLoad -> [(FinalProbe,Raw 0xe59fcfff)]
  OutOfRangeControl -> [(FinalProbe,Raw 0xea000100)]
  VolatileRegister -> [(FinalProbe,Raw 0xe1a0000c)]
  VolatileFlags -> [(FinalLoad,Raw 0x15950018)]
  ZeroLsr -> [(OffsetShift,Raw 0xe1a00020)]
  RegisterShift -> [(OffsetShift,Raw 0xe1a00630)]
  YieldInstruction -> [(FinalProbe,Raw 0xe320f001)]
  StoreOverlapFault -> [(FinalProbe,Raw 0xe5add004)]
  LoadOverlapFault -> [(FinalProbe,Raw 0xe5bdd004)]
  PushSp -> [(FinalProbe,Raw 0xe92d2000)]
  PopSp -> [(FinalProbe,Raw 0xe8bd2000)]

expectedRefusal :: Fault -> Maybe AdmissionError
expectedRefusal fault = case fault of
  Control -> Nothing
  EquivalentImmediate -> Nothing
  UnreachableNop -> Nothing
  WrongLookupId -> Just CallArgumentsDiffer
  MissingExtent -> Just OrderedEffectsDiffer
  NullReturns -> Just OutcomeDiffer
  NullCallsMode -> Just OrderedEffectsDiffer
  WrongModeComparison -> Just OrderedEffectsDiffer
  WrongReadTarget -> Just ExternalCallRejected
  WrongReadOffset -> Just CallArgumentsDiffer
  HardcodedOffset -> Just CallArgumentsDiffer
  HardcodedCount -> Just CallArgumentsDiffer
  HardcodedDestination -> Just CallArgumentsDiffer
  WrongDestinationField -> Just ReadProvenanceRejected
  DroppedOffsetRead -> Just OrderedEffectsDiffer
  DroppedLengthRead -> Just OrderedEffectsDiffer
  WrongFifthArgument -> Just FifthArgumentDiffer
  FifthBelowSp -> Just UnallocatedStackAccess
  WrongCallback -> Just CallArgumentsDiffer
  WrongFirstResource -> Just CallArgumentsDiffer
  WrongSecondResource -> Just CallArgumentsDiffer
  WrongTimeout -> Just CallArgumentsDiffer
  ReversedRelease -> Just CallArgumentsDiffer
  WrongDelay -> Just CallArgumentsDiffer
  MissingDelay -> Just OrderedEffectsDiffer
  MissingProgress -> Just OrderedEffectsDiffer
  WrongProgressScale -> Just CallArgumentsDiffer
  WrongRounding -> Just CallArgumentsDiffer
  NonwrappingCount -> Just OrderedEffectsDiffer
  CachedFinalLength -> Just OrderedEffectsDiffer
  WrongFinalValue -> Just CallArgumentsDiffer
  HardcodedFinalLength -> Just CallArgumentsDiffer
  DroppedFinalProgress -> Just OrderedEffectsDiffer
  WrongErrorStatus -> Just ReturnStatusDiffer
  LostReadStatus -> Just ReturnStatusDiffer
  LeakedIgnoredStatus -> Just ReturnStatusDiffer
  WrongRestore -> Just PreservedRegisterDiffers
  WrongLr -> Just ReturnLrDiffers
  BelowSpStore -> Just UnallocatedStackAccess
  BelowSpLoad -> Just UnallocatedStackAccess
  UninitializedLoad -> Just UninitializedStackRead
  MisalignedCall -> Just CallStackUnaligned
  StackOutside -> Just StackOutOfRange
  UnprovenSp -> Just StackProvenanceLost
  DeallocatedLoad -> Just UnallocatedStackAccess
  OutOfRangeLoad -> Just ReadProvenanceRejected
  OutOfRangeControl -> Just ExecutionOutOfRange
  VolatileRegister -> Just CallArgumentsDiffer
  VolatileFlags -> Just UnprovedCondition
  ZeroLsr -> Just UnsupportedShift
  RegisterShift -> Just UnsupportedShift
  YieldInstruction -> Just UnsupportedInstruction
  StoreOverlapFault -> Just StoreOverlap
  LoadOverlapFault -> Just LoadOverlap
  PushSp -> Just PushRegistersRejected
  PopSp -> Just PopRegistersRejected
