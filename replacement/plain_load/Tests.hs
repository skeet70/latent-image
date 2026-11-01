module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Set as S
import Data.Word (Word32)
import Fixtures
import Replacement.PlainLoad.Admission
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert passed message = unless passed (fail message)

refuses :: AdmissionError -> Either AdmissionError a -> Bool
refuses expected (Left actual) = actual == expected
refuses _ (Right _) = False

checkProof :: String -> B.ByteString -> IO ()
checkProof label bytes = do
  proof <- either (fail . ((label ++ ": ") ++) . show) pure (deriveProducerProof bytes)
  let inside pc = pc >= 0x6a9844 && toInteger pc+4 <= 0x6a9844+toInteger (B.length bytes) && pc `mod` 4 == 0
      executed = S.fromList (executionPcs proof)
      retainedTargets :: S.Set Word32
      retainedTargets = S.fromList [0x657114,0x6571a0,0x657184,0x6a8f1c,0x73b924,0x724b68,0x724c00,0x69b9c4]
  assert (scratchLow proof >= -4096 && scratchLow proof <= 0 && scratchLow proof `mod` 4 == 0) (label ++ ": scratch bound")
  assert (not (S.null executed) && all inside (executionPcs proof)) (label ++ ": executed carrier bounds")
  assert (S.member (refusalPc proof) executed) (label ++ ": missing NULL refusal PC")
  assert (not (null (returnPcs proof)) && all (`S.member` executed) (returnPcs proof)) (label ++ ": missing successful return PCs")
  assert (all (\(pc,_) -> S.member pc executed) (callSites proof)) (label ++ ": retained call PC not executed")
  assert (S.fromList (map snd (callSites proof)) == retainedTargets) (label ++ ": retained service coverage")

main :: IO ()
main = do
  arguments <- getArgs
  imagePath <- case arguments of
    [image] -> pure image
    [image,_symbols] -> pure image
    _ -> fail "usage: plain-load-tests LINKED_BIN [SYMBOLS]"
  image <- B.readFile imagePath
  checkProof "emitted plain producer" image
  forM_ [minBound .. maxBound] $ \fault -> do
    let bytes = fixture fault
    case expectedRefusal fault of
      Nothing -> checkProof (show fault) bytes
      Just expected -> assert (refuses expected (deriveProducerProof bytes)) ("typed refusal differs: " ++ show fault)
  forM_ [B.empty,B.replicate 3 0,B.replicate 245 0,B.replicate 248 0] $ \bytes ->
    assert (refuses InvalidCarrier (deriveProducerProof bytes)) "invalid carrier extent refusal differs"
  putStrLn "plain producer: emitted image, complete semantic controls, finite source corpus and typed refusals pass"
