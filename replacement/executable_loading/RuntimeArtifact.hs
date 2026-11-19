{-# LANGUAGE OverloadedStrings #-}
-- | Independent contract over a staged Root bank that links the task60 loading
-- owner: canonical image4 plus exactly the published veneers, the fixed record
-- carrying the admitted body/BSS, and the explicit qualification policy.
module Main (main) where

import Control.Monad (forM, forM_)
import Data.Aeson (Value, eitherDecodeStrict', object, toJSON, withObject, (.:), (.=))
import Data.Aeson.Types (parseEither)
import Data.Bits ((.&.), (.|.), complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (sort)
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout
import Replacement.FixedImage.Record
import System.Environment (getArgs, getEnv)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import Text.Read (readMaybe)

data Entry = LoaderInit | LoaderRequest | LoaderCancel | DecoderInit | DecoderFeed | DecoderProgress | DecoderComplete | DecoderFinish
  deriving (Bounded, Enum, Show)

entryName :: Entry -> String
entryName entry = case entry of
  LoaderInit -> "xm1_loading_startup"
  LoaderRequest -> "xm1_loading_legacy_request"
  LoaderCancel -> "xm1_loading_legacy_cancel"
  DecoderInit -> "xm1_decoder_init"
  DecoderFeed -> "xm1_decoder_feed"
  DecoderProgress -> "xm1_decoder_progress"
  DecoderComplete -> "xm1_decoder_complete"
  DecoderFinish -> "xm1_decoder_finish"

entryAddress :: Entry -> Word32
entryAddress entry = case entry of
  LoaderInit -> 0x73ac74
  LoaderRequest -> 0x73ac94
  LoaderCancel -> 0x73aca4
  DecoderInit -> 0x6ae360
  DecoderFeed -> 0x6ae500
  DecoderProgress -> 0x6ae1dc
  DecoderComplete -> 0x6ae310
  DecoderFinish -> 0x6ae3f4

identity :: B.ByteString -> String
identity = sha256Hex . sha256

canonicalDat :: String
canonicalDat = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

symbol :: [F.Symbol] -> String -> IO F.Symbol
symbol symbols name = either H.failH pure (F.lookupSymbol symbols name)

linked :: FilePath -> FilePath -> IO (B.ByteString, [F.Symbol], Word32, Word32)
linked bodyPath symbolsPath = do
  body <- H.readRegular (16 * 1024 * 1024) bodyPath
  symbols <- H.readRegular (64 * 1024 * 1024) symbolsPath >>= either H.failH pure . F.parseSymbols
  start <- F.symbolAddress <$> symbol symbols "__fixed_body_start"
  end <- F.symbolAddress <$> symbol symbols "__fixed_body_end"
  bssStart <- F.symbolAddress <$> symbol symbols "__fixed_bss_start"
  bssEnd <- F.symbolAddress <$> symbol symbols "__fixed_bss_end"
  state <- symbol symbols "XM1_DECODER_STATE"
  H.ensure (start == imageBase + headerBytes && toInteger end == toInteger start + toInteger (B.length body)) "body symbols differ from fixed bytes"
  H.ensure (bssStart >= end && bssEnd >= bssStart && bssEnd <= imageLimit) "bank BSS leaves owned fixed memory"
  H.ensure (F.symbolAddress state >= bssStart && F.symbolAddress state + F.symbolSize state <= bssEnd && F.symbolSize state == 52) "decoder state lost its 52-byte zero-once extent"
  forM_ [minBound .. maxBound :: Entry] $ \entry -> do
    exported <- symbol symbols (entryName entry)
    H.ensure (F.symbolAddress exported >= start && F.symbolAddress exported < end && F.symbolAddress exported .&. 3 == 0 && F.symbolSize exported > 0) "loading/decoder entry leaves contiguous body"
  pure (body, symbols, bssStart, bssEnd)

branch :: Word32 -> Word32 -> IO Word32
branch site target = do
  let offset = toInteger target - toInteger site - 8
  H.ensure (offset `mod` 4 == 0 && offset >= -0x2000000 && offset < 0x2000000) "entry veneer leaves A32 branch range"
  pure (0xea000000 .|. (fromInteger (offset `div` 4) .&. 0xffffff))

readRows :: FilePath -> IO [Publication]
readRows path = do
  bytes <- H.readRegular (16 * 1024 * 1024) path
  values <- either H.failH pure (eitherDecodeStrict' bytes :: Either String [Value])
  forM values $ \value -> do
    (address, old, new) <- either H.failH pure $ parseEither (withObject "publication" $ \row ->
      (,,) <$> row .: "address" <*> row .: "old" <*> row .: "new") value
    either H.failH pure (publication address old new)

rowsValue :: [Publication] -> Value
rowsValue rows = toJSON [object ["address" .= address, "old" .= old, "new" .= new] | Publication _ address old new <- rows]

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["contract", decoder, sourcePath, stagingPath, imagePath, bodyPath, symbolsPath, rowsPath, candidatePath, scope, timeoutText, output] -> do
      timeout <- maybe (H.failH "stock-wrapper timeout is not a Word32 millisecond count") pure (readMaybe timeoutText :: Maybe Word32)
      H.ensure (timeout /= 0) "stock-wrapper timeout must be nonzero"
      H.ensure (scope `elem` ["fujimod-3d90035d", "synthetic-retention-control"]) "unknown qualification policy scope"
      source <- H.readRegular (64 * 1024 * 1024) sourcePath
      H.ensure (identity source == canonicalDat) "canonical DAT differs"
      staging <- H.readRegular (64 * 1024 * 1024) stagingPath
      stagingParsed <- either H.failH pure (U.parseContainer staging)
      H.ensure (all U.checkPassed (U.checks stagingParsed ++ U.carriedChecks stagingParsed)) "staging source container checks differ"
      image <- H.readRegular 0x400000 imagePath
      H.ensure (identity image == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") "canonical image4 differs"
      (body, symbols, bssStart, bssEnd) <- linked bodyPath symbolsPath
      timeoutSymbol <- symbol symbols "XM1_LOADING_LEGACY_TIMEOUT_MS"
      let timeoutOffset = toInteger (F.symbolAddress timeoutSymbol) - toInteger (imageBase + headerBytes)
      H.ensure (F.symbolSize timeoutSymbol == 4 && timeoutOffset >= 0 && timeoutOffset + 4 <= toInteger (B.length body)
        && U.word32At body (fromInteger timeoutOffset) == timeout) "declared stock-wrapper timeout is not the linked configuration"
      rows <- readRows rowsPath
      let word address = U.word32At image (fromIntegral (address - 0x61c400))
          image4Rows = [(address, old, new) | Publication Image4 address old new <- rows]
      veneers <- forM [minBound .. maxBound :: Entry] $ \entry -> do
        target <- F.symbolAddress <$> symbol symbols (entryName entry)
        new <- branch (entryAddress entry) target
        pure (entryAddress entry, word (entryAddress entry), new)
      H.ensure (sort image4Rows == sort veneers) "image4 publications are not exactly the loading and decoder veneers"
      expected <- either (H.failH . show) pure (applyPublications Image4 (LoadAddress 0x61c400) (reservationPublications ++ rows) image)
      candidate <- H.readRegular (64 * 1024 * 1024) candidatePath
      parsed <- either H.failH pure (U.parseContainer candidate)
      H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed)) "candidate container checks differ"
      work <- getEnv "XM1_RUN_WORK"
      H.withTemporaryAt work "loading-contract-" $ \temporary -> do
        let record = U.partRecord4 (U.parsedParts parsed)
            stream = temporary </> "image4-record.bin"
            decoded = temporary </> "image4.bin"
        B.writeFile stream (B.map complement (U.serializeRecord record))
        (timed, status, _, errors) <- H.runBounded 1800000000 1048576 decoder [stream, "0", decoded, show (U.recDecodedSize (U.recHeader record))] B.empty
        H.ensure (not timed && status == ExitSuccess) ("image4 decode failed: " ++ C.unpack errors)
        actual <- H.readRegular 0x400000 decoded
        H.ensure (actual == expected) "candidate image4 differs from canonical source plus exact publications"
      let parts = U.parsedParts parsed
      image5 <- case filter ((== 5) . U.segId) (U.partSegments parts) of
        [segment] -> pure segment
        _ -> H.failH "candidate lacks unique image5 segment"
      let postStart = toInteger (U.segFlash image5) + toInteger U.recordHeaderBytes + toInteger (B.length (U.recStream (U.partRecord5 parts)))
          offset = toInteger recordFlash - postStart
          recordLength = fromIntegral headerBytes + B.length body
      H.ensure (offset >= 0 && offset + toInteger recordLength <= toInteger (B.length (U.partPostHead parts))) "fixed record leaves post-image5 carrier"
      let carried = B.map complement (B.take recordLength (B.drop (fromInteger offset) (U.partPostHead parts)))
      expectedRecord <- either H.failH pure (makeRecord (BuildId (U.word32At carried 16)) (BodyBounds (imageBase + headerBytes) (imageBase + headerBytes + fromIntegral (B.length body))) (Bss bssStart bssEnd) body)
      H.ensure (carried == expectedRecord) "candidate fixed record differs from the linked bank and BSS"
      exports <- forM [minBound .. maxBound :: Entry] $ \entry -> do
        value <- symbol symbols (entryName entry)
        pure (object ["kind" .= show entry, "symbol" .= entryName entry, "veneer" .= entryAddress entry, "address" .= F.symbolAddress value, "bytes" .= F.symbolSize value])
      symbolBytes <- H.readRegular (64 * 1024 * 1024) symbolsPath
      B.writeFile output (encodeCanonical (object
        [ "schema" .= (1 :: Int)
        , "composition" .= ("root-fixed-bank" :: String)
        , "firmware_sha256" .= identity source
        , "staging_dat_sha256" .= identity staging
        , "candidate_dat_sha256" .= identity candidate
        , "candidate_image4_sha256" .= identity expected
        , "body_sha256" .= identity body
        , "symbols_sha256" .= identity symbolBytes
        , "body_bytes" .= B.length body
        , "body_start" .= (imageBase + headerBytes)
        , "bss" .= [bssStart, bssEnd]
        , "entries" .= exports
        , "publications" .= rowsValue (reservationPublications ++ rows)
        , "deadline_policy" .= object ["scope" .= scope, "timeout_ms" .= timeout, "counts_per_millisecond" .= (3000 :: Int), "timer_contract" .= ("reports/xm1-timer-interrupt-laws.md#1" :: String), "formed_once_per_request" .= True, "cleanup_budget" .= (0 :: Int)]
        , "drain_scope" .= if scope == "fujimod-3d90035d" then ("FujiMod-3d90035d-synchronous-copy-only" :: String) else "synthetic-external-quiescence-control"
        ]))
    _ -> H.failH "usage: runtime-artifact contract FFLZD CANONICAL_DAT STAGING_DAT CANONICAL_IMAGE4 FIXED_BODY SYMBOLS PUBLICATIONS_JSON CANDIDATE SCOPE TIMEOUT_MS OUTPUT"
