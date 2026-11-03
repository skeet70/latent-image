module Replacement.LoaderOrchestration.Interface
  ( LoaderSide(..), CodeAddress(..), Service(..), Phase(..), BootRole(..)
  , CompletionInput(..), SetupOutcome(..), Scenario(..)
  , loaderBase, carrierStart, readyAddress, serviceAddress, serviceTable, liveRegisters
  , scenarios
  ) where

import qualified A32 as A
import qualified Data.Map.Strict as M
import Data.Word (Word32)

data LoaderSide = LoaderA | LoaderB deriving (Eq, Ord, Show)
newtype CodeAddress = CodeAddress Word32 deriving (Eq, Ord, Show)
data Service = Identity | ClearShared | PrivateInitialize | Log | Prepare1 | Prepare2 | Prepare3 | Prepare4
  | PrimaryContinue | PeerContinue | Continue1 | Continue2 | Continue3 | Continue4 | Continue5 | Continue6 | Continue7
  | PublishCallback | AutoReady | ActivateTimers | Schedule deriving (Eq, Ord, Show, Enum, Bounded)
data Phase = Setup | Root | Diagnostic | Idle0 | PeerTasks deriving (Eq, Ord, Show)
data BootRole = Primary | Peer deriving (Eq, Show)
data CompletionInput = InitiallyPublished | PublishedAfterDelay | NeverPublished deriving (Eq, Show)
data SetupOutcome = SetupReturns | SetupNonreturn deriving (Eq, Show)
data Scenario = Scenario BootRole CompletionInput SetupOutcome deriving (Eq, Show)

loaderBase :: LoaderSide -> Word32
loaderBase LoaderA = 0x40000
loaderBase LoaderB = 0xd0000

carrierStart :: LoaderSide -> CodeAddress
carrierStart side = CodeAddress (loaderBase side+0x9ac)

readyAddress :: LoaderSide -> Word32
readyAddress LoaderA = 0x4005927c
readyAddress LoaderB = 0x400e923c

serviceAddress :: LoaderSide -> Service -> CodeAddress
serviceAddress side service = CodeAddress (loaderBase side+offset)
  where
    variant a b = case side of LoaderA -> a; LoaderB -> b
    offset = case service of
      Identity -> 0x27b8; ClearShared -> 0x34f0; PrivateInitialize -> 0x430; Log -> 0x1354
      Prepare1 -> 0x3834; Prepare2 -> variant 0x3a3c 0x39f0; Prepare3 -> 0x24f8; Prepare4 -> 0xd98
      PrimaryContinue -> 0x386c; PeerContinue -> 0x38a8
      Continue1 -> variant 0x3a58 0x3a0c; Continue2 -> variant 0x4a50 0x4a10
      Continue3 -> variant 0xcea0 0xce60; Continue4 -> variant 0xce78 0xce38
      Continue5 -> 0x3934; Continue6 -> variant 0x3ac0 0x3a7c; Continue7 -> 0xb5c
      PublishCallback -> 0xc6c; AutoReady -> variant 0x3f9c 0x3f58
      ActivateTimers -> variant 0xbf3c 0xbefc; Schedule -> 0x3688

serviceTable :: LoaderSide -> M.Map CodeAddress Service
serviceTable side = M.fromList [(serviceAddress side service,service) | service <- [minBound..maxBound]]

liveRegisters :: Service -> [A.Reg]
liveRegisters service = map A.Reg $ case service of
  ClearShared -> [0,1]
  Log -> [0]
  PublishCallback -> [0]
  _ -> []

scenarios :: [Scenario]
scenarios = [Scenario Primary InitiallyPublished outcome | outcome <- [SetupReturns,SetupNonreturn]]
  ++[Scenario Peer completion SetupReturns | completion <- [InitiallyPublished,PublishedAfterDelay,NeverPublished]]
