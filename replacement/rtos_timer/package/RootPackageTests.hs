{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Either (isLeft)
import qualified Data.Set as Set
import Fixtures (fixtureDat, fixtureImage4, fixtureImage5)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Artifact as Platform
import qualified Replacement.RtosPlatform.Union as Union
import Replacement.RtosTimer.RootPackage
import qualified Replacement.StartupArtifact.Compose as Startup
import Replacement.StartupArtifact.Joins (JoinKind)
import qualified Replacement.StartupArtifact.Manifest as Manifest
import qualified Replacement.StartupArtifact.Targets as Targets
import qualified Replacement.StartupArtifact.Types as T
import System.Exit (die)

main :: IO ()
main = do
  source <- either (die . show) pure (Startup.canonical fixtureDat fixtureImage4 fixtureImage5)
  let domain role base veneer body = Union.DomainMap role (R.GuestAddress base)
        (if role == R.DomainA then 0xdae0 else 0xdaa0)
        [Union.Entry Union.Timer "test" (R.GuestAddress (base + 0x430)) body (".text." ++ body) veneer]
        [Union.Carrier Union.Timer (R.Extent (R.GuestAddress (base + 0x430)) (R.GuestAddress (base + 0x530)))]
        [] [Union.Demand Union.Timer body 0x20]
      union = Union.Union 0xe7f000f0 (domain R.DomainA 0x40000 "veneer_a" "body_a") (domain R.DomainB 0xd0000 "veneer_b" "body_b")
      binary = U.putWord32 0xea000002 <> B.replicate 12 0 <> U.putWord32 0xe12fff1e <> U.putWord32 0
        <> B.concat (replicate 6 (U.putWord32 0xe1a00000)) <> B.replicate (0x100 - 0x30) 0
      symbolsA = "00040430 00000004 T veneer_a\n00040440 00000020 T body_a\n"
      symbolsB = "000d0430 00000004 T veneer_b\n000d0440 00000020 T body_b\n"
      linked = Platform.Linked binary (C.pack symbolsA) "" [(0x40460,8)] binary (C.pack symbolsB) "" [(0xd0460,8)]
      piece kind = T.Piece kind Set.empty [] [] [] []
        (T.Catalog [] [] (T.EnumeratedWrites [] (T.Premised (T.Premise "synthetic fixture"))) []) T.NoFacts
      pieces = map piece [T.TaskStorage,T.RootTask,T.AppCallback]
      firstSpan = T.Span T.Record1 (T.Guest 0) (T.Guest 4)
      firstEdits = maybe [] pure (T.edit firstSpan (U.putWord32 0xe12fff1e))
      first = (piece T.FirstStage) {T.pieceCarriers = [firstSpan], T.pieceEdits = firstEdits}
      composed selected = Startup.compose ([] :: [Startup.Join JoinKind]) source (first : selected)
      join selected bytes = do
        composition <- either (Left . show) Right (composed selected)
        joinRootPlatform source composition bytes (Startup.compositionTargets composition) union linked
      require label condition = unless condition (die label)
  composition <- either (die . show) pure (composed pieces)
  startupDat <- either (die . show) pure (Startup.preTableDat source composition)
  package <- either die pure (join pieces startupDat)
  effective <- either die pure (packageCarriers package)
  require "effective A carrier bytes differ from the final loader"
    (lookup R.DomainA effective == Just (B.take 0x100 (B.drop 0x430 (Targets.targetOf (packageTargets package) T.LoaderA))))
  require "effective B carrier bytes differ from the final loader"
    (lookup R.DomainB effective == Just (B.take 0x100 (B.drop 0x430 (Targets.targetOf (packageTargets package) T.LoaderB))))
  require "platform did not change loader bytes" (packageDat package /= startupDat)
  require "image4 changed during loader composition" (Targets.targetOf (packageTargets package) T.Image4 == fixtureImage4)
  require "image5 changed during loader composition" (Targets.targetOf (packageTargets package) T.Image5 == fixtureImage5)
  require "record1 changed during loader composition"
    (Targets.targetOf (packageTargets package) T.Record1 == Targets.targetOf (Startup.compositionTargets composition) T.Record1)
  require "first-stage canary was not changed by startup"
    (Targets.targetOf (packageTargets package) T.Record1 /= Targets.targetOf (Startup.canonicalTargets source) T.Record1)
  require "missing owned app caller accepted" (isLeft (join (take 2 pieces) startupDat))
  let overlapping = (piece T.TaskStorage) {T.pieceCarriers = [T.Span T.LoaderA (T.Guest 0x40430) (T.Guest 0x40434)]}
  require "overlapping startup and platform ownership accepted" (isLeft (join (overlapping : drop 1 pieces) startupDat))
  require "changed unadmitted startup container accepted" (isLeft (join pieces (B.cons 0 (B.drop 1 startupDat))))
  let seed rows = "{\"schema\":4,\"guard\":{\"attach\":\"primary-publisher/entry-core0\",\"attach_state\":{\"peers\":[],\"carriers\":" <> rows <>
        ",\"allowances\":[],\"registers\":[],\"board\":[],\"relations\":{\"timer\":{\"citation\":\"existing timer law\"}}}},\"pieces\":[{\"kind\":\"first-stage\",\"inputs\":{\"image\":\"seed\",\"symbols\":\"seed\"}}]}"
      resident = "{\"target\":\"record1\",\"start\":32,\"end\":36,\"residency\":\"resident\"}"
      guarded = seed ("[" <> resident <> "]")
      appCarrier = T.Span T.Image4 (T.Guest 0x72337c) (T.Guest 0x723398)
  manifestBytes <- either die pure (j5Manifest guarded [appCarrier] ["synthetic-" ++ show index | index <- [1 :: Int .. 24]])
  manifest <- either (die . show) pure (Manifest.parseManifest manifestBytes)
  require "J5 manifest omitted a required startup piece"
    (map Manifest.manifestPieceKind (Manifest.manifestPieces manifest) ==
      [T.FirstStage,T.Record2Link,T.PrimaryPublisher,T.LoaderStartup,T.TaskSubmission,T.TaskStorage,T.IdleTask,T.RootTask,T.AppCallback])
  expected <- either (die . show) pure (Manifest.parseManifest (seed ("[" <> resident <> ",{\"target\":\"image4\",\"start\":7484284,\"end\":7484312,\"residency\":\"resident\"}]")))
  require "J5 guard lost an existing declaration or added incorrect app residency"
    (Manifest.manifestAttachRules manifest == Manifest.manifestAttachRules expected)
  require "J5 manifest accepted an empty residency declaration" (isLeft (j5Manifest (seed "[]") [] (replicate 24 "synthetic")))
  require "J5 manifest accepted missing linked inputs" (isLeft (j5Manifest guarded [] (replicate 23 "synthetic")))
  putStrLn "root-package-tests: passed"
