-- Stage the owned record-2 image that links the loader-B core-1 prefix, and
-- publish the prefix entry as loader B's header entry word.
module Main where

import FujiAF.FirstStage (BootRecord (SecondRecord))
import StageBuild
import StartupStageContract (loaderPrefixDeclarations)

main :: IO ()
main = runStage (StageContract SecondRecord loaderPrefixDeclarations)
