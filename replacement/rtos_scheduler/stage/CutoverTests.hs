{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson ((.:), withObject)
import Data.Aeson.Types (parseEither)
import qualified Data.Aeson as J
import Data.List (sort)
import Data.Bits (complement)
import qualified Data.ByteString as B
import Data.Word (Word32)
import Fixtures (fixtureDat)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.Cutover
import Replacement.RtosScheduler.CutoverPins
import System.Environment (getArgs)

right :: Show e => Either e a -> IO a
right = either (fail . show) pure
expect :: String -> (CutoverError -> Bool) -> Either CutoverError a -> IO ()
expect label matches result = case result of
  Left err | matches err -> pure ()
  Left err -> fail (label ++ ": unexpected " ++ show err)
  Right _ -> fail (label ++ ": accepted")
replaceAt :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replaceAt at value bytes = B.take at bytes <> value <> B.drop (at+B.length value) bytes
changedWords :: Domain -> B.ByteString -> B.ByteString -> [Word32]
changedWords domain before after = [base+fromIntegral offset | offset <- [0,4..B.length before-4], U.word32At before offset /= U.word32At after offset]
  where GuestAddress base = domainLoaderBase domain

readInputs :: [Group] -> [String] -> IO [GroupInput]
readInputs [] [] = pure []
readInputs (group:groups) (metadata:a:as:b:bs:rest) = do
  input <- GroupInput group <$> B.readFile metadata <*> (S.Linked <$> B.readFile a <*> B.readFile as) <*> (S.Linked <$> B.readFile b <*> B.readFile bs)
  (input:) <$> readInputs groups rest
readInputs _ _ = fail "expected cutover inputs"

main :: IO ()
main = do
  inputs <- getArgs >>= readInputs [minBound .. maxBound]
  parsed <- right (U.parseContainer fixtureDat)
  let segment p domain = [s | s <- U.partSegments (U.parsedParts p), U.segId s == domainLoaderId domain] !! 0
      loader p domain = let s = segment p domain in B.map complement (B.take (domainLoaderSize domain) (B.drop (fromIntegral (U.segFlash s)) (U.partPreTable (U.parsedParts p))))
  synthetic <- right (sequence [F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) (base+offset) (U.word32At (loader parsed domain) (fromIntegral (base+offset-loaderBase))) word | (domain,base) <- [(DomainA,0x433e4),(DomainB,0xd33e4)], let GuestAddress loaderBase = domainLoaderBase domain, (offset,word) <- zip [0,4,8] [0xe24dd004,0xe28dd004,0xe12fff1e]])
  parts <- right (F.applyLoaderWordPatches synthetic (U.parsedParts parsed))
  dat <- right (U.rebuildContainer parsed parts)
  stockParsed <- right (U.parseContainer dat)
  let stock = loader stockParsed
  source <- right (S.canonicalSource dat (stock DomainA) (stock DomainB))
  admission <- right (admitCutover source inputs)
  let ownInput = [input | input <- inputs, inputGroup input == OwnFirst] !! 0
  ownTable <- right (parseRegionTable (inputMetadata ownInput))
  guardRows <- right $ parseEither (withObject "guard catalog" $ \catalog -> do
    fill <- catalog .: "fill_word"
    unless (fill == ("0xe7f000f0" :: String)) (fail "guard fill differs")
    domainsObject <- catalog .: "domains"
    forM [("a",DomainA),("b",DomainB)] $ \(label,domain) -> do
      domainObject <- domainsObject .: label
      regions <- domainObject .: "regions"
      parsedRows <- forM regions $ withObject "guard extent" $ \region -> do
        start <- region .: "start"
        end <- region .: "end"
        entries <- region .: "entries"
        unless (null (entries :: [J.Value])) (fail "guard-only catalog contains entries")
        pure (start,end)
      pure (domain,sort parsedRows)) (ownFirstGuards admission)
  let expectedGuards = [(domain,sort [(hexText start,hexText end) | region <- domainExtents (domainTable ownTable domain), let Extent (GuestAddress start) (GuestAddress end) = regionExtent region]) | domain <- domains]
  unless (guardRows == expectedGuards) (fail "own-first guard extent coverage")
  candidate <- right (stageCutover admission CombinedCandidate)
  restored <- right (restoreCutover admission (cutoverDat candidate))
  unless (restored == dat && cutoverDat candidate /= dat) (fail "combined round trip")
  forM_ [RestoreS1,RestoreS2,RestoreS3,CombinedSilentNotify,CombinedWrongPredicate,WakeNoInsert] $ \variant -> do
    control <- right (stageCutover admission variant)
    let a = cutoverLoader control DomainA
        b = cutoverLoader control DomainB
        ca = cutoverLoader candidate DomainA
        cb = cutoverLoader candidate DomainB
        changesA = changedWords DomainA ca a
        changesB = changedWords DomainB cb b
        word domain at = let GuestAddress base = domainLoaderBase domain in U.word32At (cutoverLoader control domain) (fromIntegral (at-base))
        restoredWindow low high =
          let offset = fromIntegral (low-0x40000); len = fromIntegral (high-low)
              expected = replaceAt offset (B.take len (B.drop offset (stock DomainA))) ca
              expectedWords = [at | at <- [low,low+4..high-4], U.word32At ca (fromIntegral (at-0x40000)) /= U.word32At (stock DomainA) (fromIntegral (at-0x40000))]
           in a == expected && b == cb && changesA == expectedWords && not (null expectedWords)
        correct = case variant of
          RestoreS1 -> restoredWindow 0x43ca4 0x43d20
          RestoreS2 -> restoredWindow 0x43640 0x43668
          RestoreS3 -> restoredWindow 0x46004 0x46200
          CombinedSilentNotify -> changesA == [0x4402c,0x44034] && null changesB && map (word DomainA) [0x4402c,0x44030,0x44034] == [0xe24dd004,0xf57ff04f,0xe28dd004]
          CombinedWrongPredicate -> null changesA && changesB == [0xd3fd4,0xd3fd8,0xd3fdc,0xd3fe0] && map (word DomainB) [0xd3fd4,0xd3fd8,0xd3fdc,0xd3fe0] == [0xe3500001,0x93a00001,0x912fff1e,0xe320f000]
          WakeNoInsert -> changesA == [0x46134] && null changesB && word DomainA 0x46134 == 0xe3a00000
          CombinedCandidate -> False
    unless correct (fail ("control words differ: " ++ show variant))
    roundTrip <- right (restoreCutover admission (cutoverDat control))
    unless (roundTrip == dat) (fail ("control restore: " ++ show variant))
  expect "missing group" (== GroupSetMismatch) (validateInputs (drop 1 inputs))
  expect "duplicate group" (== GroupSetMismatch) (validateInputs (inputs ++ take 1 inputs))
  forM_ inputs $ \input -> do
    let group = inputGroup input
        replaceInput replacement = [if inputGroup other == group then replacement else other | other <- inputs]
    expect (show group ++ " metadata") (== MetadataUnpinned group) (admitCutover source (replaceInput input {inputMetadata = inputMetadata input <> B.singleton 10}))
    forM_ domains $ \domain -> do
      let linked = case domain of DomainA -> inputA input; DomainB -> inputB input
          replaceLinked value = replaceInput (case domain of DomainA -> input {inputA = value}; DomainB -> input {inputB = value})
      expect (show (group,domain) ++ " bin") (== BinaryUnpinned group domain) (admitCutover source (replaceLinked linked {S.linkedBin = S.linkedBin linked <> B.singleton 0}))
      if group == OwnFirst
        then expect "own-first symbol admission" (\err -> case err of LeafRejected OwnFirst _ -> True; _ -> False) (admitCutover source (replaceLinked linked {S.linkedSymbols = B.empty}))
        else expect (show (group,domain) ++ " symbols") (== SymbolsUnpinned group domain) (admitCutover source (replaceLinked linked {S.linkedSymbols = S.linkedSymbols linked <> B.singleton 10}))
  expect "overlap" (\err -> case err of ExtentsOverlap DomainA _ _ -> True; _ -> False) (disjointExtents DomainA [Extent (GuestAddress 0x40000) (GuestAddress 0x40010),Extent (GuestAddress 0x4000c) (GuestAddress 0x40020)])
  expect "outside DAT" (== ChangedOutside 0) (confineCutover admission (replaceAt 0 (B.singleton 0) (cutoverDat candidate)))
  expect "DAT length" (\err -> case err of ChangedOutside _ -> True; _ -> False) (confineCutover admission (cutoverDat candidate <> B.singleton 0))
  right (confineCutover admission (replaceAt U.mainChecksumOffset (B.replicate 4 0) (cutoverDat candidate)))
  let first = [input | input <- inputs, inputGroup input == S1] !! 0
      damaged = (inputA first) {S.linkedBin = replaceAt (0x4402c-0x439d8) (U.putWord32 0) (S.linkedBin (inputA first))}
  case S.controlLinked S.SilentNotify DomainA damaged of
    Left (S.ControlRejected _) -> pure ()
    result -> fail ("control source refusal: " ++ show result)
  putStrLn "combined staging, pins, confinement and exact control words passed"
