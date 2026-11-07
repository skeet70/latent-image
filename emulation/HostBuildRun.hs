-- | @host-build COMMAND [ARGUMENT...]@ runs one Bazel command under host build
-- admission ("Emulation.HostAdmission").
module Main (main) where

import Emulation.HostAdmission (hostBuildMain)
import System.Environment (getArgs)

main :: IO ()
main = getArgs >>= hostBuildMain
