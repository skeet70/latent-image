module Main (main) where

import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.ByteString.Lazy as BL
import qualified FujiAF.Host as H
import System.Environment (getArgs)

expected :: [(String, String)]
expected =
  [ ( "saclac-image.elf"
    , "cc89d725f9ca28cdb4cf6268b93a2042dc7000fc8a5368156a4d64b5a6da83f5"
    )
  , ( "saclac-image.symbols"
    , "e4b1302de99d2a7027204721d2564615d8d547182b0933078824e13d12a62831"
    )
  , ( "saclac-image-wrong-driver-store.symbols"
    , "8f05c7ba9ef8ed91491ae677d97bee821e8eef9714f9082ee830095ab93fb527"
    )
  ]

main :: IO ()
main = do
  paths <- getArgs
  H.ensure (length paths == length expected) "expected three artifact paths"
  mapM_ check (zip expected paths)
  where
    check ((label, want), path) = do
      got <- show . SHA256.hashlazy <$> BL.readFile path
      H.ensure (got == want) (label ++ " digest " ++ got ++ " /= " ++ want)
