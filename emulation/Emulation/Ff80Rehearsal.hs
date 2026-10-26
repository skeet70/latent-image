{-# LANGUAGE OverloadedStrings #-}
module Emulation.Ff80Rehearsal (runFujiModRehearsal) where

import Control.Exception (bracket, finally)
import Control.Monad (unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', encode, object, withObject, (.:), (.=))
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (atomicModifyIORef', modifyIORef', newIORef)
import Data.Word (Word8, Word32, Word64)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import Emulation.ViewerRuntime (require, stagedFirmwarePath, stagedRomPath)
import EmulatorUsb
import FujiModPaths (fujimodRamPushBridge, fujimodRun)
import qualified FujiAF.Broker as P
import FujiAF.StoredGate (openReplyTail)
import FujiAF.Transport
import Numeric (readHex)
import System.FilePath ((</>))

newtype ServiceSessionId = ServiceSessionId Int
newtype DescriptorHex = DescriptorHex String

data BridgeReceipt = BridgeReceipt DescriptorHex DescriptorHex (Maybe String)
instance FromJSON BridgeReceipt where
  parseJSON = withObject "FujiMod endpoint receipt" $ \fields ->
    BridgeReceipt <$> (DescriptorHex <$> fields .: "device_descriptor")
      <*> (DescriptorHex <$> fields .: "configuration_descriptor") <*> fields .: "bridge_error"

wordsLE :: [Word32] -> [Word8]
wordsLE = concatMap (\value -> [fromIntegral (value `shiftR` shift) | shift <- [0,8,16,24]])

runFujiModRehearsal :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runFujiModRehearsal runtime nand checkpoints receipt = do
  let bridgeOutput = receipt </> "bridge"
      arguments =
        [ "--run", fujimodRun, "--boot-rom", runtime </> stagedRomPath
        , "--installed-dat", runtime </> stagedFirmwarePath, "--adjustment-nand", nand
        , "--output", bridgeOutput, "--ram-push-checkpoints", checkpoints, "--fresh-boot" ]
  nandDigest <- sha256File nand
  firmwareDigest <- sha256File (runtime </> stagedFirmwarePath)
  sessionReceipt <- bracket
    (startEmulator fujimodRamPushBridge runtime arguments (\_ -> pure (Nothing, ())))
    (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
      tick <- newIORef (1000000000 :: Word64)
      let clock = TimedClock (atomicModifyIORef' tick (\n -> (n + 1000, n)))
            (\ms -> modifyIORef' tick (+ fromIntegral ms * 1000000))
          api = emulatorUsbApi emulator
      session <- newSession clock api receipt (1024 * 1024) 600000
      held <- newHeld
      flip finally (releaseHeld held api (\_ action -> action transferTimeoutMs)) $ do
        device <- acquireDevice session held
        require (vid device == 0x04cb && pid device == 0xff80) "unexpected ff80 device identity"
        let input = inputEndpoint (descriptors device)
            output = outputEndpoint (descriptors device)
            control opcode count (ServiceSessionId sid) = tx session (P.controlTransfer (P.control opcode count sid)) 12
        _ <- control 0xc0 8 (ServiceSessionId 0)
        opened <- tx session (P.BulkIn input 8) 8
        require (drop 2 opened == openReplyTail) "ff80 C0 reply tail mismatch"
        let sid = ServiceSessionId (fromIntegral (opened !! 0) + 256 * fromIntegral (opened !! 1))
            exchange frame count = do
              _ <- control 0xd0 (length frame) sid
              _ <- tx session (P.BulkOut output frame) (length frame)
              _ <- control 0xd1 count sid
              tx session (P.BulkIn input count) count
            memory function address count payload =
              exchange (wordsLE [0x200001,0,0,0,function,0,0,address,count] ++ payload)
                (if function == 2 then 36 + fromIntegral count else 36)
            close = do
              _ <- control 0xc4 2 sid
              _ <- tx session (P.BulkIn input 2) 2
              pure ()
        flip finally close $ do
          emulatorTraceSites emulator [(0x00d803cc,"debug-dispatch"),(0x00647b78,"function-3"),(0x00dc7874,"stock-query-callback")]
          identity <- exchange (wordsLE [0x150000,0,0,0,4,0,0,0]) 128
          require (take 16 identity == wordsLE [0x150000,0,0,0] && take 10 (drop 16 identity) == [88,0,45,0,77,0,49,0,0,0]) "unexpected ff80 identity reply"
          config <- exchange (wordsLE [0x1a0002,0,0,0,2,1,0,0,256]) 0x124
          require (config !! (0x24 + 0xf7) == 1) "ff80 config f7 is not 1"
          expected <- emulatorReadMemory emulator 0x015c22a0 32
          ram <- memory 2 0x015c22a0 32 []
          require (B.pack (drop 36 ram) == expected) "ff80 function-2 reply differs from guest memory"
          before <- emulatorReadMemory emulator 0x015c22d0 4
          let payload = [88,77,49,33]
          written <- memory 3 0x015c22d0 4 payload
          readback <- memory 2 0x015c22d0 4 []
          scratch <- emulatorReadMemory emulator 0x015c22d0 4
          require (drop 36 readback == payload && scratch == B.pack payload) "ff80 function-3 scratch write did not read back"
          query <- exchange (wordsLE [0x450006,0,0,0,0,0,0,0]) 32
          (_, hits) <- emulatorTraceHits emulator 0
          serviceMode <- emulatorReadMemory emulator 0x0146a0c0 4
          pure
            [ "schema" .= (1 :: Int), "scope" .= ("emulated X-M1 1.50 ordinary-boot ff80 service" :: String)
            , "firmware_sha256" .= sha256Hex firmwareDigest
            , "installed_nand" .= object ["sha256" .= sha256Hex nandDigest, "stored_f7" .= (1 :: Int)]
            , "source" .= ("stock guest controller, IRQ, USB task and service handlers" :: String)
            , "emulator_start" .= startRecord start
            , "c0" .= object ["opcode" .= ("0xc0" :: String), "reply" .= P.hexBytes opened]
            , "identity" .= object ["tag" .= (4 :: Int), "reply" .= P.hexBytes identity]
            , "config" .= object ["opcode" .= ("0x1a0002" :: String), "function" .= (2 :: Int), "window" .= (0 :: Int), "bytes" .= (256 :: Int), "reply_sha256" .= sha256Hex (sha256 (B.pack config)), "f7" .= (1 :: Int)]
            , "ram_read" .= object ["address" .= ("0x15c22a0" :: String), "bytes" .= (32 :: Int), "guest_memory_hex" .= P.hexBytes (B.unpack expected), "reply_hex" .= P.hexBytes (drop 36 ram), "sha256" .= sha256Hex (sha256 expected)]
            , "ram_write" .= object ["address" .= ("0x15c22d0" :: String), "before" .= P.hexBytes (B.unpack before), "written" .= P.hexBytes payload, "reply" .= P.hexBytes written, "readback" .= P.hexBytes (drop 36 readback)]
            , "query" .= object ["tag" .= (0 :: Int), "reply" .= P.hexBytes query]
            , "contexts" .= map traceValue hits
            , "boundary" .= object ["service_mode_hex" .= P.hexBytes (B.unpack serviceMode)] ]
  bytes <- B.readFile (bridgeOutput </> "emulator-session.json")
  BridgeReceipt deviceHex@(DescriptorHex device) configHex@(DescriptorHex configurationHex) failure <-
    either (ioError . userError) pure (eitherDecodeStrict' bytes)
  require (failure == Nothing) ("ff80 bridge failed: " ++ show failure)
  deviceBytes <- descriptorBytes deviceHex
  configurationBytes <- descriptorBytes configHex
  unless (length deviceBytes == 18 && take 4 (drop 8 deviceBytes) == [0xcb,0x04,0x80,0xff])
    (ioError (userError "unexpected ff80 device descriptor"))
  require (length configurationBytes >= 9 && length configurationBytes <= 256)
    "unexpected ff80 configuration descriptor extent"
  let declaredBytes = fromIntegral (configurationBytes !! 2) + 256 * fromIntegral (configurationBytes !! 3) :: Int
  require (declaredBytes == length configurationBytes) "ff80 configuration descriptor total differs from its bytes"
  let result = object (sessionReceipt ++
        [ "device_descriptor" .= object ["vid" .= ("0x04cb" :: String), "pid" .= ("0xff80" :: String), "reply" .= device]
        , "configuration_descriptor" .= configurationHex ])
  writePrivateBytes (receipt </> "ff80-session.json") (BL.toStrict (encode result) <> "\n")
  BL.putStr (encode result) >> putStrLn ""
  where
    traceValue hit = object
      [ "site" .= traceSite hit, "core" .= traceCore hit, "instructions" .= traceInstructions hit
      , "r0" .= traceR0 hit, "r1" .= traceR1 hit, "r2" .= traceR2 hit, "r3" .= traceR3 hit ]

descriptorBytes :: DescriptorHex -> IO [Word8]
descriptorBytes (DescriptorHex text) = either (ioError . userError) pure (go text)
  where
    go [] = Right []
    go (a : b : rest) | [(byte, "")] <- readHex [a,b] = (byte :) <$> go rest
    go _ = Left "malformed ff80 descriptor hexadecimal"
