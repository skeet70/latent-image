{-# LANGUAGE OverloadedStrings #-}
-- | The flag/mailbox object family and its shared helper carriers, staged over the qualified
-- scheduler candidate. The family image also embeds the S1/S2 dependency spans; those are
-- compared byte for byte with the qualified inputs and are not staged as a new group.
module Replacement.RtosScheduler.Family
  ( FamilySlot (..), SlotClass (..), slotClass, slotSection
  , FamilyImage (..), FamilyRow (..), FamilyError (..), FamilyControl (..)
  , FamilyAdmission, FamilyStaged
  , familyRows, familyAbsolutes, familyPins
  , admitFamily, admitFamilyRows, stageFamily, familyDat, familyLoader, familyContract, familyCatalog, familyServiceName
  , confineFamily, restoreFamily, familyControls, disjointWindows, baseDisjoint
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Bits (xor)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.List (find, sortOn)
import qualified Data.Map.Strict as M
import Data.String (fromString)
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (readHex, showHex)
import Replacement.RtosOwnFirst.Regions
import qualified Replacement.RtosScheduler.Artifact as S
import Replacement.RtosScheduler.Cutover
  ( GroupInput (..), CutoverAdmission, CutoverStaged, admittedExtents, cutoverDat, cutoverLoader, rebuild, loadersFrom, restoreCutover )
import Replacement.RtosScheduler.CutoverPins

data FamilySlot
  = FamilyRelease
  | HelperRelease
  | HelperBlock
  | HelperBlockTimed
  | HelperCheck
  | FlagCreate
  | FlagDelete
  | FlagSet
  | FlagSetHandler
  | FlagClear
  | FlagWait
  | FlagPoll
  | FlagTimedWait
  | MailboxCreate
  | MailboxDelete
  | MailboxSend
  | MailboxReceive
  | MailboxPoll
  | MailboxTimedReceive
  deriving (Eq, Ord, Show, Enum, Bounded)

data SlotClass = CarrierSlot | EntrySlot deriving (Eq, Show)

slotClass :: FamilySlot -> SlotClass
slotClass slot = case slot of
  FamilyRelease -> CarrierSlot
  HelperRelease -> CarrierSlot
  HelperBlock -> CarrierSlot
  HelperBlockTimed -> CarrierSlot
  HelperCheck -> CarrierSlot
  _ -> EntrySlot

-- | The linker-script section name, which is also the body and shared marker stem.
slotSection :: FamilySlot -> String
slotSection slot = case slot of
  FamilyRelease -> "family_release"
  HelperRelease -> "helper_release"
  HelperBlock -> "helper_block"
  HelperBlockTimed -> "helper_block_timed"
  HelperCheck -> "helper_check"
  FlagCreate -> "family_flag_create"
  FlagDelete -> "family_flag_delete"
  FlagSet -> "family_flag_set"
  FlagSetHandler -> "family_flag_set_handler"
  FlagClear -> "family_flag_clear"
  FlagWait -> "family_flag_wait"
  FlagPoll -> "family_flag_poll"
  FlagTimedWait -> "family_flag_timed_wait"
  MailboxCreate -> "family_mailbox_create"
  MailboxDelete -> "family_mailbox_delete"
  MailboxSend -> "family_mailbox_send"
  MailboxReceive -> "family_mailbox_receive"
  MailboxPoll -> "family_mailbox_poll"
  MailboxTimedReceive -> "family_mailbox_timed_receive"

entryName :: FamilySlot -> String
entryName slot = case slot of
  FamilyRelease -> "xm1_rtos_objects_release"
  HelperRelease -> "xm1_rtos_objects_wait_release"
  HelperBlock -> "xm1_rtos_objects_wait_block"
  HelperBlockTimed -> "xm1_rtos_objects_wait_block_timed"
  HelperCheck -> "xm1_rtos_objects_check"
  _ -> "xm1_rtos_objects_" ++ slotSection slot

data FamilyImage = FamilyImage {imageBin :: B.ByteString, imageSymbols :: B.ByteString} deriving (Eq, Show)

data FamilyRow = FamilyRow
  { rowSlot :: FamilySlot
  , rowExtent :: Extent
  , rowEntry :: SymbolName
  , rowBody :: SymbolName
  , rowShared :: SymbolName
  , rowFrame :: FrameBytes
  , rowConstant :: Maybe (Extent, [Word32])
  }
  deriving (Eq, Show)

data FamilyError
  = ImageUnpinned Domain
  | BinRejected Domain
  | WindowRejected Domain FamilySlot
  | WindowOverlap Domain FamilySlot FamilySlot
  | BaseOverlap Domain FamilySlot
  | DependencyChanged Domain Group
  | DependencySymbolsChanged Domain Group
  | EntryRejected Domain FamilySlot
  | MarkerRejected Domain FamilySlot
  | FrameRejected Domain FamilySlot FrameBytes FrameBytes
  | SlackRejected Domain FamilySlot GuestAddress
  | SymbolRejected Domain SymbolName
  | UnaccountedByte Domain GuestAddress
  | ContainerRejected String
  | FamilyChangeOutside Int
  | RestoreMismatch
  | ControlRejected String
  deriving (Eq, Show)

data SymbolKind = GlobalText | LocalText | AbsoluteGlobal | AbsoluteLocal | ReadOnlyText
  deriving (Eq, Ord, Show)

kindOf :: Char -> Maybe SymbolKind
kindOf kind = case kind of
  'T' -> Just GlobalText
  't' -> Just LocalText
  'A' -> Just AbsoluteGlobal
  'a' -> Just AbsoluteLocal
  'R' -> Just ReadOnlyText
  _ -> Nothing

kindChar :: SymbolKind -> Char
kindChar kind = case kind of
  GlobalText -> 'T'
  LocalText -> 't'
  AbsoluteGlobal -> 'A'
  AbsoluteLocal -> 'a'
  ReadOnlyText -> 'R'

isAbsoluteKind :: SymbolKind -> Bool
isAbsoluteKind kind = kind == AbsoluteGlobal || kind == AbsoluteLocal

data FamilySymbol = FamilySymbol
  { symbolAddress :: GuestAddress
  , symbolSize :: Word32
  , symbolKind :: SymbolKind
  , symbolName :: SymbolName
  }
  deriving (Eq, Ord, Show)

parseFamilySymbols :: Domain -> B.ByteString -> Either FamilyError [FamilySymbol]
parseFamilySymbols domain bytes = mapM parseLine (filter (not . C.null) (C.lines bytes))
  where
    parseLine line = case C.words line of
      [address, size, kind, name] -> do
        at <- parseWord32 address
        bytesAt <- parseWord32 size
        parsed <- maybe (Left (SymbolRejected domain (SymbolName (C.unpack name)))) Right (kindOf (C.head kind))
        pure (FamilySymbol (GuestAddress at) bytesAt parsed (SymbolName (C.unpack name)))
      _ -> Left (SymbolRejected domain (SymbolName (C.unpack line)))
    parseWord32 text = case readHex (C.unpack text) of
      [(value, "")] | value <= toInteger (maxBound :: Word32) -> Right (fromInteger value)
      _ -> Left (SymbolRejected domain (SymbolName (C.unpack text)))

renderFamilySymbols :: [FamilySymbol] -> B.ByteString
renderFamilySymbols symbols = C.unlines [C.pack (pad (let GuestAddress at = symbolAddress symbol in at) ++ " " ++ pad (symbolSize symbol) ++ " " ++ [kindChar (symbolKind symbol)] ++ " " ++ name (symbolName symbol)) | symbol <- symbols]
  where
    pad at = replicate (8 - length (showHex at "")) '0' ++ showHex at ""
    name (SymbolName text) = text

data Dependency = Dependency
  { dependencyGroup :: Group
  , dependencyExtent :: Extent
  , dependencyBin :: B.ByteString
  , dependencySymbols :: [FamilySymbol]
  }

data DomainAdmission = DomainAdmission
  { domainRows :: [FamilyRow]
  , domainDependencies :: [Dependency]
  , domainBinStart :: GuestAddress
  , domainOwnedLoader :: B.ByteString
  }

data FamilyAdmission = FamilyAdmission
  { familySource :: S.Source
  , familyBaseAdmission :: CutoverAdmission
  , familyBaseStaged :: CutoverStaged
  , familyImages :: M.Map Domain FamilyImage
  , familyDependencies :: M.Map Domain [Dependency]
  , familyRowsAdmitted :: M.Map Domain [FamilyRow]
  , familyBinStarts :: M.Map Domain GuestAddress
  , familyOwnedLoaders :: M.Map Domain B.ByteString
  }

data FamilyStaged = FamilyStaged
  { stagedFamily :: FamilyAdmission
  , familyDat :: B.ByteString
  , familyLoaders :: M.Map Domain B.ByteString
  }

data FamilyControl = FamilyControl
  { controlName :: String
  , controlExpected :: String
  , controlObserved :: Either FamilyError ()
  , controlPassed :: Bool
  }
  deriving (Eq, Show)

require :: Bool -> FamilyError -> Either FamilyError ()
require ok err = unless ok (Left err)

sha :: B.ByteString -> Digest
sha bytes = Digest (SHA.sha256Hex (SHA.sha256 bytes))

slice :: GuestAddress -> Extent -> B.ByteString -> B.ByteString
slice (GuestAddress base) extent@(Extent (GuestAddress start) _) = B.take (extentBytes extent) . B.drop (fromIntegral (start - base))

put :: GuestAddress -> Extent -> B.ByteString -> B.ByteString -> B.ByteString
put (GuestAddress base) extent@(Extent (GuestAddress start) _) replacement bytes = let offset = fromIntegral (start - base) in B.take offset bytes <> replacement <> B.drop (offset + extentBytes extent) bytes

row :: FamilySlot -> Word32 -> Word32 -> Int -> Maybe (Word32, [Word32]) -> FamilyRow
row slot start len frame constant = FamilyRow slot (Extent (GuestAddress start) (GuestAddress (start + len))) (SymbolName (entryName slot)) (SymbolName ("__" ++ slotSection slot ++ "_body_end")) (SymbolName ("__" ++ slotSection slot ++ "_shared_end")) (FrameBytes frame) (fmap constantExtent constant)
  where
    constantExtent (at, wordsAt) = (Extent (GuestAddress at) (GuestAddress (at + 4 * fromIntegral (length wordsAt))), wordsAt)

-- | The nineteen family-owned windows, read independently from each domain's linker script.
familyRows :: Domain -> [FamilyRow]
familyRows domain = case domain of
  DomainA ->
    [ row HelperRelease 0x44744 0xbc 8 Nothing
    , row HelperBlock 0x44800 0xe4 8 Nothing
    , row HelperBlockTimed 0x448e4 0xdc 24 (Just (0x44970, [0xffffffe7, 0xffffffee, 0xffffffee, 0xffffffd7]))
    , row HelperCheck 0x480cc 0x70 8 Nothing
    , row FamilyRelease 0x44568 0xcc 8 Nothing
    , row FlagCreate 0x4d39c 0x98 24 Nothing
    , row FlagDelete 0x4d434 0x68 16 Nothing
    , row FlagSet 0x4813c 0x260 8 Nothing
    , row FlagSetHandler 0x4839c 0x224 8 Nothing
    , row FlagClear 0x485c0 0x138 8 Nothing
    , row FlagWait 0x486f8 0x2b4 8 Nothing
    , row FlagPoll 0x489ac 0x184 8 Nothing
    , row FlagTimedWait 0x48b30 0x308 8 Nothing
    , row MailboxCreate 0x4d5bc 0x98 24 Nothing
    , row MailboxDelete 0x4d654 0x70 16 Nothing
    , row MailboxSend 0x4aa44 0x304 8 Nothing
    , row MailboxReceive 0x4ad48 0x254 8 Nothing
    , row MailboxPoll 0x4af9c 0x148 8 Nothing
    , row MailboxTimedReceive 0x4b0e4 0x2a8 8 Nothing
    ]
  DomainB ->
    [ row HelperRelease 0xd4704 0xbc 8 Nothing
    , row HelperBlock 0xd47c0 0xe4 8 Nothing
    , row HelperBlockTimed 0xd48a4 0xdc 16 (Just (0xd4930, [0xffffffe7, 0xffffffee, 0xffffffee, 0xffffffd7]))
    , row HelperCheck 0xd808c 0x70 8 Nothing
    , row FamilyRelease 0xd4528 0xcc 8 Nothing
    , row FlagCreate 0xdd360 0x98 24 Nothing
    , row FlagDelete 0xdd3f8 0x68 16 Nothing
    , row FlagSet 0xd80fc 0x260 8 Nothing
    , row FlagSetHandler 0xd835c 0x224 8 Nothing
    , row FlagClear 0xd8580 0x138 8 Nothing
    , row FlagWait 0xd86b8 0x2b4 8 Nothing
    , row FlagPoll 0xd896c 0x184 8 Nothing
    , row FlagTimedWait 0xd8af0 0x308 8 Nothing
    , row MailboxCreate 0xdd580 0x98 24 Nothing
    , row MailboxDelete 0xdd618 0x70 16 Nothing
    , row MailboxSend 0xdaa04 0x304 8 Nothing
    , row MailboxReceive 0xdad08 0x254 8 Nothing
    , row MailboxPoll 0xdaf5c 0x148 8 Nothing
    , row MailboxTimedReceive 0xdb0a4 0x2a8 8 Nothing
    ]

-- | Stock addresses the family image declares absolute, from its linker script.
familyAbsolutes :: Domain -> [(SymbolName, GuestAddress)]
familyAbsolutes domain = case domain of
  DomainA ->
    [ (SymbolName "xm1_rtos_objects_stock_timeout", GuestAddress 0x44318)
    , (SymbolName "xm1_rtos_objects_stock_insert_timer", GuestAddress 0x44c60)
    , (SymbolName "xm1_rtos_objects_stock_remove_timer", GuestAddress 0x44ca8)
    ]
  DomainB ->
    [ (SymbolName "xm1_rtos_objects_stock_timeout", GuestAddress 0xd42d8)
    , (SymbolName "xm1_rtos_objects_stock_insert_timer", GuestAddress 0xd4c20)
    , (SymbolName "xm1_rtos_objects_stock_remove_timer", GuestAddress 0xd4c68)
    ]

familyPins :: Domain -> (Digest, Digest)
familyPins domain = case domain of
  DomainA ->
    ( Digest "f9340cfd4747b5e0539693f18cb4c4632f30d626229570222071049e67cbf97c"
    , Digest "a37f9469f4c33396a7352e6ae30053ebbd79ceaafb24c5fda083845fc0c33ba7"
    )
  DomainB ->
    ( Digest "ada63e05753ab61903fa0f4469e402729a21c0ff6175c89c3291b5d8bfb97c6a"
    , Digest "830b2ae11ddfc1d34b474b452cf3097c3db2a0318f70ccb4f10fd72f5850fc45"
    )

disjointWindows :: Domain -> [(FamilySlot, Extent)] -> Either FamilyError ()
disjointWindows domain windows = forM_ (zip ordered (drop 1 ordered)) $ \((leftSlot, left), (rightSlot, right)) ->
  require (extentEnd left <= extentStart right) (WindowOverlap domain leftSlot rightSlot)
  where ordered = sortOn (extentStart . snd) windows

baseDisjoint :: Domain -> [(FamilySlot, Extent)] -> [Extent] -> Either FamilyError ()
baseDisjoint domain windows base = forM_ windows $ \(slot, extent) ->
  forM_ base $ \other -> require (extentEnd extent <= extentStart other || extentEnd other <= extentStart extent) (BaseOverlap domain slot)

dependency :: Domain -> Group -> [GroupInput] -> Either FamilyError Dependency
dependency domain group inputs = do
  input <- case [candidate | candidate <- inputs, inputGroup candidate == group] of
    [candidate] -> Right candidate
    _ -> Left (ControlRejected ("dependency group " ++ groupLabel group ++ " is absent"))
  let linked = case domain of DomainA -> inputA input; DomainB -> inputB input
  symbols <- parseFamilySymbols domain (S.linkedSymbols linked)
  let located = [symbol | symbol <- symbols, not (isAbsoluteKind (symbolKind symbol))]
      bin = S.linkedBin linked
  GuestAddress start <- case located of
    [] -> Left (ControlRejected ("dependency group " ++ groupLabel group ++ " has no located symbol"))
    _ -> Right (minimum (map symbolAddress located))
  pure (Dependency group (Extent (GuestAddress start) (GuestAddress (start + fromIntegral (B.length bin)))) bin symbols)

firstStackFrame :: B.ByteString -> GuestAddress -> GuestAddress -> GuestAddress -> FrameBytes
firstStackFrame bin (GuestAddress base) (GuestAddress start) (GuestAddress stop) = go start
  where
    go pc
      | pc >= stop = FrameBytes 0
      | otherwise = case A.decode pc (U.word32At bin (fromIntegral (pc - base))) of
          A.StoreMultiple _ register A.DecrementBefore True registers | register == A.sp -> FrameBytes (4 * length registers)
          A.DataProcessing _ A.SUB _ destination source (A.Immediate amount) | destination == A.sp && source == A.sp -> FrameBytes (fromIntegral amount)
          _ -> go (pc + 4)

admitRow :: Domain -> B.ByteString -> GuestAddress -> [FamilySymbol] -> FamilyRow -> Either FamilyError ()
admitRow domain bin binStart symbols admitted = do
  let Extent (GuestAddress start) (GuestAddress end) = rowExtent admitted
      slot = rowSlot admitted
      address name = case [symbolAddress symbol | symbol <- symbols, symbolName symbol == name] of
        [at] -> Right at
        _ -> Left (MarkerRejected domain slot)
      GuestAddress base = binStart
  at <- case [symbolAddress symbol | symbol <- symbols, symbolName symbol == rowEntry admitted] of
    [addressAt] -> Right addressAt
    _ -> Left (EntryRejected domain slot)
  require (at == GuestAddress start) (EntryRejected domain slot)
  body <- address (rowBody admitted)
  shared <- address (rowShared admitted)
  require (GuestAddress start < body && body <= GuestAddress end) (MarkerRejected domain slot)
  require (body <= shared && shared <= GuestAddress end) (MarkerRejected domain slot)
  let GuestAddress bodyAt = body
      actual = firstStackFrame bin binStart (GuestAddress start) (GuestAddress bodyAt)
  require (actual == rowFrame admitted) (FrameRejected domain slot (rowFrame admitted) actual)
  forM_ (rowConstant admitted) $ \(constant, wordsAt) -> do
    let Extent (GuestAddress constantStart) (GuestAddress constantEnd) = constant
    require (constantStart >= start && constantEnd <= end && constantStart `mod` 4 == 0 && constantEnd - constantStart == 4 * fromIntegral (length wordsAt)) (SlackRejected domain slot (GuestAddress constantStart))
    forM_ (zip [constantStart, constantStart + 4 .. constantEnd - 4] wordsAt) $ \(wordAt, expected) ->
      require (U.word32At bin (fromIntegral (wordAt - base)) == expected) (SlackRejected domain slot (GuestAddress wordAt))
  forM_ [start, start + 4 .. end - 4] $ \wordAt -> do
    let word = U.word32At bin (fromIntegral (wordAt - base))
        covered = any (\symbol -> symbolSize symbol > 0 && symbolAddress symbol <= GuestAddress wordAt && toInteger wordAt + 4 <= toInteger (let GuestAddress symbolAt = symbolAddress symbol in symbolAt) + toInteger (symbolSize symbol)) symbols
        inConstant = case rowConstant admitted of
          Just (Extent (GuestAddress constantStart) (GuestAddress constantEnd), _) -> wordAt >= constantStart && wordAt + 4 <= constantEnd
          Nothing -> False
    require (covered || word == 0xe7f000f0 || inConstant) (SlackRejected domain slot (GuestAddress wordAt))

admitDomain :: M.Map Domain [FamilyRow] -> Domain -> CutoverAdmission -> CutoverStaged -> [GroupInput] -> M.Map Domain FamilyImage -> Either FamilyError DomainAdmission
admitDomain rowsByDomain domain base staged inputs images = do
  image <- case M.lookup domain images of
    Just image -> Right image
    Nothing -> Left (BinRejected domain)
  symbols <- parseFamilySymbols domain (imageSymbols image)
  let rows = rowsByDomain M.! domain
      bin = imageBin image
      baseExtents = [extent | (extentDomain, extent) <- admittedExtents base, extentDomain == domain]
      GuestAddress loaderBase = domainLoaderBase domain
  forM_ rows $ \admitted -> do
    let Extent (GuestAddress start) (GuestAddress end) = rowExtent admitted
    require (start < end && start >= loaderBase && end <= loaderBase + fromIntegral (domainLoaderSize domain) && start `mod` 4 == 0 && end `mod` 4 == 0) (WindowRejected domain (rowSlot admitted))
  disjointWindows domain [(rowSlot admitted, rowExtent admitted) | admitted <- rows]
  baseDisjoint domain [(rowSlot admitted, rowExtent admitted) | admitted <- rows] baseExtents
  dependencies <- forM [S1, S2] $ \group -> dependency domain group inputs
  let starts = map (extentStart . dependencyExtent) dependencies ++ map (extentStart . rowExtent) rows
      ends = map (extentEnd . dependencyExtent) dependencies ++ map (extentEnd . rowExtent) rows
      binStart = minimum starts
      binEnd = maximum ends
      s2 = [extentStart (dependencyExtent item) | item <- dependencies, dependencyGroup item == S2]
  require (s2 == [binStart]) (BinRejected domain)
  require (B.length bin == fromIntegral (let GuestAddress low = binStart; GuestAddress high = binEnd in high - low)) (BinRejected domain)
  forM_ dependencies $ \item -> do
    let dependencySpan = dependencyExtent item
        embedded = B.take (extentBytes dependencySpan) (B.drop (fromIntegral (let GuestAddress low = binStart; GuestAddress at = extentStart dependencySpan in at - low)) bin)
        located = [symbol | symbol <- symbols, not (isAbsoluteKind (symbolKind symbol)), extentContains dependencySpan (symbolAddress symbol)]
        qualified = [symbol | symbol <- dependencySymbols item, not (isAbsoluteKind (symbolKind symbol)), extentContains dependencySpan (symbolAddress symbol)]
    require (embedded == dependencyBin item) (DependencyChanged domain (dependencyGroup item))
    require (sortOn symbolKey located == sortOn symbolKey qualified) (DependencySymbolsChanged domain (dependencyGroup item))
  forM_ rows (admitRow domain bin binStart symbols)
  forM_ symbols $ \symbol -> unless (isAbsoluteKind (symbolKind symbol)) $ do
    let at = symbolAddress symbol
        size = toInteger (symbolSize symbol)
        inside extent = at >= extentStart extent && if size == 0 then at <= extentEnd extent else toInteger (let GuestAddress start = at in start) + size <= toInteger (let GuestAddress end = extentEnd extent in end)
    require (any inside (map rowExtent rows ++ map dependencyExtent dependencies)) (SymbolRejected domain (symbolName symbol))
  let allowed = [(symbolName symbol, symbolAddress symbol) | item <- dependencies, symbol <- dependencySymbols item, isAbsoluteKind (symbolKind symbol)] ++ familyAbsolutes domain
  forM_ symbols $ \symbol -> when (isAbsoluteKind (symbolKind symbol)) $
    require ((symbolName symbol, symbolAddress symbol) `elem` allowed) (SymbolRejected domain (symbolName symbol))
  let coveredAt at = any (\admitted -> extentContains (rowExtent admitted) at) rows || any (\item -> extentContains (dependencyExtent item) at) dependencies
  forM_ [0 .. B.length bin - 1] $ \offset -> do
    let at = GuestAddress (let GuestAddress low = binStart in low + fromIntegral offset)
    unless (coveredAt at) (require (B.index bin offset == 0) (UnaccountedByte domain at))
  let (pinnedBin, pinnedSymbols) = familyPins domain
  require (sha bin == pinnedBin && sha (imageSymbols image) == pinnedSymbols) (ImageUnpinned domain)
  let owned = foldl (\bytes admitted -> put (domainLoaderBase domain) (rowExtent admitted) (slice binStart (rowExtent admitted) bin) bytes) (cutoverLoader staged domain) rows
  pure (DomainAdmission rows dependencies binStart owned)

symbolKey :: FamilySymbol -> (GuestAddress, Word32, SymbolKind, SymbolName)
symbolKey symbol = (symbolAddress symbol, symbolSize symbol, symbolKind symbol, symbolName symbol)

admitFamily :: S.Source -> CutoverAdmission -> CutoverStaged -> [GroupInput] -> M.Map Domain FamilyImage -> Either FamilyError FamilyAdmission
admitFamily source base staged inputs images = admitFamilyRows (M.fromList [(domain, familyRows domain) | domain <- domains]) source base staged inputs images

admitFamilyRows :: M.Map Domain [FamilyRow] -> S.Source -> CutoverAdmission -> CutoverStaged -> [GroupInput] -> M.Map Domain FamilyImage -> Either FamilyError FamilyAdmission
admitFamilyRows rowsByDomain source base staged inputs images = do
  admitted <- forM domains $ \domain -> fmap (\item -> (domain, item)) (admitDomain rowsByDomain domain base staged inputs images)
  pure FamilyAdmission
    { familySource = source
    , familyBaseAdmission = base
    , familyBaseStaged = staged
    , familyImages = images
    , familyDependencies = M.fromList [(domain, domainDependencies item) | (domain, item) <- admitted]
    , familyRowsAdmitted = M.fromList [(domain, domainRows item) | (domain, item) <- admitted]
    , familyBinStarts = M.fromList [(domain, domainBinStart item) | (domain, item) <- admitted]
    , familyOwnedLoaders = M.fromList [(domain, domainOwnedLoader item) | (domain, item) <- admitted]
    }

patchesBetween :: FamilyAdmission -> M.Map Domain B.ByteString -> M.Map Domain B.ByteString -> Either FamilyError [F.LoaderWordPatch]
patchesBetween admitted before after = fmap concat $ forM domains $ \domain -> do
  let GuestAddress base = domainLoaderBase domain
  either (Left . ContainerRejected) Right $ sequence
    [ F.loaderWordPatch (F.LoaderId (domainLoaderId domain)) at old new
    | admittedRow <- familyRowsAdmitted admitted M.! domain
    , let Extent (GuestAddress low) (GuestAddress high) = rowExtent admittedRow
    , at <- [low, low + 4 .. high - 4]
    , let offset = fromIntegral (at - base)
    , let old = U.word32At (before M.! domain) offset
    , let new = U.word32At (after M.! domain) offset
    , old /= new
    ]

confineFamily :: FamilyAdmission -> B.ByteString -> Either FamilyError ()
confineFamily admitted dat = do
  let baseDat = cutoverDat (familyBaseStaged admitted)
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer baseDat)
  let mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  ranges <- fmap concat $ forM domains $ \domain -> do
    segment <- case [candidate | candidate <- U.partSegments (U.parsedParts parsed), U.segId candidate == domainLoaderId domain] of
      [candidate] -> Right candidate
      _ -> Left (ContainerRejected "loader segment set")
    pure [(mainStart + fromIntegral (U.segFlash segment) + fromIntegral (let GuestAddress low = extentStart (rowExtent admittedRow) in low - U.segLoad segment), extentBytes (rowExtent admittedRow)) | admittedRow <- familyRowsAdmitted admitted M.! domain]
  require (B.length dat == B.length baseDat) (FamilyChangeOutside (min (B.length dat) (B.length baseDat)))
  let allowed = (U.mainChecksumOffset, 4) : ranges
  forM_ (find (\offset -> not (any (\(low, len) -> offset >= low && offset < low + len) allowed)) [offset | (offset, True) <- zip [0 ..] (B.zipWith (/=) baseDat dat)]) (Left . FamilyChangeOutside)

restoreFamily :: FamilyAdmission -> B.ByteString -> Either FamilyError B.ByteString
restoreFamily admitted dat = do
  (parsed, current) <- either (Left . ContainerRejected . show) Right (loadersFrom dat)
  let base = M.fromList [(domain, cutoverLoader (familyBaseStaged admitted) domain) | domain <- domains]
  wordsToPatch <- patchesBetween admitted current base
  either (Left . ContainerRejected . show) Right (rebuild parsed wordsToPatch)

stageFamily :: FamilyAdmission -> Either FamilyError FamilyStaged
stageFamily admitted = do
  let baseDat = cutoverDat (familyBaseStaged admitted)
      base = M.fromList [(domain, cutoverLoader (familyBaseStaged admitted) domain) | domain <- domains]
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer baseDat)
  wordsToPatch <- patchesBetween admitted base (familyOwnedLoaders admitted)
  dat <- either (Left . ContainerRejected . show) Right (rebuild parsed wordsToPatch)
  confineFamily admitted dat
  (_, actual) <- either (Left . ContainerRejected . show) Right (loadersFrom dat)
  require (actual == familyOwnedLoaders admitted) (ContainerRejected "staged loaders differ")
  restored <- restoreFamily admitted dat
  require (restored == baseDat) RestoreMismatch
  require (dat /= baseDat) (ContainerRejected "family staging changed no bytes")
  pure (FamilyStaged admitted dat (familyOwnedLoaders admitted))

familyLoader :: FamilyStaged -> Domain -> B.ByteString
familyLoader staged domain = familyLoaders staged M.! domain

familyContract :: FamilyStaged -> Value
familyContract staged = object
  [ "schema" .= (1 :: Int)
  , "piece_kind" .= ("rtos-object-family" :: String)
  , "scope" .= ("the 14 flag/mailbox family entries and 5 shared helper carriers staged over the qualified scheduler candidate; the embedded S1/S2 dependency spans are byte-identical to the qualified inputs and are not staged as a group" :: String)
  , "frame_scope" .= ("frame_bytes is each window entry's first stack adjustment in the family image, the pinned-local basis S2/S3 also use; it is not composed demand" :: String)
  , "source_dat_sha256" .= digestText (sha (S.sourceDat source))
  , "base_candidate_dat_sha256" .= digestText (sha (cutoverDat base))
  , "candidate_dat_sha256" .= digestText (sha (familyDat staged))
  , "base_restores_to_source" .= (restoreCutover (familyBaseAdmission admitted) (cutoverDat base) == Right (S.sourceDat source))
  , "family_restores_to_base" .= (restoreFamily admitted (familyDat staged) == Right (cutoverDat base))
  , "images" .= [object
      [ "domain" .= domainLabel domain
      , "bin_bytes" .= B.length (imageBin image)
      , "bin_start" .= address (familyBinStarts admitted M.! domain)
      , "bin_sha256" .= digestText (sha (imageBin image))
      , "symbols_sha256" .= digestText (sha (imageSymbols image))
      , "pinned_bin_sha256" .= digestText (fst (familyPins domain))
      , "pinned_symbols_sha256" .= digestText (snd (familyPins domain))
      ] | domain <- domains, let image = familyImages admitted M.! domain]
  , "dependencies" .= [object
      [ "group" .= groupLabel (dependencyGroup item)
      , "domain" .= domainLabel domain
      , "start" .= address (extentStart (dependencyExtent item))
      , "end" .= address (extentEnd (dependencyExtent item))
      , "embedded_sha256" .= digestText (sha (dependencyBin item))
      , "located_symbols" .= length [symbol | symbol <- dependencySymbols item, not (isAbsoluteKind (symbolKind symbol)), extentContains (dependencyExtent item) (symbolAddress symbol)]
      ] | domain <- domains, item <- familyDependencies admitted M.! domain]
  , "domains" .= [object
      [ "domain" .= domainLabel domain
      , "windows" .= [windowValue domain admittedRow | admittedRow <- familyRowsAdmitted admitted M.! domain]
      , "entry_table" .= [object ["slot" .= slotSection (rowSlot admittedRow), "entry" .= address (extentStart (rowExtent admittedRow)), "entry_symbol" .= symbolText (rowEntry admittedRow)] | admittedRow <- familyRowsAdmitted admitted M.! domain]
      ] | domain <- domains]
  ]
  where
    admitted = stagedFamily staged
    base = familyBaseStaged admitted
    source = familySource admitted
    address (GuestAddress at) = hexText at
    symbolText (SymbolName text) = text
    digestText (Digest text) = text
    windowValue domain admittedRow = object
      [ "slot" .= slotSection (rowSlot admittedRow)
      , "class" .= (case slotClass (rowSlot admittedRow) of CarrierSlot -> "helper" :: String; EntrySlot -> "entry")
      , "start" .= address (extentStart (rowExtent admittedRow))
      , "end" .= address (extentEnd (rowExtent admittedRow))
      , "entry_symbol" .= symbolText (rowEntry admittedRow)
      , "body_symbol" .= symbolText (rowBody admittedRow)
      , "shared_symbol" .= symbolText (rowShared admittedRow)
      , "frame_bytes" .= (let FrameBytes count = rowFrame admittedRow in count)
      , "constant" .= fmap (\(extent, wordsAt) -> object ["start" .= address (extentStart extent), "end" .= address (extentEnd extent), "words" .= wordsAt]) (rowConstant admittedRow)
      , "stock_sha256" .= digestText (sha (slice (domainLoaderBase domain) (rowExtent admittedRow) (S.sourceLoaders source M.! domain)))
      , "base_sha256" .= digestText (sha (slice (domainLoaderBase domain) (rowExtent admittedRow) (cutoverLoader base domain)))
      , "owned_sha256" .= digestText (sha (slice (domainLoaderBase domain) (rowExtent admittedRow) (familyLoaders staged M.! domain)))
      ]

-- | The semantic name the scheduler-pair catalog carries for a family slot.
familyServiceName :: FamilySlot -> String
familyServiceName slot = case slot of
  FamilyRelease -> "family-release"
  HelperRelease -> "helper-release"
  HelperBlock -> "helper-block"
  HelperBlockTimed -> "helper-block-timed"
  HelperCheck -> "helper-check"
  FlagCreate -> "family-flag-create"
  FlagDelete -> "family-flag-delete"
  FlagSet -> "family-flag-set"
  FlagSetHandler -> "family-flag-set-handler"
  FlagClear -> "family-flag-clear"
  FlagWait -> "family-flag-wait"
  FlagPoll -> "family-flag-poll"
  FlagTimedWait -> "family-flag-timed-wait"
  MailboxCreate -> "family-mailbox-create"
  MailboxDelete -> "family-mailbox-delete"
  MailboxSend -> "family-mailbox-send"
  MailboxReceive -> "family-mailbox-receive"
  MailboxPoll -> "family-mailbox-poll"
  MailboxTimedReceive -> "family-mailbox-timed-receive"

-- | The admitted nineteen windows per domain in scheduler-catalog shape.
familyCatalog :: FamilyStaged -> Value
familyCatalog staged = object
  [ "fill_word" .= ("0xe7f000f0" :: String)
  , "domains" .= object
      [ fromString (domainLabel domain) .= object
          [ "regions" .= [region admittedRow | admittedRow <- familyRowsAdmitted admitted M.! domain]
          ]
      | domain <- domains
      ]
  ]
  where
    admitted = stagedFamily staged
    address (GuestAddress at) = "0x" ++ showHex at ""
    symbol (SymbolName text) = text
    region admittedRow = object
      [ "start" .= address (extentStart (rowExtent admittedRow))
      , "end" .= address (extentEnd (rowExtent admittedRow))
      , "symbol" .= symbol (rowEntry admittedRow)
      , "frame_bytes" .= (let FrameBytes count = rowFrame admittedRow in count)
      , "entries" .= [object
          [ "entry" .= address (extentStart (rowExtent admittedRow))
          , "semantic" .= object ["kind" .= ("family" :: String), "name" .= familyServiceName (rowSlot admittedRow)]
          ]]
      ]

replaceAt :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replaceAt at value bytes = B.take at bytes <> value <> B.drop (at + B.length value) bytes

control :: String -> String -> Bool -> Either FamilyError () -> FamilyControl
control label expected passed observed = FamilyControl label expected observed passed

refuses :: String -> FamilyError -> Either FamilyError a -> FamilyControl
refuses label expected result = control label (show expected) (case result of Left err -> err == expected; Right _ -> False) (() <$ result)

accepts :: String -> Either FamilyError a -> FamilyControl
accepts label result = control label "accepted" (case result of Right _ -> True; Left _ -> False) (() <$ result)

mutateImage :: Domain -> (FamilyImage -> FamilyImage) -> M.Map Domain FamilyImage -> M.Map Domain FamilyImage
mutateImage domain change = M.adjust change domain

mutateSymbols :: Domain -> (FamilySymbol -> Maybe FamilySymbol) -> FamilyImage -> FamilyImage
mutateSymbols domain change image = case parseFamilySymbols domain (imageSymbols image) of
  Left _ -> image
  Right symbols -> image {imageSymbols = renderFamilySymbols (map (\symbol -> maybe symbol id (change symbol)) symbols)}

familyControls :: S.Source -> CutoverAdmission -> CutoverStaged -> [GroupInput] -> M.Map Domain FamilyImage -> FamilyStaged -> [FamilyControl]
familyControls source base staged inputs images familyStaged =
  [ refuses "carrier-byte" (ImageUnpinned DomainA) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (\image -> image {imageBin = flipAt 0x3a6c (imageBin image)}) images))
  , refuses "dependency-byte" (DependencyChanged DomainA S1) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (\image -> image {imageBin = flipAt s1Offset (imageBin image)}) images))
  , refuses "dependency-symbols" (DependencySymbolsChanged DomainA S2) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (mutateSymbols DomainA renameDoorbell) images))
  , refuses "entry" (EntryRejected DomainA FlagSet) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (mutateSymbols DomainA moveFlagSetEntry) images))
  , refuses "marker" (MarkerRejected DomainA FlagSet) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (mutateSymbols DomainA moveFlagSetBody) images))
  , refuses "outside-window-byte" (UnaccountedByte DomainA (GuestAddress 0x43810)) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (\image -> image {imageBin = replaceAt 0x2d10 (B.singleton 1) (imageBin image)}) images))
  , refuses "bin-length" (BinRejected DomainA) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (\image -> image {imageBin = imageBin image <> B.singleton 0}) images))
  , refuses "absolute-symbol" (SymbolRejected DomainA (SymbolName "xm1_rtos_objects_stock_insert_timer")) (admitFamilyRows rows source base staged inputs (mutateImage DomainA (mutateSymbols DomainA moveStockTimer) images))
  , refuses "frame" (FrameRejected DomainA FlagSet (FrameBytes 0) (FrameBytes 8)) (admitFamilyRows (M.adjust (map (\admittedRow -> if rowSlot admittedRow == FlagSet then admittedRow {rowFrame = FrameBytes 0} else admittedRow)) DomainA rows) source base staged inputs images)
  , refuses "base-overlap" (BaseOverlap DomainA FamilyRelease) (admitFamilyRows (M.adjust (map moveFamilyRelease) DomainA rows) source base staged inputs images)
  , refuses "window-overlap" (WindowOverlap DomainA FlagSet FlagDelete) (disjointWindows DomainA [(FlagSet, Extent (GuestAddress 0x4813c) (GuestAddress 0x48144)), (FlagDelete, Extent (GuestAddress 0x48140) (GuestAddress 0x48148))])
  , refuses "outside-write" (FamilyChangeOutside 0) (confineFamily (stagedFamily familyStaged) (replaceAt 0 (B.singleton (B.index (cutoverDat staged) 0 `xor` 0xff)) (cutoverDat staged)))
  , accepts "family-restores-to-base" (restoreFamily (stagedFamily familyStaged) (familyDat familyStaged) >>= \restored -> require (restored == cutoverDat staged) RestoreMismatch)
  , accepts "candidate-differs" (require (familyDat familyStaged /= cutoverDat staged) (ContainerRejected "family staging changed no bytes"))
  ]
  where
    rows = M.fromList [(domain, familyRows domain) | domain <- domains]
    s1 = case [item | item <- familyDependencies (stagedFamily familyStaged) M.! DomainA, dependencyGroup item == S1] of
      [item] -> item
      _ -> error "family admission lacks an S1 dependency"
    GuestAddress binStart = familyBinStarts (stagedFamily familyStaged) M.! DomainA
    GuestAddress s1Start = extentStart (dependencyExtent s1)
    s1Offset = fromIntegral (s1Start - binStart) + 8
    flipAt offset bytes = B.take offset bytes <> B.singleton (B.index bytes offset `xor` 0xff) <> B.drop (offset + 1) bytes
    renameDoorbell symbol
      | symbolName symbol == SymbolName "xm1_rtos_scheduler_doorbell" = Just symbol {symbolName = SymbolName "xm1_rtos_scheduler_doorbell_renamed"}
      | otherwise = Nothing
    moveFlagSetEntry symbol
      | symbolName symbol == SymbolName "xm1_rtos_objects_family_flag_set" = Just symbol {symbolAddress = GuestAddress 0x48140}
      | otherwise = Nothing
    moveFlagSetBody symbol
      | symbolName symbol == SymbolName "__family_flag_set_body_end" = Just symbol {symbolAddress = GuestAddress 0x483a0}
      | otherwise = Nothing
    moveStockTimer symbol
      | symbolName symbol == SymbolName "xm1_rtos_objects_stock_insert_timer" = Just symbol {symbolAddress = GuestAddress 0x44c64}
      | otherwise = Nothing
    moveFamilyRelease admittedRow
      | rowSlot admittedRow == FamilyRelease = admittedRow {rowExtent = Extent (GuestAddress 0x439d8) (GuestAddress 0x43aa4)}
      | otherwise = admittedRow
