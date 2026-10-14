module Main where
-- Static ELF metadata and exact public file hashes. Never dlopen/ldd/init/list.
import qualified FujiAF.Host as H
import Control.Monad (forM, forM_)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (isInfixOf)
import System.Directory (canonicalizePath, doesFileExist)
import System.Exit (ExitCode(..))
import System.FilePath

command :: FilePath -> [String] -> IO String
command executable arguments = do
  (stopped,status,output,errors) <- H.runBounded 3000000 (1024*1024) executable arguments B.empty
  H.ensure (not stopped && status==ExitSuccess) ("static metadata command failed: " ++ C.unpack errors)
  pure (C.unpack output)
trim :: String -> String
trim = reverse . dropWhile (`elem` " \r\n\t") . reverse . dropWhile (`elem` " \r\n\t")
split :: Char -> String -> [String]
split delimiter input = case break (==delimiter) input of (first,[]) -> [first]; (first,_:rest) -> first:split delimiter rest

main :: IO ()
main = do
  version <- command "pkg-config" ["--modversion","libusb-1.0"]
  libdir <- trim <$> command "pkg-config" ["--variable=libdir","libusb-1.0"]
  includes <- command "pkg-config" ["--variable=includedir","libusb-1.0"]
  putStr version; putStrLn libdir; putStr includes
  library <- canonicalizePath (libdir </> "libusb-1.0.so")
  let emit path = do
        bytes <- H.readRegular (64*1024*1024) path
        digest <- H.sha256 bytes
        metadata <- command "readelf" ["-d",path]
        putStrLn path; putStrLn digest; putStr metadata
        pure metadata
  metadata <- emit library
  runpath <- case filter ("(RUNPATH)" `isInfixOf`) (lines metadata) of
    [line] -> case break (=='[') line of
      (_, '[':rest) -> case break (==']') rest of
        (value,']':_) -> pure value
        _ -> H.failH "invalid ELF RUNPATH"
      _ -> H.failH "invalid ELF RUNPATH"
    _ -> H.failH "ambiguous/missing ELF RUNPATH"
  let directories = split ':' runpath
  H.ensure (all isAbsolute directories && length directories<=64) "bounded absolute ELF search paths required"
  forM_ ["libudev.so.1","libc.so.6","libpthread.so.0"] $ \name -> do
    matches <- forM directories $ \directory -> do
      let path=directory </> name
      exists<-doesFileExist path
      pure [path|exists]
    path <- case concat matches of [value] -> canonicalizePath value; _ -> H.failH ("ambiguous/missing pinned dependency: "++name)
    _ <- emit path; pure ()
