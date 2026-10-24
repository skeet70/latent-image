{-# LANGUAGE OverloadedStrings #-}
-- | Portable-checkpoint keys. A key covers what decides the guest state at a
-- saved boundary or whether that state loads: the staged model, engine,
-- profile and route-driving files, the Unicorn library bytes, the Python
-- interpreter and the route's own inputs. The runtime root, launcher code and
-- viewer files are outside it.
module Emulation.CheckpointKey
  ( FileRole (..)
  , fileRole
  , UnicornLibrary (..)
  , Interpreter (..)
  , GuestState
  , guestState
  , guestStateDigest
  , guestStateValue
  , CheckpointRoute (..)
  , RouteKey (..)
  , routeKey
  , routeKeyHex
  ) where

import Control.Monad (when)
import Data.Aeson (Value, object, (.=))
import qualified Data.Aeson.Key as K
import Data.List (sort)
import Emulation.Sha256 (Sha256, sha256, sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import System.Directory (canonicalizePath, doesDirectoryExist, listDirectory, pathIsSymbolicLink)
import System.FilePath ((</>), splitDirectories)

data FileRole = GuestInput | HostOnly deriving (Eq, Show)

-- | The role of a runtime-relative path. Only the staged emulator, firmware,
-- inputs, native and tools trees hold guest inputs; the launcher's validator
-- copy, `.tools/` and run outputs beside them do not.
fileRole :: FilePath -> FileRole
fileRole path = case splitDirectories path of
  components | "__pycache__" `elem` components -> HostOnly
  tree : rest | tree `elem` guestTrees, not (hostOnly tree rest) -> GuestInput
  _ -> HostOnly

guestTrees :: [FilePath]
guestTrees = ["emulator", "firmware", "inputs", "native", "tools"]

-- | The viewer's HTTP server, page and launcher, and the Rust sources and
-- tests; the engine runs as the staged `_native.abi3.so`.
hostOnly :: FilePath -> [FilePath] -> Bool
hostOnly "emulator" ["server.py"] = True
hostOnly "emulator" ("web" : _) = True
hostOnly "tools" ["start_setup_viewer.py"] = True
hostOnly "native" ("src" : _) = True
hostOnly "native" ("tests" : _) = True
hostOnly "native" [name] = name `elem` ["Cargo.toml", "Cargo.lock", "build.rs"]
hostOnly _ _ = False

newtype UnicornLibrary = UnicornLibrary FilePath

-- | The Python that executes the device models. An executable is named by its
-- canonical Nix store path, which fixes the interpreter build and its packages.
data Interpreter
  = InterpreterExecutable FilePath
  | EmbeddedInterpreter
      { embeddedEnvironment :: FilePath
      , embeddedBase :: FilePath
      , embeddedAbi :: String
      , embeddedVersion :: String
      }

data GuestState = GuestState
  { guestFiles :: [(FilePath, Sha256)]
  , guestUnicorn :: Sha256
  , guestInterpreter :: Interpreter
  }

guestState :: FilePath -> Interpreter -> UnicornLibrary -> IO GuestState
guestState runtime interpreter (UnicornLibrary library) = do
  paths <- sort . filter ((== GuestInput) . fileRole) . concat <$> mapM (treeFiles runtime) guestTrees
  files <- mapM (\path -> (,) path <$> sha256File (runtime </> path)) paths
  unicorn <- sha256File library
  named <- case interpreter of
    InterpreterExecutable executable -> InterpreterExecutable <$> canonicalizePath executable
    embedded -> pure embedded
  pure (GuestState files unicorn named)

-- | Files beneath RUNTIME/TREE as runtime-relative paths.
treeFiles :: FilePath -> FilePath -> IO [FilePath]
treeFiles runtime tree = do
  present <- doesDirectoryExist (runtime </> tree)
  if present then go tree else pure []
  where
    go relative = do
      let absolute = runtime </> relative
      linked <- pathIsSymbolicLink absolute
      when linked $ ioError (userError ("staged runtime must not contain symlinks: " ++ absolute))
      directory <- doesDirectoryExist absolute
      if directory
        then listDirectory absolute >>= fmap concat . mapM (go . (relative </>))
        else pure [relative]

interpreterValue :: Interpreter -> Value
interpreterValue (InterpreterExecutable executable) = object ["executable" .= executable]
interpreterValue (EmbeddedInterpreter environment base abi version) = object
  [ "embedded_environment" .= environment, "base_interpreter" .= base
  , "python_abi" .= abi, "version" .= version ]

guestStateDocument :: GuestState -> Value
guestStateDocument guest = object
  [ "schema" .= (1 :: Int)
  , "files" .= [[path, sha256Hex digest] | (path, digest) <- guestFiles guest]
  , "unicorn_library_sha256" .= sha256Hex (guestUnicorn guest)
  , "interpreter" .= interpreterValue (guestInterpreter guest)
  ]

guestStateDigest :: GuestState -> Sha256
guestStateDigest = sha256 . encodeCanonical . guestStateDocument

-- | The receipt summary: the digest, the file count, the Unicorn library and the interpreter.
guestStateValue :: GuestState -> Value
guestStateValue guest = object
  [ "sha256" .= sha256Hex (guestStateDigest guest)
  , "files" .= length (guestFiles guest)
  , "unicorn_library_sha256" .= sha256Hex (guestUnicorn guest)
  , "interpreter" .= interpreterValue (guestInterpreter guest)
  ]

-- | The boundary a checkpoint is saved at.
data CheckpointRoute
  = -- | RAM-push bridge after boot, first-start route and ff80 attach, card present.
    CardAttach
  | -- | The same boundary from the cardless bridge.
    CardlessAttach
  | -- | USB-toggle server at the shooting screen after the first-start route.
    ToggleRouteBoot
  deriving (Eq, Show)

routeName :: CheckpointRoute -> String
routeName CardAttach = "ram-push-attach"
routeName CardlessAttach = "cardless-ram-push-attach"
routeName ToggleRouteBoot = "usb-toggle-route-boot"

data RouteKey = RouteKey
  { routeKeyDigest :: Sha256
  , routeKeyDocument :: Value
  }

-- | The key of ROUTE's checkpoint for GUEST and the route's own INPUTS.
routeKey :: GuestState -> CheckpointRoute -> [(K.Key, Value)] -> RouteKey
routeKey guest route inputs = RouteKey (sha256 (encodeCanonical document)) document
  where
    document = object
      [ "schema" .= (2 :: Int)
      , "route" .= routeName route
      , "guest_state" .= sha256Hex (guestStateDigest guest)
      , "inputs" .= object inputs
      ]

routeKeyHex :: RouteKey -> String
routeKeyHex = sha256Hex . routeKeyDigest
