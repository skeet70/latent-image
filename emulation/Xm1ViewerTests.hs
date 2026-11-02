module Xm1ViewerTests (main) where

import Control.Exception (IOException, try)
import Control.Monad (forM_, unless)
import qualified Main as Viewer
import Xm1RamPushE2E (ModalDiagnostic (..), ModalLifetimeGuard (..))

check :: String -> Bool -> IO ()
check label accepted = unless accepted (ioError (userError label))

acceptedOptions :: [String] -> IO Viewer.Options
acceptedOptions arguments = do
  command <- Viewer.parseCommand arguments
  case command of
    Viewer.Launch options -> pure options
    _ -> ioError (userError "expected launch configuration")

refuses :: String -> [String] -> IO ()
refuses label arguments = do
  result <- try (Viewer.parseCommand arguments) :: IO (Either IOException Viewer.Command)
  check label (either (const True) (const False) result)

main :: IO ()
main = do
  let stock = ["--ram-push-e2e", "--cardless", "--fujimod", "--unreviewed-relink"]
      guard = "--modal-lifetime-guard"
      scoped = "--ram-push-modal-scoped"
  defaults <- acceptedOptions []
  check "default viewer has no lifetime guard" (Viewer.modalLifetimeGuardOption defaults == NoModalLifetimeGuard)
  unguarded <- acceptedOptions stock
  check "default 223 invocation remains unguarded" (Viewer.modalLifetimeGuardOption unguarded == NoModalLifetimeGuard)
  control <- acceptedOptions (stock ++ [guard])
  check "stock control selects continuous lifetime guard" (Viewer.modalLifetimeGuardOption control == ContinuousModalLifetimeGuard)
  forM_ [stock ++ [scoped], scoped : stock] $ \arguments -> do
    candidate <- acceptedOptions arguments
    check "scoped 224 needs no explicit lifetime flag" (Viewer.modalLifetimeGuardOption candidate == NoModalLifetimeGuard)
    check "scoped 224 retains its payload selection" (Viewer.ramPayloadOption candidate == Viewer.BankModal75Choice)
  forM_ [stock ++ [scoped, guard], guard : scoped : stock] $ \arguments -> do
    candidate <- acceptedOptions arguments
    check "explicit guard agrees with scoped selection" (Viewer.modalLifetimeGuardOption candidate == ContinuousModalLifetimeGuard)
  _ <- acceptedOptions (stock ++ [guard, "--fresh-boot", "--trajectories", "selected-route.json"])
  diagnostic <- acceptedOptions
    ["--ram-push-e2e", "--cardless", "--fujimod", "--flash-candidate", "--modal-observation", "--trajectories", "retained-route.json"]
  check "retained no-install diagnostic stays separate"
    (Viewer.modalDiagnosticOption diagnostic == RetainedFilmModalDiagnostic && Viewer.modalLifetimeGuardOption diagnostic == NoModalLifetimeGuard)
  forM_ [guard, scoped] $ \mode -> do
    forM_
      [ ("requires RAM-push E2E", filter (/= "--ram-push-e2e") stock ++ [mode])
      , ("requires cardless", filter (/= "--cardless") stock ++ [mode])
      , ("requires FujiMod", filter (/= "--fujimod") stock ++ [mode])
      , ("requires unreviewed enforced allowlist", filter (/= "--unreviewed-relink") stock ++ [mode])
      , ("refuses allowlist override", stock ++ [mode, "--allowlist-override", "test control"])
      , ("refuses virtual card", stock ++ [mode, "--sd-card-image", "unopened-card.img"])
      , ("refuses no-install", stock ++ [mode, "--flash-candidate"])
      , ("refuses retained observation", stock ++ [mode, "--modal-observation"])
      , ("refuses warm-only", stock ++ [mode, "--warm-checkpoint"])
      , ("refuses segmented route", stock ++ [mode, "--segments", "selected"])
      , ("refuses rehearsal", stock ++ [mode, "--ff80-rehearsal"])
      , ("refuses duplicate mode", stock ++ [mode, mode])
      ] $ \(label, arguments) -> refuses (mode ++ " " ++ label) arguments
    forM_
      [ ["--classic-chrome"]
      , ["--classic-chrome-icon"]
      , ["--experiment", "hello"]
      ] $ \payload -> do
        refuses (mode ++ " refuses other payload after selection") (stock ++ [mode] ++ payload)
        refuses (mode ++ " refuses other payload before selection") (stock ++ payload ++ [mode])
  forM_ [[guard, "--ram-push-lifecycle"], ["--ram-push-lifecycle", guard], [scoped, "--ram-push-lifecycle", guard], [guard, "--ram-push-lifecycle", scoped]] $ \modes ->
    refuses "explicit lifetime guard refuses lifecycle payloads" (stock ++ modes)
  forM_ [[scoped, "--ram-push-lifecycle"], ["--ram-push-lifecycle", scoped]] $ \modes ->
    refuses "automatic scoped lifetime guard refuses lifecycle composition" (stock ++ modes)
  _ <- acceptedOptions (stock ++ ["--ram-push-lifecycle"])
  refuses "observation conflict refuses in reverse order" (stock ++ ["--modal-observation", guard])
  refuses "scoped observation conflict refuses in reverse order" (stock ++ ["--modal-observation", scoped])
  refuses "duplicate retained mode refuses" ["--modal-observation", "--modal-observation"]
  refuses "duplicate no-install mode refuses" ["--flash-candidate", "--flash-candidate"]
  refuses "lifetime guard is a closed flag" (stock ++ ["--modal-lifetime-guard=continuous"])
  help <- Viewer.parseCommand [guard, "--help"]
  check "help opens no inputs and requires no route" (case help of Viewer.Help -> True; _ -> False)
