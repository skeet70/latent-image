{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (foldM, forM, forM_)
import Data.Aeson (FromJSON(..), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout
import Replacement.FixedImage.Record
import Replacement.RtosOwnFirst.Regions (GuestAddress(..), domainLabel, domainLoaderId, domains)
import Replacement.RtosPlatform.Artifact (containedLoader)
import Replacement.RtosPlatform.Bank (Veneer(..), admitVeneers, loaderDomain, parseBank)
import Replacement.RtosPlatform.Union (parseUnion)
import System.Environment (getArgs, getEnv, setEnv)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))

newtype Rows = Rows [Publication]
instance FromJSON Rows where
  parseJSON value = do
    rows <- parseJSON value
    Rows <$> mapM (withObject "publication" $ \row -> do
      address <- row .: "address"
      old <- row .: "old"
      new <- row .: "new"
      either fail pure (publication address old new)) rows

-- | Proposed loader entry words; only 'admitVeneers' admits them.
newtype LoaderRows = LoaderRows [(Word32, Word32, Word32)]
instance FromJSON LoaderRows where
  parseJSON value = do
    rows <- parseJSON value
    LoaderRows <$> mapM (withObject "loader entry" $ \row -> (,,) <$> row .: "address" <*> row .: "old" <*> row .: "new") rows

run :: FilePath -> [String] -> IO ()
run tool arguments = do
  (stopped, status, _, errors) <- H.runBounded 1800000000 (1024 * 1024) tool arguments B.empty
  H.ensure (not stopped && status == ExitSuccess) (tool ++ ": " ++ C.unpack errors)


main :: IO ()
main = do
  arguments <- getArgs
  work <- getEnv "XM1_RUN_WORK"
  setEnv "TMPDIR" work
  case arguments of
    ["--ceiling-only", encoder, rebuild, decoder, source, output] -> do
      H.withTemporaryAt work "fixed-ceiling-" $ \temporary -> do
        patched <- patchImage encoder rebuild decoder temporary ceilingPublications source Image4
        H.readRegular (64 * 1024 * 1024) patched >>= B.writeFile output
    [encoder, rebuild, decoder, source, bodyPath, symbolsPath, rowsPath, loaderRowsPath, carriersPath, firstSymbolsPath, output] -> do
      body <- H.readRegular (fromIntegral (imageLimit - imageBase - headerBytes)) bodyPath
      symbolBytes <- H.readRegular 0x400000 symbolsPath
      symbols <- either H.failH pure (F.parseSymbols symbolBytes)
      begin <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_bss_start")
      end <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_bss_end")
      bodyStart <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_body_start")
      bodyEnd <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_body_end")
      firstSymbols <- H.readRegular 0x400000 firstSymbolsPath >>= either H.failH pure . F.parseSymbols
      buildSymbol <- either H.failH pure (F.lookupSymbol firstSymbols "XM1_FIXED_IMAGE_BUILD_ID")
      let buildOffset = F.firstRecordSlotOffset + fromIntegral (F.symbolAddress buildSymbol)
      H.ensure (F.symbolSize buildSymbol == 4 && F.symbolAddress buildSymbol >= 0x6000 && F.symbolAddress buildSymbol < 0x10000) "build id symbol leaves first stage"
      rows <- H.readRegular 0x100000 rowsPath >>= either H.failH pure . eitherDecodeStrict'
      LoaderRows loaderRows <- H.readRegular 0x100000 loaderRowsPath >>= either H.failH pure . eitherDecodeStrict'
      union <- H.readRegular 0x100000 carriersPath >>= either (H.failH . show) pure . parseUnion
      bank <- either (H.failH . show) pure (parseBank symbolBytes)
      entries <- forM loaderRows $ \(address, old, new) -> case loaderDomain (GuestAddress address) of
        Just domain -> pure (domain, address, old, new)
        Nothing -> H.failH ("loader entry row leaves both loaders: " ++ U.showHexWord address)
      _ <- either (H.failH . show) pure (admitVeneers union bank [Veneer domain (GuestAddress address) new | (domain, address, _, new) <- entries])
      loaderPatches <- either H.failH pure (sequence [F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) address old new | (domain, address, old, new) <- entries])
      let Rows requested = rows
          publications = reservationPublications ++ requested
          build = crc32 (body <> B.concat [B.concat (map U.putWord32 [address, old, new]) | Publication _ address old new <- publications]
            <> B.concat [B.concat (map U.putWord32 [address, old, new]) | (_, address, old, new) <- entries])
      record <- either H.failH pure (makeRecord (BuildId build) (BodyBounds bodyStart bodyEnd) (Bss begin end) body)
      stock <- H.readRegular (64 * 1024 * 1024) source
      parsed <- either H.failH pure (U.parseContainer stock)
      H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed)) "source container checks failed"
      H.withTemporaryAt work "fixed-image-stage-" $ \temporary -> do
        let binding = U.partPreTable (U.parsedParts parsed)
        H.ensure (U.mediaWordAt binding buildOffset == 0xfeed1170) "first-stage build-id preimage differs"
        let boundParts = (U.parsedParts parsed) { U.partPreTable = B.take buildOffset binding <> U.putMediaWord build <> B.drop (buildOffset + 4) binding }
            boundPath = temporary </> "bound.DAT"
        bound <- either H.failH pure (U.rebuildContainer parsed boundParts)
        B.writeFile boundPath bound
        stagedPath <- foldM (patchImage encoder rebuild decoder temporary publications) boundPath [Image4, Image5, Plain]
        staged <- H.readRegular (64 * 1024 * 1024) stagedPath >>= either H.failH pure . U.parseContainer
        entered <- either H.failH pure (F.applyLoaderWordPatches loaderPatches (U.parsedParts staged))
        changed <- either H.failH pure (stageRecord record entered)
        candidate <- either H.failH pure (U.rebuildContainer staged changed)
        restoredRecord <- either H.failH pure (restoreRecord (U.parsedParts staged) changed)
        inversePatches <- either H.failH pure (sequence [F.loaderWordPatch (F.loaderPatchId patch) (F.loaderPatchAddress patch) (F.loaderPatchReplacement patch) (F.loaderPatchExpected patch) | patch <- loaderPatches])
        restored <- either H.failH pure (F.applyLoaderWordPatches inversePatches restoredRecord)
        inverse <- either H.failH pure (U.rebuildContainer staged restored)
        preRecord <- H.readRegular (64 * 1024 * 1024) stagedPath
        H.ensure (inverse == preRecord) "fixed record and loader entry staging is not invertible"
        lensSafety <- case filter ((== "XM1_REQUIRES_LENS_SAFETY_CONTRACT") . F.symbolName) symbols of
          [] -> pure False
          [marker] -> do
            let offset = toInteger (F.symbolAddress marker) - toInteger (imageBase + headerBytes)
            H.ensure (F.symbolSize marker == 4 && offset >= 0 && offset + 4 <= toInteger (B.length body)) "lens safety marker leaves fixed body"
            H.ensure (U.word32At body (fromInteger offset) == 1) "invalid lens safety marker"
            pure True
          _ -> H.failH "duplicate lens safety marker"
        B.writeFile output candidate
        written <- H.readRegular (64 * 1024 * 1024) output
        H.ensure (written == candidate) "candidate readback differs"
        verified <- either H.failH pure (U.parseContainer written)
        H.ensure (all U.checkPassed (U.checks verified ++ U.carriedChecks verified)) "candidate readback checks failed"
        forM_ domains $ \domain -> do
          loader <- either (H.failH . show) pure (containedLoader (U.parsedParts verified) domain)
          B.writeFile (output ++ ".loader-" ++ domainLabel domain ++ ".bin") loader
        bodyDigest <- H.sha256 body
        candidateDigest <- H.sha256 candidate
        B.writeFile (output ++ ".json") (L.toStrict (encode (object
          [ "status" .= ("staged" :: String), "body_sha256" .= bodyDigest, "dat_sha256" .= candidateDigest
          , "body_bytes" .= B.length body, "record_bytes" .= B.length record, "build_id" .= build
          , "emulator_only" .= lensSafety, "lens_safety_contract_required" .= lensSafety
          , "publication_words" .= length publications, "loader_entry_words" .= length entries
          , "record_inverse_equal" .= True
          ])))
        putStrLn ("fixed record flash " ++ U.showHexWord recordFlash ++ ", bytes " ++ show (B.length record) ++ ", publications " ++ show (length publications) ++ ", loader entries " ++ show (length entries))
    _ -> H.failH "usage: stage-fixed-image ENCODER REBUILD DECODER SOURCE_DAT BODY_BIN SYMBOLS PUBLICATIONS_JSON LOADER_ENTRIES_JSON CARRIERS FIRST_STAGE_SYMBOLS OUTPUT_DAT"

patchImage :: FilePath -> FilePath -> FilePath -> FilePath -> [Publication] -> FilePath -> Image -> IO FilePath
patchImage encoder rebuild decoder temporary rows source image
  | null [() | Publication target _ _ _ <- rows, target == image] = pure source
  | otherwise = do
      parsed <- H.readRegular (64 * 1024 * 1024) source >>= either H.failH pure . U.parseContainer
      let parts = U.parsedParts parsed
          suffix = case image of Image4 -> "4"; Image5 -> "5"; Plain -> "plain"
          originalPath = temporary </> suffix ++ "-original.bin"
          patchedPath = temporary </> suffix ++ "-patched.bin"
          recordPath = temporary </> suffix ++ "-record.bin"
          encodedPath = temporary </> suffix ++ "-encoded.bin"
          output = temporary </> suffix ++ ".DAT"
      original <- case image of
        Plain -> U.mediaImageBytes <$> either H.failH pure (U.extractPlain parts)
        _ -> do
          let record = case image of Image4 -> U.partRecord4 parts; _ -> U.partRecord5 parts
          B.writeFile recordPath (B.map complement (U.serializeRecord record))
          run decoder [recordPath, "0", originalPath, show (U.recDecodedSize (U.recHeader record))]
          H.readRegular (16 * 1024 * 1024) originalPath
      load <- either H.failH pure (imageLoad (U.partSegments parts) image)
      patched <- either H.failH pure (applyPublications image load rows original)
      let inverse = [Publication target address new old | Publication target address old new <- rows]
      roundtrip <- either H.failH pure (applyPublications image load inverse patched)
      H.ensure (roundtrip == original) "publication staging is not invertible"
      B.writeFile patchedPath patched
      case image of
        Plain -> run rebuild ["replace-plain", source, patchedPath, output]
        _ -> do
          let window = case image of Image4 -> "0x160000"; _ -> "0x2e0000"
          run encoder [window, patchedPath, encodedPath]
          run rebuild ["replace-record" ++ suffix, "--decoder", decoder, source, encodedPath, patchedPath, output]
      pure output
