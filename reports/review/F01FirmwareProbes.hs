module Main where
import qualified FujiAF.Host as H
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import System.Directory
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath
import System.Posix.Files (createNamedPipe)

main :: IO ()
main = do
  arguments <- getArgs
  current <- getCurrentDirectory
  repository <- case arguments of [] -> pure current; ["--repository",path] -> canonicalizePath path; _ -> H.failH "usage: f01-firmware-probes [--repository ROOT]"
  let private = repository </> "private-data"
  createDirectoryIfMissing False private
  H.withTemporary "fuji-af-f01-fifo-" $ \temporary -> do
    let fifo = temporary </> "synthetic-input.fifo"
        destination = private </> takeFileName temporary
        command = [repository </> "host/firmware/export_payload.cjs",temporary,fifo,destination]
    createNamedPipe fifo 0o600
    (stopped,status,_,errors) <- H.runBounded 3000000 (1024*1024) "node" command B.empty
    exists <- doesPathExist destination
    H.ensure (not stopped && status /= ExitSuccess && not exists) "FIFO rejection failed or exceeded deadline"
    H.jsonPrint $ H.obj [("command",H.arr (map H.s ("node":command))),("status",H.n (case status of ExitSuccess -> 0; ExitFailure code -> fromIntegral code)),
      ("stderr",H.s (C.unpack errors)),("bounded_rejection",H.b True),("output_exists",H.b exists)]
