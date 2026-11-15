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
import Replacement.Image5Load.Admission (deriveProducerProof)
import System.Directory (makeAbsolute, removeDirectoryRecursive)
import System.Environment (getArgs, getEnv)

newtype BssBase = BssBase Word32
newtype BssLength = BssLength Word32

data FillRoute = RequestRejected | UnalignedBase | UnalignedLength deriving (Eq, Show)
data Route = Route FillRoute BssBase BssLength
data Role = Retained | Owned | Faulty F.Fault deriving Show

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

fixture :: B.ByteString -> B.ByteString -> Role -> Route -> CaseSpec
fixture stock producer role (Route name (BssBase base) (BssLength size)) = CaseSpec
  { caseId = roleLabel role ++ "-" ++ label name
  , caseProvenance = "X-M1 1.50 stock flashloader task -> linked image5 producer -> actual storage_read_indexed unconfigured refusal -> actual fill_bytes/fill_u32 -> flashloader done/release/callback. x70view 55436fc85c0560dd252b0410af6f2427fe198cd3; NONMATCHING caller/storage expectations confirmed from stock instructions. Synthetic mode, segment, service results, mailbox and resource sink."
  , caseExclusion = "Conditional bounded execution, not reset reachability, peripheral/DMA fidelity, physical cache coherence, resource scheduler semantics, decoder failure, read-mode failure or lower-read ownership."
  , caseEntry = address 0x73b3a0, caseExit = address 0x100000
  , caseRegions = [region "code" 0x610000 0x150000 ReadExecute code,
      region "marker" 0x42000 0x1000 ReadExecute marker,
      region "services-data" 0x810000 0x20000 ReadOnly serviceData,
      region "return" 0x100000 8 ReadExecute (assemble [0xe320f000,0xe320f000]),
      region "segment" 0x200000 0x1000 ReadOnly (wordsAt 0x1000 [(4,0x230000),(12,base),(16,size),(20,0x2c0000),(24,0x2e0000)]),
      region "bss" 0x210000 0x1000 ReadWrite (B.replicate 0x1000 0xa5),
      region "sink" 0x220000 0x1000 ReadWrite B.empty,
      region "destination" 0x230000 0x1000 ReadWrite (B.replicate 0x1000 0x5a),
      region "stack" 0x300000 0x10000 ReadWrite B.empty,
      region "fill-flag" 0x89f000 0x1000 ReadWrite (wordsAt 0x1000 [(0x8ac,0x77)]),
      region "task-id" 0x914000 0x1000 ReadWrite B.empty,
      region "done" 0x944000 0x1000 ReadWrite B.empty,
      region "mailbox" 0x957000 0x1000 ReadWrite B.empty]
  , caseFunctions = [(address a, fromIntegral (B.length body)) | (a,body) <- bodies] ++ [(address 0x42188,12),(address 0x42634,32)]
  , caseInitial = [(R0,0x200100),(Sp,0x308000),(Lr,0x100000),(Cpsr,0x13)] ++ [(r,0) | r <- [R1 .. R12]]
  , caseExpectedStatus = StopAddress, caseExpectedRegisters = [(R0,0x3c),(R1,0x957604)]
  , caseExpectedMemory = [], caseIgnoredMemory = [], caseCp14Identity = Nothing
  , caseKernel = Just (KernelCase (InstructionLimit 20000) (Just (address 0x100004)) Nothing Nothing Nothing [] [])
  }
  where
    region name' a n = mkRegion (RegionId name') (address a) n
    stockBody a end = (a,B.take (fromIntegral (end-a)) (B.drop (fromIntegral (a-0x61c400)) stock))
    ret value = assemble (wide 0 value ++ [0xe12fff1e])
    countAt offset result = assemble (wide 12 0x220000 ++ [0xe59c3000 + offset,0xe2833001,0xe58c3000 + offset] ++ wide 0 result ++ [0xe12fff1e])
    bodies = [stockBody 0x73b3a0 0x73b728,stockBody 0x6a8f1c 0x6a8ff0,stockBody 0x7221a0 0x722218,
      (0x6a96c0,producer),(0x657114,assemble (wide 0 0x200000 ++ [0xe12fff1e])),
      (0x657184,ret 1),(0x66609c,countAt 4 1),(0x723cb8,ret 60),
      (0x724b68,countAt 8 0),
      (0x724c00,assemble (wide 12 0x220000 ++ [0xe58c000c] ++ wide 3 0x944440 ++ [0xe5933000,0xe58c3010,0xe3a00000,0xe12fff1e])),
      (0x72872c,ret 0),
      (0x725318,assemble (wide 12 0x220000 ++ wide 2 0x100004 ++ [0xe59c3000,0xe3530000,0x112fff12,0xe3a03001,0xe58c3000] ++ wide 3 0x4c00 ++ [0xe5813000] ++ wide 3 0x740000 ++ [0xe581300c] ++ wide 3 0x1234 ++ [0xe5813010,0xe3a00000,0xe12fff1e])),
      (0x740000,assemble (wide 12 0x220000 ++ [0xe58c0014,0xe58c1018] ++ wide 3 0x957600 ++ [0xe5933000,0xe58c301c,0xe12fff1e]))]
    code = foldl put (B.replicate 0x150000 0) [(fromIntegral (a-0x610000),body) | (a,body) <- bodies]
    marker = foldl put (B.replicate 0x1000 0) [(0x188,ret 0),(0x634,countAt 32 0)]
    serviceData = wordsAt 0x20000 [(0x7700,0x4c)]

main :: IO ()
main = do
  args <- getArgs
  case args of
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
          specs = [(role,route,shouldPass,fixture stock bytes role route) | (role,bytes,shouldPass) <- producers, route <- routes]
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
    _ -> fail "usage: image5-native-routes RUNNER IMAGE4 OWNED_BIN REPORT --native-jobs 1"
