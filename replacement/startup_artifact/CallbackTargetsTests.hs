-- | Declared register-branch target sets on synthetic code: a declared set whose writers the scan
-- finds bounds the walk through its site; an undeclared writer, a changed image or an index value
-- outside the declaration keeps the site open.
module Main where

import Control.Monad (unless)
import Data.Bits (shiftL, shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Builder as BB
import qualified Data.ByteString.Lazy as BL
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.StartupArtifact.CallbackTargets
import Replacement.StartupArtifact.StackBounds (Segment (..), Watched (..))
import StockStack
import System.Exit (exitFailure)

base, dispatcher, site, target, setter, store, caller, extra, extraStore, slot, chooser, index, table :: Word32
base = 0x100000
dispatcher = 0x100000
site = 0x100010
target = 0x100100
setter = 0x100200
store = 0x100208
caller = 0x100300
extra = 0x100400
extraStore = 0x100410
chooser = 0x100500
slot = 0x101000
index = 0x101010
table = 0x101020

movw, movt :: Word32 -> Word32 -> Word32
movw rd value = 0xe3000000 .|. ((value .&. 0xf000) `shiftL` 4) .|. (rd `shiftL` 12) .|. (value .&. 0xfff)
movt rd value = 0xe3400000 .|. ((value `shiftR` 28) `shiftL` 16) .|. (rd `shiftL` 12) .|. ((value `shiftR` 16) .&. 0xfff)

-- | @movw/movt rd, value@.
constant :: Word32 -> Word32 -> [Word32]
constant rd value = [movw rd (value .&. 0xffff), movt rd value]

bl :: Word32 -> Word32 -> Word32
bl from to = 0xeb000000 .|. (((to - from - 8) `shiftR` 2) .&. 0xffffff)

-- | The dispatcher calls the word at `slot`; `setter` stores its argument there and `caller` passes
-- `target`; `extra`, when present, stores 0x100600 there too; `chooser` stores 1 or 2 to `index`.
image :: Bool -> B.ByteString
image withExtra = BL.toStrict (BB.toLazyByteString (foldMap BB.word32LE (map wordAt [base, base + 4 .. slot + 0x40 - 4])))
  where
    code = M.fromList (concat
      [ placed dispatcher ([0xe92d4010] ++ constant 3 slot ++ [0xe5933000, 0xe12fff33, 0xe8bd8010])
      , placed target [0xe92d40f0, 0xe8bd80f0]
      , placed (setter - 4) ([0xe12fff1e] ++ constant 1 slot ++ [0xe5810000, 0xe12fff1e])
      , placed caller ([0xe92d4010] ++ constant 0 target ++ [bl (caller + 12) setter, 0xe8bd8010])
      , [(extra + 4 * n, word) | withExtra, (n, word) <- zip [0 ..] (constant 1 slot ++ constant 2 0x100600 ++ [0xe5812000, 0xe12fff1e])]
      , placed chooser ([0xe12fff1e] ++ constant 1 index ++ [0xe3a02001, 0xe5812000, 0xe3a02002, 0xe5812000, 0xe12fff1e])
      , [(slot, target), (table, target), (table + 4, target)] ])
    placed at' words' = zip [at', at' + 4 ..] words'
    wordAt address = M.findWithDefault 0 address code

digest :: B.ByteString -> String
digest = SHA.sha256Hex . SHA.sha256

checked :: B.ByteString -> B.ByteString -> [TargetDeclaration] -> CheckedSites
checked canonical composed = declaredSites [Segment base canonical] [Segment base composed] [] []

slotDeclaration :: B.ByteString -> [Word32] -> TargetDeclaration
slotDeclaration canonical writers = TargetDeclaration site [ImageDigest base (digest canonical)] (Slots [slot] []) [CodeRange at' (at' + 4) | at' <- writers]

depthThrough :: B.ByteString -> CheckedSites -> Either StackRefusal StackDepth
depthThrough bytes sites = reportDepth <$> stockDepth (stockCode [(base, bytes)] []) (Supplied M.empty) indirect noSwitches dispatcher
  where
    indirect = IndirectTargets
      { rootTable = 0, rootSlots = M.empty, callbackSites = closedSites sites, contextLocks = [], loadedWords = const Nothing
      , supervisorEntries = [], excludeFatalReports = False }

main :: IO ()
main = do
  let plain = image False
      written = image True
      cases =
        [ ( "a declared set whose one writer is declared closes the site and bounds the walk through it"
          , let sites = checked plain plain [slotDeclaration plain [store]]
             in M.lookup site (closedSites sites) == Just (CallbackSite [target] DeclaredTargetsClosed)
                  && depthThrough plain sites == Right (StackDepth 28) )
        , ( "an undeclared writer keeps the site open and the walk refuses there"
          , let sites = checked written written [slotDeclaration written [store]]
             in openSites sites == [(site, UndeclaredWriter slot extraStore)] && depthThrough written sites == Left (UnresolvedBranch site) )
        , ( "declaring that writer closes the site with both stored values"
          , M.lookup site (closedSites (checked written written [slotDeclaration written [store, extraStore]]))
              == Just (CallbackSite [target, 0x100600] DeclaredTargetsClosed) )
        , ( "a digest that does not match the canonical image keeps the site open"
          , openSites (checked written written [slotDeclaration plain [store, extraStore]]) == [(site, DigestMismatch base)] )
        , ( "an index value outside the declared ones keeps the site open"
          , let declaration allowed = TargetDeclaration site [ImageDigest base (digest plain)]
                  (Slots [table, table + 4] [IndexWord (Watched index 4) allowed]) [CodeRange chooser (chooser + 0x20)]
             in openSites (checked plain plain [declaration [0, 1]]) == [(site, IndexOutside index 2)]
                  && M.lookup site (closedSites (checked plain plain [declaration [0, 1, 2]])) == Just (CallbackSite [target] DeclaredTargetsClosed) )
        ]
  failures <- fmap concat (mapM (\(name, ok) -> do
    putStrLn ((if ok then "ok: " else "FAILED: ") ++ name)
    pure [name | not ok]) cases)
  unless (null failures) exitFailure
