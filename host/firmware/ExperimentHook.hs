-- | Which stock instructions an experiment hook may displace into its trampoline.
module ExperimentHook
  ( positionIndependent, HookMode (..), HookAdmission (..), PlaybackEntry (..)
  , SourceSha256 (..), playbackContract, bindReplacement, admitHook, entryPublications, originalBody
  ) where

import A32 (Instruction (..), Offset (..), Operand (..), Reg, pc)
import Data.Word (Word32)

data HookMode = Observe | ReplaceEntry PlaybackEntry deriving (Eq, Show)
data HookAdmission = ObserveOnly | EmulatorOnly deriving (Eq, Show)
data PlaybackEntry = PlaybackSource | PlaybackMode deriving (Eq, Show)
newtype SourceSha256 = SourceSha256 String deriving (Eq, Show)

playbackContract :: PlaybackEntry -> (Word32, Word32, SourceSha256, String)
playbackContract PlaybackSource =
  (0x0120b40c, 0x0120b778, SourceSha256 "90da01fffdecd496e473c74aa012c9028be5d2dad33a1b8f4a95b4ea7c466d32", "xm1_playback_source_entry")
playbackContract PlaybackMode =
  (0x0120cab0, 0x0120cd74, SourceSha256 "d9b5d69e40838f9bcecc142fb1bfeaefa18e43b790a027e0442d7b31deef0f0c", "xm1_playback_mode_entry")

bindReplacement :: Word32 -> String -> Word32 -> Word32 -> SourceSha256 -> Either String HookMode
bindReplacement site handler entry end digest = do
  supported <- case [target | target <- [PlaybackSource, PlaybackMode], let (start, _, _, _) = playbackContract target, site == start] of
    [target] -> Right target
    _ -> Left "ReplaceEntry site is not a supported playback entry"
  let (expectedEntry, expectedEnd, expectedDigest, expectedHandler) = playbackContract supported
  if (entry, end, digest) /= (expectedEntry, expectedEnd, expectedDigest)
    then Left "ReplaceEntry source entry, extent or sha256 differs from the pinned playback contract"
    else if handler /= expectedHandler
      then Left ("ReplaceEntry handler must be " ++ expectedHandler)
      else Right (ReplaceEntry supported)

admitHook :: HookAdmission -> HookMode -> Either String ()
admitHook ObserveOnly (ReplaceEntry _) = Left "ReplaceEntry is emulator-only; the live experiment builder admits Observe hooks only"
admitHook _ _ = Right ()

entryPublications :: Word32 -> Word32 -> Word32 -> Word32 -> ((Word32, Word32, Word32), (Word32, Word32, Word32))
entryPublications site first second target =
  ((site + 4, second, target), (site, first, 0xe51ff004))

originalBody :: HookMode -> Maybe (Word32, Word32)
originalBody Observe = Nothing
originalBody (ReplaceEntry entry) = let (start, end, _, _) = playbackContract entry in Just (start + 8, end)

-- | Whether the instruction behaves the same at another address: it neither
-- branches nor names the PC.
positionIndependent :: Instruction -> Bool
positionIndependent instruction = case instruction of
  DataProcessing _ _ _ d n operand -> notPc [d, n] && operandIndependent operand
  MoveWide _ d _ -> d /= pc
  MoveTop _ d _ -> d /= pc
  ZeroExtend _ d m _ _ -> notPc [d, m]
  Multiply _ d a b accumulate -> notPc ([d, a, b] ++ maybe [] pure accumulate)
  Load _ _ _ d n offset _ -> notPc [d, n] && offsetIndependent offset
  Store _ _ t n offset _ -> notPc [t, n] && offsetIndependent offset
  LoadMultiple _ n _ _ registers -> notPc (n : registers)
  StoreMultiple _ n _ _ registers -> notPc (n : registers)
  Hint _ -> True
  _ -> False
 where
  notPc :: [Reg] -> Bool
  notPc = notElem pc
  operandIndependent operand = case operand of
    Immediate _ -> True
    ShiftedImmediate m _ _ -> m /= pc
    ShiftedRegister m _ s -> notPc [m, s]
  offsetIndependent offset = case offset of
    OffsetImmediate _ -> True
    OffsetRegister _ m _ _ -> m /= pc
