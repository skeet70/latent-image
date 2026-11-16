module Main (main) where

import A32 (decode)
import Control.Monad (unless)
import Data.Word (Word32)
import ExperimentHook (positionIndependent, HookMode (..), HookAdmission (..), bindReplacement, admitHook, entryPublications, originalBody)
import PlaybackContract (PlaybackEntry (..), SourceSha256 (..), playbackContract, jpegEntries)
import Numeric (showHex)
import System.Exit (exitFailure)

-- | (address, word, may it be displaced?)
cases :: [(Word32, Word32, Bool)]
cases =
  [ (0x00bfb028, 0xe92d4ff0, True) -- push {r4-r11, lr}
  , (0x00bfb02c, 0xe28db020, True) -- add r11, sp, #32
  , (0x00f41c48, 0xe92d48f0, True) -- push {r4-r7, r11, lr}
  , (0x00001000, 0xe1a04000, True) -- mov r4, r0
  , (0x00001000, 0xe59f3010, False) -- ldr r3, [pc, #16]
  , (0x00001000, 0xe28f0008, False) -- add r0, pc, #8
  , (0x00001000, 0xeb000010, False) -- bl
  , (0x00001000, 0xea000010, False) -- b
  , (0x00001000, 0xe12fff1e, False) -- bx lr
  , (0x00001000, 0xe8bd8010, False) -- pop {r4, pc}
  , (0x00001000, 0xe1a0f00e, False) -- mov pc, lr
  ]

main :: IO ()
main = do
  let failures = [(address, word, expected) | (address, word, expected) <- cases, positionIndependent (decode address word) /= expected]
  mapM_ (\(address, word, expected) -> putStrLn ("0x" ++ showHex word "" ++ " at 0x" ++ showHex address "" ++ ": expected displaceable=" ++ show expected)) failures
  unless (null failures) exitFailure
  let check label result = unless result (putStrLn label >> exitFailure)
      sourceHash = SourceSha256 "90da01fffdecd496e473c74aa012c9028be5d2dad33a1b8f4a95b4ea7c466d32"
      modeHash = SourceSha256 "d9b5d69e40838f9bcecc142fb1bfeaefa18e43b790a027e0442d7b31deef0f0c"
      source site handler entry end digest = bindReplacement site handler entry end digest
      refused (Left _) = True
      refused _ = False
  check "Observe remains admitted by the live builder" (admitHook ObserveOnly Observe == Right ())
  check "Observe publication retains its trampoline target"
    (entryPublications 0x1000 0xe1a04000 0xe1a05001 0xc40100
      == ((0x1004, 0xe1a05001, 0xc40100), (0x1000, 0xe1a04000, 0xe51ff004)))
  check "Observe excludes no original body" (originalBody Observe == Nothing)
  check "source replacement binds the exact supported contract"
    (source 0x0120b40c "xm1_playback_source_entry" 0x0120b40c 0x0120b778 sourceHash == Right (ReplaceEntry PlaybackSource))
  check "mode replacement binds the exact supported contract"
    (source 0x0120cab0 "xm1_playback_mode_entry" 0x0120cab0 0x0120cd74 modeHash == Right (ReplaceEntry PlaybackMode))
  check "replacement requires explicit emulator admission"
    (refused (admitHook ObserveOnly (ReplaceEntry PlaybackSource)) && admitHook EmulatorOnly (ReplaceEntry PlaybackSource) == Right ())
  check "replacement publication uses the owned entry and preserves rollback words"
    (entryPublications 0x0120b40c 0xe92d48f0 0xe28db010 0xc40200
      == ((0x0120b410, 0xe28db010, 0xc40200), (0x0120b40c, 0xe92d48f0, 0xe51ff004)))
  check "source body exclusion preserves both publication words"
    (originalBody (ReplaceEntry PlaybackSource) == Just (0x0120b414, 0x0120b778))
  check "mode body exclusion preserves both publication words"
    (originalBody (ReplaceEntry PlaybackMode) == Just (0x0120cab8, 0x0120cd74))
  mapM_ (check "unsupported replacement contract was accepted" . refused)
    [ source 0x0120b410 "xm1_playback_source_entry" 0x0120b40c 0x0120b778 sourceHash
    , source 0x0120b40c "other_handler" 0x0120b40c 0x0120b778 sourceHash
    , source 0x0120b40c "xm1_playback_source_entry" 0x0120cab0 0x0120b778 sourceHash
    , source 0x0120b40c "xm1_playback_source_entry" 0x0120b40c 0x0120b77c sourceHash
    , source 0x0120b40c "xm1_playback_source_entry" 0x0120b40c 0x0120b778 modeHash
    ]
  mapM_ (\entry -> do
    let (start,end,digest,handler) = playbackContract entry
    check "JPEG replacement binds its exact source contract" (source start handler start end digest == Right (ReplaceEntry entry))
    check "JPEG replacement remains emulator-only" (refused (admitHook ObserveOnly (ReplaceEntry entry)))
    check "JPEG body exclusion starts after both publication words" (originalBody (ReplaceEntry entry) == Just (start+8,end))
    mapM_ (check "invalid JPEG replacement admitted" . refused)
      [ source (start+4) handler start end digest
      , source start "wrong_handler" start end digest
      , source start handler (start+4) end digest
      , source start handler start (end+4) digest
      , source start handler start end sourceHash ]) jpegEntries
  putStrLn ("experiment hook: " ++ show (length cases) ++ " cases")
