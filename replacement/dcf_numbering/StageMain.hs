{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import qualified Emulation.Image4Staging as I
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FontWindow as FW
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import ReferenceScan (Address (..), Image (..), Reference (..))
import Replacement.DcfNumbering.Carrier
import System.Directory (doesPathExist)
import System.Environment (getArgs)

hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256

main :: IO ()
main = do
  args <- getArgs
  case args of
    [encoder, rebuild, decoder, datPath, fourPath, fivePath, plainPath, loaderAPath, loaderBPath, twoPath, binPath, symbolsPath, scratchPath, outPath, contractPath] -> do
      forM_ [outPath, contractPath] $ \path -> doesPathExist path >>= \exists -> when exists (H.failH (path ++ " already exists"))
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
      linked <- H.readRegular 0x10000 binPath
      symbols <- H.readRegular 0x100000 symbolsPath
      let images =
            [ Image "image4" (Address 0x61c400) four, Image "image5" (Address 0xbf1a00) five
            , Image "plain" (Address 0x1100000) plain, Image "loader_a" (Address 0x40000) loaderA
            , Image "loader_b" (Address 0xd0000) loaderB, Image "image2" (Address 0x130000) two
            , Image "image3" (Address 0x3bc000) three ]
          findings = census images
          refused = [finding | finding <- findings, findingVerdict finding == Refused]
          summary =
            M.toList (M.fromListWith (+) [((name, verdictKey (findingVerdict f)), 1 :: Int) | f <- findings, let Reference _ _ name _ = findingReference f])
      unless (null refused) (mapM_ (L.putStr . (<> "\n") . encode . findingValue) refused >> H.failH "references into the retired extents that are not entries remain")
      staged <- either H.failH pure (stageImage four linked symbols)
      scratch <- I.validateScratchDirectory scratchPath
      candidate <- I.runImage4Staging (I.Tools encoder rebuild decoder) scratch dat dat staged
      B.writeFile outPath (I.candidateDat candidate)
      L.writeFile contractPath $
        encode
          ( object
              [ "schema" .= (1 :: Int), "kind" .= ("owned-dcf-numbering" :: String)
              , "source_sha256" .= hash dat, "linked_sha256" .= hash linked, "symbols_sha256" .= hash symbols
              , "candidate_sha256" .= I.candidateSha256 candidate, "image4_sha256" .= I.candidateImage4Sha256 candidate
              , "extents" .= [[s, e] | Extent s e <- extents]
              , "entries" .= [object ["address" .= a, "name" .= n] | Entry a n <- entries]
              , "scanned" .= [object ["image" .= name, "base" .= base, "bytes" .= B.length bytes, "sha256" .= hash bytes] | Image name (Address base) bytes <- images]
              , "reference_counts" .= [object ["image" .= name, "verdict" .= verdict, "count" .= count] | ((name, verdict), count) <- summary]
              , "references" .= [findingValue f | f <- findings, findingVerdict f `notElem` [NotWordAddress, HalfwordSite]]
              , "refused_references" .= length refused
              ]
          )
          <> "\n"
    _ -> H.failH "usage: stage-dcf-numbering ENCODER REBUILD DECODER DAT IMAGE4 IMAGE5 PLAIN LOADER_A LOADER_B IMAGE2 LINKED SYMBOLS SCRATCH OUT_DAT OUT_CONTRACT"
  where
    verdictKey :: Verdict -> String
    verdictKey verdict = case verdict of
      EntryBranch -> "entry-branch"
      EntryPointer -> "entry-pointer"
      NotWordAddress -> "not-word-aligned"
      HalfwordSite -> "halfword-aligned-site"
      Reviewed _ -> "reviewed-data"
      Refused -> "refused"
