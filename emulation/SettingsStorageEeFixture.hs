{-# LANGUAGE OverloadedStrings #-}
module SettingsStorageEeFixture
  ( eeEqualHits, eeDifferentHits, eeEqualMutations, eeDifferentMutations, eeConversionTable, eeConversionCases
  ) where

import Data.Aeson (Value(..), toJSON)
import Data.String (fromString)
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.PropertyPairPlan (Role(..), SiteLabel(..), getterEntry, setterEntry, siteLabelWire)
import Emulation.SettingsStorageMatrix (LifecycleSite(..))
import PropertyPairFixture (bankApplyBody, hex, littleWords, wireField)
import SettingsStorageFixtureWire
  ( BankImage(..), TraceMutation, alterFirstSite, lifecycleAddress, lifecycleHit, matrixHit, matrixStack
  , wireBytes, wireFlags, wireMemory, wireR8
  )

newtype EeStoredCode = EeStoredCode Word32 deriving Eq
newtype EeRowMetadata = EeRowMetadata Word32
newtype EeCachedValue = EeCachedValue Word32
data EeConversionRow = EeConversionRow EeStoredCode EeRowMetadata EeCachedValue

sourceRows :: [EeConversionRow]
sourceRows =
  [ row 0 30, row 1 29, row 2 28, row 3 27, row 4 26, row 5 25
  , row 6 24, row 7 23, row 8 22, row 9 21, row 10 20, row 11 19
  , row 12 18, row 13 17, row 14 16, row 15 15, row 16 14, row 17 13
  , row 18 12, row 19 11, row 20 10, row 21 9, row 22 8, row 23 7
  , row 24 6, row 25 5, row 26 4, row 27 3, row 28 2, row 29 1, row 30 0
  ]
  where row code cached = EeConversionRow (EeStoredCode code) (EeRowMetadata 0x7ffffffe) (EeCachedValue cached)

eeConversionTable :: B.ByteString
eeConversionTable = B.concat
  [littleWords [code,metadata,cached] | EeConversionRow (EeStoredCode code) (EeRowMetadata metadata) (EeCachedValue cached) <- sourceRows]
  <> littleWords [0xffff,0xffff,0xffff]

sourceCachedValue :: EeStoredCode -> EeCachedValue
sourceCachedValue wanted = case [cached | EeConversionRow code _ cached <- sourceRows,code == wanted] of
  [cached] -> cached
  _ -> error "EE fixture stored code has no retained conversion row"

eeConversionCases :: [(String,Role -> BankImage -> [Value])]
eeConversionCases =
  [("observed EE conversion row " ++ show code, \role bank -> application role bank code code 11 Nothing)
  | EeConversionRow (EeStoredCode code) _ _ <- sourceRows]

eeEqualHits :: Role -> BankImage -> [Value]
eeEqualHits role bank = application role bank 0 0 11 Nothing

eeDifferentHits :: Role -> BankImage -> [Value]
eeDifferentHits role bank = application role bank 30 0 11 Nothing
  ++ application role bank 30 0 37 (Just 0xfe)
  ++ application role bank 30 0 37 (Just 0xee)

application :: Role -> BankImage -> Word32 -> Word32 -> Word32 -> Maybe Word32 -> [Value]
application role (BankImage bank) new old frameClass frameProperty = prefix ++ body
  ++ [at EeFinalWrite [0xee,new,0,0]] ++ assignment ++ [at EeFinalReturn [1]]
  where
    cached = case sourceCachedValue (EeStoredCode new) of EeCachedValue value -> value
    modeClass = 0
    whiteBalanceMode = 2
    at site arguments = lifecycleHit role site arguments new []
    prefix = [at EeCachedCall [cached],at EeFrameClassCall [],at EeFrameClassResult [frameClass]]
      ++ case frameProperty of
        Nothing -> []
        Just property -> [at EeFramePropertyCall [],at EeFramePropertyResult [property]]
    body
      | frameClass == 37 && frameProperty == Just 0xee = []
      | otherwise = [at EeOldValueCall [0xee,0,0,0]]
          ++ oldGetter
          ++ [ lifecycleHit role EeCompare [old] new
                [bank,littleWords [modeClass],littleWords [whiteBalanceMode],littleWords [cached]]
             , wireFlags (if old == new then 0x40000013 else 0x13) (at EeConditional [old])
             ]
          ++ if old == new then [] else effects
    oldGetter =
      [ matrixHit role GetterEntry getterEntry [0xee,0,0,1] new getterStack 0x624648 []
      , matrixHit role GetterReturn 0x624648 [old] new getterStack 0 []
      ]
    getterStack = matrixStack role - 16
    effects =
      [at EeResetZeroLive [0,0,0],at EeResetEightLive [0,8,0],at EeResetNineLive [0,9,0]
      ,at EeResetZeroCustom [0,0,1],at EeResetEightCustom [0,8,1],at EeResetNineCustom [0,9,1]
      ,at EeApplyFirst [0,whiteBalanceMode],at EeApplySecond [0,whiteBalanceMode]]
    assignment =
      [ matrixHit role (StorageSite BankApplyEntry) bankEntry [0xee,new,0,0] new bankStack 0x61c928 []
      , matrixHit role SetterEntry setterEntry [0xee,new,0,1] new setterStack setterReturn []
      , matrixHit role SetterReturn setterReturn [1] new setterStack 0 []
      , matrixHit role (StorageSite BankApplyReturn) 0x61c928 [1] new bankStack 0 []
      ]
    bankEntry = if role == RetainedRole then 0x6309c4 else bankApplyBody
    bankStack = matrixStack role - 16
    setterStack = bankStack - if role == RetainedRole then 32 else 8
    setterReturn = if role == RetainedRole then 0x630a04 else bankApplyBody + 8

eeEqualMutations :: [(String,TraceMutation)]
eeEqualMutations = commonMutations ++
  [ ("equal EE comparison uses a different old value", change EeCompare (register 0 1))
  , ("equal EE comparison disagrees with its completed getter", alterFirstSite GetterReturn (register 0 1))
  , ("equal EE branch clears Z", change EeConditional (wireFlags 0x13))
  , ("equal EE branch unexpectedly performs a reset", duplicateAs EeConditional EeResetZeroLive [0,0,0])
  , ("equal EE cache must use its actual source conversion row", wrongRelation 29)
  , ("coherent EE stored code31 is outside the retained table", invalidStoredCode 31 0xffffffff)
  , ("coherent EE negative stored code is outside the retained table", invalidStoredCode 0xffffffff 31)
  ]

eeDifferentMutations :: [(String,TraceMutation)]
eeDifferentMutations = commonMutations ++
  [ ("different EE comparison claims the new value", change EeCompare (register 0 30))
  , ("different EE comparison disagrees with its completed getter", alterFirstSite GetterReturn (register 0 1))
  , ("different EE branch sets Z", change EeConditional (wireFlags 0x40000013))
  , ("different EE reset order changes", swapFirst EeResetZeroLive EeResetEightLive)
  , ("different EE reset is omitted", removeFirst EeResetNineCustom)
  , ("different EE reset is repeated", duplicateFirst EeResetEightCustom)
  , ("different EE reset channel changes", change EeResetEightLive (register 1 9))
  , ("different EE reset scope changes", change EeResetZeroCustom (register 2 0))
  , ("different EE first application uses another mode", change EeApplyFirst (register 1 3))
  , ("different EE second application uses another mode", change EeApplySecond (register 1 3))
  , ("class37 EE property result is absent", removeFirst EeFramePropertyResult)
  , ("class37 EE editor exclusion performs effects", change EeFramePropertyResult (register 0 0xee))
  , ("class37 EE exclusion loses its property identity", alterNth EeFramePropertyResult 1 (register 0 0xfe))
  , ("class37 EE full path loses its class call", removeNth EeFrameClassCall 1)
  , ("class37 EE full path loses its property call", removeFirst EeFramePropertyCall)
  , ("class37 EE full path reorders device applications", swapNth EeApplyFirst EeApplySecond 1)
  , ("different EE cache must use its actual source conversion row", wrongRelation 1)
  ]

commonMutations :: [(String,TraceMutation)]
commonMutations =
  [ ("EE old-value request changes property", change EeOldValueCall (register 0 0xed))
  , ("EE old-value request changes bank", change EeOldValueCall (register 1 1))
  , ("EE old getter uses the wrong profile", alterFirstSite GetterEntry (register 3 0))
  , ("EE old getter belongs to another logical task", alterFirstSite GetterEntry (wireField ["task","task_id"] (toJSON (6 :: Word32))))
  , ("EE old getter has another caller return", alterFirstSite GetterEntry (wireField ["lr"] (toJSON (hex 0x62464c))))
  , ("EE old getter returns after comparison", swapLabels GetterReturn (StorageSite EeCompare) 0)
  , ("EE new code changes at comparison", change EeCompare (wireR8 1))
  , ("EE new code changes at the conditional", change EeConditional (wireR8 1))
  , ("EE cached snapshot disagrees with its call", change EeCompare wrongCache)
  , ("EE comparison lacks the complete bank snapshot", change EeCompare shortBank)
  , ("EE final write changes property", change EeFinalWrite (register 0 0xed))
  , ("EE final write changes new code", change EeFinalWrite (register 1 1))
  , ("EE final write changes bank", change EeFinalWrite (register 2 1))
  , ("EE final setter assigns another property", alterFirstSite SetterEntry (register 0 0xed))
  , ("EE final setter assigns another code", alterFirstSite SetterEntry (register 1 1))
  , ("EE final setter reports no assignment", alterFirstSite SetterReturn (register 0 0))
  , ("EE final assignment changes task identity", alterFirstSite SetterEntry (wireField ["task","task_id"] (toJSON (6 :: Word32))))
  , ("EE final assignment has no bank return", removeFirst BankApplyReturn)
  , ("EE final wrapper return is absent", removeFirst EeFinalReturn)
  , ("EE class result is absent", removeFirst EeFrameClassResult)
  , ("EE source events change logical task", change EeConditional (wireField ["task","task_id"] (toJSON (6 :: Word32))))
  , ("EE source events change caller stack", change EeConditional changeStack)
  ]
  where
    wrongCache = cacheSnapshot 31
    shortBank = wireMemory [B.replicate 0x1c1 0,littleWords [0],littleWords [2],littleWords [30]]
    changeStack value@(Object fields) = case KM.lookup "sp" fields of
      Just original ->
        let replacement = if original == toJSON (hex 0x1fe0) then toJSON (hex 0x1fdc) else toJSON (hex 0x3ff4)
        in wireField ["task","sp"] replacement (wireField ["sp"] replacement value)
      Nothing -> error "EE trace has no stack pointer"
    changeStack _ = error "EE trace is not an object"

wrongRelation :: Word32 -> TraceMutation
wrongRelation cached = change EeCachedCall (register 0 cached) . change EeCompare (cacheSnapshot cached)

cacheSnapshot :: Word32 -> Value -> Value
cacheSnapshot cached value@(Object fields) = case KM.lookup "memory" fields of
  Just (Array snapshots) | V.length snapshots == 4 ->
    wireField ["memory"] (Array (snapshots V.// [(3,toJSON (wireBytes (littleWords [cached])))])) value
  _ -> error "EE conversion fixture lacks four memory snapshots"
cacheSnapshot _ _ = error "EE conversion fixture is not an object"

invalidStoredCode :: Word32 -> Word32 -> TraceMutation
invalidStoredCode code cached =
  map (wireR8 code)
  . wrongRelation cached
  . change EeCompare (register 0 code)
  . change EeConditional (register 0 code)
  . alterFirstSite GetterReturn (register 0 code)
  . change EeFinalWrite (register 1 code)
  . alterFirstSite (StorageSite BankApplyEntry) (register 1 code)
  . alterFirstSite SetterEntry (register 1 code)

register :: Int -> Word32 -> Value -> Value
register index value = wireField ["registers",fromString ("r" ++ show index)] (toJSON (hex value))

change :: LifecycleSite -> (Value -> Value) -> TraceMutation
change = alterFirstSite . StorageSite

hasLabel :: SiteLabel -> Value -> Bool
hasLabel label (Object fields) = KM.lookup "site" fields == Just (String (T.pack (siteLabelWire label)))
hasLabel _ _ = False

alterNth :: LifecycleSite -> Int -> (Value -> Value) -> TraceMutation
alterNth site wanted transform = walk wanted
  where
    walk _ [] = error "EE mutation site is absent"
    walk remaining (value:rest)
      | hasLabel (StorageSite site) value = if remaining == 0 then transform value:rest else value:walk (remaining-1) rest
      | otherwise = value:walk remaining rest

removeFirst :: LifecycleSite -> TraceMutation
removeFirst site = removeNth site 0

removeNth :: LifecycleSite -> Int -> TraceMutation
removeNth site wanted = walk wanted
  where
    walk _ [] = error "EE mutation site is absent"
    walk remaining (value:rest)
      | hasLabel (StorageSite site) value = if remaining == 0 then rest else value:walk (remaining-1) rest
      | otherwise = value:walk remaining rest

duplicateFirst :: LifecycleSite -> TraceMutation
duplicateFirst site = walk
  where
    walk [] = error "EE mutation site is absent"
    walk (value:rest)
      | hasLabel (StorageSite site) value = value:value:rest
      | otherwise = value:walk rest

duplicateAs :: LifecycleSite -> LifecycleSite -> [Word32] -> TraceMutation
duplicateAs source target arguments = walk
  where
    walk [] = error "EE mutation site is absent"
    walk (value:rest)
      | hasLabel (StorageSite source) value = value:replacement:rest
      | otherwise = value:walk rest
      where
        replacement = foldr (uncurry register) relabelled (zip [0..] arguments)
        relabelled = wireField ["site"] (String (T.pack (siteLabelWire (StorageSite target))))
          (wireField ["address"] (toJSON (hex (lifecycleAddress target))) value)

swapFirst :: LifecycleSite -> LifecycleSite -> TraceMutation
swapFirst first second = swapNth first second 0

swapNth :: LifecycleSite -> LifecycleSite -> Int -> TraceMutation
swapNth first second = swapLabels (StorageSite first) (StorageSite second)

swapLabels :: SiteLabel -> SiteLabel -> Int -> TraceMutation
swapLabels first second wanted hits = map replace (zip [0 :: Int ..] hits)
  where
    locate label = case drop wanted [index | (index,value) <- zip [0 :: Int ..] hits,hasLabel label value] of
      index:_ -> index
      [] -> error "EE mutation swap site is absent"
    firstIndex = locate first
    secondIndex = locate second
    replace (index,value)
      | index == firstIndex = hits !! secondIndex
      | index == secondIndex = hits !! firstIndex
      | otherwise = value
