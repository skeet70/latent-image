-- | Host-only executable recipe image composition: links the bank kernel, the
-- boot fragments and the D3 hook, runs the D3 compiler, the bank builder and
-- the container chain, and writes recipe-complete.DAT with its manifests.
--
-- usage: build-recipe-package TOOL... OUT_DIR
-- The tools include the bank's pinned Rust archive and the cross toolchain's
-- build facts, both supplied by the Bazel target's runfiles.
-- The leading tool paths come from the Bazel target's args; run it as
-- `bazel run //host/firmware:build-recipe-package -- OUT_DIR`. STOCK, SPEC and
-- D3_SPEC override the default inputs.
module Main where

import Control.Monad (forM, forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.Text.Encoding as TE
import Data.Char (ord)
import Data.List (isPrefixOf, sortOn)
import Data.Scientific (floatingOrInteger)
import qualified Data.Text as T
import qualified Data.Vector as V
import FujiAF.JSON (decodeStrict)
import Numeric (showHex)
import System.Directory (createDirectoryIfMissing, doesFileExist, makeAbsolute, setCurrentDirectory)
import System.Environment (getArgs, getEnvironment, lookupEnv)
import System.Exit (ExitCode (..), exitWith)
import System.FilePath (takeBaseName, takeExtension)
import System.IO (IOMode (WriteMode), hPutStrLn, stderr, withFile)
import System.Process (CreateProcess (..), StdStream (UseHandle), createProcess, proc, waitForProcess)

data Tools = Tools
  { recipeBank, compiledRecipes, applyPatchSet, rebuildUpdate, lzEncode, fflzd :: FilePath
  , clang, ldLld, objcopy, nm, objdump, rustArchive, rustFacts :: FilePath
  }

-- | An output directory under the host-only RC completion directory.
newtype OutDir = OutDir FilePath

newtype Sha256 = Sha256 String deriving (Eq)

usage :: String
usage = "usage: build-recipe-package OUT_DIR under private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/"

refuse :: String -> IO a
refuse message = hPutStrLn stderr message >> exitWith (ExitFailure 1)

main :: IO ()
main = do
  args <- getArgs
  let (toolArgs, rest) = splitAt 13 args
  tools <- mapM makeAbsolute toolArgs >>= \paths -> case paths of
    [a, b, c, d, e, f, g, h, i, j, k, l, m] -> pure (Tools a b c d e f g h i j k l m)
    _ -> refuse "build-recipe-package: the Bazel target supplies thirteen tool paths"
  out <- case rest of
    [dir] | not (null dir) -> pure dir
    _ -> refuse usage
  unless (outRoot `isPrefixOf` out && length out > length outRoot) $
    refuse "output must stay under the host-only RC completion directory"
  lookupEnv "BUILD_WORKSPACE_DIRECTORY" >>= mapM_ setCurrentDirectory
  stock <- inputPath "STOCK" "private-data/firmware/x-m1-1.50/FWUP0002.DAT"
  spec <- inputPath "SPEC" "private-data/analysis/x-m1-1.50/recipes/bank-spec.json"
  d3Spec <- inputPath "D3_SPEC" "private-data/analysis/x-m1-1.50/recipes/d3-compiled.json"
  forM_ ([stock, spec, d3Spec, id4, id5, id2, rustArchive tools, rustFacts tools]
          ++ kernelSources ++ fixedSources) $ \path -> do
    present <- doesFileExist path
    unless present (refuse ("missing input: " ++ path))
  stockBytes <- B.readFile stock
  let stockDigest = sha256 stockBytes
  unless (stockDigest == Sha256 "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490") $
    refuse "pinned stock DAT required"
  build tools (OutDir out) stock spec d3Spec stockDigest

outRoot :: FilePath
outRoot = "private-data/analysis/x-m1-1.50/recipes/rc-complete-20261013/"

inputPath :: String -> FilePath -> IO FilePath
inputPath name fallback = maybe fallback id <$> lookupEnv name

id4, id5, id2 :: FilePath
id4 = "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin"
id5 = "private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin"
id2 = "private-data/analysis/x-m1-1.50/filmsim/id2-00130000.bin"

kernelSources :: [FilePath]
kernelSources =
  [ "menu/recipe_bank_arm.S", "menu/recipe_bank_probe.c", "menu/recipe_bank_hook.c"
  , "menu/recipe_bank_storage.c", "menu/recipe_bank_actions.c", "menu/recipe_bank_edit.c"
  , "replacement/service/rb_service_stock.S", "replacement/capture/rb_capture_stock.S" ]

fixedSources :: [FilePath]
fixedSources =
  [ "menu/recipe_bank.ld", "menu/recipe_bank_boot.S", "menu/recipe_bank_boot.ld"
  , "menu/recipe_compiled.S", "menu/recipe_compiled_layout.S", "menu/recipe_compiled.ld" ]

-- | Sources the input manifest pins between the specs and the linked outputs.
manifestSources :: [FilePath]
manifestSources =
  [ "host/firmware/BuildRecipeBank.hs", "host/firmware/BuildCompiledRecipes.hs"
  , "host/firmware/BuildRecipePackage.hs", "menu/recipe_bank.ld", "menu/recipe_bank_boot.S"
  , "menu/recipe_bank_boot.ld", "menu/recipe_bank_arm.S", "menu/recipe_bank_probe.c"
  , "menu/recipe_bank_hook.c", "menu/recipe_bank_storage.c", "menu/recipe_bank_actions.c"
  , "menu/recipe_bank_edit.c", "menu/recipe_bank_fields.inc", "menu/recipe_bank_ui.h"
  , "menu/recipe_bank_edit.h", "menu/recipe_bank_diagnostic.h"
  , "replacement/bank/lib.rs", "replacement/service/lib.rs"
  , "replacement/service/rb_service_stock.S", "replacement/service/rb_service.h"
  , "replacement/recipe_ui/src/lib.rs", "replacement/owned_menu/src/lib.rs"
  , "replacement/capture/src/lib.rs", "replacement/capture/rb_capture_stock.S" ]

build :: Tools -> OutDir -> FilePath -> FilePath -> FilePath -> Sha256 -> IO ()
build t (OutDir out) stock spec d3Spec (Sha256 stockDigest) = do
  forM_ ["", "/kernel", "/boot", "/d3", "/image"] $ \sub -> createDirectoryIfMissing True (out ++ sub)
  let kernel = out ++ "/kernel"
      boot = out ++ "/boot"
      d3 = out ++ "/d3"
      image = out ++ "/image"
      cc = runClang t
  objects <- forM kernelSources $ \src -> do
    let obj = kernel ++ "/" ++ takeBaseName src ++ ".o"
    if takeExtension src == ".c"
      then cc (arm ++ free ++ ["-Oz", "-Wall", "-Wextra", "-I.", "-DRB_NAME_RECORD=0x00beb784u", "-c", src, "-o", obj])
      else cc (arm ++ ["-c", src, "-o", obj])
    pure obj
  cc (arm ++ link t ++ ["-Wl,-T,menu/recipe_bank.ld", "-Wl,--defsym=RB_BASE=0x00be1f00", "-Wl,--defsym=RB_CAPACITY=0x8300"]
      ++ objects ++ [rustArchive t] ++ ["-o", kernel ++ "/kernel.elf"])
  run (objcopy t) ["-O", "binary", kernel ++ "/kernel.elf", kernel ++ "/kernel.bin"]
  run (objcopy t) ["-j", ".text", "-O", "binary", kernel ++ "/kernel.elf", kernel ++ "/kernel-text.bin"]
  runTo (kernel ++ "/kernel.syms") (nm t) ["--defined-only", "--print-size", "--numeric-sort", kernel ++ "/kernel.elf"]
  runTo (kernel ++ "/kernel.disasm") (objdump t) ["-d", "--triple=armv7-none-eabi", kernel ++ "/kernel.elf"]

  cc (arm ++ ["-c", "menu/recipe_bank_boot.S", "-o", boot ++ "/boot.o"])
  cc (arm ++ link t ++ ["-Wl,-T,menu/recipe_bank_boot.ld", "-Wl,--defsym=RB_SRC_BASE=0x011ad800"
      , "-Wl,--defsym=RB_SRC_LIMIT=0x000b7940"
      , "-Wl,--defsym=RB_VALIDATE_ENTRY=0x011ad820"
      , "-Wl,--defsym=RB_COPY_ENTRY=0x011adcdc"
      , "-Wl,--defsym=RB_STORE_ENTRY=0x011adca0", boot ++ "/boot.o", "-o", boot ++ "/boot.elf"])
  runTo (boot ++ "/boot.syms") (nm t) ["--defined-only", "--print-size", "--numeric-sort", boot ++ "/boot.elf"]
  forM_ ["sum", "entry", "publish", "check", "validate", "copy", "store"] $ \fragment ->
    run (objcopy t) ["-j", ".text.rb_boot_" ++ fragment, "-O", "binary", boot ++ "/boot.elf", boot ++ "/" ++ fragment ++ ".bin"]
  runTo (boot ++ "/boot.disasm") (objdump t) ["-d", "--triple=armv7-none-eabi", boot ++ "/boot.elf"]

  cc (arm ++ ["-c", "menu/recipe_compiled.S", "-o", d3 ++ "/hook.o"])
  cc (arm ++ ["-c", "menu/recipe_compiled_layout.S", "-o", d3 ++ "/layout.o"])
  cc (arm ++ link t ++ ["-Wl,-T,menu/recipe_compiled.ld", "-Wl,--defsym=RC_BASE=0x0061c3ac"
      , d3 ++ "/hook.o", d3 ++ "/layout.o", "-o", d3 ++ "/hook.elf"])
  run (objcopy t) ["-O", "binary", d3 ++ "/hook.elf", d3 ++ "/hook.bin"]
  run (compiledRecipes t) [d3Spec, id4, id5, id2, d3 ++ "/hook.bin", d3]
  run (applyPatchSet t) [d3 ++ "/patch-set-image4.json", id4, d3 ++ "/id4-patched.bin"]
  run (applyPatchSet t) [d3 ++ "/patch-set-image5.json", id5, d3 ++ "/id5-patched.bin"]
  run (recipeBank t) ["--complete", spec, image ++ "/bank-plan.json", image, kernel, boot, d3, stock]

  run (lzEncode t) ["0x160000", image ++ "/id4-final.bin", image ++ "/record4.bin"]
  run (lzEncode t) ["0x2e0000", image ++ "/id5-final.bin", image ++ "/record5.bin"]
  run (rebuildUpdate t) ["replace-record4", "--decoder", fflzd t, "--load-address", "0x0061c1fc"
    , stock, image ++ "/record4.bin", image ++ "/id4-final.bin", image ++ "/record4.DAT"]
  run (rebuildUpdate t) ["replace-record5", "--decoder", fflzd t
    , image ++ "/record4.DAT", image ++ "/record5.bin", image ++ "/id5-final.bin", image ++ "/record5.DAT"]
  run (rebuildUpdate t) ["write-label", image ++ "/record5.DAT", "WARM STANDARD", image ++ "/d3-boot.DAT"]
  let firmware = out ++ "/recipe-complete.DAT"
  run (rebuildUpdate t) ["replace-plain", "--flash-offset", "0x64d800"
    , image ++ "/d3-boot.DAT", image ++ "/source-package.bin", firmware]
  Sha256 datDigest <- sha256 <$> B.readFile firmware

  let pinned = [spec, d3Spec, rustArchive t, rustFacts t] ++ manifestSources
        ++ [ kernel ++ "/kernel.elf", boot ++ "/boot.elf", image ++ "/id4-final.bin"
           , image ++ "/id5-final.bin", image ++ "/source-package.bin" ]
  inputs <- forM pinned $ \path -> do
    Sha256 digest <- sha256 <$> B.readFile path
    pure (JObj [("sha256", JStr digest), ("path", JStr path)])
  B.writeFile (out ++ "/input-manifest.json") (render (JArr inputs))
  layout <- B.readFile (image ++ "/package-layout.json") >>= either refuse pure . decodeStrict >>= either refuse pure . fromAeson
  fields <- case layout of
    JObj fs -> pure fs
    _ -> refuse "package-layout.json: object required"
  -- `jq '. + added'`: a present key keeps its position, a new key is appended.
  let added = [ ("firmware", JStr firmware), ("firmware_sha256", JStr datDigest)
              , ("stock_sha256", JStr stockDigest), ("inputs", JArr inputs), ("installable", JBool False) ]
      final = [(k, maybe v id (lookup k added)) | (k, v) <- fields]
              ++ [kv | kv@(k, _) <- added, k `notElem` map fst fields]
  B.writeFile (out ++ "/recipe-complete.json") (render (JObj final))
  putStrLn ("Host-only image: " ++ firmware)
  putStrLn ("Manifest: " ++ out ++ "/recipe-complete.json")

arm, free :: [String]
arm = ["--target=armv7r-none-eabi", "-march=armv7-r", "-marm"]
free = ["-ffreestanding", "-nostdlib", "-fno-builtin", "-mno-unaligned-access"]

link :: Tools -> [String]
link t = ["-nostdlib", "-static", "-fuse-ld=lld", "--ld-path=" ++ ldLld t, "-Wl,--no-dynamic-linker", "-Wl,--build-id=none"]

-- | clang with Nix hardening flags disabled.
runClang :: Tools -> [String] -> IO ()
runClang t arguments = do
  environment <- getEnvironment
  let env' = ("NIX_HARDENING_ENABLE", "") : filter ((/= "NIX_HARDENING_ENABLE") . fst) environment
  (_, _, _, handle) <- createProcess (proc (clang t) arguments) {env = Just env'}
  waitForProcess handle >>= checkExit (clang t)

run :: FilePath -> [String] -> IO ()
run tool arguments = do
  (_, _, _, handle) <- createProcess (proc tool arguments)
  waitForProcess handle >>= checkExit tool

runTo :: FilePath -> FilePath -> [String] -> IO ()
runTo path tool arguments = withFile path WriteMode $ \h -> do
  (_, _, _, handle) <- createProcess (proc tool arguments) {std_out = UseHandle h}
  waitForProcess handle >>= checkExit tool

checkExit :: FilePath -> ExitCode -> IO ()
checkExit _ ExitSuccess = pure ()
checkExit tool (ExitFailure code) = refuse (tool ++ " failed with exit " ++ show code)

sha256 :: B.ByteString -> Sha256
sha256 bytes = Sha256 (concatMap byte (B.unpack (SHA256.hash bytes)))
 where
  byte w = let digits = showHex w "" in replicate (2 - length digits) '0' ++ digits

-- | Ordered JSON, printed the way `jq .` prints it.
data Json = JStr String | JBool Bool | JNull | JInt Integer | JArr [Json] | JObj [(String, Json)]

fromAeson :: A.Value -> Either String Json
fromAeson value = case value of
  A.String text -> Right (JStr (T.unpack text))
  A.Bool flag -> Right (JBool flag)
  A.Null -> Right JNull
  A.Number number -> either (const (Left "package-layout.json: integer values only")) (Right . JInt) (floatingOrInteger number :: Either Double Integer)
  A.Array items -> JArr <$> mapM fromAeson (V.toList items)
  A.Object members -> JObj <$> mapM (\(k, v) -> (,) (K.toString k) <$> fromAeson v) (sortOn (K.toText . fst) (KM.toList members))

render :: Json -> B.ByteString
render value = TE.encodeUtf8 (T.pack (go 0 value ++ "\n"))
 where
  go :: Int -> Json -> String
  go _ (JStr text) = quoted text
  go _ (JBool True) = "true"
  go _ (JBool False) = "false"
  go _ JNull = "null"
  go _ (JInt number) = show number
  go _ (JArr []) = "[]"
  go depth (JArr items) = "[\n" ++ joined [indent (depth + 1) ++ go (depth + 1) item | item <- items] ++ "\n" ++ indent depth ++ "]"
  go _ (JObj []) = "{}"
  go depth (JObj members) =
    "{\n" ++ joined [indent (depth + 1) ++ quoted k ++ ": " ++ go (depth + 1) v | (k, v) <- members] ++ "\n" ++ indent depth ++ "}"
  joined [] = ""
  joined (x : xs) = x ++ concatMap (",\n" ++) xs
  indent depth = replicate (2 * depth) ' '
  quoted text = '"' : concatMap escape text ++ "\""
  escape '"' = "\\\""
  escape '\\' = "\\\\"
  escape '\b' = "\\b"
  escape '\f' = "\\f"
  escape '\n' = "\\n"
  escape '\r' = "\\r"
  escape '\t' = "\\t"
  escape ch
    | ord ch < 32 || ord ch == 127 = let digits = showHex (ord ch) "" in "\\u" ++ replicate (4 - length digits) '0' ++ digits
    | otherwise = [ch]
