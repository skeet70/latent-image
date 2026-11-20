module Main (main) where

import qualified Crypto.Hash.SHA256 as SHA256
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified FujiAF.Host as H
import Numeric (showHex)
import System.Environment (getArgs)

expected :: [(String, String)]
expected =
  [ ( "saclac-image.elf"
    , "ee7e28d678a52f0bee46337c0491d866b0154945291da7f3c20fa49a0544e6d0"
    )
  , ( "saclac-image.symbols"
    , "70a29c7e903e28529426f62dc46d66713939664d111a1c8de31de46700ab7ddb"
    )
  , ( "saclac-image-wrong-driver-store.symbols"
    , "c92c87146dc2a84c8f59e3ea9a8d72583dea247ccd93d909eafec820dddf4ff1"
    )
  , ( "saclac-image-wrong-wake-publication.symbols"
    , "5fb944fcf77bacbfb2cc63646f7a32c25d1c6957d3fbe9f4ebf2a8374be8dbe2"
    )
  ]

main :: IO ()
main = do
  paths <- getArgs
  H.ensure (length paths == length expected) "expected four artifact paths"
  mapM_ check (zip expected paths)
  where
    check ((label, want), path) = do
      got <- hexDigest <$> BL.readFile path
      H.ensure (got == want) (label ++ " digest " ++ got ++ " /= " ++ want)

hexDigest :: BL.ByteString -> String
hexDigest = concatMap byte . B.unpack . SHA256.hashlazy
  where
    byte value =
      let digits = showHex value ""
      in if length digits == 1 then '0' : digits else digits
