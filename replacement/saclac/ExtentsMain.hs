{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Emits the linker script and the census sites file from the declared
-- extents, so the two extents are typed once. The sites file's owned post,
-- owned callback and owned client call entries are bound from the linked image
-- variants it observes; owned callbacks come from the payload contract's
-- declared identities.
module Main (main) where

import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import FujiAF.Host (failH)
import Replacement.Saclac.Artifact (Admitted, admit)
import Replacement.Saclac.Placement (bindOwnedEntries, bindOwnedPosts, insertWatchRanges, renderLinkerScript, renderLinkerScriptDebug)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["linker-script"] -> putStr renderLinkerScript
    ["linker-script-debug"] -> putStr renderLinkerScriptDebug
    "census-sites" : inputPath : fieldsPath : outputPath : images@(_ : _) | length images `mod` 5 == 0 -> do
      value <- either failH pure . J.eitherDecode =<< L.readFile inputPath
      identities <- either failH pure . (J.parseEither declaredIdentities =<<) . J.eitherDecode =<< L.readFile fieldsPath
      admitted <- mapM admitImage (groups images)
      updated <- either failH pure (insertWatchRanges value >>= bindOwnedPosts admitted >>= bindOwnedEntries identities admitted)
      L.writeFile outputPath (J.encode updated)
    _ -> failH "usage: saclac-extents linker-script | census-sites INPUT FIELDS OUTPUT (ELF BIN SYMBOLS DISASM SECTIONS)..."

-- | Every declared identity's stock value and replacement symbol.
declaredIdentities :: J.Value -> J.Parser [(Word32, String)]
declaredIdentities = J.withObject "payload contract" $ \contract -> do
  messages <- contract J..: "messages" :: J.Parser [J.Value]
  concat <$> mapM fromMessage messages
  where
    fromMessage = J.withObject "message" $ \message -> do
      fields <- message J..:? "fields" J..!= []
      concat <$> mapM fromField fields
    fromField = J.withObject "field" $ \field -> do
      declared <- field J..:? "identity"
      case declared of
        Nothing -> pure []
        Just identity -> J.withObject "identity" (\row -> (\value symbol -> [(value, symbol)]) <$> row J..: "stock_value" <*> row J..: "replacement_symbol") identity

groups :: [a] -> [[a]]
groups [] = []
groups items = let (group, rest) = splitAt 5 items in group : groups rest

admitImage :: [FilePath] -> IO Admitted
admitImage paths = case paths of
  [elf, bin, symbols, disasm, sections] ->
    either (failH . show) pure =<<
      (admit <$> B.readFile elf <*> B.readFile bin <*> B.readFile symbols <*> B.readFile disasm <*> B.readFile sections)
  _ -> failH "an image is ELF BIN SYMBOLS DISASM SECTIONS"
