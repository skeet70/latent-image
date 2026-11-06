module Replacement.RtosScheduler.CutoverPins (Group (..), Digest (..), metadataPin, binPin, symbolPin, groupLabel) where

import Replacement.RtosOwnFirst.Regions (Domain (..))

data Group = S1 | S2 | S3a | S3b | S3c | OwnFirst deriving (Eq, Ord, Show, Enum, Bounded)
newtype Digest = Digest String deriving (Eq, Show)

groupLabel :: Group -> String
groupLabel group = case group of
  S1 -> "s1"
  S2 -> "s2"
  S3a -> "s3a"
  S3b -> "s3b"
  S3c -> "s3c"
  OwnFirst -> "own-first"

metadataPin :: Group -> Digest
metadataPin group = Digest $ case group of
  S1 -> "fddb383c917882768abd1305ba0acc48c9a48d41695f9db8e58acd3570727fe6"
  S2 -> "52eb757d93873e6eb3004a24284eef4f48d15f9cb36de787cca7041b9a00d0bc"
  S3a -> "212692f66bc1805585035077e0570765d8e4e7b93bbcc3b980a25acbd9293d2f"
  S3b -> "46293cd10c5a81363ea8833e09c1c869b504d600cf08e9640f8bcd7aeed00cb3"
  S3c -> "f26a15b3faadaafb9cc6a56fb375d7eb36f8199c9e11d4e11e7ac33e1e3c94ff"
  OwnFirst -> "324931ab0aa0763a0dcccc1da43fb94afc215c5ccb64e37f3aa39487d2033228"

binPin :: Group -> Domain -> Digest
binPin group domain = Digest $ case (group,domain) of
  (S1,DomainA) -> "fad6b0462e8fdf38b6aedcca2859d7b2fc1aee689d00f5e0656e9367d691b0ac"
  (S1,DomainB) -> "60f4e310f618324a7ae529f66c82a3653fed83265a22e68588c97ad9632f5fdd"
  (S2,DomainA) -> "eb303f7d08eea8ddbb4557751adfe63853894360723cb3485e85129ca5d35614"
  (S2,DomainB) -> "1ee6bcb513e58269d81d2138f8354eadf603e8a46439a8767e9f19c0a09f9ee5"
  (S3a,DomainA) -> "10452e08920a6b4a562668266f38b45e5182dcf016cc12b5e1498849dff962b4"
  (S3a,DomainB) -> "3fc256d552b27a4043e40bbf0f948dc73dc7ce267e5428bf0beb679ab982f51d"
  (S3b,DomainA) -> "4a8c4bd211619225712174cf66856c18517b2af68126a1b21482c38784cea97b"
  (S3b,DomainB) -> "9a1b499f6d972dbb8456e73264b43be0d9173d845ae33f9deb1c1edb38485e00"
  (S3c,DomainA) -> "2fce2d725ca29735216ac703338527d36c114c5e605f23ae3b9556ef923c8623"
  (S3c,DomainB) -> "130afea02e090638efea1fd9c19f1c5071b2b63695f67a418910103007952f4a"
  (OwnFirst,DomainA) -> "7bef1d63ecb5fe177d313114fd63cbfbced9dca77311f5461520d42a3a643da0"
  (OwnFirst,DomainB) -> "91802f2998b8247e42cc22591fb1ac5222caccc68dd57d4be00ecef69e979149"

symbolPin :: Group -> Domain -> Maybe Digest
symbolPin group domain = fmap Digest $ case (group,domain) of
  (S1,DomainA) -> Just "b1d316ea80ba5bc7298c608c1e5501764fce8640fd2f8d64fc76572ef38acea0"
  (S1,DomainB) -> Just "7ff40d737fbbcf22c37b8aea5f157ab1d95fd8a1840499e60d8e9e9d26bae2f5"
  (S2,DomainA) -> Just "8991919d06b1f9776457ef76e863cd1ab72148b16a5ce8be8099c3add46de7fe"
  (S2,DomainB) -> Just "f2a5abf505d56d23bd918590d951b8301b9aa330d9fc28de92cf9fcac991d4d5"
  (S3a,DomainA) -> Just "151d419a020d026200b8649a49929148e41228f40e2534550a1ef5377fe80461"
  (S3a,DomainB) -> Just "09088c69ceb234e2c9d2f6f1781945df4c2c47fbc90613731844ef078b04bc3a"
  (S3b,DomainA) -> Just "b36055c6445b22bc98fd5574e6ac9024bc2e5b4efa7c4145b11c022c3640e1ab"
  (S3b,DomainB) -> Just "f0cf523bfcd94e0d373119dac201ca28c4df2961007ca1dbffc6e08b692782a3"
  (S3c,DomainA) -> Just "dc0cfe338f85eef41475f2cc7f2b0387679cffe1f2a88c6a8d79e8ea737dbcbf"
  (S3c,DomainB) -> Just "cde9782013d2ba40f0b1a3c41192e717b5fbfc24d90f06d7cea4e13f80c21721"
  (OwnFirst,_) -> Nothing
