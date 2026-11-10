{-# LANGUAGE OverloadedStrings #-}
module Emulation.ContextCatalogue
  ( ContextCatalogue, OwnedUnionCatalogueInputs (..), ContextSourceSlice (..)
  , bindStockContextCatalogue, bindOwnedUnionContextCatalogue
  , parseContextCatalogue, contextCatalogueTag, contextCatalogueProvenance
  , ContextCatalogueKind (..), contextCatalogueKind
  , bindOwnedUnionContextCatalogueForPackage, parseContextCatalogueForPackage
  , validateRecordedStockLoaderDigests
  , CatalogueInstruction, contextCatalogueInstructions, stockContextInstructions
  , catalogueInstructionAddress, catalogueInstructionBytes
  ) where

import Control.Monad (forM_, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import Data.List (find)
import Emulation.LoaderCode (loaderSpans)
import qualified Emulation.Sha256 as SHA
import Emulation.WatchObservation (WatchAddress, watchAddress, watchAddressValue)
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.RtosOwnFirst.Regions as R
import qualified Replacement.RtosPlatform.Artifact as Platform
import qualified Replacement.RtosTimer.RootPackage as Root
import qualified Replacement.StartupArtifact.Targets as Targets
import qualified Replacement.StartupArtifact.Types as Startup

data ContextCatalogue = StockCatalogue | OwnedUnionCatalogue CatalogueAuthority [DomainCatalogue] deriving (Eq)
data CatalogueAuthority = ReviewedCompleteInputs | AdmittedRootInputs | ReviewedImmutableFragments deriving (Eq)
data DomainCatalogue = DomainCatalogue R.Domain SHA.Sha256 SHA.Sha256 deriving (Eq)
data ContextCatalogueKind = StockContextCatalogue | OwnedUnionContextCatalogue deriving (Eq)
data OwnedUnionCatalogueInputs = OwnedUnionCatalogueInputs
  { catalogueLoaderA :: B.ByteString, catalogueLoaderB :: B.ByteString
  , catalogueSymbolsA :: B.ByteString, catalogueSymbolsB :: B.ByteString
  }
data ContextSourceSlice = ContextSourceSlice WatchAddress B.ByteString
data CatalogueArtifact = LoaderImage R.Domain | SymbolTable R.Domain deriving (Eq, Ord)
data CatalogueFunction = Select | NoSave | Insert deriving (Eq, Show)
newtype CodeBytes = CodeBytes Word32
data FunctionGeometry = FunctionGeometry R.GuestAddress CodeBytes
newtype EncodedInstruction = EncodedInstruction Word32 deriving (Eq)
data CatalogueInstruction = CatalogueInstruction R.GuestAddress EncodedInstruction

bindStockContextCatalogue :: B.ByteString -> B.ByteString -> Either String ContextCatalogue
bindStockContextCatalogue loaderA loaderB = do
  expectedA <- stockLoaderDigest R.DomainA
  expectedB <- stockLoaderDigest R.DomainB
  unless (B.length loaderA == 0xdae0 && SHA.sha256 loaderA == expectedA
    && B.length loaderB == 0xdaa0 && SHA.sha256 loaderB == expectedB)
    (Left "modal lifetime requires complete pinned LoaderA and LoaderB")
  pure StockCatalogue

stockLoaderDigest :: R.Domain -> Either String SHA.Sha256
stockLoaderDigest domain = SHA.parseSha256Hex $ case domain of
  R.DomainA -> "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  R.DomainB -> "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"

validateRecordedStockLoaderDigests :: Value -> Value -> Either String ()
validateRecordedStockLoaderDigests first second = do
  let digest (String text) = SHA.parseSha256Hex (T.unpack text)
      digest _ = Left "recorded stock loader identity is not a digest"
  actualA <- digest first
  actualB <- digest second
  expectedA <- stockLoaderDigest R.DomainA
  expectedB <- stockLoaderDigest R.DomainB
  unless (actualA == expectedA && actualB == expectedB) (Left "recorded stock lifetime loader identities differ")

bindOwnedUnionContextCatalogue :: OwnedUnionCatalogueInputs -> Either String ContextCatalogue
bindOwnedUnionContextCatalogue inputs = do
  forM_ R.domains $ \domain -> do
    let (loader, symbols) = domainInputs inputs domain
    pin (LoaderImage domain) loader
    pin (SymbolTable domain) symbols
  admitOwnedCatalogue ReviewedCompleteInputs inputs
  where
    pin artifact bytes = do
      expected <- artifactDigest artifact
      unless (SHA.sha256 bytes == expected) (Left "owned-union context catalogue source identity differs")

bindOwnedUnionContextCatalogueForPackage :: Root.RootPackage -> Either String ContextCatalogue
bindOwnedUnionContextCatalogueForPackage package = do
  let staged = Root.packagePlatform package
      linked = Platform.admittedLinked (Platform.stagedAdmitted staged)
      loader domain target = do
        let actual = Targets.targetOf (Root.packageTargets package) target
        expected <- maybe (Left "admitted root package lacks its platform loader") Right
          (M.lookup domain (Platform.stagedLoaders staged))
        unless (actual == expected) (Left "root package context loader differs from its admitted platform")
        pure actual
  loaderA <- loader R.DomainA Startup.LoaderA
  loaderB <- loader R.DomainB Startup.LoaderB
  admitOwnedCatalogue AdmittedRootInputs (OwnedUnionCatalogueInputs loaderA loaderB
    (Platform.linkedSymbolsA linked) (Platform.linkedSymbolsB linked))

domainInputs :: OwnedUnionCatalogueInputs -> R.Domain -> (B.ByteString, B.ByteString)
domainInputs inputs domain = case domain of
  R.DomainA -> (catalogueLoaderA inputs, catalogueSymbolsA inputs)
  R.DomainB -> (catalogueLoaderB inputs, catalogueSymbolsB inputs)

admitOwnedCatalogue :: CatalogueAuthority -> OwnedUnionCatalogueInputs -> Either String ContextCatalogue
admitOwnedCatalogue authority inputs = do
  domains <- traverse (\domain -> do
    let (loader, symbols) = domainInputs inputs domain
    unless (B.length loader == fromIntegral (R.domainLoaderSize domain))
      (Left "owned-union context requires each complete loader domain")
    parsed <- F.parseSymbols symbols
    forM_ [Select, NoSave, Insert] $ \function -> do
      symbol <- F.lookupSymbol parsed (functionName function)
      let FunctionGeometry (R.GuestAddress entry) (CodeBytes size) = functionGeometry domain function
      unless (F.symbolAddress symbol == entry && F.symbolSize symbol == size)
        (Left "owned-union context catalogue symbol geometry differs")
    pure (DomainCatalogue domain (SHA.sha256 loader) (SHA.sha256 symbols))) R.domains
  validateContextInstructions
    [ContextSourceSlice (watchAddress 0x40000) (catalogueLoaderA inputs),
     ContextSourceSlice (watchAddress 0xd0000) (catalogueLoaderB inputs)]
  pure (OwnedUnionCatalogue authority domains)

parseContextCatalogue :: Value -> [ContextSourceSlice] -> Either String ContextCatalogue
parseContextCatalogue value slices = case value of
  String "stock" -> Right StockCatalogue
  String "owned_union" -> do
    let expected = concat [loaderSpans base size | (base, size) <- [(0x40000, 0xdae0), (0xd0000, 0xdaa0)]]
        actual = [(fromIntegral (watchAddressValue start), fromIntegral (watchAddressValue start) + fromIntegral (B.length bytes)) | ContextSourceSlice start bytes <- slices]
    unless (actual == expected) (Left "owned-union context catalogue requires all immutable loader spans")
    largeDigests <- traverse (\(address, digest) -> (,) address <$> SHA.parseSha256Hex digest) fragmentDigests
    let zeroDigest = SHA.sha256 (B.replicate 12 0)
        zeroAddresses = [0x403a4,0x403b4,0x403c4,0x403d4,0x403e4,0x403f4,
                         0xd03a4,0xd03b4,0xd03c4,0xd03d4,0xd03e4,0xd03f4]
        digests = M.fromList (largeDigests ++ [(watchAddress address, zeroDigest) | address <- zeroAddresses])
    forM_ slices $ \(ContextSourceSlice start bytes) -> do
      expectedDigest <- maybe (Left "owned-union context catalogue source span is not declared") Right (M.lookup start digests)
      unless (SHA.sha256 bytes == expectedDigest)
        (Left "owned-union context catalogue immutable source differs")
    validateContextInstructions slices
    domains <- traverse (\domain -> DomainCatalogue domain <$> artifactDigest (LoaderImage domain)
      <*> artifactDigest (SymbolTable domain)) R.domains
    pure (OwnedUnionCatalogue ReviewedImmutableFragments domains)
  _ -> Left "modal context_catalogue must be stock or owned_union"

contextCatalogueTag :: ContextCatalogue -> T.Text
contextCatalogueTag StockCatalogue = "stock"
contextCatalogueTag (OwnedUnionCatalogue _ _) = "owned_union"

contextCatalogueKind :: ContextCatalogue -> ContextCatalogueKind
contextCatalogueKind StockCatalogue = StockContextCatalogue
contextCatalogueKind (OwnedUnionCatalogue _ _) = OwnedUnionContextCatalogue

contextCatalogueInstructions :: ContextCatalogue -> [CatalogueInstruction]
contextCatalogueInstructions StockCatalogue = stockContextInstructions
contextCatalogueInstructions (OwnedUnionCatalogue _ _) = ownedInstructions

catalogueInstructionAddress :: CatalogueInstruction -> WatchAddress
catalogueInstructionAddress (CatalogueInstruction (R.GuestAddress address) _) = watchAddress address

catalogueInstructionBytes :: CatalogueInstruction -> B.ByteString
catalogueInstructionBytes (CatalogueInstruction _ (EncodedInstruction word)) = U.putWord32 word

stockContextInstructions :: [CatalogueInstruction]
stockContextInstructions =
  [CatalogueInstruction (R.GuestAddress (base + offset)) (EncodedInstruction word)
  | base <- [0x40000, 0xd0000]
  , (offset, word) <- [(0x240,0xe5820000), (0x31c,0xe5823000), (0x36b8,0xe5821000)
    , (0x3690,0xe5863000), (0x36f0,0xe5863000), (0x3720,0xe5863000)
    , (0x3848,0xe5832000), (0x2440,0xe5835000)]]

contextCatalogueProvenance :: ContextCatalogue -> Value
contextCatalogueProvenance catalogue = object
  [ "catalogue" .= contextCatalogueTag catalogue
  , "runtime_context_observed" .= False
  , "installed_lifetime_admitted" .= False
  , "required_artifacts" .= case catalogue of
      StockCatalogue -> ([] :: [Value])
      OwnedUnionCatalogue _ domains ->
        [object ["domain" .= show domain, "loader_sha256" .= SHA.sha256Hex loaderDigest,
          "symbols_sha256" .= SHA.sha256Hex symbolDigest] | DomainCatalogue domain loaderDigest symbolDigest <- domains]
  , "source_authority" .= case catalogue of
      StockCatalogue -> ("pinned-stock-loader-pair" :: T.Text)
      OwnedUnionCatalogue ReviewedCompleteInputs _ -> "reviewed-complete-inputs"
      OwnedUnionCatalogue AdmittedRootInputs _ -> "admitted-root-package"
      OwnedUnionCatalogue ReviewedImmutableFragments _ -> "reviewed-immutable-fragments"
  ]

artifactDigest :: CatalogueArtifact -> Either String SHA.Sha256
artifactDigest = SHA.parseSha256Hex . artifactDigestHex

artifactDigestHex :: CatalogueArtifact -> String
artifactDigestHex artifact = case artifact of
  LoaderImage R.DomainA -> "927394ef7ec2e3fadee6cbb86710ff67d67af3aa225d68efe50023dd953780a5"
  LoaderImage R.DomainB -> "dfd5891012fb40879711d17f2af0b3f846944656ec9d515f9dc60b35fb198afa"
  SymbolTable R.DomainA -> "b2a1e12575dcb1d9275a507b8532472a100f60e1ff57e52869f68a9850e838a1"
  SymbolTable R.DomainB -> "9ce4828257fe085804a12a6da7ba4382df6a0bff7ab12c3cd913d67b6147a1d8"

functionName :: CatalogueFunction -> String
functionName function = case function of
  Select -> "xm1_rtos_scheduler_select"
  NoSave -> "xm1_rtos_scheduler_no_save"
  Insert -> "xm1_rtos_scheduler_insert"

functionGeometry :: R.Domain -> CatalogueFunction -> FunctionGeometry
functionGeometry domain function = case (domain, function) of
  (R.DomainA, Select) -> FunctionGeometry (R.GuestAddress 0x43d40) (CodeBytes 0x80)
  (R.DomainB, Select) -> FunctionGeometry (R.GuestAddress 0xd3cfc) (CodeBytes 0x80)
  (R.DomainA, NoSave) -> FunctionGeometry (R.GuestAddress 0x43d24) (CodeBytes 0x1c)
  (R.DomainB, NoSave) -> FunctionGeometry (R.GuestAddress 0xd3ce0) (CodeBytes 0x1c)
  (R.DomainA, Insert) -> FunctionGeometry (R.GuestAddress 0x43ca8) (CodeBytes 0x78)
  (R.DomainB, Insert) -> FunctionGeometry (R.GuestAddress 0xd3c64) (CodeBytes 0x78)

fragmentDigests :: [(WatchAddress, String)]
fragmentDigests =
  [ (watchAddress 0x40000, "695431b592a0fe74f19a1334a7a3b2ad1d45e28dd440dbb9e91433a02fff1b0d")
  , (watchAddress 0x40404, "a1cef5770449e4c418f6f109024538c041050879b1eb0c00831da495071dff34")
  , (watchAddress 0xd0000, "c2fe54a2bbb5e61fcecea22e2a5339cbac1d955f7ac1782ef89594f9f9dae043")
  , (watchAddress 0xd0404, "b06ef17020bcb53d494a1ed686c6ab89b0213aa65fdb795574179b75f6dd3305")
  ]

validateContextInstructions :: [ContextSourceSlice] -> Either String ()
validateContextInstructions slices = forM_ ownedInstructions $ \(CatalogueInstruction (R.GuestAddress pc) expected) -> do
  let contains (ContextSourceSlice start bytes) =
        watchAddressValue start <= pc && toInteger pc + 4 <= toInteger (watchAddressValue start) + toInteger (B.length bytes)
  ContextSourceSlice start bytes <- maybe (Left "owned-union context instruction has no complete source") Right (find contains slices)
  let actual = EncodedInstruction (U.word32At bytes (fromIntegral (pc - watchAddressValue start)))
  unless (actual == expected) (Left "owned-union context instruction differs from its source catalogue")

ownedInstructions :: [CatalogueInstruction]
ownedInstructions =
  [ instruction 0x40240 0xe5820000, instruction 0x4031c 0xe5823000
  , instruction 0x43d48 0xe5001008, instruction 0x43d2c 0xe5863000
  , instruction 0x43d84 0xe5863000, instruction 0x43db8 0xe5863000
  , instruction 0x43848 0xe5832000, instruction 0x42440 0xe5835000
  , instruction 0xd0240 0xe5820000, instruction 0xd031c 0xe5823000
  , instruction 0xd3d04 0xe5001008, instruction 0xd3ce8 0xe5863000
  , instruction 0xd3d40 0xe5863000, instruction 0xd3d74 0xe5863000
  , instruction 0xd3848 0xe5832000, instruction 0xd2440 0xe5835000
  ]
  where instruction address word = CatalogueInstruction (R.GuestAddress address) (EncodedInstruction word)

parseContextCatalogueForPackage :: Root.RootPackage -> Value -> [ContextSourceSlice] -> Either String ContextCatalogue
parseContextCatalogueForPackage package value slices = do
  unless (value == String "owned_union") (Left "admitted-root lifetime requires the owned_union catalogue")
  catalogue <- bindOwnedUnionContextCatalogueForPackage package
  let expected =
        [ ContextSourceSlice (watchAddress (fromIntegral low))
            (B.take (fromIntegral (high - low)) (B.drop (fromIntegral (low - fromIntegral base)) loader))
        | (target, base, size) <- [(Startup.LoaderA, 0x40000 :: Word32, 0xdae0), (Startup.LoaderB, 0xd0000, 0xdaa0)]
        , let loader = Targets.targetOf (Root.packageTargets package) target
        , (low, high) <- loaderSpans (fromIntegral base) size ]
      same (ContextSourceSlice first firstBytes) (ContextSourceSlice second secondBytes) =
        first == second && firstBytes == secondBytes
  unless (length slices == length expected && and (zipWith same slices expected))
    (Left "owned-union lifetime fragments differ from their admitted root package")
  pure catalogue
