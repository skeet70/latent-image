{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (complement)
import Data.Word (Word32)
import Emulation.EmulatorSlot (attachGuard, reexecInSlot, slotPolicyFromEnvironment)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import Emulation.PeerStageWitness
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Runtime (..), freshRuntime, resolve, validateFirmware, workspace)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Directory (canonicalizePath, doesFileExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..))
import System.FilePath ((</>))
import System.IO (stderr)
import System.Process (showCommandForUser)

newtype Driver = Driver FilePath
newtype RunDescription = RunDescription FilePath
newtype BootRom = BootRom FilePath

data Launch = PrepareOnly | Launch Driver RunDescription BootRom

data Options
  = Consume FilePath Argument
  | Prepare FilePath FilePath FilePath Launch

usage :: String
usage = unlines
  [ "usage: peer-stage-witness --receipt FILE --argument WORD"
  , "       peer-stage-witness --firmware DAT --fflzd FFLZD --output DIR"
  , "       peer-stage-witness --firmware DAT --fflzd FFLZD --output DIR --launch --driver DRIVER --run RUN_JSON --boot-rom ROM"
  , "Preparation writes peer-witness-contract.json and peer-witness-expected.json without starting an emulator."
  , "Launch requires an explicit driver, run description and captured 16 KiB ROM."
  ]

data Flag = ReceiptFile | ArgumentWord | FirmwareFile | FflzdFile | OutputDirectory | DriverFile | RunFile | RomFile
  deriving (Eq)

parseOptions :: [String] -> Either String Options
parseOptions arguments = do
  (values, launch) <- collect [] False arguments
  let value flag = lookup flag values
      need name flag = maybe (Left (name ++ " is required\n" ++ usage)) Right (value flag)
      only allowed = unless (all ((`elem` allowed) . fst) values) (Left ("incompatible options\n" ++ usage))
  case value ReceiptFile of
    Just path -> do
      only [ReceiptFile,ArgumentWord]
      when launch (Left "receipt mode cannot launch")
      Consume path <$> (need "--argument" ArgumentWord >>= parseArgument)
    Nothing -> do
      only [FirmwareFile,FflzdFile,OutputDirectory,DriverFile,RunFile,RomFile]
      firmware <- need "--firmware" FirmwareFile
      fflzd <- need "--fflzd" FflzdFile
      output <- need "--output" OutputDirectory
      execution <- if launch
        then Launch <$> (Driver <$> need "--driver" DriverFile) <*> (RunDescription <$> need "--run" RunFile) <*> (BootRom <$> need "--boot-rom" RomFile)
        else do
          unless (all ((`notElem` [DriverFile,RunFile,RomFile]) . fst) values) (Left "driver/run/ROM require --launch")
          pure PrepareOnly
      pure (Prepare firmware fflzd output execution)
  where
    collect values launch [] = Right (values,launch)
    collect values False ("--launch":rest) = collect values True rest
    collect _ True ("--launch":_) = Left "duplicate --launch"
    collect values launch (name:entry:rest) = do
      flag <- case name of
        "--receipt" -> Right ReceiptFile
        "--argument" -> Right ArgumentWord
        "--firmware" -> Right FirmwareFile
        "--fflzd" -> Right FflzdFile
        "--output" -> Right OutputDirectory
        "--driver" -> Right DriverFile
        "--run" -> Right RunFile
        "--boot-rom" -> Right RomFile
        _ -> Left ("unknown option: " ++ name ++ "\n" ++ usage)
      when (null entry || take 2 entry == "--") (Left ("missing value for " ++ name))
      when (flag `elem` map fst values) (Left ("duplicate option: " ++ name))
      collect ((flag,entry):values) launch rest
    collect _ _ [name] = Left ("missing value for " ++ name ++ "\n" ++ usage)

maxContainer, maxImage :: Int
maxContainer = 64 * 1024 * 1024
maxImage = 16 * 1024 * 1024

segmentById :: U.Parts -> Word32 -> IO U.SegmentEntry
segmentById parts wanted = case filter ((== wanted) . U.segId) (U.partSegments parts) of
  [entry] -> pure entry
  _ -> H.failH "container segment id is absent or repeated"

mediaSlice :: B.ByteString -> Int -> Int -> Int -> IO B.ByteString
mediaSlice bytes start offset size = do
  H.ensure (start >= 0 && offset >= 0 && size >= 0 && start + offset + size <= B.length bytes) "media extent exceeds DAT"
  pure (B.map complement (B.take size (B.drop (start + offset) bytes)))

decodeImage4 :: FilePath -> FilePath -> U.Parts -> IO B.ByteString
decodeImage4 fflzd logs parts = H.withTemporary "peer-stage-fflzd" $ \directory -> do
  let record = U.partRecord4 parts
      size = fromIntegral (U.recDecodedSize (U.recHeader record))
      input = directory </> "record.bin"
      output = directory </> "image-4.bin"
  H.ensure (size > 0 && size <= maxImage) "image4 decoded size exceeds bound"
  B.writeFile input (B.map complement (U.serializeRecord record))
  (bounded,code,out,err) <- H.runBounded 120000000 (1024 * 1024) fflzd [input,"0",output,show size] B.empty
  B.writeFile (logs </> "fflzd.stdout.log") out
  B.writeFile (logs </> "fflzd.stderr.log") err
  H.ensure (not bounded && code == ExitSuccess) "fflzd failed or exceeded wall/output bounds; see fflzd logs"
  decoded <- H.readRegular maxImage output
  H.ensure (B.length decoded == size) "fflzd output length differs from record header"
  pure decoded

pinnedRecord2 :: String
pinnedRecord2 = "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"

expectedValue :: FilePath -> B.ByteString -> B.ByteString -> B.ByteString -> B.ByteString -> Argument -> IO Value
expectedValue firmware dat record2 loader image4 (Argument arg) = do
  datHash <- checkedDigest dat
  recordHash <- checkedDigest record2
  loaderHash <- checkedDigest loader
  imageHash <- checkedDigest image4
  pure (object
    [ "schema" .= (1 :: Int), "firmware" .= firmware, "firmware_sha256" .= F.sha256Hex datHash
    , "record2_sha256" .= F.sha256Hex recordHash, "loader_b_sha256" .= F.sha256Hex loaderHash
    , "image4_sha256" .= F.sha256Hex imageHash, "argument" .= arg
    , "loader_b_load" .= (0xd0000 :: Word32), "loader_b_entry" .= (0xd0020 :: Word32)
    , "argument_source" .= (0x61c40c :: Word32), "budget" .= (20000000 :: Int)
    , "cp14_identities" .= ([0x4003,0x5003] :: [Word32])
    , "stops" .= map stopName [Record2Entry,RendezvousTail,Record2Handoff,LoaderBEntry]
    , "watch_record_limit" .= (16384 :: Int)
    ])
  where
    checkedDigest = either H.failH pure . F.parseSha256 . sha256Hex

consume :: FilePath -> Argument -> IO ()
consume path expected = do
  bytes <- H.readRegular maxReceiptBytes path
  receipt <- either H.failH pure (parseReceipt bytes)
  either H.failH pure (validateReceipt expected receipt)
  putStrLn ("peer-stage receipt accepted: " ++ path)

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["--help"] -> putStr usage
    _ -> do
      options <- either H.failH pure (parseOptions args)
      root <- workspace Nothing
      case options of
        Consume path expected -> consume (resolve root path) expected
        Prepare firmwarePath fflzdPath outputPath execution -> do
          case execution of
            PrepareOnly -> pure ()
            Launch {} -> do
              policy <- slotPolicyFromEnvironment
              reexecInSlot policy args
              attachGuard args
          datPath <- canonicalizePath (resolve root firmwarePath)
          let fflzd = resolve root fflzdPath
          validateFirmware (Firmware datPath)
          dat <- H.readRegular maxContainer datPath
          parsed <- either H.failH pure (U.parseContainer dat)
          forM_ (U.checks parsed ++ U.carriedChecks parsed ++ U.stockChecks parsed) $ \check ->
            H.ensure (U.checkPassed check) ("container contract: " ++ U.checkField check)
          let parts = U.parsedParts parsed
              mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
          H.ensure (U.partVersionMajor parts == 1 && U.partVersionMinor parts == 0x50) "requires X-M1 1.50"
          record2 <- mediaSlice dat mainStart 0x50000 0x4c80
          H.ensure (sha256Hex record2 == pinnedRecord2) "record2 is not the original peer boot record"
          segment1 <- segmentById parts 1
          segment4 <- segmentById parts 4
          H.ensure (U.segLoad segment1 == 0xd0000 && U.segSize segment1 >= 0x24 && U.segSize segment1 <= 0x20000) "segment1 loader extent differs"
          H.ensure (U.segLoad segment4 == 0x61c400) "image4 load address differs from original binding"
          loader <- mediaSlice dat mainStart (fromIntegral (U.segFlash segment1)) (fromIntegral (U.segSize segment1))
          entry <- either H.failH pure (leWordAt loader 0xc)
          load <- either H.failH pure (leWordAt loader 4)
          H.ensure (entry == 0xd0020 && load == U.segLoad segment1) "loaderB header entry or load differs"
          Runtime output <- freshRuntime root (Just (Runtime outputPath)) (UniqueRuntime "peer-stage-witness")
          image4 <- decodeImage4 fflzd output parts
          argumentWord <- either H.failH pure (leWordAt image4 0xc)
          let expected = Argument argumentWord
          binding <- expectedValue datPath dat record2 loader image4 expected
          let contract = output </> "peer-witness-contract.json"
              receipt = output </> "peer-witness.json"
          BL.writeFile contract (encode contractValue)
          BL.writeFile (output </> "peer-witness-expected.json") (encode binding)
          case execution of
            PrepareOnly -> do
              putStrLn ("peer-stage preparation: " ++ output)
              putStrLn ("receipt consumer: " ++ showCommandForUser "peer-stage-witness" ["--receipt",receipt,"--argument",show argumentWord])
              putStrLn "launch requires a separate explicit --launch invocation with --driver, --run and --boot-rom"
            Launch (Driver driverPath) (RunDescription runPath) (BootRom romPath) -> do
              driver <- canonicalizePath (resolve root driverPath)
              run <- canonicalizePath (resolve root runPath)
              rom <- canonicalizePath (resolve root romPath)
              romBytes <- H.readRegular 0x4000 rom
              H.ensure (B.length romBytes == 0x4000) "captured boot ROM must be exactly 16 KiB"
              let launchArgs = ["--run",run,"--update",datPath,"--boot-rom",rom,"--output",output,"--peer-witness",contract]
              putStrLn (showCommandForUser driver launchArgs)
              (bounded,code,out,err) <- H.runBounded 120000000 (1024 * 1024) driver launchArgs B.empty
              B.writeFile (output </> "driver.stdout.log") out
              B.writeFile (output </> "driver.stderr.log") err
              B.putStr out
              B.hPut stderr err
              hasReceipt <- doesFileExist receipt
              when hasReceipt (consume receipt expected)
              H.ensure (not bounded) "peer driver exceeded wall/output bounds"
              H.ensure (code == ExitSuccess) "peer driver exited nonzero"
              H.ensure hasReceipt "peer driver did not write peer-witness.json"
