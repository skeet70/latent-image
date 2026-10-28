{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
-- | The typed area-3 replacement contract, compiled against the retained
-- native cases. Loading checks the contract against the case identities,
-- artifact digests, functions, budgets and the independently grounded
-- expected results before any execution.
module Replacement.Area3.Contract
  ( CaseId (..)
  , ArtifactId (..)
  , Address (..)
  , addressWord
  , addressText
  , Target (..)
  , Abi (..)
  , InputDomain (..)
  , Outputs (..)
  , Effects (..)
  , Engine (..)
  , Dependency (..)
  , FunctionExtent (..)
  , Budgets (..)
  , Anchor (..)
  , ExpectedClass (..)
  , expectedClassText
  , Coverage (..)
  , Contract (..)
  , LoadedAnchor (..)
  , loadContract
  , loadAnchors
  , refusalTokens
  ) where

import Data.Aeson (Value (..))
import Control.Monad (forM, forM_, void, when)
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import Data.List (find, sort)
import Data.Maybe (fromMaybe)
import Data.Word (Word32)
import Emulation.NativeCase (Region (..), loadCase)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (decodeStrictIntegers)
import Numeric (readHex, showHex)
import System.FilePath ((</>))

import qualified Replacement.Area3.Domain as Domain

newtype CaseId = CaseId String
  deriving (Eq, Ord, Show)

newtype ArtifactId = ArtifactId String
  deriving (Eq, Ord, Show)

newtype Address = Address Word32
  deriving (Eq, Ord, Show)

addressWord :: Address -> Word32
addressWord (Address value) = value

addressText :: Address -> String
addressText (Address value) = "0x" ++ pad 8 (showHex value "")
  where
    pad width text = replicate (width - length text) '0' ++ text

data Target = Target
  { targetBody :: String
  , targetFirmware :: String
  , targetImage :: Integer
  , targetEntry :: Address
  , targetSize :: Integer
  , targetMode :: String
  }
  deriving (Eq, Show)

data Abi = Abi
  { abiConvention :: String
  , abiRecordRegister :: String
  , abiLengthRegister :: String
  , abiResultRegister :: String
  , abiAccept :: Word32
  , abiReject :: Word32
  , abiReturn :: String
  , abiPreserved :: [String]
  }
  deriving (Eq, Show)

data InputDomain = InputDomain
  { domainRecordSize :: Int
  , domainProtectedSize :: Int
  , domainProtectedLength :: Word32
  , domainRowCount :: Int
  , domainColumnCount :: Int
  , domainRowParityOffset :: Int
  , domainColumnParityOffset :: Int
  , domainRowParityBits :: Int
  , domainRowParityHighBits :: String
  }
  deriving (Eq, Show)

data Outputs = Outputs
  { outputAccept :: Word32
  , outputReject :: Word32
  , outputClasses :: [String]
  , outputRefusals :: [String]
  }
  deriving (Eq, Show)

data Effects = Effects
  { effectRecordWrite :: String
  , effectMaxRecordWriteBytes :: Int
  , effectScratchRegions :: [String]
  , effectForbiddenRegions :: [String]
  , effectPersistentChanges :: String
  }
  deriving (Eq, Show)

data Engine = Engine
  { engineCpu :: String
  , engineMode :: String
  , engineEndian :: String
  }
  deriving (Eq, Show)

data Dependency = Dependency
  { dependencyId :: ArtifactId
  , dependencySize :: Integer
  , dependencySha256 :: String
  }
  deriving (Eq, Show)

data FunctionExtent = FunctionExtent
  { functionAddress :: Address
  , functionSize :: Integer
  }
  deriving (Eq, Show)

data Budgets = Budgets
  { budgetInstructions :: Integer
  , budgetWallMillis :: Integer
  }
  deriving (Eq, Show)

data ExpectedClass
  = ExpectAccepted
  | ExpectCorrected
  | ExpectRejected
  deriving (Eq, Show)

expectedClassText :: ExpectedClass -> String
expectedClassText ExpectAccepted = "accepted"
expectedClassText ExpectCorrected = "corrected"
expectedClassText ExpectRejected = "rejected"

data Anchor = Anchor
  { anchorCase :: CaseId
  , anchorExpected :: ExpectedClass
  }
  deriving (Eq, Show)

data Coverage = Coverage
  { coverageRecordArtifact :: ArtifactId
  , coverageProtectedBits :: Int
  , coverageSingleBit :: Int
  , coverageTwoBit :: Int
  }
  deriving (Eq, Show)

data Contract = Contract
  { contractId :: String
  , contractTarget :: Target
  , contractAbi :: Abi
  , contractDomain :: InputDomain
  , contractOutputs :: Outputs
  , contractEffects :: Effects
  , contractEngine :: Engine
  , contractDependencies :: [Dependency]
  , contractFunctions :: [FunctionExtent]
  , contractBudgets :: Budgets
  , contractAnchors :: [Anchor]
  , contractCoverage :: Coverage
  , contractExclusions :: [String]
  }
  deriving (Eq, Show)

data LoadedAnchor = LoadedAnchor
  { loadedId :: CaseId
  , loadedPath :: FilePath
  , loadedValue :: Value
  , loadedRegions :: [Region]
  , loadedRecord :: Domain.Record
  , loadedSpec :: Word32
  , loadedOutcome :: Domain.Outcome
  , loadedArtifacts :: [(ArtifactId, FilePath, FilePath)]
  }

refusalTokens :: [String]
refusalTokens =
  [ "bad_request"
  , "bad_verb"
  , "bad_record_hex"
  , "bad_length_field"
  , "unsupported_length"
  , "row_parity_high_bits"
  ]

loadContract :: FilePath -> IO Contract
loadContract path = do
  raw <- readRegular (1024 * 1024) path
  value <- either failH pure (decodeStrictIntegers raw)
  decodeContract value

decodeContract :: Value -> IO Contract
decodeContract value = do
  fields value "schema_version id target abi input outputs effects engine dependencies functions budgets anchors coverage exclusions" "contract"
  void (integer (get value "schema_version") 1 1 "schema version")
  let ident = str (get value "id")
  ensure (not (null ident)) "contract id required"
  target <- decodeTarget (get value "target")
  abi <- decodeAbi (get value "abi")
  domain <- decodeDomain (get value "input")
  outputs <- decodeOutputs (get value "outputs")
  effects <- decodeEffects (get value "effects")
  engine <- decodeEngine (get value "engine")
  dependencies <- decodeDependencies (get value "dependencies")
  functions <- decodeFunctions (get value "functions")
  budgets <- decodeBudgets (get value "budgets")
  anchors <- decodeAnchors (get value "anchors")
  coverage <- decodeCoverage (get value "coverage")
  exclusions <- stringList (get value "exclusions") "exclusions"
  ensure (not (null exclusions)) "contract exclusions required"
  ensure
    ( domainRecordSize domain == Domain.recordSize
        && domainProtectedSize domain == Domain.protectedSize
        && domainProtectedLength domain == fromIntegral Domain.protectedSize
        && domainRowCount domain == Domain.rowCount
        && domainColumnCount domain == Domain.columnCount
        && domainRowParityOffset domain == Domain.rowParityOffset
        && domainColumnParityOffset domain == Domain.columnParityOffset
        && domainRowParityBits domain == Domain.rowParityBits
        && domainRowParityHighBits domain == "zero"
    )
    "contract input domain does not match the implemented record layout"
  ensure
    (effectMaxRecordWriteBytes effects == 1 && effectPersistentChanges effects == "none")
    "contract effects do not match the permitted change set"
  ensure (abiAccept abi == outputAccept outputs && abiReject abi == outputReject outputs) "contract outputs disagree with the ABI"
  ensure (outputClasses outputs == ["accepted", "corrected", "rejected"]) "contract outcome classes"
  ensure (sort (outputRefusals outputs) == sort refusalTokens) "contract refusal set"
  ensure (coverageProtectedBits coverage == Domain.protectedBits) "contract protected-bit count"
  ensure
    (coverageSingleBit coverage == fst Domain.mutationCounts && coverageTwoBit coverage == snd Domain.mutationCounts)
    "contract mutation counts"
  ensure
    (any (\f -> functionAddress f == targetEntry target && functionSize f == targetSize target) functions)
    "contract target size does not match its function extent"
  ensure
    (any ((== coverageRecordArtifact coverage) . dependencyId) dependencies)
    "contract selected record artifact missing from dependencies"
  pure
    Contract
      { contractId = ident
      , contractTarget = target
      , contractAbi = abi
      , contractDomain = domain
      , contractOutputs = outputs
      , contractEffects = effects
      , contractEngine = engine
      , contractDependencies = dependencies
      , contractFunctions = functions
      , contractBudgets = budgets
      , contractAnchors = anchors
      , contractCoverage = coverage
      , contractExclusions = exclusions
      }

decodeTarget :: Value -> IO Target
decodeTarget value = do
  fields value "body firmware image entry size mode" "target"
  ensure (not (null (str (get value "body")))) "target body"
  ensure (not (null (str (get value "firmware")))) "target firmware"
  image <- integer (get value "image") 1 64 "target image"
  entry <- addressValue (get value "entry") "target entry"
  size <- integer (get value "size") 4 (2 * 1024 * 1024) "target size"
  ensure (addressWord entry `mod` 4 == 0) "target entry alignment"
  ensure (size `mod` 4 == 0) "target size alignment"
  pure
    Target
      { targetBody = str (get value "body")
      , targetFirmware = str (get value "firmware")
      , targetImage = image
      , targetEntry = entry
      , targetSize = size
      , targetMode = str (get value "mode")
      }

decodeAbi :: Value -> IO Abi
decodeAbi value = do
  fields value "convention record protected_length result accept reject return preserved" "abi"
  let convention = str (get value "convention")
  ensure (convention == "aapcs32") "abi convention"
  let recordRegister = str (get value "record")
      lengthRegister = str (get value "protected_length")
      resultRegister = str (get value "result")
  ensure (recordRegister == "r0" && lengthRegister == "r1" && resultRegister == "r0") "abi register binding"
  acceptValue <- word32Value (get value "accept") "abi accept"
  rejectValue <- word32Value (get value "reject") "abi reject"
  ensure (str (get value "return") == "lr") "abi return register"
  preserved <- stringList (get value "preserved") "abi preserved"
  ensure
    (preserved == ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "sp"])
    "abi preserved set"
  pure
    Abi
      { abiConvention = convention
      , abiRecordRegister = recordRegister
      , abiLengthRegister = lengthRegister
      , abiResultRegister = resultRegister
      , abiAccept = acceptValue
      , abiReject = rejectValue
      , abiReturn = "lr"
      , abiPreserved = preserved
      }

decodeDomain :: Value -> IO InputDomain
decodeDomain value = do
  fields value "record_size protected_size protected_length row_count column_count row_parity_offset column_parity_offset row_parity_bits row_parity_high_bits" "input"
  recordSize' <- integer (get value "record_size") 1 4096 "input record size"
  protectedSize' <- integer (get value "protected_size") 1 4096 "input protected size"
  protectedLength' <- word32Value (get value "protected_length") "input protected length"
  rows <- integer (get value "row_count") 1 256 "input row count"
  columns <- integer (get value "column_count") 1 256 "input column count"
  rowOffset <- integer (get value "row_parity_offset") 0 4096 "input row parity offset"
  columnOffset <- integer (get value "column_parity_offset") 0 4096 "input column parity offset"
  rowBits <- integer (get value "row_parity_bits") 1 32 "input row parity bits"
  ensure (str (get value "row_parity_high_bits") == "zero") "input high row parity bits"
  pure
    InputDomain
      { domainRecordSize = fromIntegral recordSize'
      , domainProtectedSize = fromIntegral protectedSize'
      , domainProtectedLength = protectedLength'
      , domainRowCount = fromIntegral rows
      , domainColumnCount = fromIntegral columns
      , domainRowParityOffset = fromIntegral rowOffset
      , domainColumnParityOffset = fromIntegral columnOffset
      , domainRowParityBits = fromIntegral rowBits
      , domainRowParityHighBits = "zero"
      }

decodeOutputs :: Value -> IO Outputs
decodeOutputs value = do
  fields value "accept reject classes refusals" "outputs"
  acceptValue <- word32Value (get value "accept") "output accept"
  rejectValue <- word32Value (get value "reject") "output reject"
  classes <- stringList (get value "classes") "output classes"
  refusals <- stringList (get value "refusals") "output refusals"
  pure (Outputs acceptValue rejectValue classes refusals)

decodeEffects :: Value -> IO Effects
decodeEffects value = do
  fields value "record_write max_record_write_bytes scratch_regions forbidden_regions persistent_changes" "effects"
  writes <- integer (get value "max_record_write_bytes") 0 52 "effect record write bound"
  scratch <- stringList (get value "scratch_regions") "effect scratch regions"
  forbidden <- stringList (get value "forbidden_regions") "effect forbidden regions"
  pure
    Effects
      { effectRecordWrite = str (get value "record_write")
      , effectMaxRecordWriteBytes = fromIntegral writes
      , effectScratchRegions = scratch
      , effectForbiddenRegions = forbidden
      , effectPersistentChanges = str (get value "persistent_changes")
      }

decodeEngine :: Value -> IO Engine
decodeEngine value = do
  fields value "cpu mode endian" "engine"
  pure (Engine (str (get value "cpu")) (str (get value "mode")) (str (get value "endian")))

decodeDependencies :: Value -> IO [Dependency]
decodeDependencies value = do
  ensure (case value of Array _ -> not (null (list value)); _ -> False) "dependencies required"
  forM (list value) $ \entry -> do
    fields entry "id size sha256" "dependency"
    ident <- artifactIdValue (get entry "id")
    size <- integer (get entry "size") 1 (2 * 1024 * 1024) "dependency size"
    digest <- sha256Value (get entry "sha256") "dependency sha256"
    pure (Dependency ident size digest)

decodeFunctions :: Value -> IO [FunctionExtent]
decodeFunctions value = do
  ensure (case value of Array _ -> not (null (list value)); _ -> False) "functions required"
  forM (list value) $ \entry -> do
    fields entry "address size" "function"
    address <- addressValue (get entry "address") "function address"
    size <- integer (get entry "size") 4 (2 * 1024 * 1024) "function size"
    ensure (addressWord address `mod` 4 == 0 && size `mod` 4 == 0) "function alignment"
    pure (FunctionExtent address size)

decodeBudgets :: Value -> IO Budgets
decodeBudgets value = do
  fields value "instructions wall_ms" "budgets"
  instructions <- integer (get value "instructions") 1 4000000 "instruction budget"
  wall <- integer (get value "wall_ms") 1 10000 "wall budget"
  pure (Budgets instructions wall)

decodeAnchors :: Value -> IO [Anchor]
decodeAnchors value = do
  ensure (case value of Array _ -> not (null (list value)); _ -> False) "anchors required"
  forM (list value) $ \entry -> do
    fields entry "case expected" "anchor"
    ident <- caseIdValue (get entry "case")
    expected <- expectedClassValue (get entry "expected") "anchor expected"
    pure (Anchor ident expected)

decodeCoverage :: Value -> IO Coverage
decodeCoverage value = do
  fields value "record_artifact protected_bits single_bit two_bit" "coverage"
  record <- artifactIdValue (get value "record_artifact")
  bits <- integer (get value "protected_bits") 1 4096 "coverage protected bits"
  single <- integer (get value "single_bit") 0 100000 "coverage single-bit"
  two <- integer (get value "two_bit") 0 1000000 "coverage two-bit"
  pure (Coverage record (fromIntegral bits) (fromIntegral single) (fromIntegral two))

loadAnchors :: Contract -> FilePath -> IO [LoadedAnchor]
loadAnchors contract directory = forM (contractAnchors contract) (loadAnchor contract directory)

loadAnchor :: Contract -> FilePath -> Anchor -> IO LoadedAnchor
loadAnchor contract directory anchor = do
  let CaseId name = anchorCase anchor
      path = directory </> (name ++ ".json")
  (caseValue, regions, _, _) <- loadCase path
  ensure (str (get caseValue "id") == name) ("anchor " ++ name ++ ": case id mismatch")
  entry <- addressValue (get caseValue "entry") ("anchor " ++ name ++ ": entry")
  ensure
    (entry == targetEntry (contractTarget contract))
    ("anchor " ++ name ++ ": entry mismatch")
  let cpu = get caseValue "cpu"
      engine = contractEngine contract
  ensure
    ( str (get cpu "architecture") == "arm"
        && str (get cpu "model") == engineCpu engine
        && str (get cpu "mode") == engineMode engine
        && str (get cpu "endian") == engineEndian engine
    )
    ("anchor " ++ name ++ ": engine mismatch")
  let limits = get caseValue "limits"
  ensure
    ( num (get limits "instructions") == budgetInstructions (contractBudgets contract)
        && num (get limits "wall_ms") == budgetWallMillis (contractBudgets contract)
    )
    ("anchor " ++ name ++ ": budget mismatch")
  let functions = sort [(str (get f "address"), num (get f "size")) | f <- list (get caseValue "functions")]
      declared = sort [(addressText (functionAddress f), functionSize f) | f <- contractFunctions contract]
  ensure (functions == declared) ("anchor " ++ name ++ ": function set mismatch")
  recordRegion <- requireRegion regions "record" name
  specRegion <- requireRegion regions "spec" name
  record <- either (\message -> failH ("anchor " ++ name ++ ": " ++ message)) pure (Domain.mkRecord (regionBytes recordRegion))
  let spec = littleEndianWord (regionBytes specRegion)
  ensure
    (spec == domainProtectedLength (contractDomain contract))
    ("anchor " ++ name ++ ": protected length")
  let model = Domain.oracle record
  ensure
    (expectedClassText (anchorExpected anchor) == Domain.outcomeClass (Domain.oracleOutcome model))
    ("anchor " ++ name ++ ": expected class disagrees with the contract oracle")
  let expected = get caseValue "expected"
  ensure (str (get expected "status") == "returned") ("anchor " ++ name ++ ": expected status")
  expectedR0 <- addressValue (get (get expected "registers") "r0") ("anchor " ++ name ++ ": expected r0")
  ensure
    (addressWord expectedR0 == Domain.outcomeValue (Domain.oracleOutcome model))
    ("anchor " ++ name ++ ": expected r0 disagrees with the contract oracle")
  exit <- addressValue (get caseValue "exit") ("anchor " ++ name ++ ": exit")
  let initialRegisters = get caseValue "registers"
  forM_ (abiPreserved (contractAbi contract)) $ \register ->
    ensure
      (num (get (get expected "registers") register) == num (get initialRegisters register))
      ("anchor " ++ name ++ ": preserved register " ++ register)
  ensure
    (num (get (get expected "registers") "pc") == fromIntegral (addressWord exit))
    ("anchor " ++ name ++ ": expected PC")
  let recordFinal = applyRecordExpected (list (get expected "memory")) (Domain.recordBytes record)
  ensure
    (recordFinal == Domain.recordBytes (Domain.oracleFinal model))
    ("anchor " ++ name ++ ": expected record bytes disagree with the contract oracle")
  let artifacts =
        [ ( ArtifactId (str (get a "id"))
          , str (get a "path")
          , directory </> str (get a "path")
          )
        | a <- list (get caseValue "artifacts")
        ]
      identities =
        [ (ArtifactId (str (get a "id")), num (get a "size"), str (get a "sha256"))
        | a <- list (get caseValue "artifacts")
        ]
      recordArtifact = coverageRecordArtifact (contractCoverage contract)
      required = [dependency | dependency <- contractDependencies contract, dependencyId dependency /= recordArtifact]
  forM_ required $ \dependency -> do
    identity <-
      maybe
        (failH ("anchor " ++ name ++ ": dependency missing: " ++ show (dependencyId dependency)))
        pure
        (find (\(ident, _, _) -> ident == dependencyId dependency) identities)
    case identity of
      (_, size, digest) ->
        ensure
          (size == dependencySize dependency && digest == dependencySha256 dependency)
          ("anchor " ++ name ++ ": dependency identity mismatch")
  when (anchorExpected anchor == ExpectAccepted) $ do
    recordDependency <-
      maybe (failH "contract: selected record dependency missing") pure $
        find (\dependency -> dependencyId dependency == recordArtifact) (contractDependencies contract)
    ensure
      ( fromIntegral (B.length (Domain.recordBytes record)) == dependencySize recordDependency
          && sha256Hex (sha256 (Domain.recordBytes record)) == dependencySha256 recordDependency
      )
      "contract: selected record identity mismatch"
    ensure
      (fromIntegral (regionSize recordRegion) == Domain.recordSize)
      "contract: record region size"
  pure
    LoadedAnchor
      { loadedId = CaseId name
      , loadedPath = path
      , loadedValue = caseValue
      , loadedRegions = regions
      , loadedRecord = record
      , loadedSpec = spec
      , loadedOutcome = Domain.oracleOutcome model
      , loadedArtifacts = artifacts
      }

requireRegion :: [Region] -> String -> String -> IO Region
requireRegion regions name anchorName =
  case filter (\region -> str (get (description region) "id") == name) regions of
    [region] -> pure region
    _ -> failH ("anchor " ++ anchorName ++ ": region " ++ name ++ " missing or duplicated")

littleEndianWord :: B.ByteString -> Word32
littleEndianWord bytes
  | B.length bytes < 4 = 0
  | otherwise =
      fromIntegral
        ( foldl
            (\acc (index, byte) -> acc + fromIntegral byte * (256 ^ index))
            (0 :: Integer)
            (zip [0 :: Int ..] (B.unpack (B.take 4 bytes)))
        )

applyRecordExpected :: [Value] -> B.ByteString -> B.ByteString
applyRecordExpected entries bytes = foldl applyEntry bytes entries
  where
    applyEntry current entry
      | str (get entry "region") /= "record" = current
      | otherwise =
          let offset = fromIntegral (num (get entry "offset"))
              value = fromMaybe B.empty (unhexText (str (get entry "hex")))
           in B.take offset current <> value <> B.drop (offset + B.length value) current

addressValue :: Value -> String -> IO Address
addressValue value label =
  maybe (failH (label ++ ": canonical 32-bit hexadecimal required")) (pure . Address) (parseAddressText (str value))

parseAddressText :: String -> Maybe Word32
parseAddressText ('0' : 'x' : digits)
  | length digits == 8 && all isHexDigit digits =
      case readHex digits :: [(Word32, String)] of
        [(value, "")] -> Just value
        _ -> Nothing
parseAddressText _ = Nothing

word32Value :: Value -> String -> IO Word32
word32Value value label =
  case value of
    String _ -> maybe (failH (label ++ ": canonical 32-bit hexadecimal required")) pure (parseAddressText (str value))
    Number _ -> fromIntegral <$> integer value 0 4294967295 label
    _ -> failH (label ++ ": number required")

caseIdValue :: Value -> IO CaseId
caseIdValue value = do
  let text = str value
  ensure (validIdent text) "case id invalid"
  pure (CaseId text)

artifactIdValue :: Value -> IO ArtifactId
artifactIdValue value = do
  let text = str value
  ensure (validIdent text) "artifact id invalid"
  pure (ArtifactId text)

validIdent :: String -> Bool
validIdent [] = False
validIdent (first : rest) =
  first >= 'a' && first <= 'z' && length rest <= 63 && all (\char -> (char >= 'a' && char <= 'z') || (char >= '0' && char <= '9') || char == '-') rest

sha256Value :: Value -> String -> IO String
sha256Value value label = do
  let text = str value
  ensure (length text == 64 && all isHexDigit text) (label ++ ": 64 lowercase hex digits required")
  pure text

stringList :: Value -> String -> IO [String]
stringList value label = do
  ensure (case value of Array _ -> not (null (list value)); _ -> False) (label ++ ": non-empty array required")
  let items = map str (list value)
  ensure (all (not . null) items) (label ++ ": empty string")
  pure items

expectedClassValue :: Value -> String -> IO ExpectedClass
expectedClassValue value label =
  case str value of
    "accepted" -> pure ExpectAccepted
    "corrected" -> pure ExpectCorrected
    "rejected" -> pure ExpectRejected
    _ -> failH (label ++ ": accepted, corrected or rejected required")

unhexText :: String -> Maybe B.ByteString
unhexText text
  | odd (length text) = Nothing
  | otherwise = B.pack <$> go text
  where
    go [] = Just []
    go (highChar : lowChar : rest) = do
      high <- digit highChar
      low <- digit lowChar
      ((high * 16 + low) :) <$> go rest
    go _ = Nothing
    digit char
      | char >= '0' && char <= '9' = Just (fromIntegral (fromEnum char - fromEnum '0'))
      | char >= 'a' && char <= 'f' = Just (fromIntegral (fromEnum char - fromEnum 'a' + 10))
      | otherwise = Nothing
