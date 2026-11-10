{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (encode, object, eitherDecodeStrict', Value, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import qualified Emulation.Image4Staging as I
import qualified Emulation.Sha256 as SHA
import qualified Emulation.OwnedCase as O
import CustomAdmission (admitCustomBoundaryEncodings, BankApplyElfInput(..), admitBankApplyElf)
import qualified FujiAF.Host as H
import Replacement.SettingsStorage.Publication
import qualified Replacement.RecordStore.References as R
import Replacement.RecordStore.Carrier (cacheContract)
import Replacement.PropertyService.Placement (ScanImage(..), GuestAddress(..), Carrier(..), hitListing)
import ServiceInventory (Image(..))
import System.Directory (doesPathExist)
import System.Environment (getArgs)

hash :: B.ByteString -> String
hash = SHA.sha256Hex . SHA.sha256
fresh :: [FilePath] -> IO ()
fresh paths = forM_ paths $ \path -> doesPathExist path >>= \exists -> when exists (H.failH (path ++ " already exists"))
main :: IO ()
main = do
  args <- getArgs
  case args of
    ["prepare",hostSymbolsPath,hostMapPath,hostLinkerPath,outLinkerPath] -> do
      fresh [outLinkerPath]
      hostSymbols <- H.readRegular 0x100000 hostSymbolsPath
      hostMap <- H.readRegular 0x1000000 hostMapPath
      hostLinker <- H.readRegular 0x10000 hostLinkerPath
      plan <- either H.failH pure (planPublication hostSymbols hostMap)
      linker <- either H.failH pure (publicationLinker plan hostLinker)
      B.writeFile outLinkerPath linker
      L.putStr (encode (planValue plan) <> "\n")
    ["stage",encoder,rebuild,decoder,datPath,baseDatPath,stockFourPath,stageFourPath,fivePath,plainPath,indexPath,stageContractPath,hostSymbolsPath,hostMapPath,binPath,elfPath,symbolsPath,scratchPath,outPath,outImagePath,contractPath] -> do
      fresh [outPath,outImagePath,contractPath]
      dat <- H.readRegular (64*1024*1024) datPath
      baseDat <- H.readRegular (64*1024*1024) baseDatPath
      stockFour <- H.readRegular (16*1024*1024) stockFourPath
      stageFour <- H.readRegular (16*1024*1024) stageFourPath
      five <- H.readRegular (16*1024*1024) fivePath
      plain <- H.readRegular (16*1024*1024) plainPath
      index <- H.readRegular (32*1024*1024) indexPath
      contractBytes <- H.readRegular (8*1024*1024) stageContractPath
      contract <- either H.failH pure (parseStage4Contract contractBytes)
      propertyDocument <- either H.failH pure (eitherDecodeStrict' contractBytes :: Either String Value)
      hostSymbols <- H.readRegular 0x100000 hostSymbolsPath
      hostMap <- H.readRegular 0x1000000 hostMapPath
      plan <- either H.failH pure (planPublication hostSymbols hostMap)
      linked <- H.readRegular (2*1024*1024) binPath
      elf <- H.readRegular (16*1024*1024) elfPath
      symbols <- H.readRegular 0x100000 symbolsPath
      forM_ [(dat,"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"),
          (stockFour,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"),
          (five,"3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"),
          (plain,"ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"),
          (index,"c1b0cdbfb41d4241cd5250a133d90badece759860e138693d5154e78bda9b3c3")] $ \(bytes,pin) ->
        unless (hash bytes == pin) (H.failH "canonical publication input identity mismatch")
      unless (stage4SourceHash contract == hash dat && stage4ImageHash contract == hash stageFour) (H.failH "stage4 contract does not identify input image")
      ranges <- either H.failH pure (R.parseRanges index)
      let images = [(ScanImage4,Image "image4" stockFourPath 0x61c400 stockFour),
            (ScanImage5,Image "image5" fivePath 0xbf1a00 five),(ScanPlain,Image "plain" plainPath 0x1100000 plain)]
          hits = publicationReferences ranges images
      unless (null hits) (mapM_ (putStrLn . hitListing) hits >> H.failH "retired lifecycle, record-store or cache-tag references remain")
      staged <- either H.failH pure (stagePublication contract plan stageFour linked symbols)
      typedSymbols <- O.parseSymbols [C.unpack name | line <- C.lines symbols, [_,_,_,name] <- [C.words line]] symbolsPath
      used <- either H.failH pure (publishedRegions plan symbols)
      let carriers = [Carrier (GuestAddress start) (GuestAddress end) | Region start end <- used]
      either H.failH pure (admitCustomBoundaryEncodings 0x61d8dc carriers linked typedSymbols)
      _ <- either H.failH pure (admitBankApplyElf (BankApplyElfInput
        { bankApplyImageBase = GuestAddress 0x61d8dc
        , bankApplyCarriers = carriers
        , bankApplyLinkedBytes = linked
        , bankApplySymbolBytes = symbols
        , bankApplyElfBytes = elf
        }))
      scratch <- I.validateScratchDirectory scratchPath
      candidate <- I.runImage4Staging (I.Tools encoder rebuild decoder) scratch dat baseDat staged
      publication <- either H.failH pure (publicationValue plan staged symbols)
      entries <- either H.failH pure (publicationStackEntries contract symbols)
      B.writeFile outPath (I.candidateDat candidate)
      B.writeFile outImagePath staged
      L.writeFile contractPath (encode (object
        [ "schema" .= (3::Int), "kind" .= ("composed-settings-storage-record-store"::String)
        , "source_sha256" .= object ["dat" .= hash dat], "base_dat_sha256" .= hash baseDat
        , "property_contract_sha256" .= hash contractBytes, "property_contract" .= propertyDocument, "property_image4_sha256" .= hash stageFour
        , "host_symbols_sha256" .= hash hostSymbols, "host_map_sha256" .= hash hostMap, "linked_sha256" .= hash staged, "linked_bytes" .= B.length staged, "symbols_sha256" .= hash symbols, "elf_sha256" .= hash elf
        , "publication" .= publication, "stack" .= entries, "placement" .= planValue plan, "candidate_dat_sha256" .= I.candidateSha256 candidate
        , "published_sparse_sha256" .= hash linked, "staged_image4_sha256" .= I.candidateImage4Sha256 candidate, "function_ranges_sha256" .= hash index
        , "refused_references" .= length hits, "record_reference_scan" .= R.referenceSummary ranges images
        , "owned_cache" .= cacheContract
        , "qualification" .= ("publication-only; native and full-system gates remain separate"::String)
        ]) <> "\n")
    _ -> H.failH "usage: custom-publication prepare HOST_SYMBOLS HOST_MAP HOST_LD OUT_LD | stage ENCODER REBUILD DECODER CANONICAL_DAT BASE_DAT STOCK_IMAGE4 PROPERTY_IMAGE4 IMAGE5 PLAIN FUNCTION_INDEX PROPERTY_CONTRACT HOST_SYMBOLS HOST_MAP PUBLISHED_BIN PUBLISHED_ELF PUBLISHED_SYMBOLS SCRATCH OUT_DAT OUT_IMAGE4 OUT_CONTRACT"
