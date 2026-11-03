{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Source inventory of the X-M1 1.50 image4 file and media service: the
-- media task's request table, reviewed entry contracts, StockInventory
-- extents, a linear direct-call/literal/message-code census and the
-- constructed-address sites of the reviewed shared objects.
module Main (main) where

import qualified A32 as A
import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, encode, object, withObject, (.:), (.=))
import Data.Aeson.Types (Parser, parseEither)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.JSON as JSON
import ServiceInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

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

data Contracts = Contracts [Entry Family] [SharedObject]

parseContracts :: Value -> Parser Contracts
parseContracts = withObject "contracts" $ \o -> do
  exactKeys "contracts" ["entries", "objects"] o
  entries <- o .: "entries" >>= mapM (parseEntry familyLabel)
  objects <- o .: "objects" >>= mapM parseObject
  pure (Contracts entries objects)

image4Base :: Word32
image4Base = 0x61c400

-- | The media task's request table (named-recipe-reach section 6.2).
dispatchTable :: Word32
dispatchTable = 0x814740

codeFamily :: Word32 -> Bool
codeFamily value = value .&. 0xffffff00 == 0x2c00

-- | The table's {code, handler} word pairs up to the first all-zero pair.
requestTable :: Image -> Either String [(Command, EntryAddress)]
requestTable image = go dispatchTable []
  where
    go address seen = case (imageWord image address, imageWord image (address + 4)) of
      (Just 0, Just 0) -> Right (reverse seen)
      (Just code, Just handler)
        | not (codeFamily code) -> Left ("request table word at " ++ hex address ++ " is not a 0x2cXX code: " ++ hex code)
        | handler .&. 3 /= 0 || imageWord image handler == Nothing -> Left ("request table handler at " ++ hex (address + 4) ++ " is not an aligned image4 address: " ++ hex handler)
        | Command code `elem` map fst seen -> Left ("request table repeats code " ++ hex code)
        | EntryAddress handler `elem` map snd seen -> Left ("request table repeats handler " ++ hex handler)
        | length seen >= 256 -> Left "request table has no terminator within 256 pairs"
        | otherwise -> go (address + 8) ((Command code, EntryAddress handler) : seen)
      _ -> Left ("request table leaves image4 at " ++ hex address)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [imagePath', contractPath, output] -> do
      exists <- doesPathExist output
      when exists (fail "output already exists; use a fresh evidence path")
      image <- B.readFile imagePath'
      unless (digest image == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 is not canonical X-M1 1.50 base 0x61c400")
      contractBytes <- B.readFile contractPath
      contractValue <- either fail pure (JSON.decodeStrict contractBytes)
      Contracts entries objects <- either fail pure (parseEither parseContracts contractValue)
      let image4 = Image "image4" imagePath' image4Base image
          images = [image4]
      table <- either fail pure (requestTable image4)
      let addresses = [address | Entry {entryAddress = EntryAddress address} <- entries]
          handlers = M.fromList [(handler, code) | (code, EntryAddress handler) <- table]
      placed <- either fail pure (checkContracts images entries objects)
      forM_ entries $ \entry -> do
        let EntryAddress address = entryAddress entry
        case (entryMessage entry, M.lookup address handlers) of
          (Just code, Just tabled) | code /= tabled -> fail ("entry " ++ entryName entry ++ " names a message the request table does not route to it")
          (Just _, Nothing) -> fail ("entry " ++ entryName entry ++ " names a message but is not a request table handler")
          (Nothing, Just _) -> fail ("entry " ++ entryName entry ++ " is a request table handler without its message")
          _ -> pure ()
        when (entryFamily entry == MediaWorker && M.notMember address handlers) (fail ("media-worker entry " ++ entryName entry ++ " is not a request table handler"))
      case [hex handler | handler <- M.keys handlers, handler `notElem` addresses] of
        [] -> pure ()
        missing -> fail ("request table handlers without a contract: " ++ unwords missing)
      let summary = census images (addresses ++ M.keys handlers) codeFamily Nothing
          entryByAddress = M.fromList [(address, entryName entry) | entry@Entry {entryAddress = EntryAddress address} <- entries]
          codesSeen = sort (nub ([code | (_, _, _, code) <- codeSites summary] ++ map fst table))
          codeValue code@(Command value) = object
            [ "code" .= hex value
            , "handler" .= fmap (\(EntryAddress handler) -> hex handler) (lookup code table)
            , "handlerEntry" .= (lookup code table >>= \(EntryAddress handler) -> M.lookup handler entryByAddress)
            , "sites" .= [object ["site" .= hex address, "form" .= formLabel form, "register" .= A.regName register, "candidateFunction" .= fmap hex (lastFence summary site), "candidateEntry" .= (lastFence summary site >>= (`M.lookup` entryByAddress))] | (site@(Site address), form, register, seen) <- codeSites summary, seen == code]
            ]
      let ordered = sortOnAddress placed
          extents = map (entryExtent summary) ordered
      L.writeFile output (encode (object
        [ "schemaVersion" .= (1 :: Int), "firmware" .= ("X-M1 1.50" :: String)
        , "scope" .= ("Host-only source inventory of the image4 file and media service; unknown contracts and extent refusals block a complete replacement. No guest execution." :: String)
        , "inputs" .= [inputValue path bytes | (path, bytes) <- [(imagePath', image), (contractPath, contractBytes)]]
        , "extentMethod" .= ("StockInventory.deriveFunction with both loaders' saved-stack arrays; no external tails admitted; fence = min(next direct BL target in image4, next listed entry or request handler, image end). Refusals claim no end." :: String)
        , "savedStackTaintResiduals" .= residualsValue
        , "censusMethod" .= ("Linear decode of every aligned image4 word. directCalls are BL sites; branchSites are non-link B sites outside the derived extent, or outside the candidate window when the derivation refused; literalWords are aligned words equal to the entry address; constructedSites are MOVW/MOVT pairs building the entry address; message sites are a MOVW not completed by a MOVT of the same register, a MOV immediate or a PC-relative LDR of a 0x2cXX value; object sites are MOVW/MOVT pairs of one register within eight words, with no intervening MOVW of that register or unconditional transfer (B, BX, LDM or LDR into PC, data processing into PC), or PC-relative LDR values inside the object. Embedded data decodes as instructions, so every site is a candidate; candidateFunction is the nearest preceding fence, not a derived containment." :: String)
        , "callerClosure" .= ("open: Thumb, computed, register-relative and copied-pointer uses, addresses built by other instruction sequences, message codes matched by SUB/CMP range dispatch or built by other sequences, and callers outside image4 are not censused" :: String)
        , "mediaTask" .= object
            [ "requestTable" .= hex dispatchTable
            , "requests" .= [object ["code" .= hex code, "handler" .= hex handler, "entry" .= M.lookup handler entryByAddress] | (Command code, EntryAddress handler) <- table]
            ]
        , "messageCodes" .= map codeValue codesSeen
        , "entries" .= zipWith (entryValue familyLabel summary) (map fst ordered) extents
        , "objects" .= map (objectValue summary) objects
        , "families" .= [object ["family" .= familyLabel family, "entries" .= length [() | entry <- entries, entryFamily entry == family]] | family <- [minBound .. maxBound :: Family]]
        ]) <> "\n")
    _ -> fail "usage: file-manifest IMAGE4 CONTRACTS_JSON OUTPUT_JSON"
  where
    sortOnAddress placed = map snd (M.toAscList (M.fromList [(address, pair) | pair@(Entry {entryAddress = EntryAddress address}, _) <- placed]))
