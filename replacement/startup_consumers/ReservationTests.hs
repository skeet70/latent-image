{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Replacement.StartupConsumers.Reservation as R
import System.Environment (getArgs)
import System.Exit (exitFailure)

data JsonSpan = JsonSpan Word32 Word32 Word32
data JsonArena = JsonArena JsonSpan JsonSpan Word32 Word32
data JsonPool = JsonPool JsonSpan JsonSpan JsonSpan JsonSpan JsonSpan
data JsonArenas = JsonArenas JsonArena JsonArena
data JsonReservation = JsonReservation JsonPool JsonArenas Word32
data JsonRoot = JsonRoot JsonReservation

instance FromJSON JsonSpan where
  parseJSON = withObject "span" $ \o -> JsonSpan <$> o .: "start" <*> o .: "end" <*> o .: "size"

instance FromJSON JsonArena where
  parseJSON = withObject "arena" $ \o -> JsonArena <$> o .: "guest" <*> o .: "physical" <*> o .: "capacity" <*> o .: "node_bytes"

instance FromJSON JsonPool where
  parseJSON = withObject "pool" $ \o -> JsonPool <$> o .: "guest" <*> o .: "physical" <*> o .: "allocatable" <*> o .: "allocatable_physical" <*> o .: "reserved"

instance FromJSON JsonArenas where
  parseJSON = withObject "arenas" $ \o -> JsonArenas <$> o .: "a" <*> o .: "b"

instance FromJSON JsonReservation where
  parseJSON = withObject "reservation" $ \o -> JsonReservation <$> o .: "task_pool" <*> o .: "timer_arenas" <*> o .: "timer_node_bytes"

instance FromJSON JsonRoot where
  parseJSON = withObject "layout" $ \o -> JsonRoot <$> o .: "reservation"

main :: IO ()
main = do
  [path] <- getArgs
  bytes <- B.readFile path
  root <- either (fail . ("layout.json: " ++)) pure (eitherDecodeStrict' bytes)
  let JsonRoot (JsonReservation (JsonPool guest physical allocatable allocatablePhysical reserved) (JsonArenas a b) nodeBytes) = root
  unless (spanOf (R.poolGuest R.taskPool) == triple guest
    && spanOf (R.poolPhysical R.taskPool) == triple physical
    && spanOf (R.poolAllocatable R.taskPool) == triple allocatable
    && spanOf (R.poolAllocatablePhysical R.taskPool) == triple allocatablePhysical
    && spanOf (R.poolReserved R.taskPool) == triple reserved
    && arenaOf (R.timerArenaA) == arenaJson a
    && arenaOf (R.timerArenaB) == arenaJson b
    && R.timerNodeBytes == nodeBytes)
    (putStrLn "host reservation differs from the Rust layout facts" >> exitFailure)
  where
    spanOf (R.Span start end) = (start, end, end - start)
    triple (JsonSpan start end size) = (start, end, size)
    arenaOf arena = (spanOf (R.arenaGuest arena), spanOf (R.arenaPhysical arena), R.arenaCapacity arena, R.arenaNodeBytes arena)
    arenaJson (JsonArena guest' physical' capacity nodeBytes) = (triple guest', triple physical', capacity, nodeBytes)
