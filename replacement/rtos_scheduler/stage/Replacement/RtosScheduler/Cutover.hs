{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosScheduler.Cutover
  ( GroupInput (..), CutoverVariant (..), CutoverError (..), CutoverAdmission, CutoverStaged
  , admitCutover, stageCutover, cutoverDat, cutoverLoader, cutoverContract, confineCutover
  , restoreCutover, disjointExtents, validateInputs, ownFirstGuards
  , admittedExtents, rebuild, loadersFrom
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, object, (.=), (.:), withObject)
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser, parseEither)
import Data.Char (toLower)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (find, sortOn, stripPrefix)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex)
import qualified Replacement.RtosOwnFirst.Artifact as O
import Replacement.RtosOwnFirst.Pins (stockPins)
import Replacement.RtosOwnFirst.Regions
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.CutoverPins
import Replacement.RtosScheduler.Semantic

data GroupInput = GroupInput {inputGroup :: Group, inputMetadata :: B.ByteString, inputA :: S.Linked, inputB :: S.Linked}
data CutoverVariant = CombinedCandidate | RestoreS1 | RestoreS2 | RestoreS3 | CombinedSilentNotify | CombinedWrongPredicate | WakeNoInsert deriving (Eq, Show)
data CutoverError
  = GroupSetMismatch | MetadataUnpinned Group | BinaryUnpinned Group Domain | SymbolsUnpinned Group Domain
  | MetadataRejected Group String | SymbolsRejected Group Domain | WindowRejected Group Domain
  | EntryRejected Group Domain GuestAddress | MarkerRejected Group Domain SymbolName
  | SymbolRejected Group Domain SymbolName | GapRejected Group Domain GuestAddress
  | SlackRejected Group Domain GuestAddress | ExtentsOverlap Domain Extent Extent
  | LeafRejected Group String | SourceRejected S.StageError | ContainerRejected String
  | ChangedOutside Int | ControlRejected GuestAddress | RestoreMismatch
  deriving (Eq, Show)
data EntryTag = HelperTag S.Helper | ServiceTag Semantic | SlotTag SlotOffset deriving (Eq, Show)
data Entry = Entry GuestAddress EntryTag deriving (Eq, Show)
data FrameBasis = WalkedComposed | PinnedLocal deriving (Eq, Show)
data Window = Window {windowGroup :: Group, windowDomain :: Domain, windowExtent :: Extent, windowSymbol :: SymbolName, windowEntries :: [Entry], windowFrame :: FrameBytes, windowBasis :: FrameBasis} deriving (Eq, Show)
data CutoverAdmission = CutoverAdmission {admittedSource :: S.Source, admittedWindows :: [Window], admittedLoaders :: M.Map Domain B.ByteString, admittedInputs :: [GroupInput]}
data CutoverStaged = CutoverStaged {stagedAdmission :: CutoverAdmission, stagedVariant :: CutoverVariant, cutoverDat :: B.ByteString, stagedLoaders :: M.Map Domain B.ByteString}

require :: Bool -> CutoverError -> Either CutoverError ()
require ok err = unless ok (Left err)
digest :: B.ByteString -> Digest
digest = Digest . SHA.sha256Hex . SHA.sha256
digestText :: B.ByteString -> String
digestText bytes = let Digest text = digest bytes in text
linkedFor :: GroupInput -> Domain -> S.Linked
linkedFor input domain = case domain of DomainA -> inputA input; DomainB -> inputB input

validateInputs :: [GroupInput] -> Either CutoverError ()
validateInputs inputs = do
  require (sortOn id (map inputGroup inputs) == [minBound .. maxBound]) GroupSetMismatch
  forM_ inputs $ \input -> do
    let group = inputGroup input
    require (digest (inputMetadata input) == metadataPin group) (MetadataUnpinned group)
    forM_ domains $ \domain -> do
      let linked = linkedFor input domain
      require (digest (S.linkedBin linked) == binPin group domain) (BinaryUnpinned group domain)
      forM_ (symbolPin group domain) $ \pin -> require (digest (S.linkedSymbols linked) == pin) (SymbolsUnpinned group domain)

disjointExtents :: Domain -> [Extent] -> Either CutoverError ()
disjointExtents domain extents = forM_ (zip ordered (drop 1 ordered)) $ \(left,right) ->
  require (extentEnd left <= extentStart right) (ExtentsOverlap domain left right)
  where ordered = sortOn extentStart extents

parseAddress :: Value -> Parser GuestAddress
parseAddress = J.withText "address" $ \text -> case T.unpack text of
  '0':'x':digits -> case readHex digits of
    [(n,"")] | not (null digits), n <= toInteger (maxBound :: Word32) -> pure (GuestAddress (fromInteger n))
    _ -> fail "invalid address"
  _ -> fail "invalid address"
parseWindows :: GroupInput -> Either CutoverError [Window]
parseWindows input = do
  value <- either (Left . MetadataRejected group) Right (J.eitherDecodeStrict (inputMetadata input))
  either (Left . MetadataRejected group) Right (parseEither parser value)
  where
    group = inputGroup input
    parser = withObject "regions" $ \o -> do
      ds <- o .: "domains"
      fmap concat $ forM domains $ \domain -> do
        d <- ds .: (if domain == DomainA then "a" else "b")
        rs <- d .: "regions"
        forM rs $ withObject "window" $ \r -> do
          low <- r .: "start" >>= parseAddress
          high <- r .: "end" >>= parseAddress
          symbol <- SymbolName <$> r .: "symbol"
          frame <- r .: "frame_bytes"
          unless (frame >= 0) (fail "negative frame")
          entries <- r .: "entries" >>= mapM entry
          pure (Window group domain (Extent low high) symbol entries (FrameBytes frame) PinnedLocal)
    entry = withObject "entry" $ \o -> do
      at <- o .: "entry" >>= parseAddress
      semantic <- o .: "semantic"
      kind <- semantic .: "kind"
      name <- semantic .: "name"
      tag <- either fail pure (parseSemantic kind name)
      pure (Entry at (ServiceTag tag))

slice :: GuestAddress -> Extent -> B.ByteString -> B.ByteString
slice (GuestAddress base) e@(Extent (GuestAddress start) _) = B.take (extentBytes e) . B.drop (fromIntegral (start-base))
replace :: GuestAddress -> Extent -> B.ByteString -> B.ByteString -> B.ByteString
replace (GuestAddress base) e@(Extent (GuestAddress start) _) replacement original =
  let offset = fromIntegral (start-base) in B.take offset original <> replacement <> B.drop (offset+extentBytes e) original

bodyMarker :: Window -> SymbolName
bodyMarker window = SymbolName $ case (windowGroup window, windowEntries window, windowSymbol window) of
  (S3a,[Entry _ (ServiceTag ServiceExitTask)],_) -> "__s3a_exit_body_end"
  (S3a,[Entry _ (ServiceTag ServiceRotateReady)],_) -> "__s3a_rotate_body_end"
  (S3a,[Entry _ (ServiceTag ServiceInterruptRotateReady)],_) -> "__s3a_irotate_body_end"
  (S3a,[Entry _ (ServiceTag ServiceCoreRotateReady)],_) -> "__s3a_crotate_body_end"
  (S3a,[Entry _ (ServiceTag ServiceInterruptCoreRotateReady)],_) -> "__s3a_icrotate_body_end"
  (S3a,[Entry _ (ServiceTag ServiceDisableDispatch)],_) -> "__s3a_disable_body_end"
  (S3a,[Entry _ (ServiceTag ServiceEnableDispatch)],_) -> "__s3a_enable_body_end"
  (S2,_,SymbolName "xm1_rtos_scheduler_no_save") -> "__s2_select_body_end"
  (S2,_,SymbolName name) -> "__s2_" ++ maybe name id (stripPrefix "xm1_rtos_scheduler_" name) ++ "_body_end"
  (_,_,SymbolName name) -> "__" ++ maybe name id (stripPrefix "xm1_" name) ++ "_body_end"

admitGroup :: GroupInput -> Domain -> [Window] -> Either CutoverError [(Extent,B.ByteString)]
admitGroup input domain windows = do
  let group = inputGroup input
      linked = linkedFor input domain
      bin = S.linkedBin linked
      base@(GuestAddress low) = minimum (map (extentStart . windowExtent) windows)
      GuestAddress high = maximum (map (extentEnd . windowExtent) windows)
      extents = map windowExtent windows
      ordered = sortOn extentStart extents
      address n symbols = case [GuestAddress (F.symbolAddress s) | s <- symbols, F.symbolName s == n] of
        [at] -> Right at
        _ -> Left (MarkerRejected group domain (SymbolName n))
  symbols <- either (const (Left (SymbolsRejected group domain))) Right (F.parseSymbols (S.linkedSymbols linked))
  let absolute = [C.unpack n | row <- C.lines (S.linkedSymbols linked), [_,_,kind,n] <- [C.words row], kind == "A" || kind == "a"]
  require (B.length bin == fromIntegral (high-low)) (WindowRejected group domain)
  forM_ windows $ \window -> do
    let extent@(Extent start@(GuestAddress lo) end@(GuestAddress hi)) = windowExtent window
        GuestAddress loaderBase = domainLoaderBase domain
    require (lo < hi && lo >= loaderBase && hi <= loaderBase + fromIntegral (domainLoaderSize domain) && lo `mod` 4 == 0 && hi `mod` 4 == 0) (WindowRejected group domain)
    let SymbolName name = windowSymbol window
    at <- address name symbols
    require (at == start) (EntryRejected group domain start)
    forM_ (windowEntries window) $ \(Entry entry _) -> do
      require (extentContains extent entry) (EntryRejected group domain entry)
      require (any ((== entry) . GuestAddress . F.symbolAddress) symbols) (EntryRejected group domain entry)
    let marker@(SymbolName markerName) = bodyMarker window
    bodyEnd <- address markerName symbols
    require (start < bodyEnd && bodyEnd <= end) (MarkerRejected group domain marker)
    forM_ [lo,lo+4 .. hi-4] $ \wordAt -> do
      let covered s = F.symbolSize s > 0 && F.symbolAddress s <= wordAt && toInteger wordAt+4 <= toInteger (F.symbolAddress s)+toInteger (F.symbolSize s)
          literalPool = group == S2 && windowSymbol window == SymbolName "xm1_rtos_scheduler_pool" && wordAt < let GuestAddress stop = bodyEnd in stop
      require (literalPool || any covered symbols || U.word32At bin (fromIntegral (wordAt-low)) == 0xe7f000f0) (SlackRejected group domain (GuestAddress wordAt))
  forM_ symbols $ \symbol -> unless (F.symbolName symbol `elem` absolute) $ do
    let start = GuestAddress (F.symbolAddress symbol)
        highSymbol = toInteger (F.symbolAddress symbol)+toInteger (F.symbolSize symbol)
        inside (Extent lo (GuestAddress hi)) = start >= lo && highSymbol <= toInteger hi && (F.symbolSize symbol == 0 || start < GuestAddress hi)
    require (any inside extents) (SymbolRejected group domain (SymbolName (F.symbolName symbol)))
  forM_ (zip (map extentEnd ordered) (map extentStart (drop 1 ordered))) $ \(GuestAddress start,GuestAddress end) ->
    forM_ (B.findIndex (/= 0) (B.take (fromIntegral (end-start)) (B.drop (fromIntegral (start-low)) bin))) $ \offset -> Left (GapRejected group domain (GuestAddress (start+fromIntegral offset)))
  pure [(extent,slice base extent bin) | extent <- extents]

admitCutover :: S.Source -> [GroupInput] -> Either CutoverError CutoverAdmission
admitCutover source inputs = do
  validateInputs inputs
  let byGroup group = case filter ((== group) . inputGroup) inputs of [input] -> input; _ -> error "validated group set"
      first = byGroup S1
      own = byGroup OwnFirst
      leaf group = either (Left . LeafRejected group . show) Right
  s1Table <- leaf S1 (S.parseTable (inputMetadata first))
  s1Admission <- leaf S1 (S.admit source s1Table S.Candidate (inputA first) (inputB first))
  ownSource <- leaf OwnFirst (O.canonicalSource (S.sourceDat source) (S.sourceLoaders source M.! DomainA) (S.sourceLoaders source M.! DomainB))
  ownAdmission <- leaf OwnFirst (O.admit stockPins ownSource (inputMetadata own) (toOwn (inputA own)) (toOwn (inputB own)))
  let s1Windows = [Window S1 d (S.window r) (S.symbol r) [Entry (extentStart (S.window r)) (HelperTag (S.helper r))] (S.demand r) WalkedComposed | d <- domains, r <- S.pinnedRows d]
      ownWindows = [Window OwnFirst d (regionExtent r) (regionSymbol r) [Entry (regionEntry r) (SlotTag (regionSlot r))] (regionFrame r) WalkedComposed | d <- domains, r <- domainExtents (domainTable (O.admittedTable ownAdmission) d)]
  others <- fmap concat $ forM [S2,S3a,S3b,S3c] (parseWindows . byGroup)
  let windows = s1Windows ++ ownWindows ++ others
  loaders <- forM domains $ \domain -> do
    let currentWindows = filter ((== domain) . windowDomain) windows
    disjointExtents domain (map windowExtent currentWindows)
    replacements <- fmap concat $ forM [S2,S3a,S3b,S3c] $ \group -> admitGroup (byGroup group) domain (filter (\w -> windowDomain w == domain && windowGroup w == group) others)
    let leafReplacements = [(windowExtent w, slice (domainLoaderBase domain) (windowExtent w) (if windowGroup w == S1 then S.admittedLoader s1Admission domain else O.admittedLoader ownAdmission domain)) | w <- currentWindows, windowGroup w `elem` [S1,OwnFirst]]
        loader = foldl (\bytes (extent,owned) -> replace (domainLoaderBase domain) extent owned bytes) (S.sourceLoaders source M.! domain) (leafReplacements++replacements)
    pure (domain,loader)
  pure (CutoverAdmission source windows (M.fromList loaders) inputs)
  where toOwn linked = O.LinkedDomain (S.linkedBin linked) (S.linkedSymbols linked)

-- | Every admitted window extent, by domain.
admittedExtents :: CutoverAdmission -> [(Domain, Extent)]
admittedExtents admitted = [(windowDomain window, windowExtent window) | window <- admittedWindows admitted]

controlLoaders :: CutoverVariant -> CutoverAdmission -> Either CutoverError (M.Map Domain B.ByteString)
controlLoaders variant admitted = case variant of
  CombinedCandidate -> Right original
  RestoreS1 -> restoreWindow S1 (GuestAddress 0x43ca4)
  RestoreS2 -> restoreWindow S2 (GuestAddress 0x43640)
  RestoreS3 -> restoreWindow S3b (GuestAddress 0x46004)
  CombinedSilentNotify -> patch DomainA [(0x4402c,0xe3a01078,0xe24dd004),(0x44034,0xebfffcea,0xe28dd004)]
  CombinedWrongPredicate -> patch DomainB [(0xd3fd4,0xe3e01002,0xe3500001),(0xd3fd8,0xe1100001,0x93a00001),(0xd3fdc,0x03a00001,0x912fff1e),(0xd3fe0,0x012fff1e,0xe320f000)]
  WakeNoInsert -> patch DomainA [(0x46134,0xebfff90b,0xe3a00000)]
  where
    original = admittedLoaders admitted
    restoreWindow group address = case [w | w <- admittedWindows admitted, windowGroup w == group, windowDomain w == DomainA, extentStart (windowExtent w) == address] of
      [window] -> let extent = windowExtent window; base = domainLoaderBase DomainA; stock = S.sourceLoaders (admittedSource admitted) M.! DomainA in Right (M.adjust (replace base extent (slice base extent stock)) DomainA original)
      _ -> Left (ControlRejected address)
    patch domain wordsToPatch = do
      let GuestAddress base = domainLoaderBase domain
      changed <- foldPatch (original M.! domain) wordsToPatch base
      pure (M.insert domain changed original)
    foldPatch bytes [] _ = Right bytes
    foldPatch bytes ((at,before,after):rest) base = do
      let offset = fromIntegral (at-base)
      require (U.word32At bytes offset == before) (ControlRejected (GuestAddress at))
      foldPatch (B.take offset bytes <> U.putWord32 after <> B.drop (offset+4) bytes) rest base

cutoverLoader :: CutoverStaged -> Domain -> B.ByteString
cutoverLoader staged domain = stagedLoaders staged M.! domain
patches :: CutoverAdmission -> M.Map Domain B.ByteString -> M.Map Domain B.ByteString -> Either CutoverError [F.LoaderWordPatch]
patches admitted before after = either (Left . ContainerRejected) Right $ sequence
  [ F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) at old new
  | window <- admittedWindows admitted, let domain = windowDomain window
  , let Extent (GuestAddress lo) (GuestAddress hi) = windowExtent window
  , at <- [lo,lo+4 .. hi-4], let GuestAddress base = domainLoaderBase domain
  , let offset = fromIntegral (at-base), let old = U.word32At (before M.! domain) offset, let new = U.word32At (after M.! domain) offset, old /= new ]
rebuild :: U.Parsed -> [F.LoaderWordPatch] -> Either CutoverError B.ByteString
rebuild parsed wordsToPatch = do
  let step = either (Left . ContainerRejected) Right
  parts <- step (F.applyLoaderWordPatches wordsToPatch (U.parsedParts parsed))
  dat <- step (U.rebuildContainer parsed parts)
  consumed <- step (U.parseContainer dat)
  step (F.verifyPreTablePatches [] wordsToPatch parsed consumed)
  pure dat
loadersFrom :: B.ByteString -> Either CutoverError (U.Parsed,M.Map Domain B.ByteString)
loadersFrom dat = do
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  let parts = U.parsedParts parsed
  loaders <- forM domains $ \domain -> case [s | s <- U.partSegments parts, U.segId s == domainLoaderId domain] of
    [segment] -> pure (domain,B.map (255-) (B.take (domainLoaderSize domain) (B.drop (fromIntegral (U.segFlash segment)) (U.partPreTable parts))))
    _ -> Left (ContainerRejected "loader segment set")
  pure (parsed,M.fromList loaders)
confineCutover :: CutoverAdmission -> B.ByteString -> Either CutoverError ()
confineCutover admitted dat = do
  let source = admittedSource admitted
      original = S.sourceDat source
      parsed = S.sourceParsed source
      parts = U.parsedParts parsed
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  ranges <- forM (admittedWindows admitted) $ \window -> do
    let domain = windowDomain window
    case [s | s <- U.partSegments parts, U.segId s == domainLoaderId domain] of
      [segment] -> let extent@(Extent (GuestAddress lo) _) = windowExtent window in pure (mainStart + fromIntegral (U.segFlash segment) + fromIntegral (lo-U.segLoad segment),extentBytes extent)
      _ -> Left (ContainerRejected "loader segment set")
  require (B.length dat == B.length original) (ChangedOutside (min (B.length dat) (B.length original)))
  let allowed = (U.mainChecksumOffset,4):ranges
  forM_ (find (\i -> not (any (\(low,len) -> i >= low && i < low+len) allowed)) [i | (i,True) <- zip [0..] (B.zipWith (/=) original dat)]) (Left . ChangedOutside)
restoreCutover :: CutoverAdmission -> B.ByteString -> Either CutoverError B.ByteString
restoreCutover admitted dat = do
  (parsed,current) <- loadersFrom dat
  wordsToPatch <- patches admitted current (S.sourceLoaders (admittedSource admitted))
  rebuild parsed wordsToPatch
stageCutover :: CutoverAdmission -> CutoverVariant -> Either CutoverError CutoverStaged
stageCutover admitted variant = do
  owned <- controlLoaders variant admitted
  let source = admittedSource admitted
  wordsToPatch <- patches admitted (S.sourceLoaders source) owned
  dat <- rebuild (S.sourceParsed source) wordsToPatch
  confineCutover admitted dat
  (_,actual) <- loadersFrom dat
  require (actual == owned) (ContainerRejected "staged loaders differ")
  restored <- restoreCutover admitted dat
  require (restored == S.sourceDat source) RestoreMismatch
  pure (CutoverStaged admitted variant dat owned)

ownFirstGuards :: CutoverAdmission -> Value
ownFirstGuards admitted = object
  [ "fill_word" .= hexText 0xe7f000f0
  , "domains" .= object ["a" .= domainValue DomainA, "b" .= domainValue DomainB]
  ]
  where
    domainValue domain = object
      [ "regions" .=
          [ object ["start" .= hexText start, "end" .= hexText end, "entries" .= ([] :: [Value])]
          | window <- admittedWindows admitted
          , windowGroup window == OwnFirst, windowDomain window == domain
          , let Extent (GuestAddress start) (GuestAddress end) = windowExtent window
          ]
      ]

cutoverContract :: CutoverStaged -> Value
cutoverContract staged = object
  [ "schema" .= (2 :: Int), "piece_kind" .= ("kernel-scheduler" :: String)
  , "scope" .= ("stock plus S1, S2, S3a, S3b, S3c and own-first slots" :: String)
  , "own_first_included" .= True, "all_extents_disjoint" .= True
  , "variant" .= show (stagedVariant staged), "source_dat_sha256" .= digestText (S.sourceDat source)
  , "candidate_dat_sha256" .= digestText (cutoverDat staged)
  , "frame_scope" .= ("S1 and own-first are composed walks over leaf images; S2/S3 frame demands are pinned local declarations, not composed demand. Controls retain candidate frame declarations." :: String)
  , "inputs" .= [object ["group" .= groupLabel (inputGroup input), "metadata_sha256" .= digestText (inputMetadata input), "a_bin_sha256" .= digestText (S.linkedBin (inputA input)), "b_bin_sha256" .= digestText (S.linkedBin (inputB input)), "a_symbols_sha256" .= digestText (S.linkedSymbols (inputA input)), "b_symbols_sha256" .= digestText (S.linkedSymbols (inputB input))] | input <- admittedInputs admitted]
  , "loaders" .= [object ["domain" .= domainLabel domain, "stock_sha256" .= digestText (S.sourceLoaders source M.! domain), "owned_sha256" .= digestText (cutoverLoader staged domain), "extents" .= map windowValue (filter ((== domain) . windowDomain) (admittedWindows admitted)), "entry_table" .= [entryValue window entry | window <- admittedWindows admitted, windowDomain window == domain, entry <- windowEntries window]] | domain <- domains] ]
  where
    admitted = stagedAdmission staged
    source = admittedSource admitted
    address (GuestAddress at) = hexText at
    text (SymbolName name) = name
    entryValue window (Entry at tag) = object ["entry" .= address at, "group" .= groupLabel (windowGroup window), "entry_symbol" .= text (entrySymbol window tag), "semantic" .= tagValue tag]
    entrySymbol window tag = case tag of
      ServiceTag DispatchSelect -> SymbolName "xm1_rtos_scheduler_select"
      ServiceTag DispatchIrqTail -> SymbolName "xm1_rtos_scheduler_irq_tail"
      _ -> windowSymbol window
    tagValue tag = case tag of
      ServiceTag semantic -> let (kind,name) = semanticText semantic in object ["kind" .= kind,"name" .= name]
      HelperTag helper -> object ["kind" .= ("helper" :: String),"name" .= map toLower (show helper)]
      SlotTag (SlotOffset slot) -> object ["kind" .= ("slot" :: String),"slot" .= hexText slot]
    windowValue window = let extent@(Extent start end) = windowExtent window; domain = windowDomain window; FrameBytes frame = windowFrame window in object
      ["group" .= groupLabel (windowGroup window), "start" .= address start,"end" .= address end,"entry_symbol" .= text (windowSymbol window),"frame_bytes" .= frame,"frame_basis" .= show (windowBasis window),"stock_sha256" .= digestText (slice (domainLoaderBase domain) extent (S.sourceLoaders source M.! domain)),"candidate_sha256" .= digestText (slice (domainLoaderBase domain) extent (admittedLoaders admitted M.! domain)),"owned_sha256" .= digestText (slice (domainLoaderBase domain) extent (cutoverLoader staged domain))]
