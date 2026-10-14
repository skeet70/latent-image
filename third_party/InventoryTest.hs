{-# LANGUAGE OverloadedStrings #-}
module Main where
import qualified FujiAF.Host as H
import Control.Exception (SomeException, try)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import PublicInventory (inventoryUsing, verifySelected)

main :: IO ()
main = do
  let revision = replicate 40 'a'; tree = replicate 40 'b'; first = replicate 40 '1'; second = replicate 40 '2'
      listing = "100644 blob " <> C.pack first <> "\talpha.txt\0" <> "100755 blob " <> C.pack second <> "\t" <> B.pack [195,169] <> "\0"
      fixture _ arguments = case arguments of
        ["rev-parse",ref] | ref==revision++"^{commit}" -> pure (C.pack revision<>"\n")
        ["rev-parse",ref] | ref==revision++"^{tree}" -> pure (C.pack tree<>"\n")
        ["ls-tree","-rz","--full-tree",ref] | ref==revision -> pure listing
        ["cat-file","blob",objectID] | objectID==first -> pure "abc"
        ["cat-file","blob",objectID] | objectID==second -> pure B.empty
        _ -> H.failH "unexpected Git call in independent fixture"
      firstRow = H.obj [("path",H.s "alpha.txt"),("mode",H.s "100644"),("git_blob",H.s first),("size_bytes",H.n 3),("sha256",H.s "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")]
      secondRow = H.obj [("path",H.s "é"),("mode",H.s "100755"),("git_blob",H.s second),("size_bytes",H.n 0),("sha256",H.s "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")]
  manifest <- inventoryUsing fixture "synthetic:inert" revision "synthetic:public-metadata"
  H.ensure (H.get manifest "files" == H.arr [firstRow,secondRow]) "inventory bytes/hash/order/mode/UTF8 differ from hand-declared fixture"
  H.ensure (H.get manifest "git_tree" == H.s tree && H.get manifest "revision" == H.s revision) "exact revision/tree metadata differs"
  again <- inventoryUsing fixture "synthetic:inert" revision "synthetic:public-metadata"
  H.ensure (manifest==again) "inventory nondeterminism"
  let reject action = do
        result <- try (action >> pure ())
        case result :: Either SomeException () of Left _ -> pure (); Right () -> H.failH "invalid inventory input accepted"
      submodule checkout arguments = case arguments of
        ["ls-tree","-rz","--full-tree",_] -> pure ("160000 commit "<>C.pack first<>"\tsubmodule\0")
        _ -> fixture checkout arguments
      excessive checkout arguments = case arguments of
        ["ls-tree","-rz","--full-tree",_] -> pure (B.replicate (4*1024*1024+1) 0)
        _ -> fixture checkout arguments
      malformedUTF8 checkout arguments = case arguments of
        ["ls-tree","-rz","--full-tree",_] -> pure ("100644 blob "<>C.pack first<>"\t"<>B.pack [255,0])
        _ -> fixture checkout arguments
  reject (inventoryUsing fixture "synthetic" "main" "synthetic")
  reject (inventoryUsing submodule "synthetic" revision "synthetic")
  reject (inventoryUsing excessive "synthetic" revision "synthetic")
  reject (inventoryUsing malformedUTF8 "synthetic" revision "synthetic")
  let selected = H.obj [("schema_version",H.n 1),("coverage",H.s "synthetic"),("privacy_class",H.s "synthetic"),("hash_method",H.s "synthetic"),("sources",H.obj [])]
  reject (verifySelected [] selected)
  reject (verifySelected [] (H.set "sources" (H.arr []) selected))
  putStrLn "Inventory independent known-vector/UTF8/mode/order/determinism/non-blob/revision/cap/malformed-array fixtures passed"
