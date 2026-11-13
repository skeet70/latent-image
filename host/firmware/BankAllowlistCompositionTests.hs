{-# LANGUAGE OverloadedStrings #-}
module Main where

import ArmElf (parseElf)
import BankAllowlist
import BankAllowlistComposition
import Control.Monad (forM_, unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import FujiAF.JSON (encodeCanonical)
import RamPushLayout
import System.Environment (getArgs)
import System.Exit (exitFailure)

check :: String -> Bool -> IO ()
check label passed = unless passed $ do
  putStrLn ("FAIL: " ++ label)
  exitFailure

document :: [Value] -> [Value] -> B.ByteString
document calls stores = encodeCanonical (object
  [ "schema" .= (2 :: Int), "stock_calls" .= calls, "stock_regions" .= stores ])

call :: String -> String -> Value
call address name = object ["address" .= address, "name" .= name, "source" .= ("synthetic" :: String)]

store :: String -> Value
store address = object ["address" .= address, "bytes" .= (4 :: Int), "name" .= ("synthetic" :: String), "source" .= ("synthetic" :: String)]

main :: IO ()
main = do
  [sourceBasePath, sourceModalPath, modalLow, modalHigh, sourceRenderPath] <- getArgs
  base <- B.readFile sourceBasePath
  modal <- B.readFile sourceModalPath
  let basePath = "menu/recipe_bank_allowlist.json"
      modalPath = "menu/recipe_bank_modal_allowlist_fragment.json"
      lifecyclePath = "menu/recipe_bank_allowlist.lifecycle.json"
      enginePath = "menu/recipe_bank_menu_engine_allowlist_fragment.json"
      renderPath = "menu/recipe_bank_render_allowlist_fragment.json"
      empty = document [] []
      lifecycle = document [call "0x00700004" "lifecycle"] [store "0x00950004"]
      syntheticModal = document [call "0x00700008" "modal"] [store "0x00950008"]
      sources = [(basePath, empty), (lifecyclePath, lifecycle), (modalPath, syntheticModal)]
  check "standard composition preserves exact bytes" (composeBankAllowlist standardBankVariant [(basePath, base)] == Right base)
  check "group order is canonical" (bankAllowlistSourcePaths modalLifecycleBankVariant == map fst sources)
  combined <- either fail pure (composeBankAllowlist modalLifecycleBankVariant sources >>= parseAllowlist)
  check "selected entry and region declarations are retained"
    (map stockCallAddress (stockCalls combined) == [0x00700004, 0x00700008]
      && map stockRegionAddress (stockRegions combined) == [0x00950004, 0x00950008])
  policy <- either fail pure (composeBankAllowlist modalScopedBankVariant [(basePath, base), (modalPath, modal)] >>= parseAllowlist)
  forM_ [modalLow, modalHigh] $ \path -> do
    bytes <- B.readFile path
    report <- either fail pure (parseElf bytes >>= checkBank policy)
    check (path ++ ": " ++ show (map renderFinding (violations report))) (null (violations report))
  render <- B.readFile sourceRenderPath
  rendered <- either fail pure (composeBankAllowlist modalRenderBankVariant
    [(basePath, empty), (modalPath, syntheticModal), (enginePath, empty), (renderPath, render)] >>= parseAllowlist)
  check "render interfaces retain both stock resumes"
    (all (`elem` map stockCallAddress (stockCalls rendered)) [0x01164e20, 0x01163f30])
  check "missing selected fragment refuses" (isLeft (composeBankAllowlist modalLifecycleBankVariant (take 2 sources)))
  check "reordered selected fragments refuse" (isLeft (composeBankAllowlist modalLifecycleBankVariant (reverse sources)))
  check "unselected fragment refuses" (isLeft (composeBankAllowlist standardBankVariant sources))
  check "malformed schema refuses" (isLeft (composeBankAllowlist modalScopedBankVariant [(basePath, empty), (modalPath, "{}")]))
  check "duplicate entries refuse" (isLeft (composeBankAllowlist modalLifecycleBankVariant
    [(basePath, empty), (lifecyclePath, lifecycle), (modalPath, document [call "0x00700004" "duplicate"] [])]))
  check "duplicate regions refuse" (isLeft (composeBankAllowlist modalLifecycleBankVariant
    [(basePath, empty), (lifecyclePath, lifecycle), (modalPath, document [] [store "0x00950004"])]))
  putStrLn "interface allowlist composition passed"
