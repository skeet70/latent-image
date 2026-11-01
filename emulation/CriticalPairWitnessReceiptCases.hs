{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (forM, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.Types as JT
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (xor)
import qualified Data.Vector as V
import qualified Emulation.CriticalPairWitness as W
import qualified Emulation.InterfaceInitializer as I
import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrictIntegers)
import System.Environment (getArgs)


data Segment = Field K.Key | Element Int
newtype ReceiptPath = ReceiptPath [Segment]
data Change = FlipWord Integer | Remove

data RefusalCase = RefusalCase String ReceiptPath Change

field :: K.Key -> Segment
field = Field

cases :: [RefusalCase]
cases =
  [ RefusalCase "outer-schema" (ReceiptPath [field "schema"]) (FlipWord 1)
  , RefusalCase "initializer-schema" (ReceiptPath [field "initializer",field "schema"]) (FlipWord 1)
  , RefusalCase "entry-stack" (context "entry" "incoming" [field "sp"]) (FlipWord 8)
  , RefusalCase "entry-prior-irq" (context "entry" "returned" [field "registers",Element 0]) (FlipWord 0x80)
  , RefusalCase "entry-masked-irq" (context "entry" "returned" [field "cpsr"]) (FlipWord 0x80)
  , RefusalCase "exit-saved-mask" (context "exit" "incoming" [field "registers",Element 0]) (FlipWord 0x80)
  , RefusalCase "exit-restored-irq" (context "exit" "returned" [field "cpsr"]) (FlipWord 0x80)
  , RefusalCase "entry-counter-effect" (resource "entry" "after" 6 [field "views",field "physical",Element 0]) (FlipWord 1)
  , RefusalCase "exit-counter-effect" (resource "exit" "after" 6 [field "views",field "physical",Element 0]) (FlipWord 1)
  , RefusalCase "secondary-observation-publication" (resource "entry" "before" 5 [field "views",field "physical",Element 0x1624]) (FlipWord 4)
  , RefusalCase "secondary-restoration-publication" (resource "exit" "before" 5 [field "views",field "physical",Element 0x168]) (FlipWord 4)
  , RefusalCase "carrier-readback" (resource "entry" "before" 0 [field "views",field "physical",Element 0]) (FlipWord 1)
  , RefusalCase "entry-watch-classes" (watch "entry" [field "classes"]) Remove
  , RefusalCase "exit-watch-confinement" (watch "exit" [field "confinement"]) Remove
  , RefusalCase "entry-watch-tcm" (watch "entry" [field "tcm"]) Remove
  , RefusalCase "exit-watch-views" (watch "exit" [field "views"]) Remove
  , RefusalCase "entry-instruction-evidence" (ReceiptPath [field "pair",field "entry",field "steps"]) Remove
  , RefusalCase "exit-instruction-evidence" (ReceiptPath [field "pair",field "exit",field "steps"]) Remove
  , RefusalCase "caller-saved-frame" (ReceiptPath [field "pair",field "caller_frame_after",field "views",field "physical",Element 32]) (FlipWord 1)
  , RefusalCase "caller-watch-classes" (ReceiptPath [field "pair",field "caller_watch",field "classes"]) Remove
  , RefusalCase "prelude-watch-confinement" (ReceiptPath [field "pair",field "prelude_watch",field "confinement"]) Remove
  , RefusalCase "fixed-decoder" (ReceiptPath [field "pair",field "decoder",field "choices",Element 0]) (FlipWord 1)
  , RefusalCase "alias-evidence" (ReceiptPath [field "pair",field "aliases"]) Remove
  ]
  where
    context boundary side suffix = ReceiptPath ([field "pair",field boundary,field side] ++ suffix)
    resource boundary side index suffix = ReceiptPath ([field "pair",field boundary,field side,Element index] ++ suffix)
    watch boundary suffix = ReceiptPath ([field "pair",field boundary,field "watch"] ++ suffix)

mutate :: ReceiptPath -> Change -> Value -> Either String Value
mutate (ReceiptPath path) change = descend path
  where
    descend [] (Number number) | FlipWord bits <- change = do
      integer <- JT.parseEither J.parseJSON (Number number)
      pure (J.toJSON ((integer :: Integer) `xor` bits))
    descend [Field key] (Object values) | Remove <- change =
      if KM.member key values then Right (Object (KM.delete key values)) else Left "mutation field is absent"
    descend (Field key:rest) (Object values) = case KM.lookup key values of
      Nothing -> Left "mutation field is absent"
      Just value -> do
        changed <- descend rest value
        pure (Object (KM.insert key changed values))
    descend (Element index:rest) (Array values) = case values V.!? index of
      Nothing -> Left "mutation element is absent"
      Just value -> do
        changed <- descend rest value
        pure (Array (values V.// [(index,changed)]))
    descend _ _ = Left "mutation path or value has the wrong JSON type"

strict :: B.ByteString -> IO Value
strict bytes = either H.failH pure (decodeStrictIntegers bytes)

runCases :: String -> W.Contract -> B.ByteString -> IO [Value]
runCases role contract receipt = do
  either H.failH pure (W.validateReceipt contract receipt)
  value <- strict receipt
  forM cases $ \(RefusalCase name path change) -> do
    changed <- either H.failH pure (mutate path change value)
    case W.validateReceipt contract (BL.toStrict (J.encode changed)) of
      Left _ -> pure (object ["role" .= role,"case" .= name,"result" .= ("refused" :: String)])
      Right () -> H.failH (role ++ " receipt admitted " ++ name)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [sourcePath,sourceImage4Path,candidatePath,candidateImage4Path,imagePath,symbolsPath,retainedPath,ownedPath,outputPath] -> do
      source <- H.readRegular (64*1024*1024) sourcePath
      sourceImage4 <- H.readRegular (16*1024*1024) sourceImage4Path
      candidate <- H.readRegular (64*1024*1024) candidatePath
      candidateImage4 <- H.readRegular (16*1024*1024) candidateImage4Path
      image <- H.readRegular 0x1400 imagePath
      symbols <- H.readRegular 0x10000 symbolsPath
      retained <- H.readRegular (64*1024*1024) retainedPath
      owned <- H.readRegular (64*1024*1024) ownedPath
      originalContract <- either H.failH pure (W.deriveContract I.Retained source sourceImage4 source sourceImage4 B.empty B.empty)
      ownedContract <- either H.failH pure (W.deriveContract I.Owned source sourceImage4 candidate candidateImage4 image symbols)
      originalCases <- runCases "retained" originalContract retained
      ownedCases <- runCases "owned" ownedContract owned
      either H.failH pure (W.compareReceipts (originalContract,retained) (ownedContract,owned))
      unless (length originalCases == length cases && length ownedCases == length cases) (H.failH "receipt case count differs")
      BL.writeFile outputPath (J.encode (object
        [ "schema" .= (1 :: Int),"positive_receipts" .= (2 :: Int),"comparison" .= ("matched" :: String)
        , "cases" .= (originalCases ++ ownedCases)
        ]))
    _ -> H.failH "usage: critical-pair-receipt-cases SOURCE_DAT SOURCE_IMAGE4 CANDIDATE_DAT CANDIDATE_IMAGE4 LINKED_BIN LINKED_SYMBOLS RETAINED_RECEIPT OWNED_RECEIPT OUT_JSON"
