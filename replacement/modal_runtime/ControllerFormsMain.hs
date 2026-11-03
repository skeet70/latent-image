{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- Census of the A32 instruction forms in every manifest callback's structural
-- instruction set, with the shared decoder's status for each form.
module Main where

import qualified A32 as A
import Control.Monad (unless, when)
import Data.Aeson (FromJSON (..), object, (.:), (.=))
import qualified Data.Aeson as J
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Bits (shiftL, shiftR, testBit, (.&.), (.|.))
import Data.List (sortOn)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex, showHex)
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype CallbackId = CallbackId GuestAddress deriving (Eq, Ord, Show)
-- Register numbers, immediates, offsets and condition codes are not part of a form.
newtype FormName = FormName String deriving (Eq, Ord, Show)
data DecoderStatus = Semantic | DestinationsOnly | StockInventoryOnly | RejectedForm deriving (Eq, Ord, Show)
data Callback = Callback CallbackId GuestAddress [GuestAddress]
data Occurrence = Occurrence FormName DecoderStatus Bool

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

field :: Int -> Int -> Word32 -> Word32
field high low word = (word `shiftR` low) .&. ((1 `shiftL` (high - low + 1)) - 1)

shiftName :: A.Shift -> String
shiftName shift = case shift of
  A.LSL -> "lsl"
  A.LSR -> "lsr"
  A.ASR -> "asr"
  A.ROR -> "ror"

operandShape :: A.Operand -> String
operandShape operand = case operand of
  A.Immediate _ -> "imm"
  A.ShiftedImmediate _ A.LSL 0 -> "reg"
  A.ShiftedImmediate _ A.ROR 0 -> "reg,rrx"
  A.ShiftedImmediate _ shift _ -> "reg," ++ shiftName shift ++ "#imm"
  A.ShiftedRegister _ shift _ -> "reg," ++ shiftName shift ++ " reg"

offsetShape :: A.Offset -> String
offsetShape offset = case offset of
  A.OffsetImmediate _ -> "imm"
  A.OffsetRegister _ _ A.LSL 0 -> "reg"
  A.OffsetRegister _ _ A.ROR 0 -> "reg,rrx"
  A.OffsetRegister _ _ shift _ -> "reg," ++ shiftName shift ++ "#imm"

indexingName :: A.Indexing -> String
indexingName indexing = case indexing of
  A.OffsetOnly -> "offset"
  A.PreIndexed -> "pre!"
  A.PostIndexed -> "post"

widthName :: A.Width -> Bool -> String
widthName width signed = case (width, signed) of
  (A.Word, _) -> ""
  (A.Dual, _) -> "d"
  (A.Half, False) -> "h"
  (A.Half, True) -> "sh"
  (A.Byte, False) -> "b"
  (A.Byte, True) -> "sb"

blockName :: A.BlockMode -> String
blockName mode = case mode of
  A.IncrementAfter -> "ia"
  A.IncrementBefore -> "ib"
  A.DecrementAfter -> "da"
  A.DecrementBefore -> "db"

-- Names for the encodings `A.decode` returns as Compute, from the same masks.
computeName :: Word32 -> String
computeName word
  | field 27 24 word == 0 && field 7 4 word == 9 = case (testBit word 23, field 22 21 word) of
      (True, accumulate) -> (if testBit word 22 then "s" else "u") ++ (if testBit accumulate 0 then "mlal" else "mull") ++ flags
      (False, 2) -> "umaal"
      (False, 3) -> "mls"
      _ -> "multiply-other"
  | word .&. 0x0fff0ff0 == 0x016f0f10 = "clz"
  | word .&. 0x0ff00090 == 0x01000080 = "smla<x><y>"
  | word .&. 0x0ff000b0 == 0x01200080 = "smlaw<y>"
  | word .&. 0x0ff000b0 == 0x012000a0 = "smulw<y>"
  | word .&. 0x0ff00090 == 0x01600080 = "smul<x><y>"
  | word .&. 0x0fff03f0 == 0x06af0070 = "sxtb"
  | word .&. 0x0fff03f0 == 0x06bf0070 = "sxth"
  | word .&. 0x0ff003f0 == 0x06e00070 = "uxtab"
  | word .&. 0x0ff003f0 == 0x06f00070 = "uxtah"
  | word .&. 0x0ff003f0 == 0x06a00070 = "sxtab"
  | word .&. 0x0ff003f0 == 0x06b00070 = "sxtah"
  | word .&. 0x0fff0ff0 == 0x06bf0f30 = "rev"
  | word .&. 0x0fff0ff0 == 0x06bf0fb0 = "rev16"
  | word .&. 0x0fff0ff0 == 0x06ff0f30 = "rbit"
  | word .&. 0x0fe00070 == 0x07a00050 = "sbfx"
  | word .&. 0x0fe00070 == 0x07e00050 = "ubfx"
  | word .&. 0x0fe0007f == 0x07c0001f = "bfc"
  | word .&. 0x0fe00070 == 0x07c00010 = "bfi"
  | otherwise = "compute-" ++ hex (word .&. 0x0ff000f0)
  where
    flags = if testBit word 20 then ".s" else ""

pcNote :: [(String, Bool)] -> String
pcNote notes = concat [" " ++ label | (label, present) <- notes, present]

classify :: Word32 -> Word32 -> Occurrence
classify address word = case A.decode address word of
  A.Rejected reason
    | Just (A.StatusWrite _ target fields _) <- A.statusWrite word ->
        Occurrence (FormName ("msr " ++ (if target == A.CurrentStatus then "cpsr" else "spsr") ++ "_" ++ concatMap fieldLetter fields)) StockInventoryOnly conditional
    | otherwise -> Occurrence (FormName ("rejected " ++ A.forbiddenReason reason)) RejectedForm conditional
  A.Compute _ _
    | Just _ <- A.signExtension word -> Occurrence (FormName (computeName word)) StockInventoryOnly conditional
    | otherwise -> Occurrence (FormName (computeName word)) DestinationsOnly conditional
  instruction -> Occurrence (FormName (semanticName instruction)) Semantic conditional
  where
    conditional = field 31 28 word /= 14
    fieldLetter statusField = case statusField of
      A.ControlBits -> "c"
      A.ExtensionBits -> "x"
      A.StatusBits -> "s"
      A.FlagBits -> "f"
    semanticName instruction = case instruction of
      A.DataProcessing _ operation setsFlags destination source operand ->
        let tests = operation `elem` [A.TST, A.TEQ, A.CMP, A.CMN]
            moves = operation `elem` [A.MOV, A.MVN]
         in map toLowerAscii (show operation) ++ (if setsFlags && not tests then ".s" else "") ++ " " ++ operandShape operand
              ++ pcNote [("rd=pc", not tests && destination == A.pc), ("rn=pc", not moves && source == A.pc), ("rn=sp", not moves && source == A.sp), ("rd=sp", not tests && destination == A.sp)]
      A.MoveWide {} -> "movw"
      A.MoveTop {} -> "movt"
      A.ReadStatus {} -> "mrs"
      A.WriteApplicationFlags {} -> "msr apsr_nzcvq"
      A.ZeroExtend _ _ _ rotation bits -> (if bits == 8 then "uxtb" else "uxth") ++ (if rotation /= 0 then " ror" else "")
      A.Multiply _ _ _ _ accumulator -> maybe "mul" (const "mla") accumulator
      A.Load _ width signed destination base offset indexing ->
        "ldr" ++ widthName width signed ++ " " ++ offsetShape offset ++ " " ++ indexingName indexing ++ pcNote [("base=pc", base == A.pc), ("base=sp", base == A.sp), ("rt=pc", destination == A.pc)]
      A.Store _ width source base offset indexing ->
        "str" ++ widthName width False ++ " " ++ offsetShape offset ++ " " ++ indexingName indexing ++ pcNote [("base=sp", base == A.sp), ("rt=pc", source == A.pc)]
      A.LoadMultiple _ base mode writeback registers ->
        "ldm" ++ blockName mode ++ (if writeback then "!" else "") ++ pcNote [("base=sp", base == A.sp), ("+pc", A.pc `elem` registers)]
      A.StoreMultiple _ base mode writeback registers ->
        "stm" ++ blockName mode ++ (if writeback then "!" else "") ++ pcNote [("base=sp", base == A.sp), ("+lr", A.lr `elem` registers)]
      A.Branch _ link _ -> if link then "bl" else "b"
      A.BranchExchange _ link register -> (if link then "blx" else "bx") ++ (if register == A.lr then " lr" else " reg")
      A.Hint _ -> "hint"
      A.Compute {} -> "compute"
      A.Rejected _ -> "rejected"

toLowerAscii :: Char -> Char
toLowerAscii c = if c >= 'A' && c <= 'Z' then toEnum (fromEnum c + 32) else c

statusName :: DecoderStatus -> String
statusName status = case status of
  Semantic -> "a32-operand-semantics"
  DestinationsOnly -> "a32-destinations-only"
  StockInventoryOnly -> "stock-inventory-operand-step"
  RejectedForm -> "rejected"

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
      ranked = sortOn (\(_, (_, count, _, _)) -> negate count) (M.toList forms)
      formSets = [S.fromList [name | (_, Occurrence name _ _) <- entries] | Census _ _ entries <- censuses]
      covered k = let chosen = S.fromList (map fst (take k ranked)) in length [() | set <- formSets, set `S.isSubsetOf` chosen]
      family (FormName name) = takeWhile (/= ' ') name
      families = M.fromListWith (+) [(family name, count) | (name, (_, count, _, _)) <- M.toList forms]
      byStatus = M.fromListWith (\(a, b, c) (x, y, z) -> (a + x, b + y, c + z))
        [(status, (1 :: Int, count, S.size users)) | (_, (status, count, _, users)) <- M.toList forms]
      result = object
        [ "schema_version" .= (1 :: Int)
        , "firmware" .= ("Fujifilm X-M1 1.50" :: String)
        , "evidence_class" .= ("static structural census of StockInventory-inspected callback code words; no semantics, equivalence or execution" :: String)
        , "inputs" .= object ["manifest" .= manifestPath, "manifest_sha256" .= sha256Hex (sha256 manifestBytes), "image5" .= imagePath, "image5_sha256" .= sha256Hex (sha256 bytes)]
        , "form_definition" .= ("operation, flag-setting, operand/offset shape (immediate, register, shift kind with immediate or register amount, RRX), width and signedness, indexing and writeback, block mode, and PC/SP/LR roles that change semantics; register numbers, immediates, offset signs and condition codes are parameters. Conditional occurrences are counted per form." :: String)
        , "decoder_status_definition" .= object
            [ "a32-operand-semantics" .= ("host/firmware/A32.hs decode returns an operand-level constructor" :: String)
            , "a32-destinations-only" .= ("A32.hs returns Compute: it knows only the written registers" :: String)
            , "stock-inventory-operand-step" .= ("A32.hs rejects (MSR) or returns Compute (SXTB/SXTH); StockInventory's statusWrite/signExtension decode the operands" :: String)
            , "rejected" .= ("A32.hs Rejected and not recovered by StockInventory" :: String) ]
        , "callbacks" .= length censuses
        , "instructions" .= length occurrences
        , "conditional_instructions" .= length [() | (_, Occurrence _ _ True) <- occurrences]
        , "literal_words" .= sum [literals | Census _ literals _ <- censuses]
        , "distinct_forms" .= M.size forms
        , "distinct_families" .= M.size families
        , "by_decoder_status" .= [object ["status" .= statusName status, "forms" .= f, "instructions" .= n, "callbacks_using" .= u] | (status, (f, n, u)) <- M.toList byStatus]
        , "callbacks_fully_covered_by_top_forms" .= [object ["forms" .= k, "callbacks" .= covered k] | k <- [10, 20, 30, 50, 75, 100, 150, M.size forms]]
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
