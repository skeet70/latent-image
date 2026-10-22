-- Behavioural tests for the Classic Chrome icon RAM-push publications over
-- synthetic RC0 decodes, a synthetic font window and a synthetic Classic
-- Chrome publication list: order, carried word, refusals and the allocation
-- content.
module Main (main) where

import Control.Monad (unless)
import Data.Bits (complement, shiftR, (.&.))
import qualified Data.ByteString as B
import Data.IORef (IORef, modifyIORef', newIORef, readIORef)
import Data.List (findIndex)
import Data.Word (Word32)
import FujiAF.ClassicChrome (Address (..), word32LE)
import FujiAF.ClassicChromeIcon (iconGlyphRecord, label83dRecord, qLabelsArray, qValuesArray)
import FujiAF.ClassicChromeIconRam
import FujiAF.ClassicChromeRam
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

expect :: IORef [String] -> String -> Bool -> IO ()
expect failures label ok = unless ok (modifyIORef' failures (label :))

image :: Word32 -> Int -> [(Word32, Word32)] -> B.ByteString
image base size = foldl put (B.replicate size 0)
  where
    put bytes (address, value) =
      let offset = fromIntegral (address - base)
       in B.take offset bytes <> word32LE value <> B.drop (offset + 4) bytes

-- | Each publication's new word written over `bytes` at `base`.
publish :: Word32 -> B.ByteString -> [CcPublication] -> B.ByteString
publish base = foldl put
  where
    put bytes p =
      let Address a = publicationAddress p
          offset = fromIntegral (a - base)
       in if a < base || offset + 4 > B.length bytes then bytes else B.take offset bytes <> word32LE (publicationNew p) <> B.drop (offset + 4) bytes

image5Base, image2Base, fontBase :: Word32
image5Base = 0x00bf1a00
image2Base = 0x00130000
fontBase = 0x003bc000

-- | A DAT whose font window holds the FDL header, the stock U+F073 directory
-- bytes and per-mille, complemented as the container stores them.
datWith :: [(Int, [Int])] -> B.ByteString
datWith edits = B.replicate 0x0106025c 0 <> B.map complement window
  where
    window = foldl put (B.replicate 0x80000 0) ((0, map fromEnum "FDL Font") : edits)
    put bytes (offset, values) = B.take offset bytes <> B.pack (map fromIntegral values) <> B.drop (offset + length values) bytes

stockFont :: [(Int, [Int])]
stockFont = [(0x3c21d, [0x80, 0xd6, 0x00, 0x00]), (0x7c26b, [0xe8, 0x03])]

ccList :: [(Word32, Word32, Word32)] -> [CcPublication]
ccList extraActivation =
  [CcPublication Setup (Address (0x00500000 + 4 * i)) i (i + 1) "setup" | i <- [0 .. 83]]
    ++ [CcPublication Activation (Address (0x00510000 + 4 * i)) i (i + 1) "activation" | i <- [0 .. 45 - fromIntegral (length extraActivation) + 1]]
    ++ [CcPublication Activation (Address a) old new "RAW labels" | (a, old, new) <- extraActivation]

main :: IO ()
main = do
  failures <- newIORef []
  let check = expect failures
  fragment <- B.readFile "host/firmware/classic-chrome-liveview-image5.json"
  liveView <- either (\e -> hPutStrLn stderr e >> exitFailure) pure (parseLiveViewFragment fragment)
  check "live-view fragment has five words from 0x00eee960" (map liveViewAddress liveView == map Address [0x00eee960, 0x00eee964 .. 0x00eee970])

  let rc0Image4 = image 0x0061c340 (0x0089ec00 - 0x0061c340) [(0x0061c3dc, 0x49c), (0x0078b024, 0x00789e2c), (0x0078b020, 0x00782348)]
      rc0Image5 = image image5Base 0x00400000 [(a, liveViewCurrent w) | w <- liveView, let Address a = liveViewAddress w]
      rc0Image2 = B.replicate 0x00250000 0x11
      rc0 = Rc0Images rc0Image4 rc0Image5 (B.replicate 0x00360000 0) rc0Image2
      base = 0x00c20000
      allocation = iconAllocation base
      cc = ccList [(0x0061c3dc, 0x49c, 0)]
      run font ccPubs images = do
        window <- fontWindowFromDat (datWith font)
        iconRamPublications images window liveView allocation ccPubs
      refused label outcome = check label (either (const True) (const False) outcome)

  case run stockFont cc rc0 of
    Left reason -> check ("synthetic inputs accepted: " ++ reason) False
    Right (combined, carries) -> do
      let ccIndex = length cc - 1
          ccSetup = take ccSetupCount cc
          iconSetup = takeWhile ((== Setup) . publicationPhase) (drop ccSetupCount combined)
          activation = dropWhile ((== Setup) . publicationPhase) combined
          addresses = map publicationAddress combined
          at a = [p | p <- combined, publicationAddress p == Address a]
          position a = findIndex (== Address a) addresses
      check "the carried word is reported with its CC index, old, CC new and combined new"
        (carries == [CcCarry ccIndex (Address 0x0061c3dc) 0x49c 0 0x83d])
      check "every Classic Chrome publication but the carried one is unchanged and in order"
        (filter (`elem` map publicationAddress cc) addresses == map publicationAddress cc
          && [p | p <- combined, publicationAddress p `elem` map publicationAddress cc, publicationAddress p /= Address 0x0061c3dc]
               == [p | p <- cc, publicationAddress p /= Address 0x0061c3dc])
      check "the carried word keeps phase and old word with new 0x83d"
        (map (\p -> (publicationPhase p, publicationOld p, publicationNew p)) (at 0x0061c3dc) == [(Activation, 0x49c, 0x83d)])
      check "setup is the CC setup then the icon setup, then activation only" (take ccSetupCount combined == ccSetup && all ((== Activation) . publicationPhase) activation)
      check "icon setup is image-2 record and per-mille words"
        (not (null iconSetup) && all (\p -> let Address a = publicationAddress p in (a >= 0x0037b7c4 && a < 0x0037b808) || a == 0x00438268) iconSetup)
      let image2After = publish image2Base rc0Image2 combined
          recordOffset = 0x0037b7c6 - fromIntegral image2Base
      check "image-2 words write the 0x83d record and keep the bytes around it"
        (B.take 66 (B.drop recordOffset image2After) == label83dRecord
          && B.take recordOffset image2After == B.take recordOffset rc0Image2
          && B.drop (recordOffset + 66) image2After == B.drop (recordOffset + 66) rc0Image2)
      let fontBytes = B.map complement (B.take 0x80000 (B.drop 0x0106025c (datWith stockFont)))
          fontAfter = publish fontBase fontBytes combined
          directory = either (const 0) id (directoryWord (allocationRecord allocation))
      check "directory bytes name the allocation record with flags 1"
        (B.unpack (B.take 4 (B.drop 0x3c21d fontAfter)) == [fromIntegral (directory `shiftR` s) | s <- [0, 8, 16, 24]]
          && directory .&. 0xff000000 == 0x01000000)
      check "per-mille 0x0320 and the next halfword unchanged"
        (B.unpack (B.take 4 (B.drop 0x7c26b fontAfter)) == [0x20, 0x03, 0x00, 0x00] && length (at 0x0043826c) == 0)
      check "font bytes outside the directory word and per-mille unchanged"
        (B.take 0x3c21d fontAfter == B.take 0x3c21d fontBytes && B.drop 0x7c26d fontAfter == B.drop 0x7c26d fontBytes)
      check "label-icon words 0x0061c320 and 0x0061c324 go from 0 to 0x83d in activation"
        (map (\p -> (publicationPhase p, publicationOld p, publicationNew p)) (concatMap at [0x0061c320, 0x0061c324]) == replicate 2 (Activation, 0, 0x83d))
      check "Q labels pointer precedes Q values pointer, both naming the allocation copies"
        (position 0x0078b024 < position 0x0078b020
          && map publicationNew (at 0x0078b024) == [let Address a = allocationQLabels allocation in a]
          && map publicationNew (at 0x0078b020) == [let Address a = allocationQValues allocation in a])
      check "directory words precede the label-icon and Q words in activation"
        (maximum (map position [0x003f821c, 0x003f8220]) < minimum (map position [0x0061c320, 0x0078b024]))
      check "the live-view words end activation in fragment order"
        (map (\p -> (publicationAddress p, publicationOld p, publicationNew p)) (drop (length combined - 5) combined)
          == [(liveViewAddress w, liveViewCurrent w, liveViewNew w) | w <- liveView])

  refused "a CC publication at the Q labels pointer is refused" (run stockFont (ccList [(0x0061c3dc, 0x49c, 0), (0x0078b024, 0x00789e2c, 1)]) rc0)
  refused "a CC publication at label-icon word 0x0061c320 is refused" (run stockFont (ccList [(0x0061c3dc, 0x49c, 0), (0x0061c320, 0, 1)]) rc0)
  refused "a CC list without the carried word is refused" (run stockFont (ccList []) rc0)
  refused "a font whose U+F073 directory word is not stock is refused" (run [(0x3c21d, [0x81, 0xd6, 0x00, 0x00]), (0x7c26b, [0xe8, 0x03])] cc rc0)
  refused "a font whose U+F073 per-mille is not stock is refused" (run [(0x3c21d, [0x80, 0xd6, 0x00, 0x00]), (0x7c26b, [0x20, 0x03])] cc rc0)
  refused "an RC0 image 5 that differs from the live-view current words is refused" (run stockFont cc rc0 {rc0Image5 = B.replicate 0x00400000 0})
  refused "a stock Q pointer that differs is refused" (run stockFont cc rc0 {rc0Image4 = image 0x0061c340 (0x0089ec00 - 0x0061c340) [(0x0061c3dc, 0x49c)]})
  refused "a DAT without the FDL header is refused" (fontWindowFromDat (B.replicate 0x01100000 0))

  check "directory word for a record 0x00ffffff past the commands" (directoryWord (Address (0x003f9ced + 0x00ffffff)) == Right 0x01ffffff)
  refused "a record 0x01000000 past the commands is refused" (directoryWord (Address (0x003f9ced + 0x01000000)))
  refused "a record below the commands is refused" (directoryWord (Address 0x003f9cec))

  check "content holds the record, the Q values and the Q labels at their allocation offsets"
    (B.take (B.length iconGlyphRecord) iconContent == iconGlyphRecord
      && B.take (B.length qValuesArray) (B.drop 0x178 iconContent) == qValuesArray
      && B.drop 0x1c0 iconContent == qLabelsArray
      && fromIntegral (B.length iconContent) == iconContentSize)
  check "allocation addresses follow the installer at every 8-byte-aligned base"
    (and [ ccInstallerStart b + ccIconInstallerExtent <= b + ccIconContentOffset
             && allocationQLabels (iconAllocation b) == Address (b + ccIconAllocationSize - fromIntegral (B.length qLabelsArray))
         | b <- [0x00c20000, 0x00c20008, 0x00c20010, 0x00c20018, 0x00a7e8c8] ])
  check "the allocation differs from the Classic Chrome payload's" (ccIconAllocationSize /= ccAllocationSize)

  found <- readIORef failures
  unless (null found) $ do
    mapM_ (hPutStrLn stderr . ("FAIL: " ++)) (reverse found)
    exitFailure
  putStrLn "classic-chrome-icon-ram tests: ok"
