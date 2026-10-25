module Main where

import Control.Monad (unless)
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import System.Exit (exitFailure)

import RamPushLayout

check :: String -> Bool -> IO ()
check label passed = unless passed $ do
  putStrLn ("FAIL: " ++ label)
  exitFailure

main :: IO ()
main = do
  check "fixed executable offset" (runtimeExecutableBase 0x00be1a00 == 0x00be1f00)
  let alignedBases = [base | high <- [0x0090, 0x00a0, 0x00b0, 0x00c0], low <- [0, 8 .. 0xfff8], let base = high * 0x10000 + low] :: [Word32]
  let allocationExtents =
        [ runtimeInstallerBase base - base + runtimeInstallerExtent
        | low <- [0, 8 .. 0xfff8]
        , let base = 0x00c00000 + low
        ]
  check "fixed allocation contains every 8-byte-aligned low-half layout"
    (all (<= runtimeAllocationSize) allocationExtents)
  check "allocation includes aligned installer and relocated recipe tables"
    (all (\base -> runtimeInstallerBase base - base + runtimeInstallerExtent <= runtimeAllocationSize) alignedBases)
  check "the order is a permutation of every logical publication"
    (length safePublicationOrder == publicationCount
      && all (`elem` safePublicationOrder) [0 .. publicationCount - 1])
  check "bank setup publications use dependency-closed order"
    (take 69 safePublicationOrder == [0 .. 35] <> [55 .. 68] <> [88] <> [36 .. 53])
  check "Classic Chrome setup precedes all activation"
    (drop 69 (take publicationSetupCount safePublicationOrder) == [97 .. 158])
  check "root discovery follows processing and compiled-recipe activation"
    (last safePublicationOrder == 54
      && all (\index -> index `elem` take (publicationCount - 1) safePublicationOrder) [97 .. 221])
  let completeInstaller = unlines
        [ "00000000 <ram_push_install>:"
        , "  bl 0x10 <execute_operation>"
        , "  bl 0x20 <publish_mailbox>"
        , "00000030 <ram_push_rollback>:"
        , "  bl 0x10 <execute_operation>"
        , "  bl 0x20 <publish_mailbox>"
        , "00000060 <execute_operation>:"
        , "  bl 0x70 <validate_layout>"
        , "  bl 0x80 <prepare_peer>"
        , "  bl 0x90 <run_rendezvous>"
        , "000000a0 <run_rendezvous>:"
        , "  cpsid i"
        , "  bl 0xb0 <post_peer_park>"
        , "  bl 0xc0 <wait_for_signal>"
        , "  bl 0xd0 <ram_push_rendezvous_locked>"
        , "  bl 0xe0 <wait_for_signal>"
        , "  bl 0xf0 <publish_words_locked>"
        , "  cpsie i"
        , "00000100 <ram_push_rendezvous_locked>:"
        , "  bl 0x110 <validate_old_words>"
        , "  bl 0x120 <validate_rollback_words>"
        , "00000130 <publish_words_locked>:"
        , "  bl 0x140 <store_new_words>"
        , "  bl 0x150 <restore_old_words>"
        , "  bl 0x160 <synchronize_current_core>"
        , "00000170 <peer_park_action>:"
        , "  cpsid i"
        , "  bl 0x180 <load_signal>"
        , "  bl 0x190 <store_signal>"
        , "  bl 0x160 <synchronize_current_core>"
        , "  bl 0x1d0 <ram_push_cpu1_window_exit>"
        , "  cpsie i"
        , "000001a0 <peer_park_complete>:"
        , "  bl 0x190 <store_signal>"
        , "000001b0 <post_peer_park>:"
        , "  blx r4"
        , "000001c0 <ram_push_cpu0_window_exit>:"
        , "  bx lr"
        , "000001d0 <ram_push_cpu1_window_exit>:"
        , "  bx lr"
        ]
      foldedInstaller = unlines
        [ "00000000 <ram_push_install>:"
        , "  bl 0x10 <execute_operation>"
        , "  bl 0x20 <publish_mailbox>"
        , "00000048 <ram_push_rollback>:"
        , "  bl 0x50 <validate_layout>"
        ]
      wrongOrderInstaller = unlines
        [ if line == "  bl 0xd0 <ram_push_rendezvous_locked>"
            then "  bl 0xf0 <publish_words_locked>"
            else if line == "  bl 0xf0 <publish_words_locked>"
              then "  bl 0xd0 <ram_push_rendezvous_locked>"
              else line
        | line <- lines completeInstaller
        ]
  check "complete installer call closure is accepted"
    (installerHasCallClosure completeInstaller)
  check "constant-folded installer call closure is rejected"
    (not (installerHasCallClosure foldedInstaller))
  check "publication before validation is rejected"
    (not (installerHasCallClosure wrongOrderInstaller))
  check "installer without interrupt restore is rejected"
    (not (installerHasCallClosure (unlines (filter (/= "  cpsie i") (lines completeInstaller)))))
  check "installer without direct worker post is rejected"
    (not (installerHasCallClosure (unlines (filter (/= "  blx r4") (lines completeInstaller)))))
  check "name low half preserves the six stock movw words"
    (all (\base -> runtimeNameBase base .&. 0xffff == 0xb784) alignedBases)
  check "generated data never overlaps name records"
    (all (\base -> runtimeDataBase base + 0x1260 <= runtimeNameBase base) alignedBases)
  check "publication record ABI"
    ( encodePublicationRecord 0x11223344 0x55667788 0x99aabbcc 1
        == B.pack [0x44,0x33,0x22,0x11,0x88,0x77,0x66,0x55,0xcc,0xbb,0xaa,0x99,1,0,0,0]
    )
  check "direct stale fixed address is detected"
    (staleFixedReferences 0x00be1a00 0x00bebb20 (B.pack [0x00,0x1a,0xbe,0x00]) == [(0,0x00be1a00)])
  check "split movw/movt stale fixed address is detected"
    ( not (null (staleFixedReferences 0x00be1a00 0x00bebb20
        (B.pack [0x00,0x0a,0x01,0xe3,0xbe,0x00,0x40,0xe3])))
    )
  putStrLn "BuildRecipeBank RAM layout: checks passed"
