module Emulation.EvidenceMutation (Part(..), field, pathAt, modifyAt) where

import qualified Data.Aeson as J
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.Vector as V

data Part = Field K.Key | Element Int
field :: String -> Part
field = Field . K.fromString
pathAt :: error -> [Part] -> J.Value -> Either error J.Value
pathAt _ [] value = Right value
pathAt refusal (Field key:rest) (J.Object object) = maybe (Left refusal) (pathAt refusal rest) (KM.lookup key object)
pathAt refusal (Element index:rest) (J.Array values) = maybe (Left refusal) (pathAt refusal rest) (values V.!? index)
pathAt refusal _ _ = Left refusal
modifyAt :: error -> [Part] -> (J.Value -> Either error J.Value) -> J.Value -> Either error J.Value
modifyAt _ [] change value = change value
modifyAt refusal (Field key:rest) change (J.Object object) = do
  old <- maybe (Left refusal) Right (KM.lookup key object)
  new <- modifyAt refusal rest change old
  pure (J.Object (KM.insert key new object))
modifyAt refusal (Element index:rest) change (J.Array values) = do
  old <- maybe (Left refusal) Right (values V.!? index)
  new <- modifyAt refusal rest change old
  pure (J.Array (values V.// [(index,new)]))
modifyAt refusal _ _ _ = Left refusal
