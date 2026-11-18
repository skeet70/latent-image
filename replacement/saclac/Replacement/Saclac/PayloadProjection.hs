{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Replacement.Saclac.PayloadProjection
  ( Declaration
  , Direction (..)
  , QueueId (..)
  , Operation (..)
  , Role (..)
  , parseDeclaration
  , projectPayload
  , declarationJson
  ) where

import Control.Monad (unless)
import Data.Aeson
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.Aeson.Types as J
import Data.List (find, sortOn)
import Data.Maybe (isJust)
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
-- | A word carrying the identity of owned code or storage: the stock role passes the declared
-- stock value itself, an owned role the linked address of the replacement symbol the contract
-- names. It is admitted only with cited evidence of every retained consumer's use of the word.
data Identity = Identity Word32 T.Text [Consumer] deriving (Eq, Show)
-- | How retained code uses an identity word: it never dereferences it (it only stores or posts it
-- as a value), or it uses it only through an owned contract, entering the owned code or accessing
-- the owned storage, whose effects are compared elsewhere. A word a retained consumer reads with
-- stock layout assumptions has no consumer kind; its pointed-to bytes are compared instead.
data Consumer = NeverDereferenced [Citation] | OwnedContract [Citation] [Citation] deriving (Eq, Show)
data Field = Field T.Text Offset Width [Citation] (Maybe Exclusion) (Maybe Identity) deriving (Eq, Show)
data Padding = Padding Offset Width [Citation] deriving (Eq, Show)

-- | Whose stream a payload belongs to. An owned role resolves a symbol in the compared image's own
-- symbol table, refusing an absent or repeated name.
data Role = StockRole | OwnedRole (T.Text -> Either String Word32)

-- | The little-endian word at an offset that separates two senders of one
-- operation. Every entry that declares one is selected by it; an entry
-- without one is the only entry for its operation.
data Discriminator = Discriminator Offset Word32 deriving (Eq, Show)

-- | The discriminating word is four bytes wide.
discriminatorWidth :: Int
discriminatorWidth = 4

data Message
  = Message Direction QueueId Operation PayloadSize [Field] [Padding] (Maybe Discriminator)
  -- | A completion one of saclac's callbacks posts into queue 43 from the
  -- sender's task. Its payload projects to nothing whatever its length.
  | Internal QueueId Operation [Citation]
  deriving (Eq, Show)
data Declaration = Declaration T.Text [Message] [Value] [Value] deriving (Eq, Show)

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
instance FromJSON Consumer where
  parseJSON = withObject "identity consumer" $ \row -> do
    use <- row .: "use"
    sources <- row .: "sources"
    unless (not (null sources)) (fail "an identity consumer cites its use")
    case use :: T.Text of
      "never-dereferenced" -> do
        unless (not (KM.member "compared_by" row)) (fail "a never-dereferenced consumer has no compared contents")
        pure (NeverDereferenced sources)
      "owned-contract" -> do
        compared <- row .: "compared_by"
        unless (not (null compared)) (fail "an owned-contract consumer cites where its contents are compared")
        pure (OwnedContract sources compared)
      _ -> fail "an identity consumer never dereferences the word or uses it only through an owned contract"
instance ToJSON Consumer where
  toJSON (NeverDereferenced sources) = object ["use" .= ("never-dereferenced" :: T.Text), "sources" .= sources]
  toJSON (OwnedContract sources compared) =
    object ["use" .= ("owned-contract" :: T.Text), "sources" .= sources, "compared_by" .= compared]
instance FromJSON Identity where
  parseJSON = withObject "owned identity" $ \row -> do
    value <- row .: "stock_value"
    symbol <- row .: "replacement_symbol"
    consumers <- row .: "consumers"
    unless (not (T.null (T.strip symbol))) (fail "an owned identity names its replacement symbol")
    unless (not (null consumers)) (fail "an owned identity cites its retained consumers")
    pure (Identity value symbol consumers)
instance ToJSON Identity where
  toJSON (Identity value symbol consumers) =
    object ["stock_value" .= value, "replacement_symbol" .= symbol, "consumers" .= consumers]
instance FromJSON Field where
  parseJSON = withObject "payload field" $ \row -> do
    name <- row .: "name"
    offset <- row .: "offset"
    width <- row .: "bytes"
    sources <- row .: "sources"
    exclusion <- row .:? "excluded_when"
    identity <- row .:? "identity"
    unless (not (T.null (T.strip name)) && offset >= 0 && width > 0 && not (null sources)) (fail "invalid payload field")
    unless (maybe True (const (width == 4 && exclusion == Nothing)) identity)
      (fail "an owned identity is one unconditional word")
    pure (Field name (Offset offset) (Width width) sources exclusion identity)
instance ToJSON Field where
  toJSON (Field name (Offset offset) (Width width) sources exclusion identity) = object
    (["name" .= name, "offset" .= offset, "bytes" .= width, "sources" .= sources]
      ++ maybe [] (\condition -> ["excluded_when" .= condition]) exclusion
      ++ maybe [] (\declared -> ["identity" .= declared]) identity)
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
instance FromJSON Discriminator where
  parseJSON = withObject "message discriminator" $ \row -> do
    offset <- row .: "offset"
    value <- row .: "value"
    unless (offset >= 0) (fail "invalid message discriminator")
    pure (Discriminator (Offset offset) value)
instance ToJSON Discriminator where
  toJSON (Discriminator (Offset offset) value) = object ["offset" .= offset, "value" .= value]
instance FromJSON Message where
  parseJSON = withObject "message fields" $ \row -> do
    direction <- row .: "direction"
    queue <- QueueId <$> row .: "queue"
    operation <- Operation <$> row .: "operation"
    internal <- row .:? "saclac_internal"
    case internal of
      Just body -> do
        sources <- withObject "saclac-internal completion" (.: "sources") body
        unless (direction == Incoming && not (null sources)
            && not (any (`KM.member` row) ["bytes", "fields", "padding", "discriminator"]))
          (fail "a saclac-internal completion is incoming, cites its callback and declares no payload")
        pure (Internal queue operation sources)
      Nothing -> declaredMessage row direction queue operation

declaredMessage :: Object -> Direction -> QueueId -> Operation -> J.Parser Message
declaredMessage row direction queue operation = do
  size <- row .: "bytes"
  fields <- row .: "fields"
  padding <- row .:? "padding" .!= []
  discriminator <- row .:? "discriminator"
  let spans = sortOn fst
        ([(offset, width) | Field _ (Offset offset) (Width width) _ _ _ <- fields]
        ++ [(offset, width) | Padding (Offset offset) (Width width) _ <- padding])
      covered = foldl advance (Just 0) spans
      advance (Just next) (offset, width) | next == offset && width <= size - offset = Just (offset + width)
      advance _ _ = Nothing
      unconditional (Field _ (Offset start) (Width count) _ Nothing _) = [(start, start + count)]
      unconditional _ = []
      insideSelector (Discriminator (Offset offset) _) =
        any (\(start, end) -> start <= offset && offset + discriminatorWidth <= end)
          (concatMap unconditional fields)
      discriminatorFits (Discriminator (Offset offset) _) = offset + discriminatorWidth <= size
  unless (size >= 0 && size <= 4096 && covered == Just size) (fail "message fields and padding must partition its payload")
  let selectorDefined (Field _ _ _ _ Nothing _) = True
      selectorDefined (Field _ _ _ _ (Just (Exclusion (Offset offset) (Width width) _ _)) _) =
        any (\(Field _ (Offset start) (Width count) _ condition _) ->
          condition == Nothing && start <= offset && offset + width <= start + count) fields
  unless (all selectorDefined fields) (fail "conditional selector must be an unconditional field")
  unless (maybe True (\value -> discriminatorFits value && insideSelector value) discriminator)
    (fail "message discriminator must lie inside an unconditional field")
  pure (Message direction queue operation (PayloadSize size) (sortOn fieldOffset fields) padding discriminator)
  where fieldOffset (Field _ (Offset offset) _ _ _ _) = offset

instance ToJSON Message where
  toJSON (Message direction (QueueId queue) (Operation operation) (PayloadSize size) fields padding discriminator) = object
    (["direction" .= direction, "queue" .= queue, "operation" .= operation, "bytes" .= size, "fields" .= fields, "padding" .= padding]
      ++ maybe [] (\value -> ["discriminator" .= value]) discriminator)
  toJSON (Internal (QueueId queue) (Operation operation) sources) = object
    ["direction" .= Incoming, "queue" .= queue, "operation" .= operation, "saclac_internal" .= object ["sources" .= sources]]
instance FromJSON Declaration where
  parseJSON = withObject "payload projection" $ \row -> do
    schema <- row .: "schema" :: J.Parser Int
    source <- row .: "source_commit"
    messages <- row .: "messages"
    channels <- row .:? "channels" .!= []
    causal <- row .:? "causal_pairs" .!= []
    let key message = (messageAddress message, messageDiscriminator message)
        unique [] = True
        unique (message : rest) = all ((/= key message) . key) rest && unique rest
        selected rows = and
          [ isJust (messageDiscriminator message)
            || length (filter ((== messageAddress message) . messageAddress) rows) == 1
          | message <- rows ]
    unless (schema == 1 && not (T.null source) && unique messages && selected messages)
      (fail "invalid, duplicate or undiscriminated message projection")
    pure (Declaration source messages channels causal)

messageDiscriminator :: Message -> Maybe Discriminator
messageDiscriminator (Message _ _ _ _ _ _ discriminator) = discriminator
messageDiscriminator Internal {} = Nothing

messageAddress :: Message -> (Direction, QueueId, Operation)
messageAddress (Message direction queue operation _ _ _ _) = (direction, queue, operation)
messageAddress (Internal queue operation _) = (Incoming, queue, operation)

-- | The little-endian word of @width@ bytes at an offset of a payload.
wordAt :: Int -> Int -> [Word8] -> Word32
wordAt offset width bytes =
  sum [fromIntegral byte * 256 ^ index | (index, byte) <- zip [0 :: Int ..] (take width (drop offset bytes))]

parseDeclaration :: B.ByteString -> Either String Declaration
parseDeclaration = eitherDecodeStrict'

declarationJson :: Declaration -> Value
declarationJson (Declaration source messages channels causal) = object
  ["schema" .= (1 :: Int), "source_commit" .= source, "messages" .= messages
  , "channels" .= channels, "causal_pairs" .= causal]

-- | An owned identity projects to its declared stock value once the word is that value in the
-- stock role, or the declared replacement symbol's address in the owned image's own symbol table.
-- Any other word is refused: a residual stock value or an unsymbolized word in an owned role,
-- and anything but the declared value in the stock role.
projectPayload :: Role -> Declaration -> Direction -> QueueId -> Operation -> [Word8] -> Either String [Word8]
projectPayload role (Declaration _ messages _ _) direction queue operation payload = do
  let candidates = filter ((== (direction, queue, operation)) . messageAddress) messages
  unless (not (null candidates)) (Left "message has no field declaration")
  selected <- maybe (Left "message discriminator matches no declaration") Right (find (`selectedBy` payload) candidates)
  case selected of
    Internal {} -> pure []
    Message _ _ _ (PayloadSize size) fields _ _ -> do
      unless (length payload == size) (Left "message payload length differs from field declaration")
      let included Nothing = True
          included (Just (Exclusion (Offset offset) (Width width) values _)) =
            wordAt offset width payload `notElem` values
      concat <$> sequence [project field | field@(Field _ _ _ _ condition _) <- fields, included condition]
  where
    project (Field _ (Offset offset) (Width width) _ _ Nothing) = pure (take width (drop offset payload))
    project (Field name (Offset offset) _ _ _ (Just (Identity value symbol _))) = do
      let word = wordAt offset 4 payload
          refuse detail = Left ("owned identity " ++ T.unpack name ++ " at " ++ show offset ++ ": " ++ detail)
      expected <- case role of
        StockRole -> pure value
        OwnedRole resolve -> either (refuse . ("replacement " ++)) pure (resolve symbol)
      unless (word == expected) (refuse ("word " ++ show word ++ " is not " ++ show expected))
      pure [fromIntegral (value `div` 256 ^ index) | index <- [0 .. 3 :: Int]]
    selectedBy Internal {} _ = True
    selectedBy (Message _ _ _ _ _ _ Nothing) _ = True
    selectedBy (Message _ _ _ (PayloadSize size) _ _ (Just (Discriminator (Offset offset) value))) bytes =
      length bytes == size && wordAt offset discriminatorWidth bytes == value
