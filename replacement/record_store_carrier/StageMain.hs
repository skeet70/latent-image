{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as I
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import Replacement.RecordStore.Carrier
import Replacement.RecordStore.References
import Replacement.PropertyService.Placement (ScanImage (..), hitListing)
import ServiceInventory (Image (..))
import System.Directory (doesPathExist)
import System.Environment (getArgs)

hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256

main :: IO ()
main = do
  args <- getArgs
  case args of
    [encoder,rebuild,decoder,datPath,fourPath,fivePath,plainPath,indexPath,binPath,symbolsPath,scratchPath,outPath,contractPath] -> do
      forM_ [outPath,contractPath] $ \path -> doesPathExist path >>= \exists -> when exists (H.failH (path ++ " already exists"))
      dat <- H.readRegular (64*1024*1024) datPath
      four <- H.readRegular (16*1024*1024) fourPath
      five <- H.readRegular (16*1024*1024) fivePath
      plain <- H.readRegular (16*1024*1024) plainPath
      index <- H.readRegular (32*1024*1024) indexPath
      unless (hash index == "c1b0cdbfb41d4241cd5250a133d90badece759860e138693d5154e78bda9b3c3") (H.failH "function-ranges index digest mismatch")
      ranges <- either H.failH pure (parseRanges index)
      forM_ [(dat,"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"),
          (four,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"),
          (five,"3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"),
          (plain,"ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")] $ \(bytes,pin) ->
        unless (hash bytes == pin) (H.failH "canonical source digest mismatch")
      linked <- H.readRegular 0x10000 binPath
      symbols <- H.readRegular 0x100000 symbolsPath
      let images = [(ScanImage4,Image "image4" fourPath 0x61c400 four),
            (ScanImage5,Image "image5" fivePath 0xbf1a00 five),(ScanPlain,Image "plain" plainPath 0x1100000 plain)]
          hits = resolvedReferences ranges images
      unless (null hits) (mapM_ (putStrLn . hitListing) hits >> H.failH "retired carrier/data references remain")
      staged <- either H.failH pure (stageImage four linked symbols)
      scratch <- I.validateScratchDirectory scratchPath
      candidate <- I.runImage4Staging (I.Tools encoder rebuild decoder) scratch dat dat staged
      B.writeFile outPath (I.candidateDat candidate)
      L.writeFile contractPath (encode (object ["schema" .= (1::Int), "kind" .= ("owned-record-store"::String),
        "source_sha256" .= hash dat, "linked_sha256" .= hash linked, "symbols_sha256" .= hash symbols,
        "candidate_sha256" .= I.candidateSha256 candidate, "image4_sha256" .= I.candidateImage4Sha256 candidate,
        "span" .= ([0x6ac2b4,0x6ae114]::[Int]), "refused_references" .= length hits,
        "function_ranges_sha256" .= hash index, "reference_scan" .= referenceSummary ranges images]) <> "\n")
    _ -> H.failH "usage: stage-record-store ENCODER REBUILD DECODER DAT IMAGE4 IMAGE5 PLAIN FUNCTION_INDEX LINKED SYMBOLS SCRATCH OUT_DAT OUT_CONTRACT"
