{-# LANGUAGE OverloadedStrings #-}
-- | The embedded interpreter against its declared runtime: path isolation, JSON exchange, calls
-- and error text.
module Main (main) where

import Control.Concurrent (runInBoundThread)
import Control.Exception (IOException, bracket, try)
import Control.Monad (unless)
import qualified Data.Aeson as Aeson
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.List (isInfixOf)
import Emulation.EmbeddedPython (RuntimeIdentity (..))
import qualified Emulation.EmbeddedPython as Py
import System.Exit (exitFailure)
import System.FilePath ((</>))

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

evaluate :: String -> IO C.ByteString
evaluate expression =
  bracket (Py.importModule "builtins") Py.release $ \builtins ->
    bracket (Py.getAttr builtins "eval") Py.release $ \eval ->
      bracket (Py.fromJSONText (BL.toStrict (Aeson.encode expression))) Py.release $ \source ->
        bracket (Py.fromJSONText "{}") Py.release $ \globals ->
          bracket (Py.call eval [source, globals] Nothing) Py.release Py.toJSONText

main :: IO ()
main = runInBoundThread $ do
  identity <- Py.runtimeIdentity
  let library = declaredEnvironment identity </> "lib" </> declaredAbi identity
      paths = [library, library </> "lib-dynload", library </> "site-packages"]
  Py.initialize (declaredEnvironment identity) paths
  searchPath <- evaluate "__import__('sys').path"
  bytecode <- evaluate "__import__('sys').dont_write_bytecode"
  Py.insertPath "/first"
  inserted <- evaluate "__import__('sys').path[0]"
  roundTrip <- bracket (Py.fromJSONText "{\"a\":[1,2.5,\"x\",null,true]}") Py.release Py.toJSONText
  keywords <- bracket (Py.importModule "builtins") Py.release $ \builtins ->
    bracket (Py.getAttr builtins "dict") Py.release $ \dict ->
      bracket (Py.fromJSONText "{\"b\":2}") Py.release $ \options ->
        bracket (Py.call dict [] (Just options)) Py.release $ \made -> do
          bracket (Py.fromJSONText "3") Py.release (Py.dictSetItem made "c")
          Py.toJSONText made
  missing <- try (Py.importModule "no_such_module_for_embedding_test") :: IO (Either IOException Py.PyObject)
  afterError <- evaluate "1 + 1"
  results <- sequence
    [ expect "the loaded interpreter is the declared version" $
        takeWhile (/= ' ') (loadedVersion identity) == declaredVersion identity
          && compiledVersion identity == declaredVersion identity
    , expect "sys.path is exactly the configured search paths" $
        Aeson.decodeStrict searchPath == Just paths
    , expect "bytecode writes are off" (bytecode == "true")
    , expect "an inserted path comes first" (inserted == "\"/first\"")
    , expect "JSON round-trips through Python objects" (roundTrip == "{\"a\": [1, 2.5, \"x\", null, true]}")
    , expect "a call receives its keywords and a dictionary item is set" (keywords == "{\"b\": 2, \"c\": 3}")
    , expect "a failing import raises its exception text" $
        either (("no_such_module_for_embedding_test" `isInfixOf`) . show) (const False) missing
    , expect "the interpreter keeps working after an error" (afterError == "2")
    ]
  Py.finish
  unless (and results) exitFailure
