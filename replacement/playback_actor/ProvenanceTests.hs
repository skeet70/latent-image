module Main (main) where

import Control.Monad (unless)
import Data.Word (Word32)
import Replacement.PlaybackActor.Provenance
import System.Exit (exitFailure)

copyLoad, copyStore, reader :: Word32
copyLoad = 0x722130
copyStore = 0x722134
reader = 0x1249990

config :: Config
config = Config [(0x100, 0x110), (0x200, 0x210)] [copyLoad] [copyStore]

-- | The findings and the masks of the writes, in order.
run :: [Event] -> ([Finding], [MaskedWrite])
run = (\(_, f, m) -> (f, reverse m)) . foldl' go (tracker config, [], [])
  where
    go (t, found, masks) e = let (t', f, m) = step t e in (t', found ++ f, maybe masks (: masks) m)

-- | A message of 8 bytes at 0x100 whose builder stored the first 4, copied to 0x200.
copied :: [Event]
copied =
  [ Built 0 0x100 8 [0, 1, 2, 3]
  , CopyBegins 0 (Copy 0x200 0x100 8)
  , Read 0 copyLoad 0x100 4
  , Write 0 copyStore 0x200 [9, 9, 9, 9] [1, 2, 3, 4]
  , Read 0 copyLoad 0x104 4
  , Write 0 copyStore 0x204 [9, 9, 9, 9] [5, 6, 7, 8]
  , CopyEnds 0 False
  ]

check :: String -> Bool -> IO Bool
check name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

-- | The builder of playback message 0x119, image 5 0x00db2fc0 up to its send call.
builder :: [Word32]
builder =
  [ 0xe92d4800, 0xe28db004, 0xe24dd0c8, 0xe1a03000, 0xe5902018, 0xe50b20c8, 0xe5902000, 0xe50b20a8
  , 0xe5902004, 0xe50b20a4, 0xe5902008, 0xe50b2070, 0xe590201c, 0xe50b20c4, 0xe5902014, 0xe50b20b0
  , 0xe5902010, 0xe50b20ac, 0xe590200c, 0xe50b209c, 0xe3000119, 0xe50b0080, 0xe5932020, 0xe50b2068
  , 0xe593202c, 0xe50b200c, 0xe5933030, 0xe50b3008, 0xe3a010c4, 0xe24b20c8
  ]

main :: IO ()
main = do
  let (copyFindings, copyMasks) = run copied
      (overFindings, overMasks) = run (copied ++ [Write 0 0x4000 0x204 [5, 6] [0, 0], Read 0 reader 0x204 2, Write 0 0x4000 0x204 [0, 0] [1, 1]])
      (readFindings, _) = run (copied ++ [Read 0 reader 0x206 1])
      (strayFindings, _) = run (copied ++ [Read 0 copyLoad 0x204 4])
      (escapeFindings, _) = run [Built 0 0x100 8 [0, 1, 2, 3], CopyBegins 0 (Copy 0x300 0x100 8)]
      (releasedFindings, _) = run (copied ++ [Sent 0, Read 0 reader 0x104 4])
      (liveFindings, _) = run (copied ++ [Read 0 reader 0x104 4])
      (unwatchedFindings, _) = run [Built 0 0x180 8 []]
      stored = builderStoredOffsets 196 builder
      expectedStored = concat [[o .. o + 3] | o <- [0x00, 0x20, 0x24, 0x58, 0x04, 0x18, 0x1c, 0x2c, 0x48, 0x60, 0xbc, 0xc0]]
  results <-
    sequence
      [ check "a copy of stored bytes is compared" (copyMasks !! 0 == MaskedWrite (replicate 4 (Just 9)) (map Just [1, 2, 3, 4]))
      , check "a copy of marked bytes is masked" (copyMasks !! 1 == MaskedWrite (replicate 4 (Just 9)) (replicate 4 Nothing))
      , check "copying a marked byte is not interpreting it" (null copyFindings)
      , check "a defined write over marked bytes is compared, its before is masked" (overMasks !! 2 == MaskedWrite [Nothing, Nothing] [Just 0, Just 0])
      , check "an overwritten byte is read and compared as defined" (null overFindings && overMasks !! 3 == MaskedWrite [Just 0, Just 0] [Just 1, Just 1])
      , check "reading a marked byte outside a copy interprets it" (readFindings == [Interpreted reader 0x206 (Origin 0 6)])
      , check "the copy routine outside a reviewed call is untracked" (strayFindings == [Untracked copyLoad 0x204 (Origin 0 4)])
      , check "a mark copied outside watched memory escapes" (length escapeFindings == 4 && all isEscape escapeFindings)
      , check "the builder's frame stops carrying marks when it dies" (null releasedFindings && length liveFindings == 1)
      , check "a mark created outside watched memory escapes" (length unwatchedFindings == 8)
      , check "the 0x119 builder stores 48 of its 196 payload bytes" (fmap (\s -> (length s, all (`elem` s) expectedStored)) stored == Right (48, True))
      , check "a byte store in the builder is refused" (either (const True) (const False) (builderStoredOffsets 196 (builder ++ [0xe54b2014])))
      ]
  unless (and results) exitFailure
  where
    isEscape Escaped {} = True
    isEscape _ = False
