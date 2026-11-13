module Main (main) where

import BankSourceBoundaryCheck (StockEntry (..), checkStockSource)
import Control.Monad (unless)
import Data.Either (isLeft)
import RustSourceBoundaryCheck (regressions)

main :: IO ()
main = do
  regressions
  let entry = StockEntry 0x00710000
      cases =
        [ ("declared trampoline", checkStockSource [entry] "boundary.S" "RB_TRAMPOLINE entry, 0x00710000" == Right ())
        , ("retargeted trampoline", isLeft (checkStockSource [entry] "boundary.S" "RB_TRAMPOLINE entry, 0x00710004"))
        , ("split target", isLeft (checkStockSource [entry] "boundary.S" "movw r12, #0x0004\nmovt r12, #0x0071\nbx r12"))
        , ("literal target", isLeft (checkStockSource [entry] "boundary.S" "ldr r12, .Ltarget\nblx r12\n.Ltarget:\n.word 0x00710004"))
        , ("C function pointer", isLeft (checkStockSource [entry] "boundary.c" "void (*call)(void) = (void (*)(void))0x00710004u;"))
        , ("C macro function pointer", isLeft (checkStockSource [entry] "boundary.c" "#define STOCK 0x00710004u\nvoid (*call)(void) = (void (*)(void))STOCK;"))
        , ("decimal stock constant", isLeft (checkStockSource [entry] "boundary.c" "void (*call)(void) = (void (*)(void))7405572u;"))
        , ("parenthesized stock constant", isLeft (checkStockSource [entry] "boundary.c" "void (*call)(void) = (void (*)(void))(0x00710004u);"))
        , ("arithmetic retarget is not a declared constant", isLeft (checkStockSource [entry] "boundary.c" "void (*call)(void) = (void (*)(void))(0x00710000u + 4u);"))
        , ("macro arithmetic cannot hide a retarget", isLeft (checkStockSource [entry] "boundary.c" "#define STOCK (0x00710000u + 4u)\nvoid (*call)(void) = (void (*)(void))STOCK;"))
        , ("declared call with a scalar argument", checkStockSource [entry] "boundary.c" "((void (*)(unsigned))0x00710000u)(0u);" == Right ())
        , ("C scalar is not a call", checkStockSource [entry] "boundary.c" "unsigned value = 0x00710004u;" == Right ())
        ]
  mapM_ (\(label, passed) -> unless passed (fail label)) cases
