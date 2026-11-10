{-# LANGUAGE NamedFieldPuns #-}
module Emulation.SchedulerPairTaskRecords (recordCall, recordAddresses, publicationWord) where

import Control.Monad (forM_)
import Data.Bits ((.&.), complement)
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.SchedulerPairModel hiding (priority)
import Emulation.SchedulerPairPlan (TaskRecordPoint (..))
import Replacement.RtosOwnFirst.Regions (Domain (..))

recordAddresses :: Domain -> (Word32, Word32, Word32, Word32)
recordAddresses DomainA = (0x4005beac,0x4005ff0c,0x400622d4,0x40062c50)
recordAddresses DomainB = (0x400ebe6c,0x400efecc,0x400f2294,0x400f2c10)

publicationWord :: Domain -> Arguments -> Either ModelFailure Word32
publicationWord domain (Arguments (packed:_))
  | slot>=1 && slot<=120 = Right (definitions+(slot-1)*0x28+0x24)
  where
    slot=packed .&. 0xffff
    (definitions,_,_,_)=recordAddresses domain
publicationWord _ _ = Left InvalidArguments

-- X-M1 1.50 object_setup.c:118-156 and object_configuration.c:16-51 at 55436fc85c.
recordCall :: Bool -> Domain -> TaskRecordPoint -> Arguments -> Memory -> Either ModelFailure Expected
recordCall owned domain point (Arguments (packed:argument:_)) memory = do
  let slot=packed .&. 0xffff
  if slot<1 || slot>120 then
    if owned then pure (Expected 0xffffffee memory S.empty []) else Left InvalidArguments
  else do
    (result,after,writes)<-runVM (action slot) memory
    pure (Expected result after writes [])
 where
  (definitions,tasks,ids,defaultStack)=recordAddresses domain
  action slot = do
    let definition=definitions+(slot-1)*0x28
        task=tasks+(slot-1)*0x38
        exists=definition+0x24
    published<-load exists
    case point of
      CreateTaskRecord | published/=0 -> pure 0xffffffd7
      DeleteTaskRecord | owned && argument/=1 && published==0 -> pure 0xffffffd7
      DeleteTaskRecord | argument/=1 -> store exists 0 >> pure 0
      _ -> do
        state<-byte (task+12)
        binding<-load (task+8)
        if owned && state/=0 then pure 0xffffffd7
        else if owned && binding/=definition then pure 0xffffffee
        else do
          priority<-if point==CreateTaskRecord then load (argument+12) else pure 16
          size<-if point==CreateTaskRecord then load (argument+16) else pure 4
          if owned && (priority<1 || priority>16 || size>0xfffffffc) then pure 0xffffffee
          else do
            values<-case point of
              CreateTaskRecord -> do
                attributes<-load argument
                parameter<-load (argument+4)
                entry<-load (argument+8)
                stack<-load (argument+20)
                pure [attributes,parameter,entry,priority-1,(size+3) .&. complement 3,stack,0,0,slot]
              DeleteTaskRecord -> pure [0,slot-1,0,15,4,defaultStack,0,0,slot]
            forM_ (zip [0,4..32] values) $ \(offset,value)->store (definition+offset) value
            store exists (if point==CreateTaskRecord then 1 else 0)
            store (ids+(slot-1)*4) slot
            flags<-byte (task+14)
            storeByte (task+14) (flags .&. complement 7)
            store (task+32) (if domain==DomainA then 1 else 2)
            forM_ [0x24,0x28,0x2c,0x30] $ \offset->store (task+offset) 0
            storedPriority<-load (binding+12)
            storeByte (task+13) storedPriority
            storeByte (task+12) 0
            store (task+16) 0
            pure 0
recordCall _ _ _ _ _ = Left InvalidArguments
