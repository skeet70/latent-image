{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM_)
import qualified Data.ByteString as B
import Data.Word (Word8)
import qualified Emulation.NativeCase as C
import qualified Emulation.NativeObservation as N
import qualified Emulation.OwnedCase as O
import FujiAF.Host (ensure, withTemporary)
import System.FilePath ((</>))

example :: C.CaseId -> Word8 -> O.CaseSpec
example (C.CaseId ident) seed = O.CaseSpec
  { O.caseId = ident
  , O.caseProvenance = "synthetic initializer admission"
  , O.caseExclusion = "No firmware execution."
  , O.caseEntry = N.GuestAddress 0x1000
  , O.caseExit = N.GuestAddress 0x2000
  , O.caseRegions =
      [ O.mkRegion (N.RegionId "code") (N.GuestAddress 0x1000) 8 O.ReadExecute (O.assemble [0xe3a0002a, 0xe12fff1e])
      , O.mkRegion (N.RegionId "exit") (N.GuestAddress 0x2000) 4 O.ReadExecute (O.word32le 0xe7f000f0)
      , O.mkRegion (N.RegionId "stack") (N.GuestAddress 0x8000) 16 O.ReadWrite (B.replicate 16 0)
      ] ++
      [ O.mkRegion (N.RegionId ("data-" ++ show index))
          (N.GuestAddress (0x100000 + fromIntegral index * 0x20000)) (100 * 1024)
          (if even index then O.ReadOnly else O.ReadWrite)
          (B.replicate (100 * 1024) (seed + fromIntegral index))
      | index <- [1 :: Int .. 24]
      ]
  , O.caseFunctions = [(N.GuestAddress 0x1000, 8)]
  , O.caseInitial = [(register, 0) | register <- [N.R0 .. N.R12]] ++ [(N.Sp, 0x8008), (N.Lr, 0x2000), (N.Cpsr, 0x10)]
  , O.caseExpectedStatus = N.Returned
  , O.caseExpectedRegisters = [(N.R0, 42)]
  , O.caseExpectedMemory = []
  , O.caseCp14Identity = Nothing
  , O.caseKernel = Nothing
  }

checkCase :: FilePath -> O.CaseSpec -> IO ()
checkCase path spec = do
  (_, decoded, _, _) <- C.loadCase path
  ensure (length decoded == length (O.caseRegions spec)) "decoded region count differs"
  forM_ (O.caseRegions spec) $ \(O.CaseRegion original permission) -> do
    let matches = filter ((== N.regionIdOf original) . N.regionIdOf) decoded
    case matches of
      [actual] -> do
        ensure (C.baseAddress actual == C.baseAddress original && C.regionSize actual == C.regionSize original) "decoded region extent differs"
        ensure (C.regionBytes actual == C.regionBytes original) "decoded initializer bytes differ"
        let (writable, executable) = case permission of
              O.ReadOnly -> (False, False)
              O.ReadWrite -> (True, False)
              O.Device -> (True, False)
              O.ReadExecute -> (False, True)
        forM_ [('r', True), ('w', writable), ('x', executable)] $ \(access, expected) ->
          ensure (C.regionAt decoded (C.baseAddress actual) (C.regionSize actual) access == expected) "decoded region permissions differ"
      _ -> ensure False "decoded region identity differs"

main :: IO ()
main = withTemporary "owned-case-initialization" $ \root -> do
  let first = example (C.CaseId "initializer-a") 0
      second = example (C.CaseId "initializer-b") 32
  standalone <- O.writeCase (root </> "standalone") first
  checkCase standalone first
  batch <- O.writeBatch (root </> "batch") [first, second]
  (_, entries) <- C.readBatch batch
  ensure (length entries == 2) "batch case count differs"
  forM_ entries $ \entry ->
    case filter ((== C.entryId entry) . C.CaseId . O.caseId) [first, second] of
      [spec] -> checkCase (C.entryPath entry) spec
      _ -> ensure False "batch case identity differs"
  putStrLn "owned-case initialization admission passed"
