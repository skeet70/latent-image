module Main where
import qualified FujiAF.Host as H
import Control.Exception (SomeException, displayException, try)
import Data.Bits (shiftL)
import qualified Data.ByteString as B
import PublicInventory (verifySelected)
import System.Directory (getCurrentDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import System.IO (hPutStrLn, stderr)

options :: [String] -> IO [(String,String)]
options [] = pure []
options (key:value:rest) = do
  H.ensure (key `elem` ["--fffw","--fujihack","--patcher","--dat","--repository"]) "unknown source-check argument"
  following <- options rest
  H.ensure (key `notElem` map fst following) "duplicate source-check argument"
  pure ((key,value):following)
options _ = H.failH "source-check flags require a value"

main :: IO ()
main = do
  result <- try $ do
    flags <- getArgs >>= options
    current <- getCurrentDirectory
    let repository = maybe current id (lookup "--repository" flags)
        required key = maybe (H.failH ("required argument " ++ key)) pure (lookup key flags)
    fffw <- required "--fffw"; fujihack <- required "--fujihack"; patcher <- required "--patcher"
    document <- H.readRegular (4*1024*1024) (repository </> "third_party/firmware-source-inventory.json") >>= H.jsonRead
    count <- verifySelected [("https://github.com/tiredboffin/fffw",fffw),("https://github.com/fujihack/fujihack",fujihack),("https://github.com/fujihack/patcher",patcher)] document
    putStrLn ("Verified " ++ show count ++ " source blobs and " ++ show (length (H.list (H.get document "sources"))) ++ " trees")
    case lookup "--dat" flags of
      Nothing -> pure ()
      Just path -> do
        bytes <- H.readRegular (64*1024*1024) path
        digest <- H.sha256 bytes
        H.ensure (B.length bytes==38405148 && digest=="43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") "exact public X-M1 1.50 DAT required"
        let wordAt offset = sum [fromIntegral (B.index bytes (offset+index)) `shiftL` (8*index) | index <- [0..3]] :: Integer
        H.jsonPrint $ H.obj [("bytes",H.n (fromIntegral (B.length bytes))),("sha256",H.s digest),
          ("header",H.arr [H.arr [H.n (fromIntegral offset),H.n (wordAt offset)] | offset <- [0,516,520,524,528]])]
  case result :: Either SomeException () of
    Left err -> hPutStrLn stderr (displayException err) >> exitFailure
    Right () -> pure ()
