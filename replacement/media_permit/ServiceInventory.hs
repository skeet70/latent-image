{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Shared parts of the X-M1 1.50 service manifests: reviewed contract
-- parsing, a linear candidate census over digest-pinned images, and
-- StockInventory extents for listed entries.
module ServiceInventory
  ( EntryAddress (..)
  , Command (..)
  , Site (..)
  , SendFunction (..)
  , Destination (..)
  , Recovery (..)
  , Code (..)
  , Entry (..)
  , SharedObject (..)
  , Image (..)
  , Form (..)
  , formLabel
  , Census (..)
  , hex
  , digest
  , hexWord
  , exactKeys
  , kebab
  , parseEntry
  , parseObject
  , checkContracts
  , imageOf
  , imageWord
  , census
  , lastFence
  , entryValue
  , entryExtent
  , objectValue
  , residualsValue
  , inputValue
  ) where

import qualified A32 as A
import Control.Monad (unless, when)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value, object, withObject, (.:), (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import Data.Aeson.Types (Parser)
import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import Data.Char (isAsciiLower, isDigit, isHexDigit)
import Data.List (find, nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust, isNothing)
import qualified Data.Set as S
import Data.Word (Word32)
import Numeric (readHex, showHex)
import StockInventory (FunctionDeclaration (..), FunctionDerivation (..), FunctionSize (..), FunctionWindow (..), GuestAddress (..), deriveFunction, returnPremiseLabel, savedStackTaintResidualLabel, savedStackTaintResidualStatement, xm1TaskRecords)

newtype EntryAddress = EntryAddress Word32 deriving (Eq, Ord, Show)
-- | A message or command code an entry serves or sends.
newtype Command = Command Word32 deriving (Eq, Ord, Show)
newtype Site = Site Word32 deriving (Eq, Ord, Show)
-- | A message-send function whose BL sites are censused.
newtype SendFunction = SendFunction Word32 deriving (Eq, Ord, Show)
-- | The destination constant a send site loads into r0.
newtype Destination = Destination Word32 deriving (Eq, Ord, Show)

data Recovery = Recovered | Partial | Unknown deriving (Eq, Show, Enum, Bounded)

recoveryLabel :: Recovery -> String
recoveryLabel recovery = case recovery of
  Recovered -> "recovered"
  Partial -> "partial"
  Unknown -> "unknown"

data Code = Code Int String

data Entry f = Entry
  { entryAddress :: EntryAddress
  , entryName :: String
  , entryFamily :: f
  , entryRecovery :: Recovery
  , entryMessage :: Maybe Command
  , entryOperation :: String
  , entryArguments :: String
  , entryResult :: String
  , entryCodes :: [Code]
  , entryContext :: String
  , entryObjects :: [String]
  , entryCallers :: String
  , entryGrounded :: [String]
  , entryUnknown :: [String]
  , entryEvidence :: [String]
  }

-- | A reviewed shared object; constructed addresses inside it are censused.
data SharedObject = SharedObject
  { objectName :: String
  , objectAddress :: Word32
  , objectBytes :: Word32
  , objectLayout :: String
  , objectGrounded :: [String]
  , objectUnknown :: [String]
  , objectEvidence :: [String]
  }

data Image = Image
  { imageLabel :: String
  , imagePath :: FilePath
  , imageBase :: Word32
  , imageBytes :: B.ByteString
  }

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

digest :: B.ByteString -> String
digest = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack . SHA256.hash

hexWord :: String -> String -> Parser Word32
hexWord what text = case stripPrefix "0x" text of
  Just digits | not (null digits) && length digits <= 8 && all isHexDigit digits, [(number, "")] <- readHex digits -> pure number
  _ -> fail (what ++ " is not 0x-prefixed 32-bit hexadecimal: " ++ text)

exactKeys :: String -> [String] -> KeyMap.KeyMap Value -> Parser ()
exactKeys what expected o = do
  let keys = sort (map Key.toString (KeyMap.keys o))
  unless (keys == sort expected) (fail (what ++ " keys differ from the contract fields: " ++ unwords keys))

nonEmpty :: String -> [a] -> Parser ()
nonEmpty what values = when (null values) (fail (what ++ " has no evidence"))

-- | Lowercase alphanumeric words joined by single hyphens.
kebab :: String -> Bool
kebab name = not (null parts) && all (\part -> not (null part) && all (\c -> isAsciiLower c || isDigit c) part) parts
  where
    parts = split name
    split text = case break (== '-') text of
      (word, []) -> [word]
      (word, _ : rest) -> word : split rest

labelled :: (Enum a, Bounded a) => String -> (a -> String) -> String -> Parser a
labelled what label text = case [value | value <- [minBound .. maxBound], label value == text] of
  [value] -> pure value
  _ -> fail ("unknown " ++ what ++ " " ++ text)

parseEntry :: (Enum f, Bounded f) => (f -> String) -> Value -> Parser (Entry f)
parseEntry familyLabel = withObject "entry" $ \o -> do
  exactKeys "entry" ["address", "name", "family", "recovery", "message", "operation", "arguments", "result", "codes", "context", "objects", "callers", "grounded", "unknown", "evidence"] o
  address <- o .: "address" >>= hexWord "entry address"
  name <- o .: "name"
  unless (kebab name) (fail ("entry name is not kebab-case: " ++ name))
  family <- o .: "family" >>= labelled "family" familyLabel
  recovery <- o .: "recovery" >>= labelled "recovery" recoveryLabel
  message <- o .: "message" >>= maybe (pure Nothing) (fmap (Just . Command) . hexWord "message code")
  codes <- o .: "codes" >>= mapM (withObject "code" (\c -> do
    exactKeys "code" ["value", "meaning"] c
    Code <$> c .: "value" <*> c .: "meaning"))
  citations <- o .: "evidence"
  nonEmpty ("entry " ++ name) citations
  Entry (EntryAddress address) name family recovery message
    <$> o .: "operation" <*> o .: "arguments" <*> o .: "result" <*> pure codes <*> o .: "context"
    <*> o .: "objects" <*> o .: "callers" <*> o .: "grounded" <*> o .: "unknown" <*> pure citations

parseObject :: Value -> Parser SharedObject
parseObject = withObject "object" $ \o -> do
  exactKeys "object" ["name", "address", "bytes", "layout", "grounded", "unknown", "evidence"] o
  name <- o .: "name"
  address <- o .: "address" >>= hexWord "object address"
  bytes <- o .: "bytes"
  when (bytes == 0 || toInteger address + toInteger bytes > 0x100000000) (fail ("object " ++ name ++ " is empty or leaves the 32-bit address space"))
  citations <- o .: "evidence"
  nonEmpty ("object " ++ name) citations
  SharedObject name address bytes <$> o .: "layout" <*> o .: "grounded" <*> o .: "unknown" <*> pure citations

imageWord :: Image -> Word32 -> Maybe Word32
imageWord (Image _ _ base image) address
  | address < base || address .&. 3 /= 0 || toInteger address + 4 > toInteger base + toInteger (B.length image) = Nothing
  | otherwise = Just (foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index image (fromIntegral (address - base) + index))) 0 [0 .. 3])

-- | The image holding an aligned word at the address.
imageOf :: [Image] -> Word32 -> Maybe Image
imageOf images address = find (\image -> imageWord image address /= Nothing) images

-- | Unique names and addresses; pairs each entry with the listed image holding its aligned word.
checkContracts :: [Image] -> [Entry f] -> [SharedObject] -> Either String [(Entry f, Image)]
checkContracts images entries objects = do
  let addresses = [address | Entry {entryAddress = EntryAddress address} <- entries]
  unless (length (nub addresses) == length addresses) (Left "contract input repeats an entry address")
  unless (length (nub (map entryName entries)) == length entries) (Left "contract input repeats an entry name")
  unless (length (nub (map objectName objects)) == length objects) (Left "contract input repeats an object name")
  case [entryName entry | entry@Entry {entryAddress = EntryAddress address} <- entries, isNothing (imageOf images address)] of
    [] -> Right [(entry, image) | entry@Entry {entryAddress = EntryAddress address} <- entries, Just image <- [imageOf images address]]
    outside -> Left ("entries outside every listed image: " ++ unwords outside)

-- | How a censused value reaches a register.
data Form = MovwForm | MovImmediateForm | LiteralForm | MovwMovtForm deriving (Eq, Ord, Show)

formLabel :: Form -> String
formLabel form = case form of
  MovwForm -> "movw"
  MovImmediateForm -> "mov"
  LiteralForm -> "ldr-literal"
  MovwMovtForm -> "movw-movt"

data Decoded = Decoded Site Word32 A.Instruction

decodeImage :: Image -> [Decoded]
decodeImage image@(Image _ _ base bytes) = [Decoded (Site address) raw (A.decode address raw) | address <- [base, base + 4 .. base + fromIntegral (B.length bytes) - 4], Just raw <- [imageWord image address]]

-- | Value loaded by a PC-relative word LDR.
literalLoad :: Image -> Decoded -> Maybe (A.Reg, Word32)
literalLoad image (Decoded (Site address) _ instruction) = case instruction of
  A.Load _ A.Word False destination base (A.OffsetImmediate offset) A.OffsetOnly
    | base == A.pc && destination /= A.pc -> (,) destination <$> imageWord image (fromIntegral (toInteger address + 8 + toInteger offset))
  _ -> Nothing

unconditionalTransfer :: A.Instruction -> Bool
unconditionalTransfer instruction = case instruction of
  A.Branch (A.Cond 14) False _ -> True
  A.BranchExchange (A.Cond 14) False _ -> True
  A.LoadMultiple (A.Cond 14) _ _ _ registers -> A.pc `elem` registers
  A.Load (A.Cond 14) _ _ destination _ _ _ -> destination == A.pc
  A.DataProcessing (A.Cond 14) _ _ destination _ _ -> destination == A.pc
  _ -> False

-- | Addresses built by a MOVW and a later MOVT of the same register within
-- eight words, with no intervening MOVW of that register or unconditional transfer.
constructedAddresses :: [Decoded] -> [(Site, Word32)]
constructedAddresses decoded = go decoded
  where
    go [] = []
    go (Decoded site _ (A.MoveWide _ register low) : rest) =
      let ends (Decoded _ _ instruction) = case instruction of
            A.MoveWide _ other _ -> other == register
            _ -> unconditionalTransfer instruction
      in case [high | Decoded _ _ (A.MoveTop _ top high) <- takeWhile (not . ends) (take 8 rest), top == register] of
        high : _ -> (site, (high `shiftL` 16) .|. low) : go rest
        [] -> go rest
    go (_ : rest) = go rest

-- | Registers an instruction writes; a BL writes r0.
destinations :: A.Instruction -> [A.Reg]
destinations instruction = case instruction of
  A.DataProcessing _ op _ destination _ _ | op `notElem` [A.TST, A.TEQ, A.CMP, A.CMN] -> [destination]
  A.MoveWide _ destination _ -> [destination]
  A.MoveTop _ destination _ -> [destination]
  A.ReadStatus _ destination -> [destination]
  A.ZeroExtend _ destination _ _ _ -> [destination]
  A.Multiply _ destination _ _ _ -> [destination]
  A.Compute _ written -> written
  A.Load _ _ _ destination base _ indexing -> destination : [base | indexing /= A.OffsetOnly]
  A.Store _ _ _ base _ indexing | indexing /= A.OffsetOnly -> [base]
  A.LoadMultiple _ base _ writeback registers -> registers ++ [base | writeback]
  A.StoreMultiple _ base _ writeback _ | writeback -> [base]
  A.Branch _ True _ -> [A.Reg 0]
  A.BranchExchange _ True _ -> [A.Reg 0]
  A.Rejected _ -> [A.Reg 0]
  _ -> []

-- | The immediate of the nearest preceding r0 write when that write is an
-- unconditional MOV immediate or MOVW, within six words and not across an
-- unconditional transfer.
r0Constant :: [Decoded] -> Maybe Destination
r0Constant previous = case [instruction | Decoded _ _ instruction <- takeWhile (\(Decoded _ _ i) -> not (unconditionalTransfer i)) (take 6 previous), A.Reg 0 `elem` destinations instruction] of
  A.DataProcessing (A.Cond 14) A.MOV _ _ _ (A.Immediate value) : _ -> Just (Destination value)
  A.MoveWide (A.Cond 14) _ value : _ -> Just (Destination value)
  _ -> Nothing

data Census = Census
  { censusFences :: S.Set Word32
  , directCalls :: M.Map Word32 [Site]
  , branches :: M.Map Word32 [Site]
  , literalPositions :: M.Map Word32 [Site]
  , codeSites :: [(Site, Form, A.Reg, Command)]
  , addressSites :: M.Map Word32 [(Site, Form)]
  , sendSites :: [(Site, Maybe Destination)]
  }

-- | Linear census over every listed image. `codeFamily` selects censused
-- message codes; `send` names a function whose BL sites record their r0 constant.
census :: [Image] -> [Word32] -> (Word32 -> Bool) -> Maybe SendFunction -> Census
census images extraStarts codeFamily send =
  let perImage = [(image, decodeImage image) | image <- images]
      decoded = concatMap snd perImage
      inImage target = isJust (imageOf images target)
      calls = M.fromListWith (flip (++)) [(target, [site]) | Decoded site _ (A.Branch _ True target) <- decoded, inImage target]
      jumps = M.fromListWith (flip (++)) [(target, [site]) | Decoded site _ (A.Branch _ False target) <- decoded, inImage target]
      literals = M.fromListWith (flip (++)) [(raw, [site]) | Decoded site raw _ <- decoded, inImage raw]
      constructed = concatMap (constructedAddresses . snd) perImage
      paired = S.fromList (map fst constructed)
      movw = [(site, MovwForm, register, Command value) | Decoded site _ (A.MoveWide _ register value) <- decoded, codeFamily value, S.notMember site paired]
      movImmediate = [(site, MovImmediateForm, register, Command value) | Decoded site _ (A.DataProcessing _ A.MOV _ register _ (A.Immediate value)) <- decoded, codeFamily value]
      pooled = [(site, LiteralForm, register, Command value) | (image, items) <- perImage, item@(Decoded site _ _) <- items, Just (register, value) <- [literalLoad image item], codeFamily value]
      pooledAddresses = [(site, LiteralForm, value) | (image, items) <- perImage, item@(Decoded site _ _) <- items, Just (_, value) <- [literalLoad image item]]
      sends target = concat [go [] items | (_, items) <- perImage]
        where
          go _ [] = []
          go previous (item@(Decoded site _ instruction) : rest) = case instruction of
            A.Branch _ True called | SendFunction called == target -> (site, r0Constant previous) : go (take 6 (item : previous)) rest
            _ -> go (take 6 (item : previous)) rest
      imageEnds = [base + fromIntegral (B.length bytes) | Image _ _ base bytes <- images]
      fences = S.fromList (M.keys calls ++ extraStarts ++ imageEnds)
      addresses = M.fromListWith (flip (++)) [(value, [(site, form)]) | (site, form, value) <- [(site, MovwMovtForm, value) | (site, value) <- constructed] ++ pooledAddresses]
  in Census fences calls jumps literals (sort (movw ++ movImmediate ++ pooled)) addresses (maybe [] sends send)

nextFence :: Census -> Word32 -> Word32
nextFence summary address = maybe (address + 4) id (S.lookupGT address (censusFences summary))

-- | Nearest direct-call target or listed entry at or below a site.
lastFence :: Census -> Site -> Maybe Word32
lastFence summary (Site address) = S.lookupLE address (censusFences summary)

siteValue :: Census -> Site -> Value
siteValue summary site@(Site address) = object ["site" .= hex address, "candidateFunction" .= fmap hex (lastFence summary site)]

data Extent = Derived Int [String] | Refused String


deriveExtent :: Image -> Word32 -> Word32 -> Extent
deriveExtent (Image _ _ base image) address fence = case deriveFunction xm1TaskRecords (GuestAddress base) image (FunctionWindow (GuestAddress address) (GuestAddress fence)) [] of
  Right derived | FunctionDeclaration _ (FunctionSize bytes) <- derivedDeclaration derived -> Derived bytes (map returnPremiseLabel (derivationPremises derived))
  Left problem -> Refused problem

-- | The entry's window end and its extent.
entryExtent :: Census -> (Entry f, Image) -> (Word32, Extent)
entryExtent summary (entry, image) =
  let EntryAddress address = entryAddress entry
      fence = nextFence summary address
  in (fence, deriveExtent image address fence)

extentValue :: Word32 -> Word32 -> Extent -> Value
extentValue address fence outcome = case outcome of
  Derived bytes premises -> object
    [ "status" .= ("derived" :: String), "start" .= hex address, "end" .= hex (address + fromIntegral bytes)
    , "codeEnvelopeBytes" .= bytes, "windowEnd" .= hex fence, "returnPremises" .= premises
    , "scope" .= ("StockInventory reachable A32 code envelope, not transitive code size; complete candidate window classified as code or literals" :: String)
    ]
  Refused problem -> object
    [ "status" .= ("unknown" :: String), "start" .= hex address, "windowEnd" .= hex fence, "refusal" .= problem
    , "scope" .= ("No extent claimed; the next fence is a candidate, not a measured function end" :: String)
    ]

-- | Inside the derived extent, or inside the candidate window when the derivation refused.
within :: Word32 -> Word32 -> Extent -> Site -> Bool
within address fence outcome (Site site) = case outcome of
  Derived bytes _ -> site >= address && site < address + fromIntegral bytes
  Refused _ -> site >= address && site < fence

-- | The generated record of one entry with its window end and extent.
entryValue :: (f -> String) -> Census -> Entry f -> (Word32, Extent) -> Value
entryValue familyLabel summary entry (fence, outcome) =
  let EntryAddress address = entryAddress entry
      outside = filter (not . within address fence outcome)
      sites = map (siteValue summary)
  in object
    [ "address" .= hex address, "name" .= entryName entry, "family" .= familyLabel (entryFamily entry)
    , "recovery" .= recoveryLabel (entryRecovery entry)
    , "message" .= fmap (\(Command code) -> hex code) (entryMessage entry)
    , "operation" .= entryOperation entry, "arguments" .= entryArguments entry, "result" .= entryResult entry
    , "codes" .= [object ["value" .= value, "meaning" .= meaning] | Code value meaning <- entryCodes entry]
    , "context" .= entryContext entry, "objects" .= entryObjects entry
    , "callers" .= entryCallers entry
    , "directCalls" .= sites (M.findWithDefault [] address (directCalls summary))
    , "branchSites" .= sites (outside (M.findWithDefault [] address (branches summary)))
    , "literalWords" .= [hex site | Site site <- M.findWithDefault [] address (literalPositions summary)]
    , "constructedSites" .= sites [site | (site, MovwMovtForm) <- M.findWithDefault [] address (addressSites summary)]
    , "extent" .= extentValue address fence outcome
    , "grounded" .= entryGrounded entry, "unknown" .= entryUnknown entry
    , "evidence" .= entryEvidence entry
    ]

objectValue :: Census -> SharedObject -> Value
objectValue summary shared =
  let low = objectAddress shared
      high = toInteger low + toInteger (objectBytes shared)
      inside = M.takeWhileAntitone (\value -> toInteger value < high) (M.dropWhileAntitone (< low) (addressSites summary))
      touching = [(site, form, value) | (value, found) <- M.toList inside, (site, form) <- found]
  in object
    [ "name" .= objectName shared, "address" .= hex (objectAddress shared), "bytes" .= objectBytes shared, "layout" .= objectLayout shared
    , "constructedAddressSites" .= [object ["site" .= hex address, "form" .= formLabel form, "address" .= hex value, "candidateFunction" .= fmap hex (lastFence summary site)] | (site@(Site address), form, value) <- sort touching]
    , "grounded" .= objectGrounded shared, "unknown" .= objectUnknown shared, "evidence" .= objectEvidence shared
    ]

residualsValue :: [Value]
residualsValue = [object ["name" .= savedStackTaintResidualLabel residual, "statement" .= savedStackTaintResidualStatement residual] | residual <- [minBound .. maxBound]]

inputValue :: FilePath -> B.ByteString -> Value
inputValue path bytes = object ["path" .= path, "bytes" .= B.length bytes, "sha256" .= digest bytes]
