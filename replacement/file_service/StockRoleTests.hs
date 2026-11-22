module Main (main) where

import Control.Monad (unless)
import Replacement.FileService.Native (Extent (..))
import Replacement.FileService.StockRole (Publication (..), stockRolePublications)
import System.Exit (exitFailure)

row :: Word -> Publication
row address = Publication (fromIntegral address) 0 1

-- | Three retired extents: the open worker, post and route entries.
extents :: [Extent]
extents = [Extent 0x7115a4 0x7116e4, Extent 0x71fde4 0x71fe2c, Extent 0x721578 0x721744]

check :: String -> Bool -> IO Bool
check name passed = do
  unless passed (putStrLn ("FAIL " ++ name))
  pure passed

main :: IO ()
main = do
  results <-
    sequence
      [ check "rows in retired extents are dropped and the others kept in order" $
          stockRolePublications extents (map row [0x6ae1dc, 0x7115a4, 0x71fde4, 0x721578, 0x73ac74, 0x721740, 0xefca6c])
            == Right (map row [0x6ae1dc, 0x73ac74, 0xefca6c])
      , check "a row in the retired region outside every retired extent is refused" $
          isLeft (stockRolePublications extents (map row [0x71fde4, 0x71fe30]))
            && isLeft (stockRolePublications extents (map row [0x71fde4, 0x7116e4]))
      , check "a bank without retired-extent rows is refused" $
          isLeft (stockRolePublications extents (map row [0x6ae1dc, 0x73ac74]))
      , check "rows just outside the region are kept" $
          stockRolePublications extents (map row [0x7115a0, 0x712890, 0x715a54, 0x715f0c, 0x718a88, 0x71b9e8, 0x71bc28, 0x71fde4, 0x721834])
            == Right (map row [0x7115a0, 0x712890, 0x715a54, 0x715f0c, 0x718a88, 0x71b9e8, 0x71bc28, 0x721834])
      ]
  unless (and results) exitFailure
  putStrLn ("stock-role tests passed: " ++ show (length results))
  where
    isLeft = either (const True) (const False)
