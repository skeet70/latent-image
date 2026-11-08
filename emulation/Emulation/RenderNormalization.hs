{-# LANGUAGE OverloadedStrings #-}
module Emulation.RenderNormalization
  ( NativeState, parseNativeState, nativeValue, OsdFront, parseOsdFront
  , CaptureLabel(..), SourcePlanes, parseSourcePlanes, sourceExtent
  , Rectangle, parseRectangles, normalize ) where

import Codec.Picture (convertRGB8, decodePng, imageWidth, imageHeight, pixelAt)
import Control.Monad (foldM, unless)
import Data.Aeson (Value(..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Parser, withObject, (.:), parseJSON)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import FujiAF.JSON (encodeCanonical)
import Emulation.Sha256 (sha256, sha256Hex)
import Numeric (readHex)

newtype CaptureLabel = CaptureLabel T.Text deriving (Eq, Ord)
newtype Address = Address Integer deriving (Eq)
data Step = Field K.Key | Index Int deriving (Eq)
newtype NativeState = NativeState (Value, [( [Step], Address)])
data FrontIndex = First | Second deriving Eq
newtype IncrementCount = IncrementCount Integer
data OsdFront = OsdFront FrontIndex Address Address IncrementCount
newtype Coordinate = Coordinate Int
data Rectangle = Rectangle Coordinate Coordinate Coordinate Coordinate
data NormalizationRule = MainRelocation | OsdRotation | Clock

ruleName :: NormalizationRule -> T.Text
ruleName MainRelocation = "main-plane"
ruleName OsdRotation = "osd-front"
ruleName Clock = "clock"

type Path = [Step]

get :: Path -> Value -> Maybe Value
get [] value = Just value
get (Field key:rest) (Object fields) = KM.lookup key fields >>= get rest
get (Index index:rest) (Array entries) = entries V.!? index >>= get rest
get _ _ = Nothing

remove :: Path -> Value -> Value
remove [Field key] (Object fields) = Object (KM.delete key fields)
remove (Field key:rest) (Object fields) = Object (case KM.lookup key fields of
  Just value -> KM.insert key (remove rest value) fields
  Nothing -> fields)
remove (Index index:rest) (Array entries) = Array (V.imap (\i value -> if i == index then remove rest value else value) entries)
remove _ value = value

address :: Value -> Parser Address
address (String text) = case T.stripPrefix "0x" text of
  Just digits | not (T.null digits) -> case readHex (T.unpack digits) of
    [(number, "")] | number <= 4294967295 -> pure (Address number)
    _ -> fail "invalid native address"
  _ -> fail "native address requires 0x prefix"
address value = do
  number <- parseJSON value
  unless (number >= 0 && number <= 4294967295) (fail "address out of range")
  pure (Address number)

mainPaths, osdPaths :: [Path]
mainPaths = [[Field "main", Field "source"], [Field "main", Field "source_cpu_alias"],
  [Field "instances", Index 1, Field "registers", Field "0xfff6e02c"]]
osdPaths = [[Field "graphics", Index 0, Field "source"],
  [Field "instances", Index 1, Field "registers", Field "0xfff6e0b4"],
  [Field "instances", Index 1, Field "graphics", Index 1, Field "channels", Index 0, Field "source"]]

parseNativeState :: Value -> Parser NativeState
parseNativeState value@(Object fields) | not (KM.null fields) = do
  entries <- mapM (\path -> traverse (\v -> (path,) <$> address v) (get path value)) (mainPaths ++ osdPaths)
  pure (NativeState (value, [entry | Just entry <- entries]))
parseNativeState _ = fail "native state must be a nonempty object"

nativeValue :: NativeState -> Value
nativeValue (NativeState (value, _)) = value

parseOsdFront :: Value -> Parser OsdFront
parseOsdFront = withObject "OSD front input" $ \fields -> do
  initial <- fields .: "front_byte" :: Parser Integer
  index <- case initial of 0 -> pure First; 1 -> pure Second; _ -> fail "front byte must be 0 or 1"
  buffers <- fields .: "buffers" >>= mapM address
  increments <- fields .: "increments" :: Parser Integer
  unless (increments >= 0 && increments <= 18446744073709551615) (fail "increment count out of range")
  case buffers of
    [a,b] | a /= b -> pure (OsdFront index a b (IncrementCount increments))
    _ -> fail "OSD requires two distinct buffer addresses"

parseRectangles :: Value -> Parser (M.Map CaptureLabel Rectangle)
parseRectangles = withObject "clock exclusions by capture label" $ \fields -> M.fromList <$> mapM entry (KM.toList fields)
  where
    entry (key, value) = (CaptureLabel (K.toText key),) <$> withObject "clock rectangle" (\fields -> do
      x <- fields .: "x" >>= coordinate
      y <- fields .: "y" >>= coordinate
      w <- fields .: "width" >>= coordinate
      h <- fields .: "height" >>= coordinate
      unless (w > 0 && h > 0) (fail "empty clock rectangle")
      pure (Rectangle (Coordinate x) (Coordinate y) (Coordinate w) (Coordinate h))) value
    coordinate value = do
      n <- parseJSON value
      unless (n >= 0 && n <= 4294967295) (fail "rectangle coordinate out of range")
      pure n

data PlaneKind = MainPlane | GraphicsPlane deriving Eq
data Plane = Plane PlaneKind Int Int Address Value

data SourcePlanes = SourcePlanes [Plane] Integer

sourceExtent :: SourcePlanes -> Integer
sourceExtent (SourcePlanes _ extent) = extent

parseSourcePlanes :: Value -> Parser SourcePlanes
parseSourcePlanes (Array entries) | not (V.null entries) = do
  (parsed, extent) <- foldM plane ([], 0) (V.toList entries)
  pure (SourcePlanes (reverse parsed) extent)
  where
    plane (previous, end) value = withObject "source plane" (\fields -> do
      kindText <- fields .: "kind" :: Parser T.Text
      kind <- case kindText of
        "main" -> pure MainPlane
        "graphics" -> do
          _ <- fields .: "layer" >>= unsigned
          _ <- fields .: "channel" >>= unsigned
          pure GraphicsPlane
        _ -> fail "unknown source plane kind"
      offset <- fields .: "offset" >>= unsigned
      size <- fields .: "bytes" >>= unsigned
      sourceNumber <- fields .: "native_source_address" >>= unsigned
      unless (sourceNumber <= 4294967295) (fail "address out of range")
      let source = Address sourceNumber
      unless (offset == end && size > 0 && offset + size <= toInteger (maxBound :: Int)) (fail "invalid source plane extent")
      pure (Plane kind (fromInteger offset) (fromInteger size) source value : previous, offset + size)) value
    unsigned :: Value -> Parser Integer
    unsigned value = do
      number <- parseJSON value
      unless (number >= 0 && number <= 18446744073709551615) (fail "unsigned integer out of range")
      pure number
parseSourcePlanes _ = fail "source_planes must be a nonempty array"

slice :: B.ByteString -> Plane -> B.ByteString
slice bytes (Plane _ offset size _ _) = B.take size (B.drop offset bytes)

front :: OsdFront -> (FrontIndex, Address)
front (OsdFront initial a b (IncrementCount increments)) =
  let index = if even increments then initial else case initial of First -> Second; Second -> First
  in (index, if index == First then a else b)

frontNumber :: FrontIndex -> Int
frontNumber First = 0
frontNumber Second = 1

physical :: Address -> Address
physical (Address n) = Address (n .&. 0x3fffffff)

normalize :: Maybe Rectangle -> Maybe OsdFront -> Maybe OsdFront
  -> (NativeState, NativeState, SourcePlanes, Value, B.ByteString, B.ByteString)
  -> (NativeState, NativeState, SourcePlanes, Value, B.ByteString, B.ByteString)
  -> ([(K.Key, Bool)], [Value])
normalize rectangle leftFront rightFront (lf, ls, lp, lx, lb, lpng) (rf, rs, rp, rx, rb, rpng) =
  ([ ("png_bytewise_equal", if clockApplied then pixelOutside else lpng == rpng)
   , ("source_planes_bytewise_equal", sourceEqual)
   , ("native_frame_state_equal", stripped lf == stripped rf)
   , ("native_scanout_state_equal", stripped ls == stripped rs)
   , ("source_planes_equal", planeMetadata leftPlanes == planeMetadata rightPlanes)
   , ("pixels_equal", if clockApplied then pixelOutside && pixelShape lx == pixelShape rx else lx == rx)
   ], fieldRecords ++ planeRecords ++ clockRecords)
  where
    SourcePlanes leftPlanes _ = lp
    SourcePlanes rightPlanes _ = rp
    paired = zip leftPlanes rightPlanes
    mainEqual = not (null mains) && all (\(a,b) -> slice lb a == slice rb b) mains
    mains = [(a,b) | (a@(Plane MainPlane _ _ _ _),b@(Plane MainPlane _ _ _ _)) <- paired]
    osdAllowed = case (leftFront, rightFront) of
      (Just a@(OsdFront _ a0 a1 _), Just b@(OsdFront _ b0 b1 _)) ->
        a0 == b0 && a1 == b1 && fst (front a) /= fst (front b) && relation a ls && relation b rs
      _ -> False
    relation input state = lookup [Field "instances", Index 1, Field "registers", Field "0xfff6e0b4"] (addresses state) == Just (physical (snd (front input)))
    addresses (NativeState (_, entries)) = entries
    osdAddress value = case leftFront of
      Just (OsdFront _ a b _) -> value `elem` [a, b, physical a, physical b]
      Nothing -> False
    permitted path a b = (path `elem` mainPaths && mainEqual) ||
      (path `elem` osdPaths && osdAllowed && osdAddress a && osdAddress b)
    changed a b = [(path,x,y) | (path,x) <- addresses a, Just y <- [lookup path (addresses b)],
      get path (nativeValue a) /= get path (nativeValue b), permitted path x y]
    changes = changed lf rf ++ changed ls rs
    stripped state = encodeCanonical (foldr (remove . (\(path,_,_) -> path)) (nativeValue state) changes)
    fieldRecords = [record (if path `elem` mainPaths then MainRelocation else OsdRotation) (prefix <> "." <> pathName path)
      (maybe Null id (get path (nativeValue a))) (maybe Null id (get path (nativeValue b)))
      | (prefix,a,b) <- [("native_frame_state",lf,rf),("native_scanout_state",ls,rs)], (path,_,_) <- changed a b]
    pathName = T.intercalate "." . map (\step -> case step of Field key -> K.toText key; Index index -> T.pack (show index))
    planeAllowed kind a b = a /= b && ((kind == MainPlane && mainEqual) || (kind == GraphicsPlane && osdAllowed && osdAddress a && osdAddress b))
    planeMetadata ps = encodeCanonical (Array (V.fromList [if any (matches p) paired then remove [Field "native_source_address"] value else value | p@(Plane _ _ _ _ value) <- ps]))
    matches (Plane k o n _ _) (Plane k1 o1 n1 a _, Plane k2 o2 n2 b _) = k == k1 && k1 == k2 && o == o1 && o1 == o2 && n == n1 && n1 == n2 && planeAllowed k a b
    planeRecords = [record (if kind == MainPlane then MainRelocation else OsdRotation) ("source_planes." <> T.pack (show index) <> ".native_source_address") (number a) (number b)
      | (index,(Plane kind _ _ a _, Plane other _ _ b _)) <- zip [0 :: Int ..] paired, kind == other, planeAllowed kind a b]
    number (Address n) = Number (fromInteger n)
    sourceEqual = length leftPlanes == length rightPlanes && and [if clockApplied && kind == GraphicsPlane && other == GraphicsPlane then outsideBytes a b else slice lb a == slice rb b
      | (a@(Plane kind _ _ _ _),b@(Plane other _ _ _ _)) <- paired]
    clockApplied = case rectangle of
      Just _ -> any (\(a,b) -> slice lb a /= slice rb b) [(a,b) | (a@(Plane GraphicsPlane _ _ _ _),b@(Plane GraphicsPlane _ _ _ _)) <- paired]
      Nothing -> False
    pixelShape = remove [Field "rgb_sha256"]
    dimensions = do
      Number w <- get [Field "width"] lx
      Number h <- get [Field "height"] lx
      pure (floor w, floor h)
    inside x y = case rectangle of
      Just (Rectangle (Coordinate left) (Coordinate top) (Coordinate width) (Coordinate height)) -> x >= left && x < left + width && y >= top && y < top + height
      Nothing -> False
    validRect w h = case rectangle of
      Just (Rectangle (Coordinate x) (Coordinate y) (Coordinate width) (Coordinate height)) -> x + width <= w && y + height <= h
      Nothing -> False
    outsideBytes a b = case dimensions of
      Just (w,h) -> let l = slice lb a; r = slice rb b in validRect w h && B.length l == w*h*4 && B.length r == w*h*4 && and
        [B.take 4 (B.drop ((y*w+x)*4) l) == B.take 4 (B.drop ((y*w+x)*4) r) | y <- [0..h-1], x <- [0..w-1], not (inside x y)]
      Nothing -> False
    pixelOutside = case (decodePng lpng, decodePng rpng, dimensions) of
      (Right a, Right b, Just (w,h)) -> let l = convertRGB8 a; r = convertRGB8 b in validRect w h && imageWidth l == w && imageHeight l == h && imageWidth r == w && imageHeight r == h && and
        [pixelAt l x y == pixelAt r x y | y <- [0..h-1], x <- [0..w-1], not (inside x y)]
      _ -> False
    clockRecords = case rectangle of
      Just (Rectangle (Coordinate x) (Coordinate y) (Coordinate w) (Coordinate h)) | clockApplied && pixelOutside && sourceEqual ->
        [object ["rule" .= ruleName Clock, "rectangle" .= object ["x" .= x,"y" .= y,"width" .= w,"height" .= h],
          "fields" .= (["png", "pixels.rgb_sha256", "graphics.source_bytes"] :: [T.Text]),
          "reference" .= clockValues lx lb lpng, "candidate" .= clockValues rx rb rpng]]
      _ -> []
    clockValues pixelMetadata source png = object ["pixels" .= pixelMetadata,
      "source_bytes_sha256" .= sha256Hex (sha256 source), "png_sha256" .= sha256Hex (sha256 png)]
    record :: NormalizationRule -> T.Text -> Value -> Value -> Value
    record rule field a b = object (["rule" .= ruleName rule, "field" .= field, "reference" .= a, "candidate" .= b]
      ++ case rule of
        MainRelocation -> []
        Clock -> []
        OsdRotation -> ["osd_front" .= object ["reference" .= frontValue leftFront, "candidate" .= frontValue rightFront]])
    frontValue Nothing = Null
    frontValue (Just input@(OsdFront initial a b (IncrementCount increments))) = object
      ["front_byte" .= (if initial == First then 0 :: Int else 1), "buffers" .= [number a,number b], "increments" .= increments,
       "capture_front_byte" .= frontNumber (fst (front input))]
