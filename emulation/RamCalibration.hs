{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE BangPatterns #-}
module Main (main) where

import Control.Exception (IOException, bracket, catch, displayException)
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, encode, object, (.=))
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (nubBy, sortOn)
import Emulation.EmulatorSlot (reexecInSlot, slotPolicyFromEnvironment)
import Emulation.ViewerRuntime (DefaultRuntime (..), Firmware (..), Python (..), Runtime (..), StagedRuntime (..), StockOriginal (..), freshRuntime, resolve, stageViewerRuntime, stagedFirmwarePath, validateFirmware, workspace)
import EmulatorUsb (EmulatorStart (..), emulatorCapture, emulatorReadMemory, startEmulator, stopEmulator)
import FujiModPaths (fujimodRamPushBridge, fujimodRun)
import Numeric (readHex, showHex)
import System.Directory (doesFileExist)
import System.Environment (getArgs, getExecutablePath)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (IOMode (WriteMode), hPutStrLn, stderr, withBinaryFile)
import System.Process (callProcess)

newtype Address = Address Int deriving (Eq, Ord, Show)
data Engine = Retained | FujiMod deriving (Eq, Show)
newtype RegionName = RegionName String
data Region = Region RegionName Address Address

captureBytes :: Int
captureBytes = 0x05aa0000

regions :: [Region]
regions =
  [ region "low-boot-and-scratch" 0 0x40000
  , region "kernel-a-slot" 0x40000 0x60000
  , region "kernel-a-runtime-and-log" 0x60000 0xd0000
  , region "kernel-b-slot" 0xd0000 0xf0000
  , region "kernel-b-runtime" 0xf0000 0x130000
  , region "low-unused-or-workspace" 0x130000 0x61c000
  , region "segment-table" 0x61c000 0x61c400
  , region "image-4" 0x61c400 0x89ec00
  , region "image-4-bss-and-static-stacks" 0x89ec00 0xa71a00
  , region "libc-heap-reservation" 0xa71a00 0xbf1a00
  , region "image-5" 0xbf1a00 0x10e3860
  , region "image-5-tail-gap" 0x10e3860 0x1100000
  , region "plain-image" 0x1100000 0x1460000
  , region "application-heaps-and-stacks" 0x1460000 0x21a1800
  , region "face-workspace" 0x21a1800 0x21afc00
  , region "graphics-and-image-workspace" 0x21afc00 0x3360000
  , region "usb-fill-before-display" 0x3360000 0x3700000
  , region "display-and-live-view" 0x3700000 0x4a00000
  , region "usb-fill-after-display" 0x4a00000 0x5aa0000
  ]
  where region name begin end = Region (RegionName name) (Address begin) (Address end)

hex :: Int -> String
hex n = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex n ""

summary :: B.ByteString -> B.ByteString -> Region -> Value
summary body model (Region (RegionName name) (Address begin) (Address end)) = object
  [ "region" .= name, "begin" .= hex begin, "end" .= hex end, "bytes" .= (end - begin)
  , "different_bytes" .= different, "different_words" .= wordsDifferent
  , "body_zero_bytes" .= B.count 0 expected, "model_zero_bytes" .= B.count 0 actual
  , "body_ff_bytes" .= B.count 255 expected, "model_ff_bytes" .= B.count 255 actual
  , "first_different_addresses" .= map hex first ]
  where
    expected = B.take (end - begin) (B.drop begin body)
    actual = B.take (end - begin) (B.drop begin model)
    (!different, !wordsDifferent, first) = go begin 0 0 []
    go :: Int -> Int -> Int -> [Int] -> (Int, Int, [Int])
    go !address !bytes !words' samples
      | address >= end = (bytes, words', reverse samples)
      | otherwise =
          let offsets = [address .. min (end - 1) (address + 3)]
              changed = filter (\i -> B.index body i /= B.index model i) offsets
              count = length changed
              samples' = if length samples < 16 then reverse (take (16 - length samples) changed) ++ samples else samples
          in go (address + 4) (bytes + count) (words' + if count == 0 then 0 else 1) samples'

wordAt :: B.ByteString -> Int -> Int
wordAt bytes address = foldr (\i value -> value `shiftL` 8 .|. fromIntegral (B.index bytes i)) 0 [address .. address + 3]

physical :: Int -> Int
physical address = address `mod` 0x40000000

newtype CoreId = CoreId Int
newtype TaskId = TaskId Int
data Task = Task { taskCore :: CoreId, taskId :: TaskId, taskEntry :: Address, taskStack :: Region }

tasks :: B.ByteString -> Either String [Task]
tasks bytes = filter ((/= Address 0) . taskEntry) . concat <$> mapM table [(0, 0x54610), (1, 0x54694)]
  where
    inside address count = address >= 0 && count > 0 && address <= captureBytes - count
    table (core, pointer) = do
      let base = physical (wordAt bytes pointer)
          count = wordAt bytes (pointer + 4)
      unless (count >= 1 && count <= 128 && inside base (count * 0x38)) (Left ("invalid CPU" ++ show core ++ " task table"))
      forM [0 .. count - 1] $ \index -> do
        let descriptor = physical (wordAt bytes (base + index * 0x38 + 8))
        unless (inside descriptor 0x28) (Left "task descriptor is outside capture")
        let identifier = wordAt bytes (descriptor + 32)
            stack = physical (wordAt bytes (descriptor + 20))
            size = wordAt bytes (descriptor + 16)
            entry = Address (physical (wordAt bytes (descriptor + 8)))
        unless (inside stack size) (Left "task stack is outside capture")
        pure (Task (CoreId core) (TaskId identifier) entry (Region (RegionName ("cpu" ++ show core ++ "-task-" ++ show identifier)) (Address stack) (Address (stack + size))))

taskSummaries :: B.ByteString -> B.ByteString -> Value
taskSummaries body model = case (tasks body, tasks model) of
  (Right bodyTasks, Right modelTasks) -> object
    [ "body_tasks" .= map describe bodyTasks, "model_tasks" .= map describe modelTasks
    , "body_stack_extents" .= map (summary body model) (stackRegions bodyTasks)
    , "classification" .= ("Timing-dependent candidate: stack contents and high-water marks are not deterministic across unaligned boots" :: String) ]
  (bodyTasks, modelTasks) -> object ["error" .= show (fmap length bodyTasks, fmap length modelTasks)]
  where
    describe task =
      let Region _ (Address base) (Address end) = taskStack task
          Address entry = taskEntry task
          CoreId core = taskCore task
          TaskId identifier = taskId task
      in object ["core" .= core, "task_id" .= identifier, "entry" .= hex entry, "stack_base" .= hex base, "stack_bytes" .= (end - base)]

stackRegions :: [Task] -> [Region]
stackRegions = nubBy sameExtent . sortOn start . map taskStack
  where
    start (Region _ begin _) = begin
    sameExtent (Region _ begin end) (Region _ begin' end') = begin == begin' && end == end'

data DifferenceClass = TimingDependent | Configuration | ModelDefect | Unresolved deriving (Eq, Show, Enum, Bounded)

classification :: B.ByteString -> B.ByteString -> Value
classification body model = object
  [ "counts" .= [object ["class" .= show kind, "different_bytes" .= count] | (kind, count) <- totals]
  , "rules" .=
      [ "TimingDependent: body-derived task stack extents and the firmware task-id allocator counter at 0x00828094. Stack contents are candidates, not proof that each byte's cause is timing." :: String
      , "Configuration: control-table calibration interval words (+0x10 of 36 rows at 0x00828120, stride 28), and the recorder remainder [0x0144a5e0,0x0144ad48). The recorder classification is specific to Run 3's independently checked rev-2 identity."
      , "ModelDefect: requires deterministic firmware-derived disagreement after input and timing causes are excluded. No rule asserts one from a single RAM comparison."
      , "Unresolved: all remaining changed bytes, including scene buffers, allocation layout, logs and uninitialized memory. These are not counted as model defects." ] ]
  where
    stacks = either (const []) stackRegions (tasks body)
    ranges = [(begin, end, TimingDependent) | Region _ (Address begin) (Address end) <- stacks]
      ++ [(0x828094, 0x828098, TimingDependent), (0x144a5e0, 0x144ad48, Configuration)]
      ++ [(0x828120 + row * 28 + 16, 0x828120 + row * 28 + 20, Configuration) | row <- [0 .. 35]]
    selected = foldl' add [(kind, 0 :: Int) | kind <- [minBound .. maxBound]] ranges
    add counts (begin, end, kind) =
      let count = differences begin end
      in map (\(key, value) -> (key, value + if key == kind then count else 0)) counts
    differences begin end = go begin 0
      where
        go !address !count
          | address == end = count
          | otherwise = go (address + 1) (count + if B.index body address == B.index model address then 0 else 1)
    changed = differences 0 captureBytes
    accounted = sum (map snd selected)
    totals = map (\(kind, count) -> (kind, if kind == Unresolved then changed - accounted else count)) selected

inspectWords :: FilePath -> FilePath -> Int -> Int -> IO ()
inspectWords bodyPath modelPath begin end = do
  root <- workspace Nothing
  body <- B.readFile (resolve root bodyPath)
  model <- B.readFile (resolve root modelPath)
  unless (begin >= 0 && begin `mod` 4 == 0 && end `mod` 4 == 0 && begin < end && end <= min (B.length body) (B.length model))
    (ioError (userError "word inspection requires an aligned range inside both files"))
  BL.putStr (encode [ object ["address" .= hex address, "body" .= hex expected, "model" .= hex actual]
                     | address <- [begin, begin + 4 .. end - 4], let expected = wordAt body address, let actual = wordAt model address, expected /= actual ] <> "\n")

recorderReferences :: FilePath -> FilePath -> IO ()
recorderReferences path output = do
  root <- workspace Nothing
  bytes <- B.readFile (resolve root path)
  unless (B.length bytes == captureBytes) (ioError (userError "references requires the complete low-RAM capture"))
  let low = 0x0144a5e0
      high = 0x0144ad48
      target value = value < 0x80000000 && physical value >= low && physical value < high
      self address = address >= low && address < high
      hit kind address value = object ["kind" .= (kind :: String), "address" .= hex address, "target" .= hex value, "inside_recorder" .= self address]
      pointers = [hit "raw-pointer-byte-aligned" address value | address <- [0 .. captureBytes - 4], let value = wordAt bytes address, target value]
      sourceWords = concat [[begin, begin + 4 .. end - 4] | (begin, end) <- [(0x61c340, 0x89ec00), (0xbf1a00, 0x10e3860), (0x1100000, 0x1460000)]]
      branches =
        [hit "a32-immediate-branch-candidate" address destination
        | address <- sourceWords, let word = wordAt bytes address
        , word .&. 0x0e000000 == 0x0a000000, word `shiftR` 28 /= 15
        , let immediate = word .&. 0xffffff
        , let signed = if immediate .&. 0x800000 /= 0 then immediate - 0x1000000 else immediate
        , let destination = (address + 8 + signed * 4) `mod` 0x100000000, target destination ]
      constructors =
        [hit "a32-movw-movt-pair-candidate" address destination
        | address <- sourceWords, let top = wordAt bytes address, top .&. 0x0ff00000 == 0x03400000
        , previous <- [max 0 (address - 64), max 0 (address - 64) + 4 .. address - 4]
        , let bottom = wordAt bytes previous, bottom .&. 0x0ff00000 == 0x03000000
        , top .&. 0xf000f000 == bottom .&. 0xf000f000
        , let immediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
        , let destination = (immediate top `shiftL` 16) .|. immediate bottom, target destination ]
      hooks = [object ["address" .= hex address, "word" .= hex (wordAt bytes address)] | address <- [0x71feb4, 0x73b564, 0x61c1fc, 0x61c340]]
  BL.writeFile (resolve root output) (encode (object
    ["schema" .= (1 :: Int), "capture" .= path, "target_begin" .= hex low, "target_end" .= hex high,
     "pointers" .= pointers, "branches" .= branches, "constructors" .= constructors, "hook_words" .= hooks,
     "limits" .= ("Raw pointer scan covers every byte offset in captured RAM, including DRAM aliases. Branch candidates cover aligned A32 B/BL in the three loaded images; data can decode as instructions. MOVW/MOVT candidates use matching register and condition within 64 bytes without liveness filtering. Arbitrary computed targets, Thumb code and transient overwritten references are not excluded." :: String)]) <> "\n")

compareDumps :: FilePath -> [(Engine, FilePath)] -> FilePath -> IO ()
compareDumps bodyPath models output = do
  root <- workspace Nothing
  body <- checked (resolve root bodyPath)
  results <- forM models $ \(engine, path) -> do
    model <- checked (resolve root path)
    pure (object ["engine" .= show engine, "path" .= path, "regions" .= map (summary body model) regions, "tasks" .= taskSummaries body model, "classification" .= classification body model])
  BL.writeFile (resolve root output) (encode (object ["schema" .= (1 :: Int), "body" .= bodyPath, "scope" .= ("Raw non-atomic body capture versus paused post-ff80 emulator RAM; differences are not automatically model defects" :: String), "engines" .= results]) <> "\n")
  where
    checked path = do
      bytes <- B.readFile path
      unless (B.length bytes == captureBytes) (ioError (userError (path ++ ": expected exactly 0x05aa0000 bytes")))
      pure bytes

capture :: Engine -> FilePath -> FilePath -> FilePath -> IO ()
capture engine datSupplied nandSupplied output = do
  root <- workspace Nothing
  let dat = resolve root datSupplied
      nand = resolve root nandSupplied
      rom = resolve root "private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin"
  validateFirmware (Firmware dat)
  exists <- doesFileExist nand
  unless exists (ioError (userError "NAND input is absent"))
  args <- getArgs
  slotPolicyFromEnvironment >>= \policy -> reexecInSlot policy args
  Runtime directory <- freshRuntime root (Just (Runtime output)) (UniqueRuntime "ram-calibration")
  (executable, working, arguments) <- case engine of
    FujiMod -> pure (fujimodRamPushBridge, directory,
      ["--run", fujimodRun, "--boot-rom", rom, "--installed-dat", dat, "--adjustment-nand", nand,
       "--output", directory </> "bridge", "--ram-push-checkpoints", directory </> "checkpoints", "--fresh-boot"])
    Retained -> do
      staged <- stageViewerRuntime root Nothing (UniqueRuntime "ram-calibration-retained") (Just (Firmware dat)) StageStockOriginal
      let Python python = stagedPython staged
      callProcess python ["tools/firmware_registry.py", "unpack", "xm1-150"]
      binary <- getExecutablePath
      let bridge = (binary ++ ".runfiles") </> "_main/emulation/cardless-ram-push-bridge"
      pure (bridge, stagedDirectory staged,
        ["--profile", "firmware/x-m1/1.50/emulator-rec.json", "--installed-dat", stagedFirmwarePath,
         "--adjustment-nand", nand, "--output", directory </> "bridge", "--ram-push-checkpoints", directory </> "checkpoints",
         "--ui-conversion-profile", "declared-ui-cpu-fallback", "--fresh-boot"])
  bracket (startEmulator executable working arguments (\_ -> pure (Nothing, ()))) (\(emulator, _, _) -> stopEmulator emulator) $ \(emulator, start, ()) -> do
    frame <- emulatorCapture emulator "post-ff80"
    withBinaryFile (directory </> "ram-00000000-05aa0000.bin") WriteMode $ \handle ->
      forM_ [0, 0x10000 .. captureBytes - 1] $ \address -> do
        let count = min 0x10000 (captureBytes - address)
        bytes <- emulatorReadMemory emulator (fromIntegral address) count
        unless (B.length bytes == count) (ioError (userError "short bridge read"))
        B.hPut handle bytes
    BL.writeFile (directory </> "capture.json") (encode (object
      ["schema" .= (1 :: Int), "engine" .= show engine, "firmware" .= dat, "nand" .= nand,
       "card_present" .= False, "checkpoint" .= startRecord start, "frame" .= frame,
       "scope" .= ("Paused ordinary boot after ff80 enumeration, before any RAM allocation or payload" :: String)]) <> "\n")
  putStrLn directory

main :: IO ()
main = (getArgs >>= run) `catch` \problem -> hPutStrLn stderr (displayException (problem :: IOException)) >> exitFailure
  where
    run ["capture", engine, dat, nand, output] = case engine of
      "retained" -> capture Retained dat nand output
      "fujimod" -> capture FujiMod dat nand output
      _ -> ioError (userError "engine must be retained or fujimod")
    run ["words", body, model, begin, end] = do
      first <- number begin
      lastAddress <- number end
      inspectWords body model first lastAddress
    run ["references", body, output] = recorderReferences body output
    run ["compare", body, retained, fujimod, output] = compareDumps body [(Retained, retained), (FujiMod, fujimod)] output
    run ["--help"] = putStrLn (unlines
      ["ram-calibration capture retained|fujimod DAT NAND PRIVATE_OUTPUT"
      ,"ram-calibration compare BODY RETAINED_RAM FUJIMOD_RAM OUTPUT_JSON"
      ,"ram-calibration words BODY MODEL BEGIN_HEX END_HEX"
      ,"ram-calibration references RAM OUTPUT_JSON"
      ,"Classification rules are specific to the X-M1 1.50 Run-3 capture."])
    run _ = ioError (userError "use ram-calibration --help")
    number value = case readHex (if take 2 value == "0x" then drop 2 value else value) of
      [(n, "")] | n >= 0 && n <= captureBytes -> pure n
      _ -> ioError (userError "invalid hexadecimal capture address")
