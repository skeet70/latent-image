module Main where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Emulation.InterfaceInitializer (Role (..), contractValue, declarations, deriveContract)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import StageBuild (StageDeclarations (..))
import qualified StartupStageContract as S
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["retained", sourcePath, outputPath, contractPath] ->
      stage Retained sourcePath B.empty B.empty outputPath contractPath
    ["owned", sourcePath, imagePath, symbolsPath, outputPath, contractPath] -> do
      image <- H.readRegular F.firstRecordSlotLength imagePath
      symbols <- H.readRegular 0x10000 symbolsPath
      stage Owned sourcePath image symbols outputPath contractPath
    _ -> H.failH "usage: build-interface-initializer retained SOURCE_DAT OUT_DAT OUT_CONTRACT | owned SOURCE_DAT IMAGE_BIN IMAGE_SYMBOLS OUT_DAT OUT_CONTRACT"

stage :: Role -> FilePath -> B.ByteString -> B.ByteString -> FilePath -> FilePath -> IO ()
stage role sourcePath image symbolBytes outputPath contractPath = do
  source <- H.readRegular (64 * 1024 * 1024) sourcePath
  original <- either H.failH pure (S.parseOriginalStartupDat source)
  staged <- either H.failH pure $ case role of
    Retained -> Right source
    Owned -> do
      symbols <- F.parseSymbols symbolBytes
      declared <- declarations symbols
      let parsed = S.originalStartupParsed original
          parts = U.parsedParts parsed
      start <- F.symbolAddress <$> F.lookupSymbol symbols "__secondary_stage_start"
      entry <- F.symbolAddress <$> F.lookupSymbol symbols "xm1_secondary_entry" >>= F.armEntry
      offset <- F.slotOffset (fromIntegral start - 0x10000)
      plan <- F.stagePlan F.SecondRecord entry offset (U.MediaImage image) (declaredRetained declared)
      replaced <- F.replaceRecordSlot plan parts >>= F.applyLoaderWordPatches (declaredLoaderPatches declared)
      rebuilt <- U.rebuildContainer parsed replaced
      reparsed <- U.parseContainer rebuilt
      F.verifyStagedRecord plan (declaredLoaderPatches declared) parsed reparsed
      pure rebuilt
  contract <- either H.failH pure (deriveContract role source staged image symbolBytes)
  B.writeFile outputPath staged
  BL.writeFile contractPath (J.encode (contractValue contract))
