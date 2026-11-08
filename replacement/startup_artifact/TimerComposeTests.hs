{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_, unless)
import qualified Data.ByteString as B
import qualified Data.Set as Set
import qualified Emulation.Image4Staging as Tools
import qualified FujiAF.Host as H
import Replacement.StartupArtifact.Adapters
import Replacement.StartupArtifact.Compose
import Replacement.StartupArtifact.Joins
import Replacement.StartupArtifact.Stage (loadCanonical)
import Replacement.StartupArtifact.Types
import System.Environment (getArgs)
import System.Exit (exitFailure)

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [encoder, rebuild, decoder, timerABin, timerASymbols, timerBBin, timerBSymbols,
     loaderABin, loaderASymbols, loaderBBin, loaderBSymbols, dispatcherBin, dispatcherSymbols,
     installerBin, installerSymbols, datPath, scratchPath] -> do
      let tools = Tools.Tools encoder rebuild decoder
      scratch <- Tools.validateScratchDirectory scratchPath
      source <- loadCanonical tools scratch datPath
      aBytes <- B.readFile timerABin
      aSymbols <- B.readFile timerASymbols
      bBytes <- B.readFile timerBBin
      bSymbols <- B.readFile timerBSymbols
      timer <- either (H.failH . show) pure
        (deriveTimer source (LinkedImage (ImageBytes aBytes) (SymbolBytes aSymbols))
          (LinkedImage (ImageBytes bBytes) (SymbolBytes bSymbols)))
      loaderA <- linked loaderABin loaderASymbols
      loaderB <- linked loaderBBin loaderBSymbols
      dispatcher <- linked dispatcherBin dispatcherSymbols
      installer <- linked installerBin installerSymbols
      startup <- either (H.failH . show) pure (deriveLoaderStartup source loaderA loaderB dispatcher installer)
      let piece kind ranges = Piece kind Set.empty [] [] ranges [] (Catalog [] [] (EnumeratedWrites [] (Premised (Premise "test"))) []) NoFacts
          publisher = piece PrimaryPublisher [Footprint (Span LoaderA (Guest 0x40000) (Guest 0x4dae0)) AdmissionRead (Citation "test")]
          record2 = piece Record2Link [Footprint (Span LoaderB (Guest 0xd0000) (Guest 0xddaa0)) AdmissionRead (Citation "test")]
          slots = piece KernelSlotBodies
            [ Footprint (Span LoaderA (Guest 0x40000) (Guest 0x4dae0)) AdmissionRead (Citation "test")
            , Footprint (Span LoaderB (Guest 0xd0000) (Guest 0xddaa0)) AdmissionRead (Citation "test") ]
      let joinFor name = case [j | j <- registeredJoins, joinName (joinKind j) == name] of
            [j] -> j
            _ -> error (show name ++ " is not registered once")
          cases =
            [ ("timer-primary-publisher", joinFor "timer-primary-publisher", publisher)
            , ("timer-record2-link", joinFor "timer-record2-link", record2)
            , ("timer-loader-startup", joinFor "timer-loader-startup", startup)
            , ("timer-kernel-slot-bodies", joinFor "timer-kernel-slot-bodies", slots) ]
      forM_ cases $ \(name, join, other) ->
        case compose [join] source [timer, other] of
          Left refusal -> H.failH (show name ++ " refused: " ++ show refusal)
          Right composition -> do
            let names = map (joinName . joinKind . appliedJoin) (compositionJoins composition)
            unless (names == [name]) (putStrLn (show name ++ " was not the applied join") >> exitFailure)
            unless (length (compositionPieces composition) == 2) (putStrLn (show name ++ " lost a piece") >> exitFailure)
      putStrLn "timer-compose-tests: passed"
    _ -> H.failH "usage: timer-compose-tests ENCODER REBUILD DECODER TIMER_A_BIN TIMER_A_SYMBOLS TIMER_B_BIN TIMER_B_SYMBOLS LOADER_A_BIN LOADER_A_SYMBOLS LOADER_B_BIN LOADER_B_SYMBOLS DISPATCHER_BIN DISPATCHER_SYMBOLS INSTALLER_BIN INSTALLER_SYMBOLS CANONICAL_DAT SCRATCH_DIR"
  where
    linked image symbols = LinkedImage <$> (ImageBytes <$> B.readFile image) <*> (SymbolBytes <$> B.readFile symbols)
