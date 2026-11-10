{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (object, (.=))
import Data.Bits ((.&.))
import Data.List (sort)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Emulation.NativeObservation
import Emulation.OwnedCase
import Replacement.RtosScheduler.Oracle (runCases)
import System.Directory (createDirectoryIfMissing, removeDirectoryRecursive)
import System.Environment (getArgs)
import System.FilePath ((</>))
import FujiAF.JSON (encodeCanonical)

data Service = Activate | InterruptActivate | CancelActivation | Wake | InterruptWake | CancelWake deriving (Eq, Show, Enum, Bounded)
data Row = Row Word32 Word32 Word32 Word8 Word8 Bool Bool Bool Word32 deriving Show

entry :: Service -> Word32
entry s = case s of Activate -> 0x44f2c; InterruptActivate -> 0x450f0; CancelActivation -> 0x45270; Wake -> 0x46004; InterruptWake -> 0x46200; CancelWake -> 0x46398
extent :: Service -> Int
extent s = case s of Activate -> 452; InterruptActivate -> 384; CancelActivation -> 384; Wake -> 508; InterruptWake -> 408; CancelWake -> 412
handler :: Service -> Bool
handler s = s `elem` [InterruptActivate, InterruptWake]
rows :: Service -> [Row]
rows s = [Row h c 0x53 st fl exists discarded self dispatch | h <- [0,1,2,120,0x10001,0x20001,0x10000,0x20000,0xffff,0x30001,121], (st,fl) <- [(0,0),(1,0),(1,1),(1,2),(1,7),(1,0xff),(1,0xf9),(4,0),(0x0a,0),(0x0e,2)], (exists,discarded,self,dispatch) <- [(True,False,False,0),(True,False,False,1),(True,False,False,2),(True,False,False,0x80000000),(True,True,True,1),(False,False,False,1)], let c = if handler s then 1 else 0]
  ++ [Row 0x10001 c mask 1 0 True False False 1 | c <- [0,1], mask <- [0x53,0xd3]]

expected :: Bool -> Service -> Row -> (RunnerStatus, Word32)
expected domainB s (Row h c mask st fl exists discarded self dispatch)
  | (c /= 0) /= handler s || mask .&. 0x80 /= 0 = result (-25)
  | cls > 2 || number > 120 || number >= 0x8000 || (handler s && number == 0) = result (-18)
  | not exists = result (-42)
  | s == CancelActivation = if abandoned then result (-41) else result (fromIntegral (flags .&. 1))
  | s == CancelWake = if state == 0 || abandoned then result (-41) else result (if flags .&. 2 /= 0 then 1 else 0)
  | wake && (state == 0 || (not (handler s) && abandoned)) = result (-41)
  | (not wake && state == 0) || (wake && state .&. 8 /= 0) =
      if not (handler s) && local && dispatch /= 0 && (not wake || state .&. 4 == 0)
      then (StopAddress, fromIntegral (if wake then (-19 :: Int) else -7))
      else result 0
  | flags .&. (if wake then 2 else 1) /= 0 = result (-43)
  | otherwise = result 0
 where
  result n = (Returned, fromIntegral (n :: Int))
  cls = h `div` 0x10000
  number = h .&. 0xffff
  local = cls == 0 || cls == (if domainB then 2 else 1)
  selectedSelf = self && local
  state = if number == 0 && not selectedSelf then 1 else st
  flags = if number == 0 && not selectedSelf then 0 else fl
  abandoned = selectedSelf && discarded
  wake = s `elem` [Wake,InterruptWake]

poke :: B.ByteString -> [(Word32, B.ByteString)] -> B.ByteString
poke = foldl (\bytes (offset,value) -> B.take (fromIntegral offset) bytes <> value <> B.drop (fromIntegral offset + B.length value) bytes)
w :: Word32 -> B.ByteString
w = word32le

spec :: Bool -> Service -> Row -> String -> B.ByteString -> CaseSpec
spec domainB service (Row handle context cpsr state flags exists discarded self dispatch) label code = CaseSpec
  { caseId=label, caseProvenance="X-M1 1.50 exact task_services.c 55436fc85c, S3b", caseExclusion="Stack scratch and live dispatcher frame excluded; retained helpers execute stock", caseEntry=ga (entry service + delta), caseExit=ga 0x958f00
  , caseRegions=codeRegions ++ [mkRegion (RegionId "config") (ga 0x96c000) 0x1000 ReadWrite config, mkRegion (RegionId "stack") (ga 0x959700) 0x110 ReadWrite (B.replicate 0x110 0)] ++ concatMap ram [False,True] ++ [mkRegion (RegionId name) (ga at) 4 Device (w 0) | (name,at) <- [("bank0",0xfffb30cc),("bank1",0xfffb40cc)]]
      ++ [mkRegion (RegionId "code-exit") (ga 0x958f00) 4 ReadExecute (w 0xe7f000f0)]
  , caseFunctions=[(ga at,size) | (at,size) <- functions]
  , caseInitial=[(r,if r==R0 then handle else 0xa5a50000 + fromIntegral (fromEnum r)) | r <- [R0 .. R12]] ++ [(Sp,0x959800),(Lr,0x958f00),(Cpsr,cpsr)]
  , caseExpectedStatus=fst expectation, caseExpectedRegisters=[(R0,snd expectation)], caseExpectedMemory=[], caseCp14Identity=Nothing
  , caseKernel=Just (KernelCase (InstructionLimit 4000) (Just (ga (if domainB then 0xd3640 else 0x43640))) Nothing Nothing Nothing [R1,R2,R3,R12,Lr,Pc,Cpsr] []) }
 where
  expectation = expected domainB service (Row handle context cpsr state flags exists discarded self dispatch)
  ga = GuestAddress . fromIntegral
  delta = if domainB then 0x8ffc0 else 0
  base = if domainB then 0xd0000 else 0x40000
  functions = (entry service+delta,fromIntegral (extent service)) : if domainB then [(0xd3f2c,44),(0xd3c60,124),(0xd4528,204),(0xd3d8c,72),(0xd3fd4,48),(0xd33e4,96),(0xd398c,60),(0xd39c8,40)] else [(0x43f70,44),(0x43ca4,124),(0x44568,204),(0x43dd0,72),(0x44018,44),(0x433e4,96),(0x439d8,60),(0x43a14,40)]
  codeRegions = [mkRegion (RegionId ("code-"++show at)) (ga at) size ReadExecute (B.take size (B.drop (fromIntegral (at-base)) code)) | (at,size) <- [(entry service+delta,extent service),(if domainB then 0xd33e4 else 0x433e4,if domainB then 0x1210 else 0x1250)]]
  localRam = if domainB then 0x400e0000 else 0x40050000
  table = localRam + if domainB then 0x458c else 0x45cc
  own = if domainB then 2 else 1
  targetClass = if handle `div` 0x10000 == 0 then own else handle `div` 0x10000
  targetB = targetClass == 2
  taskAt b = if b then 0x400efecc else 0x4005ff0c
  cfg = 0x96c000
  config = poke (B.replicate 0x1000 0) [(0,w 0),(0x18,w 0x900000),(0x1c,w 0x1000),(0x24,w (if exists then 1 else 0))]
  ram b =
    [mkRegion (RegionId (if b then "globals-b" else "globals-a")) (ga (rb+0x4000)) 0x1000 ReadWrite (B.take 0x1000 (B.drop 0x4000 bytes)),
     mkRegion (RegionId (if b then "tasks-b" else "tasks-a")) (ga (taskAt b)) 0x1a40 ReadWrite (B.take 0x1a40 (B.drop (fromIntegral (taskAt b-rb)) bytes))]
   where
    rb = if b then 0x400e0000 else 0x40050000
    adj = if b then 0x40 else 0
    heads = rb + 0x46dc-adj
    running = rb+0x4760-adj
    selected = rb+0x4768-adj
    number = handle .&. 0xffff
    task = taskAt b + if number >= 1 && number <= 120 then (number-1)*0x38 else 0
    selectedTask = taskAt b + 118*0x38
    selectedDefinition = [(selectedTask+8,w cfg),(selectedTask+0xc,B.singleton 1)]
    local = rb == localRam
    rel at val = (at-rb,val)
    record cls = table + (cls-1)*0x84
    recordFields cls = let other=cls==2; r=record cls; rbase=if other then 0x400e0000 else 0x40050000; a=if other then 0x40 else 0 in [(r,w (if cls==own then 0 else cls)),(r+8,w (rbase+0x45c0-a)),(r+0xc,w (if domainB then 0xd398c else 0x439d8)),(r+0x10,w (if domainB then 0xd39c8 else 0x43a14)),(r+0x20,w (rbase+0x46dc-a)),(r+0x24,w (rbase+0x475c-a)),(r+0x28,w (rbase+0x4760-a)),(r+0x2c,w (rbase+0x4768-a)),(r+0x30,w (rbase+0x4764-a)),(r+0x44,w (taskAt other)),(r+0x48,w 120),(r+0x7c,w (if other then 0xd3728 else 0x43728))]
    seed = poke (B.replicate 0x12000 0) (map (uncurry rel) ([(heads+fromIntegral p*8,w (heads+fromIntegral p*8)) | p <- [0::Int ..15]] ++ [(heads+fromIntegral p*8+4,w (heads+fromIntegral p*8)) | p <- [0::Int ..15]] ++ selectedDefinition ++ [(running,w (if self && b==targetB then task else selectedTask)),(selected,w selectedTask),(rb+0x4764-adj,w dispatch),(task+8,w cfg),(task+0xc,B.pack [state,0,flags,0]),(task+0x14,w (cfg+0x100)),(task+0x30,w (if discarded then 1 else 0)),(selectedTask+0xd,B.singleton 15)] ++ if local then [(rb+0x45a4-adj,w context),(rb+0x45c8-adj,w (record own))] ++ concatMap recordFields [1,2] else []))
    bytes = poke seed (map (uncurry rel) queueFields)
    queueFields =
      [(heads+120,w selectedTask),(heads+124,w selectedTask),
       (selectedTask,w (heads+120)),(selectedTask+4,w (heads+120)),
       (rb+0x475c-adj,w (if state .&. 1 /= 0 then 0x8001 else 0x8000)),
       (selected,w (if state .&. 1 /= 0 then task else selectedTask))]
      ++ [(at,w value) | state .&. 1 /= 0, (at,value) <- [(heads,task),(heads+4,task),(task,heads),(task+4,heads)]]

comparable :: NativeObservation -> NativeObservation -> Bool
comparable a b = nativeStatus a == nativeStatus b && nativeMemory a == nativeMemory b && all same registers && mode a == mode b && writes a == writes b
 where
  registers = [R0] ++ (if nativeStatus a==Returned then [R4 .. R11]++[Sp] else [])
  same r = M.lookup r (nativeRegisters a)==M.lookup r (nativeRegisters b)
  mode observation = (.&. 0x1ff) <$> M.lookup Cpsr (nativeRegisters observation)
  writes observation = sort [(at,size,value) | Just kernel <- [nativeKernel observation], Just order <- [observedOrder kernel], OrderedWrite (Write (GuestAddress at) (WriteBytes size)) value <- order]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [runner,loaderA,loaderB,binA,binB,work] -> do
      createDirectoryIfMissing True work
      reports <- forM [(False,loaderA,binA),(True,loaderB,binB)] $ \(b,loaderPath,binPath) -> do
        stock <- B.readFile loaderPath
        owned <- B.readFile binPath
        let delta=if b then 0x8ffc0 else 0; base=if b then 0xd0000 else 0x40000
            patch s bytes = poke bytes [(entry s+delta-base,B.take (extent s) (B.drop (fromIntegral (entry s-entry Activate)) owned))]
            candidate=foldr patch stock [minBound..maxBound]
        forM [minBound..maxBound] $ \s -> do
          let cases=rows s
              roles=[("stock",stock),("owned",candidate),("equivalent",poke candidate [(entry s+delta-base+4,w 0xe1a0b00d)]),("mutant",poke candidate [(entry s+delta-base,w 0xe3e00010<>w 0xe12fff1e)])]
              labelled=[(role++show i,spec b s row (role++show i) bytes) | (role,bytes)<-roles,(i,row)<-zip [0::Int ..] cases]
              dir=work </> (show b++show s)
          observed <- runCases runner dir labelled
          let at role i=observed M.! (role++show i)
              completed=and [nativeStatus (at "stock" i) == status && M.lookup R0 (nativeRegisters (at "stock" i)) == Just value | (i,row) <- zip [0 :: Int ..] cases, let (status,value)=expected b s row]
              matches role=length [() | i<-[0..length cases-1],comparable (at "stock" i) (at role i)]
              holds=completed && matches "owned"==length cases && matches "equivalent"==length cases && matches "mutant"<length cases
          unless holds $ putStrLn (show (b,s,completed,matches "owned",length cases,[(i,nativeStatus (at "stock" i),nativeStatus (at "owned" i)) | i<-[0..length cases-1],not (comparable (at "stock" i) (at "owned" i))]))
          if holds then removeDirectoryRecursive dir else pure ()
          let report=object ["domain_b".=b,"service".=show s,"cases".=length cases,"stock_completed".=completed,"owned_matches".=matches "owned","equivalent_matches".=matches "equivalent","mutant_matches".=matches "mutant","passed".=holds]
          B.writeFile (work </> (show b++show s++".json")) (encodeCanonical report)
          pure (holds,report)
      let flat=concat reports
      B.writeFile (work </> "summary.json") (encodeCanonical (object ["passed".=all fst flat,"entries".=map snd flat]))
      unless (all fst flat) (fail "S3b oracle failed")
    _ -> fail "runner loader-a loader-b owned-a owned-b work"
