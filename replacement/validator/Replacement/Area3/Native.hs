{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE ScopedTypeVariables #-}
-- | Native-case generation and execution for the area-3 contract. Generated
-- mutation cases reuse the native-case wire and artifact identities; the
-- record region and ABI registers are declared exactly, and scratch-stack
-- bytes are covered by the effect check instead of final equality.
module Replacement.Area3.Native
  ( Workspace (..)
  , prepareWorkspace
  , writeStockCase
  , ArmImage (..)
  , CandidateProfile (..)
  , loadArmImage
  , installArmImage
  , writeArmCase
  , RawRun (..)
  , runCaseFile
  , observeNative
  , renderObservationError
  , Expectation (..)
  , expectationFromCase
  , Check (..)
  , classifyStatus
  , checkCase
  , hexWord32
  , hexBytes
  , parseWord32
  , unhex
  ) where

import Control.DeepSeq (NFData (..))
import Control.Exception (IOException, try)
import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isHexDigit, toLower)
import Data.List (find, isInfixOf, nub)
import Data.Maybe (fromMaybe)
import Data.Word (Word32, Word64, Word8)
import Emulation.NativeCase (Region (..), loadCase)
import Emulation.NativeObservation
  ( GuestAddress (..)
  , NativeObservation (..)
  , ProvenanceTotal (..)
  , RegionId (..)
  , RunnerStatus (..)
  , Write (..)
  , WriteBytes (..)
  , lookupRegister
  , observeNative
  , provenanceCount
  , provenanceWrites
  , registerFromLabel
  , renderObservationError
  )
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host hiding (sha256)
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex, showHex)
import System.Directory (canonicalizePath, copyFile, createDirectoryIfMissing)
import System.Exit (ExitCode (..))
import System.FilePath ((</>), isRelative, splitDirectories, takeDirectory)

import Replacement.Area3.Contract (ArtifactId (..))
import qualified Replacement.Area3.Domain as Domain

hexWord32 :: Word32 -> String
hexWord32 value = "0x" ++ pad 8 (showHex value "")
  where
    pad width text = replicate (width - length text) '0' ++ text

boundedHex :: B.ByteString -> String
boundedHex bytes =
  let text = hexBytes bytes
   in if length text <= 128 then text else take 128 text ++ "..(" ++ show (B.length bytes) ++ " bytes)"

hexBytes :: B.ByteString -> String
hexBytes = concatMap byte . B.unpack
  where
    byte value =
      let text = showHex (value :: Word8) ""
       in replicate (2 - length text) '0' ++ text

parseWord32 :: String -> Maybe Word32
parseWord32 ('0' : 'x' : digits)
  | not (null digits) && length digits <= 8 && all isHexDigit digits =
      case readHex digits :: [(Word32, String)] of
        [(value, "")] -> Just value
        _ -> Nothing
parseWord32 digits
  | not (null digits) && length digits <= 8 && all isHexDigit digits =
      case readHex digits :: [(Word32, String)] of
        [(value, "")] -> Just value
        _ -> Nothing
parseWord32 _ = Nothing

unhex :: String -> Maybe B.ByteString
unhex text
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

data Workspace = Workspace
  { workspaceDir :: FilePath
  , workspaceTemplate :: Value
  , workspaceRegions :: [Region]
  , workspaceRecordArtifact :: ArtifactId
  }

regionIdent :: Region -> String
regionIdent region = str (get (description region) "id")

initialArtifact :: Region -> Maybe String
initialArtifact region =
  case str (get (get (description region) "initial") "artifact") of
    "" -> Nothing
    ident -> Just ident

regionByName :: [Region] -> String -> Region
regionByName regions name =
  fromMaybe (error ("region missing: " ++ name)) (find ((== name) . regionIdent) regions)

prepareWorkspace :: FilePath -> Value -> [Region] -> [(ArtifactId, FilePath, FilePath)] -> IO Workspace
prepareWorkspace directory template regions artifacts = do
  createDirectoryIfMissing True directory
  recordArtifact <-
    maybe (failH "workspace: record region has no artifact") pure (initialArtifact (regionByName regions "record"))
  let keep = [entry | entry@(ArtifactId ident, _, _) <- artifacts, ident /= recordArtifact]
  mapM_ copyArtifact keep
  pure
    Workspace
      { workspaceDir = directory
      , workspaceTemplate = template
      , workspaceRegions = regions
      , workspaceRecordArtifact = ArtifactId recordArtifact
      }
  where
    copyArtifact (_, relative, absolute) = do
      ensure
        (isRelative relative && ".." `notElem` splitDirectories relative)
        "workspace: artifact path must stay under the case directory"
      let target = directory </> relative
      createDirectoryIfMissing True (takeDirectory target)
      copyFile absolute target

data ArmImage = ArmImage
  { armImageBytes :: B.ByteString
  , armImageDigest :: String
  , armImageBase :: Word32
  , armImageSymbols :: [(String, Word32, Word32)]
  }

data CandidateProfile = CandidateProfile
  { profileImage :: ArmImage
  , profileInstructionBudget :: Integer
  , profileWallMillis :: Integer
  }

loadArmImage :: FilePath -> FilePath -> Word32 -> IO (Either String ArmImage)
loadArmImage imagePath symbolsPath base = do
  loaded <- try $ do
    canonicalImage <- canonicalizePath imagePath
    canonicalSymbols <- canonicalizePath symbolsPath
    bytes <- readRegular (2 * 1024 * 1024) canonicalImage
    symbolsText <- readRegular (1024 * 1024) canonicalSymbols
    symbols <- either failH pure (parseSymbols (BC.unpack symbolsText))
    let padded = padToWord bytes
    ensure (not (B.null padded)) "arm image empty"
    ensure ("area3_validate" `elem` map (\(name, _, _) -> name) symbols) "arm symbols: area3_validate missing"
    let baseWide = fromIntegral base :: Word64
        endWide = baseWide + fromIntegral (B.length padded)
    ensure
      ( all
          (\(_, address, size) ->
             let start = fromIntegral address :: Word64
                 extent = fromIntegral size :: Word64
              in start >= baseWide && start + extent <= endWide
          )
          symbols
      )
      "arm symbols: extent outside the image"
    pure
      ArmImage
        { armImageBytes = padded
        , armImageDigest = sha256Hex (sha256 padded)
        , armImageBase = base
        , armImageSymbols = symbols
        }
  case loaded of
    Left (message :: IOException) -> pure (Left (show message))
    Right image -> pure (Right image)
  where
    padToWord bytes =
      let remainder = B.length bytes `mod` 4
       in if remainder == 0 then bytes else bytes <> B.replicate (4 - remainder) 0
    parseSymbols text = mapM parse (filter (not . null . words) (lines text))
      where
        parse line =
          case words line of
            [address, size, kind, name] -> binding address size kind name
            [address, size, name] -> binding address size "t" name
            _ -> Left ("arm symbols: unparsed line: " ++ line)
        binding address size kind name = do
          entry <- maybe (Left ("arm symbols: bad address " ++ address)) Right (parseWord32 address)
          extent <- maybe (Left ("arm symbols: bad size " ++ size)) Right (parseWord32 size)
          if map toLower kind `elem` ["t", "w"] && extent > 0
            then Right (name, entry, extent)
            else Left ("arm symbols: unsupported symbol kind: " ++ kind)

installArmImage :: Workspace -> ArmImage -> IO ()
installArmImage workspace image =
  B.writeFile (workspaceDir workspace </> "candidate-code.bin") (armImageBytes image)

data RawRun = RawRun
  { rawStatus :: String
  , rawValue :: Value
  , rawExit :: ExitCode
  , rawTimedOut :: Bool
  , rawStderr :: B.ByteString
  }

runCaseFile :: FilePath -> FilePath -> IO (Either String (Value, [Region], RawRun))
runCaseFile runner path = do
  loaded <- try (loadCase path)
  case loaded of
    Left (message :: IOException) -> pure (Left (show message))
    Right (caseValue, regions, wire, _) -> do
      executable <- canonicalizePath runner
      (timedOut, exit, out, err) <- runBounded 120000000 (16 * 1024 * 1024) executable [] wire
      if timedOut
        then pure (Right (caseValue, regions, RawRun "host_timeout" Null exit True err))
        else do
          parsed <- try (jsonRead out)
          pure $ case parsed of
            Left (message :: IOException) -> Left (show message)
            Right value
              | get value "wire_version" == n 1 ->
                  Right (caseValue, regions, RawRun (str (get value "status")) value exit False err)
              | otherwise -> Right (caseValue, regions, RawRun "invalid_output" value exit False err)

data Expectation = Expectation
  { expectationRecordRegion :: String
  , expectationRecordBase :: Word64
  , expectationInitialRecord :: Domain.Record
  , expectationFinalRecord :: Domain.Record
  , expectationOutcome :: Domain.Outcome
  , expectationRegions :: [(String, B.ByteString)]
  , expectationRegionBounds :: [(String, Word64, Word64)]
  , expectationDeclaredFinal :: [(String, B.ByteString)]
  , expectationScratch :: [String]
  , expectationPreserved :: [(String, Word32)]
  , expectationExit :: Word32
  }

data Check
  = CheckPass
  | CheckMismatch [String]
  | CheckBlocked String
  | CheckUnsupported String
  | CheckUnknown String
  | CheckTruncated String
  deriving (Eq, Show)

instance NFData Check where
  rnf CheckPass = ()
  rnf (CheckMismatch problems) = rnf problems
  rnf (CheckBlocked message) = rnf message
  rnf (CheckUnsupported message) = rnf message
  rnf (CheckUnknown message) = rnf message
  rnf (CheckTruncated message) = rnf message

classifyStatus :: RunnerStatus -> Check
classifyStatus Returned = CheckPass
classifyStatus UnknownCall = CheckUnsupported "unknown_call"
classifyStatus UnsupportedInstruction = CheckUnsupported "unsupported_instruction"
classifyStatus InstructionBudget = CheckBlocked "instruction_budget"
classifyStatus WallBudget = CheckBlocked "wall_budget"
classifyStatus MemoryGuard = CheckBlocked "memory_guard"
classifyStatus FetchGuard = CheckBlocked "fetch_guard"
classifyStatus StubContract = CheckBlocked "stub_contract"
classifyStatus RunnerEngineError = CheckBlocked "engine_error"
classifyStatus InvalidCase = CheckBlocked "invalid_case"
classifyStatus HostTimeout = CheckBlocked "host_timeout"
classifyStatus RunnerFailure = CheckBlocked "runner_failure"
classifyStatus InvalidOutput = CheckBlocked "invalid_output"
classifyStatus (OtherStatus other) = CheckUnknown other

checkCase :: Expectation -> NativeObservation -> Check
checkCase expectation observation =
  case classifyStatus (nativeStatus observation) of
    CheckPass -> case concat [registers, memory, effects] of
      [] -> CheckPass
      problems -> CheckMismatch problems
    other -> other
  where
    missingRegisters =
      [ "register " ++ name ++ ": missing from runner output"
      | name <- "r0" : "pc" : map fst (expectationPreserved expectation)
      , lookupLabel name == Nothing
      ]
    lookupLabel name = registerFromLabel name >>= \register -> lookupRegister register observation
    expectedResult = Domain.outcomeValue (expectationOutcome expectation)
    registers
      | not (null missingRegisters) = missingRegisters
      | otherwise =
          [ "r0: expected " ++ hexWord32 expectedResult ++ ", observed " ++ hexWord32 result
          | Just result <- [lookupLabel "r0"]
          , result /= expectedResult
          ]
          ++ [ "register " ++ name ++ ": expected " ++ hexWord32 expected ++ ", observed " ++ hexWord32 result
             | (name, expected) <- expectationPreserved expectation
             , Just result <- [lookupLabel name]
             , result /= expected
             ]
          ++ [ "pc: expected " ++ hexWord32 (expectationExit expectation) ++ ", observed " ++ hexWord32 result
             | Just result <- [lookupLabel "pc"]
             , result /= expectationExit expectation
             ]
    memory =
      concat
        [ case lookup (RegionId ident) (nativeMemory observation) of
            Nothing -> ["region " ++ ident ++ ": missing from runner output"]
            Just actual ->
              let expected =
                    fromMaybe
                      (fromMaybe B.empty (lookup ident (expectationRegions expectation)))
                      (lookup ident (expectationDeclaredFinal expectation))
               in [ "region " ++ ident ++ ": expected " ++ boundedHex expected ++ ", observed " ++ boundedHex actual
                  | actual /= expected
                  ]
        | (ident, _, _) <- expectationRegionBounds expectation
        , compareRegion ident
        ]
    compareRegion ident
      | ident == expectationRecordRegion expectation = True
      | ident `elem` expectationScratch expectation = ident `elem` map fst (expectationDeclaredFinal expectation)
      | otherwise = True
    effects = case provenanceWrites (nativeProvenance observation) of
      Nothing ->
        [ "effect evidence truncated at "
            ++ show (provenanceTotal (provenanceCount (nativeProvenance observation)))
            ++ " writes"
        ]
      Just writes -> concatMap classifyWrite writes
    classifyWrite write =
      let address = guestAddress (writeAddress write)
          size = fromIntegral (writeBytes (writeLength write)) :: Word64
       in classifyAddress address size
    classifyAddress address size
      | inRecord address size =
          case expectationOutcome expectation of
            Domain.Corrected site ->
              let offset = Domain.bitIndex site `div` 8
                  start = expectationRecordBase expectation + fromIntegral offset
               in [ "record write outside the corrected byte at " ++ show address
                  | address < start || address + fromIntegral size > start + 1
                  ]
            _ -> ["record write on a " ++ Domain.outcomeClass (expectationOutcome expectation) ++ " outcome at " ++ show address]
      | otherwise = case regionAt address size of
          Just ident
            | ident `elem` expectationScratch expectation -> []
            | otherwise -> ["forbidden write to region " ++ ident ++ " at " ++ show address]
          Nothing -> ["write outside the declared regions at " ++ show address]
    inRecord address size =
      address >= expectationRecordBase expectation
        && address + fromIntegral size <= expectationRecordBase expectation + fromIntegral Domain.recordSize
    regionAt address size =
      (\(ident, _, _) -> ident)
        <$> find
          (\(_, base, extent) -> address >= base && address + fromIntegral size <= base + extent)
          (expectationRegionBounds expectation)

expectationFromCase :: [String] -> Value -> [Region] -> Expectation
expectationFromCase scratchRegions caseValue regions =
  Expectation
    { expectationRecordRegion = "record"
    , expectationRecordBase = recordBase
    , expectationInitialRecord = initialRecord
    , expectationFinalRecord = Domain.oracleFinal model
    , expectationOutcome = Domain.oracleOutcome model
    , expectationRegions = [(regionIdent region, regionBytes region) | region <- regions]
    , expectationRegionBounds =
        [ (regionIdent region, fromIntegral (baseAddress region), fromIntegral (regionSize region))
        | region <- regions
        ]
    , expectationDeclaredFinal =
        [ (ident, applyEntries ident (fromMaybe B.empty (lookup ident initialBytes)))
        | ident <- nub [ident | (ident, _, _) <- entries]
        ]
    , expectationScratch = scratchRegions
    , expectationPreserved = preserved
    , expectationExit = exitAddress
    }
  where
    initialBytes = [(regionIdent region, regionBytes region) | region <- regions]
    recordRegion = regionByName regions "record"
    recordBase = fromIntegral (baseAddress recordRegion)
    initialRecord =
      either (error . ("expectation: record bytes: " ++)) id (Domain.mkRecord (regionBytes recordRegion))
    model = Domain.oracle initialRecord
    entries =
      [ (str (get entry "region"), fromIntegral (num (get entry "offset")), fromMaybe B.empty (unhex (str (get entry "hex"))))
      | entry <- list (get (get caseValue "expected") "memory")
      ]
    applyEntries ident bytes =
      foldl'
        (\current (name, offset, value) -> if name == ident then replaceAt offset value current else current)
        bytes
        entries
    preserved =
      [ (name, fromMaybe 0 (parseWord32 (str (get (get (get caseValue "expected") "registers") name))))
      | name <- ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "sp"]
      ]
    exitAddress = fromMaybe 0 (parseWord32 (str (get caseValue "exit")))

replaceAt :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replaceAt offset value bytes =
  B.take offset bytes <> value <> B.drop (offset + B.length value) bytes

writeCaseFile :: Workspace -> String -> Value -> IO FilePath
writeCaseFile workspace caseId value = do
  let path = workspaceDir workspace </> (caseId ++ ".json")
  B.writeFile path (encodeCanonical value)
  pure path

recordArtifactValue :: FilePath -> String -> Value
recordArtifactValue path digest =
  obj
    [ ("id", s "record-input")
    , ("path", s path)
    , ("size", n (fromIntegral Domain.recordSize))
    , ("sha256", s digest)
    ]

replaceRecordRegion :: String -> Value -> Value
replaceRecordRegion oldIdent region
  | str (get (get region "initial") "artifact") == oldIdent =
      set
        "initial"
        (obj [("artifact", s "record-input"), ("offset", n 0), ("length", n (fromIntegral Domain.recordSize))])
        region
  | otherwise = region

expectedObject :: Value -> Domain.Oracle -> Value
expectedObject templateRegisters expected =
  obj
    [ ("status", s "returned")
    , ("registers", set "r0" (s (hexWord32 (Domain.outcomeValue (Domain.oracleOutcome expected)))) templateRegisters)
    , ("memory", arr memory)
    , ("events", arr [])
    ]
  where
    memory = case Domain.oracleOutcome expected of
      Domain.Corrected site ->
        let offset = Domain.bitIndex site `div` 8
            final = fromMaybe 0 (byteAt offset (Domain.recordBytes (Domain.oracleFinal expected)))
         in [obj [("region", s "record"), ("offset", n (fromIntegral offset)), ("hex", s (hexBytes (B.singleton final)))]]
      _ -> []
    byteAt offset bytes = if offset < B.length bytes then Just (B.index bytes offset) else Nothing

mutationExclusions :: [Value]
mutationExclusions =
  [ s "Generated mutation case: record-region bytes and ABI registers are declared; scratch byte values are covered by effect confinement"
  , s "Original-binary or replacement execution only; no storage read, ECC or writeback is modeled"
  ]

writeStockCase :: Workspace -> String -> String -> Domain.Record -> Domain.Oracle -> IO FilePath
writeStockCase workspace caseId provenance record expected = do
  let recordFile = caseId ++ "-record.bin"
      bytes = Domain.recordBytes record
      digest = sha256Hex (sha256 bytes)
      template = workspaceTemplate workspace
      oldIdent = unwrapArtifact (workspaceRecordArtifact workspace)
      artifacts =
        [entry | entry <- list (get template "artifacts"), str (get entry "id") /= oldIdent]
          ++ [recordArtifactValue recordFile digest]
      regions = map (replaceRecordRegion oldIdent) (list (get template "regions"))
      value =
        merge
          template
          [ ("id", s caseId)
          , ("origin", s "stock")
          , ("provenance", s provenance)
          , ("artifacts", arr artifacts)
          , ("regions", arr regions)
          , ("expected", expectedObject (get (get template "expected") "registers") expected)
          , ("exclusions", arr mutationExclusions)
          ]
  B.writeFile (workspaceDir workspace </> recordFile) bytes
  writeCaseFile workspace caseId value

writeArmCase
  :: Workspace
  -> CandidateProfile
  -> String
  -> String
  -> Domain.Record
  -> Domain.Oracle
  -> IO FilePath
writeArmCase workspace profile caseId provenance record expected = do
  let image = profileImage profile
      template = workspaceTemplate workspace
      templateRegions = list (get template "regions")
      findRegion name = fromMaybe Null (find (\region -> str (get region "id") == name) templateRegions)
      specRegion = findRegion "spec"
      recordRegion = findRegion "record"
      stackRegion = findRegion "stack"
      exitRegion = findRegion "exit"
      specIdent = str (get (get specRegion "initial") "artifact")
      oldRecordIdent = unwrapArtifact (workspaceRecordArtifact workspace)
      candidateArtifact =
        obj
          [ ("id", s "candidate-code")
          , ("path", s "candidate-code.bin")
          , ("size", n (fromIntegral (B.length (armImageBytes image))))
          , ("sha256", s (armImageDigest image))
          ]
      artifacts =
        [artifact | artifact <- list (get template "artifacts"), str (get artifact "id") == specIdent]
          ++ [candidateArtifact, recordArtifactValue (caseId ++ "-record.bin") (sha256Hex (sha256 (Domain.recordBytes record)))]
      codeRegion =
        obj
          [ ("id", s "candidate-code")
          , ("address", s (hexWord32 (armImageBase image)))
          , ("size", n (fromIntegral (B.length (armImageBytes image))))
          , ("permissions", s "rx")
          , ( "initial"
            , obj
                [ ("artifact", s "candidate-code")
                , ("offset", n 0)
                , ("length", n (fromIntegral (B.length (armImageBytes image))))
                ]
            )
          ]
      regions = [codeRegion, specRegion, replaceRecordRegion oldRecordIdent recordRegion, stackRegion, exitRegion]
      functions =
        [ obj [("address", s (hexWord32 address)), ("size", n (fromIntegral size))]
        | (name, address, size) <- armImageSymbols image
        , name == "area3_validate" || "area3_validator" `isInfixOf` name
        ]
      entryAddress =
        fromMaybe
          (error "arm image: area3_validate missing")
          (lookup "area3_validate" [(name, address) | (name, address, _) <- armImageSymbols image])
      recordBase = fromMaybe 0 (parseWord32 (str (get recordRegion "address")))
      specBase = fromMaybe 0 (parseWord32 (str (get specRegion "address")))
      registers =
        set "r0" (s (hexWord32 recordBase)) (set "r1" (s (hexWord32 specBase)) (get template "registers"))
      value =
        merge
          template
          [ ("id", s caseId)
          , ("origin", s "synthetic")
          , ("provenance", s provenance)
          , ("entry", s (hexWord32 entryAddress))
          , ("artifacts", arr artifacts)
          , ("regions", arr regions)
          , ("functions", arr functions)
          , ("registers", registers)
          , ("expected", expectedObject (get (get template "expected") "registers") expected)
          , ("limits", obj [("instructions", n (profileInstructionBudget profile)), ("wall_ms", n (profileWallMillis profile))])
          , ("exclusions", arr [s "Replacement candidate execution: no stock closure, storage read, ECC or writeback is modeled", s "Scratch byte values are covered by effect confinement"])
          ]
  B.writeFile (workspaceDir workspace </> (caseId ++ "-record.bin")) (Domain.recordBytes record)
  writeCaseFile workspace caseId value

unwrapArtifact :: ArtifactId -> String
unwrapArtifact (ArtifactId ident) = ident
