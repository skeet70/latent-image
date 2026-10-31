-- Stage an owned first-record image into a reviewed X-M1 container.
module Main where

import FujiAF.FirstStage
import StageBuild
import StartupStageContract (firstStageDeclarations)

main :: IO ()
main = runStage (StageContract FirstRecord firstStageDeclarations)
