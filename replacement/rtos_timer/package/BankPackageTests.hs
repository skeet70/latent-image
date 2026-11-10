{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson ((.:), withObject)
import Data.Aeson.Types (parseMaybe)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import qualified FujiAF.RamPush as Ram
import Replacement.RtosTimer.BankPackage
import System.Directory (canonicalizePath)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [manifestPath, elfPath, editPath, headerPath, storagePath, armPath, fieldsPath, uiPath] -> do
      manifest <- Ram.readRamPushManifest manifestPath
      elf <- input elfPath
      sources <- RecipeEditSources <$> input editPath <*> input headerPath <*> input storagePath
        <*> input armPath <*> input fieldsPath <*> input uiPath
      package <- admitted (bindRecipeEditPackage manifest elf sources)
      check "linker binding is empty" (not (B.null (recipeEditLinkerInclude package)))
      let claim = parseMaybe (withObject "package identity" $ \fields -> do
            installed <- fields .: "installed_lifetime_admitted"
            getter <- fields .: "getter"
            implementation <- withObject "getter" (.: "implementation") getter
            pure (installed, implementation)) (packageIdentityValue package)
      check "getter ownership or lifetime claim differs" (claim == Just (False, "RetainedCRecipeEditGetter" :: String))
      let corruptedSource = sources { editSource = mutate 0 (editSource sources) }
      check "different helper source accepted" (isLeft (bindRecipeEditPackage manifest elf corruptedSource))
      check "invalid ELF accepted" (isLeft (bindRecipeEditPackage manifest (mutate 0 elf) sources))
      let BankAddress helper = getterEntry (getterBinding package)
          BankAddress base = extentStart (packageAllocation package)
          changedRuntime = mutate (fromIntegral (helper - base)) (packageRuntimeBytes package)
          stale = manifest { Ram.runtimeBytes = changedRuntime }
          changed = stale { Ram.runtimeDigest = SHA.sha256Hex (SHA.sha256 changedRuntime) }
      check "stale runtime digest accepted" (isLeft (bindRecipeEditPackage stale elf sources))
      check "different carried helper accepted" (isLeft (bindRecipeEditPackage changed elf sources))
      putStrLn "recipe edit package controls passed"
    _ -> H.failH "usage: bank-package-canonical-tests MANIFEST ELF EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H"
  where
    input path = canonicalizePath path >>= H.readRegular (64 * 1024 * 1024)
    admitted = either H.failH pure
    check name condition = unless condition (H.failH name)
    mutate offset bytes
      | offset < B.length bytes = B.take offset bytes <> B.singleton (B.index bytes offset + 1) <> B.drop (offset + 1) bytes
      | otherwise = bytes
