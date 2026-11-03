module Fixtures (Fault (..), fixture, expectedRefusal) where

import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.Image5Load.Admission (AdmissionError (..))

-- Project-owned complete A32 programs assembled independently of compiler output.
data Fault = Control | EquivalentImmediate | UnreachableNop | EquivalentLr | StaleContinuationLr
  | WrongLookupId | NullReturns | NullReadsFields | WrongMainStart | MissingMainStart
  | HardcodedLength | WrongLengthField | HardcodedOffset | WrongBlockShift | WrongRounding
  | WrongBaseMask | IgnoredLengthAlignment | LengthReadForMisalignedBase
  | MissingFlagStore | WrongFlagValue | WrongFlagStoreAddress | WrongFillShift | WrongFillCallback | WrongFillTarget
  | InvertedAcceptance | AcceptAnyResult | RequestedWithoutFill
  | WrongModeComparison | SkipTakesReadPath | WrongLoadStartMarker | DroppedDestinationRead | WrongDestinationField
  | WrongStaging | MissingDecoderInit | WrongFeed | SkipFeedNonNull | WrongFifthArgument | FifthBelowSp
  | WrongReadBlock | WrongStorageTarget | LostReadStatus | LostSkipStatus | MissingDecodeStart
  | MissingDecoderFinish | WrongDecodeEnd
  | MissingWait | SingleFlagRead | InvertedWait | WrongWaitAddress | WaitSkipsReload | StaleRefillBase
  | MissingCpuFill | DroppedRefillLength | WrongFillByte | MissingCache | WrongCacheArgument
  | MissingMainEnd | ExtraRetainedCall | UnknownExternalCall | WrongErrorStatus
  | WrongRestore | WrongLr | PcRelativeFlagWrite
  | BelowSpStore | BelowSpLoad | MisalignedCall | StackOutside | UnprovenSp | OutOfRangeLoad | OutOfRangeControl
  | VolatileRegister | VolatileFlags | ZeroLsr | RegisterShift | YieldInstruction
  | StoreOverlapFault | LoadOverlapFault | PushSp | PopSp | CarrierOverflow
  deriving (Eq, Show, Enum, Bounded)

data Label = Body | Policy | Skip | Complete | Wait | WaitTest | Fill | Final | BodyReturn | Spin | Padding
  deriving (Eq, Ord, Show)
data Site = AdapterPush | AdapterCall | AdapterLow | AdapterHigh | AdapterReturn | BodyPush
  | LookupArg | LookupCall | SaveSegment | NullCompare | NullBranch | OffsetLoad | LengthLoad
  | MarkerLow | MarkerHigh | MainStartArg | MainStartCall | PolicyInit
  | BaseLoad | BaseTest | BaseBranch | LengthCheckLoad | LengthTest | LengthBranch
  | FlagLow | FlagHigh | FlagValue | FlagStoreSite | WordShift | FillCallbackLow | FillCallbackHigh | FillCall
  | SavePolicy | BlockShift | RoundingLow | RoundingHigh | RoundAdd | CountShift | FifthSet | FifthStore
  | ModeCall | StagingLow | StagingHigh | ModeCompare | ModeBranch | LoadStartArg | LoadStartCall
  | DestinationLoad | InitStaging | InitCall | FeedLow | FeedHigh | ReadBlock | ReadCount | ReadStaging
  | ReadCall | SaveStatus | LoadEndArg | LoadEndCall | DecodeStartArg | DecodeStartCall | FinishCall
  | DecodeEndArg | DecodeEndCall | SkipJoin | SkipBlock | SkipCount | SkipStaging | SkipFeed | SkipCall
  | SkipStatus | PolicyCompare | PolicyBranch | WaitLow | WaitHigh | WaitLoad | WaitCompare | WaitBack
  | WaitExit | FillBaseLoad | FillByte | FillLengthLoad | CpuFillCall | CacheStart | CacheLength | CacheCall
  | MainEndArg | FinalProbe | MainEndCall | StatusMove | ReturnBody | SpinBranch | PaddingWord | InjectedValue
  deriving (Eq, Ord, Show)
data Op = Raw Word32 | Jump Word32 Label | Invoke Word32
data Item = Mark Label | At Site Op

origin :: Word32
origin = 0x6a96c0

fixture :: Fault -> B.ByteString
fixture fault = B.concat (map U.putWord32 (reverse encoded)) <> overflow
  where
    overflow = if fault == CarrierOverflow then B.replicate 20 0 else B.empty
    items = concatMap alter program
    alter (At AdapterHigh _) | fault `elem` [StaleContinuationLr,PcRelativeFlagWrite] = []
    alter item@(At BaseLoad _) | fault == StaleRefillBase = [item,At InjectedValue (Raw 0xe1a0b000)]
    alter item@(At FillBaseLoad _) | fault == StaleRefillBase = [item,At InjectedValue (Raw 0xe1a0000b)]
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
      in (pc+4,word:wordsSoFar)

wide :: Word32 -> Word32 -> Word32
wide register word = 0xe3000000 .|. (register `shiftL` 12) .|. ((word .&. 0xf000) `shiftL` 4) .|. (word .&. 0xfff)
top :: Word32 -> Word32 -> Word32
top register word = 0xe3400000 .|. (register `shiftL` 12) .|. ((word .&. 0xf000) `shiftL` 4) .|. (word .&. 0xfff)
branch :: Word32 -> Word32 -> Word32 -> Word32
branch opcode site target = opcode .|. (fromInteger ((toInteger target-toInteger site-8) `div` 4) .&. 0xffffff)

nop :: Op
nop = Raw 0xe320f000

program :: [Item]
program =
  [ At AdapterPush (Raw 0xe92d4800),At AdapterCall (Jump 0xeb000000 Body)
  , At AdapterLow (Raw (wide 14 0x22b8)),At AdapterHigh (Raw (top 14 4)),At AdapterReturn (Raw 0xe8bd8800)
  , Mark Body,At BodyPush (Raw 0xe92d43f8),At LookupArg (Raw 0xe3a00005),At LookupCall (Invoke 0x657114)
  , At SaveSegment (Raw 0xe1a05000),At NullCompare (Raw 0xe3550000),At NullBranch (Jump 0x0a000000 Spin)
  , At OffsetLoad (Raw 0xe5957014),At LengthLoad (Raw 0xe5954018)
  , At MarkerLow (Raw (wide 9 0x19a0)),At MarkerHigh (Raw (top 9 0x78))
  , At MainStartArg (Raw 0xe1a00009),At MainStartCall (Invoke 0x42188),At PolicyInit (Raw 0xe3a06001)
  , At BaseLoad (Raw 0xe595000c),At BaseTest (Raw 0xe3100003),At BaseBranch (Jump 0x1a000000 Policy)
  , At LengthCheckLoad (Raw 0xe5951010),At LengthTest (Raw 0xe3110003),At LengthBranch (Jump 0x1a000000 Policy)
  , At FlagLow (Raw (wide 3 0xf8ac)),At FlagHigh (Raw (top 3 0x89)),At FlagValue (Raw 0xe3a02000)
  , At FlagStoreSite (Raw 0xe5832000),At WordShift (Raw 0xe1a01121)
  , At FillCallbackLow (Raw (wide 3 0x8bec)),At FillCallbackHigh (Raw (top 3 0x6a)),At FillCall (Invoke 0x66609c)
  , At SavePolicy (Raw 0xe1a06000)
  , Mark Policy,At BlockShift (Raw 0xe1a078a7),At RoundingLow (Raw (wide 8 0xffff)),At RoundingHigh (Raw (top 8 1))
  , At RoundAdd (Raw 0xe0848008),At CountShift (Raw 0xe1a088a8),At FifthSet (Raw 0xe3a03001)
  , At FifthStore (Raw 0xe58d3000),At ModeCall (Invoke 0x657184)
  , At StagingLow (Raw (wide 4 0)),At StagingHigh (Raw (top 4 0x43e4))
  , At ModeCompare (Raw 0xe3500000),At ModeBranch (Jump 0x1a000000 Skip)
  , At LoadStartArg (Raw 0xe2890008),At LoadStartCall (Invoke 0x42188)
  , At DestinationLoad (Raw 0xe5950004),At InitStaging (Raw 0xe1a01004),At InitCall (Invoke 0x6ae360)
  , At FeedLow (Raw (wide 3 0xe500)),At FeedHigh (Raw (top 3 0x6a))
  , At ReadBlock (Raw 0xe1a00007),At ReadCount (Raw 0xe1a01008),At ReadStaging (Raw 0xe1a02004)
  , At ReadCall (Invoke 0x6a8f1c),At SaveStatus (Raw 0xe1a04000)
  , At LoadEndArg (Raw 0xe2890010),At LoadEndCall (Invoke 0x42188)
  , At DecodeStartArg (Raw 0xe2890018),At DecodeStartCall (Invoke 0x42188),At FinishCall (Invoke 0x6ae3f4)
  , At DecodeEndArg (Raw 0xe2890020),At DecodeEndCall (Invoke 0x42188),At SkipJoin (Jump 0xea000000 Complete)
  , Mark Skip,At SkipBlock (Raw 0xe1a00007),At SkipCount (Raw 0xe1a01008),At SkipStaging (Raw 0xe1a02004)
  , At SkipFeed (Raw 0xe3a03000),At SkipCall (Invoke 0x6a8f1c),At SkipStatus (Raw 0xe1a04000)
  , Mark Complete,At PolicyCompare (Raw 0xe3560000),At PolicyBranch (Jump 0x1a000000 Fill)
  , At WaitLow (Raw (wide 3 0xf8ac)),At WaitHigh (Raw (top 3 0x89))
  , Mark Wait,At WaitLoad (Raw 0xe5932000),Mark WaitTest,At WaitCompare (Raw 0xe3520000)
  , At WaitBack (Jump 0x0a000000 Wait)
  , At WaitExit (Jump 0xea000000 Final)
  , Mark Fill,At FillBaseLoad (Raw 0xe595000c),At FillByte (Raw 0xe3a01000),At FillLengthLoad (Raw 0xe5952010)
  , At CpuFillCall (Invoke 0x7221c8),At CacheStart (Raw 0xe3a00000),At CacheLength (Raw 0xe3a01000)
  , At CacheCall (Invoke 0x42634)
  , Mark Final,At MainEndArg (Raw 0xe2890028),At FinalProbe nop,At MainEndCall (Invoke 0x42188)
  , At StatusMove (Raw 0xe1a00004),Mark BodyReturn,At ReturnBody (Raw 0xe8bd83f8)
  , Mark Spin,At SpinBranch (Jump 0xea000000 Spin),Mark Padding,At PaddingWord (Raw 0xffffffff)
  ]

changes :: Fault -> [(Site,Op)]
changes fault = case fault of
  Control -> []
  EquivalentImmediate -> [(LookupArg,Raw (wide 0 5))]
  UnreachableNop -> [(PaddingWord,nop)]
  EquivalentLr -> [(AdapterLow,Raw 0xe3a0ea42),(AdapterHigh,Raw 0xe38eefae)]
  StaleContinuationLr -> [(AdapterLow,Raw 0xe28fef5a)]
  WrongLookupId -> [(LookupArg,Raw 0xe3a00006)]
  NullReturns -> [(NullBranch,Jump 0x0a000000 BodyReturn)]
  NullReadsFields -> [(NullBranch,nop)]
  WrongMainStart -> [(MarkerLow,Raw (wide 9 0x19a8))]
  MissingMainStart -> [(MainStartCall,nop)]
  HardcodedLength -> [(LengthLoad,Raw 0xe3a0482e)]
  WrongLengthField -> [(LengthLoad,Raw 0xe595401c)]
  HardcodedOffset -> [(BlockShift,Raw 0xe3a07016)]
  WrongBlockShift -> [(BlockShift,Raw 0xe1a07827)]
  WrongRounding -> [(RoundingLow,Raw (wide 8 0xfffe))]
  WrongBaseMask -> [(BaseTest,Raw 0xe3100001)]
  IgnoredLengthAlignment -> [(LengthTest,Raw 0xe3110000)]
  LengthReadForMisalignedBase -> [(BaseBranch,nop)]
  MissingFlagStore -> [(FlagStoreSite,nop)]
  WrongFlagValue -> [(FlagValue,Raw 0xe3a02001)]
  WrongFlagStoreAddress -> [(FlagLow,Raw (wide 3 0xf8b0))]
  WrongFillShift -> [(WordShift,Raw 0xe1a011a1)]
  WrongFillCallback -> [(FillCallbackLow,Raw (wide 3 0x8bf0))]
  WrongFillTarget -> [(FillCall,Invoke 0x6660a0)]
  InvertedAcceptance -> [(PolicyBranch,Jump 0x0a000000 Fill)]
  AcceptAnyResult -> [(SavePolicy,Raw 0xe3a06000)]
  RequestedWithoutFill -> [(PolicyInit,Raw 0xe3a06000)]
  WrongModeComparison -> [(ModeCompare,Raw 0xe3500001)]
  SkipTakesReadPath -> [(ModeBranch,nop)]
  WrongLoadStartMarker -> [(LoadStartArg,Raw 0xe2890000)]
  DroppedDestinationRead -> [(DestinationLoad,Raw 0xe3a00000)]
  WrongDestinationField -> [(DestinationLoad,Raw 0xe5950008)]
  WrongStaging -> [(StagingHigh,Raw (top 4 0x43e5))]
  MissingDecoderInit -> [(InitCall,nop)]
  WrongFeed -> [(FeedLow,Raw (wide 3 0xe504))]
  SkipFeedNonNull -> [(SkipFeed,Raw 0xe3a03001)]
  WrongFifthArgument -> [(FifthSet,Raw 0xe3a03000)]
  FifthBelowSp -> [(FifthStore,Raw 0xe50d3004)]
  WrongReadBlock -> [(ReadBlock,Raw 0xe1a00008)]
  WrongStorageTarget -> [(ReadCall,Invoke 0x6a8f20)]
  LostReadStatus -> [(SaveStatus,nop)]
  LostSkipStatus -> [(SkipStatus,nop)]
  MissingDecodeStart -> [(DecodeStartCall,nop)]
  MissingDecoderFinish -> [(FinishCall,nop)]
  WrongDecodeEnd -> [(DecodeEndArg,Raw 0xe2890028)]
  MissingWait -> [(WaitLow,Jump 0xea000000 Final)]
  SingleFlagRead -> [(WaitBack,nop)]
  InvertedWait -> [(WaitBack,Jump 0x1a000000 Wait)]
  WrongWaitAddress -> [(WaitLow,Raw (wide 3 0xf8b0))]
  WaitSkipsReload -> [(WaitBack,Jump 0x0a000000 WaitTest)]
  StaleRefillBase -> [(FillBaseLoad,Raw 0xe595c00c)]
  MissingCpuFill -> [(CpuFillCall,nop)]
  DroppedRefillLength -> [(FillLengthLoad,Raw 0xe3a02000)]
  WrongFillByte -> [(FillByte,Raw 0xe3a010ff)]
  MissingCache -> [(CacheCall,nop)]
  WrongCacheArgument -> [(CacheLength,Raw 0xe3a01001)]
  MissingMainEnd -> [(MainEndCall,nop)]
  ExtraRetainedCall -> [(FinalProbe,Invoke 0x657184)]
  UnknownExternalCall -> [(FinalProbe,Invoke 0x700000)]
  WrongErrorStatus -> [(StatusMove,Raw 0xe3a00000)]
  WrongRestore -> [(ReturnBody,Raw 0xe8bd86f8)]
  WrongLr -> [(AdapterLow,Raw (wide 14 0x22bc))]
  PcRelativeFlagWrite -> [(AdapterLow,Raw 0xe29fef5a)]
  BelowSpStore -> [(FinalProbe,Raw 0xe50d0004)]
  BelowSpLoad -> [(FinalProbe,Raw 0xe51dc004)]
  MisalignedCall -> [(FinalProbe,Raw 0xe24dd004)]
  StackOutside -> [(FinalProbe,Raw 0xe24dda02)]
  UnprovenSp -> [(FinalProbe,Raw 0xe1a0d000)]
  OutOfRangeLoad -> [(FinalProbe,Raw 0xe59fcfff)]
  OutOfRangeControl -> [(FinalProbe,Raw 0xea000100)]
  VolatileRegister -> [(FinalProbe,Raw 0xe1a0000c)]
  VolatileFlags -> [(FinalProbe,Raw 0x01a01001)]
  ZeroLsr -> [(BlockShift,Raw 0xe1a07027)]
  RegisterShift -> [(BlockShift,Raw 0xe1a07637)]
  YieldInstruction -> [(FinalProbe,Raw 0xe320f001)]
  StoreOverlapFault -> [(FinalProbe,Raw 0xe5add004)]
  LoadOverlapFault -> [(FinalProbe,Raw 0xe5bdd004)]
  PushSp -> [(FinalProbe,Raw 0xe92d2000)]
  PopSp -> [(FinalProbe,Raw 0xe8bd2000)]
  CarrierOverflow -> []

expectedRefusal :: Fault -> Maybe AdmissionError
expectedRefusal fault = case fault of
  Control -> Nothing
  EquivalentImmediate -> Nothing
  UnreachableNop -> Nothing
  EquivalentLr -> Nothing
  StaleContinuationLr -> Just ReturnLrDiffers
  WrongLookupId -> Just CallArgumentsDiffer
  NullReturns -> Just OutcomeDiffer
  NullReadsFields -> Just ReadProvenanceRejected
  WrongMainStart -> Just CallArgumentsDiffer
  MissingMainStart -> Just OrderedEffectsDiffer
  HardcodedLength -> Just OrderedEffectsDiffer
  WrongLengthField -> Just ReadProvenanceRejected
  HardcodedOffset -> Just CallArgumentsDiffer
  WrongBlockShift -> Just CallArgumentsDiffer
  WrongRounding -> Just CallArgumentsDiffer
  WrongBaseMask -> Just OrderedEffectsDiffer
  IgnoredLengthAlignment -> Just OrderedEffectsDiffer
  LengthReadForMisalignedBase -> Just OrderedEffectsDiffer
  MissingFlagStore -> Just OrderedEffectsDiffer
  WrongFlagValue -> Just FlagStoreDiffers
  WrongFlagStoreAddress -> Just WriteProvenanceRejected
  WrongFillShift -> Just CallArgumentsDiffer
  WrongFillCallback -> Just CallArgumentsDiffer
  WrongFillTarget -> Just ExternalCallRejected
  InvertedAcceptance -> Just OrderedEffectsDiffer
  AcceptAnyResult -> Just OrderedEffectsDiffer
  RequestedWithoutFill -> Just OrderedEffectsDiffer
  WrongModeComparison -> Just OrderedEffectsDiffer
  SkipTakesReadPath -> Just OrderedEffectsDiffer
  WrongLoadStartMarker -> Just CallArgumentsDiffer
  DroppedDestinationRead -> Just OrderedEffectsDiffer
  WrongDestinationField -> Just ReadProvenanceRejected
  WrongStaging -> Just CallArgumentsDiffer
  MissingDecoderInit -> Just OrderedEffectsDiffer
  WrongFeed -> Just CallArgumentsDiffer
  SkipFeedNonNull -> Just CallArgumentsDiffer
  WrongFifthArgument -> Just FifthArgumentDiffer
  FifthBelowSp -> Just UnallocatedStackAccess
  WrongReadBlock -> Just CallArgumentsDiffer
  WrongStorageTarget -> Just ExternalCallRejected
  LostReadStatus -> Just ReturnStatusDiffer
  LostSkipStatus -> Just ReturnStatusDiffer
  MissingDecodeStart -> Just OrderedEffectsDiffer
  MissingDecoderFinish -> Just OrderedEffectsDiffer
  WrongDecodeEnd -> Just CallArgumentsDiffer
  MissingWait -> Just OrderedEffectsDiffer
  SingleFlagRead -> Just OrderedEffectsDiffer
  InvertedWait -> Just OrderedEffectsDiffer
  WrongWaitAddress -> Just ReadProvenanceRejected
  WaitSkipsReload -> Just ExecutionBoundExceeded
  StaleRefillBase -> Just CallArgumentsDiffer
  MissingCpuFill -> Just OrderedEffectsDiffer
  DroppedRefillLength -> Just OrderedEffectsDiffer
  WrongFillByte -> Just CallArgumentsDiffer
  MissingCache -> Just OrderedEffectsDiffer
  WrongCacheArgument -> Just CallArgumentsDiffer
  MissingMainEnd -> Just OrderedEffectsDiffer
  ExtraRetainedCall -> Just OrderedEffectsDiffer
  UnknownExternalCall -> Just ExternalCallRejected
  WrongErrorStatus -> Just ReturnStatusDiffer
  WrongRestore -> Just PreservedRegisterDiffers
  WrongLr -> Just ReturnLrDiffers
  PcRelativeFlagWrite -> Just UnprovedTransfer
  BelowSpStore -> Just UnallocatedStackAccess
  BelowSpLoad -> Just UnallocatedStackAccess
  MisalignedCall -> Just CallStackUnaligned
  StackOutside -> Just StackOutOfRange
  UnprovenSp -> Just StackProvenanceLost
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
  CarrierOverflow -> Just InvalidCarrier
