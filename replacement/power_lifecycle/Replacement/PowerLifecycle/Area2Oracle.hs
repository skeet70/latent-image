{-# LANGUAGE GHC2021 #-}
module Replacement.PowerLifecycle.Area2Oracle where

import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word
import Replacement.RecordStore.Hardware (hardware)
import qualified Replacement.RecordStore.Model as R

newtype Record = Record B.ByteString deriving (Eq, Show)
newtype Backing = Backing B.ByteString deriving (Eq, Show)
data Selector = Snapshot | Close deriving (Eq, Show)
data Version = Version Word8 Word8 deriving (Eq, Show)
data Update = LiveByte Int Word8 | Sequence Word16 | Save Selector deriving (Eq, Show)
data Input = Input { working :: Record, snapshot :: Record, backing :: Backing, version :: Version, updates :: [Update], initialMedium :: R.Medium, fault :: R.Fault, backupFails :: Bool } deriving (Eq, Show)
data Observation = Observation { observedWorking :: Record, observedSnapshot :: Record, observedBacking :: Backing, observedMedium :: R.Medium, observedArea2Open :: Bool, observedArea3Open :: Bool, returnedStatuses :: [Word32], observedBackup :: B.ByteString } deriving (Eq, Show)

record :: B.ByteString -> Either String Record
record bytes | B.length bytes == 512 = Right (Record bytes)
             | otherwise = Left "area2 record extent"
backingBytes :: B.ByteString -> Either String Backing
backingBytes bytes | B.length bytes == 0x19a = Right (Backing bytes)
                   | otherwise = Left "area2 backing extent"

prepare :: Version -> Backing -> Record -> (Backing, Record)
prepare (Version major minor) (Backing original) (Record old) = (Backing live, Record result)
  where
    count = getHalf original 0
    limit = getHalf original 2
    normalizedLimit = if limit >= 100 && limit < 1000 then limit else 100
    normalizedCount = if count <= 9999 then count else 0
    live | normalizedLimit == 999 && normalizedCount == 9999 = checksum (R.replace 0 (R.half 0 <> R.half 100) original)
         | otherwise = original
    fields = [(0x24, B.take 2 (B.drop 2 live)), (0x26, B.take 2 live), (0x28, B.take 0x118 (B.drop 0x10 live)), (0x1e8, B.pack [major,minor])]
      ++ [(4 + index * 8, B.take 6 (B.drop (0x128 + index * 6) live)) | index <- [0..3]]
    result = foldl (\bytes (offset,value) -> R.replace offset value bytes) old fields
    checksum bytes = R.replace 15 (B.singleton (negate (B.foldl' (+) 0 (B.take 15 bytes)))) bytes
getHalf :: B.ByteString -> Int -> Word16
getHalf bytes offset = fromIntegral (B.index bytes offset) + 256 * fromIntegral (B.index bytes (offset + 1))

observe :: Input -> Observation
observe input = finish (foldl step initial (updates input))
  where
    initial = (backing input, working input, snapshot input, R.initialStore R.Kind2 (initialMedium input) (fault input) R.Hardware2, True, True, [])
    step (live, current, saved, store, area2Open, area3Open, statuses) operation = case operation of
      LiveByte offset value -> case live of
        Backing bytes | offset >= 0 && offset < 0x19a -> (Backing (R.replace offset (B.singleton value) bytes), current, saved, store, area2Open, area3Open, statuses)
        _ -> error "area2 scenario live offset"
      Sequence value -> case current of Record bytes -> (live, Record (R.replace 0 (R.half value) bytes), saved, store, area2Open, area3Open, statuses)
      Save selector ->
        let (live', current'@(Record bytes)) = prepare (version input) live current
            Record previous = saved
            store' = if area2Open && B.drop 2 bytes /= B.drop 2 previous then snd (R.append hardware (R.Seed 0) (R.CallerRecord bytes) store) else store
            saved' = if selector == Snapshot then current' else saved
            open' = area2Open && selector /= Close
            thirdOpen' = area3Open && selector /= Close
        in (live', current', saved', store', open', thirdOpen', statuses ++ [0])
    finish (live,current,saved,store,area2Open,area3Open,statuses) = Observation current saved live (R.medium (R.persistence store)) area2Open area3Open statuses (backupObservation input)

synthetic :: [Update] -> R.Fault -> Input
synthetic operations injection = Input prepared prepared live (Version 1 80) operations (R.Medium M.empty) injection False
  where
    live = Backing (R.replace 0 (R.half 17 <> R.half 101) (B.replicate 0x19a 0x35))
    (_, prepared) = prepare (Version 1 80) live (Record (B.replicate 512 0x9a))

scenarios :: [(String, Input)]
scenarios =
  [ ("equal-snapshot", synthetic [Save Snapshot] R.NoFault)
  , ("sequence-only-no-append", synthetic [Sequence 12,Save Snapshot] R.NoFault)
  , ("changed-snapshot", synthetic [LiveByte 0x10 1,Save Snapshot] R.NoFault)
  , ("changed-close-both", synthetic [LiveByte 0x10 1,Save Close] R.NoFault)
  , ("failed-program-legacy-completion", synthetic [LiveByte 0x10 1,Save Snapshot] (R.FailRequest (R.PersistenceStep 0)))
  , ("rotation", synthetic (concat [[LiveByte 0x10 value,Save Snapshot] | value <- [1..32]]) R.NoFault)
  , ("full-pair-erase", synthetic (concat [[LiveByte 0x10 value,Save Snapshot] | value <- [1..63]]) R.NoFault)
  , ("closed-handle-legacy-completion", synthetic [Save Close,LiveByte 0x10 1,Save Snapshot] R.NoFault)
  , ("counter-rollover", (synthetic [Save Snapshot] R.NoFault) { backing = Backing (R.replace 0 (R.half 9999 <> R.half 999) (B.replicate 0x19a 0x35)) })
  , ("failed-backup-legacy-completion", (synthetic [Save Snapshot] R.NoFault) { backing = Backing (R.replace 0 (R.half 9999 <> R.half 999) (B.replicate 0x19a 0x35)), backupFails = True })
  , ("stale-record-distinct-fields", (synthetic [Save Snapshot] R.NoFault) { working = Record (B.replicate 512 0x9a), snapshot = Record (B.replicate 512 0x8b), backing = Backing (B.pack [fromIntegral (index `mod` 251) | index <- [0..0x199 :: Int]]), version = Version 2 99 })
  ]

backupObservation :: Input -> B.ByteString
backupObservation input = let (_,retained,_) = foldl step initial (updates input) in retained
  where
    initial = (backing input, B.replicate 16 0x35, B.replicate 16 0x35)
    step state@(Backing bytes, retained, shadow) operation = case operation of
      LiveByte offset value -> (Backing (R.replace offset (B.singleton value) bytes), retained, shadow)
      Sequence _ -> state
      Save _
        | getHalf bytes 0 == 9999 && getHalf bytes 2 == 999 ->
            let first = seal (R.replace 2 (R.half 100) bytes)
                second = seal (R.replace 0 (R.half 0) first)
            in synchronize second (synchronize first state)
        | otherwise -> state
    seal bytes = R.replace 15 (B.singleton (negate (B.foldl' (+) 0 (B.take 15 bytes)))) bytes
    synchronize live (_,retained,shadow) =
      let header = B.take 16 live
          device = B.pack [if B.index header index /= B.index shadow index && not (backupFails input) then B.index header index else B.index retained index | index <- [0..15]]
      in (Backing live,device,header)
