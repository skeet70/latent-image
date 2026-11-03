{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Source inventory of the X-M1 1.50 image4 file and media service: the
-- media task's request table, reviewed entry contracts, StockInventory
-- extents, a linear direct-call/literal/message-code census and the
-- constructed-address sites of the reviewed shared objects.
module Main (main) where

import qualified A32 as A
import Control.Monad (forM_, unless, when)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (Value, encode, object, withObject, (.:), (.=))
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import Data.Aeson.Types (Parser, parseEither)
import Data.Bits (shiftL, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Char (isAsciiLower, isDigit, isHexDigit)
import Data.List (nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Data.Word (Word32)
import qualified FujiAF.JSON as JSON
import Numeric (readHex, showHex)
import StockInventory (FunctionDeclaration (..), FunctionDerivation (..), FunctionSize (..), FunctionWindow (..), GuestAddress (..), deriveFunction, returnPremiseLabel, savedStackTaintResidualLabel, savedStackTaintResidualStatement, xm1TaskRecords)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype EntryAddress = EntryAddress Word32 deriving (Eq, Ord, Show)
newtype MessageCode = MessageCode Word32 deriving (Eq, Ord, Show)
newtype Site = Site Word32 deriving (Eq, Ord, Show)

data Family = CallerFacing | SynchronousForm | Poster | MediaTask | MediaWorker | HandleManager | Completion | VolumeRegistry | MediaState | Helper
  deriving (Eq, Ord, Show, Enum, Bounded)

familyLabel :: Family -> String
familyLabel family = case family of
  CallerFacing -> "caller-facing"
  SynchronousForm -> "synchronous-form"
  Poster -> "poster"
  MediaTask -> "media-task"
  MediaWorker -> "media-worker"
  HandleManager -> "handle-manager"
  Completion -> "completion"
  VolumeRegistry -> "volume-registry"
  MediaState -> "media-state"
  Helper -> "helper"

data Recovery = Recovered | Partial | Unknown deriving (Eq, Show, Enum, Bounded)

recoveryLabel :: Recovery -> String
recoveryLabel recovery = case recovery of
  Recovered -> "recovered"
  Partial -> "partial"
  Unknown -> "unknown"

-- | How a censused value reaches a register.
data Form = MovwForm | MovImmediateForm | LiteralForm | MovwMovtForm deriving (Eq, Ord, Show)

formLabel :: Form -> String
formLabel form = case form of
  MovwForm -> "movw"
  MovImmediateForm -> "mov"
  LiteralForm -> "ldr-literal"
  MovwMovtForm -> "movw-movt"

data Code = Code Int String

data Entry = Entry
  { entryAddress :: EntryAddress
  , entryName :: String
  , entryFamily :: Family
  , entryRecovery :: Recovery
  , entryMessage :: Maybe MessageCode
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

parseEntry :: Value -> Parser Entry
parseEntry = withObject "entry" $ \o -> do
  exactKeys "entry" ["address", "name", "family", "recovery", "message", "operation", "arguments", "result", "codes", "context", "objects", "callers", "grounded", "unknown", "evidence"] o
  address <- o .: "address" >>= hexWord "entry address"
  name <- o .: "name"
  unless (kebab name) (fail ("entry name is not kebab-case: " ++ name))
  family <- o .: "family" >>= labelled "family" familyLabel
  recovery <- o .: "recovery" >>= labelled "recovery" recoveryLabel
  message <- o .: "message" >>= maybe (pure Nothing) (fmap (Just . MessageCode) . hexWord "message code")
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

data Contracts = Contracts [Entry] [SharedObject]

parseContracts :: Value -> Parser Contracts
parseContracts = withObject "contracts" $ \o -> do
  exactKeys "contracts" ["entries", "objects"] o
  entries <- o .: "entries" >>= mapM parseEntry
  objects <- o .: "objects" >>= mapM parseObject
  pure (Contracts entries objects)

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

digest :: B.ByteString -> String
digest = concatMap (\byte -> let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits) . B.unpack . SHA256.hash

imageBase :: Word32
imageBase = 0x61c400

-- | The media task's request table (named-recipe-reach section 6.2).
dispatchTable :: Word32
dispatchTable = 0x814740

codeFamily :: Word32 -> Bool
codeFamily value = value .&. 0xffffff00 == 0x2c00

wordAt :: B.ByteString -> Word32 -> Maybe Word32
wordAt image address
  | address < imageBase || address .&. 3 /= 0 || toInteger address + 4 > toInteger imageBase + toInteger (B.length image) = Nothing
  | otherwise = Just (foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index image (fromIntegral (address - imageBase) + index))) 0 [0 .. 3])

-- | The table's {code, handler} word pairs up to the first all-zero pair.
requestTable :: B.ByteString -> Either String [(MessageCode, EntryAddress)]
requestTable image = go dispatchTable []
  where
    go address seen = case (wordAt image address, wordAt image (address + 4)) of
      (Just 0, Just 0) -> Right (reverse seen)
      (Just code, Just handler)
        | not (codeFamily code) -> Left ("request table word at " ++ hex address ++ " is not a 0x2cXX code: " ++ hex code)
        | handler .&. 3 /= 0 || wordAt image handler == Nothing -> Left ("request table handler at " ++ hex (address + 4) ++ " is not an aligned image4 address: " ++ hex handler)
        | MessageCode code `elem` map fst seen -> Left ("request table repeats code " ++ hex code)
        | EntryAddress handler `elem` map snd seen -> Left ("request table repeats handler " ++ hex handler)
        | length seen >= 256 -> Left "request table has no terminator within 256 pairs"
        | otherwise -> go (address + 8) ((MessageCode code, EntryAddress handler) : seen)
      _ -> Left ("request table leaves image4 at " ++ hex address)

data Decoded = Decoded Site Word32 A.Instruction

decodeImage :: B.ByteString -> [Decoded]
decodeImage image = [Decoded (Site address) raw (A.decode address raw) | address <- [imageBase, imageBase + 4 .. imageBase + fromIntegral (B.length image) - 4], Just raw <- [wordAt image address]]

-- | Value loaded by a PC-relative word LDR.
literalLoad :: B.ByteString -> Decoded -> Maybe (A.Reg, Word32)
literalLoad image (Decoded (Site address) _ instruction) = case instruction of
  A.Load _ A.Word False destination base (A.OffsetImmediate offset) A.OffsetOnly
    | base == A.pc && destination /= A.pc -> (,) destination <$> wordAt image (fromIntegral (toInteger address + 8 + toInteger offset))
  _ -> Nothing

-- | Addresses built by a MOVW and a later MOVT of the same register within
-- eight words, with no intervening MOVW of that register or unconditional transfer.
constructedAddresses :: [Decoded] -> [(Site, Word32)]
constructedAddresses decoded = go decoded
  where
    go [] = []
    go (Decoded site _ (A.MoveWide _ register low) : rest) =
      let ends (Decoded _ _ instruction) = case instruction of
            A.MoveWide _ other _ -> other == register
            A.Branch (A.Cond 14) False _ -> True
            A.BranchExchange (A.Cond 14) False _ -> True
            A.LoadMultiple (A.Cond 14) _ _ _ registers -> A.pc `elem` registers
            A.Load (A.Cond 14) _ _ destination _ _ _ -> destination == A.pc
            A.DataProcessing (A.Cond 14) _ _ destination _ _ -> destination == A.pc
            _ -> False
      in case [high | Decoded _ _ (A.MoveTop _ top high) <- takeWhile (not . ends) (take 8 rest), top == register] of
        high : _ -> (site, (high `shiftL` 16) .|. low) : go rest
        [] -> go rest
    go (_ : rest) = go rest

data Census = Census
  { censusFences :: [Word32]
  , directCalls :: M.Map Word32 [Site]
  , branches :: M.Map Word32 [Site]
  , literalPositions :: M.Map Word32 [Site]
  , codeSites :: [(Site, Form, A.Reg, MessageCode)]
  , addressSites :: [(Site, Form, Word32)]
  }

census :: B.ByteString -> [Word32] -> Census
census image extraStarts =
  let decoded = decodeImage image
      imageEnd = imageBase + fromIntegral (B.length image)
      inImage target = target >= imageBase && target < imageEnd
      calls = M.fromListWith (flip (++)) [(target, [site]) | Decoded site _ (A.Branch _ True target) <- decoded, inImage target]
      jumps = M.fromListWith (flip (++)) [(target, [site]) | Decoded site _ (A.Branch _ False target) <- decoded, inImage target]
      literals = M.fromListWith (flip (++)) [(raw, [site]) | Decoded site raw _ <- decoded, inImage raw]
      constructed = constructedAddresses decoded
      paired = S.fromList (map fst constructed)
      movw = [(site, MovwForm, register, MessageCode value) | Decoded site _ (A.MoveWide _ register value) <- decoded, codeFamily value, S.notMember site paired]
      movImmediate = [(site, MovImmediateForm, register, MessageCode value) | Decoded site _ (A.DataProcessing _ A.MOV _ register _ (A.Immediate value)) <- decoded, codeFamily value]
      pooled = [(site, LiteralForm, register, MessageCode value) | item@(Decoded site _ _) <- decoded, Just (register, value) <- [literalLoad image item], codeFamily value]
      pooledAddresses = [(site, LiteralForm, value) | item@(Decoded site _ _) <- decoded, Just (_, value) <- [literalLoad image item]]
      fences = S.toAscList (S.fromList (M.keys calls ++ extraStarts ++ [imageEnd]))
  in Census fences calls jumps literals (sort (movw ++ movImmediate ++ pooled)) ([(site, MovwMovtForm, value) | (site, value) <- constructed] ++ pooledAddresses)

nextFence :: Census -> Word32 -> Word32
nextFence summary address = case dropWhile (<= address) (censusFences summary) of
  fence : _ -> fence
  [] -> address + 4

-- | Nearest direct-call target or listed entry at or below a site.
lastFence :: Census -> Site -> Maybe Word32
lastFence summary (Site address) = case reverse (takeWhile (<= address) (censusFences summary)) of
  start : _ -> Just start
  [] -> Nothing

siteValue :: Census -> Site -> Value
siteValue summary site@(Site address) = object ["site" .= hex address, "candidateFunction" .= fmap hex (lastFence summary site)]

data Extent = Derived Int [String] | Refused String

deriveExtent :: B.ByteString -> Word32 -> Word32 -> Extent
deriveExtent image address fence = case deriveFunction xm1TaskRecords (GuestAddress imageBase) image (FunctionWindow (GuestAddress address) (GuestAddress fence)) [] of
  Right derived | FunctionDeclaration _ (FunctionSize bytes) <- derivedDeclaration derived -> Derived bytes (map returnPremiseLabel (derivationPremises derived))
  Left problem -> Refused problem

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

main :: IO ()
main = do
  args <- getArgs
  case args of
    [imagePath, contractPath, output] -> do
      exists <- doesPathExist output
      when exists (fail "output already exists; use a fresh evidence path")
      image <- B.readFile imagePath
      unless (digest image == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 is not canonical X-M1 1.50 base 0x61c400")
      contractBytes <- B.readFile contractPath
      contractValue <- either fail pure (JSON.decodeStrict contractBytes)
      Contracts entries objects <- either fail pure (parseEither parseContracts contractValue)
      table <- either fail pure (requestTable image)
      let addresses = [address | Entry {entryAddress = EntryAddress address} <- entries]
          handlers = M.fromList [(handler, code) | (code, EntryAddress handler) <- table]
      unless (length (nub addresses) == length addresses) (fail "contract input repeats an entry address")
      unless (length (nub (map entryName entries)) == length entries) (fail "contract input repeats an entry name")
      unless (length (nub (map objectName objects)) == length objects) (fail "contract input repeats an object name")
      forM_ entries $ \entry -> do
        let EntryAddress address = entryAddress entry
        when (wordAt image address == Nothing) (fail ("entry " ++ entryName entry ++ " is not an aligned image4 address"))
        case (entryMessage entry, M.lookup address handlers) of
          (Just code, Just tabled) | code /= tabled -> fail ("entry " ++ entryName entry ++ " names a message the request table does not route to it")
          (Just _, Nothing) -> fail ("entry " ++ entryName entry ++ " names a message but is not a request table handler")
          (Nothing, Just _) -> fail ("entry " ++ entryName entry ++ " is a request table handler without its message")
          _ -> pure ()
        when (entryFamily entry == MediaWorker && M.notMember address handlers) (fail ("media-worker entry " ++ entryName entry ++ " is not a request table handler"))
      case [hex handler | handler <- M.keys handlers, handler `notElem` addresses] of
        [] -> pure ()
        missing -> fail ("request table handlers without a contract: " ++ unwords missing)
      let summary = census image (addresses ++ M.keys handlers)
          entryByAddress = M.fromList [(address, entryName entry) | entry@Entry {entryAddress = EntryAddress address} <- entries]
          sites = map (siteValue summary)
          entryValue entry =
            let EntryAddress address = entryAddress entry
                fence = nextFence summary address
                outcome = deriveExtent image address fence
                outside = filter (not . within address fence outcome)
            in object
              [ "address" .= hex address, "name" .= entryName entry, "family" .= familyLabel (entryFamily entry)
              , "recovery" .= recoveryLabel (entryRecovery entry)
              , "message" .= fmap (\(MessageCode code) -> hex code) (entryMessage entry)
              , "operation" .= entryOperation entry, "arguments" .= entryArguments entry, "result" .= entryResult entry
              , "codes" .= [object ["value" .= value, "meaning" .= meaning] | Code value meaning <- entryCodes entry]
              , "context" .= entryContext entry, "objects" .= entryObjects entry
              , "callers" .= entryCallers entry
              , "directCalls" .= sites (M.findWithDefault [] address (directCalls summary))
              , "branchSites" .= sites (outside (M.findWithDefault [] address (branches summary)))
              , "literalWords" .= [hex site | Site site <- M.findWithDefault [] address (literalPositions summary)]
              , "constructedSites" .= sites [site | (site, MovwMovtForm, value) <- addressSites summary, value == address]
              , "extent" .= extentValue address fence outcome
              , "grounded" .= entryGrounded entry, "unknown" .= entryUnknown entry
              , "evidence" .= entryEvidence entry
              ]
          objectValue shared =
            let low = toInteger (objectAddress shared)
                high = low + toInteger (objectBytes shared)
                touching = [(site, form, value) | (site, form, value) <- addressSites summary, toInteger value >= low, toInteger value < high]
            in object
              [ "name" .= objectName shared, "address" .= hex (objectAddress shared), "bytes" .= objectBytes shared, "layout" .= objectLayout shared
              , "constructedAddressSites" .= [object ["site" .= hex address, "form" .= formLabel form, "address" .= hex value, "candidateFunction" .= fmap hex (lastFence summary site)] | (site@(Site address), form, value) <- sort touching]
              , "grounded" .= objectGrounded shared, "unknown" .= objectUnknown shared, "evidence" .= objectEvidence shared
              ]
          codesSeen = sort (nub ([code | (_, _, _, code) <- codeSites summary] ++ map fst table))
          codeValue code@(MessageCode value) = object
            [ "code" .= hex value
            , "handler" .= fmap (\(EntryAddress handler) -> hex handler) (lookup code table)
            , "handlerEntry" .= (lookup code table >>= \(EntryAddress handler) -> M.lookup handler entryByAddress)
            , "sites" .= [object ["site" .= hex address, "form" .= formLabel form, "register" .= A.regName register, "candidateFunction" .= fmap hex (lastFence summary site), "candidateEntry" .= (lastFence summary site >>= (`M.lookup` entryByAddress))] | (site@(Site address), form, register, seen) <- codeSites summary, seen == code]
            ]
      L.writeFile output (encode (object
        [ "schemaVersion" .= (1 :: Int), "firmware" .= ("X-M1 1.50" :: String)
        , "scope" .= ("Host-only source inventory of the image4 file and media service; unknown contracts and extent refusals block a complete replacement. No guest execution." :: String)
        , "inputs" .= [object ["path" .= path, "bytes" .= B.length bytes, "sha256" .= digest bytes] | (path, bytes) <- [(imagePath, image), (contractPath, contractBytes)]]
        , "extentMethod" .= ("StockInventory.deriveFunction with both loaders' saved-stack arrays; no external tails admitted; fence = min(next direct BL target in image4, next listed entry or request handler, image end). Refusals claim no end." :: String)
        , "savedStackTaintResiduals" .= [object ["name" .= savedStackTaintResidualLabel residual, "statement" .= savedStackTaintResidualStatement residual] | residual <- [minBound .. maxBound]]
        , "censusMethod" .= ("Linear decode of every aligned image4 word. directCalls are BL sites; branchSites are non-link B sites outside the derived extent, or outside the candidate window when the derivation refused; literalWords are aligned words equal to the entry address; constructedSites are MOVW/MOVT pairs building the entry address; message sites are a MOVW not completed by a MOVT of the same register, a MOV immediate or a PC-relative LDR of a 0x2cXX value; object sites are MOVW/MOVT pairs of one register within eight words, with no intervening MOVW of that register or unconditional transfer (B, BX, LDM or LDR into PC, data processing into PC), or PC-relative LDR values inside the object. Embedded data decodes as instructions, so every site is a candidate; candidateFunction is the nearest preceding fence, not a derived containment." :: String)
        , "callerClosure" .= ("open: Thumb, computed, register-relative and copied-pointer uses, addresses built by other instruction sequences, message codes matched by SUB/CMP range dispatch or built by other sequences, and callers outside image4 are not censused" :: String)
        , "mediaTask" .= object
            [ "requestTable" .= hex dispatchTable
            , "requests" .= [object ["code" .= hex code, "handler" .= hex handler, "entry" .= M.lookup handler entryByAddress] | (MessageCode code, EntryAddress handler) <- table]
            ]
        , "messageCodes" .= map codeValue codesSeen
        , "entries" .= map entryValue (sortOnAddress entries)
        , "objects" .= map objectValue objects
        , "families" .= [object ["family" .= familyLabel family, "entries" .= length [() | entry <- entries, entryFamily entry == family]] | family <- [minBound .. maxBound :: Family]]
        ]) <> "\n")
    _ -> fail "usage: file-manifest IMAGE4 CONTRACTS_JSON OUTPUT_JSON"
  where
    sortOnAddress entries = map snd (M.toAscList (M.fromList [(address, entry) | entry@Entry {entryAddress = EntryAddress address} <- entries]))
