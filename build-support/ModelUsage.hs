{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Applicative ((<|>))
import Control.Exception (IOException, try)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (FromJSON (parseJSON), eitherDecodeStrict', withArray, withObject, withText, (.:?), (.!=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.ByteString.Lazy.Char8 as BLC
import Data.Char (isAlphaNum, toLower)
import Data.Foldable (toList)
import Data.List (intercalate, isInfixOf, isPrefixOf, sort)
import qualified Data.Set as Set
import qualified Data.Text as T
import Numeric (showFFloat)
import Data.Time (UTCTime, defaultTimeLocale, formatTime, parseTimeM)
import System.Directory (doesDirectoryExist, doesFileExist, listDirectory)
import System.Environment (getArgs)
import System.Exit (exitFailure, exitSuccess)
import System.FilePath ((</>), dropExtension, takeExtension)
import System.IO (hPutStrLn, stderr)

data Scope = ParentScope | ChildScope deriving (Eq, Ord, Show)

data Usage = Usage
  { uInput :: !Integer
  , uOutput :: !Integer
  , uCacheRead :: !Integer
  , uCacheWrite :: !Integer
  }
  deriving (Eq, Show)

instance FromJSON Usage where
  parseJSON = withObject "usage" $ \o ->
    Usage
      <$> o .:? "input" .!= 0
      <*> o .:? "output" .!= 0
      <*> o .:? "cacheRead" .!= 0
      <*> o .:? "cacheWrite" .!= 0

instance Semigroup Usage where
  a <> b = Usage (uInput a + uInput b) (uOutput a + uOutput b) (uCacheRead a + uCacheRead b) (uCacheWrite a + uCacheWrite b)

instance Monoid Usage where
  mempty = Usage 0 0 0 0

usageTotal :: Usage -> Integer
usageTotal usage = uInput usage + uOutput usage + uCacheRead usage + uCacheWrite usage

data Part = Part (Maybe String)

instance FromJSON Part where
  parseJSON = withObject "content part" $ \o -> Part <$> o .:? "text"

data Content = CText String | CParts [Part]

instance FromJSON Content where
  parseJSON value =
    withText "content" (pure . CText . T.unpack) value
      <|> withArray "content" (fmap (CParts . toList) . traverse parseJSON) value

contentText :: Content -> String
contentText (CText text) = text
contentText (CParts parts) = unwords [text | Part (Just text) <- parts]

data Message = Message
  { mRole :: !String
  , mUsage :: Maybe Usage
  , mContent :: Maybe Content
  , mResponseId :: Maybe String
  , mAttribution :: Maybe String
  }

instance FromJSON Message where
  parseJSON = withObject "message" $ \o ->
    Message
      <$> o .:? "role" .!= ""
      <*> o .:? "usage"
      <*> o .:? "content"
      <*> o .:? "responseId"
      <*> o .:? "attribution"

data Record = Record
  { rType :: !String
  , rTimestamp :: Maybe String
  , rId :: Maybe String
  , rMessage :: Maybe Message
  }

instance FromJSON Record where
  parseJSON = withObject "record" $ \o ->
    Record
      <$> o .:? "type" .!= ""
      <*> o .:? "timestamp"
      <*> o .:? "id"
      <*> o .:? "message"

data Event = Event
  { evTime :: Maybe UTCTime
  , evScope :: Scope
  , evUsage :: Maybe Usage
  , evGrant :: Bool
  }

data Totals = Totals
  { tMessages :: !Integer
  , tUsage :: !Usage
  , tGrants :: !Integer
  }

instance Semigroup Totals where
  a <> b = Totals (tMessages a + tMessages b) (tUsage a <> tUsage b) (tGrants a + tGrants b)

instance Monoid Totals where
  mempty = Totals 0 mempty 0

addEvent :: Totals -> Event -> Totals
addEvent totals event =
  Totals
    { tMessages = tMessages totals + maybe 0 (const 1) (evUsage event)
    , tUsage = maybe (tUsage totals) (tUsage totals <>) (evUsage event)
    , tGrants = tGrants totals + if evGrant event then 1 else 0
    }

data Coverage = Coverage
  { covSessions :: !Integer
  , covTranscripts :: !Integer
  , covUnreadable :: !Integer
  , covSkippedLines :: !Integer
  , covAssistantWithoutUsage :: !Integer
  , covAuxiliaryUsage :: !Integer
  , covDuplicates :: !Integer
  , covWithoutTimestamp :: !Integer
  , covOutsideWindow :: !Integer
  }

emptyCoverage :: Coverage
emptyCoverage = Coverage 0 0 0 0 0 0 0 0 0

data Options = Options
  { optSince :: Maybe String
  , optUntil :: Maybe String
  , optIncrements :: Maybe Int
  , optFiles :: [FilePath]
  }

usageText :: String
usageText =
  unlines
    [ "usage: model-usage [--since ISO8601] [--until ISO8601] [--increments N] SESSION.jsonl..."
    , ""
    , "Reads each top-level omp session and every *.jsonl transcript under the sibling"
    , "directory named after the session file (recursively), and prints one JSON object"
    , "followed by a short text summary."
    , ""
    , "Usage accounting counts each assistant message's usage once, keyed by responseId"
    , "across every transcript of a run (falling back to the record id within one"
    , "transcript), so a resumed session that re-persists an earlier conversation is not"
    , "counted twice. Aborted and failed"
    , "attempts that report usage are counted. model_usage records are reported as"
    , "auxiliary usage and are never added to the totals. Totals are the sum of input,"
    , "output, cacheRead and cacheWrite; the reported totalTokens field is ignored."
    , ""
    , "Grant rule: a message counts when it is user-authored (attribution \"user\") or a"
    , "parent-to-child IRC message (contains \"ircfromparent\"), it is not a session"
    , "digest or an assignment, and it carries an affirmative grant phrase within 64"
    , "characters of a host-execution word. Grant phrases are grant, granted, grants,"
    , "goahead, youmayrun, youmayproceed, youcanrun, youcanproceed, permissiontorun,"
    , "authorizedtorun, approvedtorun, clearedtorun, youarecleared, youareauthorized."
    , "Host-execution words are host, build, bazel, emulator, compute, compile, run,"
    , "test, sdk, slot, native, devenv, flash, camera, lane. A grant word preceded"
    , "within 32 characters by no, not, until, without, pending, revok, will, ill,"
    , "await, before, prior, within, closed, never, cannot, dont, donot, nonext or"
    , "nofurther, or followed within 24 characters by revok or closed, does not count,"
    , "and a message that revokes grants (grantrevoked or grantsrevoked) never counts."
    , "Text is lowercased and stripped to alphanumerics before matching, because some"
    , "persisted IRC messages have lost their whitespace."
    , ""
    , "--since and --until accept YYYY-MM-DD or ISO8601 date-times; both bounds are"
    , "inclusive. --increments N divides the combined totals by N: it reports tokens per"
    , "increment and grant messages per increment, where an increment is a unit of"
    , "delivered work, not a time slice. No per-bucket breakdown is produced."
    ]

main :: IO ()
main = do
  arguments <- getArgs
  when ("--help" `elem` arguments || "-h" `elem` arguments) (putStr usageText >> exitSuccess)
  options <- case parseOptions arguments of
    Left message -> dieWith message
    Right parsed -> pure parsed
  when (null (optFiles options)) (dieWith "no session files given")
  since <- resolveBound "--since" (optSince options)
  untilBound <- resolveBound "--until" (optUntil options)
  forM_ (optIncrements options) $ \count -> unless (count > 0) (dieWith "--increments must be positive")
  (events, coverage) <- collect options
  let inWindow event = case evTime event of
        Nothing -> True
        Just time -> maybe True (time >=) since && maybe True (time <=) untilBound
      windowed = filter inWindow events
      coverage' = coverage {covOutsideWindow = fromIntegral (length events - length windowed)}
      parentTotals = foldl' addEvent mempty (filter ((== ParentScope) . evScope) windowed)
      childTotals = foldl' addEvent mempty (filter ((== ChildScope) . evScope) windowed)
      combined = parentTotals <> childTotals
      stamps = [time | Just time <- map evTime windowed]
      (start, end) = bounds since untilBound stamps startOfEpoch
  BLC.putStrLn (encodeJson (report since untilBound (optIncrements options) (start, end) parentTotals childTotals combined coverage' (optFiles options)))
  putStr (summary (optFiles options) since untilBound parentTotals childTotals combined (optIncrements options) coverage')

startOfEpoch :: UTCTime
startOfEpoch = either (error "unreachable: epoch") id (parseTimestamp "1970-01-01")

bounds :: Maybe UTCTime -> Maybe UTCTime -> [UTCTime] -> UTCTime -> (UTCTime, UTCTime)
bounds since untilBound stamps fallback =
  (maybe (edge minimum) id since, maybe (edge maximum) id untilBound)
 where
  edge pick = case stamps of
    [] -> fallback
    _ -> pick stamps

resolveBound :: String -> Maybe String -> IO (Maybe UTCTime)
resolveBound name value = case value of
  Nothing -> pure Nothing
  Just text -> case parseTimestamp text of
    Left message -> dieWith (name ++ ": " ++ message)
    Right time -> pure (Just time)

dieWith :: String -> IO a
dieWith message = hPutStrLn stderr ("model-usage: " ++ message) >> exitFailure

parseOptions :: [String] -> Either String Options
parseOptions = go (Options Nothing Nothing Nothing [])
 where
  go options [] = Right options {optFiles = reverse (optFiles options)}
  go options (argument : rest)
    | argument == "--since" = withValue rest (\value more -> go options {optSince = Just value} more)
    | argument == "--until" = withValue rest (\value more -> go options {optUntil = Just value} more)
    | argument == "--increments" = withValue rest (setIncrements options)
    | "--since=" `isPrefixOf` argument = go options {optSince = Just (drop 8 argument)} rest
    | "--until=" `isPrefixOf` argument = go options {optUntil = Just (drop 8 argument)} rest
    | "--increments=" `isPrefixOf` argument = setIncrements options (drop 13 argument) rest
    | "-" `isPrefixOf` argument = Left ("unknown option " ++ argument)
    | otherwise = go options {optFiles = argument : optFiles options} rest
  withValue rest continue = case rest of
    value : more -> continue value more
    [] -> Left "option needs a value"
  setIncrements options value more = case reads value of
    [(count, "")] -> go options {optIncrements = Just count} more
    _ -> Left ("--increments needs an integer, got " ++ show value)

parseTimestamp :: String -> Either String UTCTime
parseTimestamp text = case [time | format <- formats, Just time <- [parseTimeM True defaultTimeLocale format text]] of
  time : _ -> Right time
  [] -> Left ("cannot parse timestamp " ++ show text)
 where
  formats = ["%Y-%m-%dT%H:%M:%S%QZ", "%Y-%m-%dT%H:%M:%S%Q%Ez", "%Y-%m-%dT%H:%M:%S%Q", "%Y-%m-%d"]

formatTimestamp :: UTCTime -> String
formatTimestamp = formatTime defaultTimeLocale "%Y-%m-%dT%H:%M:%SZ"

collect :: Options -> IO ([Event], Coverage)
collect options = loop (emptyCoverage {covSessions = fromIntegral (length (optFiles options))}) Set.empty [] (optFiles options)
 where
  loop coverage _seen events [] = pure (events, coverage)
  loop coverage seen events (file : rest) = do
    exists <- doesFileExist file
    if not exists
      then do
        hPutStrLn stderr ("model-usage: cannot read " ++ file)
        loop coverage {covUnreadable = covUnreadable coverage + 1} seen events rest
      else do
        (coverage1, seen1, events1) <- readTranscript coverage seen events ParentScope file
        children <- transcriptFiles (dropExtension file)
        (coverage2, seen2, events2) <- foldChildren (coverage1, seen1, events1) children
        loop coverage2 seen2 events2 rest
  foldChildren initial [] = pure initial
  foldChildren (coverage, seen, events) (child : more) = do
    (coverage', seen', events') <- readTranscript coverage seen events ChildScope child
    foldChildren (coverage', seen', events') more

transcriptFiles :: FilePath -> IO [FilePath]
transcriptFiles directory = do
  exists <- doesDirectoryExist directory
  if not exists
    then pure []
    else do
      entries <- try (listDirectory directory) :: IO (Either IOException [FilePath])
      case entries of
        Left _ -> pure []
        Right names -> do
          found <- forM (sort names) $ \name -> do
            let path = directory </> name
            isDirectory <- doesDirectoryExist path
            if isDirectory
              then transcriptFiles path
              else pure [path | takeExtension path == ".jsonl"]
          pure (concat found)

readTranscript :: Coverage -> Set.Set String -> [Event] -> Scope -> FilePath -> IO (Coverage, Set.Set String, [Event])
readTranscript coverage seen events scope file = do
  contents <- try (BLC.readFile file) :: IO (Either IOException BLC.ByteString)
  case contents of
    Left _ -> pure (counted {covUnreadable = covUnreadable counted + 1}, seen, events)
    Right bytes -> pure (foldl' (step file scope) (counted, seen, events) (BLC.lines bytes))
   where
    counted = coverage {covTranscripts = covTranscripts coverage + 1}

step :: FilePath -> Scope -> (Coverage, Set.Set String, [Event]) -> BLC.ByteString -> (Coverage, Set.Set String, [Event])
step path scope state@(coverage, seen, events) line
  | B.null strict = state
  | not (BC.isInfixOf "\"message\"" strict || BC.isInfixOf "\"model_usage\"" strict) = state
  | otherwise = case eitherDecodeStrict' strict of
      Left _ -> (coverage {covSkippedLines = covSkippedLines coverage + 1}, seen, events)
      Right record -> interpret path scope record state
 where
  strict = BLC.toStrict line

interpret :: FilePath -> Scope -> Record -> (Coverage, Set.Set String, [Event]) -> (Coverage, Set.Set String, [Event])
interpret path scope record (coverage, seen, events)
  | rType record == "model_usage" = (coverage {covAuxiliaryUsage = covAuxiliaryUsage coverage + 1}, seen, events)
  | rType record /= "message" = state
  | otherwise = case rMessage record of
      Nothing -> (coverage {covSkippedLines = covSkippedLines coverage + 1}, seen, events)
      Just message
        | mRole message == "assistant" -> case mUsage message of
            Nothing -> (coverage {covAssistantWithoutUsage = covAssistantWithoutUsage coverage + 1}, seen, events)
            Just usage ->
              let key = case mResponseId message of
                    Just responseId -> "r:" ++ responseId
                    Nothing -> "i:" ++ path ++ "\0" ++ maybe "" id (rId record)
                  time = rTimestamp record >>= parseTimestamp'
               in if Set.member key seen
                    then (coverage {covDuplicates = covDuplicates coverage + 1}, seen, events)
                    else
                      ( coverage {covWithoutTimestamp = covWithoutTimestamp coverage + if time == Nothing then 1 else 0}
                      , Set.insert key seen
                      , Event time scope (Just usage) False : events
                      )
        | mRole message == "user" && isGrantMessage (mAttribution message) (maybe "" contentText (mContent message)) ->
            let time = rTimestamp record >>= parseTimestamp'
             in (coverage, seen, Event time scope Nothing True : events)
        | otherwise -> state
 where
  state = (coverage, seen, events)

parseTimestamp' :: String -> Maybe UTCTime
parseTimestamp' text = either (const Nothing) Just (parseTimestamp text)

isGrantMessage :: Maybe String -> String -> Bool
isGrantMessage attribution raw
  | not eligible = False
  | revokes = False
  | otherwise = grantWord || grantPhrase
 where
  normalized = normalize raw
  revokes = "grantrevoked" `isInfixOf` normalized || "grantsrevoked" `isInfixOf` normalized
  eligible =
    (attribution == Just "user" || "ircfromparent" `isInfixOf` normalized)
      && not (any (`isPrefixOf` normalized) ["sessionupdate", "completeassignment", "asyncresult", "budgetnotice"])
  grantWord = any affirmative (occurrences "grant" normalized)
  affirmative index =
    let before = slice (index - 32) index normalized
        after = slice (index + 5) (index + 29) normalized
        near = slice (index - 64) (index + 64) normalized
     in not (negated before)
          && not (any (`isInfixOf` after) ["revok", "closed"])
          && any (`isInfixOf` near) hostWords
  grantPhrase = any phraseHit [index | phrase <- phrases, index <- occurrences phrase normalized]
  phrases = ["goahead", "youmayrun", "youmayproceed", "youcanrun", "youcanproceed", "permissiontorun", "authorizedtorun", "approvedtorun", "clearedtorun", "youarecleared", "youareauthorized"]
  phraseHit index =
    let before = slice (index - 32) index normalized
        near = slice (index - 64) (index + 64) normalized
     in not (negated before) && any (`isInfixOf` near) hostWords

negated :: String -> Bool
negated before = any (`isInfixOf` before) negationWords

negationWords :: [String]
negationWords = ["no", "not", "until", "without", "pending", "revok", "will", "ill", "await", "before", "prior", "within", "closed", "never", "cannot", "dont", "donot", "nonext", "nofurther"]

hostWords :: [String]
hostWords = ["host", "build", "bazel", "emulator", "compute", "compile", "run", "test", "sdk", "slot", "native", "devenv", "flash", "camera", "lane"]

normalize :: String -> String
normalize = filter isAlphaNum . map toLower

occurrences :: String -> String -> [Int]
occurrences needle haystack = [index | (index, suffix) <- zip [0 ..] (tailsOf haystack), needle `isPrefixOf` suffix]
 where
  tailsOf [] = [[]]
  tailsOf text@(_ : rest) = text : tailsOf rest

slice :: Int -> Int -> String -> String
slice from to text = take (max 0 (to - low)) (drop low text)
 where
  low = max 0 from

data J
  = JStr String
  | JInt Integer
  | JDec Double
  | JBool Bool
  | JArr [J]
  | JObj [(String, J)]

render :: Int -> J -> String
render _ (JStr text) = jsonString text
render _ (JInt number) = show number
render _ (JDec value) = showFFloat (Just 2) value ""
render _ (JBool value) = if value then "true" else "false"
render _ (JArr []) = "[]"
render depth (JArr values) = "[\n" ++ intercalate ",\n" [indent (depth + 1) ++ render (depth + 1) value | value <- values] ++ "\n" ++ indent depth ++ "]"
render _ (JObj []) = "{}"
render depth (JObj fields) = "{\n" ++ intercalate ",\n" [indent (depth + 1) ++ jsonString name ++ ": " ++ render (depth + 1) value | (name, value) <- fields] ++ "\n" ++ indent depth ++ "}"

indent :: Int -> String
indent depth = replicate (depth * 2) ' '

jsonString :: String -> String
jsonString text = "\"" ++ concatMap escape text ++ "\""
 where
  escape character = case character of
    '"' -> "\\\""
    '\\' -> "\\\\"
    '\n' -> "\\n"
    '\r' -> "\\r"
    '\t' -> "\\t"
    _ | fromEnum character < 0x20 -> "\\u" ++ replicate (4 - length hex) '0' ++ hex
      | otherwise -> [character]
     where
      hex = toHex (fromEnum character)
  toHex 0 = "0"
  toHex number = go number ""
   where
    go 0 acc = if null acc then "0" else acc
    go value acc = go (value `div` 16) (hexDigit (value `mod` 16) : acc)
    hexDigit value = "0123456789abcdef" !! value

usageJson :: Usage -> J
usageJson usage =
  JObj
    [ ("input", JInt (uInput usage))
    , ("output", JInt (uOutput usage))
    , ("cacheRead", JInt (uCacheRead usage))
    , ("cacheWrite", JInt (uCacheWrite usage))
    , ("total", JInt (usageTotal usage))
    ]

totalsJson :: Totals -> J
totalsJson totals =
  JObj
    [ ("assistantMessages", JInt (tMessages totals))
    , ("usage", usageJson (tUsage totals))
    , ("grants", JInt (tGrants totals))
    ]

report :: Maybe UTCTime -> Maybe UTCTime -> Maybe Int -> (UTCTime, UTCTime) -> Totals -> Totals -> Totals -> Coverage -> [FilePath] -> J
report since untilBound increments (start, end) parentTotals childTotals combined coverage files =
  JObj
    [ ("schemaVersion", JInt 1)
    , ("sessions", JArr (map JStr files))
    , ( "window"
      , JObj
          [ ("since", JStr (maybe "" formatTimestamp since))
          , ("until", JStr (maybe "" formatTimestamp untilBound))
          , ("first", JStr (formatTimestamp start))
          , ("last", JStr (formatTimestamp end))
          , ("applied", JBool (since /= Nothing || untilBound /= Nothing))
          ]
      )
    , ("parent", totalsJson parentTotals)
    , ("children", totalsJson childTotals)
    , ("combined", totalsJson combined)
    , ("increments", incrementsJson increments combined)
    , ( "coverage"
      , JObj
          [ ("sessions", JInt (covSessions coverage))
          , ("transcriptsRead", JInt (covTranscripts coverage))
          , ("unreadableTranscripts", JInt (covUnreadable coverage))
          , ("malformedLinesSkipped", JInt (covSkippedLines coverage))
          , ("assistantMessagesWithoutUsage", JInt (covAssistantWithoutUsage coverage))
          , ("auxiliaryUsageRecordsIgnored", JInt (covAuxiliaryUsage coverage))
          , ("duplicateRecordsDropped", JInt (covDuplicates coverage))
          , ("recordsWithoutTimestamp", JInt (covWithoutTimestamp coverage))
          , ("recordsOutsideWindow", JInt (covOutsideWindow coverage))
          ]
      )
    ]

encodeJson :: J -> BLC.ByteString
encodeJson = BLC.pack . render 0

incrementsJson :: Maybe Int -> Totals -> J
incrementsJson Nothing _ = JObj []
incrementsJson (Just count) combined =
  JObj
    [ ("requested", JInt (fromIntegral count))
    , ("tokensPerIncrement", JDec (perIncrement (usageTotal (tUsage combined)) count))
    , ("grantsPerIncrement", JDec (perIncrement (tGrants combined) count))
    ]

perIncrement :: Integer -> Int -> Double
perIncrement total count = fromIntegral total / fromIntegral (max 1 count)

incrementsText :: Maybe Int -> Totals -> [String]
incrementsText Nothing _ = []
incrementsText (Just count) combined =
  [ "increments " ++ show count ++ ": " ++ decimal (perIncrement (usageTotal (tUsage combined)) count) ++ " tokens per increment, " ++ decimal (perIncrement (tGrants combined) count) ++ " grants per increment"
  ]

decimal :: Double -> String
decimal value = showFFloat (Just 2) value ""

summary :: [FilePath] -> Maybe UTCTime -> Maybe UTCTime -> Totals -> Totals -> Totals -> Maybe Int -> Coverage -> String
summary files since untilBound parentTotals childTotals combined increments coverage =
  unlines $
    [ "model-usage: " ++ show (length files) ++ " session(s), " ++ show (covTranscripts coverage) ++ " transcript(s)"
    , "window: " ++ windowText
    , "parent   " ++ totalsText parentTotals
    , "children " ++ totalsText childTotals
    , "combined " ++ totalsText combined
    , "grants   " ++ show (tGrants combined) ++ " (parent " ++ show (tGrants parentTotals) ++ ", children " ++ show (tGrants childTotals) ++ ")"
    ]
      ++ incrementsText increments combined
      ++ [ "coverage: skipped " ++ show (covSkippedLines coverage) ++ " malformed line(s), " ++ show (covAssistantWithoutUsage coverage) ++ " assistant message(s) without usage, " ++ show (covAuxiliaryUsage coverage) ++ " model_usage record(s) ignored, " ++ show (covDuplicates coverage) ++ " duplicate record(s) dropped, " ++ show (covOutsideWindow coverage) ++ " record(s) outside window, " ++ show (covUnreadable coverage) ++ " unreadable transcript(s)"
         ]
 where
  windowText = maybe "none" formatTimestamp since ++ " .. " ++ maybe "now" formatTimestamp untilBound
  totalsText totals = show (tMessages totals) ++ " assistant messages  in " ++ show (uInput (tUsage totals)) ++ " out " ++ show (uOutput (tUsage totals)) ++ " cacheRead " ++ show (uCacheRead (tUsage totals)) ++ " cacheWrite " ++ show (uCacheWrite (tUsage totals)) ++ " total " ++ show (usageTotal (tUsage totals))
