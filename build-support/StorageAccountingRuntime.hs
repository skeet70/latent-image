{-# LANGUAGE CApiFFI #-}
{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE ScopedTypeVariables #-}
module StorageAccountingRuntime
  ( CommandOutcome (..)
  , OutputLimit (..)
  , runBoundedCommand
  , btrfsFilesystemId
  , StateDirectory
  , withStateLock
  , withStateRead
  , stateDisplayPath
  , readStateBytes
  , writeStateBytes
  ) where

import Control.Concurrent (MVar, ThreadId, forkIO, killThread, newEmptyMVar, putMVar, threadDelay, tryReadMVar)
import Control.Exception (IOException, bracket, catch, displayException, finally, mask, onException, try, uninterruptibleMask_)
import Control.Monad (forM, unless, when)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.IORef (IORef, modifyIORef', newIORef, readIORef, writeIORef)
import Data.List (isPrefixOf, nub)
import Data.Maybe (catMaybes)
import qualified Data.Text as T
import Data.Word (Word8, Word64)
import Foreign.C.Error (eAGAIN, eINTR, eWOULDBLOCK, getErrno, throwErrnoIfMinus1, throwErrnoIfMinus1_)
import Foreign.C.String (CString)
import Foreign.C.Types (CInt (..), CUInt (..), CULong (..))
import Foreign.Marshal.Alloc (allocaBytesAligned)
import Foreign.Marshal.Array (peekArray)
import Foreign.Marshal.Utils (fillBytes)
import Foreign.Ptr (Ptr, castPtr, plusPtr)
import Foreign.Storable (alignment, sizeOf)
import GHC.Clock (getMonotonicTimeNSec)
import qualified GHC.Foreign as GHC
import GHC.IO.Encoding (getFileSystemEncoding)
import Numeric (showHex)
import StorageAccountingTypes (FilesystemId (..), Seconds (..), StoragePath (..))
import System.Directory (canonicalizePath, getHomeDirectory, makeAbsolute)
import System.Exit (ExitCode)
import System.FilePath ((</>), isAbsolute, makeRelative, normalise, splitDirectories, takeDirectory, takeFileName)
import System.IO (Handle, hClose, hFlush)
import System.IO.Error (isAlreadyExistsError, isDoesNotExistError)
import System.Posix.Directory (createDirectory)
import System.Posix.Files (FileStatus, deviceID, fileID, fileMode, fileOwner, getFdStatus, getSymbolicLinkStatus, isDirectory, isRegularFile, isSymbolicLink, linkCount, removeLink, rename, setFdMode)
import System.Posix.IO (OpenFileFlags (..), OpenMode (..), closeFd, defaultFileFlags, fdToHandle, openFd)
import System.Posix.Signals (nullSignal, sigKILL, sigTERM, signalProcessGroup)
import System.Posix.Types (Fd (..), ProcessGroupID)
import qualified System.Posix.User as User
import System.Process (CreateProcess (..), ProcessHandle, StdStream (..), createProcess, getPid, getProcessExitCode, proc, terminateProcess)

foreign import ccall unsafe "flock" c_flock :: CInt -> CInt -> IO CInt
foreign import capi unsafe "fcntl.h open" c_openDirectory :: CString -> CInt -> IO CInt
foreign import capi unsafe "fcntl.h value O_DIRECTORY" c_oDirectory :: CInt
foreign import capi unsafe "fcntl.h value O_NOFOLLOW" c_oNoFollow :: CInt
foreign import capi unsafe "fcntl.h value O_CLOEXEC" c_oCloexec :: CInt
foreign import capi unsafe "linux/btrfs.h value BTRFS_IOC_FS_INFO" c_btrfsFsInfoRequest :: CULong
foreign import capi unsafe "linux/btrfs.h value BTRFS_FSID_SIZE" c_btrfsFsidSize :: CInt
foreign import capi unsafe "linux/ioctl.h _IOC_SIZE" c_ioctlSize :: CULong -> CUInt
foreign import capi unsafe "sys/ioctl.h ioctl" c_btrfsIoctl :: CInt -> CULong -> Ptr () -> IO CInt

btrfsFilesystemId :: StoragePath -> IO FilesystemId
btrfsFilesystemId (StoragePath path) = bracket openDirectory closeFd $ \(Fd descriptor) -> do
  let bufferSize = fromIntegral (c_ioctlSize c_btrfsFsInfoRequest)
      uuidSize = fromIntegral c_btrfsFsidSize
      uuidOffset = 2 * sizeOf (0 :: Word64)
  ensure (uuidSize == 16 && bufferSize >= uuidOffset + uuidSize) "unsupported Btrfs filesystem-info ABI"
  allocaBytesAligned bufferSize (alignment (0 :: Word64)) $ \buffer -> do
    fillBytes buffer 0 bufferSize
    throwErrnoIfMinus1_ "BTRFS_IOC_FS_INFO" (c_btrfsIoctl descriptor c_btrfsFsInfoRequest (castPtr buffer))
    -- fsid follows max_id and num_devices in btrfs_ioctl_fs_info_args.
    uuid <- peekArray uuidSize (castPtr (buffer `plusPtr` uuidOffset) :: Ptr Word8)
    pure (FilesystemId (T.pack ("btrfs:" ++ concatMap renderByte (zip [0 :: Int ..] uuid))))
 where
  openDirectory = do
    encoding <- getFileSystemEncoding
    GHC.withCString encoding path $ \encoded ->
      Fd <$> throwErrnoIfMinus1 "open Btrfs directory" (c_openDirectory encoded (c_oDirectory .|. c_oNoFollow .|. c_oCloexec))
  renderByte (index, byte) =
    (if index `elem` [4, 6, 8, 10] then "-" else "") ++
      case showHex byte "" of
        [digit] -> ['0', digit]
        digits -> digits

data CommandOutcome
  = CommandCompleted ExitCode B.ByteString B.ByteString
  | CommandTimedOut B.ByteString B.ByteString
  | CommandOutputExceeded B.ByteString B.ByteString
  | CommandSpawnFailed String

newtype OutputLimit = OutputLimit Int

maxCommandStderr, terminationGraceMicros, readerGraceMicros, reapGraceMicros, pollMicros :: Int
maxCommandStderr = 16 * 1024
maxCommandTimeoutSeconds :: Integer
maxCommandTimeoutSeconds = 24 * 60 * 60
terminationGraceMicros = 1000000
readerGraceMicros = 1000000
reapGraceMicros = 2000000
pollMicros = 10000

data OutcomeReason = ReasonExited ExitCode | ReasonTimedOut | ReasonOverflowed

runBoundedCommand :: Seconds -> OutputLimit -> FilePath -> [String] -> IO CommandOutcome
runBoundedCommand timeoutValue (OutputLimit outputLimit) program arguments
  | requestedSeconds <= 0 = pure (CommandSpawnFailed "command timeout must be positive")
  | otherwise = mask $ \restore -> do
      let process = (proc program arguments)
            { std_in = NoStream
            , std_out = CreatePipe
            , std_err = CreatePipe
            , create_group = True
            }
          timeoutMicros = min maxCommandTimeoutSeconds requestedSeconds * 1000000
          outputBound = max 0 outputLimit
      spawned <- try (createProcess process)
      case spawned of
        Left (err :: IOException) -> pure (CommandSpawnFailed (displayException err))
        Right (_, Just outHandle, Just errHandle, processHandle) -> do
          processGroup <- fmap fromIntegral <$> getPid processHandle
          outChunks <- newIORef []
          errChunks <- newIORef []
          overflow <- newIORef False
          outDone <- newEmptyMVar
          errDone <- newEmptyMVar
          cleaned <- newIORef False
          outThread <- forkIO (readStream outHandle outputBound outChunks overflow True `finally` putMVar outDone ())
          errThread <- forkIO (readStream errHandle maxCommandStderr errChunks overflow False `finally` putMVar errDone ())
          let cleanup = do
                alreadyCleaned <- readIORef cleaned
                unless alreadyCleaned $ do
                  cleanupProcess processGroup processHandle outThread errThread outHandle errHandle outDone errDone
                  writeIORef cleaned True
              body = do
                start <- getMonotonicTimeNSec
                let deadline = toInteger start + timeoutMicros * 1000
                reason <- awaitReason processHandle overflow deadline
                cleanup
                out <- collectChunks outChunks
                err <- collectChunks errChunks
                exceeded <- readIORef overflow
                pure $ case reason of
                  ReasonExited exitCode
                    | exceeded -> CommandOutputExceeded out err
                    | otherwise -> CommandCompleted exitCode out err
                  ReasonTimedOut -> CommandTimedOut out err
                  ReasonOverflowed -> CommandOutputExceeded out err
          restore body `onException` cleanup
        Right (_, outHandle, errHandle, processHandle) -> do
          processGroup <- fmap fromIntegral <$> getPid processHandle
          uninterruptibleMask_ $ do
            terminateGroup processGroup processHandle
            _ <- waitForExit processHandle reapGraceMicros
            mapM_ closeQuietly (catMaybes [outHandle, errHandle])
          pure (CommandSpawnFailed "cannot create process pipes")
 where
  requestedSeconds = unSeconds timeoutValue

awaitReason :: ProcessHandle -> IORef Bool -> Integer -> IO OutcomeReason
awaitReason processHandle overflow deadline = go
 where
  go = do
    exceeded <- readIORef overflow
    if exceeded
      then pure ReasonOverflowed
      else do
        exitCode <- getProcessExitCode processHandle
        case exitCode of
          Just code -> pure (ReasonExited code)
          Nothing -> do
            now <- getMonotonicTimeNSec
            if toInteger now >= deadline
              then pure ReasonTimedOut
              else threadDelay pollMicros >> go

readStream :: Handle -> Int -> IORef [B.ByteString] -> IORef Bool -> Bool -> IO ()
readStream handle limit chunks overflow stopOnOverflow = go 0 `catch` \(_ :: IOException) -> pure ()
 where
  go total = do
    chunk <- B.hGetSome handle 65536
    if B.null chunk
      then pure ()
      else do
        let remaining = limit - total
        if remaining <= 0
          then if stopOnOverflow
            then writeIORef overflow True
            else go total
          else if B.length chunk > remaining
            then do
              modifyIORef' chunks (B.take remaining chunk :)
              if stopOnOverflow
                then writeIORef overflow True
                else go (total + remaining)
            else do
              modifyIORef' chunks (chunk :)
              go (total + B.length chunk)

collectChunks :: IORef [B.ByteString] -> IO B.ByteString
collectChunks chunks = B.concat . reverse <$> readIORef chunks

cleanupProcess :: Maybe ProcessGroupID -> ProcessHandle -> ThreadId -> ThreadId -> Handle -> Handle -> MVar () -> MVar () -> IO ()
cleanupProcess processGroup processHandle outThread errThread outHandle errHandle outDone errDone =
  uninterruptibleMask_ $ do
    terminateGroup processGroup processHandle
    _ <- waitForExit processHandle reapGraceMicros
    outFinished <- waitForMVar outDone readerGraceMicros
    errFinished <- waitForMVar errDone readerGraceMicros
    unless outFinished (killThread outThread)
    unless errFinished (killThread errThread)
    _ <- waitForMVar outDone readerGraceMicros
    _ <- waitForMVar errDone readerGraceMicros
    closeQuietly outHandle
    closeQuietly errHandle

terminateGroup :: Maybe ProcessGroupID -> ProcessHandle -> IO ()
terminateGroup processGroup processHandle = case processGroup of
  Nothing -> terminateProcess processHandle `catch` \(_ :: IOException) -> pure ()
  Just group -> do
    signalGroup sigTERM group
    alive <- groupAlive group
    when alive $ do
      _ <- waitForGroupGone group terminationGraceMicros
      signalGroup sigKILL group

signalGroup :: CInt -> ProcessGroupID -> IO ()
signalGroup signal group = signalProcessGroup signal group `catch` \(_ :: IOException) -> pure ()

groupAlive :: ProcessGroupID -> IO Bool
groupAlive group = (signalProcessGroup nullSignal group >> pure True) `catch` \(_ :: IOException) -> pure False

waitForGroupGone :: ProcessGroupID -> Int -> IO Bool
waitForGroupGone group limit = go 0
 where
  go elapsed = do
    alive <- groupAlive group
    if not alive
      then pure True
      else if elapsed >= limit
        then pure False
        else threadDelay pollMicros >> go (elapsed + pollMicros)

waitForExit :: ProcessHandle -> Int -> IO Bool
waitForExit processHandle limit = go 0
 where
  go elapsed = do
    exitCode <- getProcessExitCode processHandle
    case exitCode of
      Just _ -> pure True
      Nothing
        | elapsed >= limit -> pure False
        | otherwise -> threadDelay pollMicros >> go (elapsed + pollMicros)

waitForMVar :: MVar () -> Int -> IO Bool
waitForMVar done limit = go 0
 where
  go elapsed = do
    finished <- tryReadMVar done
    case finished of
      Just () -> pure True
      Nothing
        | elapsed >= limit -> pure False
        | otherwise -> threadDelay pollMicros >> go (elapsed + pollMicros)

closeQuietly :: Handle -> IO ()
closeQuietly handle = hClose handle `catch` \(_ :: IOException) -> pure ()

newtype StateDirectory = StateDirectory (Fd, FilePath)

stateDisplayPath :: StateDirectory -> FilePath
stateDisplayPath (StateDirectory (_, path)) = path

withStateLock :: FilePath -> (StateDirectory -> IO a) -> IO a
withStateLock input action = mask $ \restore -> do
  directory <- openStateDirectory True input
  restore (bracket (acquireStateLock directory) closeFd (\_ -> action directory))
    `finally` closeQuietlyFd (directoryFd directory)

withStateRead :: FilePath -> (StateDirectory -> IO a) -> IO a
withStateRead input action = mask $ \restore -> do
  directory <- openStateDirectory False input
  restore (action directory) `finally` closeQuietlyFd (directoryFd directory)

readStateBytes :: OutputLimit -> StateDirectory -> IO (Maybe B.ByteString)
readStateBytes (OutputLimit limit) directory = mask $ \restore -> do
  ensure (limit >= 0) "accounting state bound must be nonnegative"
  opened <- try (openFd (statePath directory "accounting.json") ReadOnly defaultFileFlags { nofollow = True, cloexec = True, nonBlock = True })
  case opened of
    Left (err :: IOException)
      | isDoesNotExistError err -> pure Nothing
      | otherwise -> ioError err
    Right fd -> do
      validated <- try $ do
        status <- getFdStatus fd
        caller <- User.getEffectiveUserID
        ensure (isRegularFile status && not (isSymbolicLink status)) "accounting state must be a regular file"
        ensure (fileOwner status == caller) "accounting state must be owned by the invoking account"
        ensure ((fileMode status .&. 0o077) == 0) "accounting state must not be accessible by other accounts"
        ensure (linkCount status == 1) "accounting state must not be hard-linked"
      case validated of
        Left (err :: IOException) -> closeQuietlyFd fd >> ioError err
        Right () -> do
          handle <- fdToHandle fd `onException` closeQuietlyFd fd
          bytes <- restore (readBoundedHandle limit handle) `finally` hClose handle
          pure (Just bytes)

writeStateBytes :: OutputLimit -> StateDirectory -> BL.ByteString -> IO ()
writeStateBytes (OutputLimit limit) directory payload = mask $ \restore -> do
  ensure (limit >= 0) "accounting state bound must be nonnegative"
  ensure (BL.length payload <= fromIntegral limit) "accounting state exceeds the configured bound"
  let target = statePath directory "accounting.json"
      temporary = statePath directory "accounting.json.tmp"
  fd <- createTemporary temporary
  setFdMode fd 0o600 `onException` abandonTemporary fd temporary
  handle <- fdToHandle fd `onException` abandonTemporary fd temporary
  (restore (do
      BL.hPut handle payload
      hFlush handle
      rename temporary target
      ) `onException` removeTemporaryNoFd temporary) `finally` hClose handle

createTemporary :: FilePath -> IO Fd
createTemporary temporary = go (3 :: Int)
 where
  flags = defaultFileFlags { creat = Just 0o600, exclusive = True, nofollow = True, cloexec = True }
  go attempts = do
    opened <- try (openFd temporary WriteOnly flags)
    case opened of
      Right fd -> pure fd
      Left (err :: IOException)
        | isAlreadyExistsError err && attempts > 1 -> do
            existing <- lstatMaybe temporary
            case existing of
              Nothing -> go (attempts - 1)
              Just status -> do
                caller <- User.getEffectiveUserID
                ensure (isRegularFile status && not (isSymbolicLink status) && fileOwner status == caller)
                  "state temporary path is unsafe"
                removeLink temporary
                go (attempts - 1)
        | otherwise -> ioError err

abandonTemporary :: Fd -> FilePath -> IO ()
abandonTemporary fd temporary = do
  closeQuietlyFd fd
  removeTemporaryNoFd temporary

removeTemporaryNoFd :: FilePath -> IO ()
removeTemporaryNoFd temporary = removeLink temporary `catch` \(_ :: IOException) -> pure ()

readBoundedHandle :: Int -> Handle -> IO B.ByteString
readBoundedHandle limit handle = go 0 []
 where
  go total chunks = do
    chunk <- B.hGetSome handle 65536
    if B.null chunk
      then pure (B.concat (reverse chunks))
      else if total + toInteger (B.length chunk) > toInteger limit
        then ioError (userError "accounting state exceeds the configured bound")
        else go (total + toInteger (B.length chunk)) (chunk : chunks)

acquireStateLock :: StateDirectory -> IO Fd
acquireStateLock directory = do
  fd <- openFd (statePath directory "accounting.lock") ReadWrite defaultFileFlags { creat = Just 0o600, nofollow = True, cloexec = True }
  (do
    status <- getFdStatus fd
    caller <- User.getEffectiveUserID
    ensure (isRegularFile status && fileOwner status == caller) "state lock must be a regular file owned by the invoking account"
    case fd of
      Fd descriptor -> acquireLock descriptor
    ) `onException` closeQuietlyFd fd
  pure fd
 where
  acquireLock descriptor = do
    result <- c_flock descriptor 6
    if result == 0
      then pure ()
      else do
        errno <- getErrno
        if errno == eINTR
          then acquireLock descriptor
          else if errno == eAGAIN || errno == eWOULDBLOCK
            then ioError (userError "another storage accounting run holds the state lock")
            else ioError (userError "cannot acquire storage accounting state lock")

statePath :: StateDirectory -> FilePath -> FilePath
statePath (StateDirectory (fd, _)) name = fdDirectoryPath fd </> name

fdDirectoryPath :: Fd -> FilePath
fdDirectoryPath (Fd raw) = "/proc/self/fd/" ++ show raw

directoryFd :: StateDirectory -> Fd
directoryFd (StateDirectory (fd, _)) = fd

openStateDirectory :: Bool -> FilePath -> IO StateDirectory
openStateDirectory create input = do
  absoluteResult <- try (normalise <$> makeAbsolute input) :: IO (Either IOException FilePath)
  absolute <- either (ioError . userError . ("state directory cannot be made absolute: " ++) . displayException) pure absoluteResult
  let parent = takeDirectory absolute
      name = takeFileName absolute
  ensure (isAbsolute absolute) "state directory must be absolute"
  ensure (not (null name) && name /= "." && name /= ".." && '/' `notElem` name) "invalid state directory name"
  parentCanonical <- requireTrustedStatePath parent
  (childStatus, childFd) <- bracket (openValidatedDirectory parentCanonical) closeFd $ \parentFd -> do
    let childPath = fdDirectoryPath parentFd </> name
    status <- ensureChildDirectory create childPath input
    ensure (isDirectory status && not (isSymbolicLink status)) "state directory must be a directory, not a symlink or file"
    fd <- openValidatedDirectoryAt childPath status
    pure (status, fd)
  (do
    fdStatus <- getFdStatus childFd
    caller <- User.getEffectiveUserID
    ensure (sameFile childStatus fdStatus) "state directory changed while opening"
    ensure (fileOwner fdStatus == caller) "state directory must be owned by the invoking account"
    if create
      then setFdMode childFd 0o700
      else ensure (ownerOnly fdStatus) "state directory must be owner-only"
    finalStatus <- getFdStatus childFd
    ensure (isDirectory finalStatus && fileOwner finalStatus == caller && ownerOnly finalStatus) "state directory is not owner-only"
    pure (StateDirectory (childFd, parentCanonical </> name))
    ) `onException` closeQuietlyFd childFd

ensureChildDirectory :: Bool -> FilePath -> FilePath -> IO FileStatus
ensureChildDirectory create path input = do
  existing <- lstatMaybe path
  case existing of
    Just status -> pure status
    Nothing
      | not create -> ioError (userError ("state directory does not exist: " ++ input))
      | otherwise -> do
          createDirectory path 0o700 `catch` \(err :: IOException) ->
            unless (isAlreadyExistsError err) (ioError err)
          lstatMaybe path >>= maybe (ioError (userError "state directory disappeared while creating")) pure

openValidatedDirectory :: FilePath -> IO Fd
openValidatedDirectory path = do
  status <- getSymbolicLinkStatus path
  ensure (isDirectory status && not (isSymbolicLink status)) "state path must be a directory, not a symlink or file"
  openValidatedDirectoryAt path status

openValidatedDirectoryAt :: FilePath -> FileStatus -> IO Fd
openValidatedDirectoryAt path expected = do
  fd <- openFd path ReadOnly defaultFileFlags { nofollow = True, directory = True, cloexec = True }
  (do
    status <- getFdStatus fd
    ensure (sameFile expected status) "state directory changed while opening"
    ) `onException` closeQuietlyFd fd
  pure fd

lstatMaybe :: FilePath -> IO (Maybe FileStatus)
lstatMaybe path = do
  result <- try (getSymbolicLinkStatus path) :: IO (Either IOException FileStatus)
  case result of
    Right status -> pure (Just status)
    Left err
      | isDoesNotExistError err -> pure Nothing
      | otherwise -> ioError err

sameFile :: FileStatus -> FileStatus -> Bool
sameFile first second = deviceID first == deviceID second && fileID first == fileID second

ownerOnly :: FileStatus -> Bool
ownerOnly status = (fileMode status .&. 0o077) == 0

closeQuietlyFd :: Fd -> IO ()
closeQuietlyFd fd = closeFd fd `catch` \(_ :: IOException) -> pure ()

requireTrustedStatePath :: FilePath -> IO FilePath
requireTrustedStatePath absolute = do
  canonicalResult <- try (canonicalizePath absolute) :: IO (Either IOException FilePath)
  canonical <- either (ioError . userError . ("state path cannot be canonicalized: " ++) . displayException) pure canonicalResult
  environmentHome <- try getHomeDirectory :: IO (Either IOException FilePath)
  passwdHome <- try (User.getEffectiveUserID >>= User.getUserEntryForID >>= \entry -> pure (User.homeDirectory entry)) :: IO (Either IOException FilePath)
  let homePaths = nub (either (const []) (: []) environmentHome ++ either (const []) (: []) passwdHome)
  homePairs <- fmap catMaybes $ forM homePaths $ \home -> do
    homeCanonicalResult <- try (canonicalizePath home) :: IO (Either IOException FilePath)
    pure $ case homeCanonicalResult of
      Right homeCanonical -> Just (home, homeCanonical)
      Left _ -> Nothing
  let expected = case
        [ normalise (homeCanonical </> makeRelative prefix absolute)
        | (home, homeCanonical) <- homePairs
        , prefix <- [home, homeCanonical]
        , inside prefix absolute
        ] of
        candidate : _ -> candidate
        [] -> absolute
  ensure (canonical == expected) "state path must not redirect through a symlink"
  pure canonical

inside :: FilePath -> FilePath -> Bool
inside parent child = splitDirectories (normalise parent) `isPrefixOf` splitDirectories (normalise child)

ensure :: Bool -> String -> IO ()
ensure condition message = unless condition (ioError (userError message))

unSeconds :: Seconds -> Integer
unSeconds (Seconds value) = value
