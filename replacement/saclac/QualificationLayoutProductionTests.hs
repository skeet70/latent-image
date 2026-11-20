{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified FujiAF.Host as H
import Replacement.Saclac.QualificationLayout
import System.Environment (getArgs)

roots :: [RootSpec]
roots =
  [ RootSpec "OWNER" "saclac/root/src/stock/composition.rs"
  , RootSpec "RECOGNITION" "saclac/root/src/stock/composition.rs"
  ]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [dwarfdump, production, qualification] -> check dwarfdump production qualification
    _ -> H.failH "expected: DWARFDUMP PRODUCTION_ELF QUALIFICATION_ELF"

check :: FilePath -> FilePath -> FilePath -> IO ()
check dwarfdump production qualification = do
  report <- checkLayouts dwarfdump production qualification roots
  H.ensure (reportVerdict report == "match")
    ("layout check refused " ++ production ++ ": " ++ show (reportMismatches report))
  H.ensure (reportTypesCompared report > 0)
    ("layout check compared no types for " ++ production)
