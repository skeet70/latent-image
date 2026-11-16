{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (object, (.=))
import qualified Data.ByteString as B
import Data.Bits ((.&.), (.|.), shiftL)
import Data.Char (toLower)
import Data.Word (Word32)
import qualified Fixtures as F
import Emulation.NativeObservation
import Emulation.OwnedCase
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import qualified Replacement.PlainLoad.Admission as Plain
import Replacement.Image5Load.Admission (deriveProducerProof, refusalPc)
import System.Directory (makeAbsolute, removeDirectoryRecursive)
import System.Environment (getArgs, getEnv)

newtype BssBase = BssBase Word32
newtype FlashLength = FlashLength Word32
newtype BssLength = BssLength Word32

data FillRoute = RequestRejected | UnalignedBase | UnalignedLength deriving (Eq, Show)
data Route = Route FillRoute BssBase BssLength
data Role = Retained | Owned | Faulty F.Fault deriving Show
data Producer = Image5 | Plain deriving Show
data Scenario = CpuFill Route | MissingSegment Producer GuestAddress | PlainSkip FlashLength

label :: Show a => a -> String
label = map toLower . show

routes :: [Route]
routes = [Route RequestRejected (BssBase 0x210020) (BssLength 64),
          Route UnalignedBase (BssBase 0x210021) (BssLength 64),
          Route UnalignedLength (BssBase 0x210020) (BssLength 63)]

address :: Word32 -> GuestAddress
address = GuestAddress . fromIntegral

put :: B.ByteString -> (Int, B.ByteString) -> B.ByteString
put bytes (offset, value) = B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

wordsAt :: Int -> [(Int, Word32)] -> B.ByteString
wordsAt size = foldl put (B.replicate size 0) . map (\(offset,value) -> (offset,word32le value))

wide :: Word32 -> Word32 -> [Word32]
wide reg value = [0xe3000000 .|. (reg `shiftL` 12) .|. ((value .&. 0xf000) `shiftL` 4) .|. (value .&. 0xfff),
                 0xe3400000 .|. (reg `shiftL` 12) .|. (((value `div` 65536) .&. 0xf000) `shiftL` 4) .|. ((value `div` 65536) .&. 0xfff)]

roleLabel :: Role -> String
roleLabel (Faulty fault) = "faulty-" ++ label fault
roleLabel role = label role

fixture :: B.ByteString -> B.ByteString -> Role -> Scenario -> CaseSpec
fixture stock producer role scenario = CaseSpec
  { caseId = roleLabel role ++ "-" ++ scenarioName
  , caseProvenance = "X-M1 1.50 stock flashloader task -> retained or linked producer -> actual storage refusal/CPU fill or missing-segment halt. x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; NONMATCHING caller/storage expectations confirmed from stock instructions. Synthetic mode, segment lookup, service results, mailbox and resource sink."
  , caseExclusion = "Conditional bounded execution, not reset reachability, peripheral/DMA fidelity, physical cache coherence, resource scheduler semantics, decoder failure, read-mode failure or lower-read ownership."
  , caseEntry = address 0x73b3a0, caseExit = address 0x100000
  , caseRegions = [region "code" 0x610000 0x150000 ReadExecute code,
      region "marker" 0x42000 0x1000 ReadExecute marker,
      region "services-data" 0x810000 0x20000 ReadOnly serviceData,
      region "return" 0x100000 8 ReadExecute (assemble [0xe320f000,0xe320f000]),
      region "segment" 0x200000 0x1000 ReadOnly (wordsAt 0x1000
        [(4,0x230000),(12,base),(16,size),(20,0x2c0000),(24,flashLength),
         (0x200,0x200300),(0x204,0x230000),(0x208,0x60000),
         (0x300,44),(0x304,0x230000),(0x308,0x20000),(0x318,maxBound)]),
      region "bss" 0x210000 0x1000 ReadWrite (B.replicate 0x1000 0xa5),
      region "sink" 0x220000 0x1000 ReadWrite B.empty,
      region "destination" 0x230000 0x1000 ReadWrite (B.replicate 0x1000 0x5a),
      region "stack" 0x300000 0x10000 ReadWrite B.empty,
      region "fill-flag" 0x89f000 0x1000 ReadWrite (wordsAt 0x1000 [(0x8ac,0x77)]),
      region "task-id" 0x914000 0x1000 ReadWrite B.empty,
      region "done" 0x944000 0x1000 ReadWrite B.empty,
      region "mailbox" 0x957000 0x1000 ReadWrite B.empty]
  , caseFunctions = [(address a, fromIntegral (B.length body)) | (a,body) <- bodies] ++ [(address 0x4208c,12),(address 0x42188,12),(address 0x42634,32)]
  , caseInitial = [(R0,0x200100),(Sp,0x308000),(Lr,0x100000),(Cpsr,0x13)] ++ [(r,0) | r <- [R1 .. R12]]
  , caseExpectedStatus = expectedStatus, caseExpectedRegisters = expectedRegisters
  , caseExpectedMemory = [], caseIgnoredMemory = [], caseCp14Identity = Nothing
  , caseKernel = Just (KernelCase (InstructionLimit 20000) (Just (address 0x100004)) Nothing Nothing Nothing [] [])
  }
  where
    (base,size) = case scenario of
      CpuFill (Route _ (BssBase start) (BssLength bytes)) -> (start,bytes)
      MissingSegment _ _ -> (0x210020,64)
      PlainSkip _ -> (0x210020,64)
    flashLength = case scenario of PlainSkip (FlashLength bytes) -> bytes; _ -> 0x2e0000
    (scenarioName,missing,target,expectedStatus,expectedRegisters) = case scenario of
      CpuFill (Route name _ _) -> (label name,False,Image5,StopAddress,[(R0,0x3c),(R1,0x957604)])
      MissingSegment selected refusal -> ("missing-" ++ label selected,True,selected,InstructionBudget,[(Pc,guestWord refusal)])
      PlainSkip (FlashLength bytes) -> ("plain-skip-" ++ show bytes,False,Plain,StopAddress,[(R0,0x3c),(R1,0x957604)])
    (producerEntry,commandIndex) = case target of Image5 -> (0x6a96c0,0); Plain -> (0x6a9844,3)
    region name' a n = mkRegion (RegionId name') (address a) n
    stockBody a end = (a,B.take (fromIntegral (end-a)) (B.drop (fromIntegral (a-0x61c400)) stock))
    ret value = assemble (wide 0 value ++ [0xe12fff1e])
    countAt offset result = assemble (wide 12 0x220000 ++ [0xe59c3000 + offset,0xe2833001,0xe58c3000 + offset] ++
      (if result <= 255 then [0xe3a00000 + result] else wide 0 result) ++ [0xe12fff1e])
    resourceEvent release = wide 12 0x220000 ++ [0xe59c3080,0xe28c2c01] ++
      (if release then [0xe3800102] else []) ++ [0xe7820103,0xe2833001,0xe58c3080]
    bodies = [stockBody 0x73b3a0 0x73b728,stockBody 0x6a8f1c 0x6a8ff0,stockBody 0x7221a0 0x722218,
      stockBody 0x73b924 0x73b9ec,stockBody 0x73b760 0x73b7a0,
      (producerEntry,producer),(0x657114,assemble (wide 12 0x220000 ++ [0xe58c0024] ++ wide 0 (if missing then 0 else 0x200000) ++ [0xe12fff1e])),
      (0x657184,countAt 44 1),(0x6571a0,countAt 40 0x200200),(0x66609c,countAt 4 1),(0x723cb8,ret 60),(0x723a14,ret 0),
      (0x724b68,assemble (resourceEvent False) <> countAt 8 0),
      (0x724c00,assemble (resourceEvent True ++ [0xe3c00102] ++ wide 12 0x220000 ++ [0xe58c000c] ++ wide 3 (0x944440+commandIndex*4) ++ [0xe5933000,0xe58c3010,0xe3a00000,0xe12fff1e])),
      (0x69b9c4,assemble (wide 12 0x220000 ++ [0xe58c0034]) <> countAt 48 0),
      (0x72503c,assemble (wide 12 0x220000 ++ [0xe59c3040,0xe28c2b01,0xe0822203,0xe5820000,
        0xe5910000,0xe5820004,0xe5910004,0xe5820008,0xe5910008,0xe5900000,0xe582000c,
        0xe2833001,0xe58c3040,0xe3a00000,0xe12fff1e])),
      (0x72872c,ret 0),
      (0x725318,assemble (wide 12 0x220000 ++ wide 2 0x100004 ++ [0xe59c3000,0xe3530000,0x112fff12,0xe3a03001,0xe58c3000] ++ wide 3 (0x4c00+commandIndex) ++ [0xe5813000] ++ wide 3 0x740000 ++ [0xe581300c] ++ wide 3 0x1234 ++ [0xe5813010,0xe3a00000,0xe12fff1e])),
      (0x740000,assemble (wide 12 0x220000 ++ [0xe58c0014,0xe58c1018,0xe59c3038,0xe2833001,0xe58c3038] ++ wide 3 0x957600 ++ [0xe5933000,0xe58c301c,0xe12fff1e]))]
    code = foldl put (B.replicate 0x150000 0) [(fromIntegral (a-0x610000),body) | (a,body) <- bodies]
    marker = foldl put (B.replicate 0x1000 0) [(0x8c,ret 0),(0x188,ret 0),(0x634,countAt 32 0)]
    serviceData = wordsAt 0x20000 [(0x7700+fromIntegral commandIndex*4,0x4c+commandIndex)]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner,stockPath,ownedPath,plainPath,output,"--missing-segments","--native-jobs","1"] ->
      runMissing runner stockPath ownedPath plainPath output
    [runner,stockPath,plainPath,output,"--plain-skip","--native-jobs","1"] ->
      runPlainSkip runner stockPath plainPath output
    [runner,stockPath,ownedPath,output,"--native-jobs","1"] -> do
      stock <- B.readFile stockPath
      owned <- B.readFile ownedPath
      runnerBytes <- B.readFile runner
      unless (sha256Hex (sha256 stock) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "stock image4 identity differs")
      _ <- either (fail . show) pure (deriveProducerProof owned)
      root <- makeAbsolute output
      work <- getEnv "XM1_RUN_WORK"
      let retained = B.take 388 (B.drop (0x6a96c0-0x61c400) stock)
          producers = [(Retained,retained,True),(Owned,owned,True)] ++
            [(Faulty fault,F.fixture fault,False) | fault <- [F.MissingCpuFill,F.WrongErrorStatus,F.MissingCache]]
          specs = [(role,route,shouldPass,fixture stock bytes role (CpuFill route)) | (role,bytes,shouldPass) <- producers, route <- routes]
      batch <- writeBatch (work ++ "/routes.cases") [spec | (_,_,_,spec) <- specs]
      observed <- loadObservedBatch runner batch
      results <- forM (zip specs observed) $ \((role,Route name (BssBase base) (BssLength size),shouldPass,spec),(_,obs)) -> do
        let memory regionName = regionBytesOf obs (RegionId regionName)
            sink = memory "sink"
            values = [readWordAt sink n | n <- [0,4..32]]
            expected = [1,if name == RequestRejected then 1 else 0,1,0x4c,1,0x1234,1,0,1]
            expectedBss = B.replicate (fromIntegral (base-0x210000)) 0xa5 <> B.replicate (fromIntegral size) 0 <> B.replicate (4096-fromIntegral (base-0x210000+size)) 0xa5
            holds = nativeStatus obs == StopAddress && values == expected && memory "bss" == expectedBss
              && memory "destination" == B.replicate 4096 0x5a && readWordAt (memory "done") 0x440 == 1
              && readWordAt (memory "mailbox") 0x600 == 0
              && readWordAt (memory "fill-flag") 0x8ac == (if name == RequestRejected then 0 else 0x77)
        pure (nativeStatus obs == StopAddress && holds == shouldPass,object ["case" .= caseId spec,"role" .= roleLabel role,"route" .= label name,"passed" .= (nativeStatus obs == StopAddress && holds == shouldPass),"oracle_accepts" .= holds,
          "status" .= runnerStatusLabel (nativeStatus obs),"pc" .= registerValue obs Pc,"sink_words" .= values,
          "bss_sha256" .= sha256Hex (sha256 (memory "bss")),"destination_sha256" .= sha256Hex (sha256 (memory "destination"))])
      B.writeFile root (encodeCanonical (object ["schema" .= (1 :: Int),"image4_sha256" .= sha256Hex (sha256 stock),
        "owned_sha256" .= sha256Hex (sha256 owned),"runner_sha256" .= sha256Hex (sha256 runnerBytes),
        "passed" .= all fst results,"results" .= map snd results]))
      unless (all fst results) (fail ("native producer route differs: " ++ root))
      removeDirectoryRecursive (work ++ "/routes.cases")
      putStrLn "six actual flashloader/image5 skip-refusal/CPU-fill routes and nine executed controls pass"
    _ -> fail "usage: image5-native-routes RUNNER IMAGE4 IMAGE5_BIN REPORT --native-jobs 1 | RUNNER IMAGE4 IMAGE5_BIN PLAIN_BIN REPORT --missing-segments --native-jobs 1 | RUNNER IMAGE4 PLAIN_BIN REPORT --plain-skip --native-jobs 1"

runMissing :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runMissing runner stockPath ownedPath plainPath output = do
  stock <- B.readFile stockPath
  owned <- B.readFile ownedPath
  plain <- B.readFile plainPath
  runnerBytes <- B.readFile runner
  unless (sha256Hex (sha256 stock) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "stock image4 identity differs")
  image5Proof <- either (fail . show) pure (deriveProducerProof owned)
  plainProof <- either (fail . show) pure (Plain.deriveProducerProof plain)
  root <- makeAbsolute output
  work <- getEnv "XM1_RUN_WORK"
  let retained start bytes = B.take bytes (B.drop (start-0x61c400) stock)
      producers =
        [(Image5,Retained,retained 0x6a96c0 388,0x6a96dc,True),
         (Image5,Owned,owned,refusalPc image5Proof,True),
         (Plain,Retained,retained 0x6a9844 244,0x6a986c,True),
         (Plain,Owned,plain,Plain.refusalPc plainProof,True),
         (Image5,Faulty F.NullReturns,F.fixture F.NullReturns,0,False)]
      specs = [(target,role,shouldRefuse,halt,spec) |
        (target,role,bytes,halt,shouldRefuse) <- producers,
        let spec = fixture stock bytes role (MissingSegment target (address halt))]
  batch <- writeBatch (work ++ "/missing.cases") [spec | (_,_,_,_,spec) <- specs]
  observed <- loadObservedBatch runner batch
  results <- forM (zip specs observed) $ \((target,role,shouldRefuse,halt,spec),(_,obs)) -> do
    let memory name = regionBytesOf obs (RegionId name)
        sink = memory "sink"
        sinkWords = [readWordAt sink n | n <- [0,4..44]]
        (segmentId,extents,index) = case target of Image5 -> (5,0,0); Plain -> (6,1,3)
        busy = readWordAt (memory "mailbox") 0x600
        done = readWordAt (memory "done") (0x440+index*4)
        flag = readWordAt (memory "fill-flag") 0x8ac
        holds = nativeStatus obs == InstructionBudget && registerValue obs Pc == halt
          && sinkWords == [1,0,1,0,0,0,0,0,0,segmentId,extents,0]
          && busy == 1 && done == 0 && flag == 0x77
          && readWordAt sink 0x38 == 0 && readWordAt sink 0x80 == 1
          && readWordAt sink 0x100 == (if index == 0 then 0x4c else 0x4f)
          && memory "bss" == B.replicate 4096 0xa5
          && memory "destination" == B.replicate 4096 0x5a
        acceptedControl = nativeStatus obs == StopAddress && not holds && done == 1 && busy == 0
        passed = if shouldRefuse then holds else acceptedControl
        InstructionCount instructions = nativeInstructions obs
    pure (passed,object ["case" .= caseId spec,"target" .= label target,"role" .= roleLabel role,
      "passed" .= passed,"oracle_accepts" .= holds,"status" .= runnerStatusLabel (nativeStatus obs),
      "pc" .= registerValue obs Pc,"instructions" .= instructions,"sink_words" .= sinkWords,
      "command_done" .= done,"command_busy" .= busy,"fill_flag" .= flag,
      "callback_calls" .= readWordAt sink 0x38,"resource_count" .= readWordAt sink 0x80,
      "bss_sha256" .= sha256Hex (sha256 (memory "bss")),"destination_sha256" .= sha256Hex (sha256 (memory "destination"))])
  B.writeFile root (encodeCanonical (object ["schema" .= (1 :: Int),"image4_sha256" .= sha256Hex (sha256 stock),
    "owned_image5_sha256" .= sha256Hex (sha256 owned),"owned_plain_sha256" .= sha256Hex (sha256 plain),
    "runner_sha256" .= sha256Hex (sha256 runnerBytes),"passed" .= all fst results,"results" .= map snd results]))
  unless (all fst results) (fail ("missing-segment caller oracle differs: " ++ root))
  removeDirectoryRecursive (work ++ "/missing.cases")
  putStrLn "four actual missing-segment caller refusals and one executed premature-return control pass"

runPlainSkip :: FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runPlainSkip runner stockPath plainPath output = do
  stock <- B.readFile stockPath
  plain <- B.readFile plainPath
  runnerBytes <- B.readFile runner
  unless (sha256Hex (sha256 stock) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "stock image4 identity differs")
  _ <- either (fail . show) pure (Plain.deriveProducerProof plain)
  root <- makeAbsolute output
  work <- getEnv "XM1_RUN_WORK"
  let retained = B.take 244 (B.drop (0x6a9844-0x61c400) stock)
      specs = [(bytes,spec) | (role,producer) <- [(Retained,retained),(Owned,plain)],
        bytes <- [0,0x40001], let spec = fixture stock producer role (PlainSkip (FlashLength bytes))]
  batch <- writeBatch (work ++ "/plain-skip.cases") (map snd specs)
  observed <- loadObservedBatch runner batch
  results <- forM (zip specs observed) $ \((bytes,spec),(_,obs)) -> do
    let memory name = regionBytesOf obs (RegionId name)
        sink = memory "sink"
        word = readWordAt sink
        progress = if bytes == 0 then [] else [0x20000,0x40000,0x60000]
        count = length progress
        notifications = [[word (0x400+index*16+offset) | offset <- [0,4,8,12]] | index <- [0..count-1]]
        expectedNotifications = [[95,0x5f3c6f00,4,position] | position <- progress]
        resourceSequence = [word offset | offset <- [0x100,0x104..0x114]]
        busy = readWordAt (memory "mailbox") 0x600
        done = readWordAt (memory "done") 0x44c
        highest = readWordAt (memory "task-id") 0x834
        holds = nativeStatus obs == StopAddress
          && resourceSequence == [0x4f,0x24,0x34,0x80000034,0x80000024,0x8000004f]
          && word 0x80 == 6 && word 8 == 3 && word 12 == 0x4f && word 16 == 1
          && word 20 == 0x1234 && word 24 == 0 && word 28 == 0 && word 0x38 == 1
          && word 36 == 6 && word 40 == fromIntegral (1+count) && word 44 == 1
          && word 48 == fromIntegral count && word 52 == (if count == 0 then 0 else 14)
          && word 64 == fromIntegral count && notifications == expectedNotifications
          && highest == (if count == 0 then 0 else 0x60000) && busy == 0 && done == 1
          && memory "bss" == B.replicate 4096 0xa5 && memory "destination" == B.replicate 4096 0x5a
          && readWordAt (memory "fill-flag") 0x8ac == 0x77
        InstructionCount instructions = nativeInstructions obs
    pure (holds,object ["case" .= caseId spec,"flash_length" .= bytes,"passed" .= holds,
      "status" .= runnerStatusLabel (nativeStatus obs),"pc" .= registerValue obs Pc,"instructions" .= instructions,
      "resource_sequence" .= resourceSequence,"resource_count" .= word 128,"delay_calls" .= word 48,"delay_ticks" .= word 52,
      "notification_count" .= word 64,"notifications" .= notifications,"highest_progress" .= highest,
      "command_done" .= done,"command_busy" .= busy,"callback_argument" .= word 20,"callback_failed" .= word 24,
      "callback_calls" .= word 0x38,
      "bss_sha256" .= sha256Hex (sha256 (memory "bss")),"destination_sha256" .= sha256Hex (sha256 (memory "destination"))])
  B.writeFile root (encodeCanonical (object ["schema" .= (1 :: Int),"image4_sha256" .= sha256Hex (sha256 stock),
    "owned_plain_sha256" .= sha256Hex (sha256 plain),"runner_sha256" .= sha256Hex (sha256 runnerBytes),
    "passed" .= all fst results,"results" .= map snd results]))
  unless (all fst results) (fail ("plain skip caller/progress oracle differs: " ++ root))
  removeDirectoryRecursive (work ++ "/plain-skip.cases")
  putStrLn "four actual plain skip caller/progress/notification routes pass without fresh executable bytes"
