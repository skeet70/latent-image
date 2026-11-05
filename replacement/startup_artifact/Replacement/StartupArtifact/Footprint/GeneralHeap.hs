{-# LANGUAGE OverloadedStrings #-}
-- | The general heap's carriers, the bytes outside them its qualification
-- assumes, and the reference scans its adapter checks and joins run. Own
-- carriers are subtracted when the piece is derived.
module Replacement.StartupArtifact.Footprint.GeneralHeap
  ( carriers, image4Carriers, image5Carrier, retiredBins, sentinel, errnoWord, graphicsTable, graphicsTargets
  , retainedCallees, windows, closureExtents, closureCalls, footprint
  , Reference (..), references, referencesNaming, referenceSite
  , inCarriers, graphicsTableForwards, entrySurface, ownedSurface, retiredStateUnreferenced, ownedNamesNoRetired
  , image5CarrierUnnamed
  ) where

import qualified A32 as A
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import qualified Data.Set as S
import Data.Text (Text)
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.GeneralHeap.Admission as H
import qualified Replacement.LoaderOrchestration.Artifact as L
import qualified Replacement.LoaderOrchestration.Interface as LI
import qualified Replacement.StartupArtifact.Footprint.Producers as Producers
import Replacement.StartupArtifact.Footprint.TaskSubmission (closure)
import Replacement.StartupArtifact.Types

-- | The five carriers of the heap admission (replacement/general_heap/Replacement/GeneralHeap/Admission.hs:50).
carriers :: [Span]
carriers = [ Span (target image) (Guest low) (Guest high) | H.Carrier image low high <- H.carriers ]
  where target image = case image of H.Image4 -> Image4; H.Image5 -> Image5

image4Carriers :: [Span]
image4Carriers = filter ((== Image4) . spanTarget) carriers

image5Carrier :: Span
image5Carrier = Span Image5 (Guest 0xbf1c1c) (Guest 0xbf1cd4)

-- | The carrier that replaces stock malloc_av_ bins, sbrk_base and the trim threshold
-- (reports/stock-heap-survey.md; private-data/analysis/x-m1-1.50/lz/id4.dis:629325-629330).
retiredBins :: Span
retiredBins = Span Image4 (Guest 0x89d4fc) (Guest 0x89d908)

-- | The owned sentinel word, canonical value 0 (replacement/general_heap/general-heap.ld:17).
sentinel :: Word32
sentinel = 0x89d4f8

-- | The reentrancy context the public entries pass: *0x89c970 = 0x89c978 in canonical image 4.
errnoWord :: Word32
errnoWord = 0x89c978

-- | The graphics callback table and its retained targets
-- (replacement/general_heap/Replacement/GeneralHeap/Artifact.hs:157).
graphicsTable :: Span
graphicsTable = Span Image4 (Guest 0x890f08) (Guest 0x890f18)

graphicsTargets :: [Word32]
graphicsTargets = [0x890ed8, 0x890ef8, 0x890ecc, 0x890ee8]

retainedCallees :: [Word32]
retainedCallees = H.retainedCallees

image4Base :: Word32
image4Base = 0x61c400

-- | The heap's image-4 carriers and the J6 windows (Footprint/Producers.hs).
windows :: [L.CodeWindow]
windows = Producers.windows ++ [L.CodeWindow (LI.CodeAddress low) (LI.CodeAddress high) | Span _ (Guest low) (Guest high) <- image4Carriers]

-- | Direct-BL closure of lock, unlock and memmove over canonical image 4.
closureExtents :: B.ByteString -> [(Word32, Word32)]
closureExtents image4 =
  [ (low, high) | L.CalleeExtent (LI.CodeAddress low) (LI.CodeAddress high) <-
      L.closedCalleeExtents (LI.CodeAddress image4Base) image4 windows S.empty (map LI.CodeAddress retainedCallees) ]

closureCalls :: B.ByteString -> [(Word32, Word32)]
closureCalls image4 =
  [ (site, target) | (low, high) <- closureExtents image4, site <- [low, low + 4 .. high - 4]
  , A.Branch _ True target <- [A.decode site (U.word32At image4 (fromIntegral (site - image4Base)))] ]

entry :: Target -> Word32 -> Word32 -> Text -> Maybe Footprint
entry target start end citation = (\range -> Footprint range RuntimePremise (Citation citation)) <$> span' target start end

-- | Canonical loader A, loader B and decoded image 4.
footprint :: B.ByteString -> B.ByteString -> B.ByteString -> [Footprint]
footprint loaderA loaderB image4 =
  mapMaybe (\(start, end) -> entry Image4 start end services) (closureExtents image4)
  ++ mapMaybe (\(_, target, start, end, citation) -> entry target start end citation)
       ( closure LoaderA 0x40000 loaderA [] (loaderRoots 0x40000 loaderA) loaderCitation
      ++ closure LoaderB 0xd0000 loaderB [] (loaderRoots 0xd0000 loaderB) loaderCitation )
  ++ mapMaybe id
       [ entry Image4 0x890ecc 0x890f18 "replacement/general_heap/Replacement/GeneralHeap/Artifact.hs:156-157 (graphics callback table [0x890f08,0x890f18) and its retained targets 0x890ecc, 0x890ed8, 0x890ee8, 0x890ef8 forward to the owned public entries)"
       , entry Image4 0x69cdf8 0x69cf84 "reports/replacement-inventory-baseline.md:848 (the phase dispatcher clears [0x89ec00,0xbf1a00), which holds the arena, only in phase 0 and only in mode 1, before any task runs)" ]
  where
    services = "replacement/general_heap/Replacement/GeneralHeap/Admission.hs:57 (retained lock 0x72b164, unlock 0x72b1cc, memmove 0x89d908, the only retained calls the frame proof admits), closed over direct calls with the heap, producer, idle, root and dispatcher carriers excluded"
    loaderCitation = "direct calls from the heap's retained callees' image-4 closure into the loader, closed over direct calls"
    loaderRoots base loader = S.toList (S.fromList [target | (_, target) <- closureCalls image4, target >= base, target < base + fromIntegral (B.length loader)])

-- | A word of code or data that names an address: a branch target, a literal word or a
-- @movw@/@movt@ pair, with the site that holds it.
data Reference = BranchReference Word32 Word32 | LiteralReference Word32 Word32 | FormedReference Word32 Word32
  deriving (Eq, Ord, Show)

-- | Every reference in a flat image at the base, every aligned word: @blx@ and unconditional @b@/@bl@
-- targets, conditional @b@/@bl@ targets inside the same image, the word itself, and a @movt rd@ with the
-- last @movw rd@ in the 16 words before it. Code reaches another image only by unconditional calls.
references :: Word32 -> B.ByteString -> [Reference]
references base bytes = go M.empty 0
  where
    count = B.length bytes `div` 4
    limit = base + fromIntegral (B.length bytes)
    go held index
      | index >= count = []
      | otherwise =
          let site = base + fromIntegral (4 * index)
              word = U.word32At bytes (4 * index)
              condition = word `shiftR` 28
              branch
                | condition == 0xf && word .&. 0xfe000000 == 0xfa000000 = [BranchReference site (site + 8 + offset word + ((word `shiftR` 23) .&. 2))]
                | condition /= 0xf && (word `shiftR` 25) .&. 7 == 5
                , let target = site + 8 + offset word
                , condition == 0xe || (target >= base && target < limit) = [BranchReference site target]
                | otherwise = []
              register = (word `shiftR` 12) .&. 0xf
              half = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)
              isMovw = condition /= 0xf && word .&. 0x0ff00000 == 0x03000000
              isMovt = condition /= 0xf && word .&. 0x0ff00000 == 0x03400000
              formed = [ FormedReference site ((half `shiftL16`) .|. low) | isMovt, Just (at, low) <- [M.lookup register held], index - at <= 16 ]
              held' | isMovw = M.insert register (index, half) held
                    | otherwise = held
           in branch ++ [LiteralReference site word] ++ formed ++ go held' (index + 1)
    offset word = let imm = word .&. 0xffffff in (if imm .&. 0x800000 /= 0 then imm .|. 0xff000000 else imm) * 4
    shiftL16 value = value * 0x10000

-- | The references whose named address the predicate selects.
referencesNaming :: (Word32 -> Bool) -> [Reference] -> [Reference]
referencesNaming selected = filter (selected . named)

referenceSite :: Reference -> Word32
referenceSite reference = case reference of
  BranchReference site _ -> site
  LiteralReference site _ -> site
  FormedReference site _ -> site

named :: Reference -> Word32
named reference = case reference of
  BranchReference _ target -> target
  LiteralReference _ value -> value
  FormedReference _ value -> value

inSpans :: [Span] -> Word32 -> Bool
inSpans spans address = any (\(Span _ (Guest low) (Guest high)) -> low <= address && address < high) spans

inCarriers :: Word32 -> Bool
inCarriers = inSpans carriers

wordAt :: B.ByteString -> Word32 -> Word32 -> Maybe Word32
wordAt bytes base address
  | address >= base && fromIntegral (address - base) + 4 <= B.length bytes = Just (U.word32At bytes (fromIntegral (address - base)))
  | otherwise = Nothing

-- | Image-4 table words hold the retained adapters, and each adapter's BL names the heap public entry
-- it forwards to (private-data/analysis/x-m1-1.50/lz/id4.dis:616907-616918).
graphicsTableForwards :: B.ByteString -> Bool
graphicsTableForwards image4 =
  map (wordAt image4 image4Base) [0x890f08, 0x890f0c, 0x890f10, 0x890f14] == map Just graphicsTargets
    && and [ fmap (A.decode site) (wordAt image4 image4Base site) == Just (A.Branch (A.Cond 14) True target)
           | (site, target) <- [(0x890ee0, 0x89da3c), (0x890ef0, 0x89cdc4), (0x890f00, 0x89cda4)] ]

-- | Data words that encode a branch into a heap carrier, with the word: image 5 0x109ba48 holds
-- @blx 0x89d26c@ between undecodable words, inside no decomp function
-- (private-data/analysis/x-m1-1.50/lz/id5.dis:1183641-1183643; xm1-decomp lookup 0x109ba48).
dataWords :: [(Word32, Word32)]
dataWords = [(0x109ba48, 0xfae00607)]

-- | Outside the heap carriers, the references naming an address inside one that is not an entry,
-- except a 'dataWords' branch still holding its word.
entrySurface :: [Word32] -> [(Word32, B.ByteString)] -> [Reference]
entrySurface entries segments =
  [ reference | (base, bytes) <- segments, reference <- referencesNaming inCarriers (references base bytes)
  , not (inCarriers (referenceSite reference)), named reference `notElem` entries, not (dataWord base bytes reference) ]
  where
    dataWord base bytes reference = case reference of
      BranchReference site _ -> any (\(address, word) -> address == site && wordAt bytes base site == Just word) dataWords
      _ -> False

-- | Inside the staged heap carriers, every reference naming an address inside a carrier names an
-- admitted owned instruction, an entry, the metadata words or the sentinel.
ownedSurface :: [Word32] -> [Word32] -> (Word32, Word32) -> B.ByteString -> Bool
ownedSurface instructions entries (metadataStart, metadataEnd) image4 =
  all (\reference -> admittedTarget (named reference)) (referencesNaming inCarriers (ownedReferences image4))
  where
    pcs = S.fromList (instructions ++ entries)
    admittedTarget address = S.member address pcs || (metadataStart <= address && address < metadataEnd) || address == sentinel

-- | The references of the staged heap's image-4 carriers.
ownedReferences :: B.ByteString -> [Reference]
ownedReferences image4 =
  [ reference | Span _ (Guest low) (Guest high) <- image4Carriers
  , reference <- references low (B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - image4Base)) image4)) ]

-- | Retired stock heap state: the bins and break base, the bookkeeping block and the break cell
-- (E-STOCK-HEAP-SURVEY-150).
retiredState :: Word32 -> Bool
retiredState address = inSpans [retiredBins, Span Image4 (Guest sentinel) (Guest (sentinel + 4))] address
  || (address >= 0x9adda4 && address < 0x9addd8) || address == 0x829a08

-- | The backing helper that advances the break cell.
backingHelper :: (Word32, Word32)
backingHelper = (0x72b110, 0x72b164)

-- | Outside the heap carriers no reference names retired state, except the backing helper's own
-- formations of the break cell, and every branch to the backing helper lies inside a heap carrier.
retiredStateUnreferenced :: [(Word32, B.ByteString)] -> Bool
retiredStateUnreferenced segments =
  null [ () | (base, bytes) <- segments, reference <- referencesNaming retiredState (references base bytes)
       , not (inCarriers (referenceSite reference)), not (helperFormation reference) ]
    && null [ () | (base, bytes) <- segments, BranchReference site target <- references base bytes
            , target == fst backingHelper, not (inCarriers site) ]
  where
    helperFormation reference = case reference of
      FormedReference site 0x829a08 -> fst backingHelper <= site && site < snd backingHelper
      _ -> False

-- | The staged heap code names neither the bookkeeping block, the break cell nor the backing helper.
ownedNamesNoRetired :: B.ByteString -> Bool
ownedNamesNoRetired image4 = null (referencesNaming stockOnly (ownedReferences image4))
  where stockOnly address = (address >= 0x9adda4 && address < 0x9addd8) || address == 0x829a08 || address == fst backingHelper

-- | No reference in the given images names the image-5 heap carrier. The caller passes the images
-- that run before the image-5 producer publishes record 5: records 1 and 2, loaders A and B and
-- image 4, the heap's own image-4 code included.
image5CarrierUnnamed :: [(Word32, B.ByteString)] -> Bool
image5CarrierUnnamed segments = null [ () | (base, bytes) <- segments, _ <- referencesNaming (inSpans [image5Carrier]) (references base bytes) ]
