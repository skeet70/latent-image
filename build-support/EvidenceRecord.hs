{-# LANGUAGE OverloadedStrings #-}
module EvidenceRecord (run) where

import Control.Exception (throwIO)
import Control.Monad (forM, forM_, unless, when)
import Crypto.Hash.SHA256 qualified as SHA
import Data.Aeson
import Data.Aeson.Key qualified as K
import Data.Aeson.KeyMap qualified as KM
import Data.Aeson.Types (parseEither)
import Data.ByteString qualified as BS
import Data.ByteString.Lazy qualified as BL
import Data.List (nub, sort)
import Data.Text qualified as T
import Data.Text.Encoding qualified as TE
import Data.Time (UTCTime, diffUTCTime)
import Data.Vector qualified as V
import Numeric (showHex)
import System.FilePath ((</>), isAbsolute, splitDirectories)
import System.IO (IOMode (ReadMode), hFileSize, withBinaryFile)
import System.Directory (doesFileExist)
import Text.Read (readMaybe)

newtype RecordId = RecordId T.Text deriving (Eq, Show)
newtype ArtifactPath = ArtifactPath FilePath deriving (Eq, Ord, Show)
newtype Sha256 = Sha256 T.Text deriving Show
newtype GitCommit = GitCommit T.Text deriving Show
newtype RunLabel = RunLabel T.Text deriving Show
instance FromJSON RunLabel where
  parseJSON = withText "run label" $ \t -> if not (T.null t) && T.all (\c -> c `elem` ("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-" :: String)) t then pure (RunLabel t) else fail "invalid run label"
instance ToJSON RunLabel where toJSON (RunLabel t) = String t
data Outcome = Passed | Failed | Unjudged deriving Show

instance FromJSON Sha256 where
  parseJSON = withText "SHA-256" $ \t -> if hexText 64 t then pure (Sha256 t) else fail "expected lowercase SHA-256"
instance ToJSON Sha256 where toJSON (Sha256 t) = String t
instance FromJSON GitCommit where
  parseJSON = withText "commit" $ \t -> if hexText 40 t then pure (GitCommit t) else fail "expected full lowercase Git commit"
instance ToJSON GitCommit where toJSON (GitCommit t) = String t
instance FromJSON Outcome where
  parseJSON = withText "closed run outcome" $ \t -> case t of
    "passed" -> pure Passed
    "failed" -> pure Failed
    "unjudged" -> pure Unjudged
    _ -> fail "expected closed run outcome"
instance ToJSON Outcome where
  toJSON outcome = String $ case outcome of Passed -> "passed"; Failed -> "failed"; Unjudged -> "unjudged"

hexText :: Int -> T.Text -> Bool
hexText n t = T.length t == n && T.all (\c -> c `elem` ("0123456789abcdef" :: String)) t

data Operation = Identity | Length | DecimalBetween T.Text T.Text deriving Show
data Fact = Fact T.Text ArtifactPath [T.Text] Operation deriving Show
data Recipe = Recipe RecordId T.Text T.Text T.Text [ArtifactPath] [Fact] deriving Show
data Mode = Write | Check deriving Eq

instance FromJSON ArtifactPath where
  parseJSON = withText "relative artifact path" $ \t -> do
    let p = T.unpack t
    unless (not (null p) && not (isAbsolute p) && all (`notElem` ["..", "."]) (splitDirectories p)) $ fail "artifact path must be workspace-relative"
    pure (ArtifactPath p)
instance FromJSON Fact where
  parseJSON = withObject "fact" $ \o -> do
    op <- o .:? "operation" .!= ("value" :: T.Text)
    operation <- case op of
      "value" -> pure Identity
      "length" -> pure Length
      "decimal-between" -> DecimalBetween <$> o .: "prefix" <*> o .: "suffix"
      _ -> fail "unknown fact operation"
    Fact <$> o .: "name" <*> o .: "path" <*> o .:? "pointer" .!= [] <*> pure operation
instance FromJSON Recipe where
  parseJSON = withObject "recipe" $ \o -> Recipe . RecordId <$> o .: "id" <*> o .: "claim" <*> o .: "scope" <*> o .: "notes" <*> o .: "artifacts" <*> o .: "facts"

failure :: String -> IO a
failure = throwIO . userError

readBounded :: FilePath -> IO BS.ByteString
readBounded p = do
  bytes <- withBinaryFile p ReadMode $ \h -> do
    size <- hFileSize h
    when (size > 16 * 1024 * 1024) $ failure (p <> ": input exceeds 16 MiB")
    BS.hGet h (16 * 1024 * 1024 + 1)
  when (BS.length bytes > 16 * 1024 * 1024) $ failure (p <> ": input exceeds 16 MiB")
  pure bytes

jsonFile :: FromJSON a => FilePath -> IO a
jsonFile p = do
  bytes <- readBounded p
  either (failure . ((p <> ": ") <>)) pure (eitherDecodeStrict' bytes)

hashFile :: FilePath -> IO T.Text
hashFile p = withBinaryFile p ReadMode $ \h -> do
  let loop ctx = do
        chunk <- BS.hGetSome h (1024 * 1024)
        if BS.null chunk then pure (SHA.finalize ctx) else loop (SHA.update ctx chunk)
  digest <- loop SHA.init
  pure $ T.pack $ concatMap (\b -> let s = showHex b "" in if length s == 1 then '0':s else s) (BS.unpack digest)

field :: FromJSON a => T.Text -> Value -> IO a
field k v = either failure pure $ parseEither (withObject "object" (.: K.fromText k)) v

select :: [T.Text] -> Value -> Either String Value
select [] v = Right v
select (key:rest) (Object o) = maybe (Left ("missing field " <> T.unpack key)) (select rest) (KM.lookup (K.fromText key) o)
select (key:rest) (Array a) = case readMaybe (T.unpack key) of
  Just i | i >= 0 && i < V.length a -> select rest (a V.! i)
  _ -> Left "invalid array index"
select _ _ = Left "pointer enters a scalar"

factValue :: FilePath -> Fact -> IO (K.Key, Value)
factValue root (Fact name (ArtifactPath p) pointer op) = do
  result <- case op of
    DecimalBetween prefix suffix -> do
      unless (null pointer && not (T.null prefix) && not (T.null suffix)) $ failure "decimal-between requires nonempty delimiters and no pointer"
      text <- TE.decodeUtf8 <$> readBounded (root </> p)
      unless (T.count prefix text == 1) $ failure (p <> ": expected one numeric summary")
      let (_, start) = T.breakOn prefix text
          (number, end) = T.breakOn suffix (T.drop (T.length prefix) start)
      case readMaybe (T.unpack number) :: Maybe Integer of
        Just n | n >= 0 && not (T.null end) -> pure (toJSON n)
        _ -> failure (p <> ": invalid numeric summary")
    _ -> do
      input <- jsonFile (root </> p)
      value <- either (failure . ((p <> ": ") <>)) pure (select pointer input)
      case (op, value) of
        (Identity, _) -> pure value
        (Length, Array a) -> pure (toJSON (V.length a))
        (Length, Object o) -> pure (toJSON (KM.size o))
        _ -> failure (p <> ": length requires array or object")
  pure (K.fromText name, result)

render :: KM.KeyMap Value -> T.Text -> IO T.Text
render values template = case T.breakOn "{{" template of
  (prefix, suffix) | T.null suffix -> pure prefix
  (prefix, suffix) -> do
    let (name, closing) = T.breakOn "}}" (T.drop 2 suffix)
    when (T.null closing) $ failure "unterminated template reference"
    value <- maybe (failure ("unknown template reference " <> T.unpack name)) pure (KM.lookup (K.fromText name) values)
    let text = case value of String s -> s; _ -> TE.decodeUtf8 (BL.toStrict (encode value))
    remaining <- render values (T.drop 2 closing)
    pure (prefix <> text <> remaining)

runDirectory :: ArtifactPath -> Maybe FilePath
runDirectory (ArtifactPath p) = go [] (splitDirectories p)
  where
    go prefix ("runs":stamp:_) = Just (foldl (</>) "" (prefix <> ["runs",stamp]))
    go prefix (x:xs) = go (prefix <> [x]) xs
    go _ [] = Nothing

regenerate :: FilePath -> Recipe -> Value -> IO Value
regenerate root (Recipe _ claim scope notes artifacts facts) old = do
  unless (length facts == length (nub [n | Fact n _ _ _ <- facts])) $ failure "duplicate fact name"
  let paths = sort $ nub (artifacts <> [p | Fact _ p _ _ <- facts])
      runs = sort $ nub [p | Just p <- map runDirectory paths]
  runFacts <- forM runs $ \p -> do
    metadata <- jsonFile (root </> p </> "run.json")
    commit <- field "commit" metadata :: IO GitCommit
    outcome <- field "outcome" metadata :: IO Outcome
    opened <- field "opened_at" metadata :: IO UTCTime
    closed <- field "closed_at" metadata :: IO UTCTime
    when (closed < opened) $ failure (p <> ": run closes before opening")
    exitStatus <- field "exit_status" metadata :: IO (Maybe Int)
    command <- field "command" metadata :: IO [T.Text]
    label <- field "label" metadata :: IO RunLabel
    let verdictPath = p </> "result/verdict.json"
    exists <- doesFileExist (root </> verdictPath)
    verdict <- if exists then Just <$> (jsonFile (root </> verdictPath) :: IO Value) else pure Nothing
    pure (object ["path" .= p, "commit" .= commit, "label" .= label, "outcome" .= outcome, "exit_status" .= exitStatus, "elapsed_seconds" .= (realToFrac (diffUTCTime closed opened) :: Double), "opened_at" .= opened, "closed_at" .= closed, "command" .= command, "verdict" .= verdict], [ArtifactPath (p </> "run.json")] <> [ArtifactPath verdictPath | exists])
  let allPaths = sort $ nub (paths <> concatMap snd runFacts)
  hashes <- forM allPaths $ \(ArtifactPath p) -> do
    digest <- hashFile (root </> p)
    pure (object ["path" .= p, "sha256" .= digest])
  values <- KM.fromList <$> mapM (factValue root) facts
  claimText <- render values claim
  scopeText <- render values scope
  closedTimes <- mapM (field "closed_at" . fst) runFacts :: IO [UTCTime]
  firmwareValue <- maybe (failure "recipe needs a firmware_sha256 fact") pure (KM.lookup "firmware_sha256" values)
  firmware <- either failure pure (parseEither parseJSON firmwareValue) :: IO Sha256
  newest <- case closedTimes of [] -> failure "recipe needs a cited run"; _ -> pure (maximum closedTimes)
  commands <- forM runFacts $ \(v,_) -> do
    RunLabel label <- field "label" v
    argv <- field "command" v
    pure ("run-output exec --label " <> shellQuote label <> " -- " <> T.intercalate " " (map shellQuote argv))
  case old of
    Object o -> pure $ Object $ foldl (\m (k,v) -> KM.insert k v m) o
      [("claim",String claimText),("scope",String scopeText),("limitations",toJSON [notes]),("artifacts",toJSON hashes),("figures",object ["facts" .= Object values,"runs" .= map fst runFacts]),("generated_by",String "//build-support:evidence-record"),("firmware_sha256",toJSON firmware),("recorded_at",toJSON newest),("commands",toJSON commands)]
    _ -> failure "ledger record is not an object"

shellQuote :: T.Text -> T.Text
shellQuote t = "'" <> T.replace "'" "'\\''" t <> "'"

run :: [String] -> IO ()
run args = case args of
  mode:root:recipePath:ledgerPath:rest | mode `elem` ["write","check","check-available"] && length rest <= 1 -> execute (if mode == "write" then Write else Check) (mode == "check-available") root recipePath ledgerPath rest
  _ -> failure "usage: evidence-record (write|check|check-available) WORKSPACE RECIPES_JSON LEDGER_JSONL [README]"

execute :: Mode -> Bool -> FilePath -> FilePath -> FilePath -> [FilePath] -> IO ()
execute mode available root recipePath ledgerPath readmes = do
  recipes <- jsonFile (root </> recipePath) :: IO [Recipe]
  let ids = [i | Recipe i _ _ _ _ _ <- recipes]
  unless (length ids == length (nub ids)) $ failure "duplicate recipe id"
  bytes <- BS.readFile (root </> ledgerPath)
  let linesIn = BS.split 10 bytes
  decoded <- forM (filter (not . BS.null) linesIn) $ \line -> either failure (\v -> pure (line,v)) (eitherDecodeStrict' line)
  forM_ recipes $ \(Recipe (RecordId ident) _ _ _ _ _) -> do
    matching <- filterMRecord ident decoded
    unless (length matching == 1) $ failure (T.unpack ident <> ": expected exactly one ledger record")
  result <- forM decoded $ \(line, old) -> do
    ident <- RecordId <$> field "id" old
    case [r | r@(Recipe i _ _ _ _ _) <- recipes, i == ident] of
      [] -> case old of
        Object fields | KM.member "generated_by" fields -> failure "generated ledger record has no recipe"
        _ -> pure line
      [recipe@(Recipe (RecordId name) _ _ _ paths facts)] -> do
        presence <- mapM (\(ArtifactPath p) -> doesFileExist (root </> p)) (nub (paths <> [p | Fact _ p _ _ <- facts]))
        if available && not (or presence)
          then putStrLn (T.unpack name <> ": unavailable, no cited artifacts present") >> pure line
          else do
            new <- regenerate root recipe old
            when (mode == Check && new /= old) $ failure (T.unpack name <> ": generated record differs; run evidence-record write")
            pure (BL.toStrict (encode new))
      _ -> failure "duplicate recipe"
  forM_ readmes $ \path -> do
    original <- TE.decodeUtf8 <$> BS.readFile (root </> path)
    checkReadmeRecipes original ids
    updated <- foldReadme original result ids
    when (mode == Check && updated /= original) $ failure (path <> ": generated README differs")
    when (mode == Write) $ BS.writeFile (root </> path) (TE.encodeUtf8 updated)
  when (mode == Write) $ BS.writeFile (root </> ledgerPath) (BS.intercalate "\n" result <> "\n")
  where
    filterMRecord ident = fmap concat . mapM (\pair@(_,v) -> do actual <- field "id" v; pure [pair | actual == ident])

checkReadmeRecipes :: T.Text -> [RecordId] -> IO ()
checkReadmeRecipes text ids = forM_ ["<!-- evidence-record ", "<!-- /evidence-record "] $ \prefix ->
  forM_ (drop 1 (T.splitOn prefix text)) $ \chunk -> do
    let (name, ending) = T.breakOn " -->" chunk
    when (T.null ending || RecordId name `notElem` ids) $ failure "generated README block has no recipe"

foldReadme :: T.Text -> [BS.ByteString] -> [RecordId] -> IO T.Text
foldReadme text _ [] = pure text
foldReadme text records (RecordId ident:rest) = do
  let start = "<!-- evidence-record " <> ident <> " -->"
      end = "<!-- /evidence-record " <> ident <> " -->"
      (before, suffix) = T.breakOn start text
      (_, after) = T.breakOn end (T.drop (T.length start) suffix)
  when (T.null suffix || T.null after || T.count start text /= 1 || T.count end text /= 1) $ failure ("expected one README marker pair for " <> T.unpack ident)
  values <- mapM (either failure pure . eitherDecodeStrict') records
  matching <- fmap concat $ forM values $ \v -> do
    name <- field "id" v
    pure [v | name == ident]
  record <- case matching of [v] -> pure v; _ -> failure "missing README record"
  claim <- field "claim" record
  scope <- field "scope" record
  let body = start <> "\n\n" <> ident <> ": " <> claim <> "\n\nScope: " <> scope <> "\n\n"
  foldReadme (before <> body <> after) records rest
