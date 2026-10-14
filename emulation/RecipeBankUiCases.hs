{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
-- Recipe-bank UI-behavior cases (RECIPE-UI-BEHAVIOR-01). Each case drives the
-- bank's menu modules through the synthetic stock consumer in
-- emulation/recipe-bank-ui-model.c: the modal-75 cell dispatch, the 0xe/0xf
-- transitions, the stock delegates and the label-record read the drawer uses.
-- The case image is composed from the builder's plan (the emitted menu nodes,
-- values arrays, label records and name records), the linked six-module blob,
-- the storage model's media spans and id4's conversion tables and charset
-- rows. Behavior expectations are derived here from the plan, the card fixture
-- and the storage contract before execution; the residue of the regions with
-- no behavior claim is pinned only to satisfy the runner's exact-final-memory
-- contract.
module Main where

import Control.Monad
import Data.Aeson hiding (encodeFile)
import Data.Aeson.Types (Parser, parseEither, parseMaybe)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Data.Bits
import Data.Foldable (toList)
import Data.List
import Data.Maybe
import Data.String (fromString)
import Data.Word
import Emulation.NativeCase (runCase)
import FujiAF.Host (sha256, str)
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

-- bank file codec, built independently of menu/recipe_bank_storage.c
checksum :: B.ByteString -> Word32
checksum f = complement (sum [fromIntegral (complement (if i >= 0x18 && i < 0x1c then 0 else B.index f i) :: Word8) | i <- [0 .. 511]])
  where complement w = maxBound - w

encodeFile :: Word32 -> Word16 -> [(B.ByteString, B.ByteString)] -> B.ByteString
encodeFile gen mask slots = putU32 0x18 ck file0
  where
    hdr = B.concat [B.pack (map (fromIntegral . fromEnum) "FJRB"), u16le 1, u16le 0x20, u16le 150, u16le 7, u16le 12, u16le 0x28, u32le gen, u32le 462, u32le 0, u16le mask, u16le 0]
    payloads = B.concat [maybe (B.replicate 0x28 0) snd (at i) | i <- [0 .. 6]]
    names = B.concat [maybe (B.replicate 26 0) fst (at i) | i <- [0 .. 6]]
    at i = if i < length slots then Just (slots !! i) else Nothing
    body = payloads <> names <> B.replicate 18 0
    file0 = hdr <> body
    ck = checksum file0
    putU32 o v b = B.take o b <> u32le v <> B.drop (o + 4) b

name26 :: String -> B.ByteString
name26 s = B.pack (take 26 (concatMap (\c -> [fromIntegral (fromEnum c), 0]) (take 12 s) ++ repeat 0))

fileSlot :: B.ByteString -> B.ByteString
fileSlot f = B.take 0x220 (f <> B.replicate 0x220 0)

cardWith3 :: B.ByteString -> B.ByteString -> B.ByteString
cardWith3 a c = fileSlot a <> B.replicate 0x220 0 <> fileSlot c

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

tabImage :: B.ByteString -> B.ByteString
tabImage id4 = B.take 0xc00 (B.drop (fromIntegral (0x81b09c - 0x61c400)) id4) `rewrite` tabAt `append` tabBlob
  where
    (tabBlob, tabAt) = loadTables id4
    loadTables img = (B.concat (map snd pieces), M.fromList (zip (map fst pieces) (snd (foldl step (0x81b09c + 0xc00, []) (map snd pieces)))))
      where
        base = 0x61c400 :: Integer
        u32at a = readU32 img (fromIntegral (a - base))
        ptr p = fromIntegral (readU32 (B.take 0xc00 (B.drop (fromIntegral (0x81b09c - base)) img)) p)
        entries = [(fromIntegral p `div` 4, ptr p) | p <- [0, 4 .. 0xbfc], ptr p /= 0]
        nrows t = length (take 64 (takeWhile (\i -> u32at (t + 12 * i) /= 0xffff) [0 :: Integer ..]))
        pieces = [(pr, B.take (fromIntegral (12 * n)) (B.drop (fromIntegral (t - base)) img)) | (pr, t) <- entries, let n = nrows t]
        step (a, acc) b = (a + fromIntegral (B.length b), acc ++ [fromIntegral a])
    rewrite blk at = B.concat [if ptr == 0 then B.take 4 (B.drop p blk) else u32le (fromIntegral (M.findWithDefault 0 (fromIntegral p `div` 4) at)) | p <- [0, 4 .. 0xbfc], let ptr = readU32 blk p]
    append a b = a <> b

tableCode :: B.ByteString -> Word32 -> Int -> Word32
tableCode id4 a row = readU32 id4 (fromIntegral (a - 0x61c400) + 12 * row)

-- steps ------------------------------------------------------------------------------

data Step
  = Dispatch Word32 Word32 Word32 Int
  | Draw Word32 Int Int Int
  | WriteWord Word32 Word32
  | WriteHalf Word32 Word32
  | Copy Word32 Word32 Word32
  | Call Word32 Int
  | Halt

stepWords :: Step -> [Word32]
stepWords (Dispatch frame cursor event out) = [0, frame, cursor, event, fromIntegral out]
stepWords (Draw page row field out) = [1, page, fromIntegral row, fromIntegral field, fromIntegral out]
stepWords (WriteWord a v) = [2, a, v, 0, 0]
stepWords (WriteHalf a v) = [3, a, v, 0, 0]
stepWords (Copy dst src n) = [4, dst, src, n, 0]
stepWords (Call a out) = [5, a, 0, 0, fromIntegral out]
stepWords Halt = [6, 0, 0, 0, 0]

stepBytes :: [Step] -> B.ByteString
stepBytes steps = B.concat (map (B.concat . map u32le . stepWords) (steps ++ [Halt]))

-- the case environment ---------------------------------------------------------------

data Env = Env
  { eMenuBase :: Word32
  , eMenu :: B.ByteString
  , eSlot :: Word32
  , eRecall :: Word32
  , eSubs :: [Word32]
  , eEditScreen :: Word32
  , eDialog :: Word32
  , eFields :: [Word32]
  , eEditor :: Word32
  , eRootValues :: Word32
  , eRootLabels :: Word32
  , eNameBase :: Word32
  , eCharsetCopy :: Word32
  , eRootEditRow :: Int
  , eRegions :: [(String, Word32, Int)]
  , eOut :: Word32
  , eLog :: Word32
  , eOut26 :: Word32
  , eFileB :: Word32
  , eSteps :: Word32
  , eFrame :: Word32
  , eCursor :: Word32
  , eRootNode :: Word32
  , eStockNode :: Word32
  , eStockValues :: Word32
  , eStockChild1 :: Word32
  , eStockChild2 :: Word32
  , eStockParent :: Word32
  , eHandler :: Word32
  , eEpochBump :: Word32
  , eRefuse :: Word32
  , eStagePayload :: Word32
  , eFieldsMap :: [Field]
  }

wordAt :: Env -> Word32 -> Int -> Word32
wordAt e a off =
  let o = fromIntegral (a - eMenuBase e) + off
   in if o < 0 || o + 4 > B.length (eMenu e)
        then error ("node word outside the menu image: " ++ hex (fromIntegral a) ++ " +" ++ show off)
        else readU32 (eMenu e) o

regionOf :: Env -> Word32 -> (String, Int)
regionOf e a = case [(n, fromIntegral (a - b)) | (n, b, s) <- eRegions e, a >= b, fromIntegral (a - b) < s] of
  [(n, o)] -> (n, o)
  [] -> error ("address outside the case regions: " ++ hex (fromIntegral a))
  rs -> error ("address in several regions: " ++ hex (fromIntegral a) ++ " " ++ show (map fst rs))

claimAt :: Env -> Word32 -> B.ByteString -> (String, Int, B.ByteString)
claimAt e a b = let (n, o) = regionOf e a in (n, o, b)

claimU32 :: Env -> Word32 -> Word32 -> (String, Int, B.ByteString)
claimU32 e a v = claimAt e a (u32le v)

-- fixtures ---------------------------------------------------------------------------

slotNames :: [String]
slotNames = ["ALPHA", "BRAVO", "CHARLIE", "DELTA", "ECHO", "FOXTROT", "GOLF"]

swapNames :: [String]
swapNames = ["SWAP-A", "SWAP-B", "SWAP-C", "SWAP-D", "SWAP-E", "SWAP-F", "SWAP-G"]

newCardNames :: [String]
newCardNames = ["NEW-A", "NEW-B", "NEW-C", "NEW-D", "NEW-E", "NEW-F", "NEW-G"]

staleName :: B.ByteString
staleName = name26 "STALE00"

payloadFor :: Word32 -> B.ByteString
payloadFor k = putHalf 0x00 (0x1234 + k) (putHalf 0x02 (0x5678 + k) validPayload)
  where
    putHalf o v b = B.take o b <> u16le (fromIntegral v) <> B.drop (o + 2) b

validPayload :: B.ByteString
validPayload = B.pack [0x34, 0x12, 0x78, 0x56, 0x00, 0x40, 0x00, 0x80, 0x60, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x03, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00]

slotsA :: [(B.ByteString, B.ByteString)]
slotsA = [(name26 n, payloadFor k) | (k, n) <- zip [0 ..] slotNames]

cardA :: B.ByteString
cardA = encodeFile 7 0x7f slotsA

cardB :: B.ByteString
cardB = encodeFile 9 0x7f [(name26 n, payloadFor k) | (k, n) <- zip [0 ..] swapNames]

cardC :: B.ByteString
cardC = encodeFile 9 0x7f [(name26 n, payloadFor k) | (k, n) <- zip [0 ..] newCardNames]

extractLive :: [Field] -> B.ByteString
extractLive fields = foldl (\b f -> placeCode b f (fromMaybe 0 (lookup (fProp f) extractCodes))) (B.replicate 0x1c2 0) fields

extractCodes :: [(Word32, Word32)]
extractCodes = [(0x104, 1), (0x110, 3), (0x111, 4), (0x10e, 4), (0x10f, 3), (0xef, 2), (0xed, 3), (0x141, 0xb), (0xfe, 5)]

extractPayload :: B.ByteString
extractPayload = B.pack [0, 0, 0, 0, 0, 0x40, 0, 0x80, 0x60, 0x80, 0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0, 0, 0, 0, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0x0b, 0x03, 0, 0, 0x05, 0, 0, 0]

placeCode :: B.ByteString -> Field -> Word32 -> B.ByteString
placeCode b f c
  | fBytes f == 2 =
      B.take off b <> u16le (fromIntegral v16) <> B.drop (off + 2) b
  | otherwise = B.take off b <> B.pack [fromIntegral byte] <> B.drop (off + 1) b
  where
    off = fOff f
    old = B.index b off
    cur :: Word32
    cur = fromIntegral old .|. (fromIntegral (B.index b (off + 1)) `shiftL` 8)
    v16 :: Word32
    v16 = (cur .&. complement (fMask f `shiftL` fShift f)) .|. ((c `shiftL` fShift f) .&. fMask f)
    byte = fromIntegral ((fromIntegral old .&. complement (fMask f)) .|. ((c `shiftL` fShift f) .&. fMask f))

-- case specs -------------------------------------------------------------------------

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
  , spFunctions :: [String]
  }

-- No-claim residue: only the byte ranges whose contents are operational state. Every
-- other change in a case must appear as an explicit behavior claim, so a wrong claim
-- fails instead of being pinned.
pinnedBytes :: Inputs -> [(String, Int, Int)]
pinnedBytes in' =
  [ ("cells", 0x7e00, 0x8800)
  , ("cells", 0x9000, 0xa000)
  , ("media-state", 0, 0x200)
  , ("modal-state", 0, 0x162)
  , ("tables-ptr", 0x89edc0 - 0x81b09c, 0x89edd0 - 0x81b09c)
  , ("kdata", 0, B.length (inKbin in') - B.length (inKtext in'))
  , ("span-low", 0x970000 - 0x96cc64, 0x970800 - 0x96cc64)
  ]

-- the stock conversion path the kernel's code check reaches: declared as one range so
-- its thunk and table reads execute inside the mapped lowcode span
-- the arm.S trampolines every bank flow can branch to (tail branches need exact entries)
alwaysFns :: [String]
alwaysFns =
  [ "rb_call_stage_open"
  , "rb_call_transition"
  , "rb_call_redraw"
  , "rb_call_set_dispatch"
  , "rb_call_editor_confirm"
  , "rb_call_postlude"
  , "rb_call_leave_edge"
  ]

stockFns :: [(Word32, Int)]
stockFns =
  [ (0x0061c400, 0x14dc)
  , (0x0061d900, 0x810)
  , (0x00630000, 0x4000)
  , (0x00644000, 0x2000)
  ]

specSet :: Env -> B.ByteString -> Word32 -> [Spec]
specSet e id4 epochBump = slotCases ++ otherCases
  where
    sub k = eSubs e !! k
    field k = eFields e !! k
    out o = eOut e + 4 * fromIntegral o
    cardCells = [("rbm_a_present", 1), ("rbm_a_size", 512)]
    drRow1 = tableCode id4 0x00775428 1
    stagedDr = B.take 0x05 validPayload <> B.pack [fromIntegral ((B.index validPayload 0x05 .&. 0x3f) .|. ((fromIntegral drRow1 `shiftL` 6) .&. 0xc0))] <> B.drop 0x06 validPayload

    slotCases =
      [ Spec
          { spId = "n-slot-" ++ show k
          , spProv =
              "contract: the root row " ++ hex 0x7d ++ " (index " ++ show (eRootEditRow e) ++ ") pushes the slot list, repoints its parent at the pushing frame and republishes the seven names from the card over the stale records; slot row " ++ show k ++ " opens submenu " ++ show k ++ " with the cursor at row 0; recall row " ++ show k ++ " applies slot " ++ show k ++ " (" ++ slotNames !! k ++ ") to the live row and leaves the card byte-identical"
          , spSteps =
              [ Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0
              , Draw (eSlot e) k 1 k
              , Dispatch (eSlot e) (fromIntegral k) 8 1
              , Dispatch (eRecall e) (fromIntegral k) 8 2
              ]
          , spClaims =
              [ claimU32 e (out 0) 0xffffffff
              , claimU32 e (out 1) 0xffffffff
              , claimU32 e (out 2) 1
              , claimAt e (sub k + 0x1c) (u32le (eSlot e))
              , claimAt e (eSlot e + 0x1c) (u32le (eRootNode e))
              , claimU32 e (eCursor e) 0
              , claimU32 e (eFrame e) (eRecall e)
              , claimAt e (eOut26 e + 32 * fromIntegral k) (name26 (slotNames !! k))
              , claimAt e 0x980000 cardA
              , claimAt e (0x96cc64 + 0x168) (B.take 2 (payloadFor (fromIntegral k)))
              , claimAt e (0x96cc64 + 0x16a) (B.take 2 (B.drop 2 (payloadFor (fromIntegral k))))
              ]
                ++ [claimAt e (eNameBase e + (0x83f + fromIntegral s - 0x83c) * 0x42) (name26 (slotNames !! s)) | s <- [0 .. 6]]
          , spRefuse = 0
          , spSetResult = 0x2a
          , spConfirmResult = 0x37
          , spLive = B.replicate 0x1c2 0
          , spCard = cardA
          , spModelCells = cardCells
          , spFunctions = ["rb_bank_interpose"]
          }
      | k <- [0 .. 6]
      ]

    otherCases =
      [ swapCase
      , swapMidScan
      , stockCase
      , staleBankCase
      , staleStockCase
      , refusedCase
      , itemCase True
      , itemCase False
      , editCase True
      , editCase False
      , confirmCase "save" True
      , confirmCase "save" False
      , confirmCase "reset" True
      , confirmCase "reset" False
      , refusedDialogCase
      , refusedDialogRetryCase
      , renameFailCase
      , renameCancelCase
      ]

    refusedDialogCase =
      Spec
        { spId = "n-refused-dialog"
        , spProv = "contract: a refused dialog raise from the edit screen's leaving edge pops back to the submenu and restores the caller's row (2), so the hidden dialog row is not left selected"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , WriteWord (eRefuse e) 1
            , Dispatch (eEditScreen e) 0 13 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 0xffffffff
            , claimU32 e (eCursor e) 2
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e (eEditScreen e + 0x1c) (u32le (sub 0))
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose", "rb_stage_edit_open"]
        }

    refusedDialogRetryCase =
      Spec
        { spId = "n-refused-dialog-retry"
        , spProv = "contract: after a refused dialog raise, a SET on the submenu's edit row opens the edit screen again rather than a dialog with no pending confirmation"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , WriteWord (eRefuse e) 1
            , Dispatch (eEditScreen e) 0 13 2
            , WriteWord (eRefuse e) 0
            , Dispatch (sub 0) 2 8 3
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 0xffffffff
            , claimU32 e (out 3) 1
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eEditScreen e)
            , claimAt e (eEditScreen e + 0x1c) (u32le (sub 0))
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose", "rb_stage_edit_open"]
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
            , Copy 0x980000 (eFileB e) 512
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
            , claimAt e (eSlot e + 0x1c) (u32le (eRootNode e))
            , claimU32 e (eFrame e) (eSlot e)
            ]
              ++ [claimAt e (eNameBase e + (0x83f + fromIntegral s - 0x83c) * 0x42) (name26 (swapNames !! s)) | s <- [0 .. 6]]
              ++ [claimAt e 0x980000 cardB]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose", "rb_media_epoch_bump"]
        }

    swapMidScan =
      Spec
        { spId = "n-swap-midscan"
        , spProv = "contract: a card change during the load's scan is refused with RB_MEDIA_CHANGED while the resident bank holds; the push still completes and the records keep their previous bytes instead of mixing the two cards"
        , spSteps = [Dispatch (eRootNode e) (fromIntegral (eRootEditRow e)) 8 0]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (eCursor e) 0
            , claimAt e (eSlot e + 0x1c) (u32le (eRootNode e))
            , claimU32 e (eFrame e) (eSlot e)
            , claimAt e 0x980000 swappedCard
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardWith3 cardA cardC
        , spModelCells = cardCells ++ [("rbm_swap_step", 2), ("rbm_swap_a_size", 512)]
        , spFunctions = ["rb_bank_interpose"]
        }
      where
        swappedCard = fileSlot cardC <> B.replicate 0x220 0 <> fileSlot cardC

    stockCase =
      Spec
        { spId = "n-stock-page"
        , spProv = "contract: an unrelated stock page (class 5) delegates every row and the leave edge unchanged: the interposer returns the delegate's word, the frame, cursor and the stock children's parent fields are untouched, and the row whose token equals the bank root token but whose child is not a bank page is not claimed"
        , spSteps =
            [ Dispatch (eStockNode e) 0 8 0
            , Dispatch (eStockNode e) 1 8 1
            , Dispatch (eStockNode e) 0 13 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0x2a
            , claimU32 e (out 1) 0x2a
            , claimU32 e (out 2) 0
            , claimU32 e (eLog e + 0) 2
            , claimU32 e (eLog e + 4) 1
            , claimU32 e (eLog e + 4 * 7) 5
            , claimU32 e (eLog e + 4 * 8) 0x7d
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eStockNode e)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose"]
        }

    staleBankCase =
      Spec
        { spId = "n-stale-bank"
        , spProv = "contract: a slot-list cursor the values array does not carry (99) clamps the cursor cell to row 0 and drops the event without entering the stock delegate"
        , spSteps = [Dispatch (eSlot e) 99 8 0]
        , spClaims =
            [ claimU32 e (out 0) 1
            , claimU32 e (eLog e + 0) 0
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eSlot e)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose"]
        }

    staleStockCase =
      Spec
        { spId = "n-stale-stock"
        , spProv = "contract: a stale cursor on an unrelated stock page is clamped and dropped before delegation, so the stock delegate's unchecked values[cursor] read is never reached"
        , spSteps = [Dispatch (eStockNode e) 9 8 0]
        , spClaims =
            [ claimU32 e (out 0) 1
            , claimU32 e (eLog e + 0) 0
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eStockNode e)
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose"]
        }

    refusedCase =
      Spec
        { spId = "n-refused-push"
        , spProv = "contract: a refused push transition leaves the pushed child's parent field at its emitted value and the frame, cursor and records unchanged"
        , spSteps = [Dispatch (eSlot e) 0 8 0, Draw (eSlot e) 0 1 0]
        , spClaims =
            [ claimU32 e (out 0) 0
            , claimAt e (sub 0 + 0x1c) (u32le (wordAt e (sub 0) 0x1c))
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eSlot e)
            ]
        , spRefuse = 1
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose"]
        }

    itemCase ok =
      Spec
        { spId = if ok then "n-item-ok" else "n-item-cancel"
        , spProv =
            "contract: the edit screen's first row opens the DR field list, DR row 1 stages the table code, the field host's leaving edge raises the item dialog, and the dialog's "
              ++ (if ok then "OK keeps the staged code" else "CANCEL restores the code the item session replaced")
              ++ "; both pop back to the edit screen with the card untouched"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) 0 8 2
            , Dispatch (field 0) 1 8 3
            , Dispatch (field 0) 0 13 4
            , Dispatch (eDialog e) (if ok then 0 else 1) 8 5
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 0xffffffff
            , claimU32 e (out 3) 1
            , claimU32 e (out 4) 1
            , claimU32 e (out 5) 1
            , claimU32 e (eCursor e) 0
            , claimU32 e (eFrame e) (eEditScreen e)
            , claimAt e (eEditScreen e + 0x1c) (u32le (sub 0))
            , claimAt e (eDialog e + 0x1c) (u32le (field 0))
            , claimAt e 0x980000 cardA
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose", "rb_stage_edit_open"]
        }

    editCase ok =
      Spec
        { spId = if ok then "n-edit-ok" else "n-edit-cancel"
        , spProv =
            "contract: the whole-edit dialog is raised from the edit screen's leaving edge after a staged DR change, and its "
              ++ (if ok then "OK commits the staged payload into a new-generation RECIPEA.BIN carrying the slot's name" else "CANCEL writes nothing")
              ++ "; both pop back to the submenu"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 2 8 1
            , Dispatch (eEditScreen e) 0 8 2
            , Dispatch (field 0) 1 8 3
            , Dispatch (field 0) 0 13 4
            , Dispatch (eDialog e) 0 8 5
            , Dispatch (eEditScreen e) 0 13 6
            , Dispatch (eDialog e) (if ok then 0 else 1) 8 7
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 0xffffffff
            , claimU32 e (out 3) 1
            , claimU32 e (out 4) 1
            , claimU32 e (out 5) 1
            , claimU32 e (out 6) 1
            , claimU32 e (out 7) 1
            , claimU32 e (eCursor e) 2
            , claimU32 e (eFrame e) (sub 0)
            , claimAt e (eEditScreen e + 0x1c) (u32le (sub 0))
            ]
              ++ [claimAt e 0x980220 expectedFile | ok]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose", "rb_stage_edit_open", "rb_stage_commit"]
        }
      where
        expectedFile =
          if ok
            then encodeFile 8 0x7f ((name26 (slotNames !! 0), stagedDr) : drop 1 slotsA)
            else cardA

    confirmCase verb ok =
      Spec
        { spId = "n-" ++ verb ++ "-" ++ (if ok then "ok" else "cancel")
        , spProv =
            "contract: the submenu's " ++ verb ++ " row (index " ++ show verbRow ++ ") raises its confirmation dialog on the submenu page, whose parent is the submenu and whose cursor is row 0; the dialog's "
              ++ (if ok then "OK runs the bank action on slot 0" else "CANCEL writes nothing")
              ++ "; both pop once back to the submenu"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) (fromIntegral verbRow) 8 1
            , Dispatch (eDialog e) (if ok then 0 else 1) 8 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 1
            , claimU32 e (out 2) 1
            , claimAt e (eDialog e + 0x1c) (u32le (sub 0))
            , claimU32 e (eCursor e) (fromIntegral verbRow)
            , claimU32 e (eFrame e) (sub 0)
            ]
              ++ ( if ok && verb == "save"
                     then
                       [ claimAt e 0x980220 (B.take 0x20 expectedFile)
                       , claimAt e 0x980358 (B.take 0xb6 (B.drop 0x138 expectedFile))
                       ]
                     else [claimAt e 0x980220 expectedFile | ok]
                 )
              ++ [claimAt e (eNameBase e + (0x83f - 0x83c) * 0x42) expectedName | ok && verb == "reset"]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = if verb == "save" then extractLive (eFieldsMap e) else B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose", "rb_save_current", "rb_reset", "rb_readback"]
        }
      where
        verbRow = if verb == "save" then 0 else 3
        expectedFile =
          if not ok
            then cardA
            else
              if verb == "save"
                then encodeFile 8 0x7f ((name26 (slotNames !! 0), extractPayload) : drop 1 slotsA)
                else encodeFile 8 0x7e ((B.replicate 26 0, B.replicate 0x28 0) : drop 1 slotsA)
        expectedName = if ok && verb == "reset" then B.replicate 26 0 else name26 (slotNames !! 0)

    renameFailCase =
      Spec
        { spId = "n-rename-fail"
        , spProv = "contract: opening the bank editor from the submenu's EDIT CUSTOM NAME row pushes the editor node with its parent at the submenu and fills the buffer with the slot's name; a confirm whose write is refused keeps the editor open on the same row, reports the read-only label and leaves the card and the name records untouched"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 1 8 1
            , WriteHalf 0x146761e 0x1e
            , Dispatch (eEditor e) 0x2a 8 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0xffffffff
            , claimU32 e (out 2) 1
            , claimU32 e (eLog e + 4 * 5) 0x92a
            , claimU32 e (eLog e + 4 * 10) 0x7e3
            , claimAt e 0x146761e (u16le 0x1e)
            , claimAt e 0x954385 (B.pack (map (fromIntegral . fromEnum) "ALPHA") <> B.replicate 9 0)
            , claimAt e (eEditor e + 0x1c) (u32le (sub 0))
            , claimU32 e (eFrame e) (eEditor e)
            , claimU32 e (eCursor e) 0x2a
            , claimU32 e 0x9504a8 1
            , claimAt e 0x980000 cardA
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells ++ [("rbm_write_comp", 8)]
        , spFunctions = ["rb_bank_interpose", "rb_name_edit_open", "rb_name_commit"]
        }

    renameCancelCase =
      Spec
        { spId = "n-rename-cancel"
        , spProv = "contract: the editor's unclaimed DISP/BACK cell (state 0x1e, event 13) exits without a bank write; the frame returns to the page the push repointed, the modal state clears and the card and the name records keep their bytes"
        , spSteps =
            [ Dispatch (eSlot e) 0 8 0
            , Dispatch (sub 0) 1 8 1
            , Dispatch (eEditor e) 0 13 2
            ]
        , spClaims =
            [ claimU32 e (out 0) 0xffffffff
            , claimU32 e (out 1) 0xffffffff
            , claimU32 e (out 2) 0xffffffff
            , claimAt e 0x146761e (u16le 0)
            , claimU32 e (eFrame e) (sub 0)
            , claimU32 e (eCursor e) 0
            , claimU32 e 0x9504a8 1
            , claimAt e 0x954385 (B.pack (map (fromIntegral . fromEnum) "ALPHA") <> B.replicate 9 0)
            , claimAt e (eEditor e + 0x1c) (u32le (sub 0))
            , claimAt e 0x980000 cardA
            ]
        , spRefuse = 0
        , spSetResult = 0x2a
        , spConfirmResult = 0x37
        , spLive = B.replicate 0x1c2 0
        , spCard = cardA
        , spModelCells = cardCells
        , spFunctions = ["rb_bank_interpose", "rb_name_edit_open"]
        }

-- inputs and the case image ----------------------------------------------------------

data Inputs = Inputs
  { inBuild :: FilePath
  , inKs, inMs, inUs :: Syms
  , inKbin, inKtext, inMdata :: B.ByteString
  , inUiA, inUiB, inUiC, inUiP :: B.ByteString
  , inId4 :: B.ByteString
  , inPlan :: [PlanWrite]
  , inFields :: [Field]
  , inTab :: B.ByteString
  , inCharsetBytes :: B.ByteString
  , inNameBase :: Word32
  , inHandler :: Word32
  , inEpochBump :: Word32
  , inStagePayload :: Word32
  , inPlanCellBytes :: [(Word32, B.ByteString)]
  , inPresenterSize :: Int
  , inMediaLo, inMediaHi :: Word32
  , inMediaImg :: B.ByteString
  , inLowcode :: B.ByteString
  , inMenuBase :: Word32
  , inMenu :: B.ByteString
  , inRootValues :: Word32
  , inRootLabels :: Word32
  }

nodeBytes :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> B.ByteString
nodeBytes i t cls k fl par vals labs sel =
  B.concat [u32le i, u32le t, u32le cls, u32le k, u32le 0, u32le 0, u32le fl, u32le par, u32le vals, u32le labs, u32le sel]

cellsImage :: Inputs -> Env -> Spec -> B.ByteString
cellsImage in' e sp =
  overlay (B.replicate 0xa000 0) $
    [ (fromIntegral (eCharsetCopy e - 0x950000), inCharsetBytes in')
    , (fromIntegral (eRootNode e - 0x950000), nodeBytes 1 0 5 7 1 0 (eRootValues e) (eRootLabels e) 0x2d)
    , (fromIntegral (eStockNode e - 0x950000), nodeBytes 0x20 6 5 6 1 (eStockParent e) (eStockValues e) 0 0)
    , (fromIntegral (eStockValues e - 0x950000), B.concat [u32le 0x7c, u32le (eStockChild1 e), u32le 0x7d, u32le (eStockChild2 e), u32le 0x7ffffffe, u32le 0])
    , (fromIntegral (eStockChild1 e - 0x950000), nodeBytes 0x21 6 0x100 6 1 0xdeadbeef 0 0 0)
    , (fromIntegral (eStockChild2 e - 0x950000), nodeBytes 0x22 6 0x100 6 1 0 0 0 0)
    , (fromIntegral (eStockParent e - 0x950000), nodeBytes 0x23 6 5 6 1 0 0 0 0)
    , (fromIntegral (eSteps e - 0x950000), stepBytes (spSteps sp))
    , (fromIntegral (eHandler e - 0x950000), u32le (inHandler in'))
    , (fromIntegral (eEpochBump e - 0x950000), u32le (inEpochBump in'))
    , (fromIntegral (addr (inUs in') "ui_name_base" - 0x950000), u32le (inNameBase in'))
    , (fromIntegral (addr (inUs in') "ui_refuse" - 0x950000), u32le (spRefuse sp))
    , (fromIntegral (addr (inUs in') "ui_set_result" - 0x950000), u32le (spSetResult sp))
    , (fromIntegral (addr (inUs in') "ui_confirm_result" - 0x950000), u32le (spConfirmResult sp))
    , (fromIntegral (eFileB e - 0x950000), cardB)
    ]

spanLowImage :: Inputs -> Env -> Spec -> B.ByteString
spanLowImage in' e sp =
  overlay (B.replicate (0x980660 - 0x96cc64) 0) $
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
regionList in' e sp =
  [ ("ktext", 0x011dcca0, B.length ktext, 5, ktext)
  , ("kdata", 0x011dcca0 + fromIntegral textLen, dataLen, 3, B.replicate dataLen 0)
  , ("media", inMediaLo in', B.length (inMediaImg in'), 5, inMediaImg in')
  , ("lowcode", 0x0061c400, B.length (inLowcode in'), 5, inLowcode in')
  , ("span-low", 0x0096cc64, B.length spanLow, 3, spanLow)
  , ("media-state", 0x00910140, 0x200, 3, B.replicate 0x200 0)
  , ("tables-ptr", 0x0081b09c, 0x89edd8 - 0x81b09c, 3, inTab in')
  , ("tables-edit", 0x00775428, 0x77b0a0 - 0x775428, 1, B.take (0x77b0a0 - 0x775428) (B.drop (0x775428 - 0x61c400) (inId4 in')))
  , ("menu", eMenuBase e, B.length (eMenu e), 3, eMenu e)
  , ("cells", 0x00950000, B.length cells, 3, cells)
  , ("modal-cells", 0x00cd1ae4, 0xa00, 3, overlay (B.replicate 0xa00 0) [(fromIntegral (a - 0xcd1ae4), b) | (a, b) <- inPlanCellBytes in'])
  , ("modal-state", 0x0146761c, 0x162, 3, B.replicate 0x162 0)
  , ("ui-a", 0xc12aac, B.length (inUiA in'), 5, inUiA in')
  , ("ui-b", 0xc72e10, B.length (inUiB in'), 5, inUiB in')
  , ("ui-c", 0xd38d98, 0xd3da00 - 0xd38d98, 5, inUiC in')
  , ("presenter", 0x0113a014, inPresenterSize in', 5, inUiP in')
  ]
  where
    ktext = inKtext in'
    textLen = B.length ktext
    dataLen = B.length (inKbin in') - textLen
    spanLow = spanLowImage in' e sp
    cells = cellsImage in' e sp
    planCells = overlay (B.replicate 0x63c 0) [(fromIntegral (a - 0xcd1ae4), b) | (a, b) <- inPlanCellBytes in']

-- the runner -------------------------------------------------------------------------

symOf :: Inputs -> String -> (Integer, Integer)
symOf in' n
  | hasSym (inKs in') n = (addr (inKs in') n, sizeOf (inKs in') n)
  | hasSym (inMs in') n = (addr (inMs in') n, sizeOf (inMs in') n)
  | hasSym (inUs in') n = (addr (inUs in') n, sizeOf (inUs in') n)
  | otherwise = error ("unknown function symbol: " ++ n)

stubDefs :: [Value]
stubDefs =
  [ stub "0x00723cb8" [] [("r0", 3)]
  , stub "0x0071ddf0" [("r0", 0)] [("r0", 0)]
  , stub "0x0071f708" [] [("r0", 0x43)]
  , stub "0x006439e0" [] [("r0", 1)]
  , stub "0x0061d8dc" [("r0", 0x0096cc74)] [("r0", 0)]
  ]
  where
    stub :: String -> [(String, Word32)] -> [(String, Word32)] -> Value
    stub a ins outs = object ["address" .= a, "inputs" .= bindings ins, "outputs" .= bindings outs, "side_effects" .= ("registers-only" :: String)]
    bindings :: [(String, Word32)] -> Value
    bindings bs = object [fromString k .= hex (fromIntegral v) | (k, v) <- bs]

caseJson :: Inputs -> Env -> Spec -> [(String, String)] -> [Value] -> [Value] -> [String] -> Value
caseJson in' e sp digests residues events fnNames =
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
    , "functions" .= ([fnObject n | n <- fnNames] ++ [object ["address" .= hex (fromIntegral a), "size" .= (z :: Int)] | (a, z) <- stockFns])
    , "stubs" .= stubDefs
    , "limits" .= object ["instructions" .= (1000000 :: Int), "wall_ms" .= (4000 :: Int)]
    , "expected" .=
        object
          [ "status" .= ("returned" :: String)
          , "registers" .= object ["r0" .= hex 0]
          , "memory" .= ([object ["region" .= n, "offset" .= o, "hex" .= hexB b] | (n, o, b) <- spClaims sp] ++ residues)
          , "events" .= events
          ]
    , "exclusions" .=
        ( [ "OBSERVED RESIDUE (no behavior claim): the unclaimed bytes of the pinned regions"
          , "the synthetic consumer stands in for the stock dispatch, transition, delegate and label consumers; no stock instruction executes"
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
    exitAddr = 0xd3d920 :: Integer
    fnObject n
      | "fn-" `isPrefixOf` n = object ["address" .= hex (fromIntegral (hexI (drop 3 n) :: Integer) :: Integer), "size" .= (0x10 :: Int)]
      | otherwise = let (a, z) = symOf in' n in object ["address" .= hex (fromIntegral a), "size" .= z]
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

-- observed memory and residue

memMap :: Env -> Value -> M.Map String B.ByteString
memMap e r = M.fromList (fromMaybe [] (parseMaybe p =<< parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "memory"))) r))
  where
    p = withArray "mem" $ \a -> pure [(nameOf k, unhex v) | x <- toList a, Just (k, v) <- [row x]]
    row x = parseMaybe (withObject "r" $ \o -> (,) <$> (o .: "address" :: Parser String) <*> o .: "bytes") x
    nameOf a = case [n | (n, b, _) <- eRegions e, fromIntegral b == hexI a] of
      (n : _) -> n
      [] -> a

runnerEvents :: Value -> [Value]
runnerEvents r = fromMaybe [] (parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (.: "events"))) r)

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

-- the first byte where the observed final memory differs from the initial image plus the
-- claims and residues, so a claim mismatch reports its exact address and values
firstDelta :: Env -> Spec -> [(String, Word32, Int, Int, B.ByteString)] -> [(String, Int, B.ByteString)] -> M.Map String B.ByteString -> Maybe String
firstDelta e sp regs expected observed =
  listToMaybe
    [ spId sp ++ ": " ++ n ++ " 0x" ++ showHex (fromIntegral base + idx) "" ++ " expected " ++ hexB (B.singleton x) ++ " observed " ++ hexB (B.singleton y) ++ " (initial " ++ hexB (B.singleton i) ++ ")"
    | (n, base, _s, _p, initBytes) <- regs
    , not (B.null initBytes)
    , Just v <- [M.lookup n observed]
    , let desired = foldl (\bs (_, o, b) -> B.take o bs <> b <> B.drop (o + B.length b) bs) initBytes [(n', o, b) | (n', o, b) <- expected, n' == n]
    , (idx, (x, y)) <- take 1 [(i', (B.index desired i', B.index v i')) | i' <- [0 .. min (B.length desired) (B.length v) - 1], B.index desired i' /= B.index v i']
    , let a = fromIntegral base + idx
    , let i = B.index initBytes idx
    ]
  where
    _ = e

residueTuples :: [Value] -> [(String, Int, B.ByteString)]
residueTuples = mapMaybe (\v -> parseMaybe (withObject "r" (\o -> (,,) <$> o .: "region" <*> o .: "offset" <*> (unhex . T.unpack <$> o .: "hex"))) v)

runnerMissingFn :: Inputs -> Value -> Maybe String
runnerMissingFn in' r = do
  rs <- parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (.: "status"))) r
  if rs == ("unknown_call" :: String) then do
    tr <- parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (.: "trace"))) r
    lastE <- case reverse (case tr of Array v -> toList v; _ -> []) of (x : _) -> Just x; [] -> Nothing
    pc <- parseMaybe (withObject "e" (\o -> o .: "pc")) lastE >>= parseHexV
    ins <- parseMaybe (withObject "e" (\o -> o .: "instruction")) lastE >>= parseHexV
    if (ins .&. 0x0f000000) == 0x0b000000
      then do
        let imm = fromIntegral (ins .&. 0xffffff) :: Integer
            d = (if imm >= 0x800000 then imm - 0x1000000 else imm) * 4
        pure (fromMaybe ("fn-" ++ hex (fromIntegral pc + 8 + d)) (symAt in' (fromIntegral pc + 8 + d)))
      else
        if (ins .&. 0x0f000000) == 0x0a000000
          then do
            -- a tail branch into a block the case has not declared, e.g. a tail-merged exit
            let imm = fromIntegral (ins .&. 0xffffff) :: Integer
                d = (if imm >= 0x800000 then imm - 0x1000000 else imm) * 4
            pure (fromMaybe ("fn-" ++ hex (fromIntegral pc + 8 + d)) (symAt in' (fromIntegral pc + 8 + d)))
          else
        if (ins .&. 0x0ffffff0) == 0x012fff30
          then do
            let rm = ins .&. 15
                regName = if rm == 14 then "lr" else "r" ++ show rm
            rv <- parseMaybe (withObject "rep" (\o -> o .: "runner" >>= withObject "rn" (\rn -> rn .: "registers" >>= withObject "regs" (\rs -> rs .: fromString regName)))) r >>= parseHexV
            pure (fromMaybe ("fn-" ++ hex (fromIntegral rv)) (symAt in' (fromIntegral rv)))
          else Nothing
  else Nothing

symAt :: Inputs -> Integer -> Maybe String
symAt in' a =
  case hit (inKs in') of
    Just n -> Just n
    Nothing -> case hit (inMs in') of
      Just n -> Just n
      Nothing -> hit (inUs in')
  where
    hit s = listToMaybe [n | (n, (b, z)) <- sortOn (\(_, (b, z)) -> (max z 1, b)) (M.toList (case s of Syms m -> m)), b <= a, a < b + max z 1]

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

loadInputs :: FilePath -> FilePath -> FilePath -> FilePath -> String -> IO Inputs
loadInputs build planPath id4Path fieldsPath nameRecordArg = do
  ks <- parseSyms (build </> "kernel.syms")
  ms <- parseSyms (build </> "model.syms")
  us <- parseSyms (build </> "ui-model.syms")
  unless (hasSym us "ui_run") (error "ui-model.syms has no ui_run: build the UI model first")
  kbin <- B.readFile (build </> "kernel.bin")
  ktext <- B.readFile (build </> "kernel-text.bin")
  mdata <- B.readFile (build </> "model-data.bin")
  uiA <- B.readFile (build </> "ui-a.bin")
  uiB <- B.readFile (build </> "ui-b.bin")
  uiC <- B.readFile (build </> "ui-c.bin")
  uiP <- B.readFile (build </> "ui-p.bin")
  id4 <- B.readFile id4Path
  plan <- parsePlan planPath
  fieldsRaw <- B.readFile fieldsPath
  mgetter <- B.readFile (build </> "model-getter.bin")
  msetter <- B.readFile (build </> "model-setter.bin")
  let jsonFields = parseFields fieldsRaw
      halfProps = [0x1cb, 0x1cc] :: [Word32]
      fields = filter (\f -> fProp f `notElem` halfProps) jsonFields ++ [Field 0x1cb 0 0 2 0xffff, Field 0x1cc 2 0 2 0xffff]
      (menuBase, menuImg0) = composeMenu (menuWrites plan)
      nameBasePlan = case [pwAddress w | w <- plan, pwRegion w == "name_records"] of
        [a] -> a
        as -> error ("expected one name-record write, got " ++ show (length as))
      nameBase = fromIntegral (hexI nameRecordArg)
      charsetAddr = 0x00957200 :: Word32
      charset = B.take 0x160 (B.drop (0x78bac8 - 0x61c400) id4)
      editorNode = nodeByRole plan "bank name editor node header"
      menuImg = overlay menuImg0 [(fromIntegral (editorNode - menuBase) + 0x20, u32le charsetAddr)]
      handler = fromIntegral (addr ks "rb_bank_interpose")
      epochBump = fromIntegral (addr ks "rb_media_epoch_bump")
      stagePayload = fromIntegral (addr ks "rb_stage_payload")
      planCells = [(pwAddress w, pwNew w) | w <- plan, pwRegion w == "image5", "modal action cell" `isInfixOf` pwRole w]
      mediaSpans = ["rbm_low_close", "rbm_read", "rbm_write", "rbm_close", "rbm_open", "rbm_stat", "rbm_index_free", "rbm_mapping_delete", "rbm_resolve", "rbm_tag_release"]
      mediaSpansHi = mediaSpans ++ ["rbm_event_hook", "rbm_detach_event", "rbm_attach_event", "rbm_media_eval"]
      mediaLo = fromIntegral (minimum [addr ms n | n <- mediaSpans])
      mediaHi =
        fromIntegral
          ( maximum
              ( (maximum [addr ms n + sizeOf ms n | n <- mediaSpansHi])
                  : [a + max z 4 | (n, (a, z)) <- M.toList (case ms of Syms m -> m), take 3 n == "rbm", a < 0x900000]
              )
          )
      getterLen = fromIntegral (sizeOf ms "rbm_get")
      lowcode =
        overlay
          (B.take (0x646000 - 0x61c400) (B.drop 0 id4))
          [(0x61e110 - 0x61c400, mgetter), (0x624694 - 0x61c400, msetter)]
      parentValues = nodeByRole plan "parent values array with the two items"
      parentLabels = nodeByRole plan "parent labels array with the two items"
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
      { inBuild = build
      , inKs = ks
      , inMs = ms
      , inUs = us
      , inKbin = kbin
      , inKtext = ktext
      , inMdata = mdata
      , inUiA = uiA
      , inUiB = uiB
      , inUiC = uiC
      , inUiP = uiP
      , inId4 = id4
      , inPlan = plan
      , inFields = fields
      , inTab = tabImage id4
      , inCharsetBytes = charset
      , inNameBase = nameBase
      , inHandler = handler
      , inEpochBump = epochBump
      , inStagePayload = stagePayload
      , inPlanCellBytes = planCells
      , inPresenterSize = B.length uiP
      , inMediaLo = mediaLo
      , inMediaHi = mediaHi
      , inMediaImg = compose mediaLo mediaHi (chunks ++ [(fromIntegral (addr ms "helpers"), helpers)])
      , inLowcode = lowcode
      , inMenuBase = menuBase
      , inMenu = menuImg
      , inRootValues = parentValues
      , inRootLabels = parentLabels
      }

envFrom :: Inputs -> Env
envFrom in' =
  Env
    { eMenuBase = inMenuBase in'
    , eMenu = inMenu in'
    , eSlot = nodeByRole plan "slot list node header"
    , eRecall = nodeByRole plan "recall list node header"
    , eSubs = nodesByRole plan "sub-list node header"
    , eEditScreen = nodeByRole plan "edit screen node header"
    , eDialog = nodeByRole plan "dialog node header"
    , eFields = nodesByRole plan "field-list node header"
    , eEditor = nodeByRole plan "bank name editor node header"
    , eRootValues = inRootValues in'
    , eRootLabels = inRootLabels in'
    , eNameBase = inNameBase in'
    , eCharsetCopy = 0x00957200
    , eRootEditRow = rootEditRow
    , eRegions = regionBases in'
    , eOut = fromIntegral (addr us "ui_out")
    , eLog = fromIntegral (addr us "ui_log")
    , eOut26 = fromIntegral (addr us "ui_out26")
    , eFileB = fromIntegral (addr us "ui_file_b")
    , eSteps = fromIntegral (addr us "ui_steps")
    , eFrame = 0x009571d0
    , eCursor = 0x009571d8
    , eRootNode = 0x00957400
    , eStockNode = 0x00957440
    , eStockValues = 0x00957480
    , eStockChild1 = 0x009574c0
    , eStockChild2 = 0x00957500
    , eStockParent = 0x00957540
    , eHandler = fromIntegral (addr us "ui_handler")
    , eEpochBump = fromIntegral (addr us "ui_epoch_bump")
    , eRefuse = fromIntegral (addr us "ui_refuse")
    , eStagePayload = inStagePayload in'
    , eFieldsMap = inFields in'
    }
  where
    plan = inPlan in'
    us = inUs in'
    menuImg = inMenu in'
    menuBase = inMenuBase in'
    slotNode = nodeByRole plan "slot list node header"
    parentValues = nodeByRole plan "parent values array with the two items"
    rows =
      [ ( readU32 menuImg (fromIntegral (parentValues - menuBase) + 8 * i)
        , readU32 menuImg (fromIntegral (parentValues - menuBase) + 8 * i + 4)
        )
      | i <- [0 .. 63]
      ]
    rootEditRow = fromMaybe (error "the root row 0x7d was not found in the emitted parent values") (findIndex (\(t, c) -> t == 0x7d && c == slotNode) rows)

regionBases :: Inputs -> [(String, Word32, Int)]
regionBases in' =
  [ ("ktext", 0x011dcca0, B.length (inKtext in'))
  , ("kdata", 0x011dcca0 + fromIntegral textLen, dataLen)
  , ("media", inMediaLo in', fromIntegral (inMediaHi in' - inMediaLo in'))
  , ("lowcode", 0x0061c400, B.length (inLowcode in'))
  , ("span-low", 0x0096cc64, 0x980660 - 0x96cc64)
  , ("media-state", 0x00910140, 0x200)
  , ("tables-ptr", 0x0081b09c, 0x89edd8 - 0x81b09c)
  , ("tables-edit", 0x00775428, 0x77b0a0 - 0x775428)
  , ("menu", inMenuBase in', B.length (inMenu in'))
  , ("cells", 0x00950000, 0xa000)
  , ("modal-cells", 0x00cd1ae4, 0xa00)
  , ("modal-state", 0x0146761c, 0x162)
  , ("ui-a", 0xc12aac, 0xc12c00 - 0xc12aac)
  , ("ui-b", 0xc72e10, 0xc76800 - 0xc72e10)
  , ("ui-c", 0xd38d98, 0xd3da00 - 0xd38d98)
  , ("presenter", 0x0113a014, inPresenterSize in')
  ]
  where
    textLen = B.length (inKtext in')
    dataLen = B.length (inKbin in') - textLen

residueFor :: Env -> Inputs -> Spec -> M.Map String B.ByteString -> [Value]
residueFor e in' sp observed =
  [ object ["region" .= n, "offset" .= lo', "hex" .= hexB (B.take (hi' - lo') (B.drop lo' v))]
  | (n, lo, hi) <- pinnedBytes in'
  , Just v <- [M.lookup n observed]
  , (lo', hi') <- gapsOf lo (min hi (B.length v)) (claims n)
  ]
  where
    claims n = sortOn fst [(o, o + B.length b) | (n', o, b) <- spClaims sp, n' == n]
    gapsOf lo hi spans = go lo spans
      where
        go cur [] = [(cur, hi) | cur < hi]
        go cur ((a, b) : rest)
          | b <= cur = go cur rest
          | a > cur = (cur, min a hi) : go (max cur b) rest
          | otherwise = go (max cur b) rest

runSpec :: FilePath -> FilePath -> Env -> Inputs -> Spec -> IO Value
runSpec runner out e in' sp = do
  let dir = out </> spId sp
  createDirectoryIfMissing True dir
  let regs = regionList in' e sp
      modelFns =
        [ n
        | (n, (a, z)) <- M.toList (case inUs in' of Syms m -> m)
        , z >= 4
        , (a >= 0x00c00000 && a < 0x00d40000) || (a >= 0x0113a014 && a < 0x0113a100)
        ]
  unless (map (\(n, a, _, _, _) -> (n, a)) regs == map (\(n, a, _) -> (n, a)) (eRegions e)) $
    error ("the region table and the case image disagree in " ++ spId sp)
  let arts = [(n, b) | (n, _, _, _, b) <- regs, not (B.null b)]
  forM_ arts $ \(n, b) -> B.writeFile (dir </> (n ++ ".bin")) b
  digests <- forM arts $ \(n, b) -> (\d -> (n, d)) <$> sha256 b
  let writeCase spc fnNames residues evs =
        BL.writeFile (dir </> "case.json") (encode (caseJson in' e spc digests residues evs fnNames))
      loop fns fuel = do
        writeCase sp fns [] []
        (_, r1) <- runCase runner (dir </> "case.json")
        case runnerMissingFn in' r1 of
          Just n | fuel > 0 && n `notElem` fns && not (covered fns n) -> loop (fns ++ [n]) (fuel - 1)
          _ -> pure (fns, r1)
      covered _ n
        | "fn-" `isPrefixOf` n =
            let a = fromIntegral (hexI (drop 3 n) :: Integer) :: Word32
             in any (\(b, z) -> a >= b && a < b + fromIntegral z) stockFns
        | otherwise = False
  (fns, r1) <- loop (spFunctions sp ++ alwaysFns ++ modelFns) (48 :: Int)
  let observed = memMap e r1
      residues = residueFor e in' sp observed
      -- the saved slot's payload bytes come from the live walk, whose expected image is
      -- regenerated from the run; the new file's header and names stay explicit claims
      cardPayload =
        if spId sp == "n-save-ok"
          then case M.lookup "span-low" observed of
            Just v -> [("span-low", 0x980240 - 0x96cc64, B.take (0x358 - 0x240) (B.drop (0x980240 - 0x96cc64) v))]
            Nothing -> []
          else []
      sp' = sp { spClaims = spClaims sp ++ cardPayload }
  writeCase sp' fns residues (runnerEvents r1)
  (_, r2) <- runCase runner (dir </> "case.json")
  let status = runnerStatus r2
      miss = if rstat == "unknown_call" then runnerMissingFn in' r2 else Nothing
      reason = reportReason r2
      rstat = reportRunnerStatus r2
      delta = if status == "failed" then firstDelta e sp regs (spClaims sp ++ residueTuples residues) (memMap e r2) else Nothing
  pure
    ( object
        ( [ "id" .= spId sp
          , "status" .= status
          , "mismatches" .= runnerMismatches r2
          , "reason" .= reason
          , "runner_status" .= rstat
          ]
            ++ maybe [] (\d -> ["first_delta" .= d]) delta
            ++ maybe [] (\n -> ["missing_fn" .= n]) miss
        )
    )

findingsText :: [Spec] -> [Value] -> String
findingsText specs results =
  unlines
    ( [ "recipe-bank UI-behavior cases: findings"
      , ""
      , "The consumer boundary is synthetic (emulation/recipe-bank-ui-model.c): the modal-75"
      , "cell dispatch, the 0xe/0xf transition records, the four stock delegates and the"
      , "label-record read stand in for the stock code; no stock instruction executes."
      , ""
      , "Case status:"
      ]
        ++ [ "  " ++ spId sp ++ ": " ++ fromMaybe "?" (parseMaybe (withObject "r" (.: "status")) r)
           | (sp, r) <- zip specs results
           ]
        ++ [ ""
           , "No behavior claim: the unclaimed bytes of the pinned regions (pinned from the"
           , "discovery run only to satisfy the runner's exact-final-memory contract)."
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
  fieldsPath <- maybe (fail "--fields") pure (opt "fields")
  nameRecordArg <- maybe (fail "--name-record") pure (opt "name-record")
  createDirectoryIfMissing True out
  in' <- loadInputs build planPath id4Path fieldsPath nameRecordArg
  let e = envFrom in'
      specs = specSet e (inId4 in') (inEpochBump in')
  results <- forM specs $ \sp -> runSpec runner out e in' sp
  BL.writeFile (out </> "results.json") (encode (object ["cases" .= results]))
  writeFile (out </> "findings.txt") (findingsText specs results)
  putStrLn ("ran " ++ show (length specs) ++ " cases")
  unless (all ((== Just ("passed" :: String)) . parseMaybe (withObject "r" (.: "status"))) results) exitFailure
