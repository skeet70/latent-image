{-# LANGUAGE OverloadedStrings #-}
-- | The own-first natural pair's plan: the slot entries, the data words each slot reads, the
-- guarded extents with their expected bytes, the fill words inside them and the +0x1618 tails.
module Emulation.OwnFirstPairPlan
  ( Role (..)
  , roleWire
  , parseRole
  , Slot (..)
  , slotOffset
  , slotFromOffset
  , WordName (..)
  , wordNameWire
  , Pointee (..)
  , SlotReads (..)
  , slotReads
  , DomainWords (..)
  , domainWords
  , domainWord
  , loaderEntry
  , GuardedRange (..)
  , PlanEntry (..)
  , PlanTail (..)
  , Plan (..)
  , derivePlan
  , planValue
  , decodePlan
  , fillWord
  , primaryIdentity
  , contextError
  , taskIndex
  ) where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, withText, (.:), (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import Data.Aeson.Types (Parser)
import Data.Bits (complement, shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Int (Int32)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import Replacement.RtosOwnFirst.Regions

-- | Expectations come from the stock loaders, the staged loaders, or a guest control's loaders.
data Role = StockRole | OwnedRole | ControlRestoredRole | ControlWrongIRole deriving (Eq, Ord, Show, Enum, Bounded)

roleWire :: Role -> String
roleWire role = case role of
  StockRole -> "stock"
  OwnedRole -> "owned"
  ControlRestoredRole -> "control-restored"
  ControlWrongIRole -> "control-wrong-i"

parseRole :: String -> Either String Role
parseRole text = maybe (Left ("unknown role " ++ text)) Right (lookup text [(roleWire role, role) | role <- [minBound .. maxBound]])

data Slot
  = Slot60 | SlotFc | Slot100 | Slot128 | Slot12c | Slot130 | Slot134 | Slot138 | Slot13c
  | Slot148 | Slot14c | Slot150 | Slot154 | Slot164 | Slot168 | Slot16c | Slot1618
  deriving (Eq, Ord, Show, Enum, Bounded)

slotOffset :: Slot -> Word32
slotOffset slot = case slot of
  Slot60 -> 0x60
  SlotFc -> 0xfc
  Slot100 -> 0x100
  Slot128 -> 0x128
  Slot12c -> 0x12c
  Slot130 -> 0x130
  Slot134 -> 0x134
  Slot138 -> 0x138
  Slot13c -> 0x13c
  Slot148 -> 0x148
  Slot14c -> 0x14c
  Slot150 -> 0x150
  Slot154 -> 0x154
  Slot164 -> 0x164
  Slot168 -> 0x168
  Slot16c -> 0x16c
  Slot1618 -> 0x1618

slotFromOffset :: Word32 -> Maybe Slot
slotFromOffset offset = lookup offset [(slotOffset slot, slot) | slot <- [minBound .. maxBound]]

data WordName = ContextWord | CurrentTask | DispatchEnabled | TimerBase | TimeOffset deriving (Eq, Ord, Show, Enum, Bounded)

wordNameWire :: WordName -> T.Text
wordNameWire name = case name of
  ContextWord -> "context"
  CurrentTask -> "current"
  DispatchEnabled -> "dispatch"
  TimerBase -> "timer-base"
  TimeOffset -> "time-offset"

data Pointee = NoPointee | PointeeAtEntry | PointeeAtEntryAndReturn deriving (Eq, Show)

pointeeWire :: Pointee -> T.Text
pointeeWire pointee = case pointee of
  NoPointee -> "none"
  PointeeAtEntry -> "entry"
  PointeeAtEntryAndReturn -> "entry-and-return"

-- | What the observer records for a slot: words at entry, the byte at current-task + 0xe, reads of
-- the word at entry r0, and words at the continuation.
data SlotReads = SlotReads
  { readsAtEntry :: [WordName]
  , readsTaskFlags :: Bool
  , readsPointee :: Pointee
  , readsAtReturn :: [WordName]
  } deriving (Eq, Show)

slotReads :: Slot -> SlotReads
slotReads slot = case slot of
  Slot60 -> SlotReads [CurrentTask] True NoPointee []
  SlotFc -> SlotReads [ContextWord, TimerBase] False PointeeAtEntry [TimeOffset, TimerBase]
  Slot100 -> SlotReads [ContextWord, TimerBase, TimeOffset] False PointeeAtEntryAndReturn [TimerBase]
  Slot128 -> SlotReads [ContextWord, CurrentTask] False PointeeAtEntryAndReturn []
  Slot12c -> SlotReads [ContextWord, CurrentTask] False PointeeAtEntryAndReturn []
  Slot130 -> context
  Slot134 -> context
  Slot138 -> context
  Slot13c -> context
  Slot148 -> context
  Slot14c -> none
  Slot150 -> SlotReads [DispatchEnabled] False NoPointee []
  Slot154 -> SlotReads [ContextWord, DispatchEnabled] False NoPointee []
  Slot164 -> none
  Slot168 -> none
  Slot16c -> SlotReads [ContextWord, CurrentTask] False NoPointee []
  Slot1618 -> none
 where
  context = SlotReads [ContextWord] False NoPointee []
  none = SlotReads [] False NoPointee []

-- | A loader's data words at their stock addresses, and the task table base the index uses.
data DomainWords = DomainWords
  { wordsContext :: Word32
  , wordsCurrent :: Word32
  , wordsDispatch :: Word32
  , wordsTimerBase :: Word32
  , wordsTimeOffset :: Word32
  , wordsTaskTable :: Word32
  } deriving (Eq, Show)

domainWords :: Domain -> DomainWords
domainWords domain = case domain of
  DomainA -> DomainWords 0x400545a4 0x40054760 0x40054764 0x4005476c 0x40054774 0x4005ff0c
  DomainB -> DomainWords 0x400e4564 0x400e4720 0x400e4724 0x400e472c 0x400e4734 0x400efecc

domainWord :: Domain -> WordName -> Word32
domainWord domain name = let words' = domainWords domain in case name of
  ContextWord -> wordsContext words'
  CurrentTask -> wordsCurrent words'
  DispatchEnabled -> wordsDispatch words'
  TimerBase -> wordsTimerBase words'
  TimeOffset -> wordsTimeOffset words'

-- | The instruction each loader's startup enters it at.
loaderEntry :: Domain -> Word32
loaderEntry domain = case domain of
  DomainA -> 0x40020
  DomainB -> 0xd0020

fillWord :: Word32
fillWord = 0xe7f000f0

primaryIdentity :: Word32
primaryIdentity = 0x4003

-- | -25 as the slots return it.
contextError :: Word32
contextError = 0xffffffe7

-- | The +0x128 index of a current-task node.
taskIndex :: Domain -> Word32 -> Word32
taskIndex domain current =
  let delta = fromIntegral (current - wordsTaskTable (domainWords domain)) :: Int32
   in fromIntegral (delta `shiftR` 3) * 0xb6db6db7 + 1

data GuardedRange = GuardedRange {guardedDomain :: Domain, guardedStart :: Word32, guardedEnd :: Word32, guardedSha256 :: String}
  deriving (Eq, Show)

data PlanEntry = PlanEntry {planEntryDomain :: Domain, planEntrySlot :: Slot, planEntryAddress :: Word32} deriving (Eq, Show)

data PlanTail = PlanTail {planTailDomain :: Domain, planTailBody :: Word32} deriving (Eq, Show)

data Plan = Plan
  { planRole :: Role
  , planDatSha256 :: String
  , planEntries :: [PlanEntry]
  , planTails :: [PlanTail]
  , planGuarded :: [GuardedRange]
  , planFills :: [Word32]
  } deriving (Eq, Show)

word32At :: B.ByteString -> Int -> Word32
word32At bytes offset = foldr (\index acc -> (acc `shiftL` 8) .|. fromIntegral (B.index bytes (offset + index))) 0 [0 .. 3]

-- | An unconditional A32 `b` at `from` to `to`.
branchTo :: Word32 -> Word32 -> Word32 -> Bool
branchTo from to word = word .&. 0xff000000 == 0xea000000 && from + 8 + ((signed .&. 0xffffff) `shiftL` 2) == to
 where
  signed = if word .&. 0x800000 /= 0 then (word .&. 0xffffff) .|. complement 0xffffff else word .&. 0xffffff

-- | The plan for one role from the region table, the role's DAT digest and the loaders the role's
-- extents must hold.
derivePlan :: Role -> String -> RegionTable -> M.Map Domain B.ByteString -> Either String Plan
derivePlan role datDigest table loaders = do
  unless (tableFill table == fillWord) (Left "regions.json fill word differs")
  perDomain <- forM domains $ \domain -> do
    loader <- maybe (Left ("no loader for domain " ++ domainLabel domain)) Right (M.lookup domain loaders)
    unless (B.length loader == domainLoaderSize domain) (Left ("loader " ++ domainLabel domain ++ " has the wrong size"))
    let dt = domainTable table domain
        GuestAddress base = domainLoaderBase domain
        bytesOf (Extent (GuestAddress start) (GuestAddress end)) = B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - base)) loader)
        wordAt address = word32At loader (fromIntegral (address - base))
    entries <- forM (domainExtents dt) $ \region -> do
      let SlotOffset offset = regionSlot region
          GuestAddress entry = regionEntry region
      slot <- maybe (Left ("regions.json names an unknown slot " ++ hexText offset)) Right (slotFromOffset offset)
      pure (PlanEntry domain slot entry)
    let guarded = [ GuardedRange domain start end (sha256Hex (sha256 (bytesOf extent)))
                  | region <- domainExtents dt, let extent@(Extent (GuestAddress start) (GuestAddress end)) = regionExtent region ]
        fills = [ address | GuardedRange _ start end _ <- guarded, address <- [start, start + 4 .. end - 4], wordAt address == fillWord ]
        GuestAddress tailEntry = regionEntry (tableTail dt)
        Extent (GuestAddress body) _ = bodyExtent (tableBody dt)
        tails = [PlanTail domain body | branchTo tailEntry body (wordAt tailEntry)]
    pure (entries, guarded, fills, tails)
  let entries = concat [e | (e, _, _, _) <- perDomain]
  unless (length entries == 34 && all (\slot -> length (filter ((== slot) . planEntrySlot) entries) == 2) [minBound .. maxBound])
    (Left "regions.json does not name the 17 slots in both loaders")
  pure Plan
    { planRole = role
    , planDatSha256 = datDigest
    , planEntries = entries
    , planTails = concat [t | (_, _, _, t) <- perDomain]
    , planGuarded = concat [g | (_, g, _, _) <- perDomain]
    , planFills = concat [f | (_, _, f, _) <- perDomain]
    }

hexWord :: Word32 -> String
hexWord value = "0x" ++ showHex value ""

-- | The observer's `--plan` document.
planValue :: Plan -> Value
planValue plan = object
  [ "schema" .= (1 :: Int)
  , "role" .= roleWire (planRole plan)
  , "dat_sha256" .= planDatSha256 plan
  , "domains" .=
      [ object
          [ "domain" .= domainLabel domain
          , "loader_entry" .= hexWord (loaderEntry domain)
          , "words" .= object [K.fromText (wordNameWire name) .= hexWord (domainWord domain name) | name <- [minBound .. maxBound]]
          ]
      | domain <- domains ]
  , "entries" .=
      [ object
          [ "domain" .= domainLabel domain
          , "slot" .= hexWord (slotOffset slot)
          , "entry" .= hexWord address
          , "reads" .= map wordNameWire (readsAtEntry reads')
          , "task_flags" .= readsTaskFlags reads'
          , "pointee" .= pointeeWire (readsPointee reads')
          , "return_reads" .= map wordNameWire (readsAtReturn reads')
          ]
      | PlanEntry domain slot address <- planEntries plan, let reads' = slotReads slot ]
  , "tails" .= [object ["domain" .= domainLabel domain, "body" .= hexWord body] | PlanTail domain body <- planTails plan]
  , "guarded" .=
      [ object ["domain" .= domainLabel domain, "start" .= hexWord start, "end" .= hexWord end, "sha256" .= digest]
      | GuardedRange domain start end digest <- planGuarded plan ]
  , "fills" .= map hexWord (planFills plan)
  ]

parseHex :: Value -> Parser Word32
parseHex = withText "hex word" $ \text -> case T.unpack text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits, value <= toInteger (maxBound :: Word32) -> pure (fromInteger value)
  _ -> fail ("not a hexadecimal word: " ++ T.unpack text)

parseDomain :: Value -> Parser Domain
parseDomain = withText "domain" $ \text -> case text of
  "a" -> pure DomainA
  "b" -> pure DomainB
  _ -> fail "unknown domain"

newtype ParsedPlan = ParsedPlan Plan

instance FromJSON ParsedPlan where
  parseJSON = withObject "own-first pair plan" $ \fields -> do
    schema <- fields .: "schema"
    unless (schema == (1 :: Int)) (fail "unsupported plan schema")
    role <- fields .: "role" >>= either fail pure . parseRole
    planDomains <- fields .: "domains" >>= mapM (withObject "plan domain" $ \entry -> do
      domain <- entry .: "domain" >>= parseDomain
      entered <- entry .: "loader_entry" >>= parseHex
      named <- entry .: "words"
      values <- forM [minBound .. maxBound] $ \name -> (,) name <$> (named .: K.fromText (wordNameWire name) >>= parseHex)
      pure (domain, entered, values))
    unless (planDomains == [(domain, loaderEntry domain, [(name, domainWord domain name) | name <- [minBound .. maxBound]]) | domain <- domains])
      (fail "plan loader entries or data words differ from the stock addresses")
    entries <- fields .: "entries" >>= mapM (withObject "entry" $ \entry -> do
      offset <- entry .: "slot" >>= parseHex
      slot <- maybe (fail "unknown slot") pure (slotFromOffset offset)
      PlanEntry <$> (entry .: "domain" >>= parseDomain) <*> pure slot <*> (entry .: "entry" >>= parseHex))
    tails <- fields .: "tails" >>= mapM (withObject "tail" $ \entry ->
      PlanTail <$> (entry .: "domain" >>= parseDomain) <*> (entry .: "body" >>= parseHex))
    guarded <- fields .: "guarded" >>= mapM (withObject "guarded range" $ \entry ->
      GuardedRange <$> (entry .: "domain" >>= parseDomain) <*> (entry .: "start" >>= parseHex)
        <*> (entry .: "end" >>= parseHex) <*> entry .: "sha256")
    fills <- fields .: "fills" >>= mapM parseHex
    digest <- fields .: "dat_sha256"
    pure (ParsedPlan (Plan role digest entries tails guarded fills))

decodePlan :: B.ByteString -> Either String Plan
decodePlan bytes = (\(ParsedPlan plan) -> plan) <$> A.eitherDecodeStrict' bytes
