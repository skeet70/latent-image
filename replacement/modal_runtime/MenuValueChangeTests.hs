{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value, object, (.=))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Data.Text as T
import Data.Word (Word32)
import Replacement.ModalRuntime.MenuEnginePair (Hit (..), Site (..), ValueSite (..))
import Replacement.ModalRuntime.ValueChange
import System.Exit (exitFailure)

stockCallback, ownedCallback :: Word32
stockCallback = 0x00f3a834
ownedCallback = 0x00a9c120

le :: Word32 -> B.ByteString
le value = B.pack [fromIntegral (value `div` (256 ^ index)) | index <- [0 .. 3 :: Int]]

hit :: Site -> (Word32, Word32, Word32, Word32) -> [B.ByteString] -> Hit
hit site registers memory = Hit site 0 registers (Just 0x01029460) Nothing memory Nothing Nothing

-- The `0x01029188` reads: four bytes before the header, the twelve-byte header and the seven
-- payload words with the callback last; the callback cell, the pending block and the state.
receive :: Word32 -> Word32 -> Hit
receive callback prior = hit (OtherSite "pipeline-4010-receive") (0, 0, 0, 0)
  [ B.replicate 4 0 <> B.concat (map le [0x4010, 0x1c, 0x30]) <> B.concat (map le [1, 2, 3, 4, 0, 0x00f124b6, callback]) <> B.replicate 4 0
  , le prior, le 0 <> B.replicate 24 0, le 4 ]

complete :: Word32 -> Word32 -> Hit
complete state callback = hit (OtherSite "pipeline-4010-complete") (0, 0, 0, 0) [le state, le callback]

invoked :: Word32 -> Hit
invoked status = hit (OtherSite "pipeline-4010-callback") (status, 0, 0, 0) []

notify :: Word32 -> Hit
notify kind = hit (ValueSite ValueNotify) (kind, 1, 0x1d, 0xf6) []

submit :: Word32 -> Hit
submit callback = hit (OtherSite "pipeline-4010-submit") (0x00a4c9f0, callback, 0, 0) []

pendingRead :: Word32 -> Word32 -> [(T.Text, B.ByteString)]
pendingRead state callback =
  [ ("pipeline-pending", le state <> B.replicate 24 0), ("pipeline-callback", le callback)
  , ("pipeline-completion-mask", le 0x100 <> le 0x300) ]

segment :: T.Text -> T.Text -> (Int, Int, Int) -> [(T.Text, B.ByteString)] -> [Hit] -> SegmentRecord
segment phase key state = SegmentRecord phase [Landing key state]

-- DOWN submits with the role's callback; the half-press segment completes it and then closes.
role :: Word32 -> [Hit] -> Terminal -> RoleRecord
role callback closeHits terminal = RoleRecord
  [ segment "value-change" "down" (1, 246, 1) (pendingRead 1 callback) [submit callback, receive callback 0, complete 1 callback]
  , segment "value-close" "half-press" (1, 246, 1) (pendingRead 0 0) closeHits ]
  (Just [SettingsWrite 0x0096cd32 1 [0x89] 0xa9 "setter"])
  terminal

completion :: Word32 -> [Hit]
completion callback = [complete 2 callback, invoked 0, notify 2]

finished :: Terminal
finished = Finished [("install", "PASS"), ("trajectory", "PASS"), ("unload", "PASS")]

declared :: Correspondence
declared = either error id (correspondence [CallbackPair (CallbackWord stockCallback) (CallbackWord ownedCallback) "posts saclac 0x71"])

none :: Correspondence
none = either error id (correspondence [])

passes :: Correspondence -> RoleRecord -> RoleRecord -> Bool
passes table stock owned = fst (compareRoles table stock owned)

stockRole :: RoleRecord
stockRole = role stockCallback (completion stockCallback) finished

-- Adds an IMAGE SIZE apply hit at `site` to the DOWN segment.
withApply :: Site -> RoleRecord -> RoleRecord
withApply site record = record { roleSegments = map addApply (roleSegments record) }
 where
  addApply s
    | segmentPhase s == "value-change" = s { segmentHits = hit site (0, 0, 0, 0) [] : segmentHits s }
    | otherwise = s

ownedCallbacks :: Correspondence -> RoleRecord -> RoleRecord -> [CallbackRef]
ownedCallbacks table stock owned = verdictCallbacks (snd (compareRoles table stock owned))

document :: [Value] -> Value
document callbacks = object ["schema" .= (1 :: Int), "callbacks" .= callbacks]

entry :: T.Text -> T.Text -> Value
entry stock owned = object ["stock" .= stock, "owned" .= owned, "invocation" .= ("posts saclac 0x71" :: T.Text)]

cases :: [(String, Bool)]
cases =
  [ ("identical roles pass", passes none stockRole stockRole)
  , ("a declared owned callback passes", passes declared stockRole (role ownedCallback (completion ownedCallback) finished))
  , ("an undeclared owned callback fails", not (passes none stockRole (role ownedCallback (completion ownedCallback) finished)))
  , ("a declared callback in the wrong completion fails"
    , not (passes declared stockRole (role ownedCallback (completion stockCallback) finished)))
  , ("Close before the completion fails"
    , not (passes none stockRole (role stockCallback [notify 2, complete 2 stockCallback, invoked 0] finished)))
  , ("a missing completion fails", not (passes none stockRole (role stockCallback [notify 2] finished)))
  , ("a failure status fails", not (passes none stockRole (role stockCallback [complete 2 stockCallback, invoked 1, notify 2] finished)))
  , ("a missing submission fails"
    , not (passes none stockRole (stockRole { roleSegments = [ s { segmentHits = [] } | s <- take 1 (roleSegments stockRole) ] ++ drop 1 (roleSegments stockRole) })))
  , ("completion polls without a pending configuration are not events"
    , passes none stockRole (role stockCallback (complete 0 0 : complete 1 stockCallback : completion stockCallback) finished))
  , ("a different pending state fails"
    , not (passes none stockRole (stockRole { roleSegments = map (\s -> s { segmentReads = pendingRead 2 stockCallback }) (roleSegments stockRole) })))
  , ("the completion mask follows the schedule and is not compared"
    , passes none stockRole (stockRole { roleSegments = map (\s -> s { segmentReads = [(label, if label == "pipeline-completion-mask" then le 0 else bytes) | (label, bytes) <- segmentReads s] }) (roleSegments stockRole) }))
  , ("a different landing fails"
    , not (passes none stockRole (stockRole { roleSegments = map (\s -> s { segmentLandings = [Landing "down" (1, 5, 3)] }) (roleSegments stockRole) })))
  , ("equal unload refusals pass"
    , passes none (stockRole { roleTerminal = UnloadRefused "controller 74 state 0x18" }) (stockRole { roleTerminal = UnloadRefused "controller 74 state 0x18" }))
  , ("different terminals fail", not (passes none stockRole (stockRole { roleTerminal = UnloadRefused "controller 74 state 0x18" })))
  , ("roles without a terminal fail", not (passes none (stockRole { roleTerminal = NoTerminal }) (stockRole { roleTerminal = NoTerminal })))
  , ("settings writes recorded by one role fail", not (passes none stockRole (stockRole { roleSettingsWrites = Nothing })))
  , ("different settings writes fail"
    , not (passes none stockRole (stockRole { roleSettingsWrites = Just [SettingsWrite 0x0096cd32 1 [0x89] 0xc9 "setter"] })))
  , ("a different segment count fails", not (passes none stockRole (stockRole { roleSegments = take 1 (roleSegments stockRole) })))
  , ("the correspondence document parses", either (const False) (const True) (parseCorrespondence (document [entry "0x00f3a834" "0x00a9c120"])))
  , ("an owned word declared twice is refused"
    , isLeft (parseCorrespondence (document [entry "0x00f3a834" "0x00a9c120", entry "0x00f3a80c" "0x00a9c120"])))
  , ("a short callback word is refused", isLeft (parseCorrespondence (document [entry "0xf3a834" "0x00a9c120"])))
  , ("an owned apply against stock's arm passes"
    , passes none (withApply (ValueSite StockValueArm) stockRole) (withApply (ValueSite ValueApply) stockRole))
  , ("the owned role entering stock's replaced arm fails"
    , not (passes none (withApply (ValueSite StockValueArm) stockRole) (withApply (ValueSite StockValueArm) stockRole)))
  , ("owned callback words are reported in stock terms"
    , let reported = ownedCallbacks declared stockRole (role ownedCallback (completion ownedCallback) finished)
      in StockCallback stockCallback `elem` reported && StockCallback ownedCallback `notElem` reported)
  ]

main :: IO ()
main = do
  let failed = [name | (name, ok) <- cases, not ok]
  mapM_ (putStrLn . ("FAIL: " ++)) failed
  unless (null failed) exitFailure
  putStrLn ("menu-value-change-tests: " ++ show (length cases) ++ " cases pass")
