{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosTimer.RootPackage
  ( RootPackage, joinRootPlatform, packageDat, packageTargets, packageStartupTargets, packageCarriers, packageContract, j5Manifest
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value (..), object, (.=), encode, eitherDecodeStrict', toJSON)
import qualified Data.Aeson.KeyMap as K
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import qualified Emulation.Sha256 as SHA
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Aggregate as Aggregate
import qualified Replacement.RtosPlatform.Artifact as Platform
import qualified Replacement.RtosPlatform.Union as Union
import qualified Replacement.StartupArtifact.Compose as Startup
import qualified Replacement.StartupArtifact.Manifest as Manifest
import qualified Replacement.StartupArtifact.Targets as Targets
import qualified Replacement.StartupArtifact.Types as Startup


data RootPackage = RootPackage
  { packageDat :: B.ByteString
  , packageTargets :: Targets.TargetBytes
  , packageStartupTargets :: Targets.TargetBytes
  , packagePlatform :: Platform.Staged
  }

packageCarriers :: RootPackage -> Either String [(R.Domain, B.ByteString)]
packageCarriers RootPackage{packagePlatform = staged} = forM R.domains $ \domain -> do
  let domainMap = Union.domainMap (Platform.admittedUnion (Platform.stagedAdmitted staged)) domain
      extents = [Union.carrierExtent carrier | carrier <- Union.mapCarriers domainMap]
      R.GuestAddress base = Union.mapBase domainMap
  case extents of
    [] -> Left "root package has no platform carrier"
    _ -> do
      let low = minimum [address | R.Extent (R.GuestAddress address) _ <- extents]
          high = maximum [address | R.Extent _ (R.GuestAddress address) <- extents]
      bytes <- Aggregate.sliceAggregate (Aggregate.AggregateImage base (Platform.stagedLoader staged domain)) low (high - low)
      pure (domain, bytes)

joinRootPlatform :: Startup.Canonical -> Startup.Composition k -> B.ByteString -> Targets.TargetBytes -> Union.Union -> Platform.Linked -> Either String RootPackage
joinRootPlatform source composition startupDat observed union linked = do
  let pieces = Startup.compositionPieces composition
  forM_ [Startup.TaskStorage, Startup.RootTask, Startup.AppCallback] $ \kind ->
    unless (length [() | piece <- pieces, Startup.pieceKind piece == kind] == 1)
      (Left ("root platform requires one admitted " ++ show kind ++ " piece"))
  forM_ pieces $ \piece -> forM_ (Startup.pieceCarriers piece) $ \span_ ->
    forM_ [Union.unionA union, Union.unionB union] $ \domain ->
      forM_ (Union.mapCarriers domain) $ \carrier ->
        unless (disjoint span_ domain carrier)
          (Left ("startup carrier overlaps platform supply: " ++ show (Startup.pieceKind piece, span_, carrier)))
  verified <- shown (Startup.verifyComposed source composition startupDat
    (Targets.targetOf observed Startup.Image4) (Targets.targetOf observed Startup.Image5))
  unless (verified == observed) (Left "startup decoded targets differ from its admitted composition")
  platformSource <- shown (Platform.canonicalSource startupDat
    (Targets.targetOf observed Startup.LoaderA) (Targets.targetOf observed Startup.LoaderB))
  admitted <- shown (Platform.admit union platformSource linked)
  staged <- shown (Platform.stage admitted)
  restored <- shown (Platform.restore admitted (Platform.stagedDat staged))
  unless (restored == startupDat) (Left "platform restore did not recover the admitted startup DAT")
  let replace (target, bytes) = (target, case target of
        Startup.LoaderA -> Platform.stagedLoader staged R.DomainA
        Startup.LoaderB -> Platform.stagedLoader staged R.DomainB
        _ -> bytes)
  targets <- maybe (Left "root platform lost a decoded target") Right
    (Targets.targetBytes (M.fromList (map replace (Targets.targetList observed))))
  pure (RootPackage (Platform.stagedDat staged) targets observed staged)
  where
    shown :: Show e => Either e a -> Either String a
    shown = either (Left . show) Right
    disjoint (Startup.Span target (Startup.Guest low) (Startup.Guest high)) domain carrier =
      let domainTarget = if Union.mapDomain domain == R.DomainA then Startup.LoaderA else Startup.LoaderB
          R.Extent (R.GuestAddress start) (R.GuestAddress end) = Union.carrierExtent carrier
       in target /= domainTarget || high <= start || end <= low

packageContract :: Value -> B.ByteString -> RootPackage -> Value
packageContract startupContract startupDat package@RootPackage{packagePlatform = staged} = object
  [ "schema" .= (1 :: Int)
  , "piece_kind" .= ("timer-root-platform" :: String)
  , "startup_dat_sha256" .= SHA.sha256Hex (SHA.sha256 startupDat)
  , "candidate_dat_sha256" .= SHA.sha256Hex (SHA.sha256 (packageDat package))
  , "startup_loader_a_sha256" .= digest (packageStartupTargets package) Startup.LoaderA
  , "startup_loader_b_sha256" .= digest (packageStartupTargets package) Startup.LoaderB
  , "candidate_image4_sha256" .= digest (packageTargets package) Startup.Image4
  , "startup_contract_before_platform_rebinding" .= startupContract
  , "platform_contract" .= Platform.contractValue staged
  , "runtime_executed" .= False
  , "limits" .=
      ([ "Startup attach-state and application contracts describe the pre-platform input; their runtime witnesses must be requalified on the final package."
       , "Frame and indirect-target premises in the platform contract remain open."
       , "Byte composition does not establish continuous arena ownership or physical memory laws."
       ] :: [String])
  ]
  where
    digest targets target = SHA.sha256Hex (SHA.sha256 (Targets.targetOf targets target))

j5Manifest :: B.ByteString -> [Startup.Span] -> [FilePath] -> Either String B.ByteString
j5Manifest guardManifest appCarriers paths = do
  baseline <- either (Left . show) Right (Manifest.parseManifest guardManifest)
  unless (Startup.labelText (Manifest.manifestAttach baseline) == "primary-publisher/entry-core0")
    (Left "root platform requires the source-bound primary-publisher J5 attach guard")
  guard <- preservedGuard
  case paths of
    [first, firstSymbols, pair, pairSymbols, publisher, publisherSymbols,
     loaderA, loaderASymbols, loaderB, loaderBSymbols, dispatcher, dispatcherSymbols,
     installer, installerSymbols, submission, submissionSymbols, storage, storageSymbols,
     idle, idleSymbols, root, rootSymbols, app, appSymbols] ->
      Right (BL.toStrict (encode (object
      [ "schema" .= (4 :: Int)
      , "guard" .= guard
      , "pieces" .=
          [ single Startup.FirstStage first firstSymbols
          , single Startup.Record2Link pair pairSymbols
          , single Startup.PrimaryPublisher publisher publisherSymbols
          , object
              [ "kind" .= Startup.pieceKindName Startup.LoaderStartup
              , "inputs" .= object
                  [ "loader_a" .= linked loaderA loaderASymbols, "loader_b" .= linked loaderB loaderBSymbols
                  , "dispatcher" .= linked dispatcher dispatcherSymbols, "installer" .= linked installer installerSymbols
                  ]
              ]
          , single Startup.TaskSubmission submission submissionSymbols
          , single Startup.TaskStorage storage storageSymbols
          , single Startup.IdleTask idle idleSymbols
          , single Startup.RootTask root rootSymbols
          , single Startup.AppCallback app appSymbols
          ]
      ])))
    _ -> Left "root platform requires the complete 24-path J5 linked-input set"
  where
    preservedGuard = do
      document <- eitherDecodeStrict' guardManifest
      case document of
        Object top | Just (Object guard) <- K.lookup "guard" top
                   , Just (Object rules) <- K.lookup "attach_state" guard
                   , Just (Array carriers) <- K.lookup "carriers" rules
                   , not (null carriers) ->
          let resident (Startup.Span target (Startup.Guest start) (Startup.Guest end)) =
                object ["target" .= Startup.targetName target, "start" .= start, "end" .= end, "residency" .= ("resident" :: String)]
              extended = K.insert "carriers" (toJSON (foldr (:) (map resident appCarriers) carriers)) rules
           in Right (Object (K.insert "attach_state" (Object extended) guard))
        _ -> Left "root platform requires the existing nonempty J5 residency declaration"
    linked image symbols = object ["image" .= image, "symbols" .= symbols]
    single :: Startup.PieceKind -> FilePath -> FilePath -> Value
    single kind image symbols = object ["kind" .= Startup.pieceKindName kind, "inputs" .= linked image symbols]
