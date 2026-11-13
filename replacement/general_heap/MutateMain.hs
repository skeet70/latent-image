{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Sha256 as SHA
import FujiAF.ClassicChrome (Address (..))
import FujiAF.ClassicChromeRam (encodeBranch)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.GeneralHeap.Artifact (deriveArtifact)
import System.Environment (getArgs)
import System.Exit (die)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [imagePath, symbolsPath, outputPath, receiptPath] -> do
      image <- B.readFile imagePath
      symbolsBytes <- B.readFile symbolsPath
      _ <- either (die . show) pure (deriveArtifact image symbolsBytes)
      symbols <- either die pure (F.parseSymbols symbolsBytes)
      let address name = either die (pure . F.symbolAddress) (F.lookupSymbol symbols name)
      source <- address "xm1_heap_malloc_r"
      target <- address "xm1_heap_trim_r"
      branch <- either die pure (encodeBranch (Address source) (Address target))
      resume <- either die pure (encodeBranch (Address (target + 12)) (Address (source + 4)))
      let offset = fromIntegral (source - 0x89cda4)
          cave = fromIntegral (target - 0x89cda4)
          replace position bytes original = B.take position original <> bytes <> B.drop (position + B.length bytes) original
          trampoline = B.concat (map U.putWord32 [U.word32At image offset, 0xe3a0300c, 0xe5803000, resume])
          mutant = replace offset (U.putWord32 branch) (replace cave trampoline image)
          digest = SHA.sha256Hex . SHA.sha256
      unless (source >= 0x89cda4 && offset + 4 <= B.length image && mutant /= image) (die "invalid heap mutation site")
      _ <- either (die . show) pure (deriveArtifact mutant symbolsBytes)
      B.writeFile outputPath mutant
      L.writeFile receiptPath (encode (object
        [ "variant" .= ("malloc-stores-errno-12-on-success" :: String)
        , "source_sha256" .= digest image, "mutant_sha256" .= digest mutant
        , "site" .= source, "target" .= target
        , "old_word" .= U.word32At image offset, "new_word" .= branch
        , "expected_difference" .= ("successful malloc changes context errno while satisfying role-local checks" :: String)
        ]))
    _ -> die "heap-mutant LINKED_BIN SYMBOLS OUT_BIN RECEIPT_JSON"
