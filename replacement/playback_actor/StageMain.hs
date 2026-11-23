{-# LANGUAGE OverloadedStrings #-}

-- | playback-actor-stage PLAIN OWNED_ELF OUT_PLAIN OUT_CONTRACT
--
-- Writes the stock plain image with the owned ELF's machine sections in place: each machine whose
-- action-table rows the ELF publishes has its action extent hold the owned code and trap words
-- after it, and its rows name the owned actions. Every other byte stays stock.
-- `rebuild-update replace-plain` then carries OUT_PLAIN into a DAT.
module Main (main) where

import qualified ArmElf as E
import Control.Monad (forM_, unless, when)
import Data.Aeson (encode, object, (.=))
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32, Word8)
import Emulation.Sha256 (sha256, sha256Hex)
import Replacement.PlaybackActor.Cases (OwnedMachine (..), ownedMachines)
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
  [ Machine (ownedName m) (low, ownedEnd m) (ownedRows m) (ownedActions m)
  | m <- ownedMachines
  , low : _ <- [ownedActions m]
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
          rowsOf m = [machineRows m .. machineRows m + 4 * fromIntegral (length (machineStockActions m)) - 1]
          owned a = case [b | s <- sections, E.sectionAddress s <= a, a < E.sectionAddress s + E.sectionSize s, Just b <- [E.byteAt elf a]] of
            b : _ -> Just b
            [] -> Nothing
          carried = [m | m <- machines, any (\a -> owned a /= Nothing) (rowsOf m)]
          admitted s = or [inExtent (E.sectionAddress s) (E.sectionSize s) m | m <- carried]
          inExtent address size m =
            let (low, high) = machineActions m
                rows = machineRows m
                rowsEnd = rows + 4 * fromIntegral (length (machineStockActions m))
             in (address >= low && address + size <= high) || (address == rows && address + size == rowsEnd)
          offset a = fromIntegral (a - plainBase) :: Int
          word a = foldr (\i acc -> acc * 256 + fromIntegral (B.index plain (offset a + i))) 0 [0, 1, 2, 3] :: Word32
      when (null carried) (refuse "the owned ELF publishes no machine's table rows")
      forM_ sections $ \s -> unless (admitted s) (refuse ("section " ++ E.sectionName s ++ " at " ++ hex (E.sectionAddress s) ++ " lies outside every carrier"))
      forM_ carried $ \m -> forM_ (zip [0 ..] (machineStockActions m)) $ \(i, stock) ->
        unless (word (machineRows m + 4 * i) == stock) (refuse (machineName m ++ " table row " ++ show i ++ " is not stock"))
      let patch m =
            let (low, high) = machineActions m
             in [(a, maybe (le trapWord !! fromIntegral (a `mod` 4)) id (owned a)) | a <- [low .. high - 1]]
                  ++ [(a, maybe 0 id (owned a)) | a <- rowsOf m]
          patches = concatMap patch carried
      unless (all (\m -> all (\a -> owned a /= Nothing) (rowsOf m)) carried) (refuse "the owned ELF does not republish every table row")
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
            | m <- carried
            ]
        , "changed_bytes" .= length [() | i <- [0 .. B.length plain - 1], B.index plain i /= B.index bytes i]
        ]
    _ -> refuse "usage: playback-actor-stage PLAIN OWNED_ELF OUT_PLAIN OUT_CONTRACT"
  where
    word' bytes a = foldr (\i acc -> acc * 256 + fromIntegral (B.index bytes (fromIntegral (a - plainBase) + i))) 0 [0, 1, 2, 3] :: Word32
