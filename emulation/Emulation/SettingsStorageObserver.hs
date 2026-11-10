{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.SettingsStorageObserver
  ( StorageObserverInputs(..), ObserverNand(..), runStorageMatrix, readSettingsRegions, observeBoundedPrefix ) where

import Control.Exception (SomeException, throwIO, try, finally)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value(..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftL, (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32)
import Emulation.PropertyPairPlan
import Emulation.SettingsStorageMatrix
import Emulation.Sha256 (Sha256, sha256File, sha256Hex, parseSha256Hex)
import qualified Emulation.WatchObservation as W
import EmulatorUsb
import Numeric (showHex)
import System.Directory (createDirectory, makeAbsolute, getFileSize)
import System.FilePath ((</>))
import System.IO (Handle, IOMode(..), hFlush, withBinaryFile)
import Text.Printf (printf)

data ObserverNand = AdjustmentSeed FilePath | CommittedSeed FilePath deriving (Eq, Show)
data StorageObserverInputs = StorageObserverInputs
  { storageBridge :: FilePath
  , storageRun :: FilePath
  , storageRom :: FilePath
  , storageDat :: FilePath
  , storageInitialNand :: ObserverNand
  , storageSpec :: FilePath
  }
newtype SessionNumber = SessionNumber Int deriving (Eq, Show)
newtype TraceCursor = TraceCursor Int deriving (Eq, Show)
newtype BankBytes = BankBytes B.ByteString
newtype RecordBytes = RecordBytes B.ByteString
newtype ScannerRowBytes = ScannerRowBytes B.ByteString
newtype ModeClassWord = ModeClassWord Word32
newtype InputBitmap = InputBitmap Word32
newtype ControllerStateWord = ControllerStateWord Word16
newtype ControllerEventWord = ControllerEventWord Word16

data StorageState = StorageState
  { bank0 :: BankBytes, bank1 :: BankBytes, previous :: BankBytes, backing :: BankBytes
  , customPrevious :: RecordBytes, customWorking :: RecordBytes
  , modeClass :: ModeClassWord, scannerRow :: ScannerRowBytes, inputBitmap :: InputBitmap
  , controller30State :: ControllerStateWord, controller30Event :: ControllerEventWord, controller74State :: ControllerStateWord
  }
data ActiveSession = ActiveSession
  { activeNumber :: SessionNumber, activeEmulator :: Emulator, activeDirectory :: FilePath
  , activeStart :: EmulatorStart, activeLocalCursor :: IORef TraceCursor, activeHitStart :: TraceCursor
  , activeInputCommitted :: Maybe Sha256
  }

die :: String -> IO a
die = ioError . userError

field :: K.Key -> Value -> IO Value
field key (Object fields) = maybe (die ("missing lifecycle evidence field " ++ show key)) pure (KM.lookup key fields)
field _ _ = die "lifecycle evidence is not an object"

text :: Value -> IO T.Text
text (String value) = pure value
text _ = die "lifecycle evidence is not text"

number :: A.FromJSON value => Value -> IO value
number value = case A.fromJSON value of A.Success parsed -> pure parsed; A.Error message -> die message

jsonFile :: FilePath -> IO Value
jsonFile path = B.readFile path >>= either die pure . A.eitherDecodeStrict'

hexBytes :: B.ByteString -> T.Text
hexBytes = T.pack . concatMap (\byte -> let value = showHex byte "" in replicate (2-length value) '0' ++ value) . B.unpack

readExact :: Emulator -> Word32 -> Int -> IO B.ByteString
readExact emulator address count = do
  bytes <- emulatorReadMemory emulator address count
  unless (B.length bytes == count) (die "lifecycle memory observation is short")
  pure bytes

readSettingsRegions :: Emulator -> IO Value
readSettingsRegions emulator = A.toJSON <$> forM settingsRanges (\range -> do
  bytes <- readExact emulator (rangeStart range) (fromIntegral (rangeEnd range-rangeStart range))
  pure (object ["start" .= rangeStart range,"bytes" .= hexBytes bytes]))

readStorageState :: Emulator -> IO StorageState
readStorageState emulator = StorageState
  <$> image 0x96cc64 <*> image 0x96ce26 <*> image 0x89ec00 <*> image 0x9c281c
  <*> record 0x89f838 <*> record 0x89f874
  <*> (ModeClassWord <$> word 0x951564) <*> (ScannerRowBytes <$> readExact emulator 0x8284bc 28)
  <*> (InputBitmap <$> word 0x94f8c4)
  <*> (ControllerStateWord <$> half 0x14675c4) <*> (ControllerEventWord <$> half 0x1467720) <*> (ControllerStateWord <$> half 0x146761c)
  where
    image address = BankBytes <$> readExact emulator address 0x1c2
    record address = RecordBytes <$> readExact emulator address 52
    word address = (`word32At` 0) <$> readExact emulator address 4
    half address = do
      bytes <- readExact emulator address 2
      pure (fromIntegral (B.index bytes 0) .|. fromIntegral (B.index bytes 1) `shiftL` 8)

stateValue :: StorageState -> Value
stateValue state = object
  [ "bank0" .= image (bank0 state),"bank1" .= image (bank1 state),"previous" .= image (previous state),"backing" .= image (backing state)
  , "custom_previous" .= record (customPrevious state),"custom_working" .= record (customWorking state)
  , "mode_class" .= (case modeClass state of ModeClassWord value -> value)
  , "c_scanner_row" .= (case scannerRow state of ScannerRowBytes bytes -> hexBytes bytes)
  , "c_input_bitmap" .= (case inputBitmap state of InputBitmap value -> value)
  , "controller30_state" .= (case controller30State state of ControllerStateWord value -> value)
  , "controller30_event" .= (case controller30Event state of ControllerEventWord value -> value)
  , "controller74_state" .= (case controller74State state of ControllerStateWord value -> value) ]
  where image (BankBytes bytes) = hexBytes bytes; record (RecordBytes bytes) = hexBytes bytes

observeBoundedPrefix :: Emulator -> IO Value
observeBoundedPrefix emulator = do
  elapsed <- emulatorIdleTicks emulator postSettle
  ticks <- field "ticks" elapsed >>= number
  unless (ticks == tickValue postSettle) (die "post-settle prefix missed its exact tick boundary")
  state <- emulatorUiState emulator
  first <- field "instructions" elapsed
  second <- field "instructions" state
  unless (first == second) (die "UI snapshot advanced beyond the prefix boundary")
  pure (object ["elapsed" .= elapsed,"state" .= state])

cursorValue :: TraceCursor -> Int
cursorValue (TraceCursor value) = value
sessionValue :: SessionNumber -> Int
sessionValue (SessionNumber value) = value

runStorageMatrix :: StorageObserverInputs -> FilePath -> StorageMatrix -> (K.Key -> Value -> IO ()) -> IO ()
runStorageMatrix inputs output matrix record = withBinaryFile (output </> "hits.jsonl") WriteMode $ \hitsFile -> do
  active <- newIORef Nothing
  global <- newIORef (TraceCursor 0)
  sessions <- newIORef []
  steps <- newIORef []
  let drain current = drainSession hitsFile global current
      close = do
        current <- readIORef active
        case current of
          Nothing -> pure ()
          Just session -> do
            writeIORef active Nothing
            _ <- drain session `finally` stopEmulator (activeEmulator session)
            sdk <- jsonFile (activeDirectory session </> "emulator-session.json")
            finish <- readIORef global
            let manifest = object
                  [ "session" .= sessionValue (activeNumber session),"hit_start" .= cursorValue (activeHitStart session),"hit_end" .= cursorValue finish
                  , "sdk" .= sdk,"start" .= startRecord (activeStart session),"input_committed_sha256" .= fmap sha256Hex (activeInputCommitted session) ]
            modifyIORef' sessions (manifest:)
      start ordinal seed = do
        let directory = output </> printf "boot-%04d" (sessionValue ordinal)
        createDirectory directory
        path <- makeAbsolute (case seed of AdjustmentSeed supplied -> supplied; CommittedSeed supplied -> supplied)
        consumed <- sha256File path
        let medium = case seed of AdjustmentSeed _ -> ["--adjustment-nand",path]; CommittedSeed _ -> ["--committed-nand",path]
            arguments = ["--run",storageRun inputs,"--boot-rom",storageRom inputs,"--installed-dat",storageDat inputs]
              ++ medium ++ ["--output",directory,"--ram-push-checkpoints",directory </> "unused-checkpoints","--fresh-boot","--reset-observation",storageSpec inputs]
        hitStart <- readIORef global
        (emulator,started,()) <- startEmulator (storageBridge inputs) directory arguments (\_ -> pure (Nothing,()))
        local <- newIORef (TraceCursor 0)
        let committed = case seed of CommittedSeed _ -> Just consumed; AdjustmentSeed _ -> Nothing
            current = ActiveSession ordinal emulator directory started local hitStart committed
        writeIORef active (Just current)
        unless (bootStart started == FreshBootStart) (die "storage lifecycle bridge did not cold-boot")
        supplied <- field "inputs" (startRecord started)
        let sourceKey = case seed of AdjustmentSeed _ -> "adjustment_nand"; CommittedSeed _ -> "committed_nand"
        actual <- field sourceKey supplied >>= text
        unless (actual == T.pack (sha256Hex consumed)) (die "storage lifecycle SDK consumed another NAND input")
        case committed of
          Nothing -> pure ()
          Just _ -> do
            kind <- field "source_kind" supplied >>= text
            adjustment <- field "adjustment_nand" supplied
            records <- field "record_nand" supplied
            unless (kind == "full-committed" && adjustment == Null && records == Null) (die "committed reboot reseeded its medium")
        _ <- drain current
        pure current
      currentSession = readIORef active >>= maybe (die "storage lifecycle has no active session") pure
      flushFiles = do
        observations <- reverse <$> readIORef steps
        manifests <- reverse <$> readIORef sessions
        BL.writeFile (output </> "steps.json") (A.encode observations <> "\n")
        BL.writeFile (output </> "emulator-session.json") (A.encode (object ["schema" .= (1::Int),"kind" .= ("settings-storage-lifecycle"::T.Text),"sessions" .= manifests]) <> "\n")
      work = do
        initial <- start (SessionNumber 0) (storageInitialNand inputs)
        record "emulator_start" (startRecord (activeStart initial))
        initialReset <- B.readFile (activeDirectory initial </> "reset-observation.json")
        B.writeFile (output </> "reset-observation.json") initialReset
        forM_ (zip [1 :: Int ..] (matrixSteps matrix)) $ \(ordinal,step) -> do
          old <- currentSession
          let emulator = activeEmulator old
          before <- readSettingsRegions emulator
          storageBefore <- readStorageState emulator
          beginWatch emulator
          from <- readIORef global
          (result,watch) <- case storageStepAction step of
            PressKey key -> observePress emulator (emulatorDisplayKey emulator key)
            PressHalfShutter -> observePress emulator (emulatorDisplayHalfPress emulator)
            SetModeDial position -> do
              dial <- emulatorModeDial emulator position
              actual <- field "position" dial >>= text
              unless (actual == T.pack (modeDialName position)) (die "storage lifecycle mode-dial response differs")
              prefix <- observeBoundedPrefix emulator
              watched <- emulatorWatchEnd emulator
              pure (object ["kind" .= ("mode-dial"::T.Text),"dial" .= dial,"bounded_post_settle_prefix" .= prefix],watched)
            ColdReboot -> do
              watched <- emulatorWatchEnd emulator
              finalRegions <- readSettingsRegions emulator
              finalStorage <- readStorageState emulator
              label <- either die pure (nandExportLabel (matrixStepNameText (storageStepName step)))
              exported <- emulatorNandExport emulator label
              status <- field "status" exported >>= text
              unless (status == "exported") (die "committed NAND export was refused")
              boundary <- field "boundary" exported
              afterBoundary <- field "after" exported
              unless (boundary == afterBoundary) (die "committed NAND export advanced the machine")
              artifact <- field "artifact" exported
              exportPath <- field "path" artifact >>= text
              let expectedPath = activeDirectory old </> "nand-" ++ T.unpack (matrixStepNameText (storageStepName step)) ++ ".nand.gz"
              unless (T.unpack exportPath == expectedPath) (die "committed NAND export artifact path differs")
              digest <- sha256File expectedPath
              extent <- getFileSize expectedPath
              advertised <- field "sha256" artifact >>= text >>= either die pure . parseSha256Hex . T.unpack
              lengthBytes <- field "bytes" artifact >>= number
              unless (digest == advertised && extent == lengthBytes) (die "committed NAND export artifact digest or extent differs")
              close
              oldSdk <- jsonFile (activeDirectory old </> "emulator-session.json")
              newHitsStart <- readIORef global
              fresh <- start (SessionNumber (sessionValue (activeNumber old)+1)) (CommittedSeed expectedPath)
              reset <- jsonFile (activeDirectory fresh </> "reset-observation.json")
              pure (object
                [ "kind" .= ("cold-reboot"::T.Text),"export" .= exported,"prior_session" .= oldSdk
                , "new_start" .= startRecord (activeStart fresh),"new_reset" .= reset,"input_committed_sha256" .= sha256Hex advertised
                , "next_session" .= sessionValue (activeNumber fresh),"new_hits_start" .= cursorValue newHitsStart
                , "regions_before_reboot" .= finalRegions,"storage_before_reboot" .= stateValue finalStorage ],watched)
          now <- currentSession
          after <- readSettingsRegions (activeEmulator now)
          storageAfter <- readStorageState (activeEmulator now)
          to <- drain now
          let observation = object
                [ "step" .= ordinal,"name" .= matrixStepNameText (storageStepName step),"session" .= sessionValue (activeNumber old)
                , "action" .= storageActionValue (storageStepAction step),"hits" .= [cursorValue from,cursorValue to],"watch" .= watch
                , "regions_before" .= before,"regions_after" .= after,"storage_before" .= stateValue storageBefore,"storage_after" .= stateValue storageAfter,"result" .= result ]
          modifyIORef' steps (observation:)
          printf "storage step %d %s: hits %d..%d\n" ordinal (T.unpack (matrixStepNameText (storageStepName step))) (cursorValue from) (cursorValue to)
        close
  outcome <- try work
  cleanup <- try close
  flushFiles
  case (outcome,cleanup) of
    (Left failure,_) -> throwIO (failure::SomeException)
    (Right (),Left failure) -> throwIO (failure::SomeException)
    (Right (),Right ()) -> pure ()

beginWatch :: Emulator -> IO ()
beginWatch emulator = do
  limit <- either die pure (W.watchLimit watchLimit)
  ranges <- either die pure (mapM (\range -> W.watchRange (W.watchAddress (rangeStart range)) (W.watchAddress (rangeEnd range))) settingsRanges)
  armed <- emulatorExecutionWatchBegin emulator ranges [] limit
  value <- field "armed" armed
  unless (value == Bool True) (die "storage lifecycle settings watch was refused")

observePress :: Emulator -> IO Value -> IO (Value,Value)
observePress emulator press = do
  idle <- emulatorIdle emulator preKeyIdle
  ran <- field "ran" idle >>= number
  unless (ran == instructionsValue preKeyIdle) (die "storage lifecycle pre-key idle is short")
  pressed <- press
  failure <- field "failure" pressed
  unless (failure == Null) (die "storage lifecycle key was refused")
  prefix <- observeBoundedPrefix emulator
  watched <- emulatorWatchEnd emulator
  pure (object ["kind" .= ("key"::T.Text),"idle" .= idle,"press" .= pressed,"bounded_post_settle_prefix" .= prefix],watched)

drainSession :: Handle -> IORef TraceCursor -> ActiveSession -> IO TraceCursor
drainSession file global current = do
  TraceCursor local <- readIORef (activeLocalCursor current)
  reply <- emulatorTraceHitsPage (activeEmulator current) local 65536
  values <- field "hits" reply >>= \value -> case value of Array entries -> pure (V.toList entries); _ -> die "storage lifecycle trace hits are not an array"
  count <- field "count" reply >>= number
  unless (count >= local+length values && (not (null values) || count == local)) (die "storage lifecycle trace page is incomplete")
  labels <- forM values $ \value -> field "site" value >>= text
  forM_ values $ \value -> BL.hPut file (A.encode value <> "\n")
  writeIORef (activeLocalCursor current) (TraceCursor (local+length values))
  TraceCursor old <- readIORef global
  let next = TraceCursor (old+length values)
  writeIORef global next
  hFlush file
  forM_ (zip values labels) $ \(value,label) ->
    when (case parseSiteLabel (T.unpack label) of Just (Guard _) -> True; _ -> False) (die ("storage lifecycle fetched a retired instruction: " ++ show value))
  unless (cursorValue next <= hitLimit) (die "storage lifecycle trace hit limit reached")
  if local+length values < count then drainSession file global current else pure next
