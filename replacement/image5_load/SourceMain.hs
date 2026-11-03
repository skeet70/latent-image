{-# LANGUAGE OverloadedStrings #-}
-- Usage: image5-load-source IMAGE4_BIN OUTPUT_JSON
-- StockInventory derivation of the X-M1 1.50 image5 producer window and its
-- direct call sites and immediate-formed constants.
module Main (main) where

import Control.Monad (unless, when)
import Emulation.Sha256 (sha256, sha256Hex)
import Data.Aeson (object, (.=))
import qualified Data.Aeson as J
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word32)
import Numeric (showHex)
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

imageBase, producerStart, producerFence :: Word32
imageBase = 0x61c400
producerStart = 0x6a96c0
producerFence = 0x6a9844

canonicalImage4 :: String
canonicalImage4 = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

newtype Register = Register Word32 deriving (Eq, Show)
data Site = Site Word32 Word32
data Constant = Constant Word32 Register Word32

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

digest :: B.ByteString -> String
digest = sha256Hex . sha256

wordAt :: B.ByteString -> Word32 -> Either String Word32
wordAt image address = do
  let offset = fromIntegral (address - imageBase)
  when (address < imageBase || offset + 4 > B.length image) (Left ("word outside image4: " ++ hex address))
  pure (foldr (\i acc -> (acc `shiftL` 8) .|. fromIntegral (B.index image (offset + i))) 0 [0..3])

-- Unconditional BL is a site; conditional BL and BLX forms refuse.
call :: Word32 -> Word32 -> Either String (Maybe Site)
call address word
  | word .&. 0xff000000 == 0xeb000000 =
      let offset = word .&. 0x00ffffff
          signed = if offset .&. 0x00800000 /= 0 then offset .|. 0xff000000 else offset
      in Right (Just (Site address (address + 8 + (signed `shiftL` 2))))
  | word .&. 0x0f000000 == 0x0b000000 = Left ("conditional BL at " ++ hex address)
  | word .&. 0x0ffffff0 == 0x012fff30 = Left ("BLX register at " ++ hex address)
  | word `shiftR` 25 == 0x7d = Left ("BLX immediate at " ++ hex address)
  | otherwise = Right Nothing

-- Unconditional MOVW, rotation-0 MOV immediate and MOVT.
low, high :: Word32 -> Maybe (Register, Word32)
low word
  | word .&. 0xfff00000 == 0xe3000000 = Just (destination word, ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff))
  | word .&. 0xffff0f00 == 0xe3a00000 = Just (destination word, word .&. 0xff)
  | otherwise = Nothing
high word
  | word .&. 0xfff00000 == 0xe3400000 = Just (destination word, ((word `shiftR` 4) .&. 0xf000) .|. (word .&. 0xfff))
  | otherwise = Nothing

destination :: Word32 -> Register
destination word = Register ((word `shiftR` 12) .&. 0xf)

-- A MOVT pairs only with a low-half write to the same register in the code
-- word at the preceding address; conditional MOVT refuses.
constants :: [(Word32, Word32)] -> Either String [Constant]
constants code = sequence ([conditional address | (address, word) <- code, word .&. 0x0ff00000 == 0x03400000, word `shiftR` 28 /= 0xe]
  ++ [pair previous current | (previous, current@(_, word)) <- zip (Nothing : map Just code) code, Just _ <- [high word]])
  where
    conditional address = Left ("conditional MOVT at " ++ hex address)
    pair (Just (before, lowWord)) (address, word)
      | before + 4 == address, Just (register, lower) <- low lowWord, Just (register', upper) <- high word, register == register' =
          Right (Constant address register ((upper `shiftL` 16) .|. lower))
    pair _ (address, _) = Left ("MOVT without an adjacent low half at " ++ hex address)

main :: IO ()
main = do
  args <- getArgs
  (imagePath, outputPath) <- case args of
    [i, o] -> pure (i, o)
    _ -> fail "Usage: image5-load-source IMAGE4_BIN OUTPUT_JSON"
  exists <- doesPathExist outputPath
  when exists (fail ("refusing to overwrite " ++ outputPath))
  image <- B.readFile imagePath
  unless (digest image == canonicalImage4) (fail "image4 is not canonical X-M1 1.50 base 0x61c400")
  let window = FunctionWindow (GuestAddress producerStart) (GuestAddress producerFence)
  inspection <- either fail pure (inspectFunction xm1TaskRecords (GuestAddress imageBase) image window [])
  derivation <- either fail pure (deriveFunction xm1TaskRecords (GuestAddress imageBase) image window [])
  let FunctionDeclaration (GuestAddress start) (FunctionSize size) = derivedDeclaration derivation
      codeAddresses = [a | GuestAddress a <- inspectedCodeWords inspection]
      literalAddresses = [a | GuestAddress a <- inspectedLiteralWords inspection]
  unless (start == producerStart && start + fromIntegral size == producerFence) (fail "derived extent differs from the window")
  code <- either fail pure (traverse (\a -> (,) a <$> wordAt image a) codeAddresses)
  calls <- either fail pure (traverse (uncurry call) code)
  formed <- either fail pure (constants code)
  L.writeFile outputPath (J.encode (object
    [ "schema" .= (1 :: Int)
    , "tool" .= ("replacement/image5_load:image5-load-source" :: String)
    , "input" .= object ["path" .= imagePath, "bytes" .= B.length image, "sha256" .= digest image]
    , "window" .= [hex producerStart, hex producerFence]
    , "derived_size" .= size
    , "code_words" .= length codeAddresses
    , "literal_words" .= map hex literalAddresses
    , "return_premises" .= map returnPremiseLabel (derivationPremises derivation)
    , "direct_calls" .= [object ["pc" .= hex pc, "target" .= hex target] | Just (Site pc target) <- calls]
    , "formed_constants" .= [object ["pc" .= hex pc, "register" .= ("r" ++ show r), "value" .= hex value] | Constant pc (Register r) value <- formed]
    ]))
  putStrLn ("wrote " ++ outputPath ++ "; size=" ++ show size ++ "; calls=" ++ show (length [() | Just _ <- calls]))
