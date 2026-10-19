{-# LANGUAGE OverloadedStrings, ForeignFunctionInterface, ScopedTypeVariables #-}
module FujiAF.Host where
import Control.Concurrent
import Control.Exception
import Control.Monad
import Data.Aeson hiding (decodeStrict)
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Scientific (floatingOrInteger)
import Data.IORef
import Foreign.C.String
import Foreign.C.Types
import FujiAF.JSON
import System.Directory
import System.Exit
import System.FilePath
import System.IO
import System.Posix.Files
import System.Posix.IO
import System.Posix.Types
import System.Posix.Signals
import System.Process
import System.Timeout

foreign import ccall unsafe "open" c_open :: CString -> CInt -> IO CInt
failH :: String -> IO a
failH = ioError . userError
ensure :: Bool -> String -> IO ()
ensure yes message = unless yes (failH message)
obj :: [(String, Value)] -> Value
obj = Object . KM.fromList . map (\(k,v) -> (K.fromString k,v))
s :: String -> Value
s = String . T.pack
n :: Integer -> Value
n = Number . fromInteger
b :: Bool -> Value
b = Bool
arr :: [Value] -> Value
arr = Array . V.fromList
get :: Value -> String -> Value
get (Object x) key = maybe Null id (KM.lookup (K.fromString key) x)
get _ _ = Null
str :: Value -> String
str (String x) = T.unpack x
str _ = ""
num :: Value -> Integer
num (Number x) = either (const 0) id (floatingOrInteger x :: Either Double Integer)
num (String x) = case reads (T.unpack x) of [(v,"")] -> v; _ -> 0
num _ = 0
integer :: Value -> Integer -> Integer -> String -> IO Integer
integer (Number x) low high label = case floatingOrInteger x :: Either Double Integer of
  Right v | low <= v && v <= high -> pure v
  _ -> failH (label ++ ": integer out of range")
integer _ _ _ label = failH (label ++ ": integer required")
list :: Value -> [Value]
list (Array x) = V.toList x
list _ = []
keys :: Value -> [String]
keys (Object x) = map K.toString (KM.keys x)
keys _ = []
set :: String -> Value -> Value -> Value
set key value (Object x) = Object (KM.insert (K.fromString key) value x)
set _ _ x = x
merge :: Value -> [(String,Value)] -> Value
merge = foldl (\v (k,x) -> set k x v)
fields :: Value -> String -> String -> IO ()
fields v names label = ensure (case v of Object _ -> null (keys v \\ words names) && null (words names \\ keys v); _ -> False) (label ++ ": exact fields required")
  where (\\) = filter . flip notElem
jsonRead :: B.ByteString -> IO Value
jsonRead = either failH pure . decodeStrict
jsonPrint :: Value -> IO ()
jsonPrint = BC.putStrLn . encodeCanonical

-- Linux host: inspect both the pathname and the opened descriptor. Nonblocking
-- open ensures a regular-file-to-FIFO replacement cannot wait for a writer.
readRegularWith :: IO () -> Bool -> Int -> FilePath -> IO B.ByteString
readRegularWith between noLinks limit path = do
  status <- (if noLinks then getSymbolicLinkStatus else getFileStatus) path
  ensure (isRegularFile status && fileSize status <= fromIntegral limit) "input must be a capped regular file"
  between
  bracket (withCString path $ \p -> do
    fd <- c_open p (2048 + if noLinks then 131072 else 0)
    ensure (fd >= 0) "input open failed"
    pure (Fd fd)) closeFd $ \fd -> do
      opened <- getFdStatus fd
      ensure (isRegularFile opened && fileSize opened <= fromIntegral limit) "input descriptor must be a capped regular file"
      -- duplicate ownership: the bracket owns fd; the Handle owns its duplicate.
      bracket (do
        duplicate <- dup fd
        fdToHandle duplicate `onException` closeFd duplicate) hClose $ \h -> do
        bytes <- B.hGet h (limit+1)
        ensure (B.length bytes <= limit) "input exceeds finite byte budget"
        pure bytes
readRegular :: Int -> FilePath -> IO B.ByteString
readRegular = readRegularWith (pure ()) True

-- Readers always drain concurrently, retain a finite prefix, and request process
-- termination at the cap. The timeout kills and reaps the child, preserving bytes.
runBounded :: Int -> Int -> FilePath -> [String] -> B.ByteString -> IO (Bool,ExitCode,B.ByteString,B.ByteString)
runBounded micros cap executable arguments input = mask $ \restore -> do
  (Just hin,Just hout,Just herr,ph) <- createProcess (proc executable arguments){std_in=CreatePipe,std_out=CreatePipe,std_err=CreatePipe,create_group=True}
  hSetBuffering hin NoBuffering
  processPid <- getPid ph
  out <- newIORef B.empty; err <- newIORef B.empty
  excess <- newIORef False
  let reader h ref done = (let loop = do
                                chunk <- B.hGetSome h 4096
                                unless (B.null chunk) $ do
                                  current <- readIORef ref
                                  let combined = current <> chunk
                                  writeIORef ref (B.take cap combined)
                                  when (B.length combined > cap) (writeIORef excess True >> terminateProcess ph)
                                  loop
                           in loop) `finally` (hClose h >> putMVar done ())
  od <- newEmptyMVar; ed <- newEmptyMVar
  ot <- forkIO (reader hout out od); et <- forkIO (reader herr err ed)
  wt <- forkIO ((B.hPut hin input `catch` \(_ :: IOException) -> pure ()) `finally` (hClose hin `catch` \(_ :: IOException) -> pure ()))
  let cleanup = do
        forM_ processPid (\p -> signalProcessGroup sigKILL p `catch` \(_ :: IOException) -> pure ())
        terminateProcess ph `catch` \(_ :: IOException) -> pure ()
        void (timeout 1000000 (waitForProcess ph))
        killThread ot; killThread et; killThread wt
  status <- restore (timeout micros (do
    code <- waitForProcess ph
    takeMVar od; takeMVar ed
    pure code)) `onException` cleanup
  code <- case status of Just c -> pure c; Nothing -> cleanup >> pure (ExitFailure 1)
  -- Even a successful parent may leave descendants holding stdin. Close the
  -- whole owned group and cancel the input writer before returning.
  case status of Just _ -> cleanup; Nothing -> pure ()
  over <- readIORef excess
  o <- readIORef out; e <- readIORef err
  pure (status == Nothing || over,code,o,e)
sha256 :: B.ByteString -> IO String
sha256 bytes = do
  (timed,code,out,_) <- runBounded 15000000 1024 "sha256sum" [] bytes
  ensure (not timed && code == ExitSuccess && length (BC.unpack out) >= 64) "SHA256 helper failed"
  pure (take 64 (BC.unpack out))
withTemporary :: String -> (FilePath -> IO a) -> IO a
withTemporary prefix action = do
  base <- getTemporaryDirectory
  bracket (do (p,h) <- openTempFile base prefix; hClose h; removeFile p; createDirectory p; pure p) removeDirectoryRecursive action
schemaCheck :: FilePath -> FilePath -> [Value] -> IO ()
schemaCheck checker schema documents = withTemporary "fuji-af-schema" $ \root -> do
  let blobs = map encodeCanonical documents
  ensure (length blobs <= 8192 && sum (map B.length blobs) <= 8*1024*1024) "schema document budget exceeded"
  paths <- forM (zip [0::Int ..] blobs) $ \(i,bytes) -> do
    let path = root </> (show i ++ ".json")
    B.writeFile path bytes; pure path
  (timed,code,out,err) <- runBounded 15000000 (1024*1024) checker (["--schemafile",schema] ++ paths) B.empty
  ensure (not timed && code == ExitSuccess) ("schema rejection: " ++ BC.unpack (B.take 2048 out <> B.take 2048 err))
