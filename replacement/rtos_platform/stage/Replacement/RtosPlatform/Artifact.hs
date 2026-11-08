{-# LANGUAGE OverloadedStrings #-}

-- | Admission, staging and restore of the aggregate platform image over the
-- declared retired-carrier union.
module Replacement.RtosPlatform.Artifact
  ( CanonicalSource (..)
  , Linked (..)
  , Admitted (..)
  , DomainAdmitted (..)
  , Staged (..)
  , StageError (..)
  , canonicalSource
  , requirePinned
  , admit
  , stage
  , restore
  , confineChanges
  , stagedLoader
  , contractValue
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isDigit)
import Data.List (find)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex)
import Replacement.RtosOwnFirst.Frame (Frame, FrameError, framesWith, OwnedCode (..))
import Replacement.RtosOwnFirst.Regions (Domain (..), Extent (..), GuestAddress (..), domains, domainLabel, domainLoaderBase, domainLoaderId, domainLoaderSize, extentBytes, extentContains, hexText)
import Replacement.RtosPlatform.Union

data CanonicalSource = CanonicalSource
  { sourceDat :: B.ByteString
  , sourceParsed :: U.Parsed
  , sourceLoaders :: M.Map Domain B.ByteString
  }

data Linked = Linked
  { linkedBinA :: B.ByteString
  , linkedSymbolsA :: B.ByteString
  , linkedDisasmA :: B.ByteString
  , linkedPlacementsA :: [(Word32, Int)]
  , linkedBinB :: B.ByteString
  , linkedSymbolsB :: B.ByteString
  , linkedDisasmB :: B.ByteString
  , linkedPlacementsB :: [(Word32, Int)]
  }

data DomainAdmitted = DomainAdmitted
  { domainSymbols :: [F.Symbol]
  , domainInstalled :: [(Extent, B.ByteString)]
  , domainFrames :: M.Map GuestAddress Frame
  , domainFramePremises :: [String]
  }

data Admitted = Admitted
  { admittedUnion :: Union
  , admittedSource :: CanonicalSource
  , admittedLinked :: Linked
  , admittedDomains :: M.Map Domain DomainAdmitted
  }

data Staged = Staged
  { stagedAdmitted :: Admitted
  , stagedDat :: B.ByteString
  , stagedLoaders :: M.Map Domain B.ByteString
  }

data StageError
  = ContainerRejected
  | SegmentRejected Domain
  | LoaderMismatch Domain
  | SourceUnpinned String
  | FitRejected FitError
  | SymbolsUnparsed Domain
  | EntryMissing Domain Entry
  | EntryMoved Domain Entry GuestAddress
  | VeneerSize Domain Entry Int
  | SymbolOutsideCarrier Domain String GuestAddress Int
  | DemandMissing Domain Demand
  | DemandSize Domain Demand Int
  | DataSymbol Domain String
  | SlackNotFillOrZero Domain GuestAddress
  | ExternalCall Domain GuestAddress
  | FrameRejected Domain GuestAddress FrameError
  | ChangeOutside Int
  | StagedLoaderMismatch Domain
  | PatchRejected String
  | RestoreMismatch
  deriving (Eq, Show)

require :: Bool -> StageError -> Either StageError ()
require ok err = unless ok (Left err)

sha :: B.ByteString -> String
sha = SHA.sha256Hex . SHA.sha256

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> Either StageError CanonicalSource
canonicalSource dat loaderA loaderB = do
  parsed <- either (const (Left ContainerRejected)) Right (U.parseContainer dat)
  require (U.rebuildContainer parsed (U.parsedParts parsed) == Right dat) ContainerRejected
  loaders <- forM domains $ \domain -> (,) domain <$> containedLoader (U.parsedParts parsed) domain
  let given = M.fromList (zip domains [loaderA, loaderB])
  forM_ domains $ \domain -> require (B.length (given M.! domain) == domainLoaderSize domain) (LoaderMismatch domain)
  pure (CanonicalSource dat parsed (M.fromList loaders))

requirePinned :: CanonicalSource -> Either StageError ()
requirePinned source =
  require (sha (sourceDat source) == "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") (SourceUnpinned "dat")
    >> require (sha (sourceLoaders source M.! DomainA) == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (SourceUnpinned "loader-a")
    >> require (sha (sourceLoaders source M.! DomainB) == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (SourceUnpinned "loader-b")

segmentOf :: U.Parts -> Domain -> Either StageError U.SegmentEntry
segmentOf parts domain = do
  segment <- case filter ((== domainLoaderId domain) . U.segId) (U.partSegments parts) of
    [found] -> Right found
    _ -> Left (SegmentRejected domain)
  let GuestAddress base = domainLoaderBase domain
      size = fromIntegral (U.segSize segment)
  require (U.segLoad segment == base && size == domainLoaderSize domain && fromIntegral (U.segFlash segment) + size <= B.length (U.partPreTable parts)) (SegmentRejected domain)
  pure segment

containedLoader :: U.Parts -> Domain -> Either StageError B.ByteString
containedLoader parts domain = do
  segment <- segmentOf parts domain
  pure (B.map complement (B.take (fromIntegral (U.segSize segment)) (B.drop (fromIntegral (U.segFlash segment)) (U.partPreTable parts))))

symbolRows :: Domain -> B.ByteString -> Either StageError [F.Symbol]
symbolRows domain bytes = either (const (Left (SymbolsUnparsed domain))) Right (F.parseSymbols bytes)

-- | Non-absolute symbols whose nm type letter is data.
dataSymbols :: B.ByteString -> [String]
dataSymbols bytes =
  [ C.unpack name
  | row <- C.lines bytes
  , [_, _, kind, name] <- [C.words row]
  , C.unpack kind `elem` ["d", "D", "b", "B"]
  ]

addressOf :: Domain -> [F.Symbol] -> String -> Either StageError GuestAddress
addressOf domain symbols name = case [GuestAddress (F.symbolAddress s) | s <- symbols, F.symbolName s == name] of
  [at] -> Right at
  _ -> Left (SymbolsUnparsed domain)

slice :: GuestAddress -> Extent -> B.ByteString -> B.ByteString
slice (GuestAddress base) extent@(Extent (GuestAddress start) _) =
  B.take (extentBytes extent) . B.drop (fromIntegral (start - base))

put :: GuestAddress -> Extent -> B.ByteString -> B.ByteString -> B.ByteString
put (GuestAddress base) extent@(Extent (GuestAddress start) _) replacement original =
  let at = fromIntegral (start - base) in B.take at original <> replacement <> B.drop (at + extentBytes extent) original

installCarrier :: Domain -> Word32 -> [F.Symbol] -> [(Word32, Int)] -> B.ByteString -> Extent -> Either StageError B.ByteString
installCarrier domain fill symbols placements raw extent = do
  let padded = raw <> B.replicate (extentBytes extent - B.length raw) 0
      spans = [(F.symbolAddress s, F.symbolAddress s + F.symbolSize s) | s <- symbols, F.symbolSize s > 0]
      covered at = any (\(lo, hi) -> lo <= at && at < hi) spans
        || any (\(lo, len) -> lo <= at && at < lo + fromIntegral len) placements
      words32 = [U.word32At padded off | off <- [0, 4 .. extentBytes extent - 4]]
  forM_ (zip [0, 4 .. extentBytes extent - 4] words32) $ \(off, word) ->
    when (not (covered (startAddress + fromIntegral off)) && word /= 0 && word /= fill) (Left (SlackNotFillOrZero domain (GuestAddress (startAddress + fromIntegral off))))
  pure (B.concat (map U.putWord32 (map (\word -> if word == 0 then fill else word) words32)))
  where
    GuestAddress startAddress = extentStart extent

withinRun :: Domain -> [Extent] -> F.Symbol -> Either StageError ()
withinRun domain runs_ symbol =
  let lo = F.symbolAddress symbol
      hi = toInteger lo + toInteger (F.symbolSize symbol)
      inside (Extent (GuestAddress start) (GuestAddress end)) = lo >= start && hi <= toInteger end && (F.symbolSize symbol == 0 || lo < end)
  in require (any inside runs_) (SymbolOutsideCarrier domain (F.symbolName symbol) (GuestAddress lo) (fromIntegral (F.symbolSize symbol)))

hexTarget :: B.ByteString -> Maybe Word32
hexTarget text = case C.unpack text of
  '0' : 'x' : digits | not (null digits) && all isDigit digits -> case readHex digits of
    [(value, "")] | value <= toInteger (maxBound :: Word32) -> Just (fromInteger value)
    _ -> Nothing
  _ -> Nothing

-- | Targets of direct branches and calls to an absolute address.
branchTargets :: B.ByteString -> [Word32]
branchTargets text = concatMap line (C.lines text)
  where
    line raw = case C.words (C.dropWhile (== ' ') raw) of
      (addr : rest) | not (C.null addr) && C.last addr == ':' -> case rest of
        (op : operand : _)
          | op == "b" || op == "bl" || op == "blx" -> maybe [] pure (hexTarget operand)
        _ -> []
      _ -> []

admit :: Union -> CanonicalSource -> Linked -> Either StageError Admitted
admit union source linked = do
  forM_ domains $ \domain -> either (Left . FitRejected) Right (fit (domainMap union domain))
  admitted <- forM domains $ \domain -> do
    let dm = domainMap union domain
        linkedBin = if domain == DomainA then linkedBinA linked else linkedBinB linked
        linkedSymbols = if domain == DomainA then linkedSymbolsA linked else linkedSymbolsB linked
        linkedDisasm = if domain == DomainA then linkedDisasmA linked else linkedDisasmB linked
        placements = if domain == DomainA then linkedPlacementsA linked else linkedPlacementsB linked
        binBase = minimum (map (extentStart . carrierExtent) (mapCarriers dm))
        bin = linkedBin
        runExtents = runs dm
    symbols <- symbolRows domain linkedSymbols
    forM_ (dataSymbols linkedSymbols) $ \name -> Left (DataSymbol domain name)
    forM_ (mapEntries dm) $ \entry -> do
      at <- addressOf domain symbols (entryVeneer entry)
      require (at == entryAddress entry) (EntryMoved domain entry at)
      let veneer = [s | s <- symbols, F.symbolName s == entryVeneer entry]
      forM_ veneer $ \s -> require (F.symbolSize s == 4) (VeneerSize domain entry (fromIntegral (F.symbolSize s)))
    let veneers = [entryVeneer e | e <- mapEntries dm]
    forM_ (filter (\s -> F.symbolSize s > 0 && F.symbolName s `notElem` veneers) symbols) $ \s -> withinRun domain runExtents s
    forM_ (mapDemand dm) $ \d -> case [s | s <- symbols, F.symbolName s == demandSymbol d] of
      [] -> Left (DemandMissing domain d)
      (s : _) -> require (fromIntegral (F.symbolSize s) == demandBytes d) (DemandSize domain d (fromIntegral (F.symbolSize s)))
    installed <- forM (mapCarriers dm) $ \c -> do
      let extent = carrierExtent c
      bytes <- installCarrier domain (unionFill union) symbols placements (slice binBase extent bin) extent
      pure (extent, bytes)
    forM_ (branchTargets linkedDisasm) $ \target ->
      let inUnion = any (\c -> extentContains (carrierExtent c) (GuestAddress target)) (mapCarriers dm)
          declared = [e | e <- mapExternals dm, externalAddress e == GuestAddress target]
      in unless (inUnion || not (null declared)) (Left (ExternalCall domain (GuestAddress target)))
    let bodyEntries =
          [ F.symbolAddress s
          | e <- mapEntries dm
          , s <- symbols
          , F.symbolName s == entrySymbol e
          ]
        externals = S.fromList [addr | e <- mapExternals dm, let GuestAddress addr = externalAddress e]
        walkedResult = framesWith externals (OwnedCode binBase bin runExtents) (map GuestAddress bodyEntries)
    (walked, premises) <- case walkedResult of
      Left err -> pure (M.empty, [show err])
      Right (walkedFrames, externalEdges) -> pure (walkedFrames, map show externalEdges)
    pure (domain, DomainAdmitted symbols installed walked premises)
  pure (Admitted union source linked (M.fromList admitted))

stage :: Admitted -> Either StageError Staged
stage admitted = do
  let source = admittedSource admitted
      union = admittedUnion admitted
  owned <- forM domains $ \domain -> do
    let dm = domainMap union domain
        base = mapBase dm
        stock = sourceLoaders source M.! domain
        installed = domainInstalled (admittedDomains admitted M.! domain)
        loader = foldl (\bytes (extent, ownedBytes) -> put base extent ownedBytes bytes) stock installed
    pure (domain, loader)
  let loaders = M.fromList owned
  patches <- fmap concat $ forM domains $ \domain -> do
    let dm = domainMap union domain
        current = sourceLoaders source M.! domain
        target = loaders M.! domain
    either (Left . PatchRejected) Right $
      sequence
        [ F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) (start + fromIntegral off) (U.word32At current at) (U.word32At target at)
        | c <- mapCarriers dm
        , let extent@(Extent (GuestAddress start) _) = carrierExtent c
        , let GuestAddress loaderBase = mapBase dm
        , off <- [0, 4 .. extentBytes extent - 4]
        , let at = fromIntegral (start - loaderBase) + off
        , U.word32At current at /= U.word32At target at
        ]
  dat <- rebuild (sourceParsed source) patches
  confineChanges admitted dat
  consumed <- either (const (Left ContainerRejected)) Right (U.parseContainer dat)
  forM_ domains $ \domain -> do
    loader <- containedLoader (U.parsedParts consumed) domain
    require (loader == loaders M.! domain) (StagedLoaderMismatch domain)
  restored <- restore admitted dat
  require (restored == sourceDat source) RestoreMismatch
  pure (Staged admitted dat loaders)

rebuild :: U.Parsed -> [F.LoaderWordPatch] -> Either StageError B.ByteString
rebuild parsed patches = do
  let step = either (const (Left ContainerRejected)) Right
  parts <- step (F.applyLoaderWordPatches patches (U.parsedParts parsed))
  bytes <- step (U.rebuildContainer parsed parts)
  consumed <- step (U.parseContainer bytes)
  step (F.verifyPreTablePatches [] patches parsed consumed)
  pure bytes

restore :: Admitted -> B.ByteString -> Either StageError B.ByteString
restore admitted dat = do
  parsed <- either (const (Left ContainerRejected)) Right (U.parseContainer dat)
  patches <- fmap concat $ forM domains $ \domain -> do
    current <- containedLoader (U.parsedParts parsed) domain
    let dm = domainMap (admittedUnion admitted) domain
        stock = sourceLoaders (admittedSource admitted) M.! domain
    either (Left . PatchRejected) Right $
      sequence
        [ F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) (start + fromIntegral off) (U.word32At current at) (U.word32At stock at)
        | c <- mapCarriers dm
        , let extent@(Extent (GuestAddress start) _) = carrierExtent c
        , let GuestAddress loaderBase = mapBase dm
        , off <- [0, 4 .. extentBytes extent - 4]
        , let at = fromIntegral (start - loaderBase) + off
        , U.word32At current at /= U.word32At stock at
        ]
  rebuild parsed patches

confineChanges :: Admitted -> B.ByteString -> Either StageError ()
confineChanges admitted dat = do
  let source = admittedSource admitted
      original = sourceDat source
      parsed = sourceParsed source
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  ranges <- fmap concat $ forM domains $ \domain -> do
    segment <- segmentOf (U.parsedParts parsed) domain
    pure
      [ (mainStart + fromIntegral (U.segFlash segment) + fromIntegral (start - U.segLoad segment), extentBytes extent)
      | c <- mapCarriers (domainMap (admittedUnion admitted) domain)
      , let extent@(Extent (GuestAddress start) _) = carrierExtent c
      ]
  require (B.length dat == B.length original) (ChangeOutside (min (B.length dat) (B.length original)))
  let allowed = (U.mainChecksumOffset, 4) : ranges
  forM_ (find (\i -> not (any (\(lo, len) -> i >= lo && i < lo + len) allowed)) [i | (i, True) <- zip [0 ..] (B.zipWith (/=) original dat)]) (Left . ChangeOutside)

stagedLoader :: Staged -> Domain -> B.ByteString
stagedLoader staged domain = stagedLoaders staged M.! domain

contractValue :: Staged -> Value
contractValue staged = object
  [ "schema" .= (1 :: Int)
  , "piece_kind" .= ("rtos-platform" :: String)
  , "candidate_dat_sha256" .= sha (stagedDat staged)
  , "loaders" .=
      [ object
          [ "domain" .= domainLabel domain
          , "stock_sha256" .= sha (sourceLoaders source M.! domain)
          , "staged_sha256" .= sha (stagedLoader staged domain)
          , "carriers" .= [object ["group" .= groupLabel (carrierGroup c), "start" .= addressText (extentStart (carrierExtent c)), "end" .= addressText (extentEnd (carrierExtent c))] | c <- mapCarriers (domainMap union domain)]
          ]
      | domain <- domains
      ]
  , "entries" .=
      [ object
          [ "group" .= groupLabel (entryGroup e)
          , "name" .= entryName e
          , "address" .= addressText (entryAddress e)
          , "symbol" .= entrySymbol e
          , "veneer" .= entryVeneer e
          ]
      | domain <- domains, e <- mapEntries (domainMap union domain)
      ]
  , "frame_premises" .=
      [ object ["domain" .= domainLabel domain, "premise" .= premise]
      | domain <- domains, premise <- domainFramePremises (admittedDomains admitted M.! domain)
      ]
  ]
  where
    admitted = stagedAdmitted staged
    source = admittedSource admitted
    union = admittedUnion admitted
    addressText (GuestAddress value) = hexText value
