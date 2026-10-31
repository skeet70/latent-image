{-# LANGUAGE OverloadedStrings #-}

module Main where

import ArmElf
  ( Elf (..)
  , Section (..)
  , Symbol (..)
  , SymbolKind (..)
  , SymbolSection (..)
  , parseElf
  , sectionAllocated
  , sectionExecutable
  )
import Control.Monad (forM, unless, when)
import Data.Aeson (Value (..))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.JSON (decodeStrictIntegers, encodeCanonical)
import Numeric (readHex)
import Replacement.ModalRuntime.Closure
import System.Directory (doesPathExist)
import System.Environment (getArgs)
import System.Exit (exitFailure, exitSuccess)
import System.IO (hPutStrLn, stderr)

data Pinned = Pinned
  { pinnedOption :: String
  , pinnedName :: String
  , pinnedBase :: Word32
  , pinnedSize :: Int
  , pinnedDigest :: String
  }

-- | The images resident when the RAM-push package runs: both loaders, the RC0
-- image 4 with its 0xc0-byte prefix, image 5 and the plain image.
pinnedImages :: [Pinned]
pinnedImages =
  [ Pinned "--loader-a" "loader-40000" 0x00040000 56032 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  , Pinned "--loader-b" "loader-d0000" 0x000d0000 55968 "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"
  , Pinned "--id4" "rc0-image4" 0x0061c340 2631872 "a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551"
  , Pinned "--id5" "image5" 0x00bf1a00 5185120 "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"
  , Pinned "--plain" "plain" 0x01100000 3538944 "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"
  ]

main :: IO ()
main = do
  arguments <- getArgs
  output <- required arguments "--out"
  exists <- doesPathExist output
  when exists (failWith "output path already exists")
  images <- forM pinnedImages $ \pinned -> do
    path <- required arguments (pinnedOption pinned)
    bytes <- B.readFile path
    unless (B.length bytes == pinnedSize pinned && sha256Hex (sha256 bytes) == pinnedDigest pinned) $
      failWith (pinnedName pinned ++ ": size or digest differs from the pinned image")
    pure (Region (pinnedName pinned) StockImage (pinnedBase pinned) bytes)
  kernel <- either failWith pure . parseElf =<< B.readFile =<< required arguments "--kernel-elf"
  manifest <- either failWith pure . decodeStrictIntegers =<< B.readFile =<< required arguments "--manifest"
  publications <- either failWith pure (parsePublications manifest)
  let proof = buildProof (Inputs (images ++ bankRegions kernel) publications (bankFunctions kernel))
  B.writeFile output (encodeCanonical (proofValue proof))
  if proofAdmitted proof then exitSuccess else exitFailure

bankRegions :: Elf -> [Region]
bankRegions kernel =
  [ Region
      ("bank" ++ sectionName section)
      (if sectionExecutable section then BankText else BankData)
      (sectionAddress section)
      (B.take (fromIntegral (sectionSize section)) (B.drop (fromIntegral (sectionOffset section)) (elfBytes kernel)))
  | section <- elfSections kernel
  , sectionAllocated section
  , sectionType section == 1
  , sectionSize section > 0
  ]

bankFunctions :: Elf -> [BankFunction]
bankFunctions kernel =
  [ BankFunction (symbolName symbol) (symbolValue symbol) (symbolSize symbol)
  | symbol <- elfSymbols kernel
  , symbolKind symbol == FunctionSymbol
  , symbolSize symbol > 0
  , symbolSection symbol /= Undefined
  ]

parsePublications :: Value -> Either String [Publication]
parsePublications document = case document of
  Object fields -> case KM.lookup "publications" fields of
    Just (Array items) -> mapM publication (V.toList items)
    _ -> Left "manifest: publications array required"
  _ -> Left "manifest: object required"
 where
  publication (Object fields) = Publication <$> hexField fields "address" <*> hexField fields "old" <*> hexField fields "new"
  publication _ = Left "manifest: publication object required"
  hexField fields key = case KM.lookup (K.fromString key) fields of
    Just (String text)
      | '0' : 'x' : digits <- T.unpack text
      , length digits == 8
      , [(value, "")] <- readHex digits ->
          Right value
    _ -> Left ("manifest: eight-digit hex " ++ key ++ " required")

required :: [String] -> String -> IO String
required arguments name = case dropWhile (/= name) arguments of
  (_ : value : _) -> pure value
  _ -> failWith ("missing " ++ name)

failWith :: String -> IO a
failWith message = hPutStrLn stderr message >> exitFailure
