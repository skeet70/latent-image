{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- Census of the A32 instruction forms in every manifest callback's structural
-- instruction set, with the shared decoder's status for each form.
module Main where

import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), object, (.:), (.=))
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Bits (shiftL, (.|.))
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import Replacement.ModalRuntime.InstructionForm (FormName (..), Occurrence (..), classify, formDefinition, statusName)
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype CallbackId = CallbackId GuestAddress deriving (Eq, Ord, Show)
data Callback = Callback CallbackId GuestAddress [GuestAddress]

instance FromJSON Callback where
  parseJSON = J.withObject "callback" $ \fields -> do
    entry <- address =<< fields .: "address"
    end <- address =<< fields .: "window_end_exclusive"
    branches <- fields .: "external_branch_candidates"
    tails <- mapM (J.withObject "external branch" (\branch -> address =<< branch .: "target")) branches
    pure (Callback (CallbackId entry) end tails)
    where
      address text = case text of
        '0' : 'x' : digits | length digits == 8, [(value, "")] <- readHex digits -> pure (GuestAddress value)
        _ -> fail "eight-digit guest hex address required"

newtype Manifest = Manifest [Callback]

instance FromJSON Manifest where
  parseJSON = J.withObject "manifest" $ \fields -> Manifest <$> fields .: "functions"

imageBase :: Word32
imageBase = 0xbf1a00

hex :: Word32 -> String
hex value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""

wordAt :: B.ByteString -> Word32 -> Word32
wordAt bytes address
  | address < imageBase || toInteger address + 4 > toInteger imageBase + toInteger (B.length bytes) = error "image read outside input"
  | otherwise = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index bytes (fromIntegral (address - imageBase) + index))) 0 [0 .. 3]

data Census = Census CallbackId Int [(Word32, Occurrence)]

inspect :: B.ByteString -> Callback -> Either String Census
inspect bytes (Callback ident@(CallbackId entry) end tails) = do
  inspection <- inspectFunction xm1TaskRecords (GuestAddress imageBase) bytes (FunctionWindow entry end) tails
  let code = [address | GuestAddress address <- inspectedCodeWords inspection]
  pure (Census ident (length (inspectedLiteralWords inspection)) [(address, classify address (wordAt bytes address)) | address <- code])

main :: IO ()
main = do
  arguments <- getArgs
  let required name = case dropWhile (/= name) arguments of _ : value : _ -> pure value; _ -> fail ("missing " ++ name)
  manifestPath <- required "--manifest"
  imagePath <- required "--id5"
  outputPath <- required "--out"
  exists <- doesPathExist outputPath
  when exists (fail "output already exists")
  manifestBytes <- B.readFile manifestPath
  unless (sha256Hex (sha256 manifestBytes) == "f27b35628371e150cdc0cfea84b11424b52c8022f7604f832a04bb890273cd67") (fail "manifest identity differs")
  Manifest callbacks <- either fail pure (J.eitherDecodeStrict manifestBytes)
  unless (length callbacks == 5795) (fail "manifest census differs")
  bytes <- B.readFile imagePath
  unless (B.length bytes == 5185120 && sha256Hex (sha256 bytes) == "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a") (fail "image5 identity differs")
  censuses <- either fail pure (mapM (inspect bytes) callbacks)
  let occurrences = [(ident, occurrence) | Census ident _ entries <- censuses, (_, occurrence) <- entries]
      forms = M.fromListWith merge
        [ (name, (status, 1 :: Int, if conditional then 1 else 0 :: Int, S.singleton ident))
        | (ident, Occurrence name status conditional) <- occurrences ]
      merge (status, a, b, c) (_, x, y, z) = (status, a + x, b + y, S.union c z)
      ranked = sortOn (\(FormName name, (_, count, _, _)) -> (negate count, name)) (M.toList forms)
      formSets = [S.fromList [name | (_, Occurrence name _ _) <- entries] | Census _ _ entries <- censuses]
      covered k = let chosen = S.fromList (map fst (take k ranked)) in length [() | set <- formSets, set `S.isSubsetOf` chosen]
      family (FormName name) = takeWhile (/= ' ') name
      families = M.fromListWith (+) [(family name, count) | (name, (_, count, _, _)) <- M.toList forms]
      byStatus = M.fromListWith (\(a, b, c) (x, y, z) -> (a + x, b + y, S.union c z))
        [(status, (1 :: Int, count, users)) | (_, (status, count, _, users)) <- M.toList forms]
      result = object
        [ "schema_version" .= (1 :: Int)
        , "firmware" .= ("Fujifilm X-M1 1.50" :: String)
        , "evidence_class" .= ("static structural census of StockInventory-inspected callback code words; no semantics, equivalence or execution" :: String)
        , "inputs" .= object ["manifest" .= manifestPath, "manifest_sha256" .= sha256Hex (sha256 manifestBytes), "image5" .= imagePath, "image5_sha256" .= sha256Hex (sha256 bytes)]
        , "form_definition" .= formDefinition
        , "decoder_status_definition" .= object
            [ "a32-operand-semantics" .= ("host/firmware/A32.hs decode returns an operand-level constructor" :: String)
            , "a32-destinations-only" .= ("A32.hs returns Compute: it knows only the written registers" :: String)
            , "stock-inventory-operand-step" .= ("A32.hs rejects (MSR) or returns Compute (SXTB/SXTH, BFI/BFC); A32.statusWrite/signExtension/bitFieldInsert decode the operands for StockInventory" :: String)
            , "rejected" .= ("A32.hs Rejected and not recovered by StockInventory" :: String) ]
        , "callbacks" .= length censuses
        , "instructions" .= length occurrences
        , "conditional_instructions" .= length [() | (_, Occurrence _ _ True) <- occurrences]
        , "literal_words" .= sum [literals | Census _ literals _ <- censuses]
        , "distinct_forms" .= M.size forms
        , "distinct_families" .= M.size families
        , "by_decoder_status" .= [object ["status" .= statusName status, "forms" .= f, "instructions" .= n, "callbacks_using" .= S.size u] | (status, (f, n, u)) <- M.toList byStatus]
        , "callbacks_fully_covered_by_top_forms" .= [object ["forms" .= k, "callbacks" .= covered k] | k <- S.toList (S.fromList ([k | k <- [10, 20, 30, 40, 50], k < M.size forms] ++ [M.size forms]))]
        , "forms" .= [ object ["form" .= name, "decoder_status" .= statusName status, "instructions" .= count, "conditional" .= conditional, "callbacks" .= S.size users]
            | (FormName name, (status, count, conditional, users)) <- ranked ]
        , "families" .= [object ["family" .= name, "instructions" .= count] | (name, count) <- sortOn (negate . snd) (M.toList families)]
        , "single_callback_forms" .= [ object ["form" .= name, "callback" .= hex callback]
            | (FormName name, (_, _, _, users)) <- ranked, S.size users == 1, CallbackId (GuestAddress callback) <- S.toList users ]
        , "limits" .= ([ "A form is a syntactic class. A translation rule must still fix predication, NZCV, wrapping, shifter carry-out, PC-relative values, access width/alignment and order, and call-clobber semantics."
                       , "Only StockInventory-reachable code words of each callback are counted, not helpers they call." ] :: [String])
        ]
  L.writeFile outputPath (J.encode result)
  putStrLn ("wrote " ++ outputPath ++ "; forms=" ++ show (M.size forms) ++ "; instructions=" ++ show (length occurrences))
