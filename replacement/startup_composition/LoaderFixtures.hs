module LoaderFixtures
  ( InstallStack(..), dispatcherFixture, replaceLoaderWord, branchWord
  , relocatedPublication, scalarCallback
  ) where

import qualified A32 as A
import Data.Bits ((.&.), (.|.), shiftL)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as Text
import Data.Word (Word32)
import qualified Fixtures as Dispatch
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.LoaderOrchestration.Interface as L
import qualified Replacement.StartupComposition.Artifact as C
import Numeric (showHex)

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

appendLoaderWords :: L.LoaderSide -> C.NativeFiles -> [Word32] -> Either String C.NativeFiles
appendLoaderWords side native wordsToAppend =
  let bytes = C.nativeBytes native<>B.concat (map U.putWord32 wordsToAppend)
      L.CodeAddress start = L.carrierStart side
      row address name = Text.pack (showHex address ""++" 00000000 T "++name++"\n")
      symbols = B.concat [row start "__loader_orchestration_start"
        ,row (start+fromIntegral (B.length bytes)) "__loader_orchestration_end"
        ,row start "xm1_loader_orchestration_entry",row (start+12) "xm1_loader_orchestration"]
  in if B.length bytes>340 then Left "synthetic loader exceeds carrier window" else Right (C.NativeFiles bytes symbols)

relocatedPublication :: L.LoaderSide -> C.NativeFiles -> L.CodeAddress -> Either String (C.NativeFiles,L.CodeAddress)
relocatedPublication side native store@(L.CodeAddress storePc) = do
  let L.CodeAddress start = L.carrierStart side
      assignment = L.CodeAddress (storePc-4)
      wordAt pc = U.word32At (C.nativeBytes native) (fromIntegral (pc-start))
      originalStore = wordAt storePc
      originalAssignment = wordAt (storePc-4)
      thunkPc = start+fromIntegral (B.length (C.nativeBytes native))
      jump from to = branchWord from to .&. 0xfeffffff
  register <- case (A.decode storePc originalStore,A.decode (storePc-4) originalAssignment) of
    (A.Store _ A.Word source _ _ A.OffsetOnly,A.DataProcessing _ A.MOV False destination _ (A.Immediate 1))
      | source==destination -> case source of A.Reg index -> Right index
    _ -> Left "synthetic publication assignment is not a constant-one store"
  let redirected = replaceLoaderWord side native assignment (jump assignment (L.CodeAddress thunkPc))
      skippedStore = replaceLoaderWord side redirected store (originalStore .&. 0x0fffffff)
  extended <- appendLoaderWords side skippedStore
    [originalAssignment,originalStore,0xe3500000 .|. (fromIntegral register `shiftL` 16)
    ,jump (L.CodeAddress (thunkPc+12)) store]
  pure (extended,L.CodeAddress (thunkPc+4))

scalarCallback :: L.LoaderSide -> C.NativeFiles -> L.CodeAddress -> Either String C.NativeFiles
scalarCallback side native publication@(L.CodeAddress pc) =
  let L.CodeAddress start = L.carrierStart side
      thunk = start+fromIntegral (B.length (C.nativeBytes native))
      jump from to = branchWord from to .&. 0xfeffffff
      redirected = replaceLoaderWord side native publication (jump publication (L.CodeAddress thunk))
  in appendLoaderWords side redirected
    [0xe3a00000,branchWord (L.CodeAddress (thunk+4)) (L.serviceAddress side L.PublishCallback)
    ,jump (L.CodeAddress (thunk+8)) (L.CodeAddress (pc+4))]
