{-# LANGUAGE GHC2021 #-}
module DispatcherScenarios (dispatcherScenarios) where

import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.List (find)
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import Scenarios

data PipelineCall = PipelineCall Address [Word32]

dispatcherScenarios :: B.ByteString -> [Scenario]
dispatcherScenarios image = map pipelineError commands ++ terminalCases ++ successfulCases ++ [postCase]
 where
  commands = filter (\command -> any (elem command . list . snd) rows) ([0..0x31] ++ [0x36..0x6f])
  rows = takeWhile ((/=0x33) . fst) [(readWord image (0xea4870+i*8), readWord image (0xea4874+i*8)) | i <- [0..255]]
  list pointer = takeWhile (/=0x80) [readWord image (pointer+i*4) | i <- [0..255]]
  route command = case find (elem command . list . snd) rows of
    Nothing -> error ("no stock dispatcher route for command " ++ show command)
    Just (kind,pointer) -> (kind,list pointer)
  before command = previous 0x100
   where
    previous operation [] = operation
    previous operation (current:rest)
      | current == command = operation
      | otherwise = previous (current*2+1) rest
  slot :: Word32 -> Word32
  slot kind = if kind == 7 then state+0x38 else state
  initial kind operation = base ++ [word (context+4) kind,word (slot kind) operation]
  prologue = [call 0x7235d8 [] 0x33,call 0x7236b0 [0x33] 0,call 0x724638 [] 0,call 0xe4eca8 [state] 0]
  pipelineCall kind command status operation =
    let PipelineCall target arguments = evidence command
     in Call target (zip [N.R0,N.R1,N.R2] (context:arguments)) [(N.R0,status)] [word (slot kind) operation]
  pipelineError command =
    let (kind,values) = route command
        operation = command*2+1
     in Scenario ("dispatch-pipeline-"++show command++"-error") Dispatch [context]
          (initial kind (before command values)) [word (context+0x40) 1,word (slot kind) operation] Nothing
          ([call 0x724638 [] 0] ++ prologue ++ [pipelineCall kind command 1 operation,callbackCall 0xfe 1])
  finishTrace kind = case find (\i -> readWord image (0xea5678+i*16) == kind) [0..50] of
    Nothing -> error "missing operation trace row"
    Just index -> call 0x42188 [readWord image (0xea5680+index*16)] 0
  terminalCases =
    [Scenario ("dispatch-terminal-kind-"++show kind) Dispatch [context]
      (initial kind (before 0x7f values)) [word (context+0x40) 0] Nothing
      ([call 0x724638 [] 0] ++ prologue ++ [finishTrace kind,callbackCall 0xfe 0])
    | (kind,pointer) <- rows, let values = list pointer, 0x7f `elem` values]
  successfulCases =
    [Scenario ("dispatch-complete-status-"++show status++"-reentries-"++show repeats) Dispatch [context]
      (initial 10 0x100) [word state 1,word (context+0x40) 0] Nothing
      ([call 0x724638 [] 0]
        ++ concat [prologue ++ [pipelineCall 10 0 status (if iteration == repeats then 1 else 0x100)] | iteration <- [1..repeats]]
        ++ prologue ++ [finishTrace 10,callbackCall 0xfe status])
    | status <- [0,5], repeats <- [1 :: Int,2]] ++
    [Scenario ("dispatch-error-status-"++show status) Dispatch [context]
      (initial 10 0x100) [word state 1,word (context+0x40) 1] Nothing
      ([call 0x724638 [] 0] ++ prologue ++ [pipelineCall 10 0 status 1,callbackCall 0xfe status])
    | status <- [2,3,4,6,7,8,0x80000000,0xffffffff]]
  postCase = Scenario "dispatch-post-terminal" Dispatch [context]
    (initial 50 105 ++ [word 0x015b52f0 callback])
    [word context 0,word (context+0x34) 0x69,word (context+0x3c) 0,word (context+0x40) 0,word (context+0x44) 3,word (context+0x48) 2,word (context+0x4c) 0,word 0x015b4f7c 0xff,word 0x015b4f80 1]
    Nothing ([call 0x724638 [] 0] ++ prologue ++ [call 0xe45084 [] 0,call 0xeb0d04 [] 0,call callback [0] 0])
  evidence command = decode (readWord image (0xea0b58+command*4)) Nothing Nothing
  decode pc first second
    | instruction .&. 0xff000000 == 0xeb000000 =
        let displacement = (fromIntegral (instruction .&. 0x00ffffff) :: Int32) `shiftL` 8 `shiftR` 6
            target = pc + 8 + fromIntegral displacement
         in PipelineCall (Address target) (case (first,second) of (Nothing,Nothing) -> []; (Just a,Nothing) -> [a]; (Just a,Just b) -> [a,b]; _ -> error "invalid stock pipeline arguments")
    | pc >= 0xea16dc = error "pipeline arm has no call"
    | otherwise = decode (pc+4) (operand 1 first) (operand 2 second)
   where
    instruction = readWord image pc
    operand register old
      | instruction .&. 0xfffff000 == (0xe3a00000 .|. (register `shiftL` 12)) = Just (instruction .&. 0xff)
      | instruction == (0xe1a00009 .|. (register `shiftL` 12)) = Just 0
      | instruction == (0xe1a00001 .|. (register `shiftL` 12)) = first
      | otherwise = old
