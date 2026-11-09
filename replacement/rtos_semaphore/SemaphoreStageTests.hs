{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, forM_, unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Fixtures (fixtureDat)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.Cutover
import Replacement.RtosSemaphore.Stage
import System.Environment (getArgs)

right :: Show e => Either e a -> IO a
right = either (fail . show) pure

readInputs :: [String] -> IO [GroupInput]
readInputs arguments = case split [minBound .. maxBound] arguments of
  Right (groups, []) -> forM groups $ \(group, metadata, a, as, b, bs) -> do
    regionBytes <- B.readFile metadata
    linkedA <- S.Linked <$> B.readFile a <*> B.readFile as
    linkedB <- S.Linked <$> B.readFile b <*> B.readFile bs
    pure (GroupInput group regionBytes linkedA linkedB)
  Right (_, rest) -> fail ("unexpected trailing inputs: " ++ show rest)
  Left problem -> fail problem
  where
    split [] args = Right ([], args)
    split (group : groups) (metadata : a : as : b : bs : rest) = do
      (inputs, args) <- split groups rest
      pure ((group, metadata, a, as, b, bs) : inputs, args)
    split _ _ = Left "expected metadata, A bin/symbols and B bin/symbols for each group"

main :: IO ()
main = do
  inputs <- getArgs >>= readInputs
  parsed <- right (U.parseContainer fixtureDat)
  let segment p domain = [s | s <- U.partSegments (U.parsedParts p), U.segId s == domainLoaderId domain] !! 0
      loader p domain =
        let s = segment p domain
         in B.map complement (B.take (domainLoaderSize domain) (B.drop (fromIntegral (U.segFlash s)) (U.partPreTable (U.parsedParts p))))
  synthetic <- right (sequence
    [ F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) (base + offset)
        (U.word32At (loader parsed domain) (fromIntegral (base + offset - loaderBase))) word
    | (domain, base) <- [(DomainA, 0x433e4), (DomainB, 0xd33e4)]
    , let GuestAddress loaderBase = domainLoaderBase domain
    , (offset, word) <- zip [0, 4, 8] [0xe24dd004, 0xe28dd004, 0xe12fff1e]
    ])
  parts <- right (F.applyLoaderWordPatches synthetic (U.parsedParts parsed))
  dat <- right (U.rebuildContainer parsed parts)
  stockParsed <- right (U.parseContainer dat)
  let stock = loader stockParsed
  source <- right (S.canonicalSource dat (stock DomainA) (stock DomainB))
  admission <- right (admitCutover source inputs)
  base <- right (stageCutover admission CombinedCandidate)
  (baseParsed, _) <- right (loadersFrom (cutoverDat base))
  let baseDat = cutoverDat base
      baseLoaders = M.fromList [(DomainA, cutoverLoader base DomainA), (DomainB, cutoverLoader base DomainB)]
      semaphoreA = B.replicate (carrierLength DomainA) 0x5a
      semaphoreB = B.replicate (carrierLength DomainB) 0xa5
  (candidate, patched) <- right (stageOverBase baseDat semaphoreA semaphoreB)
  unless (B.length candidate == B.length baseDat && candidate /= baseDat) (fail "candidate DAT shape differs")
  forM_ domains $ \domain -> do
    let before = baseLoaders M.! domain
        after = patched M.! domain
        image = if domain == DomainA then semaphoreA else semaphoreB
        GuestAddress loaderBase = domainLoaderBase domain
        GuestAddress serviceBase = extentStart (semaphoreService domain)
        offsetOf extent = fromIntegral (let Extent (GuestAddress start) _ = extent in start - loaderBase)
        imageOffset extent = fromIntegral (let Extent (GuestAddress start) _ = extent in start - serviceBase)
        allowed index = any (\extent -> index >= offsetOf extent && index < offsetOf extent + extentBytes extent) (semaphoreRegions domain)
        changes = [index | (index, changed) <- zip [0 ..] (B.zipWith (/=) before after), changed]
    unless (not (null changes) && all allowed changes) (fail (domainLabel domain ++ " loader changes leave the semaphore extents"))
    forM_ (semaphoreRegions domain) $ \extent -> do
      let at = offsetOf extent
          owned = B.take (extentBytes extent) (B.drop (imageOffset extent) image)
      unless (B.take (extentBytes extent) (B.drop at after) == owned) (fail (domainLabel domain ++ " region is not the carrier slice"))
  let tampered = B.take 0x100 candidate <> B.singleton (B.index candidate 0x100 + 1) <> B.drop 0x101 candidate
  case confineSemaphore baseParsed baseDat tampered of
    Left _ -> pure ()
    Right _ -> fail "a change outside the semaphore extents was accepted"
  case stageOverBase baseDat (B.take 4 semaphoreA) semaphoreB of
    Left _ -> pure ()
    Right _ -> fail "a short carrier image was accepted"
  candidateParsed <- right (U.parseContainer candidate)
  inverse <- right (semaphorePatches patched baseLoaders)
  restored <- right (rebuild candidateParsed inverse)
  unless (restored == baseDat) (fail "semaphore restore differs from the base DAT")
  putStrLn "semaphore staging, confinement and restore passed"
