-- | CPython embedded in this process through libpython's exported C API. The declared base
-- interpreter's libpython is loaded with global symbols on first use. The interpreter is
-- initialized on a bound thread, which then holds the GIL; every later call must come from that
-- thread. A failing call raises an 'IOError' carrying the Python exception's text and leaves the
-- Python error indicator clear.
module Emulation.EmbeddedPython
  ( PyObject
  , RuntimeIdentity (..)
  , runtimeIdentity
  , initialize
  , finish
  , release
  , importModule
  , getAttr
  , dictSetItem
  , insertPath
  , fromJSONText
  , toJSONText
  , call
  ) where

#include "recorder-python-runtime.h"
#include "patchlevel.h"

import Control.Concurrent (isCurrentThreadBound)
import Control.Exception (bracket)
import Control.Monad (forM_, unless, void, when)
import qualified Data.ByteString as B
import Data.Int (Int64)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Foreign (FunPtr, Ptr, alloca, nullPtr, peek, withArrayLen, withMany)
import Foreign.C (CInt (..), CSize (..), CString)
import qualified GHC.Foreign as GF
import GHC.IO.Encoding (utf8)
import System.IO.Unsafe (unsafePerformIO)
import System.Posix.DynamicLinker (DL, RTLDFlags (RTLD_GLOBAL, RTLD_NOW), dlopen, dlsym)
import System.Posix.Types (CSsize (..))

-- | A strong reference owned by the caller; 'release' drops it.
newtype PyObject = PyObject (Ptr ())

data InitConfig

libpython :: DL
libpython = unsafePerformIO $ dlopen
  (#{const_str AFPY_DECLARED_BASE} ++ "/lib/lib" ++ #{const_str AFPY_DECLARED_ABI} ++ ".so.1.0")
  [RTLD_NOW, RTLD_GLOBAL]
{-# NOINLINE libpython #-}

symbol :: String -> FunPtr a
symbol name = unsafePerformIO (dlsym libpython name)
{-# NOINLINE symbol #-}

foreign import ccall "dynamic" dynConfig :: FunPtr (IO (Ptr InitConfig)) -> IO (Ptr InitConfig)
foreign import ccall "dynamic" dynConfigUnit :: FunPtr (Ptr InitConfig -> IO ()) -> Ptr InitConfig -> IO ()
foreign import ccall "dynamic" dynConfigInt :: FunPtr (Ptr InitConfig -> IO CInt) -> Ptr InitConfig -> IO CInt
foreign import ccall "dynamic" dynConfigError :: FunPtr (Ptr InitConfig -> Ptr CString -> IO CInt) -> Ptr InitConfig -> Ptr CString -> IO CInt
foreign import ccall "dynamic" dynConfigSetInt :: FunPtr (Ptr InitConfig -> CString -> Int64 -> IO CInt) -> Ptr InitConfig -> CString -> Int64 -> IO CInt
foreign import ccall "dynamic" dynConfigSetStr :: FunPtr (Ptr InitConfig -> CString -> CString -> IO CInt) -> Ptr InitConfig -> CString -> CString -> IO CInt
foreign import ccall "dynamic" dynConfigSetList :: FunPtr (Ptr InitConfig -> CString -> CSize -> Ptr CString -> IO CInt) -> Ptr InitConfig -> CString -> CSize -> Ptr CString -> IO CInt
foreign import ccall "dynamic" dynInt :: FunPtr (IO CInt) -> IO CInt
foreign import ccall "dynamic" dynString :: FunPtr (IO CString) -> IO CString
foreign import ccall "dynamic" dynUnit :: FunPtr (IO ()) -> IO ()
foreign import ccall "dynamic" dynObject :: FunPtr (IO (Ptr ())) -> IO (Ptr ())
foreign import ccall "dynamic" dynObjectUnit :: FunPtr (Ptr () -> IO ()) -> Ptr () -> IO ()
foreign import ccall "dynamic" dynObjectObject :: FunPtr (Ptr () -> IO (Ptr ())) -> Ptr () -> IO (Ptr ())
foreign import ccall "dynamic" dynNameObject :: FunPtr (CString -> IO (Ptr ())) -> CString -> IO (Ptr ())
foreign import ccall "dynamic" dynObjectName :: FunPtr (Ptr () -> CString -> IO (Ptr ())) -> Ptr () -> CString -> IO (Ptr ())
foreign import ccall "dynamic" dynObjectNameSet :: FunPtr (Ptr () -> CString -> Ptr () -> IO CInt) -> Ptr () -> CString -> Ptr () -> IO CInt
foreign import ccall "dynamic" dynCall :: FunPtr (Ptr () -> Ptr () -> Ptr () -> IO (Ptr ())) -> Ptr () -> Ptr () -> Ptr () -> IO (Ptr ())
foreign import ccall "dynamic" dynSize :: FunPtr (CSsize -> IO (Ptr ())) -> CSsize -> IO (Ptr ())
foreign import ccall "dynamic" dynIndexSet :: FunPtr (Ptr () -> CSsize -> Ptr () -> IO CInt) -> Ptr () -> CSsize -> Ptr () -> IO CInt
foreign import ccall "dynamic" dynText :: FunPtr (CString -> CSsize -> IO (Ptr ())) -> CString -> CSsize -> IO (Ptr ())
foreign import ccall "dynamic" dynTextBytes :: FunPtr (Ptr () -> Ptr CSsize -> IO CString) -> Ptr () -> Ptr CSsize -> IO CString

pyInitConfigCreate :: IO (Ptr InitConfig)
pyInitConfigCreate = dynConfig (symbol "PyInitConfig_Create")
pyInitConfigFree :: Ptr InitConfig -> IO ()
pyInitConfigFree = dynConfigUnit (symbol "PyInitConfig_Free")
pyInitConfigGetError :: Ptr InitConfig -> Ptr CString -> IO CInt
pyInitConfigGetError = dynConfigError (symbol "PyInitConfig_GetError")
pyInitConfigSetInt :: Ptr InitConfig -> CString -> Int64 -> IO CInt
pyInitConfigSetInt = dynConfigSetInt (symbol "PyInitConfig_SetInt")
pyInitConfigSetStr :: Ptr InitConfig -> CString -> CString -> IO CInt
pyInitConfigSetStr = dynConfigSetStr (symbol "PyInitConfig_SetStr")
pyInitConfigSetStrList :: Ptr InitConfig -> CString -> CSize -> Ptr CString -> IO CInt
pyInitConfigSetStrList = dynConfigSetList (symbol "PyInitConfig_SetStrList")
pyInitializeFromInitConfig :: Ptr InitConfig -> IO CInt
pyInitializeFromInitConfig = dynConfigInt (symbol "Py_InitializeFromInitConfig")
pyIsInitialized :: IO CInt
pyIsInitialized = dynInt (symbol "Py_IsInitialized")
pyFinalizeEx :: IO CInt
pyFinalizeEx = dynInt (symbol "Py_FinalizeEx")
pyGetVersion :: IO CString
pyGetVersion = dynString (symbol "Py_GetVersion")
pyIncRef :: Ptr () -> IO ()
pyIncRef = dynObjectUnit (symbol "Py_IncRef")
pyDecRef :: Ptr () -> IO ()
pyDecRef = dynObjectUnit (symbol "Py_DecRef")
pyImportModule :: CString -> IO (Ptr ())
pyImportModule = dynNameObject (symbol "PyImport_ImportModule")
pyObjectGetAttrString :: Ptr () -> CString -> IO (Ptr ())
pyObjectGetAttrString = dynObjectName (symbol "PyObject_GetAttrString")
pyDictSetItemString :: Ptr () -> CString -> Ptr () -> IO CInt
pyDictSetItemString = dynObjectNameSet (symbol "PyDict_SetItemString")
pyObjectCall :: Ptr () -> Ptr () -> Ptr () -> IO (Ptr ())
pyObjectCall = dynCall (symbol "PyObject_Call")
pyObjectStr :: Ptr () -> IO (Ptr ())
pyObjectStr = dynObjectObject (symbol "PyObject_Str")
pyTupleNew :: CSsize -> IO (Ptr ())
pyTupleNew = dynSize (symbol "PyTuple_New")
pyTupleSetItem :: Ptr () -> CSsize -> Ptr () -> IO CInt
pyTupleSetItem = dynIndexSet (symbol "PyTuple_SetItem")
pyUnicodeFromStringAndSize :: CString -> CSsize -> IO (Ptr ())
pyUnicodeFromStringAndSize = dynText (symbol "PyUnicode_FromStringAndSize")
pyUnicodeAsUTF8AndSize :: Ptr () -> Ptr CSsize -> IO CString
pyUnicodeAsUTF8AndSize = dynTextBytes (symbol "PyUnicode_AsUTF8AndSize")
pySysGetObject :: CString -> IO (Ptr ())
pySysGetObject = dynNameObject (symbol "PySys_GetObject")
pyListInsert :: Ptr () -> CSsize -> Ptr () -> IO CInt
pyListInsert = dynIndexSet (symbol "PyList_Insert")
pyErrGetRaisedException :: IO (Ptr ())
pyErrGetRaisedException = dynObject (symbol "PyErr_GetRaisedException")
pyErrClear :: IO ()
pyErrClear = dynUnit (symbol "PyErr_Clear")

-- | The runtime the binary was built for (the `lab_recorder_python` declaration and the
-- compiled-in headers) and the libpython it loaded.
data RuntimeIdentity = RuntimeIdentity
  { declaredEnvironment :: String
  , declaredBase :: String
  , declaredAbi :: String
  , declaredVersion :: String
  , compiledVersion :: String
  , loadedVersion :: String
  }

runtimeIdentity :: IO RuntimeIdentity
runtimeIdentity =
  RuntimeIdentity #{const_str AFPY_DECLARED_ENV} #{const_str AFPY_DECLARED_BASE} #{const_str AFPY_DECLARED_ABI}
    #{const_str AFPY_DECLARED_VERSION} #{const_str PY_VERSION}
    <$> (pyGetVersion >>= peekUtf8)

failure :: String -> IO a
failure = ioError . userError

withUtf8 :: String -> (CString -> IO a) -> IO a
withUtf8 = GF.withCString utf8

peekUtf8 :: CString -> IO String
peekUtf8 text = T.unpack . TE.decodeUtf8Lenient <$> B.packCString text

-- | The UTF-8 bytes of a str object, or Nothing with the Python error set.
utf8Bytes :: Ptr () -> IO (Maybe B.ByteString)
utf8Bytes object = alloca $ \sizePointer -> do
  text <- pyUnicodeAsUTF8AndSize object sizePointer
  if text == nullPtr
    then pure Nothing
    else peek sizePointer >>= \size -> Just <$> B.packCStringLen (text, fromIntegral size)

-- | The raised exception's text; clears the error indicator.
raisedError :: IO String
raisedError = do
  exception <- pyErrGetRaisedException
  text <- if exception == nullPtr then pure nullPtr else pyObjectStr exception
  message <- if text == nullPtr
    then pure "CPython call failed"
    else maybe "CPython exception conversion failed" (T.unpack . TE.decodeUtf8Lenient) <$> utf8Bytes text
  pyDecRef text
  pyDecRef exception
  pyErrClear
  pure message

checked :: IO (Ptr ()) -> IO PyObject
checked action = do
  object <- action
  if object == nullPtr then raisedError >>= failure else pure (PyObject object)

checkedStatus :: IO CInt -> IO ()
checkedStatus action = do
  status <- action
  when (status < 0) (raisedError >>= failure)

-- | Initializes the isolated interpreter with `home`, exactly `paths` as `sys.path` and no
-- bytecode writes, then imports `json`.
initialize :: FilePath -> [FilePath] -> IO ()
initialize home paths = do
  bound <- isCurrentThreadBound
  unless bound (failure "embedded CPython must be initialized on a bound thread")
  bracket pyInitConfigCreate pyInitConfigFree $ \config -> do
    when (config == nullPtr) (failure "Isolated CPython initialization failed")
    let step action = do
          status <- action
          when (status < 0) $ alloca $ \messagePointer -> do
            set <- pyInitConfigGetError config messagePointer
            message <- if set == 1 then peek messagePointer >>= peekUtf8 else pure "Isolated CPython initialization failed"
            failure message
    withUtf8 "write_bytecode" $ \name -> step (pyInitConfigSetInt config name 0)
    withUtf8 "home" $ \name -> withUtf8 home $ \value -> step (pyInitConfigSetStr config name value)
    withUtf8 "module_search_paths" $ \name -> withMany withUtf8 paths $ \texts ->
      withArrayLen texts $ \count array -> step (pyInitConfigSetStrList config name (fromIntegral count) array)
    step (pyInitializeFromInitConfig config)
  importModule "json" >>= release

finish :: IO ()
finish = do
  initialized <- pyIsInitialized
  when (initialized /= 0) (void pyFinalizeEx)

release :: PyObject -> IO ()
release (PyObject object) = pyDecRef object

importModule :: String -> IO PyObject
importModule name = withUtf8 name (checked . pyImportModule)

getAttr :: PyObject -> String -> IO PyObject
getAttr (PyObject object) name = withUtf8 name (checked . pyObjectGetAttrString object)

dictSetItem :: PyObject -> String -> PyObject -> IO ()
dictSetItem (PyObject dictionary) name (PyObject value) =
  withUtf8 name $ \key -> checkedStatus (pyDictSetItemString dictionary key value)

-- | Puts `path` first on `sys.path`.
insertPath :: FilePath -> IO ()
insertPath path = do
  list <- withUtf8 "path" pySysGetObject
  when (list == nullPtr) (failure "sys.path is unavailable")
  bracket (strObject (TE.encodeUtf8 (T.pack path))) release $ \(PyObject text) -> checkedStatus (pyListInsert list 0 text)

strObject :: B.ByteString -> IO PyObject
strObject bytes = B.useAsCStringLen bytes $ \(text, size) -> checked (pyUnicodeFromStringAndSize text (fromIntegral size))

-- | `json.loads` of UTF-8 JSON text.
fromJSONText :: B.ByteString -> IO PyObject
fromJSONText bytes = withJsonFunction "loads" $ \loads ->
  bracket (strObject bytes) release $ \argument -> call loads [argument] Nothing

-- | `json.dumps` of `object` as UTF-8.
toJSONText :: PyObject -> IO B.ByteString
toJSONText object = withJsonFunction "dumps" $ \dumps ->
  bracket (call dumps [object] Nothing) release $ \(PyObject text) ->
    utf8Bytes text >>= maybe (raisedError >>= failure) pure

withJsonFunction :: String -> (PyObject -> IO a) -> IO a
withJsonFunction name continuation =
  bracket (importModule "json") release $ \json -> bracket (getAttr json name) release continuation

-- | `function(*arguments, **keywords)`.
call :: PyObject -> [PyObject] -> Maybe PyObject -> IO PyObject
call (PyObject function) arguments keywords =
  bracket (checked (pyTupleNew (fromIntegral (length arguments)))) release $ \(PyObject tuple) -> do
    forM_ (zip [0 ..] arguments) $ \(index, PyObject argument) -> do
      pyIncRef argument
      checkedStatus (pyTupleSetItem tuple index argument)
    checked (pyObjectCall function tuple (maybe nullPtr (\(PyObject dictionary) -> dictionary) keywords))
