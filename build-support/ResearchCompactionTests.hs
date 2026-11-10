{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ForeignFunctionInterface #-}
module Main (main) where

import Control.Concurrent (forkOS, newEmptyMVar, putMVar, takeMVar, tryPutMVar)
import Control.Exception (SomeException, bracket, bracket_, finally, try)
import Control.Monad (unless, void)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word8)
import Foreign.C.Types (CInt (..), CSize (..))
import Foreign.Ptr (Ptr, intPtrToPtr, nullPtr)
import Foreign.Storable (peek)
import Numeric (showHex)
import System.Directory (canonicalizePath, createDirectoryIfMissing, createDirectoryLink, createFileLink, doesPathExist, findExecutable, getTemporaryDirectory, removePathForcibly, renameDirectory)
import System.Environment (getArgs, getEnv, getExecutablePath, lookupEnv, setEnv, unsetEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath ((</>))
import System.IO (IOMode (ReadMode), hPutStr, stderr, withBinaryFile)
import System.Posix.Files (createLink, deviceID, fileID, getFileStatus)
import System.Posix.IO (OpenMode (ReadOnly), closeFd, defaultFileFlags, openFd)
import System.Posix.Process (getProcessID)
import System.Posix.Temp (mkdtemp)
import System.Posix.Types (COff (..), Fd (..))
import System.Process (readProcessWithExitCode)

foreign import ccall unsafe "unshare" c_unshare :: CInt -> IO CInt
foreign import ccall unsafe "mmap" c_mmap :: Ptr Word8 -> CSize -> CInt -> CInt -> CInt -> COff -> IO (Ptr Word8)
foreign import ccall unsafe "munmap" c_munmap :: Ptr Word8 -> CSize -> IO CInt

fixtureInspector :: [String] -> IO ()
fixtureInspector arguments = do
  pid <- getEnv "COMPACTION_FIXTURE_PID"
  mode <- lookupEnv "COMPACTION_FIXTURE_INSPECTION"
  case mode of
    Just "unavailable" -> putStr ("p" ++ pid ++ "\0fNOFD\0\n")
    Just "mapped-alias" -> do
      path <- getEnv "COMPACTION_FIXTURE_MAPPED_FILE"
      status <- getFileStatus path
      putStr ("p" ++ pid ++ "\0fmem\0D0x" ++ showHex (deviceID status + 1) "" ++ "\0i" ++ show (fileID status) ++ "\0tREG\0n(deleted)\0\n")
    Just "closed-descriptor" ->
      putStr ("p" ++ pid ++ "\0f2147483647\0tunknown\0n/proc/" ++ pid ++ "/fd/2147483647 (readlink: No such file or directory)\0\n")
    Just "descriptor-probe" -> do
      descriptor <- getEnv "COMPACTION_FIXTURE_DESCRIPTOR"
      putStr ("p" ++ pid ++ "\0f" ++ descriptor ++ "\0tunknown\0n/proc/" ++ pid ++ "/fd/" ++ descriptor ++ " (readlink: No such file or directory)\0\n")
    _ -> do
      executable <- getEnv "COMPACTION_FIXTURE_LSOF"
      (status, output, errors) <- readProcessWithExitCode executable (arguments ++ ["-a", "-p", pid]) ""
      putStr output
      hPutStr stderr errors
      exitWith status

withThreadFile :: FilePath -> IO () -> IO ()
withThreadFile path action = do
  ready <- newEmptyMVar
  release <- newEmptyMVar
  done <- newEmptyMVar
  _ <- forkOS $ do
    outcome <- try $ do
      result <- c_unshare 0x400
      check (result == 0) "cannot unshare the fixture thread descriptor table"
      bracket (openFd path ReadOnly defaultFileFlags) closeFd $ \_ -> do
        putMVar ready (Right ())
        takeMVar release
    case (outcome :: Either SomeException ()) of
      Left problem -> void (tryPutMVar ready (Left (show problem)))
      Right () -> pure ()
    putMVar done ()
  started <- takeMVar ready
  case started of
    Left problem -> takeMVar done >> ioError (userError problem)
    Right () -> action `finally` (putMVar release () >> takeMVar done)

withMappedFile :: FilePath -> (Ptr Word8 -> IO ()) -> IO ()
withMappedFile path action = bracket acquire release action
 where
  acquire = bracket (openFd path ReadOnly defaultFileFlags) closeFd $ \(Fd descriptor) -> do
    address <- c_mmap nullPtr 1 1 2 descriptor 0
    check (address /= intPtrToPtr (-1)) "cannot map the fixture file"
    pure address
  release address = c_munmap address 1 >>= \result -> check (result == 0) "cannot release the fixture mapping"

check :: Bool -> String -> IO ()
check ok message = unless ok (ioError (userError message))

run :: FilePath -> [String] -> Bool -> IO ()
run executable arguments expectedSuccess = do
  (status, output, errors) <- readProcessWithExitCode executable arguments ""
  check ((status == ExitSuccess) == expectedSuccess) (unwords arguments ++ "\n" ++ output ++ errors)

fixture :: FilePath -> String -> (FilePath -> FilePath -> IO ()) -> IO ()
fixture parent name action = do
  let workspace = parent </> name
      artifacts = workspace </> "private-data/scratch/completed"
  createDirectoryIfMissing True artifacts
  action workspace artifacts

selection :: FilePath -> FilePath -> [FilePath] -> [FilePath] -> IO FilePath
selection workspace root names protected = do
  let path = workspace </> "selection.json"
      candidates = [object ["path" .= name, "reason" .= ("superseded-success-diagnostic" :: String), "basis" .= ("Synthetic regression input; replacement evidence remains protected" :: String)] | name <- names]
      value = object ["schema" .= (1 :: Int), "roots" .= [object ["path" .= root, "files" .= candidates]], "protected" .= protected, "limitations" .= ([] :: [String])]
  BL.writeFile path (encode value)
  pure path

makePlan :: FilePath -> FilePath -> FilePath -> [FilePath] -> [FilePath] -> IO FilePath
makePlan executable workspace root names protected = do
  selected <- selection workspace root names protected
  let path = workspace </> "plan.json"
  run executable ["plan", "--workspace", workspace, "--selection", selected, "--out", path] True
  pure path

apply :: FilePath -> FilePath -> Bool -> IO ()
apply executable path expectedSuccess = do
  content <- B.readFile path
  let hex byte = let value = showHex byte "" in replicate (2 - length value) '0' ++ value
      digest = concatMap hex (B.unpack (SHA.hash content))
  run executable ["apply", "--plan", path, "--sha256", digest] expectedSuccess

assertContents :: FilePath -> B.ByteString -> IO ()
assertContents path expected = B.readFile path >>= \actual -> check (actual == expected) ("changed protected contents: " ++ path)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    "+w":_ -> fixtureInspector arguments
    [input] -> runTests input
    _ -> ioError (userError "expected compaction executable")

runTests :: FilePath -> IO ()
runTests input = do
  executable <- canonicalizePath input
  temporary <- getTemporaryDirectory
  inspector <- findExecutable "lsof" >>= maybe (ioError (userError "lsof missing")) canonicalizePath
  self <- getExecutablePath >>= canonicalizePath
  pid <- getProcessID
  originalPath <- getEnv "PATH"
  bracket (mkdtemp (temporary </> "research-compaction-")) removePathForcibly $ \parent -> do
    let binaries = parent </> "bin"
    createDirectoryIfMissing True binaries
    createFileLink self (binaries </> "lsof")
    setEnv "COMPACTION_FIXTURE_LSOF" inspector
    setEnv "COMPACTION_FIXTURE_PID" (show pid)
    setEnv "PATH" (binaries ++ ":" ++ originalPath)
    fixture parent "selected-only" $ \workspace root -> do
      let selected = "old result\nwith spaces.json"
          retained = root </> "current.json"
      B.writeFile (root </> selected) "superseded"
      B.writeFile retained "required evidence"
      path <- makePlan executable workspace root [selected] [retained]
      apply executable path True
      exists <- doesPathExist (root </> selected)
      check (not exists) "selected obsolete result survived"
      assertContents retained "required evidence"

    fixture parent "hardlink" $ \workspace root -> do
      B.writeFile (root </> "selected.json") "shared"
      createLink (root </> "selected.json") (root </> "retained.json")
      selected <- selection workspace root ["selected.json"] [root </> "retained.json"]
      run executable ["plan", "--workspace", workspace, "--selection", selected, "--out", workspace </> "plan.json"] False
      assertContents (root </> "selected.json") "shared"
      assertContents (root </> "retained.json") "shared"

    fixture parent "unavailable-inspection" $ \workspace root -> do
      B.writeFile (root </> "old.json") "obsolete"
      path <- makePlan executable workspace root ["old.json"] []
      bracket_ (setEnv "COMPACTION_FIXTURE_INSPECTION" "unavailable") (unsetEnv "COMPACTION_FIXTURE_INSPECTION") (apply executable path False)
      assertContents (root </> "old.json") "obsolete"

    fixture parent "thread-descriptor" $ \workspace root -> do
      B.writeFile (root </> "held.json") "thread owned"
      B.writeFile (root </> "idle.json") "obsolete"
      path <- makePlan executable workspace root ["held.json", "idle.json"] []
      withThreadFile (root </> "held.json") (apply executable path True)
      assertContents (root </> "held.json") "thread owned"
      exists <- doesPathExist (root </> "idle.json")
      check (not exists) "idle result survived thread-specific inspection"

    fixture parent "mapped-closed-descriptor" $ \workspace root -> do
      B.writeFile (root </> "mapped.json") "mapped evidence"
      B.writeFile (root </> "idle.json") "obsolete"
      path <- makePlan executable workspace root ["mapped.json", "idle.json"] []
      withMappedFile (root </> "mapped.json") $ \address -> do
        apply executable path True
        byte <- peek address
        check (byte == 109) "mapped evidence changed"
      assertContents (root </> "mapped.json") "mapped evidence"
      exists <- doesPathExist (root </> "idle.json")
      check (not exists) "idle result survived mapped-file inspection"

    fixture parent "mapped-device-alias" $ \workspace root -> do
      let held = root </> "mapped.json"
      B.writeFile held "mapped evidence"
      B.writeFile (root </> "idle.json") "obsolete"
      path <- makePlan executable workspace root ["mapped.json", "idle.json"] []
      withMappedFile held $ \_ ->
        bracket_ (setEnv "COMPACTION_FIXTURE_INSPECTION" "mapped-alias" >> setEnv "COMPACTION_FIXTURE_MAPPED_FILE" held) (unsetEnv "COMPACTION_FIXTURE_INSPECTION" >> unsetEnv "COMPACTION_FIXTURE_MAPPED_FILE") (apply executable path True)
      assertContents held "mapped evidence"
      exists <- doesPathExist (root </> "idle.json")
      check (not exists) "idle result survived aliased mapped-file inspection"

    fixture parent "closed-descriptor-probe" $ \workspace root -> do
      B.writeFile (root </> "old.json") "obsolete"
      path <- makePlan executable workspace root ["old.json"] []
      bracket_ (setEnv "COMPACTION_FIXTURE_INSPECTION" "closed-descriptor") (unsetEnv "COMPACTION_FIXTURE_INSPECTION") (apply executable path True)
      exists <- doesPathExist (root </> "old.json")
      check (not exists) "closed unrelated descriptor blocked maintenance"

    fixture parent "live-descriptor-probe" $ \workspace root -> do
      B.writeFile (root </> "held.json") "in use"
      B.writeFile (root </> "idle.json") "obsolete"
      path <- makePlan executable workspace root ["held.json", "idle.json"] []
      bracket (openFd (root </> "held.json") ReadOnly defaultFileFlags) closeFd $ \(Fd descriptor) ->
        bracket_ (setEnv "COMPACTION_FIXTURE_INSPECTION" "descriptor-probe" >> setEnv "COMPACTION_FIXTURE_DESCRIPTOR" (show descriptor)) (unsetEnv "COMPACTION_FIXTURE_INSPECTION" >> unsetEnv "COMPACTION_FIXTURE_DESCRIPTOR") (apply executable path True)
      assertContents (root </> "held.json") "in use"
      exists <- doesPathExist (root </> "idle.json")
      check (not exists) "idle result survived live descriptor reconciliation"

    fixture parent "changed" $ \workspace root -> do
      B.writeFile (root </> "changed.json") "old"
      B.writeFile (root </> "unchanged.json") "obsolete"
      path <- makePlan executable workspace root ["changed.json", "unchanged.json"] []
      B.writeFile (root </> "changed.json") "new evidence"
      apply executable path True
      assertContents (root </> "changed.json") "new evidence"
      exists <- doesPathExist (root </> "unchanged.json")
      check (not exists) "unmodified selected result survived"

    fixture parent "open" $ \workspace root -> do
      B.writeFile (root </> "open.json") "in use"
      B.writeFile (root </> "idle.json") "obsolete"
      path <- makePlan executable workspace root ["open.json", "idle.json"] []
      withBinaryFile (root </> "open.json") ReadMode $ \_ -> apply executable path True
      assertContents (root </> "open.json") "in use"
      exists <- doesPathExist (root </> "idle.json")
      check (not exists) "idle selected result survived alongside open file"

    fixture parent "ancestor-swap" $ \workspace root -> do
      let nested = root </> "role"
          outside = workspace </> "outside"
      createDirectoryIfMissing True nested
      createDirectoryIfMissing True outside
      B.writeFile (nested </> "result.json") "same bytes"
      B.writeFile (outside </> "result.json") "same bytes"
      path <- makePlan executable workspace root ["role/result.json"] []
      renameDirectory nested (root </> "saved-role")
      createDirectoryLink outside nested
      apply executable path True
      assertContents (outside </> "result.json") "same bytes"
      assertContents (root </> "saved-role/result.json") "same bytes"

    fixture parent "changed-protection" $ \workspace root -> do
      let kept = root </> "witness.json"
      B.writeFile (root </> "old.json") "obsolete"
      B.writeFile kept "counterexample"
      path <- makePlan executable workspace root ["old.json"] [kept]
      B.writeFile kept "new counterexample"
      apply executable path False
      assertContents (root </> "old.json") "obsolete"
      assertContents kept "new counterexample"

    fixture parent "wrong-digest" $ \workspace root -> do
      B.writeFile (root </> "old.json") "obsolete"
      path <- makePlan executable workspace root ["old.json"] []
      run executable ["apply", "--plan", path, "--sha256", replicate 64 '0'] False
      assertContents (root </> "old.json") "obsolete"

    fixture parent "protected-directory" $ \workspace root -> do
      B.writeFile (root </> "result.json") "current"
      selected <- selection workspace root ["result.json"] [root]
      run executable ["plan", "--workspace", workspace, "--selection", selected, "--out", workspace </> "plan.json"] False
      assertContents (root </> "result.json") "current"

    fixture parent "media-protection" $ \workspace root -> do
      B.writeFile (root </> "card.img") "synthetic media"
      selected <- selection workspace root ["card.img"] []
      run executable ["plan", "--workspace", workspace, "--selection", selected, "--out", workspace </> "plan.json"] False
      assertContents (root </> "card.img") "synthetic media"

    fixture parent "escaping-path" $ \workspace root -> do
      B.writeFile (workspace </> "outside.json") "outside"
      selected <- selection workspace root ["../../../../outside.json"] []
      run executable ["plan", "--workspace", workspace, "--selection", selected, "--out", workspace </> "plan.json"] False
      assertContents (workspace </> "outside.json") "outside"
  putStrLn "research compaction consumer protection passed"
