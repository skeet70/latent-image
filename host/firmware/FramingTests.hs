{-# LANGUAGE OverloadedStrings #-}
module Main where
import qualified FujiAF.Host as H
import Control.Exception (SomeException, try)
import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import System.Directory
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath
import System.IO
import System.Posix.Files (createNamedPipe, createSymbolicLink)

main :: IO ()
main = do
  arguments <- getArgs
  runner <- case arguments of [path] -> canonicalizePath path; _ -> H.failH "usage: framing-tests FRAMING_PROBE"
  (stopped, status, output, errors) <- H.runBounded 3000000 65536 runner ["--self-test"] B.empty
  H.ensure (not stopped && status == ExitSuccess) (C.unpack errors)
  value <- H.jsonRead output
  H.ensure (H.get value "sha256_vectors" == H.s "passed") "independent SHA256 vector checks failed"
  let synthetic = H.get value "synthetic"
      marker = H.get (H.get synthetic "markers") "52494852"
  H.ensure (H.get synthetic "bytes" == H.n 160 && H.get synthetic "byte_sum_mod_2_32" == H.s "0x00003048") "hand-derived marker byte sum differs"
  H.ensure (H.get marker "count" == H.n 40 && H.list (H.get marker "first_offsets") == map H.n [0,4..124]) "marker count/offset cap differs"
  H.withTemporary "fuji-af-framing-guards-" $ \root -> do
    let wrong = root </> "wrong.dat"; link = root </> "link.dat"; fifo = root </> "fifo"
    B.writeFile wrong "synthetic unsupported DAT\n"
    createSymbolicLink wrong link; createNamedPipe fifo 0o600
    let sparse = root </> "wrong-exact-size.dat"
    withBinaryFile sparse WriteMode $ \handle -> hSetFileSize handle 38405148
    forM_ [wrong,link,fifo,root,"/dev/null",sparse] $ \path -> do
      result <- try (H.runBounded 3000000 65536 runner [path] B.empty)
      case result :: Either SomeException (Bool,ExitCode,B.ByteString,B.ByteString) of
        Right (False,ExitFailure _,out,_) -> H.ensure (B.null out) "rejected input emitted private metadata"
        _ -> H.failH "framing input rejection exceeded its wall cap or succeeded"
  putStrLn "Framing independent hash/marker/cap/nofollow/FIFO/special/exact-size-hash probes passed"
