module Main (main) where

import Control.Monad (unless)
import Data.Either (isLeft)
import qualified FujiAF.RamPush as Ram
import Replacement.RtosTimer.BankPackage (BankAddress (..))
import Replacement.RtosTimer.CombinedPublication (composePublicationVector, applyCarrierPublication, PublicationDirection (..))
import qualified FujiAF.UpdateContainer as U
import Replacement.PropertyService.Placement (GuestAddress (..))

main :: IO ()
main = do
  rows <- either fail pure (composePublicationVector arms original delta)
  let normalize row = row {Ram.publicationIndex = 0}
      preserved = filter ((`notElem` obsolete) . Ram.publicationAddress) original
      (setup, activation) = splitAt 113 preserved
  check "remaining bank publications or activation order changed"
    (map normalize rows == map normalize (setup ++ delta ++ activation))
  check "publication indices differ" (map Ram.publicationIndex rows == [0..length rows - 1])
  let logicalIndices = zipWith (\index row -> row {Ram.publicationIndex = index}) ([55..227] ++ [0..54]) original
  check "logical source indices changed installer order" (composePublicationVector arms logicalIndices delta == Right rows)
  reject "duplicate source publication index accepted" (alter 0 (\row -> row {Ram.publicationIndex = 1}) original) delta
  reject "missing obsolete table write accepted" (drop 1 original) delta
  reject "wrong obsolete target accepted" (alter 0 (\row -> row {Ram.publicationNew = Ram.WordValue 0x1104}) original) delta
  reject "obsolete write with wrong core accepted" (alter 0 (\row -> row {Ram.publicationCore = Ram.Cpu0}) original) delta
  reject "obsolete write with fabricated core coverage accepted" (alter 0 (\row -> row {Ram.publicationCore = Ram.Both}) original) delta
  reject "obsolete write moved to activation accepted" (alter 0 (\row -> row {Ram.publicationPhase = Ram.Activation}) original) delta
  reject "nonobsolete bank phase changed" (alter 20 (\row -> row {Ram.publicationPhase = Ram.Activation}) original) delta
  reject "bank address collision accepted" (alter 20 (\row -> row {Ram.publicationAddress = Ram.publicationAddress (original !! 21)}) original) delta
  reject "property bank collision accepted" original [deltaWord {Ram.publicationAddress = Ram.publicationAddress (original !! 20)}]
  reject "duplicate property word accepted" original (delta ++ delta)
  reject "unaligned property word accepted" original [deltaWord {Ram.publicationAddress = Ram.Address 0x62c121}]
  reject "activation property word accepted" original [deltaWord {Ram.publicationPhase = Ram.Activation}]
  reject "unchanged property word accepted" original [deltaWord {Ram.publicationNew = Ram.publicationOld deltaWord}]
  let base = GuestAddress 0x62c120
      before = U.putWord32 0xe1a00000
      after = U.putWord32 0xe12fff1e
  check "property delta bytes differ" (applyCarrierPublication base before delta InstallPublication == Right after)
  check "inverse did not recover canonical bytes" (applyCarrierPublication base after delta RollbackPublication == Right before)
  check "stale canonical preimage accepted" (isLeft (applyCarrierPublication base after delta InstallPublication))
  check "corrupt recipe inverse accepted" (isLeft (applyCarrierPublication base (U.putWord32 0) delta RollbackPublication))
  check "out-of-range carrier word accepted" (isLeft (applyCarrierPublication (GuestAddress 0x62c124) before delta InstallPublication))
  putStrLn "combined publication behavior controls passed"
  where
    check message ok = unless ok (fail message)
    reject message bank property = check message (isLeft (composePublicationVector arms bank property))
    alter index change rows = take index rows ++ [change (rows !! index)] ++ drop (index + 1) rows

arms :: (BankAddress, BankAddress)
arms = (BankAddress 0x1000, BankAddress 0x1100)

obsolete :: [Ram.Address]
obsolete = map Ram.Address ([0x61eac4,0x61eac8 .. 0x61eae4] ++ [0x625064,0x625068 .. 0x625084])

original :: [Ram.Publication]
original = zipWith (\index row -> row {Ram.publicationIndex = index}) [0..]
  ([(word Ram.Setup site target) {Ram.publicationCore = Ram.UnknownCore} | (site, target) <- zip obsolete (replicate 9 0x1000 ++ replicate 9 0x1100)]
  ++ [word Ram.Setup (Ram.Address site) (site + 4) | site <- [0x800000,0x800004 .. 0x8001c0]]
  ++ [word Ram.Activation (Ram.Address site) (site + 4) | site <- [0x900000,0x900004 .. 0x900180]])
  where word phase site value = Ram.Publication 0 phase site (Ram.WordValue 0) (Ram.WordValue value) Ram.Both

delta :: [Ram.Publication]
delta = [deltaWord]

deltaWord :: Ram.Publication
deltaWord = Ram.Publication 0 Ram.Setup (Ram.Address 0x62c120) (Ram.WordValue 0xe1a00000) (Ram.WordValue 0xe12fff1e) Ram.Both
