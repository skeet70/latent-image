{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module RustSourceBoundaryCheck (main, checkDeclaration, regressions, lexRust, Token (..)) where

import Control.Monad (forM_, unless)
import Data.Aeson (FromJSON (..), eitherDecodeStrict', withObject, (.:))
import qualified Data.ByteString as B
import Data.Char (isAlphaNum, isSpace)
import Data.List (isPrefixOf, sort, tails)
import qualified Data.Set as S
import System.Directory (doesDirectoryExist, doesFileExist, listDirectory)
import System.Environment (getArgs)
import System.FilePath (takeDirectory, takeExtension, (</>))
import System.IO (IOMode (ReadMode), hGetContents', hSetEncoding, utf8, withFile)

newtype SourcePath = SourcePath FilePath deriving (Eq, Ord, Show)
newtype Reason = Reason String deriving Show
newtype ItemName = ItemName String deriving Show
data ItemKind = Function | Implementation | Macro | Structure deriving Show
data BoundaryFile = BoundaryFile SourcePath Reason deriving Show
data BoundaryItem = BoundaryItem SourcePath ItemKind ItemName Reason deriving Show
data KnownException = KnownException SourcePath [Int] Reason deriving Show
data Declaration = Declaration [SourcePath] [BoundaryFile] [BoundaryItem] [KnownException] deriving Show
data Token = Token { spelling :: String, line :: Int } deriving (Eq, Show)
data Refusal = Refusal Int String deriving (Eq, Show)

instance FromJSON SourcePath where parseJSON value = SourcePath <$> parseJSON value
instance FromJSON Reason where
  parseJSON value = do
    reason <- parseJSON value
    unless (not (null reason)) (fail "boundary reason is empty")
    pure (Reason reason)
instance FromJSON ItemName where parseJSON value = ItemName <$> parseJSON value
instance FromJSON ItemKind where
  parseJSON value = do
    kind <- parseJSON value
    case (kind :: String) of
      "fn" -> pure Function
      "impl" -> pure Implementation
      "macro" -> pure Macro
      "struct" -> pure Structure
      _ -> fail "unknown boundary item kind"
instance FromJSON BoundaryFile where
  parseJSON = withObject "boundary file" $ \value -> BoundaryFile <$> value .: "path" <*> value .: "reason"
instance FromJSON BoundaryItem where
  parseJSON = withObject "boundary item" $ \value -> BoundaryItem <$> value .: "path" <*> value .: "kind" <*> value .: "name" <*> value .: "reason"
instance FromJSON KnownException where
  parseJSON = withObject "known internal unsafe" $ \value -> KnownException <$> value .: "path" <*> value .: "lines" <*> value .: "reason"
instance FromJSON Declaration where
  parseJSON = withObject "source boundary declaration" $ \value -> Declaration <$> value .: "source_roots" <*> value .: "boundary_files" <*> value .: "boundary_items" <*> value .: "known_exceptions"

lexRust :: String -> Either String [Token]
lexRust = go 1
  where
    go _ [] = Right []
    go at input@(character : rest)
      | "//" `isPrefixOf` input = go at (dropWhile (/= '\n') rest)
      | "/*" `isPrefixOf` input = block at 1 (drop 2 input) >>= uncurry go
      | character == '"' = quoted at '"' rest >>= uncurry go
      | character == '\'', Just after <- characterLiteral rest = go at after
      | "br" `isPrefixOf` input, Just (hashes, after) <- rawStart (drop 2 input) = raw at hashes after >>= uncurry go
      | character == 'r', Just (hashes, after) <- rawStart rest = raw at hashes after >>= uncurry go
      | isSpace character = go (if character == '\n' then at + 1 else at) rest
      | isAlphaNum character || character == '_' =
          let (word, after) = span (\c -> isAlphaNum c || c == '_') input
          in (Token word at :) <$> go at after
      | otherwise = (Token [character] at :) <$> go at rest
    block :: Int -> Int -> String -> Either String (Int, String)
    block _ _ [] = Left "unterminated block comment"
    block at depth input
      | "/*" `isPrefixOf` input = block at (depth + 1) (drop 2 input)
      | "*/" `isPrefixOf` input = if depth == 1 then Right (at, drop 2 input) else block at (depth - 1) (drop 2 input)
      | character : rest <- input = block (if character == '\n' then at + 1 else at) depth rest
    quoted _ _ [] = Left "unterminated string"
    quoted at delimiter (character : rest)
      | character == delimiter = Right (at, rest)
      | character == '\\', escaped : after <- rest = quoted (if escaped == '\n' then at + 1 else at) delimiter after
      | otherwise = quoted (if character == '\n' then at + 1 else at) delimiter rest
    characterLiteral ('\\' : _ : '\'' : rest) = Just rest
    characterLiteral ('\\' : 'x' : _ : _ : '\'' : rest) = Just rest
    characterLiteral ('\\' : 'u' : '{' : rest) = case dropWhile (/= '}') rest of
      '}' : '\'' : after -> Just after
      _ -> Nothing
    characterLiteral (_ : '\'' : rest) = Just rest
    characterLiteral _ = Nothing
    rawStart rest = let (hashes, after) = span (== '#') rest in case after of
      '"' : body -> Just (hashes, body)
      _ -> Nothing
    raw _ _ [] = Left "unterminated raw string"
    raw at hashes input
      | ('"' : hashes) `isPrefixOf` input = Right (at, drop (1 + length hashes) input)
      | character : rest <- input = raw (if character == '\n' then at + 1 else at) hashes rest

itemExtent :: [Token] -> ItemKind -> ItemName -> Either String (Int, Int)
itemExtent tokens kind (ItemName name) = case matches of
  [start] -> do
    finish <- case kind of
      Macro -> do
        opening <- maybe (Left "boundary macro has no delimiter") Right (findFrom (start + 2) ((`elem` ["{", "(", "["]) . spelling))
        closeDelimited opening [] (drop opening tokens)
      Structure -> do
        delimiter <- maybe (Left "boundary struct has no terminator") Right (findFrom start ((`elem` [";", "{"]) . spelling))
        if spelling (tokens !! delimiter) == ";" then pure delimiter else close delimiter 0 (drop delimiter tokens)
      Function -> do
        delimiter <- maybe (Left "boundary function has no terminator") Right (findFrom start ((`elem` [";", "{"]) . spelling))
        if spelling (tokens !! delimiter) == ";" then pure delimiter else close delimiter 0 (drop delimiter tokens)
      _ -> do
        opening <- maybe (Left "boundary item has no body") Right (findFrom start ((== "{") . spelling))
        close opening 0 (drop opening tokens)
    let beginning = 1 + maximum (-1 : [index | (index, token) <- zip [0 ..] (take start tokens), spelling token `elem` [";", "{", "}"]])
    pure (beginning, finish)
  _ -> Left ("boundary item must identify exactly one definition: " ++ name)
  where
    findFrom start predicate = case [index | (index, token) <- zip [start ..] (drop start tokens), predicate token] of
      index : _ -> Just index
      [] -> Nothing
    matches = [index | (index, token : rest) <- zip [0 ..] (tails tokens), case kind of
      Function -> spelling token == "fn" && take 1 (map spelling rest) == [name]
      Structure -> spelling token == "struct" && take 1 (map spelling rest) == [name]
      Macro -> spelling token == name && take 1 (map spelling rest) == ["!"]
      Implementation -> spelling token == "impl" && name `elem` takeWhile (`notElem` ["{", ";"]) (map spelling rest)]
    close :: Int -> Int -> [Token] -> Either String Int
    close _ _ [] = Left "boundary item has unbalanced braces"
    close index depth (token : rest) = case spelling token of
      "{" -> close (index + 1) (depth + 1) rest
      "}" | depth == 1 -> Right index
          | otherwise -> close (index + 1) (depth - 1) rest
      _ -> close (index + 1) depth rest
    closeDelimited _ _ [] = Left "boundary macro has unbalanced delimiters"
    closeDelimited index stack (token : rest)
      | Just ending <- lookup (spelling token) [("{", "}"), ("(", ")"), ("[", "]")] = closeDelimited (index + 1) (ending : stack) rest
      | ending : after <- stack, spelling token == ending = if null after then Right index else closeDelimited (index + 1) after rest
      | spelling token `elem` ["}", ")", "]"] = Left "boundary macro has mismatched delimiters"
      | otherwise = closeDelimited (index + 1) stack rest

checkSource :: Bool -> [(ItemKind, ItemName)] -> String -> Either String [Refusal]
checkSource whole items source = do
  tokens <- lexRust source
  extents <- mapM (uncurry (itemExtent tokens)) items
  pure [Refusal (line token) (spelling token) | (index, token) <- zip [0 ..] tokens
    , spelling token `elem` ["unsafe", "asm", "global_asm", "no_mangle", "export_name", "link_section"]
    , not whole, not (any (\(low, high) -> low <= index && index <= high) extents)]

sourceFiles :: FilePath -> IO [FilePath]
sourceFiles path = do
  directory <- doesDirectoryExist path
  if directory then listDirectory path >>= fmap concat . mapM (sourceFiles . (path </>))
  else pure [path | takeExtension path == ".rs"]

checkDeclaration :: FilePath -> IO ()
checkDeclaration manifest = do
  Declaration roots boundaries items exceptions <- either fail pure . eitherDecodeStrict' =<< B.readFile manifest
  let root = takeDirectory manifest
  paths <- concat <$> mapM (\(SourcePath path) -> sourceFiles (if path == "." then root else root </> path)) roots
  unless (not (null paths)) (fail (manifest ++ ": source declaration contains no Rust files"))
  let relative path = SourcePath (drop (length root + 1) path)
      available = S.fromList (map relative paths)
      declared = [path | BoundaryFile path _ <- boundaries] ++ [path | BoundaryItem path _ _ _ <- items] ++ [path | KnownException path _ _ <- exceptions]
  unless (all (`S.member` available) declared) (fail (manifest ++ ": boundary declaration names a missing source file"))
  forM_ paths $ \path -> do
    present <- doesFileExist path
    let input = if present then path else "bazel-bin" </> path
    source <- withFile input ReadMode (\handle -> hSetEncoding handle utf8 >> hGetContents' handle)
    let selected = [(kind, name) | BoundaryItem itemPath kind name _ <- items, itemPath == relative path]
        whole = any (\(BoundaryFile file _) -> file == relative path) boundaries
    refusals <- either (fail . ((path ++ ": ") ++)) pure (checkSource whole selected source)
    let known = concat [locations | KnownException file locations _ <- exceptions, file == relative path]
    unless (knownSites known refusals)
      (fail (path ++ ": unsafe sites differ from declared boundaries and known internal exceptions: " ++ show refusals))
  putStrLn (manifest ++ ": checked " ++ show (length paths) ++ " Rust sources")

knownSites :: [Int] -> [Refusal] -> Bool
knownSites declared found = sort declared == sort [at | Refusal at _ <- found]

regressions :: IO ()
regressions = do
  let expect label expected actual = unless (actual == expected) (fail (label ++ ": " ++ show actual))
  expect "undeclared unsafe" (Right [Refusal 2 "unsafe"]) (checkSource False [] "fn core() {\nunsafe { external(); } }")
  expect "boundary function" (Right []) (checkSource False [(Function, ItemName "entry")] "pub unsafe extern \"C\" fn entry() { unsafe { external(); } }")
  expect "neighbor remains checked" (Right [Refusal 2 "unsafe"]) (checkSource False [(Function, ItemName "entry")] "unsafe fn entry() {}\nfn core() { unsafe {} }")
  expect "comments and literals" (Right []) (checkSource False [] "/* unsafe /* unsafe */ */ fn core() { let s = r###\"unsafe { }\"###; let x = \"unsafe\"; let c = '{'; }")
  expect "unsafe assembly" (Right [Refusal 1 "global_asm"]) (checkSource False [] "global_asm!(\"unsafe\");")
  expect "callback ABI newtype" (Right []) (checkSource False [(Structure, ItemName "Completion")] "pub struct Completion(pub unsafe extern \"C\" fn(i32));")
  expect "bodyless function does not cover its neighbor" (Right [Refusal 2 "unsafe"]) (checkSource False [(Function, ItemName "stock")] "extern \"C\" { fn stock(); }\nfn core() { unsafe { x() } }")
  expect "brace macro does not cover its neighbor" (Right [Refusal 2 "unsafe"]) (checkSource False [(Macro, ItemName "global_asm")] "global_asm! { \"nop\" }\nfn core() { unsafe { x() }; }")
  expect "parenthesized macro delimiters nest" (Right [Refusal 2 "unsafe"]) (checkSource False [(Macro, ItemName "global_asm")] "global_asm!(nested({ [] }));\nfn core() { unsafe {} }")
  unless (knownSites [2] [Refusal 2 "unsafe"] && not (knownSites [1] [Refusal 2 "unsafe"]) && not (knownSites [2] []))
    (fail "known internal unsafe sites are not exact")
  let outcomes = [checkSource False [(Function, ItemName "entry")] source | source <- ["unsafe fn missing() {}", "unsafe fn entry() {} unsafe fn entry() {}"]]
  unless (all (either (const True) (const False)) outcomes) (fail "stale or ambiguous boundary declaration admitted")

main :: IO ()
main = regressions >> getArgs >>= mapM_ checkDeclaration
