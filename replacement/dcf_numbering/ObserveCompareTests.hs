{-# LANGUAGE OverloadedStrings #-}

-- | dcf-numbering-observe compare on synthetic observations.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import System.Directory (createDirectory, doesPathExist, getTemporaryDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.Process (readProcessWithExitCode)

task :: Value
task = object ["kernel" .= ("a" :: T.Text), "task_id" .= (3 :: Int), "kind" .= ("available" :: T.Text)]

hit :: T.Text -> T.Text -> T.Text -> T.Text -> [(T.Text, T.Text)] -> [T.Text] -> Value
hit site address lr sp registers captured =
  object
    [ "site" .= site, "address" .= address, "lr" .= lr, "sp" .= sp, "task" .= task, "core" .= (0 :: Int)
    , "registers" .= object [K.fromText r .= v | (r, v) <- registers]
    , "observation" .= object ["reads" .= [object ["bytes" .= b] | b <- captured]]
    ]

returned :: Int -> T.Text -> [(T.Text, T.Text)] -> [T.Text] -> Value
returned site address registers captured =
  object
    [ "site" .= ("call-return" :: T.Text), "address" .= address, "lr" .= address, "task" .= task, "core" .= (0 :: Int)
    , "registers" .= object [K.fromText r .= v | (r, v) <- registers]
    , "observation" .= object ["site" .= site, "reads" .= [object ["bytes" .= b] | b <- captured]]
    ]

-- | One role's observation of an interface read_numbers call from 0xed5908: a copy of five bytes
-- into the caller's frame, a stored profile pair, an internal last_used entry for stock, and the
-- returned pair; plus the fetches of one step.
data Role = Role
  { copyDestination :: T.Text, copySource :: T.Text, copyBytes :: T.Text, storedFile :: T.Text
  , internalEntry :: Bool, outputFile :: T.Text, fetches :: [(T.Text, Int)] }

stockRole :: Role
stockRole = Role "0xa19d00" "0x781f60" "5c4443494d5c0000" "0x1c" True "190c" [("0x716330", 4)]

hits :: Role -> [Value]
hits role =
  [hit "dcf:read_numbers" "0x70fb68" "0xed5908" "0xa19e00" [("r0", "0xa19f00"), ("r1", "0xa19f04")] []]
    ++ [hit "dcf:last_used" "0x716324" "0x70fbe0" "0xa19d80" [("r0", "0x43"), ("r1", "0x0"), ("r2", "0xa19d90"), ("r3", "0xa19d94")] [] | internalEntry role]
    ++ [ hit "call:copy_bytes" "0x716400" "0x70fbe0" "0xa19cf0" [("r0", copyDestination role), ("r1", copySource role), ("r2", "0x5")] [copyBytes role]
       , hit "call:store_profile_file" "0x716410" "0x70fbe0" "0xa19cf0" [("r0", "0x43"), ("r1", "0x0"), ("r2", storedFile role)] []
       , returned 0x70fb68 "0xed5908" [("r0", "0x0")] ["c303", outputFile role]
       ]

writeRole :: FilePath -> T.Text -> Role -> IO ()
writeRole directory name role = do
  createDirectory directory
  BL.writeFile (directory </> "hits.jsonl") (BL.concat [A.encode h <> "\n" | h <- hits role])
  BL.writeFile (directory </> "steps.json") . A.encode $
    [object ["step" .= (1 :: Int), "watch" .= object ["violation" .= Null, "fetches_by_pc" .= object [K.fromText pc .= n | (pc, n) <- fetches role], "writes" .= ([] :: [Value])], "state" .= object ["context" .= ("0x0" :: T.Text)], "screen" .= object ["sha256" .= ("screen" :: T.Text)]]]
  BL.writeFile (directory </> "receipt.json") . A.encode $
    object
      [ "role" .= name, "outcome" .= object ["status" .= ("completed" :: T.Text)], "reset_segments" .= ([] :: [Value])
      , "inputs" .= object ["update" .= object ["sha256" .= name]], "attached_screen" .= object ["sha256" .= ("screen" :: T.Text)]
      , "attached_state" .= object ["context" .= ("0x0" :: T.Text)] ]

verdictOf :: FilePath -> IO Value
verdictOf path = do
  present <- doesPathExist path
  bytes <- if present then BL.readFile path else pure ""
  pure $ case A.decode bytes of
    Just (Object fields) | Just verdict <- KM.lookup "verdict" fields -> verdict
    _ -> Null

main :: IO ()
main = do
  arguments <- getArgs
  observe <- case arguments of
    [tool] -> pure tool
    _ -> fail "usage: observe-compare-tests DCF_NUMBERING_OBSERVE"
  base <- maybe getTemporaryDirectory pure =<< lookupEnv "TEST_TMPDIR"
  let root = base </> "dcf-observe-compare"
      symbols = root </> "owned.symbols"
      owned = stockRole {internalEntry = False}
  createDirectory root
  writeFile symbols "0070fb68 00000004 T dcf_entry_read_numbers\n00716328 000001a4 t create_folder\n0071d450 00000000 A dcf_translate_status\n"
  writeRole (root </> "stock") "stock" stockRole
  let cases =
        [ ("frame-and-constant-addresses", owned {copyDestination = "0xa19d40", copySource = "0x70f82c", copyBytes = "5c4443494d5cffff"}, ExitSuccess, "equal")
        , ("copied-byte", owned {copyBytes = "5c4443494e5c0000"}, ExitFailure 1, "different")
        , ("context-destination", owned {copyDestination = "0xa00010"}, ExitFailure 1, "different")
        , ("stored-file", owned {storedFile = "0x1d"}, ExitFailure 1, "different")
        , ("returned-file", owned {outputFile = "1a0c"}, ExitFailure 1, "different")
        , ("fetch-outside-owned-functions", owned {fetches = [("0x716330", 4), ("0x716500", 1)]}, ExitFailure 1, "different")
        ]
  failures <- fmap concat . sequence $
    [ do
        writeRole (root </> name) "owned" role
        let output = root </> (name ++ ".json")
        (code, _, err) <- readProcessWithExitCode observe ["compare", root </> "stock", root </> name, symbols, output] ""
        verdict <- verdictOf output
        pure [name ++ ": exit " ++ show code ++ ", verdict " ++ show verdict ++ "\n" ++ err | code /= expectedCode || verdict /= String expected]
    | (name, role, expectedCode, expected) <- cases
    ]
  forM_ failures putStrLn
  unless (null failures) exitFailure
