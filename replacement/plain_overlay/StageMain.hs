{-# LANGUAGE OverloadedStrings #-}

-- | plain-overlay-stage REBUILD DAT IMAGE4 IMAGE5 PLAIN LOADER_A LOADER_B IMAGE2 OUT_DIR (UNIT_JSON ELF)...
--
-- Stages each owned unit's ELF into the stock plain image, refuses while the census finds a
-- reference into the units' extents it does not admit, and carries the staged plain image into
-- the DAT through `rebuild-update replace-plain`. OUT_DIR receives plain.bin, candidate.DAT,
-- rebuild.txt and contract.json.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FontWindow as FW
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import ReferenceScan (Address (..), Image (..), Reference (..))
import Replacement.PlainOverlay.Stage
import Replacement.PlainOverlay.Unit
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.Process (readProcessWithExitCode)

hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256

main :: IO ()
main = do
  args <- getArgs
  case args of
    rebuild : datPath : fourPath : fivePath : plainPath : loaderAPath : loaderBPath : twoPath : outDir : unitArgs@(_ : _ : _) -> do
      exists <- doesPathExist outDir
      when exists (H.failH (outDir ++ " already exists"))
      pairs <- unitPairs unitArgs
      dat <- H.readRegular (64 * 1024 * 1024) datPath
      four <- H.readRegular (16 * 1024 * 1024) fourPath
      five <- H.readRegular (16 * 1024 * 1024) fivePath
      plain <- H.readRegular (16 * 1024 * 1024) plainPath
      loaderA <- H.readRegular (1024 * 1024) loaderAPath
      loaderB <- H.readRegular (1024 * 1024) loaderBPath
      two <- H.readRegular (16 * 1024 * 1024) twoPath
      forM_
        [ (dat, "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
        , (four, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
        , (five, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
        , (plain, "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
        , (loaderA, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
        , (loaderB, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
        , (two, "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5")
        ]
        $ \(bytes, pin) -> unless (hash bytes == pin) (H.failH "canonical source digest mismatch")
      three <- either H.failH (pure . U.mediaImageBytes) (U.parseContainer dat >>= FW.extractFont . U.parsedParts)
      unless (hash three == "a52dfa0aaea0c3eecb106aed809e99fed71cb9a754dc7cd412ff488b98542dac") (H.failH "image 3 digest mismatch")
      loaded <- forM pairs $ \(unitPath, elfPath) -> do
        unit <- either H.failH pure . parseUnit =<< H.readRegular (1024 * 1024) unitPath
        elfBytes <- H.readRegular (16 * 1024 * 1024) elfPath
        elf <- either H.failH pure (E.parseElf elfBytes)
        pure (unit, elf, elfBytes)
      let units = [u | (u, _, _) <- loaded]
          images =
            [ (Image4, Image "image4" (Address 0x61c400) four), (Image5, Image "image5" (Address 0xbf1a00) five)
            , (Plain, Image "plain" (Address plainBase) plain), (LoaderA, Image "loader_a" (Address 0x40000) loaderA)
            , (LoaderB, Image "loader_b" (Address 0xd0000) loaderB), (Image2, Image "image2" (Address 0x130000) two)
            , (Image3, Image "image3" (Address 0x3bc000) three) ]
          findings = census units images
          refused = [f | f <- findings, findingVerdict f == Refused]
          summary =
            M.toList (M.fromListWith (+) [((name, verdictKey (findingVerdict f)), 1 :: Int) | f <- findings, let Reference _ _ name _ = findingReference f])
      unless (null refused) (mapM_ (L.putStr . (<> "\n") . encode . findingValue) refused >> H.failH "references into the units' extents that are not admitted remain")
      (staged, stagedUnits) <- either (H.failH . show) pure (stagePlain plain [(u, e) | (u, e, _) <- loaded])
      let changed = [i | i <- [0 .. B.length plain - 1], B.index plain i /= B.index staged i]
          declared i = let a = plainBase + fromIntegral i in any (\u -> any (`inside` a) (unitSpans u)) units
      unless (all declared changed) (H.failH "a staged byte lies outside every declared span")
      createDirectory outDir
      let plainOut = outDir </> "plain.bin"
          datOut = outDir </> "candidate.DAT"
      B.writeFile plainOut staged
      (code, out, err) <- readProcessWithExitCode rebuild ["replace-plain", datPath, plainOut, datOut] ""
      writeFile (outDir </> "rebuild.txt") (out ++ err)
      unless (code == ExitSuccess) (H.failH ("rebuild-update replace-plain failed: " ++ err))
      candidate <- H.readRegular (64 * 1024 * 1024) datOut
      carried <- either H.failH (pure . U.mediaImageBytes) (U.parseContainer candidate >>= U.extractPlain . U.parsedParts)
      unless (carried == staged) (H.failH "the candidate DAT does not carry the staged plain image")
      L.writeFile (outDir </> "contract.json") $
        encode
          ( object
              [ "schema" .= (1 :: Int), "kind" .= ("plain-overlay-stage" :: String)
              , "source_sha256" .= hash dat, "plain_sha256" .= hash plain
              , "staged_plain_sha256" .= hash staged, "candidate_sha256" .= hash candidate
              , "changed_bytes" .= length changed
              , "units" .=
                  [ object
                      [ "unit" .= unitValue (stagedUnit s), "elf_sha256" .= hash elfBytes
                      , "defined_bytes" .= stagedDefined s
                      , "rows" .= [object ["address" .= hexWord (rowAddress r), "stock" .= hexWord (rowStock r), "owned" .= hexWord w] | (r, w) <- stagedRows s]
                      , "overlay_sha256" .= let Span lo hi = overlaySpan (unitOverlay (stagedUnit s)) in hash (B.take (fromIntegral (hi - lo)) (B.drop (fromIntegral (lo - plainBase)) staged))
                      ]
                  | (s, (_, _, elfBytes)) <- zip stagedUnits loaded
                  ]
              , "scanned" .= [object ["image" .= name, "base" .= hexWord base, "bytes" .= B.length bytes, "sha256" .= hash bytes] | (_, Image name (Address base) bytes) <- images]
              , "reference_counts" .= [object ["image" .= name, "verdict" .= verdict, "count" .= count] | ((name, verdict), count) <- summary]
              , "references" .= [findingValue f | f <- findings, findingVerdict f `notElem` [NotWordAddress, HalfwordSite]]
              , "refused_references" .= length refused
              ]
          )
          <> "\n"
    _ -> H.failH "usage: plain-overlay-stage REBUILD DAT IMAGE4 IMAGE5 PLAIN LOADER_A LOADER_B IMAGE2 OUT_DIR (UNIT_JSON ELF)..."
  where
    unitPairs (u : e : rest) = ((u, e) :) <$> unitPairs rest
    unitPairs [] = pure []
    unitPairs _ = H.failH "units come as UNIT_JSON ELF pairs"
