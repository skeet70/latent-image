module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import Data.List (intercalate)
import Emulation.Sha256 (sha256, sha256Hex)
import System.Directory (setCurrentDirectory)
import System.Environment (getArgs, lookupEnv)

main :: IO ()
main = do
  workspace <- lookupEnv "BUILD_WORKSPACE_DIRECTORY"
  forM_ workspace setCurrentDirectory
  arguments <- getArgs
  case arguments of
    [source, destination] -> do
      image <- B.readFile source
      unless (sha256Hex (sha256 image) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 is not pinned X-M1 1.50")
      let window address count = B.take count (B.drop (address - 0x61c400) image)
          literal bytes = "[" ++ intercalate "," (map show (B.unpack bytes)) ++ "]"
          declaration name bytes = "#[no_mangle]\npub(crate) static " ++ name ++ ": [u8; " ++ show (B.length bytes) ++ "] = " ++ literal bytes ++ ";\n"
          tailDeclaration = "#[repr(C, align(2))]\npub(crate) struct TailDefaultBytes(pub [u8; 40]);\n#[no_mangle]\npub(crate) static TAIL_BYTES: TailDefaultBytes = TailDefaultBytes(" ++ literal (window 0x81b074 40) ++ ");\n"
      writeFile destination (declaration "SLOT_BYTES" (window 0x7cb7dc 200) ++ tailDeclaration)
    _ -> fail "expected IMAGE4 OUTPUT_RS"
