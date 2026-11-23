-- | The owned save file machine's extent, entries and retained services, and the synthetic
-- saves both roles run through the stock registry dispatch.
module Replacement.SaveFileMachine.Cases
  ( Image (..)
  , images
  , ownedExtent
  , entries
  , stockFunctions
  , unreachableDirections
  , Block (..)
  , blocks
  , frameBase
  , Step (..)
  , Scenario (..)
  , scenarios
  , completion
  , pendingCall
  , openHandle
  , saveWorker
  , cancelWorker
  ) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Word (Word32)
import Replacement.FileService.Native (Extent (..), word32le)

-- | A pinned stock image and the address of its first byte.
data Image = Image {imageLabel :: String, imageBase :: Word32, imageDigest :: String}

images :: [Image]
images =
  [ Image "loader-a" 0x40000 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  , Image "image4" 0x61c400 "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
  , Image "image5" 0xbf1a00 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
  , Image "plain" 0x01100000 "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
  ]

ownedExtent :: Extent
ownedExtent = Extent 0x011d5090 0x011d5df8

-- | The addresses stock code or the file service reaches, with the veneer the owned link places
-- there.
entries :: [(String, Word32)]
entries =
  [ ("sfm_entry_on_deleted", 0x011d50d0)
  , ("sfm_entry_on_closed", 0x011d5100)
  , ("sfm_entry_on_written", 0x011d5130)
  , ("sfm_entry_on_positioned", 0x011d5164)
  , ("sfm_entry_on_opened", 0x011d5198)
  , ("sfm_entry_begin", 0x011d5600)
  , ("sfm_entry_begin_append", 0x011d5688)
  , ("sfm_entry_request_cancel", 0x011d5714)
  , ("sfm_entry_append_buffer", 0x011d57ac)
  , ("sfm_entry_set_output_mode", 0x011d5850)
  , ("sfm_entry_copy_name", 0x011d591c)
  , ("sfm_entry_set_name", 0x011d5934)
  ]

-- | The stock functions in the extent, start and size with literal pools and jump tables.
stockFunctions :: [(Word32, Word32)]
stockFunctions =
  [ (0x011d5090, 28), (0x011d50ac, 36), (0x011d50d0, 48), (0x011d5100, 48), (0x011d5130, 52)
  , (0x011d5164, 52), (0x011d5198, 52), (0x011d51cc, 36), (0x011d51f0, 96), (0x011d5250, 64)
  , (0x011d5290, 100), (0x011d52f4, 72), (0x011d533c, 48), (0x011d536c, 148), (0x011d5400, 184)
  , (0x011d54b8, 136), (0x011d5540, 192), (0x011d5600, 136), (0x011d5688, 140), (0x011d5714, 152)
  , (0x011d57ac, 132), (0x011d5830, 32), (0x011d5850, 92), (0x011d58ac, 56), (0x011d58e4, 56)
  , (0x011d591c, 24), (0x011d5934, 192), (0x011d59f4, 184), (0x011d5aac, 568)
  ]

-- | Conditional branch directions of 'stockFunctions' no input reaches: (branch address,
-- @taken@ or @not-taken@, the bytes that decide it).
unreachableDirections :: [(Word32, String, String)]
unreachableDirections = []

-- | A writable region and its initial bytes before a scenario's patches.
data Block = Block {blockName :: String, blockAddress :: Word32, blockBytes :: B.ByteString}

frameBase :: Word32
frameBase = 0x00400000

-- | The machine RAM, the registry state the stock registration, post and dispatch touch, the
-- completion's status and task words, and a frame holding descriptors and text buffers.
blocks :: [Block]
blocks =
  [ Block "machine" 0x014ddba8 (B.replicate 0x80 0)
  , Block "registry-headers" 0x0096e04c (B.replicate 0x24 0)
  , Block "slots" 0x0095761c (B.replicate 0x180 0)
  , Block "registry-flag" 0x008faccc (B.replicate 4 0)
  , Block "save-status" 0x0154d970 (B.replicate 4 0)
  , Block "save-task" 0x0155ccac (B.replicate 0x10 0)
  , Block "frame" frameBase (B.replicate 0x800 0)
  ]

-- | One driver call, its literal arguments and whether r0 is kept. The driver drains the
-- registry queue after every call.
data Step = Step {stepEntry :: Word32, stepArguments :: [Word32], stepStack :: [Word32], stepReturns :: Bool}

data Scenario = Scenario
  { scenarioName :: String
  , scenarioProvenance :: String
  , scenarioPatches :: [(Word32, B.ByteString)]
  , scenarioSteps :: [Step]
  }

completion, pendingCall, openHandle, saveWorker, cancelWorker :: Word32
completion = 0x00e4e5dc
pendingCall = 0x00280000
openHandle = 0x00004a11
saveWorker = 1
cancelWorker = 2

-- Inputs ------------------------------------------------------------------------------------

descriptorA, descriptorB, textBuffer, nameOut :: Word32
descriptorA = frameBase
descriptorB = frameBase + 0x200
textBuffer = frameBase + 0x400
nameOut = frameBase + 0x500

savedPath, outputMode, taskWord :: Word32
savedPath = 0x014ddba8
outputMode = 0x014ddbe4
taskWord = 0x0155ccb8

text :: String -> B.ByteString
text value = BC.pack value <> B.singleton 0

newName :: String
newName = "L:\\DCIM\\100_FUJI\\DSCF0001"

-- | A descriptor of @kind@ at @address@: stale handle, write index and append flag, the scatter
-- list @list@ (padded with zero entries), @name@ and the prefix source word.
descriptor :: Word32 -> Word32 -> [(Word32, Word32)] -> String -> Word32 -> (Word32, B.ByteString)
descriptor address kind list name source = (address, bytes)
  where
    entriesBytes = B.concat [word32le d <> word32le s | (d, s) <- take 16 (list ++ repeat (0, 0))]
    nameBytes = B.take 48 (text name <> B.replicate 48 0)
    bytes =
      word32le 0xdead0001 <> word32le kind <> entriesBytes <> word32le 7 <> nameBytes
        <> B.replicate (0x148 - 0x8c - 48) 0x5a
        <> word32le source <> word32le 0xff

threeBuffers :: [(Word32, Word32)]
threeBuffers = [(0x10000000, 0x2000), (0x10100000, 0), (0x10200000, 0x6c)]

word :: Word32 -> B.ByteString
word = word32le

call :: Word32 -> [Word32] -> Step
call entry arguments = Step entry arguments [] False

returning :: Word32 -> [Word32] -> Step
returning entry arguments = Step entry arguments [] True

begin, beginAppend :: Word32 -> Word32 -> Step
begin d c = call 0x011d5600 [saveWorker, d, c]
beginAppend d c = call 0x011d5688 [saveWorker, d, c]

cancel :: Word32 -> Step
cancel pending = call 0x011d5714 [pending]

opened, positioned, written :: Word32 -> Word32 -> Step
opened status value = call 0x011d5198 [status, value]
positioned status value = call 0x011d5164 [status, value]
written status value = call 0x011d5130 [status, value]

closed, deleted :: Word32 -> Step
closed status = call 0x011d5100 [status]
deleted status = call 0x011d50d0 [status]

-- | Open, then one write result per scatter entry up to the terminator, then the close result.
fullSave :: Step -> Int -> [Step]
fullSave start buffers = [start, opened 0 openHandle] ++ [written 0 0x100 | _ <- [1 .. buffers]] ++ [closed 0]

waitingTask :: (Word32, B.ByteString)
waitingTask = (taskWord, word 0x0155aa00)

scenario :: String -> String -> [(Word32, B.ByteString)] -> [Step] -> Scenario
scenario = Scenario

scenarios :: [Scenario]
scenarios =
  [ scenario "save-jpeg" "A JPEG save through 0x011d5600: name, open, three scatter entries (the second with size 0), close and completion 0 to a waiting task."
      [descriptor descriptorA 0 threeBuffers newName 0, waitingTask]
      (fullSave (begin descriptorA completion) 3)
  , scenario "save-jpeg-no-task" "The same save with no task recorded at 0x0155ccb8."
      [descriptor descriptorA 0 threeBuffers newName 0]
      (fullSave (begin descriptorA completion) 3)
  , scenario "save-jpeg-no-completion" "A save started with a null completion."
      [descriptor descriptorA 0 [(0x10000000, 0x40)] newName 0]
      (fullSave (begin descriptorA 0) 1)
  , scenario "save-empty-list" "A descriptor whose first entry is empty: entry 0 is still written, then the close."
      [descriptor descriptorA 0 [] newName 0, waitingTask]
      (fullSave (begin descriptorA completion) 1)
  , scenario "save-full-list" "Fifteen scatter entries."
      [descriptor descriptorA 0 [(0x10000000 + 0x1000 * i, 0x10 + i) | i <- [0 .. 14]] newName 0, waitingTask]
      (fullSave (begin descriptorA completion) 15)
  , scenario "save-mpo-code-prefix" "Kind 1 with prefix source 1: the code properties name the file, .MPO."
      [descriptor descriptorA 1 threeBuffers newName 1, waitingTask]
      (fullSave (begin descriptorA completion) 3)
  , scenario "open-raf-version-prefix" "Kind 3 with output mode 1: the version properties name the file, .RAF."
      [descriptor descriptorA 3 threeBuffers newName 0, (outputMode, word 1)]
      [begin descriptorA completion]
  , scenario "open-raw-mode-2" "Kind 6 with output mode 2: .RAW, no prefix."
      [descriptor descriptorA 6 threeBuffers newName 0, (outputMode, word 2)]
      [begin descriptorA completion]
  , scenario "open-wav-mode-3" "Kind 5 with output mode 3: .WAV, no prefix."
      [descriptor descriptorA 5 threeBuffers newName 0, (outputMode, word 3)]
      [begin descriptorA completion]
  , scenario "open-mode-7" "Kind 0 with an output mode outside 0..3 left in RAM."
      [descriptor descriptorA 0 threeBuffers newName 0, (outputMode, word 7)]
      [begin descriptorA completion]
  , scenario "open-named" "Kind 7 opens its name as it stands, mode 1."
      [descriptor descriptorA 7 threeBuffers "L:\\DCIM\\100_FUJI\\DSCF0042.JPG" 0]
      [begin descriptorA completion]
  , scenario "append-raf" "A RAF append through 0x011d5688: open mode 2, seek, the writes, close."
      [descriptor descriptorB 3 threeBuffers "L:\\DCIM\\100_FUJI\\DSCF0001.RAF" 0, waitingTask]
      [beginAppend descriptorB completion, opened 0 openHandle, positioned 0 0, written 0 1, written 0 2, written 0 3, closed 0]
  , scenario "append-jpg-mpo" "Appends of kinds 0 and 1, each started and dispatched in turn: each opens its stored name in mode 2."
      [ descriptor descriptorA 0 threeBuffers "L:\\A.JPG" 0
      , descriptor descriptorB 1 threeBuffers "L:\\B.MPO" 0
      ]
      [beginAppend descriptorA completion, beginAppend descriptorB completion]
  , scenario "append-raw-wav" "Appends of kinds 6 and 5."
      [ descriptor descriptorA 6 threeBuffers "L:\\A.RAW" 0
      , descriptor descriptorB 5 threeBuffers "L:\\B.WAV" 0
      ]
      [beginAppend descriptorA completion, beginAppend descriptorB completion]
  , scenario "open-fails-prepare" "Open result 3: completion -4, report 0x70."
      [descriptor descriptorA 0 threeBuffers newName 0, waitingTask]
      [begin descriptorA completion, opened 3 0]
  , scenario "open-fails-other" "Open result 6: completion -1."
      [descriptor descriptorA 0 threeBuffers newName 0, waitingTask]
      [begin descriptorA completion, opened 6 0]
  , scenario "open-fails-no-completion" "Open result 6 with a null completion."
      [descriptor descriptorA 0 threeBuffers newName 0]
      [begin descriptorA 0, opened 6 0]
  , scenario "seek-fails-prepare" "Seek result 3 on an append: completion -4, report 0x71."
      [descriptor descriptorB 3 threeBuffers "L:\\B.RAF" 0, waitingTask]
      [beginAppend descriptorB completion, opened 0 openHandle, positioned 3 0]
  , scenario "seek-fails-other" "Seek result 8: completion -2."
      [descriptor descriptorB 3 threeBuffers "L:\\B.RAF" 0, waitingTask]
      [beginAppend descriptorB completion, opened 0 openHandle, positioned 8 0]
  , scenario "seek-fails-no-completion" "Seek result 8 with a null completion."
      [descriptor descriptorB 3 threeBuffers "L:\\B.RAF" 0]
      [beginAppend descriptorB 0, opened 0 openHandle, positioned 8 0]
  , scenario "write-fails-prepare" "Second write result 3: release, then the close result completes with -4."
      [descriptor descriptorA 0 threeBuffers newName 0, waitingTask]
      [begin descriptorA completion, opened 0 openHandle, written 0 0x2000, written 3 0, closed 0]
  , scenario "write-fails-other" "First write result 14: release, the close result completes with -2."
      [descriptor descriptorA 0 threeBuffers newName 0, waitingTask]
      [begin descriptorA completion, opened 0 openHandle, written 14 0, closed 0]
  , scenario "close-fails" "Close result 4: completion -3, report 0x72, no saved path."
      [descriptor descriptorA 0 [(0x10000000, 0x40)] newName 0, waitingTask]
      (init (fullSave (begin descriptorA completion) 1) ++ [closed 4])
  , scenario "close-fails-no-completion" "Close result 4 with a null completion."
      [descriptor descriptorA 0 [(0x10000000, 0x40)] newName 0]
      (init (fullSave (begin descriptorA 0) 1) ++ [closed 4])
  , scenario "cancel-while-open" "Cancel after the first write request: release; the write and a late open result notify the owner with 0; the close result runs the pending call."
      [descriptor descriptorA 0 threeBuffers newName 0, waitingTask]
      [begin descriptorA completion, opened 0 openHandle, cancel pendingCall, written 0 0x2000, opened 0 openHandle, closed 0]
  , scenario "cancel-while-open-null" "The same cancel with a null completion and no pending call."
      [descriptor descriptorA 0 threeBuffers newName 0]
      [begin descriptorA 0, opened 0 openHandle, cancel 0, written 0 0x2000, closed 0]
  , scenario "cancel-while-closing" "Cancel while the close is pending: the close result deletes the file, the delete result completes with 0 and runs the pending call."
      [descriptor descriptorA 0 [(0x10000000, 0x40)] newName 0, waitingTask]
      (init (fullSave (begin descriptorA completion) 1) ++ [cancel pendingCall, closed 0, deleted 0])
  , scenario "cancel-while-closing-null" "The same with a null completion and no pending call."
      [descriptor descriptorA 0 [(0x10000000, 0x40)] newName 0]
      (init (fullSave (begin descriptorA 0) 1) ++ [cancel 0, closed 0, deleted 0])
  , scenario "cancel-after-save" "Cancel after a completed save: worker 2 registers, the saved file is deleted, and the delete result clears the path and runs the pending call."
      [descriptor descriptorA 0 [(0x10000000, 0x40)] newName 0, waitingTask]
      (fullSave (begin descriptorA completion) 1 ++ [cancel pendingCall, deleted 0])
  , scenario "cancel-after-save-no-pending" "The same with no pending call."
      [descriptor descriptorA 0 [(0x10000000, 0x40)] newName 0]
      (fullSave (begin descriptorA completion) 1 ++ [cancel 0, deleted 0])
  , scenario "cancel-idle" "Cancel with no saved path runs the pending call at once."
      [(savedPath, B.singleton 0)]
      [cancel pendingCall]
  , scenario "cancel-idle-no-pending" "Cancel with no saved path and no pending call."
      []
      [cancel 0]
  , scenario "cancel-before-start" "Cancel between 0x011d5600 and the start message: the record re-registers with worker 2, both messages dispatch."
      [descriptor descriptorA 0 threeBuffers newName 0, (savedPath, text "L:\\OLD.JPG")]
      [begin descriptorA completion, cancel pendingCall, deleted 0]
  , scenario "append-buffer" "0x011d57ac over used, size-only and free entries, then full lists."
      [ descriptor descriptorA 0 [(0x10000000, 0), (0, 0x20)] newName 0
      , descriptor descriptorB 0 [(0x10000000 + i, 1) | i <- [0 .. 14]] newName 0
      ]
      [ returning 0x011d57ac [descriptorA, 0x12000000, 0x300]
      , returning 0x011d57ac [descriptorA, 0x13000000, 0x400]
      , returning 0x011d57ac [descriptorB, 0x14000000, 0x500]
      , call 0x011d57ac [descriptorB, 0x15000000, 0x600]
      ]
  , scenario "append-buffer-all-used" "Sixteen used entries."
      [descriptor descriptorB 0 [(0x10000000 + i, 1) | i <- [0 .. 15]] newName 0]
      [returning 0x011d57ac [descriptorB, 0x14000000, 0x500]]
  , scenario "append-buffer-slot-14" "Fourteen used entries: the buffer takes entry 14 and entry 15 becomes the terminator."
      [descriptor descriptorB 0 [(0x10000000 + i, 1) | i <- [0 .. 13]] newName 0]
      [returning 0x011d57ac [descriptorB, 0x14000000, 0x500]]
  , scenario "output-modes" "0x011d5850 with modes 0 to 4."
      []
      [returning 0x011d5850 [m] | m <- [0, 1, 2, 3, 4]]
  , scenario "copy-name" "0x011d591c copies 48 name bytes out."
      [descriptor descriptorA 0 [] "L:\\DCIM\\100_FUJI\\DSCF0007.JPG" 0]
      [call 0x011d591c [nameOut, descriptorA]]
  ]
    ++ [ scenario ("set-name-" ++ label) ("0x011d5934 on " ++ show value ++ " with kind " ++ show kind ++ ".")
           [descriptor descriptorA kind [] newName 0, (textBuffer, text value)]
           [call 0x011d5934 [textBuffer, descriptorA]]
       | (label, kind, value) <-
           [ ("raf", 3, "L:\\DCIM\\100_FUJI\\DSCF0009.TMP")
           , ("jpg", 0, "L:\\X.TMP")
           , ("mpo", 1, "L:\\X.TMP")
           , ("leading-dot", 0, ".TMP")
           , ("no-dot", 0, replicate 50 'A')
           , ("dot-43", 0, replicate 43 'A' ++ ".TMP")
           , ("dot-44", 0, replicate 44 'A' ++ ".TMP")
           ]
       ]
