{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM)
import Data.Aeson (Value, object, toJSON, (.=))
import Data.Bits ((.&.), (.|.), complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32, Word64)
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

newtype SymbolName = SymbolName String

data Entry = LoaderInit | LoaderRequest | LoaderCancel | DecoderInit | DecoderFeed | DecoderProgress | DecoderComplete | DecoderFinish
  deriving (Bounded, Enum, Show)

entryName :: Entry -> SymbolName
entryName entry = SymbolName $ case entry of
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

retired :: [(Word32, Word32)]
retired = [(0x6841f4, 0x6848d8), (0x6ae18c, 0x6ae638)]

identity :: B.ByteString -> String
identity = sha256Hex . sha256

canonical :: FilePath -> IO B.ByteString
canonical path = do
  image <- H.readRegular 0x400000 path
  H.ensure (identity image == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") "canonical image4 differs"
  pure image
extractImage4 :: FilePath -> FilePath -> FilePath -> IO ()
extractImage4 decoder sourcePath output = do
  source <- H.readRegular (64 * 1024 * 1024) sourcePath
  H.ensure (identity source == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "canonical DAT differs"
  parsed <- either H.failH pure (U.parseContainer source)
  H.ensure (all U.checkPassed (U.checks parsed ++ U.carriedChecks parsed)) "canonical container checks differ"
  work <- getEnv "XM1_RUN_WORK"
  H.withTemporaryAt work "loading-source-" $ \temporary -> do
    let record = U.partRecord4 (U.parsedParts parsed)
        stream = temporary </> "image4-record.bin"
    B.writeFile stream (B.map complement (U.serializeRecord record))
    (timed, status, _, errors) <- H.runBounded 1800000000 1048576 decoder [stream, "0", output, show (U.recDecodedSize (U.recHeader record))] B.empty
    H.ensure (not timed && status == ExitSuccess) ("canonical image4 decode failed: " ++ C.unpack errors)
  _ <- canonical output
  pure ()


symbol :: [F.Symbol] -> SymbolName -> IO F.Symbol
symbol symbols (SymbolName name) = either H.failH pure (F.lookupSymbol symbols name)

linked :: FilePath -> FilePath -> IO (B.ByteString, [F.Symbol], Word32, Word32)
linked bodyPath symbolsPath = do
  body <- H.readRegular 0x40000 bodyPath
  symbols <- H.readRegular 0x400000 symbolsPath >>= either H.failH pure . F.parseSymbols
  start <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_body_start")
  end <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_body_end")
  bssStart <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_bss_start")
  bssEnd <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_bss_end")
  state <- symbol symbols (SymbolName "XM1_DECODER_STATE")
  H.ensure (start == imageBase + headerBytes && toInteger end == toInteger start + toInteger (B.length body)) "body symbols differ from fixed bytes"
  H.ensure (bssStart >= end && bssEnd >= bssStart && bssEnd <= imageLimit) "runtime BSS leaves owned fixed memory"
  H.ensure (F.symbolAddress state >= bssStart && F.symbolAddress state + F.symbolSize state <= bssEnd && F.symbolSize state == 52) "decoder state lost its 52-byte zero-once extent"
  mapM_ (\entry -> do
      exported <- symbol symbols (entryName entry)
      H.ensure (F.symbolAddress exported >= start && F.symbolAddress exported < end && F.symbolAddress exported .&. 3 == 0 && F.symbolSize exported > 0) "runtime entry leaves contiguous body")
    [minBound .. maxBound]
  pure (body, symbols, bssStart, bssEnd)

branch :: Word32 -> Word32 -> IO Word32
branch site target = do
  let offset = toInteger target - toInteger site - 8
  H.ensure (offset `mod` 4 == 0 && offset >= -0x2000000 && offset < 0x2000000) "entry veneer leaves A32 branch range"
  pure (0xea000000 .|. (fromInteger (offset `div` 4) .&. 0xffffff))

publications :: B.ByteString -> [F.Symbol] -> IO [Publication]
publications original symbols = do
  entries <- forM [minBound .. maxBound] $ \entry -> do
    address <- F.symbolAddress <$> symbol symbols (entryName entry)
    instruction <- branch (entryAddress entry) address
    pure (entryAddress entry, instruction)
  let word address = U.word32At original (fromIntegral (address - 0x61c400))
      loaderRows = take 3 entries
      tombstones = [(address, maybe 0xe7f000f0 id (lookup address entries)) | (low, high) <- retired, address <- [low, low + 4 .. high - 4]]
  mapM (\(address, new) -> either H.failH pure (publication address (word address) new))
    [(address, new) | (address, new) <- loaderRows ++ tombstones, word address /= new]

rowsValue :: [Publication] -> Value
rowsValue rows = toJSON [object ["address" .= address, "old" .= old, "new" .= new] | Publication _ address old new <- rows]

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["extract-image4", decoder, sourcePath, output] ->
      extractImage4 decoder sourcePath output
    ["publications", imagePath, bodyPath, symbolsPath, output] -> do
      image <- canonical imagePath
      (_, symbols, _, _) <- linked bodyPath symbolsPath
      rows <- publications image symbols
      B.writeFile output (encodeCanonical (rowsValue rows))
    ["contract", decoder, sourcePath, stagingPath, imagePath, bodyPath, symbolsPath, candidatePath, scope, ticksText, output] -> do
      ticks <- maybe (H.failH "policy ticks are not a finite Word64") pure (readMaybe ticksText :: Maybe Word64)
      H.ensure (ticks /= 0) "policy ticks must be nonzero"
      source <- H.readRegular (64 * 1024 * 1024) sourcePath
      H.ensure (identity source == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "canonical DAT differs"
      staging <- H.readRegular (64 * 1024 * 1024) stagingPath
      stagingParsed <- either H.failH pure (U.parseContainer staging)
      H.ensure (all U.checkPassed (U.checks stagingParsed ++ U.carriedChecks stagingParsed)) "staging source container checks differ"
      image <- canonical imagePath
      (body, symbols, bssStart, bssEnd) <- linked bodyPath symbolsPath
      rows <- publications image symbols
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
      H.ensure (carried == expectedRecord) "candidate fixed record differs from linked runtime and BSS"
      exports <- forM [minBound .. maxBound] $ \entry -> do
        value <- symbol symbols (entryName entry)
        let SymbolName name = entryName entry
        pure (object ["kind" .= show entry, "symbol" .= name, "veneer" .= entryAddress entry, "address" .= F.symbolAddress value, "bytes" .= F.symbolSize value])
      symbolBytes <- H.readRegular 0x400000 symbolsPath
      B.writeFile output (encodeCanonical (object
        [ "schema" .= (1 :: Int)
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
        , "decoder_irq_abi" .= object ["entry_sp_alignment" .= (4 :: Int), "rust_sp_alignment" .= (8 :: Int), "veneer_max_stack_bytes" .= (12 :: Int), "restores_original_sp_and_r4" .= True]
        , "retired" .= [[low, high] | (low, high) <- retired]
        , "publications" .= rowsValue (reservationPublications ++ rows)
        , "deadline_policy" .= object ["scope" .= scope, "ticks" .= ticks, "formed_once_per_request" .= True, "cleanup_budget" .= (0 :: Int)]
        , "drain_scope" .= if scope == "fujimod-3d90035d" then ("FujiMod-3d90035d-synchronous-copy-only" :: String) else "synthetic-external-quiescence-control"
        ]))
    _ -> H.failH "usage: runtime-artifact extract-image4 FFLZD DAT OUTPUT | publications IMAGE4 BODY SYMBOLS OUTPUT | contract FFLZD CANONICAL_DAT STAGING_DAT IMAGE4 BODY SYMBOLS CANDIDATE SCOPE TICKS OUTPUT"
