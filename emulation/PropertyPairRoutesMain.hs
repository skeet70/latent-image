-- | property-pair-routes [--effects-from RETAINED_OBSERVATION_DIR] CRAWL_GRAPH OUTPUT: writes the
-- natural-pair route file from a menu crawl.
module Main (main) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.List (sort)
import qualified Data.Text as T
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256, sha256Hex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (die)
import System.FilePath ((</>))

main :: IO ()
main = getArgs >>= \arguments -> case arguments of
  ["--effects-from", observation, graphPath, output] -> write (Just observation) graphPath output
  [graphPath, output] -> write Nothing graphPath output
  _ -> die "usage: property-pair-routes [--effects-from RETAINED_OBSERVATION_DIR] CRAWL_GRAPH OUTPUT"

write :: Maybe FilePath -> FilePath -> FilePath -> IO ()
write observation graphPath output = do
  exists <- doesPathExist output
  if exists then die "property-pair-routes: OUTPUT exists" else pure ()
  bytes <- B.readFile graphPath
  edges <- either (die . ("property-pair-routes: " ++)) pure (decodeCrawlEdges bytes)
  let (crawlRoutes, playbackTrials, timeDrivenTrials) = selectCrawlRoutes edges
  (source, routes, rebasedTrials) <- case observation of
    Nothing -> pure (CrawlEffectsSource, crawlRoutes, [])
    Just directory -> do
      retained <- RetainedObservation <$> B.readFile (directory </> "receipt.json") <*> B.readFile (directory </> "steps.json") <*> B.readFile (directory </> "hits.jsonl")
      rebased <- either (die . ("property-pair-routes: effects refused: " ++) . show) pure (retainedEffects retained crawlRoutes)
      pure (rebasedEffectsSource rebased, rebasedEffectsRoutes rebased, rebasedEffectsTimeDrivenTrials rebased)
  let sourceText = case source of
        CrawlEffectsSource -> "; each route records its crawl edge's effects"
        RetainedObservationSource receipt _ _ -> "; each route records the setter-entry r0 values and commits of its last step in the retained observation with receipt sha256 " ++ sha256Hex receipt
      file = RouteFile
        { routeCrawlGraph = sha256Hex (sha256 bytes)
        , routeSelection = T.pack ("reached crawl edges with a crawl-property-setter or crawl-property-commit effect, other than edges leaving the no-card playback screen and edges whose only effects are the 0x1c9/0x1c8/0x27f/0x280 block, one per ordered effect sequence (setter property ids and commits), the fewest presses from the attach root then the lowest trial" ++ sourceText)
        , routeEffectsSource = source
        , routeExclusions = [Exclusion playbackExclusion playbackTrials, Exclusion timeDrivenExclusion (sort (timeDrivenTrials ++ rebasedTrials))]
        , routeList = phaseRoutes ++ routes ++ declaredRoutes
        }
  either (die . ("property-pair-routes: generated file refused: " ++)) (const (pure ())) (decodeRouteFile (BL.toStrict (encodeRouteFile file)))
  BL.writeFile output (encodeRouteFile file)
  putStrLn ("property-pair-routes: " ++ show (length (routeList file)) ++ " routes")
