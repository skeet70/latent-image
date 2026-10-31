{-# LANGUAGE OverloadedStrings #-}
-- | Builds a developer experiment under experiments/NAME into a RAM-push
-- runtime: `--trampolines` writes the hook trampolines before the link, and
-- the final step turns the linked ELF into runtime.bin, the manifest and
-- experiment.json.
module Main where

import A32 (decode)
import ExperimentHook (positionIndependent)
import ArmElf (Elf (..), Symbol (..), parseElf)
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (Value (..))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isAlphaNum, isHexDigit)
import Data.List (isPrefixOf, nub, sort)
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (readHex, showHex)
import System.Directory (listDirectory)
import System.Environment (getArgs, lookupEnv)
import System.FilePath (takeExtension, (</>))
import System.Posix.Files (setFileMode)
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import RamPushToolchain

-- | Allocation layout; transport/FujiAF/RamPush.hs `experimentPayload` holds the same numbers.
allocationBytes, codeWindowBytes, hookCapacity :: Word32
allocationBytes = 0x8000
codeWindowBytes = 0x5000
hookCapacity = 8

-- | `ldr pc, [pc, #-4]`: jump through the word that follows.
jumpThroughNext :: Word32
jumpThroughNext = 0xe51ff004

data Image = Image { imageName :: String, imageBase :: Word32, imageBytes :: B.ByteString }

data Hook = Hook
  { hookIndex :: Int
  , hookSite :: Word32
  , hookHandler :: String
  , hookFirst :: Word32
  , hookSecond :: Word32
  }

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["--abi"] -> putStrLn (unwords (map show [2 * hookCapacity, hookCapacity, hookCapacity, 7]))
    ["--trampolines", experiment, id4, id5, plain, output] -> do
      images <- loadImages id4 id5 plain
      hooks <- readHooks experiment images
      B.writeFile output (C.pack (trampolineSource hooks))
      putStrLn ("experiment: " ++ show (length hooks) ++ " hooks")
    [baseText, linked, out, experiment, id4, id5, plain] -> do
      base <- parseHex "base" baseText
      images <- loadImages id4 id5 plain
      hooks <- readHooks experiment images
      build base linked out experiment images hooks
    _ -> H.failH "usage: build-experiment-ram --abi | --trampolines EXPERIMENT ID4 ID5 PLAIN OUT.S | BASE LINKED_DIR OUT_DIR EXPERIMENT ID4 ID5 PLAIN"

parseHex :: String -> String -> IO Word32
parseHex label text = case text of
  '0' : 'x' : digits | not (null digits), length digits <= 8, all isHexDigit digits, [(value, "")] <- readHex digits -> pure (fromInteger value)
  _ -> H.failH (label ++ " must be 0x-prefixed hexadecimal: " ++ text)

loadImages :: FilePath -> FilePath -> FilePath -> IO [Image]
loadImages id4 id5 plain = do
  four <- H.readRegular (8 * 1024 * 1024) id4
  five <- H.readRegular (8 * 1024 * 1024) id5
  flat <- H.readRegular (8 * 1024 * 1024) plain
  digests <- mapM H.sha256 [four, five, flat]
  H.ensure (digests == [stockId4Sha256, stockId5Sha256, stockPlainSha256])
    "the stock 1.50 image-4, image-5 and plain decodes differ from their pinned hashes; redo step 1 of docs/experimenting-on-your-x-m1.md"
  pure [Image "image 4" 0x0061c400 four, Image "image 5" 0x00bf1a00 five, Image "plain" 0x01100000 flat]

stockId4Sha256, stockId5Sha256, stockPlainSha256 :: String
stockId4Sha256 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
stockId5Sha256 = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
stockPlainSha256 = "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"

-- | The stock word at `address`, with the image holding it.
stockWord :: [Image] -> Word32 -> Maybe (String, Word32)
stockWord images address = case [image | image <- images, imageBase image <= address, toInteger address + 4 <= toInteger (imageBase image) + toInteger (B.length (imageBytes image))] of
  image : _ -> let offset = fromIntegral (address - imageBase image) in Just (imageName image, littleEndian (B.take 4 (B.drop offset (imageBytes image))))
  [] -> Nothing

littleEndian :: B.ByteString -> Word32
littleEndian bytes = sum [fromIntegral byte * 256 ^ position | (position, byte) <- zip [0 :: Int ..] (B.unpack bytes)]

readHooks :: FilePath -> [Image] -> IO [Hook]
readHooks experiment images = do
  document <- H.readRegular (64 * 1024) (experiment </> "hooks.json") >>= H.jsonRead
  entries <- case H.get document "hooks" of
    Array _ -> pure (H.list (H.get document "hooks"))
    _ -> H.failH "hooks.json: {\"hooks\": [{\"address\": \"0x...\", \"handler\": \"name\"}, ...]} required"
  H.ensure (length entries <= fromIntegral hookCapacity) ("hooks.json: at most " ++ show hookCapacity ++ " hooks")
  hooks <- forM (zip [0 ..] entries) $ \(index, entry) -> do
    addressText <- text entry "address"
    handler <- text entry "handler"
    H.ensure (not (null handler) && all (\c -> isAlphaNum c || c == '_') handler) ("hook " ++ show index ++ ": handler must be a C or Rust symbol name")
    site <- parseHex ("hook " ++ show index ++ " address") addressText
    H.ensure (site .&. 3 == 0) ("hook at " ++ hex site ++ " is not word aligned")
    (image, first) <- maybe (H.failH ("hook at " ++ hex site ++ " is outside the stock image 4, image 5 and plain decodes")) pure (stockWord images site)
    (_, second) <- maybe (H.failH ("hook at " ++ hex site ++ " runs past the end of " ++ image)) pure (stockWord images (site + 4))
    forM_ [(site, first), (site + 4, second)] $ \(address, word) ->
      unless (positionIndependent (decode address word)) $
        H.failH ("hook at " ++ hex site ++ ": the instruction " ++ hex word ++ " at " ++ hex address
          ++ " reads or writes the PC or branches, so it cannot run displaced in the trampoline; hook a different address")
    pure (Hook index site handler first second)
  let sites = sort (map hookSite hooks)
  H.ensure (and (zipWith (\a b -> b >= a + 8) sites (drop 1 sites))) "hooks.json: hooked words overlap; hook sites must be at least 8 bytes apart"
  pure hooks
 where
  text value key = case H.get value key of
    String t -> pure (T.unpack t)
    _ -> H.failH ("hooks.json: every hook needs a string \"" ++ key ++ "\"")

-- | One trampoline per hook: save the caller's registers and flags, call the
-- handler with a pointer to the saved r0-r12 and lr, restore, run the two
-- displaced instructions and continue at the site's third word.
trampolineSource :: [Hook] -> String
trampolineSource hooks = unlines $
  [ "@ Generated by build-experiment-ram from hooks.json.", ".syntax unified", ".arm", ".section .text.experiment_trampolines,\"ax\",%progbits" ]
  ++ concatMap trampoline hooks
 where
  trampoline hook =
    [ ".global " ++ name hook, ".type " ++ name hook ++ ",%function", ".balign 4", name hook ++ ":"
    , "    push {r0-r12, lr}", "    mrs r12, cpsr", "    push {r12, lr}", "    add r0, sp, #8"
    , "    bl " ++ hookHandler hook
    , "    pop {r12, lr}", "    msr cpsr_f, r12", "    pop {r0-r12, lr}"
    , "    .word " ++ hex (hookFirst hook), "    .word " ++ hex (hookSecond hook)
    , "    ldr pc, [pc, #-4]", "    .word " ++ hex (hookSite hook + 8)
    , ".size " ++ name hook ++ ", .-" ++ name hook ]
  name hook = "__experiment_hook_" ++ show (hookIndex hook)

-- | Stock code words in the AF decision functions; an unused hook slot
-- rewrites one of them with its own value.
paddingCandidates :: [Word32]
paddingCandidates = [0x00f41c50 + 4 * i | i <- [0 .. 15]] ++ [0x00f42ac4 + 4 * i | i <- [0 .. 15]]

build :: Word32 -> FilePath -> FilePath -> FilePath -> [Image] -> [Hook] -> IO ()
build base linked out experiment images hooks = do
  forM_ pinnedTools $ \(name, expected) -> do
    actual <- lookupEnv name
    unless (actual == Just expected) (H.failH ("experiment relink tool differs: " ++ name))
  H.ensure (base .&. 7 == 0 && toInteger base + toInteger allocationBytes <= 0x0b3f6e00) "experiment allocation must be 8-byte aligned inside populated DRAM"
  elf <- B.readFile (linked </> "kernel.elf") >>= either H.failH pure . parseElf
  let symbol name = case [symbolValue s | s <- elfSymbols elf, symbolName s == name] of
        [value] -> pure value
        _ -> H.failH ("unique link symbol required: " ++ name)
  installer <- symbol "__ram_push_installer_start"
  install <- symbol "ram_push_install"
  rollback <- symbol "ram_push_rollback"
  status <- symbol "ram_push_status"
  table <- symbol "__ram_push_publications_start"
  tableEnd <- symbol "__ram_push_publications_end"
  codeEnd <- symbol "__experiment_code_end"
  installerEnd <- symbol "__experiment_installer_end"
  H.ensure (codeEnd <= base + codeWindowBytes)
    ("experiment code and data take " ++ show (codeEnd - base) ++ " bytes; the limit is " ++ show codeWindowBytes)
  H.ensure (installer == (base + codeWindowBytes + 31) .&. 0xffffffe0) "installer start differs from the experiment layout"
  H.ensure (installerEnd <= base + allocationBytes) "installer runs past the allocation"
  H.ensure (tableEnd - table == 16 * 2 * hookCapacity) "publication table size differs from the hook capacity"
  H.ensure (install - installer == 0x600) "installer entry offset differs from the shared installer layout"
  trampolines <- forM hooks $ \hook -> do
    target <- symbol ("__experiment_hook_" ++ show (hookIndex hook))
    H.ensure (base <= target && target < base + codeWindowBytes) "trampoline escapes the code window"
    pure target
  let used = concat [[hookSite hook, hookSite hook + 4] | hook <- hooks]
      spare = filter (`notElem` used) paddingCandidates
      padCount = fromIntegral hookCapacity - length hooks
  padding <- forM (take (2 * padCount) spare) $ \address ->
    maybe (H.failH ("padding word " ++ hex address ++ " is outside the stock decodes")) (pure . (,) address . snd) (stockWord images address)
  H.ensure (length padding == 2 * padCount) "not enough padding words"
  let (setupPads, activationPads) = splitAt padCount padding
      setups = [(hookSite hook + 4, hookSecond hook, target) | (hook, target) <- zip hooks trampolines]
        ++ [(address, word, word) | (address, word) <- setupPads]
      activations = [(hookSite hook, hookFirst hook, jumpThroughNext) | hook <- hooks]
        ++ [(address, word, word) | (address, word) <- activationPads]
      publications = [(a, old, new, 0 :: Int) | (a, old, new) <- setups] ++ [(a, old, new, 1) | (a, old, new) <- activations]
  H.ensure (length (nub [a | (a, _, _, _) <- publications]) == length publications) "publication addresses must be distinct"
  forM_ publications $ \(address, _, _, _) ->
    H.ensure (toInteger address + 4 <= toInteger base || toInteger address >= toInteger base + toInteger allocationBytes) "a hook site lies inside the allocation"
  raw <- B.readFile (linked </> "linked.bin")
  H.ensure (B.length raw <= fromIntegral allocationBytes) "linked image exceeds the allocation"
  let tableBytes = B.concat [word32 a <> word32 old <> word32 new <> word32 (fromIntegral phase) | (a, old, new, phase) <- publications]
      runtime = overlay (raw <> B.replicate (fromIntegral allocationBytes - B.length raw) 0) (fromIntegral (table - base)) tableBytes
  runtimeHash <- H.sha256 runtime
  experimentFiles <- sort . filter ((`elem` [".rs", ".c", ".h", ".S", ".json"]) . takeExtension) <$> listDirectory experiment
  identities <- forM (map (experiment </>) experimentFiles ++ builderSources) $ \path -> do
    digest <- H.sha256 =<< B.readFile path
    pure (path, H.s digest)
  let entries = H.obj [("install", H.s (hex install)), ("rollback", H.s (hex rollback)), ("status", H.s (hex status))]
      manifest = H.obj
        [ ("schema", H.n 1)
        , ("source_identity", H.obj (identities ++ [("ram-push-toolchain", H.s ramPushToolchainDigest)]))
        , ("base", H.s (hex base)), ("size", H.n (toInteger allocationBytes))
        , ("runtime_sha256", H.s runtimeHash), ("entry", entries)
        , ("publications", H.arr
            [ H.obj [("index", H.n index), ("phase", H.s (if phase == 0 then "setup" else "activation")), ("address", H.s (hex address)), ("old", H.s (hex old)), ("new", H.s (hex new)), ("core", H.s "unknown")]
            | (index, (address, old, new, phase)) <- zip [0 :: Integer ..] publications ]) ]
      symbols = [ (symbolName s, H.s (hex (symbolValue s)))
                | s <- elfSymbols elf, base <= symbolValue s, symbolValue s < base + codeWindowBytes
                , not (null (symbolName s)), not ("$" `isPrefixOf` symbolName s), not ("__" `isPrefixOf` symbolName s) ]
      metadata = H.obj
        [ ("schema", H.n 1), ("experiment", H.s experiment), ("base", H.s (hex base)), ("runtime_sha256", H.s runtimeHash)
        , ("hooks", H.arr [H.obj [("address", H.s (hex (hookSite hook))), ("handler", H.s (hookHandler hook)), ("trampoline", H.s (hex target))] | (hook, target) <- zip hooks trampolines])
        , ("symbols", H.obj symbols) ]
  setFileMode out 0o700
  forM_ [("runtime.bin", runtime), ("ram-push-manifest.json", encodeCanonical manifest), ("experiment.json", encodeCanonical metadata)] $ \(name, bytes) -> do
    B.writeFile (out </> name) bytes
    setFileMode (out </> name) 0o600
  when (null hooks) (putStrLn "experiment: no hooks; the payload is uploaded but nothing calls it")
  putStrLn ("experiment " ++ experiment ++ ": " ++ show (length hooks) ++ " hooks; allocation " ++ hex allocationBytes ++ "; runtime " ++ runtimeHash)
 where
  t = ramPushToolchain
  pinnedTools = [("RAM_PUSH_CLANG", toolClang t), ("RAM_PUSH_LD_LLD", toolLdLld t), ("RAM_PUSH_OBJCOPY", toolObjcopy t), ("RAM_PUSH_NM", toolNm t), ("RAM_PUSH_OBJDUMP", toolObjdump t)]

builderSources :: [FilePath]
builderSources =
  [ "menu/ram_push_installer.c", "menu/ram_push_abi.h", "experiments/sdk/experiment.ld", "experiments/sdk/experiment.h"
  , "host/firmware/build-experiment-ram.sh", "host/firmware/BuildExperimentRam.hs" ]

word32 :: Word32 -> B.ByteString
word32 value = B.pack [fromIntegral (value `div` (256 ^ k)) | k <- [0 .. 3 :: Int]]

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

overlay :: B.ByteString -> Int -> B.ByteString -> B.ByteString
overlay bytes offset replacement = B.take offset bytes <> replacement <> B.drop (offset + B.length replacement) bytes
