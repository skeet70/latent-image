{-# LANGUAGE OverloadedStrings #-}

-- | file-service-observe compare on synthetic observations: verdicts, and a heap bound far below
-- the size of the hits it reads.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Numeric (showHex)
import System.Directory (createDirectory, doesPathExist, getTemporaryDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.Process (readProcessWithExitCode)

-- | Divide calls from outside the component, each carrying a 2 KiB snapshot: about 85 MB per role.
noiseHits :: Int
noiseHits = 40000

noise :: Int -> Value
noise i =
  object
    [ "site" .= ("backend:divide" :: T.Text), "address" .= ("0x4d978" :: T.Text), "lr" .= ("0x4da7c" :: T.Text)
    , "registers" .= object ["r0" .= T.pack (show i), "r1" .= ("0x4" :: T.Text)]
    , "memory" .= [T.replicate 2048 "0"], "observation" .= object ["reads" .= ([] :: [Value]), "site" .= (0x4d978 :: Int)]
    ]

task :: Value
task = object ["kernel" .= ("a" :: T.Text), "task_id" .= (28 :: Int), "kind" .= ("available" :: T.Text)]

backendEntry :: T.Text -> T.Text -> T.Text -> [(T.Text, T.Text)] -> [T.Text] -> Value
backendEntry name address lr registers pointed =
  object
    [ "site" .= ("backend:" <> name), "address" .= address, "lr" .= lr, "task" .= task
    , "registers" .= object [K.fromText r .= v | (r, v) <- registers]
    , "observation" .= object ["reads" .= [object ["bytes" .= b] | b <- pointed]]
    ]

backendReturn :: Int -> T.Text -> T.Text -> Value
backendReturn site address r0 =
  object
    [ "site" .= ("call-return" :: T.Text), "address" .= address, "lr" .= address, "task" .= task
    , "registers" .= object ["r0" .= r0], "observation" .= object ["site" .= site, "reads" .= ([] :: [Value])]
    ]

-- | One role's component calls: fs_close of a handle, local_record into the caller's frame,
-- entry_update of an 11-byte name, and a divide at the role's own call site, whose enclosing
-- function's return the bridge also records.
data Role = Role {closeHandle :: T.Text, localRecordResult :: T.Text, entryName :: T.Text, divideSite :: Int, divisor :: T.Text}

stockRole :: Role
stockRole = Role "0x5" "0xa19d6c" "44534346333039374a50470018547200" 0x712800 "0x200"

divideHit :: T.Text -> Int -> [(T.Text, T.Text)] -> Value
divideHit kind site registers =
  object
    [ "site" .= (kind <> ":" <> hex site), "address" .= hex site, "lr" .= ("0x711830" :: T.Text), "task" .= task
    , "registers" .= object [K.fromText r .= v | (r, v) <- registers], "observation" .= object ["reads" .= ([] :: [Value])]
    ]
  where
    hex = T.pack . ("0x" ++) . flip showHex ""

calls :: Role -> [Value]
calls role =
  [ backendEntry "fs_close" "0x6ffff4" "0x711800" [("r0", closeHandle role)] []
  , backendReturn 0x6ffff4 "0x711800" "0x0"
  , backendEntry "local_record" "0x7111c4" "0x711810" [] []
  , backendReturn 0x7111c4 "0x711810" (localRecordResult role)
  , backendEntry "entry_update" "0x714940" "0x711820" [("r0", "0x4"), ("r2", "0x1"), ("r3", "0x1")] [entryName role, "0000000001000000"]
  , backendReturn 0x714940 "0x711820" "0x0"
  , divideHit "divide-call" (divideSite role) [("r0", "0x1fff"), ("r1", divisor role)]
  , divideHit "divide-return" (divideSite role + 4) [("r0", "0xf")]
  , backendReturn (divideSite role) "0x711830" "0x0"
  , backendReturn (divideSite role + 4) "0x711830" "0x0"
  ]

writeRole :: FilePath -> T.Text -> Int -> Role -> IO ()
writeRole directory name noisy role = do
  createDirectory directory
  let (before, after) = splitAt (noisy `div` 2) (map noise [0 .. noisy - 1])
  BL.writeFile (directory </> "hits.jsonl") (BL.concat [A.encode hit <> "\n" | hit <- before ++ calls role ++ after])
  BL.writeFile (directory </> "steps.json") "[]"
  BL.writeFile (directory </> "receipt.json") . A.encode $
    object
      [ "role" .= name, "outcome" .= object ["status" .= ("completed" :: T.Text)], "reset_segments" .= ([] :: [Value])
      , "inputs" .= object ["update" .= object ["sha256" .= name]], "attached_screen" .= object ["sha256" .= ("screen" :: T.Text)]
      ]

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
    _ -> fail "usage: observe-compare-tests FILE_SERVICE_OBSERVE"
  base <- maybe getTemporaryDirectory pure =<< lookupEnv "TEST_TMPDIR"
  let root = base </> "observe-compare"
      symbols = root </> "owned.symbols"
  createDirectory root
  writeFile symbols "00ba0020 0 NOTYPE __fixed_body_start\n00bc0000 0 NOTYPE __fixed_body_end\n"
  writeRole (root </> "stock") "stock" noiseHits stockRole
  let cases =
        [ ("stack-and-unread-name-bytes", noiseHits, stockRole {localRecordResult = "0xa19d40", entryName = "44534346333039374a50470000000000", divideSite = 0xba1000}, ExitSuccess, "equal")
        , ("close-handle", 0, stockRole {closeHandle = "0x6", divideSite = 0xba1000}, ExitFailure 1, "different")
        , ("read-name-byte", 0, stockRole {entryName = "44534346333039384a50470018547200", divideSite = 0xba1000}, ExitFailure 1, "different")
        , ("divisor", 0, stockRole {divisor = "0x400", divideSite = 0xba1000}, ExitFailure 1, "different")
        ]
  failures <- fmap concat . sequence $
    [ do
        writeRole (root </> name) "owned" noisy role
        let output = root </> (name ++ ".json")
        (code, _, err) <- readProcessWithExitCode observe ["compare", root </> "stock", root </> name, symbols, output, "+RTS", "-M48m", "-RTS"] ""
        verdict <- verdictOf output
        pure [name ++ ": exit " ++ show code ++ ", verdict " ++ show verdict ++ "\n" ++ err | code /= expectedCode || verdict /= String expected]
    | (name, noisy, role, expectedCode, expected) <- cases
    ]
  forM_ failures putStrLn
  unless (null failures) exitFailure
