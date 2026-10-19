{-# LANGUAGE OverloadedStrings #-}
-- | A RAM-push review record, `transport/ram-push-review.json` (recipe bank
-- and D3) or `transport/ram-push-cc-review.json` (Classic Chrome): every
-- source identity and the publication contract digest the camera binary pins
-- for that payload, each with the review state of the pinned bytes.
module FujiAF.RamPushReview
  ( Sha256Hex, unSha256Hex, sha256Hex
  , CommitId, unCommitId, commitId, ReviewId, unReviewId, reviewIdentifier, CheckId, unCheckId, checkIdentifier, IsoDate, unIsoDate
  , PinIdentity(..), PinSource(..), Component(..), ReviewVerdict(..), PinStatus(..)
  , ArtifactRef(..), Review(..), ContentCheck(..), Pin(..), ContractPin(..), RecordKind(..), ReviewRecord(..)
  , Subject(..), GateFinding(..)
  , parseReviewRecord, renderReviewRecord, repin, verdictFindings, describeFinding, pinsModuleSource
  ) where

import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value(..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as M
import Data.Char (isDigit)
import Data.List (intercalate, isPrefixOf, nub, sort, sortOn)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Scientific (toBoundedInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)

newtype Sha256Hex = Sha256Hex { unSha256Hex :: String } deriving (Eq, Ord, Show)
newtype CommitId = CommitId { unCommitId :: String } deriving (Eq, Show)
newtype ReviewId = ReviewId { unReviewId :: String } deriving (Eq, Show)
newtype CheckId = CheckId { unCheckId :: String } deriving (Eq, Show)
newtype IsoDate = IsoDate { unIsoDate :: String } deriving (Eq, Show)

-- | A key of the manifest's `source_identity` object.
newtype PinIdentity = PinIdentity { unPinIdentity :: String } deriving (Eq, Ord, Show)

-- | Where a pin's digest comes from. A workspace or private-data path pin's
-- identity is its path; the private RC0 and spec inputs carry a label.
data PinSource
  = WorkspaceFile FilePath
  | PrivateFile FilePath
  | BuilderSources
  | Toolchain
  deriving (Eq, Show)

data Component = Mechanism | BankContent deriving (Eq, Show)

data ReviewVerdict = Approve | ChangesRequired deriving (Eq, Show)

data PinStatus
  = Approved ReviewId CommitId
  | ContentChecked CheckId
  | Unreviewed CommitId
  deriving (Eq, Show)

data ArtifactRef = ArtifactRef { artifactPath :: FilePath, artifactSha256 :: Sha256Hex } deriving (Eq, Show)

data Review = Review
  { reviewId :: ReviewId
  , reviewReviewer :: String
  , reviewSession :: String
  , reviewVerdict :: ReviewVerdict
  , reviewScope :: String
  , reviewDate :: IsoDate
  , reviewCommit :: CommitId
  } deriving (Eq, Show)

-- | The O1 bank-content tier: the relinked ELF's allowlist receipt, the
-- emulator run and the owner's visual check of one candidate.
data ContentCheck = ContentCheck
  { checkId :: CheckId
  , checkCommit :: CommitId
  , checkDate :: IsoDate
  , checkElfAllowlist :: ArtifactRef
  , checkEmulatorRun :: ArtifactRef
  , checkOwnerVisual :: String
  } deriving (Eq, Show)

data Pin = Pin
  { pinIdentity :: PinIdentity
  , pinSource :: PinSource
  , pinComponent :: Component
  , pinDigest :: Sha256Hex
  , pinStatus :: PinStatus
  } deriving (Eq, Show)

data ContractPin = ContractPin { contractDigest :: Sha256Hex, contractStatus :: PinStatus } deriving (Eq, Show)

-- | The payload a record pins.
data RecordKind = BankD3Record | ClassicChromeRecord deriving (Eq, Show)

data ReviewRecord = ReviewRecord
  { recordKind :: RecordKind
  , recordReviews :: [Review]
  , recordContentChecks :: [ContentCheck]
  , recordPins :: [Pin]
  , recordContract :: ContractPin
  } deriving (Eq, Show)

kindText :: RecordKind -> String
kindText BankD3Record = "xm1-ram-push-review-record"
kindText ClassicChromeRecord = "xm1-classic-chrome-ram-push-review-record"

data Subject = PinSubject PinIdentity | ContractSubject deriving (Eq, Show)

data GateFinding
  = Unapproved Subject PinStatus
  | BytesDiffer PinIdentity Sha256Hex Sha256Hex
  | BytesUnreadable PinIdentity String
  deriving (Eq, Show)

sha256Hex :: String -> Maybe Sha256Hex
sha256Hex candidate
  | length candidate == 64 && all lowerHex candidate = Just (Sha256Hex candidate)
  | otherwise = Nothing

lowerHex :: Char -> Bool
lowerHex c = isDigit c || (c >= 'a' && c <= 'f')

type Parse = Either String

reject :: String -> Parse a
reject = Left

require :: Bool -> String -> Parse ()
require yes message = unless yes (Left message)

field :: Value -> String -> Value
field (Object object) key = maybe Null id (M.lookup (K.fromString key) object)
field _ _ = Null

commitId :: String -> Either String CommitId
commitId = commitField "commit" . String . T.pack

reviewIdentifier :: String -> Either String ReviewId
reviewIdentifier = fmap ReviewId . nameField "review id" . String . T.pack

checkIdentifier :: String -> Either String CheckId
checkIdentifier = fmap CheckId . nameField "content check id" . String . T.pack

closed :: String -> Value -> [String] -> Parse ()
closed what (Object object) keys = require (sort (map K.toString (M.keys object)) == sort keys) (what++": closed keys "++show (sort keys)++" required")
closed what _ _ = reject (what++": JSON object required")

text :: String -> Value -> Parse String
text what (String value) = do
  let string = T.unpack value
  require (not (null string) && length string <= 1024 && all (>= ' ') string) (what++": bounded printable text required")
  pure string
text what _ = reject (what++": text required")

digestField :: String -> Value -> Parse Sha256Hex
digestField what value = text what value >>= maybe (reject (what++": lowercase SHA-256 required")) pure . sha256Hex

commitField :: String -> Value -> Parse CommitId
commitField what value = do
  commit <- text what value
  require (length commit == 40 && all lowerHex commit) (what++": full lowercase commit hash required")
  pure (CommitId commit)

nameField :: String -> Value -> Parse String
nameField what value = do
  name <- text what value
  require (length name <= 64 && all (\c -> isDigit c || c `elem` ("-_." :: String) || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')) name) (what++": identifier of letters, digits, '-', '_' or '.' required")
  pure name

dateField :: String -> Value -> Parse IsoDate
dateField what value = do
  date <- text what value
  require (length date == 10 && all isDigit (take 4 date ++ take 2 (drop 5 date) ++ drop 8 date) && date !! 4 == '-' && date !! 7 == '-') (what++": YYYY-MM-DD required")
  pure (IsoDate date)

list :: String -> Value -> Int -> Parse [Value]
list what (Array values) cap = do
  require (V.length values <= cap) (what++": at most "++show cap++" entries")
  pure (V.toList values)
list what _ _ = reject (what++": list required")

artifact :: String -> Value -> Parse ArtifactRef
artifact what value = do
  closed what value ["path","sha256"]
  ArtifactRef <$> text (what++".path") (field value "path") <*> digestField (what++".sha256") (field value "sha256")

parseReview :: Value -> Parse Review
parseReview value = do
  closed "review" value ["commit","date","id","reviewer","scope","session","verdict"]
  identifier <- ReviewId <$> nameField "review id" (field value "id")
  verdict <- case field value "verdict" of
    String "approve" -> pure Approve
    String "changes-required" -> pure ChangesRequired
    _ -> reject "review verdict must be approve or changes-required"
  Review identifier
    <$> text "review reviewer" (field value "reviewer")
    <*> text "review session" (field value "session")
    <*> pure verdict
    <*> text "review scope" (field value "scope")
    <*> dateField "review date" (field value "date")
    <*> commitField "review commit" (field value "commit")

parseCheck :: Value -> Parse ContentCheck
parseCheck value = do
  closed "content check" value ["commit","date","elf_allowlist_receipt","emulator_run","id","owner_visual_check"]
  ContentCheck
    <$> (CheckId <$> nameField "content check id" (field value "id"))
    <*> commitField "content check commit" (field value "commit")
    <*> dateField "content check date" (field value "date")
    <*> artifact "content check elf_allowlist_receipt" (field value "elf_allowlist_receipt")
    <*> artifact "content check emulator_run" (field value "emulator_run")
    <*> text "content check owner_visual_check" (field value "owner_visual_check")

parseStatus :: String -> Value -> Parse PinStatus
parseStatus what value = case field value "verdict" of
  String "approved" -> do
    closed what value ["commit","review","verdict"]
    Approved <$> (ReviewId <$> nameField (what++" review") (field value "review")) <*> commitField (what++" commit") (field value "commit")
  String "content-checked" -> do
    closed what value ["check","verdict"]
    ContentChecked . CheckId <$> nameField (what++" check") (field value "check")
  String "unreviewed" -> do
    closed what value ["commit","verdict"]
    Unreviewed <$> commitField (what++" commit") (field value "commit")
  _ -> reject (what++": verdict must be approved, content-checked or unreviewed")

relativePath :: FilePath -> Bool
relativePath path = not (null path) && not ("/" `isPrefixOf` path) && ".." `notElem` segments && "" `notElem` segments
  where segments = splitOn '/' path
        splitOn c s = case break (== c) s of (a, []) -> [a]; (a, _:rest) -> a : splitOn c rest

parsePin :: Value -> Parse Pin
parsePin value = do
  identity <- text "pin identity" (field value "identity")
  require (length identity <= 256) "pin identity longer than 256 characters"
  let what = "pin "++identity
      keys extra = closed what value (["component","identity","sha256","source","status"] ++ extra)
  source <- case field value "source" of
    String "workspace" -> do
      keys []
      require (relativePath identity && not ("private-data/" `isPrefixOf` identity)) (what++": workspace identity must be a relative path outside private-data")
      pure (WorkspaceFile identity)
    String "private" -> do
      keys ["path"]
      path <- text (what++" path") (field value "path")
      require (relativePath path && "private-data/" `isPrefixOf` path) (what++": private path must lie under private-data/")
      require (identity == path || not ('/' `elem` identity)) (what++": a private identity is its path or a label")
      pure (PrivateFile path)
    String "builder" -> keys [] >> pure BuilderSources
    String "toolchain" -> keys [] >> pure Toolchain
    _ -> reject (what++": source must be workspace, private, builder or toolchain")
  component <- case field value "component" of
    String "mechanism" -> pure Mechanism
    String "bank-content" -> pure BankContent
    _ -> reject (what++": component must be mechanism or bank-content")
  Pin (PinIdentity identity) source component
    <$> digestField (what++" sha256") (field value "sha256")
    <*> parseStatus (what++" status") (field value "status")

parseContract :: Value -> Parse ContractPin
parseContract value = do
  closed "publication_contract" value ["sha256","status"]
  ContractPin <$> digestField "publication_contract sha256" (field value "sha256")
              <*> parseStatus "publication_contract status" (field value "status")

parseReviewRecord :: B.ByteString -> Either String ReviewRecord
parseReviewRecord document = do
  value <- decodeStrictIntegers document
  closed "review record" value ["content_checks","kind","pins","publication_contract","reviews","schema"]
  case field value "schema" of
    Number n | toBoundedInteger n == Just (1 :: Int) -> pure ()
    _ -> reject "review record schema must be 1"
  kind <- case field value "kind" of
    String kindName | T.unpack kindName == kindText BankD3Record -> pure BankD3Record
                    | T.unpack kindName == kindText ClassicChromeRecord -> pure ClassicChromeRecord
    _ -> reject "review record kind mismatch"
  reviews <- list "reviews" (field value "reviews") 256 >>= mapM parseReview
  checks <- list "content_checks" (field value "content_checks") 256 >>= mapM parseCheck
  pinValues <- list "pins" (field value "pins") 32
  pins <- forM pinValues parsePin
  contract <- parseContract (field value "publication_contract")
  let reviewIds = map reviewId reviews
      checkIds = map checkId checks
      identities = map pinIdentity pins
  require (not (null pins)) "pins must not be empty"
  require (length (nub (map unReviewId reviewIds)) == length reviewIds) "review ids must be distinct"
  require (length (nub (map unCheckId checkIds)) == length checkIds) "content check ids must be distinct"
  require (length (nub identities) == length identities) "pin identities must be distinct"
  let resolves what status = case status of
        Approved identifier _ -> require (identifier `elem` reviewIds) (what++": unknown review "++unReviewId identifier)
        ContentChecked identifier -> require (identifier `elem` checkIds) (what++": unknown content check "++unCheckId identifier)
        Unreviewed _ -> pure ()
  forM_ pins $ \pin -> do
    let what = "pin "++unPinIdentity (pinIdentity pin)
    resolves what (pinStatus pin)
    case pinStatus pin of
      ContentChecked _ -> require (pinComponent pin == BankContent) (what++": content-checked applies only to bank-content pins")
      _ -> pure ()
  resolves "publication_contract" (contractStatus contract)
  case contractStatus contract of
    ContentChecked _ -> reject "publication_contract: content-checked applies only to bank-content pins"
    _ -> pure ()
  pure (ReviewRecord kind reviews checks pins contract)

-- | Pins and the contract whose status does not admit them to a camera run:
-- unreviewed, or approved by a review whose verdict is changes-required.
verdictFindings :: ReviewRecord -> [GateFinding]
verdictFindings record =
  [Unapproved (PinSubject (pinIdentity pin)) (pinStatus pin) | pin <- recordPins record, not (admits (pinStatus pin))]
  ++ [Unapproved ContractSubject status | let status = contractStatus (recordContract record), not (admits status)]
  where
    admits (Approved identifier _) = any (\review -> reviewId review == identifier && reviewVerdict review == Approve) (recordReviews record)
    admits (ContentChecked _) = True
    admits (Unreviewed _) = False

describeFinding :: GateFinding -> String
describeFinding finding = case finding of
  Unapproved subject status -> subjectName subject ++ ": " ++ statusText status
  BytesDiffer (PinIdentity identity) recorded current -> identity ++ ": recorded " ++ unSha256Hex recorded ++ ", current bytes " ++ unSha256Hex current
  BytesUnreadable (PinIdentity identity) reason -> identity ++ ": unreadable: " ++ reason
  where
    subjectName (PinSubject (PinIdentity identity)) = identity
    subjectName ContractSubject = "publication contract"
    statusText (Approved identifier commit) = "approved by " ++ unReviewId identifier ++ " at " ++ unCommitId commit ++ ", whose verdict is changes-required"
    statusText (ContentChecked identifier) = "content-checked by " ++ unCheckId identifier
    statusText (Unreviewed commit) = "unreviewed, pinned at " ++ unCommitId commit

-- | The module the camera binary compiles in: `RamPushReviewedPins
-- (reviewedContract)` from a bank and D3 record, `RamPushCcReviewedPins
-- (reviewedCcContract)` from a Classic Chrome record.
pinsModuleSource :: ReviewRecord -> String
pinsModuleSource record = unlines $
  [ "-- Generated by //transport:" ++ target ++ " from transport/" ++ source ++ "."
  , "module " ++ moduleName ++ " (" ++ binding ++ ") where"
  , ""
  , "import FujiAF.RamPush (ReviewedContract (..))"
  , ""
  , binding ++ " :: ReviewedContract"
  , binding ++ " = ReviewedContract"
  ]
  ++ zipWith (\prefix line -> "  " ++ prefix ++ " " ++ line) ("[" : repeat ",") entries
  ++ [ "  ]"
     , "  " ++ show (unSha256Hex (contractDigest (recordContract record)))
     ]
  where
    entries = [ "(" ++ show identity ++ "," ++ show (unSha256Hex (pinDigest pin)) ++ ")"
              | pin <- sortOn pinIdentity (recordPins record), let PinIdentity identity = pinIdentity pin ]
    (target, source, moduleName, binding) = case recordKind record of
      BankD3Record -> ("ram-push-pins-module", "ram-push-review.json", "RamPushReviewedPins", "reviewedContract")
      ClassicChromeRecord -> ("ram-push-cc-pins-module", "ram-push-cc-review.json", "RamPushCcReviewedPins", "reviewedCcContract")

-- | The record's checked-in layout: one canonical JSON line per review,
-- content check and pin.
renderReviewRecord :: ReviewRecord -> B.ByteString
renderReviewRecord record = C.pack (unlines (
  [ "{"
  , "  \"schema\": 1,"
  , "  \"kind\": \"" ++ kindText (recordKind record) ++ "\","
  , "  \"reviews\": " ++ items (map reviewValue (recordReviews record)) ++ ","
  , "  \"content_checks\": " ++ items (map checkValue (recordContentChecks record)) ++ ","
  , "  \"pins\": " ++ items (map pinValue (recordPins record)) ++ ","
  , "  \"publication_contract\": " ++ canonical (contractValue (recordContract record))
  , "}" ]))
  where
    canonical = C.unpack . encodeCanonical
    items [] = "[]"
    items values = "[\n" ++ intercalate ",\n" (map (("    " ++) . canonical) values) ++ "\n  ]"
    object pairs = Object (M.fromList [(K.fromString key, value) | (key, value) <- pairs])
    str = String . T.pack
    artifactValue ref = object [("path", str (artifactPath ref)), ("sha256", str (unSha256Hex (artifactSha256 ref)))]
    reviewValue review = object
      [ ("id", str (unReviewId (reviewId review))), ("reviewer", str (reviewReviewer review))
      , ("session", str (reviewSession review))
      , ("verdict", str (case reviewVerdict review of Approve -> "approve"; ChangesRequired -> "changes-required"))
      , ("scope", str (reviewScope review)), ("date", str (unIsoDate (reviewDate review)))
      , ("commit", str (unCommitId (reviewCommit review))) ]
    checkValue check = object
      [ ("id", str (unCheckId (checkId check))), ("commit", str (unCommitId (checkCommit check)))
      , ("date", str (unIsoDate (checkDate check)))
      , ("elf_allowlist_receipt", artifactValue (checkElfAllowlist check))
      , ("emulator_run", artifactValue (checkEmulatorRun check))
      , ("owner_visual_check", str (checkOwnerVisual check)) ]
    statusValue status = object $ case status of
      Approved identifier commit -> [("verdict", str "approved"), ("review", str (unReviewId identifier)), ("commit", str (unCommitId commit))]
      ContentChecked identifier -> [("verdict", str "content-checked"), ("check", str (unCheckId identifier))]
      Unreviewed commit -> [("verdict", str "unreviewed"), ("commit", str (unCommitId commit))]
    sourceFields source = case source of
      WorkspaceFile _ -> [("source", str "workspace")]
      PrivateFile path -> [("source", str "private"), ("path", str path)]
      BuilderSources -> [("source", str "builder")]
      Toolchain -> [("source", str "toolchain")]
    pinValue pin = object (
      [ ("identity", str (unPinIdentity (pinIdentity pin)))
      , ("component", str (case pinComponent pin of Mechanism -> "mechanism"; BankContent -> "bank-content"))
      , ("sha256", str (unSha256Hex (pinDigest pin)))
      , ("status", statusValue (pinStatus pin)) ]
      ++ sourceFields (pinSource pin))
    contractValue contract = object
      [ ("sha256", str (unSha256Hex (contractDigest contract))), ("status", statusValue (contractStatus contract)) ]

-- | Sets each named subject's digest to the given one and its status to
-- `status`; the result is the record re-read through 'parseReviewRecord'.
repin :: PinStatus -> [(Subject, Sha256Hex)] -> ReviewRecord -> Either String ReviewRecord
repin status current record = do
  forM_ current $ \(subject, _) -> case subject of
    PinSubject identity -> require (identity `elem` map pinIdentity (recordPins record)) ("no pin named "++unPinIdentity identity)
    ContractSubject -> pure ()
  let update pin = case lookup (PinSubject (pinIdentity pin)) current of
        Just digest -> pin { pinDigest = digest, pinStatus = status }
        Nothing -> pin
      contract = case lookup ContractSubject current of
        Just digest -> ContractPin digest status
        Nothing -> recordContract record
  parseReviewRecord (renderReviewRecord record { recordPins = map update (recordPins record), recordContract = contract })
