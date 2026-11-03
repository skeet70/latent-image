module Main where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Set as S
import Fixtures
import Replacement.Image5Load.Admission
import System.Environment (getArgs)

assert :: Bool -> String -> IO ()
assert passed message = unless passed (fail message)

refuses :: AdmissionError -> Either AdmissionError a -> Bool
refuses expected (Left actual) = actual == expected
refuses _ (Right _) = False

checkProof :: String -> B.ByteString -> IO ()
checkProof label bytes = do
  proof <- either (fail . ((label ++ ": ") ++) . show) pure (deriveProducerProof bytes)
  let inside pc = pc >= 0x6a96c0 && toInteger pc+4 <= 0x6a96c0+toInteger (B.length bytes) && pc `mod` 4 == 0
      executed = S.fromList (executionPcs proof)
  assert (scratchLow proof >= -4096 && scratchLow proof <= 0 && scratchLow proof `mod` 4 == 0) (label ++ ": scratch bound")
  assert (not (S.null executed) && all inside (executionPcs proof)) (label ++ ": executed carrier bounds")
  assert (S.member (refusalPc proof) executed) (label ++ ": missing NULL refusal PC")
  assert (not (null (returnPcs proof)) && all (`S.member` executed) (returnPcs proof)) (label ++ ": missing successful return PCs")
  assert (all (\(pc,_) -> S.member pc executed) (callSites proof)) (label ++ ": retained call PC not executed")
  assert (S.fromList (map snd (callSites proof)) == S.fromList retainedTargets) (label ++ ": retained service coverage")

main :: IO ()
main = do
  arguments <- getArgs
  imagePath <- case arguments of
    [image] -> pure image
    [image,_symbols] -> pure image
    _ -> fail "usage: image5-load-tests LINKED_BIN [SYMBOLS]"
  image <- B.readFile imagePath
  checkProof "emitted image5 producer" image
  forM_ [minBound .. maxBound] $ \fault -> do
    let bytes = fixture fault
    case expectedRefusal fault of
      Nothing -> checkProof (show fault) bytes
      Just expected -> case deriveProducerProof bytes of
        Left actual | actual == expected -> pure ()
        result -> fail ("typed refusal differs: " ++ show fault ++ " expected " ++ show expected ++ " got "
          ++ either show (const "admission") result)
  forM_ [B.empty,B.replicate 3 0,B.replicate (carrierBytes+1) 0,B.replicate (carrierBytes+4) 0] $ \bytes ->
    assert (refuses InvalidCarrier (deriveProducerProof bytes)) "invalid carrier extent refusal differs"
  putStrLn "image5 producer: emitted image, semantic controls, finite source corpus and typed refusals pass"
