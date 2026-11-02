{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM_, unless)
import qualified Data.Aeson as J
import Data.Aeson ((.:))
import Data.Aeson.Types (parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import qualified FujiAF.UpdateContainer as U
import Numeric (showHex)
import Replacement.IdleTask.Admission
import Replacement.IdleTask.Artifact
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert premise message = unless premise (fail message)
right :: Show error => Either error value -> IO value
right = either (fail . show) pure
refuses :: Either error value -> Bool
refuses (Left _) = True
refuses _ = False

newtype InstructionWords = InstructionWords [Word32]
newtype EntryStackLow = EntryStackLow Int
newtype WaitAddress = WaitAddress Word32
newtype SymbolAddress = SymbolAddress Word32
data SymbolRole = Start | End | RustEntry

encoded :: InstructionWords -> B.ByteString
encoded (InstructionWords words32) = B.concat (map U.putWord32 words32)

-- Synthetic A32 programs below are authored examples, not captured firmware bytes.
validPrograms :: [(InstructionWords,EntryStackLow,WaitAddress)]
validPrograms =
  [ (InstructionWords [0xe320f003,0xeafffffd],EntryStackLow 0,WaitAddress 0x69cbf0)
  , (InstructionWords [0xe1a00000,0xe320f003,0xeafffffd],EntryStackLow 0,WaitAddress 0x69cbf4)
  , (InstructionWords [0xeaffffff,0xe320f003,0xeafffffd],EntryStackLow 0,WaitAddress 0x69cbf4)
  , (InstructionWords [0xe1a01001,0xe1a0c00c,0xe320f003,0xeafffffd],EntryStackLow 0,WaitAddress 0x69cbf8)
  , (InstructionWords [0xe52db004,0xe1a0b00d,0xe320f003,0xeafffffd],EntryStackLow (-4),WaitAddress 0x69cbf8)
  , (InstructionWords [0xe92d4800,0xe28db000,0xe320f003,0xeafffffd],EntryStackLow (-8),WaitAddress 0x69cbf8)
  , (InstructionWords [0xe24dd008,0xe320f003,0xeafffffd],EntryStackLow (-8),WaitAddress 0x69cbf4)
  , (InstructionWords [0xe52de004,0xe320f003,0xeafffffd],EntryStackLow (-4),WaitAddress 0x69cbf4)
  ]

invalidPrograms :: [InstructionWords]
invalidPrograms = map InstructionWords
  [ [0xe320f002,0xeafffffd]
  , [0xe320f004,0xeafffffd]
  , [0xe320f001,0xe320f003,0xeafffffd]
  , [0xe320f000,0xe320f003,0xeafffffd]
  , [0xeafffffe,0xeafffffd]
  , [0xe12fff1e,0xe320f003,0xeafffffd]
  , [0xeb000000,0xe320f003,0xeafffffd]
  , [0xe59f0000,0xe320f003,0xeafffffd]
  , [0xe5800000,0xe320f003,0xeafffffd]
  , [0xe3a00001,0xe320f003,0xeafffffd]
  , [0xe3a0b001,0xe320f003,0xeafffffd]
  , [0xe1a0b00d,0xe320f003,0xeafffffd]
  , [0xe1a0d000,0xe320f003,0xeafffffd]
  , [0xe2500000,0xe320f003,0xeafffffd]
  , [0xe320f003,0xe24dd004,0xeafffffc]
  , [0xe320f003,0xe52de004,0xeafffffc]
  , [0xe320f003,0xe2800001,0xeafffffc]
  , [0xe320f003,0xeafffffc]
  , [0xe320f003,0xeaffffff]
  , [0x0320f003,0xeafffffd]
  , [0xe320f003,0xe320f003,0xeafffffd]
  , [0xe92d8000,0xe320f003,0xeafffffd]
  , [0xe24ddc04,0xe320f003,0xeafffffd]
  , [0xeafffffe,0xe320f003,0xeafffffd]
  , [0xe320f003,0xeafffffd,0xe1a00000]
  , [0xffffffff,0xe320f003,0xeafffffd]
  ]

symbolName :: SymbolRole -> C.ByteString
symbolName Start = "__idle_task_start"
symbolName End = "__idle_task_end"
symbolName RustEntry = "xm1_idle_task"

syntheticSymbols :: B.ByteString -> B.ByteString
syntheticSymbols bytes = C.unlines
  [ "0069cbf0 00000000 T __idle_task_start"
  , C.pack (showHex (0x69cbf0+B.length bytes) " 00000000 T __idle_task_end")
  , C.pack ("0069cbf0 " ++ showHex (B.length bytes) " T xm1_idle_task")
  ]

changeSymbol :: SymbolRole -> SymbolAddress -> B.ByteString -> B.ByteString
changeSymbol wanted (SymbolAddress replacement) = C.unlines . map change . C.lines
  where
    change line = case C.words line of
      [_,size,kind,name] | name == symbolName wanted ->
        C.unwords [C.pack (showHex replacement ""),size,kind,name]
      _ -> line

checkArtifact :: B.ByteString -> B.ByteString -> IO LinkedArtifact
checkArtifact bytes symbols = do
  artifact <- right (deriveArtifact bytes symbols)
  let proof = artifactProof artifact
  assert (artifactBytes artifact == bytes) "artifact changed linked bytes"
  assert (wfiPc proof+4 == resumePc proof && wfiPc proof `elem` executionPcs proof
    && resumePc proof `elem` executionPcs proof) "artifact lost its wait/resume contract"
  forM_ [B.empty,B.take 7 bytes,B.replicate 17 0,B.replicate 20 0] $ \malformed ->
    assert (refuses (deriveArtifact malformed symbols)) "malformed carrier accepted"
  assert (refuses (deriveArtifact bytes B.empty)) "missing symbols accepted"
  assert (refuses (deriveArtifact bytes (symbols <> symbols))) "duplicate symbols accepted"
  assert (refuses (deriveArtifact bytes (symbols <> "malformed\n"))) "malformed symbol row accepted"
  forM_ [(Start,0x69cbf4),(RustEntry,0x69cbf2),(RustEntry,0x69cbf4),(End,0x69cc04),(End,0x69cbf1)] $ \(role,address) ->
    assert (refuses (deriveArtifact bytes (changeSymbol role (SymbolAddress address) symbols))) "invalid symbol extent accepted"
  forM_ [0,4..B.length bytes-4] $ \offset -> do
    let corrupted = B.take offset bytes <> U.putWord32 0xffffffff <> B.drop (offset+4) bytes
    assert (refuses (deriveArtifact corrupted symbols)) "malformed actual instruction accepted"
  pure artifact

checkSource :: LinkedArtifact -> FilePath -> FilePath -> IO ()
checkSource artifact datPath imagePath = do
  dat <- B.readFile datPath
  image <- B.readFile imagePath
  source <- right (canonicalSource dat image)
  let staged = stageImage4 source artifact
      result = stagedBytes staged
      offset = 0x69cbf0-0x61c400
      linked = artifactBytes artifact
      padding = B.concat (replicate ((16-B.length linked) `div` 4) (U.putWord32 0xe320f000))
  assert (B.length result == B.length image && B.take offset result == B.take offset image
    && B.drop (offset+16) result == B.drop (offset+16) image) "image4 staging escaped the idle carrier"
  assert (B.take 16 (B.drop offset result) == linked <> padding) "image4 did not carry linked code and unreachable padding"
  (entry,extent,wait,resume,pcs,low) <- right (parseEither (J.withObject "idle contract" $ \value ->
    (,,,,,) <$> value .: "entry" <*> value .: "replaced_extent" <*> value .: "wfi_pc"
      <*> value .: "resume_pc" <*> value .: "execution_pcs" <*> value .: "scratch_low_from_entry_sp") (contractValue staged))
  let proof = artifactProof artifact
  assert (entry == (0x69cbf0 :: Word32) && extent == ([0x69cbf0,0x69cc00] :: [Word32])
    && wait == wfiPc proof && resume == resumePc proof && pcs == executionPcs proof && low == scratchLow proof)
    "staged contract differs from admitted behavior"

main :: IO ()
main = do
  forM_ validPrograms $ \(program,EntryStackLow low,WaitAddress wait) -> do
    let bytes = encoded program
    proof <- right (deriveIdleProof bytes)
    assert (scratchLow proof == low && wfiPc proof == wait && resumePc proof == wait+4
      && executionPcs proof == [0x69cbf0,0x69cbf4..0x69cbf0+fromIntegral (B.length bytes)-4])
      "alternate valid program changed its observable wait/frame contract"
    _ <- checkArtifact bytes (syntheticSymbols bytes)
    pure ()
  forM_ invalidPrograms $ \program ->
    assert (refuses (deriveIdleProof (encoded program))) "forbidden idle behavior admitted"
  forM_ [B.empty,B.singleton 0,B.replicate 6 0,B.replicate 18 0,B.replicate 20 0] $ \bytes ->
    assert (refuses (deriveIdleProof bytes)) "malformed idle extent admitted"
  assert (refuses (canonicalSource B.empty B.empty)) "noncanonical source accepted"
  arguments <- getArgs
  case arguments of
    [imagePath,symbolsPath] -> do
      bytes <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      _ <- checkArtifact bytes symbols
      pure ()
    [imagePath,symbolsPath,datPath,sourceImagePath] -> do
      bytes <- B.readFile imagePath
      symbols <- B.readFile symbolsPath
      artifact <- checkArtifact bytes symbols
      checkSource artifact datPath sourceImagePath
    _ -> fail "usage: idle-task-tests LINKED_BIN SYMBOLS [CANONICAL_DAT CANONICAL_IMAGE4]"
  putStrLn "idle task finite host-admission tests passed; no runtime acceptance"
