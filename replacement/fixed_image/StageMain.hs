{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (foldM)
import Data.Aeson (FromJSON(..), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout
import Replacement.FixedImage.Record
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
    [encoder, rebuild, decoder, source, bodyPath, symbolsPath, rowsPath, firstSymbolsPath, output] -> do
      body <- H.readRegular 0x40000 bodyPath
      symbols <- H.readRegular 0x400000 symbolsPath >>= either H.failH pure . F.parseSymbols
      begin <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_bss_start")
      end <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_bss_end")
      bodyStart <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_body_start")
      bodyEnd <- F.symbolAddress <$> either H.failH pure (F.lookupSymbol symbols "__fixed_body_end")
      firstSymbols <- H.readRegular 0x400000 firstSymbolsPath >>= either H.failH pure . F.parseSymbols
      buildSymbol <- either H.failH pure (F.lookupSymbol firstSymbols "XM1_FIXED_IMAGE_BUILD_ID")
      let buildOffset = F.firstRecordSlotOffset + fromIntegral (F.symbolAddress buildSymbol)
      H.ensure (F.symbolSize buildSymbol == 4 && F.symbolAddress buildSymbol >= 0x6000 && F.symbolAddress buildSymbol < 0x10000) "build id symbol leaves first stage"
      rows <- H.readRegular 0x100000 rowsPath >>= either H.failH pure . eitherDecodeStrict'
      let Rows requested = rows
          publications = reservationPublications ++ requested
          build = crc32 (body <> B.concat [B.concat (map U.putWord32 [address, old, new]) | Publication _ address old new <- publications])
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
        changed <- either H.failH pure (stageRecord record (U.parsedParts staged))
        candidate <- either H.failH pure (U.rebuildContainer staged changed)
        restored <- either H.failH pure (restoreRecord (U.parsedParts staged) changed)
        inverse <- either H.failH pure (U.rebuildContainer staged restored)
        preRecord <- H.readRegular (64 * 1024 * 1024) stagedPath
        H.ensure (inverse == preRecord) "fixed record staging is not invertible"
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
        bodyDigest <- H.sha256 body
        candidateDigest <- H.sha256 candidate
        B.writeFile (output ++ ".json") (L.toStrict (encode (object
          [ "status" .= ("staged" :: String), "body_sha256" .= bodyDigest, "dat_sha256" .= candidateDigest
          , "body_bytes" .= B.length body, "record_bytes" .= B.length record, "build_id" .= build
          , "emulator_only" .= lensSafety, "lens_safety_contract_required" .= lensSafety
          , "publication_words" .= length publications, "record_inverse_equal" .= True
          ])))
        putStrLn ("fixed record flash " ++ U.showHexWord recordFlash ++ ", bytes " ++ show (B.length record) ++ ", publications " ++ show (length publications))
    _ -> H.failH "usage: stage-fixed-image ENCODER REBUILD DECODER SOURCE_DAT BODY_BIN SYMBOLS PUBLICATIONS_JSON FIRST_STAGE_SYMBOLS OUTPUT_DAT"

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
      patched <- either H.failH pure (applyPublications image rows original)
      let inverse = [Publication target address new old | Publication target address old new <- rows]
      roundtrip <- either H.failH pure (applyPublications image inverse patched)
      H.ensure (roundtrip == original) "publication staging is not invertible"
      B.writeFile patchedPath patched
      case image of
        Plain -> run rebuild ["replace-plain", source, patchedPath, output]
        _ -> do
          let window = case image of Image4 -> "0x160000"; _ -> "0x2e0000"
          run encoder [window, patchedPath, encodedPath]
          run rebuild ["replace-record" ++ suffix, "--decoder", decoder, source, encodedPath, patchedPath, output]
      pure output
