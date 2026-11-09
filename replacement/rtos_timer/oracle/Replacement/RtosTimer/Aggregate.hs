{-# LANGUAGE OverloadedStrings #-}

module Replacement.RtosTimer.Aggregate
  ( Aggregate
  , loadAggregate
  , aggregateRegions
  , aggregateFunctions
  , admissionControls
  ) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.List (isPrefixOf)
import Data.Word (Word32)
import Emulation.OwnedCase
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Aggregate as A
import qualified Replacement.RtosPlatform.Union as U

data Aggregate = Aggregate A.AggregateImage [A.AggregateSymbol] [A.SectionPlacement]

contains :: Word32 -> Word32 -> Word32 -> Word32 -> Bool
contains base size address bytes = bytes > 0 && address >= base &&
  toInteger address + toInteger bytes <= toInteger base + toInteger size

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

loadAggregate :: U.DomainMap -> FilePath -> FilePath -> FilePath -> IO Aggregate
loadAggregate domain binPath symbolsPath placementsPath = do
  symbols <- either fail pure . A.parseAggregateSymbols =<< B.readFile symbolsPath
  placements <- either fail pure . A.parsePlacements =<< B.readFile placementsPath
  unless (not (null placements)) (fail "aggregate placements are empty")
  image <- either fail pure . A.aggregateImage symbols =<< B.readFile binPath
  forM_ placements $ \placement -> do
    unless (placementAdmitted domain placement) (fail ("aggregate placement outside declared carriers: " ++ show placement))
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
  pure (Aggregate image symbols placements)

aggregateRegions :: String -> Aggregate -> [CaseRegion]
aggregateRegions label (Aggregate image symbols placements) =
  [ region (label ++ "-placement-" ++ show index) (A.placementAddress placement) (A.placementBytes placement)
      (if executable placement then ReadExecute else ReadOnly)
  | (index, placement) <- zip [0 :: Int ..] placements
  ] ++
  [ region (label ++ "-" ++ A.symbolName symbol) (A.symbolAddress symbol) 4 ReadExecute
  | symbol <- symbols, "xm1_platform_veneer_" `isPrefixOf` A.symbolName symbol
  ]
  where
    region name address bytes permission = mkRegion (RegionId name) (GuestAddress (fromIntegral address)) (fromIntegral bytes) permission
      (either error id (A.sliceAggregate image address bytes))

aggregateFunctions :: Aggregate -> [(Word32, Word32)]
aggregateFunctions (Aggregate _ symbols placements) =
  [(A.symbolAddress symbol, A.symbolSize symbol) | symbol <- symbols] ++
  [(A.placementAddress placement, A.placementBytes placement) | placement <- placements, executable placement]

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
  ]
  where
    domain = U.DomainMap R.DomainA (R.GuestAddress 0x1000) 16
      [U.Entry U.Timer "timer" (R.GuestAddress 0x1000) "body" "xm1_platform_veneer_timer"]
      [U.Carrier U.Timer (R.Extent (R.GuestAddress 0x1000) (R.GuestAddress 0x1010))] [] []
    veneer = A.AggregateSymbol "xm1_platform_veneer_timer" 0x1000 4
    packed = A.SectionPlacement 0x1004 8 ".text.body"
    readonly = A.SectionPlacement 0x100c 4 ".rodata.pool"
    admitted = symbolAdmitted domain [packed]
