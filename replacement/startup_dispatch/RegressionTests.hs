module Main where

import Control.Monad (forM, unless)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified FujiAF.UpdateContainer as U
import Fixtures (Fault (..), carrier)
import Replacement.StartupDispatch.Artifact
import System.Environment (getArgs)

data EncodedZeroShift = Lsr32 | Rrx deriving (Eq, Show, Enum, Bounded)
newtype OperandOffset = OperandOffset Int

rejectShift :: B.ByteString -> B.ByteString -> OperandOffset -> EncodedZeroShift -> IO Bool
rejectShift bytes symbols (OperandOffset offset) shift = do
  let encoding = case shift of Lsr32 -> 0x20; Rrx -> 0x60
      word = U.word32At bytes offset .|. encoding
      mutated = B.take offset bytes <> U.putWord32 word <> B.drop (offset+4) bytes
      result = deriveArtifact mutated symbols
  putStrLn (show shift ++ " emitted operand: " ++ either show (const "admitted") result)
  pure $ case result of Left (CodeRejected _) -> True; _ -> False

main :: IO ()
main = do
  arguments <- getArgs
  (emittedBytes,emittedSymbols) <- case arguments of
    [imagePath,symbolsPath] -> (,) <$> B.readFile imagePath <*> B.readFile symbolsPath
    _ -> fail "expected emitted image and symbols"
  case deriveArtifact emittedBytes emittedSymbols of
    Left failure -> fail ("emitted control rejected: " ++ show failure)
    Right _ -> pure ()
  offset <- case [OperandOffset index | index <- [0,4 .. B.length emittedBytes-4],
    U.word32At emittedBytes index .&. 0xffff0ff0 == 0xe1a00000] of
      first:_ -> pure first
      [] -> fail "emitted control has no unshifted MOV operand"
  shifts <- mapM (rejectShift emittedBytes emittedSymbols offset) [minBound .. maxBound]
  outcomes <- forM [minBound .. maxBound] $ \fault -> do
    let (bytes,symbols) = carrier fault
        result = deriveArtifact bytes symbols
        admits = fault `elem` [Control,ScalarExtend,RotatedExtend,ConditionalExtend]
        expected = if admits then "admitted" else "CodeRejected"
        passed = case result of
          Right _ -> admits
          Left (CodeRejected _) -> not admits
          _ -> False
        actual = either show (const "admitted") result
    putStrLn (show fault ++ ": expected=" ++ expected ++ "; actual=" ++ actual)
    pure passed
  unless (and shifts && and outcomes) (fail "startup dispatcher admission regressions failed")
