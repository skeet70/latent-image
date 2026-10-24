-- | The USB personality toggle (reports/usb-personality-toggle.md): its
-- image-5 sites, the merge of its two writes into a base image-5 write set,
-- and the assembly of the output steps from the base extension's steps.
module FujiAF.UsbToggle
  ( Write (..)
  , WriteKind (..)
  , BaseStep (..)
  , hookSite
  , hookCurrent
  , hookStockTarget
  , extentStart
  , extentLength
  , extentSha256
  , stubCallees
  , image5Base
  , image5Sha256
  , toggleWrites
  , mergeWrites
  , stubBlTargets
  , writeEnd
  , stepPath
  , baseStep
  , assembleSteps
  , outputNames
  ) where

import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value (Null), eitherDecodeStrict')
import Data.Bits (shiftR, (.&.))
import qualified Data.ByteString as B
import Data.List (sortOn)
import Data.Word (Word32)
import FujiAF.ClassicChrome (Address (..), blTarget, encodeBl)
import qualified FujiAF.Host as H
import Numeric (showHex)
import System.FilePath (takeFileName)

data WriteKind = BlobWrite | WordWrite deriving (Eq, Show)

-- | One in-place write. Word writes carry exactly four bytes.
data Write = Write
  { wKind :: WriteKind
  , wId :: String
  , wGroup :: String
  , wRole :: String
  , wAddress :: Word32
  , wCurrent :: B.ByteString
  , wNew :: B.ByteString
  }
  deriving (Eq, Show)

-- | Connect handler A's read of cache[0xf7]: @bl 0x0069f8fc@.
hookSite, hookCurrent, hookStockTarget :: Word32
hookSite = 0x00ecf900
hookCurrent = 0xebdf3ffd
hookStockTarget = 0x0069f8fc

-- | The unreferenced image-5 function extent the stub replaces from its entry.
extentStart :: Word32
extentStart = 0x00bf4454

extentLength :: Int
extentLength = 0x90

extentSha256 :: String
extentSha256 = "a862a878075d05fd011efb724fdd7503fb406bddac8c1a00f27998486efd4ec0"

-- | Byte getter, byte setter, debounced key state and system mode word.
stubCallees :: [Word32]
stubCallees = [0x0069f8fc, 0x0069fc94, 0x006a0d00, 0x0073c15c]

image5Base :: Word32
image5Base = 0x00bf1a00

image5Sha256 :: String
image5Sha256 = "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"

writeEnd :: Write -> Word32
writeEnd w = wAddress w + fromIntegral (B.length (wNew w))

-- | The stub blob over the extent's current bytes, and the hook word.
toggleWrites :: B.ByteString -> B.ByteString -> Either String [Write]
toggleWrites stub currentExtent
  | B.null stub || B.length stub `mod` 4 /= 0 = Left "stub blob must be whole words"
  | B.length stub > extentLength = Left "stub blob exceeds the extent"
  | B.length currentExtent /= extentLength = Left "current extent must be 0x90 bytes"
  | otherwise = do
      hook <- encodeBl (Address hookSite) (Address extentStart)
      if blTarget (Address hookSite) hookCurrent /= Just (Address hookStockTarget)
        then Left "hook current word does not branch to 0x0069f8fc"
        else
          Right
            [ Write BlobWrite "usb-toggle-stub" "TOGGLE" "stub usb_toggle in the unreferenced function extent [0x00bf4454, 0x00bf44e4)" extentStart (B.take (B.length stub) currentExtent) stub
            , Write WordWrite "usb-toggle-hook" "TOGGLE" "bl usb_toggle replaces connect handler A's bl 0x0069f8fc (cache[0xf7])" hookSite (le hookCurrent) (le hook)
            ]

-- | Disjoint union in address order. A base write that overlaps the hook
-- word or any byte of the extent is refused, the extent tail beyond the stub
-- included.
mergeWrites :: [Write] -> [Write] -> Either String [Write]
mergeWrites base toggle =
  case [(b, claim) | b <- base, claim@(lo, hi, _) <- claims, lo < writeEnd b, wAddress b < hi] of
    [] -> Right (sortOn wAddress (base ++ toggle))
    (b, (_, _, name)) : _ -> Left ("base write " ++ wId b ++ " overlaps " ++ name)
  where
    claims =
      (extentStart, extentStart + fromIntegral extentLength, "the stub extent [0x00bf4454, 0x00bf44e4)")
        : [(wAddress a, writeEnd a, wId a) | a <- toggle]

-- | Targets of every A32 BL (condition AL) in a blob linked at the extent.
stubBlTargets :: B.ByteString -> [Word32]
stubBlTargets blob =
  [ t
  | (k, w) <- zip [0 ..] (words32 blob)
  , w .&. 0x0f000000 == 0x0b000000
  , Just (Address t) <- [blTarget (Address (extentStart + 4 * k)) (0xe0000000 + (w .&. 0x0fffffff))]
  ]

-- | A base extension step and, when it names a file, that file's path and
-- bytes; 'bsSegment' is set for image patch sets.
data BaseStep = BaseStep
  { bsStep :: Value
  , bsSegment :: Maybe Integer
  , bsFile :: Maybe (FilePath, B.ByteString)
  }
  deriving (Eq, Show)

stepPath :: Value -> Maybe FilePath
stepPath step = case H.get step "path" of
  Null -> Nothing
  v -> Just (H.str v)

-- | Binds a step to the bytes of the file it names, which must match the
-- step's SHA-256.
baseStep :: Value -> Maybe B.ByteString -> Either String BaseStep
baseStep step supplied = case (stepPath step, supplied) of
  (Nothing, Nothing) -> Right (BaseStep step Nothing Nothing)
  (Nothing, Just _) -> Left "bytes supplied for a step that names no file"
  (Just path, Nothing) -> Left ("base part " ++ path ++ " not supplied")
  (Just path, Just bytes)
    | sha256Hex bytes /= H.str (H.get step "sha256") -> Left ("base part " ++ path ++ " differs from its step SHA-256")
    | H.str (H.get step "kind") == "image-patch-set" -> do
        v <- eitherDecodeStrict' bytes
        Right (BaseStep step (Just (H.num (H.get (H.get v "image") "segment_id"))) (Just (path, bytes)))
    | otherwise -> Right (BaseStep step Nothing (Just (path, bytes)))

-- | Files the builder writes itself; no base part may take one of these names.
outputNames :: [FilePath]
outputNames = ["usb-toggle-image5.json", "usb-toggle.json", "changed-words.json", "changed-words.md"]

-- | The output steps (image-4 steps, the merged image-5 step, then every
-- other base step in base order, the base image-5 step dropped) and the base
-- files to copy, each under its file name.
assembleSteps :: Value -> [BaseStep] -> Either String ([Value], [(FilePath, B.ByteString)])
assembleSteps image5Step parts =
  case [name | (i, name) <- zip [0 :: Int ..] names, name `elem` outputNames || name `elem` take i names] of
    name : _ -> Left ("base part file name " ++ name ++ " is taken")
    [] -> Right ([out p | p <- kept, bsSegment p == Just 4] ++ [image5Step] ++ [out p | p <- kept, bsSegment p /= Just 4], files)
  where
    kept = [p | p <- parts, bsSegment p /= Just 5]
    files = [(takeFileName path, bytes) | BaseStep _ _ (Just (path, bytes)) <- kept]
    names = map fst files
    out p = case bsFile p of
      Nothing -> bsStep p
      Just (path, _) -> H.set "path" (H.s (takeFileName path)) (bsStep p)

sha256Hex :: B.ByteString -> String
sha256Hex = concatMap (\x -> let d = showHex x "" in replicate (2 - length d) '0' ++ d) . B.unpack . SHA256.hash

words32 :: B.ByteString -> [Word32]
words32 bytes
  | B.length bytes < 4 = []
  | otherwise = sum [fromIntegral (B.index bytes i) * 256 ^ i | i <- [0 .. 3]] : words32 (B.drop 4 bytes)

le :: Word32 -> B.ByteString
le w = B.pack [fromIntegral (w `shiftR` s) | s <- [0, 8, 16, 24]]
