{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (SomeException, bracket, displayException, try)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (FromJSON (..), Value (..), object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither)
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef
import Data.List (sortOn)
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.PropertyPairPlan (Instructions (..), PhysicalRange (..), maxIdle, preKeyIdle, settingsRanges)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256, sha256File, sha256Hex)
import EmulatorUsb
import Numeric (showHex)
import System.Directory (createDirectory, doesPathExist, makeAbsolute)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (..), IOMode (..), hFlush, hSetBuffering, stdout, withBinaryFile)
import Text.Read (readMaybe)

data Binding = Binding { bindingPath :: FilePath, bindingDigest :: Sha256 }
data Inputs = Inputs { update :: Binding, runDocument :: Binding, rom :: Binding, adjustment :: Binding, bridge :: Binding }
data Source = Source { sourceInputs :: Inputs, sourceSpecDigest :: Sha256 }
newtype RowLimit = RowLimit Int
data Spec = Spec { writes :: [PhysicalRange], readRanges :: [PhysicalRange], fetches :: [PhysicalRange], limit :: RowLimit }
data Options = Options { sourceDirectory :: FilePath, perturbation :: Instructions, outputDirectory :: FilePath }
newtype HitCursor = HitCursor Int
newtype LogicalBlock = LogicalBlock Word32
newtype PhysicalBlock = PhysicalBlock Word32

instance FromJSON Binding where
  parseJSON = withObject "input binding" $ \o -> Binding <$> o .: "path" <*> (o .: "sha256" >>= either fail pure . parseSha256Hex)

instance FromJSON Inputs where
  parseJSON = withObject "erased stock inputs" $ \o -> do
    seeded <- o .:? "record-nand" :: Parser (Maybe Value)
    unless (seeded == Nothing) (fail "timing control requires erased record NAND")
    Inputs <$> o .: "update" <*> o .: "run" <*> o .: "boot-rom" <*> o .: "adjustment-nand" <*> o .: "bridge"

instance FromJSON Source where
  parseJSON = withObject "completed stock receipt" $ \o -> do
    role <- o .: "role" :: Parser T.Text
    unless (role == "stock") (fail "source role must be stock")
    outcome <- o .: "outcome"
    status <- outcome .: "status" :: Parser T.Text
    unless (status == "completed") (fail "source observation must be completed")
    inputs <- o .: "inputs"
    files <- o .: "files"
    specDigest <- files .: "spec.json" >>= either fail pure . parseSha256Hex
    pure (Source inputs specDigest)

instance FromJSON Spec where
  parseJSON = withObject "reset observation spec" $ \o -> do
    writeRanges <- o .: "write_ranges" >>= ranges
    capturedReads <- o .: "read_ranges" >>= ranges
    fetchRanges <- o .: "fetch_ranges" >>= ranges
    rowLimit <- o .: "watch_limit"
    unless (rowLimit > 0) (fail "watch limit must be positive")
    pure (Spec writeRanges capturedReads fetchRanges (RowLimit rowLimit))
   where
    ranges value = do
      pairs <- A.parseJSON value
      forM pairs $ \pair -> case pair of
        [start,end] | start < end -> pure (PhysicalRange start end)
        _ -> fail "invalid watch range"

need :: Bool -> String -> IO ()
need condition message = unless condition (ioError (userError message))

field :: T.Text -> Value -> Value
field name (Object fields) = maybe Null id (KM.lookup (K.fromText name) fields)
field _ _ = Null

json :: FromJSON a => B.ByteString -> IO a
json = either (ioError . userError) pure . A.eitherDecodeStrict'

writeJson :: FilePath -> Value -> IO ()
writeJson path value = BL.writeFile path (A.encode value <> "\n")

hex :: B.ByteString -> T.Text
hex = T.pack . concatMap (\byte -> let digits = showHex byte "" in replicate (2-length digits) '0' ++ digits) . B.unpack

bindings :: Inputs -> [(T.Text, Binding)]
bindings inputs = [("update",update inputs),("run",runDocument inputs),("boot-rom",rom inputs),("adjustment-nand",adjustment inputs),("bridge",bridge inputs)]

bindingValue :: Binding -> Value
bindingValue binding = object ["path" .= bindingPath binding,"sha256" .= sha256Hex (bindingDigest binding)]

parseOptions :: [String] -> IO Options
parseOptions [source,delta,output] = case readMaybe delta :: Maybe Integer of
  Just count | count >= 0 && count <= toInteger (instructionsValue maxIdle) -> Options <$> makeAbsolute source <*> pure (Instructions (fromInteger count)) <*> makeAbsolute output
  _ -> ioError (userError "DELTA must be an integer in [0,90000000]")
parseOptions _ = ioError (userError "usage: record-store-timing-control COMPLETED_ERASED_STOCK_DIR DELTA NEW_OUTPUT_DIR")

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  options <- getArgs >>= parseOptions
  exists <- doesPathExist (outputDirectory options)
  need (not exists) "output directory must be new"
  createDirectory (outputDirectory options)
  result <- try (prepare options)
  case result of
    Right () -> pure ()
    Left failure -> do
      writeJson (outputDirectory options </> "failure.json") (object ["error" .= displayException (failure :: SomeException)])
      putStrLn (displayException failure)
      exitFailure

prepare :: Options -> IO ()
prepare options = do
  receiptBytes <- B.readFile (sourceDirectory options </> "receipt.json")
  source <- json receiptBytes
  let inputs = sourceInputs source
      directory = outputDirectory options
  need (sha256Hex (bindingDigest (update inputs)) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "source is not canonical X-M1 1.50"
  forM_ (bindings inputs) $ \(_,binding) -> do
    digest <- sha256File (bindingPath binding)
    need (digest == bindingDigest binding) ("input checksum differs: " ++ bindingPath binding)
  specBytes <- B.readFile (sourceDirectory options </> "spec.json")
  need (sha256 specBytes == sourceSpecDigest source) "source spec checksum differs"
  spec <- json specBytes
  B.writeFile (directory </> "spec.json") specBytes
  B.writeFile (directory </> "source-receipt.json") receiptBytes
  executable <- getExecutablePath
  executableDigest <- sha256File executable
  let provenance = object
        ["source_receipt" .= object ["path" .= (sourceDirectory options </> "receipt.json"),"sha256" .= sha256Hex (sha256 receiptBytes)]
        ,"inputs" .= object [K.fromText name .= bindingValue binding | (name,binding) <- bindings inputs]
        ,"observer" .= object ["path" .= executable,"sha256" .= sha256Hex executableDigest]
        ,"extra_before_final_up" .= instructionsValue (perturbation options),"pre_key_idle" .= instructionsValue preKeyIdle
        ,"keys" .= map crawlKeyWire keys]
  writeJson (directory </> "inputs.json") provenance
  result <- try (observe options inputs spec)
  hashes <- forM ["inputs.json","source-receipt.json","spec.json","reset-observation.json","emulator-session.json","hits.jsonl","steps.json","medium.json","final-readouts.json"] $ \name -> do
    present <- doesPathExist (directory </> name)
    digest <- if present then Just . sha256Hex <$> sha256File (directory </> name) else pure Nothing
    pure (K.fromString name .= digest)
  let outcome = case result of
        Right () -> object ["status" .= ("completed" :: T.Text)]
        Left failure -> object ["status" .= ("refused" :: T.Text),"error" .= displayException (failure :: SomeException)]
  writeJson (directory </> "receipt.json") (object ["schema" .= (1 :: Int),"role" .= ("stock-timing-control" :: T.Text),"provenance" .= provenance,"files" .= object hashes,"outcome" .= outcome])
  either (ioError . userError . displayException) pure result

keys :: [CrawlKey]
keys = [KeyOk,KeyDown,KeyOk,KeyDown,KeyUp]

readouts :: Emulator -> [PhysicalRange] -> IO Value
readouts emulator ranges = A.toJSON <$> forM ranges (\range -> do
  bytes <- emulatorReadMemory emulator (rangeStart range) (fromIntegral (rangeEnd range - rangeStart range))
  pure (object ["start" .= rangeStart range,"bytes" .= hex bytes]))

medium :: Emulator -> IO Value
medium emulator = A.toJSON <$> forM (map LogicalBlock ([0x52..0x59] ++ [0x68,0x69])) (\(LogicalBlock logical) -> do
  bytes <- emulatorReadMemory emulator (0x95daf0 + 2*logical) 2
  need (B.length bytes == 2) "short logical mapping read"
  let mapped = fromIntegral (B.index bytes 0) .|. (fromIntegral (B.index bytes 1) `shiftL` 8) :: Word32
  if mapped == 0xffff then pure (object ["logical" .= logical,"physical" .= Null,"data" .= T.replicate 131072 "ff","spare" .= Null])
  else do
    let PhysicalBlock physical = PhysicalBlock (0x380 + mapped)
    reply <- emulatorBridgeCommand emulator (object ["command" .= ("nand-read" :: T.Text),"first" .= (physical*64),"count" .= (64 :: Int)])
    pure (object ["logical" .= logical,"physical" .= physical,"data" .= field "data" reply,"spare" .= field "spare" reply]))

watchRequest :: Spec -> Value
watchRequest spec = object
  ["command" .= ("watch-begin" :: T.Text),"views" .= ("fixed" :: T.Text),"limit" .= rowLimit
  ,"ranges" .= encodeRanges merged,"write_ranges" .= encodeRanges (writes spec),"read_ranges" .= encodeRanges (readRanges spec),"fetch_ranges" .= encodeRanges (fetches spec)]
 where
  RowLimit rowLimit = limit spec
  encodeRanges ranges = [[rangeStart range,rangeEnd range] | range <- ranges]
  merged = foldr merge [] (sortOn rangeStart (writes spec ++ readRanges spec ++ fetches spec))
  merge range [] = [range]
  merge range (next:rest)
    | rangeEnd range >= rangeStart next = merge (PhysicalRange (rangeStart range) (max (rangeEnd range) (rangeEnd next))) rest
    | otherwise = range:next:rest

observe :: Options -> Inputs -> Spec -> IO ()
observe options inputs spec = do
  cursor <- newIORef (HitCursor 0)
  steps <- newIORef []
  let directory = outputDirectory options
      arguments = ["--run",bindingPath (runDocument inputs),"--boot-rom",bindingPath (rom inputs),"--installed-dat",bindingPath (update inputs)
        ,"--adjustment-nand",bindingPath (adjustment inputs),"--output",directory,"--ram-push-checkpoints",directory </> "unused-checkpoints"
        ,"--fresh-boot","--reset-observation",directory </> "spec.json"]
  withBinaryFile (directory </> "hits.jsonl") WriteMode $ \hits -> do
    let drain emulator = do
          HitCursor first <- readIORef cursor
          reply <- emulatorTraceHitsPage emulator first 65536
          rows <- either (ioError . userError) pure (parseEither (withObject "trace page" (.: "hits")) reply :: Either String (V.Vector Value))
          forM_ rows (\row -> BL.hPut hits (A.encode row <> "\n"))
          let end = first + V.length rows
          writeIORef cursor (HitCursor end)
          hFlush hits
          case field "count" reply of Number total | end < truncate total -> drain emulator; _ -> pure end
        stop (emulator,_,()) = stopEmulator emulator
    bracket (startEmulator (bindingPath (bridge inputs)) directory arguments (\_ -> pure (Nothing,()))) stop $ \(emulator,start,()) -> do
      need (bootStart start == FreshBootStart) "bridge did not fresh boot"
      writeJson (directory </> "start.json") (startRecord start)
      _ <- drain emulator
      forM_ (zip [1 :: Int ..] keys) $ \(step,key) -> do
        before <- readouts emulator settingsRanges
        HitCursor first <- readIORef cursor
        _ <- emulatorBridgeCommand emulator (watchRequest spec)
        extra <- if step == length keys && instructionsValue (perturbation options) > 0 then emulatorIdle emulator (perturbation options) else pure Null
        idle <- emulatorIdle emulator preKeyIdle
        press <- emulatorDisplayKey emulator key
        observed <- emulatorWatchEnd emulator
        lastHit <- drain emulator
        after <- readouts emulator settingsRanges
        modifyIORef' steps (object ["step" .= step,"parent" .= (step-1),"keys" .= map crawlKeyWire (take step keys),"hits" .= [first,lastHit]
          ,"perturbation_idle" .= extra,"idle" .= idle,"press" .= press,"watch" .= observed,"regions_before" .= before,"regions_after" .= after] :)
        writeJson (directory </> "steps.json") . A.toJSON . reverse =<< readIORef steps
        need (field "failure" press == Null) ("key failed: " ++ show press)
        putStrLn ("step " ++ show step ++ " " ++ crawlKeyWire key)
      writeJson (directory </> "medium.json") =<< medium emulator
      writeJson (directory </> "final-readouts.json") =<< readouts emulator (writes spec)
