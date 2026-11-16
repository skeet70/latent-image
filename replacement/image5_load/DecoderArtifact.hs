{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM)
import Data.Aeson (Value, object, toJSON, (.=))
import Data.Bits ((.&.), (.|.), complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Emulation.Sha256 (sha256, sha256Hex)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import qualified FujiAF.UpdateContainer as U
import Replacement.FixedImage.Layout
import Replacement.FixedImage.Record
import qualified Replacement.Image5Load.Artifact as Producer
import System.Environment (getArgs, getEnv)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))
import Data.Word (Word32)

newtype SymbolName = SymbolName String

data Export = Init | Feed | Progress | Complete | Finish deriving (Show, Enum, Bounded)

name :: Export -> SymbolName
name entry = SymbolName (case entry of Init -> "xm1_decoder_init"; Feed -> "xm1_decoder_feed"; Progress -> "xm1_decoder_progress"; Complete -> "xm1_decoder_complete"; Finish -> "xm1_decoder_finish")

entryAddress :: Export -> Word32
entryAddress entry = case entry of Init -> 0x6ae360; Feed -> 0x6ae500; Progress -> 0x6ae1dc; Complete -> 0x6ae310; Finish -> 0x6ae3f4

retired :: [(Word32,Word32)]
retired = [(0x6841f4,0x6848d8),(0x6ae18c,0x6ae638)]

identity :: B.ByteString -> String
identity = sha256Hex . sha256

canonical :: FilePath -> IO B.ByteString
canonical path = do
  image <- H.readRegular 0x400000 path
  H.ensure (identity image == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") "canonical image4 differs"
  pure image

symbol :: [F.Symbol] -> SymbolName -> IO F.Symbol
symbol symbols (SymbolName value) = either H.failH pure (F.lookupSymbol symbols value)

linked :: FilePath -> FilePath -> IO (B.ByteString,[F.Symbol])
linked bodyPath symbolsPath = do
  body <- H.readRegular 0x40000 bodyPath
  symbols <- H.readRegular 0x400000 symbolsPath >>= either H.failH pure . F.parseSymbols
  start <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_body_start")
  end <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_body_end")
  first <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_bss_start")
  lastByte <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_bss_end")
  state <- symbol symbols (SymbolName "XM1_DECODER_STATE")
  H.ensure (start == imageBase+headerBytes && toInteger end == toInteger start+toInteger (B.length body)) "body symbols differ from fixed bytes"
  H.ensure (first == F.symbolAddress state && F.symbolSize state == 52 && lastByte == first+52 && first >= end && lastByte <= imageLimit) "private zero-once decoder state differs"
  mapM_ (\entry -> do
    exported <- symbol symbols (name entry)
    let address = F.symbolAddress exported
    H.ensure (address >= start && address < end && address .&. 3 == 0 && F.symbolSize exported > 0) "decoder entry leaves the contiguous fixed body") [minBound..maxBound]
  pure (body,symbols)

branch :: Word32 -> Word32 -> IO Word32
branch site target = do
  let offset = toInteger target-toInteger site-8
  H.ensure (offset `mod` 4 == 0 && offset >= -0x2000000 && offset < 0x2000000) "entry veneer leaves A32 branch range"
  pure (0xea000000 .|. (fromInteger (offset `div` 4) .&. 0xffffff))

publications :: B.ByteString -> B.ByteString -> B.ByteString -> [F.Symbol] -> IO [Publication]
publications original producer producerSymbols symbols = do
  _ <- either (H.failH . show) pure (Producer.deriveArtifact producer producerSymbols)
  entries <- forM [minBound..maxBound] $ \entry -> do
    address <- F.symbolAddress <$> symbol symbols (name entry)
    instruction <- branch (entryAddress entry) address
    pure (entryAddress entry,instruction)
  let word address = U.word32At original (fromIntegral (address-0x61c400))
      tombstones = [(address,maybe 0xe7f000f0 id (lookup address entries)) | (low,high) <- retired, address <- [low,low+4..high-4]]
      padded = producer <> B.concat (replicate ((388-B.length producer) `div` 4) (U.putWord32 0xe320f000))
      producerRows = [(0x6a96c0+fromIntegral offset,U.word32At padded offset) | offset <- [0,4..384]]
  mapM (\(address,new) -> either H.failH pure (publication address (word address) new))
    [(address,new) | (address,new) <- tombstones ++ producerRows, word address /= new]

rowsValue :: [Publication] -> Value
rowsValue rows = toJSON [object ["address" .= address,"old" .= old,"new" .= new] | Publication _ address old new <- rows]

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["script",bindingsPath] -> do
      declarations <- lines <$> readFile bindingsPath
      let (bindings,rest) = break (== "SECTIONS") declarations
      H.ensure (not (null rest)) "measurement linker bindings lack SECTIONS"
      putStr (unlines (bindings ++ ["MEMORY {",linkerMemory,"}","SECTIONS {",
        ".text : { __fixed_body_start = .; KEEP(*(.decoder.*)) *(.text .text.*) *(.rodata .rodata.*) . = ALIGN(4); __fixed_body_end = .; } > OWNED_FIXED",
        ".data : { *(.data .data.*) } > OWNED_FIXED",
        ".bss (NOLOAD) : { __fixed_bss_start = .; *(.bss .bss.*) *(COMMON) . = ALIGN(4); __fixed_bss_end = .; } > OWNED_FIXED",
        "/DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }","}",
        "ASSERT(SIZEOF(.data) == 0, \"decoder has initialized writable data\")",
        "ASSERT(__fixed_bss_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), \"decoder leaves owned fixed memory\")"]))
    ["omit-clock-release",bodyPath,symbolsPath,output,receipt] -> do
      (body,symbols) <- linked bodyPath symbolsPath
      start <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_body_start")
      let sites = [(offset,old) | offset <- [0,4..B.length body-4],
            let old = U.word32At body offset, old .&. 0xff000000 == 0xeb000000,
            let immediate = toInteger (old .&. 0xffffff),
            let displacement = if immediate >= 0x800000 then immediate-0x1000000 else immediate,
            toInteger start+toInteger offset+8+4*displacement == 0x649ddc]
      (offset,old) <- case sites of
        [site] -> pure site
        _ -> H.failH "clock-release defect needs exactly one direct retained clock-give call"
      let mutant = B.concat [B.take offset body,B.pack [0,0,160,225],B.drop (offset+4) body]
      B.writeFile output mutant
      B.writeFile receipt (encodeCanonical (object ["schema" .= (1 :: Int),
        "scope" .= ("synthetic-owned-decoder-clock-leak-control" :: String),
        "source_body_sha256" .= identity body,"candidate_body_sha256" .= identity mutant,
        "site" .= (start+fromIntegral offset),"old" .= old,"new" .= (0xe1a00000 :: Word32),
        "retained_service" .= (0x649ddc :: Word32)]))
    ["publications",imagePath,producerPath,producerSymbolsPath,bodyPath,symbolsPath,output] -> do
      image <- canonical imagePath
      producer <- H.readRegular 388 producerPath
      producerSymbols <- H.readRegular 0x10000 producerSymbolsPath
      (_,symbols) <- linked bodyPath symbolsPath
      rows <- publications image producer producerSymbols symbols
      B.writeFile output (encodeCanonical (rowsValue rows))
    ["contract",decoder,sourcePath,imagePath,producerPath,producerSymbolsPath,bodyPath,symbolsPath,candidatePath,output] -> do
      source <- H.readRegular (64*1024*1024) sourcePath
      image <- canonical imagePath
      H.ensure (identity source == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "canonical DAT differs"
      producer <- H.readRegular 388 producerPath
      producerSymbols <- H.readRegular 0x10000 producerSymbolsPath
      (body,symbols) <- linked bodyPath symbolsPath
      rows <- publications image producer producerSymbols symbols
      expected <- either H.failH pure (applyPublications Image4 (LoadAddress 0x61c400) (reservationPublications++rows) image)
      candidate <- H.readRegular (64*1024*1024) candidatePath
      parsed <- either H.failH pure (U.parseContainer candidate)
      H.ensure (all U.checkPassed (U.checks parsed++U.carriedChecks parsed)) "candidate container checks differ"
      work <- getEnv "XM1_RUN_WORK"
      H.withTemporaryAt work "decoder-contract-" $ \temporary -> do
        let record = U.partRecord4 (U.parsedParts parsed)
            stream = temporary </> "image4-record.bin"
            decoded = temporary </> "image4.bin"
        B.writeFile stream (B.map complement (U.serializeRecord record))
        (timed,status,_,errors) <- H.runBounded 1800000000 1048576 decoder [stream,"0",decoded,show (U.recDecodedSize (U.recHeader record))] B.empty
        H.ensure (not timed && status == ExitSuccess) ("image4 decode failed: " ++ C.unpack errors)
        actual <- H.readRegular 0x400000 decoded
        H.ensure (actual == expected) "candidate image4 differs from canonical source plus exact publications"
        B.writeFile (output ++ ".image4.bin") actual
      bssStart <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_bss_start")
      bssEnd <- F.symbolAddress <$> symbol symbols (SymbolName "__fixed_bss_end")
      let parts = U.parsedParts parsed
      image5 <- case filter ((== 5) . U.segId) (U.partSegments parts) of
        [segment] -> pure segment
        _ -> H.failH "candidate lacks unique image5 segment"
      let postStart = toInteger (U.segFlash image5) + toInteger U.recordHeaderBytes + toInteger (B.length (U.recStream (U.partRecord5 parts)))
          offset = toInteger recordFlash-postStart
          recordLength = fromIntegral headerBytes+B.length body
      H.ensure (offset >= 0 && offset+toInteger recordLength <= toInteger (B.length (U.partPostHead parts))) "fixed record leaves post-image5 carrier"
      let carried = B.map complement (B.take recordLength (B.drop (fromInteger offset) (U.partPostHead parts)))
      expectedRecord <- either H.failH pure (makeRecord (BuildId (U.word32At carried 16)) (BodyBounds (imageBase+headerBytes) (imageBase+headerBytes+fromIntegral (B.length body))) (Bss bssStart bssEnd) body)
      H.ensure (carried == expectedRecord) "candidate fixed record differs from linked body and zero-once BSS"
      exports <- forM [minBound..maxBound] $ \entry -> do
        value <- symbol symbols (name entry)
        let SymbolName label = name entry
        pure (object ["kind" .= show entry,"symbol" .= label,"veneer" .= entryAddress entry,"address" .= F.symbolAddress value,"bytes" .= F.symbolSize value])
      symbolBytes <- H.readRegular 0x400000 symbolsPath
      B.writeFile output (encodeCanonical (object ["schema" .= (1 :: Int),"firmware_sha256" .= identity source,
        "candidate_dat_sha256" .= identity candidate,"candidate_image4_sha256" .= identity expected,
        "body_sha256" .= identity body,"symbols_sha256" .= identity symbolBytes,"body_bytes" .= B.length body,
        "body_start" .= (imageBase+headerBytes),"bss" .= [bssStart,bssEnd],"entries" .= exports,
        "irq_abi" .= object ["entry_sp_alignment" .= (4 :: Int),"rust_sp_alignment" .= (8 :: Int),"veneer_max_stack_bytes" .= (12 :: Int),"restores_original_sp_and_r4" .= True],
        "retired" .= [[low,high] | (low,high) <- retired],"publications" .= rowsValue (reservationPublications++rows),
        "producer_sha256" .= identity producer,"producer_symbols_sha256" .= identity producerSymbols,
        "qualification" .= ("host-staged-awaiting-producer-irq-consumer-execution" :: String)]))
    _ -> H.failH "usage: image5-decoder-artifact script DECODER_LD | publications IMAGE4 PRODUCER_BIN PRODUCER_SYMBOLS BODY SYMBOLS OUTPUT | contract DECODER CANONICAL_DAT IMAGE4 PRODUCER_BIN PRODUCER_SYMBOLS BODY SYMBOLS CANDIDATE_DAT OUTPUT | omit-clock-release BODY SYMBOLS OUTPUT_BODY RECEIPT"
