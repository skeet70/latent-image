{-# LANGUAGE GHC2021 #-}

-- | Byte-derived closure of retained words whose value lies in retired property code. A closure
-- names the site, the exact word and the role the bound bytes give it; nothing here is an address
-- allowlist, and every derivation yields nothing when a bound byte differs.
module Replacement.PropertyService.RetainedClosure
  ( ClosedRole (..)
  , ClosedSite (..)
  , closesHit
  , FunctionBinding (..)
  , reachableInstructions
  , instructionSites
  , extentMatches
  , imageFor
  ) where

import Data.Bits (shiftR, testBit, (.&.))
import qualified Data.ByteString as B
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (GuestAddress (..), Reference (..), ScanHit (..), ScanImage)
import ServiceInventory (Image (..), imageWord)

data ClosedRole
  = -- | A reachable instruction of a bound function that does not transfer control.
    ExecutedInstruction
  | -- | Bytes of a bound container whose bound consumers read them only as non-address data.
    ConsumedData
  | -- | Bytes no code materializes an address of and no data word names, between bound neighbours
    -- whose bound readers' index bounds stop outside them.
    UnreferencedData
  deriving (Eq, Show)

data ClosedSite = ClosedSite
  { closedImage :: ScanImage
  , closedSite :: GuestAddress
  , closedWord :: Word32
  , closedRole :: ClosedRole
  }
  deriving (Eq, Show)

-- | An executed instruction closes only its numeric match; data closures also close the branch a
-- scan decodes from the word.
closesHit :: ClosedSite -> ScanHit -> Bool
closesHit closed hit =
  closedImage closed == hitImage hit
    && closedSite closed == hitSite hit
    && closedWord closed == hitWord hit
    && case (closedRole closed, hitReference hit) of
      (_, WordInto) -> True
      (ConsumedData, BranchInto _) -> True
      (UnreferencedData, BranchInto _) -> True
      _ -> False

-- | A function's complete words from its entry; the extent is half-open.
data FunctionBinding = FunctionBinding
  { bindingImageBase :: Word32
  , bindingEntry :: GuestAddress
  , bindingWords :: [Word32]
  }

-- | Word addresses reachable from the entry inside the bound extent, or Nothing when the image
-- differs from the binding or a reachable instruction writes PC other than by a direct branch, a
-- `bx lr`, an LDM that loads PC or `ldr pc, [sp], #4`.
reachableInstructions :: Image -> FunctionBinding -> Maybe (S.Set Word32)
reachableInstructions image binding
  | imageBase image /= bindingImageBase binding = Nothing
  | not (and [imageWord image address == Just expected | (address, expected) <- zip [entry, entry + 4 ..] (bindingWords binding)]) = Nothing
  | otherwise = visit S.empty [entry]
  where
    GuestAddress entry = bindingEntry binding
    end = toInteger entry + 4 * toInteger (length (bindingWords binding))
    inside address = address >= entry && toInteger address < end
    visit seen [] = Just seen
    visit seen (address : pending)
      | not (inside address) || S.member address seen = visit seen pending
      | otherwise = do
          word <- imageWord image address
          next <- successors address word
          visit (S.insert address seen) (next ++ pending)

-- | Control successors of one A32 word; Nothing for a PC write this walk does not model.
successors :: Word32 -> Word32 -> Maybe [Word32]
successors address word
  | condition == 15 = if word .&. 0xfe000000 == 0xfa000000 then Just [next] else Nothing
  | word .&. 0x0f000000 == 0x0a000000 = Just (branchDestination : [next | conditional])
  | word .&. 0x0f000000 == 0x0b000000 = Just [next]
  | word .&. 0x0ffffff0 == 0x012fff10 = if word .&. 15 == 14 then Just [next | conditional] else Nothing
  | word .&. 0x0ffffff0 == 0x012fff30 = Just [next]
  | word .&. 0x0e108000 == 0x08108000 = Just [next | conditional]
  | word .&. 0x0fffffff == 0x049df004 = Just [next | conditional]
  | writesPc = Nothing
  | otherwise = Just [next]
  where
    condition = word `shiftR` 28
    conditional = condition /= 14
    next = address + 4
    branchDestination =
      let encoded = toInteger (word .&. 0x00ffffff)
          displacement = if encoded >= 0x800000 then encoded - 0x1000000 else encoded
      in fromInteger (toInteger address + 8 + displacement * 4)
    destination = (word `shiftR` 12) .&. 15
    opcode = (word `shiftR` 21) .&. 15
    compareOnly = word .&. 0x0c000000 == 0 && opcode >= 8 && opcode <= 11 && testBit word 20
    statusWrite = word .&. 0x0fb0f000 == 0x0320f000 || word .&. 0x0fb0fff0 == 0x0120f000
    multiply = word .&. 0x0e0000f0 == 0x00000090
    writesPc =
      destination == 15
        && not compareOnly
        && not statusWrite
        && not multiply
        && (word .&. 0x0c000000 == 0 || word .&. 0x0c100000 == 0x04100000)

-- | The bound sites whose word equals the expected encoding and which the walk reaches.
instructionSites :: ScanImage -> Image -> FunctionBinding -> [(GuestAddress, Word32)] -> [ClosedSite]
instructionSites label image binding sites = case reachableInstructions image binding of
  Nothing -> []
  Just reachable ->
    [ ClosedSite label site word ExecutedInstruction
    | (site@(GuestAddress address), expected) <- sites
    , S.member address reachable
    , Just word <- [imageWord image address]
    , word == expected
    ]

-- | Whether the image holds exactly the bytes with this digest over [start, end).
extentMatches :: Image -> GuestAddress -> GuestAddress -> SHA.Sha256 -> Bool
extentMatches image (GuestAddress start) (GuestAddress end) expected =
  start >= imageBase image
    && end >= start
    && toInteger end <= toInteger (imageBase image) + toInteger (B.length (imageBytes image))
    && SHA.sha256 (B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - imageBase image)) (imageBytes image))) == expected

imageFor :: ScanImage -> [(ScanImage, Image)] -> Maybe Image
imageFor = lookup
