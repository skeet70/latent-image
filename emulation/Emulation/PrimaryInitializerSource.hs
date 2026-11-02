{-# LANGUAGE OverloadedStrings #-}
module Emulation.PrimaryInitializerSource
  ( Guest (..), Physical (..), ByteCount (..), ContainerOffset (..)
  , Source, Geometry (..), CallerFrame (..), Publication (..), ReturnValues (..), RootPair (..)
  , parseSource, sourceDigest, sourceLoaderDigest, sourceLoader, sourceGeometry, sourceBytes, sourceContainer
  , sourceCaller, sourcePublications, sourceReturn, sourceRoots, sourceValue, publications
  ) where

import qualified A32 as A
import Control.Monad (forM, forM_, unless)
import Data.Aeson (Value, object, (.=))
import Data.Bits (complement, shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.List (sort)
import Data.Word (Word32, Word64)
import Emulation.FirstStageWitness (leWordAt, sha256Hex)
import qualified Emulation.InterfaceInitializer as I
import qualified FujiAF.FirstStage as F
import qualified FujiAF.UpdateContainer as U
import qualified StartupStageContract as S

newtype Guest = Guest Word32 deriving (Eq, Ord, Show)
newtype Physical = Physical Word64 deriving (Eq, Ord, Show)
newtype ByteCount = ByteCount Word32 deriving (Eq, Ord, Show)
newtype ContainerOffset = ContainerOffset Word64 deriving (Eq, Ord, Show)

data Geometry = Geometry
  { geometryLoad :: Guest
  , geometryBytes :: ByteCount
  , geometryContainerStart :: ContainerOffset
  , geometryFlash :: Physical
  } deriving (Eq, Show)

data CallerFrame = CallerFrame
  { callerTop :: Guest
  , callerSp :: Guest
  , callerFp :: Guest
  } deriving (Eq, Show)

data Publication = Publication
  { publicationGuest :: Guest
  , publicationValue :: Word32
  , publicationPc :: Guest
  } deriving (Eq, Show)

data ReturnValues = ReturnValues
  { returnR0 :: Guest, returnR1 :: Guest, returnR2 :: Guest, returnR3 :: Guest
  , returnR12 :: Word32
  } deriving (Eq, Show)

data RootPair = RootPair { primaryRoot :: Guest, secondaryRoot :: Guest } deriving (Eq, Show)

data CanonicalContainer = CanonicalContainer B.ByteString U.Parsed
data Source = Source F.Sha256 F.Sha256 B.ByteString Geometry CallerFrame [Publication] ReturnValues RootPair CanonicalContainer

sourceDigest :: Source -> F.Sha256
sourceDigest (Source value _ _ _ _ _ _ _ _) = value

sourceLoaderDigest :: Source -> F.Sha256
sourceLoaderDigest (Source _ value _ _ _ _ _ _ _) = value

sourceLoader :: Source -> B.ByteString
sourceLoader (Source _ _ value _ _ _ _ _ _) = value

sourceGeometry :: Source -> Geometry
sourceGeometry (Source _ _ _ value _ _ _ _ _) = value

sourceCaller :: Source -> CallerFrame
sourceCaller (Source _ _ _ _ value _ _ _ _) = value

sourcePublications :: Source -> [Publication]
sourcePublications (Source _ _ _ _ _ value _ _ _) = value

sourceReturn :: Source -> ReturnValues
sourceReturn (Source _ _ _ _ _ _ value _ _) = value

sourceRoots :: Source -> RootPair
sourceRoots (Source _ _ _ _ _ _ _ value _) = value

sourceBytes :: Source -> B.ByteString
sourceBytes (Source _ _ _ _ _ _ _ _ (CanonicalContainer bytes _)) = bytes

sourceContainer :: Source -> U.Parsed
sourceContainer (Source _ _ _ _ _ _ _ _ (CanonicalContainer _ parsed)) = parsed

require :: Bool -> String -> Either String ()
require condition message = unless condition (Left message)

parseSource :: B.ByteString -> Either String Source
parseSource bytes = do
  original <- I.parseOriginalSource bytes
  let parsed = S.originalStartupParsed original
      parts = U.parsedParts parsed
      carrier = U.partPreTable parts
      segment ident = case filter ((== ident) . U.segId) (U.partSegments parts) of
        [found] -> Right found
        _ -> Left "primary source loader segment is not unique"
  primary <- segment 0
  secondary <- segment 1
  let load = U.segLoad primary
      size = U.segSize primary
      flash = U.segFlash primary
      start = toInteger flash
      end = start + toInteger size
      absolute = toInteger U.stagingLength + toInteger (U.hMainOffset (U.parsedHeader parsed)) + start
  require (load == 0x40000 && size == 0xdae0 && U.segFlashLen primary >= size
    && end <= toInteger (B.length carrier) && absolute + toInteger size <= toInteger (B.length bytes))
    "primary source loader geometry leaves its declared container extent"
  let loader = B.map complement (B.take (fromIntegral size) (B.drop (fromIntegral flash) carrier))
  digest <- F.parseSha256 (sha256Hex loader)
  canonical <- F.parseSha256 "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
  require (digest == canonical) "primary source requires the canonical whole loader A"
  primaryHeader <- leWordAt loader 0x10
  let secondaryHeaderOffset = toInteger (U.segFlash secondary) + 0x10
  require (U.segLoad secondary == 0xd0000 && U.segSize secondary >= 0x14
    && secondaryHeaderOffset + 4 <= toInteger (B.length carrier)) "secondary source header leaves its segment"
  secondaryHeader <- leWordAt (B.map complement (B.take 4 (B.drop (fromInteger secondaryHeaderOffset) carrier))) 0
  require (primaryHeader == 0x4004dae0 && secondaryHeader == 0x400ddaa0) "primary and independent secondary source roots differ"
  frame <- callerFrame loader
  (stores, returned) <- publications loader
  require (length stores == 1419
    && sort (map publicationGuest stores) == map Guest [0x4004dae0,0x4004dae4 .. 0x4004f108])
    "primary source publication does not cover its complete table exactly once"
  pure (Source (S.originalStartupDigest original) digest loader
    (Geometry (Guest load) (ByteCount size) (ContainerOffset (fromInteger absolute)) (Physical (fromIntegral flash)))
    frame stores returned (RootPair (Guest primaryHeader) (Guest secondaryHeader)) (CanonicalContainer bytes parsed))

callerFrame :: B.ByteString -> Either String CallerFrame
callerFrame loader = do
  load <- A.decode 0x400a8 <$> leWordAt loader 0xa8
  offset <- case load of
    A.Load (A.Cond 14) A.Word False (A.Reg 4) base (A.OffsetImmediate amount) A.OffsetOnly | base == A.pc -> Right amount
    _ -> Left "primary source stack literal load differs"
  let literal = 0xb0 + toInteger offset
  require (literal >= 0 && literal + 4 <= toInteger (B.length loader)) "primary source stack literal leaves loader A"
  top <- leWordAt loader (fromInteger literal)
  forM_ [(0xac,0xe1a0d004),(0x9ac,0xe92d4830),(0x9b0,0xe28db00c),(0x9b4,0xe1a04000)] $ \(site,word) ->
    require (leWordAt loader site == Right word) "primary source caller frame premise differs"
  forM_ [0x409dc,0x40aac] $ \site -> do
    call <- A.decode site <$> leWordAt loader (fromIntegral (site - 0x40000))
    require (call == A.Branch (A.Cond 14) True 0x40430) "primary source direct caller does not target the retained leaf"
  require (top == 0x40063460) "primary source caller stack top differs"
  pure (CallerFrame (Guest top) (Guest (top - 16)) (Guest (top - 4)))

publications :: B.ByteString -> Either String ([Publication], ReturnValues)
publications loader = do
  let wide register low high = do
        lowInstruction <- A.decode (0x40000 + fromIntegral low) <$> leWordAt loader low
        highInstruction <- A.decode (0x40000 + fromIntegral high) <$> leWordAt loader high
        case (lowInstruction,highInstruction) of
          (A.MoveWide (A.Cond 14) destination lower,A.MoveTop (A.Cond 14) upperDestination upper)
            | destination == register && upperDestination == register -> Right (lower .|. (upper `shiftL` 16))
          _ -> Left "primary source wide publication literal differs"
      store offset value pc = Publication (Guest (0x4004dae0 + offset)) value (Guest pc)
  root <- wide (A.Reg 3) 0x438 0x43c
  require (root == 0x4004dae0) "primary source initializer root differs"
  services <- forM [0 .. 93 :: Int] $ \index -> do
    let offset = 0x440 + 12 * index
    value <- wide (A.Reg 2) offset (offset + 4)
    require (leWordAt loader (offset + 8) == Right (0xe5832000 + fromIntegral index * 4))
      "primary source service or tick store differs"
    pure (store (fromIntegral index * 4) value (0x40000 + fromIntegral offset + 8))
  require (publicationValue (last services) == 0x400c0800) "primary source tick pointer differs"
  extensions <- forM [0 .. 4 :: Int] $ \index -> do
    let offset = 0x8a8 + index * 16
    value <- wide (A.Reg 1) offset (offset + 8)
    require (leWordAt loader (offset + 12) == Right 0xe7831002) "primary source extension store differs"
    pure (store (0x1618 + fromIntegral index * 4) value (0x40000 + fromIntegral offset + 12))
  pool <- wide (A.Reg 0) 0x8fc 0x900
  extended <- wide (A.Reg 5) 0x904 0x908
  countInstruction <- A.decode 0x40998 <$> leWordAt loader 0x998
  count <- case countInstruction of
    A.DataProcessing (A.Cond 14) A.CMP True _ (A.Reg 12) (A.Immediate value) | value == 120 -> Right value
    _ -> Left "primary source descriptor bound differs"
  let sites = [0x40918,0x40928,0x40930,0x40940,0x40948,0x40958,0x40960,0x40970,0x40978,0x40988,0x40990]
      descriptor index = zipWith3 (\field value pc -> store (0x178 + index * 44 + field * 4) value pc)
        [0..] ([index + 1] ++ [pool + index * 40 + field * 4 | field <- [0..7]]
          ++ [extended + index * 56 + 24,extended + index * 56 + 28]) sites
      returned = ReturnValues (Guest pool) (Guest (pool + (count - 1) * 40 + 28))
        (Guest (extended + (count - 1) * 56 + 28)) (Guest (root + count * 44)) count
  require (all (\(Publication (Guest guest) _ _) -> guest .&. 3 == 0) (services ++ extensions))
    "primary source publication alignment differs"
  pure (services ++ extensions ++ concatMap descriptor [0 .. count - 1],returned)

sourceValue :: Source -> Value
sourceValue source =
  let Geometry (Guest load) (ByteCount bytes) (ContainerOffset container) (Physical flash) = sourceGeometry source
      CallerFrame (Guest top) (Guest sp) (Guest fp) = sourceCaller source
      RootPair (Guest primary) (Guest secondary) = sourceRoots source
   in object
      [ "source_sha256" .= F.sha256Hex (sourceDigest source), "loader_a_sha256" .= F.sha256Hex (sourceLoaderDigest source)
      , "segment0" .= object ["load" .= load,"bytes" .= bytes,"flash" .= flash,"container_start" .= container]
      , "caller_frame_premise" .= object ["stack_top" .= top,"sp" .= sp,"fp" .= fp]
      , "roots" .= object ["primary" .= primary,"secondary" .= secondary]
      , "known_direct_calls" .= [object ["call" .= site,"return" .= (site + 4)] | site <- [0x409dc,0x40aac :: Word32]]
      , "caller_closure" .= ("alternate-and-computed-references-open" :: String)
      , "publication_words" .= length (sourcePublications source)
      ]
