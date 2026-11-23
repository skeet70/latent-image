{-# LANGUAGE OverloadedStrings #-}

-- | playback-actor-stage PLAIN OWNED_ELF OUT_PLAIN OUT_CONTRACT
--
-- Writes the stock plain image with the owned ELF's machine sections in place: each machine's
-- action extent holds the owned code and trap words after it, and its action-table rows name
-- the owned actions. Every other byte stays stock. `rebuild-update replace-plain` then carries
-- OUT_PLAIN into a DAT.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM, forM_, unless, when)
import Data.Aeson (encode, object, (.=))
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32, Word8)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (showHex)
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

plainBase :: Word32
plainBase = 0x01100000

plainDigest :: String
plainDigest = "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"

-- | A machine's carrier: the action extent and the stock table rows it republishes.
data Machine = Machine
  { machineName :: String
  , machineActions :: (Word32, Word32)
  , machineRows :: Word32
  , machineStockActions :: [Word32]
  }

machines :: [Machine]
machines =
  [ Machine "machine0" (0x011e8ce4, 0x011ea434) 0x01218ec8
      [ 0x011e8ce4, 0x011e8da4, 0x011e8db8, 0x011e8e48, 0x011e8e9c, 0x011e8f3c, 0x011e8fd4, 0x011e906c
      , 0x011e9104, 0x011e9170, 0x011e919c, 0x011e91b4, 0x011e9370, 0x011e94c0, 0x011e95ac, 0x011e9768
      , 0x011e98d8, 0x011e99b4, 0x011e9b08, 0x011e9b88, 0x011e9d8c, 0x011e9fa0, 0x011ea01c, 0x011ea030
      , 0x011ea044, 0x011ea05c, 0x011ea070, 0x011ea088, 0x011ea09c, 0x011ea154, 0x011ea1dc
      ]
  ]

trapWord :: Word32
trapWord = 0xe7f000f0

le :: Word32 -> [Word8]
le v = [fromIntegral (v `shiftR` s) | s <- [0, 8, 16, 24]]

hex :: Word32 -> String
hex v = "0x" ++ showHex v ""

refuse :: String -> IO a
refuse message = hPutStrLn stderr ("playback-actor-stage: " ++ message) >> exitFailure

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [plainPath, elfPath, outPlain, outContract] -> do
      forM_ [outPlain, outContract] $ \path -> doesPathExist path >>= \e -> when e (refuse (path ++ " exists"))
      plain <- B.readFile plainPath
      unless (sha256Hex (sha256 plain) == plainDigest) (refuse "plain image identity differs")
      elfBytes <- B.readFile elfPath
      elf <- either refuse pure (E.parseElf elfBytes)
      let sections = [s | s <- E.elfSections elf, E.sectionAllocated s, E.sectionSize s > 0]
          admitted s = or [inExtent (E.sectionAddress s) (E.sectionSize s) m | m <- machines]
          inExtent address size m =
            let (low, high) = machineActions m
                rows = machineRows m
                rowsEnd = rows + 4 * fromIntegral (length (machineStockActions m))
             in (address >= low && address + size <= high) || (address == rows && address + size == rowsEnd)
          offset a = fromIntegral (a - plainBase) :: Int
          word a = foldr (\i acc -> acc * 256 + fromIntegral (B.index plain (offset a + i))) 0 [0, 1, 2, 3] :: Word32
      forM_ sections $ \s -> unless (admitted s) (refuse ("section " ++ E.sectionName s ++ " at " ++ hex (E.sectionAddress s) ++ " lies outside every carrier"))
      forM_ machines $ \m -> forM_ (zip [0 ..] (machineStockActions m)) $ \(i, stock) ->
        unless (word (machineRows m + 4 * i) == stock) (refuse (machineName m ++ " table row " ++ show i ++ " is not stock"))
      let owned a = case [b | s <- sections, E.sectionAddress s <= a, a < E.sectionAddress s + E.sectionSize s, Just b <- [E.byteAt elf a]] of
            b : _ -> Just b
            [] -> Nothing
          patch m =
            let (low, high) = machineActions m
                rowsEnd = machineRows m + 4 * fromIntegral (length (machineStockActions m))
             in [(a, maybe (le trapWord !! fromIntegral (a `mod` 4)) id (owned a)) | a <- [low .. high - 1]]
                  ++ [(a, maybe 0 id (owned a)) | a <- [machineRows m .. rowsEnd - 1]]
          patches = concatMap patch machines
      rowsComplete <- forM machines $ \m -> pure (all (\a -> owned a /= Nothing) [machineRows m .. machineRows m + 4 * fromIntegral (length (machineStockActions m)) - 1])
      unless (and rowsComplete) (refuse "the owned ELF does not republish every table row")
      let changes = M.fromList [(offset a, b) | (a, b) <- patches]
          bytes = B.pack [M.findWithDefault (B.index plain i) i changes | i <- [0 .. B.length plain - 1]]
      B.writeFile outPlain bytes
      BL.writeFile outContract $ encode $ object
        [ "schema" .= (1 :: Int)
        , "plain_sha256" .= plainDigest
        , "owned_elf_sha256" .= sha256Hex (sha256 elfBytes)
        , "staged_plain_sha256" .= sha256Hex (sha256 bytes)
        , "machines" .=
            [ object
                [ "name" .= machineName m
                , "actions" .= [hex (fst (machineActions m)), hex (snd (machineActions m))]
                , "rows" .= hex (machineRows m)
                , "owned_rows" .= [hex (word' bytes (machineRows m + 4 * i)) | i <- [0 .. fromIntegral (length (machineStockActions m)) - 1]]
                ]
            | m <- machines
            ]
        , "changed_bytes" .= length [() | i <- [0 .. B.length plain - 1], B.index plain i /= B.index bytes i]
        ]
    _ -> refuse "usage: playback-actor-stage PLAIN OWNED_ELF OUT_PLAIN OUT_CONTRACT"
  where
    word' bytes a = foldr (\i acc -> acc * 256 + fromIntegral (B.index bytes (fromIntegral (a - plainBase) + i))) 0 [0, 1, 2, 3] :: Word32
