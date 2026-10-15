module UpdaterRoute
  ( UpdaterBinding
  , EqualVersionPolicy (..)
  , bindUpdaterRoute
  , updaterImage4, updaterImage5
  , updaterBootPollerAddress, updaterDispBranchAddress
  , updaterPermissionAddress, updaterModeAddress, updaterBodyPolicyAddress, updaterUiInitializerAddress
  , updaterVersionPolicy, updaterSourceDigest
  ) where

import Control.Monad (forM_)
import Data.Bits ((.&.), (.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import qualified FujiAF.Host as H
import qualified FujiAF.UpdateContainer as U
import Data.Word (Word32)
import RecoveryFirmware

data EqualVersionPolicy = StockEqualVersion | RamEqualVersionExtension deriving (Eq, Show)
data UpdaterFamily = Xm101 | Xm120 | Xm150
newtype InstructionOffset = InstructionOffset Int
newtype CodeDigest = CodeDigest String deriving (Eq)

-- Inputs are the strict decoder's outputs for this DAT, before other RAM patches.
data UpdaterBinding = UpdaterBinding
  { updaterImage4 :: B.ByteString
  , updaterImage5 :: B.ByteString
  , updaterBootPollerAddress :: RamAddress
  , updaterDispBranchAddress :: RamAddress
  , updaterBodyPolicyAddress :: RamAddress
  , updaterUiInitializerAddress :: RamAddress
  , updaterPermissionAddress :: RamAddress
  , updaterModeAddress :: RamAddress
  , updaterVersionPolicy :: EqualVersionPolicy
  , updaterSourceDigest :: FirmwareDigest
  }

matches :: B.ByteString -> Int -> [(Int, Word32)] -> Bool
matches bytes offset wordsAt = offset >= 0 && all one wordsAt
  where
    one (at, expected) = offset + at + 4 <= B.length bytes && U.word32At bytes (offset + at) == expected

unique :: B.ByteString -> Int -> [(Int, Word32)] -> IO InstructionOffset
unique bytes extent wordsAt = case [offset | offset <- [0,4..B.length bytes-extent], matches bytes offset wordsAt] of
  [offset] -> pure (InstructionOffset offset)
  _ -> H.failH "Missing or ambiguous source-bound updater instruction pattern"

branch :: B.ByteString -> InstructionOffset -> IO InstructionOffset
branch bytes (InstructionOffset offset) = do
  let word = U.word32At bytes offset
      immediate = toInteger (word .&. 0xffffff)
      signed = if immediate >= 0x800000 then immediate - 0x1000000 else immediate
      target = toInteger offset + 8 + 4 * signed
  H.ensure (word .&. 0xff000000 == 0xeb000000 && target >= 0 && target + 4 <= toInteger (B.length bytes)) "Updater call is not an in-image A32 BL"
  pure (InstructionOffset (fromInteger target))

movImmediate :: Word32 -> Word32
movImmediate word = ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff)

movPair :: B.ByteString -> InstructionOffset -> IO Word32
movPair bytes (InstructionOffset offset) = do
  H.ensure (offset >= 0 && offset + 8 <= B.length bytes) "Updater address materialization is outside image"
  let low = U.word32At bytes offset
      high = U.word32At bytes (offset+4)
  H.ensure (low .&. 0xfff0f000 == 0xe3003000 && high .&. 0xfff0f000 == 0xe3403000) "Updater pointer is not a MOVW/MOVT r3 pair"
  pure (movImmediate low .|. (movImmediate high `shiftL` 16))

address :: RamAddress -> InstructionOffset -> IO RamAddress
address (RamAddress base) (InstructionOffset offset) = do
  H.ensure (offset >= 0 && toInteger base + toInteger offset <= 0xffffffff) "Updater address overflows"
  pure (RamAddress (base + fromIntegral offset))

pin :: B.ByteString -> InstructionOffset -> ByteCount -> CodeDigest -> IO ()
pin bytes (InstructionOffset offset) count expected = do
  region <- slice bytes (FileOffset offset) count
  digest <- CodeDigest <$> H.sha256 region
  H.ensure (digest == expected) "Updater code window differs from the inspected firmware family"

replace :: B.ByteString -> InstructionOffset -> [Word32] -> B.ByteString
replace bytes (InstructionOffset offset) wordsAt = B.take offset bytes <> B.concat (map U.putWord32 wordsAt) <> B.drop (offset+4*length wordsAt) bytes

family :: FirmwareVersion -> IO UpdaterFamily
family (FirmwareVersion major minor) = case (major,minor) of
  (1,1) -> pure Xm101
  (1,0x20) -> pure Xm120
  (1,0x50) -> pure Xm150
  _ -> H.failH "No inspected updater execution family for this DAT version"

pollerDigest :: UpdaterFamily -> CodeDigest
pollerDigest Xm101 = CodeDigest "19cb19934c6f37e63fbe8eeb352b5bef0362efab4f69f2621772aaed7fd5ea2c"
pollerDigest Xm120 = CodeDigest "f9f430429b4c556c58789cf4bbe1837dc9a50806ebc030d5c15d227f51c8b162"
pollerDigest Xm150 = CodeDigest "b8857e22e8c87effd41b3ed4d22f26cbb92bd2d0d2ce5281fd4235955bee4c05"

policyDigest :: UpdaterFamily -> CodeDigest
policyDigest Xm101 = CodeDigest "c2256ec6fd4c3965f2103f56d54fbc8dd3bb70ee39289e15a948324568eb354d"
policyDigest Xm120 = CodeDigest "de08583b37fe2dd4869b3024fdbbe478dd4c129ab1bdbd5f4fee08e4f9dddfe4"
policyDigest Xm150 = CodeDigest "3fca3b9899de56744e1c4fcf4c12e938e1e1deb931cdb610b11b9b069d50b045"

initializerDigest :: UpdaterFamily -> CodeDigest
initializerDigest Xm101 = CodeDigest "1e169e7e415d5bb5621af4e8e1ea9d0d9b4566f139db10df5422e5df9cd4410f"
initializerDigest Xm120 = CodeDigest "cd7b8f7e15c248e32538de406b0dc7007bffedbbb584ad48343e9e6afb1b34f3"
initializerDigest Xm150 = CodeDigest "210f35252c36dd5bc12b9f4bf5db4ab86b55d7c693cc0d0d73e414073ff8fd25"

requestDigest :: UpdaterFamily -> CodeDigest
requestDigest Xm101 = CodeDigest "98304cd4124b2e35f8b973429c4f1e79063b2707fc7410b99523b3009f304a96"
requestDigest Xm120 = CodeDigest "5ad0a4da0e7af4ee5fbf04b2c38ad17fdb377691a3e307f0fd27d97e2a8ec8d2"
requestDigest Xm150 = CodeDigest "0062a888325de0dfe3d8875d80cac3289f8a534f6c27ae7befdd965f9ad7efd0"

selectorDigest :: UpdaterFamily -> CodeDigest
selectorDigest Xm101 = CodeDigest "6fe2070b5fb53151c9544511c5fdd98f459249c703e5c546283d5b498b0ea49c"
selectorDigest Xm120 = CodeDigest "ab6e8888d528fb66169c5fd61377e97a62f4714fe4e1d5989d8b1f6ccec5d950"
selectorDigest Xm150 = CodeDigest "c643f0a462ae5e5ba8708536928fd19133a5a8282fb23af9b3052444a1edf9b8"

modeCaseDigest :: UpdaterFamily -> CodeDigest
modeCaseDigest Xm101 = CodeDigest "02c4391282506c0956a8b8cd310615aee42f1d0780def7b761c9b83cd6f3ffdf"
modeCaseDigest Xm120 = CodeDigest "def373a1ad9978d7388075140ab70d49a657b6f41d2594a8182704a1055d8f0d"
modeCaseDigest Xm150 = CodeDigest "22deb6797a54f0818776337d6319b55d840610e794f7fb7df2b92dfcbeb81ba4"

checkImage :: Firmware -> SegmentKind -> RamAddress -> B.ByteString -> IO ()
checkImage firmware kind (RamAddress base) bytes = case [entry | Segment candidate entry <- firmwareSegments firmware, candidate == kind] of
  [entry] -> do
    H.ensure (U.segFlag entry == 1 && U.segLoad entry == base) "Updater decode load address differs from its DAT"
    header <- B.map (\value -> 255-value) <$> slice (firmwareMain firmware) (FileOffset (fromIntegral (U.segFlash entry))) (ByteCount 20)
    H.ensure (fromIntegral (U.word32At header 0) == B.length bytes) "Updater decode length differs from its DAT"
  _ -> H.failH "Updater DAT segment is missing or ambiguous"

-- Only the held-DISP branch and the 1.01 equal-minor comparison change in RAM.
bindUpdaterRoute :: Firmware -> (RamAddress, B.ByteString) -> (RamAddress, B.ByteString) -> IO UpdaterBinding
bindUpdaterRoute firmware (base4,image4) (base5,image5) = do
  executionFamily <- family (firmwareVersion firmware)
  checkImage firmware Application4 base4 image4
  checkImage firmware Application5 base5 image5
  InstructionOffset initializer <- unique image5 52
    [ (0,0xe3530008), (4,0x1a00000a), (16,0xe3a02008)
    , (20,0xe1c320b0), (28,0xe3a00014), (44,0xe3a0003c), (48,0xe8bd8800)
    ]
  pin image5 (InstructionOffset initializer) (ByteCount 52) (initializerDigest executionFamily)
  _ <- movPair image5 (InstructionOffset (initializer+8))
  forM_ [24,32,40] $ \offset -> do
    _ <- branch image5 (InstructionOffset (initializer+offset))
    pure ()
  request <- branch image5 (InstructionOffset (initializer+36))
  pin image5 request (ByteCount 44) (requestDigest executionFamily)
  InstructionOffset entry <- unique image4 48
    [ (8,0xe5d33029), (12,0xe3130002), (16,0x0a000006)
    , (20,0xe3a00009), (28,0xe3500001), (32,0x1a000002)
    , (36,0xe3a0000a), (44,0xe8bd8818)
    ]
  pin image4 (InstructionOffset (entry-0x19c)) (ByteCount 0x1f4) (pollerDigest executionFamily)
  permission <- movPair image4 (InstructionOffset entry)
  InstructionOffset setter <- branch image4 (InstructionOffset (entry+40))
  pin image4 (InstructionOffset setter) (ByteCount 36) (selectorDigest executionFamily)
  _ <- branch image4 (InstructionOffset (entry+24))
  H.ensure (matches image4 setter [(0,0xe92d4800),(4,0xe28db004),(24,0xe2400001),(28,0xe350000f),(32,0x979ff100)]) "Updater selector function ABI differs"
  let modeCase = setter + 86*4
  pin image4 (InstructionOffset modeCase) (ByteCount 60) (modeCaseDigest executionFamily)
  modeBase <- movPair image4 (InstructionOffset modeCase)
  H.ensure (matches image4 modeCase [(12,0xe7931002),(16,0xe3510007),(20,0x88bd8800),(24,0xe3a02001),(28,0xe1a03112),(32,0xe3130099),(44,0x13a01008),(52,0x17831002),(56,0xe8bd8800)]) "Updater selector-10 mask or mode-8 store differs"
  modeCaseAddress <- address base4 (InstructionOffset modeCase)
  let RamAddress caseAddress = modeCaseAddress
      modeOffsetWord = U.word32At image4 (modeCase+8)
  H.ensure (U.word32At image4 (setter+19*4) == caseAddress && modeOffsetWord .&. 0xfff0f000 == 0xe3002000) "Updater jump-table selector-10 binding differs"
  H.ensure (U.word32At image4 (modeCase+36) == (U.word32At image4 modeCase .&. 0x0fffffff) .|. 0x10000000 && U.word32At image4 (modeCase+40) == (U.word32At image4 (modeCase+4) .&. 0x0fffffff) .|. 0x10000000 && U.word32At image4 (modeCase+48) == (modeOffsetWord .&. 0x0fffffff) .|. 0x10000000) "Updater conditional store changes address"
  let mode = toInteger modeBase + toInteger (movImmediate modeOffsetWord)
  H.ensure (mode <= 0xffffffff) "Updater mode address overflows"
  InstructionOffset policy <- unique image5 0x8c
    [(0,0xe52db004),(4,0xe28db000),(20,0xe7933002),(28,0x1a00001a),(40,0xe5932210),(44,0xe3720001),(48,0x1a000005),(60,0xe5933214),(64,0xe3730001),(68,0x03a00001),(84,0xe5933218),(88,0xe1520003),(92,0x83a00001),(100,0x13a00000),(116,0xe5930214),(120,0xe593321c),(124,0xe1500003)]
  pin image5 (InstructionOffset policy) (ByteCount 0x8c) (policyDigest executionFamily)
  let selector = case executionFamily of Xm150 -> 0xe3530000; _ -> 0xe3530001
  H.ensure (U.word32At image5 (policy+24) == selector) "Updater body selector differs"
  forM_ [8,32,52,76,108] $ \offset -> do
    _ <- movPair image5 (InstructionOffset (policy+offset))
    pure ()
  pollerAddress <- address base4 (InstructionOffset (entry-0x19c))
  entryAddress <- address base4 (InstructionOffset (entry+32))
  policyAddress <- address base5 (InstructionOffset policy)
  initializerAddress <- address base5 (InstructionOffset initializer)
  let (policyResult,patched5) = case executionFamily of
        Xm101 -> (RamEqualVersionExtension,replace image5 (InstructionOffset (policy+128)) [0x33a00000,0x23a00001])
        _ -> (StockEqualVersion,image5)
  pure UpdaterBinding
    { updaterImage4 = replace image4 (InstructionOffset (entry+32)) [0xe320f000]
    , updaterImage5 = patched5
    , updaterBootPollerAddress = pollerAddress
    , updaterDispBranchAddress = entryAddress
    , updaterBodyPolicyAddress = policyAddress
    , updaterUiInitializerAddress = initializerAddress
    , updaterPermissionAddress = RamAddress (permission+0x29)
    , updaterModeAddress = RamAddress (fromInteger mode)
    , updaterVersionPolicy = policyResult
    , updaterSourceDigest = firmwareDigest firmware
    }
