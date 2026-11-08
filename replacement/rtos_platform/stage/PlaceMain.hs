{-# OPTIONS_GHC -Wno-x-partial -Wno-name-shadowing -Wno-type-defaults #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Data.List (isPrefixOf, sortOn)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.Word (Word32)
import Numeric (readHex, showHex)
import Replacement.RtosOwnFirst.Regions (Domain (..), Extent (..), GuestAddress (..), extentBytes, extentStart, hexText)
import Replacement.RtosPlatform.Union
import System.Environment (getArgs)
import System.Exit (die)

data Sec = Sec {secName :: String, secSize :: Int, secAlign :: Int, secMembers :: [String]} deriving (Show, Eq)
data RunState = RunState {runIndex :: Int, runExtent :: Extent, runCursor :: Int}
data Placement = Placement {placeRun :: Int, placeSec :: Sec} deriving (Show, Eq)

hexValue :: String -> Maybe Word32
hexValue s = case readHex s of
  [(n, "")] | n <= toInteger (maxBound :: Word32) -> Just (fromInteger n)
  _ -> Nothing

decValue :: String -> Maybe Int
decValue s = case reads s of
  [(n, "")] -> Just n
  _ -> Nothing

parseRow :: String -> Maybe Sec
parseRow line =
  let rest = dropWhile (not . isPrefixOf ".") (words line)
  in if length rest >= 10
       then let name = head rest
                size = rest !! 4
                flg = rest !! 6
                align = last rest
            in case (hexValue size, decValue align) of
                 (Just n, Just a) | n > 0 && 'A' `elem` flg && wanted name -> Just (Sec name (fromIntegral n) a [name])
                 _ -> Nothing
       else Nothing
  where
    wanted name = not (any (`isPrefixOf` name) [".ARM", ".rel", ".debug", ".comment", ".note", ".llvm_addrsig", ".symtab", ".strtab", ".shstrtab"])

alignTo :: Int -> Int -> Int
alignTo value alignment = if alignment <= 1 then value else ((value + alignment - 1) `div` alignment) * alignment

pack :: [Extent] -> [Sec] -> Either (String, Int, Int) [Placement]
pack runs sections = go initial (sortOn (negate . secSize) sections) []
  where
    initial = [RunState i extent 0 | (i, extent) <- zip [0 ..] runs]
    go _ [] placed = Right (reverse placed)
    go states (sec : rest) placed = case tryStates states of
      Nothing -> Left (secName sec, secSize sec, maximum (0 : map (extentBytes . runExtent) states))
      Just (updated, placement) -> go updated rest (placement : placed)
      where
        tryStates [] = Nothing
        tryStates (state : others) =
          let start = alignTo (runCursor state) (secAlign sec)
          in if start + secSize sec <= extentBytes (runExtent state)
               then Just (state {runCursor = start + secSize sec} : others, Placement (runIndex state) sec)
               else case tryStates others of
                 Just (remaining, placement) -> Just (state : remaining, placement)
                 Nothing -> Nothing

groupPlacements :: [Placement] -> [(Int, [Sec])]
groupPlacements placements = [(i, [placeSec p | p <- placements, placeRun p == i]) | i <- [0 .. maximum (-1 : map placeRun placements)]]

addressText :: GuestAddress -> String
addressText (GuestAddress value) = hexText value

main :: IO ()
main = do
  args <- getArgs
  case args of
    [carriersPath, domainText, readelfPath, output] -> do
      bytes <- B.readFile carriersPath
      union <- either (die . show) pure (parseUnion bytes)
      domain <- case domainText of
        "a" -> pure DomainA
        "b" -> pure DomainB
        _ -> die "domain must be a or b"
      let dm = domainMap union domain
          runExtents = runs dm
      text <- C.readFile readelfPath
      let parsed = [s | line <- C.lines text, Just s <- [parseRow (C.unpack line)]]
          s2 = [s | s <- parsed, any (`isPrefixOf` secName s) [".text.s2_", ".rodata.s2_pool"]]
          composite = if null s2 then [] else [Sec ".s2_group" (sum (map secSize s2)) 4 (map secName s2)]
          sections = [s | s <- parsed, s `notElem` s2] ++ composite
      placements <- either (\(name, size, largest) -> die ("packing deficit: " ++ name ++ " " ++ show size ++ " > " ++ show largest)) pure (pack runExtents sections)
      let groups = groupPlacements placements
          used = [i | (i, secs) <- groups, not (null secs)]
          entryName = entryVeneer (head (mapEntries dm))
          veneerSlug e = drop (length ("xm1_platform_veneer_" :: String)) (entryVeneer e)
          veneerMemory = ["  v_" ++ entryVeneer e ++ " (rx) : ORIGIN = " ++ addressText (entryAddress e) ++ ", LENGTH = 4" | e <- mapEntries dm]
          veneerSections = [".veneer_" ++ entryVeneer e ++ " : { KEEP(*(.veneer." ++ veneerSlug e ++ ")) } > v_" ++ entryVeneer e | e <- mapEntries dm]
          memory = ["  c" ++ show i ++ " (rx) : ORIGIN = " ++ addressText (extentStart e) ++ ", LENGTH = 0x" ++ showHex (extentBytes e) "" | (i, e) <- zip [0 ..] runExtents] ++ veneerMemory
          sectionLines = [".r" ++ show i ++ " : { " ++ unwords ["*(" ++ m ++ ")" | s <- secs, m <- secMembers s] ++ " . = ALIGN(4); } > c" ++ show i | (i, secs) <- groups, not (null secs)]
          firstUsed = head used
          forbidden = "  .forbidden (NOLOAD) : { *(.data .data.* .bss .bss.* COMMON) } > c" ++ show firstUsed
          fixed = ["  .symtab 0 : { *(.symtab) }", "  .shstrtab 0 : { *(.shstrtab) }", "  .strtab 0 : { *(.strtab) }", "  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.* .debug* .llvm_addrsig .ARM.attributes) }"]
          externals = [externalName e ++ " = " ++ addressText (externalAddress e) ++ ";" | e <- mapExternals dm]
          body = unlines (["ENTRY(" ++ entryName ++ ")", "", "MEMORY {"] ++ memory ++ ["}", "", "SECTIONS {", ".rt : { *(.text) *(.rodata) } > c" ++ show firstUsed] ++ veneerSections ++ sectionLines ++ [forbidden] ++ fixed ++ ["}"] ++ externals ++ ["ASSERT(SIZEOF(.forbidden) == 0, \"data or bss present\")"])
      writeFile output body
    _ -> die "usage: place-platform CARRIERS a|b READELF_TEXT OUT_LD"
