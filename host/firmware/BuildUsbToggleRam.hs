{-# LANGUAGE OverloadedStrings #-}
module Main where

import ArmElf (Elf (..), Section (..), Symbol (..), parseElf, sectionAllocated)
import Control.Monad (forM, forM_, unless)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isHexDigit)
import Data.Word (Word32)
import FujiAF.ClassicChrome (Address (..))
import qualified FujiAF.Host as H
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex, showHex)
import RamPushLayout (installerHasCallClosure)
import RamPushToolchain
import System.Directory (canonicalizePath)
import System.Environment (getArgs, lookupEnv)
import System.Posix.Files (setFileMode)
import UsbToggleRam
import UsbToggleRamBuilder (usbToggleRamBuilderDigest)

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["--abi"] -> putStrLn "23 22 1 6"
    [baseText, linked, out, input, stub, symbols] -> do
      base <- case baseText of
        '0' : 'x' : digits | not (null digits), length digits <= 8, all isHexDigit digits, [(value, "")] <- readHex digits -> pure (Address value)
        _ -> H.failH "USB toggle base must be hexadecimal"
      build base linked out input stub symbols
    _ -> H.failH "usage: build-usb-toggle-ram --abi | BASE LINKED_DIR OUT_DIR RC0_ID5 STUB STUB_SYMBOLS"

build :: Address -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
build baseAddress@(Address base) linked out input stubPath symbolsPath = do
  forM_ pinned $ \(name, expected) -> do
    actual <- lookupEnv name
    unless (actual == Just expected) (H.failH ("usb-toggle ram-push: " ++ name ++ " is not the compiled toolchain path " ++ expected))
  image <- H.readRegular (8 * 1024 * 1024) input
  extent <- either H.failH pure (stockExtent image)
  stubBytes <- H.readRegular 4096 =<< canonicalizePath stubPath
  stub <- either H.failH pure (reviewedStub stubBytes)
  symbols <- C.unpack <$> (H.readRegular (64 * 1024) =<< canonicalizePath symbolsPath)
  let stubEntries = [address | [address, _, "usb_toggle"] <- map words (lines symbols)]
  H.ensure (case stubEntries of [text] -> readHex text == [(0x00bf4454 :: Word32, "")]; _ -> False)
    "reviewed stub symbol usb_toggle is not at 0x00bf4454"
  planned <- either H.failH pure (publications baseAddress stub extent)
  elfBytes <- H.readRegular (1024 * 1024) (linked ++ "/kernel.elf")
  elf <- either H.failH pure (parseElf elfBytes)
  disassembly <- C.unpack <$> H.readRegular (1024 * 1024) (linked ++ "/kernel.disasm")
  H.ensure (installerHasCallClosure disassembly) "shared installer call closure differs"
  let symbol name = case [symbolValue s | s <- elfSymbols elf, symbolName s == name] of
        [value] -> pure value
        _ -> H.failH ("unique USB toggle link symbol required: " ++ name)
  start <- symbol "__ram_push_installer_start"
  end <- symbol "__ram_push_end"
  runtimeStart <- symbol "__ram_push_start"
  install <- symbol "ram_push_install"
  rollback <- symbol "ram_push_rollback"
  status <- symbol "ram_push_status"
  table <- symbol "__ram_push_publications_start"
  tableEnd <- symbol "__ram_push_publications_end"
  rendezvous <- symbol "__ram_push_rendezvous_start"
  rendezvousEnd <- symbol "__ram_push_rendezvous_end"
  mailbox <- symbol "__ram_push_mailbox_start"
  mailboxEnd <- symbol "__ram_push_mailbox_end"
  H.ensure (installerStart baseAddress == Address start && runtimeStart == base) "USB toggle installer start differs"
  H.ensure ((install - start, rollback - start, status - start, end - start) == (0x600, 0x83c, 0x868, 0xd00))
    "USB toggle specialized installer entry offsets or extent differ"
  H.ensure ((table - start, tableEnd - table, rendezvous - start, rendezvousEnd - rendezvous, mailbox - start, mailboxEnd - mailbox) == (0xa88, 23 * 16, 0xc00, 128, 0xc80, 104))
    "USB toggle installer table, rendezvous or mailbox layout differs"
  H.ensure (end >= start && toInteger end <= toInteger base + toInteger allocationSize && elfEntry elf == install)
    "USB toggle linked extent or entry differs"
  raw <- H.readRegular (1024 * 1024) (linked ++ "/linked.bin")
  H.ensure (B.length raw == fromIntegral (end - start)) "USB toggle linked binary extent differs"
  let allocated = filter sectionAllocated (elfSections elf)
  H.ensure (map sectionName allocated == [".ram_push.text", ".ram_push.publications", ".ram_push.rendezvous", ".ram_push.mailbox"])
    "USB toggle runtime contains a foreign allocated section"
  forM_ allocated $ \section -> do
    H.ensure (sectionAddress section >= start && toInteger (sectionAddress section) + toInteger (sectionSize section) <= toInteger end)
      "USB toggle section escapes installer extent"
    unless (sectionType section == 8) $
      H.ensure (B.take (fromIntegral (sectionSize section)) (B.drop (fromIntegral (sectionAddress section - start)) raw)
        == B.take (fromIntegral (sectionSize section)) (B.drop (fromIntegral (sectionOffset section)) elfBytes))
        "USB toggle linked bytes differ from ELF section bytes"
  let zero = B.replicate (fromIntegral allocationSize) 0
      placed = overlay zero (fromIntegral (start - base)) raw
      runtime = overlay placed (fromIntegral (table - base)) (publicationTable planned)
  runtimeHash <- H.sha256 runtime
  identities <- forM sourcePaths $ \path -> (,) path . H.s <$> (H.sha256 =<< B.readFile path)
  imageHash <- H.sha256 image
  stubHash <- H.sha256 stubBytes
  let identity = identities ++ [("rc0_image5", H.s imageHash), ("usb_toggle_stub", H.s stubHash), ("build-usb-toggle-ram", H.s usbToggleRamBuilderDigest), ("ram-push-toolchain", H.s ramPushToolchainDigest)]
      manifest = H.obj
        [ ("schema", H.n 1), ("source_identity", H.obj identity), ("base", H.s (hex base)), ("size", H.n (toInteger allocationSize))
        , ("runtime_sha256", H.s runtimeHash)
        , ("entry", H.obj [("install", H.s (hex install)), ("rollback", H.s (hex rollback)), ("status", H.s (hex status))])
        , ("publications", H.arr
            [ H.obj [("index", H.n index), ("phase", H.s (case publicationPhase p of Setup -> "setup"; Activation -> "activation")), ("address", H.s (hex address)), ("old", H.s (hex (publicationOld p))), ("new", H.s (hex (publicationNew p))), ("core", H.s "unknown")]
            | (index, p) <- zip [0 :: Integer ..] planned, let Address address = publicationAddress p ]) ]
  setFileMode out 0o700
  forM_ [("runtime.bin", runtime), ("ram-push-manifest.json", encodeCanonical manifest)] $ \(name, bytes) -> do
    B.writeFile (out ++ "/" ++ name) bytes
    setFileMode (out ++ "/" ++ name) 0o600
  putStrLn ("USB toggle: 23 publications; allocation 0xd20; runtime " ++ runtimeHash)
 where
  t = ramPushToolchain
  pinned = [("RAM_PUSH_CLANG", toolClang t), ("RAM_PUSH_LD_LLD", toolLdLld t), ("RAM_PUSH_OBJCOPY", toolObjcopy t), ("RAM_PUSH_NM", toolNm t), ("RAM_PUSH_OBJDUMP", toolObjdump t), ("PATH", toolCoreutilsBin t)]

overlay :: B.ByteString -> Int -> B.ByteString -> B.ByteString
overlay bytes offset replacement = B.take offset bytes <> replacement <> B.drop (offset + B.length replacement) bytes

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits
