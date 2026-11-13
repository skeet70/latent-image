{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Every reference into the retired extents, from the listing bytes: a branch
-- or computed PC target inside an extent, a data word whose value is an extent
-- address, and a @movw@/@movt@ pair that builds one.
module Replacement.Saclac.References
  ( Reference (..)
  , ReferenceKind (..)
  , referenceKindLabel
  , references
  , referencesValue
  , insideExtents
  ) where

import qualified A32
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Bits ((.&.), (.|.), shiftL)
import Data.Char (isHexDigit)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Numeric (readHex, showHex)
import Replacement.Saclac.Artifact (Extent (..), extentContains, extents)

data ReferenceKind
  = BranchReference
  | DataWordReference
  | MoveWideReference
  | PcArithmeticReference
  deriving (Eq, Show)

referenceKindLabel :: ReferenceKind -> String
referenceKindLabel BranchReference = "branch"
referenceKindLabel DataWordReference = "data-word"
referenceKindLabel MoveWideReference = "movw-movt"
referenceKindLabel PcArithmeticReference = "pc-arithmetic"

data Reference = Reference
  { referenceListing :: String
  , referenceAddress :: Word32
  , referenceTarget :: Word32
  , referenceKind :: ReferenceKind
  } deriving (Eq, Show)

insideExtents :: Word32 -> Bool
insideExtents address = any (`extentContains` address) extents

hexWord32 :: C.ByteString -> Maybe Word32
hexWord32 text
  | C.length text > 8 || not (C.all isHexDigit text) = Nothing
  | otherwise = case readHex (C.unpack text) of
      [(value, "")] -> Just (fromIntegral (value :: Integer))
      _ -> Nothing

-- | The address and word of one listing row.
row :: C.ByteString -> Maybe (Word32, Word32)
row line = do
  let trimmed = C.dropWhile (== ' ') line
  (addressText, rest) <- case C.elemIndex ':' trimmed of
    Nothing -> Nothing
    Just index -> Just (C.take index trimmed, C.drop (index + 1) trimmed)
  address <- hexWord32 addressText
  word <- hexWord32 (C.take 8 (C.dropWhile (== ' ') rest))
  pure (address, word)

-- | Every reference into an extent, in listing order.
references :: String -> B.ByteString -> [Reference]
references name bytes = reverse (snd (foldl' step (M.empty, []) (C.lines bytes)))
  where
    step (pending, found) line = case row line of
      Nothing -> (if label line then M.empty else pending, found)
      Just (address, word) ->
        let instruction = A32.decode address word
            built = case instruction of
              A32.MoveTop _ destination high -> case M.lookup destination pending of
                Just (lowAddress, low) ->
                  let combined = (high `shiftL` 16) .|. (low .&. 0xffff)
                   in [record lowAddress combined MoveWideReference | insideExtents combined]
                Nothing -> []
              _ -> []
            cleared = foldl' (flip M.delete) pending (writtenRegisters instruction)
            pending'
              | transfers instruction = M.empty
              | otherwise = case instruction of
                  A32.MoveWide _ destination low -> M.insert destination (address, low) cleared
                  _ -> cleared
         in (pending', reverse (branch address word ++ dataWord address word ++ built ++ pcArithmetic address word) ++ found)
    record address target kind = Reference name address target kind
    -- A label is an entry another path can take, so no earlier movw reaches
    -- the following word on this path alone.
    label line =
      let trimmed = C.dropWhile (== ' ') line
          token = C.takeWhile (/= ' ') trimmed
      in C.isInfixOf "<" trimmed && hexWord32 token /= Nothing
    -- A call clobbers r0-r3 and r12; lr changes on any linked branch.
    writtenRegisters instruction = case instruction of
      A32.DataProcessing _ operation _ destination _ _
        | operation `elem` [A32.CMP, A32.CMN, A32.TST, A32.TEQ] -> []
        | otherwise -> [destination]
      A32.MoveWide _ destination _ -> [destination]
      A32.MoveTop _ destination _ -> [destination]
      A32.ReadStatus _ destination -> [destination]
      A32.WriteApplicationFlags _ _ -> []
      A32.ZeroExtend _ destination _ _ _ -> [destination]
      A32.Multiply _ destination _ _ _ -> [destination]
      A32.Compute _ registers -> registers
      A32.Load _ _ _ destination _ _ _ -> [destination]
      A32.LoadMultiple _ _ _ _ registers -> registers
      A32.Branch _ True _ -> [A32.Reg 0, A32.Reg 1, A32.Reg 2, A32.Reg 3, A32.Reg 12, A32.lr]
      A32.Branch _ False _ -> []
      A32.BranchExchange _ True _ -> [A32.Reg 0, A32.Reg 1, A32.Reg 2, A32.Reg 3, A32.Reg 12, A32.lr]
      A32.BranchExchange _ False _ -> []
      A32.Store {} -> []
      A32.StoreMultiple {} -> []
      A32.Hint _ -> []
      A32.Rejected _ -> []
    -- Execution can leave the fall-through between a movw and a movt.
    transfers instruction = case instruction of
      A32.Branch {} -> True
      A32.BranchExchange {} -> True
      A32.Load _ _ _ destination _ _ _ -> destination == A32.pc
      A32.LoadMultiple _ _ _ _ registers -> A32.pc `elem` registers
      A32.DataProcessing _ _ _ destination _ _ -> destination == A32.pc
      A32.Compute _ registers -> A32.pc `elem` registers
      A32.Multiply _ destination _ _ _ -> destination == A32.pc
      A32.Rejected _ -> True
      _ -> False
    branch address word = case A32.decode address word of
      A32.Branch _ _ target | insideExtents target -> [record address target BranchReference]
      _ -> []
    dataWord address word
      | insideExtents word = [record address word DataWordReference]
      | otherwise = []
    pcArithmetic address word = case A32.decode address word of
      A32.DataProcessing _ operation _ destination base (A32.Immediate immediate)
        | base == A32.pc
        , operation `elem` [A32.ADD, A32.SUB]
        , destination /= A32.pc ->
            let value = address + 8 + if operation == A32.ADD then immediate else negate immediate
             in [record address value PcArithmeticReference | insideExtents value]
      _ -> []

-- | The scan as a JSON object, with the references whose source address lies
-- outside every extent listed separately.
referencesValue :: [Reference] -> Value
referencesValue found = object
  [ "schema" .= (1 :: Int)
  , "extents" .=
      [object ["low" .= hexAddress (extentLow extent), "high" .= hexAddress (extentHigh extent)]
      | extent <- extents]
  , "references" .= [referenceRow reference | reference <- found]
  , "outside" .= [referenceRow reference | reference <- found, not (insideExtents (referenceAddress reference))]
  , "counts" .= object
      [ "total" .= length found
      , "outside" .= length [() | reference <- found, not (insideExtents (referenceAddress reference))]
      , "by_kind" .= object
          [ key .= length [() | reference <- found, referenceKind reference == kind]
          | (key, kind) <-
              [ ("branch", BranchReference)
              , ("data-word", DataWordReference)
              , ("movw-movt", MoveWideReference)
              , ("pc-arithmetic", PcArithmeticReference)
              ]
          ]
      ]
  ]
  where
    referenceRow reference = object
      [ "listing" .= referenceListing reference
      , "address" .= hexAddress (referenceAddress reference)
      , "target" .= hexAddress (referenceTarget reference)
      , "kind" .= referenceKindLabel (referenceKind reference)
      ]

hexAddress :: Word32 -> String
hexAddress value = "0x" ++ replicate (8 - length digits) '0' ++ digits
  where
    digits = showHex value ""
