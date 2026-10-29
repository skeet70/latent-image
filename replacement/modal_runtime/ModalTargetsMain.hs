module Main where

import qualified Data.ByteString as B
import Data.Aeson (Value, object, (.=))
import Data.Word (Word16, Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import Numeric (readHex, showHex)
import Replacement.ModalRuntime.Targets
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

main :: IO ()
main = do
  arguments <- getArgs
  id5Path <- required arguments "--id5"
  bankText <- required arguments "--bank-interpose"
  output <- required arguments "--out"
  bank <- parseAddress bankText
  exists <- doesPathExist output
  if exists then failWith "output path already exists" else pure ()
  id5 <- B.readFile id5Path
  if B.length id5 /= id5SizeValue || sha256Hex (sha256 id5) /= id5Sha256
    then failWith "id5 size or digest differs from the pinned image"
    else case validateModal75 id5 bank of
      Left problem -> failWith problem
      Right report -> B.writeFile output (encodeCanonical (reportValue report))

reportValue :: TargetReport -> Value
reportValue report =
  object
    [ "controller" .= hex (reportController report)
    , "cell_slots" .= reportCellSlots report
    , "action_slots" .= map hex16 (reportActionSlots report)
    , "stock_targets" .= map hex (reportStockTargets report)
    , "bank_interpositions" .=
        [ object ["action_slot" .= hex16 (interpositionActionSlot entry), "target" .= hex (interpositionTarget entry)]
        | entry <- reportBankInterpositions report
        ]
    , "transition_targets" .= map hex (reportTransitionTargets report)
    , "prehook" .= hex (reportPrehook report)
    ]

hex :: Word32 -> String
hex value = let digits = showHex value "" in "0x" ++ replicate (8 - length digits) '0' ++ digits

hex16 :: Word16 -> String
hex16 value = let digits = showHex value "" in "0x" ++ replicate (4 - length digits) '0' ++ digits

required :: [String] -> String -> IO String
required arguments name =
  case dropWhile (/= name) arguments of
    (_ : value : _) -> pure value
    _ -> failWith ("missing " ++ name)

parseAddress :: String -> IO Word32
parseAddress text =
  case text of
    '0' : 'x' : digits
      | not (null digits)
      , [(word, "")] <- readHex digits -> pure (word :: Word32)
    _ -> failWith ("invalid address: " ++ text)

failWith :: String -> IO a
failWith message = do
  hPutStrLn stderr message
  exitFailure
