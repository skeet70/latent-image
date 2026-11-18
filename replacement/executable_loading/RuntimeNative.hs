{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (object, (.=))
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL)
import Data.Char (isSpace)
import Data.List (nubBy)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import Data.Word (Word32)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex)
import System.Directory (makeAbsolute, removeDirectoryRecursive)
import System.Environment (getArgs, getEnv)

sink, resultAddress, callbackAddress, helperAddress :: Word32
sink = 0x250000
resultAddress = 0x260000
callbackAddress = 0x240000
helperAddress = 0x241000

address :: Word32 -> GuestAddress
address = GuestAddress . fromIntegral

region :: String -> Word32 -> Int -> Permission -> B.ByteString -> CaseRegion
region name base size = mkRegion (RegionId name) (address base) size

put :: B.ByteString -> (Int, B.ByteString) -> B.ByteString
put bytes (offset, value) = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

wordsAt :: Int -> [(Int, Word32)] -> B.ByteString
wordsAt size = foldl put (B.replicate size 0) . map (\(offset, value) -> (offset, word32le value))

wide :: Word32 -> Word32 -> [Word32]
wide reg value =
  [ 0xe3000000 .|. (reg `shiftL` 12) .|. ((value .&. 0xf000) `shiftL` 4) .|. (value .&. 0xfff)
  , 0xe3400000 .|. (reg `shiftL` 12) .|. (((value `div` 65536) .&. 0xf000) `shiftL` 4) .|. ((value `div` 65536) .&. 0xfff)
  ]

ret :: Word32 -> B.ByteString
ret value = assemble (wide 0 value ++ [0xe12fff1e])

branch :: Word32 -> Word32 -> Word32
branch site target = 0xea000000 .|. (fromIntegral ((toInteger target - toInteger site - 8) `div` 4) .&. 0xffffff)

createChannel, createTask, sendMessage, receiveMessage, callback, progress, clockMask :: B.ByteString
createChannel = assemble $
  wide 12 sink ++ [0xe58c0000, 0xe5912000, 0xe58c2004, 0xe5912004, 0xe58c2008, 0xe3a00000, 0xe12fff1e]
createTask = assemble $
  wide 12 sink ++ concat [[0xe5902000 + offset, 0xe58c2020 + offset] | offset <- ([0,4..24] :: [Word32])] ++ [0xe3a00000, 0xe12fff1e]
sendMessage = assemble $
  wide 12 sink ++
  [0xe58c0050, 0xe5912000, 0xe58c2054, 0xe5912004, 0xe58c2058, 0xe5913008] ++
  concat [[0xe5932000 + offset, 0xe58c2060 + offset] | offset <- ([0,4] :: [Word32])] ++
  [0xe3a00000, 0xe12fff1e]
receiveMessage = assemble $
  wide 12 sink ++
  [0xe58c0080, 0xe58c2084, 0xe59c2054, 0xe5812000, 0xe3a02008, 0xe5812004, 0xe5913008] ++
  concat [[0xe59c2060 + offset, 0xe5832000 + offset] | offset <- ([0,4] :: [Word32])] ++
  [0xe3a00000, 0xe12fff1e]
callback = assemble $ wide 12 sink ++ [0xe59c30a0, 0xe2833001, 0xe58c30a0, 0xe58c00a4, 0xe58c10a8, 0xe12fff1e]
progress = assemble $ wide 12 sink ++ [0xe59c30b0, 0xe2833001, 0xe58c30b0, 0xe58c00b4, 0xe3a00000, 0xe12fff1e]
clockMask = assemble $
  wide 12 0xfff91000 ++ [0xe59c3000, 0xe2833001, 0xe58c3000] ++
  wide 12 0x916528 ++ [0xe59c3000, 0xe3530002, 0x1a000008] ++
  wide 12 sink ++ [0xe59c20c0, 0xe3520000, 0x1a000003] ++
  wide 12 0xfff52004 ++ [0xe3a03000, 0xe58c3000, 0xe3a00000, 0xe12fff1e]

stubBodies :: [(Word32, B.ByteString)]
stubBodies =
  [ (0x724cdc, createChannel)
  , (0x725dd8, createTask)
  , (0x72503c, sendMessage)
  , (0x725318, receiveMessage)
  , (0x723cb8, ret 60)
  , (0x724b68, ret 0)
  , (0x724c00, ret 0)
  , (0x657114, ret 0x230000)
  , (0x6571a0, ret 0x230100)
  , (0x657184, ret 0)
  , (0x73b924, progress)
  , (0x723ce8, ret 0)
  , (0x6499d0, ret 0)
  , (callbackAddress, callback)
  , (helperAddress, clockMask)
  ]

lowStubs :: B.ByteString
lowStubs = foldl put (B.replicate 0x1000 0)
  [ (0x560, word32le (branch 0x42560 helperAddress))
  , (0x568, assemble [0xe12fff1e])
  , (0x634, assemble [0xe12fff1e])
  ]

parseWord :: String -> Maybe Word32
parseWord text = case readHex text of
  [(value, rest)] | all isSpace rest && value <= (0xffffffff :: Integer) -> Just (fromIntegral value)
  _ -> Nothing

textFunctions :: FilePath -> Word32 -> Word32 -> IO [(GuestAddress, Word32)]
textFunctions path start end = do
  rows <- lines <$> readFile path
  let parse row = case words row of
        [a, z, kind, _] | kind `elem` ["t", "T"] -> do
          base <- parseWord a
          size <- parseWord z
          if size > 0 && base >= start && base + size <= end then Just (address base, size) else Nothing
        _ -> Nothing
  pure (nubBy (\(left, _) (right, _) -> left == right) (mapMaybe parse rows))

runtimeBounds :: FilePath -> IO (Word32, Word32, Word32, Word32, Word32)
runtimeBounds path = do
  symbols <- parseSymbols ["__fixed_body_start", "__fixed_body_end", "__fixed_bss_start", "__fixed_bss_end", "xm1_loading_qualification_drive"] path
  let word name = guestWord (symbolAddress symbols name)
      (_, entryBytes) = functionSymbol symbols "xm1_loading_qualification_drive"
  pure (word "__fixed_body_start", word "__fixed_body_end", word "__fixed_bss_start", word "__fixed_bss_end", entryBytes)

prepareFixture :: String -> FilePath -> FilePath -> IO (CaseSpec, Word32, (Int, Int))
prepareFixture role bodyPath symbolsPath = do
  body <- B.readFile bodyPath
  (bodyStart, bodyEnd, bssStart, bssEnd, _) <- runtimeBounds symbolsPath
  unless (bodyStart + fromIntegral (B.length body) == bodyEnd && bssStart >= bodyEnd && bssEnd >= bssStart)
    (fail "loading runtime body/BSS bounds differ")
  symbols <- parseSymbols ["xm1_loading_qualification_drive", "xm1_loading_task", "XM1_LOADING_DEADLINE_CALLS", "XM1_LOADING_ENVELOPES"] symbolsPath
  bodyFunctions <- textFunctions symbolsPath bodyStart bodyEnd
  let entry = guestWord (symbolAddress symbols "xm1_loading_qualification_drive")
      taskEntry = guestWord (symbolAddress symbols "xm1_loading_task")
      deadlineCalls = guestWord (symbolAddress symbols "XM1_LOADING_DEADLINE_CALLS")
      envelopes = guestWord (symbolAddress symbols "XM1_LOADING_ENVELOPES")
      stubs = [(address stubBase, fromIntegral (B.length bytes)) | (stubBase, bytes) <- stubBodies]
      lowFunctions = [(address 0x42560, 4), (address 0x42568, 4), (address 0x42634, 4)]
      fixture = fixtureTemplate role body bodyStart bssStart bssEnd entry (bodyFunctions ++ stubs ++ lowFunctions)
  unless (deadlineCalls >= bssStart && deadlineCalls + 4 <= bssEnd && envelopes >= bssStart && envelopes + 9 * 24 <= bssEnd)
    (fail "deadline counter or envelope records leave runtime BSS")
  pure (fixture, taskEntry, (fromIntegral (deadlineCalls - bssStart), fromIntegral (envelopes - bssStart)))
  where
    fixtureTemplate role' body bodyStart bssStart bssEnd entry functions = CaseSpec
      { caseId = "loading-runtime-" ++ role'
      , caseProvenance = "Synthetic bounded RTOS and synchronous-copy host fixture around the actual linked ARM loading runtime. The case executes configured startup, Sender envelope construction, Receiver decode, NativeProducer/NandReadPorts stop/drain/publication and the legacy callback. External quiescence is explicit artifact policy."
      , caseExclusion = "Host-only conditional synchronous-copy/quiescence fixture; not physical NAND quiescence evidence, reset reachability or a production deadline value."
      , caseEntry = address entry
      , caseExit = address 0x100000
      , caseRegions = [region "return" 0x100000 8 ReadExecute (assemble [0xe320f000, 0xe320f000]),
          region "body" bodyStart (fromIntegral (B.length body)) ReadExecute body,
          region "bss" bssStart (fromIntegral (bssEnd - bssStart)) ReadWrite B.empty,
          region "low-stubs" 0x42000 0x1000 ReadExecute lowStubs] ++
          [region ("stub-" ++ show index) baseAddress (B.length bytes) ReadExecute bytes | (index, (baseAddress, bytes)) <- zip [(0::Int)..] stubBodies] ++
          [region "skip-table" 0x4000 0x1000 ReadWrite (wordsAt 0x1000 [(0x310, 0)]),
           region "boot-header" 0x4061c000 0x1000 ReadWrite (wordsAt 0x1000 [(0x00c, 0x4061c184), (0x188, 0x4310)]),
           region "descriptor" 0x230000 0x1000 ReadOnly (wordsAt 0x1000 [(0,6),(4,0x1100000),(8,0x360000),(12,0),(16,0),(20,0x5a0000),(24,0x360000),(0x100,0),(0x104,0x1100000),(0x108,0x360000)]),
           region "sink" sink 0x1000 ReadWrite (wordsAt 0x1000 [(0xc0, if role' == "retention" then 1 else 0)]), region "result" resultAddress 0x1000 ReadWrite B.empty,
           region "stack" 0x300000 0x20000 ReadWrite B.empty, region "destination" 0x1100000 0x360000 ReadWrite B.empty,
           region "service-semaphores" 0x817000 0x1000 ReadWrite B.empty, region "controller-tag" 0x812000 0x1000 ReadWrite B.empty,
           region "task-state" 0x914000 0x1000 ReadWrite B.empty, region "nand-state" 0x916000 0x1000 ReadWrite B.empty,
           region "done" 0x944000 0x1000 ReadWrite B.empty, region "mailbox" 0x957000 0x1000 ReadWrite B.empty,
           region "kernel-root" 0x96b000 0x1000 ReadWrite B.empty,
           region "core0" 0x400c0000 0x1000 ReadWrite (wordsAt 0x1000 [(0x804,17)]),
           region "counter" 0xfff91000 0x1000 ReadWrite B.empty, region "nand" 0xfff52000 0x1000 ReadWrite B.empty,
           region "handle-counter" 0x409c2000 0x1000 ReadWrite B.empty, region "critical-count" 0x40a0f000 0x1000 ReadWrite B.empty,
           region "nand-clock-gate" 0xfff88000 0x1000 ReadWrite B.empty, region "device-enable" 0x941000 0x1000 ReadWrite B.empty]
      , caseFunctions = functions
      , caseInitial = [(R0,3),(R1,callbackAddress),(R2,0x12345678),(R3,resultAddress),(Sp,0x320000),(Lr,0x100000),(Cpsr,0x13)] ++ [(reg,0) | reg <- [R4 .. R12]]
      , caseExpectedStatus = Returned, caseExpectedRegisters = [(R0,0)], caseExpectedMemory = [], caseIgnoredMemory = []
      , caseCp14Identity = Nothing, caseKernel = Just (KernelCase (InstructionLimit 16000000) Nothing Nothing Nothing Nothing [] [])
      }

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner, successBody, successSymbols, retentionBody, retentionSymbols, output, "--native-jobs", "1"] -> do
      prepared <- sequence [prepareFixture "success" successBody successSymbols, prepareFixture "retention" retentionBody retentionSymbols]
      let specs = [spec | (spec, _, _) <- prepared]
          taskEntries = [taskEntry | (_, taskEntry, _) <- prepared]
          offsets = [offset | (_, _, offset) <- prepared]
      work <- getEnv "XM1_RUN_WORK"
      batch <- writeBatchWithProfile Resource8 (work ++ "/loading-runtime.cases") specs
      observed <- loadObservedBatch runner batch
      runnerBytes <- B.readFile runner
      root <- makeAbsolute output
      results <- forM (zip (zip3 specs taskEntries offsets) (map snd observed)) $ \((spec, taskEntry, (deadlineOffset, envelopeOffset)), observation) -> do
        let memory name = regionBytesOf observation (RegionId name)
            completion = [readWordAt (memory "result") offset | offset <- [0,4..28]]
            transport = [readWordAt (memory "sink") offset | offset <- [0,4..8]]
            task = [readWordAt (memory "sink") offset | offset <- [0x20,0x28,0x2c,0x30]]
            message = [readWordAt (memory "sink") offset | offset <- [0x50,0x54,0x58]]
            payload = [readWordAt (memory "sink") offset | offset <- [0x60,0x64]]
            envelope = [readWordAt (memory "bss") (envelopeOffset + 3 * 24 + offset) | offset <- [0,4..20]]
            callbackWords = [readWordAt (memory "sink") offset | offset <- [0xa0,0xa4,0xa8]]
            retained = caseId spec == "loading-runtime-retention"
            expectedCompletion = if retained then [3,1,3,10,0,0,0,0x6201] else [0,1,3,0,0,0x360000,0,0]
            expectedCallback = if retained then [1,0x12345678,1] else [1,0x12345678,0]
            state = readWordAt (memory "nand-state") 0x528
            handle = readWordAt (memory "nand-state") 0x538
            token = readWordAt (memory "nand-state") 0x540
            done = readWordAt (memory "done") 0x44c
            busy = readWordAt (memory "mailbox") 0x600
            progressWords = [readWordAt (memory "sink") 0xb0, readWordAt (memory "sink") 0xb4]
            progressOk = if retained then progressWords == [0,0] else progressWords == [28,0x360000]
            deadlineCalls = readWordAt (memory "bss") deadlineOffset
            lifecycle = if retained then state == 2 && handle /= 0 && token == 0 && done == 0 && busy == 1 else state == 0 && handle == 0 && token == 0 && done == 1 && busy == 0
            passed = nativeStatus observation == Returned && completion == expectedCompletion && callbackWords == expectedCallback
              && transport == [60,48,16] && task == [60,4,0xc00,taskEntry] && message == [60,0x3c3c4c03,8] && payload == [1,3]
              && take 2 envelope == [1,1] && envelope !! 2 /= 0 && envelope !! 4 == callbackAddress && envelope !! 5 == 0x12345678
              && lifecycle && progressOk && deadlineCalls == 1
        pure (passed, object ["case" .= caseId spec, "passed" .= passed, "status" .= runnerStatusLabel (nativeStatus observation),
          "completion_words" .= completion, "transport_words" .= transport, "task_words" .= task, "message_words" .= message,
          "payload_words" .= payload, "envelope_words" .= envelope, "callback_words" .= callbackWords, "progress_words" .= progressWords,
          "state" .= state, "handle" .= handle,
          "token" .= token, "done" .= done, "busy" .= busy, "deadline_calls" .= deadlineCalls,
          "stop_registers" .= [(show register, value) | (register, value) <- M.toList (nativeRegisters observation)],
          "engine_error" .= show (nativeEngineError observation)])
      successBytes <- B.readFile successBody
      retentionBytes <- B.readFile retentionBody
      B.writeFile root (encodeCanonical (object ["schema" .= (1::Int), "passed" .= all fst results,
        "runner_sha256" .= sha256Hex (sha256 runnerBytes), "success_body_sha256" .= sha256Hex (sha256 successBytes),
        "retention_body_sha256" .= sha256Hex (sha256 retentionBytes), "results" .= map snd results,
        "scope" .= ("conditional-host-synchronous-copy-and-explicit-quiescence" :: String)]))
      unless (all fst results) (fail ("loading runtime native qualifier differs: " ++ root))
      removeDirectoryRecursive (work ++ "/loading-runtime.cases")
      putStrLn "actual linked ARM loading Sender/Receiver/producer success and retained-quiescence control pass"
    _ -> fail "usage: loading-runtime-native RUNNER SUCCESS_BIN SUCCESS_SYMBOLS RETENTION_BIN RETENTION_SYMBOLS REPORT --native-jobs 1"
