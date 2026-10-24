{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
module Main where

import Control.Concurrent (runInBoundThread)
import Control.Exception (SomeException, bracket, catch, displayException, finally)
import Control.Monad (forM_)
import Data.Aeson (Value (..), eitherDecodeStrict', encode, (.=))
import qualified Data.Aeson as Aeson
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy as BL
import Data.List (sort)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Emulation.CheckpointKey (CheckpointRoute (..), Interpreter (..), RouteKey (..), UnicornLibrary (..), guestState, routeKey, routeKeyHex)
import Emulation.EmbeddedPython (PyObject, RuntimeIdentity (..))
import qualified Emulation.EmbeddedPython as Py
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import qualified FujiAF.Host as H
import GHC.Clock (getMonotonicTime)
import System.Directory (canonicalizePath, doesDirectoryExist, getCurrentDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (BufferMode (LineBuffering), hFlush, hPutStrLn, hSetBuffering, stderr, stdout)

data PyArg = ArgJSON Value | ArgObject PyObject

data UiConversion = StrictConversion | DeclaredCpuFallback

uiConversionValue :: UiConversion -> Value
uiConversionValue StrictConversion = Null
uiConversionValue DeclaredCpuFallback = H.s "declared-ui-cpu-fallback"

data RestoredFrom = InstalledCheckpoint | AttachCheckpoint

restoreValue :: RestoredFrom -> Value
restoreValue InstalledCheckpoint = H.s "installed"
restoreValue AttachCheckpoint = H.s "restored"

data BridgeOptions = BridgeOptions
  { optionProfile :: FilePath
  , optionInstalledDat :: FilePath
  , optionAdjustmentNand :: FilePath
  , optionOutput :: FilePath
  , optionCheckpointRoot :: FilePath
  , optionFreshBoot :: Bool
  , optionUiConversion :: UiConversion
  }

parseOptions :: [String] -> IO BridgeOptions
parseOptions = go (BridgeOptions "firmware/x-m1/1.50/emulator-rec.json" "" "" "" "" False StrictConversion)
  where
    go options [] = do
      forM_
        [ ("--installed-dat", optionInstalledDat options)
        , ("--adjustment-nand", optionAdjustmentNand options)
        , ("--output", optionOutput options)
        , ("--ram-push-checkpoints", optionCheckpointRoot options)
        ] $ \(name, value) -> H.ensure (not (null value)) ("cardless bridge requires " ++ name)
      H.ensure (not (null (optionProfile options))) "cardless bridge requires --profile"
      pure options
    go options ("--profile" : value : rest) = go options { optionProfile = value } rest
    go options ("--installed-dat" : value : rest) = go options { optionInstalledDat = value } rest
    go options ("--adjustment-nand" : value : rest) = go options { optionAdjustmentNand = value } rest
    go options ("--output" : value : rest) = go options { optionOutput = value } rest
    go options ("--ram-push-checkpoints" : value : rest) = go options { optionCheckpointRoot = value } rest
    go options ("--fresh-boot" : rest) = go options { optionFreshBoot = True } rest
    go options ("--ui-conversion-profile" : value : rest) = case value of
      "strict" -> go options { optionUiConversion = StrictConversion } rest
      "declared-ui-cpu-fallback" -> go options { optionUiConversion = DeclaredCpuFallback } rest
      _ -> H.failH "--ui-conversion-profile must be strict or declared-ui-cpu-fallback"
    go _ (argument : _) = H.failH ("unknown cardless bridge option: " ++ argument)

initialize :: IO RuntimeIdentity
initialize = do
  identity <- Py.runtimeIdentity
  let declared = declaredVersion identity
      home = declaredEnvironment identity
      library = home </> "lib" </> declaredAbi identity
  H.ensure (compiledVersion identity == declared && takeWhile (/= ' ') (loadedVersion identity) == declared)
    "declared Python embedding environment does not match the loaded interpreter"
  Py.initialize home [library, library </> "lib-dynload", library </> "site-packages"]
  root <- getCurrentDirectory >>= canonicalizePath
  forM_ [root </> "native/python", root, root </> "tools"] Py.insertPath
  pure identity

owned :: IO PyObject -> (PyObject -> IO a) -> IO a
owned action = bracket action Py.release

withImport :: String -> (PyObject -> IO a) -> IO a
withImport name = owned (Py.importModule name)

withAttr :: PyObject -> String -> (PyObject -> IO a) -> IO a
withAttr object name = owned (Py.getAttr object name)

withJSON :: Value -> (PyObject -> IO a) -> IO a
withJSON value = owned (Py.fromJSONText (encodeCanonical value))

jsonObject :: PyObject -> IO Value
jsonObject object = Py.toJSONText object >>= H.jsonRead

attributeValue :: PyObject -> String -> IO Value
attributeValue object name = withAttr object name jsonObject

withArguments :: [PyArg] -> ([PyObject] -> IO a) -> IO a
withArguments [] continuation = continuation []
withArguments (ArgJSON value : rest) continuation =
  withJSON value $ \decoded -> withArguments rest (continuation . (decoded :))
withArguments (ArgObject object : rest) continuation =
  withArguments rest (continuation . (object :))

callObject :: PyObject -> String -> [PyArg] -> Value -> (PyObject -> IO a) -> IO a
callObject object name arguments keywords continuation =
  withAttr object name $ \function ->
    withArguments arguments $ \values ->
      withJSON keywords $ \keywordObject ->
        owned (Py.call function values (Just keywordObject)) continuation

callCallable :: PyObject -> [PyArg] -> PyObject -> (PyObject -> IO a) -> IO a
callCallable function arguments keywords continuation =
  withArguments arguments $ \values ->
    owned (Py.call function values (Just keywords)) continuation

callValue :: PyObject -> String -> [PyArg] -> Value -> IO Value
callValue object name arguments keywords = callObject object name arguments keywords jsonObject

withItem :: PyObject -> Integer -> (PyObject -> IO a) -> IO a
withItem object index continuation =
  withImport "operator" $ \moduleObject ->
    withAttr moduleObject "getitem" $ \function ->
      withJSON (H.n index) $ \indexObject ->
        owned (Py.call function [object, indexObject] Nothing) continuation

setKeyJSON :: PyObject -> String -> Value -> IO ()
setKeyJSON dictionary name value = withJSON value (Py.dictSetItem dictionary name)

setKeyObject :: PyObject -> String -> PyObject -> IO ()
setKeyObject = Py.dictSetItem

withPath :: PyObject -> FilePath -> (PyObject -> IO a) -> IO a
withPath pathlib path continuation =
  withAttr pathlib "Path" $ \constructor ->
    withJSON (H.s path) $ \argument ->
      owned (Py.call constructor [argument] Nothing) continuation

writeEvent :: Value -> IO ()
writeEvent value = BL.putStr (encode value) >> BC.putStrLn "" >> hFlush stdout

elapsedSince :: Double -> IO Double
elapsedSince started = do
  now <- getMonotonicTime
  pure (now - started)

-- | Board options the cold boot sets over the loaded profile. With the bridge
-- options, this record is everything the bridge applies to the board before
-- the attach route; the key includes both.
data ColdBoot = ColdBoot
  { coldEntry :: String
  , coldCardPresent :: Bool
    -- | An attribute of emulator.sd_card.
  , coldCardProfile :: String
  }

cardlessColdBoot :: ColdBoot
cardlessColdBoot = ColdBoot "rom" False "BLANK_SDHC_PROFILE"

coldBootValue :: ColdBoot -> Value
coldBootValue (ColdBoot entry present profile) = H.obj
  [("entry", H.s entry), ("sd_card_present", H.b present), ("sd_card_profile", H.s profile)]

checkpointKey :: RuntimeIdentity -> BridgeOptions -> IO RouteKey
checkpointKey runtime options = do
  root <- getCurrentDirectory >>= canonicalizePath
  unicorn <- lookupEnv "X70_UNICORN_LIBRARY" >>= maybe (H.failH "X70_UNICORN_LIBRARY is missing") pure
  let RuntimeIdentity home base abi declared _ _ = runtime
  guest <- guestState root (EmbeddedInterpreter home base abi declared) (UnicornLibrary unicorn)
  installed <- sha256File (optionInstalledDat options)
  adjustment <- sha256File (optionAdjustmentNand options)
  pure (routeKey guest CardlessAttach
    [ ("profile", H.s (optionProfile options))
    , ("installed_dat", H.s (sha256Hex installed))
    , ("adjustment_nand", H.s (sha256Hex adjustment))
    , ("ui_conversion_profile", uiConversionValue (optionUiConversion options))
    , ("cold_boot", coldBootValue cardlessColdBoot) ])

-- The start command and server share CPython's stdin buffer.
readStart :: [FilePath] -> IO (Maybe FilePath)
readStart directories = do
  line <- withImport "sys" $ \sys -> withAttr sys "stdin" $ \input ->
    callValue input "readline" [] (H.obj [])
  value <- case line of
    String text -> either (H.failH . ("invalid emulator start command: " ++)) pure (eitherDecodeStrict' (TE.encodeUtf8 text))
    _ -> H.failH "emulator start command is not text"
  H.ensure (sort (H.keys value) == ["command", "installed"]) "RAM-push start requires command and installed only"
  H.ensure (H.str (H.get value "command") == "start") "RAM-push start requires command start"
  case H.get value "installed" of
    Null -> pure Nothing
    String text
      | T.unpack text `elem` directories -> pure (Just (T.unpack text))
      | otherwise -> H.failH "RAM-push start names no listed install checkpoint"
    _ -> H.failH "RAM-push start installed must be a listed directory or null"

withRestored :: PyObject -> PyObject -> Value -> PyObject -> (PyObject -> PyObject -> PyObject -> PyObject -> IO a) -> IO a
withRestored scanout directory record output continuation =
  callObject scanout "_restore_ram_push_checkpoint" [ArgObject directory, ArgJSON record, ArgObject output] (H.obj []) $ \tuple ->
    withItem tuple 0 $ \board ->
      withItem tuple 1 $ \session ->
        withItem tuple 2 $ \bridge ->
          withItem tuple 3 $ \attachment -> continuation board session bridge attachment

runServer :: PyObject -> PyObject -> PyObject -> PyObject -> PyObject -> PyObject -> Value -> PyObject -> IO ()
runServer scanout board session output bridge attachment checkpoint checkpointRoot =
  callObject scanout "_run_ram_push_server"
    [ ArgObject board, ArgObject session, ArgObject output, ArgObject bridge
    , ArgObject attachment, ArgJSON checkpoint, ArgObject checkpointRoot ]
    (H.obj []) (\_ -> pure ())

saveAttachCheckpoint :: PyObject -> String -> Value -> PyObject -> PyObject -> PyObject -> Bool -> Value -> IO ()
saveAttachCheckpoint scanout key identity attachment session checkpointRoot replace attachRecord =
  withJSON (H.obj [("key", H.s key), ("inputs", identity)]) $ \record -> do
    callObject record "update" [ArgObject attachment] (H.obj []) (\_ -> pure ())
    callObject scanout "_save_ram_push_checkpoint"
      [ ArgObject session, ArgObject checkpointRoot, ArgJSON (H.s key)
      , ArgJSON attachRecord, ArgObject record ]
      (Aeson.object ["replace" .= replace]) (\_ -> pure ())

recoveryMetadata :: BridgeOptions -> Value
recoveryMetadata options = Aeson.object
  [ "phase" .= ("independent-installed-cold-boot-cardless" :: String)
  , "media" .= optionInstalledDat options
  , "adjustment_nand" .= optionAdjustmentNand options
  , "card_present" .= False
  , "card" .= Aeson.object
      [ "present" .= False
      , "binding" .= ("cardless-volatile-ram-bank" :: String) ] ]

serve :: Double -> RuntimeIdentity -> BridgeOptions -> IO ()
serve started runtime options =
  withImport "tools.continue_rec_scanout" $ \scanout ->
  withImport "pathlib" $ \pathlib ->
  withImport "emulator.boot" $ \boot ->
  withImport "emulator.sd_card" $ \sd ->
  withImport "emulator.image_parameter_devices" $ \parameters ->
  withImport "tools.probe_setup_menus" $ \menus -> do
    routeKeyValue <- checkpointKey runtime options
    keySeconds <- elapsedSince started
    let key = routeKeyHex routeKeyValue
        identity = routeKeyDocument routeKeyValue
    attachRecord <- attributeValue scanout "ATTACH_RECORD"
    installRecord <- attributeValue scanout "INSTALL_RECORD"
    withPath pathlib (optionCheckpointRoot options) $ \checkpointRoot -> withPath pathlib (optionOutput options) $ \output -> do
      installed <- if optionFreshBoot options
        then pure (Aeson.toJSON ([] :: [Value]))
        else callValue scanout "_install_checkpoints"
          [ArgObject checkpointRoot, ArgJSON (H.s key)] (H.obj [])
      let directories =
            [ directory
            | item <- H.list installed
            , let directory = H.str (H.get item "directory")
            , not (null directory) ]
          checkpointDirectory = optionCheckpointRoot options </> key
          freshStart :: String
          freshStart = if optionFreshBoot options then "fresh-boot" else "cold"
          serveRestored :: RestoredFrom -> FilePath -> Value -> Double -> IO ()
          serveRestored start directory record restoreStarted = do
            withPath pathlib directory $ \directoryPath ->
              withRestored scanout directoryPath record output $ \board session bridge attachment -> do
                restoreSeconds <- elapsedSince restoreStarted
                let installedField = case start of
                      InstalledCheckpoint -> ["installed" .= directory]
                      AttachCheckpoint -> []
                    checkpoint = Aeson.object
                      ([ "key" .= key, "inputs" .= identity, "directory" .= checkpointDirectory
                       , "key_seconds" .= keySeconds, "start" .= restoreValue start
                       , "restore_seconds" .= restoreSeconds ] ++ installedField)
                runServer scanout board session output bridge attachment checkpoint checkpointRoot
          coldBoot = do
            bootStarted <- getMonotonicTime
            withAttr boot "make_board" $ \factory ->
              callObject scanout "load_profile"
                [ArgJSON (H.s (optionProfile options)), ArgObject factory] (H.obj []) $ \loaded -> do
                storage <- callValue loaded "get" [ArgJSON (H.s "storage_profile"), ArgJSON (H.s "erased")] (H.obj [])
                journals <- callValue loaded "get" [ArgJSON (H.s "user_journals")] (H.obj [])
                settings <- callValue loaded "get" [ArgJSON (H.s "settings")] (H.obj [])
                H.ensure (storage == H.s "erased" && journals == Null && settings /= H.s "owner")
                  "independent installed NAND requires erased fixture selection and no injected owner journals"
                setKeyJSON loaded "entry" (H.s (coldEntry cardlessColdBoot))
                setKeyJSON loaded "sd_card_present" (H.b (coldCardPresent cardlessColdBoot))
                withAttr sd (coldCardProfile cardlessColdBoot) $ \profile ->
                  setKeyObject loaded "sd_card_profile" profile
                case optionUiConversion options of
                  StrictConversion -> pure ()
                  DeclaredCpuFallback ->
                    withAttr parameters "UIConversionProfile" $ \enumeration ->
                      callObject enumeration "parse"
                        [ArgJSON (uiConversionValue DeclaredCpuFallback)] (H.obj []) $ \uiProfile ->
                          setKeyObject loaded "ui_conversion_profile" uiProfile
                callCallable factory [] loaded $ \board -> do
                  callObject scanout "attach_installed_dat"
                    [ArgObject board, ArgJSON (H.s (optionInstalledDat options))] (H.obj []) (\_ -> pure ())
                  callObject scanout "attach_adjustment_nand"
                    [ArgObject board, ArgJSON (H.s (optionAdjustmentNand options))] (H.obj []) (\_ -> pure ())
                  withJSON (recoveryMetadata options) $ \recovery ->
                    setKeyObject loaded "recovery" recovery
                  withAttr menus "MenuSession" $ \sessionClass ->
                    withJSON (H.obj []) $ \keywords -> do
                      setKeyObject keywords "options" loaded
                      callCallable sessionClass [ArgObject board, ArgObject output] keywords $ \session ->
                        callObject scanout "_ram_push_attach" [ArgObject board, ArgObject session] (H.obj []) $ \pair ->
                          withItem pair 0 $ \bridge ->
                            withItem pair 1 $ \attachment -> do
                              bootSeconds <- elapsedSince bootStarted
                              saveStarted <- getMonotonicTime
                              callObject bridge "drain_scheduled" [] (H.obj []) (\_ -> pure ())
                              saveAttachCheckpoint scanout key identity attachment session
                                checkpointRoot (optionFreshBoot options) attachRecord
                              saveSeconds <- elapsedSince saveStarted
                              let checkpoint = Aeson.object
                                    [ "key" .= key, "inputs" .= identity, "directory" .= checkpointDirectory
                                    , "key_seconds" .= keySeconds, "start" .= freshStart
                                    , "boot_seconds" .= bootSeconds, "saved" .= True
                                    , "save_seconds" .= saveSeconds ]
                              runServer scanout board session output bridge attachment checkpoint checkpointRoot
      writeEvent (Aeson.object
        [ "ok" .= True
        , "event" .= ("start-options" :: String)
        , "boot_key" .= key
        , "installed" .= installed ])
      chosen <- readStart directories
      case chosen of
        Just directory -> do
          restoreStarted <- getMonotonicTime
          record <- withPath pathlib directory $ \directoryPath ->
            callValue scanout "_read_ram_push_record"
              [ArgObject directoryPath, ArgJSON installRecord, ArgJSON (H.s key)] (H.obj [])
          serveRestored InstalledCheckpoint directory record restoreStarted
        Nothing -> do
          exists <- doesDirectoryExist checkpointDirectory
          if exists && not (optionFreshBoot options)
            then do
              restoreStarted <- getMonotonicTime
              record <- withPath pathlib checkpointDirectory $ \directoryPath ->
                callValue scanout "_read_ram_push_record"
                  [ArgObject directoryPath, ArgJSON attachRecord, ArgJSON (H.s key)] (H.obj [])
              serveRestored AttachCheckpoint checkpointDirectory record restoreStarted
            else coldBoot

run :: [String] -> IO ()
run arguments = do
  started <- getMonotonicTime
  options <- parseOptions arguments
  hSetBuffering stdout LineBuffering
  runtime <- initialize
  finally (serve started runtime options) Py.finish

main :: IO ()
main = runInBoundThread (getArgs >>= run) `catch` \(exception :: SomeException) -> do
  hPutStrLn stderr ("cardless-ram-push-bridge: " ++ displayException exception)
  exitFailure
