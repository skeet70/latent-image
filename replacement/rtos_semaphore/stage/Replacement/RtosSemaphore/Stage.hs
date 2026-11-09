{-# LANGUAGE OverloadedStrings #-}
-- | The semaphore carriers over a qualified scheduler/own-first candidate: the two stock extents
-- per domain, the loader patch and the DAT confinement.
module Replacement.RtosSemaphore.Stage
  ( semaphoreService
  , semaphoreConfig
  , semaphoreRegions
  , semaphoreEntries
  , carrierLength
  , patchLoader
  , semaphorePatches
  , confineSemaphore
  , stageOverBase
  ) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions
import Replacement.RtosScheduler.Cutover (loadersFrom, rebuild)

semaphoreService, semaphoreConfig :: Domain -> Extent
semaphoreService DomainA = Extent (GuestAddress 0x47604) (GuestAddress 0x48044)
semaphoreService DomainB = Extent (GuestAddress 0xd75c4) (GuestAddress 0xd8004)
semaphoreConfig DomainA = Extent (GuestAddress 0x4d2a8) (GuestAddress 0x4d39c)
semaphoreConfig DomainB = Extent (GuestAddress 0xdd26c) (GuestAddress 0xdd360)

semaphoreRegions :: Domain -> [Extent]
semaphoreRegions domain = [semaphoreService domain, semaphoreConfig domain]

-- | The seven entry extents per domain, in catalog order.
semaphoreEntries :: Domain -> [(String, Extent)]
semaphoreEntries DomainA =
  [ ("signal", Extent (GuestAddress 0x47604) (GuestAddress 0x47844))
  , ("interrupt-signal", Extent (GuestAddress 0x47844) (GuestAddress 0x47a54))
  , ("wait", Extent (GuestAddress 0x47a54) (GuestAddress 0x47c94))
  , ("poll", Extent (GuestAddress 0x47c94) (GuestAddress 0x47dd0))
  , ("timed-wait", Extent (GuestAddress 0x47dd0) (GuestAddress 0x48044))
  , ("create", Extent (GuestAddress 0x4d2a8) (GuestAddress 0x4d338))
  , ("delete", Extent (GuestAddress 0x4d338) (GuestAddress 0x4d39c))
  ]
semaphoreEntries DomainB =
  [ ("signal", Extent (GuestAddress 0xd75c4) (GuestAddress 0xd7804))
  , ("interrupt-signal", Extent (GuestAddress 0xd7804) (GuestAddress 0xd7a14))
  , ("wait", Extent (GuestAddress 0xd7a14) (GuestAddress 0xd7c54))
  , ("poll", Extent (GuestAddress 0xd7c54) (GuestAddress 0xd7d90))
  , ("timed-wait", Extent (GuestAddress 0xd7d90) (GuestAddress 0xd8004))
  , ("create", Extent (GuestAddress 0xdd26c) (GuestAddress 0xdd2fc))
  , ("delete", Extent (GuestAddress 0xdd2fc) (GuestAddress 0xdd360))
  ]

carrierLength :: Domain -> Int
carrierLength domain =
  let Extent (GuestAddress serviceStart) _ = semaphoreService domain
      Extent _ (GuestAddress configEnd) = semaphoreConfig domain
   in fromIntegral (configEnd - serviceStart)

sliceExtent :: GuestAddress -> Extent -> B.ByteString -> B.ByteString
sliceExtent (GuestAddress base) extent bytes =
  B.take (extentBytes extent) (B.drop (fromIntegral (start - base)) bytes)
  where Extent (GuestAddress start) _ = extent

patchLoader :: Domain -> B.ByteString -> B.ByteString -> B.ByteString
patchLoader domain image baseLoader = foldl replace baseLoader (semaphoreRegions domain)
  where
    replace bytes extent =
      let Extent (GuestAddress start) _ = extent
          GuestAddress base = domainLoaderBase domain
          offset = fromIntegral (start - base)
          owned = sliceExtent (extentStart (semaphoreService domain)) extent image
       in B.take offset bytes <> owned <> B.drop (offset + B.length owned) bytes

semaphorePatches :: M.Map Domain B.ByteString -> M.Map Domain B.ByteString -> Either String [F.LoaderWordPatch]
semaphorePatches baseLoaders patched = sequence
  [ F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) at old new
  | domain <- domains
  , extent <- semaphoreRegions domain
  , let Extent (GuestAddress low) (GuestAddress high) = extent
  , let GuestAddress base = domainLoaderBase domain
  , at <- [low, low + 4 .. high - 4]
  , let offset = fromIntegral (at - base)
  , let old = U.word32At (baseLoaders M.! domain) offset
  , let new = U.word32At (patched M.! domain) offset
  , old /= new
  ]

semaphoreDatRange :: U.Parsed -> Domain -> Extent -> (Int, Int)
semaphoreDatRange parsed domain extent =
  case [segment | segment <- U.partSegments (U.parsedParts parsed), U.segId segment == domainLoaderId domain] of
    [segment] ->
      let Extent (GuestAddress low) _ = extent
          mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
       in (mainStart + fromIntegral (U.segFlash segment) + fromIntegral (low - U.segLoad segment), extentBytes extent)
    _ -> error "semaphore loader segment set differs"

confineSemaphore :: U.Parsed -> B.ByteString -> B.ByteString -> Either String ()
confineSemaphore parsed base candidate
  | B.length base /= B.length candidate = Left "semaphore candidate length differs"
  | otherwise =
      case [index | (index, changed) <- zip [0 ..] (B.zipWith (/=) base candidate), changed, not (allowed index)] of
        [] -> Right ()
        index : _ -> Left ("semaphore change outside its extents at " ++ show index)
  where
    allowed index =
      (index >= U.mainChecksumOffset && index < U.mainChecksumOffset + 4)
        || any (\(low, length') -> index >= low && index < low + length') ranges
    ranges = [semaphoreDatRange parsed domain extent | domain <- domains, extent <- semaphoreRegions domain]

-- | Patches both semaphore carriers over the base loaders, rebuilds the DAT and confines the change.
stageOverBase :: B.ByteString -> B.ByteString -> B.ByteString
  -> Either String (B.ByteString, M.Map Domain B.ByteString)
stageOverBase baseDat semaphoreA semaphoreB = do
  unless (B.length semaphoreA >= carrierLength DomainA && B.length semaphoreB >= carrierLength DomainB)
    (Left "semaphore carrier image is shorter than its two extents")
  (baseParsed, baseLoaders) <- either (Left . show) Right (loadersFrom baseDat)
  let patched = M.fromList
        [ (DomainA, patchLoader DomainA semaphoreA (baseLoaders M.! DomainA))
        , (DomainB, patchLoader DomainB semaphoreB (baseLoaders M.! DomainB))
        ]
  wordsToPatch <- semaphorePatches baseLoaders patched
  candidate <- either (Left . show) Right (rebuild baseParsed wordsToPatch)
  (_, actual) <- either (Left . show) Right (loadersFrom candidate)
  unless (actual == patched) (Left "staged semaphore loaders differ")
  confineSemaphore baseParsed baseDat candidate
  pure (candidate, patched)
