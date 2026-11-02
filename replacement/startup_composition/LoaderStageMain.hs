module Main (main) where

import Control.Monad (forM_)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as Lazy
import qualified Emulation.Image4Staging as Image4
import qualified FujiAF.Host as H
import qualified Replacement.StartupComposition.Artifact as C
import Replacement.StartupComposition.LoaderStartup
import System.Directory (canonicalizePath)
import System.Environment (getArgs)
import System.FilePath ((</>))
import System.IO (hClose)
import System.Posix.IO (OpenMode (WriteOnly), defaultFileFlags, creat, exclusive, fdToHandle, openFd)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder,rebuild,decoder,datPath,imagePath,sourceAPath,sourceBPath,aPath,aSymbolsPath,bPath,bSymbolsPath,dispatchPath,dispatchSymbolsPath,installPath,installSymbolsPath,scratchPath] -> do
      sources <- C.SourceFiles <$> H.readRegular 0x4000000 datPath <*> H.readRegular 0x300000 imagePath
        <*> H.readRegular 0x20000 sourceAPath <*> H.readRegular 0x20000 sourceBPath
      let native image symbols = C.NativeFiles <$> H.readRegular 0x1000 image <*> H.readRegular 0x10000 symbols
      a <- native aPath aSymbolsPath
      b <- native bPath bSymbolsPath
      dispatcher <- native dispatchPath dispatchSymbolsPath
      installer <- native installPath installSymbolsPath
      source <- either (H.failH . show) pure (canonicalSource sources)
      artifact <- either (H.failH . show) pure (deriveArtifact a b dispatcher installer)
      scratch <- Image4.validateScratchDirectory scratchPath
      root <- canonicalizePath scratchPath
      let names = ["canonical.dat","canonical-image4.bin","canonical-loader-a.bin","canonical-loader-b.bin"
            ,"candidate.dat","candidate-image4.bin","candidate-loader-a.bin","candidate-loader-b.bin"
            ,"loader-a.bin","loader-a.symbols","loader-b.bin","loader-b.symbols"
            ,"dispatcher.bin","dispatcher.symbols","installer.bin","installer.symbols","contract.json"]
      handles <- mapM (\name -> openFd (root </> name) WriteOnly defaultFileFlags {creat=Just 0o600,exclusive=True} >>= fdToHandle) names
      candidate <- stageCandidate (Image4.Tools encoder rebuild decoder) scratch source artifact
      let bytes = [C.sourceDat sources,C.sourceImage4 sources,C.sourceLoaderA sources,C.sourceLoaderB sources
            ,candidateBytes candidate,candidateImage4 candidate,candidateLoader candidate LoaderA,candidateLoader candidate LoaderB
            ,C.nativeBytes a,C.nativeSymbols a,C.nativeBytes b,C.nativeSymbols b
            ,C.nativeBytes dispatcher,C.nativeSymbols dispatcher,C.nativeBytes installer,C.nativeSymbols installer
            ,Lazy.toStrict (J.encode (candidateContractValue candidate))]
      forM_ (zip handles bytes) $ \(handle,content) -> B.hPut handle content >> hClose handle
    _ -> H.failH "usage: stage-loader-startup-composition ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LOADER_A LOADER_B NATIVE_A SYMBOLS_A NATIVE_B SYMBOLS_B DISPATCH_BIN DISPATCH_SYMBOLS INSTALL_BIN INSTALL_SYMBOLS SCRATCH_DIR"
