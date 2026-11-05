module Emulation.SchedulerPairS2 where

import Control.Monad (unless, when)
import Data.Bits ((.&.), (.|.), complement)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.SchedulerPairCatalog (Address (..), DispatchPoint (..))
import Emulation.SchedulerPairModel (Memory)
import Replacement.RtosOwnFirst.Regions (Domain (..))

data Redesign = NullSelectionOwnRegisters | NullLoopReleasesGlobalLock | RemoteTerminateRestarts deriving (Eq,Ord,Show)
data DispatchRole = StockDispatch | OwnedDispatch deriving (Eq,Show)
data Destination = SelectStage | SelectedTask | ExceptionTail | CallerReturn | ExceptionDelivery | NullWindow | TaskEntry deriving (Eq,Show)
data Registers = Registers { registers :: [Word32], processorState :: Word32, tpidrurw :: Word32, cpacr :: Word32, fpexc :: Word32 } deriving (Eq,Show)
data Loaded = Loaded { loadedSelected :: Maybe Word32, loadedDiscarded :: Maybe Word32, loadedEnabled :: Maybe Word32, loadedGlobalLock :: Maybe Word32, sourcePoint :: DispatchPoint } deriving (Eq,Show)
data DispatchEvent = DispatchEvent { dispatchDomain :: Domain, dispatchPoint :: DispatchPoint, dispatchRole :: DispatchRole, beforeRegisters :: Registers, afterRegisters :: Registers, beforeMemory :: Memory, afterMemory :: Memory, loaded :: Loaded, destination :: Destination } deriving (Eq,Show)
data DispatchFailure = MissingRegister Int | MissingMemory Address | MissingLoaded | WrongDestination | SavedContext | RunningSelection | ThreadIdentity | FloatingPointState | TrampolineState | RestoredFrame | ContextWord | GlobalLock deriving (Eq,Show)

redesignName :: Redesign -> String
redesignName NullSelectionOwnRegisters = "null-selection-own-registers"
redesignName NullLoopReleasesGlobalLock = "null-loop-releases-leaked-global-lock"
redesignName RemoteTerminateRestarts = "remote-terminate-restarts"

require :: DispatchFailure -> Bool -> Either DispatchFailure ()
require failure condition = unless condition (Left failure)
reg :: Registers -> Int -> Either DispatchFailure Word32
reg state index = case drop index (registers state) of value:_ | length (registers state)==16 -> Right value; _ -> Left (MissingRegister index)
word :: Memory -> Word32 -> Either DispatchFailure Word32
word memory address = maybe (Left (MissingMemory (Address address))) Right (M.lookup (Address address) memory)
need :: Maybe a -> Either DispatchFailure a
need = maybe (Left MissingLoaded) Right

stateAddresses :: Domain -> (Word32,Word32,Word32,Word32,Word32)
stateAddresses DomainA = (0x40054760,0x400545a4,0x400546d8,0x40000,0x11)
stateAddresses DomainB = (0x400e4720,0x400e4564,0x400e4698,0xd0000,0x12)

checkDispatch :: DispatchEvent -> Either DispatchFailure [Redesign]
checkDispatch event = do
  let (running,context,_,base,tag) = stateAddresses (dispatchDomain event)
      before = beforeMemory event
      after = afterMemory event
      initial = beforeRegisters event
      final = afterRegisters event
      values = loaded event
      owned = dispatchRole event==OwnedDispatch
      point = dispatchPoint event
      want destination' = require WrongDestination (destination event==destination')
      changed address value = word after address >>= require SavedContext . (==value)
      preserved address = do old <- word before address; changed address old
      save task bytes resume = do
        sp <- reg initial 13
        changed (task+0x18) (sp-bytes)
        changed (task+0x1c) (base+resume)
        mapM_ (\(index,offset) -> reg initial index >>= changed (sp-bytes+offset)) (zip ([4..11] ++ [14 | bytes==36]) [0,4..])
      select = do
        task <- need (loadedSelected values)
        actual <- word after running
        require RunningSelection (actual==task)
        if task==0 then do
          want NullWindow
          when owned $ do
            reg final 13 >>= require ContextWord . (==if dispatchDomain event==DomainA then 0xa0000 else 0x130000)
            word after context >>= require ContextWord . (==0)
            require ContextWord (processorState final .&. 0x1ff==0x93 .|. (processorState initial .&. 0x40))
        else do
          want SelectedTask
          config <- word after (task+8)
          identity <- word after (config+0x20)
          require ThreadIdentity (tpidrurw final==identity)
          require FloatingPointState (fpexc final==if cpacr final .&. 0xf00000/=0 then 0 else fpexc initial)
          sp <- word after (task+0x18)
          pc <- word after (task+0x1c)
          reg final 13 >>= require SavedContext . (==sp)
          reg final 15 >>= require SavedContext . (==pc)
      restore count = do
        sp <- reg initial 13
        mapM_ (\(index,offset) -> do value <- word before (sp+offset); reg final index >>= require RestoredFrame . (==value)) (zip (take count ([4..11] ++ [14])) [0,4..])
        reg final 13 >>= require RestoredFrame . (==sp+fromIntegral count*4)
      exceptionDestination task = do
        flags <- word after (task+12)
        pending <- word after (task+0x10)
        want (if flags .&. 0x40000/=0 && pending/=0 then ExceptionDelivery else CallerReturn)
  case point of
    Save -> do
      task <- word before running
      discarded <- if owned then need (loadedDiscarded values) else pure 0
      if discarded==1 then preserved (task+0x18) >> preserved (task+0x1c) else save task 36 0x3668
      want SelectStage
    NoSave -> do
      word after context >>= require ContextWord . (==0)
      require ContextWord (processorState final .&. 0x1ff == 0x93 .|. (processorState initial .&. 0x40))
      want SelectStage
    Select -> select
    Resume -> do
      restore 9
      task <- reg initial 1
      exceptionDestination task
    FirstEntry -> do
      task <- word before running
      config <- word after (task+8)
      argument <- word after (config+4)
      entry <- word after (config+8)
      reg final 0 >>= require TrampolineState . (==argument)
      reg final 14 >>= require TrampolineState . (==base+if dispatchDomain event==DomainA then 0x53f0 else 0x53b0)
      reg final 15 >>= require TrampolineState . (==entry)
      word after (task+0x30) >>= require TrampolineState . (==0)
      require TrampolineState (processorState final .&. 0x1ff==0x53)
      want TaskEntry
    IrqReturn -> do
      task <- word before running
      enabled <- need (loadedEnabled values)
      if enabled==0 then want ExceptionTail else do
        let switching discarded = do
              if discarded==1 then preserved (task+0x18) >> preserved (task+0x1c) else save task 32 0x37a4
              want SelectStage
        if owned then do
          discarded <- need (loadedDiscarded values)
          if discarded==1 then switching discarded else do
            selected <- need (loadedSelected values)
            if selected==task then want ExceptionTail else switching discarded
        else do
          selected <- need (loadedSelected values)
          if selected==task then want ExceptionTail else need (loadedDiscarded values) >>= switching
    IrqResume -> restore 8 >> want ExceptionTail
    IrqTail -> do
      sp <- reg initial 13
      saved <- word before sp
      let status = (saved .&. complement 0x40) .|. (processorState initial .&. 0x40)
      require RestoredFrame (processorState final .&. 0x1ff==status .&. 0x1ff)
      mapM_ (\(index,offset) -> word before (sp+offset) >>= \value -> reg final index >>= require RestoredFrame . (==value)) (zip [0,1,2,3,12,14,15] [4,8..])
      reg final 13 >>= require RestoredFrame . (==sp+32)
      want CallerReturn
  when (point `elem` [Save,NoSave,Select,IrqReturn]) $
    mapM_ (\index -> reg initial index >>= \value -> reg final index >>= require RestoredFrame . (==value)) [7..11]
  when (point `elem` [Save,Resume,Select,IrqReturn,IrqResume]) $
    require RestoredFrame (processorState final .&. 0x1ff==processorState initial .&. 0x1ff)
  let selected = loadedSelected values
      discarded = loadedDiscarded values
      enabled = loadedEnabled values
      task = M.lookup (Address running) before
      nullFromIrq = selected==Just 0 && sourcePoint values==IrqReturn && maybe False (/=0) enabled
      nullUnlock = selected==Just 0 && loadedGlobalLock values==Just tag
      restart = discarded==Just 1 && (point==Save || (sourcePoint values==IrqReturn && point `elem` [IrqReturn,Select] && maybe False (/=0) enabled && selected==task))
  when (owned && nullUnlock) $ word after 0x400c0804 >>= require GlobalLock . (==0)
  pure ([NullSelectionOwnRegisters | nullFromIrq] ++ [NullLoopReleasesGlobalLock | nullUnlock] ++ [RemoteTerminateRestarts | restart])

checkDoorbell :: Domain -> Memory -> [(Address,Word32)] -> Either DispatchFailure ()
checkDoorbell domain after bank = do
  let (_,_,reschedule,_,_) = stateAddresses domain
      clear = if domain==DomainA then 0xfffb30ec else 0xfffb40ec
  word after reschedule >>= require ContextWord . (==1)
  require ContextWord (bank==[(Address clear,0x1000000)])
