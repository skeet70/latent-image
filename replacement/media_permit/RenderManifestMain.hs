{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Source inventory of the X-M1 1.50 display and render service over
-- image4, image5 and the plain image: reviewed entry contracts,
-- StockInventory extents, a linear candidate census and the queue sends
-- whose destination constant names a reviewed render queue.
module Main (main) where

import Control.Monad (forM_, unless, when)
import Data.Aeson (Value, encode, object, withObject, (.:), (.=))
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (nub)
import Data.Maybe (isJust)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified FujiAF.JSON as JSON
import ServiceInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

data Family = Actor | Dispatcher | CommandHandler | Poster | Probe | Graphics | Display | Completion | Helper
  deriving (Eq, Ord, Show, Enum, Bounded)

familyLabel :: Family -> String
familyLabel family = case family of
  Actor -> "actor"
  Dispatcher -> "dispatcher"
  CommandHandler -> "command-handler"
  Poster -> "poster"
  Probe -> "probe"
  Graphics -> "graphics"
  Display -> "display"
  Completion -> "completion"
  Helper -> "helper"

-- | A kernel message queue whose sends are censused.
newtype QueueId = QueueId Word32 deriving (Eq, Ord, Show)

data Queue = Queue { queueId :: QueueId, queueName :: String, queueEvidence :: [String] }

parseQueue :: Value -> Parser Queue
parseQueue = withObject "queue" $ \o -> do
  exactKeys "queue" ["id", "name", "evidence"] o
  identifier <- o .: "id" >>= hexWord "queue id"
  citations <- o .: "evidence"
  when (null citations) (fail ("queue " ++ hex identifier ++ " has no evidence"))
  name <- o .: "name"
  unless (kebab name) (fail ("queue name is not kebab-case: " ++ name))
  pure (Queue (QueueId identifier) name citations)

data Contracts = Contracts [Entry Family] [SharedObject] [Queue]

parseContracts :: Value -> Parser Contracts
parseContracts = withObject "contracts" $ \o -> do
  exactKeys "contracts" ["entries", "objects", "queues"] o
  Contracts <$> (o .: "entries" >>= mapM (parseEntry familyLabel)) <*> (o .: "objects" >>= mapM parseObject) <*> (o .: "queues" >>= mapM parseQueue)

-- | The image4 kernel message send (reports/rtos-platform-manifest-draft.md).
messageSend :: SendFunction
messageSend = SendFunction 0x72503c

pinned :: [(String, Word32, String)]
pinned =
  [ ("image4", 0x61c400, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
  , ("image5", 0xbf1a00, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
  , ("plain", 0x1100000, "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
  ]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [image4Path, image5Path, plainPath, contractPath, output] -> do
      exists <- doesPathExist output
      when exists (fail "output already exists; use a fresh evidence path")
      images <- mapM (\(path, (label, base, sha)) -> do
          bytes <- B.readFile path
          unless (digest bytes == sha) (fail (label ++ " is not canonical X-M1 1.50 at base " ++ hex base))
          pure (Image label path base bytes)) (zip [image4Path, image5Path, plainPath] pinned)
      contractBytes <- B.readFile contractPath
      contractValue <- either fail pure (JSON.decodeStrict contractBytes)
      Contracts entries objects queues <- either fail pure (parseEither parseContracts contractValue)
      placed <- either fail pure (checkContracts images entries objects)
      unless (length (nub (map queueId queues)) == length queues) (fail "contract input repeats a queue id")
      unless (length (nub (map queueName queues)) == length queues) (fail "contract input repeats a queue name")
      -- A command handler serves its code; a poster sends it.
      forM_ entries $ \entry -> case (entryFamily entry, entryMessage entry) of
        (CommandHandler, Nothing) -> fail ("command-handler entry " ++ entryName entry ++ " has no command code")
        (family, Just _) | family `notElem` [CommandHandler, Poster] -> fail ("entry " ++ entryName entry ++ " has a command code but neither serves nor posts one")
        _ -> pure ()
      let addresses = [address | Entry {entryAddress = EntryAddress address} <- entries]
          summary = census images addresses (const False) (Just messageSend)
          entryByAddress = M.fromList [(address, entryName entry) | entry@Entry {entryAddress = EntryAddress address} <- entries]
          sendValue site@(Site address) = object ["site" .= hex address, "candidateFunction" .= fmap hex (lastFence summary site), "candidateEntry" .= (lastFence summary site >>= (`M.lookup` entryByAddress))]
          queueValue queue = let QueueId identifier = queueId queue in object
            [ "id" .= hex identifier, "name" .= queueName queue, "evidence" .= queueEvidence queue
            , "sends" .= [sendValue site | (site, Just (Destination destination)) <- sendSites summary, destination == identifier]
            ]
          imageSends image = [() | (Site address, _) <- sendSites summary, isJust (imageOf [image] address)]
      let ordered = sortOnAddress placed
          extents = map (entryExtent summary) ordered
      L.writeFile output (encode (object
        [ "schemaVersion" .= (1 :: Int), "firmware" .= ("X-M1 1.50" :: String)
        , "scope" .= ("Host-only source inventory of the display and render service over image4, image5 and the plain image; unknown contracts and extent refusals block a complete replacement. No guest execution." :: String)
        , "inputs" .= ([inputValue (imagePath image) (imageBytes image) | image <- images] ++ [inputValue contractPath contractBytes])
        , "extentMethod" .= ("StockInventory.deriveFunction with both loaders' saved-stack arrays over the entry's own image; no external tails admitted; fence = min(next direct BL target in any listed image, next listed entry, image end). Refusals claim no end." :: String)
        , "savedStackTaintResiduals" .= residualsValue
        , "censusMethod" .= ("Linear decode of every aligned word of the three images. directCalls are BL sites in any image; branchSites are non-link B sites outside the derived extent, or outside the candidate window when the derivation refused; literalWords are aligned words equal to the entry address; constructedSites are MOVW/MOVT pairs building the entry address; object sites are MOVW/MOVT pairs of one register within eight words, with no intervening MOVW of that register or unconditional transfer, or PC-relative LDR values inside the object. Queue sends are BL sites of 0x72503c whose r0 is set by an unconditional MOV immediate or MOVW within the six preceding words, with no intervening unconditional transfer or other r0 write. Embedded data decodes as instructions, so every site is a candidate; candidateFunction is the nearest preceding fence, not a derived containment." :: String)
        , "callerClosure" .= ("open: Thumb, computed, register-relative and copied-pointer uses, addresses built by other instruction sequences, sends whose queue id reaches r0 any other way or across a branch, wrappers of 0x72503c, and callers outside the three images are not censused" :: String)
        , "sendCensus" .= object
            [ "sendFunction" .= (let SendFunction address = messageSend in hex address)
            , "sites" .= length (sendSites summary)
            , "constantDestination" .= length [() | (_, Just _) <- sendSites summary]
            , "destinations" .= [object ["id" .= hex destination, "sites" .= count] | (destination, count) <- M.toAscList (M.fromListWith (+) [(destination, 1 :: Int) | (_, Just (Destination destination)) <- sendSites summary])]
            , "perImage" .= [object ["image" .= imageLabel image, "sites" .= length (imageSends image)] | image <- images]
            ]
        , "queues" .= map queueValue queues
        , "entries" .= zipWith (entryValue familyLabel summary) (map fst ordered) extents
        , "objects" .= map (objectValue summary) objects
        , "families" .= [object ["family" .= familyLabel family, "entries" .= length [() | entry <- entries, entryFamily entry == family]] | family <- [minBound .. maxBound :: Family]]
        ]) <> "\n")
    _ -> fail "usage: render-manifest IMAGE4 IMAGE5 PLAIN CONTRACTS_JSON OUTPUT_JSON"
  where
    sortOnAddress placed = map snd (M.toAscList (M.fromList [(address, pair) | pair@(Entry {entryAddress = EntryAddress address}, _) <- placed]))
