-- | The owned task-pool and timer-arena reservation, mirroring
-- @replacement/rtos_layout/src/lib.rs@. The Rust definition emits these values in
-- @//replacement/rtos_timer/oracle:layout.json@; 'reservation-facts-tests' compares them.
module Replacement.StartupConsumers.Reservation
  ( Span (..), spanSize
  , TaskPool (..), TimerArena (..)
  , taskPool, timerArenaA, timerArenaB, timerNodeBytes, timerCapacity
  ) where

import Data.Word (Word32)

data Span = Span { spanStart :: Word32, spanEnd :: Word32 }
  deriving (Eq, Show)

spanSize :: Span -> Word32
spanSize span' = spanEnd span' - spanStart span'

-- | The task pool with the tail the timer arenas own.
data TaskPool = TaskPool
  { poolGuest :: Span
  , poolPhysical :: Span
  , poolAllocatable :: Span
  , poolAllocatablePhysical :: Span
  , poolReserved :: Span
  }
  deriving (Eq, Show)

data TimerArena = TimerArena
  { arenaGuest :: Span
  , arenaPhysical :: Span
  , arenaCapacity :: Word32
  , arenaNodeBytes :: Word32
  }
  deriving (Eq, Show)

timerNodeBytes :: Word32
timerNodeBytes = 0x08

timerCapacity :: Word32
timerCapacity = 184

taskPool :: TaskPool
taskPool = TaskPool
  { poolGuest = Span 0x40a10200 0x40a5b200
  , poolPhysical = Span 0xa10200 0xa5b200
  , poolAllocatable = Span 0x40a10200 0x40a5a200
  , poolAllocatablePhysical = Span 0xa10200 0xa5a200
  , poolReserved = Span 0x40a5a200 0x40a5b200
  }

timerArenaA, timerArenaB :: TimerArena
timerArenaA = TimerArena (Span 0x40a5a200 0x40a5aa00) (Span 0xa5a200 0xa5aa00) timerCapacity (timerCapacity * timerNodeBytes)
timerArenaB = TimerArena (Span 0x40a5aa00 0x40a5b200) (Span 0xa5aa00 0xa5b200) timerCapacity (timerCapacity * timerNodeBytes)
