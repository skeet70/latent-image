{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Either (isLeft)
import qualified Replacement.RtosPlatform.Aggregate as Aggregate
import qualified Replacement.RtosPlatform.Union as Union
import qualified Replacement.RtosOwnFirst.Regions as R
import Replacement.RtosCyclic.Suite (Image (..))
import Replacement.RtosCyclic.JoinSuite
import System.Exit (die)

main :: IO ()
main = do
  let stock = B.replicate 32 0x55
      code = B.replicate 4 0x11 <> B.replicate 4 0 <> B.replicate 4 0x22
      symbols = [Aggregate.AggregateSymbol "entry" 0x40008 4, Aggregate.AggregateSymbol "body" 0x40010 4]
      image = Aggregate.AggregateImage 0x40008 code
      placements =
        [ Aggregate.SectionPlacement 0x4000c 4 ".rodata.table"
        , Aggregate.SectionPlacement 0x40010 4 ".text.body"
        ]
      domain = Union.DomainMap R.DomainA (R.GuestAddress 0x40000) 32
        [Union.Entry Union.Cyclic "configure" (R.GuestAddress 0x40008) "entry" ".text.entry" "entry"]
        [Union.Carrier Union.Cyclic (R.Extent (R.GuestAddress 0x40008) (R.GuestAddress 0x40018))]
        [] []
      union = Union.Union 0xe7f000f0 domain domain
      staged = B.take 8 stock <> code <> B.pack [0xf0, 0x00, 0xf0, 0xe7] <> B.drop 24 stock
      admit loader = admitPackagedKernel union stock loader image symbols placements
      changed offset byte bytes = B.take offset bytes <> B.singleton byte <> B.drop (offset + 1) bytes
      require condition label = unless condition (die label)
  case admit staged of
    Right (PackagedKernel loader mappings) -> do
      require (loader == staged) "admission changed packaged loader bytes"
      require (any (\(Image address bytes, executable, functions) -> address == 0x4000c && bytes == B.replicate 4 0 && not executable && null functions) mappings) "readonly bytes admitted as executable"
    Left refusal -> die ("exact packaged code and readonly data refused: " ++ refusal)
    Right StandaloneKernel -> die "packaged admission returned standalone mode"
  require (isLeft (admit (changed 12 0x55 staged))) "allocated readonly zero was not checked"
  require (isLeft (admit (changed 8 0x12 staged))) "changed function bytes accepted"
  require (isLeft (admit (changed 0 0 staged))) "changed retained stock byte accepted"
  require (isLeft (admit (changed 20 0 staged))) "changed carrier slack accepted"
  require (isLeft (admit (B.take 31 staged))) "truncated loader accepted"
  require (isLeft (admitPackagedKernel union stock staged image symbols [Aggregate.SectionPlacement 0x4001c 12 ".outside"])) "out-of-image placement accepted"
  putStrLn "join-admission-tests: passed"
