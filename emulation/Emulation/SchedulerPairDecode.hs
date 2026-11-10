{-# LANGUAGE BangPatterns #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairDecode (decodeCheckedObservation, decodeControlObservation, foldObservation) where

import qualified Data.Aeson as J
import Data.Aeson.Decoding (toEitherValue)
import Data.Aeson.Decoding.ByteString.Lazy (lbsToTokens)
import Data.Aeson.Decoding.Tokens
import qualified Data.Aeson.KeyMap as K
import qualified Data.ByteString.Lazy as L
import qualified Data.Map.Strict as M
import qualified Data.Set as S
import Emulation.SchedulerPairCheck
import Emulation.SchedulerPairEvidence
  ( CallEvidence, emptyCallEvidence, accumulateEvidence
  , ControlEvidence, emptyControlEvidence, accumulateControlEvidence )
import Emulation.SchedulerPairModel (Memory, Arguments (..))
import Emulation.SchedulerPairPlan (Address, Plan, Semantic (..))
import qualified Emulation.SchedulerPairS2 as D

data Classes s = MissingClasses | ParsedClasses s | InvalidClasses String

decodeCheckedObservation :: Plan -> L.ByteString -> Either String (Observation, CallEvidence)
decodeCheckedObservation plan = foldObservation (accumulateEvidence plan) emptyCallEvidence

decodeControlObservation :: Plan -> L.ByteString -> Either String (Observation, ControlEvidence)
decodeControlObservation plan = foldObservation (accumulateControlEvidence plan) (emptyControlEvidence plan)

foldObservation :: (s -> Call -> s) -> s -> L.ByteString -> Either String (Observation, s)
foldObservation step initial bytes = case lbsToTokens bytes of
  TkRecordOpen members -> fields K.empty MissingClasses members
  TkErr failure -> Left failure
  _ -> Left "expected a scheduler observation object"
 where
  fields !metadata !classes members = case members of
    TkPair "classes" tokens -> case classes of
      MissingClasses -> case tokens of
        TkArrayOpen items -> do
          (decoded, next) <- arrayCalls step initial items
          fields metadata (either InvalidClasses ParsedClasses decoded) next
        _ -> do
          next <- skipValue tokens
          fields metadata (InvalidClasses "classes must be an array") next
      _ -> do
        next <- skipValue tokens
        fields metadata classes next
    TkPair key tokens
      | K.member key metadata -> do
          next <- skipValue tokens
          fields metadata classes next
      | otherwise -> do
          (value, next) <- toEitherValue tokens
          fields (K.insert key value metadata) classes next
    TkRecordEnd remaining -> do
      if L.all (`elem` [9,10,13,32]) remaining then pure () else Left "trailing scheduler observation data"
      case classes of
        MissingClasses -> Left "missing scheduler observation classes"
        InvalidClasses failure -> Left failure
        ParsedClasses folded -> case J.fromJSON (J.Object (K.insert "classes" (J.Array mempty) metadata)) of
          J.Error failure -> Left failure
          J.Success observation -> Right (observation, folded)
    TkRecordErr failure -> Left failure

arrayCalls :: (s -> Call -> s) -> s -> TkArray k String -> Either String (Either String s, k)
arrayCalls step initial = loop (Right initial)
 where
  loop !calls items = case items of
    TkItem tokens -> do
      (value, next) <- toEitherValue tokens
      case calls of
        Left failure -> loop (Left failure) next
        Right prior -> case J.fromJSON value of
          J.Error failure -> loop (Left failure) next
          J.Success call -> forceCall call `seq` let !updated = step prior call in loop (Right updated) next
    TkArrayEnd next -> Right (calls, next)
    TkArrayErr failure -> Left failure

skipValue :: Tokens k String -> Either String k
skipValue tokens = case tokens of
  TkLit _ next -> Right next
  TkText _ next -> Right next
  TkNumber _ next -> Right next
  TkArrayOpen items -> array items
  TkRecordOpen members -> record members
  TkErr failure -> Left failure
 where
  array items = case items of
    TkItem value -> skipValue value >>= array
    TkArrayEnd next -> Right next
    TkArrayErr failure -> Left failure
  record members = case members of
    TkPair _ value -> skipValue value >>= record
    TkRecordEnd next -> Right next
    TkRecordErr failure -> Left failure

forceList :: (a -> ()) -> [a] -> ()
forceList force = foldl' (\() value -> force value) ()

forceMaybe :: (a -> ()) -> Maybe a -> ()
forceMaybe _ Nothing = ()
forceMaybe force (Just value) = force value

forceScalar :: a -> ()
forceScalar value = value `seq` ()

forceMemory :: Memory -> ()
forceMemory = M.foldlWithKey' (\() address value -> address `seq` value `seq` ()) ()

forceWritten :: Written -> ()
forceWritten (Written address size value) = address `seq` size `seq` value `seq` ()

forceOrigin :: WriteOrigin -> ()
forceOrigin (WriteOrigin core pc sp cpsr task context instructions) =
  core `seq` pc `seq` sp `seq` cpsr `seq` task `seq` context `seq` instructions `seq` ()

forceRegisters :: D.Registers -> ()
forceRegisters (D.Registers values cpsr thread access exception) =
  forceList forceScalar values `seq` cpsr `seq` thread `seq` access `seq` exception `seq` ()

forceSet :: S.Set Address -> ()
forceSet = S.foldl' (\() address -> address `seq` ()) ()

forceBoundary :: Boundary -> ()
forceBoundary (Boundary kind address instructions core registers memory writes bank value task origins bankOrigins unreadable) =
  kind `seq` address `seq` instructions `seq` core `seq`
  forceRegisters registers `seq` forceMemory memory `seq`
  forceList forceWritten writes `seq` forceList forceWritten bank `seq`
  forceMaybe forceScalar value `seq` task `seq` forceList forceOrigin origins `seq` forceList forceOrigin bankOrigins `seq`
  forceSet unreadable

forceTrace :: CallTrace -> ()
forceTrace (CallTrace before after completion history origin) = forceRegisters before `seq` forceRegisters after `seq`
  completion `seq` forceList forceBoundary history `seq` forceMaybe forceScalar origin

forceSemantic :: Semantic -> ()
forceSemantic (HelperSemantic helper) = helper `seq` ()
forceSemantic (DispatchSemantic point) = point `seq` ()
forceSemantic DoorbellSemantic = ()
forceSemantic (ServiceSemantic service) = service `seq` ()
forceSemantic (TaskRecordSemantic point) = point `seq` ()
forceSemantic (FamilySemantic service) = service `seq` ()
forceSemantic (SemaphoreSemantic service) = service `seq` ()

-- Classes are fully evaluated before advancing the token stream.
forceCall :: Call -> ()
forceCall (Call domain address semantic core cpsr (Arguments arguments) before after r0 returnedCpsr writes bank count first trace task context stack origins bankOrigins unreadable) =
  domain `seq` address `seq` forceSemantic semantic `seq` core `seq` cpsr `seq`
  forceList forceScalar arguments `seq` forceMemory before `seq` forceMemory after `seq`
  r0 `seq` returnedCpsr `seq` forceList forceWritten writes `seq` forceList forceWritten bank `seq`
  count `seq` first `seq` forceMaybe forceTrace trace `seq` task `seq` context `seq`
  forceMaybe forceStack stack `seq` forceList forceOrigin origins `seq` forceList forceOrigin bankOrigins `seq`
  forceSet unreadable
 where
  forceStack (TaskStack low high) = low `seq` high `seq` ()
