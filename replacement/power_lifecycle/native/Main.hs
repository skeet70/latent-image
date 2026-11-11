{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.PowerLifecycle.RecordHelperSuite (runSuite, sourceIdentity)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner, cache, output, image4, image5, elf] -> do
      createDirectoryIfMissing True output
      image4Hash <- sha256Hex . sha256 <$> B.readFile image4
      ownedHash <- sha256Hex . sha256 <$> B.readFile elf
      results <- runSuite runner cache output image4 image5 elf
      let passed = all (null . snd) results
      L.writeFile (output </> "record-helper-report.json") (encode (object
        [ "source" .= sourceIdentity, "image4_path" .= image4, "image4_sha256" .= image4Hash
        , "owned_path" .= elf, "owned_sha256" .= ownedHash, "runner" .= runner
        , "passed" .= passed
        , "cases" .= [object ["id" .= key, "differences" .= differences] | (key, differences) <- results]
        , "scope" .= ("Leaf stock bodies that are pure functions of their registers and declared ordinary memory. No MMIO, no undefined callee, no interrupt, timing or device claim." :: String)
        ]))
      print (length results, passed)
      unless passed exitFailure
    _ -> fail "usage: record-helper-suite RUNNER CACHE OUTPUT IMAGE4 IMAGE5 OWNED_ELF"
