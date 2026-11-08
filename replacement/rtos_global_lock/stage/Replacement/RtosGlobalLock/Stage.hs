{-# LANGUAGE OverloadedStrings #-}

-- | Admission and in-place staging of the owned global-lock extents over loaders A and B.
module Replacement.RtosGlobalLock.Stage
  ( Linked (..)
  , Source
  , Admitted
  , Staged
  , StageError (..)
  , canonicalSource
  , requirePinned
  , admit
  , stage
  , stagedDat
  , stagedLoader
  , contractValue
  , extentTable
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domains, domainLoaderBase, domainLoaderId, domainLoaderSize)

data Linked = Linked {linkedBin :: B.ByteString, linkedSymbols :: B.ByteString}

data Source = Source
  { sourceDat :: B.ByteString
  , sourceParsed :: U.Parsed
  , sourceLoaders :: M.Map Domain B.ByteString
  , sourceContained :: M.Map Domain B.ByteString
  }

data Row = Row {rowSymbol :: String, rowStart :: Word32, rowLength :: Word32, rowStock :: [Word32]}

data Installed = Installed
  { installedRow :: Row
  , installedStock :: B.ByteString
  , installedOwned :: B.ByteString
  }

data Admitted = Admitted
  { admittedSource :: Source
  , admittedInstalled :: M.Map Domain [Installed]
  , admittedLoaders :: M.Map Domain B.ByteString
  }

data Staged = Staged {stagedAdmission :: Admitted, stagedDat :: B.ByteString}

data StageError
  = ContainerRejected
  | LoaderMismatch Domain
  | SourceUnpinned
  | SymbolsRejected Domain
  | SymbolMissing Domain String
  | EntryNotAtStart Domain String
  | SymbolOutsideExtents Domain String
  | StockMismatch Domain String
  | SlackNotFill Domain GuestAddress
  | OwnedByteOutsideExtents Domain GuestAddress
  | PatchRejected String
  | ChangeOutside Int
  | StagedLoaderMismatch Domain
  | RestoreMismatch
  deriving (Eq, Show)

require :: Bool -> StageError -> Either StageError ()
require ok err = unless ok (Left err)

sha :: B.ByteString -> String
sha = SHA.sha256Hex . SHA.sha256

fillWord :: Word32
fillWord = 0xe7f000f0

pins :: [(Maybe Domain, String)]
pins =
  [ (Nothing, "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490")
  , (Just DomainA, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882")
  , (Just DomainB, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")
  ]

segmentOf :: U.Parts -> Domain -> Either StageError U.SegmentEntry
segmentOf parts domain = do
  segment <- case filter ((== domainLoaderId domain) . U.segId) (U.partSegments parts) of
    [found] -> Right found
    _ -> Left (LoaderMismatch domain)
  let GuestAddress base = domainLoaderBase domain
      size = fromIntegral (U.segSize segment)
  require (U.segLoad segment == base && size == domainLoaderSize domain
    && fromIntegral (U.segFlash segment) + size <= B.length (U.partPreTable parts)) (LoaderMismatch domain)
  pure segment

containedLoader :: U.Parts -> Domain -> Either StageError B.ByteString
containedLoader parts domain = do
  segment <- segmentOf parts domain
  pure (B.map complement (B.take (fromIntegral (U.segSize segment)) (B.drop (fromIntegral (U.segFlash segment)) (U.partPreTable parts))))

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> Either StageError Source
canonicalSource dat loaderA loaderB = do
  parsed <- either (const (Left ContainerRejected)) Right (U.parseContainer dat)
  require (U.rebuildContainer parsed (U.parsedParts parsed) == Right dat) ContainerRejected
  contained <- forM domains $ \domain -> (,) domain <$> containedLoader (U.parsedParts parsed) domain
  let given = M.fromList (zip domains [loaderA, loaderB])
  forM_ domains $ \domain -> require (B.length (given M.! domain) == domainLoaderSize domain) (LoaderMismatch domain)
  pure (Source dat parsed given (M.fromList contained))

requirePinned :: Source -> Either StageError ()
requirePinned source = forM_ pins $ \(domain, expected) ->
  let actual = maybe (sourceDat source) (\d -> sourceLoaders source M.! d) domain
   in require (sha actual == expected) SourceUnpinned

slotTry, slotRelease, slotObserve, helperTry, helperRelease, helperObserve :: [Word32]
slotTry = [0xe92d4800, 0xe28db004, 0xe3500000, 0x03000804, 0x0344000c, 0xeb00053b, 0xe8bd8800]
slotRelease = [0xe92d4800, 0xe28db004, 0xe3500000, 0x03000804, 0x0344000c, 0xeb00053e, 0xe8bd8800]
slotObserve = [0xe92d4800, 0xe28db004, 0xe3500000, 0x03000804, 0x0344000c, 0xeb000542, 0xe8bd8800]
helperTry =
  [0xe1902f9f, 0xe3520000, 0x13e00000, 0x112fff1e, 0x01802f91, 0xe3520000, 0x0e072fba, 0x03a00000, 0x012fff1e, 0xeafffff5]
helperRelease =
  [0xe3a01000, 0xe1902f9f, 0xe3520000, 0x03e00000, 0x012fff1e, 0x11802f91, 0xe3520000, 0x0e072fba, 0x03a00000, 0x012fff1e, 0xeafffff4]
helperObserve = [0xe5900000, 0xe12fff1e]

loaderBase :: Domain -> Word32
loaderBase domain = let GuestAddress base = domainLoaderBase domain in base

rows :: Domain -> [Row]
rows domain =
  [ Row "xm1_rtos_global_lock_try" (base + 0x12cc) 0x1c slotTry
  , Row "xm1_rtos_global_lock_release" (base + 0x12e8) 0x1c slotRelease
  , Row "xm1_rtos_global_lock_observe" (base + 0x1304) 0x1c slotObserve
  , Row "helper_try" (base + 0x27d4) 0x28 helperTry
  , Row "helper_release" (base + 0x27fc) 0x2c helperRelease
  , Row "helper_observe" (base + 0x2828) 0x8 helperObserve
  ]
  where
    GuestAddress base = domainLoaderBase domain

rowEnd :: Row -> Word32
rowEnd row = rowStart row + rowLength row

extentTable :: Domain -> [(Word32, Word32)]
extentTable domain = [(rowStart row, rowEnd row) | row <- rows domain]

slice :: Word32 -> Row -> B.ByteString -> B.ByteString
slice base row = B.take (fromIntegral (rowLength row)) . B.drop (fromIntegral (rowStart row - base))

replaceExtents :: Word32 -> [(Row, B.ByteString)] -> B.ByteString -> B.ByteString
replaceExtents base replacements original = foldl put original replacements
  where
    put bytes (row, replacement) =
      let at = fromIntegral (rowStart row - base) in B.take at bytes <> replacement <> B.drop (at + B.length replacement) bytes

firstNonzero :: Word32 -> B.ByteString -> Maybe GuestAddress
firstNonzero start bytes = (\index -> GuestAddress (start + fromIntegral index)) <$> B.findIndex (/= 0) bytes

installRow :: Domain -> Word32 -> B.ByteString -> [F.Symbol] -> Row -> B.ByteString -> Either StageError Installed
installRow domain binBase bin symbols row stock = do
  installed <- forM words32 $ \(address, word) ->
    if covered address || word == fillWord
      then Right word
      else if word == 0 then Right fillWord else Left (SlackNotFill domain (GuestAddress address))
  pure (Installed row stock (B.concat (map U.putWord32 installed)))
  where
    raw = slice binBase row bin
    padded = raw <> B.replicate (fromIntegral (rowLength row) - B.length raw) 0
    words32 = [(rowStart row + fromIntegral offset, U.word32At padded offset) | offset <- [0, 4 .. fromIntegral (rowLength row) - 4]]
    spans = [(F.symbolAddress symbol, F.symbolAddress symbol + F.symbolSize symbol) | symbol <- symbols]
    covered address = any (\(low, high) -> low <= address && address < high) spans

admitDomain :: Source -> Domain -> Linked -> Either StageError (Domain, [Installed], B.ByteString)
admitDomain source domain (Linked bin symbolsBytes) = do
  symbols <- either (const (Left (SymbolsRejected domain))) Right (F.parseSymbols symbolsBytes)
  let GuestAddress base = domainLoaderBase domain
      given = sourceLoaders source M.! domain
      contained = sourceContained source M.! domain
      table = rows domain
  require (given == contained) (LoaderMismatch domain)
  forM_ table $ \row -> do
    symbol <- case filter ((== rowSymbol row) . F.symbolName) symbols of
      [found] -> Right found
      _ -> Left (SymbolMissing domain (rowSymbol row))
    require (F.symbolAddress symbol == rowStart row) (EntryNotAtStart domain (rowSymbol row))
    require (F.symbolSize symbol <= rowLength row) (EntryNotAtStart domain (rowSymbol row))
    require (slice base row given == B.concat (map U.putWord32 (rowStock row))) (StockMismatch domain (rowSymbol row))
  let extents = [(rowStart row, rowEnd row) | row <- table]
      inside symbol =
        let low = F.symbolAddress symbol
            high = toInteger low + toInteger (F.symbolSize symbol)
            within (start, end) = low >= start && high <= toInteger end && (F.symbolSize symbol == 0 || low < end)
         in any within extents
  forM_ symbols $ \symbol -> require (inside symbol) (SymbolOutsideExtents domain (F.symbolName symbol))
  let binLow = minimum (map rowStart table)
      binHigh = maximum (map rowEnd table)
      offset address = fromIntegral (address - binLow)
      gaps = zip (map rowEnd table) (map rowStart (drop 1 table))
  require (B.length bin <= fromIntegral (binHigh - binLow)) (OwnedByteOutsideExtents domain (GuestAddress binHigh))
  forM_ gaps $ \(gapStart, gapEnd) ->
    forM_ (firstNonzero gapStart (B.take (offset gapEnd - offset gapStart) (B.drop (offset gapStart) bin))) $
      Left . OwnedByteOutsideExtents domain
  installed <- forM table $ \row -> installRow domain binLow bin symbols row (slice base row given)
  let loader = replaceExtents base [(installedRow entry, installedOwned entry) | entry <- installed] given
  pure (domain, installed, loader)

admit :: Source -> Linked -> Linked -> Either StageError Admitted
admit source linkedA linkedB = do
  pairs <- forM (zip domains [linkedA, linkedB]) $ \(domain, linked) -> do
    (domain', installed, loader) <- admitDomain source domain linked
    pure (domain', (installed, loader))
  pure
    ( Admitted
        source
        (M.fromList [(domain, installed) | (domain, (installed, _)) <- pairs])
        (M.fromList [(domain, loader) | (domain, (_, loader)) <- pairs])
    )

stagedLoader :: Staged -> Domain -> B.ByteString
stagedLoader staged domain = admittedLoaders (stagedAdmission staged) M.! domain

extentPatches :: Admitted -> Domain -> (Installed -> B.ByteString) -> (Installed -> B.ByteString) -> Either StageError [F.LoaderWordPatch]
extentPatches admitted domain currentOf wantedOf = fmap concat $ forM (admittedInstalled admitted M.! domain) $ \entry -> do
  let current = currentOf entry
      wanted = wantedOf entry
      length' = fromIntegral (rowLength (installedRow entry))
  fmap concat $ forM [0, 4 .. length' - 4] $ \offset -> do
    let before = U.word32At current offset
        after = U.word32At wanted offset
    if before == after
      then pure []
      else (: []) <$> either (Left . PatchRejected) Right (F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) (rowStart (installedRow entry) + fromIntegral offset) before after)

rebuild :: U.Parsed -> [F.LoaderWordPatch] -> Either StageError B.ByteString
rebuild parsed patches = do
  parts <- either (Left . PatchRejected) Right (F.applyLoaderWordPatches patches (U.parsedParts parsed))
  bytes <- either (Left . PatchRejected) Right (U.rebuildContainer parsed parts)
  consumed <- either (const (Left ContainerRejected)) Right (U.parseContainer bytes)
  either (Left . PatchRejected) Right (F.verifyPreTablePatches [] patches parsed consumed)
  pure bytes

extentRanges :: Source -> Admitted -> [(Int, Int)]
extentRanges source admitted = concatMap ranges domains
  where
    mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader (sourceParsed source)))
    ranges domain = case segmentOf (U.parsedParts (sourceParsed source)) domain of
      Left _ -> []
      Right segment ->
        [ ( mainStart + fromIntegral (U.segFlash segment) + fromIntegral (rowStart (installedRow entry) - U.segLoad segment)
          , fromIntegral (rowLength (installedRow entry))
          )
        | entry <- admittedInstalled admitted M.! domain
        ]

firstChangeOutside :: [(Int, Int)] -> B.ByteString -> B.ByteString -> Maybe Int
firstChangeOutside allowed before after =
  case [index | (index, True) <- zip [0 ..] (B.zipWith (/=) before after), not (within index)] of
    [] -> Nothing
    (index : _) -> Just index
  where
    within index = any (\(start, size) -> index >= start && index < start + size) allowed

confineChanges :: Source -> Admitted -> B.ByteString -> Either StageError ()
confineChanges source admitted staged = do
  let dat = sourceDat source
  require (B.length staged == B.length dat) (ChangeOutside 0)
  case firstChangeOutside ((U.mainChecksumOffset, 4) : extentRanges source admitted) dat staged of
    Nothing -> Right ()
    Just index -> Left (ChangeOutside index)

restore :: Admitted -> B.ByteString -> Either StageError B.ByteString
restore admitted dat = do
  parsed <- either (const (Left ContainerRejected)) Right (U.parseContainer dat)
  patches <- fmap concat $ forM domains $ \domain -> do
    current <- containedLoader (U.parsedParts parsed) domain
    extentPatches admitted domain (\entry -> slice (loaderBase domain) (installedRow entry) current) installedStock
  rebuild parsed patches

verifyRestore :: Source -> Admitted -> B.ByteString -> Either StageError ()
verifyRestore source admitted dat = do
  restored <- restore admitted dat
  require (restored == sourceDat source) RestoreMismatch

stage :: Source -> Admitted -> Either StageError Staged
stage source admitted = do
  patches <- fmap concat $ forM domains $ \domain ->
    extentPatches admitted domain (\entry -> slice (loaderBase domain) (installedRow entry) (sourceLoaders source M.! domain)) installedOwned
  staged <- rebuild (sourceParsed source) patches
  confineChanges source admitted staged
  consumed <- either (const (Left ContainerRejected)) Right (U.parseContainer staged)
  forM_ domains $ \domain -> do
    loader <- containedLoader (U.parsedParts consumed) domain
    require (loader == admittedLoaders admitted M.! domain) (StagedLoaderMismatch domain)
  verifyRestore source admitted staged
  pure (Staged admitted staged)

contractValue :: Staged -> Value
contractValue staged =
  object
    [ "dat_sha256" .= sha (sourceDat source)
    , "candidate_dat_sha256" .= sha (stagedDat staged)
    , "fill_word" .= fillWord
    , "domains" .= object [label domain .= domainValue domain | domain <- domains]
    ]
  where
    source = admittedSource (stagedAdmission staged)
    label DomainA = "a"
    label DomainB = "b"
    domainValue domain =
      object
        [ "loader_base" .= hex (domainLoaderBase domain)
        , "loader_sha256" .= sha (sourceLoaders source M.! domain)
        , "staged_loader_sha256" .= sha (admittedLoaders (stagedAdmission staged) M.! domain)
        , "extents" .=
            [ object
                [ "symbol" .= rowSymbol (installedRow entry)
                , "start" .= hex (GuestAddress (rowStart (installedRow entry)))
                , "end" .= hex (GuestAddress (rowEnd (installedRow entry)))
                , "stock_sha256" .= sha (installedStock entry)
                , "owned_sha256" .= sha (installedOwned entry)
                ]
            | entry <- admittedInstalled (stagedAdmission staged) M.! domain
            ]
        ]

hex :: GuestAddress -> String
hex (GuestAddress value) = "0x" ++ showHexWord value

showHexWord :: Word32 -> String
showHexWord value = go value ""
  where
    go 0 acc = if null acc then "0" else acc
    go n acc = go (n `div` 16) (("0123456789abcdef" !! fromIntegral (n `mod` 16)) : acc)
