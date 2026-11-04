-- | Generated outputs are written only to paths that do not exist yet.
module Replacement.ModalRuntime.NewFile (writeNewFile) where

import Control.Exception (bracket)
import System.IO (hClose, hPutStr)
import System.Posix.IO (OpenFileFlags (..), OpenMode (WriteOnly), defaultFileFlags, fdToHandle, openFd)

-- | Creates `path` with `contents`; fails, leaving the file as it was, when `path` exists.
writeNewFile :: FilePath -> String -> IO ()
writeNewFile path contents =
  bracket (openFd path WriteOnly defaultFileFlags {creat = Just 0o644, exclusive = True} >>= fdToHandle) hClose
    (`hPutStr` contents)
