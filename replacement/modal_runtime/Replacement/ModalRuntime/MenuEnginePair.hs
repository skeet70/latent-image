{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | The owned menu engine's guest pair checks over RAM-push e2e observations
-- (`e2e-observation-N.json`): each role's events are classified at `0xd4dd2c`, the owned role
-- fails on an entry to `0xbfa234` from the fallback call for an owned class or on the adapter's
-- guard trap, and the two roles must read the same engine state and post the same `0x1d` events.
module Replacement.ModalRuntime.MenuEnginePair
  ( Role (..)
  , Hit (..)
  , Observation (..)
  , parseObservation
  , RoleSummary (..)
  , checkRole
  , comparePair
  , Site (..)
  , parseSite
  , fallbackReturn
  , engineReads
  ) where

import Control.Monad (foldM, forM, unless, when)
import Data.Aeson ((.:), (.:?))
import qualified Data.Aeson as J
import qualified Data.Aeson.Types as J
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (readHex)
import Replacement.ModalRuntime.MenuEngineClass

data Role = StockRole | OwnedRole deriving (Eq, Show)

-- | A trace site of the pair trajectories (`emulation/menu-engine-pair-trajectories`); every other
-- label is 'OtherSite'.
data Site = StageEntry | KindSwitch | Seek | EventPost | GuardTrap | OtherSite T.Text
  deriving (Eq, Show)

parseSite :: T.Text -> Site
parseSite label = case label of
  "menu-stage-entry" -> StageEntry
  "menu-kind-switch" -> KindSwitch
  "menu-seek" -> Seek
  "menu-event-post" -> EventPost
  "menu-engine-guard-failure" -> GuardTrap
  other -> OtherSite other

-- | One recorded trace hit: site, core, r0-r3, lr and the site's reads at entry.
data Hit = Hit
  { hitSite :: Site
  , hitCore :: Int
  , hitRegisters :: (Word32, Word32, Word32, Word32)
  , hitLr :: Maybe Word32
  , hitMemory :: [B.ByteString]
  }
  deriving (Eq, Show)

-- | One segment's observation: its phase, labelled reads and trace hits in recording order.
data Observation = Observation {observationPhase :: T.Text, observationReads :: [(T.Text, B.ByteString)], observationHits :: [Hit]}
  deriving (Eq, Show)

-- | The return address of `0xbf95c8`'s call to `0xbfa234`: the owned adapter's fallback resumes
-- stock there, and every service call into `0xbfa234` returns elsewhere.
fallbackReturn :: Word32
fallbackReturn = 0x00bf95d4

-- | The reads both roles must equal: the block, the last-move pair and the snapshot areas.
engineReads :: [T.Text]
engineReads =
  [ "menu-block", "menu-last-move", "snapshot-root"
  , "snapshot-depth1-0", "snapshot-depth1-1", "snapshot-depth2-0", "snapshot-depth2-1" ]

hexBytes :: String -> J.Parser B.ByteString
hexBytes text = B.pack <$> go text
 where
  go (high : low : rest) | [(byte, "")] <- readHex [high, low] = (byte :) <$> go rest
  go [] = pure []
  go _ = fail "not hex bytes"

hexWord :: String -> J.Parser Word32
hexWord text = case text of
  '0' : 'x' : digits | [(value, "")] <- readHex digits -> pure value
  _ -> fail ("not a hex word: " ++ text)

parseObservation :: J.Value -> J.Parser Observation
parseObservation = J.withObject "e2e observation" $ \fields -> do
  phase <- fields .: "phase"
  segmentReads <- fields .: "reads" >>= mapM (J.withObject "read" $ \read_ -> (,) <$> read_ .: "label" <*> (hexBytes =<< read_ .: "bytes"))
  hits <- fields .: "trace_hits" >>= mapM (J.withObject "trace hit" $ \hit -> do
    let register name = hexWord =<< hit .: name
    registers <- (,,,) <$> register "r0" <*> register "r1" <*> register "r2" <*> register "r3"
    lr <- traverse hexWord =<< hit .:? "lr"
    memory <- maybe (pure []) (mapM hexBytes) =<< hit .:? "memory"
    Hit <$> (parseSite <$> hit .: "site") <*> hit .: "core" <*> pure registers <*> pure lr <*> pure memory)
  pure (Observation phase segmentReads hits)

-- | A role's events by class and its fallback-site entries.
data RoleSummary = RoleSummary
  { summaryClasses :: M.Map String Int
  , summaryFallbackEntries :: Int
  , summaryEvents :: Int
  }
  deriving (Eq, Show)

data Walk = Walk {walkLatest :: M.Map Int EventClass, walkSummary :: RoleSummary, walkFailures :: [String]}

-- | Classifies each stage entry from its registers and its block read; for the owned role, an
-- entry to `0xbfa234` returning to 'fallbackReturn' fails when the latest stage entry on its core
-- has an owned class, and any guard-trap entry fails.
checkRole :: MenuTree -> Role -> [Observation] -> Either String RoleSummary
checkRole tree role observations = do
  walk <- foldM step (Walk M.empty (RoleSummary M.empty 0 0) [])
    [(observationPhase observation, hit) | observation <- observations, hit <- observationHits observation]
  case walkFailures walk of
    [] -> Right (walkSummary walk)
    failures -> Left (unlines (reverse failures))
 where
  step walk (phase, hit)
    | hitSite hit == StageEntry = do
        block <- case hitMemory hit of
          bytes : _ -> menuBlock bytes
          [] -> Left ("stage entry without its block read in " ++ T.unpack phase)
        let (kind, node, ident, extra) = hitRegisters hit
            class_ = deriveClass tree block (StageEvent kind node ident extra)
            summary = walkSummary walk
        pure walk
          { walkLatest = M.insert (hitCore hit) class_ (walkLatest walk)
          , walkSummary = summary
              { summaryClasses = M.insertWith (+) (show class_) 1 (summaryClasses summary)
              , summaryEvents = summaryEvents summary + 1 } }
    | hitSite hit == KindSwitch && hitLr hit == Just fallbackReturn = do
        let summary = walkSummary walk
            counted = walk {walkSummary = summary {summaryFallbackEntries = summaryFallbackEntries summary + 1}}
        pure $ case (role, M.lookup (hitCore hit) (walkLatest walk)) of
          (OwnedRole, Just class_) | isOwned class_ ->
            counted {walkFailures = ("fallback entry for owned class " ++ show class_ ++ " in " ++ T.unpack phase) : walkFailures walk}
          (OwnedRole, Nothing) ->
            counted {walkFailures = ("fallback entry with no stage entry on its core in " ++ T.unpack phase) : walkFailures walk}
          _ -> counted
    | hitSite hit == KindSwitch && hitLr hit == Nothing =
        Left ("kind-switch entry without lr in " ++ T.unpack phase)
    | hitSite hit == GuardTrap =
        pure walk {walkFailures = ("adapter guard trap entered in " ++ T.unpack phase) : walkFailures walk}
    | otherwise = pure walk

-- | Both roles ran the same segments, read the same engine state at the end of each and posted the
-- same `0x1d` events in the same order.
comparePair :: [Observation] -> [Observation] -> Either String ()
comparePair stock owned = do
  unless (map observationPhase stock == map observationPhase owned)
    (Left "the roles' observations have different phases")
  differences <- fmap concat . forM (zip stock owned) $ \(left, right) ->
    forM engineReads (\label -> do
      let lookupRead observation = case [bytes | (name, bytes) <- observationReads observation, name == label] of
            [bytes] -> Right bytes
            _ -> Left ("read " ++ T.unpack label ++ " is missing or repeated in " ++ T.unpack (observationPhase observation))
      a <- lookupRead left
      b <- lookupRead right
      pure [T.unpack (observationPhase left) ++ " " ++ T.unpack label | a /= b])
  when (concat differences /= []) (Left ("engine reads differ: " ++ unwords (concat differences)))
  let posts observations = [(r1, r2, r3) | observation <- observations, hit <- observationHits observation
        , hitSite hit == EventPost, let (r0, r1, r2, r3) = hitRegisters hit, r0 == 0x1d]
  unless (posts stock == posts owned)
    (Left ("0x1d posts differ: stock " ++ show (posts stock) ++ ", owned " ++ show (posts owned)))
