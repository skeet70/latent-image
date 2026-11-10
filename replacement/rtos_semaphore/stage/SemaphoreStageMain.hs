{-# LANGUAGE OverloadedStrings #-}
-- | Stages the seven owned semaphore carriers at their stock extents over the qualified
-- scheduler/own-first candidate.
module Main (main) where

import Control.Monad (forM)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.Host as H
import Replacement.RtosOwnFirst.Regions
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.Cutover
import Replacement.RtosScheduler.CutoverPins
import Replacement.RtosSemaphore.Stage
import System.Directory (createDirectory, doesPathExist)
import System.Environment (getArgs)
import System.FilePath ((</>))

data InputPaths = InputPaths Group FilePath FilePath FilePath FilePath FilePath

splitInputs :: [Group] -> [String] -> Either String ([InputPaths], [String])
splitInputs [] args = Right ([], args)
splitInputs (group : groups) (metadata : a : as : b : bs : rest) = do
  (inputs, args) <- splitInputs groups rest
  pure (InputPaths group metadata a as b bs : inputs, args)
splitInputs _ _ = Left "expected metadata, A bin/symbols and B bin/symbols for each group"

addressText :: GuestAddress -> String
addressText (GuestAddress value) = hexText value

digestOf :: B.ByteString -> String
digestOf = SHA.sha256Hex . SHA.sha256

readInputs :: [String] -> IO ([GroupInput], [FilePath])
readInputs arguments = do
  (paths, rest) <- either H.failH pure (splitInputs [minBound .. maxBound] arguments)
  inputs <- forM paths $ \(InputPaths group metadata a as b bs) -> do
    regionBytes <- B.readFile metadata
    linkedA <- S.Linked <$> B.readFile a <*> B.readFile as
    linkedB <- S.Linked <$> B.readFile b <*> B.readFile bs
    pure (GroupInput group regionBytes linkedA linkedB)
  pure (inputs, rest)

main :: IO ()
main = do
  (groups, rest) <- getArgs >>= readInputs
  (datPath, loaderAPath, loaderBPath, semA, semB, output) <- case rest of
    [dat, loaderA, loaderB, a, b, out] -> pure (dat, loaderA, loaderB, a, b, out)
    _ -> H.failH "usage: stage-semaphore GROUPS SOURCE_DAT LOADER_A LOADER_B SEMAPHORE_A_BIN SEMAPHORE_B_BIN NEW_OUTPUT_DIRECTORY"
  let readInput = H.readRegularWith (pure ()) False
      refuse :: Show e => Either e a -> IO a
      refuse = either (H.failH . show) pure
  exists <- doesPathExist output
  H.ensure (not exists) (output ++ " already exists")
  dat <- readInput (64 * 1024 * 1024) datPath
  loaderA <- readInput 0x20000 loaderAPath
  loaderB <- readInput 0x20000 loaderBPath
  semaphoreA <- readInput 0x20000 semA
  semaphoreB <- readInput 0x20000 semB
  source <- refuse (S.canonicalSource dat loaderA loaderB)
  refuse (S.requirePinned source)
  admitted <- refuse (admitCutover source groups)
  base <- refuse (stageCutover admitted CombinedCandidate)
  (candidate, patched) <- either (H.failH . id) pure (stageOverBase (cutoverDat base) semaphoreA semaphoreB)
  createDirectory output
  B.writeFile (output </> "candidate.DAT") candidate
  B.writeFile (output </> "loader-a.bin") (patched M.! DomainA)
  B.writeFile (output </> "loader-b.bin") (patched M.! DomainB)
  L.writeFile (output </> "contract.json") (J.encode (contract source base candidate semaphoreA semaphoreB patched) <> L.singleton 10)
  L.writeFile (output </> "semaphore-regions.json") (J.encode (catalogValue) <> L.singleton 10)
  where
    contract source base candidate semaphoreA semaphoreB patched = J.object
      [ "schema" J..= (1 :: Int)
      , "piece_kind" J..= ("semaphore-family" :: String)
      , "scope" J..= ("qualified scheduler/own-first base plus the seven semaphore carriers in place at their stock extents; timer arena and the release/cancel/enqueue helpers remain retained" :: String)
      , "source_dat_sha256" J..= digestOf (S.sourceDat source)
      , "base_dat_sha256" J..= digestOf (cutoverDat base)
      , "candidate_dat_sha256" J..= digestOf candidate
      , "semaphore_a_bin_sha256" J..= digestOf semaphoreA
      , "semaphore_b_bin_sha256" J..= digestOf semaphoreB
      , "loaders" J..= [J.object
          [ "domain" J..= domainLabel domain
          , "stock_sha256" J..= digestOf (S.sourceLoaders source M.! domain)
          , "base_sha256" J..= digestOf (cutoverLoader base domain)
          , "candidate_sha256" J..= digestOf (patched M.! domain)
          , "regions" J..= [J.object
              [ "start" J..= addressText (extentStart extent)
              , "end" J..= addressText (extentEnd extent)
              , "stock_sha256" J..= digestOf (slice (domainLoaderBase domain) extent (S.sourceLoaders source M.! domain))
              , "base_sha256" J..= digestOf (slice (domainLoaderBase domain) extent (cutoverLoader base domain))
              , "candidate_sha256" J..= digestOf (slice (domainLoaderBase domain) extent (patched M.! domain))
              ] | extent <- semaphoreRegions domain]
          ] | domain <- domains]
      , "retained_seams" J..= (["object locks 0x439d8/0x43a14, 0xd398c/0xd39c8", "notify 0x44018/0xd3fd4", "dispatcher save 0x43640/0xd3640", "mask pair 0x42560/0x42568, 0xd2560/0xd2568", "wait release 0x44568/0xd4528", "wait cancel 0x44744/0xd4704", "wait enqueue 0x44800/0x448e4, 0xd47c0/0xd48a4", "timer removal 0x44ca8/0xd4c68, timeout callback 0x44318/0xd42d8, tick 0x44d8c/0xd4d4c (timer arena unmerged)"] :: [String])
      ]
    slice (GuestAddress base) extent bytes =
      B.take (extentBytes extent) (B.drop (fromIntegral (start - base)) bytes)
      where Extent (GuestAddress start) _ = extent
    catalogValue = J.object
      [ "fill_word" J..= ("0xe7f000f0" :: String)
      , "domains" J..= J.object
          [ "a" J..= domainValue DomainA
          , "b" J..= domainValue DomainB
          ]
      ]
    domainValue domain = J.object
      [ "regions" J..= [J.object
          [ "start" J..= addressText (extentStart extent)
          , "end" J..= addressText (extentEnd extent)
          , "entries" J..= [J.object
              [ "entry" J..= addressText (extentStart extent)
              , "semantic" J..= J.object ["kind" J..= ("semaphore" :: String), "name" J..= name]
              ]
            ]
          ] | (name, extent) <- semaphoreEntries domain]
      ]
