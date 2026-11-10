{-# LANGUAGE GHC2021 #-}
module Oracle where

import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Word (Word16, Word32, Word8)
import PropertyFields (PropertyId(..))
import Emulation.OwnedCase (readWordAt, word32le)
import qualified Replacement.PropertyService.Oracle as P
import Replacement.PropertyService.Classify (Owned)

newtype BackupKey = BackupKey Word16 deriving (Eq, Show)
data TaskContext = Task | Interrupt deriving (Eq, Ord, Show)
data DriverEffect = DriverEffect BackupKey Word8 Word8 deriving (Eq, Show)
data MirrorState = MirrorState { liveBytes :: B.ByteString, shadowBytes :: B.ByteString } deriving (Eq, Show)
data Event = Event Word32 Word32 Word32 Word32 deriving (Eq, Show)

replace :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replace at value old = B.take at old <> value <> B.drop (at + B.length value) old

checksum :: B.ByteString -> B.ByteString
checksum bytes = B.take 15 bytes <> B.singleton (negate (B.foldl' (+) 0 (B.take 15 bytes)))

keysFromImage :: B.ByteString -> [BackupKey]
keysFromImage image = [BackupKey (fromIntegral (B.index bytes i) + 256 * fromIntegral (B.index bytes (i+1))) | i <- [0,2..30]]
  where bytes = B.take 32 (B.drop (0x7cb7d4 - 0x61c400) image)

closeMirror :: [BackupKey] -> B.ByteString -> MirrorState -> (MirrorState, [DriverEffect])
closeMirror keys incoming old = (MirrorState final final, effects)
  where
    final = checksum incoming
    effects = [DriverEffect key value previous | (i,key) <- zip [0..15] keys, let value = B.index final i, let previous = B.index (shadowBytes old) i, value /= previous]

edges :: TaskContext -> [Event]
edges Task = [Event 0x724b68 0x25 0xffffffff 0, Event 0x724c00 0x25 0 0]
edges Interrupt = [Event 0x726c74 2 0x3b 0]

requestEdges :: TaskContext -> [Event] -> [Event]
requestEdges Task middle = [Event 0x724b68 0x25 0xffffffff 0] ++ middle ++ [Event 0x724c00 0x25 0 0]
requestEdges Interrupt middle = Event 0x726c74 2 0x3b 0 : middle

newtype DeferredPairByte = DeferredPairByte Word8

pairByte :: B.ByteString -> DeferredPairByte
pairByte banks = DeferredPairByte (B.index banks 0x126)

driverEvents :: DeferredPairByte -> [DriverEffect] -> [Event]
driverEvents (DeferredPairByte deferred) = map (\(DriverEffect (BackupKey key) value previous) -> Event 0x6a59b8 (fromIntegral key) (fromIntegral value) (fromIntegral previous * 256 + fromIntegral deferred * 65536))

eventBytes :: [Event] -> B.ByteString
eventBytes events = word32le (fromIntegral (length events)) <> B.concat [B.concat (map word32le [site,a,b,c]) | Event site a b c <- events] <> B.replicate (0x800 - 4 - 16 * length events) 0

initialMirror :: P.Fixture -> MirrorState
initialMirror fx = MirrorState (B.map complement (P.backupWindow fx)) (P.backupWindow fx)

propertyResult :: [BackupKey] -> TaskContext -> P.Fixture -> Owned -> P.Operation -> Maybe (P.Effect, MirrorState, [Event])
propertyResult keys context fx owned operation = do
  effect <- P.modelEffect fx owned operation
  let opened = readWordAt (P.effectCalls effect) 0 /= 0
      closed = readWordAt (P.effectCalls effect) 4 /= 0
      old = initialMirror fx
      incoming = B.take 16 (B.drop 16 (P.effectCalls effect))
      (new, drivers) = if closed then closeMirror keys incoming old else (old, [])
      events = (if opened then requestEdges context [] else []) ++ (if closed then requestEdges context (driverEvents (pairByte (P.fixtureBanks fx)) drivers) else [])
  pure (effect { P.effectPrevious = if opened || closed then replace 0x1c4 (word32le 0) (P.effectPrevious effect) else P.effectPrevious effect }, new, events)

newtype HeaderByteValue = HeaderByteValue Word8
data ExchangeDirection = CaptureAlternate | RestoreAlternate | IgnoreInvalidDirection
data StorageOperation = Capture | Restore | CopyActiveToPrevious | Exchange ExchangeDirection | FieldWrite P.OracleCase Owned | ApplyField P.OracleCase Owned | Initialize Owned | RetainedHeaderWrite HeaderByteValue

data StorageState = StorageState
  { storageFixture :: P.Fixture
  , storageBacking :: B.ByteString
  , storageEvents :: [Event]
  }

withStatus :: P.Fixture -> P.Fixture
withStatus fx = fx { P.fixturePrevious = replace 0x1c4 (word32le 0) (P.fixturePrevious fx) }

modelOperation :: [BackupKey] -> TaskContext -> StorageState -> StorageOperation -> Maybe StorageState
modelOperation keys context state operation = case operation of
  RetainedHeaderWrite (HeaderByteValue value) ->
    let changed = replace 2 (B.singleton value) backing
        (final,drivers) = closeMirror keys (B.take 16 changed) mirror
    in pure state { storageBacking = replace 0 (liveBytes final) (replace 0x19c (shadowBytes final) changed), storageEvents = storageEvents state ++ driverEvents (pairByte (P.fixtureBanks fx)) drivers }
  Capture -> pure state { storageFixture = fx { P.fixturePrevious = replace 0 (B.take 0x1c2 (P.fixtureBanks fx)) (P.fixturePrevious fx) } }
  CopyActiveToPrevious -> modelOperation keys context state Capture
  Exchange CaptureAlternate -> pure state { storageFixture = fx { P.fixtureBanks = replace 0x1c2 (B.take 0x1c2 (P.fixtureBanks fx)) (P.fixtureBanks fx) } }
  Exchange RestoreAlternate -> pure (writeBody state { storageFixture = fx { P.fixtureBanks = replace 0 (B.take 0x1c2 (B.drop 0x1c2 (P.fixtureBanks fx))) (P.fixtureBanks fx) } })
  Exchange IgnoreInvalidDirection -> pure state
  Restore -> pure (writeBody state { storageFixture = fx { P.fixtureBanks = replace 0 (B.take 0x1c2 (P.fixturePrevious fx)) (P.fixtureBanks fx) } })
  FieldWrite oracleCase owned -> writeField state oracleCase owned
  ApplyField oracleCase owned -> applyField state oracleCase owned
  Initialize startupOwned -> do
    let loaded = withStatus fx
          { P.fixtureBanks = loadImage 0x1c2 (loadImage 0 (P.fixtureBanks fx))
          , P.fixturePrevious = loadImage 0 (P.fixturePrevious fx)
          }
        initialized = state { storageFixture = loaded, storageEvents = storageEvents state ++ concat (replicate 9 (requestEdges context [])) }
    if B.index (P.fixtureConfig fx) 0xe8 /= 0 then pure initialized
      else applyField initialized (P.OracleCase (PropertyId 0x49) (P.Set P.Bank0 (P.SlotArgument 1) P.SetFieldMax)) startupOwned
  where
    fx = storageFixture state
    backing = storageBacking state
    mirror = MirrorState (B.take 16 backing) (B.take 16 (B.drop 0x19c backing))
    loadImage at image = foldl (\bytes (offset,value) -> replace (at+offset) value bytes) image
      [(0,shadowBytes mirror),(16,B.take 0x118 (B.drop 16 backing)),(0x140,B.take 40 (B.drop 0x140 backing)),(0x19a,B.take 40 (P.fixtureTailDefaults fx))]
    writeField current oracleCase owned = do
      let input = storageFixture current
          shared = storageBacking current
          shadow = B.take 16 (B.drop 0x19c shared)
      preliminary <- P.modelEffect input owned (P.oracleOperation oracleCase)
      let opened = readWordAt (P.effectCalls preliminary) 0 /= 0
          source = if opened then input { P.fixtureBanks = replace 0 (B.map complement shadow) (P.fixtureBanks input) } else input
      effect <- P.modelEffect source owned (P.oracleOperation oracleCase)
      let closed = readWordAt (P.effectCalls effect) 4 /= 0
          (final,drivers) = closeMirror keys (B.take 16 (B.drop 16 (P.effectCalls effect))) (MirrorState (B.take 16 shared) shadow)
          newFixture = input { P.fixtureBanks = P.effectBanks effect, P.fixturePrevious = P.effectPrevious effect }
      pure current
        { storageFixture = if opened || closed then withStatus newFixture else newFixture
        , storageBacking = if closed then replace 0 (liveBytes final) (replace 0x19c (shadowBytes final) shared) else shared
        , storageEvents = storageEvents current ++ (if opened then requestEdges context [] else []) ++ (if closed then requestEdges context (driverEvents (pairByte (P.fixtureBanks input)) drivers) else [])
        }
    applyField current oracleCase owned = do
      let input = storageFixture current
          active = case P.oracleOperation oracleCase of P.Set P.Bank0 _ _ -> True; _ -> False
          refreshed = if active then current
            { storageFixture = withStatus input { P.fixtureBanks = replace 16 (B.take 0x118 (B.drop 16 (storageBacking current))) (P.fixtureBanks input) }
            , storageEvents = storageEvents current ++ requestEdges context []
            } else current
      written <- writeField refreshed oracleCase owned
      effect <- P.modelEffect (storageFixture refreshed) owned (P.oracleOperation oracleCase)
      pure (if active && P.effectR0 effect == 1 then writeBody written else written)
    writeBody current =
      let input = storageFixture current
          shared = storageBacking current
          (final,drivers) = closeMirror keys (B.take 16 shared) (MirrorState (B.take 16 shared) (B.take 16 (B.drop 0x19c shared)))
          body = B.take 0x118 (B.drop 16 (P.fixtureBanks input))
      in current
        { storageFixture = withStatus input
        , storageBacking = replace 16 body (replace 0 (liveBytes final) (replace 0x19c (shadowBytes final) shared))
        , storageEvents = storageEvents current ++ requestEdges context (driverEvents (pairByte (P.fixtureBanks input)) drivers)
        }
