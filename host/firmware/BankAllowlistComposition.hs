{-# LANGUAGE OverloadedStrings #-}
module BankAllowlistComposition
  ( bankAllowlistSourcePaths
  , composeBankAllowlist
  ) where

import BankAllowlist
import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.List (nub)
import Data.Word (Word32)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import RamPushLayout

bankAllowlistSourcePaths :: BankVariant -> [FilePath]
bankAllowlistSourcePaths variant = "menu/recipe_bank_allowlist.json" : map fragment (publicationGroups variant)
 where
  fragment LifecyclePublications = "menu/recipe_bank_allowlist.lifecycle.json"
  fragment Modal75Publication = "menu/recipe_bank_modal_allowlist_fragment.json"
  fragment MenuEnginePublication = "menu/recipe_bank_menu_engine_allowlist_fragment.json"
  fragment RenderPublication = "menu/recipe_bank_render_allowlist_fragment.json"
  fragment MenuValuePublication = "menu/recipe_bank_menu_value_allowlist_fragment.json"

composeBankAllowlist :: BankVariant -> [(FilePath, B.ByteString)] -> Either String B.ByteString
composeBankAllowlist variant sources = do
  unless (map fst sources == bankAllowlistSourcePaths variant) $
    Left "allowlist sources differ from the selected publication groups"
  parsed <- mapM (parseAllowlist . snd) sources
  let combined = Allowlist
        (concatMap stockCalls parsed)
        (concatMap stockRegions parsed)
  case sources of
    [(_, base)] -> pure base
    _ -> do
      unique "stock call" (map stockCallAddress (stockCalls combined))
      unique "stock store" [(stockRegionAddress store, stockRegionBytes store) | store <- stockRegions combined]
      let bytes = encodeCanonical (allowlistValue combined)
      _ <- parseAllowlist bytes
      pure bytes
 where
  unique :: Eq a => String -> [a] -> Either String ()
  unique label keys = unless (length (nub keys) == length keys) $
    Left ("duplicate composed " ++ label)

allowlistValue :: Allowlist -> Value
allowlistValue allowlist = object
  [ "schema" .= (2 :: Int)
  , "stock_calls" .= map callValue (stockCalls allowlist)
  , "stock_regions" .= map storeValue (stockRegions allowlist)
  ]
 where
  callValue call = object
    [ "address" .= hexWord (stockCallAddress call)
    , "name" .= stockCallName call
    , "source" .= stockCallSource call
    ]
  storeValue store = object
    [ "address" .= hexWord (stockRegionAddress store)
    , "bytes" .= stockRegionBytes store
    , "name" .= stockRegionName store
    , "source" .= stockRegionSource store
    ]

hexWord :: Word32 -> String
hexWord word = "0x" ++ replicate (8 - length digits) '0' ++ digits
 where
  digits = showHex word ""
