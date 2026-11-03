module Main (main) where

import qualified A32 as A
import Control.Monad (unless)
import Data.Bits ((.&.), (.|.))
import qualified Data.ByteString as B
import qualified Fixtures as Fixture
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.KernelInstall.Admission as K
import qualified Replacement.KernelInstall.Artifact as I
import Replacement.StartupComposition.Artifact
import qualified Replacement.StartupDispatch.Artifact as D
import System.Environment (getArgs)

assert :: String -> Bool -> IO ()
assert label condition = unless condition (fail label)
accepted :: Either JoinError JoinedArtifact -> IO JoinedArtifact
accepted = either (fail . show) pure
refused :: String -> JoinError -> Either JoinError JoinedArtifact -> IO ()
refused label expected result = case result of
  Left actual -> assert label (actual == expected)
  Right _ -> fail (label ++ " admitted")

data FixtureStack = AlignedExtraFrame | MisalignedInstallEntry

fixtureAtInstallStack :: B.ByteString -> FixtureStack -> B.ByteString
fixtureAtInstallStack bytes layout =
  let patternBytes = U.putWord32 0xe320f000 <> U.putWord32 0xe3500000
      (prefix,suffix) = B.breakSubstring patternBytes bytes
      allocation = case layout of AlignedExtraFrame -> 16; MisalignedInstallEntry -> 4
  in if B.null suffix then error "synthetic fixture stack marker absent"
     else prefix <> U.putWord32 (0xe24dd000 .|. allocation) <> B.drop 4 suffix

main :: IO ()
main = do
  arguments <- getArgs
  case arguments of
    [dispatcherPath,dispatcherSymbolsPath,installerPath,installerSymbolsPath] -> do
      dispatcher <- NativeFiles <$> B.readFile dispatcherPath <*> B.readFile dispatcherSymbolsPath
      installation <- NativeFiles <$> B.readFile installerPath <*> B.readFile installerSymbolsPath
      joined <- accepted (deriveArtifact dispatcher installation)
      let proof = joinedProof joined
      assert "actual joined owned stack is S-104" (stackOffsetValue (joinedLow proof) == -104)
      assert "actual dispatcher remains S-72" (stackOffsetValue (dispatcherLow proof) == -72)
      assert "actual installer remains P-40" (stackOffsetValue (installerLow proof) == -40)
      assert "every actual Install call uses S-64" (all ((== -64) . stackOffsetValue . boundaryStack) (installationBoundaries proof))
      assert "all caller-live words are above the callee frame" (all (\boundary -> all
        ((>= stackOffsetValue (boundaryStack boundary)) . stackOffsetValue) (boundaryLiveWords boundary)) (installationBoundaries proof))
      let (fixtureBytes,fixtureSymbols) = Fixture.carrier Fixture.Control
          baseline = NativeFiles fixtureBytes fixtureSymbols
          deeper = NativeFiles (fixtureAtInstallStack fixtureBytes AlignedExtraFrame) fixtureSymbols
          misaligned = NativeFiles (fixtureAtInstallStack fixtureBytes MisalignedInstallEntry) fixtureSymbols
      baselineJoined <- accepted (deriveArtifact baseline installation)
      assert "synthetic S-48 installer call composes to S-88" (stackOffsetValue (joinedLow (joinedProof baselineJoined)) == -88)
      deeperJoined <- accepted (deriveArtifact deeper installation)
      assert "synthetic S-64 installer call composes to S-104" (stackOffsetValue (joinedLow (joinedProof deeperJoined)) == -104)
      assert "component admits word-aligned but not eight-byte-aligned fixture" (case D.deriveArtifact (nativeBytes misaligned) (nativeSymbols misaligned) of Right _ -> True; Left _ -> False)
      refused "joined cache ABI rejects misaligned Install entry" InstallAlignmentRejected (deriveArtifact misaligned installation)
      refused "dispatcher image cannot be omitted" (DispatcherRejected D.ExtentRejected) (deriveArtifact (dispatcher {nativeBytes = B.empty}) installation)
      refused "installer image cannot be omitted" (InstallerRejected I.ExtentRejected) (deriveArtifact dispatcher (installation {nativeBytes = B.empty}))
      refused "dispatcher symbols cannot be omitted" (DispatcherRejected D.SymbolRejected) (deriveArtifact (dispatcher {nativeSymbols = B.empty}) installation)
      refused "installer symbols cannot be omitted" (InstallerRejected I.SymbolRejected) (deriveArtifact dispatcher (installation {nativeSymbols = B.empty}))
      let linked = nativeBytes installation
          calls = [(offset,site) | offset <- [0,4..B.length linked-4],let site = 0x725a04+fromIntegral offset
            ,A.Branch _ True target <- [A.decode site (U.word32At linked offset)],target == 0x42634]
      (offset,site) <- case calls of [call] -> pure call; _ -> fail "emitted installer cache call is not unique"
      let changed = U.putWord32 (0xeb000000 .|. (fromInteger ((0x42630-toInteger site-8) `div` 4) .&. 0xffffff))
          wrongCache = installation {nativeBytes = B.take offset linked <> changed <> B.drop (offset+4) linked}
      refused "joined gate retains installer target confinement" (InstallerRejected (I.CodeRejected K.ExternalCallRejected)) (deriveArtifact dispatcher wrongCache)
      putStrLn "joined frame admission: actual and synthetic aligned compositions accepted; misalignment, omitted images/symbols and wrong retained target refused"
    _ -> fail "usage: startup-composition-tests DISPATCH_BIN DISPATCH_SYMBOLS INSTALL_BIN INSTALL_SYMBOLS"
