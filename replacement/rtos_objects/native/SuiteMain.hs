{-# LANGUAGE OverloadedStrings #-}

-- | Runs the F2 family cases in both entry domains against the stock loaders and the owned
-- stage bins, through the cached native batch, and writes a terminal JSON.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (object, (.=), encode)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.FilePath ((</>))
import Replacement.RtosObjects.Layout (parseFacts)
import Replacement.RtosObjects.Suite (FamilyRun (..), Image (..), extents, runFamily)
import Replacement.RtosOwnFirst.Regions (Domain (..))

main :: IO ()
main = do
  [runner, cacheRoot, layoutPath, loaderA, loaderB, ownedA, ownedB, resultsPath] <- getArgs
  facts <- either fail pure . parseFacts =<< B.readFile layoutPath
  loaderABytes <- B.readFile loaderA
  loaderBBytes <- B.readFile loaderB
  ownedABytes <- B.readFile ownedA
  ownedBBytes <- B.readFile ownedB
  createDirectoryIfMissing True cacheRoot
  let stockA = Image 0x40000 loaderABytes
      stockB = Image 0xd0000 loaderBBytes
      ownedAImage = Image (minimum (map fst (extents DomainA))) ownedABytes
      ownedBImage = Image (minimum (map fst (extents DomainB))) ownedBBytes
  runA <- runFamily runner cacheRoot cacheRoot facts DomainA stockA ownedAImage
  runB <- runFamily runner cacheRoot cacheRoot facts DomainB stockB ownedBImage
  let runs = [runA, runB]
      failures =
        [(name, message) | run <- runs, (name, Left message) <- familyCases run]
          ++ [(name, unlines differences) | run <- runs, (name, Right differences) <- familyCases run, not (null differences)]
      cases =
        [ object
            [ "domain" .= show (familyDomain run)
            , "case" .= name
            , "result" .= either ("failed: " ++) (const "ok") result
            ]
        | run <- runs
        , (name, result) <- familyCases run
        ]
      reuse =
        [ object ["domain" .= show (familyDomain run), "stock" .= show (fst (familyReuse run)), "owned" .= show (snd (familyReuse run))]
        | run <- runs
        ]
      document = object ["cases" .= cases, "reuse" .= reuse, "failures" .= length failures]
  BL.writeFile resultsPath (encode document)
  forM_ runs $ \run -> forM_ (familyCases run) $ \(name, result) ->
    putStrLn (show (familyDomain run) ++ " " ++ name ++ ": " ++ either ("failed: " ++) (const "ok") result)
  forM_ failures $ \(name, message) -> putStrLn (name ++ ": " ++ message)
  unless (null failures) exitFailure
