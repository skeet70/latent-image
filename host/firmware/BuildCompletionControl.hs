-- Build a declared loader-A completion control DAT from a reviewed X-M1 1.50
-- container (FujiAF.CompletionControl).
module Main where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import FujiAF.CompletionControl
import FujiAF.FirstStage
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import System.Environment (getArgs)

maxContainer :: Int
maxContainer = 64 * 1024 * 1024

main :: IO ()
main = do
  arguments <- getArgs
  (source, output, control) <- case arguments of
    [source, output, "--control", name] -> either H.failH (pure . (,,) source output) (parseCompletionControl name)
    _ -> H.failH "usage: build-completion-control SOURCE_DAT OUT_DAT --control held|delayed|non-one"
  bytes <- H.readRegular maxContainer source
  parsed <- either H.failH pure (U.parseContainer bytes)
  artifact <- either H.failH pure (buildCompletionControl control parsed)
  let mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  forM_ (controlLoaderPatches artifact) $ \patch -> do
    offset <- either H.failH pure (loaderWordPatchOffset (U.parsedParts parsed) patch)
    putStrLn
      ( "loader word " ++ U.showHexWord (loaderPatchAddress patch) ++ " " ++ U.showHexWord (loaderPatchExpected patch)
          ++ " -> " ++ U.showHexWord (loaderPatchReplacement patch) ++ " at DAT offset " ++ show (mainStart + offset)
      )
  forM_ (controlVeneerExtent artifact) $ \(offset, size) ->
    putStrLn ("record-1 veneer [" ++ show offset ++ "," ++ show (offset + size) ++ ") at DAT offset " ++ show (mainStart + firstRecordSlotOffset + offset))
  digest <- H.sha256 (controlBytes artifact)
  B.writeFile output (controlBytes artifact)
  putStrLn ("completion control " ++ completionControlName control ++ ": output " ++ output ++ ", sha256 " ++ digest)
