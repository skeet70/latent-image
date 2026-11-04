{-# LANGUAGE OverloadedStrings #-}
-- | The contract's @application@ section: task1, pump, queue1-adapter, UI
-- library, publication and initializer sites with the code word each holds,
-- the shared-queue scope sites, both domains' task cells, digests of the
-- task1 and pump code, and the readiness bounds. Sites and cells are
-- emulation/fujimod-xm1/src/application_pump_capture.rs:12-35 plus the image5
-- producer's entry and return (lz/id4.dis:144722) and the initializer chain
-- (lz/id4.dis:132029-132030, lz/id5.dis:431838-431843).
module Replacement.StartupArtifact.Application
  ( SiteKind (..), siteKindName, SiteLabel (..), CodeWord (..), Sha256Hex (..)
  , CodeRegion (..), codeRegionName, codeRegionBounds
  , PlainImage, plainImage, RoleImages (..)
  , ApplicationSite (..), QueueSite (..), CodeDigest (..), Application (..)
  , application, applicationValue
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Maybe (isJust, listToMaybe)
import Data.Text (Text)
import qualified Data.Text as T
import Data.Word (Word32, Word64)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.StartupArtifact.Targets
import Replacement.StartupArtifact.Types

data SiteKind = Task1Site | PumpSite | HookTargetSite | AdapterSite | UilibSite | SharedSite | PublicationSite | InitializerSite
  deriving (Eq, Ord, Show, Enum, Bounded)

siteKindName :: SiteKind -> Text
siteKindName kind = case kind of
  Task1Site -> "task1"
  PumpSite -> "pump"
  HookTargetSite -> "hook-target"
  AdapterSite -> "adapter"
  UilibSite -> "uilib"
  SharedSite -> "shared"
  PublicationSite -> "publication"
  InitializerSite -> "initializer"

-- | @app-@ or @queue-@ and the PC in lowercase hex.
newtype SiteLabel = SiteLabel Text deriving (Eq, Ord, Show)

prefixed :: Text -> Pc -> SiteLabel
prefixed prefix (Pc pc) = SiteLabel (prefix <> T.pack (showHex pc ""))

-- | A little-endian instruction word.
newtype CodeWord = CodeWord Word32 deriving (Eq, Show)

newtype Sha256Hex = Sha256Hex Text deriving (Eq, Show)

sha256 :: B.ByteString -> Sha256Hex
sha256 = Sha256Hex . T.pack . SHA.sha256Hex . SHA.sha256

data CodeRegion = Task1Body | Task1Setup | PumpCode deriving (Eq, Ord, Show, Enum, Bounded)

codeRegionName :: CodeRegion -> Text
codeRegionName region = case region of
  Task1Body -> "task1-body"
  Task1Setup -> "task1-setup"
  PumpCode -> "pump"

-- | [start, end) in image5.
codeRegionBounds :: CodeRegion -> (Word32, Word32)
codeRegionBounds region = case region of
  Task1Body -> (0xc22f60, 0xc2327c)
  Task1Setup -> (0xbfb124, 0xbfb1b8)
  PumpCode -> (0xbfbd20, 0xbfbd40)

sites :: [(Pc, SiteKind)]
sites = [(Pc pc, kind) | (kind, pcs) <- table, pc <- pcs]
  where
    table =
      [ (Task1Site, [ 0xc22f60, 0xc22f64, 0xc22f6c, 0xc22f70, 0xc22f74, 0xc22f84, 0xbfb124, 0xc22bd0, 0xc22fb4, 0xc22fd4, 0xc22fd8
                    , 0xc23094, 0xc230a0, 0xc230b4, 0xc230c0, 0xc230d8, 0xc230f4, 0xc23104, 0xc23114, 0xc2311c, 0xc2312c, 0xc2314c
                    , 0xc23150, 0xc231ec, 0xc231fc, 0xc23274, 0xc23278 ])
      , (PumpSite, [0xbfbd20, 0xbfbd24 .. 0xbfbd3c])
      , (HookTargetSite, [0xbfb1b8, 0xbfb20c, 0xbfb1e0])
      , (AdapterSite, [0xdb5124, 0xdb515c, 0xdb5170, 0xdb5174, 0xdb5178, 0xdb51d0])
      , (UilibSite, [0x11090a4, 0x1109130, 0x1108a2c, 0x110f9d8])
      , (SharedSite, [0x725214])
      , (PublicationSite, [0x6a9840, 0x6a9844, 0x6a9934])
      , (InitializerSite, [0x69d2e0, 0x69d2e4, 0xd985a4, 0xd985c8, 0xd985d8])
      ]

queueSites :: [Pc]
queueSites = map Pc [0x725318, 0x725378, 0x72537c, 0x7253c0, 0x7253f8, 0x7253fc, 0x725414, 0x725418, 0x72541c]

data Domain = Domain0 | Domain1 deriving (Eq, Show)

data TaskLayout = TaskLayout
  { layoutDomain :: Domain, layoutCreation :: Guest, layoutTask :: Guest, layoutIds :: Guest
  , layoutDomains :: Guest, layoutCurrent :: Guest, layoutSelected :: Guest }

taskLayouts :: [TaskLayout]
taskLayouts =
  [ TaskLayout Domain0 (Guest 0x4005beac) (Guest 0x4005ff0c) (Guest 0x400622d4) (Guest 0x400545cc) (Guest 0x40054760) (Guest 0x40054768)
  , TaskLayout Domain1 (Guest 0x400ebe6c) (Guest 0x400efecc) (Guest 0x400f2294) (Guest 0x400e458c) (Guest 0x400e4720) (Guest 0x400e4728) ]

newtype Instructions = Instructions Word64

-- | Readiness settle interval and the search end, the first press.
readinessSettle, readinessEnd :: Instructions
readinessSettle = Instructions 1000000
readinessEnd = Instructions 80000000

-- | The plain window's media bytes at its load address.
data PlainImage = PlainImage Guest B.ByteString deriving (Eq, Show)

plainImage :: U.Parts -> Either Refusal PlainImage
plainImage parts = do
  U.MediaImage bytes <- either (const (Left (ApplicationRefused PlainWindowUnreadable))) Right (U.extractPlain parts)
  let layout = U.partTableLayout parts
  pure (PlainImage (Guest (U.mediaWordAt (U.partTableTail parts) (U.plainLoadOffset layout - U.tableEndOffset layout))) bytes)

-- | The code one role runs: the decoded targets and the plain window.
data RoleImages = RoleImages { roleTargets :: TargetBytes, rolePlain :: PlainImage }

-- | The bytes of [start, end) and the stager target holding them; the plain window is none.
codeAt :: RoleImages -> Word32 -> Word32 -> Maybe (Maybe Target, B.ByteString)
codeAt (RoleImages targets (PlainImage (Guest load) plain)) start end = listToMaybe
  [ (owner, B.take (fromIntegral (end - start)) (B.drop (fromIntegral (start - base)) bytes))
  | (owner, base, bytes) <- [(Just Image4, targetBase Image4, targetOf targets Image4), (Just Image5, targetBase Image5, targetOf targets Image5), (Nothing, load, plain)]
  , base <= start, start < end, toInteger end - toInteger base <= toInteger (B.length bytes) ]

data ApplicationSite = ApplicationSite { siteLabel :: SiteLabel, siteKind :: SiteKind, sitePc :: Pc, siteWord :: CodeWord } deriving (Eq, Show)

data QueueSite = QueueSite { queueLabel :: SiteLabel, queuePc :: Pc, queueWord :: CodeWord } deriving (Eq, Show)

data CodeDigest = CodeDigest { digestRegion :: CodeRegion, digestRetained :: Sha256Hex, digestComposed :: Sha256Hex } deriving (Eq, Show)

-- | Sites and queue sites by PC.
data Application = Application { applicationSites :: [ApplicationSite], applicationQueueSites :: [QueueSite], applicationCode :: [CodeDigest] }
  deriving (Eq, Show)

refuse :: ApplicationProblem -> Either Refusal a
refuse = Left . ApplicationRefused

-- | Carriers are the composition's, by owning piece; a site is stock in both roles.
application :: [(PieceKind, Span)] -> RoleImages -> RoleImages -> Either Refusal Application
application carriers retained composed = do
  appSites <- forM (sortOn fst sites) $ \(pc, kind) -> ApplicationSite (prefixed "app-" pc) kind pc <$> word pc
  queues <- forM queueSites $ \pc -> QueueSite (prefixed "queue-" pc) pc <$> word pc
  digests <- forM [minBound .. maxBound] $ \region -> do
    let (start, end) = codeRegionBounds region
        bytesIn images = maybe (refuse (OutsideImages (Pc start))) (pure . snd) (codeAt images start end)
    CodeDigest region <$> (sha256 <$> bytesIn retained) <*> (sha256 <$> bytesIn composed)
  pure (Application appSites queues digests)
  where
    word pc@(Pc address) = do
      let end = address + 4
          inImages images = maybe (refuse (OutsideImages pc)) pure (codeAt images address end)
      (owner, canonicalBytes) <- inImages retained
      forM_ owner $ \target -> forM_ carriers $ \(kind, carrier) ->
        when (isJust (intersectSpan (Span target (Guest address) (Guest end)) carrier)) (refuse (SiteInCarrier kind pc))
      (_, composedBytes) <- inImages composed
      unless (composedBytes == canonicalBytes) (refuse (SiteWordDiffers pc))
      pure (CodeWord (U.word32At composedBytes 0))

applicationValue :: Application -> Value
applicationValue (Application appSites queues digests) = object
  [ "sites" .= [ object [ "label" .= labelOf name, "kind" .= siteKindName kind, "retained_pc" .= pc, "composed_pc" .= pc, "word" .= value ]
               | ApplicationSite name kind (Pc pc) (CodeWord value) <- appSites ]
  , "queue_sites" .= [object ["label" .= labelOf name, "pc" .= pc, "word" .= value] | QueueSite name (Pc pc) (CodeWord value) <- queues]
  , "task_layouts" .= map layoutValue taskLayouts
  , "code" .= [ object [ "label" .= codeRegionName region, "start" .= fst (codeRegionBounds region), "end" .= snd (codeRegionBounds region)
                       , "retained_sha256" .= before, "composed_sha256" .= after ]
              | CodeDigest region (Sha256Hex before) (Sha256Hex after) <- digests ]
  , "readiness" .= object ["settle" .= count readinessSettle, "end" .= count readinessEnd]
  ]
  where
    labelOf (SiteLabel name) = name
    count (Instructions value) = value
    guest (Guest value) = value
    layoutValue layout = object
      [ "domain" .= (case layoutDomain layout of Domain0 -> 0 :: Int; Domain1 -> 1)
      , "creation" .= guest (layoutCreation layout), "task" .= guest (layoutTask layout), "ids" .= guest (layoutIds layout)
      , "domains" .= guest (layoutDomains layout), "current" .= guest (layoutCurrent layout), "selected" .= guest (layoutSelected layout) ]
