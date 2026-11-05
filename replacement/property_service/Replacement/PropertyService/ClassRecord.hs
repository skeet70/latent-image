{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The JSON record of a classification, shared by the table generator and the stage contract.
module Replacement.PropertyService.ClassRecord
  ( hex
  , reasonLabel
  , classValue
  , ownedLabel
  , stockLabel
  , summaryValue
  ) where

import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson as J
import Numeric (showHex)
import PropertyFields (PropertyId (..))
import Replacement.PropertyService.Classify

hex :: (Integral a, Show a) => a -> String
hex value = "0x" ++ showHex value ""

reasonLabel :: DelegationReason -> String
reasonLabel reason = case reason of
  ExplorationIncomplete explored -> "exploration-incomplete:" ++ exploredLabel explored
  OneSidedFallback side -> "one-sided-fallback:" ++ sideLabel side
  ExitAdjusts -> "exit-adjusts"
  Shape side fault -> "shape:" ++ sideLabel side ++ ":" ++ faultLabel fault
  where
    exploredLabel explored = case explored of
      GetterArms -> "getter"
      SetterArms -> "setter"
      SetterExits -> "setter-exit"
    sideLabel side = case side of
      Getter -> "getter"
      Setter -> "setter"
    faultLabel fault = case fault of
      UnmodelledGuard -> "unmodelled-guard"
      UnexpectedCall -> "call"
      UnexpectedStore -> "store"
      UnexpectedEnd -> "end"
      ModeNotCovered -> "mode-not-covered"
      FieldDisagrees -> "field-disagrees"
      SignedField -> "signed-field"
      DefaultFormUnrecognized -> "default-form"
      DefaultsDiffer -> "defaults-differ"
      DefaultOutOfRange -> "default-out-of-range"
      FieldOutOfRange -> "field-out-of-range"
      RecordOutOfRange -> "record-out-of-range"
      RecordDefaultDisagrees -> "record-default-disagrees"

classValue :: Property -> Value
classValue (Property (PropertyId identifier) (GuestAddress getter) (GuestAddress setter) verdict) = object
  [ "property" .= hex identifier, "getterArm" .= hex getter, "setterArm" .= hex setter
  , "class" .= case verdict of
      Owned owned -> ownedLabel owned
      Delegated _ -> "delegated"
  , "reason" .= case verdict of
      Delegated reason -> Just (reasonLabel reason)
      _ -> Nothing
  , "field" .= case verdict of
      Owned (OwnedField arm) -> Just (fieldValue arm)
      Owned (OwnedReadOnlyField arm) -> Just (fieldValue arm)
      Owned (OwnedSlotField arm) -> Just (slotValue arm)
      Owned (OwnedWriteOnlyPair arm) -> Just (pairValue arm)
      _ -> Nothing
  ]
  where
    fieldValue (FieldArm (BankOffset offset) width (BitRange lsb bits) source save hook) = object
      ([ "offset" .= offset, "width" .= widthLabel width
       , "lsb" .= lsb, "bits" .= bits
       , "default" .= case source of
           FieldDefaultOnly fallback -> defaultValue fallback
           FieldStock stock _ -> object ["stock" .= stockLabel stock]
           FieldConfig (ConfigOffset config) _ -> object ["config" .= hex (configBase + fromIntegral config)]
       , "save" .= (save == SaveBank0Previous)
       ]
        ++ [ "provisional" .= hex provisional | Just provisional <- [provisionalOf source] ]
        ++ [ "hook" .= object ["gate" .= hex (configBase + fromIntegral gate)] | ConfigGatedResave (ConfigOffset gate) <- [hook] ]
        ++ [ "hook" .= ("bank0-bracket" :: String) | hook == Bank0Bracket ])
    pairValue (PairArm (BankOffset first) (BankOffset second) value save) = object
      ["first" .= first, "second" .= second, "width" .= widthLabel Byte, "default" .= hex value, "save" .= (save == SaveBank0Previous), "hook" .= ("bank0-bracket" :: String)]
    provisionalOf source = case source of
      FieldStock _ p -> Just p
      FieldConfig _ p -> Just p
      FieldDefaultOnly _ -> Nothing
    defaultValue fallback = case fallback of
      DefaultConstant value -> J.toJSON (hex value)
      DefaultTail (TailOffset tail') tailWidth -> object ["tail" .= hex tail', "width" .= widthLabel tailWidth]
    slotValue (SlotArm (BankBranch (BankOffset offset) width (BitRange lsb bits) fallback) (RecordBranch (RecordOffset recordOffset') recordWidth' (BitRange recordLsb recordBits')) save slotDefault' mirror) = object
      ([ "bank" .= object
           [ "offset" .= offset, "width" .= widthLabel width, "lsb" .= lsb, "bits" .= bits
           , "default" .= case fallback of
               BankConstant value -> object ["constant" .= hex value]
               BankTailByte (TailOffset tail') -> object ["tailByte" .= hex tail']
           ]
       , "record" .= object ["offset" .= recordOffset', "width" .= widthLabel recordWidth', "lsb" .= recordLsb, "bits" .= recordBits']
       , "save" .= (save == SaveBank0Previous)
       ]
        ++ [ "stock" .= stockLabel stock | SlotStock stock <- [slotDefault'] ]
        ++ [ "mirror" .= ("record0" :: String) | mirror == MirrorRecord0 ])
    widthLabel width = case width of
      Byte -> "byte" :: String
      Half -> "halfword"

ownedLabel :: Owned -> String
ownedLabel owned = case owned of
  OwnedField _ -> "owned-field"
  OwnedSlotField _ -> "owned-slot-field"
  OwnedReadOnlyField _ -> "owned-read-only-field"
  OwnedWriteOnlyPair _ -> "owned-write-only-pair"
  OwnedUnassigned -> "owned-unassigned"

-- | The leaf's address, with 0x61d65c's selector.
stockLabel :: StockDefault -> String
stockLabel stock = case stock of
  Stock61df44 -> "0x61df44"
  Stock61d8a4 -> "0x61d8a4"
  Stock61d59c -> "0x61d59c"
  Stock61d618 -> "0x61d618"
  Stock61d5f0 -> "0x61d5f0"
  Stock61d65c selector -> "0x61d65c(" ++ show (2 + fromEnum selector) ++ ")"

summaryValue :: [Property] -> Value
summaryValue properties = object
  [ "properties" .= length properties
  , "ownedField" .= count (\c -> case c of Owned (OwnedField _) -> True; _ -> False)
  , "ownedSlotField" .= count (\c -> case c of Owned (OwnedSlotField _) -> True; _ -> False)
  , "ownedReadOnlyField" .= count (\c -> case c of Owned (OwnedReadOnlyField _) -> True; _ -> False)
  , "ownedWriteOnlyPair" .= count (\c -> case c of Owned (OwnedWriteOnlyPair _) -> True; _ -> False)
  , "ownedUnassigned" .= count (== Owned OwnedUnassigned)
  , "delegated" .= count (\c -> case c of Delegated _ -> True; _ -> False)
  ]
  where
    count predicate = length (filter (predicate . propertyClass) properties)
