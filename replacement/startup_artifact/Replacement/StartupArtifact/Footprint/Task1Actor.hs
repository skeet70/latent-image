{-# LANGUAGE OverloadedStrings #-}
module Replacement.StartupArtifact.Footprint.Task1Actor (footprint) where

import Data.List (nub)
import Replacement.StartupArtifact.Types
import qualified Replacement.Task1Actor.Artifact as T1

footprint :: T1.Artifact -> [Footprint]
footprint artifact =
  [ Footprint (Span target (Guest address) (Guest (address+4))) RuntimePremise
      (Citation "replacement/task1_actor/task1-actor.ld:2-31; direct BL targets decoded by Replacement.Task1Actor.Artifact")
  | service <- nub (map T1.callService (T1.semanticMap artifact))
  , service /= T1.Setup
  , (bound,_,T1.CodeAddress address) <- T1.serviceBindings
  , bound == service
  , let target = if address < 0xbf1a00 then Image4 else Image5
  ]
