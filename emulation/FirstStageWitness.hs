{-# LANGUAGE OverloadedStrings #-}
-- Natural-ROM first-stage witness on the pinned FujiMod driver.
module Main where

import Control.Monad (forM_)
import Data.Aeson (FromJSON (..), Value (..), eitherDecodeStrict', encode, object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Bits ((.&.))
import Data.List (find)
import qualified Data.Map.Strict as Map
import Data.Word (Word32)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Runtime (..), freshRuntime, resolve, validateFirmware, workspace)
import FujiModPaths (fujimodDriver, fujimodRun)
import FujiAF.FirstStage (Sha256, parseSha256)
import qualified FujiAF.Host as H
import Numeric (readHex, showHex)
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)
import System.Process (readCreateProcessWithExitCode, proc)

data Options = Options
  { optFirmware :: FilePath
  , optSymbols :: FilePath
  , optOutput :: Maybe FilePath
  }

data Hit = Hit
  { hitSite :: String
  , hitCpsr :: Word32
  , hitSp :: String
  , hitRegisters :: Map.Map String String
  }

instance FromJSON Hit where
  parseJSON = withObject "hit" $ \value -> do
    site <- value .: "site"
    cpsr <- value .: "cpsr"
    sp <- value .: "sp"
    registers <- value .: "registers" >>= withObject "registers" (pure . Map.fromList . map parseRegisterRow . KeyMap.toList)
    pure (Hit site cpsr sp registers)

parseRegisterRow :: (Key.Key, Value) -> (String, String)
parseRegisterRow (key, String text) = (Key.toString key, text)
parseRegisterRow (key, _) = (Key.toString key, "")

data ReadRow = ReadRow
  { rowLabel :: String
  , rowHex :: Maybe String
  , rowSha256 :: Sha256
  , rowLength :: Int
  }

instance FromJSON ReadRow where
  parseJSON = withObject "read" $ \value -> do
    label <- value .: "label"
    hexValue <- value .:? "hex"
    digestText <- value .: "sha256"
    digest <- either (fail . ("witness sha256: " ++)) pure (parseSha256 digestText)
    lengthValue <- value .: "length"
    pure (ReadRow label hexValue digest lengthValue)

data Receipt = Receipt
  { receiptResult :: String
  , receiptStops :: [Hit]
  , receiptReads :: [ReadRow]
  }

instance FromJSON Receipt where
  parseJSON = withObject "receipt" $ \value ->
    Receipt
      <$> value .: "result"
      <*> value .: "stops"
      <*> value .: "reads"

usage :: String
usage =
  "usage: first-stage-witness --firmware DAT --symbols SYMBOLS [--output DIR]"

parseOptions :: [String] -> Either String Options
parseOptions = go Nothing Nothing Nothing
  where
    go firmware symbols output ("--firmware" : value : rest) = go (Just value) symbols output rest
    go firmware symbols output ("--symbols" : value : rest) = go firmware (Just value) output rest
    go firmware symbols output ("--output" : value : rest) = go firmware symbols (Just value) rest
    go (Just firmware) (Just symbols) output [] = Right (Options firmware symbols output)
    go Nothing _ _ [] = Left ("--firmware is required\n" ++ usage)
    go _ Nothing _ [] = Left ("--symbols is required\n" ++ usage)
    go _ _ _ (unexpected : _) = Left ("unexpected argument: " ++ unexpected ++ "\n" ++ usage)

parseSymbols :: B.ByteString -> Either String [(Word32, String)]
parseSymbols bytes = mapM parseLine (filter (not . C.null) (C.lines bytes))
  where
    parseLine line =
      case C.words line of
        [address, _size, _kind, name] -> do
          value <- parseWord32 (C.unpack address)
          pure (value, C.unpack name)
        _ -> Left ("symbol row has no expected shape: " ++ C.unpack line)

parseWord32 :: String -> Either String Word32
parseWord32 text =
  case readHex (dropPrefix text) of
    [(value, "")]
      | value >= 0 && value <= 0xffffffff -> Right (fromIntegral (value :: Integer))
    _ -> Left ("value is not a 32-bit hexadecimal number: " ++ text)
  where
    dropPrefix value
      | take 2 value `elem` ["0x", "0X"] = drop 2 value
      | otherwise = value

symbol :: [(Word32, String)] -> String -> Either String Word32
symbol rows name =
  case [address | (address, symbolName) <- rows, symbolName == name] of
    [address] -> Right address
    [] -> Left ("candidate symbols lack " ++ name)
    _ -> Left ("candidate symbols repeat " ++ name)

contract :: [(Word32, String)] -> Either String Value
contract rows = do
  entry <- symbol rows "xm1_first_stage_entry"
  storage <- symbol rows "xm1_retained_storage_init"
  trailer <- symbol rows "xm1_retained_trailer_check"
  stage <- symbol rows "xm1_retained_stage_images"
  handoff <- symbol rows "xm1_first_stage_handoff_vector"
  pure
    ( object
        [ "schema" .= (1 :: Int)
        , "budget" .= (20000000 :: Int)
        , "stops"
            .= [ object ["label" .= label, "address" .= address]
               | (label, address) <-
                   [ ("owned-first-stage-entry", entry)
                   , ("retained-storage-init", storage)
                   , ("retained-trailer-gate", trailer)
                   , ("retained-stage-images", stage)
                   , ("owned-handoff-vector", handoff)
                   ]
               ]
        , "reads"
            .= [ object ["label" .= label, "address" .= address, "length" .= lengthValue]
               | (label, address, lengthValue) <-
                   [ ("handoff-words", 0x4300, 8)
                   , ("skip-list", 0x4310, 0x4f0)
                   , ("loader-a", 0x40000, 0x20000)
                   , ("loader-b", 0xd0000, 0x20000)
                   , ("segment-table", 0x61c000, 0x400)
                   , ("image-4", 0x61c400, 0x282800)
                   , ("image-4-bss", 0x89ec00, 0x352e00)
                   , ("image-4-arg", 0x61c40c, 4)
                   , ("image-4-publication", 0x61c188, 4)
                   , ("peer-cells", 0x40020000, 8)
                   , ("retained-event-word", 0x400a00c4, 4)
                   ]
               ]
        ]
    )

hexWord :: String -> Int -> Either String Word32
hexWord text offset =
  case readHex (take 8 (drop offset text)) of
    [(value, "")]
      | value >= 0 && value <= 0xffffffff -> Right (fromIntegral (value :: Integer))
    _ -> Left ("witness hex is not a word: " ++ text)

formatWord :: Word32 -> String
formatWord value = "0x" ++ showHex value ""

readRow :: [ReadRow] -> String -> Either String ReadRow
readRow rows label =
  maybe (Left ("witness read missing: " ++ label)) Right (find ((== label) . rowLabel) rows)

readHexField :: [ReadRow] -> String -> Either String String
readHexField rows label = do
  row <- readRow rows label
  maybe (Left ("witness read lacks hex: " ++ label)) Right (rowHex row)

readDigest :: [ReadRow] -> String -> Either String Sha256
readDigest rows label = rowSha256 <$> readRow rows label

hit :: [Hit] -> String -> Either String Hit
hit hits label =
  maybe (Left ("witness stop missing: " ++ label)) Right (find ((== label) . hitSite) hits)

register :: Hit -> String -> Either String String
register hitValue name =
  maybe (Left ("witness stop lacks register " ++ name)) Right (Map.lookup name (hitRegisters hitValue))

validate :: Receipt -> IO ()
validate receipt = do
  H.ensure (receiptResult receipt == "passed") "witness result is not passed"
  let hits = receiptStops receipt
      rows = receiptReads receipt
      check :: String -> Either String a -> IO a
      check label action = either (\message -> H.failH (label ++ ": " ++ message)) pure action
  entryHit <- check "entry stop" (hit hits "owned-first-stage-entry")
  H.ensure (hitSp entryHit == "0xfff00f00") "owned entry did not see the ROM handoff stack"
  H.ensure (hitCpsr entryHit .&. 0x1f == 0x13) "owned entry was not in SVC"
  H.ensure (hitCpsr entryHit .&. 0xc0 == 0xc0) "owned entry did not see IF set"
  entryR0 <- check "entry r0" (register entryHit "r0")
  entryR1 <- check "entry r1" (register entryHit "r1")
  H.ensure (entryR0 == "0x0") "owned entry r0 is not the ROM handoff 0"
  H.ensure (entryR1 == "0x2") "owned entry r1 is not the ROM handoff 2"
  stageHit <- check "stage stop" (hit hits "retained-stage-images")
  stageR0 <- check "stage r0" (register stageHit "r0")
  stageR1 <- check "stage r1" (register stageHit "r1")
  H.ensure (stageR0 == "0x0") "stage entry did not receive status 0"
  H.ensure (stageR1 == "0x0") "stage entry did not receive core 0"
  handoffHit <- check "handoff stop" (hit hits "owned-handoff-vector")
  H.ensure (hitCpsr handoffHit .&. 0x1f == 0x13) "handoff vector was not in SVC"
  wordsHex <- check "handoff words" (readHexField rows "handoff-words")
  target <- check "handoff target" (hexWord wordsHex 0)
  argument <- check "handoff argument" (hexWord wordsHex 8)
  H.ensure (target >= 0x40000 && target < 0x40000 + 0xdae0) "handoff target is not in loader A"
  H.ensure (target .&. 3 == 0) "handoff target is not word aligned"
  imageArgText <- check "image 4 argument" (readHexField rows "image-4-arg")
  imageArg <- check "image 4 argument word" (hexWord imageArgText 0)
  H.ensure (argument == imageArg) "handoff argument differs from 0x61c40c"
  handoffR0 <- check "handoff r0" (register handoffHit "r0")
  handoffR2 <- check "handoff r2" (register handoffHit "r2")
  handoffR3 <- check "handoff r3" (register handoffHit "r3")
  H.ensure (handoffR0 == formatWord argument) "handoff r0 differs from the argument"
  H.ensure (handoffR2 == formatWord target) "handoff r2 differs from the target"
  H.ensure (handoffR3 == "0x4300") "handoff r3 is not the 0x4300 word address"
  peerHex <- check "peer cells" (readHexField rows "peer-cells")
  H.ensure (peerHex == "0000000000000000") "peer cells did not settle to zero"
  bssRow <- check "bss read" (readRow rows "image-4-bss")
  H.ensure (rowLength bssRow == 0x352e00) "image-4 bss length differs"
  bssDigestText <- H.sha256 (B.replicate 0x352e00 0)
  bssDigest <- either H.failH pure (parseSha256 bssDigestText)
  H.ensure (rowSha256 bssRow == bssDigest) "image-4 bss is not zero-filled"
  forM_ ["loader-a", "loader-b", "image-4", "skip-list", "segment-table", "image-4-publication"] $ \label -> do
    _ <- check ("read " ++ label) (readDigest rows label)
    pure ()

main :: IO ()
main = do
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  options <- either H.failH pure (parseOptions arguments)
  root <- workspace Nothing
  let dat = resolve root (optFirmware options)
      symbolsPath = resolve root (optSymbols options)
      rom = resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
  validateFirmware (Firmware dat)
  symbolBytes <- H.readRegular 0x10000 symbolsPath
  rows <- either H.failH pure (parseSymbols symbolBytes)
  witnessContract <- either H.failH pure (contract rows)
  runtime <- freshRuntime root (Runtime <$> optOutput options) (UniqueRuntime "first-stage-witness")
  let Runtime directory = runtime
      contractPath = directory </> "first-stage-witness-contract.json"
  createDirectoryIfMissing True directory
  BL.writeFile contractPath (encode witnessContract)
  (code, out, err) <- readCreateProcessWithExitCode
    (proc fujimodDriver
      [ "--run", fujimodRun
      , "--update", dat
      , "--boot-rom", rom
      , "--output", directory
      , "--witness", contractPath
      ])
    ""
  putStr out
  hPutStrLn stderr err
  H.ensure (code == ExitSuccess) "FujiMod witness run exited nonzero"
  receiptBytes <- H.readRegular (2 * 1024 * 1024) (directory </> "first-stage-witness.json")
  receipt <- either H.failH pure (eitherDecodeStrict' receiptBytes)
  validate receipt
  putStrLn ("first-stage witness passed: " ++ directory </> "first-stage-witness.json")
