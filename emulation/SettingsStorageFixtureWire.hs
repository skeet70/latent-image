{-# LANGUAGE OverloadedStrings #-}
module SettingsStorageFixtureWire
  ( BankImage(..), TraceMutation, matrixHit, lifecycleHit, lifecycleAddress, matrixStack
  , wireBytes, wireMemory, wireR8, wireFlags, alterFirstSite
  ) where

import Data.Aeson (Value(..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.PropertyPairPlan (Role(..), SiteLabel(..), siteLabelWire)
import qualified Emulation.SettingsStorageMatrix as Storage
import Numeric (showHex)
import PropertyPairFixture (hex, taskHit, wireField)

newtype BankImage = BankImage B.ByteString deriving Eq

type TraceMutation = [Value] -> [Value]

wireBytes :: B.ByteString -> T.Text
wireBytes = T.pack . concatMap (\byte -> let digits=showHex byte "" in replicate (2-length digits) '0' ++ digits) . B.unpack

wireMemory :: [B.ByteString] -> Value -> Value
wireMemory snapshots (Object fields) = Object (KM.insert "memory" (A.toJSON (map wireBytes snapshots)) fields)
wireMemory _ _ = error "memory trace fixture is not an object"

wireR8 :: Word32 -> Value -> Value
wireR8 value = wireField ["registers","r8"] (A.toJSON (hex value))

wireFlags :: Word32 -> Value -> Value
wireFlags value = wireField ["cpsr"] (A.toJSON value) . wireField ["task","cpsr"] (A.toJSON value)

matrixStack :: Role -> Word32
matrixStack role = if role == RetainedRole then 0x1fe0 else 0x3ff8

matrixHit :: Role -> SiteLabel -> Word32 -> [Word32] -> Word32 -> Word32 -> Word32 -> [B.ByteString] -> Value
matrixHit role label address arguments r8 sp lr snapshots = withMemory (wireField ["registers"] registers (taskHit role label address arguments sp lr))
  where
    values = take 4 (arguments ++ repeat 0) ++ replicate 4 0 ++ [r8] ++ replicate 4 0
    registers = object [K.fromString ("r" ++ show index) .= hex value | (index,value) <- zip [0::Int ..] values]
    withMemory = if null snapshots then id else wireMemory snapshots

lifecycleAddress :: Storage.LifecycleSite -> Word32
lifecycleAddress site = case [address | (address,label) <- Storage.lifecycleSites, label == site] of
  [address] -> address
  _ -> error "lifecycle fixture site has no unique fixed address"

lifecycleHit :: Role -> Storage.LifecycleSite -> [Word32] -> Word32 -> [B.ByteString] -> Value
lifecycleHit role site arguments r8 = matrixHit role (StorageSite site) (lifecycleAddress site) arguments r8 (matrixStack role) 0

alterFirstSite :: SiteLabel -> (Value -> Value) -> TraceMutation
alterFirstSite label change = walk
  where
    wanted = String (T.pack (siteLabelWire label))
    walk [] = error "trace mutation site is absent"
    walk (value@(Object fields):rest)
      | KM.lookup "site" fields == Just wanted = change value:rest
    walk (value:rest) = value:walk rest
