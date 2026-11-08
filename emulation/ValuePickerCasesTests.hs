{-# LANGUAGE GHC2021 #-}
module Main where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.List (nub)
import Emulation.RenderCases
import Emulation.ValuePickerCases

main :: IO ()
main = do
  check "case ids are unique" (length names == length (nub names))
  check "each case remains within event bound" (all ((<=128) . length . pickerCalls) pickerCases)
  check "all report records have the caller ABI size" (all ((==68) . B.length . pickerReport) pickerCases)
  let early = named "mode-2"
  check "mode 2 preserves caller report" (pickerReport early == B.replicate 68 0xa5)
  check "mode 2 calls only local record clear" (map serviceAddress (pickerCalls early) == [ServiceAddress 0x43550])
  let centered = pickerReport (named "centered-short-list")
  check "centering reports two omitted rows" (B.index centered 0x3e == 2)
  let lastPage = pickerReport (named "page-3-cursor-5")
  check "last page selected ordinal starts at zero" (B.index lastPage 0x3c == 0)
  check "last page reports the child auxiliary" (B.take 4 (B.drop 0x40 lastPage) == wordsLE [0x705])
  check "paged caller disables scrolling" (pickerStackArguments (named "page-3-cursor-5") (ReportAddress 0x03000000) == wordsLE [0,2,0,0,1,0,0x03000000])
  check "scrolled caller enables scrolling" (pickerStackArguments (named "scroll-2") (ReportAddress 0x03000000) == wordsLE [0,2,0,1,1,0,0x03000000])
  let state = named "state-Hidden-hook-DrawLeaf"
      stateReport = pickerReport state
  check "hidden row is absent from visible cursor ordinal" (B.index stateReport 0x3c == 0)
  check "normal completion restores saved font as a service side effect" (case reverse (pickerCalls state) of c:_ -> serviceWrites c == [AbsoluteMemory 0x015e46c0 0 (wordsLE [9])]; _ -> False)
  let stock = pickerCalls (named "bracket-StockGate-label-2110")
      bank = pickerCalls (named "bracket-BankGate-label-2110")
      widthTargets = filter (`elem` [ServiceAddress 0x1112a6c,ServiceAddress 0x11391a0]) . map serviceAddress
  check "stock prepass and draw use the stock bound" (widthTargets stock == [ServiceAddress 0x11391a0,ServiceAddress 0x11391a0])
  check "bank prepass leaves draw gate unchanged" (widthTargets bank == [ServiceAddress 0x1112a6c,ServiceAddress 0x11391a0])
  let fn = named "fn-centered-short-list"
  check "Fn caller has its own geometry and return" (pickerX fn == 14 && pickerY fn == 3 && pickerReturnAddress fn == 0x0116c588)
  check "Fn caller passes the in-frame report" (pickerStackArguments fn (ReportAddress 0x0310104c) == wordsLE [0,2,0,1,1,0,0x0310104c])
  check "Fn centering report feeds the caller marker position" (B.index (pickerReport fn) 0x3e == 2)
  putStrLn "value-picker generator contract tests passed"
  where
    names = map pickerName pickerCases
    named name = case filter ((==name) . pickerName) pickerCases of [c] -> c; _ -> error ("missing case "++name)
    check name ok = unless ok (fail name)
