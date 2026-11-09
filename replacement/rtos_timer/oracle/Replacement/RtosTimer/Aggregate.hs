{-# LANGUAGE OverloadedStrings #-}

module Replacement.RtosTimer.Aggregate
  ( Aggregate
  , loadAggregate
  , aggregateRegions
  , aggregateControlRegions
  , aggregateFunctions
  , admissionControls
  ) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.List (isPrefixOf, nub, sort, sortOn)
import Data.Word (Word32)
import Emulation.NativeObservation (GuestAddress (..), RegionId (..))
import Emulation.OwnedCase
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Aggregate as A
import qualified Replacement.RtosPlatform.Union as U

data Aggregate = Aggregate A.AggregateImage [A.AggregateSymbol] [A.SectionPlacement]

contains :: Word32 -> Word32 -> Word32 -> Word32 -> Bool
contains base size address bytes = bytes > 0 && address >= base &&
  toInteger address + toInteger bytes <= toInteger base + toInteger size

disjoint :: Word32 -> Word32 -> R.Extent -> Bool
disjoint address bytes (R.Extent (R.GuestAddress low) (R.GuestAddress high)) =
  toInteger address + toInteger bytes <= toInteger low || address >= high

placementAdmitted :: U.DomainMap -> A.SectionPlacement -> Bool
placementAdmitted domain placement =
  (executable placement || ".rodata" `isPrefixOf` A.placementSection placement) &&
  any carrierContains (U.mapCarriers domain)
  where
    carrierContains carrier = case U.carrierExtent carrier of
      R.Extent (R.GuestAddress low) (R.GuestAddress high) ->
        contains low (high - low) (A.placementAddress placement) (A.placementBytes placement)

executable :: A.SectionPlacement -> Bool
executable = isPrefixOf ".text" . A.placementSection

symbolAdmitted :: U.DomainMap -> [A.SectionPlacement] -> A.AggregateSymbol -> Bool
symbolAdmitted domain placements symbol
  | "xm1_platform_veneer_" `isPrefixOf` A.symbolName symbol = any fixed (U.mapEntries domain)
  | otherwise = any packed placements
  where
    fixed entry = A.symbolName symbol == U.entryVeneer entry &&
      R.GuestAddress (A.symbolAddress symbol) == U.entryAddress entry && A.symbolSize symbol == 4
    packed placement = executable placement &&
      contains (A.placementAddress placement) (A.placementBytes placement) (A.symbolAddress symbol) (A.symbolSize symbol)

loadAggregate :: [(Word32, Word32)] -> Word32 -> B.ByteString -> U.DomainMap -> FilePath -> FilePath -> FilePath -> IO Aggregate
loadAggregate foreignCode loaderBase loaderBytes domain binPath symbolsPath placementsPath = do
  forM_ foreignCode $ \(address, bytes) ->
    unless (all (disjoint address bytes . U.carrierExtent) (U.mapCarriers domain))
      (fail ("foreign fixture code overlaps owned aggregate carrier: " ++ show (address, bytes)))
  symbols <- either fail pure . A.parseAggregateSymbols =<< B.readFile symbolsPath
  placements <- either fail pure . A.parsePlacements =<< B.readFile placementsPath
  unless (not (null placements)) (fail "aggregate placements are empty")
  let ordered = sortOn A.placementAddress placements
  forM_ (zip ordered (drop 1 ordered)) $ \(left, right) ->
    unless (toInteger (A.placementAddress left) + toInteger (A.placementBytes left) <= toInteger (A.placementAddress right))
      (fail "aggregate placements overlap")
  image <- either fail pure . A.aggregateImage symbols =<< B.readFile binPath
  forM_ placements $ \placement -> do
    unless (placementAdmitted domain placement) (fail ("aggregate placement outside declared carriers: " ++ show placement))
    forM_ (U.mapEntries domain) $ \entry -> case U.entryAddress entry of
      R.GuestAddress address ->
        unless (disjoint (A.placementAddress placement) (A.placementBytes placement)
          (R.Extent (R.GuestAddress address) (R.GuestAddress (address + 4))))
          (fail "aggregate placement overlaps fixed ABI veneer")
    either fail (const (pure ())) (A.sliceAggregate image (A.placementAddress placement) (A.placementBytes placement))
  forM_ symbols $ \symbol ->
    unless (symbolAdmitted domain placements symbol) (fail ("aggregate symbol outside packed code or exact fixed ABI: " ++ show symbol))
  forM_ (filter ((== U.Timer) . U.entryGroup) (U.mapEntries domain)) $ \entry -> do
    let offset = if U.mapDomain domain == R.DomainA then 0 else 0x8ffc0
        expected = [("timer_initialize", 0x44a50), ("timer_insert", 0x44c60),
          ("timer_remove", 0x44ca8), ("timer_tick", 0x44d8c)]
    unless (any (\(name, address) -> U.entryName entry == name &&
      U.entryAddress entry == R.GuestAddress (address + offset)) expected)
      (fail ("aggregate timer ABI address differs: " ++ U.entryName entry))
    unless (any (\symbol -> A.symbolName symbol == U.entryVeneer entry &&
      R.GuestAddress (A.symbolAddress symbol) == U.entryAddress entry && A.symbolSize symbol == 4) symbols)
      (fail ("aggregate timer veneer missing: " ++ U.entryName entry))
    let body = U.entrySymbol entry ++ "_body"
    unless (any ((== body) . A.symbolName) symbols) (fail ("aggregate timer body missing: " ++ body))
  unless (length (filter ((== U.Timer) . U.entryGroup) (U.mapEntries domain)) == 4)
    (fail "aggregate requires four timer ABI entries")
  let payload = A.imageBytes image
      payloadBase = A.imageBase image
  unless (payloadBase >= loaderBase &&
      toInteger payloadBase + toInteger (B.length payload) <= toInteger loaderBase + toInteger (B.length loaderBytes))
    (fail "effective aggregate carrier slice is outside its loader")
  let offset = fromIntegral (payloadBase - loaderBase)
      installed = B.take offset loaderBytes <> payload <> B.drop (offset + B.length payload) loaderBytes
  pure (Aggregate (A.AggregateImage loaderBase installed) symbols placements)

aggregateRegions :: String -> Aggregate -> [CaseRegion]
aggregateRegions label (Aggregate image _ placements) =
  [ mkRegion (RegionId (label ++ "-" ++ show index)) (GuestAddress (fromIntegral low))
      (fromIntegral (high - low)) (permission low)
      (either error id (A.sliceAggregate image low (high - low)))
  | (index, (low, high)) <- zip [0 :: Int ..] (zip boundaries (drop 1 boundaries))
  ]
  where
    readonly = mergeSpans [(A.placementAddress placement, A.placementAddress placement + A.placementBytes placement)
      | placement <- placements, not (executable placement)]
    base = A.imageBase image
    top = base + fromIntegral (B.length (A.imageBytes image))
    boundaries = sort (nub (base : top : concatMap (\(low, high) -> [low, high]) readonly))
    permission address
      | any (\(low, high) -> address >= low && address < high) readonly = ReadOnly
      | otherwise = ReadExecute

aggregateControlRegions :: String -> [(Word32, Word32)] -> Aggregate -> [CaseRegion]
aggregateControlRegions label patches (Aggregate image symbols placements) =
  aggregateRegions label (Aggregate (A.AggregateImage base patched) symbols placements)
  where
    base = A.imageBase image
    patched = foldl patch (A.imageBytes image) patches
    patch bytes (address, value)
      | not (contains base (fromIntegral (B.length bytes)) address 4) =
          error "aggregate control patch outside installed image"
      | otherwise =
          let offset = fromIntegral (address - base)
           in B.take offset bytes <> word32le value <> B.drop (offset + 4) bytes

aggregateFunctions :: Aggregate -> [(Word32, Word32)]
aggregateFunctions (Aggregate _ symbols placements) = compactFunctions roots spans
  where
    roots = [A.symbolAddress symbol | symbol <- symbols, "xm1_platform_veneer_" `isPrefixOf` A.symbolName symbol]
    spans = [(A.symbolAddress symbol, A.symbolAddress symbol + A.symbolSize symbol) | symbol <- symbols] ++
      [(A.placementAddress placement, A.placementAddress placement + A.placementBytes placement)
      | placement <- placements, executable placement]

compactFunctions :: [Word32] -> [(Word32, Word32)] -> [(Word32, Word32)]
compactFunctions roots spans =
  [ (start, end - start)
  | (low, high) <- mergeSpans spans
  , let boundaries = low : sort (nub [root | root <- roots, root > low, root < high]) ++ [high]
  , (start, end) <- zip boundaries (drop 1 boundaries)
  ]

mergeSpans :: [(Word32, Word32)] -> [(Word32, Word32)]
mergeSpans = reverse . foldl' append [] . sortOn fst
  where
    append ((low, high) : rest) (nextLow, nextHigh)
      | nextLow <= high = (low, max high nextHigh) : rest
    append spans span' = span' : spans

admissionControls :: [(String, Bool)]
admissionControls =
  [ ("fixed-veneer", admitted veneer)
  , ("wrong-veneer-name", not (admitted (veneer {A.symbolName = "xm1_platform_veneer_wrong"})))
  , ("wrong-veneer-address", not (admitted (veneer {A.symbolAddress = 0x1004})))
  , ("wrong-veneer-size", not (admitted (veneer {A.symbolSize = 8})))
  , ("packed-code", admitted (A.AggregateSymbol "body" 0x1004 8))
  , ("oversized-packed-code", not (admitted (A.AggregateSymbol "body" 0x1004 12)))
  , ("out-of-carrier-code", not (admitted (A.AggregateSymbol "body" 0x2000 4)))
  , ("readonly-data", placementAdmitted domain readonly)
  , ("readonly-not-executable", not (executable readonly))
  , ("readonly-not-function", not (symbolAdmitted domain [readonly] (A.AggregateSymbol "body" 0x100c 4)))
  , ("out-of-carrier-data", not (placementAdmitted domain (readonly {A.placementAddress = 0x2000})))
  , ("oversized-data", not (placementAdmitted domain (readonly {A.placementBytes = 8})))
  , ("foreign-fixture-disjoint", disjoint 0x1010 4 carrier)
  , ("foreign-fixture-overlap", not (disjoint 0x1008 4 carrier))
  , ("foreign-fixture-crossing", not (disjoint 0xffc 8 carrier))
  , ("fetch-union-compaction", compactFunctions [0x1000, 0x1008]
      [(0x1000, 0x1004), (0x1004, 0x100c), (0x1004, 0x1008)] == [(0x1000, 8), (0x1008, 4)])
  , ("fetch-union-preserves-gap", compactFunctions []
      [(0x1000, 0x1004), (0x1008, 0x100c)] == [(0x1000, 4), (0x1008, 4)])
  ]
  where
    domain = U.DomainMap R.DomainA (R.GuestAddress 0x1000) 16
      [U.Entry U.Timer "timer" (R.GuestAddress 0x1000) "body" ".text.body" "xm1_platform_veneer_timer"]
      [U.Carrier U.Timer (R.Extent (R.GuestAddress 0x1000) (R.GuestAddress 0x1010))] [] []
    veneer = A.AggregateSymbol "xm1_platform_veneer_timer" 0x1000 4
    packed = A.SectionPlacement 0x1004 8 ".text.body"
    readonly = A.SectionPlacement 0x100c 4 ".rodata.pool"
    admitted = symbolAdmitted domain [packed]
    carrier = R.Extent (R.GuestAddress 0x1000) (R.GuestAddress 0x1010)
