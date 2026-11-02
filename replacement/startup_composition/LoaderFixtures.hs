module LoaderFixtures (InstallStack(..), dispatcherFixture, replaceLoaderWord, branchWord) where

import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Fixtures as Dispatch
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.LoaderOrchestration.Interface as L
import qualified Replacement.StartupComposition.Artifact as C

data InstallStack = Baseline | AlignedExtraFrame | MisalignedEntry deriving (Eq, Show)

dispatcherFixture :: InstallStack -> Either String C.NativeFiles
dispatcherFixture layout =
  let (bytes,symbols) = Dispatch.carrier Dispatch.Control
      marker = U.putWord32 0xe320f000 <> U.putWord32 0xe3500000
      (prefix,suffix) = B.breakSubstring marker bytes
      allocation = case layout of Baseline -> 0; AlignedExtraFrame -> 16; MisalignedEntry -> 4
      changed = if allocation==0 then bytes else prefix<>U.putWord32 (0xe24dd000 .|. allocation)<>B.drop 4 suffix
  in if B.null suffix then Left "original synthetic dispatcher stack marker absent"
     else Right (C.NativeFiles changed symbols)

replaceLoaderWord :: L.LoaderSide -> C.NativeFiles -> L.CodeAddress -> Word32 -> C.NativeFiles
replaceLoaderWord side native (L.CodeAddress pc) value =
  let L.CodeAddress start = L.carrierStart side
      offset = fromIntegral (pc-start)
      bytes = C.nativeBytes native
  in native {C.nativeBytes=B.take offset bytes<>U.putWord32 value<>B.drop (offset+4) bytes}

branchWord :: L.CodeAddress -> L.CodeAddress -> Word32
branchWord (L.CodeAddress pc) (L.CodeAddress target) =
  0xeb000000 .|. (fromInteger ((toInteger target-toInteger pc-8) `div` 4) .&. 0xffffff)
