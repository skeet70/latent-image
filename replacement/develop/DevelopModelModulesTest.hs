{-# LANGUAGE GHC2021 #-}
module Main (main) where

import Data.List (sort)
import DevelopModelModules (modelModules)
import StageCatalogue (catalogueModules)

-- Fails when a module under model/ is not named by the stage catalogue, so a new model
-- module cannot be added without deciding whether it is a stage or a support module.
main :: IO ()
main = case [moduleName | moduleName <- sort modelModules,moduleName `notElem` catalogueModules] of
  [] -> pure ()
  missing -> fail ("model modules missing from StageCatalogue: " ++ unwords missing)
