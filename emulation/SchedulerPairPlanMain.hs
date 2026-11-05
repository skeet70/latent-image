module Main (main) where

import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import Emulation.SchedulerPairPlan
import qualified FujiAF.Host as H
import Replacement.RtosOwnFirst.Regions (Domain (..))
import System.Environment (getArgs)
import System.IO (IOMode (WriteMode), withBinaryFile)
import System.Directory (doesPathExist)

main :: IO ()
main = do
  args <- getArgs
  case args of
    regionsPath:roleText:datPath:loaderAPath:loaderBPath:output:additionalRegions -> do
      exists <- doesPathExist output
      H.ensure (not exists) "output already exists"
      role <- either H.failH pure (parseRole roleText)
      regions <- mapM (\path -> B.readFile path >>= either H.failH pure . parseCatalog) (regionsPath:additionalRegions)
      dat <- B.readFile datPath
      a <- B.readFile loaderAPath
      b <- B.readFile loaderBPath
      plan <- either H.failH pure (derivePlan role dat regions (M.fromList [(DomainA,a),(DomainB,b)]))
      withBinaryFile output WriteMode (\handle -> L.hPut handle (J.encode (planValue plan) <> L.singleton 10))
    _ -> H.failH "usage: scheduler-pair-plan REGIONS ROLE DAT EXPECTED_LOADER_A EXPECTED_LOADER_B NEW_PLAN_JSON [ADDITIONAL_REGIONS ...]"
