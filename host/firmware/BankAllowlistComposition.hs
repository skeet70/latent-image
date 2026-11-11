{-# LANGUAGE OverloadedStrings #-}
module BankAllowlistComposition
  ( bankAllowlistSourcePaths
  , composeBankAllowlist
  ) where

import A32 (Reg (..))
import BankAllowlist
import Control.Monad (unless)
import Data.Aeson (Value, object, toJSON, (.=))
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
        (concatMap stockStores parsed)
        (concatMap declarations parsed)
  case sources of
    [(_, base)] -> pure base
    _ -> do
      unique "stock call" (map stockCallAddress (stockCalls combined))
      unique "stock store" [(stockStoreAddress store, stockStoreBytes store) | store <- stockStores combined]
      unique "indirect declaration"
        [(declarationFunction declaration, declarationAccess declaration, declarationThrough declaration)
        | declaration <- declarations combined]
      let bytes = encodeCanonical (allowlistValue combined)
      _ <- parseAllowlist bytes
      pure bytes
 where
  unique :: Eq a => String -> [a] -> Either String ()
  unique label keys = unless (length (nub keys) == length keys) $
    Left ("duplicate composed " ++ label)

allowlistValue :: Allowlist -> Value
allowlistValue allowlist = object
  [ "schema" .= (1 :: Int)
  , "stock_calls" .= map callValue (stockCalls allowlist)
  , "stock_stores" .= map storeValue (stockStores allowlist)
  , "unproven" .= map declarationValue (declarations allowlist)
  ]
 where
  callValue call = object
    [ "address" .= hexWord (stockCallAddress call)
    , "name" .= stockCallName call
    , "source" .= stockCallSource call
    ]
  storeValue store = object
    [ "address" .= hexWord (stockStoreAddress store)
    , "bytes" .= stockStoreBytes store
    , "name" .= stockStoreName store
    , "source" .= stockStoreSource store
    ]
  declarationValue declaration = object
    [ "function" .= declarationFunction declaration
    , "access" .= (case declarationAccess declaration of StoreAccess -> "store" :: String; BranchAccess -> "branch")
    , "through" .= provenanceValue (declarationThrough declaration)
    , "reason" .= declarationReason declaration
    ]

provenanceValue :: Provenance -> Value
provenanceValue provenance = case provenance of
  Argument (Reg register) -> object ["argument" .= ("r" ++ show register)]
  CallerFrame offset -> object ["caller-frame" .= offset]
  Cell location -> object ["cell" .= locationValue location]
  Result callee -> object ["result" .= (case callee of BankFunction name -> name; StockFunction address -> hexWord address)]
  Index location -> object ["index" .= locationValue location]
  Deref inner -> object ["deref" .= provenanceValue inner]
 where
  locationValue location = case location of
    BankObject name -> toJSON name
    GeneratedData -> toJSON ("generated-data" :: String)
    StackFrame -> toJSON ("stack-frame" :: String)
    StockAddress address -> toJSON (hexWord address)
    AllocationCell (AllocationOffset offset) -> object ["allocation-offset" .= hexWord offset]

hexWord :: Word32 -> String
hexWord word = "0x" ++ replicate (8 - length digits) '0' ++ digits
 where
  digits = showHex word ""
