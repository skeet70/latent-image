{-# LANGUAGE OverloadedStrings, ScopedTypeVariables #-}
module Main where

import FujiAF.Host
import FujiAF.JSON (encodeCanonical)
import Emulation.NativeCase (runCase)
import Control.Monad (forM_, when)
import Data.Aeson (Value(..))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import qualified Data.Text as T
import Data.List (isInfixOf, isPrefixOf)
import Data.Maybe (mapMaybe)
import Numeric (readHex, showHex)
import System.Directory (copyFile, createDirectoryIfMissing, getFileSize)
import System.Environment (getArgs)
import System.Exit (ExitCode(..))
import System.FilePath ((</>))
import System.Process (readProcessWithExitCode)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [pilotBin, pilotElf, pilotSymbols, pilotDisasm, defectBin, template, runner, readelf] ->
      runTest pilotBin pilotElf pilotSymbols pilotDisasm defectBin template runner readelf
    _ -> failH "usage: pilot-abi-tests BIN ELF SYMBOLS DISASM DEFECT_BIN TEMPLATE RUNNER READELF"

runTest :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
runTest pilotBin pilotElf pilotSymbols pilotDisasm defectBin template runner readelf = do
  attributes <- elfAttributes readelf pilotElf
  ensure (not ("ABI_VFP_args" `isInfixOf` attributes)) "the pilot carries a VFP argument attribute"
  ensure ("cortex-r4" `isInfixOf` attributes) "the pilot does not carry the cortex-r4 CPU attribute"
  ensure ("ABI_align_preserved" `isInfixOf` attributes) "the pilot does not carry the 8-byte alignment attribute"
  symbols <- readSymbols pilotSymbols
  case lookup "xm1_abi_probe" symbols of
    Just (0x1000, size) | size >= 4 -> pure ()
    _ -> failH "the entry symbol is not xm1_abi_probe at 0x1000"
  (loopAddress, loopSize) <- maybe (failH "the pilot lacks xm1_panic_loop") pure (lookup "xm1_panic_loop" symbols)
  ensure (loopAddress `mod` 4 == 0 && loopSize >= 4 && loopSize `mod` 4 == 0) "the panic loop extent is invalid"
  scanInstructions pilotDisasm

  withTemporary "xm1-rust-abi" $ \root -> do
    positive <- runStaged (root </> "positive") template pilotBin
    (positiveCode, positiveReport) <- runCase runner positive
    ensure (positiveCode == 0) ("positive native case failed: " ++ show positiveReport)
    ensure (get positiveReport "status" == s "passed") ("positive status: " ++ show positiveReport)
    let positiveRunner = get positiveReport "runner"
    ensure (get positiveRunner "status" == s "returned") ("positive runner status: " ++ show positiveReport)
    ensure (null (list (get positiveReport "mismatches"))) ("positive mismatches: " ++ show positiveReport)
    ensure (get (get positiveRunner "registers") "r0" == s "0x00000000") "positive r0 is not zero"
    putStrLn "positive: status=passed runner=returned"

    defect <- runStaged (root </> "defect") template defectBin
    (defectCode, defectReport) <- runCase runner defect
    ensure (defectCode == 1) ("the ABI defect artifact was accepted: " ++ show defectReport)
    ensure (get defectReport "status" == s "failed") ("defect status: " ++ show defectReport)
    let defectRunner = get defectReport "runner"
    ensure (get defectRunner "status" == s "returned") ("defect runner status: " ++ show defectReport)
    ensure (not (null (list (get defectReport "mismatches")))) ("defect mismatches: " ++ show defectReport)
    ensure (get (get defectRunner "registers") "r0" == s "0x00000001") "the defect did not change the ABI result"
    ensure (resultBytes defectRunner /= resultBytes positiveRunner) "the defect did not change the result memory"
    putStrLn "defect: status=failed runner=returned r0=0x00000001"

    let panicDir = root </> "panic"
    panicPath <- runStaged panicDir template pilotBin >> runPanicCase panicDir loopAddress loopSize
    (panicCode, panicReport) <- runCase runner panicPath
    ensure (panicCode == 0) ("panic smoke failed: " ++ show panicReport)
    ensure (get panicReport "status" == s "passed") ("panic smoke status: " ++ show panicReport)
    let panicRunner = get panicReport "runner"
    ensure (get panicRunner "status" == s "instruction_budget") ("panic loop did not reach the budget: " ++ show panicReport)
    ensure (get (get panicRunner "registers") "r7" == s "0x00000077") "the panic loop changed a preserved register"
    putStrLn "panic: status=passed runner=instruction_budget"

runStaged :: FilePath -> FilePath -> FilePath -> IO FilePath
runStaged root template bin = do
  let artifact = root </> "pilot.bin"
      casePath = root </> "pilot.json"
  createDirectoryIfMissing True root
  copyFile bin artifact
  size <- getFileSize artifact
  bytes <- B.readFile artifact
  digest <- sha256 bytes
  raw <- BC.readFile template
  B.writeFile casePath (BC.pack (replace "@SIZE@" (show size) (replace "@SHA@" digest (BC.unpack raw))))
  pure casePath

runPanicCase :: FilePath -> Integer -> Integer -> IO FilePath
runPanicCase root loopAddress loopSize = do
  positive <- B.readFile (root </> "pilot.json") >>= jsonRead
  let address = hex32 loopAddress
      value = merge positive
        [ ("id", s "rust-panic-loop")
        , ("provenance", s "the piloted panic handler's halt loop entered directly")
        , ("entry", s address)
        , ("functions", arr [obj [("address", s address), ("size", n loopSize)]])
        , ("limits", obj [("instructions", n 1000), ("wall_ms", n 1000)])
        , ("expected", obj
            [ ("status", s "instruction_budget")
            , ("registers", obj [("r7", s "0x00000077")])
            , ("memory", arr [])
            , ("events", arr [])
            ])
        ]
      path = root </> "panic.json"
  B.writeFile path (encodeCanonical value)
  pure path

resultBytes :: Value -> T.Text
resultBytes report = T.concat
  [ bytes | row <- list (get report "memory"), get row "address" == s "0x00003000", String bytes <- [get row "bytes"] ]

elfAttributes :: FilePath -> FilePath -> IO String
elfAttributes readelf elf = do
  (code, out, err) <- readProcessWithExitCode readelf ["-A", elf] ""
  ensure (code == ExitSuccess) ("llvm-readelf failed: " ++ err)
  pure out

readSymbols :: FilePath -> IO [(String, (Integer, Integer))]
readSymbols path = do
  raw <- BC.readFile path
  pure [ (BC.unpack name, (address, size))
       | line <- BC.lines raw
       , let parts = BC.words line
       , length parts >= 4
       , (address, size) <- [(hexValue (parts !! 0), hexValue (parts !! 1))]
       , let name = parts !! 3
       ]

hexValue :: BC.ByteString -> Integer
hexValue text = case readHex (BC.unpack text) of
  [(value, "")] -> value
  _ -> 0

hex32 :: Integer -> String
hex32 value = "0x" ++ replicate (8 - length digits) '0' ++ digits
  where digits = showHex value ""

scanInstructions :: FilePath -> IO ()
scanInstructions path = do
  raw <- BC.readFile path
  forM_ (mapMaybe mnemonic (BC.lines raw)) $ \name ->
    when (forbidden name) (failH ("linked artifact contains forbidden mnemonic " ++ name))

mnemonic :: BC.ByteString -> Maybe String
mnemonic line = case BC.words line of
  (address : name : _) | not (BC.null address) && BC.last address == ':' -> Just (BC.unpack name)
  _ -> Nothing

forbidden :: String -> Bool
forbidden name = name `elem` words "sdiv udiv mrc mcr ldrex strex ldrexd strexd dmb dsb isb clrex swp swpb svc smc eret rfe srs" || "v" `isPrefixOf` name

replace :: String -> String -> String -> String
replace old new = go
  where
    go [] = []
    go source@(first : rest)
      | old `isPrefixOf` source = new ++ go (drop (length old) source)
      | otherwise = first : go rest
