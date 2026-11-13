{-# LANGUAGE OverloadedStrings #-}
module BankSourceBoundaryCheck (StockEntry (..), checkStockSource, checkBankSources, boundarySources, boundaryDeclarations) where

import Control.Monad (forM_, unless)
import Data.Aeson (eitherDecodeStrict', withObject, (.:), FromJSON (..))
import qualified Data.ByteString as B
import Data.Bits (shiftL, (.|.))
import Data.Char (isAlpha, isDigit, isHexDigit)
import Data.List (isInfixOf, isPrefixOf, tails)
import Data.Maybe (catMaybes)
import Data.Word (Word32)
import Numeric (readHex)
import RustSourceBoundaryCheck (lexRust, Token (..))
import System.FilePath (takeExtension)
import Text.Read (readMaybe)

newtype StockEntry = StockEntry Word32 deriving (Eq, Show)

newtype BoundarySources = BoundarySources [FilePath]
instance FromJSON BoundarySources where
  parseJSON = withObject "bank source boundaries" $ \value -> BoundarySources <$> value .: "boundary_files"

boundaryDeclarations :: [FilePath]
boundaryDeclarations = ["replacement/" ++ name ++ "/unsafe-boundary.json" | name <-
  ["bank", "capture", "service", "owned_menu", "modal_runtime", "recipe_ui", "menu_engine", "menu_value", "render_adapter", "menu_widgets", "render_commands", "save", "media_permit", "property_service", "settings_storage", "kernel"]]
  ++ ["replacement/develop/unsafe-font-boundary.json"]

boundarySources :: FilePath
boundarySources = "build-support/bank-source-boundaries.json"

word :: String -> Maybe Word32
word ('0':prefix:rest) | prefix `elem` ("xX" :: String) = case readHex (takeWhile isHexDigit rest) of
  [(value, "")] | all (`elem` ("uUlL" :: String)) (dropWhile isHexDigit rest) -> Just value
  _ -> Nothing
word input = let (digits, suffix) = span isDigit input in
  if all (`elem` ("uUlL" :: String)) suffix then readMaybe digits else Nothing

checkStockSource :: [StockEntry] -> FilePath -> String -> Either String ()
checkStockSource entries path source = do
  targets <- if takeExtension path == ".S" then assembly else cTargets
  forM_ targets $ \target -> unless (StockEntry target `elem` entries) (Left (path ++ ": undeclared stock entry " ++ show target))
 where
  rows = map (words . map (\c -> if c `elem` (",#" :: String) then ' ' else c) . takeWhile (/= '@')) (lines source)
  assembly = do
    trampolines <- mapM trampoline [row | row@(name : _) <- rows, "RB_" `isPrefixOf` name, "TRAMPOLINE" `isInfixOf` name]
    pure (trampolines ++ pairs ++ literals)
  trampoline [_, _, value] = maybe (Left (path ++ ": unsupported trampoline target")) Right (word value)
  trampoline _ = Left (path ++ ": malformed trampoline row")
  pairs = [lower .|. (upper `shiftL` 16) | ["movw", register, low] : ["movt", same, high] : _ <- tails rows, register == same, Just lower <- [word low], Just upper <- [word high]]
  literals = [target | ["ldr", "r12", label] : [transfer, "r12"] : _ <- tails rows, transfer `elem` ["bx", "blx"]
    , [defined] : [".word", value] : _ <- tails rows, defined == label ++ ":", Just target <- [word value]]
  cTargets = do
    raw <- lexRust source
    let tokens = map spelling raw
        definitions = [(name, map spelling (takeWhile ((== at) . line) rest))
          | Token "#" at : Token "define" _ : Token name _ : rest <- tails raw]
        resolve value = case word value of
          Just target -> pure (Just target)
          Nothing -> case lookup value definitions of
            Just expression -> case dropWhile (== "(") expression of
              number : after | Just target <- word number -> do
                unless (all (== ")") after) (Left (path ++ ": unsupported stock-address macro expression"))
                pure (Just target)
              _ -> pure Nothing
            Nothing -> pure Nothing
        operand suffix = case dropWhile (== "(") suffix of
          value : after -> do
            target <- resolve value
            case target of
              Just _ -> unless (case dropWhile (== ")") after of [] -> True; next : _ -> next `elem` [";", ",", "}", "("])
                (Left (path ++ ": unsupported stock-address expression"))
              Nothing -> pure ()
            pure target
          [] -> pure Nothing
        casts = [after | "(" : rest <- tails tokens, Just (inside, after) <- [closing 1 [] rest]
          , first : _ <- [inside], initial : _ <- [first], isAlpha initial || initial == '_'
          , ["(", "*", ")"] `isInfixOf` inside]
    catMaybes <$> mapM operand casts
  closing :: Int -> [String] -> [String] -> Maybe ([String], [String])
  closing _ _ [] = Nothing
  closing depth found (token : rest)
    | token == "(" = closing (depth + 1) (token : found) rest
    | token == ")", depth == 1 = Just (reverse found, rest)
    | token == ")" = closing (depth - 1) (token : found) rest
    | otherwise = closing depth (token : found) rest

checkBankSources :: [StockEntry] -> IO ()
checkBankSources entries = do
  BoundarySources paths <- either fail pure . eitherDecodeStrict' =<< B.readFile boundarySources
  forM_ paths $ \path -> readFile path >>= either fail pure . checkStockSource entries path
