module StartupStageContract
  ( firstStageDeclarations
  , loaderPrefixDeclarations
  , firstStagePlan
  , loaderPrefixPlan
  , composedPlans
  ) where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.FirstStage
import qualified FujiAF.UpdateContainer as U
import SecondaryStageContract
import StageBuild

loaderB :: LoaderId
loaderB = LoaderId 1

data PrefixSymbol
  = PrefixEntry
  | PrefixInstall
  | PrefixClearWorkspace
  | PrefixCompletionCell
  | PrefixDispatch
  | PrefixExit
  deriving (Bounded, Enum)

data ImageBound = ImageStart | ImageEnd

data LoaderSymbol
  = LoaderImage ImageBound
  | LoaderStockEntry
  | LoaderContinuation Continuation
  | LoaderForbidden Forbidden ImageBound

data Continuation = Core0Continuation | Core1Continuation
  deriving (Bounded, Enum)

data Access = FetchAccess | ReadAccess

data Replaced = ReplacedEntry | ReplacedEntryPool | ReplacedDecision | ReplacedWait
  deriving (Bounded, Enum)

data Forbidden = Forbidden Access Replaced

forbidden :: Replaced -> Forbidden
forbidden replaced = Forbidden (access replaced) replaced
  where
    access ReplacedEntryPool = ReadAccess
    access _ = FetchAccess

accessText :: Access -> String
accessText FetchAccess = "fetch"
accessText ReadAccess = "read"

replacedText :: Replaced -> String
replacedText ReplacedEntry = "entry"
replacedText ReplacedEntryPool = "entry_pool"
replacedText ReplacedDecision = "decision"
replacedText ReplacedWait = "wait"

boundText :: ImageBound -> String
boundText ImageStart = "start"
boundText ImageEnd = "end"

prefixSymbolName :: PrefixSymbol -> String
prefixSymbolName symbol = "xm1_loader_prefix_" ++ suffix symbol
  where
    suffix PrefixEntry = "entry"
    suffix PrefixInstall = "install"
    suffix PrefixClearWorkspace = "clear_workspace"
    suffix PrefixCompletionCell = "completion_cell"
    suffix PrefixDispatch = "dispatch"
    suffix PrefixExit = "exit"

loaderSymbolName :: LoaderSymbol -> String
loaderSymbolName symbol = "xm1_loader_b_" ++ suffix symbol
  where
    suffix (LoaderImage bound) = "image_" ++ boundText bound
    suffix LoaderStockEntry = "stock_entry"
    suffix (LoaderContinuation Core0Continuation) = "core0_continuation"
    suffix (LoaderContinuation Core1Continuation) = "core1_continuation"
    suffix (LoaderForbidden (Forbidden access replaced) bound) =
      "forbid_" ++ accessText access ++ "_" ++ replacedText replaced ++ "_" ++ boundText bound

inside :: (Word32, Word32) -> Word32 -> Bool
inside (start, end) value = within (fromIntegral start, fromIntegral end) (fromIntegral value)

loaderPrefixDeclarations :: [Symbol] -> Either String StageDeclarations
loaderPrefixDeclarations symbols = do
  secondary <- secondaryDeclarations symbols
  let address name = symbolAddress <$> lookupSymbol symbols name
      loaderAddress = address . loaderSymbolName
  image <- (,) <$> address "__secondary_stage_start" <*> address "__secondary_stage_end"
  forM_ [minBound .. maxBound] $ \symbol -> do
    location <- address (prefixSymbolName symbol)
    unless (inside image location) (Left (prefixSymbolName symbol ++ " is outside the owned record-2 image"))
  loader <- (,) <$> loaderAddress (LoaderImage ImageStart) <*> loaderAddress (LoaderImage ImageEnd)
  spans <- forM [minBound .. maxBound] $ \replaced -> do
    let kind = forbidden replaced
        bound = loaderSymbolName . LoaderForbidden kind
    range@(start, end) <- (,) <$> loaderAddress (LoaderForbidden kind ImageStart) <*> loaderAddress (LoaderForbidden kind ImageEnd)
    unless (fst loader <= start && start < end && end <= snd loader)
      (Left (bound ImageStart ++ " is empty or outside loader B"))
    pure (kind, range)
  let ordered = sortOn snd spans
      describe (Forbidden access replaced) = accessText access ++ " " ++ replacedText replaced
  forM_ (zip ordered (drop 1 ordered)) $ \((kind, (_, end)), (next, (start, _))) ->
    when (end > start) (Left ("loader-B forbidden " ++ describe kind ++ " overlaps " ++ describe next))
  retained <- forM [minBound .. maxBound] $ \continuation -> do
    let name = loaderSymbolName (LoaderContinuation continuation)
    location <- address name
    unless (inside loader location) (Left (name ++ " is outside loader B"))
    when (any (\(_, range) -> inside range location) spans) (Left (name ++ " is inside a forbidden extent"))
    pure (name, location)
  stockEntry <- loaderAddress LoaderStockEntry
  entrySpan <- loaderAddress (LoaderForbidden (forbidden ReplacedEntry) ImageStart)
  unless (entrySpan == stockEntry) (Left "the stock loader-B entry does not start the forbidden entry extent")
  stock <- armEntry stockEntry
  entry <- address (prefixSymbolName PrefixEntry) >>= armEntry
  publication <- loaderEntryPublication loaderB (fst loader) stock entry
  let summary =
        ["retained loader-B continuation " ++ name ++ " " ++ U.showHexWord location | (name, location) <- retained]
          ++ [ "candidate loader-B forbidden " ++ describe kind ++ " [" ++ U.showHexWord start ++ "," ++ U.showHexWord end ++ ")"
             | (kind, (start, end)) <- spans
             ]
  pure
    secondary
      { declaredLoaderPatches = [publication]
      , declaredSummary = declaredSummary secondary ++ summary
      }

firstStageDeclarations :: [Symbol] -> Either String StageDeclarations
firstStageDeclarations symbols = do
  let offset = symbolSlotOffset FirstRecord symbols
  vectorStart <- offset "xm1_vector_table_start"
  beforeReset <- offset "xm1_vector_before_reset_end"
  afterReset <- offset "xm1_vector_after_reset_start"
  vectorEnd <- offset "xm1_vector_table_end"
  bodyStart <- offset "xm1_retained_body_start"
  bodyEnd <- offset "xm1_retained_body_end"
  dataStart <- offset "xm1_retained_data_start"
  dataEnd <- offset "xm1_retained_data_end"
  beforeExtent <- retainedExtent vectorStart beforeReset
  afterExtent <- retainedExtent afterReset vectorEnd
  bodyExtent <- retainedExtent bodyStart bodyEnd
  dataExtent <- retainedExtent dataStart dataEnd
  let bodyRange = (slotOffsetBytes bodyStart, slotOffsetBytes bodyEnd)
  forM_
    [ "xm1_retained_core_init"
    , "xm1_retained_storage_init"
    , "xm1_retained_skip_scan"
    , "xm1_retained_segment_table"
    , "xm1_retained_trailer_check"
    , "xm1_retained_stage_images"
    , "xm1_retained_storage_fini"
    ]
    $ \name -> do
      symbol <- lookupSymbol symbols name
      let address = fromIntegral (symbolAddress symbol)
      if address < fst bodyRange || address >= snd bodyRange
        then Left (name ++ " is outside the retained body extent")
        else pure ()
  pure (StageDeclarations [beforeExtent, afterExtent, bodyExtent, dataExtent] [] [])

firstStagePlan :: B.ByteString -> [Symbol] -> Either String StagePlan
firstStagePlan image symbols = do
  declarations <- firstStageDeclarations symbols
  linkedPlan FirstStageImage declarations image symbols

loaderPrefixPlan :: B.ByteString -> [Symbol] -> Either String (StagePlan, LoaderWordPatch)
loaderPrefixPlan image symbols = do
  declarations <- loaderPrefixDeclarations symbols
  plan <- linkedPlan SecondaryPrefixImage declarations image symbols
  case declaredLoaderPatches declarations of
    [publication] -> pure (plan, publication)
    _ -> Left "combined secondary/prefix image lacks its unique loader-B publication"

composedPlans :: B.ByteString -> [Symbol] -> B.ByteString -> [Symbol] -> Either String ComposedStagePlan
composedPlans first firstSymbols second secondSymbols = do
  firstPlan <- firstStagePlan first firstSymbols
  (secondPlan, publication) <- loaderPrefixPlan second secondSymbols
  composedStagePlan firstPlan secondPlan publication

data LinkedImage = FirstStageImage | SecondaryPrefixImage

data LinkedSymbol = LinkedStart | LinkedEnd | LinkedEntry | LinkedMain | LinkedHandoff | LinkedHandoffVector | LinkedPrefixEntry

linkedSymbolName :: LinkedImage -> LinkedSymbol -> String
linkedSymbolName image symbol = case symbol of
  LinkedStart -> "__" ++ stage ++ "_start"
  LinkedEnd -> "__" ++ stage ++ "_end"
  LinkedPrefixEntry -> "xm1_loader_prefix_entry"
  LinkedEntry -> prefix ++ "entry"
  LinkedMain -> prefix ++ "main"
  LinkedHandoff -> prefix ++ "handoff"
  LinkedHandoffVector -> prefix ++ "handoff_vector"
  where
    (stage, prefix) = case image of
      FirstStageImage -> ("first_stage", "xm1_first_stage_")
      SecondaryPrefixImage -> ("secondary_stage", "xm1_secondary_")

linkedPlan :: LinkedImage -> StageDeclarations -> B.ByteString -> [Symbol] -> Either String StagePlan
linkedPlan kind declarations image symbols = do
  let name = linkedSymbolName kind
      address symbol = symbolAddress <$> lookupSymbol symbols (name symbol)
      (record, expectedStart, capacityEnd, sites) = case kind of
        FirstStageImage -> (FirstRecord, 0x6000, 0x10000, [])
        SecondaryPrefixImage -> (SecondRecord, 0x10400, 0x11800, [LinkedPrefixEntry])
  start <- address LinkedStart
  end <- address LinkedEnd
  entrySymbol <- lookupSymbol symbols (name LinkedEntry)
  let imageEnd = toInteger start + toInteger (B.length image)
  unless (start == expectedStart && imageEnd == toInteger end && end <= capacityEnd)
    (Left "linked startup image extent differs from its declared padding or flat image")
  unless (symbolSize entrySymbol > 0) (Left "linked startup entry has zero size")
  forM_ ([LinkedEntry, LinkedMain, LinkedHandoff, LinkedHandoffVector] ++ sites) $ \symbol -> do
    site <- address symbol
    unless (site >= start && site < end && site `mod` 4 == 0)
      (Left (name symbol ++ " is not an aligned site in the linked startup image"))
  entry <- armEntry (symbolAddress entrySymbol)
  load <- slotOffset (fromIntegral start - recordGuestBase record)
  stagePlan record entry load (U.MediaImage image) (declaredRetained declarations)
