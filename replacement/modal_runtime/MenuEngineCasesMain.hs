{-# LANGUAGE GHC2021 #-}

-- | Writes the owned menu engine's Rust oracle cases from `--menu-observe` receipt directories.
--
-- usage: menu-engine-cases --out FILE NAME=DIR...
-- Each DIR holds menu-observe.json and the menu-observe-config.json it names.
module Main where

import Control.Monad (forM, when)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.ModalRuntime.MenuEngineCases
import System.Directory (doesPathExist)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  (outPath, specs) <- case arguments of
    "--out" : out : rest@(_ : _) -> pure (out, rest)
    _ -> fail "usage: menu-engine-cases --out FILE NAME=DIR..."
  exists <- doesPathExist outPath
  when exists (fail (outPath ++ " already exists"))
  cases <- forM specs $ \spec -> do
    (name, dir) <- case break (== '=') spec of
      (name, '=' : dir) | not (null name), not (null dir) -> pure (name, dir)
      _ -> fail ("not NAME=DIR: " ++ spec)
    configBytes <- B.readFile (dir ++ "/menu-observe-config.json")
    receiptBytes <- B.readFile (dir ++ "/menu-observe.json")
    config <- either (fail . ((name ++ " config: ") ++)) pure (J.eitherDecodeStrict configBytes)
    receipt <- either (fail . ((name ++ " receipt: ") ++)) pure (J.eitherDecodeStrict receiptBytes)
    let configSha = sha256Hex (sha256 configBytes)
        receiptSha = sha256Hex (sha256 receiptBytes)
    extracted <- either (fail . ((name ++ ": ") ++)) pure (extract configSha config receipt)
    putStrLn (name ++ ": service calls " ++ show (length (caseCalls extracted))
      ++ ", snapshot writes " ++ show (length (caseSnapshotWrites extracted)) ++ ", last-move writes " ++ show (length (caseLastMoveWrites extracted)))
    pure (name, receiptSha, configSha, extracted)
  writeFile outPath (rustCases cases)
