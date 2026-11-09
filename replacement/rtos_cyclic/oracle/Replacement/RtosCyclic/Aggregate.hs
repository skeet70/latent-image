-- | Storage bridges unallocated gaps with canonical loader bytes and splits at readonly data.
-- Fetch declarations cover only allocated code, with every public ABI entry a function start.
module Replacement.RtosCyclic.Aggregate (bindAggregate, admissionControls) where

import Control.Monad (forM, forM_, unless)
import qualified Data.ByteString as B
import Data.List (isPrefixOf, sortOn)
import Data.Word (Word32)
import qualified Replacement.RtosPlatform.Aggregate as A
import qualified Replacement.RtosPlatform.Union as U
import qualified Replacement.RtosOwnFirst.Regions as R
import Replacement.RtosCyclic.Suite (Image (..), ControlRun (..), foreignCodeExtents)

type Slice = (Image, Bool, [(Word32, Word32)])

bindAggregate :: U.DomainMap -> Image -> B.ByteString -> [A.AggregateSymbol] -> [A.SectionPlacement] -> Either String [Slice]
bindAggregate domain stock bytes symbols placements = do
  unless (not (null placements)) (Left "aggregate placements are empty")
  forM_ foreignCodeExtents $ \(address, size) ->
    unless (toInteger address + toInteger size <= toInteger (imageBase stock) ||
            toInteger address >= toInteger (imageBase stock) + toInteger (B.length (imageBytes stock)))
      (Left "aggregate storage mapping overlaps foreign fixture code")
  forM_ (U.mapCarriers domain) $ \carrier ->
    forM_ foreignCodeExtents $ \(address, size) -> do
      let R.Extent (R.GuestAddress low) (R.GuestAddress high) = U.carrierExtent carrier
      unless (toInteger address + toInteger size <= toInteger low || address >= high)
        (Left "foreign fixture code overlaps an owned union carrier")
  image <- A.aggregateImage symbols bytes
  let ordered = sortOn A.placementAddress placements
  forM_ (zip ordered (drop 1 ordered)) $ \(left, right) ->
    unless (toInteger (A.placementAddress left) + toInteger (A.placementBytes left) <= toInteger (A.placementAddress right))
      (Left "aggregate placements overlap")
  forM_ placements $ \p -> do
    unless (A.placementBytes p > 0 && sectionKind p /= Nothing && any (contains (A.placementAddress p) (A.placementBytes p)) (U.runs domain))
      (Left ("aggregate section is outside packed carriers or has an unsupported kind: " ++ A.placementSection p))
  forM_ symbols $ \s ->
    unless (A.symbolSize s > 0 && allocated s) (Left ("aggregate function has no executable placement or exact fixed ABI entry: " ++ A.symbolName s))
  forM_ (U.mapEntries domain) $ \entry -> do
    unless (any (fixed entry) symbols) (Left ("aggregate is missing fixed ABI entry: " ++ U.entryVeneer entry))
    unless (U.entryGroup entry `notElem` [U.Cyclic, U.Timer] || any (\s -> A.symbolName s == U.entrySymbol entry) symbols || any (\p -> A.placementSection p `elem` [".text." ++ U.entrySymbol entry, ".text." ++ U.entrySymbol entry ++ "_entry"]) placements)
      (Left ("aggregate is missing declared body: " ++ U.entrySymbol entry))
  let payload = A.imageBytes image
      payloadBase = A.imageBase image
  unless (payloadBase >= imageBase stock &&
      toInteger payloadBase + toInteger (B.length payload) <= toInteger (imageBase stock) + toInteger (B.length (imageBytes stock)))
    (Left "effective aggregate carrier slice is outside its loader")
  let offset = fromIntegral (payloadBase - imageBase stock)
      installed = B.take offset (imageBytes stock) <> payload <> B.drop (offset + B.length payload) (imageBytes stock)
      loaderTop = imageBase stock + fromIntegral (B.length installed)
      readonlySpans = merge readonly
      mappings = gaps (imageBase stock) loaderTop readonlySpans
      functions = concatMap splitPublic (merge executable)
  forM mappings $ \(address, size, isCode) -> do
    let payload = B.take (fromIntegral size) (B.drop (fromIntegral (address - imageBase stock)) installed)
        fetches = [(at, count) | (at, count, _) <- functions, isCode, inside at count address size]
    pure (Image address payload, isCode, fetches)
  where
    fixed entry s = A.symbolName s == U.entryVeneer entry && R.GuestAddress (A.symbolAddress s) == U.entryAddress entry && A.symbolSize s == 4
    allocated s = any (\p -> sectionKind p == Just True && inside (A.symbolAddress s) (A.symbolSize s) (A.placementAddress p) (A.placementBytes p)) placements || any (`fixed` s) (U.mapEntries domain)
    executable = [(A.placementAddress p, A.placementBytes p, True) | p <- placements, sectionKind p == Just True] ++ [(A.symbolAddress s, A.symbolSize s, True) | s <- symbols]
    readonly = [(A.placementAddress p, A.placementBytes p, False) | p <- placements, sectionKind p == Just False]
    publicStarts = [address | entry <- U.mapEntries domain, let R.GuestAddress address = U.entryAddress entry]
    splitPublic (address, size, _) =
      let cuts = sortOn id (address : [at | at <- publicStarts, at > address, at < address + size] ++ [address + size])
       in [(low, high - low, True) | (low, high) <- zip cuts (drop 1 cuts)]

sectionKind :: A.SectionPlacement -> Maybe Bool
sectionKind p
  | ".text" `isPrefixOf` A.placementSection p = Just True
  | ".rodata" `isPrefixOf` A.placementSection p = Just False
  | otherwise = Nothing

inside :: Word32 -> Word32 -> Word32 -> Word32 -> Bool
inside at size base bytes = at >= base && toInteger at + toInteger size <= toInteger base + toInteger bytes

contains :: Word32 -> Word32 -> R.Extent -> Bool
contains at size (R.Extent (R.GuestAddress low) (R.GuestAddress high)) = inside at size low (high - low)

merge :: [(Word32, Word32, Bool)] -> [(Word32, Word32, Bool)]
merge = foldr join [] . sortOn (\(at, _, _) -> at)
  where
    join x [] = [x]
    join (at, size, code) ((next, bytes, _) : rest)
      | toInteger at + toInteger size >= toInteger next = (at, max (at + size) (next + bytes) - at, code) : rest
      | otherwise = (at, size, code) : (next, bytes, code) : rest

gaps :: Word32 -> Word32 -> [(Word32, Word32, Bool)] -> [(Word32, Word32, Bool)]
gaps low high [] = [(low, high - low, True) | low < high]
gaps low high ((at, size, _) : rest) =
  [(low, at - low, True) | low < at] ++ [(at, size, False)] ++ gaps (at + size) high rest

admissionControls :: [ControlRun]
admissionControls = [ControlRun "aggregate-admission" name "binding" holds | (name, holds) <- checks]
  where
    entry = U.Entry U.Cyclic "configure" (R.GuestAddress 0x1000) "body" "veneer"
    domain = U.DomainMap R.DomainA (R.GuestAddress 0x1000) 0x20 [entry] [U.Carrier U.Cyclic (R.Extent (R.GuestAddress 0x1000) (R.GuestAddress 0x1020))] [] []
    veneer = A.AggregateSymbol "veneer" 0x1000 4
    body = A.AggregateSymbol "body" 0x1004 8
    placements = [A.SectionPlacement 0x1004 8 ".text.body", A.SectionPlacement 0x100c 4 ".rodata.table"]
    tailSymbol = A.AggregateSymbol "tail" 0x1010 4
    symbols = [veneer, body, tailSymbol]
    allPlacements = placements ++ [A.SectionPlacement 0x1010 4 ".text.tail"]
    stock = Image 0x1000 (B.replicate 32 0xa5)
    admitted ss ps = bindAggregate domain stock (B.pack [0 .. 19]) ss ps
    refused ss ps = case admitted ss ps of Left _ -> True; Right _ -> False
    checks =
      [ ("fixed-veneer-packed-code-readonly", case admitted symbols allPlacements of
           Right slices -> any (\(Image at payload, code, spans) -> at == 0x100c && payload == B.pack [12 .. 15] && not code && null spans) slices
             && any (\(Image at payload, code, spans) -> at == 0x1000 && payload == B.pack [0 .. 11] && code && spans == [(0x1000, 12)]) slices
           Left _ -> False)
      , ("executable-placement-without-positive-symbol", case admitted symbols [A.SectionPlacement 0x1004 8 ".text.body", A.SectionPlacement 0x100c 8 ".text.alias"] of
           Right slices -> any (\(Image at payload, code, spans) -> at == 0x1000 && payload == B.pack [0 .. 19] <> B.replicate 12 0xa5 && code && spans == [(0x1000, 20)]) slices
           Left _ -> False)
      , ("wrong-veneer-name", refused [veneer {A.symbolName = "wrong"}, body, tailSymbol] allPlacements)
      , ("wrong-veneer-address", refused [veneer {A.symbolAddress = 0x1001}, body, tailSymbol] allPlacements)
      , ("wrong-veneer-size", refused [veneer {A.symbolSize = 8}, body, tailSymbol] allPlacements)
      , ("function-outside-placement", refused [veneer, body {A.symbolSize = 12}, tailSymbol] allPlacements)
      , ("section-outside-carrier", refused symbols (allPlacements ++ [A.SectionPlacement 0x1020 4 ".rodata.outside"]))
      , ("readonly-not-function", refused (symbols ++ [A.AggregateSymbol "data-as-code" 0x100c 4]) allPlacements)
      , ("zero-size-not-function", refused (symbols ++ [A.AggregateSymbol "label" 0x100c 0]) allPlacements)
      , ("wrong-image-span", case bindAggregate domain stock (B.pack [0 .. 20]) symbols allPlacements of Left _ -> True; Right _ -> False)
      , ("readonly-overlaps-code", refused symbols (allPlacements ++ [A.SectionPlacement 0x1004 4 ".rodata.overlap"]))
      , ("canonical-gap-bytes-preserved", case admitted symbols allPlacements of
           Right slices -> any (\(Image at payload, code, spans) -> at == 0x1010 && payload == B.pack [16 .. 19] <> B.replicate 12 0xa5 && code && spans == [(0x1010, 4)]) slices
           Left _ -> False)
      , ("adjacent-public-entry-remains-function-start",
           let adjacentDomain = domain {U.mapEntries = U.mapEntries domain ++ [U.Entry U.Shared "adjacent" (R.GuestAddress 0x1004) "unused" "body"]}
               adjacentSymbols = [veneer, body {A.symbolSize = 4}, tailSymbol]
               adjacentPlacements = [A.SectionPlacement 0x1008 4 ".text.continuation", A.SectionPlacement 0x100c 4 ".rodata.table", A.SectionPlacement 0x1010 4 ".text.tail"]
            in case bindAggregate adjacentDomain stock (B.pack [0 .. 19]) adjacentSymbols adjacentPlacements of
                 Right slices -> any (\(_, code, spans) -> code && spans == [(0x1000, 4), (0x1004, 8)]) slices
                 Left _ -> False)
      ] ++ [ ("foreign-code-overlaps-unused-carrier", case bindAggregate (domain {U.mapCarriers = U.mapCarriers domain ++ [U.Carrier U.Shared (R.Extent (R.GuestAddress address) (R.GuestAddress (address + size)))]}) stock (B.pack [0 .. 19]) symbols allPlacements of Left _ -> True; Right _ -> False)
        | (address, size) <- foreignCodeExtents
      ]
