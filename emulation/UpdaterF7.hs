{-# LANGUAGE OverloadedStrings #-}
-- | @updater-f7@: installs a restore card's DAT through the ordinary X-M1 1.50
-- updater in the full-system emulator over a committed NAND, then cold-boots
-- the committed result to the shooting screen. It reads the update
-- completion's flag word, the stored adjustment bytes 0x80, 0x81 and 0xf7
-- before and after, and the reloaded cache bytes, and writes a receipt.
--
-- Phase A continues the recovery bootstrap's terminal RAM with the restore
-- card and the installed NAND, selects BODY with OK, OK, UP, OK, OK and runs
-- until the whole-area commit has run and the NAND is idle. Phase B starts a
-- new process from the exported NAND only.
module Main (main) where

import Control.Exception (IOException, catch, displayException)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), eitherDecode, eitherDecodeStrict, encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, parseEither, withArray, withObject, (.:), (.:?))
import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt, isHexDigit)
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as Map
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word8)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), FixedSceneFrame, Python (..), Runtime (..), StagedRuntime (..), StockOriginal (..), Workspace (..), fixedSceneFrameArguments, parseFixedSceneFrame, require, resolve, stageViewerRuntime, validateFirmware)
import qualified Emulation.ViewerRuntime as ViewerRuntime
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectory, doesFileExist)
import System.Environment (getArgs)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath (splitDirectories, takeDirectory, takeFileName, (</>))
import System.IO (BufferMode (LineBuffering), Handle, IOMode (WriteMode), hClose, hFlush, hGetLine, hPutStrLn, hSetBuffering, openFile, stderr, stdout)
import System.Posix.Files (setFileMode)
import System.Process (CreateProcess (..), ProcessHandle, StdStream (..), createProcess, proc, readProcess, waitForProcess)

newtype Address = Address Word32 deriving (Eq, Ord)
data Window = Window Address Int

newtype RecoveryRam = RecoveryRam FilePath
newtype SdImage = SdImage FilePath
newtype InstalledNand = InstalledNand FilePath
newtype ReceiptDirectory = ReceiptDirectory FilePath
newtype Instructions = Instructions Integer deriving (Eq, Ord)

data Options = Options
  { recoveryRamOption :: Maybe FilePath
  , sdImageOption :: Maybe FilePath
  , installedNandOption :: Maybe FilePath
  , receiptOption :: Maybe FilePath
  , fixedSceneFrameOption :: Maybe FixedSceneFrame
  }

data Command = Help | Validate Firmware | Launch Options

data MenuKey = KeyOk | KeyUp deriving (Eq)

keyName :: MenuKey -> String
keyName KeyOk = "ok"
keyName KeyUp = "up"

showAddress :: Address -> String
showAddress (Address value) = "0x" ++ showHex value ""

-- Firmware sites and data, X-M1 1.50 (byte-read; reports/update-validation-and-boot.md).
completionSite, headerParserSite, templateReloadSite, wholeAreaCommitSite :: Address
completionSite = Address 0x00db91e0
headerParserSite = Address 0x00db9cd4
templateReloadSite = Address 0x0069fe20
wholeAreaCommitSite = Address 0x00db932c

-- | Kind-0 flag block; the flag word is at +0xc (0x014e3578).
flagBlock :: Window
flagBlock = Window (Address 0x014e356c) 0x10

cachePointer, systemModeWord, connectHookWord :: Address
cachePointer = Address 0x0096ca50
systemModeWord = Address 0x009570dc
connectHookWord = Address 0x00ecf900

-- | Stored adjustment offsets read from page 0 of the area's first block.
storedOffsets :: [Int]
storedOffsets = [0x80, 0x81, 0xf7]

-- | The stored-f7 fixture holds the area's first logical block at physical block 0x381.
inputAreaFirstBlock :: Int
inputAreaFirstBlock = 0x381

pagesPerBlock, pageBytes :: Int
pagesPerBlock = 64
pageBytes = 2048

-- Instruction schedule. The restore card's updater screen is up by 60 M
-- instructions; an ordinary cold boot shows the first-start language prompt by 100 M.
continuationChunk, updaterScreenAt, completionBound, firstStartAt, shootingSettle :: Integer
continuationChunk = 15000000
updaterScreenAt = 135000000
completionBound = 3500000000
firstStartAt = 100000000
shootingSettle = 45000000

updaterKeys, firstStartKeys :: [MenuKey]
updaterKeys = [KeyOk, KeyOk, KeyUp, KeyOk, KeyOk]
firstStartKeys = replicate 12 KeyUp ++ [KeyOk, KeyOk]

usage :: String
usage = unlines
  [ "updater-f7 --recovery-ram RAM --sd-image CARD --installed-nand NAND --receipt-output DIR"
  , ""
  , "Runs the ordinary X-M1 1.50 updater from a //recovery:build-image restore card over a"
  , "committed NAND in the full-system emulator, exports the committed NAND and cold-boots it"
  , "to the shooting screen. RAM is the ram-rescue-engine raw output for CARD (with its"
  , ".state.json companion). All paths are beneath workspace/private-data/; DIR must be new."
  , "Optional --fixed-scene-frame N pins every scene render to the positive frame index N."
  , "Holds one emulator slot. No camera or USB I/O."
  ]

parseCommand :: [String] -> IO Command
parseCommand ["--help"] = pure Help
parseCommand ["--validate-firmware", path] = pure (Validate (Firmware path))
parseCommand arguments = Launch <$> go (Options Nothing Nothing Nothing Nothing Nothing) arguments
  where
    go options [] = pure options
    go options ("--recovery-ram" : value : rest) = go options {recoveryRamOption = Just value} rest
    go options ("--sd-image" : value : rest) = go options {sdImageOption = Just value} rest
    go options ("--installed-nand" : value : rest) = go options {installedNandOption = Just value} rest
    go options ("--receipt-output" : value : rest) = go options {receiptOption = Just value} rest
    go options ("--fixed-scene-frame" : value : rest) = do
      frame <- either (ioError . userError) pure (parseFixedSceneFrame value)
      go options {fixedSceneFrameOption = Just frame} rest
    go _ (flag : _) = ioError (userError ("Unknown or incomplete option " ++ flag ++ "\n" ++ usage))

privateRegularFile :: Workspace -> String -> Maybe FilePath -> IO FilePath
privateRegularFile root name supplied = do
  path <- maybe (ioError (userError (name ++ " is required\n" ++ usage))) pure supplied
  private <- canonicalizePath (resolve root "private-data")
  resolved <- canonicalizePath (resolve root path)
  require (splitDirectories private `isPrefixOf` splitDirectories resolved) (name ++ " must be beneath workspace/private-data/")
  exists <- doesFileExist resolved
  require exists (name ++ " must be a regular file")
  pure resolved

freshReceipt :: Workspace -> Maybe FilePath -> IO ReceiptDirectory
freshReceipt root supplied = do
  path <- maybe (ioError (userError ("--receipt-output is required\n" ++ usage))) pure supplied
  private <- canonicalizePath (resolve root "private-data")
  let target = resolve root path
  parent <- canonicalizePath (takeDirectory target)
  require (splitDirectories private `isPrefixOf` splitDirectories parent) "--receipt-output must be beneath workspace/private-data/"
  require (takeFileName target `notElem` ["", ".", ".."]) "--receipt-output must name a fresh directory"
  let destination = parent </> takeFileName target
  createDirectory destination
  setFileMode destination 0o700
  pure (ReceiptDirectory destination)

fileSha256 :: FilePath -> IO String
fileSha256 path = take 64 <$> readProcess "sha256sum" [path] ""

decodeHex :: T.Text -> Either String B.ByteString
decodeHex text
  | odd (T.length text) || not (T.all isHexDigit text) = Left "odd or non-hex byte string"
  | otherwise = Right (B.pack (pairs (T.unpack text)))
  where
    pairs (high : low : rest) = fromIntegral (digitToInt high * 16 + digitToInt low) : pairs rest
    pairs _ = []

wordLE :: B.ByteString -> Int -> Word32
wordLE bytes offset = foldr (\index acc -> (acc `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

-- Committed NAND image (emulator/nand_image.py export): gzip of one JSON document.

data NandImage = NandImage
  { nandData :: Map.Map Int B.ByteString
  , nandSpare :: Map.Map Int B.ByteString
  }

pageRows :: Value -> Parser (Map.Map Int B.ByteString)
pageRows = withArray "page rows" $ \rows -> Map.fromList <$> mapM row (V.toList rows)
  where
    row = withArray "page row" $ \cells -> case V.toList cells of
      [pageValue, String hex] -> do
        page <- parseNumber pageValue
        either fail (\bytes -> pure (page, bytes)) (decodeHex hex)
      _ -> fail "page row is [page, hex]"
    parseNumber (Number n) = pure (truncate n)
    parseNumber _ = fail "page index is a number"

readNandImage :: FilePath -> IO NandImage
readNandImage path = do
  (code, json, failure) <- readProcessWithExitCodeBytes "gzip" ["-dc", path]
  require (code == ExitSuccess) ("gzip could not read " ++ path ++ ": " ++ failure)
  value <- either (\e -> ioError (userError ("NAND image JSON: " ++ e))) pure (eitherDecode json)
  either (\e -> ioError (userError ("NAND image layout: " ++ e))) pure $ flip parseEither value $
    withObject "NAND image" $ \o -> do
      kind <- o .: "kind"
      unless (kind == ("type1-nand-committed-image" :: T.Text)) (fail "unexpected NAND image kind")
      NandImage <$> (o .: "committed_data_pages" >>= pageRows) <*> (o .: "committed_spare_pages" >>= pageRows)

readProcessWithExitCodeBytes :: FilePath -> [String] -> IO (ExitCode, BL.ByteString, String)
readProcessWithExitCodeBytes command arguments = do
  (_, Just out, _, handle) <- createProcess (proc command arguments) {std_out = CreatePipe}
  bytes <- B.hGetContents out
  code <- waitForProcess handle
  pure (code, BL.fromStrict bytes, if code == ExitSuccess then "" else "exit " ++ show code)

newtype BlockTag = BlockTag B.ByteString deriving (Eq)

blockTag :: NandImage -> Int -> Maybe BlockTag
blockTag image block = BlockTag . B.take 4 <$> Map.lookup (block * pagesPerBlock) (nandSpare image)

data AreaPage = AreaPage Int [(Int, Word8)]

-- | Page 0 of the committed block carrying the area's first logical tag.
areaFirstPage :: BlockTag -> NandImage -> IO AreaPage
areaFirstPage tag image = do
  let blocks = [page `div` pagesPerBlock | (page, spare) <- Map.toList (nandSpare image)
                                          , page `mod` pagesPerBlock == 0, BlockTag (B.take 4 spare) == tag]
  block <- case blocks of
    [one] -> pure one
    _ -> ioError (userError ("Expected one committed block with the area's first tag; found " ++ show (length blocks)))
  page <- maybe (ioError (userError "Area block has no committed page 0")) pure (Map.lookup (block * pagesPerBlock) (nandData image))
  require (B.length page == pageBytes) "Committed page has the wrong length"
  pure (AreaPage block [(offset, B.index page offset) | offset <- storedOffsets])

areaJson :: AreaPage -> Value
areaJson (AreaPage block bytes) = object
  [ "physical_block" .= ("0x" ++ showHex block "")
  , "bytes" .= object [K.fromString ("0x" ++ showHex offset "") .= value | (offset, value) <- bytes]
  ]

-- Console protocol of tools/continue_rec_scanout.py.

data Console = Console {consoleInput :: Handle, consoleOutput :: Handle, consoleProcess :: ProcessHandle}

startConsole :: Python -> FilePath -> [String] -> IO Console
startConsole (Python python) errorLog arguments = do
  logHandle <- openFile errorLog WriteMode
  (Just input, Just output, _, handle) <- createProcess (proc python ("tools/continue_rec_scanout.py" : arguments))
    {std_in = CreatePipe, std_out = CreatePipe, std_err = UseHandle logHandle}
  hSetBuffering input LineBuffering
  let awaitPrompt = do
        line <- hGetLine output
        unless ("Commands:" `isPrefixOf` line) awaitPrompt
  awaitPrompt
  _ <- hGetLine output
  pure (Console input output handle)

request :: Console -> String -> IO Value
request console line = do
  hPutStrLn (consoleInput console) line
  hFlush (consoleInput console)
  let next = do
        reply <- C.hGetLine (consoleOutput console)
        if "{" `B.isPrefixOf` reply
          then either (\e -> ioError (userError ("Console reply to " ++ line ++ ": " ++ e))) pure (eitherDecodeStrict reply)
          else next
  value <- next
  case value of
    Object o | Just message <- KM.lookup "error" o ->
      ioError (userError ("Console refused " ++ line ++ ": " ++ show message ++ maybe "" (\m -> " " ++ show m) (KM.lookup "message" o)))
    _ -> pure value

quitConsole :: Console -> IO ()
quitConsole console = do
  hPutStrLn (consoleInput console) "quit"
  hClose (consoleInput console)
  code <- waitForProcess (consoleProcess console)
  require (code == ExitSuccess) ("Emulator process ended with " ++ show code)

parsed :: String -> (Value -> Parser a) -> Value -> IO a
parsed context parser value = either (\e -> ioError (userError (context ++ ": " ++ e))) pure (parseEither parser value)

data Progress = Progress {progressInstructions :: Instructions, progressFrame :: Maybe T.Text}

progress :: Value -> IO Progress
progress = parsed "execution summary" $ withObject "summary" $ \o -> do
  failure <- o .:? "failure"
  failed <- o .:? "execution_error"
  case (failure, failed) of
    (Just Null, _) -> pure ()
    (Nothing, _) -> pure ()
    (Just stop, _) -> fail ("guest stopped: " ++ show stop)
  case failed of
    Just Null -> pure ()
    Nothing -> pure ()
    Just stop -> fail ("execution error: " ++ show stop)
  count <- o .: "instructions"
  frame <- o .:? "frame" >>= maybe (pure Nothing) (withObject "frame" (.:? "png_sha256"))
  pure (Progress (Instructions count) frame)

runChunk :: Console -> IO Progress
runChunk console = request console ("run " ++ show continuationChunk) >>= progress

runUntil :: Console -> Integer -> IO Progress
runUntil console target = do
  step <- runChunk console
  let Instructions count = progressInstructions step
  if count >= target then pure step else runUntil console target

-- | Every updater and first-start press holds 8,000,000 instructions with an
-- 8,000,000-instruction release cap; the bridge's shorter per-class defaults
-- leave the updater confirmation screen unanswered.
pressKey :: Console -> MenuKey -> IO Progress
pressKey console key = request console (unwords ["key", keyName key, show keyPulse, show keyPulse]) >>= progress
  where
    keyPulse = 8000000 :: Int

readBytes :: Console -> Address -> Int -> IO B.ByteString
readBytes console address count = do
  reply <- request console ("read " ++ showAddress address ++ " " ++ show count)
  hex <- parsed "read" (withObject "read" (.: "hex")) reply
  either (\e -> ioError (userError ("read: " ++ e))) pure (decodeHex hex)

readWord :: Console -> Address -> IO Word32
readWord console address = (`wordLE` 0) <$> readBytes console address 4

data CacheBytes = CacheBytes Word32 [(Int, Word8)]

readCache :: Console -> IO CacheBytes
readCache console = do
  base <- readWord console cachePointer
  require (base /= 0) "Adjustment cache pointer is not set"
  values <- forM storedOffsets $ \offset -> do
    byte <- readBytes console (Address (base + fromIntegral offset)) 1
    pure (offset, B.head byte)
  pure (CacheBytes base values)

cacheJson :: CacheBytes -> Value
cacheJson (CacheBytes base values) = object
  [ "cache_base" .= ("0x" ++ showHex base "")
  , "bytes" .= object [K.fromString ("0x" ++ showHex offset "") .= value | (offset, value) <- values]
  ]

watch :: Console -> Address -> [Window] -> IO ()
watch console address windows = do
  _ <- request console (unwords ("watch" : showAddress address : [showAddress start ++ ":" ++ show count | Window start count <- windows]))
  pure ()

data Hit = Hit {hitAddress :: T.Text, hitWindows :: Map.Map T.Text T.Text, hitRaw :: Value}

takeHits :: Console -> IO [Hit]
takeHits console = do
  reply <- request console "hits"
  parsed "hits" (withObject "hits" $ \o -> o .: "hits" >>= mapM hit) reply
  where
    hit value = flip (withObject "hit") value $ \o ->
      Hit <$> o .: "address" <*> o .: "windows" <*> pure value

data NandState = NandState {nandBusy :: Bool, nandTransactions :: Map.Map T.Text Integer} deriving (Eq)

nandState :: Console -> IO NandState
nandState console = request console "nandstat" >>= parsed "nandstat" (withObject "nandstat" $ \o -> NandState <$> o .: "busy" <*> o .: "transactions")

siteText :: Address -> T.Text
siteText = T.pack . showAddress

-- | The kind-0 flag word at the first completion hit.
completionFlag :: [Hit] -> IO Word32
completionFlag hits = case [hit | hit <- hits, hitAddress hit == siteText completionSite] of
  hit : _ -> do
    let Window (Address start) _ = flagBlock
    hex <- maybe (ioError (userError "Completion hit lacks the flag block")) pure (Map.lookup (T.pack ("0x" ++ showHex start "")) (hitWindows hit))
    bytes <- either (ioError . userError) pure (decodeHex hex)
    pure (wordLE bytes 0xc)
  [] -> ioError (userError "The update completion 0x00db91e0 did not run")

runToCommitIdle :: Console -> IO ([Hit], NandState)
runToCommitIdle console = loop [] Nothing
  where
    loop hits previous = do
      step <- runChunk console
      new <- takeHits console
      state <- nandState console
      let all' = hits ++ new
          committed = any ((== siteText wholeAreaCommitSite) . hitAddress) all'
          Instructions count = progressInstructions step
      if committed && not (nandBusy state) && previous == Just state
        then pure (all', state)
        else do
          require (count < completionBound) "Updater did not reach an idle committed NAND within the bound"
          loop all' (Just state)

data Expectation = Expectation T.Text Bool Value

expectationJson :: Expectation -> Value
expectationJson (Expectation name passed observed) = object ["name" .= name, "passed" .= passed, "observed" .= observed]

launch :: Options -> IO ()
launch options = do
  arguments <- getArgs
  policy <- slotPolicyFromEnvironment
  reexecInSlot policy arguments
  hSetBuffering stdout LineBuffering
  root <- ViewerRuntime.workspace Nothing
  ram <- RecoveryRam <$> privateRegularFile root "--recovery-ram" (recoveryRamOption options)
  card <- SdImage <$> privateRegularFile root "--sd-image" (sdImageOption options)
  nand <- InstalledNand <$> privateRegularFile root "--installed-nand" (installedNandOption options)
  let RecoveryRam ramPath = ram
      SdImage cardPath = card
      InstalledNand nandPath = nand
  companion <- doesFileExist (ramPath ++ ".state.json")
  require companion "--recovery-ram needs its .state.json companion"
  ReceiptDirectory receipt <- freshReceipt root (receiptOption options)
  inputHashes <- forM [("recovery_ram", ramPath), ("sd_image", cardPath), ("installed_nand", nandPath)] $ \(name, path) -> do
    digest <- fileSha256 path
    pure (K.fromString name .= object ["path" .= path, "sha256" .= digest])

  before <- readNandImage nandPath
  tag <- maybe (ioError (userError "Installed NAND has no committed spare page at block 0x381")) pure (blockTag before inputAreaFirstBlock)
  storedBefore <- areaFirstPage tag before

  staged <- stageViewerRuntime root (Just (Runtime (receipt </> "runtime"))) (UniqueRuntime "updater-f7") Nothing StageStockOriginal
  let python@(Python pythonPath) = stagedPython staged
  putStrLn ("Runtime: " ++ stagedDirectory staged)
  _ <- readProcess pythonPath ["tools/firmware_registry.py", "unpack", "xm1-150"] ""

  -- Phase A: ordinary updater over the installed NAND.
  updater <- startConsole python (receipt </> "updater.stderr")
    ([ "--recovery-ram", ramPath, "--sd-image", cardPath, "--installed-nand", nandPath
     , "--menus", "--pause-after", "0", "--output", receipt </> "updater-frames"
     ] ++ fixedSceneFrameArguments (fixedSceneFrameOption options))
  watch updater completionSite [flagBlock, Window cachePointer 4]
  watch updater headerParserSite [flagBlock]
  watch updater templateReloadSite []
  watch updater wholeAreaCommitSite [flagBlock]
  screen <- runUntil updater updaterScreenAt
  cacheAtScreen <- readCache updater
  keyFrames <- forM updaterKeys $ \key -> do
    step <- pressKey updater key
    pure (T.pack ("after " ++ keyName key), progressFrame step)
  let updaterFrames = ("updater screen", progressFrame screen) : keyFrames
  early <- takeHits updater
  (late, finalNand) <- runToCommitIdle updater
  let hits = early ++ late
  flag <- completionFlag hits
  cacheAfterCommit <- readCache updater
  let exported = receipt </> "updated.nand"
  _ <- request updater ("nand " ++ exported)
  quitConsole updater

  after <- readNandImage exported
  storedAfter <- areaFirstPage tag after
  exportedHash <- fileSha256 exported

  -- Phase B: cold boot of the committed NAND alone.
  reboot <- startConsole python (receipt </> "reboot.stderr")
    ([ "--profile", "firmware/x-m1/1.50/emulator-rec.json", "--installed-nand", exported
     , "--menus", "--pause-after", "0", "--output", receipt </> "reboot-frames"
     ] ++ fixedSceneFrameArguments (fixedSceneFrameOption options))
  firstStart <- runUntil reboot firstStartAt
  forM_ firstStartKeys (pressKey reboot)
  shooting <- settle reboot
  cacheAfterReboot <- readCache reboot
  mode <- readWord reboot systemModeWord
  hook <- readWord reboot connectHookWord
  quitConsole reboot

  let storedValue (AreaPage _ bytes) offset = lookup offset bytes
      cacheValue (CacheBytes _ values) offset = lookup offset values
      expectations =
        [ Expectation "flag word & 3 == 3 at 0x00db91e0" (flag .&. 3 == 3) (toJSONHex flag)
        , Expectation "template reload 0x0069fe20 and whole-area commit 0x00db932c ran"
            (all (\site -> any ((== siteText site) . hitAddress) hits) [templateReloadSite, wholeAreaCommitSite]) Null
        , Expectation "stored 0xf7 before == 1" (storedValue storedBefore 0xf7 == Just 1) (toJSONMaybe (storedValue storedBefore 0xf7))
        , Expectation "stored 0xf7 after == 0" (storedValue storedAfter 0xf7 == Just 0) (toJSONMaybe (storedValue storedAfter 0xf7))
        , Expectation "cache 0xf7 after reboot == stored 0xf7 after"
            (cacheValue cacheAfterReboot 0xf7 == storedValue storedAfter 0xf7) (toJSONMaybe (cacheValue cacheAfterReboot 0xf7))
        ]
      document = object
        [ "schema" .= (1 :: Int)
        , "scope" .= ("emulated X-M1 1.50 ordinary updater over a committed NAND, then a cold boot of the committed result" :: T.Text)
        , "inputs" .= object inputHashes
        , "updater" .= object
            [ "frames" .= [object ["step" .= name, "png_sha256" .= frame] | (name, frame) <- updaterFrames]
            , "cache_at_updater_screen" .= cacheJson cacheAtScreen
            , "completion_flag_word_0x014e3578" .= toJSONHex flag
            , "hits" .= map hitRaw hits
            , "cache_after_commit" .= cacheJson cacheAfterCommit
            , "nand_at_idle" .= object ["busy" .= nandBusy finalNand, "transactions" .= nandTransactions finalNand]
            ]
        , "stored_before" .= areaJson storedBefore
        , "stored_after" .= areaJson storedAfter
        , "area_first_block_tag" .= let BlockTag bytes = tag in concatMap (\b -> let h = showHex b "" in if length h == 1 then '0' : h else h) (B.unpack bytes)
        , "exported_nand" .= object ["path" .= exported, "sha256" .= exportedHash]
        , "reboot" .= object
            [ "first_start_frame" .= progressFrame firstStart
            , "shooting_frame" .= progressFrame shooting
            , "instructions" .= let Instructions count = progressInstructions shooting in count
            , "cache" .= cacheJson cacheAfterReboot
            , "system_mode_word_0x009570dc" .= mode
            , "connect_hook_word_0x00ecf900" .= toJSONHex hook
            ]
        , "expectations" .= map expectationJson expectations
        ]
  BL.writeFile (receipt </> "receipt.json") (encode document)
  C.putStrLn (BL.toStrict (encode document))
  let failed = [name | Expectation name passed _ <- expectations, not passed]
  unless (null failed) $ do
    hPutStrLn stderr ("Failed expectations: " ++ show failed)
    exitFailure
  where
    toJSONHex value = String (T.pack ("0x" ++ showHex value ""))
    toJSONMaybe = maybe Null (Number . fromIntegral)
    settle console = do
      start <- runChunk console
      let Instructions from = progressInstructions start
      runUntil console (from + shootingSettle - continuationChunk)

main :: IO ()
main = (getArgs >>= parseCommand >>= run) `catch` report
  where
    run Help = putStr usage
    run (Validate firmware) = validateFirmware firmware
    run (Launch options) = launch options
    report :: IOException -> IO ()
    report failure = hPutStrLn stderr (displayException failure) >> exitFailure
