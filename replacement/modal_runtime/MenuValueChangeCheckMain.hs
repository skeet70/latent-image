module Main (main) where

import Control.Monad (unless)
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Replacement.ModalRuntime.ValueChange (compareRoles, parseCorrespondence, readRoleRecord, verdictValue)
import System.Environment (getArgs)
import System.Exit (die, exitFailure)

usage :: String
usage = "usage: menu-value-change-check --correspondence FILE --stock RECEIPT --owned RECEIPT --output FILE"

main :: IO ()
main = do
  arguments <- getArgs
  (correspondencePath, stockPath, ownedPath, output) <- case arguments of
    ["--correspondence", correspondencePath, "--stock", stockPath, "--owned", ownedPath, "--output", output] ->
      pure (correspondencePath, stockPath, ownedPath, output)
    _ -> die usage
  table <- either die pure . (>>= parseCorrespondence) . J.eitherDecodeStrict' =<< B.readFile correspondencePath
  stock <- either (die . ("stock receipt: " ++)) pure =<< readRoleRecord stockPath
  owned <- either (die . ("owned receipt: " ++)) pure =<< readRoleRecord ownedPath
  let result@(passed, _) = compareRoles table stock owned
  BL.writeFile output (J.encode (verdictValue result))
  putStrLn ("menu-value-change-check: " ++ (if passed then "PASS" else "FAIL") ++ " " ++ output)
  unless passed exitFailure
