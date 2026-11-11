{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.LensTransport.CodecSuite (runSuite, sourceIdentity)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner,cache,output,image,elf] -> do
      createDirectoryIfMissing True output
      imageHash <- sha256Hex . sha256 <$> B.readFile image
      ownedHash <- sha256Hex . sha256 <$> B.readFile elf
      results <- runSuite runner cache output image elf
      let passed = all (null . snd) results
      L.writeFile (output </> "codec-report.json") (encode (object
        [ "source" .= sourceIdentity, "image_path" .= image, "image_sha256" .= imageHash
        , "owned_path" .= elf, "owned_sha256" .= ownedHash, "runner" .= runner
        , "passed" .= passed, "cases" .= [object ["id" .= key,"differences" .= differences] | (key,differences) <- results]
        , "scope" .= ("Bounded constructor publication, fixed-four-byte response classification and retained previous-response caller callback. Raw batch inputs, runner outputs and cache evidence remain under the supplied output/cache roots. No short-frame or oversized stock call, physical driver, IRQ, deadline or lifecycle claim." :: String)
        ]))
      print (length results,passed)
      unless passed exitFailure
    _ -> fail "usage: codec-suite RUNNER CACHE OUTPUT IMAGE4 OWNED_CODEC_ELF"
