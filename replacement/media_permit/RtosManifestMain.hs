{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import qualified A32 as A
import Control.Monad (unless, when)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (FromJSON (parseJSON), ToJSON, Value, encode, object, withObject, (.:), (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.Bits (shiftL, (.|.))
import Data.Char (isHexDigit)
import Data.List (isInfixOf, nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import Data.Word (Word32)
import qualified FujiAF.JSON as JSON
import GHC.Generics (Generic)
import Numeric (readHex, showHex)
import StockInventory (FunctionDeclaration (..), FunctionDerivation (..), FunctionSize (..), FunctionWindow (..), GuestAddress (..), SavedStackArray (..), deriveFunction, returnPremiseLabel, savedStackTaintResidualLabel, savedStackTaintResidualStatement, xm1TaskRecords, xm1TaskRecordsA, xm1TaskRecordsB)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype SlotOffset = SlotOffset Word32 deriving (Eq, Ord, Show)
newtype EntryAddress = EntryAddress Word32 deriving (Eq, Ord, Show)
data Domain = A | B deriving (Eq, Show)
data Recovery = Recovered | Partial deriving (Show)
data Contract = Contract
  { operation :: String
  -- | A µITRON 4.0 service whose behavior the X-M1 body matches, with stated differences.
  , analogue :: Maybe String
  , arguments :: Maybe String
  , result :: Maybe String
  , errors :: [(Int, String)]
  , context :: Maybe String
  , objects :: [String]
  , computedWrappers :: [Word32]
  , evidence :: [String]
  , recovery :: Recovery
  -- | Proposed census arms or native cases for facts the source cannot settle.
  , experiments :: [String]
  , gaps :: Maybe String
  -- | How the B body differs from the A body.
  , domainNotes :: Maybe String
  -- | Decomp functions the contract was read from.
  , decompSources :: [DecompSource]
  } deriving (Show)

-- | A function of the x70view decomp, at the commit the contract input names.
data DecompSource = DecompSource
  { sourceDomain :: Domain
  , sourceName :: String
  , sourceAddress :: Word32
  , sourcePath :: String
  -- | False when the definition carries a NONMATCHING marker.
  , sourceExact :: Bool
  } deriving (Show)
instance FromJSON DecompSource where
  parseJSON = withObject "decomp source" $ \o -> do
    let keys = sort (map Key.toString (KeyMap.keys o))
    unless (keys == ["address", "domain", "exact", "name", "path"]) (fail ("decomp source keys differ: " ++ unwords keys))
    DecompSource <$> o .: "domain" <*> o .: "name" <*> (o .: "address" >>= hexWord "decomp address") <*> o .: "path" <*> o .: "exact"

-- | A condition the caller must establish because the slot body does not check it.
data Precondition = Precondition { preconditionName :: String, statement :: String }
instance FromJSON Precondition where
  parseJSON = withObject "precondition" (\o -> Precondition <$> o .: "name" <*> o .: "statement")

data InputIdentity = InputIdentity { inputPath :: FilePath, inputBytes :: Int, inputSha256 :: String } deriving (Generic)
instance ToJSON InputIdentity

digest :: B.ByteString -> String
digest = concatMap (\byte -> let digits = showHex byte "" in replicate (2-length digits) '0' ++ digits) . B.unpack . SHA256.hash

data Surface = Surface
  { surfaceName :: String
  , entries :: [String]
  , surfaceContract :: String
  , surfaceEvidence :: [String]
  } deriving (Generic)
instance FromJSON Surface
instance ToJSON Surface

data DeviceLaw = DeviceLaw
  { lawName :: String
  , grounded :: String
  , missing :: String
  , scenarios :: [String]
  , lawEvidence :: [String]
  } deriving (Generic)
instance FromJSON DeviceLaw
instance ToJSON DeviceLaw

data SurfaceMap = SurfaceMap
  { nonRoot :: [Surface]
  , deviceLaws :: [DeviceLaw]
  , objectConsumers :: [Surface]
  , remainingClosure :: [String]
  } deriving (Generic)
instance FromJSON SurfaceMap
instance ToJSON SurfaceMap

newtype Sha256 = Sha256 String deriving (Eq)
instance FromJSON Sha256 where
  parseJSON value = parseJSON value >>= \text ->
    if length text == 64 && all (`elem` ("0123456789abcdef" :: String)) text then pure (Sha256 text) else fail ("not a lowercase SHA256: " ++ text)

instance FromJSON Domain where
  parseJSON value = parseJSON value >>= \name -> case (name :: String) of
    "A" -> pure A
    "B" -> pure B
    _ -> fail ("unknown domain " ++ name)

hexWord :: String -> String -> Parser Word32
hexWord what text = case stripPrefix "0x" text of
  Just digits | not (null digits) && length digits <= 8 && all isHexDigit digits, [(number, "")] <- readHex digits -> pure number
  _ -> fail (what ++ " is not 0x-prefixed 32-bit hexadecimal: " ++ text)

newtype FunctionStart = FunctionStart Word32 deriving (Eq)
instance FromJSON FunctionStart where
  parseJSON value = parseJSON value >>= fmap FunctionStart . hexWord "function start"

-- | A reviewed contract for one published slot.
data SlotContract = SlotContract { contractOffset :: Word32, _slotContract :: Contract, _contractPreconditions :: [Precondition] }
instance FromJSON SlotContract where
  parseJSON = withObject "slot contract" $ \o -> do
    offset <- o .: "offset" >>= hexWord "slot offset"
    let keys = sort (map Key.toString (KeyMap.keys o))
        expected = sort ["offset", "recovery", "operation", "analogue", "arguments", "result", "errors", "context", "objects", "wrappers", "preconditions", "experiments", "evidence", "gaps", "domainNotes", "decomp"]
    unless (keys == expected) (fail ("slot contract " ++ hex offset ++ " keys differ from the contract fields: " ++ unwords keys))
    status <- o .: "recovery" >>= \text -> case (text :: String) of
      "recovered" -> pure Recovered
      "partial" -> pure Partial
      _ -> fail ("slot contract recovery must be recovered or partial: " ++ text)
    failures <- o .: "errors" >>= mapM (withObject "error" (\e -> (,) <$> e .: "value" <*> e .: "meaning"))
    wrappers <- o .: "wrappers" >>= mapM (hexWord "wrapper entry")
    citations <- o .: "evidence"
    when (null citations) (fail ("slot contract " ++ hex offset ++ " has no evidence"))
    body <- Contract <$> o .: "operation" <*> o .: "analogue" <*> (Just <$> o .: "arguments") <*> (Just <$> o .: "result")
      <*> pure failures <*> (Just <$> o .: "context") <*> o .: "objects" <*> pure wrappers <*> pure citations <*> pure status
      <*> o .: "experiments" <*> o .: "gaps" <*> o .: "domainNotes" <*> o .: "decomp"
    SlotContract offset body <$> o .: "preconditions"

-- | A full x70view commit id.
newtype DecompCommit = DecompCommit String deriving (Eq)
instance FromJSON DecompCommit where
  parseJSON value = parseJSON value >>= \text ->
    if length text == 40 && all (`elem` ("0123456789abcdef" :: String)) text then pure (DecompCommit text) else fail ("not a full lowercase commit id: " ++ text)

exactKeys :: String -> [String] -> KeyMap.KeyMap Value -> Parser ()
exactKeys what expected o =
  let keys = sort (map Key.toString (KeyMap.keys o))
  in unless (keys == sort expected) (fail (what ++ " keys differ: " ++ unwords keys))

-- | The decomp commit the contracts were read at, and one contract per published slot.
data SlotContracts = SlotContracts DecompCommit [SlotContract]
instance FromJSON SlotContracts where
  parseJSON = withObject "slot contracts" (\o -> exactKeys "slot contracts" ["decompCommit", "slotContracts"] o >> (SlotContracts <$> o .: "decompCommit" <*> o .: "slotContracts"))

-- A reviewed start of an unpublished function, bound to one domain's loader image.
data FunctionBoundary = FunctionBoundary
  { boundaryDomain :: Domain
  , loaderSha256 :: Sha256
  , boundaryStart :: FunctionStart
  , boundaryEvidence :: [String]
  , basis :: String
  } deriving (Generic)
instance FromJSON FunctionBoundary

newtype FunctionBoundaries = FunctionBoundaries { boundaries :: [FunctionBoundary] } deriving (Generic)
instance FromJSON FunctionBoundaries

-- | A slot body the decomp reconstructs byte-exactly, admitted as its extent where StockInventory refuses one.
data DecompExtent = DecompExtent
  { extentDomain :: Domain
  , extentLoaderSha256 :: Sha256
  , extentStart :: Word32
  , extentBytes :: Int
  , extentSha256 :: Sha256
  , extentName :: String
  , extentPath :: String
  , extentBasis :: String
  }
instance FromJSON DecompExtent where
  parseJSON = withObject "decomp extent" $ \o -> do
    let keys = sort (map Key.toString (KeyMap.keys o))
    unless (keys == ["basis", "bytes", "domain", "loaderSha256", "name", "path", "sha256", "start"]) (fail ("decomp extent keys differ: " ++ unwords keys))
    DecompExtent <$> o .: "domain" <*> o .: "loaderSha256" <*> (o .: "start" >>= hexWord "extent start") <*> o .: "bytes"
      <*> o .: "sha256" <*> o .: "name" <*> o .: "path" <*> o .: "basis"

data DecompExtents = DecompExtents DecompCommit [DecompExtent]
instance FromJSON DecompExtents where
  parseJSON = withObject "decomp extents" (\o -> exactKeys "decomp extents" ["decompCommit", "extents"] o >> (DecompExtents <$> o .: "decompCommit" <*> o .: "extents"))

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

publisherEntries :: String -> Either String [(SlotOffset, EntryAddress)]
publisherEntries source = do
  let serviceLines = takeWhile (not . isInfixOf "];") (drop 1 (dropWhile (not . isInfixOf "const SERVICES:") (lines source)))
      services = concatMap hexValues serviceLines
      extensionLines = [line | line <- lines source, any (`isInfixOf` line) ["(0x1618)", "(0x161c)", "(0x1620)", "(0x1624)"]]
      extensions = [ (SlotOffset offset, EntryAddress target) | line <- extensionLines, [offset, target] <- [hexValues line] ]
  unless (length services == 93 && length extensions == 4) (Left "publisher must contain 93 service entries and four extensions")
  pure (zipWith (\index address -> (SlotOffset (4*index), EntryAddress address)) [0..] services ++ extensions)

hexValues :: String -> [Word32]
hexValues [] = []
hexValues ('0':'x':rest) = case readHex (filter (/= '_') digits) of
  [(number, "")] -> number : hexValues suffix
  _ -> []
  where (digits, suffix) = span (\c -> isHexDigit c || c == '_') rest
hexValues (_:rest) = hexValues rest

wordAt :: B.ByteString -> Int -> Word32
wordAt image offset = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index image (offset+index))) 0 [0..3]

instructions :: Word32 -> B.ByteString -> [(Word32, A.Instruction)]
instructions base image = [(base+fromIntegral offset, A.decode (base+fromIntegral offset) (wordAt image offset)) | offset <- [0,4..B.length image-4]]

functionStarts :: String -> [Word32]
functionStarts listing = mapMaybe start (lines listing)
  where
    start line = do
      rest <- stripPrefix "// ===== " line
      case readHex (takeWhile isHexDigit rest) of
        [(number, "")] -> Just number
        _ -> Nothing

-- Both loaders' T task records are declared for every domain.
savedStackArrays :: [(Domain, SavedStackArray)]
savedStackArrays = [(A, xm1TaskRecordsA), (B, xm1TaskRecordsB)]

savedStackEvidence :: Domain -> [String]
savedStackEvidence domain =
  [ "reports/replacement-inventory-baseline.md:444", "reports/replacement-inventory-baseline.md:1111", "reports/replacement-inventory-baseline.md:1124" ]
  ++ [listing ++ ":" ++ line | line <- ["3449", "3487", "3531"]]
  where
    listing = "private-data/analysis/x-m1-1.50/decode/" ++ (if domain == A then "loader-40000.dis" else "loader-d0000.dis")

extent :: Word32 -> B.ByteString -> [Word32] -> Maybe DecompExtent -> EntryAddress -> Either String Value
extent base image starts decompExtent (EntryAddress address) =
  let imageEnd = base + fromIntegral (B.length image)
      fence = minimum (imageEnd : [entry | entry <- starts, entry > address])
      outcome = deriveFunction xm1TaskRecords (GuestAddress base) image (FunctionWindow (GuestAddress address) (GuestAddress fence)) []
  in case (outcome, decompExtent) of
    (Right derived, Nothing) -> case derivedDeclaration derived of
      FunctionDeclaration _ (FunctionSize bytes) -> Right (object
        [ "status" .= ("derived" :: String), "start" .= hex address, "end" .= hex (address+fromIntegral bytes)
        , "codeEnvelopeBytes" .= bytes, "windowEnd" .= hex fence
        , "returnPremises" .= map returnPremiseLabel (derivationPremises derived)
        , "scope" .= ("StockInventory reachable A32 code envelope, not transitive code size; complete candidate window classified as code or literals" :: String)
        ])
    (Right _, Just _) -> Left ("decomp extent supplied for " ++ hex address ++ ", whose extent StockInventory derives")
    (Left problem, Nothing) -> Right (object
      [ "status" .= ("unknown" :: String), "start" .= hex address, "windowEnd" .= hex fence
      , "refusal" .= problem, "scope" .= ("No extent claimed; next entry candidate is a fence, not a measured function end" :: String)
      ])
    (Left problem, Just decomp)
      | address + fromIntegral (extentBytes decomp) > fence -> Left ("decomp extent " ++ hex address ++ " runs past its window end " ++ hex fence)
      | otherwise -> Right (object
          [ "status" .= ("decomp-exact" :: String), "start" .= hex address, "end" .= hex (address + fromIntegral (extentBytes decomp))
          , "windowEnd" .= hex fence
          , "bytes" .= extentBytes decomp, "sha256" .= (let Sha256 text = extentSha256 decomp in text)
          , "name" .= extentName decomp, "path" .= extentPath decomp, "basis" .= extentBasis decomp
          , "stockInventoryRefusal" .= problem
          , "scope" .= ("Byte range of a byte-exact decomp function at the slot target, hashed against this loader; StockInventory's premises stay unproved" :: String)
          ])

decodeInput :: FromJSON a => String -> B.ByteString -> Either String a
decodeInput what bytes = either (\problem -> Left (what ++ ": " ++ problem)) Right (JSON.decodeStrict bytes >>= parseEither parseJSON)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [publisherA, publisherB, loaderA, loaderB, listingA, image4, surfacePath, boundaryPath, contractPath, decompExtentPath, output] -> do
      exists <- doesPathExist output
      unless (not exists) (fail "output already exists; use a fresh evidence path")
      publisherBytesA <- B.readFile publisherA
      publisherBytesB <- B.readFile publisherB
      let sourceA = C.unpack publisherBytesA
          sourceB = C.unpack publisherBytesB
      entriesA <- either fail pure (publisherEntries sourceA)
      entriesB <- either fail pure (publisherEntries sourceB)
      unless (map fst entriesA == map fst entriesB) (fail "A/B published slot offsets differ")
      bytesA <- B.readFile loaderA
      bytesB <- B.readFile loaderB
      unless (B.length bytesA `mod` 4 == 0 && B.length bytesB `mod` 4 == 0) (fail "unaligned loader input")
      listingBytes <- B.readFile listingA
      imageBytes <- B.readFile image4
      unless (digest bytesA == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (fail "loader A is not canonical X-M1 1.50")
      unless (digest bytesB == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (fail "loader B is not canonical X-M1 1.50")
      unless (digest imageBytes == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 is not canonical X-M1 1.50 base0x61c400")
      let exportedA = functionStarts (C.unpack listingBytes)
          callers = instructions 0x61c400 imageBytes
      surfaceBytes <- B.readFile surfacePath
      surfaces <- either fail pure (decodeInput "surface map" surfaceBytes :: Either String SurfaceMap)
      boundaryBytes <- B.readFile boundaryPath
      reviewed <- either fail pure (boundaries <$> (decodeInput "function boundaries" boundaryBytes :: Either String FunctionBoundaries))
      let loaderOf domain = if domain == A then (0x40000, bytesA) else (0xd0000, bytesB)
          entriesOf domain = if domain == A then entriesA else entriesB
          admitBoundary boundary = do
            let (base, bytes) = loaderOf (boundaryDomain boundary)
                FunctionStart start = boundaryStart boundary
            unless (loaderSha256 boundary == Sha256 (digest bytes)) (Left ("boundary " ++ hex start ++ " names a different loader image"))
            unless (start >= base && start < base + fromIntegral (B.length bytes) && start `mod` 4 == 0) (Left ("boundary " ++ hex start ++ " is not an aligned address of its loader"))
            unless (not (null (boundaryEvidence boundary))) (Left ("boundary " ++ hex start ++ " has no evidence"))
            pure (boundaryDomain boundary, start)
      admitted <- either fail pure (mapM admitBoundary reviewed)
      let boundaryStarts domain = [start | (owner, start) <- admitted, owner == domain]
      contractBytes <- B.readFile contractPath
      SlotContracts contractCommit supplied <- either fail pure (decodeInput "slot contracts" contractBytes)
      let published = [off | (SlotOffset off, _) <- entriesA]
          suppliedOffsets = map contractOffset supplied
      unless (length (nub suppliedOffsets) == length suppliedOffsets) (fail "slot contract input repeats an offset")
      case [off | off <- suppliedOffsets, off `notElem` published] of
        [] -> pure ()
        stray -> fail ("slot contract input names unpublished offsets: " ++ unwords (map hex stray))
      case [off | off <- published, off `notElem` suppliedOffsets] of
        [] -> pure ()
        missing -> fail ("slot contract input has no contract for published offsets: " ++ unwords (map hex missing))
      decompExtentBytes <- B.readFile decompExtentPath
      DecompExtents extentCommit suppliedExtents <- either fail pure (decodeInput "decomp extents" decompExtentBytes)
      unless (extentCommit == contractCommit) (fail "decomp extents and slot contracts name different decomp commits")
      let admitExtent decomp = do
            let (base, bytes) = loaderOf (extentDomain decomp)
                start = extentStart decomp
                size = extentBytes decomp
                offsetInImage = fromIntegral (start - base)
            unless (extentLoaderSha256 decomp == Sha256 (digest bytes)) (Left ("decomp extent " ++ hex start ++ " names a different loader image"))
            unless (start `elem` [address | (_, EntryAddress address) <- entriesOf (extentDomain decomp)]) (Left ("decomp extent " ++ hex start ++ " is not a published target of its domain"))
            unless (size > 0 && size `mod` 4 == 0 && start >= base && offsetInImage <= B.length bytes && size <= B.length bytes - offsetInImage) (Left ("decomp extent " ++ hex start ++ " is not a word range of its loader"))
            unless (extentSha256 decomp == Sha256 (digest (B.take size (B.drop offsetInImage bytes)))) (Left ("decomp extent " ++ hex start ++ " bytes differ from its loader"))
            unless (not (null (extentBasis decomp))) (Left ("decomp extent " ++ hex start ++ " has no basis"))
            pure ((extentDomain decomp == A, start), decomp)
      admittedExtents <- either fail pure (mapM admitExtent suppliedExtents)
      unless (length (nub (map fst admittedExtents)) == length admittedExtents) (fail "decomp extent input repeats a start")
      let contractOf (SlotOffset off) = M.fromList [(contractOffset s, s) | s <- supplied] M.! off
          decompExtentOf domain address = lookup (domain == A, address) admittedExtents
          makeDomain domain base bytes slotEntries starts = do
            let decoded = instructions base bytes
                fences = sort (nub (starts ++ [target | (_, A.Branch _ True target) <- decoded, target >= base, target < base+fromIntegral (B.length bytes)] ++ [address | (_, EntryAddress address) <- slotEntries]))
            slots <- sequence [do
                let SlotContract _ info preconditions = contractOf offset
                    slotExtent = decompExtentOf domain address
                case slotExtent of
                  Just decomp | not (any (\source -> sourceDomain source == domain && sourceAddress source == address && sourceName source == extentName decomp && sourceExact source) (decompSources info)) ->
                    Left ("decomp extent " ++ hex address ++ " is not an exact decomp source of slot " ++ hex off)
                  _ -> Right ()
                derivedExtent <- extent base bytes fences slotExtent entry
                pure (object
                  [ "offset" .= hex off, "target" .= hex address, "recovery" .= case recovery info of { Recovered -> ("recovered" :: String); Partial -> "partial" }
                  , "operation" .= operation info, "analogue" .= analogue info, "arguments" .= arguments info, "result" .= result info
                  , "errors" .= [object ["value" .= status,"meaning" .= meaning] | (status,meaning) <- errors info]
                  , "context" .= context info, "objects" .= objects info
                  , "preconditions" .= [object ["name" .= preconditionName p, "statement" .= statement p] | p <- preconditions]
                  , "directImage4Calls" .= [hex site | (site,A.Branch _ True target) <- callers,target == address]
                  , "computedWrapperEntries" .= map hex (computedWrappers info)
                  , "experiments" .= experiments info
                  , "gaps" .= gaps info
                  , "domainNotes" .= domainNotes info
                  , "decomp" .= [object
                      [ "domain" .= show (sourceDomain source), "name" .= sourceName source, "address" .= hex (sourceAddress source)
                      , "path" .= sourcePath source, "exact" .= sourceExact source
                      ] | source <- decompSources info]
                  , "callerClosure" .= ("open: named wrappers are source-evidenced lower bounds, not exhaustive computed/Thumb/tail/alias closure; linear BL matches may be data" :: String)
                  , "extent" .= derivedExtent
                  , "evidence" .= evidence info
                  ]) | (offset@(SlotOffset off),entry@(EntryAddress address)) <- slotEntries]
            pure (object [ "domain" .= show domain, "root" .= hex (if domain == A then 0x4004dae0 else 0x400ddaa0 :: Word32), "slots" .= slots ])
      domainA <- either fail pure (makeDomain A 0x40000 bytesA entriesA (exportedA ++ boundaryStarts A))
      domainB <- either fail pure (makeDomain B 0xd0000 bytesB entriesB (boundaryStarts B))
      let DecompCommit commitText = contractCommit
      L.writeFile output (encode (object
        [ "schemaVersion" .= (1 :: Int), "firmware" .= ("X-M1 1.50" :: String)
        , "scope" .= ("Source inventory; partial contracts and extent refusals block complete RTOS replacement. No new guest qualification." :: String)
        , "decompCommit" .= commitText
        , "inputs" .= [InputIdentity path (B.length bytes) (digest bytes) | (path,bytes) <- [(publisherA,publisherBytesA),(publisherB,publisherBytesB),(loaderA,bytesA),(loaderB,bytesB),(listingA,listingBytes),(image4,imageBytes),(surfacePath,surfaceBytes),(boundaryPath,boundaryBytes),(contractPath,contractBytes),(decompExtentPath,decompExtentBytes)]]
        , "extentMethod" .= ("StockInventory.deriveFunction with both loaders' savedStackArrays; no external tails admitted; fence=min(next published entry, direct BL target, A Ghidra export, reviewed function boundary bound to the same domain loader digest, image end). A exports and boundaries never transferred to B. Where StockInventory refuses, a reviewed byte-exact decomp function at the target whose bytes hash to this loader is the extent; other refusals preserve missing extents." :: String)
        , "savedStackArrays" .= [object
            [ "domain" .= show domain, "start" .= hex start, "stride" .= stride, "count" .= count, "field" .= hex field
            , "evidence" .= savedStackEvidence domain
            ] | (domain, SavedStackArray (GuestAddress start) stride count field) <- savedStackArrays]
        , "savedStackTaintResiduals" .= [object ["name" .= savedStackTaintResidualLabel residual, "statement" .= savedStackTaintResidualStatement residual] | residual <- [minBound .. maxBound]]
        , "functionBoundaries" .= [object
            [ "domain" .= show (boundaryDomain boundary), "start" .= hex start
            , "loaderSha256" .= (let Sha256 text = loaderSha256 boundary in text)
            , "evidence" .= boundaryEvidence boundary, "basis" .= basis boundary
            ] | boundary <- reviewed, let FunctionStart start = boundaryStart boundary]
        , "domains" .= [domainA, domainB]
        , "surfaceMap" .= surfaces
        ]) <> "\n")
    _ -> fail "usage: rtos-manifest PUBLISHER_A PUBLISHER_B LOADER_A LOADER_B GHIDRA_A IMAGE4 SURFACE_JSON BOUNDARIES_JSON CONTRACTS_JSON DECOMP_EXTENTS_JSON OUTPUT_JSON"
