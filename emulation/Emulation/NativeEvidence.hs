{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.NativeEvidence
  ( EvidenceMode (..), ComparisonScope (..), EvidenceStore, newEvidenceStore, writeEvidence
  , InputPool, newInputPool, stageInput
  ) where

import Control.Concurrent.MVar
import Control.Exception (bracket)
import Control.Monad (unless, when)
import Data.Aeson
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt, isHexDigit)
import Data.List (sort)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.NativeObservation as N
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import System.Directory (makeAbsolute, removeDirectoryRecursive, renameFile)
import System.FilePath ((</>), takeDirectory)
import System.Posix.Files (createLink, setFileMode)
import System.Posix.Temp (mkdtemp)

data EvidenceMode = CompactSuccesses | FullDiagnostics deriving (Eq, Show)
data ComparisonScope = PublicSpans | ExactDiagnostics deriving (Eq, Show)
newtype ClassKey = ClassKey String deriving (Eq, Ord)
newtype EvidencePath = EvidencePath FilePath
newtype Digest = Digest String deriving (Eq, Ord)
newtype Mismatch = Mismatch T.Text
newtype Exclusion = Exclusion T.Text
newtype ByteOffset = ByteOffset Int
newtype ByteCount = ByteCount Int

data Verdict = Passed | Failed deriving (Eq)
data Facts = Facts N.RunnerStatus Word32 [SpanFact]
data SpanFact = SpanFact N.RegionId ByteOffset ByteCount Digest Digest

data Diagnostic = Diagnostic
  { rawVerdict :: Verdict
  , rawMismatches :: [Mismatch]
  , declaredExclusions :: [Exclusion]
  , caseIdentity :: Digest
  , wireIdentity :: Digest
  , runnerIdentity :: Digest
  , publicFacts :: Facts
  }

data EvidenceStore = EvidenceStore EvidenceMode (MVar (M.Map ClassKey EvidencePath))

newEvidenceStore :: EvidenceMode -> IO EvidenceStore
newEvidenceStore mode = EvidenceStore mode <$> newMVar M.empty

data InputPool = InputPool FilePath (MVar (M.Map Digest EvidencePath))

newInputPool :: FilePath -> IO InputPool
newInputPool out = do
  root <- mkdtemp (out </> ".native-inputs-")
  InputPool root <$> newMVar M.empty

stageInput :: InputPool -> FilePath -> B.ByteString -> IO Sha256
stageInput (InputPool root inputs) path content = do
  let hash = sha256 content
      label = sha256Hex hash
      key = Digest label
  modifyMVar_ inputs $ \known -> do
    source <- case M.lookup key known of
      Just (EvidencePath existing) -> pure existing
      Nothing -> do
        let pooled = root </> label
        B.writeFile pooled content
        setFileMode pooled 0o400
        pure pooled
    bracket (mkdtemp (takeDirectory path </> ".native-stage-")) removeDirectoryRecursive $ \temporary -> do
      let staged = temporary </> "input"
      createLink source staged
      renameFile staged path
    pure (M.insert key (EvidencePath source) known)
  pure hash

-- Class ownership is published only after the full report has been written.
writeEvidence :: EvidenceStore -> ComparisonScope -> FilePath -> Value -> Value -> Value -> IO ()
writeEvidence (EvidenceStore mode owners) scope dir caseValue report result =
  case (mode, scope, parseEither (parseDiagnostic caseValue result) report) of
    (CompactSuccesses, PublicSpans, Right diagnostic) -> modifyMVar_ owners $ \classes -> do
      path <- makeAbsolute (dir </> "runner-report.json")
      let key = classKey diagnostic
      case M.lookup key classes of
        Nothing -> do
          BL.writeFile path (encode report)
          pure (M.insert key (EvidencePath path) classes)
        Just (EvidencePath exemplar) -> do
          -- A retry of the representative must not replace its full diagnostics.
          if path == exemplar then BL.writeFile path (encode report)
            else BL.writeFile path (encode (compact key diagnostic exemplar))
          pure classes
    _ -> BL.writeFile (dir </> "runner-report.json") (encode report)

parseDiagnostic :: Value -> Value -> Value -> Parser Diagnostic
parseDiagnostic caseValue result = withObject "runner report" $ \report -> do
  schema <- report .: "schema_version"
  unless (schema == (1 :: Int)) (fail "unsupported report schema")
  behavior <- withObject "behavioral result" (\o -> o .: "status") result
  unless (behavior == ("passed" :: T.Text)) (fail "behavioral failure")
  raw <- report .: "status" >>= verdict
  mismatches <- map Mismatch <$> report .: "mismatches"
  exclusions <- map Exclusion <$> report .: "exclusions"
  caseHash <- report .: "case_sha256" >>= parseDigest
  wireHash <- report .: "wire_sha256" >>= parseDigest
  runnerHash <- report .: "runner_sha256" >>= parseDigest
  exit <- report .: "runner_exit"
  unless (exit == (0 :: Int)) (fail "runner exit")
  stderrText <- report .:? "stderr" .!= ""
  unless (T.null stderrText) (fail "runner stderr")
  runner <- report .: "runner"
  facts <- withObject "runner" (\o -> do
    wire <- o .: "wire_version"
    unless (wire == (1 :: Int)) (fail "unsupported runner schema")
    label <- o .: "status"
    let status = N.runnerStatusFromLabel label
    unless (status == N.Returned) (fail "nonreturn")
    engineError <- o .: "unicorn_error"
    unless (engineError == (0 :: Int)) (fail "engine error")
    registers <- o .: "registers"
    r0 <- withObject "registers" (\r -> r .: "r0" >>= word) registers
    memory <- o .: "memory"
    spans <- parseSpans caseValue memory
    expected <- withObject "case" (.: "expected") caseValue
    (expectedStatus, expectedR0) <- withObject "expected" (\e -> do
      expectedLabel <- e .: "status"
      registers' <- e .: "registers"
      value <- withObject "registers" (\r -> r .: "r0" >>= word) registers'
      pure (N.runnerStatusFromLabel expectedLabel, value)) expected
    unless (expectedStatus == status && expectedR0 == r0) (fail "public register mismatch")
    unless (all matching spans) (fail "public memory mismatch")
    pure (Facts status r0 spans)) runner
  pure (Diagnostic raw mismatches exclusions caseHash wireHash runnerHash facts)
  where
    verdict "passed" = pure Passed
    verdict "failed" = pure Failed
    verdict (_ :: T.Text) = fail "unsupported raw status"

word :: Value -> Parser Word32
word = withText "word32" $ \text -> maybe (fail "invalid word32") pure (N.parseWord32 (T.unpack text))

parseSpans :: Value -> [Value] -> Parser [SpanFact]
parseSpans caseValue memory = withObject "case" (\c -> do
  version <- c .: "schema_version"
  unless (version == (1 :: Int)) (fail "unsupported case schema")
  regions <- c .: "regions" >>= mapM (withObject "region" (\r -> (,) <$> r .: "id" <*> r .: "address"))
  observed <- mapM (withObject "memory" (\r -> (,) <$> r .: "address" <*> r .: "bytes")) memory
  expected <- c .: "expected"
  claims <- withObject "expected" (.: "memory") expected
  mapM (withObject "claim" (\claim -> do
    name <- claim .: "region"
    offset <- claim .: "offset"
    when (offset < 0) (fail "negative claim offset")
    expectedHex <- claim .: "hex"
    expectedBytes <- bytes expectedHex
    address <- maybe (fail "missing region") pure (lookup name regions)
    actualHex <- maybe (fail "missing memory") pure (lookup (address :: T.Text) observed)
    let lengthBytes = B.length expectedBytes
        slice = T.take (2 * lengthBytes) (T.drop (2 * offset) actualHex)
    actualBytes <- bytes slice
    unless (B.length actualBytes == lengthBytes) (fail "short claimed memory")
    pure (SpanFact (N.RegionId (T.unpack name)) (ByteOffset offset) (ByteCount lengthBytes)
      (digest expectedBytes) (digest actualBytes)))) claims) caseValue

matching :: SpanFact -> Bool
matching (SpanFact _ _ _ (Digest expected) (Digest observed)) = expected == observed

bytes :: T.Text -> Parser B.ByteString
bytes text = do
  unless (even (T.length text) && T.all isHexDigit text) (fail "invalid hex bytes")
  pure (B.pack (go (T.unpack text)))
  where
    go (a:b:rest) = fromIntegral (digitToInt a * 16 + digitToInt b) : go rest
    go _ = []

digest :: B.ByteString -> Digest
digest = Digest . sha256Hex . sha256

classKey :: Diagnostic -> ClassKey
classKey d = ClassKey (sha256Hex (sha256 (BL.toStrict (encode (object
  [ "raw_status" .= verdictLabel (rawVerdict d)
  , "mismatches" .= sort [text | Mismatch text <- rawMismatches d]
  , "exclusions" .= sort [text | Exclusion text <- declaredExclusions d]
  , "runner_sha256" .= digestText (runnerIdentity d)
  ])))))

verdictLabel :: Verdict -> T.Text
verdictLabel Passed = "passed"
verdictLabel Failed = "failed"

digestText :: Digest -> String
digestText (Digest text) = text

parseDigest :: String -> Parser Digest
parseDigest text = do
  unless (length text == 64 && all isHexDigit text) (fail "invalid digest")
  pure (Digest text)

compact :: ClassKey -> Diagnostic -> FilePath -> Value
compact (ClassKey key) d exemplar = let Facts status r0 spans = publicFacts d in object
  [ "schema_version" .= (1 :: Int)
  , "representation" .= ("compact-public-output-v1" :: T.Text)
  , "input_retention" .= ("case-descriptor-artifact-paths" :: T.Text)
  , "status" .= verdictLabel (rawVerdict d)
  , "behavioral_status" .= ("passed" :: T.Text)
  , "mismatches" .= [text | Mismatch text <- rawMismatches d]
  , "exclusions" .= [text | Exclusion text <- declaredExclusions d]
  , "case_sha256" .= digestText (caseIdentity d)
  , "wire_sha256" .= digestText (wireIdentity d)
  , "runner_sha256" .= digestText (runnerIdentity d)
  , "diagnostic_class_sha256" .= key
  , "full_diagnostic_exemplar" .= exemplar
  , "exemplar_scope" .= ("same raw mismatch/exclusion class, not identical inputs" :: T.Text)
  , "runner_status" .= N.runnerStatusLabel status
  , "runner_exit" .= (0 :: Int), "unicorn_error" .= (0 :: Int)
  , "comparison" .= object
      [ "expected_status" .= N.runnerStatusLabel status
      , "observed_status" .= N.runnerStatusLabel status
      , "expected_r0" .= N.hexWord32 r0, "observed_r0" .= N.hexWord32 r0
      , "memory" .= map spanJson spans ]
  ]
  where
    spanJson (SpanFact (N.RegionId region) (ByteOffset offset) (ByteCount size) expected observed) = object
      [ "region" .= region, "offset" .= offset, "length" .= size
      , "expected_sha256" .= digestText expected, "observed_sha256" .= digestText observed ]
