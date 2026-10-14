module Main where
import qualified FujiAF.Host as H
import Control.Exception (SomeException, displayException, try)
import qualified Data.ByteString as B
import FujiAF.JSON (encodeCanonical)
import PublicInventory (inventory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  result <- try run
  case result :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()
  where
    run = do
      arguments <- getArgs
      case arguments of
        [checkout, revision, origin, "--output", destination] -> do
          manifest <- inventory checkout revision origin
          B.writeFile destination (encodeCanonical manifest <> B.singleton 10)
          putStrLn ("Inventoried " ++ show (length (H.list (H.get manifest "files"))) ++ " public blobs at " ++ revision)
        _ -> H.failH "usage: source-inventory CHECKOUT EXACT_REVISION URL --output OUTPUT"
