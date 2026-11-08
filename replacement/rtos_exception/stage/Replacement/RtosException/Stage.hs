{-# LANGUAGE OverloadedStrings #-}
module Replacement.RtosException.Stage
  ( DomainSpec (..)
  , ExtentSpec (..)
  , StageError (..)
  , Linked (..)
  , Source
  , Admitted
  , Staged
  , specs
  , canonicalSource
  , requirePinned
  , admit
  , stage
  , stagedDat
  , stagedLoader
  , contractValue
  , confineChanges
  , restore
  ) where

import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Bits (complement)
import Data.List (isPrefixOf)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.RtosOwnFirst.Regions (Domain (..), GuestAddress (..), domainLabel, domainLoaderBase, domainLoaderId, domainLoaderSize, domains)

data ExtentSpec = ExtentSpec { extentLow :: Word32, extentHigh :: Word32 } deriving (Eq, Show)

data DomainSpec = DomainSpec
  { specDomain :: Domain
  , specBase :: Word32
  , specLow :: Word32
  , specHigh :: Word32
  , specExtents :: [ExtentSpec]
  } deriving (Eq, Show)

specs :: [DomainSpec]
specs =
  [ DomainSpec DomainA 0x40000 0x40100 0x43834
      [ ExtentSpec 0x40100 0x40368
      , ExtentSpec 0x403a0 0x40410
      , ExtentSpec 0x40cac 0x40d50
      , ExtentSpec 0x40d50 0x40d98
      , ExtentSpec 0x430a8 0x432f8
      , ExtentSpec 0x43810 0x43834
      ]
  , DomainSpec DomainB 0xd0000 0xd0100 0xd3834
      [ ExtentSpec 0xd0100 0xd0368
      , ExtentSpec 0xd03a0 0xd0410
      , ExtentSpec 0xd0cac 0xd0d50
      , ExtentSpec 0xd0d50 0xd0d98
      , ExtentSpec 0xd30a8 0xd32f8
      , ExtentSpec 0xd3810 0xd3834
      ]
  ]

schedulerReturn :: DomainSpec -> (Word32, Word32, Word32)
schedulerReturn spec = case specDomain spec of
  DomainA -> (0x437d4, 0xe169f000, 0xe16ff000)
  DomainB -> (0xd37d4, 0xe169f000, 0xe16ff000)

data StageError
  = ContainerRejected String
  | LoaderMismatch Domain
  | SourceUnpinned
  | SymbolsRejected Domain
  | SymbolOutside Domain String
  | EntryMismatch Domain String
  | ByteOutside Domain Word32
  | PatchRejected String
  | ChangeOutside Int
  | RestoreMismatch
  deriving (Eq, Show)

data Linked = Linked { linkedBin :: B.ByteString, linkedSymbols :: B.ByteString } deriving (Eq, Show)

data Source = Source
  { sourceDat :: B.ByteString
  , sourceParsed :: U.Parsed
  , sourceLoaders :: M.Map Domain B.ByteString
  }

data Admitted = Admitted
  { admittedSource :: Source
  , admittedLoaders :: M.Map Domain B.ByteString
  , admittedLinked :: M.Map Domain Linked
  }

data Staged = Staged
  { stagedAdmission :: Admitted
  , stagedDat :: B.ByteString
  }

require :: Bool -> StageError -> Either StageError ()
require ok err = unless ok (Left err)

sha :: B.ByteString -> String
sha = SHA.sha256Hex . SHA.sha256

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

sliceAt :: DomainSpec -> ExtentSpec -> B.ByteString -> B.ByteString
sliceAt spec extent bytes =
  B.take (fromIntegral (extentHigh extent - extentLow extent))
    (B.drop (fromIntegral (extentLow extent - specLow spec)) bytes)

sliceFrom :: Word32 -> ExtentSpec -> B.ByteString -> B.ByteString
sliceFrom origin extent bytes =
  B.take (fromIntegral (extentHigh extent - extentLow extent))
    (B.drop (fromIntegral (extentLow extent - origin)) bytes)

putAt :: DomainSpec -> ExtentSpec -> B.ByteString -> B.ByteString -> B.ByteString
putAt spec extent replacement bytes =
  let offset = fromIntegral (extentLow extent - specBase spec)
      size = fromIntegral (extentHigh extent - extentLow extent)
   in B.take offset bytes <> replacement <> B.drop (offset + size) bytes

contained :: U.Parsed -> Domain -> Either StageError B.ByteString
contained parsed domain = do
  segment <- segmentOf parsed domain
  pure (B.map complement (B.take (domainLoaderSize domain) (B.drop (fromIntegral (U.segFlash segment)) (U.partPreTable (U.parsedParts parsed)))))

segmentOf :: U.Parsed -> Domain -> Either StageError U.SegmentEntry
segmentOf parsed domain = case filter ((== domainLoaderId domain) . U.segId) (U.partSegments (U.parsedParts parsed)) of
  [segment]
    | U.segLoad segment == base
    , fromIntegral (U.segSize segment) == domainLoaderSize domain
    , fromIntegral (U.segFlash segment) + domainLoaderSize domain <= B.length (U.partPreTable (U.parsedParts parsed)) -> Right segment
  _ -> Left (LoaderMismatch domain)
  where
    GuestAddress base = domainLoaderBase domain

canonicalSource :: B.ByteString -> B.ByteString -> B.ByteString -> Either StageError Source
canonicalSource dat a b = do
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  require (U.rebuildContainer parsed (U.parsedParts parsed) == Right dat) (ContainerRejected "noncanonical round trip")
  forM_ (zip domains [a, b]) $ \(domain, bytes) -> do
    original <- contained parsed domain
    require (original == bytes && B.length bytes == domainLoaderSize domain) (LoaderMismatch domain)
  pure (Source dat parsed (M.fromList (zip domains [a, b])))

requirePinned :: Source -> Either StageError ()
requirePinned source =
  require
    ( map sha [sourceDat source, sourceLoaders source M.! DomainA, sourceLoaders source M.! DomainB]
        == [ "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"
           , "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
           , "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
           ]
    )
    SourceUnpinned

entries :: Domain -> [(String, Word32)]
entries domain =
  [ (name, base + offset)
  | (name, offset) <- entriesAt
  , let base = if domain == DomainA then 0x40000 else 0xd0000
  ]
  where
    entriesAt =
      [ ("xm1_exception_undefined", 0x100)
      , ("xm1_exception_prefetch", 0x128)
      , ("xm1_exception_data", 0x148)
      , ("xm1_exception_reserved", 0x168)
      , ("xm1_exception_fiq", 0x184)
      , ("xm1_exception_svc", 0x1b4)
      , ("xm1_exception_svc_diag", 0x1e4)
      , ("xm1_exception_irq", 0x204)
      , ("xm1_exception_dispatcher", 0xcac)
      , ("xm1_exception_ack_wrapper", 0xd50)
      , ("xm1_exception_eoi_wrapper", 0xd60)
      , ("xm1_exception_fiq_hook", 0xd70)
      , ("xm1_exception_ack", 0x30a8)
      , ("xm1_exception_fault_record", 0x3a0)
      , ("xm1_exception_vector_slot", 0x3810)
      ]

inside :: DomainSpec -> Word32 -> Word32 -> Bool
inside spec low size = any (\(ExtentSpec lo hi) -> low >= lo && low + size <= hi) (specExtents spec)

admit :: Source -> Linked -> Linked -> Either StageError Admitted
admit source a b = do
  admitted <- forM (zip specs [a, b]) $ \(spec, artifact) -> do
    let domain = specDomain spec
    symbols <- either (const (Left (SymbolsRejected domain))) Right (F.parseSymbols (linkedSymbols artifact))
    forM_ (entries domain) $ \(name, expected) -> do
      symbol <- maybe (Left (EntryMismatch domain name)) Right (lookupSymbol symbols name)
      require (F.symbolAddress symbol == expected && F.symbolSize symbol > 0) (EntryMismatch domain name)
    forM_ symbols $ \symbol ->
      when (F.symbolSize symbol > 0 && "xm1_exception_" `isPrefixOf` F.symbolName symbol) $
        require (inside spec (F.symbolAddress symbol) (F.symbolSize symbol)) (SymbolOutside domain (F.symbolName symbol))
    require (B.length (linkedBin artifact) == fromIntegral (specHigh spec - specLow spec)) (ByteOutside domain (specHigh spec))
    let loader0 = foldl
          (\bytes extent -> putAt spec extent (sliceAt spec extent (linkedBin artifact)) bytes)
          (sourceLoaders source M.! domain)
          (specExtents spec)
        (repairAt, preimage, replacement) = schedulerReturn spec
        repairOffset = fromIntegral (repairAt - specBase spec)
    require (U.word32At loader0 repairOffset == preimage) (PatchRejected ("scheduler return preimage differs at " ++ hex repairAt))
    let loader = B.take repairOffset loader0 <> U.putWord32 replacement <> B.drop (repairOffset + 4) loader0
    pure (domain, loader, artifact)
  pure
    ( Admitted
        source
        (M.fromList [(domain, loader) | (domain, loader, _) <- admitted])
        (M.fromList [(domain, artifact) | (domain, _, artifact) <- admitted])
    )
  where
    lookupSymbol symbols name = case [symbol | symbol <- symbols, F.symbolName symbol == name] of
      [symbol] -> Just symbol
      _ -> Nothing

installedLoader :: Admitted -> Domain -> B.ByteString
installedLoader admitted domain = admittedLoaders admitted M.! domain

patchesFor :: Admitted -> U.Parsed -> Bool -> Either StageError [F.LoaderWordPatch]
patchesFor admitted parsed restoring = fmap concat $ forM specs $ \spec -> do
  current <- contained parsed (specDomain spec)
  let source = admittedSource admitted
      target = if restoring then sourceLoaders source M.! specDomain spec else installedLoader admitted (specDomain spec)
      base = specBase spec
  repair <- let (repairAt, preimage, replacement) = schedulerReturn spec
             in if restoring
                  then either (Left . PatchRejected) Right (F.loaderWordPatch (F.LoaderId (domainLoaderId (specDomain spec))) repairAt replacement preimage)
                  else either (Left . PatchRejected) Right (F.loaderWordPatch (F.LoaderId (domainLoaderId (specDomain spec))) repairAt preimage replacement)
  extents <- either (Left . PatchRejected) Right $ sequence
    [ F.loaderWordPatch (F.LoaderId (domainLoaderId (specDomain spec))) at before after
    | ExtentSpec lo hi <- specExtents spec
    , at <- [lo, lo + 4 .. hi - 4]
    , let offset = fromIntegral (at - base)
    , let before = U.word32At current offset
    , let after = U.word32At target offset
    , before /= after
    ]
  pure (repair : extents)

rebuild :: U.Parsed -> [F.LoaderWordPatch] -> Either StageError B.ByteString
rebuild parsed patches = do
  parts <- either (Left . PatchRejected) Right (F.applyLoaderWordPatches patches (U.parsedParts parsed))
  dat <- either (Left . PatchRejected) Right (U.rebuildContainer parsed parts)
  consumed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  either (Left . PatchRejected) Right (F.verifyPreTablePatches [] patches parsed consumed)
  pure dat

confineChanges :: Admitted -> B.ByteString -> Either StageError ()
confineChanges admitted dat = do
  let source = admittedSource admitted
      original = sourceDat source
      parsed = sourceParsed source
      mainStart = U.stagingLength + fromIntegral (U.hMainOffset (U.parsedHeader parsed))
  ranges <- fmap concat $ forM specs $ \spec -> do
    segment <- segmentOf parsed (specDomain spec)
    let (repairAt, _, _) = schedulerReturn spec
    pure
      ( ( mainStart + fromIntegral (U.segFlash segment) + fromIntegral (repairAt - U.segLoad segment), 4 )
      : [ ( mainStart + fromIntegral (U.segFlash segment) + fromIntegral (extentLow extent - U.segLoad segment)
        , fromIntegral (extentHigh extent - extentLow extent)
        )
        | extent <- specExtents spec
        ]
      )
  require (B.length dat == B.length original) (ChangeOutside (min (B.length dat) (B.length original)))
  let allowed = (U.mainChecksumOffset, 4) : ranges
  forM_ (findOutside allowed original dat) (Left . ChangeOutside)

findOutside :: [(Int, Int)] -> B.ByteString -> B.ByteString -> [Int]
findOutside allowed original dat =
  [ index
  | (index, True) <- zip [0 ..] (B.zipWith (/=) original dat)
  , not (any (\(low, size) -> index >= low && index < low + size) allowed)
  ]

restore :: Admitted -> B.ByteString -> Either StageError B.ByteString
restore admitted dat = do
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  patches <- patchesFor admitted parsed True
  rebuild parsed patches

stage :: Admitted -> Either StageError Staged
stage admitted = do
  let source = admittedSource admitted
  patches <- patchesFor admitted (sourceParsed source) False
  dat <- rebuild (sourceParsed source) patches
  confineChanges admitted dat
  parsed <- either (Left . ContainerRejected) Right (U.parseContainer dat)
  forM_ specs $ \spec ->
    require (contained parsed (specDomain spec) == Right (installedLoader admitted (specDomain spec))) (LoaderMismatch (specDomain spec))
  restored <- restore admitted dat
  require (restored == sourceDat source) RestoreMismatch
  pure (Staged admitted dat)

stagedLoader :: Staged -> Domain -> B.ByteString
stagedLoader staged = installedLoader (stagedAdmission staged)

contractValue :: Staged -> Value
contractValue staged = object
  [ "schema" .= (1 :: Int)
  , "piece_kind" .= ("exception-entry-exit" :: String)
  , "source_dat_sha256" .= sha (sourceDat source)
  , "candidate_dat_sha256" .= sha (stagedDat staged)
  , "image4_abort_install" .= object
      [ "vector_slots" .= (["0x2c", "0x30"] :: [String])
      , "handlers" .= (["0x61c73c", "0x61c7f0"] :: [String])
      , "dispatcher" .= ("0x40cac" :: String)
      ]
  , "retained_diagnostics" .= object
      [ "resume_gate" .= ("0x4004f10c/0x400df0cc" :: String)
      , "fiq_hook" .= ("0x4004f110/0x400df0d0" :: String)
      , "exception_hook" .= ("0x4004f114/0x400df0d4" :: String)
      , "record" .= ("0x4004f118/0x400df0d8" :: String)
      ]
  , "domains" .= [domainValue spec | spec <- specs]
  ]
  where
    source = admittedSource (stagedAdmission staged)
    domainValue spec = object
      [ "domain" .= domainLabel (specDomain spec)
      , "base" .= hex (specBase spec)
      , "stock_sha256" .= sha (sourceLoaders source M.! specDomain spec)
      , "owned_sha256" .= sha (installedLoader (stagedAdmission staged) (specDomain spec))
      , "linked_bin_sha256" .= sha (linkedBin (admittedLinked (stagedAdmission staged) M.! specDomain spec))
      , "linked_symbols_sha256" .= sha (linkedSymbols (admittedLinked (stagedAdmission staged) M.! specDomain spec))
      , "extents" .= [ extentValue spec extent | extent <- specExtents spec ]
      , "scheduler_return" .= let (repairAt, preimage, replacement) = schedulerReturn spec in object
          [ "address" .= hex repairAt, "stock" .= hex preimage, "owned" .= hex replacement
          , "owner" .= ("s2-irq-resume" :: String)
          , "extent" .= (if specDomain spec == DomainA then ["0x437a4", "0x437dc"] else ["0xd37a4", "0xd37dc"] :: [String])
          ]
      , "entries" .= [ object ["symbol" .= name, "address" .= hex address] | (name, address) <- entries (specDomain spec) ]
      ]
    extentValue spec extent = object
      [ "start" .= hex (extentLow extent)
      , "end" .= hex (extentHigh extent)
      , "stock_sha256" .= sha (sliceFrom (specBase spec) extent (sourceLoaders source M.! specDomain spec))
      , "owned_sha256" .= sha (sliceAt spec extent (linkedBin (admittedLinked (stagedAdmission staged) M.! specDomain spec)))
      ]
