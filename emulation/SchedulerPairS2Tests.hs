module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.Map.Strict as M
import Emulation.SchedulerPairCatalog (Address (..), DispatchPoint (..))
import Emulation.SchedulerPairS2
import Replacement.RtosOwnFirst.Regions (Domain (..))

main :: IO ()
main = do
  forM_ [DomainA,DomainB] $ \domain -> do
    let (running,context,reschedule,base,_) = stateAddresses domain
        task = 0x900000
        config = 0x901000
        stack = 0x902000
        target = 0x903000
        memory = M.fromList [(Address address,value) | (address,value) <-
          [(running,task),(context,0),(reschedule,1),(task+8,config),(task+12,1),(task+0x10,0),(task+0x18,stack),(task+0x1c,target),(task+0x30,0),
           (config+4,123),(config+8,target),(config+0x20,2),(0x400c0804,0)] ++ zip [stack,stack+4..stack+32] [0..8]]
        regs = Registers (replicate 13 0 ++ [stack,99,target]) 0x93 2 0xf00000 0
        values point = Loaded (Just task) (Just 0) (Just 1) Nothing point
        event point = DispatchEvent domain point OwnedDispatch regs regs memory memory (values point) SelectedTask
        updateReg index value state = state {registers=take index (registers state) ++ [value] ++ drop (index+1) (registers state)}
        saved point bytes resume = (event point)
          { afterMemory=M.union (M.fromList [(Address (stack-bytes+offset),value) | (offset,value) <- zip [0,4..] (replicate 8 0 ++ [99 | bytes==36])])
              (M.insert (Address (task+0x18)) (stack-bytes) (M.insert (Address (task+0x1c)) (base+resume) memory))
          , afterRegisters=updateReg 13 (stack-bytes) (updateReg 15 (base+resume) regs), destination=SelectStage
          }
        restored count = foldr (uncurry updateReg) (updateReg 13 (stack+fromIntegral count*4) regs) (zip (take count ([4..11] ++ [14])) [0..])
        examples =
          [ saved Save 36 0x3668
          , (event NoSave) {destination=SelectStage}
          , event Select
          , (event Resume) {beforeRegisters=updateReg 1 task regs,afterRegisters=restored 9,destination=CallerReturn}
          , (event FirstEntry) {afterRegisters=updateReg 0 123 (updateReg 14 (base+if domain==DomainA then 0x53f0 else 0x53b0) regs) {processorState=0x53},destination=TaskEntry}
          , (event IrqReturn) {destination=ExceptionTail}
          , (event IrqResume) {afterRegisters=restored 8,destination=ExceptionTail}
          , (event IrqTail) {beforeMemory=M.insert (Address stack) 0x93 memory,afterRegisters=foldr (uncurry updateReg) (updateReg 13 (stack+32) regs) (zip [0,1,2,3,12,14,15] [1..7]),destination=CallerReturn}
          ]
    forM_ examples $ \example -> do
      let point=dispatchPoint example
      unless (either (const False) (const True) (checkDispatch example)) (fail ("positive model refused: " ++ show (domain,point,checkDispatch example)))
      unless (either (const True) (const False) (checkDispatch example {destination=NullWindow})) (fail ("wrong outcome accepted: " ++ show (domain,point)))
      let corrupt = case point of
            Save -> example {afterMemory=M.insert (Address (task+0x18)) 0 (afterMemory example)}
            NoSave -> example {afterMemory=M.insert (Address context) 1 (afterMemory example)}
            Select -> example {afterMemory=M.insert (Address running) 0 (afterMemory example)}
            Resume -> example {afterRegisters=updateReg 4 99 (afterRegisters example)}
            FirstEntry -> example {afterRegisters=(afterRegisters example) {processorState=0x13}}
            IrqReturn -> example {destination=SelectStage}
            IrqResume -> example {afterRegisters=updateReg 4 99 (afterRegisters example)}
            IrqTail -> example {afterRegisters=updateReg 13 stack (afterRegisters example)}
      unless (either (const True) (const False) (checkDispatch corrupt)) (fail ("corrupt dispatch accepted: " ++ show (domain,point)))
    let discarded = (event Save) {loaded=(values Save) {loadedDiscarded=Just 1},destination=SelectStage}
    unless (checkDispatch discarded==Right [RemoteTerminateRestarts]) (fail "discarded save did not retain restart")
    let nullEvent = (event Select)
          { loaded=(values Select) {loadedSelected=Just 0,loadedGlobalLock=Just (if domain==DomainA then 0x11 else 0x12),sourcePoint=IrqReturn}
          , afterMemory=M.insert (Address running) 0 memory
          , afterRegisters=updateReg 13 (if domain==DomainA then 0xa0000 else 0x130000) regs
          , destination=NullWindow
          }
    unless (checkDispatch nullEvent==Right [NullSelectionOwnRegisters,NullLoopReleasesGlobalLock]) (fail "null triggers not evaluated from loads")
    unless (either (const True) (const False) (checkDispatch nullEvent {afterMemory=M.insert (Address 0x400c0804) (if domain==DomainA then 0x11 else 0x12) (afterMemory nullEvent)})) (fail "leaked IRQ tag accepted")
    let clear = Address (if domain==DomainA then 0xfffb30ec else 0xfffb40ec)
    unless (checkDoorbell domain memory [(clear,0x1000000)]==Right ()) (fail "doorbell clear refused")
    unless (either (const True) (const False) (checkDoorbell domain memory [])) (fail "missing doorbell clear accepted")
  putStrLn "scheduler S2 model tests passed"
