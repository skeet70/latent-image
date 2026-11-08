{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, forM_, unless)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.List (sort)
import Fixtures (fixtureDat)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.Cutover
import Replacement.RtosScheduler.CutoverPins
import Replacement.RtosScheduler.Family
import System.Environment (getArgs)

data InputPaths = InputPaths Group FilePath FilePath FilePath FilePath FilePath

splitInputs :: [Group] -> [String] -> Either String ([InputPaths], [String])
splitInputs [] args = Right ([], args)
splitInputs (group:groups) (metadata:a:as:b:bs:rest) = do
  (inputs, args) <- splitInputs groups rest
  pure (InputPaths group metadata a as b bs:inputs, args)
splitInputs _ _ = Left "expected metadata, A bin/symbols and B bin/symbols for each group"

data FamilyPaths = FamilyPaths FilePath FilePath FilePath FilePath

splitFamily :: [String] -> Either String (FamilyPaths, [String])
splitFamily (aBin:aSymbols:bBin:bSymbols:rest) = Right (FamilyPaths aBin aSymbols bBin bSymbols, rest)
splitFamily _ = Left "expected family A bin/symbols and B bin/symbols"

right :: Show e => Either e a -> IO a
right = either (fail . show) pure

changedWords :: Domain -> B.ByteString -> B.ByteString -> [GuestAddress]
changedWords domain before after = [GuestAddress (base + fromIntegral offset) | offset <- [0,4 .. B.length before - 4], U.word32At before offset /= U.word32At after offset]
  where GuestAddress base = domainLoaderBase domain

rowWords :: [FamilyRow] -> [GuestAddress]
rowWords rows = [GuestAddress at | admittedRow <- rows, let Extent (GuestAddress low) (GuestAddress high) = rowExtent admittedRow, at <- [low, low + 4 .. high - 4]]

main :: IO ()
main = do
  (inputs, rest) <- getArgs >>= either fail pure . splitInputs [minBound .. maxBound]
  (familyPaths, args) <- either fail pure (splitFamily rest)
  unless (null args) (fail "unexpected arguments")
  groups <- forM inputs $ \(InputPaths group metadata a as b bs) ->
    GroupInput group <$> B.readFile metadata <*> (S.Linked <$> B.readFile a <*> B.readFile as) <*> (S.Linked <$> B.readFile b <*> B.readFile bs)
  let FamilyPaths aBin aSymbols bBin bSymbols = familyPaths
  images <- do
    a <- FamilyImage <$> B.readFile aBin <*> B.readFile aSymbols
    b <- FamilyImage <$> B.readFile bBin <*> B.readFile bSymbols
    pure (M.fromList [(DomainA, a), (DomainB, b)])
  parsed <- right (U.parseContainer fixtureDat)
  let loaderFrom container domain = let entry = segmentFrom container domain in B.map complement (B.take (domainLoaderSize domain) (B.drop (fromIntegral (U.segFlash entry)) (U.partPreTable (U.parsedParts container))))
      segmentFrom container domain = [candidate | candidate <- U.partSegments (U.parsedParts container), U.segId candidate == domainLoaderId domain] !! 0
  synthetic <- right (sequence [F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) (base + offset) (U.word32At (loaderFrom parsed domain) (fromIntegral (base + offset - loaderBase))) word | (domain, base) <- [(DomainA, 0x433e4), (DomainB, 0xd33e4)], let GuestAddress loaderBase = domainLoaderBase domain, (offset, word) <- zip [0, 4, 8] [0xe24dd004, 0xe28dd004, 0xe12fff1e]])
  parts <- right (F.applyLoaderWordPatches synthetic (U.parsedParts parsed))
  dat <- right (U.rebuildContainer parsed parts)
  patched <- right (U.parseContainer dat)
  source <- right (S.canonicalSource dat (loaderFrom patched DomainA) (loaderFrom patched DomainB))
  base <- right (admitCutover source groups)
  baseStaged <- right (stageCutover base CombinedCandidate)
  family <- right (admitFamily source base baseStaged groups images)
  staged <- right (stageFamily family)
  again <- right (stageFamily family)
  unless (familyDat again == familyDat staged) (fail "family staging is not deterministic")
  forM_ domains $ \domain -> unless (familyLoader again domain == familyLoader staged domain) (fail ("family loader is not deterministic " ++ show domain))
  let baseDat = cutoverDat baseStaged
      baseLoader domain = cutoverLoader baseStaged domain
  unless (familyDat staged /= baseDat) (fail "family candidate equals the base candidate")
  forM_ domains $ \domain -> do
    let rows = familyRows domain
        changed = changedWords domain (baseLoader domain) (familyLoader staged domain)
        allowed = rowWords rows
    unless (length rows == 19) (fail ("window count " ++ show domain))
    unless (length [admittedRow | admittedRow <- rows, slotClass (rowSlot admittedRow) == CarrierSlot] == 5) (fail ("carrier count " ++ show domain))
    unless (length [admittedRow | admittedRow <- rows, slotClass (rowSlot admittedRow) == EntrySlot] == 14) (fail ("entry count " ++ show domain))
    unless (not (null changed) && all (`elem` allowed) changed) (fail ("loader changes outside family windows " ++ show domain))
    unless (all (\admittedRow -> rowFrame admittedRow > FrameBytes 0) rows) (fail ("zero frame demand " ++ show domain))
  restoredFamily <- right (restoreFamily family (familyDat staged))
  unless (restoredFamily == baseDat) (fail "family candidate does not restore to the base candidate")
  restoredBase <- right (restoreCutover base baseDat)
  unless (restoredBase == S.sourceDat source) (fail "base candidate does not restore to the source DAT")
  let controls = familyControls source base baseStaged groups images staged
      names = map controlName controls
  unless (all controlPassed controls) (fail ("controls failed: " ++ show [controlName item | item <- controls, not (controlPassed item)]))
  unless (sort names == sort ["absolute-symbol","base-overlap","bin-length","carrier-byte","dependency-byte","dependency-symbols","entry","family-restores-to-base","frame","marker","outside-window-byte","outside-write","window-overlap","candidate-differs"]) (fail ("control set " ++ show names))
  putStrLn "family admission, dependency binding, staging, restoration and refusing controls passed"
