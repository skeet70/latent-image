{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (object, (.=))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Emulation.NativeObservation (nativeStatus, runnerStatusLabel)
import Emulation.OwnedCase (required)
import Emulation.Sha256 (sha256,sha256Hex)
import FujiAF.JSON (encodeCanonical)
import Replacement.RtosOwnFirst.Artifact (loaderSha256)
import Replacement.RtosOwnFirst.Regions (domains,domainLabel)
import Replacement.RtosScheduler.Layout (parseLayout)
import Replacement.RtosScheduler.Oracle (mkEnv,runCases)
import Replacement.RtosScheduler.Regions (parseSchedulerRegions)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import Replacement.RtosScheduler.S3a
import System.Directory (canonicalizePath,createDirectoryIfMissing,doesPathExist,makeAbsolute,removeDirectoryRecursive,setCurrentDirectory)
import System.Environment (getArgs,lookupEnv)
import System.Exit (exitFailure)
import System.FilePath ((</>),takeDirectory)

orFail :: Either String a -> IO a
orFail=either fail pure

optional :: [String] -> String -> IO (Maybe FilePath)
optional args flag = case dropWhile (/= flag) args of
  [] -> pure Nothing
  [_] -> fail (flag ++ " needs a value")
  _ : value : _ -> Just <$> canonicalizePath value

main :: IO ()
main=do
  args<-getArgs
  let input name=required args name >>= canonicalizePath
  runner<-input "--runner"
  regionsPath<-input "--regions"
  layoutPath<-input "--layout"
  ownedPaths<-mapM input ["--owned-a","--owned-b"]
  symbolsPaths<-mapM (optional args) ["--symbols-a","--symbols-b"]
  placementPaths<-mapM (optional args) ["--placements-a","--placements-b"]
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  loaders<-mapM (\name->input name >>= B.readFile) ["--loader-a","--loader-b"]
  owned<-mapM B.readFile ownedPaths
  parsedSymbols<-mapM (traverse (\path->either fail pure . Agg.parseAggregateSymbols =<< B.readFile path)) symbolsPaths
  placements<-mapM (traverse (\path->either fail pure . Agg.parsePlacements =<< B.readFile path)) placementPaths
  work<-required args "--work" >>= makeAbsolute
  summary<-required args "--summary" >>= makeAbsolute
  exists<-doesPathExist work
  when exists (fail "work directory already exists")
  regions<-B.readFile regionsPath >>= orFail . parseSchedulerRegions
  layout<-B.readFile layoutPath >>= orFail . parseLayout
  env<-orFail (mkEnv layout regions)
  createDirectoryIfMissing True work
  reports<-fmap concat . forM (zip domains (zip3 loaders owned (zip parsedSymbols placements))) $ \(domain,(loader,bin,(symbolRows,placementRows)))->do
    unless (sha256Hex (sha256 loader)==loaderSha256 domain) (fail "loader identity differs")
    forM services $ \service->do
      let rows=zip [0::Int ..] (scenarios service)
          label role index=role++"-"++show index
          linked = case (symbolRows, placementRows) of
            (Just parsed, Just plans) -> Just (Aggregate bin parsed plans)
            (Just parsed, Nothing) -> Just (Aggregate bin parsed [])
            _ -> Just (Standalone bin)
          spec role index row=makeSpec env domain loader (if role `elem` ["owned","mutant"] then linked else Nothing) service row (label role index)
          build role index row=let raw=spec role index row in case role of "equivalent"->controlSpec True raw; "mutant"->controlSpec False raw; _->raw
          roles=["stock","owned","stock-repeat","equivalent","mutant"]
          cases=[(label role index,build role index row) | (index,row)<-rows,role<-roles]
          directory=work </> domainLabel domain++"-"++serviceName service
      results<-runCases runner directory cases
      let reference index=results M.! label "stock" index
          observation role index=results M.! label role index
          diffs role index row=difference (spec "stock" index row) (reference index) (observation role index)
          badStock=[(index,show row,runnerStatusLabel (nativeStatus (reference index))) | (index,row)<-rows,not (contractHolds env domain service row (spec "stock" index row) (reference index))]
          mismatch role=[(index,show row,show (diffs role index row)) | (index,row)<-rows,not (null (diffs role index row))]
          badOwned=[index | (index,row)<-rows,not (contractHolds env domain service row (spec "owned" index row) (observation "owned" index))]
          holds=null badStock && null badOwned && all (null . mismatch) ["owned","stock-repeat","equivalent"] && not (null (mismatch "mutant"))
          report=object ["domain" .= domainLabel domain,"service" .= serviceName service,"cases" .= length rows,"owned_matches" .= (length rows-length (mismatch "owned")),"stock_contract_failures" .= take 5 badStock,"owned_contract_failures" .= take 5 badOwned,"first_mismatches" .= take 5 (mismatch "owned"),"stock_repeat_matches" .= null (mismatch "stock-repeat"),"equivalent_matches" .= null (mismatch "equivalent"),"mutant_refused" .= not (null (mismatch "mutant")),"holds" .= holds]
      putStrLn (domainLabel domain++" "++serviceName service++": "++show (length rows)++" cases; holds="++show holds++"; stock="++show (take 1 badStock)++"; owned="++show (take 1 (mismatch "owned")))
      when holds (removeDirectoryRecursive directory)
      pure (holds,report)
  createDirectoryIfMissing True (takeDirectory summary)
  B.writeFile summary (encodeCanonical (object ["schema" .= (1::Int),"reports" .= map snd reports,"holds" .= all fst reports,"comparison" .= ("Persistent bytes and write multiset, r0, preserved registers/SP at return, CPSR mode/I/F/A; dispatcher stops exclude live frames. Stock callees retained. Equivalent is an independently emitted tail-branch wrapper. Mutant returns 1 without effects."::String),"loader_sha256" .= map (sha256Hex . sha256) loaders,"owned_sha256" .= map (sha256Hex . sha256) owned]))
  forM_ reports $ \(holds,_)->unless holds exitFailure
