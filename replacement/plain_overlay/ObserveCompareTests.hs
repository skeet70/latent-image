{-# LANGUAGE OverloadedStrings #-}

-- | plain-overlay-observe compare on synthetic observations of the save file machine unit.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import System.Directory (createDirectory, doesPathExist, getTemporaryDirectory)
import System.Environment (getArgs, lookupEnv)
import System.Exit (ExitCode (..), exitFailure)
import System.FilePath ((</>))
import System.Process (readProcessWithExitCode)

-- | One role's observation: its entry hits, the fetches in the extent, overlay residency and the
-- DRAM chunk digests of its one step.
data Role = Role {roleHits :: [Value], roleFetches :: [(T.Text, Int)], roleResident :: Bool, roleChunk :: T.Text}

stockRole :: Role
stockRole = Role [] [] True "c0"

task :: Value
task = object ["kernel" .= ("a" :: T.Text), "task_id" .= (3 :: Int), "kind" .= ("available" :: T.Text)]

-- | An entry hit at SITE (`address`) from image 5 with r0..r3.
entryAt :: T.Text -> T.Text -> [T.Text] -> Value
entryAt site address values =
  object
    [ "site" .= site, "address" .= address, "lr" .= ("0xe51aec" :: T.Text), "core" .= (0 :: Int), "task" .= task
    , "registers" .= object [K.fromText ("r" <> T.pack (show i)) .= v | (i, v) <- zip [0 :: Int ..] values]
    ]

-- | The return to image 5 of the call entered at SITE, with r0 and the captured output bytes.
returnFrom :: T.Text -> T.Text -> [T.Text] -> Value
returnFrom site r0 captured =
  object
    [ "site" .= ("call-return" :: T.Text), "address" .= ("0xe51aec" :: T.Text), "lr" .= ("0xe51aec" :: T.Text), "core" .= (0 :: Int), "task" .= task
    , "registers" .= object ["r0" .= r0]
    , "observation" .= object ["site" .= site, "reads" .= [object ["bytes" .= b] | b <- captured]]
    ]

-- | An entry of `begin` (three register arguments) with R1 and R3.
entryHit :: T.Text -> T.Text -> Value
entryHit r1 r3 = entryAt "entry:save_file_machine:begin" "0x11d5600" ["0x1", r1, "0xe4e5dc", r3]

-- | `begin` returning with the word it leaves at descriptor + 0x14c.
beginReturn :: T.Text -> Value
beginReturn appended = returnFrom "0x11d5600" "0x0" [appended]

reachedRole :: Role
reachedRole = stockRole {roleHits = [entryHit "0x14e0000" "0x0", beginReturn "00000000"], roleFetches = [("0x11d5604", 3)]}

-- | `write_exif_header` returning 4, the first eight bytes of its output window WRITTEN, then its
-- IFD0 identity and +0x154 words.
headerRole :: T.Text -> Role
headerRole written =
  stockRole
    { roleHits =
        [ entryAt "entry:exif_writer:write_exif_header" "0x11db098" ["0x400000", "0x500000", "0x501000", "0x510000"]
        , returnFrom "0x11db098" "0x4" [written, T.replicate 224 "0", T.replicate 16 "0"]
        ]
    , roleFetches = [("0x11db098", 1)]
    }

writeRole :: FilePath -> Value -> T.Text -> Role -> IO ()
writeRole directory unit name role = do
  createDirectory directory
  let residency = [object ["overlay" .= (46 :: Int), "resident" .= roleResident role]]
      memory point = BL.writeFile (directory </> ("memory-" ++ point ++ ".json")) (A.encode (object ["chunk_bytes" .= (1048576 :: Int), "masked" .= [[1 :: Int, 2]], "chunks" .= ["a0" :: T.Text, roleChunk role]]))
  BL.writeFile (directory </> "hits.jsonl") (BL.concat [A.encode h <> "\n" | h <- roleHits role])
  memory "attached"
  memory "1"
  BL.writeFile (directory </> "steps.json") . A.encode $
    [ object
        [ "step" .= (1 :: Int), "watch" .= object ["violation" .= Null, "fetches_by_pc" .= object [K.fromText pc .= n | (pc, n) <- roleFetches role], "writes" .= ([] :: [Value])]
        , "screen" .= object ["sha256" .= ("screen" :: T.Text)], "residency" .= residency, "memory" .= roleChunk role ] ]
  BL.writeFile (directory </> "receipt.json") . A.encode $
    object
      [ "role" .= name, "outcome" .= object ["status" .= ("completed" :: T.Text)], "reset_segments" .= ([] :: [Value]), "units" .= [unit]
      , "inputs" .= object ["update" .= object ["sha256" .= name], "plain" .= object ["sha256" .= name]]
      , "attached_screen" .= object ["sha256" .= ("screen" :: T.Text)], "attached_residency" .= residency, "attached_memory" .= roleChunk role ]

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
  (observe, manifest, elf, exifManifest, exifElf) <- case arguments of
    [tool, unitPath, elfPath, exifPath, exifElfPath] -> pure (tool, unitPath, elfPath, exifPath, exifElfPath)
    _ -> fail "usage: observe-compare-tests PLAIN_OVERLAY_OBSERVE SAVE_UNIT SAVE_ELF EXIF_UNIT EXIF_ELF"
  unit <- B.readFile manifest >>= either fail pure . A.eitherDecodeStrict'
  exifUnit <- B.readFile exifManifest >>= either fail pure . A.eitherDecodeStrict'
  base <- maybe getTemporaryDirectory pure =<< lookupEnv "TEST_TMPDIR"
  let root = base </> "plain-overlay-observe-compare"
  createDirectory root
  writeRole (root </> "stock") unit "stock" stockRole
  writeRole (root </> "stock-reached") unit "stock" reachedRole
  writeRole (root </> "stock-header") exifUnit "stock" (headerRole "ffd8ffe1cdcdcdcd")
  let ownedReached = reachedRole {roleFetches = [("0x11d5600", 1)]}
      unitOf name = if "header" `T.isInfixOf` T.pack name then exifUnit else unit
      cases =
        [ ("unreached", "stock", "owned", stockRole, [elf], ExitSuccess, "equal")
        , ("fetch-in-owned-function", "stock", "owned", stockRole {roleFetches = [("0x11d5600", 1)]}, [elf], ExitSuccess, "equal")
        , ("fetch-outside-owned-functions", "stock", "owned", stockRole {roleFetches = [("0x11d5d44", 1)]}, [elf], ExitFailure 1, "different")
        , ("entry-reached", "stock", "owned", stockRole {roleHits = [entryHit "0x14e0000" "0x0"]}, [elf], ExitFailure 1, "different")
        , ("not-resident", "stock", "owned", stockRole {roleResident = False}, [elf], ExitFailure 1, "different")
        , ("unreached-memory-chunk", "stock", "owned", stockRole {roleChunk = "c1"}, [elf], ExitFailure 1, "different")
        , ("stock-rerun", "stock", "stock", stockRole, [], ExitSuccess, "equal")
        , ("reached-equal", "stock-reached", "owned", ownedReached, [elf], ExitSuccess, "equal")
        , ("reached-memory-chunk", "stock-reached", "owned", ownedReached {roleChunk = "c1"}, [elf], ExitSuccess, "equal")
        , ("reached-unread-register", "stock-reached", "owned", ownedReached {roleHits = [entryHit "0x14e0000" "0x5", beginReturn "00000000"]}, [elf], ExitSuccess, "equal")
        , ("reached-argument", "stock-reached", "owned", ownedReached {roleHits = [entryHit "0x14e0004" "0x0", beginReturn "00000000"]}, [elf], ExitFailure 1, "different")
        , ("reached-output-written-wrong", "stock-reached", "owned", ownedReached {roleHits = [entryHit "0x14e0000" "0x0", beginReturn "01000000"]}, [elf], ExitFailure 1, "different")
        , ("header-past-result", "stock-header", "owned", headerRole "ffd8ffe1aaaaaaaa", [exifElf], ExitSuccess, "equal")
        , ("header-within-result", "stock-header", "owned", headerRole "ffd8ffe0cdcdcdcd", [exifElf], ExitFailure 1, "different")
        ]
  failures <- fmap concat . sequence $
    [ do
        writeRole (root </> name) (unitOf name) role observation
        let output = root </> (name ++ ".json")
        (code, _, err) <- readProcessWithExitCode observe (["compare", root </> reference, root </> name, output] ++ elves) ""
        verdict <- verdictOf output
        pure [name ++ ": exit " ++ show code ++ ", verdict " ++ show verdict ++ "\n" ++ err | code /= expectedCode || verdict /= String expected]
    | (name, reference, role, observation, elves, expectedCode, expected) <- cases
    ]
  forM_ failures putStrLn
  unless (null failures) exitFailure
