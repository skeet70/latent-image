{-# LANGUAGE OverloadedStrings #-}
-- The Classic Chrome RAM-push runtime and manifest
-- (reports/classic-chrome-patch.md section 8.2.5, option A), from the link
-- host/firmware/build-classic-chrome-ram.sh makes, and its read plans.
module Main where

import Control.Monad (forM, forM_, unless, when)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Char (isHexDigit)
import Data.List (intercalate, sortOn)
import Data.Word (Word32)
import Numeric (readHex, showHex)
import System.Directory (doesPathExist)
import System.Environment (getArgs, lookupEnv)
import System.Posix.Files (setFileMode)

import FujiAF.ClassicChrome (Address (..), dataBlockSize, encodeDataBlock, parseParameters)
import FujiAF.ClassicChromeRam
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import qualified FujiAF.Host as H
import ClassicChromeRamBuilder (classicChromeRamBuilderDigest)
import RamPushToolchain
import RecipeBankRamLayout (installerHasCallClosure)

usage :: String
usage =
  "usage: build-classic-chrome-ram --abi"
    ++ " | BASE PARAMETERS_JSON LINKED_DIR OUT_DIR RC0_ID4 RC0_ID5 RC0_PLAIN RC0_ID2 CC_ID4 CC_ID5 CC_PLAIN"
    ++ " | --read-plans MANIFEST PREPUSH_PLAN POST_REBOOT_PLAN"

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    ["--abi"] ->
      putStrLn (unwords [show ccPublicationCount, show ccSetupCount, show ccActivationCount, show ccAbiVersion])
    ["--read-plans", manifest, prepush, postReboot] -> writeReadPlans manifest prepush postReboot
    [base, parameters, linkedDir, out, rc0Id4, rc0Id5, rc0PlainPath, rc0Id2, ccId4, ccId5, ccPlain] -> do
      requirePinnedToolchain
      baseValue <- maybe (H.failH "BASE must be 0x and up to eight hex digits") pure (parseHex base)
      build baseValue parameters linkedDir out
        [ ("rc0_image4", rc0Id4, "a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551")
        , ("rc0_image5", rc0Id5, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
        , ("rc0_plain", rc0PlainPath, "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
        , ("rc0_image2", rc0Id2, "236d12a11c8b386f71192675c12398358840327f21e392e6eaa91ddfd6e020a5")
        , ("cc_flash_image4", ccId4, "00493fab3f7c67fddf3899e8f38395def2b593b93e4dcb73206bfb6cc2ddff6a")
        , ("cc_flash_image5", ccId5, "e740aeb28ce070a659583bd0bb5cfd9ecc5846ac302564741fcdf37b9b2089bb")
        , ("cc_flash_plain", ccPlain, "7e60e04e2dde4cecea4d575e02ae64b8f6053af3f459c42b5222f980e3c301fa")
        ]
    _ -> H.failH usage

-- | Refuses unless the relink script ran with exactly the compiled tool paths.
requirePinnedToolchain :: IO ()
requirePinnedToolchain = forM_ pinned $ \(variable, expected) -> do
  actual <- lookupEnv variable
  unless (actual == Just expected) $
    H.failH ("classic-chrome ram-push: " ++ variable ++ " is not the compiled toolchain path " ++ expected)
  where
    t = ramPushToolchain
    pinned =
      [ ("RAM_PUSH_CLANG", toolClang t)
      , ("RAM_PUSH_LD_LLD", toolLdLld t)
      , ("RAM_PUSH_OBJCOPY", toolObjcopy t)
      , ("RAM_PUSH_NM", toolNm t)
      , ("RAM_PUSH_OBJDUMP", toolObjdump t)
      , ("PATH", toolCoreutilsBin t)
      ]

-- | The workspace files the manifest's source identity names.
sourcePaths :: [FilePath]
sourcePaths =
  [ "menu/ram_push_installer.c"
  , "menu/ram_push_abi.h"
  , "menu/classic_chrome_ram.S"
  , "menu/classic_chrome_ram_stock.ld"
  , "menu/classic_chrome_ram_runtime.ld"
  , "host/firmware/build-classic-chrome-ram.sh"
  ]

build :: Word32 -> FilePath -> FilePath -> FilePath -> [(String, FilePath, String)] -> IO ()
build base parametersPath linkedDir out inputs = do
  setFileMode out 0o700
  H.ensure (base `mod` 8 == 0 && toInteger base + toInteger ccAllocationSize <= 0x100000000) "base must be 8-byte aligned with room for the allocation"
  images <- forM inputs $ \(label, path, expected) -> do
    bytes <- H.readRegular (64 * 1024 * 1024) path
    actual <- H.sha256 bytes
    H.ensure (actual == expected) (label ++ " SHA-256 " ++ actual ++ " differs from the pinned " ++ expected)
    pure (label, path, bytes)
  [rc0Id4, rc0Id5, rc0PlainImage, rc0Id2, ccId4, ccId5, ccPlainImage] <- pure [bytes | (_, _, bytes) <- images]
  parameterBytes <- H.readRegular (1024 * 1024) parametersPath
  parameters <- either (H.failH . ("parameter file refused: " ++)) pure (parseParameters parameterBytes)

  symbols <- readSymbols (linkedDir ++ "/kernel.syms")
  linkMap <- readFile (linkedDir ++ "/kernel.map")
  H.ensure (null (bankSymbols (map fst symbols))) ("the link defines recipe-bank symbols: " ++ unwords (take 8 (bankSymbols (map fst symbols))))
  H.ensure (null (foreignInputObjects linkMap)) ("the link places bytes from objects other than classic_chrome_ram.o and ram_push_installer.o: " ++ unwords (foreignInputObjects linkMap))
  disassembly <- C.unpack <$> B.readFile (linkedDir ++ "/kernel.disasm")
  H.ensure (installerHasCallClosure disassembly) "installer lacks the two-core rendezvous publication closure"
  let linked name = case [a | (n, a) <- symbols, n == name] of
        [a] -> pure a
        _ -> H.failH ("link symbols: unique definition required for " ++ name)
  linkedStart <- linked "__cc_image_start"
  linkedEnd <- linked "__ram_push_end"
  imageEnd <- linked "__cc_image_end"
  installerStart <- linked "__ram_push_installer_start"
  publicationsStart <- linked "__ram_push_publications_start"
  publicationsEnd <- linked "__ram_push_publications_end"
  install <- linked "ram_push_install"
  rollback <- linked "ram_push_rollback"
  status <- linked "ram_push_status"
  dataStart <- linked ccDataBlockSymbol
  dataEnd <- linked "cc_ram_data_end"
  let offsets = (imageEnd - base, linkedEnd - installerStart, install - installerStart, rollback - installerStart, status - installerStart)
  H.ensure (linkedStart == base && installerStart == ccInstallerStart base)
    "Classic Chrome code does not start the runtime at the base with the installer at the next 32-byte boundary"
  H.ensure (offsets == (ccImageExtent, ccInstallerExtent, ccInstallOffset, ccRollbackOffset, ccStatusOffset))
    ("image extent, installer extent and entry offsets " ++ unwords [hex32 o | let (a, b, c, d, e) = offsets, o <- [a, b, c, d, e]] ++ " differ from the pinned layout")
  H.ensure (linkedEnd - base <= ccAllocationSize) "linked runtime exceeds the allocation"
  H.ensure (dataStart >= linkedStart && dataEnd - dataStart == fromIntegral dataBlockSize && dataEnd <= imageEnd) "cc_ram_data is not the 100-byte block inside the Classic Chrome image"
  H.ensure (publicationsEnd - publicationsStart == fromIntegral (16 * ccPublicationCount)) "installer publication section differs from the payload's table size"

  ramSymbolTable <- either H.failH pure (ramSymbols [(n, Address a) | (n, a) <- symbols])
  publications <-
    either (H.failH . ("shape check refused: " ++)) pure $
      ccRamPublications
        (Rc0Images rc0Id4 rc0Id5 rc0PlainImage rc0Id2)
        (FlashImages ccId4 ccId5 ccPlainImage)
        ramSymbolTable
  H.ensure (length publications == ccPublicationCount) "publication count differs from the pinned count"
  H.ensure
    (all (\p -> let Address a = publicationAddress p in toInteger a + 4 <= toInteger base || toInteger a >= toInteger base + toInteger ccAllocationSize) publications)
    "a publication lies inside the allocation"

  linkedBytes <- B.readFile (linkedDir ++ "/linked.bin")
  H.ensure (B.length linkedBytes == fromIntegral (linkedEnd - linkedStart)) "linked binary extent differs from its symbols"
  let table = B.concat
        [ word32 a <> word32 (publicationOld p) <> word32 (publicationNew p) <> word32 (if publicationPhase p == Setup then 0 else 1)
        | p <- publications, let Address a = publicationAddress p ]
      size = fromIntegral ccAllocationSize
      zeroRuntime = B.replicate size 0
      placed = overlay zeroRuntime (fromIntegral (linkedStart - base)) linkedBytes
      withTable = overlay placed (fromIntegral (publicationsStart - base)) table
  runtime <- either H.failH pure (overlayDataBlock (fromIntegral (dataStart - base)) (encodeDataBlock parameters) withTable)
  runtimeHash <- H.sha256 runtime

  sourceIdentity <- forM sourcePaths $ \path -> (,) path . H.s <$> (H.sha256 =<< B.readFile path)
  parametersHash <- H.sha256 parameterBytes
  imageIdentity <- forM images $ \(label, _, bytes) -> (,) label . H.s <$> H.sha256 bytes
  let identity =
        sourceIdentity
          ++ imageIdentity
          ++ [ ("classic_chrome_parameters", H.s parametersHash)
             , ("build-classic-chrome-ram", H.s classicChromeRamBuilderDigest)
             , ("ram-push-toolchain", H.s ramPushToolchainDigest)
             ]
  B.writeFile (out ++ "/runtime.bin") runtime
  setFileMode (out ++ "/runtime.bin") 0o600
  B.writeFile (out ++ "/ram-push-manifest.json") $ encodeCanonical $ H.obj
    [ ("schema", H.n 1)
    , ("source_identity", H.obj identity)
    , ("base", H.s (hex32 base))
    , ("size", H.n (toInteger size))
    , ("runtime_sha256", H.s runtimeHash)
    , ("entry", H.obj [("install", H.s (hex32 install)), ("rollback", H.s (hex32 rollback)), ("status", H.s (hex32 status))])
    , ( "publications"
      , H.arr
          [ H.obj
              [ ("index", H.n index)
              , ("phase", H.s (if publicationPhase p == Setup then "setup" else "activation"))
              , ("address", H.s (hex32 a))
              , ("old", H.s (hex32 (publicationOld p)))
              , ("new", H.s (hex32 (publicationNew p)))
              , ("core", H.s "unknown")
              ]
          | (index, p) <- zip [0 ..] publications, let Address a = publicationAddress p ]
      )
    ]
  setFileMode (out ++ "/ram-push-manifest.json") 0o600
  putStrLn ("classic-chrome ram-push: " ++ show (length publications) ++ " publications, allocation 0x" ++ showHex size "" ++ ", runtime " ++ runtimeHash)

-- | Pre-push: every publication's old word and the reviewer windows of
-- reports/ram-push-session-script.md section 2.3. Post-reboot: every old
-- word and the stock callback slot.
writeReadPlans :: FilePath -> FilePath -> FilePath -> IO ()
writeReadPlans manifestPath prepushPath postRebootPath = do
  document <- H.readRegular (512 * 1024) manifestPath
  value <- either H.failH pure (decodeStrictIntegers document)
  oldWords <- forM (H.list (H.get value "publications")) $ \p -> do
    address <- maybe (H.failH "manifest address is not hex") pure (parseHex (H.str (H.get p "address")))
    old <- maybe (H.failH "manifest old word is not hex") pure (parseHex (H.str (H.get p "old")))
    pure (address, old)
  H.ensure (length oldWords == ccPublicationCount) "manifest publication count differs from the Classic Chrome payload"
  let groups = coalesce (sortOn fst oldWords)
      publicationWindows = [(start, len, oldPurpose start ws) | (start, len, ws) <- groups]
      heapBreak = (0x00829a00, 16, "heap break 0x00829a08")
      sample1 = (0x4089e9f8, 100, "execcpu worker rows sample 1: five 20-byte rows {task/channel, logical core, result, kind, waiting-task selector}; expect task/core (0x58,1),(0x59,1),(0x5a,1),(0x5b,0),(0x5c,0) and zero kind and waiting selector at rest")
      reviewerTail =
        [ (0x01469860, 32, "query output cells 0x01469878")
        , (0x01469b20, 32, "request-owned bootstrap window")
        , (0x0146a0c0, 16, "service mode 0x0146a0c0, expect 3; table pointer 0x0146a0c4, expect 0x015c22a0")
        , (0x015c22a0, 32, "query callback table incl. slot 0x015c22ac, expect 0x00dc7874")
        , (0x4089e9f4, 104, "execcpu worker rows sample 2, about 1.1 s after sample 1: word 0x4089e9f4 then the same five rows at 0x4089e9f8; same expectations")
        , (0x40a0fbac, 4, "channel 0x58 queue message size; expect 0x24")
        , (0x4089ea94, 4, "post prehook; expect 0x00726478 (stock trace recorder installed by 0x0069d1f8)")
        , (0x40a5b5e0, 16, "message-trace state; expect byte0 0, byte1 1, byte2 1, +0xc 0x41ac9800")
        ]
      ordered = sortOn (\(a, _, _) -> a) (heapBreak : publicationWindows)
      (early, late) = splitAt (length ordered - 8) ordered
      prepush = early ++ [sample1] ++ late ++ reviewerTail
      postReboot = publicationWindows ++ [(0x015c22ac, 4, "Stock callback slot must equal 0x00dc7874 (little-endian) after power-off and ordinary reboot")]
  writePlan prepushPath prepush
  writePlan postRebootPath postReboot
  putStrLn ("read plans: " ++ show (length prepush) ++ " and " ++ show (length postReboot) ++ " windows")
  where
    oldPurpose start ws =
      let full = "Old words (little-endian): " ++ intercalate ", " [hex32 a ++ "=" ++ hex32 v | (a, v) <- ws]
       in if length full <= 256
            then full
            else show (length ws) ++ " publication old words " ++ hex32 start ++ ".." ++ hex32 (maximum (map fst ws)) ++ "; expected values are the manifest's old words"

-- | Windows of at most 248 bytes, each starting at a publication word.
coalesce :: [(Word32, Word32)] -> [(Word32, Int, [(Word32, Word32)])]
coalesce [] = []
coalesce (first : rest) = go (fst first) [first] rest
  where
    go start acc [] = [close start acc]
    go start acc (w@(a, _) : more)
      | a + 4 - start <= 248 = go start (acc ++ [w]) more
      | otherwise = close start acc : go a [w] more
    close start acc = (start, fromIntegral (fst (last acc) + 4 - start), acc)

writePlan :: FilePath -> [(Word32, Int, String)] -> IO ()
writePlan path windows = do
  exists <- doesPathExist path
  when exists (H.failH ("read plan exists and is not overwritten: " ++ path))
  B.writeFile path $ encodeCanonical (H.obj
    [ ( "windows"
      , H.arr [H.obj [("index", H.n i), ("address", H.s (hex32 a)), ("length", H.n (toInteger l)), ("purpose", H.s p)] | (i, (a, l, p)) <- zip [0 ..] windows]
      )
    ]) <> "\n"
  setFileMode path 0o600

readSymbols :: FilePath -> IO [(String, Word32)]
readSymbols path = do
  text <- C.unpack <$> B.readFile path
  forM (lines text) $ \line -> case words line of
    address : rest | not (null rest) && all isHexDigit address, [(v, "")] <- readHex address -> pure (last rest, v)
    _ -> H.failH ("link symbols: malformed row in " ++ path)

overlay :: B.ByteString -> Int -> B.ByteString -> B.ByteString
overlay bytes offset fresh = B.take offset bytes <> fresh <> B.drop (offset + B.length fresh) bytes

parseHex :: String -> Maybe Word32
parseHex ('0' : 'x' : digits) | not (null digits) && length digits <= 8, [(v, "")] <- readHex digits = Just v
parseHex _ = Nothing

word32 :: Word32 -> B.ByteString
word32 w = B.pack [fromIntegral (w `div` (256 ^ i)) | i <- [0 .. 3 :: Int]]

hex32 :: Word32 -> String
hex32 value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits
