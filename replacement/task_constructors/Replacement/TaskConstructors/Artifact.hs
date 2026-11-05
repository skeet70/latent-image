module Replacement.TaskConstructors.Artifact
  ( CanonicalSource, LinkedArtifact, Segment (..), canonicalSource, deriveArtifact
  , stageImages, segments, retainedCalls, constructors
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Data.List (sortOn)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.TaskConstructors.Admission

data CanonicalSource = CanonicalSource B.ByteString B.ByteString
newtype LinkedArtifact = LinkedArtifact [Segment]
data Segment = Segment { segmentConstructor :: Constructor, segmentBytes :: B.ByteString } deriving (Eq, Show)

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> Either String CanonicalSource
canonicalSource dat image4 image5 = do
  forM_ [(dat,"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
    ,(image4,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
    ,(image5,"3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")] $ \(bytes, expected) ->
      require (SHA.sha256Hex (SHA.sha256 bytes) == expected) "task constructors require canonical X-M1 1.50 sources"
  require (case A.decode 0xec56a0 (U.word32At image5 (fromIntegral (0xec56a0 - 0xbf1a00 :: Word32))) of
    A.Branch (A.Cond 14) True target -> target == 0xd0a238
    _ -> False) "syscont UI constructor seam differs"
  pure (CanonicalSource image4 image5)

retainedCalls :: CanonicalSource -> Constructor -> [CodeAddress]
retainedCalls (CanonicalSource image4 image5) (Constructor (CodeAddress start) (CodeAddress end)) =
  [CodeAddress target | site <- [start,start+4..end-4]
  , A.Branch _ True target <- [A.decode site (U.word32At image (fromIntegral (site-base)))]]
  where
    (base,image) = if start < 0xbf1a00 then (0x61c400,image4) else (0xbf1a00,image5)

deriveArtifact :: CanonicalSource -> B.ByteString -> B.ByteString -> Either String LinkedArtifact
deriveArtifact source packed symbolBytes = do
  symbols <- F.parseSymbols symbolBytes
  let at name = F.symbolAddress <$> F.lookupSymbol symbols name
  loaded <- forM constructors $ \constructor@(Constructor (CodeAddress low) (CodeAddress high)) -> do
    let stem = symbolStem constructor
    start <- at ("__ctor_" ++ stem ++ "_start")
    end <- at ("__ctor_" ++ stem ++ "_end")
    load <- at ("__ctor_" ++ stem ++ "_load")
    entry <- at ("xm1_ctor_" ++ stem)
    codeEnd <- at ("__ctor_" ++ stem ++ "_code_end")
    body <- at ("xm1_ctor_" ++ stem ++ "_body")
    require (start == low && end == high && entry == low) "constructor linked entry or extent differs"
    let offset = fromIntegral load
        size = fromIntegral (high-low)
        bytes = B.take size (B.drop offset packed)
    require (toInteger load + toInteger size <= toInteger (B.length packed)) "constructor packed extent is truncated"
    require (codeEnd > low && codeEnd <= high && codeEnd `mod` 4 == 0 && body >= low && body < codeEnd) "constructor code extent differs"
    require (body == low) "constructor Rust body does not preserve the stock entry"
    require (B.all (== 0) (B.drop (fromIntegral (codeEnd-low)) bytes)) "constructor padding is not zero"
    admitDescriptorFrame constructor (B.take (fromIntegral (codeEnd-low)) bytes)
    admitCode constructor bytes (retainedCalls source constructor)
    pure (PackedOffset load, Segment constructor bytes)
  let ordered = sortOn fst loaded
      offsets = scanl (+) 0 [fromIntegral (B.length bytes) | (_,Segment _ bytes) <- ordered]
  require (map fst ordered == map PackedOffset (init offsets) && last offsets == fromIntegral (B.length packed)) "constructor packed extents are not a contiguous partition"
  pure (LinkedArtifact (map snd loaded))

segments :: LinkedArtifact -> [Segment]
segments (LinkedArtifact rows) = rows

stageImages :: CanonicalSource -> LinkedArtifact -> (B.ByteString,B.ByteString)
stageImages (CanonicalSource image4 image5) (LinkedArtifact rows) =
  (foldl (replace 0x61c400) image4 [row | row@(Segment (Constructor (CodeAddress start) _) _) <- rows, start < 0xbf1a00]
  ,foldl (replace 0xbf1a00) image5 [row | row@(Segment (Constructor (CodeAddress start) _) _) <- rows, start >= 0xbf1a00])
  where
    replace base original (Segment (Constructor (CodeAddress start) (CodeAddress end)) bytes) =
      B.take (fromIntegral (start-base)) original <> bytes <> B.drop (fromIntegral (end-base)) original
