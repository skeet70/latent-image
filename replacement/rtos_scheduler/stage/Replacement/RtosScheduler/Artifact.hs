{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosScheduler.Artifact
  ( Helper (..), Variant (..), Row (..), Table (..), Linked (..), Source, Admitted, Staged
  , StageError (..), pinnedRows, canonicalSource, requirePinned, parseTable, admit, stage
  , stagedDat, stagedLoader, contractValue, confineChanges, restore, controlLinked
  , sourceDat, sourceParsed, sourceLoaders, admittedLoader
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, object, (.=), (.:), withObject)
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser, parseEither)
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (find, sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), Extent (..), FrameBytes (..), SymbolName (..), domains, domainLabel, domainLoaderBase, domainLoaderId, domainLoaderSize, extentBytes, extentContains, hexText)
import Replacement.RtosOwnFirst.Frame

data Helper = Acquire | Release | Find | Insert | Remove | Reset | Prepare | Notify | Requeue | Rotate deriving (Eq, Ord, Show, Enum, Bounded)
data Variant = Candidate | StockInsert | SilentNotify | WrongPredicate deriving (Eq, Show)
data Row = Row {helper :: Helper, window :: Extent, symbol :: SymbolName, demand :: FrameBytes} deriving (Eq, Show)
data Table = Table {rowsA :: [Row], rowsB :: [Row]} deriving (Eq, Show)
data Linked = Linked {linkedBin :: B.ByteString, linkedSymbols :: B.ByteString} deriving (Eq, Show)
data Source = Source {sourceDat :: B.ByteString, sourceParsed :: U.Parsed, sourceLoaders :: M.Map Domain B.ByteString}
data Installed = Installed {installedRows :: [Row], installedLoader :: B.ByteString, installedLinked :: Linked, installedFrames :: M.Map GuestAddress Frame}
data Admitted = Admitted {admittedSource :: Source, admittedDomains :: M.Map Domain Installed, admittedVariant :: Variant}
data Staged = Staged {stagedAdmission :: Admitted, stagedDat :: B.ByteString}
data StageError = ContainerRejected String | LoaderMismatch Domain | SourceUnpinned | TableRejected String | PinMismatch Domain | SymbolsRejected Domain | EntryMismatch Domain Helper | MarkerMismatch Domain String | SymbolOutside Domain String | ByteOutside Domain GuestAddress | SlackNotFill Domain GuestAddress | FrameRejected Domain FrameError | FrameMismatch Domain Helper FrameBytes FrameBytes | PatchRejected String | ChangeOutside Int | RestoreMismatch | ControlRejected String deriving (Eq, Show)

require :: Bool -> StageError -> Either StageError ()
require ok err = unless ok (Left err)
sha :: B.ByteString -> String
sha = SHA.sha256Hex . SHA.sha256
symbolText :: SymbolName -> String
symbolText (SymbolName text) = text
name :: Helper -> String
name h = case h of
  Acquire -> "acquire"
  Release -> "release"
  Find -> "find"
  Insert -> "insert"
  Remove -> "remove"
  Reset -> "reset"
  Prepare -> "prepare"
  Notify -> "notify"
  Requeue -> "requeue"
  Rotate -> "rotate"

pinnedRows :: Domain -> [Row]
pinnedRows domain = zipWith make [minBound .. maxBound] values
  where
    values = case domain of
      DomainA -> [(0x439d8,0x43a14,8),(0x43a14,0x43a3c,8),(0x43c48,0x43ca4,8),(0x43ca4,0x43d20,16),(0x43d20,0x43dd0,24),(0x43dd0,0x43e18,8),(0x43f70,0x43f9c,16),(0x44018,0x44044,12),(0x44044,0x44134,32),(0x44134,0x441b4,8)]
      DomainB -> [(0xd398c,0xd39c8,8),(0xd39c8,0xd39f0,8),(0xd3c04,0xd3c60,8),(0xd3c60,0xd3cdc,16),(0xd3cdc,0xd3d8c,24),(0xd3d8c,0xd3dd4,8),(0xd3f2c,0xd3f58,16),(0xd3fd4,0xd4004,12),(0xd4004,0xd40f4,32),(0xd40f4,0xd4174,8)]
    make h (lo,hi,frame) = Row h (Extent (GuestAddress lo) (GuestAddress hi)) (SymbolName ("xm1_rtos_scheduler_" ++ name h)) (FrameBytes frame)
shim :: Domain -> Extent
shim domain = case domain of
  DomainA -> Extent (GuestAddress 0x43c74) (GuestAddress 0x43ca4)
  DomainB -> Extent (GuestAddress 0xd3c30) (GuestAddress 0xd3c60)
raise :: Domain -> Extent
raise domain = let base = case domain of DomainA -> 0x433e4; DomainB -> 0xd33e4 in Extent (GuestAddress base) (GuestAddress (base + 0x60))

hex :: Value -> Parser Word32
hex = J.withText "hex address" $ \text -> case T.unpack text of
  '0':'x':digits -> case readHex digits of
    [(n,"")] | not (null digits), n <= toInteger (maxBound :: Word32) -> pure (fromInteger n)
    _ -> fail "invalid hex address"
  _ -> fail "invalid hex address"
parseTable :: B.ByteString -> Either StageError Table
parseTable bytes = do
  value <- either (Left . TableRejected) Right (J.eitherDecodeStrict bytes)
  either (Left . TableRejected) Right (parseEither parser value)
  where
    parser = withObject "scheduler regions" $ \o -> do
      fill <- o .: "fill_word" >>= hex
      unless (fill == 0xe7f000f0) (fail "fill word differs")
      ds <- o .: "domains"
      a <- ds .: "a" >>= domain DomainA
      b <- ds .: "b" >>= domain DomainB
      pure (Table a b)
    domain d = withObject "domain" $ \o -> do
      base <- o .: "loader_base" >>= hex
      let GuestAddress expectedBase = domainLoaderBase d
      unless (base == expectedBase) (fail "loader base differs")
      irq <- o .: "stock_raise_irq" >>= hex
      unless (GuestAddress irq == extentStart (raise d)) (fail "raise IRQ address differs")
      rs <- o .: "regions" >>= mapM row
      shims <- o .: "shims"
      case shims of
        [v] -> withObject "shim" (\s -> do
          h <- s .: "helper"
          unless (h == ("prepare" :: String)) (fail "shim helper differs")
          branch <- s .: "branch"
          branchAddress <- branch .: "address" >>= hex
          branchSymbol <- SymbolName <$> branch .: "symbol"
          let prepareRow = pinnedRows d !! 6
          unless (GuestAddress branchAddress == extentStart (window prepareRow) && branchSymbol == symbol prepareRow) (fail "shim branch differs")
          body <- s .: "body"
          lo <- body .: "address" >>= hex
          hi <- body .: "end" >>= hex
          sym <- body .: "symbol"
          host <- body .: "host_helper"
          frame <- body .: "frame_bytes"
          unless (Extent (GuestAddress lo) (GuestAddress hi) == shim d && sym == ("xm1_rtos_scheduler_prepare_body" :: String) && host == ("find" :: String) && frame == (16 :: Int)) (fail "shim window differs")) v
        _ -> fail "shim set differs"
      pure rs
    row = withObject "helper" $ \o -> do
      h <- o .: "helper"
      kind <- case [k | k <- [minBound .. maxBound], name k == h] of
        [k] -> pure k
        _ -> fail "unknown helper"
      lo <- o .: "start" >>= hex
      hi <- o .: "end" >>= hex
      entry <- o .: "entry" >>= hex
      unless (entry == lo) (fail "entry differs from start")
      sym <- SymbolName <$> o .: "symbol"
      frame <- o .: "frame_bytes"
      unless (frame >= 0) (fail "negative frame")
      pure (Row kind (Extent (GuestAddress lo) (GuestAddress hi)) sym (FrameBytes frame))

contained :: U.Parsed -> Domain -> Either StageError B.ByteString
contained parsed d = do
  segment <- segmentOf parsed d
  pure (B.map complement (B.take (domainLoaderSize d) (B.drop (fromIntegral (U.segFlash segment)) (U.partPreTable (U.parsedParts parsed)))))
segmentOf :: U.Parsed -> Domain -> Either StageError U.SegmentEntry
segmentOf parsed d = case filter ((== domainLoaderId d) . U.segId) (U.partSegments (U.parsedParts parsed)) of
  [s] | U.segLoad s == base && fromIntegral (U.segSize s) == domainLoaderSize d && fromIntegral (U.segFlash s) + domainLoaderSize d <= B.length (U.partPreTable (U.parsedParts parsed)) -> Right s
  _ -> Left (LoaderMismatch d)
  where GuestAddress base = domainLoaderBase d
canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> Either StageError Source
canonicalSource dat a b = do
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  require (U.rebuildContainer parsed (U.parsedParts parsed) == Right dat) (ContainerRejected "noncanonical round trip")
  forM_ (zip domains [a,b]) $ \(d,bytes) -> do
    original <- contained parsed d
    require (original == bytes && B.length bytes == domainLoaderSize d) (LoaderMismatch d)
  pure (Source dat parsed (M.fromList (zip domains [a,b])))
requirePinned :: Source -> Either StageError ()
requirePinned s = require (map sha [sourceDat s, sourceLoaders s M.! DomainA, sourceLoaders s M.! DomainB] == ["43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490","21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882","8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"]) SourceUnpinned
slice :: GuestAddress -> Extent -> B.ByteString -> B.ByteString
slice (GuestAddress base) e@(Extent (GuestAddress lo) _) = B.take (extentBytes e) . B.drop (fromIntegral (lo-base))
put :: GuestAddress -> Extent -> B.ByteString -> B.ByteString -> B.ByteString
put (GuestAddress base) e@(Extent (GuestAddress lo) _) replacement bytes = let offset = fromIntegral (lo-base) in B.take offset bytes <> replacement <> B.drop (offset + extentBytes e) bytes

admit :: Source -> Table -> Variant -> Linked -> Linked -> Either StageError Admitted
admit source table variant a b = do
  installed <- forM (zip domains [a,b]) $ \(d,linked) -> do
    let rs = case d of DomainA -> rowsA table; DomainB -> rowsB table
        same x y = helper x == helper y && window x == window y && symbol x == symbol y
    require (length rs == 10 && and (zipWith same rs (pinnedRows d))) (PinMismatch d)
    syms <- either (const (Left (SymbolsRejected d))) Right (F.parseSymbols (linkedSymbols linked))
    let rawRows = filter (not . C.null) (C.lines (linkedSymbols linked))
        absolute = [C.unpack n | row <- rawRows, [_,_,"A",n] <- [C.words row]]
        address n = case [F.symbolAddress s | s <- syms, F.symbolName s == n] of
          [at] -> Right (GuestAddress at)
          _ -> Left (MarkerMismatch d n)
        es = map window rs
        base@(GuestAddress low) = minimum (map extentStart es)
        GuestAddress high = maximum (map extentEnd es)
        bin = linkedBin linked
    require (length rawRows == length syms) (SymbolsRejected d)
    forM_ rs $ \r -> do
      at <- address (symbolText (symbol r))
      require (at == extentStart (window r)) (EntryMismatch d (helper r))
      end <- address ("__" ++ name (helper r) ++ "_end")
      require (end == extentEnd (window r)) (MarkerMismatch d (name (helper r)))
      bodyEnd <- address ("__" ++ name (helper r) ++ if helper r == Prepare then "_branch_end" else "_body_end")
      require (extentStart (window r) < bodyEnd && bodyEnd <= extentEnd (window r)) (MarkerMismatch d (name (helper r) ++ " body"))
    shimAt <- address "__prepare_body_start"
    bodyAt <- address "xm1_rtos_scheduler_prepare_body"
    bodyEnd <- address "__prepare_body_end"
    findEnd <- address "__find_body_end"
    require (shimAt == extentStart (shim d) && bodyAt == shimAt && bodyEnd > bodyAt && bodyEnd <= extentEnd (shim d) && findEnd <= shimAt) (MarkerMismatch d "prepare shim")
    forM_ syms $ \s -> do
      let lo = GuestAddress (F.symbolAddress s)
          end = toInteger (F.symbolAddress s) + toInteger (F.symbolSize s)
          inside e = lo >= extentStart e && end <= toInteger (let GuestAddress hi = extentEnd e in hi) && (F.symbolSize s == 0 || lo < extentEnd e)
          permittedAbsolute = F.symbolSize s == 0 && (any (\e -> extentContains e lo || lo == extentEnd e) es || (F.symbolName s == "xm1_rtos_scheduler_stock_raise_irq" && lo == extentStart (raise d)))
      require (if F.symbolName s `elem` absolute then permittedAbsolute else any inside es) (SymbolOutside d (F.symbolName s))
    require (B.length bin == fromIntegral (high-low)) (ByteOutside d (GuestAddress high))
    let ordered = sortOn extentStart es
    forM_ (zip (map extentEnd ordered) (map extentStart (drop 1 ordered))) $ \(GuestAddress start, GuestAddress end) ->
      forM_ (B.findIndex (/= 0) (B.take (fromIntegral (end-start)) (B.drop (fromIntegral (start-low)) bin))) $ \offset -> Left (ByteOutside d (GuestAddress (start + fromIntegral offset)))
    forM_ es $ \e@(Extent (GuestAddress start) _) -> forM_ [0,4 .. extentBytes e-4] $ \offset -> do
      let at = start + fromIntegral offset
          covered s = F.symbolSize s > 0 && F.symbolAddress s <= at && toInteger at + 4 <= toInteger (F.symbolAddress s) + toInteger (F.symbolSize s)
      require (any covered syms || U.word32At bin (fromIntegral (at-low)) == 0xe7f000f0) (SlackNotFill d (GuestAddress at))
    let loader = foldl (\bytes e -> put (domainLoaderBase d) e (slice base e bin) bytes) (sourceLoaders source M.! d) es
        codeExtents = concatMap (\r -> if helper r == Find then [Extent (extentStart (window r)) (extentStart (shim d)), shim d] else [window r]) rs
    walked <- either (Left . FrameRejected d) Right (frames (OwnedCode (domainLoaderBase d) loader (raise d : codeExtents)) (map (extentStart . window) rs ++ [extentStart (shim d)]))
    forM_ rs $ \r -> do
      let actual = frameComposed (walked M.! extentStart (window r))
      require (actual == demand r) (FrameMismatch d (helper r) (demand r) actual)
    require (frameComposed (walked M.! extentStart (shim d)) == FrameBytes 16) (FrameMismatch d Prepare (FrameBytes 16) (frameComposed (walked M.! extentStart (shim d))))
    let restoredLoader = if variant == StockInsert && d == DomainA
          then let e = window (rs !! 3) in put (domainLoaderBase d) e (slice (domainLoaderBase d) e (sourceLoaders source M.! d)) loader
          else loader
    actualFrames <- if variant == StockInsert && d == DomainA
      then either (Left . FrameRejected d) Right (frames (OwnedCode (domainLoaderBase d) restoredLoader (raise d : codeExtents)) (map (extentStart . window) rs ++ [extentStart (shim d)]))
      else Right walked
    pure (d, Installed rs restoredLoader linked actualFrames)
  pure (Admitted source (M.fromList installed) variant)
admittedLoader :: Admitted -> Domain -> B.ByteString
admittedLoader admitted domain = installedLoader (admittedDomains admitted M.! domain)

stagedLoader :: Staged -> Domain -> B.ByteString
stagedLoader s d = installedLoader (admittedDomains (stagedAdmission s) M.! d)
rebuild :: U.Parsed -> [F.LoaderWordPatch] -> Either StageError B.ByteString
rebuild parsed patches = do
  parts <- either (Left . PatchRejected) Right (F.applyLoaderWordPatches patches (U.parsedParts parsed))
  dat <- either (Left . PatchRejected) Right (U.rebuildContainer parsed parts)
  consumed <- either (Left . PatchRejected) Right (U.parseContainer dat)
  either (Left . PatchRejected) Right (F.verifyPreTablePatches [] patches parsed consumed)
  pure dat
patchesFor :: Admitted -> U.Parsed -> Bool -> Either StageError [F.LoaderWordPatch]
patchesFor a parsed restoring = fmap concat $ forM domains $ \d -> do
  current <- contained parsed d
  let installed = admittedDomains a M.! d
      target = if restoring then sourceLoaders (admittedSource a) M.! d else installedLoader installed
      GuestAddress base = domainLoaderBase d
  either (Left . PatchRejected) Right $ sequence
    [ F.loaderWordPatch (F.LoaderId (domainLoaderId d)) at before after
    | r <- installedRows installed, let Extent (GuestAddress lo) (GuestAddress hi) = window r
    , at <- [lo,lo+4 .. hi-4], let offset = fromIntegral (at-base)
    , let before = U.word32At current offset, let after = U.word32At target offset, before /= after ]
confineChanges :: Admitted -> B.ByteString -> Either StageError ()
confineChanges a dat = do
  let source = admittedSource a
      original = sourceDat source
      parsed = sourceParsed source
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  ranges <- fmap concat $ forM domains $ \d -> do
    segment <- segmentOf parsed d
    pure [(mainStart + fromIntegral (U.segFlash segment) + fromIntegral (lo - U.segLoad segment), extentBytes e) | r <- installedRows (admittedDomains a M.! d), let e@(Extent (GuestAddress lo) _) = window r]
  require (B.length dat == B.length original) (ChangeOutside (min (B.length dat) (B.length original)))
  let allowed = (U.mainChecksumOffset,4) : ranges
  forM_ (find (\i -> not (any (\(lo,len) -> i >= lo && i < lo+len) allowed)) [i | (i,True) <- zip [0..] (B.zipWith (/=) original dat)]) (Left . ChangeOutside)
restore :: Admitted -> B.ByteString -> Either StageError B.ByteString
restore a dat = do
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  patches <- patchesFor a parsed True
  rebuild parsed patches
stage :: Admitted -> Either StageError Staged
stage a = do
  let source = admittedSource a
  patches <- patchesFor a (sourceParsed source) False
  dat <- rebuild (sourceParsed source) patches
  confineChanges a dat
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  forM_ domains $ \d -> do
    actual <- contained parsed d
    require (actual == installedLoader (admittedDomains a M.! d)) (LoaderMismatch d)
  restored <- restore a dat
  require (restored == sourceDat source) RestoreMismatch
  pure (Staged a dat)

controlLinked :: Variant -> Domain -> Linked -> Either StageError Linked
controlLinked variant d linked
  | variant == SilentNotify && d == DomainA = patchAt 0x4402c 0xe3a01078 0xe24dd004 >>= \changed -> patchOther changed 0x44034 0xebfffcea 0xe28dd004
  | variant == WrongPredicate && d == DomainB = patchAt 0xd3fd4 0xe3e01002 0xe3500001 >>= \c1 -> patchOther c1 0xd3fd8 0xe1100001 0x93a00001 >>= \c2 -> patchOther c2 0xd3fdc 0x03a00001 0x912fff1e >>= \c3 -> patchOther c3 0xd3fe0 0x012fff1e 0xe320f000
  | otherwise = Right linked
  where
    patchAt at before after = patchOther linked at before after
    patchOther input at before after = do
      let GuestAddress low = minimum (map (extentStart . window) (pinnedRows d))
          offset = fromIntegral (at-low)
          bytes = linkedBin input
      require (U.word32At bytes offset == before) (ControlRejected ("owned word differs at " ++ hexText at))
      pure input {linkedBin = B.take offset bytes <> U.putWord32 after <> B.drop (offset+4) bytes}

contractValue :: Staged -> Value
contractValue staged = object
  [ "schema" .= (1 :: Int), "piece_kind" .= ("kernel-scheduler" :: String)
  , "scope" .= ("stock plus S1 helpers; own-first slots are not included" :: String)
  , "variant" .= show (admittedVariant admitted)
  , "control_construction" .= ("notify controls patch same-size owned words before the same admission; stock-insert restores loader A insert after candidate admission" :: String)
  , "source_dat_sha256" .= sha (sourceDat source), "candidate_dat_sha256" .= sha (stagedDat staged)
  , "loaders" .= map loader domains ]
  where
    admitted = stagedAdmission staged
    source = admittedSource admitted
    address (GuestAddress at) = hexText at
    loader d = let installed = admittedDomains admitted M.! d in object
      [ "domain" .= domainLabel d, "stock_sha256" .= sha (sourceLoaders source M.! d), "owned_sha256" .= sha (installedLoader installed)
      , "linked_bin_sha256" .= sha (linkedBin (installedLinked installed)), "linked_symbols_sha256" .= sha (linkedSymbols (installedLinked installed))
      , "helpers" .= [object ["helper" .= name (helper r), "entry_symbol" .= symbolText (symbol r), "start" .= address (extentStart (window r)), "end" .= address (extentEnd (window r)), "stock_sha256" .= sha (slice (domainLoaderBase d) (window r) (sourceLoaders source M.! d)), "owned_sha256" .= sha (slice (domainLoaderBase d) (window r) (installedLoader installed)), "frame_bytes" .= (let FrameBytes n = frameComposed (installedFrames installed M.! extentStart (window r)) in n)] | r <- installedRows installed]
      , "prepare_shim" .= object ["start" .= address (extentStart (shim d)), "end" .= address (extentEnd (shim d)), "entry_symbol" .= ("xm1_rtos_scheduler_prepare_body" :: String), "stock_sha256" .= sha (slice (domainLoaderBase d) (shim d) (sourceLoaders source M.! d)), "owned_sha256" .= sha (slice (domainLoaderBase d) (shim d) (installedLoader installed)), "frame_bytes" .= (let FrameBytes n = frameComposed (installedFrames installed M.! extentStart (shim d)) in n)]
      , "entry_table" .= [object ["helper" .= name (helper r), "stock_entry" .= address (extentStart (window r)), "owned_entry" .= address (extentStart (window r)), "entry_symbol" .= symbolText (symbol r)] | r <- installedRows installed] ]
