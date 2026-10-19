-- Behavioural tests for the Classic Chrome RAM-push publications over
-- synthetic RC0 and flash-candidate decodes built from the flash design's
-- write lists: counts, phase order, branch targets, shape refusals, the data
-- block overlay, the bank-object refusals and contract stability across links.
module Main (main) where

import Control.Monad (unless)
import Data.Bits (shiftL, testBit, (.&.))
import qualified Data.ByteString as B
import Data.Int (Int64)
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (findIndex, isPrefixOf)
import Data.Word (Word32)
import FujiAF.ClassicChrome
import FujiAF.ClassicChromeRam
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

expect :: IORef [String] -> String -> Bool -> IO ()
expect failures label ok = unless ok (modifyIORef' failures (label :))

-- | A zero image of `size` bytes at `base` with the given words written.
image :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
image base size = foldl put (B.replicate size 0)
  where
    put bytes (address, value) =
      let offset = fromIntegral (address - base)
       in B.take offset bytes <> word32LE value <> B.drop (offset + 4) bytes

blob :: Word32 -> B.ByteString -> [(Word32, Word32)]
blob address bytes = [(address + fromIntegral i, wordOf (B.take 4 (B.drop i bytes))) | i <- [0, 4 .. B.length bytes - 4]]
  where wordOf b = sum [fromIntegral (B.index b k) * 256 ^ k | k <- [0 .. 3]]

at :: WordWrite -> Word32
at w = let Address a = writeAddress w in a

image4Size, image4FlashSize, image5Size, plainSize, image2Size :: Int
image4Size = 0x0089ec00 - 0x0061c340
image4FlashSize = 0x0089ec00 - 0x0061c200
image5Size = 0x00500000
plainSize = 0x00360000
image2Size = 0x00250000

entryStock :: [(Word32, Word32)]
entryStock =
  [ (0x00738fe8, 0xe52db004), (0x00736a74, 0xe52db004), (0x00d7851c, 0xe52db004), (0x00eab2e0, 0xea00000b)
  , (0x00eaac88, 0xea00004e), (0x00d461ec, 0xe5832008), (0x00d5ada4, 0xe3530000), (0x012548a4, 0xe2433003) ]

rc0Labels :: B.ByteString
rc0Labels = B.concat [B.concat (map word32LE [0x48a + n, 0x489 + n, 0x48c + n, 0x48b + n]) | n <- [0 .. 6]] <> listTerminatorLabels

rawLabels :: B.ByteString
rawLabels = B.concat [B.concat (map word32LE [0, 0x600 + n, 0, 0x5f0 + n]) | n <- [0 .. 4]] <> listTerminatorLabels

-- | Words both decode sets carry at the image-4 body addresses.
rc0Body :: [(Word32, Word32)]
rc0Body =
  [(at w, writeCurrent w) | w <- conversionRowWords ++ availabilityRowWords ++ [rawNodeLabelsWord, rawNodeValuesWord]]
    ++ [(at w, rc0LabelsDestinationStock) | w <- rc0LabelPointers]
    ++ [(a, 0xe1a00000) | a <- [0x00738fec, 0x00736a78]]
    ++ [(a, v) | (a, v) <- entryStock, a < 0x00800000]

rc0Images :: Rc0Images
rc0Images =
  Rc0Images
    (image 0x0061c340 image4Size (blob 0x0061c340 rc0ValuesStock ++ blob 0x0061c380 rc0Labels ++ rc0Body))
    (image 0x00bf1a00 image5Size ([(at w, writeCurrent w) | w <- image5FixedWords] ++ [(a, c) | HookSite (Address a) c _ _ _ <- hookSites]))
    (image 0x01100000 plainSize [(at w, writeCurrent w) | w <- plainGateWords ++ plainRawWords])
    (image 0x00130000 image2Size [])

-- | The flash candidate over `rc0Images`, with `extra` written last.
flashImages :: [(Word32, Word32)] -> [(Word32, Word32)] -> FlashImages
flashImages extra4 extra5 =
  FlashImages
    ( image 0x0061c200 image4FlashSize
        ( blob 0x0061c200 (B.pack (map fromIntegral [1 .. 100 :: Int]))
            ++ blob 0x0061c264 (B.concat (map valuePair [0, 1, 2, 5, 9, 10]) <> valuesTerminator)
            ++ blob 0x0061c2b0 (B.take 112 rc0Labels <> labelQuadruple 0x83e <> listTerminatorLabels)
            ++ blob 0x0061c340 (B.take 56 rc0ValuesStock <> valuePair 2 <> valuesTerminator)
            ++ blob 0x0061c388 (B.take 80 rawLabels <> labelQuadruple 0x83e <> listTerminatorLabels)
            ++ [(a, v) | (a, v) <- rc0Body]
            ++ [(at w, writeNew w) | w <- conversionRowWords ++ availabilityRowWords ++ rc0LabelPointers ++ [rawNodeLabelsWord, rawNodeValuesWord]]
            ++ [(0x00738fe8, 0xe350000d), (0x00738fec, 0x83a00000), (0x00736a74, 0xe350000d)]
            ++ extra4
        )
    )
    (image 0x00bf1a00 image5Size ([(at w, writeNew w) | w <- image5FixedWords] ++ [(a, c + 1) | HookSite (Address a) c _ _ _ <- hookSites] ++ extra5))
    (image 0x01100000 plainSize [(at w, writeNew w) | w <- plainGateWords ++ plainRawWords])

symbolsAt :: Word32 -> RamSymbols
symbolsAt base =
  RamSymbols (Address base) (Address (base + 0x0c)) (Address (base + 0x20)) (Address (base + 0x30)) (Address (base + 0x3c))
    (Address (base + 0x4c)) (Address (base + 0x64)) (Address (base + 0x84)) (Address (base + 0xa0)) (Address (base + 0xd8))

-- | Target of an A32 B or BL, decoded by sign-extending imm24.
branchTarget :: Word32 -> Word32 -> Word32
branchTarget site word =
  let imm = fromIntegral (word .&. 0x00ffffff) :: Int64
      signed = if testBit imm 23 then imm - (1 `shiftL` 24) else imm
   in fromIntegral (fromIntegral site + 8 + 4 * signed :: Int64)

main :: IO ()
main = do
  failures <- newIORef []
  let check = expect failures
      built = ccRamPublications rc0Images (flashImages [] []) (symbolsAt 0x00c20000)
      refused label result = check label (either (const True) (const False) result)
  case built of
    Left reason -> check ("well-formed decodes build: " ++ reason) False
    Right publications -> do
      let addresses = [a | p <- publications, let Address a = publicationAddress p]
          phases = map publicationPhase publications
          activation = drop ccSetupCount publications
          activationAddresses = [a | p <- activation, let Address a = publicationAddress p]
          position a = findIndex (== a) activationAddresses
      check "131 publications, 84 setup then 47 activation"
        (length publications == 131 && phases == replicate 84 Setup ++ replicate 47 Activation)
      check "activation starts with the eight entry words in order, then the five hook sites"
        (take 13 activationAddresses == map fst entryStock ++ [a | HookSite (Address a) _ _ _ _ <- hookSites])
      check "entry words branch with B to their symbols"
        (and [ publicationNew p .&. 0xff000000 == 0xea000000 && branchTarget a (publicationNew p) == t
             | (p, t) <- zip activation [0x00c20000, 0x00c20000, 0x00c2000c, 0x00c20020, 0x00c20030, 0x00c2003c, 0x00c2004c, 0x00c20064]
             , let Address a = publicationAddress p ])
      check "hook sites branch with BL to colour, chroma, chroma, tone, tone"
        (and [ publicationNew p .&. 0xff000000 == 0xeb000000 && branchTarget a (publicationNew p) == t
             | (p, t) <- zip (drop 8 activation) [0x00c20084, 0x00c200a0, 0x00c200a0, 0x00c200d8, 0x00c200d8]
             , let Address a = publicationAddress p ])
      check "labels pointers move before the values terminator, which moves before row 2 is named"
        (maximum (map position [0x007823c8, 0x00785d98, 0x0077d1a8]) < position 0x0061c380
          && position 0x0061c380 < position 0x0061c378 && position 0x0061c384 < position 0x0061c378)
      check "RAW labels precede the RAW node words, labels node before values node"
        (position 0x0061c388 < position 0x0078bd80 && position 0x0078bd80 < position 0x0078bd7c
          && drop 45 activationAddresses == [0x0078bd80, 0x0078bd7c])
      check "the data block and the RC0 labels tail are not published"
        (not (any (\a -> a >= 0x0061c200 && a < 0x0061c264 || a >= 0x0061c3f8 && a < 0x0061c400) addresses))
      check "the converter words after each entry stay stock" (0x00738fec `notElem` addresses)
      check "below the RC0 load every old word is zero"
        (and [publicationOld p == 0 | p <- publications, let Address a = publicationAddress p, a >= 0x0061c200 && a < 0x0061c340])
      check "every publication is a distinct changed word" (all (\p -> publicationOld p /= publicationNew p) publications)
      case ccRamPublications rc0Images (flashImages [] []) (symbolsAt 0x00c21f88) of
        Left reason -> check ("relink at another base builds: " ++ reason) False
        Right relinked -> do
          let contract ps = [(publicationPhase p, publicationAddress p, publicationOld p) | p <- ps]
          check "index, phase, address and old words do not depend on the link"
            (contract relinked == contract publications)
          check "branch words follow the link" (map publicationNew relinked /= map publicationNew publications)

  refused "an image-5 word outside the pinned ranges is refused" $
    ccRamPublications rc0Images (flashImages [] [(0x00d00000, 1)]) (symbolsAt 0x00c20000)
  refused "a word in the window's zero gap is refused" $
    ccRamPublications rc0Images (flashImages [(0x0061c2a0, 1)] []) (symbolsAt 0x00c20000)
  refused "an image-4 body word outside the rows and pointers is refused" $
    ccRamPublications rc0Images (flashImages [(0x00700000, 1)] []) (symbolsAt 0x00c20000)
  refused "a flash candidate that leaves a converter entry stock is refused" $
    ccRamPublications rc0Images (flashImages [(0x00736a74, 0xe52db004)] []) (symbolsAt 0x00c20000)
  refused "an RC0 entry word other than the stock word is refused" $
    ccRamPublications rc0Images { rc0Image5 = image 0x00bf1a00 image5Size ([(at w, writeCurrent w) | w <- image5FixedWords, at w /= 0x00d461ec] ++ [(0x00d461ec, 0xe5832009)] ++ [(a, c) | HookSite (Address a) c _ _ _ <- hookSites]) }
      (flashImages [] []) (symbolsAt 0x00c20000)
  refused "a nonzero image-2 0x83e record is refused" $
    ccRamPublications rc0Images { rc0Image2 = image 0x00130000 image2Size [(0x0037b808, 0x41)] } (flashImages [] []) (symbolsAt 0x00c20000)

  let linked = B.replicate 0x120 0xaa <> B.pack [0x43, 0x43, 0x44, 0x31] <> B.replicate 96 0 <> B.replicate 64 0xbb
      block = B.pack [0x43, 0x43, 0x44, 0x31] <> B.pack (map fromIntegral [5 .. 100 :: Int])
  case overlayDataBlock 0x120 block linked of
    Left reason -> check ("data block overlays the linked block: " ++ reason) False
    Right runtime -> check "the overlay replaces exactly the 100 block bytes"
      (B.take 0x120 runtime == B.take 0x120 linked && B.take 100 (B.drop 0x120 runtime) == block && B.drop 0x184 runtime == B.drop 0x184 linked)
  refused "a block without the linked magic is refused" (overlayDataBlock 0x120 (B.replicate 100 1) linked)
  refused "a linked block that is not zero after its magic is refused"
    (overlayDataBlock 0 block (B.pack [0x43, 0x43, 0x44, 0x31] <> B.replicate 96 0xbb))

  check "recipe-bank symbols are named" (bankSymbols ["cc_ram_convert", "rb_probe_entry", "recipe_bank_save", "__rb_image_end", "ram_push_install"] == ["rb_probe_entry", "recipe_bank_save", "__rb_image_end"])
  check "a Classic Chrome and installer link names no foreign object"
    (null (foreignInputObjects "   c20000   c20000   184     4 out/classic_chrome_ram.o:(.text)\n   c201a0   c201a0   20    16 out/ram_push_installer.o:(.text.ram_push_rendezvous_locked)\n"))
  check "a bank object in the link map is named"
    (any ("recipe_bank_hook.o" `isPrefixOf`) (foreignInputObjects "   c20500   c20500   40     4 out/recipe_bank_hook.o:(.text)\n"))
  check "the allocation holds the image and the installer" (ccAllocationSize == ccInstallerStart 0 + ccInstallerExtent && ccInstallerStart 8 + ccInstallerExtent - 8 <= ccAllocationSize)

  found <- readIORef failures
  unless (null found) $ do
    mapM_ (hPutStrLn stderr . ("FAIL: " ++)) (reverse found)
    exitFailure
  putStrLn "classic-chrome-ram tests: ok"
