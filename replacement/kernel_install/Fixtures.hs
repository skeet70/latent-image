module Fixtures (Fault (..), fixture, expectedRefusal) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Replacement.KernelInstall.Admission

-- Project-owned A32 fixture from the initial 336-byte kernel_install artifact.
data Fault = Control | EquivalentImmediate | UnreachableNop | WrongHeader | WrongRootDestination
  | MissingRoot | WrongCacheTarget | WrongCacheSize | MissingCache | WrongMarkerComparison
  | ReversedMarker | ReturningRefusal | WrongCursor | ExtraZeroWord | WrongDestination
  | BelowSpStore | BelowSpLoad | MisalignedCall | WrongRestore | WrongReturnLr | YieldInstruction
  | ZeroLsrAdd | ZeroLsrMove
  deriving (Eq, Show, Enum, Bounded)

fixture :: Fault -> (B.ByteString,B.ByteString)
fixture fault =
  let changes :: [(Int,Word32)]
      changes = case fault of
        Control -> []
        EquivalentImmediate -> [(0xac,0xe30010fa)]
        UnreachableNop -> [(0x10c,0xe320f000)]
        WrongHeader -> [(0x60,0xe5901014)]
        WrongRootDestination -> [(0x78,0xe5802008)]
        MissingRoot -> [(0x68,0xe320f000)]
        WrongCacheTarget -> [(0x7c,0xebe472ec)]
        WrongCacheSize -> [(0x74,0xe3a01040)]
        MissingCache -> [(0x7c,0xe320f000)]
        WrongMarkerComparison -> [(0x88,0xe3510000)]
        ReversedMarker -> [(0x8c,0x0a00001d)]
        ReturningRefusal -> [(0x108,0xe8bd8830)]
        WrongCursor -> [(0xcc,0xe3a01078)]
        ExtraZeroWord -> [(0x120,0xe30012d1)]
        WrongDestination -> [(0x14c,0x409c3604)]
        BelowSpStore -> [(0x40,0xe50d2004)]
        BelowSpLoad -> [(0x60,0xe51d1004)]
        MisalignedCall -> [(4,0xe92d1030)]
        WrongRestore -> [(0x30,0xe8bd8400)]
        WrongReturnLr -> [(0x28,0xe305ea44)]
        YieldInstruction -> [(0x18,0xe320f001)]
        ZeroLsrAdd -> [(0xf0,0xe0803025)]
        ZeroLsrMove -> [(0x18,0xe1a03021)]
      bytes = B.concat [U.putWord32 (maybe word id (lookup offset changes)) | (offset,word) <- zip [0,4..] program]
      symbols = C.pack "00725a04 00000000 T __kernel_install_start\n00725b54 00000000 T __kernel_install_end\n00725a04 00000034 T xm1_kernel_install_entry\n00725a58 000000fc T xm1_kernel_install\n"
  in (bytes,symbols)

expectedRefusal :: Fault -> Maybe AdmissionError
expectedRefusal fault = case fault of
  Control -> Nothing
  EquivalentImmediate -> Nothing
  UnreachableNop -> Nothing
  WrongHeader -> Just ReadProvenanceRejected
  WrongRootDestination -> Just WriteProvenanceRejected
  MissingRoot -> Just OrderedEffectsDiffer
  WrongCacheTarget -> Just ExternalCallRejected
  WrongCacheSize -> Just CacheArgumentsDiffer
  MissingCache -> Just OrderedEffectsDiffer
  WrongMarkerComparison -> Just MarkerComparisonRejected
  ReversedMarker -> Just MarkerOutcomeDiffer
  ReturningRefusal -> Just MarkerOutcomeDiffer
  WrongCursor -> Just OrderedEffectsDiffer
  ExtraZeroWord -> Just OrderedEffectsDiffer
  WrongDestination -> Just OrderedEffectsDiffer
  BelowSpStore -> Just UnallocatedStackAccess
  BelowSpLoad -> Just UnallocatedStackAccess
  MisalignedCall -> Just CallStackUnaligned
  WrongRestore -> Just PreservedRegisterDiffers
  WrongReturnLr -> Just ReturnFlagsOrLrDiffer
  YieldInstruction -> Just UnsupportedInstruction
  ZeroLsrAdd -> Just UnsupportedShift
  ZeroLsrMove -> Just UnsupportedShift

program :: [Word32]
program =
  [ 0xe92d4800,0xe92d1010,0xeb000011,0xe8bd1010
  , 0xe30f1c2c,0xe34410a0,0xe1a03001,0xe3a02000
  , 0xe3a00000,0xe3500000,0xe305ea40,0xe340e072
  , 0xe8bd8800,0xe92d4800,0xe1a0b00d,0xe3a02000
  , 0xe5802000,0xe2800004,0xe2511001,0x1afffffb
  , 0xe8bd8800,0xe92d4830,0xe28db008,0xe3a00701
  , 0xe5901010,0xe59f00cc,0xe5801000,0xe3a0180d
  , 0xe5912010,0xe3a01020,0xe5802004,0xebe472eb
  , 0xe59f00b4,0xe5901000,0xe3510003,0x1a00001d
  , 0xe59f40a8,0xe3a01078,0xe3a05078,0xe1a00004
  , 0xebffffe3,0xe59f0098,0xe5805000,0xe3a010fa
  , 0xe5805004,0xe3a02000,0xe50013c4,0xe3a01001
  , 0xe5805008,0xe59f5080,0xe580100c,0xe3a0107a
  , 0xe5801010,0xe3070531,0xe59f1068,0xe5810000
  , 0xe3a00004,0xe3a01c01,0xe3510000,0x0a000007
  , 0xe0803005,0xe5832000,0xe2411004,0xe5832200
  , 0xe2800004,0xeafffff7,0xeafffffe,0xeafffffd
  , 0xe2840ff5,0xe3a01f5a,0xebffffc5,0xe2850fc1
  , 0xe3a01e2d,0xebffffc2,0xe2840e1e,0xe3a01078
  , 0xe8bd4830,0xeaffffbe,0x0096bd20,0x007caf3c
  , 0x40a0f870,0x40a0fc30,0x409c3500,0x409c3600
  ]
