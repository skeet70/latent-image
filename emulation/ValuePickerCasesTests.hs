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
  let callerFrames =
        [ ("Picker9304",0x01169308,0x10ac,4,3,0,[0,2,0,1,1,0,0x031010ac])
        , ("Picker9a14",0x01169a18,0x1054,6,4,0,[0,2,0,1,1,0,0x03101054])
        , ("Pickera1b8",0x0116a1bc,0x1084,3,5,6,[0,2,3,1,1,0,0x03101084])
        , ("Pickera8b0",0x0116a8b4,0x105c,14,4,0,[0,2,0,1,1,0,0x0310105c])
        , ("Pickerb52c",0x0116b530,0x106c,1,4,0,[255,0,0,0,1,0,0x0310106c])
        , ("Pickerb568",0x0116b56c,0x106c,1,4,0,[255,0,0,0,1,0,0x0310106c])
        ]
  mapM_ (\(site,lr,offset,x,y,w,stack) -> do
    let c = named (site++"-rows-3-cursor-2")
    check (site++" caller registers and lifetime") (pickerReturnAddress c == lr && pickerReportOffset c == offset && pickerX c == x && pickerY c == y && pickerWidth c == w)
    check (site++" private stack arguments") (pickerStackArguments c (ReportAddress (0x03100000+fromIntegral offset)) == wordsLE stack)) callerFrames
  mapM_ (\site -> mapM_ (\mode -> do
    let c = named (site++"mode-"++show mode)
    check "skipped callers preserve report and clear only hook" (pickerReport c == B.replicate 68 0xa5 && map serviceAddress (pickerCalls c) == [ServiceAddress 0x43550])) ([2,3] :: [Int])) ["","fn-","Picker9304-","Picker9a14-","Pickera8b0-"]
  mapM_ (\mode -> do
    let calls = pickerCalls (named ("mode-"++show mode))
    check "saved selection uses slot two" (case calls of _:snapshot:_ -> serviceAddress snapshot == ServiceAddress 0x61ce1c && serviceInputs snapshot == [(Argument 0,0x30),(Argument 1,2)]; _ -> False)) ([7,9,13] :: [Int])
  mapM_ (\mode -> check "other modes use the current snapshot" (case pickerCalls (named ("mode-"++show mode)) of _:snapshot:_ -> serviceAddress snapshot == ServiceAddress 0x61cd14; _ -> False)) ([5,6,11,12,14,255,0xffffffff] :: [Int])
  let horizontalLeaves = [bytes | c <- pickerCalls (named "Pickerb52c-rows-3-cursor-2"), serviceAddress c == ServiceAddress 0x1112e78, MemoryArgument (Argument 0) 0 bytes <- serviceReads c]
  check "horizontal caller advances columns without advancing rows" (map (B.take 8 . B.drop 4) horizontalLeaves == map halfLE [[0,72,0xffec,0xffe8],[60,72,0xffec,0xffe8],[120,72,0xffec,0xffe8]])
  let styledLeaves = [bytes | c <- pickerCalls (named "Pickera1b8-rows-3-cursor-2"), serviceAddress c == ServiceAddress 0x1112e78, MemoryArgument (Argument 0) 0 bytes <- serviceReads c]
  check "assignment caller retains width and style" (all (\bytes -> B.take 2 (B.drop 8 bytes) == halfLE [100] && B.take 4 (B.drop 20 bytes) == wordsLE [3]) styledLeaves)
  putStrLn "value-picker generator contract tests passed"
  where
    names = map pickerName pickerCases
    named name = case filter ((==name) . pickerName) pickerCases of [c] -> c; _ -> error ("missing case "++name)
    check name ok = unless ok (fail name)
