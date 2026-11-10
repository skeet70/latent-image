{-# LANGUAGE OverloadedStrings #-}
module SettingsStorageS1Fixture
  ( s1ApplicationHits, s1ReplayFrame, s1Mutations, s1ComparisonMutations
  ) where

import Data.Aeson (Value(..), toJSON)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.PropertyPairPlan (Role, SiteLabel(..), siteLabelWire)
import Emulation.SettingsStorageMatrix (LifecycleSite(..))
import PropertyPairFixture (hex, littleWords, wireField)
import SettingsStorageFixtureWire
  ( TraceMutation, alterFirstSite, lifecycleAddress, matrixHit, matrixStack, wireMemory )

s1ReplayFrame :: B.ByteString
s1ReplayFrame = littleWords [7,0,0] <> B.pack [12..79]

s1ApplicationHits :: Role -> [Value]
s1ApplicationHits role =
  [ at S1ActionEntry [] top 0xbfb0cc [s1ReplayFrame]
  , at S1GuardCall [0x53] action 0xbfb0cc []
  , at S1GuardResult [0] action 0xc77ba0 []
  , at S1RequestCall [2,0x1d] action 0xc77ba0 []
  , at S1DispatchCall [] action 0xc77bb8 []
  , at S1DispatchResult [2] action 0xc77bbc []
  , at S1FlagCall [1] action 0xc77bbc []
  , at S1FlagReturn [1,0,0xda84,frame] action 0xc77be8 [littleWords [1]]
  , at S1ReplayCall [0,0,0xda84,frame] action 0xc77be8 []
  , at S1FrameCopyCall [slot,frame,0x50,5] replay 15 [s1ReplayFrame,B.pack [1,0]]
  , at S1FrameCopyReturn [slot+0x50,frame+0x50,0x50,0] replay 0xd34ed4 []
  , at S1ReplayReturn [slot+0x50,frame+0x50,0x1401,0x1501] action 0xc77bf0 []
  , at S1ActionReturnInstruction [2,frame+0x50,0x1401,0x1501] action 0xc77bf0 []
  , at S1ActionReturned [2,frame+0x50,0x1401,0] top 0xc77bf0 []
  , at S1DequeueEntry [frame,queue] dequeue 0xd3ab20 [B.pack [0,0,1,0]]
  , at S1DequeueCopyCall [frame,slot,0x50,5] copying 0xd3ab20 []
  , at S1DequeueCopyReturn [frame+0x50,slot+0x50,0x50,0] copying 0xd34f6c [s1ReplayFrame]
  , at S1DequeueSuccess [0] copying 0xd34fa8 []
  , at S1DequeueReturnInstruction [1] (dequeue - 12) 0xd34fa8 []
  , at S1DequeueWrapperReturn [1] dequeue 0xd34fa8 []
  , at S1DequeueReturned [1] top 0xd34fa8 []
  , at S1ApplicationActionEntry [] top 0xbfb0cc []
  , at S1ApplicationGuardCall [] action 0xbfb0cc []
  , at S1ApplicationGuardResult [1] action 0xc249a0 []
  , at S1ApplicationRouterCall [] action 0xc249b0 [s1ReplayFrame,B.pack [3,0]]
  , at S1ApplicationKeyTest [0,0,0,7] router 0xc249b4 []
  , at S1ApplicationQualifierTest [0,0,0,0] router 0xc249b4 []
  , at S1ApplicationEventStore [0,0,0,0x14676e4] router 0xc249b4 []
  , at S1ApplicationDispatch [74,0,0,0x14676e4] router 0xc249b4 [s1ReplayFrame,B.pack [3,0],B.pack [0,0]]
  , at S1ApplicationRouterReturn [0x3c] action 0xc249b4 []
  ]
  where
    top = matrixStack role
    action = top - 16
    replay = action - 48
    dequeue = top - 8
    copying = dequeue - 80
    router = action - 16
    at site arguments sp lr snapshots = foldr setRegister raw (saved site)
      where
        raw = matrixHit role (StorageSite site) (lifecycleAddress site) arguments 0 sp lr snapshots
        setRegister (index,value) = wireField ["registers",K.fromString ("r" ++ show index)] (toJSON (hex value))
        saved :: LifecycleSite -> [(Int,Word32)]
        saved current
          | current == S1FrameCopyCall = [(4,queue),(5,0x5802),(6,0),(7,3),(11,replay+36),(12,7)]
          | current == S1FrameCopyReturn = [(4,queue),(5,0x5802),(6,0),(7,3),(11,replay+36),(12,slot+0x50)]
          | current == S1DequeueCopyReturn = [(4,frame),(11,dequeue-4),(12,frame+0x50)]
          | current `elem` [S1DequeueCopyCall,S1DequeueSuccess,S1DequeueReturnInstruction] = [(4,frame),(11,dequeue-4)]
          | current `elem` [S1ApplicationKeyTest,S1ApplicationQualifierTest,S1ApplicationEventStore,S1ApplicationDispatch] = [(11,router+12)]
          | current `elem` [S1FlagCall,S1FlagReturn,S1ReplayCall,S1ReplayReturn,S1ActionReturnInstruction] = [(4,2),(11,action+12)]
          | current `elem` [S1GuardCall,S1GuardResult,S1RequestCall,S1DispatchCall,S1DispatchResult,S1ApplicationGuardCall,S1ApplicationGuardResult,S1ApplicationRouterCall,S1ApplicationRouterReturn] = [(11,action+12)]
          | otherwise = []

frame, queue, slot :: Word32
frame = 0x944580
queue = 0x949dd8
slot = queue + 0x50

s1Mutations :: [(String,TraceMutation)]
s1Mutations =
  [ ("s1-action161-guard-rejected", argument S1GuardResult 0 1)
  , ("s1-action161-status-rejected", argument S1DispatchResult 0 0)
  , ("s1-action161-return-status", argument S1ActionReturned 0 0)
  , ("s1-flag-not-set", memory S1FlagReturn [littleWords [0]])
  , ("s1-source-exit-only", takeWhile (not . isSite S1DequeueEntry))
  , ("s1-replay-copy-length", argument S1FrameCopyCall 2 79)
  , ("s1-replay-copy-source", argument S1FrameCopyCall 1 (frame + 4))
  , ("s1-replay-copy-destination", argument S1FrameCopyCall 0 (slot + 0x50))
  , ("s1-dequeue-copy-length", argument S1DequeueCopyCall 2 79)
  , ("s1-dequeue-copy-source", argument S1DequeueCopyCall 1 (slot + 0x50))
  , ("s1-dequeue-copy-destination", argument S1DequeueCopyCall 0 (frame + 4))
  , ("s1-dequeue-success-absent", removeSite S1DequeueSuccess)
  , ("s1-dequeue-return-failure", argument S1DequeueReturned 0 0)
  , ("s1-dequeue-success-order", swapSites S1DequeueSuccess S1DequeueCopyReturn)
  , ("s1-dequeue-before-replay", moveBefore S1DequeueEntry S1ReplayCall)
  , ("s1-application-guard-rejected", argument S1ApplicationGuardResult 0 0)
  , ("s1-application-key", argument S1ApplicationKeyTest 3 8)
  , ("s1-application-qualifier", argument S1ApplicationQualifierTest 3 0x80)
  , ("s1-m0-state", memory S1ApplicationRouterCall [s1ReplayFrame,B.pack [4,0]])
  , ("s1-m74-event", memory S1ApplicationDispatch [s1ReplayFrame,B.pack [3,0],B.pack [1,0]])
  , ("s1-m74-dispatch-target", argument S1ApplicationDispatch 0 75)
  , ("s1-m74-event-store-value", argument S1ApplicationEventStore 2 1)
  , ("s1-m74-event-order", swapSites S1ApplicationEventStore S1ApplicationDispatch)
  , ("s1-application-before-dequeue", moveBefore S1ApplicationActionEntry S1DequeueEntry)
  ] ++
  [ ("s1-replay-byte-" ++ show index, memory S1DequeueCopyReturn [changedFrameByte index])
  | index <- [0..79]
  ]

s1ComparisonMutations :: [(String,TraceMutation)]
s1ComparisonMutations =
  [ ("s1-m74-state", memory S1ApplicationDispatch [s1ReplayFrame,B.pack [4,0],B.pack [0,0]])
  , ("s1-replay-tail-cross-role",
      memory S1ActionEntry [changedFrameByte 79]
      . memory S1FrameCopyCall [changedFrameByte 79,B.pack [1,0]]
      . memory S1DequeueCopyReturn [changedFrameByte 79]
      . memory S1ApplicationRouterCall [changedFrameByte 79,B.pack [3,0]]
      . memory S1ApplicationDispatch [changedFrameByte 79,B.pack [3,0],B.pack [0,0]])
  , ("s1-entry-tail-cross-role", memory S1ActionEntry [changedFrameByte 79])
  ]

changedFrameByte :: Int -> B.ByteString
changedFrameByte index = B.take index s1ReplayFrame <> B.singleton (B.index s1ReplayFrame index `xor` 1) <> B.drop (index+1) s1ReplayFrame

argument :: LifecycleSite -> Int -> Word32 -> TraceMutation
argument site index value = alterFirstSite (StorageSite site)
  (wireField ["registers",K.fromString ("r" ++ show index)] (toJSON (hex value)))

memory :: LifecycleSite -> [B.ByteString] -> TraceMutation
memory site snapshots = alterFirstSite (StorageSite site) (wireMemory snapshots)

isSite :: LifecycleSite -> Value -> Bool
isSite site (Object fields) = KM.lookup "site" fields == Just (String (T.pack (siteLabelWire (StorageSite site))))
isSite _ _ = False

removeSite :: LifecycleSite -> TraceMutation
removeSite site = filter (not . isSite site)

swapSites :: LifecycleSite -> LifecycleSite -> TraceMutation
swapSites first second hits = case (filter (isSite first) hits,filter (isSite second) hits) of
  ([left],[right]) -> map (\hit -> if isSite first hit then right else if isSite second hit then left else hit) hits
  _ -> error "S1 swap mutation requires unique sites"

moveBefore :: LifecycleSite -> LifecycleSite -> TraceMutation
moveBefore moving destination hits = case filter (isSite moving) hits of
  [selected] -> insert selected (removeSite moving hits)
  _ -> error "S1 move mutation requires a unique site"
  where
    insert _ [] = error "S1 move mutation destination is absent"
    insert selected (hit:rest)
      | isSite destination hit = selected:hit:rest
      | otherwise = hit:insert selected rest
