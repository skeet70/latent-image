{-# LANGUAGE OverloadedStrings #-}

-- | Stock against the fixed-bank kernel startup bodies in both domains, with mutants.
-- Takes --runner, --loader-{a,b}, --carrier-{a,b}-PIECE for each retained extent,
-- --bank-{a,b}, --results and an optional --cache-root.
module Main (main) where

import Control.Monad (forM_, unless)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import Emulation.CachedNativeBatch (defaultNativeCacheRoot)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import FujiAF.Host (failH)
import FujiAF.StorageBudget (checkStorageBudget)
import Replacement.RtosKernelInit.BankSuite
import Replacement.RtosOwnFirst.Regions (Domain (..), domainLabel)
import System.Directory (createDirectoryIfMissing, makeAbsolute)
import System.Environment (getArgs, lookupEnv)
import System.Exit (exitFailure)
import System.FilePath (takeDirectory, (</>))

bankOrigin :: Word32
bankOrigin = 0x00ba_0020

-- | Extent bases of the carrier pieces, A then B.
carrierBases :: [(String, Word32, Word32)]
carrierBases =
  [ ("start", 0x40430, 0xd0430)
  , ("tick", 0x40b5c, 0xd0b5c)
  , ("core", 0x43a58, 0xd3a0c)
  , ("tasks", 0x43e18, 0xd3dd4)
  , ("autostart", 0x43f9c, 0xd3f58)
  , ("semaphores", 0x47580, 0xd7540)
  , ("event_flags", 0x48044, 0xd8004)
  , ("data_queues", 0x48e38, 0xd8df8)
  , ("mailboxes", 0x4a9bc, 0xda97c)
  , ("fixed_pools", 0x4b38c, 0xdb34c)
  , ("objects", 0x4cea0, 0xdce60)
  ]

-- | Offsets into each domain's bank body.
mutants :: Domain -> [Mutant]
mutants _ = []

main :: IO ()
main = do
  runner <- required "--runner" >>= makeAbsolute
  cacheRoot <- maybe defaultNativeCacheRoot id <$> optional "--cache-root"
  resultsPath <- required "--results"
  batchRoot <- do
    work <- lookupEnv "XM1_RUN_WORK"
    pure $ case work of
      Just directory | not (null directory) -> directory </> "kernel-bank"
      _ -> takeDirectory resultsPath </> "batches"
  loaderA <- required "--loader-a"
  loaderB <- required "--loader-b"
  digestA <- sha256File loaderA
  digestB <- sha256File loaderB
  unless (sha256Hex digestA == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (failH (loaderA ++ " is not the pinned X-M1 1.50 loader"))
  unless (sha256Hex digestB == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (failH (loaderB ++ " is not the pinned X-M1 1.50 loader"))
  stockA <- Image 0x40000 <$> B.readFile loaderA
  stockB <- Image 0xd0000 <$> B.readFile loaderB
  ownedA <- ownedFor DomainA
  ownedB <- ownedFor DomainB
  checkStorageBudget (takeDirectory resultsPath)
  createDirectoryIfMissing True cacheRoot
  createDirectoryIfMissing True batchRoot
  createDirectoryIfMissing True (takeDirectory resultsPath)
  runA <- runBankSuite runner cacheRoot batchRoot DomainA stockA ownedA (mutants DomainA)
  runB <- runBankSuite runner cacheRoot batchRoot DomainB stockB ownedB (mutants DomainB)
  let runs = [runA, runB]
      failures =
        [(domainLabel (suiteDomain run), name, outcomeText result) | run <- runs, (name, result) <- suiteCases run, result /= Right []]
          ++ [(domainLabel (suiteDomain run), "mutant " ++ name, message) | run <- runs, (name, Left message) <- suiteMutants run]
      document =
        object
          [ "schema" .= (1 :: Int)
          , "comparisons" .= sum [length (suiteCases run) | run <- runs]
          , "mutants" .= sum [length (suiteMutants run) | run <- runs]
          , "failures" .= length failures
          , "inputs" .=
              object
                [ "loader_a_sha256" .= sha256Hex digestA
                , "loader_b_sha256" .= sha256Hex digestB
                , "bank_a_sha256" .= sha256Hex (sha256 (imageBytes (ownedBank ownedA)))
                , "bank_b_sha256" .= sha256Hex (sha256 (imageBytes (ownedBank ownedB)))
                ]
          , "cases" .=
              [ object ["domain" .= domainLabel (suiteDomain run), "case" .= name, "result" .= outcomeText result]
              | run <- runs
              , (name, result) <- suiteCases run
              ]
          , "mutant_results" .=
              [ object ["domain" .= domainLabel (suiteDomain run), "mutant" .= name, "detected" .= either (const False) (const True) result, "differences" .= either pure id result]
              | run <- runs
              , (name, result) <- suiteMutants run
              ]
          , "reuse" .=
              [ object ["domain" .= domainLabel (suiteDomain run), "stock" .= show stock, "owned" .= show owned, "mutants" .= show mutated]
              | run <- runs
              , let (stock, owned, mutated) = suiteReuse run
              ]
          ]
  BL.writeFile resultsPath (encode document)
  forM_ failures $ \(domain, name, message) -> putStrLn (domain ++ " " ++ name ++ ": " ++ message)
  putStrLn (show (sum [length (suiteCases run) | run <- runs]) ++ " comparisons, " ++ show (sum [length (suiteMutants run) | run <- runs]) ++ " mutants, " ++ show (length failures) ++ " failures")
  unless (null failures) exitFailure
  where
    outcomeText (Left message) = "failed: " ++ message
    outcomeText (Right []) = "ok"
    outcomeText (Right differences) = "failed: " ++ unwords differences
    ownedFor domain = do
      let label = domainLabel domain
      pieces <- mapM (\(name, a, b) -> Image (if domain == DomainA then a else b) <$> (required ("--carrier-" ++ label ++ "-" ++ name) >>= B.readFile)) carrierBases
      bank <- Image bankOrigin <$> (required ("--bank-" ++ label) >>= B.readFile)
      pure (Owned pieces bank)

required :: String -> IO FilePath
required flag = optional flag >>= maybe (failH (flag ++ " is required")) pure

-- | The last occurrence wins, so a command line overrides Bazel defaults.
optional :: String -> IO (Maybe FilePath)
optional flag = do
  arguments <- getArgs
  pure $ case [value | (name, value) <- zip arguments (drop 1 arguments), name == flag] of
    [] -> Nothing
    values -> Just (last values)
