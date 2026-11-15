module BuildInventory (BuildInventory (..), InventoryUnit (..), InventoryImage (..), discover) where

import Control.Monad (filterM, forM)
import Data.Char (isAlpha, isAlphaNum, isSpace)
import Data.List (isInfixOf, isPrefixOf, isSuffixOf, nub, sort)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import qualified Data.ByteString.Char8 as B
import qualified FujiAF.Host as H
import System.Directory (doesDirectoryExist, doesFileExist, listDirectory)
import System.FilePath ((</>), takeExtension)
import Text.ParserCombinators.ReadP

newtype InventoryUnit = InventoryUnit String deriving (Eq, Ord, Show)
newtype InventoryImage = InventoryImage String deriving (Eq, Ord, Show)
data BuildInventory = BuildInventory { firmwareUnits :: [InventoryUnit], rootImages :: [InventoryImage] } deriving (Eq, Show)
data Token = Identifier String | Literal String | Punctuation Char deriving (Eq, Show)

discover :: FilePath -> IO BuildInventory
discover root = do
  let replacement = root </> "replacement"
  exists <- doesDirectoryExist replacement
  H.ensure exists "replacement build inventory is unavailable"
  names <- sort <$> listDirectory replacement
  units <- fmap concat $ forM names $ \name -> do
    let directory = replacement </> name
        build = directory </> "BUILD.bazel"
    present <- doesFileExist build
    if not present then pure [] else do
      tokens <- readMetadata build
      source <- hasFirmwareSource directory
      pure [InventoryUnit name | source && firmwareMetadata tokens]
  let rootBuild = replacement </> "rtos_timer/BUILD.bazel"
  hasRoot <- doesFileExist rootBuild
  constituents <- if not hasRoot then pure [] else do
    tokens <- readMetadata rootBuild
    either H.failH pure (immutableMetadata tokens)
    let bindings = assignments tokens
        stages = [body | body <- calls "haskell_binary" tokens, M.lookup "name" (fields body) == Just [Literal "stage-root-platform"]]
    stage <- case stages of
      [body] -> pure body
      _ -> H.failH "Root packager build declaration is missing or ambiguous"
    inputs <- either H.failH pure (resolve bindings (M.findWithDefault [] "data" (fields stage)))
    let images = [InventoryImage (take (length label - 4) label) | label <- inputs, "//" `isPrefixOf` label, ".bin" `isSuffixOf` label]
    H.ensure (not (null images)) "Root packager declares no linked image inputs"
    pure images
  pure (BuildInventory units (sort (nub constituents)))

hasFirmwareSource :: FilePath -> IO Bool
hasFirmwareSource directory = do
  names <- listDirectory directory
  let paths = [directory </> name | name <- names, name `notElem` [".git","target","bazel-bin","bazel-out","bazel-testlogs"]]
  files <- filterM doesFileExist paths
  if any ((`elem` [".rs",".S"]) . takeExtension) files then pure True else do
    directories <- filterM doesDirectoryExist paths
    or <$> mapM hasFirmwareSource directories

firmwareMetadata :: [Token] -> Bool
firmwareMetadata tokens = any firmwareRule tokens || any rustCompiler tokens || any nativeBuild tokens || any sourceGroup (calls "filegroup" tokens)
  where
    firmwareRule (Identifier name) = name `elem` ["xm1_rust_elf","xm1_rust_staticlib","xm1_rust_rlib"]
    firmwareRule _ = False
    rustCompiler (Literal label) = "@lab_xm1_rust" `isPrefixOf` label
    rustCompiler _ = False
    nativeBuild (Literal command) = any (`isInfixOf` command) ["nix-build","armelf","--target=armv7r","--target=armv7a"]
    nativeBuild _ = False
    sourceGroup body = M.lookup "name" (fields body) `elem` [Just [Literal "module"],Just [Literal "sources"]]

readMetadata :: FilePath -> IO [Token]
readMetadata path = do
  bytes <- H.readRegularWith (pure ()) False (16*1024*1024) path
  case [tokens | (tokens,"") <- readP_to_S (spacing *> many (token <* spacing) <* eof) (B.unpack bytes)] of
    [] -> H.failH ("unreadable build metadata: " ++ path)
    parses -> pure (last parses)
  where
    spacing = do
      _ <- munch (\character -> isSpace character && character /= '\n')
      (char '#' >> munch (/= '\n') >> spacing) <++ pure ()
    token = triple '"' <++ triple '\'' <++ quoted '"' <++ quoted '\'' <++ identifier <++ (Punctuation <$> satisfy (`notElem` ['"','\'']))
    identifier = do
      first <- satisfy (\character -> isAlpha character || character == '_')
      rest <- munch (\character -> isAlphaNum character || character == '_')
      pure (Identifier (first:rest))
    quoted delimiter = Literal <$> between (char delimiter) (char delimiter) (many (escaped <++ satisfy (/= delimiter)))
    escaped = char '\\' >> get
    triple delimiter = Literal <$> between (string (replicate 3 delimiter)) (string (replicate 3 delimiter)) (manyTill get (look >>= \remaining -> if replicate 3 delimiter `isPrefixOf` remaining then pure () else pfail))

immutableMetadata :: [Token] -> Either String ()
immutableMetadata = inspect . filter (/= Punctuation '\n')
  where
    inspect tokens = case tokens of
      Punctuation '.' : Identifier _ : Punctuation '(' : _ -> Left "method invocation in immutable Root metadata"
      Punctuation ']' : Punctuation '=' : Punctuation '=' : rest -> inspect rest
      Punctuation ']' : Punctuation '=' : _ -> Left "indexed assignment in immutable Root metadata"
      Punctuation operator : Punctuation '=' : _ | operator `elem` ("+-*/%|" :: String) -> Left "augmented assignment in immutable Root metadata"
      _ : rest -> inspect rest
      [] -> Right ()

assignments :: [Token] -> M.Map String [Token]
assignments tokens = case tokens of
  Identifier name : Punctuation '=' : rest ->
    let (body,after) = statement rest in M.insertWith (\_ _ -> unsupported) name body (assignments after)
  Punctuation opener : rest | Just closer <- lookup opener [('(',')'),('[',']'),('{','}')] ->
    assignments (snd (balanced closer rest))
  _ : rest -> assignments rest
  [] -> M.empty
  where
    unsupported = [Punctuation '?']
    statement = collect []
    collect accumulated [] = (reverse accumulated,[])
    collect accumulated (Punctuation '\n':rest) = (reverse accumulated,rest)
    collect accumulated (Punctuation opener:rest) | Just closer <- lookup opener [('(',')'),('[',']'),('{','}')] =
      let (body,after) = balanced closer rest in collect (Punctuation closer : reverse body ++ Punctuation opener : accumulated) after
    collect accumulated (item:rest) = collect (item:accumulated) rest

calls :: String -> [Token] -> [[Token]]
calls name = scan . filter (/= Punctuation '\n')
  where
    scan tokens = case tokens of
      Identifier candidate : Punctuation '(' : rest | candidate == name ->
        let (body,after) = balanced ')' rest in body : scan after
      _ : rest -> scan rest
      [] -> []

balanced :: Char -> [Token] -> ([Token],[Token])
balanced closer = consume []
  where
    consume accumulated [] = (reverse accumulated,[])
    consume accumulated (Punctuation punctuation:rest)
      | punctuation == closer = (reverse accumulated,rest)
      | Just nested <- lookup punctuation [('(',')'),('[',']'),('{','}')] =
          let (body,after) = balanced nested rest
           in consume (Punctuation nested : reverse body ++ Punctuation punctuation : accumulated) after
    consume accumulated (item:rest) = consume (item:accumulated) rest

fields :: [Token] -> M.Map String [Token]
fields tokens = case tokens of
  Identifier name : Punctuation '=' : rest ->
    let (body,after) = fieldValue rest in M.insert name body (fields after)
  _ : rest -> fields rest
  [] -> M.empty
  where
    fieldValue = collect []
    collect accumulated [] = (reverse accumulated,[])
    collect accumulated (Punctuation ',':rest) = (reverse accumulated,rest)
    collect accumulated (Punctuation opener:rest) | Just closer <- lookup opener [('(',')'),('[',']'),('{','}')] =
      let (body,after) = balanced closer rest in collect (Punctuation closer : reverse body ++ Punctuation opener : accumulated) after
    collect accumulated (item:rest) = collect (item:accumulated) rest

resolve :: M.Map String [Token] -> [Token] -> Either String [String]
resolve bindings = complete S.empty
  where
    complete seen tokens = do
      (values,remaining) <- expression seen (filter (/= Punctuation '\n') tokens)
      if null remaining then Right values else Left "unsupported Root input expression"
    expression seen tokens = do
      (values,remaining) <- term seen tokens
      case remaining of
        Punctuation '+' : rest -> do
          (more,after) <- expression seen rest
          pure (values ++ more,after)
        _ -> pure (values,remaining)
    term seen tokens = case tokens of
      Punctuation '[' : rest -> literals rest
      Punctuation '(' : rest -> do
        (values,remaining) <- expression seen rest
        case remaining of
          Punctuation ')' : after -> Right (values,after)
          _ -> Left "unclosed Root input expression"
      Identifier name : rest
        | S.member name seen -> Left ("cyclic Root input alias: " ++ name)
        | otherwise -> case M.lookup name bindings of
            Nothing -> Left ("unresolved Root input alias: " ++ name)
            Just body -> do
              values <- complete (S.insert name seen) body
              pure (values,rest)
      _ -> Left "unsupported Root input expression"
    literals tokens = case tokens of
      Punctuation ']' : rest -> Right ([],rest)
      Literal label : Punctuation ',' : rest -> do
        (more,after) <- literals rest
        pure (label:more,after)
      Literal label : Punctuation ']' : rest -> Right ([label],rest)
      _ -> Left "unsupported Root input list"
