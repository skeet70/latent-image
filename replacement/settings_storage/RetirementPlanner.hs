{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module RetirementPlanner
  ( CaseOffset (..)
  , selectCaseWindow
  , LegacyFailure (..)
  , parseLegacyFailure
  , deadFrameFailure
  , expectationsMatch
  , OldEvidence (..)
  , ManifestEvidence (..)
  , CurrentEvidence (..)
  , PreservedReason (..)
  , RetirementDecision (..)
  , Candidate (..)
  , classifyRetirement
  ) where

import Data.Aeson (Value (..))
import qualified Data.ByteString as B
import Data.List (isPrefixOf, nub, sort, sortOn)
import qualified Data.Set as S
import Emulation.CachedNativeBatch (CacheVerdict (..))
import Emulation.NativeCase (CaseId (..), CaseStatus (..))
import Emulation.PrimaryInitializerSource (ByteCount (..))
import Emulation.Sha256 (Sha256, sha256)
import FujiAF.Host (arr, get, keys, list, num, set, str)
import FujiAF.JSON (encodeCanonical)
import System.Exit (ExitCode)

newtype CaseOffset = CaseOffset { unCaseOffset :: Int }
  deriving (Eq, Ord, Show)

selectCaseWindow :: CaseOffset -> Int -> [a] -> Either String [(Int, a)]
selectCaseWindow (CaseOffset offset) count cases
  | offset < 0 = Left "case offset must be non-negative"
  | count < 1 || count > 4096 = Left "case window size must be between 1 and 4096"
  | offset >= length cases = Left "case offset is out of range"
  | otherwise = Right (take count (drop offset (zip [0 :: Int ..] cases)))

data LegacyFailure = DeadReturnedFrame | OtherFailure [String]
  deriving (Eq, Ord, Show)

deadFrameFailure :: String
deadFrameFailure = "memory stack: differs from exact allowed final bytes"

parseLegacyFailure :: [String] -> LegacyFailure
parseLegacyFailure messages
  | messages == [deadFrameFailure] = DeadReturnedFrame
  | otherwise = OtherFailure messages

data OldEvidence = OldEvidence
  { oldPath :: FilePath
  , oldBytes :: ByteCount
  , oldDigest :: Sha256
  , oldCaseId :: CaseId
  , oldWireSha :: Sha256
  , oldRunnerSha :: Sha256
  , oldRunnerExit :: ExitCode
  , oldRaw :: Value
  , oldFailureClass :: LegacyFailure
  , oldRawCache :: Maybe Value
  }

data ManifestEvidence = ManifestEvidence
  { manifestEntry :: FilePath
  , manifestSha :: Sha256
  , manifestRecordedCaseId :: CaseId
  , manifestWireSha :: Sha256
  , manifestRunnerSha :: Sha256
  , manifestRunnerExit :: ExitCode
  , manifestRawSha :: Sha256
  }

data CurrentEvidence = CurrentEvidence
  { currentCaseId :: CaseId
  , currentVerdict :: CacheVerdict
  , currentRunnerSha :: Sha256
  , currentRawBytes :: B.ByteString
  , currentExpectationMatches :: Bool
  }

data PreservedReason
  = CitedResult
  | OldRawBindingPresent
  | OldReportMissing
  | OldCaseMismatch
  | MissingBinding
  | WireMismatch
  | RunnerMismatch
  | ExitMismatch
  | RawMismatch
  | CallerOrOtherFailure
  | CurrentAbiFailure
  | ExpectationMismatch
  | FailureClassWitness
  deriving (Eq, Show)

data Candidate = Candidate
  { candidatePath :: FilePath
  , candidateBytes :: ByteCount
  , candidateDigest :: Sha256
  , candidateCaseId :: CaseId
  , candidateOldWireSha :: Sha256
  , candidateManifestWireSha :: Sha256
  , candidateOldRunnerSha :: Sha256
  , candidateCurrentRunnerSha :: Sha256
  , candidateRawSha :: Sha256
  , candidateManifestSha :: Sha256
  , candidateCurrentVerdict :: CacheVerdict
  }

data RetirementDecision = RetirementCandidate Candidate | RetirementPreserved PreservedReason

expectationsMatch :: Value -> Value -> Bool
expectationsMatch oldCase currentCase =
  case (memorySegments oldExpected, memorySegments currentExpected, ignoredMemory currentCase) of
    (Just oldSegments, Just currentSegments, Just ignored) ->
      nonMemory oldExpected == nonMemory currentExpected
        && compareMemory oldSegments currentSegments ignored
    _ -> False
  where
    oldExpected = get oldCase "expected"
    currentExpected = get currentCase "expected"
    nonMemory value = set "memory" (arr []) value

memorySegments :: Value -> Maybe [(String, Int, B.ByteString)]
memorySegments expected = do
  segments <- mapM decodeSegment (list (get expected "memory"))
  validateSegments segments
  where
    decodeSegment entry = do
      let region = str (get entry "region")
          offset = fromIntegral (num (get entry "offset"))
          hex = str (get entry "hex")
      bytes <- decodeHex hex
      if offset < 0 || B.null bytes then Nothing else Just (region, offset, bytes)

validateSegments :: [(String, Int, B.ByteString)] -> Maybe [(String, Int, B.ByteString)]
validateSegments segments = go Nothing [] (sortOn (\(region, offset, _) -> (region, offset)) segments)
  where
    go _ done [] = Just (reverse done)
    go previous done (current:rest) =
      case previous of
        Just (region, end) | region == currentRegion current && currentOffset current < end -> Nothing
        _ -> go (Just (currentRegion current, currentOffset current + B.length (currentBytes current))) (current : done) rest

currentRegion :: (String, Int, B.ByteString) -> String
currentRegion (region, _, _) = region

currentOffset :: (String, Int, B.ByteString) -> Int
currentOffset (_, offset, _) = offset

currentBytes :: (String, Int, B.ByteString) -> B.ByteString
currentBytes (_, _, bytes) = bytes

ignoredMemory :: Value -> Maybe [(String, Int, Int)]
ignoredMemory value =
  validateRanges
    [ (str (get item "region"), fromIntegral (num (get item "offset")), fromIntegral (num (get item "length")))
    | item <- if "ignored_memory" `elem` keys value then list (get value "ignored_memory") else []
    ]
  where
    validateRanges ranges = go Nothing [] (sortOn (\(region, offset, _) -> (region, offset)) ranges)
    go _ done [] = Just (reverse done)
    go previous done (current:rest) =
      case previous of
        Just (region, end) | region == rangeRegion current && rangeOffset current < end -> Nothing
        _ | rangeOffset current < 0 || rangeLength current <= 0 -> Nothing
        _ -> go (Just (rangeRegion current, rangeOffset current + rangeLength current)) (current : done) rest
    rangeRegion (region, _, _) = region
    rangeOffset (_, offset, _) = offset
    rangeLength (_, _, length') = length'

compareMemory :: [(String, Int, B.ByteString)] -> [(String, Int, B.ByteString)] -> [(String, Int, Int)] -> Bool
compareMemory oldSegments currentSegments ignored =
  all compareRegion (nub (map (\(region, _, _) -> region) oldSegments ++ map (\(region, _, _) -> region) currentSegments ++ map (\(region, _, _) -> region) ignored))
  where
    compareRegion region =
      let boundaries =
            sort . nub $
              concat
                [ [offset, offset + B.length bytes]
                | (segmentRegion, offset, bytes) <- oldSegments ++ currentSegments
                , segmentRegion == region
                ]
                ++ concat
                  [ [offset, offset + length']
                  | (rangeRegion, offset, length') <- ignored
                  , rangeRegion == region
                  ]
          intervals = zip boundaries (drop 1 boundaries)
      in all (compareInterval region) intervals
    compareInterval region (start, end)
      | end <= start = True
      | any (\(rangeRegion, offset, length') -> rangeRegion == region && offset <= start && end <= offset + length') ignored = True
      | otherwise =
          sliceAt oldSegments region start (end - start) == sliceAt currentSegments region start (end - start)

sliceAt :: [(String, Int, B.ByteString)] -> String -> Int -> Int -> Maybe B.ByteString
sliceAt segments region offset length' =
  case
    [ B.take length' (B.drop (offset - start) bytes)
    | (segmentRegion, start, bytes) <- segments
    , segmentRegion == region
    , start <= offset
    , offset + length' <= start + B.length bytes
    ] of
    [bytes] -> Just bytes
    _ -> Nothing

decodeHex :: String -> Maybe B.ByteString
decodeHex text
  | odd (length text) = Nothing
  | otherwise = B.pack <$> traverse pair (chunksOfTwo text)
  where
    pair [high, low] = do
      highValue <- hexDigit high
      lowValue <- hexDigit low
      pure (fromIntegral (highValue * 16 + lowValue))
    pair _ = Nothing
    hexDigit character
      | character >= '0' && character <= '9' = Just (fromEnum character - fromEnum '0')
      | character >= 'a' && character <= 'f' = Just (fromEnum character - fromEnum 'a' + 10)
      | character >= 'A' && character <= 'F' = Just (fromEnum character - fromEnum 'A' + 10)
      | otherwise = Nothing
    chunksOfTwo [] = []
    chunksOfTwo (a:b:rest) = [a, b] : chunksOfTwo rest
    chunksOfTwo _ = []

classifyRetirement :: [FilePath] -> S.Set LegacyFailure -> OldEvidence -> Maybe ManifestEvidence -> CurrentEvidence -> RetirementDecision
classifyRetirement protected witnesses old binding current
  | Just _ <- oldRawCache old = RetirementPreserved OldRawBindingPresent
  | oldCaseId old /= currentCaseId current = RetirementPreserved OldCaseMismatch
  | oldFailureClass old /= DeadReturnedFrame = RetirementPreserved CallerOrOtherFailure
  | not (currentAbiPassed (currentVerdict current)) = RetirementPreserved CurrentAbiFailure
  | not (currentExpectationMatches current) = RetirementPreserved ExpectationMismatch
  | otherwise = case binding of
      Nothing -> RetirementPreserved MissingBinding
      Just manifest
        | oldWireSha old /= manifestWireSha manifest -> RetirementPreserved WireMismatch
        | oldRunnerSha old /= manifestRunnerSha manifest -> RetirementPreserved RunnerMismatch
        | oldRunnerSha old /= currentRunnerSha current -> RetirementPreserved RunnerMismatch
        | oldRunnerExit old /= manifestRunnerExit manifest -> RetirementPreserved ExitMismatch
        | sha256 (encodeRaw (oldRaw old)) /= manifestRawSha manifest -> RetirementPreserved RawMismatch
        | encodeRaw (oldRaw old) /= currentRawBytes current -> RetirementPreserved RawMismatch
        | any (`protects` oldPath old) protected -> RetirementPreserved CitedResult
        | oldFailureClass old `S.member` witnesses -> RetirementPreserved FailureClassWitness
        | otherwise -> RetirementCandidate Candidate
            { candidatePath = oldPath old
            , candidateBytes = oldBytes old
            , candidateDigest = oldDigest old
            , candidateCaseId = oldCaseId old
            , candidateOldWireSha = oldWireSha old
            , candidateManifestWireSha = manifestWireSha manifest
            , candidateOldRunnerSha = oldRunnerSha old
            , candidateCurrentRunnerSha = currentRunnerSha current
            , candidateRawSha = manifestRawSha manifest
            , candidateManifestSha = manifestSha manifest
            , candidateCurrentVerdict = currentVerdict current
            }
  where
    encodeRaw = encodeCanonical
    currentAbiPassed (CacheVerdict CasePassed _) = True
    currentAbiPassed _ = False
    protects directory path = path == directory || (directory ++ "/") `isPrefixOf` path
