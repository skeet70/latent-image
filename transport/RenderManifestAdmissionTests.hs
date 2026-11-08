{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import FujiAF.JSON (encodeCanonical)
import FujiAF.RamPush (RamPushManifest(..), readRamPushManifest)
import FujiAF.SHA256 (sha256)
import RamPushLayout (modalRenderBankVariant)
import System.Environment (getArgs)
import System.FilePath ((</>), takeDirectory)

main :: IO ()
main = do
  arguments <- getArgs
  path <- case arguments of
    [manifest] -> pure manifest
    _ -> fail "usage: render-manifest-admission-tests RAM_PUSH_MANIFEST"
  manifest <- readRamPushManifest path
  unless (manifestBankVariant manifest == Just modalRenderBankVariant && length (manifestPublications manifest) == 228)
    (fail "expected the 228-publication render manifest")
  bytes <- B.readFile (takeDirectory path </> "render-source-identity.json")
  sources <- either fail pure (J.eitherDecodeStrict' bytes :: Either String (M.Map FilePath String))
  unless (M.size sources == 14 && encodeCanonical (J.toJSON sources) == bytes)
    (fail "render source identity must contain fourteen canonical source hashes")
  unless (lookup "render_sources_sha256" (sourceIdentity manifest) == Just (sha256 bytes))
    (fail "render source identity digest differs from manifest")
  forM_ (M.toList sources) $ \(source, expected) -> do
    actual <- sha256 <$> B.readFile source
    unless (actual == expected) (fail ("render source identity differs: " ++ source))
  putStrLn ("render manifest admitted: 228 publications, " ++ show (length (sourceIdentity manifest)) ++ " source identities, fourteen render sources verified")
