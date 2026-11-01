module Main where

import Control.Monad (forM_, unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Either (isLeft)
import Data.List (sort)
import Data.Word (Word32)
import Numeric (showHex)
import System.Environment (getArgs, getEnvironment)
import System.Exit (ExitCode (..), exitFailure)
import System.Process (CreateProcess (env), proc, readCreateProcessWithExitCode, readProcessWithExitCode)

import RamPushLayout

check :: String -> Bool -> IO ()
check label passed = unless passed $ do
  putStrLn ("FAIL: " ++ label)
  exitFailure

main :: IO ()
main = do
  [archivePath, builderPath, relinkerPath, bashPath] <- getArgs
  archiveDigest <- SHA256.hash <$> B.readFile archivePath
  let hexByte byte = let digits = showHex byte "" in replicate (2 - length digits) '0' ++ digits
  check "default bank archive preserves f0b8386 and #133 byte identity"
    (concatMap hexByte (B.unpack archiveDigest) == "0fb86c8296323f492efc4f3455a6d3766ab7a372e3f1bb90896c851d4c92e61a")
  environment <- getEnvironment
  forM_ ["lifecycle", "lifecycle,modal75"] $ \groups -> do
    (builderCode, _, _) <- readProcessWithExitCode builderPath
      (["--ram-push-variant", groups, "0x00c40000"] ++ replicate 7 "absent") ""
    check ("unavailable lifecycle builder selection refuses before opening inputs: " ++ groups)
      (builderCode == ExitFailure 2)
    (relinkerCode, _, _) <- readCreateProcessWithExitCode
      ((proc bashPath [relinkerPath])
        {env = Just (("RB_PUBLICATION_GROUPS", groups) : filter ((/= "RB_PUBLICATION_GROUPS") . fst) environment)}) ""
    check ("unavailable lifecycle relinker selection refuses before tool invocation: " ++ groups)
      (relinkerCode == ExitFailure 2)
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
  let testAddresses =
        [PublicationAddress (0x00100000 + 4 * index) | index <- [0 .. 221 :: Word32]]
          <> [captureHookAddress]
      order = either error id (safePublicationOrderFor testAddresses)
      addressAt index = testAddresses !! publicationIndex index
  check "the derived order names every logical publication once"
    (length order == publicationCount
      && sort (map publicationIndex order) == [0 .. publicationCount - 1]
      && sort (map addressAt order) == sort testAddresses)
  check "the capture hook is the first activation publication"
    (addressAt (order !! publicationSetupCount) == captureHookAddress
      && publicationSetupCount + publicationActivationCount == publicationCount)
  check "a missing capture hook is rejected"
    (isLeft (safePublicationOrderFor (init testAddresses)))
  check "a duplicate capture hook is rejected"
    (isLeft (safePublicationOrderFor (testAddresses <> [captureHookAddress])))
  check "a duplicate logical address is rejected"
    (isLeft (safePublicationOrderFor (take 221 testAddresses <> [PublicationAddress 0x00100000, captureHookAddress])))
  check "a missing hook record at a different extra address is rejected"
    (isLeft (safePublicationOrderFor (init testAddresses <> [PublicationAddress 0x00e9e03c])))
  check "standard variant preserves the active default layout"
    (publicationCountFor standardBankVariant == publicationCount
      && publicationActivationCountFor standardBankVariant == publicationActivationCount
      && runtimeAllocationSizeFor standardBankVariant == 0x1dc98
      && runtimeInstallerExtentFor standardBankVariant == runtimeInstallerExtent
      && safePublicationOrderForVariant standardBankVariant testAddresses == Right order)
  check "scoped counts retain setup and add one activation"
    (publicationCountFor modalScopedBankVariant == 224
      && publicationSetupCount == 131
      && publicationActivationCountFor modalScopedBankVariant == 93)
  check "scoped allocation distinguishes the installer growth"
    (runtimeInstallerExtentFor modalScopedBankVariant == runtimeInstallerExtent + 32
      && runtimeAllocationSizeFor modalScopedBankVariant == 0x1dcb8)
  check "scoped allocation contains every aligned low-half layout"
    (all (\base -> runtimeInstallerBase base - base + runtimeInstallerExtentFor modalScopedBankVariant
      <= runtimeAllocationSizeFor modalScopedBankVariant) alignedBases)
  let scopedAddresses = testAddresses <> [modalHookAddress]
      scopedOrder = either error id (safePublicationOrderForVariant modalScopedBankVariant scopedAddresses)
      scopedAddressAt index = scopedAddresses !! publicationIndex index
      scopedRefuses = isLeft . safePublicationOrderForVariant modalScopedBankVariant
  check "scoped order preserves every prior activation and installs the modal entry last"
    (scopedOrder == order <> [PublicationIndex 223]
      && sort (map publicationIndex scopedOrder) == [0 .. 223]
      && scopedAddressAt (head (reverse scopedOrder)) == modalHookAddress)
  check "scoped order refuses a missing modal hook"
    (scopedRefuses (testAddresses <> [PublicationAddress 0x00bfb02c]))
  check "scoped order refuses duplicate modal hooks"
    (scopedRefuses (modalHookAddress : tail scopedAddresses))
  check "scoped order refuses a modal hook moved from logical index 223"
    (scopedRefuses (modalHookAddress : testAddresses))
  check "scoped order refuses duplicate dependent publications"
    (scopedRefuses (head testAddresses : head testAddresses : drop 2 scopedAddresses))
  check "scoped order still requires the capture hook"
    (scopedRefuses (take 222 testAddresses <> [PublicationAddress 0x00e9e03c, modalHookAddress]))
  check "modal hook emits an unconditional A32 branch"
    (modalHookWordFor modalScopedBankVariant 0x00c40000 0x00c40500 0xe92d4ff0 == Right 0xea011534)
  check "modal hook refuses a different stock source push"
    (isLeft (modalHookWordFor modalScopedBankVariant 0x00c40000 0x00c40500 0xe92d4870))
  check "modal hook refuses a misaligned target"
    (isLeft (modalHookWordFor modalScopedBankVariant 0x00c40000 0x00c40502 0xe92d4ff0))
  check "modal hook refuses a target outside executable storage"
    (isLeft (modalHookWordFor modalScopedBankVariant 0x00c40000 0x00c4a800 0xe92d4ff0))
  check "modal hook accepts the last positive A32 branch displacement"
    (not (isLeft (modalHookWordFor modalScopedBankVariant 0x02bfab28 0x02bfb02c 0xe92d4ff0)))
  check "modal hook refuses the first out-of-domain A32 displacement"
    (isLeft (modalHookWordFor modalScopedBankVariant 0x02bfab30 0x02bfb030 0xe92d4ff0))
  let hook = publicationAddress modalHookAddress
      beforeHook = hook - runtimeAllocationSizeFor modalScopedBankVariant
  check "modal hook permits an allocation ending at its stock entry"
    (not (isLeft (modalHookWordFor modalScopedBankVariant beforeHook (beforeHook + 0x500) 0xe92d4ff0)))
  check "modal hook refuses allocation overlap at its stock entry"
    (isLeft (modalHookWordFor modalScopedBankVariant hook (hook + 0x500) 0xe92d4ff0))
  check "modal hook permits an allocation after its source word"
    (not (isLeft (modalHookWordFor modalScopedBankVariant (hook + 8) (hook + 0x508) 0xe92d4ff0)))
  check "modal hook refuses allocation address overflow"
    (isLeft (modalHookWordFor modalScopedBankVariant 0xffff0000 0xffff0500 0xe92d4ff0))
  check "publication groups are canonicalized independent of input order"
    (bankVariant [Modal75Publication, LifecyclePublications] == Right modalLifecycleBankVariant
      && publicationGroups modalLifecycleBankVariant == [LifecyclePublications, Modal75Publication])
  check "duplicate publication groups are refused"
    (isLeft (bankVariant [Modal75Publication, Modal75Publication])
      && isLeft (bankVariant [LifecyclePublications, Modal75Publication, LifecyclePublications]))
  check "an empty group selection is the active default"
    (bankVariant [] == Right standardBankVariant)
  check "lifecycle and composed variants carry distinct counts and allocations"
    (publicationCountFor lifecycleBankVariant == 226
      && publicationActivationCountFor lifecycleBankVariant == 95
      && runtimeAllocationSizeFor lifecycleBankVariant == 0x1dcd8
      && publicationCountFor modalLifecycleBankVariant == 227
      && publicationActivationCountFor modalLifecycleBankVariant == 96
      && runtimeAllocationSizeFor modalLifecycleBankVariant == 0x1dcf8)
  check "composed allocation distinguishes equal aligned installer extents"
    (runtimeInstallerExtentFor lifecycleBankVariant == runtimeInstallerExtentFor modalLifecycleBankVariant
      && runtimeAllocationSizeFor modalLifecycleBankVariant == runtimeAllocationSizeFor lifecycleBankVariant + 32)
  let lifecycleAddresses = testAddresses <> lifecycleHookAddresses
      composedAddresses = lifecycleAddresses <> [modalHookAddress]
      lifecycleOrder = either error id (safePublicationOrderForVariant lifecycleBankVariant lifecycleAddresses)
      composedOrder = either error id (safePublicationOrderForVariant modalLifecycleBankVariant composedAddresses)
  check "lifecycle activations follow every standard activation"
    (lifecycleOrder == order <> map PublicationIndex [223 .. 225])
  check "composed activation installs modal after lifecycle"
    (composedOrder == lifecycleOrder <> [PublicationIndex 226]
      && composedAddresses !! publicationIndex (head (reverse composedOrder)) == modalHookAddress)
  check "lifecycle order refuses missing or reordered hook destinations"
    (isLeft (safePublicationOrderForVariant lifecycleBankVariant (testAddresses <> reverse lifecycleHookAddresses))
      && isLeft (safePublicationOrderForVariant lifecycleBankVariant
        (testAddresses <> take 2 lifecycleHookAddresses <> [PublicationAddress 0x00e6bb94])))
  check "composed order refuses a modal hook before lifecycle hooks"
    (isLeft (safePublicationOrderForVariant modalLifecycleBankVariant
      (testAddresses <> [modalHookAddress] <> lifecycleHookAddresses)))
  check "each group allocation contains every aligned low-half layout"
    (and [runtimeInstallerBase base - base + runtimeInstallerExtentFor variant <= runtimeAllocationSizeFor variant
      | base <- alignedBases, variant <- [lifecycleBankVariant, modalLifecycleBankVariant]])
  check "modal publication is refused without its group"
    (isLeft (modalHookWordFor lifecycleBankVariant 0x00c40000 0x00c40500 0xe92d4ff0))
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
