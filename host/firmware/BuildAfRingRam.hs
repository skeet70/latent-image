{-# LANGUAGE OverloadedStrings #-}
module Main where

import AfRingAllowlist (checkAfRing)
import ArmElf (elfSymbols, Symbol (..), parseElf)
import Control.Monad (forM, forM_, unless)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isHexDigit)
import Data.Word (Word32)
import Numeric (readHex, showHex)
import System.Environment (getArgs, lookupEnv)
import System.Posix.Files (setFileMode)
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import RamPushLayout (installerHasCallClosure)
import RamPushToolchain

newtype AfBase = AfBase Word32

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["--abi"] -> putStrLn "6 3 3 5"
    [baseText, linked, out, input] -> do
      base <- case readHex (drop 2 baseText) of
        [(value, "")] | take 2 baseText == "0x", let digits = drop 2 baseText, not (null digits), length digits <= 8, all isHexDigit digits, value <= (0xffffffff :: Integer) -> pure (AfBase (fromInteger value))
        _ -> H.failH "AF ring base must be hexadecimal"
      build base linked out input
    _ -> H.failH "usage: build-af-ring-ram --abi | BASE LINKED_DIR OUT_DIR RC0_ID5"

sources :: [FilePath]
sources =
  [ "menu/ram_push_installer.c", "menu/ram_push_abi.h"
  , "menu/af_ring_payload.c", "menu/af_ring_hooks.S", "menu/af_ring_ram.ld", "menu/af_ring_allowlist.json"
  , "host/firmware/build-af-ring-ram.sh", "host/firmware/BuildAfRingRam.hs", "host/firmware/AfRingAllowlist.hs"
  , "host/firmware/A32.hs", "host/firmware/ArmElf.hs", "host/firmware/BankAllowlist.hs", "host/firmware/RamPushLayout.hs"
  , "host/FujiAF/Host.hs", "host/FujiAF/JSON.hs"
  ]

build :: AfBase -> FilePath -> FilePath -> FilePath -> IO ()
build (AfBase base) linked out input = do
  forM_ pinned $ \(name, expected) -> do
    actual <- lookupEnv name
    unless (actual == Just expected) (H.failH ("AF ring relink tool differs: " ++ name))
  H.ensure (base .&. 7 == 0 && toInteger base + 0x6800 <= 0x0b3f6e00) "AF allocation must fit aligned populated bulk DRAM"
  image <- H.readRegular (8 * 1024 * 1024) input
  imageHash <- H.sha256 image
  H.ensure (imageHash == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") "RC0 X-M1 1.50 image-5 identity differs"
  elfBytes <- B.readFile (linked ++ "/kernel.elf")
  elf <- either H.failH pure (parseElf elfBytes)
  allowlist <- B.readFile "menu/af_ring_allowlist.json"
  either H.failH pure (checkAfRing base elfBytes allowlist)
  disassembly <- C.unpack <$> B.readFile (linked ++ "/kernel.disasm")
  H.ensure (installerHasCallClosure disassembly) "shared installer call closure differs"
  let symbol name = case [symbolValue s | s <- elfSymbols elf, symbolName s == name] of
        [value] -> pure value
        _ -> H.failH ("unique AF link symbol required: " ++ name)
  installer <- symbol "__ram_push_installer_start"
  install <- symbol "ram_push_install"
  rollback <- symbol "ram_push_rollback"
  status <- symbol "ram_push_status"
  table <- symbol "__ram_push_publications_start"
  tableEnd <- symbol "__ram_push_publications_end"
  installerEnd <- symbol "__af_installer_end"
  ring <- symbol "af_ring_storage"
  synthetic <- symbol "af_ring_synthetic"
  H.ensure (installer == (base + 0x1000 + 31) .&. 0xffffffe0 && ring == base + 0x4000) "AF allocation offsets differ"
  H.ensure (installerEnd - installer == 0xbe0 && tableEnd - table == 96) "AF installer or publication extent differs"
  H.ensure ((install - installer, rollback - installer, status - installer) == (0x600, 0x834, 0x860)) "AF count-specialized installer entry offsets differ"
  hooks <- forM sites $ \(address, first, second, name) -> do
    let offset = fromIntegral (address - 0x00bf1a00)
    H.ensure (B.take 8 (B.drop offset image) == word32 first <> word32 second) ("AF original prologue differs at " ++ hex address)
    target <- symbol name
    H.ensure (base <= target && target < base + 0x1000) "AF hook escapes content window"
    pure (address, first, second, target)
  let publications = [(a + 4, second, target, 0 :: Int) | (a, _, second, target) <- hooks]
        ++ [(a, first, 0xe51ff004, 1) | (a, first, _, _) <- hooks]
  forM_ publications $ \(address, _, _, _) ->
    H.ensure (toInteger address + 4 <= toInteger base || address >= base + 0x6800) "AF publication overlaps allocation"
  raw <- B.readFile (linked ++ "/linked.bin")
  H.ensure (B.length raw == 0x6440) "AF linked extent differs"
  let tableBytes = B.concat [word32 a <> word32 old <> word32 new <> word32 (fromIntegral phase) | (a, old, new, phase) <- publications]
      runtime = overlay (raw <> B.replicate (0x6800 - B.length raw) 0) (fromIntegral (table - base)) tableBytes
  runtimeHash <- H.sha256 runtime
  identities <- forM sources $ \path -> do
    digest <- H.sha256 =<< B.readFile path
    pure (path, H.s digest)
  let identity = identities ++ [("rc0_image5", H.s imageHash), ("ram-push-toolchain", H.s ramPushToolchainDigest)]
      entries = H.obj [("install", H.s (hex install)), ("rollback", H.s (hex rollback)), ("status", H.s (hex status))]
      manifest = H.obj
        [ ("schema", H.n 1), ("source_identity", H.obj identity), ("base", H.s (hex base)), ("size", H.n 0x6800)
        , ("runtime_sha256", H.s runtimeHash), ("entry", entries)
        , ("publications", H.arr
            [ H.obj [("index", H.n index), ("phase", H.s (if phase == 0 then "setup" else "activation")), ("address", H.s (hex address)), ("old", H.s (hex old)), ("new", H.s (hex new)), ("core", H.s "unknown")]
            | (index, (address, old, new, phase)) <- zip [0 :: Integer ..] publications ]) ]
      metadata = H.obj
        [ ("schema", H.n 1), ("ring_address", H.s (hex (ring + 0x40000000))), ("ring_bytes", H.n 0x2440)
        , ("synthetic_entry", H.s (hex synthetic)), ("runtime_sha256", H.s runtimeHash) ]
  setFileMode out 0o700
  forM_ [("runtime.bin", runtime), ("ram-push-manifest.json", encodeCanonical manifest), ("af-ring.json", encodeCanonical metadata)] $ \(name, bytes) -> do
    B.writeFile (out ++ "/" ++ name) bytes
    setFileMode (out ++ "/" ++ name) 0o600
  putStrLn ("AF ring: 6 publications; allocation 0x6800; runtime " ++ runtimeHash)
 where
  t = ramPushToolchain
  pinned = [("RAM_PUSH_CLANG", toolClang t), ("RAM_PUSH_LD_LLD", toolLdLld t), ("RAM_PUSH_OBJCOPY", toolObjcopy t), ("RAM_PUSH_NM", toolNm t), ("RAM_PUSH_OBJDUMP", toolObjdump t), ("PATH", toolCoreutilsBin t)]
  sites = [(0x00f41c48, 0xe92d48f0, 0xe28db014, "af_ring_direction"), (0x00f42abc, 0xe92d4870, 0xe28db010, "af_ring_peak"), (0x00f44fec, 0xe92d49f0, 0xe28db018, "af_ring_accepted")]

word32 :: Word32 -> B.ByteString
word32 value = B.pack [fromIntegral (value `div` (256 ^ n)) | n <- [0 .. 3 :: Int]]

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

overlay :: B.ByteString -> Int -> B.ByteString -> B.ByteString
overlay bytes offset replacement = B.take offset bytes <> replacement <> B.drop (offset + B.length replacement) bytes
