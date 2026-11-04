{-# LANGUAGE OverloadedStrings #-}

-- | Admission, staging and restore of the owned RTOS slot bodies in place over loaders A and B.
module Replacement.RtosOwnFirst.Artifact
  ( CanonicalSource
  , LinkedDomain (..)
  , Admitted
  , Staged
  , SourceRole (..)
  , TableFault (..)
  , FrameSite (..)
  , StagingStep (..)
  , StageError (..)
  , EntryRow (..)
  , canonicalSource
  , requirePinned
  , admit
  , stage
  , restore
  , verifyRestore
  , admittedTable
  , admittedLoader
  , admittedFrame
  , entryTable
  , stagedDat
  , stagedLoader
  , contractValue
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Aeson.Types (Pair)
import Data.Bits (complement, (.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (find, sortOn)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Frame
import Replacement.RtosOwnFirst.Regions

data CanonicalSource = CanonicalSource
  { sourceDat :: B.ByteString
  , sourceParsed :: U.Parsed
  , -- | The supplied loader files.
    sourceLoaders :: M.Map Domain B.ByteString
  , -- | The loaders the DAT's pre-table holds.
    sourceContained :: M.Map Domain B.ByteString
  }

-- | One domain's `xm1_rust_elf` outputs: the flat image from the lowest extent start, and its llvm-nm list.
data LinkedDomain = LinkedDomain {linkedBin :: B.ByteString, linkedSymbols :: B.ByteString}

data InstalledExtent = InstalledExtent
  { installedRegion :: Region
  , installedStock :: B.ByteString
  , installedOwned :: B.ByteString
  , -- | Slack words the stager wrote with the fill word.
    installedFilled :: Int
  }

data AdmittedDomain = AdmittedDomain
  { domainRows :: DomainTable
  , domainExtentsInstalled :: [InstalledExtent]
  , domainLoader :: B.ByteString
  , domainFrames :: M.Map GuestAddress Frame
  , domainLinked :: LinkedDomain
  }

data Admitted = Admitted
  { admittedRegionsJson :: B.ByteString
  , admittedTable :: RegionTable
  , admittedDomains :: M.Map Domain AdmittedDomain
  }

data Staged = Staged
  { stagedSource :: CanonicalSource
  , stagedAdmitted :: Admitted
  , stagedDat :: B.ByteString
  , stagedPatched :: M.Map Domain Int
  }

data SourceRole = SourceDat | SourceLoader Domain deriving (Eq, Show)

data TableFault
  = BaseMismatch
  | FillWordMismatch
  | ExtentMalformed SlotOffset
  | ExtentsOverlap SlotOffset SlotOffset
  | SlotRepeated SlotOffset
  | TailSlotMismatch
  | BodyOutsideHost
  deriving (Eq, Show)

data FrameSite = SlotFrame SlotOffset | TailBodyFrame deriving (Eq, Show)

data StagingStep = PatchStep | RebuildStep | ReparseStep | PreTableStep deriving (Eq, Show)

data StageError
  = ContainerRejected
  | SegmentRejected Domain
  | LoaderMismatch Domain
  | SourceUnpinned SourceRole
  | RegionsUnparsed String
  | TableRejected Domain TableFault
  | StockMismatch Domain SlotOffset
  | SymbolsUnparsed Domain
  | SymbolMissing Domain SymbolName
  | EntryNotAtStart Domain SlotOffset
  | BodyNotAtAddress Domain
  | SymbolOutsideRegions Domain SymbolName
  | OwnedByteOutsideRegions Domain GuestAddress
  | FrameRejected Domain FrameError
  | -- | Declared, then walked.
    FrameMismatch Domain FrameSite FrameBytes FrameBytes
  | StagingRejected StagingStep String
  | -- | A DAT offset.
    ChangeOutsideRegions Int
  | StagedLoaderMismatch Domain
  | RestoreRejected StagingStep String
  | RestoreMismatch
  deriving (Eq, Show)

-- | A stock entry and the owned symbol linked at it.
data EntryRow = EntryRow
  { entrySlot :: SlotOffset
  , entryStock :: GuestAddress
  , entrySymbol :: SymbolName
  , entryOwned :: GuestAddress
  }
  deriving (Eq, Show)

require :: Bool -> StageError -> Either StageError ()
require premise failure = unless premise (Left failure)

digest :: B.ByteString -> String
digest = SHA.sha256Hex . SHA.sha256

fillWord :: Word32
fillWord = 0xe7f000f0

pins :: [(SourceRole, String)]
pins =
  [ (SourceDat, "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
  , (SourceLoader DomainA, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
  , (SourceLoader DomainB, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
  ]

segmentOf :: U.Parts -> Domain -> Either StageError U.SegmentEntry
segmentOf parts domain = do
  segment <- case filter ((== domainLoaderId domain) . U.segId) (U.partSegments parts) of
    [found] -> Right found
    _ -> Left (SegmentRejected domain)
  let GuestAddress base = domainLoaderBase domain
      size = fromIntegral (U.segSize segment)
  require (U.segLoad segment == base && size == domainLoaderSize domain
    && fromIntegral (U.segFlash segment) + size <= B.length (U.partPreTable parts)) (SegmentRejected domain)
  pure segment

containedLoader :: U.Parts -> Domain -> Either StageError B.ByteString
containedLoader parts domain = do
  segment <- segmentOf parts domain
  pure (B.map complement (B.take (fromIntegral (U.segSize segment)) (B.drop (fromIntegral (U.segFlash segment)) (U.partPreTable parts))))

-- | The DAT, which must rebuild to itself, and the loader A and B files.
canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> Either StageError CanonicalSource
canonicalSource dat loaderA loaderB = do
  parsed <- either (const (Left ContainerRejected)) Right (U.parseContainer dat)
  require (U.rebuildContainer parsed (U.parsedParts parsed) == Right dat) ContainerRejected
  contained <- forM domains $ \domain -> (,) domain <$> containedLoader (U.parsedParts parsed) domain
  let given = M.fromList (zip domains [loaderA, loaderB])
  forM_ domains $ \domain -> require (B.length (given M.! domain) == domainLoaderSize domain) (LoaderMismatch domain)
  pure (CanonicalSource dat parsed given (M.fromList contained))

-- | The X-M1 1.50 DAT and its two loaders.
requirePinned :: CanonicalSource -> Either StageError ()
requirePinned source = forM_ pins $ \(role, expected) ->
  let actual = case role of
        SourceDat -> sourceDat source
        SourceLoader domain -> sourceLoaders source M.! domain
   in require (digest actual == expected) (SourceUnpinned role)

slice :: GuestAddress -> Extent -> B.ByteString -> B.ByteString
slice (GuestAddress base) extent@(Extent (GuestAddress start) _) =
  B.take (extentBytes extent) . B.drop (fromIntegral (start - base))

replaceExtents :: GuestAddress -> [(Extent, B.ByteString)] -> B.ByteString -> B.ByteString
replaceExtents (GuestAddress base) replacements original = foldl put original replacements
  where
    put bytes (Extent (GuestAddress start) _, replacement) =
      let at = fromIntegral (start - base) in B.take at bytes <> replacement <> B.drop (at + B.length replacement) bytes

checkTable :: Word32 -> Domain -> DomainTable -> Either StageError ()
checkTable fill domain table = do
  let fault = Left . TableRejected domain
      GuestAddress loaderBase = domainLoaderBase domain
      loaderEnd = loaderBase + fromIntegral (domainLoaderSize domain)
      rows = domainExtents table
      sorted = sortOn (extentStart . regionExtent) rows
  unless (tableBase table == domainLoaderBase domain) (fault BaseMismatch)
  unless (fill == fillWord) (fault FillWordMismatch)
  unless (regionSlot (tableTail table) == SlotOffset 0x1618) (fault TailSlotMismatch)
  forM_ rows $ \region -> do
    let Extent (GuestAddress start) (GuestAddress end) = regionExtent region
    unless (start .&. 3 == 0 && end .&. 3 == 0 && start < end && start >= loaderBase && end <= loaderEnd) (fault (ExtentMalformed (regionSlot region)))
    require (regionEntry region == extentStart (regionExtent region)) (EntryNotAtStart domain (regionSlot region))
  forM_ (zip sorted (drop 1 sorted)) $ \(left, right) ->
    unless (extentEnd (regionExtent left) <= extentStart (regionExtent right)) (fault (ExtentsOverlap (regionSlot left) (regionSlot right)))
  forM_ (zip [0 :: Int ..] rows) $ \(index, region) ->
    when (regionSlot region `elem` map regionSlot (take index rows)) (fault (SlotRepeated (regionSlot region)))
  let body = tableBody table
      Extent bodyStart@(GuestAddress bodyLow) bodyEnd@(GuestAddress bodyHigh) = bodyExtent body
  case find ((== bodyHost body) . regionSlot) (tableRegions table) of
    Just host
      | bodyLow .&. 3 == 0 && bodyHigh .&. 3 == 0 && bodyStart < bodyEnd && bodyStart > extentStart (regionExtent host)
          && extentContains (regionExtent host) bodyStart && bodyEnd <= extentEnd (regionExtent host) -> Right ()
    _ -> fault BodyOutsideHost

-- | An llvm-nm row: address, size, type letter, name.
data SymbolRow = SymbolRow {rowSymbol :: F.Symbol, rowAbsolute :: Bool}

symbolRows :: Domain -> B.ByteString -> Either StageError [SymbolRow]
symbolRows domain bytes = do
  symbols <- either (const (Left (SymbolsUnparsed domain))) Right (F.parseSymbols bytes)
  let kinds = [kind | row <- C.lines bytes, not (C.null row), [_, _, kind, _] <- [C.words row]]
  require (length kinds == length symbols) (SymbolsUnparsed domain)
  pure (zipWith (\symbol kind -> SymbolRow symbol (kind == "A")) symbols kinds)

placedAt :: Domain -> [SymbolRow] -> SymbolName -> Either StageError Word32
placedAt domain rows name@(SymbolName text) = case [rowSymbol row | row <- rows, F.symbolName (rowSymbol row) == text, not (rowAbsolute row)] of
  [symbol] -> Right (F.symbolAddress symbol)
  _ -> Left (SymbolMissing domain name)

firstNonzero :: Word32 -> B.ByteString -> Maybe GuestAddress
firstNonzero start bytes = (\index -> GuestAddress (start + fromIntegral index)) <$> B.findIndex (/= 0) bytes

-- | The owned bytes of one extent, with the trailing run of zero or fill words past the last symbol written as fill.
installExtent :: Word32 -> GuestAddress -> B.ByteString -> [SymbolRow] -> Region -> B.ByteString -> InstalledExtent
installExtent fill binBase bin rows region stock =
  InstalledExtent region stock (B.concat (map U.putWord32 installed)) (length [() | (index, word) <- indexed, index >= slackFrom, word == 0])
  where
    extent@(Extent (GuestAddress start) _) = regionExtent region
    raw = slice binBase extent bin
    padded = raw <> B.replicate (extentBytes extent - B.length raw) 0
    words32 = [U.word32At padded offset | offset <- [0, 4 .. extentBytes extent - 4]]
    indexed = zip [0 :: Int ..] words32
    symbolEnd = maximum (start : [F.symbolAddress symbol + F.symbolSize symbol | SymbolRow symbol False <- rows, extentContains extent (GuestAddress (F.symbolAddress symbol))])
    firstFree = fromIntegral ((symbolEnd - start + 3) `div` 4)
    trailing = length (takeWhile (`elem` [0, fill]) (reverse words32))
    slackFrom = max firstFree (length words32 - trailing)
    installed = [if index >= slackFrom && word == 0 then fill else word | (index, word) <- indexed]

admitDomain :: CanonicalSource -> Word32 -> Domain -> DomainTable -> LinkedDomain -> Either StageError AdmittedDomain
admitDomain source fill domain table linked = do
  checkTable fill domain table
  let base = domainLoaderBase domain
      given = sourceLoaders source M.! domain
      contained = sourceContained source M.! domain
      rows = domainExtents table
      extents = map regionExtent rows
  forM_ rows $ \region ->
    require (slice base (regionExtent region) given == slice base (regionExtent region) contained) (StockMismatch domain (regionSlot region))
  require (given == contained) (LoaderMismatch domain)
  symbols <- symbolRows domain (linkedSymbols linked)
  forM_ rows $ \region -> do
    address <- placedAt domain symbols (regionSymbol region)
    require (GuestAddress address == extentStart (regionExtent region)) (EntryNotAtStart domain (regionSlot region))
  let body = tableBody table
  bodyAddress <- placedAt domain symbols (bodySymbol body)
  require (GuestAddress bodyAddress == extentStart (bodyExtent body)) (BodyNotAtAddress domain)
  forM_ [rowSymbol row | row <- symbols, not (rowAbsolute row)] $ \symbol -> do
    let low = F.symbolAddress symbol
        high = toInteger low + toInteger (F.symbolSize symbol)
        inside (Extent (GuestAddress start) (GuestAddress end)) =
          low >= start && high <= toInteger end && (F.symbolSize symbol == 0 || low < end)
    require (any inside extents) (SymbolOutsideRegions domain (SymbolName (F.symbolName symbol)))
  let ordered = sortOn extentStart extents
      binBase@(GuestAddress binLow) = minimum (map extentStart extents)
      GuestAddress binHigh = maximum (map extentEnd extents)
      bin = linkedBin linked
      offset (GuestAddress address) = fromIntegral (address - binLow)
      gaps = zip (map extentEnd ordered) (map extentStart (drop 1 ordered))
  require (B.length bin <= fromIntegral (binHigh - binLow)) (OwnedByteOutsideRegions domain (GuestAddress binHigh))
  forM_ gaps $ \(gapStart@(GuestAddress low), gapEnd) ->
    forM_ (firstNonzero low (B.take (offset gapEnd - offset gapStart) (B.drop (offset gapStart) bin))) $
      Left . OwnedByteOutsideRegions domain
  let installed = [installExtent fill binBase bin symbols region (slice base (regionExtent region) given) | region <- rows]
      loader = replaceExtents base [(regionExtent (installedRegion entry), installedOwned entry) | entry <- installed] given
      bodyStart = extentStart (bodyExtent body)
  walked <- either (Left . FrameRejected domain) Right
    (frames (OwnedCode base loader extents) (map regionEntry rows ++ [bodyStart]))
  let composed address = frameComposed (walked M.! address)
  forM_ rows $ \region ->
    require (composed (regionEntry region) == regionFrame region)
      (FrameMismatch domain (SlotFrame (regionSlot region)) (regionFrame region) (composed (regionEntry region)))
  require (composed bodyStart == bodyFrame body) (FrameMismatch domain TailBodyFrame (bodyFrame body) (composed bodyStart))
  pure (AdmittedDomain table installed loader walked linked)

-- | Admits the regions.json bytes and both domains' linked bodies over the canonical loaders.
admit :: CanonicalSource -> B.ByteString -> LinkedDomain -> LinkedDomain -> Either StageError Admitted
admit source regionsJson linkedA linkedB = do
  table <- either (Left . RegionsUnparsed) Right (parseRegionTable regionsJson)
  admitted <- forM (zip domains [linkedA, linkedB]) $ \(domain, linked) ->
    (,) domain <$> admitDomain source (tableFill table) domain (domainTable table domain) linked
  pure (Admitted regionsJson table (M.fromList admitted))

admittedLoader :: Admitted -> Domain -> B.ByteString
admittedLoader admitted domain = domainLoader (admittedDomains admitted M.! domain)

admittedFrame :: Admitted -> Domain -> GuestAddress -> Maybe Frame
admittedFrame admitted domain address = M.lookup address (domainFrames (admittedDomains admitted M.! domain))

entryTable :: Admitted -> Domain -> [EntryRow]
entryTable admitted domain =
  [ EntryRow (regionSlot region) (regionEntry region) (regionSymbol region) (extentStart (regionExtent region))
  | region <- domainExtents (domainRows (admittedDomains admitted M.! domain))
  ]

stagedLoader :: Staged -> Domain -> B.ByteString
stagedLoader staged = admittedLoader (stagedAdmitted staged)

-- | Loader word patches turning `from` into `to` over every installed extent of `domain`.
extentPatches :: Domain -> Admitted -> (Region -> B.ByteString) -> (Region -> B.ByteString) -> Either String [F.LoaderWordPatch]
extentPatches domain admitted from to =
  sequence
    [ F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) (start + fromIntegral offset) before after
    | entry <- domainExtentsInstalled (admittedDomains admitted M.! domain)
    , let region = installedRegion entry
          GuestAddress start = extentStart (regionExtent region)
    , offset <- [0, 4 .. extentBytes (regionExtent region) - 4]
    , let before = U.word32At (from region) offset
          after = U.word32At (to region) offset
    , before /= after
    ]

-- | Applies the patches through the pre-table path and returns the rebuilt DAT.
rebuildWith :: (StagingStep -> String -> StageError) -> U.Parsed -> [F.LoaderWordPatch] -> Either StageError B.ByteString
rebuildWith refuse parsed patches = do
  let step :: StagingStep -> Either String a -> Either StageError a
      step label = either (Left . refuse label) Right
  parts <- step PatchStep (F.applyLoaderWordPatches patches (U.parsedParts parsed))
  bytes <- step RebuildStep (U.rebuildContainer parsed parts)
  consumed <- step ReparseStep (U.parseContainer bytes)
  step PreTableStep (F.verifyPreTablePatches [] patches parsed consumed)
  pure bytes

-- | DAT byte ranges, as (offset, length), holding each installed extent.
extentRanges :: CanonicalSource -> Admitted -> Either StageError [(Int, Int)]
extentRanges source admitted = do
  let parsed = sourceParsed source
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  fmap concat $ forM domains $ \domain -> do
    segment <- segmentOf (U.parsedParts parsed) domain
    pure
      [ (mainStart + fromIntegral (U.segFlash segment) + fromIntegral (start - U.segLoad segment), extentBytes extent)
      | entry <- domainExtentsInstalled (admittedDomains admitted M.! domain)
      , let extent@(Extent (GuestAddress start) _) = regionExtent (installedRegion entry)
      ]

firstChangeOutside :: [(Int, Int)] -> B.ByteString -> B.ByteString -> Maybe Int
firstChangeOutside allowed before after = find outside [index | (index, True) <- zip [0 ..] (B.zipWith (/=) before after)]
  where
    outside index = not (any (\(start, size) -> index >= start && index < start + size) allowed)

stage :: CanonicalSource -> Admitted -> Either StageError Staged
stage source admitted = do
  patched <- forM domains $ \domain -> do
    let given = sourceLoaders source M.! domain
        base = domainLoaderBase domain
        owned = M.fromList [(regionSlot (installedRegion entry), installedOwned entry) | entry <- domainExtentsInstalled (admittedDomains admitted M.! domain)]
    patches <- either (Left . StagingRejected PatchStep) Right
      (extentPatches domain admitted (\region -> slice base (regionExtent region) given) ((owned M.!) . regionSlot))
    pure (domain, patches)
  let patches = concatMap snd patched
      dat = sourceDat source
  staged <- rebuildWith StagingRejected (sourceParsed source) patches
  ranges <- extentRanges source admitted
  require (B.length staged == B.length dat) (ChangeOutsideRegions (B.length dat))
  forM_ (firstChangeOutside ((U.mainChecksumOffset, 4) : ranges) dat staged) (Left . ChangeOutsideRegions)
  consumed <- either (const (Left ContainerRejected)) Right (U.parseContainer staged)
  forM_ domains $ \domain -> do
    loader <- containedLoader (U.parsedParts consumed) domain
    require (loader == admittedLoader admitted domain) (StagedLoaderMismatch domain)
  verifyRestore source admitted staged
  pure (Staged source admitted staged (M.fromList [(domain, length list) | (domain, list) <- patched]))

-- | The DAT with every installed extent written back to the canonical loader's bytes.
restore :: CanonicalSource -> Admitted -> B.ByteString -> Either StageError B.ByteString
restore source admitted dat = do
  parsed <- either (Left . RestoreRejected ReparseStep) Right (U.parseContainer dat)
  patches <- fmap concat $ forM domains $ \domain -> do
    current <- containedLoader (U.parsedParts parsed) domain
    let base = domainLoaderBase domain
        given = sourceLoaders source M.! domain
    either (Left . RestoreRejected PatchStep) Right
      (extentPatches domain admitted (\region -> slice base (regionExtent region) current) (\region -> slice base (regionExtent region) given))
  rebuildWith RestoreRejected parsed patches

verifyRestore :: CanonicalSource -> Admitted -> B.ByteString -> Either StageError ()
verifyRestore source admitted dat = do
  restored <- restore source admitted dat
  require (restored == sourceDat source) RestoreMismatch

slotText :: SlotOffset -> String
slotText (SlotOffset offset) = hexText offset

addressText :: GuestAddress -> String
addressText (GuestAddress address) = hexText address

symbolText :: SymbolName -> String
symbolText (SymbolName name) = name

frameValues :: Frame -> [Pair]
frameValues frame =
  [ "frame_bytes" .= bytes (frameComposed frame)
  , "own_frame_bytes" .= bytes (frameOwn frame)
  , "calls" .=
      [ object ["site" .= addressText site, "target" .= addressText target, "depth_bytes" .= depth, "kind" .= kindText kind]
      | CallEdge site target depth kind <- frameCalls frame
      ]
  ]
  where
    bytes (FrameBytes count) = count
    kindText kind = case kind of
      Call -> "call" :: String
      TailCall -> "tail-call"

contractValue :: Staged -> Value
contractValue staged =
  object
    [ "schema" .= (1 :: Int)
    , "piece_kind" .= ("kernel-slot-bodies" :: String)
    , "qualification" .= ("standalone-host-staged" :: String)
    , "source" .= object
        [ "dat_sha256" .= digest (sourceDat source)
        , "loader_a_sha256" .= digest (sourceLoaders source M.! DomainA)
        , "loader_b_sha256" .= digest (sourceLoaders source M.! DomainB)
        ]
    , "regions_sha256" .= digest (admittedRegionsJson admitted)
    , "fill_word" .= hexText (tableFill (admittedTable admitted))
    , "allowed_dat_checksum" .= object ["offset" .= hexText (fromIntegral U.mainChecksumOffset), "bytes" .= (4 :: Int)]
    , "candidate_dat_sha256" .= digest (stagedDat staged)
    , "loaders" .= map loaderValue domains
    ]
  where
    source = stagedSource staged
    admitted = stagedAdmitted staged
    loaderValue domain =
      let entry = admittedDomains admitted M.! domain
          table = domainRows entry
          walked = domainFrames entry
          installedRows = domainExtentsInstalled entry
          slots = take (length (tableRegions table)) installedRows
          tailInstalled = installedRows !! length (tableRegions table)
          body = tableBody table
          extentPairs installed =
            let region = installedRegion installed
                Extent start end = regionExtent region
             in [ "slot" .= slotText (regionSlot region)
                , "entry" .= addressText (regionEntry region)
                , "start" .= addressText start
                , "end" .= addressText end
                , "symbol" .= symbolText (regionSymbol region)
                , "stock_sha256" .= digest (installedStock installed)
                , "owned_sha256" .= digest (installedOwned installed)
                , "filled_words" .= installedFilled installed
                ]
                  ++ frameValues (walked M.! regionEntry region)
          Extent bodyStart bodyEnd = bodyExtent body
          bodyPairs =
            [ "address" .= addressText bodyStart
            , "end" .= addressText bodyEnd
            , "symbol" .= symbolText (bodySymbol body)
            , "host_slot" .= slotText (bodyHost body)
            ]
              ++ frameValues (walked M.! bodyStart)
       in object
            [ "domain" .= domainLabel domain
            , "loader_base" .= addressText (domainLoaderBase domain)
            , "segment_id" .= domainLoaderId domain
            , "stock_loader_sha256" .= digest (sourceLoaders source M.! domain)
            , "staged_loader_sha256" .= digest (domainLoader entry)
            , "owned_bin_sha256" .= digest (linkedBin (domainLinked entry))
            , "owned_symbols_sha256" .= digest (linkedSymbols (domainLinked entry))
            , "patched_words" .= (stagedPatched staged M.! domain)
            , "slots" .= map (object . extentPairs) slots
            , "tail" .= object (extentPairs tailInstalled ++ ["body" .= object bodyPairs])
            , "entry_table" .=
                [ object
                    [ "slot" .= slotText slot
                    , "stock_entry" .= addressText stock
                    , "owned_symbol" .= symbolText symbol
                    , "owned_entry" .= addressText owned
                    ]
                | EntryRow slot stock symbol owned <- entryTable admitted domain
                ]
            ]
