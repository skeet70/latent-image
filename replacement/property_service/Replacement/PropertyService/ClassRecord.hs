{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The JSON record of a classification, shared by the table generator and the stage contract.
module Replacement.PropertyService.ClassRecord
  ( hex
  , reasonLabel
  , classValue
  , summaryValue
  ) where

import Data.Aeson (Value, object, (.=))
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
      Owned (OwnedField _) -> "owned-field" :: String
      Owned (OwnedSlotField _) -> "owned-slot-field"
      Owned OwnedUnassigned -> "owned-unassigned"
      Delegated _ -> "delegated"
  , "reason" .= case verdict of
      Delegated reason -> Just (reasonLabel reason)
      _ -> Nothing
  , "field" .= case verdict of
      Owned (OwnedField arm) -> Just (fieldValue arm)
      Owned (OwnedSlotField arm) -> Just (slotValue arm)
      _ -> Nothing
  ]
  where
    fieldValue (FieldArm (BankOffset offset) width (BitRange lsb bits) (FieldDefault value) save) = object
      [ "offset" .= offset, "width" .= widthLabel width
      , "lsb" .= lsb, "bits" .= bits, "default" .= hex value
      , "save" .= (save == SaveBank0Previous)
      ]
    slotValue (SlotArm (BankBranch (BankOffset offset) width (BitRange lsb bits) fallback) (RecordBranch (RecordOffset recordOffset') recordWidth' (BitRange recordLsb recordBits')) save) = object
      [ "bank" .= object
          [ "offset" .= offset, "width" .= widthLabel width, "lsb" .= lsb, "bits" .= bits
          , "default" .= case fallback of
              BankConstant (FieldDefault value) -> object ["constant" .= hex value]
              BankTailByte (TailOffset tail') -> object ["tailByte" .= hex tail']
          ]
      , "record" .= object ["offset" .= recordOffset', "width" .= widthLabel recordWidth', "lsb" .= recordLsb, "bits" .= recordBits']
      , "save" .= (save == SaveBank0Previous)
      ]
    widthLabel width = case width of
      Byte -> "byte" :: String
      Half -> "halfword"

summaryValue :: [Property] -> Value
summaryValue properties = object
  [ "properties" .= length properties
  , "ownedField" .= count (\c -> case c of Owned (OwnedField _) -> True; _ -> False)
  , "ownedSlotField" .= count (\c -> case c of Owned (OwnedSlotField _) -> True; _ -> False)
  , "ownedUnassigned" .= count (== Owned OwnedUnassigned)
  , "delegated" .= count (\c -> case c of Delegated _ -> True; _ -> False)
  ]
  where
    count predicate = length (filter (predicate . propertyClass) properties)
