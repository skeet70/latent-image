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
    , "2e48a0a1f3a2e8eeb3d31893a363afb0b9881e82e149a510f22b0d25d0c98c68"
    )
  , ( "saclac-image.symbols"
    , "156a45deee07715f2fef14f8bca050e56c4e994dabacbe838245e4ce22eee644"
    )
  , ( "saclac-image-wrong-driver-store.symbols"
    , "03b593adec8bd0cd53102700b040af2915894a7e023361773fd40f2e74f64782"
    )
  , ( "saclac-image-wrong-wake-publication.symbols"
    , "ba78c0673c08763624a54cd9aab611dafadefe5ccb395ad8ac4155bf09548948"
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
