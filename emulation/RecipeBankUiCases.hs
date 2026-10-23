{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- Recipe-bank UI cases use a modeled modal/navigation/drawer boundary and the
-- real stock property getter, setter, commit dispatcher and consumer cells.
module Main where

import Control.Monad
import Data.Aeson hiding (encodeFile)
import Data.Aeson.Types (Parser, parseEither, parseMaybe)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified FujiAF.RecipeBankFields as F
import Data.Bits
import Data.Foldable (toList)
import Data.List
import Data.Maybe
import Data.String (fromString)
import Data.Word
import Emulation.NativeCase (runCase)
import Emulation.ParallelCases (mapBounded, readJobs)
import Emulation.Sha256 (sha256, sha256Hex)
import FujiAF.Host (str)
import Numeric
import System.Directory
import System.Environment
import System.Exit (exitFailure)
import System.FilePath

hex :: Integer -> String
hex x = "0x" ++ replicate (8 - length s) '0' ++ s where s = showHex x ""

hexB :: B.ByteString -> String
hexB = concatMap (\w -> let h = showHex w "" in replicate (2 - length h) '0' ++ h) . B.unpack

unhex :: String -> B.ByteString
unhex s = B.pack (go s)
  where
    go (a : b : r) = fromIntegral (d a * 16 + d b) : go r
    go _ = []
    d c = fromEnum c - (if c >= 'a' then fromEnum 'a' - 10 else fromEnum '0')

hexI :: String -> Integer
hexI s = case readHex (if take 2 s == "0x" then drop 2 s else s) of [(v, "")] -> v; _ -> 0

u32le :: Word32 -> B.ByteString
u32le v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8), fromIntegral (v `shiftR` 16), fromIntegral (v `shiftR` 24)]

u16le :: Word16 -> B.ByteString
u16le v = B.pack [fromIntegral v, fromIntegral (v `shiftR` 8)]

readU32 :: B.ByteString -> Int -> Word32
readU32 b o = fromIntegral (B.index b o) + fromIntegral (B.index b (o + 1)) `shiftL` 8
  + fromIntegral (B.index b (o + 2)) `shiftL` 16 + fromIntegral (B.index b (o + 3)) `shiftL` 24

-- bank file codecs, built independently of menu/recipe_bank_storage.c
checksum :: B.ByteString -> Word32
checksum f = complement (sum [fromIntegral (complement (if i >= 0x18 && i < 0x1c then 0 else B.index f i) :: Word8) | i <- [0 .. B.length f - 1]])
  where complement w = maxBound - w

putU32 :: Int -> Word32 -> B.ByteString -> B.ByteString
putU32 o v b = B.take o b <> u32le v <> B.drop (o + 4) b

bankMagic :: B.ByteString
bankMagic = B.pack (map (fromIntegral . fromEnum) "FJRB")

encodeFileV1 :: Word32 -> Word16 -> [(B.ByteString, B.ByteString)] -> B.ByteString
encodeFileV1 gen mask slots = putU32 0x18 (checksum file0) file0
  where
    hdr = B.concat [bankMagic, u16le 1, u16le 0x20, u16le 150, u16le 7, u16le 12, u16le 0x28, u32le gen, u32le 462, u32le 0, u16le mask, u16le 0]
    payloads = B.concat [maybe (B.replicate 0x28 0) snd (at i) | i <- [0 .. 6]]
    names = B.concat [maybe (B.replicate 26 0) fst (at i) | i <- [0 .. 6]]
    at i = if i < length slots then Just (slots !! i) else Nothing
    body = payloads <> names <> B.replicate 18 0
    file0 = hdr <> body

newtype Generation = Generation Word32 deriving (Num)

data Selection = NothingSelected | Custom BankSlot

data AutoUpdate = AutoDisable | AutoEnable

data CaptureMode = CaptureP | CaptureS | CaptureA | CaptureM

data Base = BaseInvalid | BaseValid CaptureMode B.ByteString

-- A populated record holds its 26-byte name, saved payload and working payload.
data Record = Unpopulated | Populated B.ByteString B.ByteString B.ByteString

encodeFileV2 :: Generation -> Selection -> AutoUpdate -> Base -> [Record] -> B.ByteString
encodeFileV2 (Generation gen) selection auto base records = putU32 0x18 (checksum file0) file0
  where
    slots = take 7 (records ++ repeat Unpopulated)
    mask = foldl' (.|.) 0 [bit k | (k, Populated {}) <- zip [0 ..] slots] :: Word16
    selectionByte = case selection of
      NothingSelected -> 0
      Custom (BankSlot k) -> fromIntegral k + 1
    autoByte = case auto of AutoDisable -> 0; AutoEnable -> 1
    (baseValid, baseMode, basePayload) = case base of
      BaseInvalid -> (0, 0, B.replicate 0x28 0)
      BaseValid mode payload -> (1, modeByte mode, payload)
    modeByte CaptureP = 1
    modeByte CaptureS = 2
    modeByte CaptureA = 3
    modeByte CaptureM = 4
    record Unpopulated = B.replicate 106 0
    record (Populated name saved working) = name <> saved <> working
    hdr = B.concat
      [ bankMagic, u16le 2, u16le 64, u16le 150, u16le 7, u16le 12, u16le 0x28, u32le gen, u32le 782, u32le 0, u16le mask, u16le 0
      , B.pack [selectionByte, autoByte, baseValid, baseMode], B.replicate 0x1c 0 ]
    file0 = hdr <> basePayload <> B.concat (map record slots) <> B.replicate (0x400 - 0x34e) 0

-- The generation a write over a migrated v1 bank produces: NOTHING SELECTED,
-- auto-update DISABLE, base invalid.
migratedV2 :: Generation -> [Record] -> B.ByteString
migratedV2 gen = encodeFileV2 gen NothingSelected AutoDisable BaseInvalid

migratedRecord :: (B.ByteString, B.ByteString) -> Record
migratedRecord (name, payload) = Populated name payload payload

name26 :: String -> B.ByteString
name26 s = B.pack (take 26 (concatMap (\c -> [fromIntegral (fromEnum c), 0]) (take 12 s) ++ repeat 0))

fileStride :: Int
fileStride = 0x420

fileSlot :: B.ByteString -> B.ByteString
fileSlot f = B.take fileStride (f <> B.replicate fileStride 0)

emptySlots :: Int -> B.ByteString
emptySlots n = B.replicate (n * fileStride) 0

cardWith3 :: B.ByteString -> B.ByteString -> B.ByteString
cardWith3 a c = fileSlot a <> emptySlots 1 <> fileSlot c

newtype Syms = Syms (M.Map String (Integer, Integer))

addr, sizeOf :: Syms -> String -> Integer
addr (Syms m) n = maybe (error ("symbol missing: " ++ n)) fst (M.lookup n m)
sizeOf (Syms m) n = maybe (error ("symbol missing: " ++ n)) snd (M.lookup n m)

hasSym :: Syms -> String -> Bool
hasSym (Syms m) n = M.member n m

parseSyms :: FilePath -> IO Syms
parseSyms p = Syms . M.fromList . catMaybes . map row . lines <$> readFile p
  where
    row l = case words l of
      [a, z, _, n] -> Just (n, (hexI a, hexI z))
      [a, _, n] -> Just (n, (hexI a, 0))
      _ -> Nothing

data FunctionInventory = KernelUiInventory | ForeignInventory deriving (Eq)

parseFunctions :: FunctionInventory -> FilePath -> IO [(Integer, Integer)]
parseFunctions inventory path = do
  rows <- lines <$> readFile path
  catMaybes <$> mapM one rows
  where
    one line = case words line of
      [_, address, bytes, "FUNC", _, _, section, name]
        | section /= "UND" -> case reads bytes of
            [(size, "")] | size > 0 ->
              pure $
                if inventory == KernelUiInventory && name `elem`
                  [ "rb_bank_draw_selected", "rb_bank_emit_failure_from_stock"
                  , "rb_bank_redraw_dispatch", "rb_call_stock_redraw_dispatch"
                  , "rb_call_draw_action_list"
                  , "rb_bank_current_value_label"
                  , "rb_bank_draw_name", "rb_call_name_style"
                  , "rb_call_name_background", "rb_call_name_title"
                  , "rb_call_name_shell", "rb_call_name_label", "rb_call_name_material"
                  , "rb_call_name_utf16", "rb_call_name_stretch", "rb_call_name_ascii"
                  , "rb_call_name_footer", "rb_call_name_label_position"
                  , "rb_media_epoch_entry_a"
                  , "rb_media_epoch_entry_b", "rb_media_open_guard_entry", "rb_media_write_guard_entry"
                  ]
                then Nothing
                else Just (hexI address, size)
            _ -> fail ("invalid function size in " ++ path)
      fields | "FUNC" `elem` fields -> fail ("invalid function metadata in " ++ path)
      _ -> pure Nothing

overlay :: B.ByteString -> [(Int, B.ByteString)] -> B.ByteString
overlay = foldl (\b (o, v) -> B.take o b <> v <> B.drop (o + B.length v) b)

-- the builder's plan -----------------------------------------------------------------

data PlanWrite = PlanWrite
  { pwRegion :: String
  , pwAddress :: Word32
  , pwNew :: B.ByteString
  , pwRole :: String
  }

parsePlan :: FilePath -> IO [PlanWrite]
parsePlan p = do
  raw <- B.readFile p
  either (error . ("invalid plan: " ++)) pure (eitherDecodeStrict' raw >>= parseEither parse)
  where
    parse = withObject "plan" $ \o -> do
      ws <- o .: "writes" :: Parser [Value]
      mapM one ws
    one = withObject "write" $ \w ->
      PlanWrite
        <$> w .: "region"
        <*> (fromIntegral <$> (w .: "address" >>= asInteger))
        <*> (unhex . T.unpack <$> w .: "new")
        <*> w .: "role"
    asInteger :: Value -> Parser Integer
    asInteger v = case v of
      String t -> case reads (T.unpack t) of [(n, "")] -> pure n; _ -> fail ("bad plan address " ++ T.unpack t)
      Number n -> pure (truncate n)
      _ -> fail "bad plan address"

menuRegions :: [String]
menuRegions = ["menu_values", "menu_labels", "menu_parent", "name_records", "edit_half"]

menuWrites :: [PlanWrite] -> [PlanWrite]
menuWrites = filter (\w -> pwRegion w `elem` menuRegions)

composeMenu :: [PlanWrite] -> (Word32, B.ByteString)
composeMenu ws = (base, overlay (B.replicate size 0) [(fromIntegral (pwAddress w - base), pwNew w) | w <- ws])
  where
    base = minimum [pwAddress w | w <- ws]
    end = maximum [pwAddress w + fromIntegral (B.length (pwNew w)) | w <- ws]
    size = fromIntegral (((end - base) + 0xff) .&. complement 0xff)

nodesByRole :: [PlanWrite] -> String -> [Word32]
nodesByRole ws role = sort [pwAddress w | w <- ws, role `isInfixOf` pwRole w]

nodeByRole :: [PlanWrite] -> String -> Word32
nodeByRole ws role = case nodesByRole ws role of
  [a] -> a
  as -> error ("expected one write for role " ++ show role ++ ", got " ++ show (length as))

-- the field map ----------------------------------------------------------------------

data Field = Field { fProp :: !Word32, fOff :: !Int, fShift :: !Int, fBytes :: !Int, fMask :: !Word32 }

parseFields :: B.ByteString -> [Field]
parseFields raw = either (error . ("invalid field map: " ++)) id (eitherDecodeStrict' raw >>= parseEither p)
  where
    p = withObject "f" $ \o -> do
      pfm <- o .: "payload_field_map"
      fl <- pfm .: "fields" :: Parser [Value]
      mapM one fl
    one v = withObject "field" inner v
    inner f = do
      prop <- f .: "property"
      off <- f .: "offset"
      bits <- f .: "bits"
      case bitsOf (str' bits) of
        Just (s, m, bytes) ->
          pure (Field (fromIntegral (hexI (str' prop))) (fromIntegral (hexI (str' off))) s bytes m)
        Nothing -> fail ("bits range not mapped: " ++ str' bits)
    bitsOf s = case span (/= '-') s of
      (lo, '-' : hi) ->
        let l = read lo :: Int
            h = read hi :: Int
            w = h - l + 1
         in if w >= 1 && w <= 16 then Just (l, ((1 `shiftL` w) - 1) `shiftL` l, if h > 7 then 2 else 1) else Nothing
      (b, "") ->
        let l = read b :: Int
         in Just (l, 1 `shiftL` l, 1)
      _ -> Nothing
    str' :: Value -> String
    str' (String t) = T.unpack t
    str' _ = ""


-- steps ------------------------------------------------------------------------------

newtype FieldHost = FieldHost Word32
newtype FieldCode = FieldCode Word32
newtype BankSlot = BankSlot Word32

data Step
  = Dispatch Word32 Word32 Word32 Int
  | Draw Word32 Int Int Int
  | WriteWord Word32 Word32
  | WriteHalf Word32 Word32
  | Copy Word32 Word32 Word32
  | Call Word32 Int
  | InputName String
  | ReadField Word32 Word32 Int
  | NativeWrite GuestAddress FieldHost FieldCode Int
  | Navigate Word32 Word32 Int
  | Refresh
  | Halt

stepWords :: Step -> [Word32]
stepWords (Dispatch frame cursor event out) = [0, frame, cursor, event, fromIntegral out]
stepWords (Draw page row field out) = [1, page, fromIntegral row, fromIntegral field, fromIntegral out]
stepWords (WriteWord a v) = [2, a, v, 0, 0]
stepWords (WriteHalf a v) = [3, a, v, 0, 0]
stepWords (Copy dst src n) = [4, dst, src, n, 0]
stepWords (Call a out) = [5, a, 0, 0, fromIntegral out]
stepWords (InputName name)
  | length name > 12 || any (\c -> c < ' ' || c > '~') name = error "invalid modeled editor input"
  | otherwise = 7 : [readU32 bytes o | o <- [0, 4, 8]] ++ [0]
  where bytes = B.pack (map (fromIntegral . fromEnum) name) <> B.replicate (12 - length name) 0
stepWords (ReadField getter host out) = [8, getter, host, 0, fromIntegral out]
stepWords (NativeWrite setter (FieldHost host) (FieldCode code) out) =
  [11, guestWord setter, host, code, fromIntegral out]
stepWords (Navigate cursor event out) = [9, 0, cursor, event, fromIntegral out]
stepWords Refresh = [10, 0, 0, 0, 0]
stepWords Halt = [6, 0, 0, 0, 0]

stepBytes :: [Step] -> B.ByteString
stepBytes steps
  | length steps + 1 > 63 = error "UI fixture exceeds the modeled step ABI"
  | otherwise = B.concat (map (B.concat . map u32le . stepWords) (steps ++ [Halt]))

-- the case environment ---------------------------------------------------------------

data Env = Env
  { eMenuBase :: Word32
  , eMenu :: B.ByteString
  , eSlot :: Word32
  , eSelect :: Word32
  , eSubs :: [Word32]
  , eEditScreen :: Word32
  , eDialog :: Word32
  , eFields :: [Word32]
  , eEditor :: Word32
  , eNameBase :: Word32
  , eCharsetCopy :: Word32
  , eRootEditRow :: Int
  , eRootSelectRow :: Int
  , eRegions :: [(String, Word32, Int)]
  , eOut :: Word32
  , eUiLog :: Word32
  , eOut26 :: Word32
  , eFileB :: Word32
  , eSteps :: Word32
  , eFrame :: Word32
  , eCursor :: Word32
  , eCurrentRow :: Word32
  , eRootNode :: Word32
  , eStockNode :: Word32
  , eStockValues :: Word32
  , eStockChild1 :: Word32
  , eStockChild2 :: Word32
  , eStockParent :: Word32
  , eHandler :: Word32
  , eEpochBump :: Word32
  , eRefuse :: Word32
  , eFieldGetter :: Word32
  , eStatusGetter :: Word32
  -- The bank's FUN_0115815c interposition; the 0xf1 callback FUN_01123f2c
  -- calls it with root 1.
  , eDrawRoot :: GuestAddress
  -- The media model's write-completion cell rbm_write_comp; 0 completes writes.
  , eWriteComp :: Word32
  , eKernelBase :: Word32
  , eKernel :: B.ByteString
  }

regionOf :: Env -> Word32 -> (String, Int)
regionOf e a = case [(n, fromIntegral (a - b)) | (n, b, s) <- eRegions e, a >= b, fromIntegral (a - b) < s] of
  [(n, o)] -> (n, o)
  [] -> error ("address outside the case regions: " ++ hex (fromIntegral a))
  rs -> error ("address in several regions: " ++ hex (fromIntegral a) ++ " " ++ show (map fst rs))

claimAt :: Env -> Word32 -> B.ByteString -> (String, Int, B.ByteString)
claimAt e a b =
  let (n, o) = regionOf e a
      capacity = fromMaybe (error "claim region missing") (lookup n [(r, s) | (r, _, s) <- eRegions e])
   in if B.null b || o + B.length b > capacity
        then error ("claim outside region: " ++ n ++ " at " ++ show o)
        else (n, o, b)

claimU32 :: Env -> Word32 -> Word32 -> (String, Int, B.ByteString)
claimU32 e a v = claimAt e a (u32le v)

-- A word of the composed menu image or of the kernel image the case maps.
menuWord :: Env -> Word32 -> Word32
menuWord e a
  | a >= eMenuBase e && fromIntegral (a - eMenuBase e) + 4 <= B.length (eMenu e) = readU32 (eMenu e) (fromIntegral (a - eMenuBase e))
  | a >= eKernelBase e && fromIntegral (a - eKernelBase e) + 4 <= B.length (eKernel e) = readU32 (eKernel e) (fromIntegral (a - eKernelBase e))
  | otherwise = error ("menu word outside the composed menu and the kernel image: " ++ hex (fromIntegral a))

-- fixtures ---------------------------------------------------------------------------

slotNames :: [String]
slotNames = ["ALPHA", "BRAVO", "CHARLIE", "DELTA", "ECHO", "FOXTROT", "GOLF"]

swapNames :: [String]
swapNames = ["SWAP-A", "SWAP-B", "SWAP-C", "SWAP-D", "SWAP-E", "SWAP-F", "SWAP-G"]

newCardNames :: [String]
newCardNames = ["NEW-A", "NEW-B", "NEW-C", "NEW-D", "NEW-E", "NEW-F", "NEW-G"]


payloadFor :: Word32 -> B.ByteString
payloadFor k = putHalf 0x00 (0x1234 + k) (putHalf 0x02 (0x5678 + k) validPayload)
  where
    putHalf o v b = B.take o b <> u16le (fromIntegral v) <> B.drop (o + 2) b

-- Every code is one that a selection admits in P, S and M (the stock row check's
-- availability columns and menu lists): film 3 (+0x24), 0xf9 = 1 (+0x1e bits
-- 5-6, IMAGE SIZE row 1 with 0xf6 = 0), 0x217 = 5 (+0x16 bits 5-7), 0xf3 = 1
-- (+0x1c bit 7), 0x1c5 = 6 (+0x22 bits 4-7), 0x214 = 6 (+0x25 bits 0-3) and
-- 0x1c6 = 0x7f (+0x04, key 0, the one key its matrix 0x0079b474 admits in S
-- and M).
validPayload :: B.ByteString
validPayload = B.take 0x13 raw <> B.singleton 0x20 <> B.drop 0x14 raw
  where
    raw = B.pack [0x34, 0x12, 0x78, 0x56, 0x7f, 0x40, 0x00, 0x80, 0x60, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa0, 0x60, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x20, 0x00, 0x0b, 0x03, 0x60, 0x00, 0x03, 0x06, 0x00, 0x00]

slotsA :: [(B.ByteString, B.ByteString)]
slotsA = [(name26 n, payloadFor k) | (k, n) <- zip [0 ..] slotNames]

cardA :: B.ByteString
cardA = encodeFileV1 7 0x7f slotsA

cardB :: B.ByteString
cardB = encodeFileV1 9 0x7f [(name26 n, payloadFor k) | (k, n) <- zip [0 ..] swapNames]

cardC :: B.ByteString
cardC = encodeFileV1 9 0x7f [(name26 n, payloadFor k) | (k, n) <- zip [0 ..] newCardNames]

recordsA :: [Record]
recordsA = map migratedRecord slotsA

extractLive :: B.ByteString
extractLive = ordinaryLive (B.replicate 4 0)

-- Nonzero ordinary fields in the literal preset, from the stock getter map.
ordinaryLive :: B.ByteString -> B.ByteString
ordinaryLive halfwords =
  overlay (B.replicate 0x1c2 0)
    [ (0x10, halfwords)
    , (0x5a, B.singleton 0x20)
    , (0x26, B.singleton 0x60)
    , (0x34, B.singleton 0x80)
    , (0x2b, B.singleton 0x80)
    , (0x33, B.singleton 0x60)
    , (0x2d, B.singleton 0x40)
    , (0x52, B.singleton 0x0b)
    , (0x85, B.singleton 0x30)
    , (0x8a, B.singleton 0x30)
    , (0x36, B.singleton 0x20)
    , (0x3a, B.singleton 0xa0)
    , (0x97, B.singleton 0x04)
    , (0x86, B.singleton 0x60)
    , (0x90, B.singleton 0x60)
    , (0x7d, B.singleton 0x20)
    , (0x1a6, B.singleton 0x7f)
    ]

-- The live record after a selection of a payload over `halfwords`: the
-- payload's 0x146 bit is 0, so the selection commits ISO AUTO (row 0x1b). The
-- auto flag stays 0, the ISO step 0x009568b4 is the 0x14f key of the AUTO
-- maximum 0x152's code 0, which is 4, and 0x141 holds code 4, whose key that
-- is.
selectedLive :: B.ByteString -> B.ByteString
selectedLive halfwords = overlay (ordinaryLive halfwords) [(0x52, B.singleton 0x04)]

-- The stock getter's capture of extractLive.
extractPayload :: B.ByteString
extractPayload = B.replicate 4 0 <> B.drop 4 validPayload

-- The base every selection case starts from: extractLive captured in P.
extractBase :: Base
extractBase = BaseValid CaptureP extractPayload

-- The generation a selection from NOTHING SELECTED checkpoints first: the
-- captured base with the selection unchanged.
checkpointFile :: Generation -> [Record] -> B.ByteString
checkpointFile gen = encodeFileV2 gen NothingSelected AutoDisable extractBase

selectionFile :: Generation -> Selection -> [Record] -> B.ByteString
selectionFile gen selection = encodeFileV2 gen selection AutoDisable extractBase

-- case specs -------------------------------------------------------------------------

-- AutoIso: the slot's 0x146 bit is 0, so ISO AUTO commits with the AUTO
-- maximum's step 4. SkippedIso: the slot's ISO is one the mode does not
-- admit, so the ISO step cell keeps its initial 0.
data SelectIso = AutoIso | SkippedIso

data StockSelectConsumer
  = SignedProperty138Consumer | WhiteBalanceShiftConsumer | FocusModeConsumer
  | PropertyF2Consumer | JpegFineConsumer | AeBracketingStepConsumer
  | IsoBracketingStepConsumer | PrimaryIsoConsumer | SignedProperty1c5Consumer
  | Property153Consumer | Property1aConsumer | MovieFrameRateConsumer
  | Property217Consumer | SecondaryIsoConsumer | ImageSizeAspectConsumer
  | PropertyF3Consumer | ImageSizeF2Consumer
  deriving (Enum, Bounded)

stockSelectConsumerClaims :: Env -> SelectIso -> [(String, Int, B.ByteString)]
stockSelectConsumerClaims e iso = map claim [minBound .. maxBound]
  where
    claim :: StockSelectConsumer -> (String, Int, B.ByteString)
    claim consumer = case consumer of
      SignedProperty138Consumer -> claimU32 e 0x95685c 0xfffffffa
      WhiteBalanceShiftConsumer -> claimU32 e 0x956868 30
      FocusModeConsumer -> claimU32 e 0x956884 1
      PropertyF2Consumer -> claimU32 e 0x95688c 6
      JpegFineConsumer -> claimU32 e 0x956894 0
      AeBracketingStepConsumer -> claimU32 e 0x9568a4 1
      IsoBracketingStepConsumer -> claimU32 e 0x9568a8 1
      PrimaryIsoConsumer -> claimU32 e 0x9568b4 (case iso of AutoIso -> 4; SkippedIso -> 0)
      SignedProperty1c5Consumer -> claimU32 e 0x9568c4 0
      Property153Consumer -> claimU32 e 0x9568d8 8
      Property1aConsumer -> claimU32 e 0x95694c 1
      -- The fixture's drive key DAT_00956844 is 0, at which a selection writes
      -- 0x214/0x217 through 0x0061c91c and not through the 0x00d4fc74 arms that
      -- set these displayed-rate cells, so both keep their initial 0.
      MovieFrameRateConsumer -> claimU32 e 0x956b30 0
      Property217Consumer -> claimU32 e 0x956b34 0
      SecondaryIsoConsumer -> claimU32 e 0x956b74 25
      -- the 0xf6 arm's aspect cell for IMAGE SIZE row 1 (FUN_00633b0c)
      ImageSizeAspectConsumer -> claimU32 e 0x956898 1
      -- the 0xf3 arm's cell (FUN_00633a4c)
      PropertyF3Consumer -> claimU32 e 0x956b58 1
      -- FUN_0063ead0's 0xf2 cell (FUN_00633a8c)
      ImageSizeF2Consumer -> claimU32 e 0x956b60 6

-- Words of the model's ui_log: the stock SET delegate's calls, the stock
-- editor confirm's calls and the last redraw request.
data UiLogWord = SetCalls | ConfirmCalls | RedrawRequest | SetClass | SetToken

uiLogIndex :: UiLogWord -> Word32
uiLogIndex SetCalls = 0
uiLogIndex ConfirmCalls = 2
uiLogIndex RedrawRequest = 5
uiLogIndex SetClass = 7
uiLogIndex SetToken = 8

data MediaSwapBoundary = BeforeDiscoveryStat

swapBoundaryCell :: MediaSwapBoundary -> (String, Word32)
swapBoundaryCell BeforeDiscoveryStat = ("rbm_swap_boundary", 4)

data Spec = Spec
  { spId :: String
  , spProv :: String
  , spSteps :: [Step]
  , spClaims :: [(String, Int, B.ByteString)]
  , spRefuse :: Word32
  , spSetResult :: Word32
  , spConfirmResult :: Word32
  , spLive :: B.ByteString
  , spCard :: B.ByteString
  , spModelCells :: [(String, Word32)]
  }

data WorkspaceKind = KernelWorkspace | MediaWorkspace | UiWorkspace | StackWorkspace
  | ModalWorkspace | EditorWorkspace | LabelWorkspace | NavigationWorkspace | StockReloadWorkspace | ConversionWorkspace | StockProcessingWorkspace
  deriving (Show)

newtype RegionId = RegionId String deriving (Eq)

data AbiWorkspace = AbiWorkspace
  { workspaceKind :: WorkspaceKind
  , workspaceRegion :: RegionId
  , workspaceStart :: Int
  , workspaceEnd :: Int
  }

-- FUN_0061ceac's copy for the bank's edit root 0x72 (0x009412d4 + id * 0x2c).
menuSnapshotCopy :: Word32
menuSnapshotCopy = 0x0094266c

-- FUN_01165aa0 keys the field drawer's highlight (0x015e6540) and current-value
-- marker (0x015e6544) on words 4 and 5 of a FUN_0061cd14 snapshot and computes
-- the marker row only when word 0 is 2. FUN_011662a0 draws the list and marker
-- only in modes that take that snapshot fresh, or in modes 7 and 13, which put
-- the marker on the highlight; menuSnapshotCopy's word 5 never places it. For
-- word 0 = 2 and word 2 (the node type) = 9, FUN_01165aa0 sets 0x015e64dc = 3
-- and FUN_011662a0 draws no left pane.
-- Each accessor takes the fresh snapshot and returns one word.
data FrameSnapshotWord = SnapshotDepth | SnapshotNodeType | SnapshotCursorToken | SnapshotCurrentToken

snapshotAccessor :: FrameSnapshotWord -> GuestAddress
snapshotAccessor SnapshotDepth = GuestAddress 0x0061ca2c
snapshotAccessor SnapshotNodeType = GuestAddress 0x0061d1a4
snapshotAccessor SnapshotCursorToken = GuestAddress 0x0061d244
snapshotAccessor SnapshotCurrentToken = GuestAddress 0x0061d294

-- The cells region starts at menuSnapshotCopy and ends at the case stack's top.
cellsBase, cellsEnd :: Word32
cellsBase = menuSnapshotCopy
cellsEnd = 0x0095a000

defaultsRowAddress :: Word32
defaultsRowAddress = 0x007cb804

-- The tables-edit region ends with the stock defaults row.
tablesEditEnd :: Word32
tablesEditEnd = defaultsRowAddress + 0x28

-- rb_call_stock_draw_root replays FUN_0115815c's push {r4, r5, r11, lr} and
-- branches here. The stock drawer behind this point lies outside the mapped
-- images; the model returns at once through FUN_0115815c's pop.
stockDrawTail :: GuestAddress
stockDrawTail = GuestAddress 0x01158160

stockDrawTailCode :: B.ByteString
stockDrawTailCode = u32le 0xe8bd8830

-- The same region carries model bodies for FUN_0115815c's two callees, declared
-- as one function span. Each stores its arguments in the draw-state cells at
-- stockPaneRecord and returns: FUN_01166200(preparation) at +0,
-- FUN_011662a0(root, kind) at +4 and +8.
stockPaneRecord :: Word32
stockPaneRecord = 0x015e4800

stockDrawTailEnd :: Word32
stockDrawTailEnd = 0x011662b4

stockDrawTailImage :: B.ByteString
stockDrawTailImage =
  overlay (B.replicate (fromIntegral (stockDrawTailEnd - base)) 0)
    [ (0, stockDrawTailCode)
    , (fromIntegral (0x01166200 - base), B.concat (map u32le [movwR12, movtR12, 0xe58c0000, 0xe12fff1e]))
    , (fromIntegral (0x011662a0 - base), B.concat (map u32le [movwR12, movtR12, 0xe58c0004, 0xe58c1008, 0xe12fff1e]))
    ]
  where
    base = guestWord stockDrawTail
    movwR12 = 0xe300c000 .|. ((stockPaneRecord .&. 0xf000) `shiftL` 4) .|. (stockPaneRecord .&. 0x0fff)
    movtR12 = 0xe340c000 .|. ((stockPaneRecord `shiftR` 12) .&. 0xf0000) .|. ((stockPaneRecord `shiftR` 16) .&. 0x0fff)

stockPaneCallFns :: [(Integer, Integer)]
stockPaneCallFns = [(0x01166200, fromIntegral (stockDrawTailEnd - 0x01166200))]

-- The stock bodies behind rb_bank_draw_backdrop's trampolines (draw mode
-- 0x01112b2c, filled rect 0x01113514, backdrop gate 0x01135db0) lie outside the
-- mapped images. The gate returns 0 (draw) and each body returns at once; one
-- function span covers the three, which keeps the pane case inside the
-- native runner's function limit.
stockDrawCallsEnd :: Word32
stockDrawCallsEnd = 0x01135db8

stockDrawCallsImage :: B.ByteString
stockDrawCallsImage =
  overlay (B.replicate (fromIntegral (stockDrawCallsEnd - 0x01112b2c)) 0)
    [ (0, u32le 0xe12fff1e)
    , (0x01113514 - 0x01112b2c, u32le 0xe12fff1e)
    , (0x01135db0 - 0x01112b2c, u32le 0xe3a00000)
    , (0x01135db0 - 0x01112b2c + 4, u32le 0xe12fff1e)
    ]

stockDrawCallFns :: [(Integer, Integer)]
stockDrawCallFns = [(0x01112b2c, fromIntegral (stockDrawCallsEnd - 0x01112b2c))]

-- A case that calls the draw-root hook maps the stock draw tail in place of the
-- ui-b delegates (stock SET, editor confirm, leaving edge), which only
-- rb_ui_stock reaches; a fetch from their range faults. A draw-root case that
-- opens SELECT CUSTOM SETTING from the shooting menu selects through the stock
-- SET delegate and maps both.
data RegionProfile = ProcessingProfile | DrawRootProfile | SelectDrawProfile | PaneDrawProfile deriving (Eq)

regionProfile :: Env -> Spec -> RegionProfile
regionProfile e sp
  | any callsPane (spSteps sp) = PaneDrawProfile
  | any callsDrawRoot (spSteps sp) && any opensSelect (spSteps sp) = SelectDrawProfile
  | any callsDrawRoot (spSteps sp) = DrawRootProfile
  | otherwise = ProcessingProfile
  where
    callsPane (WriteWord a _) = a == 0x015e4894
    callsPane _ = False
    callsDrawRoot (ReadField a _ _) = a == guestWord (eDrawRoot e)
    callsDrawRoot _ = False
    opensSelect (Dispatch node row 8 _) = node == eRootNode e && row == fromIntegral (eRootSelectRow e)
    opensSelect _ = False

-- The verb-page draw reaches no table region; the pane case drops the two
-- field tables and the ui-b delegates to stay inside the 16-region case ABI.
inProfile :: RegionProfile -> RegionId -> Bool
inProfile ProcessingProfile name =
  name `notElem` [RegionId "stock-draw-tail", RegionId "draw-state", RegionId "stock-draw-calls"]
inProfile DrawRootProfile name =
  name `notElem` [RegionId "ui-b", RegionId "draw-state", RegionId "stock-draw-calls"]
inProfile SelectDrawProfile name =
  name `notElem` [RegionId "draw-state", RegionId "stock-draw-calls"]
inProfile PaneDrawProfile name =
  name `notElem` [RegionId "ui-b", RegionId "tables-ptr", RegionId "tables-edit"]

caseWorkspaces :: Inputs -> Env -> Spec -> [AbiWorkspace]
caseWorkspaces in' e sp =
  [w | w <- abiWorkspaces in' e, inProfile (regionProfile e sp) (workspaceRegion w)]

cellsSize :: Int
cellsSize = fromIntegral (cellsEnd - cellsBase)

cellOffset :: Word32 -> Int
cellOffset a = fromIntegral (a - cellsBase)

abiWorkspaces :: Inputs -> Env -> [AbiWorkspace]
abiWorkspaces in' e =
  [ at UiWorkspace 0x00957e00 0x1200
  , at StackWorkspace 0x00959000 0x1000
  , area MediaWorkspace "media-state" 0 0x200
  , area ModalWorkspace "modal-state" 2 4
  , area ModalWorkspace "modal-state" 0x15e 0x160
  , at EditorWorkspace 0x00954384 0xf
  , at EditorWorkspace 0x009504a8 4
  -- FUN_0063d688 derives this Boolean from notification query 0x1c.
  , at StockProcessingWorkspace 0x00956858 4
  -- FUN_0063d614 mirrors the processing gate.
  , at StockProcessingWorkspace 0x00956c40 4
  -- The bank's draw-time preparation writes the current-row cell, and earlier
  -- candidates' field OK wrote both; the model has no stock walk or block
  -- restore to rewrite them.
  , at NavigationWorkspace (eCurrentRow e) 4
  , at NavigationWorkspace menuSnapshotCopy 0x2c
  , area KernelWorkspace "kdata" 0 (B.length (inKbin in') - B.length (inKtext in'))
  , area MediaWorkspace "span-low" (0x970000 - 0x96cc64) (0x970800 - 0x96cc64)
  -- FUN_0061d9ec reloads only the ordinary block; the C record starts at +0x140.
  , area StockReloadWorkspace "mirror" 0x10 0x128
  -- The ordinary property-0x1c6 setter saves its previous byte at 0x0062d744.
  , area StockReloadWorkspace "mirror" 0x1a6 0x1a7
  -- FUN_00631a00 writes the Boolean conversion-not-found word.
  , area ConversionWorkspace "mirror" 0x1c8 0x1cc
  ]
  ++ [at LabelWorkspace (eNameBase e + (3 + slot) * 0x42) 26 | slot <- [0 .. 6]]
  ++ [at NavigationWorkspace (node + 0x1c) 4 | node <- eSlot e : eSelect e : eEditor e : eEditScreen e : eDialog e : eSubs e ++ eFields e]
  where
    area kind region lo hi = AbiWorkspace kind (RegionId region) lo hi
    at kind address count = let (region, offset) = regionOf e address in area kind region offset (offset + count)

stockFns :: [(Word32, Int)]
stockFns =
  [ (0x0061c400, 0x6464cc - 0x61c400)
  , (0x006464cc, 0x6464f4 - 0x6464cc)
  , (0x006464f4, 0x646520 - 0x6464f4)
  , (0x00646574, 48)
  , (0x00646af0, 116)
  , (0x00646c04, 152)
  , (0x0004d978, 244)
  , (0x0072b3a4, 16)
  , (0x00d4d604, 20)
  , (0x00d4fc74, 10768)
  ]

specSet :: Env -> Word32 -> [Spec]
specSet e epochBump = slotCases ++ otherCases
  where
    sub k = eSubs e !! k
    field k = eFields e !! k
    out o = eOut e + 4 * fromIntegral o
    uiLog w = eUiLog e + 4 * uiLogIndex w
    rowToken node row = menuWord e (menuWord e (node + 0x20) + row * 8)
    -- The edit-screen row whose child is field list k.
    screenRow :: Int -> Word32
    screenRow k =
      maybe (error "a field list is not on the edit screen") fromIntegral $
        findIndex (== field k) [menuWord e (menuWord e (eEditScreen e + 0x20) + 8 * r + 4) | r <- [0 .. 8]]
    drawRoot o = ReadField (guestWord (eDrawRoot e)) 1 o
    cardCells = [("rbm_a_present", 1), ("rbm_a_size", 512)]
    stagedDr = B.take 0x05 validPayload <> B.singleton 0x80 <> B.drop 0x06 validPayload
    -- The DR edit commits under DISABLE: slot 0 keeps its saved payload.
    stagedDrFile = migratedV2 8 (Populated (name26 (slotNames !! 0)) validPayload stagedDr : drop 1 recordsA)
    -- The image-quality row-0 arm at 0x00d51c00 selects both quality flags.
    qualityClaims = [claimU32 e 0x956890 1, claimU32 e 0x956b68 1]
    consumerClaims =
      [ claimU32 e cell downstream
      | (cell, downstream) <-
          [(0x956864, 3), (0x9568a0, 1), (0x9568b8, 3), (0x9568bc, 1),
           (0x9568c0, 3), (0x95699c, 2), (0x956b40, 3), (0x956b44, 4)]
      ] ++ qualityClaims ++ stockSelectConsumerClaims e AutoIso
    -- SELECT CUSTOM SETTING's row r: NOTHING SELECTED at row 0, CUSTOM r
    -- (slot r - 1) below it.
    selectToken :: Word32 -> Word32
    selectToken r = 0x60 + r
    customRow (BankSlot slot) = slot + 1

    slotCases =
      [ Spec
          { spId = "n-slot-" ++ show k
          , spProv =
              "contract: the root row " ++ hex 0x7d ++ " opens the slot list and displays the card's name for slot " ++ show k ++ "; its submenu opens and BACK returns through the slot list to the shooting menu; OK on SELECT CUSTOM SETTING's CUSTOM " ++ show (k + 1) ++ " row from NOTHING SELECTED checkpoints the live settings' capture as the base into the alternate file, applies " ++ slotNames !! k ++ ", writes selection " ++ show (k + 1) ++ " into the other file and hands the event to the stock SET delegate with the select list's class and row token, whose leaf exit pops and blinks"
          , spSteps =
              [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0
              , Draw (eSlot e) k 1 k
              , Navigate (fromIntegral k) 8 1
              , Navigate 0 13 3
              , Navigate 0 13 4
              , Navigate (fromIntegral (eRootSelectRow e)) 8 5
              , Navigate row 8 2
              ]
          , spClaims =
              [ claimU32 e (out 0) 0xffffffff
              , claimU32 e (out 1) 0xffffffff
              , claimU32 e (out 2) 0x2a
              , claimU32 e (uiLog SetCalls) 1
              , claimU32 e (uiLog SetClass) 0x285
              , claimU32 e (uiLog SetToken) (selectToken row)
              , claimU32 e (out 3) 0xffffffff
              , claimU32 e (out 4) 0xffffffff
              , claimU32 e (out 5) 0xffffffff
              , claimU32 e (eCursor e) row
              , claimU32 e (eFrame e) (eSelect e)
              , claimAt e (eOut26 e + 32 * fromIntegral k) (name26 (slotNames !! k))
              , claimAt e 0x980000 (fileSlot (selectionFile 9 (Custom slot) recordsA) <> fileSlot (checkpointFile 8 recordsA) <> emptySlots 1)
              , claimAt e 0x96cc64 (selectedLive (B.take 4 (payloadFor (fromIntegral k))))
              ]
                ++ consumerClaims
          , spRefuse = 0
          , spSetResult = 0x2a
          , spConfirmResult = 0x37
          , spLive = extractLive
          , spCard = cardA
          , spModelCells = cardCells
          }
      | k <- [0 .. 6]
      , let slot = BankSlot (fromIntegral k)
      , let row = customRow slot
      ]

    otherCases =
      [ swapCase
      , swapMidScan
      , sameFrameMediaCase
      , stockCase
      , staleBankCase
      , refusedCase
      , fieldOkCase
      , fieldStoredCase
      , fieldColdCase
      , editScreenRowCase
      , editLeaveCase
      , editFailCase False
      , editFailCase True
      , isoKeepsAutoFlagCase False
      , isoKeepsAutoFlagCase True
      , nativeEntryCase
      , nativeHoverCase
      , nativeDirectCase
      , saveOkCase
      , saveFailCase
      , resetCase True
      , resetCase False
      , refusedDialogCase
      , refusedDialogRetryCase
      , renameFailCase
      , renameSuccessCase
      , renameCancelCase
      , unnamedCase
      , unnamedSetCase
      , fieldGroupCase [0, 1, 2]
      , fieldGroupCase [3, 4, 5]
      , fieldGroupCase [6, 7, 8]
      , editedSelectCase
      , firstSaveCase
      , resetSelectCase
      , lastSlotCase
      , verbPageCase
      , verbPaneCase
      , selectRefusedCase
      , selectNothingCase
      , selectMarkerCase
      , editOrderCase
      , renameUnsetCase
      , renameUnsetEmptyCase
      , renameCapCase
      , renameUnderCapCase
      ] ++ [failureCase cancel | cancel <- [False, True]]

    oneUnsetCard = encodeFileV1 7 0x7d (take 1 slotsA ++ [(B.replicate 26 0, B.replicate 0x28 0)] ++ drop 2 slotsA)

    selectNothingCase =
      Spec
        { spId = "n-select-nothing-restores-base"
        , spProv = "contract: after OK on CUSTOM 1 checkpoints the live settings' capture as the base and selects ALPHA, OK on NOTHING SELECTED (row 0) re-applies that base through the stock consumers, which restores the live record a selection of the captured payload produces, writes one generation with selection 0 and the base kept into the file the selection did not take, and hands the event to the stock SET delegate with the select list's class and row token"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootSelectRow e)) 8 0
            , Navigate (customRow (BankSlot 0)) 8 1
            , Navigate 0 8 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0x2a
            , claimU32 e (out 2) 0x2a
            , claimU32 e (uiLog SetCalls) 2
            , claimU32 e (uiLog SetClass) 0x285
            , claimU32 e (uiLog SetToken) (selectToken 0)
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eSelect e)
            , claimAt e 0x980000 (fileSlot (selectionFile 9 (Custom (BankSlot 0)) recordsA) <> fileSlot (selectionFile 10 NothingSelected recordsA) <> emptySlots 1)
            , claimAt e 0x96cc64 (selectedLive (B.replicate 4 0))
            ] ++ consumerClaims
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = extractLive
        , spCard = cardA
        , spModelCells = cardCells
        }

    selectMarkerCase =
      Spec
        { spId = "n-select-reopen-marker"
        , spProv = "contract: after OK on CUSTOM 3 selects CHARLIE and BACK leaves SELECT CUSTOM SETTING, reopening it with the stock current-row cell on row 0 (the stock push's copy of the entry cursor), the draw-root hook called with root 1 puts the current-row cell on CUSTOM 3's row, so the frame snapshot marks row 3 while the highlight stays on row 0; slot 3's verb page entered afterwards leaves the snapshot at depth 2 with node type 9, for which FUN_011662a0 draws no left pane and so no previous-selection marker"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootSelectRow e)) 8 0
            , Navigate row 8 1
            , Navigate 0 13 2
            , Navigate (fromIntegral (eRootSelectRow e)) 8 3
            , WriteWord (eCurrentRow e) 0
            , drawRoot 4
            , Call (guestWord (snapshotAccessor SnapshotCurrentToken)) 5
            , Call (guestWord (snapshotAccessor SnapshotCursorToken)) 6
            , Navigate 0 13 7
            , Navigate (fromIntegral (eRootEditRow e)) 8 8
            , Navigate slotIndex 8 9
            , Call (guestWord (snapshotAccessor SnapshotDepth)) 10
            , Call (guestWord (snapshotAccessor SnapshotNodeType)) 11
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0x2a
            , claimU32 e (out 2) 0xffffffff
            , claimU32 e (out 3) 0xffffffff
            , claimU32 e (out 5) (selectToken row)
            , claimU32 e (out 6) (selectToken 0)
            , claimU32 e (out 7) 0xffffffff
            , claimU32 e (out 8) 0xffffffff
            , claimU32 e (out 9) 0xffffffff
            , claimU32 e (out 10) 2
            , claimU32 e (out 11) 9
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetToken) (selectToken row)
            , claimU32 e (eFrame e) (sub (fromIntegral slotIndex))
            , claimAt e 0x980000 (fileSlot (selectionFile 9 (Custom slot) recordsA) <> fileSlot (checkpointFile 8 recordsA) <> emptySlots 1)
            , claimAt e 0x96cc64 (selectedLive (B.take 4 (payloadFor slotIndex)))
            ] ++ consumerClaims
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = extractLive
        , spCard = cardA
        , spModelCells = cardCells
        }
      where
        slotIndex = 2
        slot = BankSlot slotIndex
        row = customRow slot

    verbPageCase =
      saveOkCase
        { spId = "n-verb-page-no-left-pane"
        , spProv = "contract: entering each slot's verb page from the slot list leaves the stock frame snapshot at depth 2 with node type 9, the inputs for which FUN_01165aa0 sets 0x015e64dc = 3 and FUN_011662a0 draws no left pane, so the slot list's selection block is not drawn on the verb page; BACK returns to the slot list on the entered slot's row"
        , spSteps = concat
            [ [ Dispatch (eSlot e) (fromIntegral k) 8 (4 * k)
              , Call (guestWord (snapshotAccessor SnapshotDepth)) (4 * k + 1)
              , Call (guestWord (snapshotAccessor SnapshotNodeType)) (4 * k + 2)
              , Navigate 0 13 (4 * k + 3)
              ]
            | k <- [0 .. 6]
            ]
        , spClaims =
            concat
              [ [ claimU32 e (out (4 * k)) 0xffffffff
                , claimU32 e (out (4 * k + 1)) 2
                , claimU32 e (out (4 * k + 2)) 9
                , claimU32 e (out (4 * k + 3)) 0xffffffff
                ]
              | k <- [0 .. 6]
              ]
              ++ [ claimU32 e (eFrame e) (eSlot e)
                 , claimU32 e (eCursor e) 6
                 , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
                 ]
        }

    verbPaneCase =
      Spec
        { spId = "n-verb-page-pane-state"
        , spProv = "contract: a verb page drawn while the entry slide is pending (0x015e4894 = 9 and 0x015e489c = 9, the state FUN_00c77a6c's 0x92f send leaves on entry to root 0x61) starts that animation and takes the renderer's default pane branch: FUN_01166200 receives the pending preparation 9, FUN_011662a0 receives the verb page's root and kind 0, and both cells stay with the animator. Before the review fix the bank zeroed both cells, which stopped FUN_01166200 from starting the slide; FUN_011662a0's cases 7 and 9 draw the left pane's bar through 0x015e64e4, so passing kind 0 selects the branch that reads the depth-2/type-9 snapshot and draws no pane"
        , spSteps =
            [ WriteWord (eFrame e) (sub 2)
            , WriteWord 0x015e4894 9
            , WriteWord 0x015e489c 9
            , ReadField (guestWord (eDrawRoot e)) 1 0
            ]
        , spClaims =
            [ claimU32 e (eFrame e) (sub 2)
            , claimU32 e 0x015e4894 9
            , claimU32 e 0x015e489c 9
            , claimU32 e stockPaneRecord 9
            , claimU32 e (stockPaneRecord + 4) (menuWord e (sub 2))
            , claimU32 e (stockPaneRecord + 8) 0
            , claimAt e 0x980000 cardA
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = []
        }

    selectRefusedCase =
      saveOkCase
        { spId = "n-select-unset-refused"
        , spProv = "contract: OK on SELECT CUSTOM SETTING's CUSTOM 2 row, an unset slot, records RB_UNSET and requests the failure redraw 0x400 before any capture or write: it applies nothing, leaves the card unchanged and keeps the select list open on that row without the stock SET delegate"
        , spSteps = [Dispatch (eSelect e) (customRow (BankSlot 1)) 8 0, Call (eStatusGetter e) 1]
        , spClaims =
            [ claimU32 e (out 0) 1
            , claimU32 e (out 1) 6
            , claimU32 e (uiLog SetCalls) 0
            , claimU32 e (uiLog RedrawRequest) 0x400
            , claimU32 e (eFrame e) (eSelect e)
            , claimU32 e (eCursor e) (customRow (BankSlot 1))
            , claimAt e 0x980000 (fileSlot oneUnsetCard <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spLive = B.replicate 0x1c2 0
        , spCard = oneUnsetCard
        }

    -- The shooting menu's page-1 fields in its row order (ISO, DYNAMIC RANGE,
    -- FILM SIMULATION), WHITE BALANCE after FILM SIMULATION as in the Q menu,
    -- then its page-2 fields in its row order (COLOR, SHARPNESS, HIGHLIGHT TONE,
    -- SHADOW TONE, NOISE REDUCTION).
    screenProperties = [0x141, 0x104, 0xfe, 0xed, 0x10e, 0x10f, 0x110, 0x111, 0xef]
    fieldOfProperty p =
      field (fromMaybe (error ("no edit field for property " ++ hex (toInteger p))) (findIndex ((== p) . F.fieldProperty) F.editFields))

    editOrderCase =
      fieldOkCase
        { spId = "n-edit-screen-order"
        , spProv = "contract: the edit screen's rows open ISO, DYNAMIC RANGE and FILM SIMULATION (shooting-menu page 1, in its row order), WHITE BALANCE (after FILM SIMULATION, as in the Q menu), then COLOR, SHARPNESS, HIGHLIGHT TONE, SHADOW TONE and NOISE REDUCTION (page 2, in its row order); BACK from each list returns to the edit screen on its row; nothing is written"
        , spSteps =
            [Dispatch (eSlot e) 0 8 0, Dispatch (sub 0) 2 8 1]
              ++ concat
                [ [ Dispatch (eEditScreen e) (fromIntegral r) 8 (2 + 3 * r)
                  , Copy (out (3 + 3 * r)) (eFrame e) 4
                  , Navigate 0 13 (4 + 3 * r)
                  ]
                | r <- [0 .. 8]
                ]
        , spClaims =
            concat
              [ [ claimU32 e (out (2 + 3 * r)) 0xffffffff
                , claimU32 e (out (3 + 3 * r)) (fieldOfProperty p)
                , claimU32 e (out (4 + 3 * r)) 0xffffffff
                ]
              | (r, p) <- zip [0 ..] screenProperties
              ]
              ++ [ claimU32 e (eFrame e) (eEditScreen e)
                 , claimU32 e (eCursor e) 8
                 , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
                 , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
                 ]
        }

    renameUnsetCase =
      renameFailCase
        { spId = "n-rename-unset"
        , spProv = "contract: renaming an unset slot to NOVEMBER sets it: the new generation carries the name over the live settings the stock getter reads, not the per-slot defaults row 0x007cb804, as both its saved and working payloads, with the slot's set bit, the editor returns to the slot's verb page in browse state and the slot list displays NOVEMBER"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
            , Navigate 1 8 0
            , Navigate 1 8 1
            , InputName "NOVEMBER"
            , Navigate 0x2a 8 2
            , Draw (eSlot e) 1 1 0
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0xffffffff
            , claimU32 e (out 2) 0xffffffff
            , claimAt e 0x146761e (u16le 0)
            , claimU32 e (eFrame e) (sub 1)
            , claimU32 e (eCursor e) 1
            , claimAt e (eOut26 e) (name26 "NOVEMBER")
            , claimAt e 0x980000 (fileSlot oneUnsetCard <> fileSlot named <> emptySlots 1)
            , claimAt e 0x96cc64 extractLive
            ]
        , spCard = oneUnsetCard
        , spModelCells = cardCells
        , spLive = extractLive
        }
      where
        named = migratedV2 8 (take 1 recordsA ++ [Populated (name26 "NOVEMBER") extractPayload extractPayload] ++ drop 2 recordsA)

    renameUnsetEmptyCase =
      renameUnsetCase
        { spId = "n-rename-unset-empty"
        , spProv = "contract: confirming the empty name an unset slot's editor opens with writes nothing, returns to the slot's verb page in browse state and leaves the slot list displaying CUSTOM 2"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
            , Navigate 1 8 0
            , Navigate 1 8 1
            , Navigate 0x2a 8 2
            , Draw (eSlot e) 1 1 0
            ]
        , spClaims =
            [ claimU32 e (out 2) 0xffffffff
            , claimAt e 0x146761e (u16le 0)
            , claimU32 e (eFrame e) (sub 1)
            , claimU32 e (eCursor e) 1
            , claimAt e (eOut26 e) (name26 "CUSTOM 2")
            , claimAt e 0x980000 (fileSlot oneUnsetCard <> emptySlots 2)
            , claimAt e 0x96cc64 extractLive
            ]
        }

    renameCapCase =
      renameFailCase
        { spId = "n-rename-twelve-cap"
        , spProv = "contract: with twelve characters staged, SET on a character row and on the space row adds nothing and does not enter the stock editor confirm, as FUN_00d4a2a0 ignores an insert into full buffer A; SET on the delete row still enters it; confirming stores the twelve characters"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
            , Navigate 0 8 0
            , Navigate 1 8 1
            , InputName "ABCDEFGHIJKL"
            , Navigate 0 8 2
            , Navigate 0x24 8 3
            , Navigate 0x29 8 5
            , Navigate 0x2a 8 6
            , Draw (eSlot e) 0 1 0
            ]
        , spClaims =
            [ claimU32 e (out 2) 1
            , claimU32 e (out 3) 1
            , claimU32 e (out 5) 0x37
            , claimU32 e (out 6) 0xffffffff
            , claimU32 e (uiLog ConfirmCalls) 1
            , claimAt e 0x146761e (u16le 0)
            , claimU32 e (eFrame e) (sub 0)
            , claimU32 e (eCursor e) 1
            , claimAt e (eOut26 e) (name26 "ABCDEFGHIJKL")
            , claimAt e 0x980000 (fileSlot cardA <> fileSlot renamed <> emptySlots 1)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spModelCells = cardCells
        }
      where
        renamed = migratedV2 8 (Populated (name26 "ABCDEFGHIJKL") validPayload validPayload : drop 1 recordsA)

    renameUnderCapCase =
      renameFailCase
        { spId = "n-rename-eleven-insert"
        , spProv = "contract: with eleven characters staged, SET on a character row enters the stock editor confirm and returns its result; DISP/BACK then cancels without a bank write"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
            , Navigate 0 8 0
            , Navigate 1 8 1
            , InputName "ABCDEFGHIJK"
            , Navigate 0 8 2
            , Navigate 0 13 3
            ]
        , spClaims =
            [ claimU32 e (out 2) 0x37
            , claimU32 e (out 3) 0xffffffff
            , claimU32 e (uiLog ConfirmCalls) 1
            , claimAt e 0x146761e (u16le 0)
            , claimU32 e (eFrame e) (sub 0)
            , claimU32 e (eCursor e) 1
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spModelCells = cardCells
        }

    sameFrameMediaCase =
      saveOkCase
        { spId = "n-same-frame-media-refresh"
        , spProv = "contract: replacing media and refreshing the existing verb page cannot rebind its accepted identity or authorize Save Current; refusal leaves the page, card files and live settings unchanged"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0
            , Navigate 0 8 1
            , Copy 0x980000 (eFileB e) (fromIntegral fileStride)
            , Call epochBump 2
            , Refresh
            , Navigate 0 8 3
            , Navigate 0 8 4
            , Call (eStatusGetter e) 6
            ]
        , spClaims =
            [ claimU32 e (out 3) 1
            , claimU32 e (out 4) 1
            , claimU32 e (out 6) 13
            , claimU32 e (uiLog SetCalls) 0
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e 0x980000 (fileSlot cardB <> emptySlots 2)
            , claimAt e 0x96cc64 extractLive
            ]
        }

    unnamedCase =
      saveOkCase
        { spId = "n-unnamed-slot-labels"
        , spProv = "contract: seven unset slots display CUSTOM 1 through CUSTOM 7 without writing a bank file"
        , spSteps = Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0
            : [Draw (eSlot e) k 1 k | k <- [0 .. 6]]
        , spClaims = [claimAt e 0x980000 (emptySlots 3)]
            ++ [claimU32 e (eFrame e) (eSlot e), claimU32 e (eCursor e) 0]
            ++ [claimAt e (eOut26 e + 32 * fromIntegral k) (name26 ("CUSTOM " ++ show (k + 1))) | k <- [0 .. 6]]
        , spCard = emptySlots 3
        , spModelCells = []
        }

    unnamedSetCase =
      unnamedCase
        { spId = "n-unnamed-set-slot-labels"
        , spProv = "contract: seven set slots with empty stored names display CUSTOM 1 through CUSTOM 7 without changing their stored names or payloads"
        , spCard = unnamedBank
        , spModelCells = cardCells
        , spClaims = [claimAt e 0x980000 (fileSlot unnamedBank <> emptySlots 2)]
            ++ [claimU32 e (eFrame e) (eSlot e), claimU32 e (eCursor e) 0]
            ++ [claimAt e (eOut26 e + 32 * fromIntegral k) (name26 ("CUSTOM " ++ show (k + 1))) | k <- [0 .. 6]]
        }
      where
        unnamedBank = encodeFileV1 7 0x7f [(name26 "", payloadFor k) | k <- [0 .. 6]]

    fieldGroupCase group =
      fieldOkCase
        { spId = "n-fields-" ++ intercalate "-" (map show group)
        , spProv = "contract: OK on a row in each of fields " ++ show group ++ " stores that field before its list closes, writing one new generation per changed code into the alternate bank file, whose slot-0 working payload accumulates the edits while its saved payload stays the input card's, and hands the event to the stock SET delegate; BACK leaves each list and then the edit screen with no confirmation; editing never changes the live record"
        , spSteps =
            [Dispatch (eSlot e) 0 8 0, Dispatch (sub 0) 2 8 1]
            ++ concat
              [[ Dispatch (eEditScreen e) (screenRow k) 8 (2 + 3 * i)
               , Dispatch (field k) (editCursor !! k) 8 (3 + 3 * i)
               , Dispatch (field k) 0 13 (4 + 3 * i)
               ] | (i, k) <- zip [0 ..] group]
            ++ [Dispatch (eEditScreen e) 0 13 leaveOut]
        , spClaims =
            [ claimU32 e (out leaveOut) 0xffffffff
            , claimU32 e (eFrame e) (sub 0)
            , claimU32 e (eCursor e) 2
            , claimAt e 0x980000 (fileSlot fileA <> fileSlot fileB <> emptySlots 1)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ] ++ concat
              [[claimU32 e (out (3 + 3 * i)) 0x2a, claimU32 e (out (4 + 3 * i)) 0xffffffff] | i <- [0 .. length group - 1]]
        }
      where
        leaveOut = 2 + 3 * length group
        -- Each changed field takes the next generation, into B for odd counts
        -- and A for even ones; generation 7 is the v1 input card.
        states = scanl (\p k -> placeEdit (F.editFields !! k) p) validPayload
          [k | k <- group, editChanged (F.editFields !! k)]
        written = length states - 1
        fileAt 0 = cardA
        fileAt j = migratedV2 (7 + fromIntegral j) (Populated (name26 (slotNames !! 0)) validPayload (states !! j) : drop 1 recordsA)
        fileA = fileAt (if even written then written else written - 1)
        fileB = if written == 0 then B.empty else fileAt (if odd written then written else written - 1)

    editedSelectCase =
      fieldOkCase
        { spId = "n-select-edited"
        , spProv = "contract: selecting CUSTOM 1, a slot holding the nine edited field codes, from NOTHING SELECTED checkpoints the live settings' capture as the base and applies the codes through the stock consumers, except ISO code 0, which the stock check does not admit in mode 0x21 and which keeps its live code, writes selection 1 and then hands the event to the stock SET delegate with the select list's class"
        , spSteps = [Dispatch (eSelect e) (customRow (BankSlot 0)) 8 0]
        , spClaims =
            [ claimU32 e (out 0) 0x2a
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetClass) 0x285
            , claimU32 e (eFrame e) (eSelect e)
            , claimU32 e (eCursor e) (customRow (BankSlot 0))
            , claimAt e 0x980000 (fileSlot (selectionFile 9 (Custom (BankSlot 0)) editedRecords) <> fileSlot (checkpointFile 8 editedRecords) <> emptySlots 1)
            , claimAt e 0x96cc64 editedLive
            ] ++ [claimU32 e cell value | (cell, value) <- [(0x956864, 0), (0x9568a0, 4), (0x9568b8, 0), (0x9568bc, 0), (0x9568c0, 2), (0x95699c, 4), (0x956b40, 0), (0x956b44, 1)]] ++ qualityClaims ++ stockSelectConsumerClaims e SkippedIso
        , spCard = editedCard
        , spLive = extractLive
        }
      where
        editedSlots = (name26 (slotNames !! 0), editedPayload) : drop 1 slotsA
        editedCard = encodeFileV1 7 0x7f editedSlots
        editedRecords = map migratedRecord editedSlots
        editedLive = overlay (ordinaryLive (B.take 4 validPayload))
          [ (0x5a, B.singleton 0), (0x26, B.singleton 0), (0x2b, B.singleton 0x60), (0x33, B.singleton 0)
          , (0x34, B.singleton 0x20), (0x2d, B.singleton 0x80), (0x85, B.singleton 0), (0x8a, B.singleton 0) ]

    -- The row each field's OK selects, and the payload those rows produce.
    editCursor = [0, 0, 1, 0, 0, 0, 0, 19, 0] :: [Word32]
    -- the nine edited codes over the admitted fixture's other fields, with the
    -- 0x146 bit at 1 that staging ISO code 0 writes
    editedPayload = placeBits isoAutoFlag (fromIntegral (F.fieldMask isoAutoFlag))
      (foldl (\p f -> placeBits f (masked f editedCodes) p) validPayload F.editFields)
    editedCodes = B.pack [0x34,0x12,0x78,0x56,0,0,0,0x60,0,0x20,0,0,0,0,0,0,0x80,0,0,0x20,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
    fieldValue f p = foldr (\b acc -> acc `shiftL` 8 .|. fromIntegral (B.index p (fromIntegral (F.fieldOffset f) + b))) (0 :: Word32) [0 .. fromIntegral (F.fieldBytes f) - 1]
    masked f p = fieldValue f p .&. fromIntegral (F.fieldMask f)
    editChanged f = masked f editedPayload /= masked f validPayload
    -- Staging ISO (0x141) also stages its auto flag 0x146 = (code /= 0x1b).
    placeEdit f p
      | F.fieldProperty f == 0x141 =
          placeBits isoAutoFlag (if masked f editedPayload `shiftR` fromIntegral (F.fieldShift f) == 0x1b then 0 else fromIntegral (F.fieldMask isoAutoFlag)) placed
      | otherwise = placed
      where placed = placeBits f (masked f editedPayload) p
    isoAutoFlag = head [g | g <- F.payloadFields, F.fieldProperty g == 0x146]
    placeBits f bits p =
      let v = (fieldValue f p .&. complement (fromIntegral (F.fieldMask f))) .|. bits
          o = fromIntegral (F.fieldOffset f)
          n = fromIntegral (F.fieldBytes f)
      in B.take o p <> B.pack [fromIntegral (v `shiftR` (8 * b)) | b <- [0 .. n - 1]] <> B.drop (o + n) p

    lastSlotCase =
      saveOkCase
        { spId = "n-last-slot-rename-save"
        , spProv = "contract: renaming slot 7 to a maximum-length name then overwriting its payload with Save Current preserves that name, the other six slots and live settings, and Save Current's OK hands the event to the stock SET delegate on the slot-7 verb page"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0
            , Navigate 6 8 1
            , Navigate 1 8 2
            , InputName "SIERRA-SLOT7"
            , Navigate 0x2a 8 3
            , Navigate 0 8 4
            , Draw (eSlot e) 6 1 0
            , Draw (eSlot e) 0 1 1
            ]
        , spClaims =
            [ claimU32 e (out 3) 0xffffffff
            , claimU32 e (out 4) 0x2a
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetClass) 0x276
            , claimU32 e (uiLog SetToken) (rowToken (sub 6) 0)
            , claimU32 e (eFrame e) (sub 6)
            , claimU32 e (eCursor e) 0
            , claimAt e (eOut26 e) (name26 "SIERRA-SLOT7")
            , claimAt e (eOut26 e + 32) (name26 "ALPHA")
            , claimAt e 0x980000 (fileSlot saved <> fileSlot renamed <> emptySlots 1)
            , claimAt e 0x96cc64 extractLive
            ]
        }
      where
        saved = migratedV2 9 (take 6 recordsA ++ [Populated (name26 "SIERRA-SLOT7") extractPayload extractPayload])
        renamed = migratedV2 8 (take 6 recordsA ++ [Populated (name26 "SIERRA-SLOT7") (payloadFor 6) (payloadFor 6)])

    firstSaveCase =
      saveOkCase
        { spId = "n-first-save-select"
        , spProv = "contract: Save Current into an empty bank writes one set slot with an empty stored name and hands the event to the stock SET delegate; reopening SELECT CUSTOM SETTING accepts the file with six unset slots, and OK on CUSTOM 1 checkpoints the live settings' capture as the base, reproduces the saved ordinary fields, writes selection 1 and hands its OK to the stock SET delegate too; the displayed name is CUSTOM 1"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
            , Navigate 0 8 0
            , Navigate 0 8 1
            , Navigate 0 13 5
            , Navigate 0 13 6
            , Navigate (fromIntegral (eRootSelectRow e)) 8 7
            , Navigate (customRow (BankSlot 0)) 8 3
            , Draw (eSelect e) (fromIntegral (customRow (BankSlot 0))) 1 0
            ]
        , spClaims =
            [ claimU32 e (out 1) 0x2a
            , claimU32 e (uiLog SetCalls) 2
            , claimU32 e (uiLog SetClass) 0x285
            , claimU32 e (uiLog SetToken) (selectToken (customRow (BankSlot 0)))
            , claimU32 e (out 3) 0x2a
            , claimU32 e (eFrame e) (eSelect e)
            , claimU32 e (eCursor e) (customRow (BankSlot 0))
            , claimAt e 0x980000 (fileSlot (selectionFile 3 (Custom (BankSlot 0)) savedRecords) <> fileSlot (checkpointFile 2 savedRecords) <> emptySlots 1)
            , claimAt e 0x96cc64 (selectedLive (B.replicate 4 0))
            , claimAt e (eOut26 e) (name26 "CUSTOM 1")
            ] ++ consumerClaims
        , spCard = emptySlots 3
        , spModelCells = []
        }
      where
        savedRecords = [Populated (name26 "") extractPayload extractPayload]
    navigationReset (BankSlot slot) =
      [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
      , Navigate slot 8 0
      , Navigate 3 8 1
      , Navigate 0 8 2
      , Navigate 0 13 5
      , Navigate 0 13 6
      , Navigate (fromIntegral (eRootSelectRow e)) 8 7
      ]


    resetSelectCase =
      (resetCase True)
        { spId = "n-reset-select-other"
        , spProv = "contract: resetting slot 7 clears its maximum-length name, payload and set bit; a subsequent OK on CUSTOM 2 reloads the bank, checkpoints the live settings' capture as the base into the file the reset did not take, applies slot 1 unchanged, including its name and both full-width fields, writes selection 2 into the other file and hands its OK to the stock SET delegate"
        , spSteps = navigationReset (BankSlot 6)
            ++ [ Navigate (customRow (BankSlot 1)) 8 3
               , Draw (eSelect e) (fromIntegral (customRow (BankSlot 1))) 1 0
               , Draw (eSlot e) 6 1 1
               ]
        , spClaims =
            [ claimU32 e (out 2) 1
            , claimU32 e (out 3) 0x2a
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetToken) (selectToken (customRow (BankSlot 1)))
            , claimU32 e (eFrame e) (eSelect e)
            , claimU32 e (eCursor e) (customRow (BankSlot 1))
            , claimAt e 0x980000 (fileSlot (checkpointFile 9 resetRecords) <> fileSlot (selectionFile 10 (Custom (BankSlot 1)) resetRecords) <> emptySlots 1)
            , claimAt e 0x96cc64 (selectedLive (B.take 4 (payloadFor 1)))
            , claimAt e (eOut26 e) (name26 "BRAVO")
            , claimAt e (eOut26 e + 32) (name26 "CUSTOM 7")
            ] ++ consumerClaims
        , spCard = longNameBank
        , spLive = extractLive
        }
      where
        longNameBank = encodeFileV1 7 0x7f (take 6 slotsA ++ [(name26 "SIERRA-SLOT7", payloadFor 6)])
        resetRecords = take 6 recordsA ++ [Unpopulated]

    failureCase cancel =
      base
        { spId = "n-reset-write-fail" ++ (if cancel then "-cancel" else "")
        , spProv = "contract: a read-only reset completion reports the storage error, leaves the bank unchanged and keeps the confirmation open; " ++ (if cancel then "CANCEL exits without retrying the write" else "the dialog remains on OK")
        , spSteps = spSteps base ++ [Dispatch (eDialog e) 1 8 (lastOut + 1) | cancel]
        , spClaims =
            [ claimU32 e (out lastOut) 1
            , claimU32 e (eFrame e) (if cancel then sub 0 else eDialog e)
            , claimU32 e (eCursor e) (if cancel then 3 else 0)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (spLive base)
            ] ++ [claimU32 e (out (lastOut + 1)) 1 | cancel]
        , spModelCells = cardCells ++ [("rbm_write_comp", 8)]
        }
      where
        base = resetCase True
        lastOut = 2 :: Int

    saveFailCase =
      saveOkCase
        { spId = "n-save-write-fail"
        , spProv = "contract: a read-only completion of Save Current's OK reports the storage error, leaves the bank unchanged and keeps the verb page open on the Save Current row without the stock SET delegate"
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (uiLog SetCalls) 0
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 extractLive
            ]
        , spModelCells = cardCells ++ [("rbm_write_comp", 8)]
        }

    refusedDialogCase =
      Spec
        { spId = "n-refused-dialog"
        , spProv = "contract: a refused Reset confirmation raise keeps the submenu frame and restores the caller's row (3), so the hidden dialog row is not left selected, and writes nothing"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , WriteWord (eRefuse e) 1
            , Dispatch (sub 0) 3 8 1
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (eCursor e) 3
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }

    refusedDialogRetryCase =
      Spec
        { spId = "n-refused-dialog-retry"
        , spProv = "contract: after a refused confirmation raise, SET on the submenu's Reset row raises the confirmation dialog with its cursor on row 0"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , WriteWord (eRefuse e) 1
            , Dispatch (sub 0) 3 8 1
            , WriteWord (eRefuse e) 0
            , Dispatch (sub 0) 3 8 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 1
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eDialog e)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }

    swapCase =
      Spec
        { spId = "n-swap"
        , spProv = "contract: the first root push publishes card A's names; copying card B over RECIPEA.BIN and bumping the media epoch makes the next root push publish card B's names; the pop records the child's cursor and the push resets the recorded nonzero cursor to row 0"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0
            , Draw (eSlot e) 0 1 0
            , WriteWord (eCursor e) 5
            , Dispatch (eSlot e) 0 13 1
            , Copy 0x980000 (eFileB e) (fromIntegral fileStride)
            , Call epochBump 2
            , Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 3
            , Draw (eSlot e) 0 1 8
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0xffffffff
            , claimU32 e (out 3) 0xffffffff
            , claimU32 e (eCursor e) 0
            , claimAt e (eOut26 e) (name26 (slotNames !! 0))
            , claimAt e (eOut26 e + 32 * 8) (name26 (swapNames !! 0))
            , claimU32 e (eFrame e) (eSlot e)
            ]
              ++ [claimAt e 0x980000 (fileSlot cardB)]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }

    swapMidScan =
      Spec
        { spId = "n-swap-midscan"
        , spProv = "contract: a card change during discovery completes the page push without publishing names from either partially read bank; all seven rows display their CUSTOM fallback labels"
        , spSteps = [Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0]
            ++ [Draw (eSlot e) k 1 k | k <- [0 .. 6]]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eSlot e)
            , claimAt e 0x980000 swappedCard
            ] ++ [claimAt e (eOut26 e + 32 * fromIntegral k) (name26 ("CUSTOM " ++ show (k + 1))) | k <- [0 .. 6]]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardWith3 cardA cardC
        , spModelCells = cardCells ++ [swapBoundaryCell BeforeDiscoveryStat, ("rbm_swap_a_size", 512)]
        }
      where
        swappedCard = fileSlot cardC <> emptySlots 1 <> fileSlot cardC

    stockCase =
      Spec
        { spId = "n-stock-page"
        , spProv = "contract: an unrelated stock page (class 5), including a row with the bank root token but a non-bank child, preserves the frame, cursor, stock child parent fields, live state and durable bank"
        , spSteps =
            [ Dispatch (eStockNode e) 0 8 0
            , Dispatch (eStockNode e) 1 8 1
            , Dispatch (eStockNode e) 0 13 2
            ]
        , spClaims =
            [ claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eStockNode e)
            , claimAt e 0x980000 cardA
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }

    staleBankCase =
      Spec
        { spId = "n-stale-bank"
        , spProv = "contract: a slot-list cursor the values array does not carry (99) clamps the cursor cell to row 0 and drops the event without entering the stock delegate"
        , spSteps = [Dispatch (eSlot e) 99 8 0]
        , spClaims =
            [ claimU32 e (out 0) 1
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eSlot e)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }


    refusedCase =
      Spec
        { spId = "n-refused-push"
        , spProv = "contract: a refused submenu push returns refusal and preserves the current slot-list frame and selected row"
        , spSteps = [Dispatch (eSlot e) 0 8 0]
        , spClaims =
            [ claimU32 e (out 0) 0
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eSlot e)
            ]
        , spRefuse = 1
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }

    fieldColdCase =
      fieldOkCase
        { spId = "n-field-cold-stored-marker"
        , spProv = "contract: opening the DR list of a slot whose stored code 1 (pinned row 1) differs from the live code 0, with the native entry cursor on row 1 and the stock current-row cell on the live row 0, the draw-root hook called with root 1 leaves the drawer's frame snapshot at depth 2, highlighting and marking row 1; the card and live record are untouched"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) (screenRow 0) 8 2
            , WriteWord (eCursor e) 1
            , WriteWord (eCurrentRow e) 0
            , ReadField (eFieldGetter e) 0x267 3
            , drawRoot 4
            , Call (guestWord (snapshotAccessor SnapshotDepth)) 5
            , Call (guestWord (snapshotAccessor SnapshotCursorToken)) 6
            , Call (guestWord (snapshotAccessor SnapshotCurrentToken)) 7
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 0xffffffff
            , claimU32 e (out 3) 1
            , claimU32 e (out 5) 2
            , claimU32 e (out 6) (rowToken (field 0) 1)
            , claimU32 e (out 7) (rowToken (field 0) 1)
            , claimU32 e (eFrame e) (field 0)
            , claimU32 e (eCursor e) 1
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        }

    editScreenRowCase =
      fieldOkCase
        { spId = "n-edit-screen-current-row"
        , spProv = "contract: on the edit screen of a staged edit, the draw-root hook called with root 1 leaves the stock current-row cell on row 3, so the frame snapshot's current-value word stays row 3's token"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , WriteWord (eCurrentRow e) 3
            , drawRoot 3
            , Call (guestWord (snapshotAccessor SnapshotCurrentToken)) 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) (rowToken (eEditScreen e) 3)
            , claimU32 e (eFrame e) (eEditScreen e)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        }

    fieldOkCase =
      Spec
        { spId = "n-field-ok"
        , spProv = "contract: the edit screen's DYNAMIC RANGE row opens the DR field list; OK on DR row 2, whose code differs from the stored code 1, writes a new-generation bank file carrying code 2 in the slot's working payload, with its saved payload and name unchanged, then hands the event to the stock SET delegate with the list's class and row token and returns its result; the getter reads code 2 and the live record is untouched"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) (screenRow 0) 8 2
            , Dispatch (field 0) 2 8 3
            , ReadField (eFieldGetter e) 0x267 4
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 0xffffffff
            , claimU32 e (out 3) 0x2a
            , claimU32 e (out 4) 2
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetClass) 0x267
            , claimU32 e (uiLog SetToken) (rowToken (field 0) 2)
            , claimU32 e (eCursor e) 2
            , claimU32 e (eFrame e) (field 0)
            , claimAt e 0x980000 (fileSlot cardA <> fileSlot stagedDrFile <> emptySlots 1)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }

    fieldStoredCase =
      fieldOkCase
        { spId = "n-field-ok-stored"
        , spProv = "contract: OK on DR row 1, the slot's stored code, writes no bank file and still hands the event to the stock SET delegate; the getter reads code 1"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) (screenRow 0) 8 2
            , Dispatch (field 0) 1 8 3
            , ReadField (eFieldGetter e) 0x267 4
            ]
        , spClaims =
            [ claimU32 e (out 3) 0x2a
            , claimU32 e (out 4) 1
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetToken) (rowToken (field 0) 1)
            , claimU32 e (eCursor e) 1
            , claimU32 e (eFrame e) (field 0)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        }

    editLeaveCase =
      fieldOkCase
        { spId = "n-edit-leave"
        , spProv = "contract: after OK stores DR code 2, BACK leaves the field list and BACK on the edit screen returns to the submenu's EDIT row with no confirmation; the card holds exactly the one generation the OK wrote and the live record is untouched"
        , spSteps =
            spSteps fieldOkCase
              ++ [ Navigate 0 13 5
                 , Navigate 0 13 6
                 ]
        , spClaims =
            [ claimU32 e (out 3) 0x2a
            , claimU32 e (out 5) 0xffffffff
            , claimU32 e (out 6) 0xffffffff
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (eCursor e) 2
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e 0x980000 (fileSlot cardA <> fileSlot stagedDrFile <> emptySlots 1)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        }

    editFailCase back =
      fieldOkCase
        { spId = "n-edit-write-fail" ++ (if back then "-back" else "")
        , spProv = "contract: a read-only completion of the field OK's write reports the storage error, leaves the bank unchanged, keeps the DR list open without the stock SET delegate and keeps the getter on the stored code 1; " ++ (if back then "BACK then leaves the list without retrying the write" else "the list stays open")
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) (screenRow 0) 8 2
            , Dispatch (field 0) 2 8 3
            , ReadField (eFieldGetter e) 0x267 4
            ] ++ [Navigate 0 13 5 | back]
        , spClaims =
            [ claimU32 e (out 3) 1
            , claimU32 e (out 4) 1
            , claimU32 e (uiLog SetCalls) 0
            , claimU32 e (eCursor e) (if back then screenRow 0 else 2)
            , claimU32 e (eFrame e) (if back then eEditScreen e else field 0)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ] ++ [claimU32 e (out 5) 0xffffffff | back]
        , spModelCells = cardCells ++ [("rbm_write_comp", 8)]
        }

    -- Slot 0 of cardA is a snapshot taken in ISO AUTO: 0x141 holds the numeric
    -- code 0x0b and the 0x146 bit (+0x1f bit 7) is 0. Each case then stores DR
    -- code 2, and the new generation must carry that flag unchanged.
    isoCursor code =
      fromMaybe (error "ISO code not in the list")
        (findIndex (== code) (takeWhile (/= 0x7ffffffe) [rowToken (field 7) r | r <- [0 .. 27]]))
    isoKeepsAutoFlagCase failed =
      fieldOkCase
        { spId = if failed then "n-iso-write-fail-keeps-auto-flag" else "n-iso-reselect-keeps-auto-flag"
        , spProv = "contract: in a slot snapshotted in ISO AUTO (0x141 = 0x0b, 0x146 = 0), " ++ (if failed then "an ISO OK on code 0x0c whose write fails puts back both the stored code and the staged 0x146" else "OK on the stored ISO code 0x0b writes nothing and leaves the staged 0x146") ++ ", so the following DR OK on code 2 writes a generation whose working payload differs from the slot's payload only in DR"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) (screenRow 7) 8 2
            , Dispatch (field 7) (fromIntegral (isoCursor (if failed then 0x0c else 0x0b))) 8 3
            ]
            ++ [WriteWord (eWriteComp e) 0 | failed]
            ++ [ if failed then Navigate 0 13 4 else Dispatch (field 7) 0 13 4
               , Dispatch (eEditScreen e) (screenRow 0) 8 5
               , Dispatch (field 0) 2 8 6
               ]
        , spClaims =
            [ claimU32 e (out 3) (if failed then 1 else 0x2a)
            , claimU32 e (out 6) 0x2a
            , claimU32 e (eFrame e) (field 0)
            , claimU32 e (eCursor e) 2
            , claimAt e 0x980000 (fileSlot cardA <> fileSlot stagedDrFile <> emptySlots 1)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spModelCells = cardCells ++ [("rbm_write_comp", 8) | failed]
        }

    nativeEntryCase =
      fieldOkCase
        { spId = "n-native-field-before-ok"
        , spProv = "contract: a native property write while the DR list is open does not change its getter value or write the card; BACK returns to the edit screen without writing card or live settings"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) (screenRow 0) 8 2
            , NativeWrite (GuestAddress 0x0061c91c) (FieldHost 0x267) (FieldCode 2) 3
            , ReadField (eFieldGetter e) 0x267 4
            , Navigate 0 13 5
            , ReadField (eFieldGetter e) 0x267 6
            ]
        , spClaims =
            [ claimU32 e (out 4) 1
            , claimU32 e (out 6) 1
            , claimU32 e (eFrame e) (eEditScreen e)
            , claimU32 e (eCursor e) (screenRow 0)
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        }

    nativeHoverCase =
      fieldOkCase
        { spId = "n-native-field-after-ok"
        , spProv = "contract: after DR OK stores code 2, a native cursor write of valid code 0 cannot replace it; leaving the list and the edit screen keeps code 2 as the only change to the slot's working payload, with the slot name, its saved payload and live settings unchanged"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) (screenRow 0) 8 2
            , Dispatch (field 0) 2 8 3
            , WriteWord (eCursor e) 0
            , NativeWrite (GuestAddress 0x0061c91c) (FieldHost 0x267) (FieldCode 0) 8
            , ReadField (eFieldGetter e) 0x267 9
            , Navigate 0 13 4
            , Navigate 0 13 5
            ]
        , spClaims =
            [ claimU32 e (out 3) 0x2a
            , claimU32 e (out 4) 0xffffffff
            , claimU32 e (out 5) 0xffffffff
            , claimU32 e (out 9) 2
            , claimU32 e (eCursor e) 2
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e 0x980000 (fileSlot cardA <> fileSlot stagedDrFile <> emptySlots 1)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        }

    nativeDirectCase =
      fieldOkCase
        { spId = "n-native-field-outside-leaf"
        , spProv = "contract: a native DR property write from the edit screen does not change the getter value, and BACK on the edit screen returns to the submenu with no confirmation and no card write"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , NativeWrite (GuestAddress 0x0061c91c) (FieldHost 0x267) (FieldCode 2) 2
            , ReadField (eFieldGetter e) 0x267 3
            , Navigate 0 13 4
            ]
        , spClaims =
            [ claimU32 e (out 3) 1
            , claimU32 e (out 4) 0xffffffff
            , claimU32 e (eFrame e) (sub 0)
            , claimU32 e (eCursor e) 2
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        }

    saveOkCase =
      Spec
        { spId = "n-save-ok"
        , spProv = "contract: OK on the submenu's Save Current row (index 0) writes a new-generation bank file carrying slot 0's live snapshot under its kept name, raises no dialog, then hands the event to the stock SET delegate with the verb page's class and row token and returns its result"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 0 8 1
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0x2a
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetClass) 0x270
            , claimU32 e (uiLog SetToken) (rowToken (sub 0) 0)
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e 0x980000 (fileSlot cardA <> fileSlot savedFile <> emptySlots 1)
            , claimAt e 0x96cc64 extractLive
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = extractLive
        , spCard = cardA
        , spModelCells = cardCells
        }
      where
        savedFile = migratedV2 8 (Populated (name26 (slotNames !! 0)) extractPayload extractPayload : drop 1 recordsA)

    resetCase ok =
      Spec
        { spId = "n-reset-" ++ (if ok then "ok" else "cancel")
        , spProv =
            "contract: the submenu's reset row (index 3) raises its confirmation dialog on the submenu page, whose parent is the submenu and whose cursor is row 0; the dialog's "
              ++ (if ok then "OK runs the bank action on slot 0" else "CANCEL writes nothing")
              ++ "; both pop once back to the submenu"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 3 8 1
            , Dispatch (eDialog e) (if ok then 0 else 1) 8 2
            ] ++ [Draw (eSlot e) 0 1 0 | ok]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 1
            , claimU32 e (eCursor e) 3
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e 0x980000 (fileSlot cardA <> (if ok then fileSlot resetFile else emptySlots 1) <> emptySlots 1)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ] ++ [claimAt e (eOut26 e) (name26 "CUSTOM 1") | ok]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }
      where
        resetFile = migratedV2 8 (Unpopulated : drop 1 recordsA)

    renameFailCase =
      Spec
        { spId = "n-rename-fail"
        , spProv = "contract: a read-only rename completion reports RB_READ_ONLY, keeps the editor open on the confirm row, and preserves the displayed slot name, card files and live settings"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
            , Navigate 0 8 0
            , Navigate 1 8 1
            , Navigate 0x2a 8 2
            , Call (eStatusGetter e) 3
            , Draw (eSlot e) 0 1 0
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0xffffffff
            , claimU32 e (out 2) 1
            , claimU32 e (out 3) 2
            , claimAt e 0x146761e (u16le 0x1e)
            , claimU32 e (eFrame e) (eEditor e)
            , claimU32 e (eCursor e) 0x2a
            , claimAt e (eOut26 e) (name26 "ALPHA")
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells ++ [("rbm_write_comp", 8)]
        }

    renameSuccessCase =
      renameFailCase
        { spId = "n-rename-select"
        , spProv = "contract: modeled text entry supplies NOVEMBER through the opened editor; confirming persists only slot 0's name, returns to its submenu, and reopening SELECT CUSTOM SETTING displays the new name on CUSTOM 1, whose OK checkpoints the live settings' capture as the base, applies the unchanged payload, writes selection 1 and hands the event to the stock SET delegate"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 4
            , Navigate 0 8 0
            , Navigate 1 8 1
            , InputName "NOVEMBER"
            , Navigate 0x2a 8 2
            , Navigate 0 13 5
            , Navigate 0 13 6
            , Navigate (fromIntegral (eRootSelectRow e)) 8 7
            , Draw (eSelect e) (fromIntegral (customRow (BankSlot 0))) 1 0
            , Navigate (customRow (BankSlot 0)) 8 3
            ]
        , spClaims =
            [ claimU32 e (out 2) 0xffffffff
            , claimU32 e (out 3) 0x2a
            , claimU32 e (uiLog SetCalls) 1
            , claimU32 e (uiLog SetToken) (selectToken (customRow (BankSlot 0)))
            , claimAt e 0x980000 (fileSlot (checkpointFile 9 renamedRecords) <> fileSlot (selectionFile 10 (Custom (BankSlot 0)) renamedRecords) <> emptySlots 1)
            , claimAt e (eOut26 e) (name26 "NOVEMBER")
            , claimAt e 0x96cc64 (selectedLive (B.take 4 validPayload))
            , claimAt e 0x146761e (u16le 0)
            , claimU32 e (eFrame e) (eSelect e)
            , claimU32 e (eCursor e) (customRow (BankSlot 0))
            ] ++ consumerClaims
        , spModelCells = cardCells
        , spLive = extractLive
        }
      where
        renamedRecords = Populated (name26 "NOVEMBER") validPayload validPayload : drop 1 recordsA

    renameCancelCase =
      Spec
        { spId = "n-rename-cancel"
        , spProv = "contract: DISP/BACK cancels a changed editor name without a bank write; the submenu's name row and browse state are restored and the displayed slot name remains ALPHA"
        , spSteps =
            [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 3
            , Navigate 0 8 0
            , Navigate 1 8 1
            , InputName "NOVEMBER"
            , Navigate 0 13 2
            , Draw (eSlot e) 0 1 0
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0xffffffff
            , claimU32 e (out 2) 0xffffffff
            , claimAt e 0x146761e (u16le 0)
            , claimU32 e (eFrame e) (sub 0)
            , claimU32 e (eCursor e) 1
            , claimAt e (eOut26 e) (name26 "ALPHA")
            , claimAt e 0x980000 (fileSlot cardA <> emptySlots 2)
            , claimAt e 0x96cc64 (B.replicate 0x1c2 0)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        }

-- inputs and the case image ----------------------------------------------------------

newtype GuestAddress = GuestAddress Word32

guestWord :: GuestAddress -> Word32
guestWord (GuestAddress address) = address

data Inputs = Inputs
  { inKs, inMs, inUs :: Syms
  , inKbin, inKtext, inMdata :: B.ByteString
  , inFunctions :: [(Integer, Integer)]
  , inUiA, inUiB, inUiC :: B.ByteString
  , inId4 :: B.ByteString
  , inId5 :: B.ByteString
  , inDivide :: B.ByteString
  , inKernelBase :: Word32
  , inPlan :: [PlanWrite]
  , inFields :: [Field]
  , inTab :: B.ByteString
  , inCharsetBytes :: B.ByteString
  , inNameBase :: Word32
  , inHandler :: Word32
  , inEpochBump :: Word32
  , inPlanCellBytes :: [(Word32, B.ByteString)]
  , inMediaLo, inMediaHi :: Word32
  , inMediaImg :: B.ByteString
  , inLowcode :: B.ByteString
  , inLowcodeBase :: GuestAddress
  , inMenuBase :: Word32
  , inMenu :: B.ByteString
  }

nodeBytes :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
nodeBytes i t cls k fl par vals labs sel =
  B.concat [u32le i, u32le t, u32le cls, u32le k, u32le 0, u32le 0, u32le fl, u32le par, u32le vals, u32le labs, u32le sel]

cellsImage :: Inputs -> Env -> Spec -> B.ByteString
cellsImage in' e sp =
  overlay (B.replicate cellsSize 0) $
    [ (cellOffset 0x00951564, u32le 1)
    , (cellOffset 0x00956838, u32le 0x21)
    , (cellOffset (eCharsetCopy e), inCharsetBytes in')
    , (cellOffset (eStockNode e), nodeBytes 0x20 6 5 6 1 (eStockParent e) (eStockValues e) 0 0)
    , (cellOffset (eStockValues e), B.concat [u32le 0x7c, u32le (eStockChild1 e), u32le 0x7d, u32le (eStockChild2 e), u32le 0x7ffffffe, u32le 0])
    , (cellOffset (eStockChild1 e), nodeBytes 0x21 6 0x100 6 1 0xdeadbeef 0 0 0)
    , (cellOffset (eStockChild2 e), nodeBytes 0x22 6 0x100 6 1 0 0 0 0)
    , (cellOffset (eStockParent e), nodeBytes 0x23 6 5 6 1 0 0 0 0)
    , (cellOffset (eSteps e), stepBytes (spSteps sp))
    , (cellOffset (eHandler e), u32le (inHandler in'))
    , (cellOffset (eEpochBump e), u32le (inEpochBump in'))
    , (cellOffset (fromIntegral (addr (inUs in') "ui_name_base")), u32le (inNameBase in'))
    , (cellOffset (fromIntegral (addr (inUs in') "ui_refuse")), u32le (spRefuse sp))
    , (cellOffset (fromIntegral (addr (inUs in') "ui_set_result")), u32le (spSetResult sp))
    , (cellOffset (fromIntegral (addr (inUs in') "ui_confirm_result")), u32le (spConfirmResult sp))
    , (cellOffset (eFileB e), cardB)
    ]

spanLowImage :: Inputs -> Env -> Spec -> B.ByteString
spanLowImage in' e sp =
  overlay (B.replicate (0x980c60 - 0x96cc64) 0) $
    [ (0, spLive sp)
    , (0x970000 - 0x96cc64, inMdata in')
    , (0x970800 - 0x96cc64, fieldTable)
    , (fromIntegral (fieldCountAddr - 0x96cc64), u32le (fromIntegral fieldCount))
    , (0x980000 - 0x96cc64, spCard sp)
    ]
      ++ [ (fromIntegral (addr (inMs in') n - 0x96cc64), u32le v) | (n, v) <- spModelCells sp ]
  where
    fieldCount = length (inFields in')
    fieldTable = B.concat [B.concat [u32le (fProp f), u32le (fromIntegral (fOff f)), u32le (fromIntegral (fShift f)), u32le (fromIntegral (fBytes f)), u32le (fMask f)] | f <- inFields in']
    fieldCountAddr = fromIntegral (addr (inMs in') "rbm_field_count")

regionList :: Inputs -> Env -> Spec -> [(String, Word32, Int, Int, B.ByteString)]
regionList in' e sp = filter (\(n, _, _, _, _) -> inProfile (regionProfile e sp) (RegionId n)) $
  [ ("ktext", inKernelBase in', B.length ktext, 5, ktext)
  , ("kdata", dataBase, B.length dataMenu, 3, dataMenu)
  , ("lowcode", lowBase, B.length lowImage, 5, lowImage)
  , ("stock-divide", 0x0004d978, 244, 5, inDivide in')
  , ("stock-draw-tail", guestWord stockDrawTail, B.length stockDrawTailImage, 5, stockDrawTailImage)
  , ("span-low", 0x0096cc64, B.length spanLow, 3, spanLow)
  , ("media-state", 0x00910140, 0x200, 3, B.replicate 0x200 0)
  , ("tables-ptr", 0x0081b09c, 0x860, 3, inTab in')
  , ("mirror", 0x0089ec00, 0x200, 3, overlay (B.replicate 0x200 0) [(0x140, validPayload), (0x1c8, u32le 1)])
  , ("tables-edit", 0x00774f48, fromIntegral (tablesEditEnd - 0x774f48), 1,
      overlay (B.take (fromIntegral (tablesEditEnd - 0x774f48)) (B.drop (0x774f48 - 0x61c400) (inId4 in')))
        [(fromIntegral (pwAddress w - 0x774f48), pwNew w) | w <- inPlan in',
         pwRegion w == "id4_code", pwAddress w >= 0x774f48,
         toInteger (pwAddress w) + toInteger (B.length (pwNew w)) <= toInteger tablesEditEnd])
  , ("cells", cellsBase, B.length cells, 3, cells)
  , ("modal-cells", 0x00cd1ae4, 0xa00, 3, overlay (B.replicate 0xa00 0) [(fromIntegral (a - 0xcd1ae4), b) | (a, b) <- inPlanCellBytes in'])
  , ("modal-state", 0x0146761c, 0x162, 3, B.replicate 0x162 0)
  , ("draw-state", 0x015e46c0, 0x200, 3, B.replicate 0x200 0)
  , ("stock-draw-calls", 0x01112b2c, fromIntegral (stockDrawCallsEnd - 0x01112b2c), 5, stockDrawCallsImage)
  , ("ui-a", 0xc12aac, B.length (inUiA in'), 5, inUiA in')
  , ("ui-b", 0xc72e10, B.length (inUiB in'), 5, inUiB in')
  , ("ui-c", 0xd38d98, 0xd52684 - 0xd38d98, 5,
      overlay (B.replicate (0xd52684 - 0xd38d98) 0)
        [(0, inUiC in'), (0xd4d604 - 0xd38d98, B.take 20608 (B.drop (0xd4d604 - 0xbf1a00) (inId5 in')))])
  ]
  where
    ktext = inKtext in'
    textLen = B.length ktext
    dataLen = B.length (inKbin in') - textLen
    dataBase = inKernelBase in' + fromIntegral textLen
    dataMenu = compose dataBase (eMenuBase e + fromIntegral (B.length (eMenu e)))
      [(dataBase, B.drop textLen (inKbin in')), (eMenuBase e, eMenu e)]
    spanLow = spanLowImage in' e sp
    cells = cellsImage in' e sp
    -- The stock id4 prefix and the media model share one region; the zero gap
    -- between them holds no declared function.
    lowBase = guestWord (inLowcodeBase in')
    lowImage
      | lowBase + fromIntegral (B.length (inLowcode in')) > inMediaLo in' = error "the stock low code overlaps the media model"
      | otherwise = compose lowBase (inMediaHi in') [(lowBase, inLowcode in'), (inMediaLo in', inMediaImg in')]

-- the runner -------------------------------------------------------------------------

stubDefs :: [Value]
stubDefs =
  [ stub "0x00723cb8" [] [("r0", 3)]
  , stub "0x0071ddf0" [("r0", 0)] [("r0", 0)]
  , stub "0x0071f708" [] [("r0", 0x43)]
  , stub "0x0063c51c" [("r1", 0)] [("r0", 0)]
  , stub "0x00641438" [("r1", 0)] [("r0", 0)]
  , stub "0x00641334" [] [("r0", 0)]
  , stub "0x0063e810" [] [("r0", 0)]
  , stub "0x0063fb74" [] [("r0", 0)]
  , stub "0x00640e78" [] [("r0", 0)]
  , stub "0x0072b858" [] [("r0", 0)]
  -- The modeled notification configuration has every enable bit clear.
  , stub "0x00632950" [] [("r0", 0)]
  ]
  where
    stub :: String -> [(String, Word32)] -> [(String, Word32)] -> Value
    stub a ins outs = object ["address" .= a, "inputs" .= bindings ins, "outputs" .= bindings outs, "side_effects" .= ("registers-only" :: String)]
    bindings :: [(String, Word32)] -> Value
    bindings bs = object [fromString k .= hex (fromIntegral v) | (k, v) <- bs]

functionSpans :: Inputs -> Spec -> [(Integer, Integer)]
functionSpans in' sp = map validate (sortOn fst declared)
  where
    profile = regionProfile (envFrom in') sp
    omitted (a, z) =
      or [ a >= fromIntegral b && a + z <= fromIntegral b + fromIntegral s
         | (n, b, s) <- eRegions (envFrom in'), not (inProfile profile (RegionId n)) ]
    primary = nonOverlapping (sortOn fst (nub (map validate (filter (not . omitted) (inFunctions in')))))
    nonOverlapping spans
      | any (\((a, z), (b, _)) -> a + z > b) (zip spans (drop 1 spans)) = error "ELF function spans overlap"
      | otherwise = spans
    stock = [(0x61c3ac, 76) | guestWord (inLowcodeBase in') <= 0x61c3ac]
        ++ [(fromIntegral a, fromIntegral z) | (a, z) <- stockFns]
        ++ [ (fromIntegral (guestWord stockDrawTail), fromIntegral (B.length stockDrawTailCode))
           | profile /= ProcessingProfile ]
        ++ [ (fromIntegral a, fromIntegral z) | profile == PaneDrawProfile, (a, z) <- stockDrawCallFns ++ stockPaneCallFns ]
    stubs = [a | v <- stubDefs, Just a <- [parseMaybe (withObject "stub" (.: "address")) v >>= parseHexV]]
    declared = foldl add primary stock
    add spans span' = spans ++ uncovered span' (sortOn fst ([(a, a + z) | (a, z) <- spans] ++ [(a, a + 4) | a <- stubs]))
    uncovered (a, z) = go a (a + z)
    go lo hi _ | lo >= hi = []
    go lo hi [] = [(lo, hi - lo)]
    go lo hi ((a, b) : rest)
      | b <= lo = go lo hi rest
      | a >= hi = [(lo, hi - lo)]
      | otherwise = [(lo, a - lo) | lo < a] ++ go (max lo b) hi rest
    validate span'@(a, z)
      | a `mod` 4 /= 0 || z <= 0 || z `mod` 4 /= 0 = error ("unaligned function span: " ++ show span')
      | a <= exitAddr && exitAddr < a + z = error ("exit lies inside function span: " ++ show span')
      | not (any contains (regionList in' (envFrom in') sp)) = error ("function outside executable region: " ++ show span')
      | otherwise = span'
      where contains (_, base, size, permissions, _) =
              permissions == 5 && a >= fromIntegral base && a + z <= fromIntegral base + fromIntegral size
    exitAddr = addr (inUs in') "ui_exit_boundary"

caseJson :: Inputs -> Env -> Spec -> [(String, String)] -> Value
caseJson in' e sp digests =
  object
    [ "schema_version" .= (1 :: Int)
    , "id" .= spId sp
    , "origin" .= ("synthetic" :: String)
    , "provenance" .= ("Recipe-bank UI-behavior case (RECIPE-UI-BEHAVIOR-01); synthetic stock consumer in emulation/recipe-bank-ui-model.c: " ++ spProv sp)
    , "cpu" .= object ["architecture" .= ("arm" :: String), "model" .= ("cortex-r5f" :: String), "mode" .= ("arm" :: String), "endian" .= ("little" :: String)]
    , "artifacts" .= [object ["id" .= n, "path" .= (n ++ ".bin"), "size" .= B.length b, "sha256" .= d] | (n, b) <- arts, let d = fromMaybe "" (lookup n digests)]
    , "entry" .= hex (fromIntegral entry)
    , "exit" .= hex (fromIntegral exitAddr)
    , "registers" .=
        object
          ( ["lr" .= hex (fromIntegral exitAddr), "sp" .= hex (0x959fe0 :: Integer), "cpsr" .= hex 0x10]
              ++ [fromString ("r" ++ show i) .= hex (0 :: Integer) | i <- [0 :: Int .. 12]]
          )
    , "regions" .= map regJson regs
    , "functions" .= [object ["address" .= hex a, "size" .= z] | (a, z) <- functionSpans in' sp]
    , "stubs" .= stubDefs
    , "limits" .= object ["instructions" .= (4000000 :: Int), "wall_ms" .= (4000 :: Int)]
    , "expected" .=
        object
          [ "status" .= ("returned" :: String)
          , "registers" .= object ["r0" .= hex 0]
          , "memory" .= [object ["region" .= n, "offset" .= o, "hex" .= hexB b] | (n, o, b) <- spClaims sp]
          , "events" .= ([] :: [Value])
          ]
    , "exclusions" .=
        ( [ "ABI-private workspace differences are classified by RecipeBankUiCases, never pinned from execution; explicit public claims take precedence"
          , "stock ordinary reload workspace is mirror+0x10..0x127; the explicit C payload at mirror+0x140..0x167 remains compared"
          , "stub contracts remain enforced; incidental stub event order is not a behavior assertion"
          , "editor text input is modeled; rename persistence and SELECT CUSTOM SETTING execute the public menu actions"
          , "modal dispatch, transition, delegates and label drawing are modeled; property consumers execute stock instructions"
          , "the camera run and the real media device are outside this harness"
          ]
            :: [String]
        )
    ]
  where
    ktext = inKtext in'
    textLen = B.length ktext
    arts = [(n, b) | (n, _, _, _, b) <- regs, not (B.null b)]
    regs = regionList in' e sp
    entry = addr (inUs in') "ui_run"
    exitAddr = addr (inUs in') "ui_exit_boundary"
    regJson (n, a, s, p, b) =
      object
        [ "id" .= n
        , "address" .= hex (fromIntegral a)
        , "size" .= s
        , "permissions" .= permName p
        , "initial" .=
            ( if B.null b
                then object ["zero" .= True]
                else object ["artifact" .= n, "offset" .= (0 :: Int), "length" .= B.length b]
            )
        ]

permName :: Int -> String
permName 1 = "r"
permName 3 = "rw"
permName 5 = "rx"
permName _ = error "bad permission"

-- Public claims and effects outside the declared ABI workspaces are compared independently.
memMap :: Env -> Value -> M.Map String B.ByteString
memMap e r = M.fromList (fromMaybe [] (parseMaybe p =<< parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "memory"))) r))
  where
    p = withArray "mem" $ \a -> pure [(nameOf k, unhex v) | x <- toList a, Just (k, v) <- [row x]]
    row x = parseMaybe (withObject "r" $ \o -> (,) <$> (o .: "address" :: Parser String) <*> o .: "bytes") x
    nameOf a = case [n | (n, b, _) <- eRegions e, fromIntegral b == hexI a] of
      (n : _) -> n
      [] -> a

runnerStatus :: Value -> String
runnerStatus r = fromMaybe "??" (parseMaybe (withObject "r" (.: "status")) r)

reportReason :: Value -> String
reportReason r = fromMaybe "" (parseMaybe (withObject "r" (.: "reason")) r)

reportRunnerStatus :: Value -> String
reportRunnerStatus r = fromMaybe "" (parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (.: "status"))) r)

runnerMismatches :: Value -> [String]
runnerMismatches r = fromMaybe [] (parseMaybe (withArray "m" (pure . mapMaybe strV . toList)) =<< parseMaybe (withObject "r" (.: "mismatches")) r)
  where
    strV (String t) = Just (T.unpack t)
    strV _ = Nothing

data DifferenceKind = PublicClaimDifference | NonWorkspaceDifference deriving (Eq)

data MemoryDifference = MemoryDifference
  { differenceStart :: Int
  , differenceEnd :: Int
  , differenceKind :: DifferenceKind
  }

memoryProblems :: Inputs -> Env -> Spec -> M.Map String B.ByteString -> [String]
memoryProblems in' e sp observed =
  ["runner memory inventory differs" | sort (M.keys observed) /= sort [n | (n, _, _, _, _) <- regs]]
  ++ ["stock conversion flag is not Boolean" | Just mirror <- [M.lookup "mirror" observed], B.length mirror == 0x200, readU32 mirror 0x1c8 > 1]
  ++ ["stock processing gate is not Boolean" | Just cells <- [M.lookup "cells" observed], B.length cells == cellsSize, readU32 cells (cellOffset 0x00956858) > 1]
  ++ ["stock processing mirror is not Boolean" | Just cells <- [M.lookup "cells" observed], B.length cells == cellsSize, readU32 cells (cellOffset 0x00956c40) > 1]
  ++ ["stock processing mirror differs from gate" | Just cells <- [M.lookup "cells" observed], B.length cells == cellsSize, readU32 cells (cellOffset 0x00956c40) /= readU32 cells (cellOffset 0x00956858)]
  ++ concatMap compareRegion regs
  where
    regs = regionList in' e sp
    workspaces = caseWorkspaces in' e sp
    claimed n i = any (\(n', o, b) -> n == n' && o <= i && i < o + B.length b) (spClaims sp)
    checkedSpans n size = outside 0 (sortOn fst [(workspaceStart w, workspaceEnd w) | w <- workspaces, let RegionId region = workspaceRegion w, region == n])
      ++ [(o, o + B.length b) | (n', o, b) <- spClaims sp, n == n']
      where
        outside cur [] = [(cur, size) | cur < size]
        outside cur ((lo, hi) : rest) = [(cur, lo) | cur < lo] ++ outside (max cur hi) rest
    compareRegion (n, base, size, _, initial) = case M.lookup n observed of
      Nothing -> ["runner omitted region " ++ n]
      Just actual
        | B.length actual /= size -> ["runner returned wrong size for region " ++ n]
        | otherwise ->
            [ n ++ " [" ++ hex (fromIntegral base + fromIntegral (differenceStart delta))
                ++ "," ++ hex (fromIntegral base + fromIntegral (differenceEnd delta)) ++ "): "
                ++ (case differenceKind delta of PublicClaimDifference -> "public claim differs"; NonWorkspaceDifference -> "unexpected non-workspace effect")
            | delta <- differences n
                [word | word : _ <- group (sort
                  [lo + offset | (lo, hi) <- checkedSpans n size
                    , let wanted = B.take (hi - lo) (B.drop lo expected)
                    , let got = B.take (hi - lo) (B.drop lo actual)
                    , wanted /= got
                    , (offset, (before, after)) <- zip [0 ..] (B.zip wanted got), before /= after])]
            ]
        where
          start = if B.null initial then B.replicate size 0 else initial
          expected = overlay start [(o, b) | (n', o, b) <- spClaims sp, n == n']
    differences n = foldr mergeDifference []
      where
        kind i = if claimed n i then PublicClaimDifference else NonWorkspaceDifference
        mergeDifference i (delta : rest)
          | differenceStart delta == i + 1 && differenceKind delta == kind i =
              delta {differenceStart = i} : rest
        mergeDifference i rest = MemoryDifference i (i + 1) (kind i) : rest

parseHexV :: Value -> Maybe Integer
parseHexV v = case str v of
  '0' : 'x' : rest -> Just (foldl (\acc c -> acc * 16 + fromIntegral (digitVal c)) 0 rest)
  _ -> Nothing
 where
  digitVal c
    | c >= '0' && c <= '9' = fromEnum c - fromEnum '0'
    | c >= 'a' && c <= 'f' = fromEnum c - fromEnum 'a' + 10
    | otherwise = 0

-- loading and running -----------------------------------------------------------------

compose :: Word32 -> Word32 -> [(Word32, B.ByteString)] -> B.ByteString
compose lo hi ps =
  foldl
    (\acc (a, p) -> B.take (fromIntegral (a - lo)) acc <> p <> B.drop (fromIntegral (a - lo) + B.length p) acc)
    (B.replicate (fromIntegral (hi - lo)) 0)
    ps

loadInputs :: FilePath -> FilePath -> FilePath -> Integer -> FilePath -> FilePath -> String -> Integer -> IO Inputs
loadInputs build planPath id4Path id4Base id5Path fieldsPath nameRecordArg kernelBase = do
  ks <- parseSyms (build </> "kernel.syms")
  unless (addr ks "__rb_image_start" == kernelBase) (fail "--rb-base differs from linked __rb_image_start")
  ms <- parseSyms (build </> "model.syms")
  us <- parseSyms (build </> "ui-model.syms")
  unless (hasSym us "ui_run") (error "ui-model.syms has no ui_run: build the UI model first")
  unless (hasSym us "ui_exit_boundary" && sizeOf us "ui_exit_boundary" == 0) $
    fail "the UI exit boundary must be a zero-size linker symbol"
  kernelFunctions <- parseFunctions KernelUiInventory (build </> "kernel.symbols")
  mediaFunctions <- parseFunctions ForeignInventory (build </> "model.symbols")
  uiFunctions <- parseFunctions ForeignInventory (build </> "ui-model.symbols")
  kbin <- B.readFile (build </> "kernel.bin")
  ktext <- B.readFile (build </> "kernel-text.bin")
  mdata <- B.readFile (build </> "model-data.bin")
  uiA <- B.readFile (build </> "ui-a.bin")
  uiB <- B.readFile (build </> "ui-b.bin")
  uiC <- B.readFile (build </> "ui-c.bin")
  id4Raw <- B.readFile id4Path
  loader <- B.readFile "private-data/analysis/x-m1-1.50/decode/loader-40000.bin"
  let divide = B.take 244 (B.drop (0x4d978 - 0x40000) loader)
  unless (B.length divide == 244) (fail "the retained 1.50 loader lacks the complete unsigned-divide function")
  unless (id4Base <= 0x61c400 && id4Base >= 0x61c1fc) (fail "--id4-base outside the 1.50 prefix range")
  let id4 = B.drop (fromIntegral (0x61c400 - id4Base)) id4Raw
  id5 <- B.readFile id5Path
  plan <- parsePlan planPath
  fieldsRaw <- B.readFile fieldsPath
  let jsonFields = parseFields fieldsRaw
      halfProps = [0x1cb, 0x1cc] :: [Word32]
      fields = filter (\f -> fProp f `notElem` halfProps) jsonFields ++ [Field 0x1cb 0 0 2 0xffff, Field 0x1cc 2 0 2 0xffff]
      (menuBase, menuImg0) = composeMenu (menuWrites plan)
      nameBasePlan = case [pwAddress w | w <- plan, pwRegion w == "name_records"] of
        [a] -> a
        as -> error ("expected one name-record write, got " ++ show (length as))
      nameBase = fromIntegral (hexI nameRecordArg)
      charsetAddr = 0x00957600 :: Word32
      charset = B.take 0x160 (B.drop (0x78bac8 - 0x61c400) id4)
      editorNode = nodeByRole plan "bank name editor node header"
      menuImg = overlay menuImg0 [(fromIntegral (editorNode - menuBase) + 0x20, u32le charsetAddr)]
      handler = fromIntegral (addr ks "rb_bank_interpose")
      epochBump = fromIntegral (addr ks "rb_media_epoch_bump")
      planCells =
        [(pwAddress w, pwNew w) | w <- plan, pwRegion w == "image5",
         pwAddress w >= 0x00cd1ae4,
         fromIntegral (pwAddress w) + B.length (pwNew w) <= (0x00cd24e4 :: Int)]
      mediaSpans = ["rbm_low_close", "rbm_read", "rbm_write", "rbm_close", "rbm_open", "rbm_stat", "rbm_index_free", "rbm_mapping_delete", "rbm_resolve", "rbm_tag_release"]
      mediaLo = fromIntegral (minimum [addr ms n | n <- mediaSpans])
      mediaHi = max 0x0072b3b4 (fromIntegral (addr ms "helpers" + sizeOf ms "helpers"))
      lowcode =
        overlay (B.take (fromIntegral (0x646c9c - id4Base)) id4Raw)
          ([(fromIntegral (toInteger (pwAddress w) - id4Base), pwNew w) | w <- plan,
            pwRegion w == "id4_code", toInteger (pwAddress w) >= id4Base,
            toInteger (pwAddress w) + toInteger (B.length (pwNew w)) <= 0x646c9c]
           ++ [(fromIntegral (0x61d9ec - id4Base), bxlr), (fromIntegral (0x61d8dc - id4Base), bxlr)])
      bxlr = B.pack [0x1e, 0xff, 0x2f, 0xe1]
  when (nameBasePlan /= nameBase) $
    error
      ( "the plan's name-record base "
          ++ hex (fromIntegral nameBasePlan)
          ++ " differs from --name-record "
          ++ hex (fromIntegral nameBase)
          ++ ": the blob was compiled with a different RB_NAME_RECORD"
      )
  chunks <- forM mediaSpans $ \n -> (,) (fromIntegral (addr ms n)) <$> B.readFile (build </> ("model-" ++ n ++ ".bin"))
  helpers <- B.readFile (build </> "model-helpers.bin")
  pure
    Inputs
      { inKs = ks
      , inMs = ms
      , inUs = us
      , inKbin = kbin
      , inKtext = ktext
      , inMdata = mdata
      , inUiA = uiA
      , inUiB = uiB
      , inUiC = uiC
      , inId4 = id4
      , inId5 = id5
      , inDivide = divide
      , inKernelBase = fromIntegral kernelBase
      , inPlan = plan
      , inFields = fields
      , inFunctions = kernelFunctions
          ++ [(a, z) | (a, z) <- mediaFunctions, a >= fromIntegral mediaLo, a + z <= fromIntegral mediaHi]
          ++ uiFunctions
      , inTab = B.take 0x860 (B.drop (0x81b09c - 0x61c400) id4)
      , inCharsetBytes = charset
      , inNameBase = nameBase
      , inHandler = handler
      , inEpochBump = epochBump
      , inPlanCellBytes = planCells
      , inMediaLo = mediaLo
      , inMediaHi = mediaHi
      , inMediaImg = compose mediaLo mediaHi (chunks ++
          [(fromIntegral (addr ms "helpers"), helpers), (0x72b3a4, B.take 16 (B.drop (0x72b3a4 - 0x61c400) id4))])
      , inLowcode = lowcode
      , inLowcodeBase = GuestAddress (fromIntegral id4Base)
      , inMenuBase = menuBase
      , inMenu = menuImg
      }

envFrom :: Inputs -> Env
envFrom in' =
  Env
    { eMenuBase = inMenuBase in'
    , eMenu = inMenu in'
    , eSlot = nodeByRole plan "slot list node header"
    , eSelect = nodeByRole plan "select list node header"
    , eSubs = nodesByRole plan "sub-list node header"
    , eEditScreen = nodeByRole plan "edit screen node header"
    , eDialog = nodeByRole plan "dialog node header"
    , eFields = nodesByRole plan "field-list node header"
    , eEditor = nodeByRole plan "bank name editor node header"
    , eNameBase = inNameBase in'
    , eStatusGetter = fromIntegral (addr (inKs in') "rb_status_last")
    , eCharsetCopy = 0x00957600
    , eRootEditRow = rootEditRow
    , eRootSelectRow = rootSelectRow
    , eRegions = regionBases in'
    , eOut = fromIntegral (addr us "ui_out")
    , eUiLog = fromIntegral (addr us "ui_log")
    , eOut26 = fromIntegral (addr us "ui_out26")
    , eFileB = fromIntegral (addr us "ui_file_b")
    , eSteps = fromIntegral (addr us "ui_steps")
    , eFrame = 0x009571d0
    , eCursor = 0x009571d8
    , eCurrentRow = 0x0095732c
    , eRootNode = 0x0078ea98
    , eStockNode = 0x00957440
    , eStockValues = 0x00957480
    , eStockChild1 = 0x009574c0
    , eStockChild2 = 0x00957500
    , eStockParent = 0x00957540
    , eHandler = fromIntegral (addr us "ui_handler")
    , eEpochBump = fromIntegral (addr us "ui_epoch_bump")
    , eRefuse = fromIntegral (addr us "ui_refuse")
    , eDrawRoot = GuestAddress (fromIntegral (addr (inKs in') "rb_bank_draw_root"))
    , eFieldGetter = 0x0061c93c
    , eWriteComp = fromIntegral (addr (inMs in') "rbm_write_comp")
    , eKernelBase = inKernelBase in'
    , eKernel = inKbin in'
    }
  where
    plan = inPlan in'
    us = inUs in'
    parentBytes = case [pwNew w | w <- plan, pwRole w == "parent values array with the two items"] of
      [bytes] -> bytes
      _ -> error "the parent values array is not unique"
    slotNode = nodeByRole plan "slot list node header"
    rows =
      [ ( readU32 parentBytes (8 * i)
        , readU32 parentBytes (8 * i + 4)
        )
      | i <- [0 .. B.length parentBytes `div` 8 - 1]
      ]
    rootEditRow = fromMaybe (error "the root row 0x7d was not found in the emitted parent values") (findIndex (\(t, c) -> t == 0x7d && c == slotNode) rows)
    rootSelectRow = fromMaybe (error "the root row 0x7e was not found in the emitted parent values") (findIndex (\(t, c) -> t == 0x7e && c == nodeByRole plan "select list node header") rows)

regionBases :: Inputs -> [(String, Word32, Int)]
regionBases in' =
  [ ("ktext", inKernelBase in', B.length (inKtext in'))
  , ("kdata", inKernelBase in' + fromIntegral textLen,
      fromIntegral (inMenuBase in' - inKernelBase in') - textLen + B.length (inMenu in'))
  , ("lowcode", guestWord (inLowcodeBase in'), fromIntegral (inMediaHi in' - guestWord (inLowcodeBase in')))
  , ("stock-divide", 0x0004d978, 244)
  , ("stock-draw-tail", guestWord stockDrawTail, B.length stockDrawTailImage)
  , ("span-low", 0x0096cc64, 0x980c60 - 0x96cc64)
  , ("media-state", 0x00910140, 0x200)
  , ("tables-ptr", 0x0081b09c, 0x860)
  , ("mirror", 0x0089ec00, 0x200)
  , ("tables-edit", 0x00774f48, fromIntegral (tablesEditEnd - 0x774f48))
  , ("cells", cellsBase, cellsSize)
  , ("modal-cells", 0x00cd1ae4, 0xa00)
  , ("modal-state", 0x0146761c, 0x162)
  , ("draw-state", 0x015e46c0, 0x200)
  , ("stock-draw-calls", 0x01112b2c, fromIntegral (stockDrawCallsEnd - 0x01112b2c))
  , ("ui-a", 0xc12aac, 0xc12c00 - 0xc12aac)
  , ("ui-b", 0xc72e10, 0xc76800 - 0xc72e10)
  , ("ui-c", 0xd38d98, 0xd52684 - 0xd38d98)
  ]
  where
    textLen = B.length (inKtext in')
    dataLen = B.length (inKbin in') - textLen

runSpec :: FilePath -> FilePath -> Env -> Inputs -> Spec -> IO Value
runSpec runner out e in' sp = do
  let dir = out </> spId sp
      regs = regionList in' e sp
      workspaces = caseWorkspaces in' e sp
  createDirectoryIfMissing True dir
  unless (map (\(n, a, _, _, _) -> (n, a)) regs == [(n, a) | (n, a, _) <- eRegions e, inProfile (regionProfile e sp) (RegionId n)]) $
    error ("the region table and the case image disagree in " ++ spId sp)
  forM_ workspaces $ \w -> do
    let RegionId region = workspaceRegion w
    unless (any (\(n, _, size, permissions, _) -> n == region && permissions == 3 && workspaceStart w >= 0 && workspaceStart w < workspaceEnd w && workspaceEnd w <= size) regs) $
      error ("invalid ABI workspace in " ++ spId sp)
  let arts = [(n, b) | (n, _, _, _, b) <- regs, not (B.null b)]
  forM_ arts $ \(n, b) -> B.writeFile (dir </> (n ++ ".bin")) b
  let digests = [(n, sha256Hex (sha256 b)) | (n, b) <- arts]
  BL.writeFile (dir </> "case.json") (encode (caseJson in' e sp digests))
  (_, report) <- runCase runner (dir </> "case.json")
  BL.writeFile (dir </> "native-result.json") (encode report)
  let nativeStatus = runnerStatus report
      rstat = reportRunnerStatus report
      nativeProblems = runnerMismatches report
      classified problem = problem == "stub events differ" || any (\(n, _, _, _, _) -> problem == "memory " ++ n ++ ": differs from exact allowed final bytes") regs
      publicProblems = memoryProblems in' e sp (memMap e report)
      problems = filter (not . classified) nativeProblems ++ publicProblems
      status
        | nativeStatus `notElem` ["passed", "failed"] = nativeStatus
        | rstat /= "returned" || not (null problems) = "failed"
        | otherwise = "passed"
      workspaceJson w = let RegionId region = workspaceRegion w in object
        ["kind" .= show (workspaceKind w), "region" .= region, "offset" .= workspaceStart w, "size" .= (workspaceEnd w - workspaceStart w)]
  pure (object
    [ "id" .= spId sp
    , "status" .= status
    , "mismatches" .= problems
    , "reason" .= reportReason report
    , "runner_status" .= rstat
    , "native_case_status" .= nativeStatus
    , "native_case_mismatches" .= nativeProblems
    , "comparison" .= ("One validated execution; public claims and every non-workspace byte are checked. NativeCase whole-memory differences within declared ABI workspaces and incidental stub event order are not behavior assertions." :: String)
    , "abi_workspaces" .= map workspaceJson workspaces
    ])

findingsText :: [Spec] -> [Value] -> String
findingsText specs results =
  unlines
    ( [ "recipe-bank UI-behavior cases: findings"
      , ""
      , "The modal dispatch, navigation, delegates and drawer are modeled ABI boundaries."
      , "The property getter, setter, commit dispatcher and consumer-cell writes execute"
      , "stock 1.50 instructions. Two ordinary-block copy wrappers are bx lr substitutions."
      , "The notification configuration query models an all-disabled bitset."
      , "The bank's draw-root hook runs; the stock drawer behind 0x01158160 is a modeled"
      , "immediate return, so markers are checked through the stock frame snapshot"
      , "accessors FUN_0061ca2c/FUN_0061d1a4/FUN_0061d244/FUN_0061d294, not through drawn pixels."
      , "Stale current-row and entry-cursor cells are written as native preconditions."
      , "Draw-root cases map the modeled tail in place of the ui-b stock delegates,"
      , "which those cases never enter; a fetch there faults. The one draw-root case"
      , "that selects through SELECT CUSTOM SETTING maps both."
      , ""
      , "Case status:"
      ]
        ++ [ "  " ++ spId sp ++ ": " ++ fromMaybe "?" (parseMaybe (withObject "r" (.: "status")) r)
           | (sp, r) <- zip specs results
           ]
        ++ [ ""
           , "Each case executes once with a closed function inventory and enforced stub contracts."
           , "Explicit public claims and every byte outside the declared ABI workspaces are checked."
           , "No private final-memory bytes or incidental stub-event sequence are pinned from execution."
           , "Text entry is modeled; the name write and the subsequent selection use public menu actions."
           ]
    )

main :: IO ()
main = do
  args <- getArgs
  let opt n = lookup n [(drop 2 a, b) | (a, b) <- pair args]
      pair (a : b : r) = (a, b) : pair r
      pair _ = []
  build <- maybe (fail "--build") pure (opt "build")
  out <- maybe (fail "--out") pure (opt "out")
  runner <- maybe (fail "--runner") pure (opt "runner")
  planPath <- maybe (fail "--plan") pure (opt "plan")
  id4Path <- maybe (fail "--id4") pure (opt "id4")
  id5Path <- maybe (fail "--id5") pure (opt "id5")
  let id4Base = hexI (fromMaybe "0x0061c400" (opt "id4-base"))
      kernelBase = hexI (fromMaybe "0x00be1f00" (opt "rb-base"))
  fieldsPath <- maybe (fail "--fields") pure (opt "fields")
  nameRecordArg <- maybe (fail "--name-record") pure (opt "name-record")
  jobs <- readJobs (opt "jobs")
  createDirectoryIfMissing True out
  in' <- loadInputs build planPath id4Path id4Base id5Path fieldsPath nameRecordArg kernelBase
  let e = envFrom in'
      specs = specSet e (inEpochBump in')
  results <- mapBounded jobs (\sp -> runSpec runner out e in' sp) specs
  BL.writeFile (out </> "results.json") (encode (object ["cases" .= results]))
  writeFile (out </> "findings.txt") (findingsText specs results)
  putStrLn ("ran " ++ show (length specs) ++ " cases")
  unless (all ((== Just ("passed" :: String)) . parseMaybe (withObject "r" (.: "status"))) results) exitFailure
