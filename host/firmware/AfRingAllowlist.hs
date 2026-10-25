module AfRingAllowlist (checkAfRing) where

import qualified A32 as A
import ArmElf
import BankAllowlist (Allowlist (..), StockCall (..), Declaration (..), Access (..), parseAllowlist)
import Control.Monad (unless, forM_)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.List (isPrefixOf, sortOn)
import Data.Word (Word32)

-- Indexed ring and callback-output stores retain the declarations in the allowlist.
checkAfRing :: Word32 -> B.ByteString -> B.ByteString -> Either String ()
checkAfRing base serializedElf allowlistBytes = do
  elf <- parseElf serializedElf
  policy <- parseAllowlist allowlistBytes
  unless (null (stockStores policy)) (Left "AF content permits no stock stores")
  unless (map stockCallAddress (stockCalls policy) == continuations) (Left "AF continuation allowlist changed")
  unless (map declarationFunction (declarations policy) == ["af_ring_record", "af_ring_synthetic", "af_ring_synthetic"] && all ((== StoreAccess) . declarationAccess) (declarations policy)) (Left "AF store declarations changed")
  let allocated = filter sectionAllocated (elfSections elf)
  forM_ allocated $ \section ->
    unless (sectionAddress section >= base && toInteger (sectionAddress section) + toInteger (sectionSize section) <= toInteger base + 0x6800)
      (Left ("allocated section escapes AF allocation: " ++ sectionName section))
  text <- case [s | s <- allocated, sectionName s == ".text"] of
    [s] -> Right s
    _ -> Left "one AF content text section required"
  unless (sectionAddress text == base && sectionSize text <= 0x1000) (Left "AF content text window changed")
  let functions = [s | s <- elfSymbols elf, symbolKind s == FunctionSymbol, symbolValue s >= base, symbolValue s < base + 0x1000]
      names = map symbolName functions
  unless (all (`elem` admittedFunctions) names && all (`elem` names) requiredFunctions) (Left "AF content function set changed")
  let mappings = sortOn symbolValue [s | s <- elfSymbols elf, "$a" `isPrefixOf` symbolName s || "$d" `isPrefixOf` symbolName s]
      isCode address = case takeWhile ((<= address) . symbolValue) mappings of
        [] -> False
        previous -> "$a" `isPrefixOf` symbolName (last previous)
      owner address = case [symbolName f | f <- functions, symbolValue f <= address, address < symbolValue f + symbolSize f] of
        [name] -> Right name
        _ -> Left "content instruction has no unique function owner"
      within address = base <= address && address < base + sectionSize text
  forM_ [base, base + 4 .. base + sectionSize text - 4] $ \address ->
    if not (isCode address) then Right () else do
      name <- owner address
      word <- maybe (Left "AF content instruction is outside ELF bytes") Right (wordAt elf address)
      let refuse = Left ("AF instruction outside allowlist at " ++ show address ++ " in " ++ name ++ ": " ++ show (A.decode address word))
      case A.decode address word of
        A.Branch _ _ target -> unless (within target) refuse
        A.BranchExchange _ _ register -> unless (register == A.lr) refuse
        A.Load _ _ _ destination register (A.OffsetImmediate displacement) _ | destination == A.pc -> do
          unless (register == A.pc && displacement == -4) refuse
          target <- maybe (Left "AF continuation literal is outside ELF bytes") Right (wordAt elf (address + 4))
          unless (target `elem` continuations) refuse
        A.Load _ _ _ destination _ _ _ -> unless (destination /= A.pc) refuse
        A.LoadMultiple _ register _ _ registers -> unless (A.pc `notElem` registers || register == A.sp) refuse
        A.Store _ _ _ register _ _ -> unless (register == A.sp || name `elem` ["af_ring_record", "af_ring_synthetic"]) refuse
        A.StoreMultiple _ register _ _ _ -> unless (register == A.sp || name `elem` ["af_ring_record", "af_ring_synthetic"]) refuse
        A.DataProcessing _ _ _ destination _ _ -> unless (destination /= A.pc) refuse
        A.Rejected A.Exclusive -> unless (name == "af_ring_record" && localExclusive word) refuse
        A.Rejected _ -> unless (systemWord name word) refuse
        _ -> Right ()
 where
  continuations = [0x00f41c50, 0x00f42ac4, 0x00f44ff4]
  requiredFunctions = ["af_ring_direction", "af_ring_peak", "af_ring_accepted", "af_ring_record", "af_ring_synthetic"]
  admittedFunctions = "ring" : requiredFunctions
  localExclusive word =
    word .&. 0x0ff00fff == 0x01900f9f
      || word .&. 0x0ff00ff0 == 0x01800f90
      || word == 0xf57ff01f
  systemWord name word =
    (name `elem` ["af_ring_direction", "af_ring_peak", "af_ring_accepted"]
      && (word .&. 0xffff0fff == 0xe10f0000 || word .&. 0xfffffff0 == 0xe128f000))
      || (name == "af_ring_record" && (word .&. 0xffff0fff == 0xee120e10 || word == 0xf57ff05f))
