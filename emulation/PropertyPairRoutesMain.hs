-- | property-pair-routes CRAWL_GRAPH OUTPUT: writes the natural-pair route file from a menu crawl.
module Main (main) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256, sha256Hex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (die)

main :: IO ()
main = getArgs >>= \arguments -> case arguments of
  [graphPath, output] -> do
    exists <- doesPathExist output
    if exists then die "property-pair-routes: OUTPUT exists" else pure ()
    bytes <- B.readFile graphPath
    edges <- either (die . ("property-pair-routes: " ++)) pure (decodeCrawlEdges bytes)
    let (crawlRoutes, playbackTrials, timeDrivenTrials) = selectCrawlRoutes edges
        file = RouteFile
          { routeCrawlGraph = sha256Hex (sha256 bytes)
          , routeSelection = T.pack "reached crawl edges with a crawl-property-setter or crawl-property-commit effect, other than edges leaving the no-card playback screen and edges whose only effects are the 0x1c9/0x1c8/0x27f/0x280 block, one per ordered effect sequence (setter property ids and commits), the fewest presses from the attach root then the lowest trial"
          , routeExclusions = [Exclusion playbackExclusion playbackTrials, Exclusion timeDrivenExclusion timeDrivenTrials]
          , routeList = phaseRoutes ++ crawlRoutes ++ declaredRoutes
          }
    either (die . ("property-pair-routes: generated file refused: " ++)) (const (pure ())) (decodeRouteFile (BL.toStrict (encodeRouteFile file)))
    BL.writeFile output (encodeRouteFile file)
    putStrLn ("property-pair-routes: " ++ show (length (routeList file)) ++ " routes")
  _ -> die "usage: property-pair-routes CRAWL_GRAPH OUTPUT"
