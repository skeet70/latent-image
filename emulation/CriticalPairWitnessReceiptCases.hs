{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Monad (foldM, forM, unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.Types as JT
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Bits (shiftR, xor, (.&.))
import Data.Word (Word64)
import qualified Data.Vector as V
import qualified Emulation.CriticalPairWitness as W
import qualified Emulation.InterfaceInitializer as I
import qualified FujiAF.Host as H
import FujiAF.JSON (decodeStrictIntegers)
import System.Environment (getArgs)


data Segment = Field K.Key | Element Int
newtype ReceiptPath = ReceiptPath [Segment]
data Change = FlipWord Integer | Remove | ReplaceValue Value

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
  , RefusalCase "entry-watch-classes" (watch "entry" [field "classes"]) Remove
  , RefusalCase "exit-watch-confinement" (watch "exit" [field "confinement"]) Remove
  , RefusalCase "entry-watch-tcm" (watch "entry" [field "tcm"]) Remove
  , RefusalCase "exit-watch-views" (watch "exit" [field "views"]) Remove
  , RefusalCase "entry-instruction-evidence" (ReceiptPath [field "pair",field "entry",field "steps"]) Remove
  , RefusalCase "exit-instruction-evidence" (ReceiptPath [field "pair",field "exit",field "steps"]) Remove
  , RefusalCase "caller-watch-classes" (ReceiptPath [field "pair",field "caller_watch",field "classes"]) Remove
  , RefusalCase "prelude-watch-confinement" (ReceiptPath [field "pair",field "prelude_watch",field "confinement"]) Remove
  , RefusalCase "fixed-decoder" (ReceiptPath [field "pair",field "decoder",field "choices",Element 0]) (FlipWord 1)
  , RefusalCase "alias-evidence" (ReceiptPath [field "pair",field "aliases"]) Remove
  ]
  where
    context boundary side suffix = ReceiptPath ([field "pair",field boundary,field side] ++ suffix)
    watch boundary suffix = ReceiptPath ([field "pair",field boundary,field "watch"] ++ suffix)

data SemanticCase = SemanticCase String [ReceiptPath] Integer W.RefusalCategory

semanticCases :: [SemanticCase]
semanticCases =
  [ SemanticCase "entry-counter-effect" (snapshot "entry" "after" 6 0) 1 W.InstructionEffectsMismatch
  , SemanticCase "exit-counter-effect" (snapshot "exit" "after" 6 0) 1 W.InstructionEffectsMismatch
  , SemanticCase "secondary-observation-publication" (initial 5 0x1624) 4 W.PublicationMismatch
  , SemanticCase "secondary-restoration-publication" (initial 5 0x168) 4 W.PublicationMismatch
  , SemanticCase "carrier-source-binding" (initial 0 0) 1 W.SourceBindingMismatch
  , SemanticCase "primary-services-source-binding" (initial 8 0) 1 W.SourceBindingMismatch
  , SemanticCase "primary-primitives-source-binding" (initial 9 0) 1 W.SourceBindingMismatch
  , SemanticCase "secondary-services-source-binding" (initial 10 0) 1 W.SourceBindingMismatch
  , SemanticCase "secondary-primitives-source-binding" (initial 11 0) 1 W.SourceBindingMismatch
  , SemanticCase "caller-saved-frame" [ReceiptPath [field "pair",field "caller_frame_after",field "views",field view,Element 32] | view <- ["virtual","physical"]] 1 W.CallerFrameMismatch
  ]
  where
    views prefix index offset = [ReceiptPath (prefix ++ [Element index,field "views",field view,Element offset]) | view <- ["virtual","physical"]]
    snapshot boundary side = views [field "pair",field boundary,field side]
    initial index offset = views [field "pair",field "resources"] index offset ++ snapshot "entry" "before" index offset

lookupAt :: ReceiptPath -> Value -> Either String Value
lookupAt (ReceiptPath path) = descend path
  where
    descend [] value = Right value
    descend (Field key:rest) (Object values) =
      maybe (Left "lookup field is absent") (descend rest) (KM.lookup key values)
    descend (Element index:rest) (Array values) =
      maybe (Left "lookup element is absent") (descend rest) (values V.!? index)
    descend _ _ = Left "lookup path has the wrong JSON type"

event :: Value -> Either String I.Event
event value = case J.fromJSON value of
  J.Error problem -> Left problem
  J.Success observed -> Right observed

epilogueAddress :: Value -> Either String (Maybe Word64)
epilogueAddress value = do
  observed <- event value
  pure $ case observed of
    I.Event I.CpuRead (Just 1) _ physical 4 _ (Just 0x726094) _ _ -> physical
    _ -> Nothing

splitEpilogue :: Value -> Either String [Value]
splitEpilogue value = do
  observed <- event value
  case observed of
    I.Event I.CpuRead (Just 1) address (Just physical) 8 (Just word) (Just 0x726094) _ _ -> do
      before <- lookupAt (ReceiptPath [field "before"]) value
      bytes <- case before of
        Array bytes | V.length bytes == 8 -> Right bytes
        _ -> Left "wide epilogue read lacks its eight-byte preimage"
      forM [0,4] $ \offset -> foldM (\current (key,changed) ->
          mutate (ReceiptPath [field key]) (ReplaceValue changed) current) value
        [ ("address",J.toJSON (address+offset)),("physical",J.toJSON (physical+offset)),("size",J.toJSON (4 :: Int))
        , ("value",J.toJSON ((word `shiftR` fromIntegral (offset*8)) .&. 0xffffffff))
        , ("before",Array (V.slice (fromIntegral offset) 4 bytes))
        ]
    _ -> pure [value]

withoutEpilogueReads :: Maybe Word64 -> Value -> Either String Value
withoutEpilogueReads selected value = do
  let path = ReceiptPath [field "pair",field "caller_watch",field "records"]
  records <- lookupAt path value
  rows <- case records of
    Array rows -> pure (V.toList rows)
    _ -> Left "caller records are not an array"
  normalized <- concat <$> mapM splitEpilogue rows
  tagged <- mapM (\row -> (row,) <$> epilogueAddress row) normalized
  let addresses = [address | (_,Just address) <- tagged]
      removed address = case address of
        Nothing -> False
        Just physical -> maybe True (== physical) selected
      kept = [row | (row,address) <- tagged,not (removed address)]
      expectedRemoved = maybe 6 (const 1) selected
  unless (addresses == [0xf33c8,0xf33cc..0xf33dc] && length tagged-length kept == expectedRemoved)
    (Left "genuine epilogue load evidence differs from the refusal case input")
  mutate path (ReplaceValue (J.toJSON kept)) value

mutate :: ReceiptPath -> Change -> Value -> Either String Value
mutate (ReceiptPath path) change = descend path
  where
    descend [] _ | ReplaceValue replacement <- change = Right replacement
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
  either (H.failH . show) pure (W.validateReceipt contract receipt)
  value <- strict receipt
  generic <- forM cases $ \(RefusalCase name path change) -> do
    changed <- either H.failH pure (mutate path change value)
    refuse name Nothing changed
  coherent <- forM semanticCases $ \(SemanticCase name paths bits category) -> do
    changed <- either H.failH pure (foldM (\current path -> mutate path (FlipWord bits) current) value paths)
    refuse name (Just category) changed
  epilogue <- forM (Nothing:map Just [0xf33c8,0xf33cc..0xf33dc]) $ \selected -> do
    changed <- either H.failH pure (withoutEpilogueReads selected value)
    refuse ("caller-epilogue-loads-" ++ maybe "all" show selected) (Just W.CallerEpilogueMismatch) changed
  pure (generic++coherent++epilogue)
  where
    refuse name expected changed = case W.validateReceipt contract (BL.toStrict (J.encode changed)) of
      Left (W.ReceiptFailure category _) -> do
        unless (maybe True (== category) expected)
          (H.failH (role ++ " receipt refused " ++ name ++ " at " ++ show category ++ ", expected " ++ show expected))
        pure (object ["role" .= role,"case" .= name,"result" .= ("refused" :: String),"category" .= show category])
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
      either (H.failH . show) pure (W.compareReceipts (originalContract,retained) (ownedContract,owned))
      let expectedCases = length cases+length semanticCases+7
      unless (length originalCases == expectedCases && length ownedCases == expectedCases) (H.failH "receipt case count differs")
      BL.writeFile outputPath (J.encode (object
        [ "schema" .= (1 :: Int),"positive_receipts" .= (2 :: Int),"comparison" .= ("matched" :: String)
        , "cases" .= (originalCases ++ ownedCases)
        ]))
    _ -> H.failH "usage: critical-pair-receipt-cases SOURCE_DAT SOURCE_IMAGE4 CANDIDATE_DAT CANDIDATE_IMAGE4 LINKED_BIN LINKED_SYMBOLS RETAINED_RECEIPT OWNED_RECEIPT OUT_JSON"
