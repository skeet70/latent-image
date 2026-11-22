-- | The owned playback actor's stock boundary in the X-M1 1.50 plain image: its retired
-- extents, entries, the retained services it calls and the state it writes.
module Replacement.PlaybackActor.Actor
  ( Span (..)
  , inside
  , Capture (..)
  , Entry (..)
  , Service (..)
  , dispatcher
  , machineEvents
  , machineStates
  , retired
  , entries
  , services
  , watchedWrites
  ) where

import qualified Data.Text as T
import Data.Word (Word32)

-- | A half-open guest address range.
data Span = Span Word32 Word32
  deriving (Eq, Show)

inside :: Span -> Word32 -> Bool
inside (Span s e) a = s <= a && a < e

-- | Bytes read at a site's entry and at its return, as (register holding the address, count).
data Capture = Capture {captureEntry :: [(Int, Int)], captureReturn :: [(Int, Int)]}

data Entry = Entry
  { entryAddress :: Word32
  , entryName :: String
  , entryArity :: Int
  , entryResult :: [T.Text]
  , entryCapture :: Capture
  }

data Service = Service
  { serviceAddress :: Word32
  , serviceName :: String
  , serviceArguments :: [T.Text]
  , serviceResult :: [T.Text]
  , serviceCapture :: Capture
  }

-- | The queue-4 machine dispatcher and the machine event and state halfwords it indexes by
-- machine.
dispatcher :: Word32
dispatcher = 0x011e8bf8

machineEvents, machineStates :: Span
machineEvents = Span 0x014ea4ec 0x014ea5ac
machineStates = Span 0x014ea5ac 0x014ea66c

retired :: [Span]
retired = []

entries :: [Entry]
entries = []

services :: [Service]
services = []

watchedWrites :: [Span]
watchedWrites = [machineEvents, machineStates]
