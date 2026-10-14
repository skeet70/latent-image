{-# LANGUAGE OverloadedStrings #-}
-- RECOVERY-EMUL-01 case generator (1.50, host-only).
--
-- Emits native-case files for the recovery-path surface the bounded runner can
-- execute first: the native writer's block loop with a declared media model and
-- interruption injection at every persistence step. Expected results are
-- derived here from the byte-read decode and the documented persistence
-- ordering (reports/update-validation-and-boot.md, reports/install-procedure.md
-- section 8), never from emulator output.
--
-- Synthetic model code replaces only the four named device-boundary functions of
-- the 1.50 storage-driver API; every other executed byte is an exact slice of the
-- pinned decode, whose correspondence to the installed bytes is E-DUMP-RAM-150.
module Main where
import FujiAF.Host
import FujiAF.JSON (encodeCanonical)
import Data.Aeson (Value)
import qualified Data.ByteString as B
import Data.Bits
import Data.List (foldl', sortOn)
import Data.Word
import Numeric
import System.Directory
import System.Environment
import System.Exit
import System.FilePath

-- ---------------------------------------------------------------------------
-- Pinned inputs (exact SHA-256 required before any case is written).
-- ---------------------------------------------------------------------------

id4Path :: FilePath
id4Path = "private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin"

id4Hash :: String
id4Hash = "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"

id4Base :: Word32
id4Base = 0x0061c400

readPinned :: FilePath -> String -> IO B.ByteString
readPinned path expected = do
  exists <- doesFileExist path
  ensure exists (path ++ ": pinned input missing")
  bytes <- B.readFile path
  actual <- sha256 bytes
  ensure (actual == expected) (path ++ ": pinned SHA-256 mismatch")
  pure bytes

sliceOf :: B.ByteString -> Word32 -> Word32 -> B.ByteString
sliceOf image base size = B.take (fromIntegral size) (B.drop (fromIntegral (base - id4Base)) image)

word32le :: Word32 -> B.ByteString
word32le v = B.pack [fromIntegral (v `shiftR` k) | k <- [0, 8, 16, 24]]

hexOf :: B.ByteString -> String
hexOf = concatMap (\x -> let g = showHex x "" in replicate (2 - length g) '0' ++ g) . B.unpack

h :: Word32 -> Value
h v = s ("0x" ++ replicate (8 - length t) '0' ++ t) where t = showHex v ""

int :: Int -> Value
int = FujiAF.Host.n . fromIntegral

-- ---------------------------------------------------------------------------
-- Writer-case memory map (case inputs, fixed across the family).
-- ---------------------------------------------------------------------------

exitAddr, modelBase, stackBase, stackSize, cellsBase, journalBase :: Word32
exitAddr = 0x3c000000
modelBase = 0x00500000
stackBase = 0x3b000000
stackSize = 0x400
cellsBase = 0x3a000000
journalBase = 0x39000000

mediumBase, stagingBase :: Word32
mediumBase = 0x38000000
stagingBase = 0x30000000

cacheBase, cachePtrAddr, idxAddr, readyAddr :: Word32
cacheBase = 0x20000000
cachePtrAddr = 0x0096ca50
idxAddr = 0x00918844
readyAddr = 0x008fc86c

tableBase, tableSize, coreBase, coreSize :: Word32
tableBase = 0x007c8c6c
tableSize = 0x148
coreBase = 0x00828570
coreSize = 0x1300

entryCheck, entryRelease, entryProgram, entryErase :: Word32
entryCheck = 0x006faf00
entryRelease = 0x006faf40
entryProgram = 0x006faf5c
entryErase = 0x006fb13c

propGetter, eraseFn, badMapFn, s1Fn, writerFn :: Word32
propGetter = 0x0069f8fc
eraseFn = 0x006a9010
badMapFn = 0x006a90ac
s1Fn = 0x006a9aa4
writerFn = 0x006ab4cc

geomReadyFn, geomStartFn, geomUnitsFn, geomUnitSizeFn :: Word32
geomReadyFn = 0x006fbaf0
geomStartFn = 0x006fbb10
geomUnitsFn = 0x006fbb54
geomUnitSizeFn = 0x006fbb9c

stubStatus, stubLog, stubWindow :: Word32
stubStatus = 0x00723cb8
stubLog = 0x00723a14
stubWindow = 0x0004263c

newTrailer, oldTrailer :: B.ByteString
newTrailer = B.pack [0x00,0x00,0x4a,0x02, 0x00,0x00,0x4a,0x02, 0x20,0x00,0x00,0x00,
                     0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0xc4,0x61,0x00,
                     0x28,0x28,0x28,0x00, 0x00,0x00,0xaa,0xaa]
oldTrailer = B.pack [0x00,0x00,0x4a,0x02, 0x00,0x00,0x4a,0x02, 0x20,0x00,0x00,0x00,
                     0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00, 0x00,0xc4,0x60,0x00,
                     0x28,0x28,0x28,0x00, 0x00,0x00,0xaa,0xaa]

unitSize :: Word32
unitSize = 0x800

-- ---------------------------------------------------------------------------
-- A32 emitter. Only admitted forms are produced (case contract): MOVW/MOVT,
-- data processing with immediate or register-specified shifts, immediate
-- LDR/STR/LDRB/STRB with pre/post-index and writeback, register-offset
-- byte/word transfers pre-indexed without writeback and immediate LSL,
-- LDM/STM including the tracked pop return, conditional execution, BL, BX LR.
-- ---------------------------------------------------------------------------

data Cond = AL | CEQ | CNE | CGE | CHS | CLT | CLE deriving (Eq, Show)

condCode :: Cond -> Word32
condCode AL = 0xe
condCode CEQ = 0x0
condCode CNE = 0x1
condCode CLT = 0xb
condCode CGE = 0xa
condCode CHS = 0x2
condCode CLE = 0xd

type Label = String

data Ins
  = I Word32
  | Bl Label
  | Bra Cond Label
  | Lit Word32 Label
  | Def Label
  | Lbl Label
  deriving Show

encodeRot :: Word32 -> Maybe (Word32, Word32)
encodeRot 0 = Just (0, 0)
encodeRot v = go 0
  where
    go :: Int -> Maybe (Word32, Word32)
    go 16 = Nothing
    go k =
      let imm8 = (v `rotateL` (2 * k)) .&. 0xff
       in if imm8 /= 0 && v == imm8 `rotateR` (2 * k) then Just (fromIntegral k, imm8) else go (k + 1)

dpImm :: Cond -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Ins
dpImm cond opcode s rd rn imm =
  case encodeRot imm of
    Just (rot, imm8) ->
      I (condCode cond `shiftL` 28 .|. 1 `shiftL` 25 .|. opcode `shiftL` 21 .|. s `shiftL` 20
         .|. rn `shiftL` 16 .|. rd `shiftL` 12 .|. rot `shiftL` 8 .|. imm8)
    Nothing -> error ("immediate not encodable: " ++ show imm)

dpReg :: Word32 -> Word32 -> Word32 -> Word32 -> (Word32, Word32) -> Word32 -> Ins
dpReg opcode s rd rn (amount, rm) shiftType =
  I (0xe `shiftL` 28 .|. opcode `shiftL` 21 .|. s `shiftL` 20
     .|. rn `shiftL` 16 .|. rd `shiftL` 12 .|. amount `shiftL` 7
     .|. shiftType `shiftL` 5 .|. rm)

movwI, movtI :: Word32 -> Word32 -> Ins
movwI rd imm = movHalf 0x30 rd imm
movtI rd imm = movHalf 0x34 rd imm

movHalf :: Word32 -> Word32 -> Word32 -> Ins
movHalf opcode rd imm =
  let i16 = imm .&. 0xffff
   in I (0xe `shiftL` 28 .|. opcode `shiftL` 20 .|. rd `shiftL` 12
         .|. (i16 `shiftR` 12) `shiftL` 16 .|. (i16 .&. 0xfff))

lsImm :: Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> Ins
lsImm p u b w l rn rd offset =
  I (0xe `shiftL` 28 .|. 1 `shiftL` 26 .|. p `shiftL` 24 .|. u `shiftL` 23
     .|. b `shiftL` 22 .|. w `shiftL` 21 .|. l `shiftL` 20
     .|. rn `shiftL` 16 .|. rd `shiftL` 12 .|. (offset .&. 0xfff))

ldrI, strI, strbI, ldrbI :: Word32 -> Word32 -> Word32 -> Ins
ldrI rn rd off = lsImm 1 1 0 0 1 rn rd off
strI rn rd off = lsImm 1 1 0 0 0 rn rd off
strbI rn rd off = lsImm 1 1 1 0 0 rn rd off
ldrbI rn rd off = lsImm 1 1 1 0 1 rn rd off

ldmStm :: Cond -> Word32 -> Word32 -> Word32 -> Word32 -> Word32 -> [Int] -> Ins
ldmStm cond p u w l rn rs =
  I (condCode cond `shiftL` 28 .|. 4 `shiftL` 25 .|. p `shiftL` 24 .|. u `shiftL` 23
     .|. w `shiftL` 21 .|. l `shiftL` 20 .|. rn `shiftL` 16
     .|. foldl' (\acc r -> acc .|. 1 `shiftL` r) 0 rs)

pushR, popR :: [Int] -> Ins
pushR rs = ldmStm AL 1 0 1 0 13 rs
popR rs = ldmStm AL 0 1 1 1 13 rs

bxLr :: Ins
bxLr = I 0xe12fff1e

movR :: Word32 -> Word32 -> Ins
lslR, lsrR :: Word32 -> Word32 -> Word32 -> Ins
movR rd rm = dpReg 13 0 rd 0 (0, rm) 0
lslR rd rm amount = dpReg 13 0 rd 0 (amount, rm) 0
lsrR rd rm amount = dpReg 13 0 rd 0 (amount, rm) 1

addR :: Word32 -> Word32 -> Word32 -> Ins
cmpR :: Word32 -> Word32 -> Ins
addR rd rn rm = dpReg 4 0 rd rn (0, rm) 0
cmpR rn rm = dpReg 10 1 0 rn (0, rm) 0

addI, subI, bicI, andI :: Word32 -> Word32 -> Word32 -> Ins
cmpI, movI :: Word32 -> Word32 -> Ins
addI rd rn imm = dpImm AL 4 0 rd rn imm
subI rd rn imm = dpImm AL 2 0 rd rn imm
cmpI rn imm = dpImm AL 10 1 0 rn imm
movI rd imm = dpImm AL 13 0 rd 0 imm
bicI rd rn imm = dpImm AL 14 0 rd rn imm
andI rd rn imm = dpImm AL 0 0 rd rn imm

ldrPost, strPost :: Word32 -> Word32 -> Int -> Ins
ldrPost rn rd off =
  I (0xe `shiftL` 28 .|. 1 `shiftL` 26 .|. 0 `shiftL` 24 .|. 1 `shiftL` 23
     .|. 0 `shiftL` 21 .|. 1 `shiftL` 20 .|. rn `shiftL` 16 .|. rd `shiftL` 12
     .|. fromIntegral (abs off))
strPost rn rd off =
  I (0xe `shiftL` 28 .|. 1 `shiftL` 26 .|. 0 `shiftL` 24 .|. 1 `shiftL` 23
     .|. 0 `shiftL` 21 .|. rn `shiftL` 16 .|. rd `shiftL` 12
     .|. fromIntegral (abs off))

strR, ldrR :: Word32 -> Word32 -> (Word32, Word32) -> Ins
strR rn rd (amount, rm) = lsReg 0 0 rn rd (amount, rm)
ldrR rn rd (amount, rm) = lsReg 0 1 rn rd (amount, rm)

-- Register-offset byte/word transfer: bits 27-25 = 011, pre-indexed (P=1),
-- add (U=1), no writeback (W=0), immediate LSL shift only.
lsReg :: Word32 -> Word32 -> Word32 -> Word32 -> (Word32, Word32) -> Ins
lsReg b l rn rd (amount, rm) =
  I (0xe `shiftL` 28 .|. 3 `shiftL` 25 .|. 1 `shiftL` 24 .|. 1 `shiftL` 23
     .|. b `shiftL` 22 .|. l `shiftL` 20 .|. rn `shiftL` 16 .|. rd `shiftL` 12
     .|. amount `shiftL` 7 .|. rm)

assemble :: Word32 -> [(Label, Word32)] -> [Ins] -> Either String (B.ByteString, [(Label, Word32)])
assemble base seeded ins = do
  let offsets = scanl step 0 ins
      step off i = case i of
        Lit _ _ -> off + 8
        Def _ -> off
        Lbl _ -> off
        _ -> off + 4
      defined = [(l, base + fromIntegral o) | (Def l, o) <- zip ins offsets]
      internal = [(l, base + fromIntegral o) | (Lbl l, o) <- zip ins offsets]
      symbols = defined ++ internal ++ seeded
  chunks <- mapM (encode symbols) (zip offsets ins)
  pure (B.concat chunks, defined ++ internal)
  where
    encode symbols (off, i) = case i of
      I w -> Right (word32le w)
      Def _ -> Right B.empty
      Lbl _ -> Right B.empty
      Bl l -> branch symbols off True AL l
      Bra c l -> branch symbols off False c l
      Lit rd l -> case lookup l symbols of
        Nothing -> Left ("unknown label " ++ l)
        Just a -> Right (word32le (unI (movwI rd (a .&. 0xffff))) <> word32le (unI (movtI rd (a `shiftR` 16))))
    branch symbols off link cond l = case lookup l symbols of
      Nothing -> Left ("unknown branch label " ++ l)
      Just target ->
        let here = base + fromIntegral off
            delta = fromIntegral target - fromIntegral here - 8 :: Int
            imm = fromIntegral ((delta `div` 4) .&. 0xffffff) :: Word32
            w = condCode cond `shiftL` 28 .|. 0x5 `shiftL` 25
                .|. (if link then 1 `shiftL` 24 else 0) .|. imm
         in if delta `mod` 4 /= 0
              then Left ("misaligned branch to " ++ l)
              else Right (word32le w)
    unI (I w) = w
    unI _ = 0

-- ---------------------------------------------------------------------------
-- The declared media model. Synthetic code at modelBase (within BL range of the
-- firmware entries); the four device-boundary entries at their firmware
-- addresses carry 8-byte BL trampolines.
--
-- Persistence steps: one per block erase and one per unit program, matching the
-- install procedure's granularity claims (whole-block erase, program at unit
-- size 0x800). Each step is journaled at journalBase; the run guards at the
-- case's stop count so an interrupted case stops with exactly that many steps
-- landed, via a store to the unmapped address 0x5150 (memory_guard).
-- ---------------------------------------------------------------------------

modelCode :: [Ins]
modelCode = concat
  [ [ Def "m_check", movI 0 0, bxLr ]
  , [ Def "m_release", movI 0 0, bxLr ]
  , [ Def "m_step", Lit 3 "cells", ldrI 3 1 0, ldrI 3 2 4, cmpR 1 2, Bra CEQ "m_trip"
    , lslR 1 1 3, Lit 2 "journal", addR 1 1 2
    , ldrI 3 2 0x20, strI 1 2 4
    , ldrbI 3 2 0x24, strbI 1 2 1
    , ldrbI 3 2 0x25, strbI 1 2 0
    , ldrI 3 1 0, addI 1 1 1, strI 3 1 0
    , movI 0 0, bxLr ]
  , [ Lbl "m_trip", movwI 0 0x5150, movtI 0 0, strI 0 0 0, bxLr ]
  , [ Def "m_erase"
    , Lit 3 "cells", strI 3 14 0x58, strI 3 12 0x5c
    , strI 3 0 0x18, movI 2 0, strI 3 2 0x1c
    , Bl "geom_units", Lit 3 "cells", strI 3 0 0x28
    , Bl "geom_unit_size", Lit 3 "cells", strI 3 0 0x2c
    , ldrI 3 1 0x28, Bl "m_mul", Lit 3 "cells", strI 3 0 0x30 ]
  , [ Lbl "m_erase_loop"
    , Lit 3 "cells", ldrI 3 1 0x1c
    , ldrI 3 2 0x18, ldrI 2 2 4, cmpR 1 2, Bra CGE "m_erase_done"
    , ldrI 3 2 0x18, ldrI 2 2 0, addR 2 2 1
    , strI 3 2 0x20, movI 2 0xff, strbI 3 2 0x24, movI 2 1, strbI 3 2 0x25
    , addI 0 3 0x20, Bl "m_step"
    , Lit 3 "cells", ldrI 3 0 0x18, ldrI 0 0 0
    , ldrI 3 1 0x1c, addR 0 0 1, Bl "m_block_base", Lbl "ret_fill_erase"
    , Lit 3 "cells", ldrI 3 1 0x30, Bl "m_fill"
    , Lit 3 "cells", ldrI 3 1 0x1c, addI 1 1 1, strI 3 1 0x1c
    , Bra AL "m_erase_loop" ]
  , [ Lbl "m_erase_done", movI 0 0, Lit 3 "cells", ldrI 3 12 0x5c, ldrI 3 14 0x58, bxLr ]
  , [ Def "m_program"
    , Lit 3 "cells", strI 3 14 0x58, strI 3 12 0x5c, strI 3 0 0x18
    , movI 2 0, ldrI 3 1 0x18, strI 1 2 20
    , Bl "geom_units", Lit 3 "cells", strI 3 0 0x28
    , Bl "geom_unit_size", Lit 3 "cells", strI 3 0 0x2c
    , ldrI 3 1 0x28, Bl "m_mul", Lit 3 "cells", strI 3 0 0x30
    , ldrI 3 1 0x18, ldrI 1 1 0, Lit 3 "cells", strI 3 1 0x20
    , movI 2 0xff, strbI 3 2 0x24, movI 2 1, strbI 3 2 0x25
    , addI 0 3 0x20, Bl "m_step", Lbl "ret_erase_step"
    , Lit 3 "cells", ldrI 3 0 0x18, ldrI 0 0 0, Bl "m_block_base", Lbl "ret_fill_prog"
    , Lit 3 "cells", ldrI 3 1 0x30, Bl "m_fill"
    , Lit 3 "cells", movI 2 0, strI 3 2 0x1c ]
  , [ Lbl "m_program_loop"
    , Lit 3 "cells", ldrI 3 1 0x1c, ldrI 3 2 0x28, cmpR 1 2, Bra CGE "m_program_verify"
    , ldrI 3 2 0x18, ldrI 2 2 0
    , strI 3 2 0x20, strbI 3 1 0x24, movI 2 2, strbI 3 2 0x25
    , addI 0 3 0x20, Bl "m_step", Lbl "ret_prog_step"
    , Lit 3 "cells", ldrI 3 0 0x18, ldrI 0 0 0, ldrI 3 1 0x28, Bl "m_mul"
    , Lit 3 "cells", ldrI 3 1 0x1c, addR 0 0 1, ldrI 3 1 0x2c, Bl "m_mul"
    , Lit 3 "cells", strI 3 0 0x50
    , ldrI 3 0 0x1c, ldrI 3 1 0x2c, Bl "m_mul"
    , Lit 3 "cells", ldrI 3 1 0x18, ldrI 1 1 12, bicI 1 1 0x40000000, addR 1 1 0
    , Lit 0 "medium", Lit 3 "cells", ldrI 3 2 0x50, addR 0 0 2
    , ldrI 3 2 0x2c, Bl "m_copy"
    , Lit 3 "cells", ldrI 3 1 0x1c, addI 1 1 1, strI 3 1 0x1c
    , Bra AL "m_program_loop" ]
  , [ Lbl "m_program_verify"
    , ldrI 3 0 0x18, ldrI 0 0 0, Bl "m_block_base", Lbl "ret_fill_verify", Lit 3 "cells", strI 3 0 0x48
    , ldrI 3 1 0x18, ldrI 1 1 12, bicI 1 1 0x40000000, Lit 3 "cells", strI 3 1 0x4c
    , ldrI 3 2 0x30, Bl "m_cmp"
    , cmpI 0 0, Bra CNE "m_program_bad"
    , movI 0 0, Lit 3 "cells", ldrI 3 12 0x5c, ldrI 3 14 0x58, bxLr ]
  , [ Lbl "m_program_bad", movI 0 5, Lit 3 "cells", ldrI 3 12 0x5c, ldrI 3 14 0x58, bxLr ]
  , [ Def "m_block_base", Lit 3 "cells", strI 3 14 0x60, strI 3 0 0x3c
    , ldrI 3 1 0x30, Bl "m_mul"
    , Lit 1 "medium", addR 0 1 0
    , Lit 3 "cells", ldrI 3 14 0x60, bxLr ]
  , [ Def "m_fill", movI 2 0, movwI 3 0xffff, movtI 3 0xffff ]
  , [ Lbl "m_fill_loop", cmpR 2 1, Bra CGE "m_fill_done"
    , strR 0 3 (0, 2), addI 2 2 4, Bra AL "m_fill_loop" ]
  , [ Lbl "m_fill_done", movI 0 0, bxLr ]
  , [ Def "m_copy" ]
  , [ Lbl "m_copy_loop"
    , cmpI 2 0, Bra CEQ "m_copy_done"
    , ldrPost 1 12 4, strPost 0 12 4
    , subI 2 2 4, Bra AL "m_copy_loop" ]
  , [ Lbl "m_copy_done", movI 0 0, bxLr ]
  , [ Def "m_cmp" ]
  , [ Lbl "m_cmp_loop"
    , cmpI 2 0, Bra CEQ "m_cmp_equal"
    , ldrPost 1 3 4, ldrPost 0 12 4, cmpR 12 3, Bra CNE "m_cmp_diff"
    , subI 2 2 4, Bra AL "m_cmp_loop" ]
  , [ Lbl "m_cmp_equal", movI 0 0, bxLr ]
  , [ Lbl "m_cmp_diff", movI 0 1, bxLr ]
  , [ Def "m_mul", movR 2 0, movR 3 1, movI 0 0 ]
  , [ Lbl "m_mul_loop", cmpI 2 0, Bra CEQ "m_mul_done"
    , andI 1 2 1, cmpI 1 0, Bra CEQ "m_mul_skip", addR 0 0 3 ]
  , [ Lbl "m_mul_skip", lsrR 2 2 1, addR 3 3 3, Bra AL "m_mul_loop" ]
  , [ Lbl "m_mul_done", bxLr ]
  ]

modelSymbols :: [(Label, Word32)]
modelSymbols = case assemble modelBase seeded modelCode of
  Right (_, syms) -> syms
  Left err -> error err
  where
    seeded =
      [ ("medium", mediumBase), ("cells", cellsBase), ("journal", journalBase)
      , ("geom_units", geomUnitsFn), ("geom_unit_size", geomUnitSizeFn) ]

modelAddr :: Label -> Word32
modelAddr l = maybe 0 id (lookup l modelSymbols)

-- ---------------------------------------------------------------------------
-- The documented persistence ordering: the step sequence the byte-read writer
-- decode and install-procedure section 8 predict. This is the expectation
-- model; the run checks the stock writer against it.
-- ---------------------------------------------------------------------------

data Step = Step { stKind :: Word32, stUnit :: Word32, stPhys :: Word32 } deriving (Eq, Show)

stepBytes :: Step -> B.ByteString
stepBytes (Step kind unit phys) =
  B.pack [fromIntegral kind, fromIntegral unit, 0, 0] <> word32le phys

data Geom = Geom
  { gUnits :: Word32 -- units per block (device-descriptor row byte, stock 64)
  , gBlocks :: Word32 -- logical blocks the scaled selection rows cover
  , gFirst :: Word32 -- first logical block of the scaled rows
  , gPreErase :: Word32 -- property 0x276 value (S2 pre-erase passes)
  , gTag :: String -- case-id tag for the family
  } deriving Show

blockBytesOf :: Geom -> Word32
blockBytesOf g = gUnits g * unitSize

runSteps :: Geom -> [Step]
runSteps g =
  [Step 1 0xff (gBlocks g - 1)]
  ++ concatMap preErasePass [1 .. gPreErase g]
  ++ concatMap blockSteps [0 .. gBlocks g - 1]
  where
    preErasePass _ = [Step 1 0xff i | i <- [0 .. gBlocks g - 1]]
    blockSteps i = Step 1 0xff i : [Step 2 u i | u <- [0 .. gUnits g - 1]]

remapEntries :: Geom -> [(Word32, Word32)]
remapEntries g =
  [(gFirst g + i, i) | i <- [0 .. gBlocks g - 1]] ++ [(0x125, gBlocks g - 1)]

unitContent :: Geom -> Word32 -> Word32 -> B.ByteString
unitContent _ logical unit
  | logical == 0x125 && unit == 0 = newTrailer <> B.replicate (fromIntegral unitSize - 32) 0xaa
  | otherwise = B.replicate (fromIntegral unitSize) 0xaa

oldMedium :: Geom -> B.ByteString
oldMedium g = B.concat [oldUnit p u | p <- [0 .. gBlocks g - 1], u <- [0 .. gUnits g - 1]]
  where
    oldUnit p u
      | p == gBlocks g - 1 && u == 0 = oldTrailer <> B.replicate (fromIntegral unitSize - 32) 0x55
      | otherwise = B.replicate (fromIntegral unitSize) 0x55

newStaging :: Geom -> B.ByteString
newStaging g = B.concat (map blockWindow [0 .. gBlocks g - 1])
  where
    -- Each block window is 0x20000 (the writer's staging stride); the block's
    -- units sit at the window start and the rest of the window is unused.
    blockWindow i =
      B.concat [unitContent g (gFirst g + i) u | u <- [0 .. gUnits g - 1]]
        <> B.replicate (fromIntegral (0x20000 - gUnits g * unitSize)) 0

replaceAt :: B.ByteString -> Int -> B.ByteString -> B.ByteString
replaceAt bytes off value = B.take bytesAt bytes <> value <> B.drop (bytesAt + B.length value) bytes
  where bytesAt = off

landing :: Geom -> B.ByteString -> Step -> B.ByteString
landing g medium (Step kind unit phys) =
  case kind of
    1 -> replaceAt medium (fromIntegral (phys * blockBytesOf g)) (B.replicate (fromIntegral (blockBytesOf g)) 0xff)
    2 ->
      let logical = gFirst g + phys
          source = unitContent g logical unit
          target = fromIntegral ((phys * gUnits g + unit) * unitSize)
       in replaceAt medium target source
    _ -> medium

changedRuns :: B.ByteString -> B.ByteString -> [(Int, B.ByteString)]
changedRuns old new = go 0
  where
    total = B.length new
    go i
      | i >= total = []
      | B.index old i == B.index new i = go (i + 1)
      | otherwise = (i, B.take (j - i) (B.drop i new)) : go j
      where j = runEnd i
    runEnd i
      | i >= total = i
      | B.index old i /= B.index new i = runEnd (i + 1)
      | otherwise = i

-- Stack words the executed code leaves behind at the guarded stop, derived from
-- the byte-read decode and the model's own push lists. SP0 = stackBase+stackSize.
-- The writer's own push holds its entry registers and its saved LR equals the
-- stack artifact's exit word, so those bytes never change. Everything below is
-- last-write-wins.
frameWords :: Geom -> Maybe Int -> [(Int, Word32)]
frameWords g stop = lastWins (writerWords ++ pathWords)
  where
    sp0 = fromIntegral (stackBase + stackSize) :: Int
    r11w = sp0 - 4
    walkSum = gBlocks g
    steps = runSteps g
    total = length steps
    landed = maybe total id stop
    s2Steps = fromIntegral (gPreErase g * gBlocks g) :: Int
    unitsPlus = 1 + fromIntegral (gUnits g) :: Int
    loopStarted = landed >= 1 + s2Steps
    blocksEntered
      | not loopStarted = 0
      | otherwise = min (fromIntegral (gBlocks g)) ((landed - 1 - s2Steps) `div` unitsPlus + 1)
    writerWords =
      (r11w - 0x54, fromIntegral (cellsBase + 0x10))
        : [(r11w - 0x58, tableBase) | loopStarted]
        ++ [(r11w - 0x50, 0x828574 + gFirst g * 2 + 10 + fromIntegral blocksEntered * 2) | loopStarted]
        ++ concatMap loopBlock [0 .. blocksEntered - 1]
    loopBlock b =
      [ (r11w - 0x2c, 0), (r11w - 0x28, 1000)
      , (r11w - 0x4c, fromIntegral b), (r11w - 0x48, 1)
      , (r11w - 0x44, 0x100), (r11w - 0x40, stagingBase + fromIntegral b * 0x20000 .|. 0x40000000)
      , (r11w - 0x3c, 1), (r11w - 0x34, 1), (r11w - 0x30, 500) ]
    imageBaseOf = stagingBase - gFirst g * 0x20000
    -- S1 chain: writer -> 0x6a9aa4 -> 0x6a9010 -> model erase.
    s1Words =
      [ (sp0 - 104, fromIntegral r11w), (sp0 - 100, 0x006ab594)
      , (sp0 - 124, 0), (sp0 - 120, 0), (sp0 - 116, imageBaseOf)
      , (sp0 - 112, fromIntegral (sp0 - 100)), (sp0 - 108, 0x006a9ac0)
      , (sp0 - 156, 0), (sp0 - 152, 1000)
      , (sp0 - 148, gBlocks g - 1), (sp0 - 144, 1), (sp0 - 136, 0), (sp0 - 132, 1000)
      , (sp0 - 164, 0x3b000394) ] -- the S1 chain's geometry-getter leftover
    -- The stock geometry getters push r11 at sp-4 and restore it; the leftover
    -- word is the live r11 of the stock caller.
    getterLeftover = [(sp0 - 100, fromIntegral r11w) | loopStarted]
    s2Pass
      | landed >= 1 + s2Steps = gPreErase g - 1
      | otherwise = fromIntegral ((landed - 1) `div` fromIntegral (gBlocks g))
    s2Words =
      [ (sp0 - 116, gPreErase g), (sp0 - 112, s2Pass), (sp0 - 108, imageBaseOf)
      , (sp0 - 104, fromIntegral r11w), (sp0 - 100, 0x006ab5d8)
      , (sp0 - 148, 0), (sp0 - 144, 1000)
      , (sp0 - 140, 0), (sp0 - 136, gBlocks g), (sp0 - 128, 0), (sp0 - 124, 1000)
      , (sp0 - 156, 0x3b00039c) ] -- the S2 chain's geometry-getter leftover
    pathWords =
      s1Words
        ++ [w | gPreErase g > 0, landed >= 1, w <- s2Words]
        ++ getterLeftover

lastWins :: [(Int, Word32)] -> [(Int, Word32)]
lastWins = go []
  where
    go acc [] = reverse acc
    go acc ((off, v) : rest) = go ((off, v) : filter ((/= off) . fst) acc) rest

-- The model's cells scratch, per guarded stop. The op function writes its
-- descriptor pointer and geometry at entry; the op block and loop index carry
-- the op identity; the helper calls leave their scratch words.
cellWords :: Geom -> Maybe Int -> [(Int, Word32)]
cellWords g stop = lastWins (common ++ current ++ helperWords)
  where
    steps = runSteps g
    total = length steps
    landed = maybe total id stop
    s2Steps = fromIntegral (gPreErase g * gBlocks g) :: Int
    unitsPlus = 1 + fromIntegral (gUnits g) :: Int
    opIndex = min total (landed + 1) -- the op whose guard ran
    describe j
      | j == 1 = (Nothing, 0, gBlocks g - 1, 0xff, 1) -- S1 erase
      | j <= 1 + s2Steps =
          let i = fromIntegral ((j - 2) `mod` fromIntegral (gBlocks g))
           in (Just 2, i, i, 0xff, 1) -- S2 erase of block i
      | otherwise =
          let q = j - 2 - s2Steps
              b = fromIntegral (q `div` unitsPlus)
              o = q `mod` unitsPlus
           in if o == 0
                then (Just 1, 0, b, 0xff, 1)
                else (Just 1, fromIntegral (o - 1), b, fromIntegral (o - 1), 2)
    (path, index, phys, unit, kind) = describe opIndex
    descPtr = case path of
      Nothing -> 0x3b00036c
      Just 2 -> 0x3b000374
      _ -> 0x3b0003b0
    blocksDone
      | landed < 1 + s2Steps = 0
      | otherwise = min (fromIntegral (gBlocks g)) ((landed - 1 - s2Steps) `div` unitsPlus)
    indexSlot = case stop of
      Nothing -> gUnits g
      Just _
        | opIndex == 1 -> 0
        | opIndex <= 1 + s2Steps -> index
        | (opIndex - 2 - s2Steps) `mod` unitsPlus == 0 ->
            if (opIndex - 2 - s2Steps) `div` unitsPlus == 0
              then if gPreErase g > 0 then gBlocks g else 1
              else gUnits g
        | otherwise -> index
    implReturn = case path of
      Nothing -> entryErase + 8
      Just 2 -> entryErase + 8
      _ -> entryProgram + 8
    common =
      [ (0x00, fromIntegral landed), (0x10, fromIntegral blocksDone)
      , (0x18, descPtr), (0x1c, indexSlot)
      , (0x20, phys), (0x24, unit .|. kind `shiftL` 8)
      , (0x28, gUnits g), (0x2c, unitSize), (0x30, blockBytesOf g)
      , (0x58, if stop == Nothing then entryProgram + 8 else implReturn)
      , (0x5c, if path == Nothing || path == Just 2 then 0x006a9098 else 0x006ab6d8) ]
    current = [] -- the trip point writes nothing beyond the op identity
    helperWords =
      [(0x3c, lastBlockBase)]
        ++ [(0x60, lastFillReturn) | lastBlockBaseRan]
        ++ (if verifyBlocks == 0
              then []
              else
                [ (0x48, mediumBase + (verifyBlocks - 1) * blockBytesOf g)
                , (0x4c, stagingBase + (verifyBlocks - 1) * 0x20000) ])
        ++ [(0x50, lastCopyOffset) | copiedUnits > 0]
    lastBlockBaseRan = case stop of
      Nothing -> True
      Just _ -> not (opIndex == 1)
    lastFillReturn
      | stop == Nothing = modelAddr "ret_fill_verify"
      | opIndex == 1 = 0
      | opIndex <= 1 + s2Steps = modelAddr "ret_fill_erase"
      | otherwise =
          let q = opIndex - 2 - s2Steps
              o = q `mod` unitsPlus
           in if o == 0
                then if q `div` unitsPlus == 0 then modelAddr "ret_fill_erase" else modelAddr "ret_fill_verify"
                else modelAddr "ret_fill_prog"
    lastBlockBase
      | opIndex == 1 = 0
      | opIndex <= 1 + s2Steps =
          let i = fromIntegral ((opIndex - 2) `mod` fromIntegral (gBlocks g)) :: Word32
           in if i == 0 then gBlocks g - 1 else i - 1
      | otherwise =
          let q = opIndex - 2 - s2Steps
              b = fromIntegral (q `div` unitsPlus) :: Word32
              o = q `mod` unitsPlus
           in if o == 0 then (b + gBlocks g - 1) `mod` gBlocks g else b
    copiedUnits
      | stop == Nothing = fromIntegral (gBlocks g * gUnits g)
      | opIndex <= 1 + s2Steps = 0
      | otherwise =
          let q = opIndex - 2 - s2Steps
              b = fromIntegral (q `div` unitsPlus) :: Int
              o = q `mod` unitsPlus
           in b * fromIntegral (gUnits g) + max 0 (o - 1)
    lastCopyOffset =
      let n = copiedUnits - 1
          b = fromIntegral n `div` fromIntegral (gUnits g) :: Word32
          u = fromIntegral n `mod` fromIntegral (gUnits g) :: Word32
       in (b * gUnits g + u) * unitSize
    verifyBlocks
      | stop == Nothing = gBlocks g
      | otherwise = fromIntegral blocksDone

expectedEvents :: Geom -> Int -> [Value]
expectedEvents g landed
  | landed == 0 = []
  | landed < 1 + s2Steps = [stubValue stubStatus, stubValue stubLog]
  | otherwise =
      [stubValue stubStatus, stubValue stubLog, stubValue stubLog]
        ++ replicate windows (stubValue stubWindow)
  where
    s2Steps = fromIntegral (gPreErase g * gBlocks g) :: Int
    unitsPlus = 1 + fromIntegral (gUnits g) :: Int
    windows
      | landed < 1 + s2Steps = 0
      | otherwise = min (fromIntegral (gBlocks g)) ((landed - 1 - s2Steps) `div` unitsPlus + 1)

stubValue :: Word32 -> Value
stubValue address = obj [("address", h address), ("inputs", obj []), ("outputs", obj [("r0", h 0)])]

buildExpected :: Geom -> Maybe Int -> Value
buildExpected g stop =
  let steps = runSteps g
      landed = case stop of
        Nothing -> length steps
        Just k -> k
      landedSteps = take landed steps
      old = oldMedium g
      final = foldl' (landing g) old landedSteps
      mediumRuns = [(off, bytes) | (off, bytes) <- changedRuns old final]
      journalBytes = B.concat (map stepBytes landedSteps)
      memorySpans =
        [("medium", off, bytes) | (off, bytes) <- mediumRuns]
          ++ [("journal", 0, journalBytes) | not (B.null journalBytes)]
          ++ wordSpans "cells" (cellWords g stop)
          ++ wordSpans "stack" [(off - fromIntegral stackBase, v) | (off, v) <- frameWords g stop]
      status = case stop of
        Nothing -> "returned"
        Just _ -> "memory_guard"
      registers = case stop of
        Nothing -> [("r0", h 0)]
        Just _ -> [("r0", h 0x5150)]
   in obj
        [ ("status", s status)
        , ("registers", obj registers)
        , ("memory", arr [obj [("region", s region), ("offset", int off), ("hex", s (hexOf bytes))] | (region, off, bytes) <- memorySpans])
        , ("events", arr (expectedEvents g landed)) ]

-- Consecutive word writes become one span; gaps keep their zero bytes out of the
-- span so undeclared bytes stay equal to the initial memory.
wordSpans :: String -> [(Int, Word32)] -> [(String, Int, B.ByteString)]
wordSpans region words' = map toSpan (groupRuns (sortOn fst words'))
  where
    groupRuns [] = []
    groupRuns ((off, v) : rest) = go off [(off, v)] rest
    go lo acc [] = [(lo, reverse acc)]
    go lo acc ((off, v) : rest)
      | off == fst (head acc) + 4 = go lo ((off, v) : acc) rest
      | otherwise = (lo, reverse acc) : go off [(off, v)] rest
    toSpan (lo, ws) =
      (region, lo, B.concat [word32le v | (_, v) <- ws])

-- ---------------------------------------------------------------------------
-- Case emission.
-- ---------------------------------------------------------------------------

artifactJson :: FilePath -> String -> B.ByteString -> IO Value
artifactJson out name bytes = do
  B.writeFile (out </> name <.> "bin") bytes
  digest <- sha256 bytes
  pure (obj [ ("id", s name), ("path", s (name ++ ".bin"))
            , ("size", int (B.length bytes)), ("sha256", s digest) ])

regionJson :: String -> Word32 -> Word32 -> String -> Value -> Value
regionJson ident addr size perms initial =
  obj [ ("id", s ident), ("address", h addr), ("size", int (fromIntegral size))
      , ("permissions", s perms), ("initial", initial) ]

initialArtifact :: String -> Word32 -> Value
initialArtifact name size = obj [("artifact", s name), ("offset", int 0), ("length", int (fromIntegral size))]

zeroInitial :: Value
zeroInitial = obj [("zero", b True)]

functionJson :: (Word32, Word32) -> Value
functionJson (addr, size) = obj [("address", h addr), ("size", int (fromIntegral size))]

stubJson :: Word32 -> Value
stubJson address =
  obj [ ("address", h address), ("inputs", obj [])
      , ("outputs", obj [("r0", h 0)]), ("side_effects", s "registers-only") ]

writerExclusions :: Geom -> [String]
writerExclusions g =
  [ "Closed Cortex-R5F ARM/little-endian user-mode profile only; no exceptions, MMIO or peripherals, DMA, caches, coprocessors, interrupts, scheduler or hardware timing; the instruction and wall budgets bound the run."
  , "The storage-driver API entries 0x006faf00, 0x006faf40, 0x006faf5c and 0x006fb13c execute declared synthetic media-model code at modelBase, not firmware bytes; erase, program and verify physics, device status semantics, bad blocks, spare behavior and power loss inside a persistence step are device questions this case cannot answer."
  , "The scheduler, log and media-window wrappers 0x00723cb8, 0x00723a14 and 0x0004263c run register-only stubs returning 0; no scheduler, logging or cache-maintenance behavior executes."
  , "Geometry is a scaled case input: units per block " ++ show (gUnits g) ++ " (stock device-descriptor row byte 64), unit size 0x800 and the writer's 0x20000 staging stride stock; the scaled selection rows cover logical blocks 0x123 to 0x125 so the hardcoded trailer erase and the loop's last block name the same physical block."
  , "Interruption cases stop by a guarded store at a declared model step boundary: they model power loss between persistence steps, not inside one; a persistence step that lands is modeled as atomic."
  , "The configuration completion 0x00db91e0 (S4) and the update-session commit (S5) are not executed."
  ]

writerProvenance :: String
writerProvenance =
  "Stock 1.50 writer code from the pinned image-4 decode (sha256 f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16; installed-byte correspondence E-DUMP-RAM-150): 0x006ab4cc block loop, 0x006a9010 erase primitive, 0x006a9aa4 trailer erase, 0x006a90ac map rebuild, 0x0069f8fc property getter, 0x006fbaf0-0x006fbbe4 geometry getters. The four storage-driver API entries carry 8-byte BL trampolines to declared synthetic media-model code; expectations are derived before execution from the byte-read persistence ordering (reports/update-validation-and-boot.md) and the install procedure's section 8 residue table."

data Shared = Shared
  { shArtifacts :: [(String, B.ByteString)]
  , shRegions :: [Value]
  , shFunctions :: [(Word32, Word32)]
  }

buildShared :: Geom -> (B.ByteString, B.ByteString, B.ByteString) -> Shared
buildShared g (modelBytes, sliceA, sliceB) = Shared
  { shArtifacts =
      [ ("id4-slice-writer", sliceA), ("id4-slice-engine", sliceB), ("model", modelBytes)
      , (tag "table", tableBytes), (tag "core", coreBytes), ("idx", idxBytes)
      , ("ready", readyBytes), ("cacheptr", cachePtrBytes), (tag "cache", cacheBytes)
      , (tag "staging", stagingBytes), (tag "medium", mediumBytes), ("stack", stackBytes) ]
  , shRegions =
      [ regionJson "code-writer" 0x0069f8fc (0x006ab750 - 0x0069f8fc) "rx" (initialArtifact "id4-slice-writer" (0x006ab750 - 0x0069f8fc))
      , regionJson "code-engine" entryCheck (0x006fbbe4 - entryCheck) "rx" (initialArtifact "id4-slice-engine" (0x006fbbe4 - entryCheck))
      , regionJson "code-model" modelBase (fromIntegral (B.length modelBytes)) "rx" (initialArtifact "model" (fromIntegral (B.length modelBytes)))
      , regionJson "data-table" tableBase tableSize "rw" (initialArtifact (tag "table") tableSize)
      , regionJson "data-core" coreBase coreSize "rw" (initialArtifact (tag "core") coreSize)
      , regionJson "data-idx" idxAddr 0x10 "rw" (initialArtifact "idx" 0x10)
      , regionJson "data-ready" readyAddr 0x10 "rw" (initialArtifact "ready" 0x10)
      , regionJson "data-cacheptr" cachePtrAddr 0x10 "rw" (initialArtifact "cacheptr" 0x10)
      , regionJson "data-cache" cacheBase 0x280 "rw" (initialArtifact (tag "cache") 0x280)
      , regionJson "staging" stagingBase (fromIntegral (B.length stagingBytes)) "rw" (initialArtifact (tag "staging") (fromIntegral (B.length stagingBytes)))
      , regionJson "medium" mediumBase (fromIntegral (B.length mediumBytes)) "rw" (initialArtifact (tag "medium") (fromIntegral (B.length mediumBytes)))
      , regionJson "journal" journalBase 0x1000 "rw" zeroInitial
      , regionJson "stack" stackBase stackSize "rw" (initialArtifact "stack" stackSize)
      , regionJson "exit" exitAddr 4 "rx" zeroInitial ]
  , shFunctions =
      [ (writerFn, 0x280), (s1Fn, 0x20), (eraseFn, 0x9c), (badMapFn, 0x1bc), (propGetter, 0x24)
      , (geomReadyFn, 0x20), (geomStartFn, 0x44), (geomUnitsFn, 0x48), (geomUnitSizeFn, 0x48)
      , (entryCheck, 16), (entryRelease, 16), (entryProgram, 16), (entryErase, 16) ]
      ++ modelFunctions
  }
  where
    tag name = name ++ "-" ++ gTag g
    tableBytes = B.concat
      [ word32le (gBlocks g - 1), word32le (gFirst g), word32le 0
      , word32le 1, word32le 0x125, word32le 1
      , word32le 0, word32le 0, word32le 2
      , word32le 1, word32le 0, word32le 2
      , word32le 0x100, word32le 0, word32le 2
      , B.replicate (fromIntegral tableSize - 0x3c) 0 ]
    remapBytes = B.concat
      [ word32le 0, word32le 1, word32le 0, word32le 0
      , B.pack (concatMap entry16 [0 .. 313])
      , B.replicate (0x12d4 - 0x284) 0
      , word32le 0, word32le 0, half16 unitSize
      , B.pack [fromIntegral (gUnits g)], B.replicate 13 0
      , B.replicate (fromIntegral coreSize - 0x12ec) 0 ]
    entry16 :: Int -> [Word8]
    entry16 i =
      let v = maybe 0 fromIntegral (lookup (fromIntegral i) (remapEntries g)) :: Int
       in [fromIntegral (v .&. 0xff), fromIntegral ((v `shiftR` 8) .&. 0xff)]
    half16 v = B.pack [fromIntegral (v .&. 0xff), fromIntegral ((v `shiftR` 8) .&. 0xff)]
    coreBytes = remapBytes
    idxBytes = word32le 0 <> B.replicate 12 0
    readyBytes = word32le 1 <> B.replicate 12 0
    cachePtrBytes = word32le cacheBase <> B.replicate 12 0
    cacheBytes = B.replicate 0x275 0 <> B.pack [0, fromIntegral (gPreErase g)] <> B.replicate (0x280 - 0x277) 0
    stagingBytes = newStaging g
    mediumBytes = oldMedium g
    stackBytes = B.replicate (fromIntegral stackSize - 4) 0 <> word32le exitAddr
    modelFunctions =
      let addrs = [a | Def l <- modelCode, let Just a = lookup l modelSymbolsAll]
          ends = tail addrs ++ [modelBase + fromIntegral (B.length modelBytes)]
       in zip addrs (zipWith (-) ends addrs)
    modelSymbolsAll = modelSymbols ++ seededSymbols
    modelSymbols = case assemble modelBase seededSymbols modelCode of
      Right (_, syms) -> syms
      Left err -> error err
    seededSymbols =
      [ ("medium", mediumBase), ("cells", cellsBase), ("journal", journalBase)
      , ("geom_units", geomUnitsFn), ("geom_unit_size", geomUnitSizeFn) ]

emitCase :: FilePath -> Geom -> Shared -> Maybe Int -> IO ()
emitCase out g shared stop = do
  let cid = case stop of
        Nothing -> "writer-" ++ gTag g ++ "-complete"
        Just k -> "writer-" ++ gTag g ++ "-interrupt-" ++ show k
      stopValue = case stop of
        Nothing -> 0xffffffff
        Just k -> fromIntegral k
      cellsBytes = B.concat [word32le 0, word32le stopValue, word32le 0, word32le 0x5150]
  cellsArtifact <- artifactJson out ("cells-" ++ cid) cellsBytes
  artifacts <- mapM (\(name, bytes) -> artifactJson out name bytes) (shArtifacts shared)
  let cellsRegion = regionJson "cells" cellsBase 0x100 "rw" (initialArtifact ("cells-" ++ cid) 16)
      regions = shRegions shared ++ [cellsRegion]
      registers = obj
        ([("r0", h 0), ("r1", h (stagingBase - gFirst g * 0x20000)), ("r2", h (cellsBase + 0x10)), ("r3", h 0)]
         ++ [("r" ++ show i, h 0) | i <- [4 :: Int .. 12]]
         ++ [("sp", h (stackBase + stackSize)), ("lr", h exitAddr), ("cpsr", h 0x10)])
      json = obj
        [ ("schema_version", FujiAF.Host.n 1)
        , ("id", s cid)
        , ("origin", s "stock")
        , ("provenance", s writerProvenance)
        , ("cpu", obj [("architecture", s "arm"), ("model", s "cortex-r5f"), ("mode", s "arm"), ("endian", s "little")])
        , ("artifacts", arr (artifacts ++ [cellsArtifact]))
        , ("entry", h writerFn)
        , ("exit", h exitAddr)
        , ("registers", registers)
        , ("regions", arr regions)
        , ("functions", arr (map functionJson (shFunctions shared)))
        , ("stubs", arr (map stubJson [stubStatus, stubLog, stubWindow]))
        , ("limits", obj [("instructions", FujiAF.Host.n 900000), ("wall_ms", FujiAF.Host.n 5000)])
        , ("expected", buildExpected g stop)
        , ("exclusions", arr (map s (writerExclusions g))) ]
  B.writeFile (out </> cid <.> "json") (encodeCanonical json <> "\n")
  putStrLn ("wrote " ++ cid)

generateWriter :: B.ByteString -> FilePath -> IO ()
generateWriter id4 out = do
  let seeded =
        [ ("medium", mediumBase), ("cells", cellsBase), ("journal", journalBase)
        , ("geom_units", geomUnitsFn), ("geom_unit_size", geomUnitSizeFn) ]
      assembled = assemble modelBase seeded modelCode
      (modelBytes, modelSyms) = case assembled of
        Right ok -> ok
        Left err -> error ("model assembly: " ++ err)
      sliceA = sliceOf id4 0x0069f8fc (0x006ab750 - 0x0069f8fc)
      sliceB0 = sliceOf id4 entryCheck (0x006fbbe4 - entryCheck)
      symbols = modelSyms ++ seeded
      trampoline entry target = case assemble entry symbols [movR 12 14, Bl target, movR 14 12, bxLr] of
        Right (bytes, _) -> bytes
        Left err -> error err
      patchAt slice off bytes = B.take off slice <> bytes <> B.drop (off + B.length bytes) slice
      sliceB = foldl' (\acc (at, bytes) -> patchAt acc (fromIntegral (at - entryCheck)) bytes) sliceB0
        [ (entryCheck, trampoline entryCheck "m_check")
        , (entryRelease, trampoline entryRelease "m_release")
        , (entryProgram, trampoline entryProgram "m_program")
        , (entryErase, trampoline entryErase "m_erase") ]
  putStrLn ("model bytes " ++ show (B.length modelBytes))
  let scaled = Geom 8 3 0x123 0 "scaled"
      preErase = Geom 8 3 0x123 2 "preerase"
      stockCount = Geom 64 1 0x125 0 "stockcount"
      emitFamily g stops = do
        let shared = buildShared g (modelBytes, sliceA, sliceB)
        mapM_ (emitCase out g shared) ([Just k | k <- stops] ++ [Nothing])
  emitFamily scaled [0 .. length (runSteps scaled) - 1]
  emitFamily preErase [0, 1, 1 + fromIntegral (gPreErase preErase) * fromIntegral (gBlocks preErase)]
  emitFamily stockCount [2, 3]

main :: IO ()
main = do
  arguments <- getArgs
  out <- case arguments of
    ["--out", dir] -> pure dir
    _ -> do
      putStrLn "usage: recovery-emul-cases --out DIR"
      exitFailure
  id4 <- readPinned id4Path id4Hash
  createDirectoryIfMissing True out
  -- The output directory belongs to this generator: stale cases from an older
  -- geometry are removed so a regeneration is the complete suite.
  stale <- filter (\f -> takeExtension f `elem` [".json", ".bin"]) <$> listDirectory out
  mapM_ (removeFile . (out </>)) stale
  generateWriter id4 out
