{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified FujiAF.Host as H
import Replacement.Saclac.QualificationLayout
import System.Environment (getArgs)

roots :: [RootSpec]
roots = [RootSpec "OWNER" "composition.rs", RootSpec "RECOGNITION" "composition.rs"]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [dwarfdump, identicalA, identicalB, fieldOrder, variantBase, variantSet, missingRoot] ->
      run dwarfdump identicalA identicalB fieldOrder variantBase variantSet missingRoot
    _ -> H.failH "expected: DWARFDUMP IDENTICAL_A IDENTICAL_B FIELD_ORDER VARIANT_BASE VARIANT_SET MISSING_ROOT"

run :: FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> FilePath -> IO ()
run dwarfdump identicalA identicalB fieldOrder variantBase variantSet missingRoot = do
  identical <- checkLayouts dwarfdump identicalA identicalB roots
  H.ensure (reportVerdict identical == "match")
    ("identical control was refused: " ++ show (reportMismatches identical))
  H.ensure (reportTypesCompared identical > 0) "identical control compared no types"

  order <- checkLayouts dwarfdump identicalA fieldOrder roots
  H.ensure (reportVerdict order == "mismatch")
    ("field-order control was accepted: " ++ show order)
  H.ensure (any (("member" `contains`) . mismatchPath) (reportMismatches order))
    ("field-order refusal did not name a member: " ++ show (reportMismatches order))

  variants <- checkLayouts dwarfdump variantBase variantSet roots
  H.ensure (reportVerdict variants == "mismatch")
    ("variant-set control was accepted: " ++ show variants)
  H.ensure (any (("enumerator" `contains`) . mismatchDetail) (reportMismatches variants))
    ("variant-set refusal did not name an enumerator: " ++ show (reportMismatches variants))

  missing <- checkLayouts dwarfdump identicalA missingRoot roots
  H.ensure (reportVerdict missing == "missing")
    ("missing-root control was accepted: " ++ show missing)
  H.ensure (not (null (reportMismatches missing))) "missing root produced no mismatch"

contains :: String -> String -> Bool
contains needle haystack = any (needle `prefix`) (suffixes haystack)
  where
    prefix a b = take (length a) b == a
    suffixes [] = [[]]
    suffixes xs@(_ : rest) = xs : suffixes rest
