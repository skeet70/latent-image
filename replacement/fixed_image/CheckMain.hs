{-# LANGUAGE OverloadedStrings #-}
module Main where

import qualified ArmElf as E
import qualified BankAllowlist as A
import Control.Monad (forM_)
import Data.Aeson (encode, object, (.=))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (sortOn)
import Data.Word (Word32)
import qualified EngineStack as S
import qualified FujiAF.Host as H
import Replacement.FixedImage.Layout
import Replacement.FixedImage.Record
import System.Environment (getArgs)
import Text.Read (readMaybe)

newtype EntryName = EntryName String
newtype OwnedStackBudget = OwnedStackBudget Word32

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [allowlistPath, elfPath, bodyPath, entryName, budgetText, output] -> do
      budget <- case readMaybe budgetText :: Maybe Integer of
        Just value | value >= 0 && value <= toInteger (maxBound :: Word32) -> pure (OwnedStackBudget (fromInteger value))
        _ -> H.failH "invalid owned stack budget"
      check allowlistPath elfPath bodyPath (EntryName entryName) budget output
    _ -> H.failH "usage: check-fixed-image ALLOWLIST ELF BODY ENTRY OWNED_STACK_BYTES OUTPUT_JSON"

check :: FilePath -> FilePath -> FilePath -> EntryName -> OwnedStackBudget -> FilePath -> IO ()
check allowlistPath elfPath bodyPath (EntryName entryName) (OwnedStackBudget budget) output = do
  allowlist <- H.readRegularWith (pure ()) False 0x100000 allowlistPath >>= either H.failH pure . A.parseAllowlist
  elfBytes <- H.readRegularWith (pure ()) False 0x400000 elfPath
  elf <- either H.failH pure (E.parseElf elfBytes)
  body <- H.readRegularWith (pure ()) False 0x40000 bodyPath
  let symbol name = case [E.symbolValue value | value <- E.elfSymbols elf, E.symbolName value == name, E.symbolSection value /= E.Undefined] of
        [value] -> pure value
        _ -> H.failH ("missing or duplicate fixed symbol " ++ name)
      allocated = sortOn E.sectionAddress (filter E.sectionAllocated (E.elfSections elf))
      backed = filter ((== 1) . E.sectionType) allocated
      code = filter E.sectionExecutable allocated
      end section = toInteger (E.sectionAddress section) + toInteger (E.sectionSize section)
      inside address = any (\section -> E.sectionAddress section <= address && toInteger address < end section) code
  first <- symbol "__fixed_body_start"
  lastByte <- symbol "__fixed_body_end"
  bssFirst <- symbol "__fixed_bss_start"
  bssLast <- symbol "__fixed_bss_end"
  _ <- either H.failH pure (makeRecord (BuildId 0) (BodyBounds first lastByte) (Bss bssFirst bssLast) body)
  H.ensure (not (null backed) && not (null code) && minimum (map E.sectionAddress backed) == first
    && maximum (map end backed) == toInteger lastByte) "fixed file sections do not cover body bounds"
  forM_ allocated $ \section -> do
    H.ensure (E.sectionAddress section >= first && end section <= toInteger imageLimit
      && E.sectionType section `elem` [1,8]
      && not (E.sectionWritable section && E.sectionExecutable section)) "fixed section leaves layout or has invalid permissions"
    if E.sectionType section == 8 then
      H.ensure (E.sectionAddress section >= bssFirst && end section <= toInteger bssLast
        && not (E.sectionExecutable section)) "fixed NOBITS section leaves BSS"
    else do
      H.ensure (end section <= toInteger lastByte) "fixed file section leaves body"
      forM_ [0 .. toInteger (E.sectionSize section) - 1] $ \offset -> do
        let address = E.sectionAddress section + fromInteger offset
        H.ensure (E.byteAt elf address == Just (B.index body (fromIntegral (address-first)))) "body bytes differ from ELF link"
  forM_ (zip allocated (drop 1 allocated)) $ \(left,right) ->
    H.ensure (end left <= toInteger (E.sectionAddress right)) "fixed sections overlap"
  H.ensure (all (\segment -> not (E.loadSegment segment && E.segmentWritable segment && E.segmentExecutable segment)) (E.elfSegments elf)) "fixed load segment is writable and executable"
  H.ensure (all (\value -> take 2 (E.symbolName value) /= "$t" || not (inside (E.symbolValue value))) (E.elfSymbols elf)) "fixed image contains Thumb code"
  report <- either H.failH pure (A.checkImageInterfaces allowlist elf (A.InterfaceLayout
    (A.AddressRange (imageBase+headerBytes) imageLimit)
    [A.AddressRange (E.sectionAddress section) (fromInteger (end section)) | section <- code]
    (map E.sectionName allocated) []))
  depth <- either (H.failH . show) pure (S.stackDepth elf inside entryName)
  bodyDigest <- H.sha256 body
  elfDigest <- H.sha256 elfBytes
  B.writeFile output (L.toStrict (encode (object
    ["interface" .= A.reportValue report, "owned_stack_bytes" .= depth, "owned_stack_budget" .= budget
    ,"body_sha256" .= bodyDigest, "elf_sha256" .= elfDigest])))
  mapM_ (putStrLn . A.renderFinding) (A.violations report)
  H.ensure (null (A.violations report)) "fixed image interface admission failed"
  H.ensure (depth <= budget) "fixed image owned stack budget exceeded"
  putStrLn ("fixed image admitted: " ++ show (A.reportInstructions report) ++ " instructions, "
    ++ show (A.reportProvenBranches report) ++ " direct transfers, " ++ show depth ++ " owned stack bytes")
