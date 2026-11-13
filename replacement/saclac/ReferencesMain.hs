{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Reports every reference into the retired saclac extents over the given
-- listings, so the retirement claim rests on the listing bytes and not on the
-- decomp's function starts.
module Main (main) where

import Control.Monad (forM)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import FujiAF.Host (failH)
import Replacement.Saclac.References (Reference (..), insideExtents, references, referencesValue)
import System.Environment (getArgs)

main :: IO ()
main = do
  arguments <- getArgs
  case options arguments of
    Just (listings, outputPath) | not (null listings) -> do
      found <- concat <$> forM listings (\(name, path) -> references name <$> B.readFile path)
      L.writeFile outputPath (J.encode (referencesValue found))
      putStrLn
        ( "saclac-extent-refs: " ++ show (length found) ++ " references, "
            ++ show (length [() | reference <- found, not (insideExtents (referenceAddress reference))])
            ++ " from outside the extents"
        )
    _ -> failH "usage: saclac-extent-refs --listing NAME=PATH ... --output PATH"

options :: [String] -> Maybe ([(String, FilePath)], FilePath)
options = go [] ""
  where
    go listings output [] = Just (reverse listings, output)
    go listings output ("--listing" : spec : rest) =
      case break (== '=') spec of
        (name, '=' : path) -> go ((name, path) : listings) output rest
        _ -> Nothing
    go listings _ ("--output" : path : rest) = go listings path rest
    go _ _ _ = Nothing
