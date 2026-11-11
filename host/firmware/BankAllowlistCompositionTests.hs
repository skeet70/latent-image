{-# LANGUAGE OverloadedStrings #-}
module Main where

import ArmElf (parseElf)
import BankAllowlist
import BankAllowlistComposition
import Control.Monad (forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import FujiAF.JSON (encodeCanonical)
import Numeric (showHex)
import RamPushLayout
import System.Environment (getArgs)
import System.Exit (exitFailure)

check :: String -> Bool -> IO ()
check label passed = unless passed $ do
  putStrLn ("FAIL: " ++ label)
  exitFailure

document :: [Value] -> [Value] -> [Value] -> B.ByteString
document calls stores indirectRows = encodeCanonical (object
  [ "schema" .= (1 :: Int), "stock_calls" .= calls, "stock_stores" .= stores, "unproven" .= indirectRows ])

call :: String -> String -> Value
call address name = object ["address" .= address, "name" .= name, "source" .= ("synthetic" :: String)]

store :: String -> Value
store address = object ["address" .= address, "bytes" .= (4 :: Int), "name" .= ("synthetic" :: String), "source" .= ("synthetic" :: String)]

indirect :: String -> String -> Value
indirect name reason = object
  [ "function" .= name, "access" .= ("branch" :: String)
  , "through" .= object ["argument" .= ("r0" :: String)], "reason" .= reason ]

main :: IO ()
main = do
  [sourceBasePath, sourceModalPath, modalLow, modalHigh, sourceRenderPath] <- getArgs
  frozenBase <- B.readFile sourceBasePath
  let basePath = "menu/recipe_bank_allowlist.json"
      lifecyclePath = "menu/recipe_bank_allowlist.lifecycle.json"
      modalPath = "menu/recipe_bank_modal_allowlist_fragment.json"
      base = "{ \"schema\":1, \"stock_calls\":[], \"stock_stores\":[], \"unproven\":[] }\n"
      lifecycle = document [call "0x00700004" "lifecycle"] [store "0x00950004"] [indirect "lifecycle" "synthetic lifecycle callback"]
      modal = document [call "0x00700008" "modal"] [store "0x00950008"] [indirect "modal" "synthetic modal callback"]
      sources = [(basePath, base), (lifecyclePath, lifecycle), (modalPath, modal)]
      composed = composeBankAllowlist modalLifecycleBankVariant sources
  let hexByte byte = let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits
  check "default allowlist preserves f0b8386 byte identity"
    (concatMap hexByte (B.unpack (SHA256.hash frozenBase)) == "c06f3bf3202e41efca6d4232dc88df974bfd9dbfd29aac696f9fa79b4e22ce8a"
      && composeBankAllowlist standardBankVariant [(basePath, frozenBase)] == Right frozenBase)
  sourceModal <- B.readFile sourceModalPath
  emittedPolicy <- either fail pure
    (composeBankAllowlist modalScopedBankVariant [(basePath, frozenBase), (modalPath, sourceModal)] >>= parseAllowlist)
  forM_ [modalLow, modalHigh] $ \path -> do
    emitted <- B.readFile path
    report <- either fail pure (parseElf emitted >>= checkBank emittedPolicy)
    check ("modal224 emitted declaration policy: " ++ path ++ ": " ++ show (map renderFinding (violations report)))
      (null (violations report))
  check "default composition preserves exact input bytes"
    (composeBankAllowlist standardBankVariant [(basePath, base)] == Right base)
  check "selected source paths follow canonical group order"
    (bankAllowlistSourcePaths modalLifecycleBankVariant == map fst sources)
  parsed <- either fail pure (composed >>= parseAllowlist)
  check "composed calls stores and declarations retain both selected groups in order"
    (map stockCallAddress (stockCalls parsed) == [0x00700004, 0x00700008]
      && map stockStoreAddress (stockStores parsed) == [0x00950004, 0x00950008]
      && map declarationFunction (declarations parsed) == ["lifecycle", "modal"])
  modalOnly <- either fail pure (composeBankAllowlist modalScopedBankVariant [(basePath, base), (modalPath, modal)] >>= parseAllowlist)
  check "modal-only composition excludes lifecycle declarations"
    (map declarationFunction (declarations modalOnly) == ["modal"])
  canonicalVariant <- either fail pure (bankVariant [Modal75Publication, LifecyclePublications])
  check "equivalent typed selections emit identical composed bytes"
    (composeBankAllowlist canonicalVariant sources == composed)
  sourceRender <- B.readFile sourceRenderPath
  let enginePath = "menu/recipe_bank_menu_engine_allowlist_fragment.json"
      renderPath = "menu/recipe_bank_render_allowlist_fragment.json"
      renderSources = [(basePath, base), (modalPath, modal), (enginePath, document [] [] []), (renderPath, sourceRender)]
  renderPolicy <- either fail pure (composeBankAllowlist modalRenderBankVariant renderSources >>= parseAllowlist)
  renderFragment <- either fail pure (parseAllowlist sourceRender)
  check "render composition retains the overlay44 stock resumes and draw services after modal"
    (map stockCallAddress (stockCalls renderPolicy) == 0x00700008 : map stockCallAddress (stockCalls renderFragment)
      && all (`elem` map stockCallAddress (stockCalls renderPolicy)) [0x01164e20, 0x01163f30])
  check "render composition refuses a missing engine fragment"
    (isLeft (composeBankAllowlist modalRenderBankVariant
      [(basePath, base), (modalPath, modal), (renderPath, sourceRender)]))
  let valuePath = "menu/recipe_bank_menu_value_allowlist_fragment.json"
      value = document [call "0x0070000c" "value"] [store "0x0095000c"]
        [ object
            [ "function" .= ("value_result" :: String), "access" .= ("store" :: String)
            , "through" .= object ["argument" .= ("r0" :: String)]
            , "reason" .= ("synthetic result buffer" :: String) ]
        , object
            [ "function" .= ("value_formatter" :: String), "access" .= ("branch" :: String)
            , "through" .= object ["deref" .= object ["deref" .= object ["argument" .= ("r1" :: String)]]]
            , "reason" .= ("synthetic formatter callback" :: String) ]
        ]
      valueSources = [(basePath, base), (modalPath, modal), (enginePath, document [] [] []), (valuePath, value)]
  valuePolicy <- either fail pure (composeBankAllowlist modalValueBankVariant valueSources >>= parseAllowlist)
  check "value composition retains exact result and formatter capabilities after modal"
    (map stockCallAddress (stockCalls valuePolicy) == [0x00700008, 0x0070000c]
      && map stockStoreAddress (stockStores valuePolicy) == [0x00950008, 0x0095000c]
      && map declarationFunction (declarations valuePolicy) == ["modal", "value_result", "value_formatter"]
      && map declarationAccess (declarations valuePolicy) == [BranchAccess, StoreAccess, BranchAccess])
  check "value composition preserves nested callback provenance"
    (case map declarationThrough (declarations valuePolicy) of
      [_, Argument _, Deref (Deref (Argument _))] -> True
      _ -> False)
  let relative = object
        [ "function" .= ("owned_vtable_consumer" :: String), "access" .= ("branch" :: String)
        , "through" .= object ["cell" .= object ["allocation-offset" .= ("0x000052d4" :: String)]]
        , "reason" .= ("synthetic exact owned allocation cell" :: String) ]
  relativePolicy <- either fail pure (composeBankAllowlist modalScopedBankVariant
    [(basePath, base), (modalPath, document [] [] [relative])] >>= parseAllowlist)
  check "composition preserves typed allocation-relative provenance"
    (map declarationThrough (declarations relativePolicy) == [Cell (AllocationCell (AllocationOffset 0x52d4))])
  check "value selection requires its engine and value fragments"
    (isLeft (composeBankAllowlist modalValueBankVariant
      [(basePath, base), (modalPath, modal), (valuePath, value)])
      && isLeft (composeBankAllowlist modalValueBankVariant (take 3 valueSources)))
  check "engine and render selections refuse value capabilities"
    (isLeft (composeBankAllowlist modalEngineBankVariant valueSources)
      && isLeft (composeBankAllowlist modalRenderBankVariant
        (take 3 valueSources ++ [(renderPath, sourceRender), (valuePath, value)])))
  check "value selection refuses duplicate callback capabilities from the engine"
    (isLeft (composeBankAllowlist modalValueBankVariant
      [ (basePath, base), (modalPath, modal)
      , (enginePath, document [] []
          [ object
              [ "function" .= ("value_formatter" :: String), "access" .= ("branch" :: String)
              , "through" .= object ["deref" .= object ["deref" .= object ["argument" .= ("r1" :: String)]]]
              , "reason" .= ("different synthetic callback description" :: String) ]
          ])
      , (valuePath, value)
      ]))
  check "missing selected fragments are refused"
    (isLeft (composeBankAllowlist modalLifecycleBankVariant (take 2 sources)))
  check "reordered selected fragments are refused"
    (isLeft (composeBankAllowlist modalLifecycleBankVariant [(basePath, base), (modalPath, modal), (lifecyclePath, lifecycle)]))
  check "wrong fragment identity is refused"
    (isLeft (composeBankAllowlist modalScopedBankVariant [(basePath, base), (lifecyclePath, modal)]))
  check "unselected fragments are refused"
    (isLeft (composeBankAllowlist standardBankVariant sources))
  check "malformed fragment schema is refused by the checker parser"
    (isLeft (composeBankAllowlist modalScopedBankVariant [(basePath, base), (modalPath, "{\"schema\":2}")]))
  check "duplicate calls across groups are refused"
    (isLeft (composeBankAllowlist modalLifecycleBankVariant
      [(basePath, base), (lifecyclePath, lifecycle), (modalPath, document [call "0x700004" "duplicate"] [] [])]))
  check "duplicate stores across groups are refused"
    (isLeft (composeBankAllowlist modalLifecycleBankVariant
      [(basePath, base), (lifecyclePath, lifecycle), (modalPath, document [] [store "0x00950004"] [])]))
  check "duplicate indirect capabilities with different prose are refused"
    (isLeft (composeBankAllowlist modalLifecycleBankVariant
      [(basePath, base), (lifecyclePath, lifecycle), (modalPath, document [] [] [indirect "lifecycle" "different reason"])]))
  putStrLn "Bank allowlist composition: checks passed"
