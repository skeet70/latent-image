-- Stage an owned record-2 startup image into a reviewed X-M1 container.
module Main where

import FujiAF.FirstStage
import SecondaryStageContract
import StageBuild

main :: IO ()
main = runStage (StageContract SecondRecord secondaryDeclarations)
