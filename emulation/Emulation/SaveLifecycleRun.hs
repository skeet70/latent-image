{-# LANGUAGE OverloadedStrings #-}
module Emulation.SaveLifecycleRun (runSaveLifecycle) where

import Control.Exception (bracket)
import Control.Monad (unless, when)
import Data.Aeson (Value (..), encode, object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson.Types as JT
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import qualified Data.Text as T
import qualified Emulation.CaptureStream as C
import qualified Emulation.SaveLifecycle as L
import EmulatorUsb
import System.Directory (createDirectoryIfMissing)
import System.FilePath ((</>))

newtype BridgeFailure = BridgeFailure T.Text


data Reply = Answer Value | Refused BridgeFailure
parseReply :: Value -> Either String Reply
parseReply value = JT.parseEither (withObject "capture reply" $ \row -> do
  ok <- row .:? "ok"
  case ok of
    Just False -> Refused . BridgeFailure <$> row .: "message"
    _ -> pure (Answer value)) value

data Progress = Progress
  { cursor :: C.Cursor
  , coverage :: C.Coverage
  , observer :: L.Observer
  , snapshots :: [Value]
  }

data CaptureEnd = WindowComplete | BridgeRefused BridgeFailure

eitherIO :: Either String a -> IO a
eitherIO = either (ioError . userError) pure
lifecycleIO :: Either L.Refusal a -> IO a
lifecycleIO = either (ioError . userError . show) pure
require :: Bool -> String -> IO ()
require condition message = unless condition (ioError (userError message))

snapshotReply :: Value -> Either String (Value, C.Instructions)
snapshotReply = JT.parseEither $ withObject "capture state" $ \row -> do
  snapshot <- row .: "snapshot"
  at <- withObject "capture snapshot" (.: "instructions") snapshot
  pure (snapshot,at)

runSaveLifecycle :: Emulator -> FilePath -> L.Source -> L.Program -> IO Value
runSaveLifecycle emulator output source program = do
  createDirectoryIfMissing True output
  BL.writeFile (output </> "source.json") (encode (L.sourceValue source) <> "\n")
  BL.writeFile (output </> "scope.json") (encode (L.scopeValue program) <> "\n")
  caller <- L.CallerCode
    <$> emulatorReadMemory emulator 0xe99e8c 4
    <*> emulatorReadMemory emulator 0xe6a754 4
    <*> emulatorReadMemory emulator 0xe6bb90 4
  BL.writeFile (output </> "caller-code.json") (encode (object
    ["save" .= B.unpack (L.savePublicationCode caller)
    ,"permit_a" .= B.unpack (L.permitPublicationACode caller)
    ,"permit_b" .= B.unpack (L.permitPublicationBCode caller)]) <> "\n")
  lifecycleIO (L.validateCallerCode source program caller)
  BL.writeFile (output </> "trace-sites.json") (encode (L.traceSitesValue source program) <> "\n")
  emulatorTraceSites emulator [(L.traceSiteAddress site,L.traceSiteLabel site) | site <- L.traceRegistrations source program]
  bracket (C.openJournal (output </> "observations.jsonl")) C.closeJournal $ \journal -> do
    beginning <- exchange journal "capture-observation-begin" (BeginSaveLifecycle program)
    accepted <- answer beginning
    lifecycleIO (L.validateBegin program accepted)
    (initialSnapshot,at) <- exchange journal "capture-observation-state" CaptureState >>= answer >>= eitherIO . snapshotReply
    initial <- lifecycleIO (L.observeSnapshot source program L.initialObserver initialSnapshot)
    let start = Progress C.initialCursor (C.startCoverage at) initial [initialSnapshot]
    drained <- drain journal start
    (pressed,pressEnd) <- advance journal C.ShutterSequence ShutterSequence drained
    (finished,ending) <- case pressEnd of
      Just failure -> pure (pressed,BridgeRefused failure)
      Nothing -> do
        afterPress <- snapshot journal pressed
        (window,windowEnd) <- advance journal C.Window CaptureWindow afterPress
        pure (window,maybe WindowComplete BridgeRefused windowEnd)
    final <- snapshot journal finished
    let result = object
          ["kind" .= ("source-bound-lifecycle-prefix" :: T.Text)
          ,"full_save_qualified" .= False
          ,"end" .= endValue ending
          ,"coverage" .= C.coverageValue (coverage final)
          ,"observation" .= L.observationValue (observer final)]
    BL.writeFile (output </> "snapshots.json") (encode (reverse (snapshots final)) <> "\n")
    BL.writeFile (output </> "result.json") (encode result <> "\n")
    C.synchronizeJournal journal
    pure result
  where
    exchange journal label command = do
      value <- emulatorCaptureCommand emulator command
      C.note journal label value
      eitherIO (parseReply value)
    answer reply = case reply of
      Answer value -> pure value
      Refused (BridgeFailure message) -> ioError (userError (T.unpack message))
    snapshot journal progress = do
      (raw,_) <- exchange journal "capture-observation-state" CaptureState >>= answer >>= eitherIO . snapshotReply
      next <- lifecycleIO (L.observeSnapshot source program (observer progress) raw)
      require (length (snapshots progress) < 3) "lifecycle snapshot bound exceeded"
      pure progress {observer=next,snapshots=raw:snapshots progress}
    drain journal progress = do
      raw <- emulatorCaptureCommand emulator (CapturePage (cursor progress))
      (next,page) <- lifecycleIO (L.consumePageWithMetadata source program (observer progress) raw)
      C.note journal "capture-observation-page" raw
      C.synchronizeJournal journal
      collected <- eitherIO (C.collectBoundary page (coverage progress))
      let pending = C.pagePending page - fromIntegral (length (C.pageHits page))
          complete = null (C.pageHits page) && pending == 0 && C.pageBoundaryComplete page
          nextProgress = progress {cursor=C.pageNext page,coverage=collected,observer=next}
      when (not (null (C.pageHits page))) $ do
        acknowledgment <- exchange journal "capture-observation-ack" (AcknowledgeCapture (C.pageNext page)) >>= answer
        eitherIO (C.validateAck (C.pageNext page) pending acknowledgment)
      if complete then pure nextProgress else do
        require (not (null (C.pageHits page))) "capture boundary is incomplete without a drainable page"
        drain journal nextProgress
    advance journal operation command progress = do
      opened <- eitherIO (C.openInterval operation (coverage progress))
      step command progress {coverage=opened}
      where
        step input current = do
          reply <- exchange journal "capture-observation-progress" input
          case reply of
            Refused failure -> do
              tailProgress <- drain journal current
              pure (tailProgress,Just failure)
            Answer raw -> do
              advancement <- eitherIO (C.parseAdvance operation raw)
              eitherIO (C.validateAdvance advancement (coverage current))
              collected <- drain journal current
              require (C.coverageObserved (coverage collected) == C.advanceInstructions advancement)
                "capture reply and drained boundary disagree"
              case C.advanceState advancement of
                C.Completed -> pure (collected,Nothing)
                C.DrainRequired -> do
                  require (C.cursorValue (cursor collected) > C.cursorValue (cursor current))
                    "capture continuation made no drain progress"
                  step ResumeCapture collected
    endValue ending = case ending of
      WindowComplete -> object ["kind" .= ("bounded-window-complete" :: T.Text)]
      BridgeRefused (BridgeFailure message) -> object
        ["kind" .= ("bridge-refused" :: T.Text),"message" .= message]
