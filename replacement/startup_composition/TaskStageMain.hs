module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (nub)
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import Replacement.StartupComposition.TaskSubmission
import System.Directory (canonicalizePath)
import System.Environment (getArgs)
import System.FilePath (takeDirectory, (</>), takeFileName)
import System.Posix.IO (OpenMode (WriteOnly), defaultFileFlags, creat, exclusive, fdToHandle, openFd)
import System.IO (hClose)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,imagePath,loaderAPath,loaderBPath,wrapperPath,wrapperSymbolsPath,criticalPath,criticalSymbolsPath,storagePath,storageSymbolsPath,scratchPath,outputPath,contractPath,outImage,outLoaderA,outLoaderB,outRecord2] -> do
      sources <- SourceFiles <$> H.readRegular 0x4000000 datPath <*> H.readRegular 0x300000 imagePath
        <*> H.readRegular 0x20000 loaderAPath <*> H.readRegular 0x20000 loaderBPath
      wrapper <- NativeFiles <$> H.readRegular 0x1000 wrapperPath <*> H.readRegular 0x10000 wrapperSymbolsPath
      critical <- NativeFiles <$> H.readRegular 0x1400 criticalPath <*> H.readRegular 0x10000 criticalSymbolsPath
      storage <- NativeFiles <$> H.readRegular 0x1000 storagePath <*> H.readRegular 0x10000 storageSymbolsPath
      source <- either (H.failH . show) pure (canonicalSource sources)
      artifact <- either (H.failH . show) pure (deriveArtifact wrapper critical storage)
      staged <- either (H.failH . show) pure (stageImage4 source artifact)
      scratch <- Image4.validateScratchDirectory scratchPath
      root <- canonicalizePath scratchPath
      paths <- mapM (\path -> do
        parent <- canonicalizePath (takeDirectory path)
        H.ensure (parent == root) "task composition outputs must be directly inside the supplied scratch directory"
        pure (parent </> takeFileName path)) [outputPath,contractPath,outImage,outLoaderA,outLoaderB,outRecord2]
      unless (length (nub paths) == length paths) (H.failH "task composition outputs must be distinct")
      handles <- mapM (\path -> openFd path WriteOnly defaultFileFlags {creat = Just 0o600, exclusive = True} >>= fdToHandle) paths
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch staged
      let actual = candidateSources candidate
          bytes = [candidateBytes candidate,L.toStrict (J.encode (candidateContractValue candidate)),candidateImage4 actual
            ,candidateLoaderA actual,candidateLoaderB actual,candidateRecord2 actual]
      forM_ (zip handles bytes) $ \(handle,content) -> B.hPut handle content >> hClose handle
    _ -> H.failH "usage: stage-task-composition ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LOADER_A LOADER_B WRAPPER_BIN WRAPPER_SYMBOLS CRITICAL_BIN CRITICAL_SYMBOLS STORAGE_BIN STORAGE_SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT OUT_IMAGE4 OUT_LOADER_A OUT_LOADER_B OUT_RECORD2"
