{-# LANGUAGE GeneralizedNewtypeDeriving #-}
module Emulation.SchedulerPairModel
  ( Memory, Arguments (..), Expected (..), ModelFailure (..), stockCall ) where

import Control.Monad (ap, when)
import Data.Bits ((.&.), (.|.), complement, shiftL, shiftR, countTrailingZeros)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.SchedulerPairPlan (Address (..))
import Replacement.RtosOwnFirst.Regions (Domain (..))
import Replacement.RtosScheduler.Regions (Helper (..))

type Memory = M.Map Address Word32
newtype Arguments = Arguments [Word32] deriving (Eq, Show)
data Expected = Expected { expectedR0 :: Word32, expectedMemory :: Memory, expectedWrites :: S.Set Address, expectedBank :: [(Address,Word32)] } deriving (Eq,Show)
data ModelFailure = MissingWord Address | InvalidArguments | InvalidPriority Word32 | EmptyBitmap deriving (Eq,Show)
newtype VM a = VM { runVM :: Memory -> Either ModelFailure (a,Memory,S.Set Address) }
instance Functor VM where fmap f action = action >>= pure . f
instance Applicative VM where pure a = VM (\m -> Right (a,m,S.empty)); (<*>) = ap
instance Monad VM where
  action >>= next = VM $ \m -> do
    (a,m1,w1) <- runVM action m
    (b,m2,w2) <- runVM (next a) m1
    pure (b,m2,S.union w1 w2)
load :: Word32 -> VM Word32
load address = VM $ \m -> maybe (Left (MissingWord (Address address))) (\value -> Right (value,m,S.empty)) (M.lookup (Address address) m)
store :: Word32 -> Word32 -> VM ()
store address value = VM $ \m -> if M.member (Address address) m then Right ((),M.insert (Address address) value m,S.singleton (Address address)) else Left (MissingWord (Address address))
byte :: Word32 -> VM Word32
byte address = (.&. 255) . (`shiftR` (fromIntegral (address .&. 3)*8)) <$> load (address .&. complement 3)
storeByte :: Word32 -> Word32 -> VM ()
storeByte address value = do
  let aligned = address .&. complement 3; shift = fromIntegral (address .&. 3)*8
  old <- load aligned
  store aligned ((old .&. complement (255 `shiftL` shift)) .|. ((value .&. 255) `shiftL` shift))
priority :: Word32 -> VM Word32
priority task = do
  p <- byte (task+13)
  if p < 16 then pure p else VM (const (Left (InvalidPriority p)))
field :: Word32 -> Word32 -> VM Word32
field record offset = load (record+offset) >>= load
setField :: Word32 -> Word32 -> Word32 -> VM ()
setField record offset value = load (record+offset) >>= (`store` value)
modifyField :: Word32 -> Word32 -> (Word32 -> Word32) -> VM ()
modifyField record offset f = field record offset >>= setField record offset . f
findReady :: Word32 -> VM Word32
findReady record = do
  bitmap <- field record 0x24
  when (bitmap == 0) (VM (const (Left EmptyBitmap)))
  heads <- load (record+0x20)
  load (heads + fromIntegral (countTrailingZeros bitmap)*8)
unlink :: Word32 -> VM ()
unlink task = do
  prev <- load (task+4)
  next <- load task
  store prev next
  store (next+4) prev
append :: Word32 -> Word32 -> VM ()
append head' task = do
  tail' <- load (head'+4)
  store (task+4) tail'
  store task head'
  store tail' task
  store (head'+4) task
insertReady :: Word32 -> Word32 -> VM Word32
insertReady record task = do
  p <- priority task
  storeByte (task+12) 1
  heads <- load (record+0x20)
  append (heads+p*8) task
  modifyField record 0x24 (.|. (1 `shiftL` fromIntegral p))
  selected <- field record 0x2c
  replace <- if selected == 0 then pure True else (p <) <$> priority selected
  if replace then setField record 0x2c task >> field record 0x30 else pure 0
removeReady :: Word32 -> Word32 -> VM Word32
removeReady record task = do
  p <- priority task
  heads <- load (record+0x20)
  unlink task
  first <- load (heads+p*8)
  when (first == heads+p*8) (modifyField record 0x24 (.&. complement (1 `shiftL` fromIntegral p)))
  selected <- field record 0x2c
  if selected /= task then pure 0 else do
    next <- if first /= heads+p*8 then pure first else do
      bitmap <- field record 0x24
      if bitmap == 0 then pure 0 else findReady record
    setField record 0x2c next
    field record 0x30
requeueReady :: Word32 -> Word32 -> Word32 -> VM Word32
requeueReady record task new = do
  when (new >= 16) (VM (const (Left (InvalidPriority new))))
  old <- priority task
  storeByte (task+13) new
  unlink task
  heads <- load (record+0x20)
  first <- load (heads+old*8)
  when (first == heads+old*8) (modifyField record 0x24 (.&. complement (1 `shiftL` fromIntegral old)))
  append (heads+new*8) task
  modifyField record 0x24 (.|. (1 `shiftL` fromIntegral new))
  selected <- field record 0x2c
  if selected == task then
    if old > new then pure 0 else do
      next <- findReady record
      setField record 0x2c next
      if next == task then pure 0 else do
        enabled <- field record 0x30
        pure (if enabled == 0 then 0 else 1)
  else do
    p <- priority selected
    if new < p then setField record 0x2c task >> field record 0x30 else pure 0
rotateReady :: Word32 -> Word32 -> VM Word32
rotateReady record p = do
  when (p >= 16) (VM (const (Left (InvalidPriority p))))
  heads <- load (record+0x20)
  let head' = heads+p*8
  first <- load head'
  if first == head' then pure 0 else do
    next <- load first
    if next == head' then pure 0 else do
      store head' next
      store (next+4) head'
      append head' first
      selected <- field record 0x2c
      if selected /= first then pure 0 else setField record 0x2c next >> field record 0x30

-- X-M1 1.50 loader listings: A 0x439d8..0x441b4, B 0xd398c..0xd4174.
stockCall :: Domain -> Helper -> Arguments -> Memory -> Either ModelFailure Expected
stockCall domain helper (Arguments [r0,r1,r2,_]) memory = do
  let local = case domain of DomainA -> r0 <= 1; DomainB -> r0 == 0 || r0 == 2
      bank = if helper /= Notify || local then [] else [(Address (if (r0 `shiftR` 1) == 0 then 0xfffb30cc else 0xfffb40cc),0x1000000)]
      action = case helper of
        Acquire -> load r0 >>= (\lock -> store lock 1) >> pure 0
        Release -> load r0 >>= (\lock -> store lock 0) >> pure r0
        Find -> findReady r0
        Insert -> insertReady r0 r1
        Remove -> removeReady r0 r1
        Reset -> do
          definition <- load (r0+8)
          p <- load (definition+12)
          storeByte (r0+13) p
          storeByte (r0+12) 0
          flags <- byte (r0+14)
          storeByte (r0+14) (flags .&. 0xf9)
          mapM_ (\offset -> store (r0+offset) 0) [0x10,0x28,0x24,0x2c]
          pure r0
        Prepare -> do
          definition <- load (r1+8)
          size <- load (definition+0x10)
          start <- load (definition+0x14)
          store (r1+0x18) (start+size)
          trampoline <- load (r0+0x7c)
          store (r1+0x1c) trampoline
          insertReady r0 r1
        Notify -> pure (if local then 1 else 0)
        Requeue -> requeueReady r0 r1 r2
        Rotate -> rotateReady r0 r1
  (value,after,writes) <- runVM action memory
  pure (Expected value after writes bank)
stockCall _ _ _ _ = Left InvalidArguments
