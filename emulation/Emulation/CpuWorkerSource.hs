{-# LANGUAGE OverloadedStrings #-}
module Emulation.CpuWorkerSource
  ( CpuWorkerSource, bindCpuWorkerSource, cpuWorkerSourceValue, cpuWorkerArena
  ) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.GeneralHeap.Admission as Heap

newtype GuestPc = GuestPc Word32
newtype ByteCount = ByteCount Int
newtype ByteOffset = ByteOffset Int

data WorkerFunction = SubmitCall | SubmitJob | CreateWorkers | WorkerTask | NotifyWorker
  | DispatchSend | DispatchReceive | WaitCurrent | WakeTask | CurrentTask | TaskAffinity
  | EnterNestedCritical | LeaveNestedCritical deriving (Show)
data PacketField = SenderTask | RequestedCore | Callback | Payload | PayloadLength
  | CompletionCallback | PoolIndex deriving (Show)
data RuntimePremise = WorkerPoolInitialization | DispatchInterfaceMethods | CurrentTaskAndAffinity
  | OwnedNestedCriticalCarrier | OwnedHeapAllocationLifetime | ActualDeliveryAndReply deriving (Show)
data WorkerImplementation = RetainedProprietaryWorkerAndDispatch deriving (Show)
data FunctionSource = FunctionSource WorkerFunction GuestPc ByteCount SHA.Sha256
data CpuWorkerSource = CpuWorkerSource SHA.Sha256 [FunctionSource] Heap.ArenaLayout

bindCpuWorkerSource :: B.ByteString -> Either String CpuWorkerSource
bindCpuWorkerSource image4 = do
  expected <- SHA.parseSha256Hex "40d7fe9afdcd067a4441df8a079cef1fb4ee9e18a8240d9abd492a6db545dbdf"
  let actual = SHA.sha256 image4
  unless (B.length image4 == 0x282800 && actual == expected)
    (Left "CPU worker source requires the admitted full02 image4 identity")
  (_, arena) <- either (Left . show) Right (Heap.decodeGeometry
    [U.word32At image4 (0x89d4fc - 0x61c400 + index * 4) | index <- [0..13]])
  sources <- traverse source functionProfiles
  pure (CpuWorkerSource actual sources arena)
  where
    source (function, pc, size, digest) = do
      codeDigest <- SHA.parseSha256Hex digest
      pure (FunctionSource function (GuestPc pc) (ByteCount size) codeDigest)

cpuWorkerArena :: CpuWorkerSource -> Heap.ArenaLayout
cpuWorkerArena (CpuWorkerSource _ _ arena) = arena

cpuWorkerSourceValue :: CpuWorkerSource -> Value
cpuWorkerSourceValue (CpuWorkerSource imageDigest sources arena) = object
  [ "kind" .= ("full02-cpu-worker-instruction-source" :: String)
  , "image4_sha256" .= SHA.sha256Hex imageDigest
  , "image4_base" .= (0x61c400 :: Word32)
  , "base_dat_sha256" .= ("cd2f3d7d187ab72151abdb325891bac5b5a3257db8640c6b0c0e836d29b37626" :: String)
  , "instruction_source_bound" .= True
  , "implementation" .= show RetainedProprietaryWorkerAndDispatch
  , "runtime_delivery_observed" .= False
  , "installed_lifetime_admitted" .= False
  , "total" .= False
  , "functions" .= map functionValue sources
  , "owned_heap_arena" .= Heap.arenaValue arena
  , "worker_pool" .= object ["base" .= (0x4089e9f8 :: Word32), "records" .= (5 :: Int), "stride_bytes" .= (20 :: Int)]
  , "request" .= object
      [ "message_kind" .= (0x6800 :: Int), "payload_bytes" .= (28 :: Int)
      , "fields" .= map fieldValue [SenderTask, RequestedCore, Callback, Payload, PayloadLength, CompletionCallback, PoolIndex]
      , "padding_initialized" .= False
      ]
  , "owned_critical_entry_branches" .=
      [ object ["site" .= (0x7235d8 :: Word32), "target" .= (0x104f4 :: Word32)]
      , object ["site" .= (0x7236b0 :: Word32), "target" .= (0x10564 :: Word32)]
      ]
  , "unobserved_runtime_premises" .= map show
      [WorkerPoolInitialization, DispatchInterfaceMethods, CurrentTaskAndAffinity,
       OwnedNestedCriticalCarrier, OwnedHeapAllocationLifetime, ActualDeliveryAndReply]
  ]
  where
    functionValue (FunctionSource function (GuestPc pc) (ByteCount size) digest) = object
      ["function" .= show function, "entry" .= pc, "extent_bytes" .= size, "sha256" .= SHA.sha256Hex digest]
    fieldValue field = let (ByteOffset offset, ByteCount size) = fieldLayout field in object
      ["field" .= show field, "offset_bytes" .= offset, "extent_bytes" .= size]

fieldLayout :: PacketField -> (ByteOffset, ByteCount)
fieldLayout field = case field of
  SenderTask -> word 0
  RequestedCore -> word 4
  Callback -> word 8
  Payload -> word 12
  PayloadLength -> (ByteOffset 16, ByteCount 1)
  CompletionCallback -> word 20
  PoolIndex -> word 24
  where word offset = (ByteOffset offset, ByteCount 4)

functionProfiles :: [(WorkerFunction, Word32, Int, String)]
functionProfiles =
  [ (SubmitCall, 0x6b0aa8, 56, "2ccc8e42a516ee62bf811ed811ac1a08788b274b26acbf852729b7d07cd0fa91")
  , (SubmitJob, 0x6b090c, 412, "3843c41ec095991ddc13749efc76913953695c41efc1a981f4466f668e76753b")
  , (CreateWorkers, 0x6b0764, 272, "e5ee4056e2c5bc558215c0045c7700941ef021fbe9173ffebf6d7460a35c89bb")
  , (WorkerTask, 0x6c6b5c, 224, "4eb30f78a8eb069d9df67398fea0c3a53ec7287f64f33fd1ccf6aa738f21cb47")
  , (NotifyWorker, 0x6b0704, 60, "b4fa913cfcc78e477f5df4dade95233d5f617a6593dd208048b954db42cb9810")
  , (DispatchSend, 0x72503c, 472, "0c02b587b70e68d634585bde3cd9f34240350253bb3a4f22341f742ed7da8b37")
  , (DispatchReceive, 0x725318, 268, "2fbc20604b7a5221ec42d3aefa73dbd5e7546f72dc98244feb86cd878f310b8e")
  , (WaitCurrent, 0x7240c8, 124, "a86471bf26954f096deb69e691f225530ec40b6ad7792d779ec3270d92434376")
  , (WakeTask, 0x724144, 212, "25eeac45d094e3b1ac0f12a571601662bfa017baaa0dbe02f354aebcfbc017f3")
  , (CurrentTask, 0x723cb8, 48, "92681accf48089835d3c844a4fd99a7c3211ea1009cccd0f88273ce3de93a245")
  , (TaskAffinity, 0x723a14, 184, "24a319889cf448e9ccc5ff16707828629b5ca74ae3b3d56be8cce1ee1b61ddc3")
  , (EnterNestedCritical, 0x7235d8, 216, "f7a97abec82fcbc37f281bb97a30d97ac3d2bb8a6827c7a29e8a2e19f49cd8df")
  , (LeaveNestedCritical, 0x7236b0, 224, "f18a7bd034c159fe37731f9b996f4fdd07f4b5cd2f96c3879541d0bfbe09ad28")
  ]
