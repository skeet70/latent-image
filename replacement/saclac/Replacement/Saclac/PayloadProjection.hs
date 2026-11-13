{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.Saclac.PayloadProjection
  ( Declaration
  , Direction (..)
  , QueueId (..)
  , Operation (..)
  , parseDeclaration
  , projectPayload
  , declarationJson
  ) where

import Control.Monad (unless)
import Data.Aeson
import qualified Data.ByteString as B
import qualified Data.Aeson.Types as J
import Data.List (find, sortOn)
import qualified Data.Text as T
import Data.Word (Word8, Word16, Word32)

newtype QueueId = QueueId Word8 deriving (Eq, Show)
newtype Operation = Operation Word16 deriving (Eq, Show)
newtype PayloadSize = PayloadSize Int deriving (Eq, Show)
newtype Offset = Offset Int deriving (Eq, Show)
newtype Width = Width Int deriving (Eq, Show)
newtype Citation = Citation T.Text deriving (Eq, Show)
data Direction = Incoming | Outgoing deriving (Eq, Show)
data Exclusion = Exclusion Offset Width [Word32] [Citation] deriving (Eq, Show)
data Field = Field T.Text Offset Width [Citation] (Maybe Exclusion) deriving (Eq, Show)
data Padding = Padding Offset Width [Citation] deriving (Eq, Show)
data Message = Message Direction QueueId Operation PayloadSize [Field] [Padding] deriving (Eq, Show)
data Declaration = Declaration T.Text [Message] deriving (Eq, Show)

instance FromJSON Direction where
  parseJSON = withText "message direction" $ \text -> case text of
    "incoming" -> pure Incoming
    "outgoing" -> pure Outgoing
    _ -> fail "message direction must be incoming or outgoing"
instance ToJSON Direction where
  toJSON Incoming = String "incoming"
  toJSON Outgoing = String "outgoing"
instance FromJSON Citation where
  parseJSON = withText "field citation" $ \text -> do
    unless (not (T.null (T.strip text))) (fail "field citation must not be empty")
    pure (Citation text)
instance ToJSON Citation where toJSON (Citation text) = toJSON text
instance FromJSON Exclusion where
  parseJSON = withObject "conditional field exclusion" $ \row -> do
    offset <- row .: "offset"
    width <- row .: "bytes"
    values <- row .: "values"
    sources <- row .: "sources"
    unless (offset >= 0 && width `elem` [1,2,4] && not (null values) && not (null sources)
      && all (\value -> toInteger value < 256 ^ width) values) (fail "invalid field exclusion")
    pure (Exclusion (Offset offset) (Width width) values sources)
instance ToJSON Exclusion where
  toJSON (Exclusion (Offset offset) (Width width) values sources) = object
    ["offset" .= offset, "bytes" .= width, "values" .= values, "sources" .= sources]
instance FromJSON Field where
  parseJSON = withObject "payload field" $ \row -> do
    name <- row .: "name"
    offset <- row .: "offset"
    width <- row .: "bytes"
    sources <- row .: "sources"
    exclusion <- row .:? "excluded_when"
    unless (not (T.null (T.strip name)) && offset >= 0 && width > 0 && not (null sources)) (fail "invalid payload field")
    pure (Field name (Offset offset) (Width width) sources exclusion)
instance ToJSON Field where
  toJSON (Field name (Offset offset) (Width width) sources exclusion) = object
    (["name" .= name, "offset" .= offset, "bytes" .= width, "sources" .= sources]
      ++ maybe [] (\condition -> ["excluded_when" .= condition]) exclusion)
instance FromJSON Padding where
  parseJSON = withObject "payload padding" $ \row -> do
    offset <- row .: "offset"
    width <- row .: "bytes"
    sources <- row .: "sources"
    unless (offset >= 0 && width > 0 && not (null sources)) (fail "invalid payload padding")
    pure (Padding (Offset offset) (Width width) sources)
instance ToJSON Padding where
  toJSON (Padding (Offset offset) (Width width) sources) = object
    ["offset" .= offset, "bytes" .= width, "sources" .= sources]
instance FromJSON Message where
  parseJSON = withObject "message fields" $ \row -> do
    direction <- row .: "direction"
    queue <- QueueId <$> row .: "queue"
    operation <- Operation <$> row .: "operation"
    size <- row .: "bytes"
    fields <- row .: "fields"
    padding <- row .:? "padding" .!= []
    let spans = sortOn fst
          ([(offset, width) | Field _ (Offset offset) (Width width) _ _ <- fields]
          ++ [(offset, width) | Padding (Offset offset) (Width width) _ <- padding])
        covered = foldl advance (Just 0) spans
        advance (Just next) (offset, width) | next == offset && width <= size - offset = Just (offset + width)
        advance _ _ = Nothing
    unless (size >= 0 && size <= 4096 && covered == Just size) (fail "message fields and padding must partition its payload")
    let selectorDefined (Field _ _ _ _ Nothing) = True
        selectorDefined (Field _ _ _ _ (Just (Exclusion (Offset offset) (Width width) _ _))) =
          any (\(Field _ (Offset start) (Width count) _ condition) ->
            condition == Nothing && start <= offset && offset + width <= start + count) fields
    unless (all selectorDefined fields) (fail "conditional selector must be an unconditional field")
    pure (Message direction queue operation (PayloadSize size) (sortOn fieldOffset fields) padding)
    where fieldOffset (Field _ (Offset offset) _ _ _) = offset
instance ToJSON Message where
  toJSON (Message direction (QueueId queue) (Operation operation) (PayloadSize size) fields padding) = object
    ["direction" .= direction, "queue" .= queue, "operation" .= operation, "bytes" .= size, "fields" .= fields, "padding" .= padding]
instance FromJSON Declaration where
  parseJSON = withObject "payload projection" $ \row -> do
    schema <- row .: "schema" :: J.Parser Int
    source <- row .: "source_commit"
    messages <- row .: "messages"
    let key (Message direction queue operation _ _ _) = (direction, queue, operation)
        unique [] = True
        unique (message : rest) = all ((/= key message) . key) rest && unique rest
    unless (schema == 1 && not (T.null source) && unique messages) (fail "invalid or duplicate message projection")
    pure (Declaration source messages)

parseDeclaration :: B.ByteString -> Either String Declaration
parseDeclaration = eitherDecodeStrict'

declarationJson :: Declaration -> Value
declarationJson (Declaration source messages) = object
  ["schema" .= (1 :: Int), "source_commit" .= source, "messages" .= messages]

projectPayload :: Declaration -> Direction -> QueueId -> Operation -> [Word8] -> Either String [Word8]
projectPayload (Declaration _ messages) direction queue operation payload = do
  Message _ _ _ (PayloadSize size) fields _ <- maybe (Left "message has no field declaration") Right
    (find (\(Message candidateDirection candidateQueue candidateOperation _ _ _) ->
      candidateDirection == direction && candidateQueue == queue && candidateOperation == operation) messages)
  unless (length payload == size) (Left "message payload length differs from field declaration")
  let included Nothing = True
      included (Just (Exclusion (Offset offset) (Width width) values _)) =
        let value = sum [fromIntegral byte * 256 ^ index | (index, byte) <- zip [0 :: Int ..] (take width (drop offset payload))]
        in value `notElem` values
  pure (concat [take width (drop offset payload) | Field _ (Offset offset) (Width width) _ condition <- fields, included condition])
