{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.ModalRuntime.RenderObjectProvenance
  ( ProvenanceBatch, ProvenanceState, ProvenanceSequence(..), HandleIdentity
  , ObjectAddress(..), HandleAddress(..), emptyProvenance, parseProvenanceBatch
  , beginBatch, consumeThrough, batchEnd, batchEventCount, handleIdentityAt, combineBatches
  ) where

import Control.Monad (unless, when)
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word64)
import Numeric (readHex, showHex)

newtype ProvenanceSequence = ProvenanceSequence Word64 deriving (Eq, Ord, Show)
newtype ObjectAddress = ObjectAddress Word32 deriving (Eq, Ord, Show)
newtype HandleAddress = HandleAddress Word32 deriving (Eq, Ord, Show)
newtype AllocationExtent = AllocationExtent Word32 deriving (Eq, Show)
newtype AllocationOrdinal = AllocationOrdinal Word64 deriving (Eq, Show)
data Constructor = ImageD4 | Vector14c | VectorD0 deriving (Eq, Ord, Show)
data HandleIdentity = HandleIdentity Constructor AllocationExtent AllocationOrdinal deriving (Eq, Show)
data BoundHandle = BoundHandle HandleAddress HandleIdentity deriving (Eq, Show)
data Clock = Clock Word64 Word64 deriving (Eq, Show)
data EventKind = Allocate Constructor HandleAddress AllocationExtent AllocationOrdinal
  | Bind ObjectAddress HandleAddress | Free HandleAddress deriving (Eq, Show)
data EventCore = Primary | Secondary deriving (Eq, Show)
data ProvenanceEvent = ProvenanceEvent ProvenanceSequence Clock EventCore EventKind deriving (Eq, Show)
data ProvenanceBatch = ProvenanceBatch Clock Clock ProvenanceSequence ProvenanceSequence [ProvenanceEvent] deriving (Eq, Show)
data ProvenanceState = ProvenanceState
  { nextSequence :: ProvenanceSequence
  , coverageEnd :: Maybe Clock
  , constructors :: M.Map Constructor AllocationOrdinal
  , allocations :: M.Map HandleAddress HandleIdentity
  , bindings :: M.Map ObjectAddress BoundHandle
  } deriving (Eq, Show)

emptyProvenance :: ProvenanceState
emptyProvenance = ProvenanceState (ProvenanceSequence 0) Nothing M.empty M.empty M.empty

instance J.ToJSON ProvenanceSequence where
  toJSON (ProvenanceSequence n) = J.toJSON n

instance J.FromJSON ProvenanceBatch where
  parseJSON = parseProvenanceBatch

instance J.ToJSON ProvenanceBatch where
  toJSON (ProvenanceBatch start end first last_ events) = J.object
    [ "version" J..= (1 :: Int), "start" J..= clockValue start, "end" J..= clockValue end
    , "sequence_start" J..= first, "sequence_end" J..= last_, "overflow" J..= False
    , "coverage" J..= ("continuous" :: String), "events" J..= map eventValue events ]
   where
    clockValue (Clock instructions tick) = J.object ["instructions" J..= instructions,"tick" J..= tick]
    addressValue n = let digits = showHex n "" in "0x" ++ replicate (8-length digits) '0' ++ digits
    eventValue (ProvenanceEvent eventSequence (Clock instructions tick) core event) = J.object
      (["sequence" J..= eventSequence,"instructions" J..= instructions,"tick" J..= tick
       ,"core" J..= (case core of Primary -> 0; Secondary -> 1 :: Int)] ++ case event of
        Allocate constructor (HandleAddress base) (AllocationExtent extent) (AllocationOrdinal ordinal) ->
          [ "kind" J..= ("allocate" :: String), "constructor" J..= (case constructor of ImageD4 -> "image_d4"; Vector14c -> "vector_14c"; VectorD0 -> "vector_d0" :: String)
          , "allocation_base" J..= addressValue base, "extent" J..= extent, "ordinal" J..= ordinal ]
        Bind (ObjectAddress object) (HandleAddress handle) ->
          ["kind" J..= ("bind" :: String),"object_base" J..= addressValue object,"allocation_base" J..= addressValue handle]
        Free (HandleAddress base) -> ["kind" J..= ("free" :: String),"allocation_base" J..= addressValue base])

combineBatches :: [ProvenanceBatch] -> Either String (Maybe ProvenanceBatch)
combineBatches [] = Right Nothing
combineBatches batches@(ProvenanceBatch start _ first _ _:_) = do
  unless (sum (map batchEventCount batches) <= 65536) (Left "render object provenance forwarding overflow")
  (end,last_) <- finalBoundary batches
  let events = concat [entries | ProvenanceBatch _ _ _ _ entries <- batches]
  pure (Just (ProvenanceBatch start end first last_ events))
 where
  finalBoundary [ProvenanceBatch _ end _ last_ _] = Right (end,last_)
  finalBoundary (ProvenanceBatch _ end _ last_ _:rest@(ProvenanceBatch nextStart _ nextFirst _ _:_)) = do
    unless (end == nextStart && last_ == nextFirst) (Left "render object provenance transport gap")
    finalBoundary rest
  finalBoundary [] = Left "render object provenance has no final boundary"

batchEventCount :: ProvenanceBatch -> Int
batchEventCount (ProvenanceBatch _ _ _ _ events) = length events


parseAddress :: String -> J.Parser Word32
parseAddress text = case text of
  '0':'x':digits | length digits == 8, [(value,"")] <- readHex digits -> pure value
  _ -> fail "render object provenance requires a hexadecimal word"

parseClock :: J.Value -> J.Parser Clock
parseClock = J.withObject "provenance clock" $ \o -> Clock <$> o J..: "instructions" <*> o J..: "tick"

parseProvenanceBatch :: J.Value -> J.Parser ProvenanceBatch
parseProvenanceBatch = J.withObject "render object provenance" $ \o -> do
  version <- o J..: "version" :: J.Parser Int
  unless (version == 1) (fail "unsupported render object provenance version")
  overflow <- o J..: "overflow"
  when overflow (fail "render object provenance overflow")
  coverage <- o J..: "coverage" :: J.Parser String
  unless (coverage == "continuous") (fail "render object provenance coverage refused")
  start <- o J..: "start" >>= parseClock
  end <- o J..: "end" >>= parseClock
  first <- ProvenanceSequence <$> o J..: "sequence_start"
  last_ <- ProvenanceSequence <$> o J..: "sequence_end"
  events <- o J..: "events" >>= mapM parseEvent
  unless (length events <= 65536) (fail "render object provenance event bound exceeded")
  pure (ProvenanceBatch start end first last_ events)
 where
  parseEvent = J.withObject "render object provenance event" $ \o -> do
    eventSequence <- ProvenanceSequence <$> o J..: "sequence"
    clock <- Clock <$> o J..: "instructions" <*> o J..: "tick"
    core <- o J..: "core" :: J.Parser Int
    unless (core == 0 || core == 1) (fail "invalid render object provenance core")
    kind <- o J..: "kind" :: J.Parser String
    event <- case kind of
      "allocate" -> do
        name <- o J..: "constructor" :: J.Parser String
        constructor <- case name of
          "image_d4" -> pure ImageD4
          "vector_14c" -> pure Vector14c
          "vector_d0" -> pure VectorD0
          _ -> fail "unknown render object constructor"
        address <- HandleAddress <$> (o J..: "allocation_base" >>= parseAddress)
        extent <- AllocationExtent <$> o J..: "extent"
        ordinal <- AllocationOrdinal <$> o J..: "ordinal"
        pure (Allocate constructor address extent ordinal)
      "bind" -> Bind <$> (ObjectAddress <$> (o J..: "object_base" >>= parseAddress))
        <*> (HandleAddress <$> (o J..: "allocation_base" >>= parseAddress))
      "free" -> Free <$> (HandleAddress <$> (o J..: "allocation_base" >>= parseAddress))
      _ -> fail "unknown render object provenance event"
    pure (ProvenanceEvent eventSequence clock (if core == 0 then Primary else Secondary) event)

batchEnd :: ProvenanceBatch -> ProvenanceSequence
batchEnd (ProvenanceBatch _ _ _ end _) = end

clockBefore :: Clock -> Clock -> Bool
clockBefore (Clock ai at) (Clock bi bt) = ai <= bi && at <= bt

beginBatch :: ProvenanceState -> ProvenanceBatch -> Either String (ProvenanceState,[ProvenanceEvent])
beginBatch state (ProvenanceBatch start end first last_ events) = do
  unless (nextSequence state == first) (Left "render object provenance sequence gap")
  unless (maybe True (== start) (coverageEnd state)) (Left "render object provenance coverage gap")
  unless (clockBefore start end) (Left "render object provenance clock reversed")
  let ProvenanceSequence a = first
      ProvenanceSequence b = last_
  unless (toInteger b - toInteger a == toInteger (length events)) (Left "render object provenance event count differs")
  _ <- foldClock start events
  pure (state {coverageEnd=Just end},events)
 where
  foldClock previous [] = unless (clockBefore previous end) (Left "render object provenance event exceeds coverage")
  foldClock previous (ProvenanceEvent _ current _ _:rest) = do
    unless (clockBefore previous current) (Left "render object provenance events out of order")
    foldClock current rest

consumeThrough :: ProvenanceSequence -> (ProvenanceState,[ProvenanceEvent]) -> Either String (ProvenanceState,[ProvenanceEvent])
consumeThrough target (state,events) = do
  unless (target >= nextSequence state) (Left "render object provenance draw sequence reversed")
  case events of
    ProvenanceEvent eventSequence _ _ kind:rest | eventSequence < target -> do
      unless (eventSequence == nextSequence state) (Left "render object provenance event sequence differs")
      updated <- applyEvent kind state
      let ProvenanceSequence n = eventSequence
      when (n == maxBound) (Left "render object provenance sequence exhausted")
      consumeThrough target (updated {nextSequence=ProvenanceSequence (n+1)},rest)
    _ -> do
      unless (nextSequence state == target) (Left "render object provenance draw exceeds captured events")
      pure (state,events)

applyEvent :: EventKind -> ProvenanceState -> Either String ProvenanceState
applyEvent event state = case event of
  Allocate constructor address@(HandleAddress base) extent@(AllocationExtent size) ordinal@(AllocationOrdinal n) -> do
    let expectedSize = case constructor of ImageD4 -> 212; Vector14c -> 332; VectorD0 -> 208
        AllocationOrdinal previous = M.findWithDefault (AllocationOrdinal 0) constructor (constructors state)
    unless (base /= 0 && base `mod` 4 == 0 && size == expectedSize
      && toInteger base + toInteger size <= 0x100000000) (Left "invalid render object allocation extent")
    unless (previous /= maxBound && n == previous+1) (Left "render object allocation ordinal differs")
    when (M.member address (allocations state)) (Left "render object allocation reused while live")
    pure state {constructors=M.insert constructor ordinal (constructors state),allocations=M.insert address (HandleIdentity constructor extent ordinal) (allocations state)}
  Bind object@(ObjectAddress address) handle -> do
    unless (address >= 0x15e7538 && address < 0x15e7538+19*27136
      && (address-0x15e7538) `mod` 212 == 0) (Left "render object bind outside object slots")
    case M.lookup handle (allocations state) of
      Nothing -> Left "render object bind lacks an exact live allocation base"
      Just identity -> pure state {bindings=M.insert object (BoundHandle handle identity) (bindings state)}
  Free address -> pure state {allocations=M.delete address (allocations state)}

handleIdentityAt :: ProvenanceState -> ObjectAddress -> HandleAddress -> Either String (Maybe HandleIdentity)
handleIdentityAt _ _ (HandleAddress 0) = Right Nothing
handleIdentityAt state object address = case M.lookup object (bindings state) of
  Nothing -> Right Nothing
  Just (BoundHandle bound identity) -> do
    unless (address == bound && M.lookup address (allocations state) == Just identity)
      (Left "render object retains a stale allocation handle")
    pure (Just identity)
